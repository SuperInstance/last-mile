//! The prefetch queue: the worker thread that renders the lines that are
//! coming, so the game never waits for a voice.
//!
//! A render costs about 0.7 s on an idle Thor and about 2.3 s on a busy
//! one — twenty ticks of the round's clock every time — so no line is ever
//! rendered where it is spoken. The host **submits** a line the moment it
//! knows it (the decision's arrival, not its due tick), the worker
//! renders it into the bank ahead of time, and when the line is due the
//! host **looks it up**, which is a file read and never a wait:
//!
//! - [`Lookup::Hit`] — the clip is in the bank, ready to play;
//! - [`Lookup::Pending`] — the worker has it, but it is not done yet;
//! - [`Lookup::Miss`] — nobody has submitted this line at all.
//!
//! ## The fallback contract
//!
//! A line that is not ready when it is due does not delay the round by
//! one tick: [`PrefetchQueue::speak`] hands back the **tone** — the same
//! stub wave the host plays today — and counts the miss. The tone is the
//! honest sound of a voice that was not ready, not an error: the round's
//! truth is the score, and the audio is for the human (the sim never
//! sees either).
//!
//! The queue is one worker on purpose: skald's daemon serves one
//! synthesis at a time well and queues the rest itself, so a second
//! worker would only move the queue from the service to the client.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::bank::{BankMeta, VoiceBank};
use crate::skald::{SpeechLine, VoiceClient};
use crate::{StubTts, Tts, TtsClip, TtsRequest, VoiceError};

/// One job for the worker: a line, its key and the engine it is keyed by.
struct Job {
    key: u64,
    line: SpeechLine,
    engine: String,
}

/// What a lookup found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lookup {
    /// The clip is in the bank, with its sidecar.
    Hit(BankMeta),
    /// The worker has the line, but it is not rendered yet.
    Pending,
    /// Nobody has submitted this line.
    Miss,
}

/// What a due line played.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Due {
    /// The rendered clip: the voice, as it was asked for.
    Clip(TtsClip),
    /// The tone: the line was not ready, the miss is counted, and the
    /// round went on.
    Tone(TtsClip),
}

impl Due {
    /// The clip either way: the caller plays it and does not care which
    /// ear it reached.
    pub fn clip(&self) -> &TtsClip {
        match self {
            Due::Clip(clip) => clip,
            Due::Tone(clip) => clip,
        }
    }

    /// Whether this was the rendered voice or the fallback tone.
    pub fn is_tone(&self) -> bool {
        matches!(self, Due::Tone(_))
    }
}

/// The queue's own counters, for the HUD and the report.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QueueStats {
    /// Lines submitted to the worker.
    pub submitted: u64,
    /// Submits the bank already answered (no render needed).
    pub bank_hits: u64,
    /// Submits the worker already had (one render serves them all).
    pub deduped: u64,
    /// Lines the worker rendered into the bank.
    pub rendered: u64,
    /// Renders that failed, by the voice service's own word.
    pub failed: u64,
    /// The last failure's reason, for the HUD's status line.
    pub last_error: Option<String>,
    /// Lines that were due before they were ready, and played as the tone.
    pub fallback_tones: u64,
    /// The render latencies the worker measured, in milliseconds, in the
    /// order they happened.
    pub latencies_ms: Vec<u64>,
    /// The render latency by key, for the lines this queue rendered
    /// itself. A key that is not here was a bank hit (ready at once) or a
    /// failure (ready never).
    pub latency_by_key: BTreeMap<u64, u64>,
}

/// What the worker and the host share: the in-flight set and the counters.
struct Shared {
    /// The keys queued or rendering right now.
    inflight: BTreeSet<u64>,
    /// The counters.
    stats: QueueStats,
}

/// The prefetch queue: a client, a bank and a worker thread.
pub struct PrefetchQueue {
    /// The daemon the worker renders through (the client itself belongs
    /// to the worker's thread).
    url: String,
    /// The bank the queue keys by and looks up in (a path, shared with
    /// the worker by clone).
    bank: VoiceBank,
    deadline_ms: u64,
    shared: Arc<Mutex<Shared>>,
    tx: Option<Sender<Job>>,
    handle: Option<JoinHandle<()>>,
}

impl PrefetchQueue {
    /// A queue of one worker rendering `line`s through `client` into
    /// `bank`, each render bounded by `deadline_ms`.
    pub fn new(client: VoiceClient, bank: VoiceBank, deadline_ms: u64) -> PrefetchQueue {
        let url = client.url();
        let (tx, rx) = std::sync::mpsc::channel::<Job>();
        let shared = Arc::new(Mutex::new(Shared {
            inflight: BTreeSet::new(),
            stats: QueueStats::default(),
        }));
        let worker_shared = Arc::clone(&shared);
        let worker_bank = bank.clone();
        let handle = std::thread::Builder::new()
            .name("lm-voice-prefetch".to_owned())
            .spawn(move || worker(rx, client, worker_bank, deadline_ms, worker_shared))
            .expect("the prefetch worker starts");
        PrefetchQueue {
            url,
            bank,
            deadline_ms,
            shared,
            tx: Some(tx),
            handle: Some(handle),
        }
    }

    /// The daemon the queue renders through.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The bank the queue renders into.
    pub fn bank(&self) -> &VoiceBank {
        &self.bank
    }

    /// How long one render may take, in milliseconds.
    pub fn deadline_ms(&self) -> u64 {
        self.deadline_ms
    }

    /// Submit a line for rendering: the moment the host knows it, which
    /// is the decision's arrival — never its due tick.
    ///
    /// A line the bank already has is not submitted (the render would be
    /// a waste), and a line the worker already has is not duplicated (one
    /// render serves every submit). The key is returned either way, so
    /// the caller can look the line up when it is due.
    pub fn submit(&self, line: &SpeechLine, engine: &str) -> u64 {
        let key = self.bank.key(line, engine);
        if self.bank.lookup(key).is_some() {
            if let Ok(mut shared) = self.shared.lock() {
                shared.stats.bank_hits += 1;
            }
            return key;
        }
        let send = {
            let Ok(mut shared) = self.shared.lock() else {
                return key;
            };
            if shared.inflight.contains(&key) {
                shared.stats.deduped += 1;
                false
            } else {
                shared.inflight.insert(key);
                shared.stats.submitted += 1;
                true
            }
        };
        if send {
            let sent = self.tx.as_ref().map(|tx| {
                tx.send(Job {
                    key,
                    line: line.clone(),
                    engine: engine.to_owned(),
                })
            });
            if matches!(sent, None | Some(Err(_))) {
                // The worker is gone: the line is not in flight, and the
                // submit is answered by the bank when one returns.
                if let Ok(mut shared) = self.shared.lock() {
                    shared.inflight.remove(&key);
                }
            }
        }
        key
    }

    /// Look a line up: hit, pending or miss. This **never blocks** — it
    /// is a read of the bank and a glance at the in-flight set.
    pub fn lookup(&self, key: u64) -> Lookup {
        if let Some(meta) = self.bank.lookup(key) {
            return Lookup::Hit(meta);
        }
        let pending = self
            .shared
            .lock()
            .map(|shared| shared.inflight.contains(&key))
            .unwrap_or(false);
        if pending {
            Lookup::Pending
        } else {
            Lookup::Miss
        }
    }

    /// The line is due **now**: the clip if it is ready, the tone if it
    /// is not, and the miss counted.
    ///
    /// This is the fallback contract: the round never waits for a voice,
    /// and a line that was not ready is heard as the tone — the same
    /// wave the host plays when there is no voice service at all.
    pub fn speak(&self, key: u64, tone: &TtsRequest) -> Due {
        // The clip is served only when the entry is whole and decodes;
        // anything less is the tone, and the miss is counted.
        if self.bank.lookup(key).is_some() {
            if let Some(clip) = self.bank.clip(key) {
                return Due::Clip(clip);
            }
        }
        if let Ok(mut shared) = self.shared.lock() {
            shared.stats.fallback_tones += 1;
        }
        let clip = StubTts::default().synthesize(tone).unwrap_or(TtsClip {
            pcm: vec![0; 16],
            sample_rate: 16_000,
            duration_ms: 300,
        });
        Due::Tone(clip)
    }

    /// The queue's counters.
    pub fn stats(&self) -> QueueStats {
        self.shared
            .lock()
            .map(|shared| shared.stats.clone())
            .unwrap_or_default()
    }

    /// Wait until the worker has nothing in flight, or `upto` passes.
    ///
    /// This is a **report's** question (and a test's), never the game's:
    /// the game looks lines up and does not care whether the worker is
    /// busy. `true` when the queue drained inside the time.
    pub fn wait(&self, upto: Duration) -> bool {
        let deadline = Instant::now() + upto;
        loop {
            let quiet = self
                .shared
                .lock()
                .map(|shared| shared.inflight.is_empty())
                .unwrap_or(true);
            if quiet {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Stop the worker and wait for it to leave. The jobs in the channel
    /// are dropped: a submit whose render never started is answered by
    /// the bank when a worker next runs.
    pub fn shutdown(&mut self) {
        // Dropping the sender ends the worker's recv loop.
        self.tx.take();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for PrefetchQueue {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// The worker's loop: one job at a time, render, store, count.
fn worker(
    rx: Receiver<Job>,
    client: VoiceClient,
    bank: VoiceBank,
    deadline_ms: u64,
    shared: Arc<Mutex<Shared>>,
) {
    while let Ok(job) = rx.recv() {
        let deadline = Instant::now() + Duration::from_millis(deadline_ms);
        let outcome = client.render(&job.line, deadline).and_then(|rendered| {
            bank.store(job.key, &job.line, &job.engine, &rendered)
                .map_err(|err| VoiceError::Malformed(err.to_string()))?;
            Ok(rendered)
        });
        if let Ok(mut shared) = shared.lock() {
            match outcome {
                Ok(rendered) => {
                    shared.stats.rendered += 1;
                    shared.stats.latencies_ms.push(rendered.latency_ms);
                    shared
                        .stats
                        .latency_by_key
                        .insert(job.key, rendered.latency_ms);
                }
                Err(err) => {
                    shared.stats.failed += 1;
                    shared.stats.last_error = Some(err.to_string());
                }
            }
            shared.inflight.remove(&job.key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mockhttp::{MockSkald, Plan};
    use crate::prosody::wav_bytes;
    use lm_core::utterance::Affect;

    /// The WAV the mock answers with: one second of a quiet wave.
    fn mock_wav() -> Vec<u8> {
        wav_bytes(&vec![100i16; 22_050], 22_050)
    }

    /// A queue over a mock daemon and a fresh bank under a temp dir.
    fn queue(tag: &str, plan: Plan) -> (MockSkald, PrefetchQueue) {
        let mock = MockSkald::start(plan);
        let dir =
            std::env::temp_dir().join(format!("lm-voice-prefetch-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bank = VoiceBank::open(&dir).expect("the bank opens");
        let client = VoiceClient::new(&mock.base(), mock.port(), 22_050);
        let q = PrefetchQueue::new(client, bank, 5_000);
        (mock, q)
    }

    /// A tone request for the fallback contract.
    fn tone(text: &str) -> TtsRequest {
        TtsRequest::new(text, "default", Affect::Urgent)
    }

    #[test]
    fn a_submitted_line_renders_into_the_bank() {
        let (mock, mut q) = queue("render", Plan::Wav(mock_wav()));
        let line = SpeechLine::new("hold the north beacon", "mia", "Neutral");
        let key = q.submit(&line, "engine-a");

        assert!(q.wait(Duration::from_secs(5)), "the worker drains");
        let stats = q.stats();
        assert_eq!(stats.submitted, 1);
        assert_eq!(stats.rendered, 1);
        assert_eq!(stats.failed, 0);
        assert_eq!(stats.latencies_ms.len(), 1);
        assert!(matches!(q.lookup(key), Lookup::Hit(_)));

        // And a second submit of the same line is a bank hit, not a
        // render: one line, one synthesis.
        q.submit(&line, "engine-a");
        let stats = q.stats();
        assert_eq!(stats.bank_hits, 1);
        assert_eq!(stats.rendered, 1);
        mock.join();
        q.shutdown();
        let _ = std::fs::remove_dir_all(q.bank().root());
    }

    #[test]
    fn an_unknown_key_is_a_miss_and_never_blocks() {
        let (mock, mut q) = queue("miss", Plan::Wav(mock_wav()));
        let started = Instant::now();
        assert!(matches!(q.lookup(0xdead_beef), Lookup::Miss));
        assert!(
            started.elapsed() < Duration::from_millis(100),
            "a lookup is a glance, not a wait"
        );
        mock.join();
        q.shutdown();
        let _ = std::fs::remove_dir_all(q.bank().root());
    }

    #[test]
    fn a_line_under_the_worker_s_thumb_is_pending() {
        // The mock holds its answer; the line is submitted and sits in
        // flight — a lookup says pending, and speaks the tone when due.
        let (mock, mut q) = queue("pending", Plan::Wav(mock_wav()));
        mock.hold();
        let line = SpeechLine::new("fall back", "aria", "Sad");
        let key = q.submit(&line, "engine-a");

        // While the render is held: pending, and never a wait.
        let lookup = q.lookup(key);
        assert!(matches!(lookup, Lookup::Pending), "{lookup:?}");

        // And due while pending: the tone, and the miss counted.
        let due = q.speak(key, &tone("fall back"));
        assert!(due.is_tone(), "a pending line is not a clip");
        assert!(due.clip().duration_ms > 0, "the tone is something to hear");
        assert_eq!(q.stats().fallback_tones, 1);

        // Released, the line lands, and the next due is the real clip.
        mock.release();
        assert!(q.wait(Duration::from_secs(5)), "the held render lands");
        let due = q.speak(key, &tone("fall back"));
        assert!(!due.is_tone(), "the rendered line is a clip now");
        assert_eq!(q.stats().fallback_tones, 1, "no new miss");
        mock.join();
        q.shutdown();
        let _ = std::fs::remove_dir_all(q.bank().root());
    }

    #[test]
    fn a_failed_render_is_counted_and_the_tone_answers() {
        let (mock, mut q) = queue("fail", Plan::Status(500, "engine sad".to_owned()));
        let line = SpeechLine::new("contact east", "jason", "Angry");
        let key = q.submit(&line, "engine-a");
        assert!(q.wait(Duration::from_secs(5)), "the failure lands");
        let stats = q.stats();
        assert_eq!(stats.failed, 1);
        assert_eq!(stats.rendered, 0);
        assert!(stats.last_error.is_some());
        // The line is neither pending nor hit: it is simply not there.
        assert!(matches!(q.lookup(key), Lookup::Miss));
        let due = q.speak(key, &tone("contact east"));
        assert!(due.is_tone());
        assert_eq!(q.stats().fallback_tones, 1);
        mock.join();
        q.shutdown();
        let _ = std::fs::remove_dir_all(q.bank().root());
    }
}
