//! The ears: the speech-to-text backend over HTTP.
//!
//! The Thor serves LFM3-Audio-2B speech-to-text behind a small door
//! (`docs/research/ears/EARS.md`): the capture goes out as WAV and the
//! words come back. The door answers `POST /v1/audio/transcriptions`
//! (multipart, the field `file`) and `GET /health`.
//!
//! Three rules shape the adapter:
//!
//! - **the deadline is the adapter's.** A call that does not answer in
//!   time is a refusal with a name ([`VoiceError::Timeout`]), and the
//!   caller decides what to do about it: the host falls back to the stub
//!   and the HUD says the ears were late;
//! - **the model gives no times.** LFM3-Audio is an ASR model: it writes
//!   words, never times (`has_timestamps` is false). So the adapter lays
//!   the words out evenly across the capture -- a **declared** fallback,
//!   not an invented timestamp. The rule is fixed, so the same capture
//!   always gives the same score;
//! - **the health probe is a probe, not a promise.**
//!   [`crate::voicemap::AsrBackend::Auto`] asks it once, at startup: the
//!   ears if they answer, the stub if they do not.
//!
//! The configuration is the `[asr]` section of the voice map: which
//! backend hears, where the ears are, and how long a call may take. It is
//! one section and one file, so the game and the intelligibility gate
//! cannot disagree about where the ears are.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Deserialize;
use ureq::unversioned::multipart::{Form, Part};

use crate::prosody::wav_bytes;
use crate::voicemap::{AsrConfig, DEFAULT_LANGUAGE};
use crate::{Asr, Transcript, TranscriptWord, VoiceError};

/// The path the ears transcribe on.
pub const TRANSCRIBE_PATH: &str = "/v1/audio/transcriptions";

/// The path the health probe asks.
pub const HEALTH_PATH: &str = "/health";

/// How long the wire measurement waits for a handshake, in milliseconds.
///
/// It is a measurement and not a call: a box that is not there must not
/// hold the turn up, so the attempt gives up quickly and the number
/// says the wire was not there.
const WIRE_TIMEOUT: Duration = Duration::from_millis(300);

/// How long the health probe waits for the door's own answer, in
/// milliseconds.
///
/// The probe is asked once, at start-up, and the app must not wait for
/// it. A door that is slow is caught by the socket instead.
const PROBE_TIMEOUT_MS: u64 = 2_000;

/// How long the socket check waits for a handshake, in milliseconds.
const SOCKET_TIMEOUT_MS: u64 = 1_000;

/// What the health probe found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Probe {
    /// The door answered, and this is whether it says it is well.
    Answered(bool),
    /// The door did not answer in time: it is busy, not gone.
    TooSlow,
    /// Nothing is there.
    NoAnswer,
}

/// What one call to the ears cost.
///
/// The three numbers are the split the round's budget is read with: the
/// wire, the box, and the model's own time inside it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CallCost {
    /// The client's own wall clock: the whole call, the network included.
    pub client_ms: u64,
    /// The service's own time, as it reported it: the model's work.
    pub server_ms: u64,
    /// The round trip on the wire: the LAN and the door's socket.
    pub wire_ms: u64,
}

impl CallCost {
    /// The network: what the answer cost to travel.
    ///
    /// This is the LAN and nothing else: it is measured by a TCP
    /// handshake with the door's own address, which the kernel answers
    /// even when the service behind it is busy. So the number says how
    /// far the words travelled, not how long the box took to think.
    pub fn network_ms(&self) -> u64 {
        self.wire_ms
    }

    /// The compute: everything the Thor did — the queue in front of the
    /// model and the model's own work.
    pub fn compute_ms(&self) -> u64 {
        self.client_ms.saturating_sub(self.wire_ms)
    }
}

/// The ears: the capture goes out as WAV and the words come back.
///
/// The client is a blocking one, with the deadline on every call, and it
/// is `Send + Sync` because the host keeps one in an ECS resource.
#[derive(Clone)]
pub struct HttpAsr {
    agent: ureq::Agent,
    base: String,
    deadline_ms: u32,
    language: String,
    last: Arc<Mutex<Option<CallCost>>>,
}

impl HttpAsr {
    /// The client for a base URL and a deadline.
    pub fn new(base: impl Into<String>, deadline_ms: u32) -> HttpAsr {
        HttpAsr {
            agent: build_agent(Duration::from_millis(u64::from(deadline_ms))),
            base: base.into().trim_end_matches('/').to_owned(),
            deadline_ms,
            language: DEFAULT_LANGUAGE.to_owned(),
            last: Arc::new(Mutex::new(None)),
        }
    }

    /// The client for a configuration.
    pub fn from_config(cfg: &AsrConfig) -> HttpAsr {
        let mut asr = HttpAsr::new(cfg.url.clone(), cfg.deadline_ms);
        asr.language = cfg.language.clone();
        asr
    }

    /// The client with another language.
    pub fn with_language(mut self, language: impl Into<String>) -> HttpAsr {
        self.language = language.into();
        self
    }

    /// The base URL of the ears.
    pub fn base(&self) -> &str {
        &self.base
    }

    /// How long one call may take, in milliseconds.
    pub fn deadline_ms(&self) -> u32 {
        self.deadline_ms
    }

    /// Whether the client has somewhere to talk to.
    pub fn is_configured(&self) -> bool {
        !self.base.trim().is_empty()
    }

    /// The URL the client transcribes on.
    pub fn url(&self) -> String {
        format!("{}{}", self.base, TRANSCRIBE_PATH)
    }

    /// The URL the health probe asks.
    pub fn health_url(&self) -> String {
        format!("{}{}", self.base, HEALTH_PATH)
    }

    /// The words the ears hear in a wave file.
    ///
    /// The bytes of the wave go out as they are and the words come back
    /// as the service heard them. This is the intelligibility gate's
    /// question: does an ear on the other side agree with what the
    /// engine was asked to say?
    pub fn hear_wav(&self, path: &Path) -> Result<String, VoiceError> {
        let wav = std::fs::read(path)
            .map_err(|err| VoiceError::Malformed(format!("{}: {err}", path.display())))?;
        let answer = self.ask(&wav)?;
        Ok(answer.text.trim().to_owned())
    }

    /// Ask the ears about a wave: the bytes go out, the answer comes
    /// back.
    ///
    /// One call, one shape: the game sends a capture and the gate sends
    /// a rendered clip, and both go out the same door in the same
    /// multipart.
    fn ask(&self, wav: &[u8]) -> Result<Answer, VoiceError> {
        if !self.is_configured() {
            return Err(VoiceError::NotConfigured);
        }
        let started = Instant::now();
        // The wire is measured before the call, so it says what the
        // network costs at the moment the words go out.
        let wire = self.wire_ms();
        let part = Part::bytes(wav)
            .file_name("capture.wav")
            .mime_str("audio/wav")
            .map_err(|err| {
                VoiceError::Malformed(format!("the capture's type is not one: {err}"))
            })?;
        let mut form = Form::new().part("file", part);
        if !self.language.trim().is_empty() {
            form = form.text("language", self.language.trim());
        }
        let response = self.agent.post(&self.url()).send(form).map_err(|err| {
            // The call is over, whatever it ended in: the cost of it is
            // what it was, late or not.
            self.record(CallCost {
                client_ms: elapsed_ms(started),
                server_ms: 0,
                wire_ms: wire,
            });
            map_error(err)
        })?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            self.record(CallCost {
                client_ms: elapsed_ms(started),
                server_ms: 0,
                wire_ms: wire,
            });
            return Err(VoiceError::Http(status));
        }
        let body = response.into_body().read_to_string().map_err(|err| {
            self.record(CallCost {
                client_ms: elapsed_ms(started),
                server_ms: 0,
                wire_ms: wire,
            });
            map_error(err)
        })?;
        let answer: Answer = serde_json::from_str(&body)
            .map_err(|err| VoiceError::Malformed(format!("the answer does not parse: {err}")))?;
        self.record(CallCost {
            client_ms: elapsed_ms(started),
            server_ms: answer.server_ms(),
            wire_ms: wire,
        });
        Ok(answer)
    }

    /// Whether the ears are up.
    ///
    /// The probe asks the door, which asks the model behind it: a door
    /// that answers is a door whose model is loaded. It is a probe, not a
    /// promise -- the ears can go down the next millisecond, and the
    /// caller is told by name when they do.
    ///
    /// A busy box is not a dead box. The Thor's door answers its socket
    /// at once and its health path seconds later, when the model is
    /// queued behind someone else's work; so when the health path is
    /// slow, the socket is asked too, and a door that answers it is up.
    /// Only a door that answers neither is down.
    pub fn healthy(&self) -> bool {
        if !self.is_configured() {
            return false;
        }
        match self.probe_health() {
            // The door answered: believe it.
            Probe::Answered(healthy) => healthy,
            // The door is slow: a busy Thor answers its socket at once
            // and its health path seconds later, when the model is queued
            // behind someone else's work. The socket is asked too, and a
            // door that answers it is up — it is just busy, and the
            // turn's own deadline is what decides whether it is late.
            Probe::TooSlow => self.socket_answers(),
            // Nothing is there.
            Probe::NoAnswer => false,
        }
    }

    /// Ask the door's health path.
    ///
    /// The probe is short on purpose: it is asked once, at start-up, and
    /// the app must not wait for it. A door that is slow is caught by
    /// the socket below.
    fn probe_health(&self) -> Probe {
        let probe = build_agent(Duration::from_millis(PROBE_TIMEOUT_MS));
        match probe.get(&self.health_url()).call() {
            Ok(response) => Probe::Answered(response.status().is_success()),
            Err(ureq::Error::Timeout(_)) => Probe::TooSlow,
            Err(_) => Probe::NoAnswer,
        }
    }

    /// Whether the door's socket answers.
    ///
    /// The kernel answers the handshake even when the service behind it
    /// is busy, so this says the door is there -- not that it is quick.
    fn socket_answers(&self) -> bool {
        let Some(addresses) = self.address() else {
            return false;
        };
        addresses.iter().any(|addr| {
            std::net::TcpStream::connect_timeout(addr, Duration::from_millis(SOCKET_TIMEOUT_MS))
                .is_ok()
        })
    }

    /// What the last call cost.
    ///
    /// The number is the last call's, and there is one call at a time:
    /// the push-to-talk is half-duplex, so a call in flight is the only
    /// call there is.
    pub fn last_call(&self) -> Option<CallCost> {
        self.last.lock().ok().and_then(|slot| *slot)
    }

    /// Write down what the last call cost.
    fn record(&self, cost: CallCost) {
        if let Ok(mut slot) = self.last.lock() {
            *slot = Some(cost);
        }
    }

    /// The round trip on the wire, in milliseconds.
    ///
    /// The measurement is a TCP handshake with the door's own address and
    /// port. The kernel answers the handshake even when the service
    /// behind it is busy, so the number is the network's — the LAN, the
    /// switch, the route — and not the box's. It is the same thing the
    /// ping measures, and the budget is read with it: the Thor is five
    /// to eight milliseconds away, and everything above that is the
    /// ears' own work.
    ///
    /// A door that cannot be reached measures zero: there is no wire to
    /// speak of, and the failure says so by name.
    pub fn wire_ms(&self) -> u64 {
        let Some(addresses) = self.address() else {
            return 0;
        };
        let started = Instant::now();
        for addr in addresses {
            if std::net::TcpStream::connect_timeout(&addr, WIRE_TIMEOUT).is_ok() {
                return elapsed_ms(started);
            }
        }
        // Nothing answered: there is no wire to speak of, and the call
        // that follows says so by name.
        0
    }

    /// The door's address: the host and the port, as the base URL names
    /// them.
    fn address(&self) -> Option<Vec<std::net::SocketAddr>> {
        let rest = self
            .base
            .strip_prefix("http://")
            .or_else(|| self.base.strip_prefix("https://"))?;
        let host_port = rest.split('/').next().unwrap_or_default();
        if host_port.is_empty() {
            return None;
        }
        let resolved = match host_port.rsplit_once(':') {
            Some((host, port)) => format!("{host}:{port}"),
            None => format!("{host_port}:80"),
        };
        use std::net::ToSocketAddrs;
        resolved.to_socket_addrs().ok().map(|addrs| addrs.collect())
    }
}

impl std::fmt::Debug for HttpAsr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpAsr")
            .field("url", &self.url())
            .field("deadline_ms", &self.deadline_ms)
            .finish()
    }
}

impl Asr for HttpAsr {
    fn transcribe(&self, pcm: &[i16], sample_rate: u32) -> Result<Transcript, VoiceError> {
        let wav = wav_bytes(pcm, sample_rate);
        let answer = self.ask(&wav)?;
        Ok(transcript_from(&answer, pcm.len(), sample_rate))
    }
}

/// How long ago the call started, in milliseconds.
fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// The HTTP client: a blocking one, with the deadline on every call.
///
/// The connect timeout is the shorter one: the health probe asks at
/// start-up, and a box that is not on the LAN must not hold the app
/// still while it finds out.
fn build_agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .timeout_per_call(Some(timeout))
        .timeout_connect(Some(timeout.min(Duration::from_secs(2))))
        .max_idle_connections_per_host(4)
        .build()
        .into()
}

/// What an HTTP failure is, in the voice service's words.
fn map_error(err: ureq::Error) -> VoiceError {
    match err {
        ureq::Error::StatusCode(code) => VoiceError::Http(code),
        ureq::Error::Timeout(_) => VoiceError::Timeout,
        ureq::Error::Io(ref e) if e.kind() == std::io::ErrorKind::TimedOut => VoiceError::Timeout,
        other => VoiceError::Malformed(format!("the service is not there: {other}")),
    }
}

/// What the door answers with.
#[derive(Clone, Debug, Default, Deserialize)]
struct Answer {
    /// The line, as the model heard it.
    #[serde(default)]
    text: String,
    /// The words of the line.
    #[serde(default)]
    words: Vec<AnswerWord>,
    /// The service's own time, in milliseconds.
    #[serde(default)]
    latency_ms: Option<f64>,
    /// Whether the model gave times. It does not: LFM3-Audio is an ASR
    /// model, and the door says so rather than inventing times.
    #[serde(default)]
    has_timestamps: bool,
}

impl Answer {
    /// The service's own time, in whole milliseconds.
    fn server_ms(&self) -> u64 {
        let Some(ms) = self.latency_ms else {
            return 0;
        };
        if !ms.is_finite() || ms <= 0.0 {
            return 0;
        }
        // The cast saturates: a service that says it took a thousand
        // years is a service that is wrong, and the number is bounded.
        ms.round() as u64
    }
}

/// One word of the answer.
#[derive(Clone, Debug, Default, Deserialize)]
struct AnswerWord {
    /// The word, as it was heard.
    #[serde(default)]
    word: String,
    /// When the word started, in seconds. The ears leave it null.
    #[serde(default)]
    start: Option<f64>,
    /// When the word ended, in seconds. The ears leave it null.
    #[serde(default)]
    end: Option<f64>,
}

/// The transcript of an answer.
///
/// The words come from the answer's own list when it has one, and from
/// the line itself when it does not. The times come from the answer when
/// it gives them, and are **spread evenly across the capture** when it
/// does not -- the declared fallback, not an invented timestamp.
fn transcript_from(answer: &Answer, samples: usize, sample_rate: u32) -> Transcript {
    let words: Vec<String> = if answer.words.is_empty() {
        split_words(&answer.text)
    } else {
        answer
            .words
            .iter()
            .map(|word| word.word.trim().to_owned())
            .filter(|word| !word.is_empty())
            .collect()
    };
    let duration_ms = capture_ms(samples, sample_rate);
    let timed = timed_words(answer, &words, duration_ms);
    let text = answer.text.trim().to_owned();
    Transcript {
        words: timed,
        text: if text.is_empty() {
            words.join(" ")
        } else {
            text
        },
    }
}

/// The words of a line, without the punctuation that is not part of them.
///
/// The door splits the same way (`tools/ears/ears.py`): "Hold" and
/// "line." are words, and the full stop is not.
fn split_words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|word| {
            word.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '\u{2019}')
                .to_owned()
        })
        .filter(|word| !word.is_empty())
        .collect()
}

/// How long the capture was, in milliseconds.
fn capture_ms(samples: usize, sample_rate: u32) -> u32 {
    if sample_rate == 0 {
        return 0;
    }
    u32::try_from(u64::try_from(samples).unwrap_or(u64::MAX) * 1000 / u64::from(sample_rate))
        .unwrap_or(u32::MAX)
}

/// The words of the answer, with their times.
///
/// The times the model gave are kept -- they are the model's own, and the
/// answer says it has them (`has_timestamps`). The times it did not give
/// are **spread evenly across the capture**: the capture's length, cut
/// into as many slices as there are words. That is a declared fallback,
/// not an invented timestamp, and the rule is fixed, so the same capture
/// always gives the same score.
fn timed_words(answer: &Answer, words: &[String], duration_ms: u32) -> Vec<TranscriptWord> {
    if words.is_empty() {
        return Vec::new();
    }
    let each = duration_ms / u32::try_from(words.len()).unwrap_or(u32::MAX).max(1);
    words
        .iter()
        .enumerate()
        .map(|(index, word)| {
            let spread = TranscriptWord {
                text: word.clone(),
                t_on_ms: each.saturating_mul(u32::try_from(index).unwrap_or(u32::MAX)),
                t_off_ms: each.saturating_mul(u32::try_from(index + 1).unwrap_or(u32::MAX)),
            };
            if !answer.has_timestamps {
                return spread;
            }
            let said = answer.words.get(index);
            let on = said.and_then(|word| word.start).map(seconds_to_ms);
            let off = said.and_then(|word| word.end).map(seconds_to_ms);
            match (on, off) {
                (Some(on), Some(off)) if off >= on => TranscriptWord {
                    text: word.clone(),
                    t_on_ms: on,
                    t_off_ms: off,
                },
                _ => spread,
            }
        })
        .collect()
}

/// A number of seconds, in whole milliseconds.
fn seconds_to_ms(seconds: f64) -> u32 {
    if !seconds.is_finite() || seconds <= 0.0 {
        return 0;
    }
    u32::try_from((seconds * 1000.0).round() as u64).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voicemap::DEFAULT_EARS_URL;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;
    use std::thread;

    /// A tiny HTTP server for the tests.
    ///
    /// It answers each path with its own status, body and delay, and it
    /// writes down the bodies it was sent. No network is needed: it is
    /// a socket on the loopback, and the test is the only client.
    struct MockServer {
        port: u16,
        bodies: Arc<Mutex<Vec<String>>>,
        heads: Arc<Mutex<Vec<String>>>,
        running: Arc<AtomicBool>,
        handle: Option<thread::JoinHandle<()>>,
    }

    impl MockServer {
        /// Start a server: `paths` maps a path to (status, body, delay).
        fn start(paths: Vec<(&'static str, u16, String, Duration)>) -> MockServer {
            let listener = TcpListener::bind("127.0.0.1:0").expect("the mock binds");
            listener.set_nonblocking(true).expect("the mock polls");
            let port = listener
                .local_addr()
                .expect("the mock has an address")
                .port();
            let bodies = Arc::new(Mutex::new(Vec::new()));
            let heads = Arc::new(Mutex::new(Vec::new()));
            let running = Arc::new(AtomicBool::new(true));
            let served = Arc::new(AtomicUsize::new(0));

            let handle = {
                let bodies = Arc::clone(&bodies);
                let heads = Arc::clone(&heads);
                let running = Arc::clone(&running);
                let served = Arc::clone(&served);
                thread::spawn(move || {
                    while running.load(Ordering::SeqCst) {
                        match listener.accept() {
                            Ok((stream, _)) => {
                                served.fetch_add(1, Ordering::SeqCst);
                                serve(stream, &paths, &bodies, &heads);
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                thread::sleep(Duration::from_millis(2));
                            }
                            Err(_) => break,
                        }
                    }
                })
            };

            MockServer {
                port,
                bodies,
                heads,
                running,
                handle: Some(handle),
            }
        }

        /// The base URL of the mock.
        fn base(&self) -> String {
            format!("http://127.0.0.1:{}", self.port)
        }

        /// The bodies the mock was sent.
        fn bodies(&self) -> Vec<String> {
            self.bodies.lock().expect("the log is not poisoned").clone()
        }

        /// The head of the last request.
        fn last_head(&self) -> String {
            self.heads
                .lock()
                .expect("the log is not poisoned")
                .last()
                .cloned()
                .unwrap_or_default()
        }
    }

    impl Drop for MockServer {
        fn drop(&mut self) {
            self.running.store(false, Ordering::SeqCst);
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    /// Answer the requests on one connection, until it closes.
    fn serve(
        mut stream: TcpStream,
        paths: &[(&'static str, u16, String, Duration)],
        bodies: &Mutex<Vec<String>>,
        heads: &Mutex<Vec<String>>,
    ) {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
        let mut buf = Vec::new();
        loop {
            let Some((head, body)) = read_request(&mut stream, &mut buf) else {
                return;
            };
            let path = head
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .to_owned();
            heads.lock().expect("the log is not poisoned").push(head);
            bodies.lock().expect("the log is not poisoned").push(body);

            let Some((_, status, answer, delay)) = paths
                .iter()
                .find(|(wanted, _, _, _)| path.starts_with(wanted))
            else {
                let _ = write_reply(&mut stream, 404, "{\"error\":\"no such path\"}");
                continue;
            };
            if !delay.is_zero() {
                thread::sleep(*delay);
            }
            let _ = write_reply(&mut stream, *status, answer);
        }
    }

    /// Read one request: the head and the body it names.
    fn read_request(stream: &mut TcpStream, buf: &mut Vec<u8>) -> Option<(String, String)> {
        let head_end = loop {
            if let Some(index) = find(buf, b"\r\n\r\n") {
                break index;
            }
            let mut chunk = [0u8; 1024];
            let read = stream.read(&mut chunk).ok()?;
            if read == 0 {
                return None;
            }
            buf.extend_from_slice(&chunk[..read]);
        };
        let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
        let length = head
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                if name.trim().eq_ignore_ascii_case("content-length") {
                    value.trim().parse::<usize>().ok()
                } else {
                    None
                }
            })
            .unwrap_or(0);
        while buf.len() < head_end + 4 + length {
            let mut chunk = [0u8; 1024];
            let read = stream.read(&mut chunk).ok()?;
            if read == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..read]);
        }
        let body = String::from_utf8_lossy(
            buf.get(head_end + 4..head_end + 4 + length)
                .unwrap_or_default(),
        )
        .to_string();
        buf.drain(..head_end + 4 + length);
        Some((head, body))
    }

    /// Where a needle is in the haystack.
    fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
    }

    /// Answer a request.
    fn write_reply(stream: &mut TcpStream, status: u16, body: &str) -> std::io::Result<()> {
        let reason = if (200..300).contains(&status) {
            "OK"
        } else {
            "Error"
        };
        let reply = format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n{body}",
            len = body.len(),
        );
        stream.write_all(reply.as_bytes())?;
        stream.flush()
    }

    /// The answer the mock gives for a transcription.
    const HEARD: &str = r#"{"text": "Hold the line.", "words": [{"word": "Hold", "start": null, "end": null}, {"word": "the", "start": null, "end": null}, {"word": "line", "start": null, "end": null}], "latency_ms": 114.95, "has_timestamps": false, "timestamps": null, "model": "LFM3-Audio-2B", "language": "en"}"#;

    /// The mock: healthy, and it hears the line.
    fn mock() -> MockServer {
        MockServer::start(vec![
            (
                HEALTH_PATH,
                200,
                r#"{"status":"ok","service":"lm-ears"}"#.to_owned(),
                Duration::ZERO,
            ),
            (TRANSCRIBE_PATH, 200, HEARD.to_owned(), Duration::ZERO),
        ])
    }

    #[test]
    fn the_words_come_back_from_the_door() {
        let mock = mock();
        let asr = HttpAsr::new(mock.base(), 2_000);

        let transcript = asr
            .transcribe(&[1, -1, 2, -2], 16_000)
            .expect("the ears hear");

        assert_eq!(transcript.text, "Hold the line.");
        assert_eq!(transcript.words.len(), 3);
        assert_eq!(transcript.words[0].text, "Hold");
        assert_eq!(transcript.words[2].text, "line");
    }

    #[test]
    fn the_capture_goes_out_as_a_wav_in_a_multipart() {
        let mock = mock();
        let asr = HttpAsr::new(mock.base(), 2_000);

        asr.transcribe(&[1, -1, 2, -2], 16_000)
            .expect("the ears hear");

        let head = mock.last_head();
        assert!(head.contains("POST /v1/audio/transcriptions"), "{head}");
        assert!(
            head.to_ascii_lowercase().contains("multipart/form-data"),
            "{head}"
        );
        let body = mock.bodies().join("\n");
        assert!(body.contains("name=\"file\""), "{body}");
        assert!(body.contains("filename=\"capture.wav\""), "{body}");
        // And the wave itself: the RIFF header and the samples.
        assert!(body.contains("RIFF"), "{body}");
        assert!(body.contains("WAVE"), "{body}");
    }

    #[test]
    fn the_words_with_no_times_are_spread_across_the_capture() {
        let mock = mock();
        let asr = HttpAsr::new(mock.base(), 2_000);

        // A second at 16 kHz: three words over a second.
        let pcm = vec![1i16; 16_000];
        let transcript = asr.transcribe(&pcm, 16_000).expect("the ears hear");

        assert_eq!(transcript.words.len(), 3);
        // The rule is fixed: the capture's length, cut in three.
        assert_eq!(transcript.words[0].t_on_ms, 0);
        assert_eq!(transcript.words[0].t_off_ms, 333);
        assert_eq!(transcript.words[1].t_on_ms, 333);
        assert_eq!(transcript.words[1].t_off_ms, 666);
        assert_eq!(transcript.words[2].t_on_ms, 666);
        assert_eq!(transcript.words[2].t_off_ms, 999);
    }

    #[test]
    fn the_times_the_model_gives_are_kept() {
        let mock = MockServer::start(vec![(
            TRANSCRIBE_PATH,
            200,
            r#"{"text": "Hold the line.", "words": [{"word": "Hold", "start": 0.25, "end": 0.75}, {"word": "the", "start": 0.75, "end": 1.0}, {"word": "line", "start": 1.0, "end": 1.5}], "latency_ms": 10.0, "has_timestamps": true}"#
                .to_owned(),
            Duration::ZERO,
        )]);
        let asr = HttpAsr::new(mock.base(), 2_000);

        let transcript = asr.transcribe(&[1; 16_000], 16_000).expect("the ears hear");

        assert_eq!(transcript.words[0].t_on_ms, 250);
        assert_eq!(transcript.words[0].t_off_ms, 750);
        assert_eq!(transcript.words[2].t_off_ms, 1_500);
    }

    #[test]
    fn a_line_with_no_word_list_is_split_into_words() {
        let mock = MockServer::start(vec![(
            TRANSCRIBE_PATH,
            200,
            r#"{"text": "Hold the line.", "latency_ms": 10.0}"#.to_owned(),
            Duration::ZERO,
        )]);
        let asr = HttpAsr::new(mock.base(), 2_000);

        let transcript = asr.transcribe(&[1; 16_000], 16_000).expect("the ears hear");

        assert_eq!(transcript.words.len(), 3);
        assert_eq!(transcript.words[1].text, "the");
        assert_eq!(transcript.text, "Hold the line.");
    }

    #[test]
    fn a_dead_door_is_a_refusal_with_a_name() {
        // Nothing is listening on this port: the connection is refused.
        let asr = HttpAsr::new("http://127.0.0.1:9", 500);

        let err = asr
            .transcribe(&[1, -1], 16_000)
            .expect_err("the ears are not there");
        assert!(
            matches!(err, VoiceError::Malformed(_)),
            "the failure is named, got {err:?}"
        );
    }

    #[test]
    fn a_late_door_is_a_timeout() {
        let mock = MockServer::start(vec![(
            TRANSCRIBE_PATH,
            200,
            HEARD.to_owned(),
            Duration::from_millis(400),
        )]);
        let asr = HttpAsr::new(mock.base(), 100);

        let err = asr
            .transcribe(&[1, -1], 16_000)
            .expect_err("the ears are too late");
        assert_eq!(err, VoiceError::Timeout);
    }

    #[test]
    fn an_answer_that_makes_no_sense_is_refused() {
        let mock = MockServer::start(vec![(
            TRANSCRIBE_PATH,
            200,
            "not json at all".to_owned(),
            Duration::ZERO,
        )]);
        let asr = HttpAsr::new(mock.base(), 2_000);

        let err = asr
            .transcribe(&[1, -1], 16_000)
            .expect_err("the answer is nonsense");
        assert!(matches!(err, VoiceError::Malformed(_)), "got {err:?}");
    }

    #[test]
    fn an_error_status_is_a_refusal_with_the_status() {
        let mock = MockServer::start(vec![(
            TRANSCRIBE_PATH,
            503,
            "{}".to_owned(),
            Duration::ZERO,
        )]);
        let asr = HttpAsr::new(mock.base(), 2_000);

        let err = asr
            .transcribe(&[1, -1], 16_000)
            .expect_err("the door is not serving");
        assert_eq!(err, VoiceError::Http(503));
    }

    #[test]
    fn the_health_probe_asks_the_door() {
        let mock = mock();
        let asr = HttpAsr::new(mock.base(), 2_000);

        assert!(asr.healthy(), "the mock is up");
        assert!(
            mock.last_head().contains("GET /health"),
            "{}",
            mock.last_head()
        );
    }

    #[test]
    fn the_health_probe_says_no_when_the_door_is_down() {
        let mock = MockServer::start(vec![(
            HEALTH_PATH,
            503,
            r#"{"status":"degraded"}"#.to_owned(),
            Duration::ZERO,
        )]);
        let asr = HttpAsr::new(mock.base(), 2_000);

        assert!(!asr.healthy(), "the door is degraded");
    }

    #[test]
    fn a_door_that_is_there_answers_its_socket() {
        // A door that is busy still holds its socket: the kernel answers
        // the handshake even when the service behind it is queued behind
        // someone else's work.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("the socket");
        let addr = listener.local_addr().expect("the address");
        let asr = HttpAsr::new(format!("http://{addr}"), 2_000);

        assert!(asr.socket_answers(), "the socket answers");
        // And the wire is measured on it.
        assert!(asr.wire_ms() < 100, "the wire took {} ms", asr.wire_ms());
    }

    #[test]
    fn a_client_with_no_ears_says_so() {
        let asr = HttpAsr::new("", 2_000);
        assert!(!asr.is_configured());
        assert!(!asr.healthy());
        assert!(matches!(
            asr.transcribe(&[1, -1], 16_000),
            Err(VoiceError::NotConfigured)
        ));
    }

    #[test]
    fn the_call_is_measured_and_split() {
        let mock = MockServer::start(vec![(
            TRANSCRIBE_PATH,
            200,
            r#"{"text": "Hold.", "latency_ms": 100.0}"#.to_owned(),
            Duration::from_millis(120),
        )]);
        let asr = HttpAsr::new(mock.base(), 2_000);

        asr.transcribe(&[1; 16_000], 16_000).expect("the ears hear");

        let cost = asr.last_call().expect("the call was measured");
        // The service says it worked for 100 ms; the call took the round
        // trip on top of that.
        assert_eq!(cost.server_ms, 100);
        assert!(cost.client_ms >= 120, "the call took {}", cost.client_ms);
        // The compute is everything the box did: the call, less the wire.
        assert_eq!(cost.compute_ms(), cost.client_ms - cost.network_ms());
        // And the wire is the LAN: the mock is on the loopback, so it is
        // a handshake's worth of milliseconds and no more.
        assert!(
            cost.network_ms() < 100,
            "the wire took {} ms",
            cost.network_ms()
        );
    }

    #[test]
    fn the_wire_is_the_lan_and_not_the_box() {
        // The mock is on the loopback: the handshake is quick, and the
        // number says nothing about how long the service thought.
        let mock = MockServer::start(vec![(HEALTH_PATH, 200, "{}".to_owned(), Duration::ZERO)]);
        let asr = HttpAsr::new(mock.base(), 2_000);

        let wire = asr.wire_ms();

        assert!(wire < 100, "the loopback took {wire} ms");
        // A base that names no address has no wire to speak of.
        assert_eq!(HttpAsr::new("not a url", 2_000).wire_ms(), 0);
    }

    #[test]
    fn a_wave_file_is_heard_as_it_is() {
        let mock = mock();
        let asr = HttpAsr::new(mock.base(), 2_000);
        let dir = std::env::temp_dir().join(format!("lm-ears-wave-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the scratch dir is made");
        let wav = dir.join("line.wav");
        let bytes = wav_bytes(&[1, -1, 2, -2], 16_000);
        std::fs::write(&wav, &bytes).expect("the wave is written");

        let heard = asr.hear_wav(&wav).expect("the ears hear the file");

        assert_eq!(heard, "Hold the line.");
        // And the file's bytes went out, not a capture built from them.
        let body = mock.bodies().join("\n");
        assert!(body.contains("RIFF"), "{body}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_wave_file_that_is_not_there_is_refused_by_name() {
        let mock = mock();
        let asr = HttpAsr::new(mock.base(), 2_000);

        let err = asr
            .hear_wav(std::path::Path::new("/tmp/lm-ears-no-such-wave.wav"))
            .expect_err("the wave is not there");

        assert!(matches!(err, VoiceError::Malformed(_)), "got {err:?}");
    }

    /// The live test: the ears on the Thor.
    ///
    /// It is behind `#[ignore]` because the Thor is a box on the LAN and
    /// not a dependency of the build: `scripts/live.sh` runs it there.
    #[test]
    #[ignore]
    fn live_ears() {
        let base =
            std::env::var(crate::ENV_EARS_BASE).unwrap_or_else(|_| DEFAULT_EARS_URL.to_owned());
        let asr = HttpAsr::new(base, 10_000);
        assert!(asr.healthy(), "the ears are not up at {}", asr.base());

        // A second of a ramp: the door answers a wave even when the wave
        // is not speech, and the answer is still a line.
        let pcm: Vec<i16> = (0..16_000).map(|i| (i % 64) as i16 - 32).collect();
        let transcript = asr.transcribe(&pcm, 16_000).expect("the ears hear");
        println!("the ears heard: {:?}", transcript.text);
        assert!(
            transcript.words.is_empty() || !transcript.text.is_empty(),
            "the ears answered with nothing at all"
        );
    }
}
