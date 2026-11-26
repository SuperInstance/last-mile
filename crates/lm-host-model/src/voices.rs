//! The agents' voices in the host: skald rendered ahead of time, played
//! from the bank, never waited for (D73, wired in).
//!
//! Until this module the host played every agent line as a **tone** — the
//! stub's ramp, whatever the line was. Now the round's lines are spoken in
//! the skald voices of `voice.toml`, and the host still never waits:
//!
//! * **at the plan** — the moment a mind's line arrives at the host (the
//!   decision's arrival, long before its airtime), [`Voices::prefetch`]
//!   hands the ask to the prefetch queue's worker, which renders it into
//!   the bank in the background;
//! * **at the airtime** — [`Voices::speak`] looks the line up: a **hit**
//!   plays the WAV through the host's audio, and a line that is still
//!   **pending** or was never submitted (a **miss**) plays the tone, the
//!   same wave as before, and the miss is counted.
//!
//! The fallback contract is the whole design: a render costs about 0.7 s
//! on an idle Thor and more on a busy one, which is twenty ticks of the
//! round's clock at least, so no line is ever rendered where it is
//! spoken. The tone is the honest sound of a voice that was not ready —
//! not an error.
//!
//! ## Nothing here decides anything
//!
//! The voices are for the human's ear alone. [`crate::host_loop`] submits
//! the prefetch between the decision's arrival and the sequencer, and
//! asks for the clip between the tick and the player; the sim never sees
//! either, the round's log never mentions them, and the golden hashes
//! move not at all. A bank that is empty, a daemon that is down or a
//! render that fails changes what the person hears and nothing else —
//! every line falls back to the tone and the round goes on.
//!
//! ## The bank
//!
//! The bank is a **per-user cache**: it lives under the machine's own
//! cache directory ([`user_bank_root`]), not in the round's runs, and it
//! carries a size cap that evicts whole entries oldest-first. Both are
//! parameters — `voice.toml`'s `[bank]` section says them, and the
//! command line's `--voice-bank` and `--voice-bank-cap-mb` override the
//! file. A warm bank is what makes the first round's lines hits, which is
//! also why the host **pre-warms** it ([`prewarm`]): a background thread
//! plays the round's seed once with the stub minds of the shipped harness
//! — the honest enumerator of the phrasebook, the same one `lm voice`
//! reports from — and asks the bank for every line it speaks, before the
//! real round's own lines arrive.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use lm_core::header::Header;
use lm_core::utterance::Utterance;
use lm_voice::bank::VoiceBank;
use lm_voice::prefetch::{Due, Lookup, PrefetchQueue, QueueStats};
use lm_voice::skald::{SpeechLine, VoiceClient};
use lm_voice::voicemap::VoiceMap;
use lm_voice::{TtsClip, TtsRequest};

/// How the clip of a due line was spoken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpokenOrigin {
    /// The bank held the clip: the rendered voice, as it was asked for.
    Hit,
    /// The worker had the line but was not done: the tone, and the miss
    /// counted as a pending one.
    PendingTone,
    /// Nobody had submitted the line (or its render had failed): the tone.
    MissTone,
}

impl SpokenOrigin {
    /// Whether the line was heard as the rendered voice.
    pub fn is_hit(self) -> bool {
        self == SpokenOrigin::Hit
    }

    /// The word the log and the HUD spell the origin by.
    pub fn as_str(self) -> &'static str {
        match self {
            SpokenOrigin::Hit => "hit",
            SpokenOrigin::PendingTone => "pending-tone",
            SpokenOrigin::MissTone => "miss-tone",
        }
    }
}

/// How one line was spoken: the ask, the origin and the render's own
/// measurement — everything the played-clip log says, and nothing the
/// round cares about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spoken {
    /// The line, as it was spoken.
    pub text: String,
    /// The speaker the voice map chose for the agent.
    pub voice: String,
    /// The emotion style the tone and the intent chose.
    pub style: String,
    /// How the clip was spoken: the bank's clip, or the tone.
    pub origin: SpokenOrigin,
    /// How long the render of this clip took, in milliseconds, when the
    /// clip is a bank hit — the latency of the render that made it, as the
    /// sidecar recorded it (a warm clip keeps the latency of the cold
    /// round that rendered it).
    pub latency_ms: Option<u64>,
    /// The bank key of the ask.
    pub key: u64,
}

/// The voice path's counters, for the HUD and the exit line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VoiceCounters {
    /// Lines due that played the rendered clip.
    pub hits: u64,
    /// Lines due that played the tone (pending or missed).
    pub tones: u64,
    /// Of the tones, the lines the worker still had in flight.
    pub pending: u64,
}

/// What the host says about the voices it attaches: the map, the bank's
/// place and cap, and the machine's own word about where the daemon is.
#[derive(Clone, Debug)]
pub struct VoicesSettings {
    /// The voice map (`voice.toml`): the speakers, the styles, the daemon
    /// and its engine.
    pub map: VoiceMap,
    /// Where the bank lives, already resolved (the command line's word,
    /// else the map's, else the per-user default).
    pub bank_root: PathBuf,
    /// The bank's cap, in bytes; `None` is no cap at all.
    pub cap_bytes: Option<u64>,
    /// The daemon's host, overriding the map: the environment's word for
    /// "the daemon is not on this machine" (the map names it as the Thor
    /// spells it, `localhost`, because the file is doctrine pinned per
    /// round and the address is a fact of the machine).
    pub base: Option<String>,
    /// The daemon's port, overriding the map.
    pub port: Option<u16>,
}

/// The agents' voices: the map, the bank and the prefetch queue, with the
/// counters of what the person has heard.
///
/// Shared as an [`Arc`]: the host loop asks it from the round's thread,
/// and the pre-warm thread asks it from its own — the queue's worker is
/// the only writer, so every method takes `&self` and none of them waits.
pub struct Voices {
    /// The voice map the lines are rendered through.
    map: VoiceMap,
    /// The queue: the worker thread, the bank and the fallback contract.
    queue: PrefetchQueue,
    /// The counters, shared with nothing but this struct's own methods.
    hits: AtomicU64,
    tones: AtomicU64,
    pending: AtomicU64,
}

impl Voices {
    /// Attach the voices: open the bank (capped, when a cap is given) and
    /// start the queue's worker against the daemon the settings name.
    pub fn open(settings: VoicesSettings) -> Result<Arc<Voices>, String> {
        let mut map = settings.map;
        // The machine's own word about the daemon wins over the map's.
        if let Some(base) = settings.base {
            map.skald.base = base;
        }
        if let Some(port) = settings.port {
            map.skald.port = port;
        }
        let bank = VoiceBank::open_capped(&settings.bank_root, settings.cap_bytes)
            .map_err(|err| err.to_string())?;
        let client = VoiceClient::new(&map.skald.base, map.skald.port, map.skald.sample_rate_hz);
        let queue = PrefetchQueue::new(client, bank, map.skald.deadline_ms);
        Ok(Arc::new(Voices {
            map,
            queue,
            hits: AtomicU64::new(0),
            tones: AtomicU64::new(0),
            pending: AtomicU64::new(0),
        }))
    }

    /// The voice map the lines are rendered through.
    pub fn map(&self) -> &VoiceMap {
        &self.map
    }

    /// The bank the queue renders into.
    pub fn bank(&self) -> &VoiceBank {
        self.queue.bank()
    }

    /// The queue's own counters: the renders, the failures and the
    /// latencies the worker measured.
    pub fn queue_stats(&self) -> QueueStats {
        self.queue.stats()
    }

    /// Wait until the worker has nothing in flight, or `upto` passes.
    ///
    /// This is a **report's** question (and a test's), never the game's:
    /// the game looks lines up and does not care whether the worker is
    /// busy. `true` when the queue drained inside the time.
    pub fn wait(&self, upto: std::time::Duration) -> bool {
        self.queue.wait(upto)
    }

    /// The counters of what the person has heard.
    pub fn counters(&self) -> VoiceCounters {
        VoiceCounters {
            hits: self.hits.load(Ordering::Relaxed),
            tones: self.tones.load(Ordering::Relaxed),
            pending: self.pending.load(Ordering::Relaxed),
        }
    }

    /// Submit a planned line for rendering: the moment the host knows it,
    /// which is the decision's arrival — never its due tick.
    ///
    /// The queue does the deduping: a line the bank already has is not
    /// submitted, and a line the worker already has is not duplicated.
    pub fn prefetch(&self, line: &Utterance) -> u64 {
        let speech = speech_of(&self.map, line);
        self.queue.submit(&speech, &self.map.skald.engine_id)
    }

    /// The line is due **now**: the clip if it is ready, the tone if it is
    /// not, the miss counted — and never a wait.
    ///
    /// This is the fallback contract at the host's ear: the round goes on
    /// whatever the voice service is doing, and the person hears the tone
    /// of a voice that was not ready.
    pub fn speak(&self, line: &Utterance) -> (TtsClip, Spoken) {
        let speech = speech_of(&self.map, line);
        let key = self.queue.bank().key(&speech, &self.map.skald.engine_id);
        // The pending glance is taken before the speak, because the speak
        // itself only says clip-or-tone: a tone whose line was in flight
        // is a different fact from a tone nobody ever submitted.
        let was_pending = matches!(self.queue.lookup(key), Lookup::Pending);
        let due = self
            .queue
            .speak(key, &TtsRequest::from_utterance(line, &speech.voice));
        let origin = match (&due, was_pending) {
            (Due::Clip(_), _) => SpokenOrigin::Hit,
            (Due::Tone(_), true) => SpokenOrigin::PendingTone,
            (Due::Tone(_), false) => SpokenOrigin::MissTone,
        };
        match origin {
            SpokenOrigin::Hit => {
                self.hits.fetch_add(1, Ordering::Relaxed);
            }
            SpokenOrigin::PendingTone => {
                self.tones.fetch_add(1, Ordering::Relaxed);
                self.pending.fetch_add(1, Ordering::Relaxed);
            }
            SpokenOrigin::MissTone => {
                self.tones.fetch_add(1, Ordering::Relaxed);
            }
        }
        let latency_ms = if origin.is_hit() {
            self.queue.bank().lookup(key).map(|meta| meta.latency_ms)
        } else {
            None
        };
        (
            due.clip().clone(),
            Spoken {
                text: line.prose(),
                voice: speech.voice,
                style: speech.style,
                origin,
                latency_ms,
                key,
            },
        )
    }
}

/// The skald ask of a line: the words, the speaker and the style, as the
/// voice map maps them.
///
/// The text is the line's prose, the speaker is the map's entry for the
/// agent that spoke, and the style is the map's answer for the line's
/// tone and intent (the intent's override winning, as the map says).
pub fn speech_of(map: &VoiceMap, line: &Utterance) -> SpeechLine {
    SpeechLine::new(
        line.prose(),
        map.voice_of(line.speaker),
        map.style_of(line.affect, line.intent),
    )
}

/// Where the bank lives when nobody says otherwise: the per-user cache
/// directory of the machine the host runs on.
///
/// On Windows that is `%LOCALAPPDATA%\last-mile\voice-bank`; elsewhere,
/// `$XDG_CACHE_HOME` or `~/.cache` under the same name. The bank is a
/// cache — hours of rendered clips for the human's ear — and it belongs
/// with the machine's other caches, not with the round's runs. A machine
/// that names none of these directories gets the system's temp dir, and
/// the host says where the bank ended up either way.
pub fn user_bank_root() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("last-mile").join("voice-bank")
}

/// Resolve where the bank lives: the command line's word, else the map's,
/// else the per-user default.
pub fn resolve_bank_root(map: &VoiceMap, cli: Option<&Path>) -> PathBuf {
    if let Some(root) = cli {
        return root.to_owned();
    }
    if !map.bank.root.trim().is_empty() {
        return PathBuf::from(map.bank.root.trim());
    }
    user_bank_root()
}

/// Pre-warm the bank from the phrasebook, in the background: play the
/// round's seed once with the stub minds of the shipped harness and ask
/// the bank for every line it speaks.
///
/// The stub round is the honest enumerator of the phrasebook — the same
/// gates, the same world, the same templates, so the lines it speaks are
/// the lines the round will say (six of them in VOICE.md's measurement,
/// not the hundreds a template-by-subject cross product would name). The
/// play costs a few CPU-seconds on its own thread and touches nothing of
/// the round the host is about to play: its sim is its own, and only the
/// asks travel, through the queue every other submit uses.
///
/// The handle is the thread's, so a test can wait for the warm bank; the
/// host fires and forgets.
pub fn prewarm(voices: Arc<Voices>, header: Header, ticks: u64) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("lm-voice-prewarm".to_owned())
        .spawn(move || {
            let lines = lm_tools::voice::stub_round_lines(&header, ticks);
            let mut asked = std::collections::BTreeSet::new();
            for line in &lines {
                asked.insert(voices.prefetch(line));
            }
            println!(
                "lm-host: the prewarm asked the bank for {} unique {} lines",
                asked.len(),
                lines.len()
            );
        })
        .expect("the prewarm thread starts")
}

/// The voices each squad of a roster speaks in, in squad order: the
/// squad's distinct speakers as the map maps its agents, oldest agent
/// first — `s0: mia+jason`, `s1: aria+jason`.
///
/// This is the HUD's own legend: the squad colour names the voice, through
/// `voice.toml`, so a reader can tell whose line they are hearing without
/// looking away from the map.
pub fn squad_voices(map: &VoiceMap, roster: &[lm_core::header::RosterEntry]) -> Vec<(u8, String)> {
    let mut squads: Vec<u8> = roster.iter().map(|entry| entry.squad).collect();
    squads.sort_unstable();
    squads.dedup();
    squads
        .into_iter()
        .map(|squad| {
            let mut names: Vec<&str> = Vec::new();
            for entry in roster.iter().filter(|entry| entry.squad == squad) {
                let voice = map.voice_of(entry.agent);
                if !names.contains(&voice) {
                    names.push(voice);
                }
            }
            (squad, names.join("+"))
        })
        .collect()
}
