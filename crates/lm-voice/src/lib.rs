//! The voice service: text to speech, speech to text, and the cache that
//! keeps the same line from being spoken twice.
//!
//! The round's score is the truth; the audio is for the human. A line is
//! synthesized **only** when the human's ear can hear it, and the sim never
//! depends on whether the human heard anything.
//!
//! The backends are stubs and placeholders on purpose:
//!
//! - [`StubTts`] and [`StubAsr`] are deterministic: no network, no clock,
//!   the same request always gives the same clip, byte for byte. They are
//!   what the tests and the headless runner use;
//! - [`RivaTts`] and [`RivaAsr`] are the adapters for the real service, and
//!   the Riva ports are still unknown (spec §12): the ports are
//!   **configuration** and no port is ever guessed. Until the spec sheet
//!   lands, an adapter builds its request and answers
//!   [`VoiceError::NotConfigured`];
//! - [`ProsodyPort`] measures a voice: the tracks Task 6 captures from the
//!   human's microphone.
//!
//! The prosody of a request is the score's prosody as SSML-style
//! parameters -- the rate, the pitch and the emphasis -- and never a
//! latent-space injection (research/03: the real levers are SSML, the
//! voice and the text).

use lm_core::fixed::Pos;
use lm_core::hash::Fnv64;
use lm_core::talk::TalkParams;
use lm_core::utterance::{Affect, SpeechAct, Utterance};
use lm_core::world::World;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;

pub mod bark;

pub use bark::{bark_key, BarkCache, DEFAULT_BARK_CAP};

/// The environment variable that names the Riva base URL.
pub const ENV_RIVA_BASE: &str = "LM_RIVA_BASE";
/// The environment variable that names the Riva TTS port.
pub const ENV_RIVA_TTS_PORT: &str = "LM_RIVA_TTS_PORT";
/// The environment variable that names the Riva ASR port.
pub const ENV_RIVA_ASR_PORT: &str = "LM_RIVA_ASR_PORT";

/// What the voice service can be asked for, and what it can be told.
///
/// The score's prosody travels as SSML-style parameters: the rate, the
/// pitch and the emphasis, plus the voice and the affect bucket. The
/// text is the wording the realizer chose -- the score's words, joined.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TtsRequest {
    /// The line, as it is spoken.
    pub text: String,
    /// The voice that speaks it.
    pub voice: String,
    /// The tone of the line: the bark's third key.
    pub affect: Affect,
    /// The pace, in Q16 (65 536 is the baseline pace).
    pub rate_q16: u32,
    /// The pitch, in semitones from the voice's baseline.
    pub pitch_steps: i8,
    /// The stressed words: the word's index and the stress, in Q16.
    pub emphasis: Vec<(usize, u32)>,
}

impl TtsRequest {
    /// A plain request: the text, the voice and the tone, at the neutral
    /// pace and pitch.
    pub fn new(text: impl Into<String>, voice: impl Into<String>, affect: Affect) -> TtsRequest {
        TtsRequest {
            text: text.into(),
            voice: voice.into(),
            affect,
            rate_q16: lm_core::utterance::Q16_ONE,
            pitch_steps: 0,
            emphasis: Vec::new(),
        }
    }

    /// The request for a line: the score's words as the text, and the
    /// score's lanes as the voice's parameters.
    ///
    /// The pace and the pitch are the line's own -- the first word's, which
    /// is the line's pace in every score the notation compiles -- and the
    /// emphasis is every word that carries a stress.
    pub fn from_utterance(u: &Utterance, voice: impl Into<String>) -> TtsRequest {
        let first = u.words.first();
        let emphasis = u
            .words
            .iter()
            .enumerate()
            .filter(|(_, word)| word.emphasis_q16 > 0)
            .map(|(index, word)| (index, word.emphasis_q16))
            .collect();
        TtsRequest {
            text: u.prose(),
            voice: voice.into(),
            affect: u.affect,
            rate_q16: first.map_or(lm_core::utterance::Q16_ONE, |w| w.rate_q16),
            pitch_steps: first.map_or(0, |w| w.pitch_steps),
            emphasis,
        }
    }
}

/// A spoken line: the samples, the rate they were made at, and how long
/// they last.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TtsClip {
    /// The samples, mono.
    pub pcm: Vec<i16>,
    /// The rate the samples were made at, in hertz.
    pub sample_rate: u32,
    /// How long the clip lasts, in milliseconds.
    pub duration_ms: u32,
}

/// One word of a transcript, and when it was said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscriptWord {
    /// The word, as it was heard.
    pub text: String,
    /// When the word started, in milliseconds.
    pub t_on_ms: u32,
    /// When the word ended, in milliseconds.
    pub t_off_ms: u32,
}

/// What the ASR heard: the words with their timings, and the line as a
/// whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transcript {
    /// The words, in order.
    pub words: Vec<TranscriptWord>,
    /// The line as text.
    pub text: String,
}

/// Everything that can go wrong in the voice service.
///
/// Every failure has a name: a refusal is an outcome, never a guess
/// (D26).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VoiceError {
    /// The backend is not configured: no host, no port, no service.
    NotConfigured,
    /// The backend did not answer in time.
    Timeout,
    /// The backend answered with an HTTP error.
    Http(u16),
    /// The backend answered with something that made no sense.
    Malformed(String),
    /// The microphone is dead: the capture is all silence, and a dead mic
    /// would otherwise become a silent, misread command.
    MicDead,
    /// The backend cannot do what was asked of it.
    Unsupported,
}

impl fmt::Display for VoiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VoiceError::NotConfigured => write!(f, "the voice backend is not configured"),
            VoiceError::Timeout => write!(f, "the voice backend did not answer in time"),
            VoiceError::Http(code) => write!(f, "the voice backend answered with HTTP {code}"),
            VoiceError::Malformed(why) => {
                write!(f, "the voice backend answered with nonsense: {why}")
            }
            VoiceError::MicDead => write!(f, "the microphone is dead"),
            VoiceError::Unsupported => write!(f, "the voice backend cannot do that"),
        }
    }
}

impl std::error::Error for VoiceError {}

/// A text-to-speech backend.
pub trait Tts {
    /// Speak a line.
    fn synthesize(&self, req: &TtsRequest) -> Result<TtsClip, VoiceError>;
}

/// A speech-to-text backend.
pub trait Asr {
    /// Listen to a capture and write down what was said.
    fn transcribe(&self, pcm: &[i16], sample_rate: u32) -> Result<Transcript, VoiceError>;
}

/// The measured tracks of a voice: what a service hears in a capture.
///
/// The tracks are integers, like the score they become: the pitch is in
/// Q16 hertz, the energy in Q16, and the host turns them into the
/// score's lanes in fixed point.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeasuredProsody {
    /// The pitch track: the frame and the pitch, in Q16 hertz.
    pub pitch_hz: Vec<(u32, u32)>,
    /// The energy track, in Q16.
    pub energy: Vec<u32>,
    /// The tempo, in beats per minute.
    pub tempo_bpm: u16,
    /// The pauses: the millisecond it starts and the millisecond it ends.
    pub pauses: Vec<(u32, u32)>,
    /// The gesture the service heard, if it heard one.
    pub act: Option<SpeechAct>,
    /// The tone the service heard.
    pub affect: Affect,
}

impl Default for MeasuredProsody {
    /// A capture with nothing measured in it: the tone is the neutral one.
    fn default() -> Self {
        MeasuredProsody {
            pitch_hz: Vec::new(),
            energy: Vec::new(),
            tempo_bpm: 0,
            pauses: Vec::new(),
            act: None,
            affect: Affect::Calm,
        }
    }
}

/// A service that measures a voice.
///
/// The prosody capture is a service, not a port: `communicator` is reused
/// as a local HTTP sidecar on the ProArt (Task 6), because a Rust port
/// would freeze one snapshot of that science into a second implementation
/// and drift from it.
pub trait ProsodyPort {
    /// Measure a capture.
    fn measure(&self, pcm: &[i16], sample_rate: u32) -> Result<MeasuredProsody, VoiceError>;
}

/// The deterministic TTS: no network, no clock.
///
/// The clip lasts 300 ms a word, and the samples are a ramp whose period
/// and span come from the affect and the voice, so the same request always
/// gives the same clip and a calm line is never the same clip as a
/// pressed one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StubTts {
    /// The rate of the clips it makes (16 kHz: the microphone's rate).
    pub sample_rate: u32,
}

impl Default for StubTts {
    fn default() -> Self {
        StubTts {
            sample_rate: 16_000,
        }
    }
}

impl Tts for StubTts {
    fn synthesize(&self, req: &TtsRequest) -> Result<TtsClip, VoiceError> {
        let words = u32::try_from(req.text.split_whitespace().count()).unwrap_or(u32::MAX);
        // The clip lasts 300 ms a word.
        let duration_ms = words.saturating_mul(300);
        let samples = duration_ms.saturating_mul(self.sample_rate) / 1000;
        Ok(TtsClip {
            pcm: ramp(req, samples),
            sample_rate: self.sample_rate,
            duration_ms,
        })
    }
}

/// The stub's wave: a ramp that repeats, with the period and the span of
/// the affect and the voice.
///
/// The stub is deterministic, not flat: a calm line and a pressed one are
/// different clips, because the bark's tone is the point (the cache keys
/// on the affect for exactly this reason).
fn ramp(req: &TtsRequest, samples: u32) -> Vec<i16> {
    let (period, span) = tone(req.affect, &req.voice);
    (0..samples)
        .map(|i| {
            let phase = i % period;
            let value = (span * i64::from(phase) * 2 / i64::from(period)) - span;
            i16::try_from(value).unwrap_or(0)
        })
        .collect()
}

/// The stub's tone: the period of the ramp and its span.
///
/// The affect sets the tone -- calm is slow and quiet, urgent is fast and
/// loud -- and the voice shifts the period a little, so two voices are
/// two clips.
fn tone(affect: Affect, voice: &str) -> (u32, i64) {
    let (period, span) = match affect {
        Affect::Calm => (160u32, 8_192i64),
        Affect::Urgent => (80, 24_576),
        Affect::Uncertain => (120, 16_384),
    };
    let mut h = Fnv64::new();
    h.write(voice.as_bytes());
    let shift = u32::try_from(h.finish() % 8).unwrap_or(0);
    (period + shift, span)
}

/// The deterministic ASR: it writes down the transcript it was scripted
/// with, whatever the capture holds.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct StubAsr {
    /// The words the stub hears.
    pub words: Vec<TranscriptWord>,
}

impl StubAsr {
    /// A stub that hears `text`: one word every 300 ms, like the stub's
    /// TTS.
    pub fn scripted(text: &str) -> StubAsr {
        let mut words = Vec::new();
        let mut at = 0u32;
        for word in text.split_whitespace() {
            words.push(TranscriptWord {
                text: word.to_owned(),
                t_on_ms: at,
                t_off_ms: at.saturating_add(300),
            });
            at = at.saturating_add(300);
        }
        StubAsr { words }
    }

    /// A stub that hears nothing.
    pub fn silent() -> StubAsr {
        StubAsr { words: Vec::new() }
    }
}

impl Asr for StubAsr {
    fn transcribe(&self, _pcm: &[i16], _sample_rate: u32) -> Result<Transcript, VoiceError> {
        let text = self
            .words
            .iter()
            .map(|word| word.text.as_str())
            .collect::<Vec<&str>>()
            .join(" ");
        Ok(Transcript {
            words: self.words.clone(),
            text,
        })
    }
}

/// The voice service's configuration (the client's `audio.*` rules).
///
/// The Riva ports are still unknown (spec §12), so they are configuration
/// and **no port is ever guessed**: a zero port means "not configured".
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceConfig {
    /// The base URL of the Riva service.
    pub riva_base: String,
    /// The TTS port of the Riva service; 0 means "not configured".
    pub riva_tts_port: u16,
    /// The ASR port of the Riva service; 0 means "not configured".
    pub riva_asr_port: u16,
    /// The voice the agents' lines are spoken in.
    pub voice: String,
    /// How many barks the cache holds.
    pub bark_cache_cap: usize,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        VoiceConfig {
            riva_base: String::new(),
            riva_tts_port: 0,
            riva_asr_port: 0,
            voice: "default".to_owned(),
            bark_cache_cap: DEFAULT_BARK_CAP,
        }
    }
}

/// Why a voice configuration could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VoiceConfigError {
    /// The file exists but cannot be read.
    Read(String),
    /// The file reads but does not parse as a [`VoiceConfig`].
    Parse(String),
}

impl fmt::Display for VoiceConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VoiceConfigError::Read(why) => write!(f, "voice config unreadable: {why}"),
            VoiceConfigError::Parse(why) => write!(f, "voice config unparseable: {why}"),
        }
    }
}

impl std::error::Error for VoiceConfigError {}

impl VoiceConfig {
    /// The configuration from a TOML document.
    pub fn parse(text: &str) -> Result<VoiceConfig, VoiceConfigError> {
        toml::from_str(text).map_err(|err| VoiceConfigError::Parse(err.to_string()))
    }

    /// The configuration from a file.
    pub fn load(path: &Path) -> Result<VoiceConfig, VoiceConfigError> {
        let text =
            std::fs::read_to_string(path).map_err(|err| VoiceConfigError::Read(err.to_string()))?;
        VoiceConfig::parse(&text)
    }
}

/// The Riva configuration the environment names, if it names one.
///
/// The live test reads the configuration from the environment, never from
/// the code: the port is whatever the configuration says, and a
/// configuration that says nothing means the service is not there.
pub fn riva_config_from_env() -> Option<VoiceConfig> {
    let base = std::env::var(ENV_RIVA_BASE).ok()?;
    if base.trim().is_empty() {
        return None;
    }
    Some(VoiceConfig {
        riva_base: base,
        riva_tts_port: port_from_env(ENV_RIVA_TTS_PORT),
        riva_asr_port: port_from_env(ENV_RIVA_ASR_PORT),
        ..VoiceConfig::default()
    })
}

/// The port the environment names, or zero when it names none.
fn port_from_env(name: &str) -> u16 {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.trim().parse::<u16>().ok())
        .unwrap_or(0)
}

/// The request the Riva adapter would send.
///
/// The shape of the call is provisional -- the spec sheet has not landed
/// (spec §12) -- but it is pinned by the tests, so the adapter cannot
/// drift while it waits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RivaTtsRequest {
    /// The base URL of the service.
    pub base: String,
    /// The port of the service.
    pub port: u16,
    /// The line, as it is spoken.
    pub text: String,
    /// The voice that speaks it.
    pub voice: String,
    /// The tone of the line.
    pub affect: Affect,
    /// The pace, in Q16.
    pub rate_q16: u32,
    /// The pitch, in semitones from the voice's baseline.
    pub pitch_steps: i8,
    /// The stressed words: the word's index and the stress, in Q16.
    pub emphasis: Vec<(usize, u32)>,
}

/// The Riva TTS adapter: a placeholder until the spec sheet lands.
///
/// The Riva ports are still unknown (spec §12), so the port is
/// configuration and no port is ever guessed: without one the adapter has
/// nothing to talk to, and says so.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RivaTts {
    /// The base URL of the service.
    pub base: String,
    /// The port of the service; 0 means "not configured".
    pub port: u16,
    /// The voice the service speaks in.
    pub voice: String,
}

impl RivaTts {
    /// The adapter for a host and a port.
    pub fn new(base: impl Into<String>, port: u16, voice: impl Into<String>) -> RivaTts {
        RivaTts {
            base: base.into(),
            port,
            voice: voice.into(),
        }
    }

    /// The adapter for a configuration that names a host and a port.
    ///
    /// A configuration that names neither is no adapter at all: the ports
    /// are unknown, and none is invented.
    pub fn from_config(cfg: &VoiceConfig) -> Option<RivaTts> {
        if cfg.riva_base.trim().is_empty() || cfg.riva_tts_port == 0 {
            return None;
        }
        Some(RivaTts {
            base: cfg.riva_base.clone(),
            port: cfg.riva_tts_port,
            voice: cfg.voice.clone(),
        })
    }

    /// Whether the adapter has a host and a port to talk to.
    pub fn is_configured(&self) -> bool {
        !self.base.trim().is_empty() && self.port != 0
    }

    /// The request the adapter would send.
    pub fn request(&self, req: &TtsRequest) -> RivaTtsRequest {
        RivaTtsRequest {
            base: self.base.clone(),
            port: self.port,
            text: req.text.clone(),
            voice: self.voice.clone(),
            affect: req.affect,
            rate_q16: req.rate_q16,
            pitch_steps: req.pitch_steps,
            emphasis: req.emphasis.clone(),
        }
    }
}

impl Tts for RivaTts {
    fn synthesize(&self, req: &TtsRequest) -> Result<TtsClip, VoiceError> {
        // The request is built, so the shape of the call is pinned while
        // it waits; the call itself waits for the spec sheet (spec §12).
        let _request = self.request(req);
        Err(VoiceError::NotConfigured)
    }
}

/// The Riva ASR adapter: a placeholder until the spec sheet lands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RivaAsr {
    /// The base URL of the service.
    pub base: String,
    /// The port of the service; 0 means "not configured".
    pub port: u16,
}

impl RivaAsr {
    /// The adapter for a host and a port.
    pub fn new(base: impl Into<String>, port: u16) -> RivaAsr {
        RivaAsr {
            base: base.into(),
            port,
        }
    }

    /// The adapter for a configuration that names a host and a port.
    pub fn from_config(cfg: &VoiceConfig) -> Option<RivaAsr> {
        if cfg.riva_base.trim().is_empty() || cfg.riva_asr_port == 0 {
            return None;
        }
        Some(RivaAsr {
            base: cfg.riva_base.clone(),
            port: cfg.riva_asr_port,
        })
    }

    /// Whether the adapter has a host and a port to talk to.
    pub fn is_configured(&self) -> bool {
        !self.base.trim().is_empty() && self.port != 0
    }

    /// The endpoint the adapter would post to.
    pub fn endpoint(&self) -> String {
        format!("{}:{}", self.base, self.port)
    }
}

impl Asr for RivaAsr {
    fn transcribe(&self, _pcm: &[i16], _sample_rate: u32) -> Result<Transcript, VoiceError> {
        // The endpoint is known; the call waits for the spec sheet (§12).
        let _endpoint = self.endpoint();
        Err(VoiceError::NotConfigured)
    }
}

/// Where a voice is: a position and a squad.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoicePlace {
    /// Where the voice is.
    pub pos: Pos,
    /// Which squad the voice belongs to.
    pub squad: u8,
}

impl VoicePlace {
    /// A voice at a position, in a squad.
    pub fn new(pos: Pos, squad: u8) -> VoicePlace {
        VoicePlace { pos, squad }
    }
}

/// Whether the human, listening where they listen, can hear the line.
///
/// The host asks the hearing physics before it asks for a clip: the
/// human's listener position is the possessed agent, else the followed
/// agent, else the camera's cell. An inaudible line costs no TTS call.
pub fn audible_to_human(
    line: &Utterance,
    speaker: VoicePlace,
    human: VoicePlace,
    world: &World,
    p: &TalkParams,
) -> bool {
    lm_core::hearing::audible_to_in_world(
        line,
        speaker.pos,
        human.pos,
        human.squad,
        speaker.squad,
        world,
        p,
    )
}

/// The voice service: the bark cache and the TTS behind it.
///
/// The service is what the host asks for a line. It asks the hearing
/// physics first -- an inaudible line costs no TTS call, because the audio
/// is for the human and the sim never depends on it -- and then the cache:
/// the same line, in the same voice, with the same affect is never
/// synthesized twice.
pub struct VoiceService {
    /// The backend that speaks.
    tts: Box<dyn Tts>,
    /// The clips that were already made.
    cache: BarkCache,
}

impl VoiceService {
    /// A service over a TTS, with the default cache.
    pub fn new(tts: Box<dyn Tts>) -> VoiceService {
        VoiceService {
            tts,
            cache: BarkCache::default(),
        }
    }

    /// A service over a TTS, with a cache of the caller's size.
    pub fn with_cache(tts: Box<dyn Tts>, cache: BarkCache) -> VoiceService {
        VoiceService { tts, cache }
    }

    /// The clip for a line, when the human can hear it.
    ///
    /// `None` means the line is out of earshot: nothing was synthesized
    /// and nothing was paid for. The line still happened in the sim.
    pub fn speak(
        &mut self,
        req: &TtsRequest,
        line: &Utterance,
        speaker: VoicePlace,
        human: VoicePlace,
        world: &World,
        p: &TalkParams,
    ) -> Result<Option<TtsClip>, VoiceError> {
        if !audible_to_human(line, speaker, human, world, p) {
            return Ok(None);
        }
        Ok(Some(self.cache.synthesize(self.tts.as_ref(), req)?))
    }

    /// The cache, for the HUD's counters.
    pub fn cache(&self) -> &BarkCache {
        &self.cache
    }
}

impl fmt::Debug for VoiceService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VoiceService")
            .field("barks", &self.cache.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lm_core::decision::{Decision, Kind, Provenance, Source, Stamped};
    use lm_core::fixed::UNITS_PER_M;
    use lm_core::header::Header;
    use lm_core::sim::Sim;
    use lm_core::utterance::{Addressee, Intent, Prosody, Word};
    use std::cell::Cell;
    use std::rc::Rc;

    /// The shipped reflex, so the round in the test really plays.
    const DEFAULT_REFLEX: &str = include_str!("../../../assets/reflexes/default.rhai");

    /// A TTS that counts the calls it gets.
    ///
    /// The counter is shared, so the test can look at it after the service
    /// has taken the backend by value.
    #[derive(Clone, Default)]
    pub(crate) struct CountingTts {
        calls: Rc<Cell<u32>>,
    }

    impl CountingTts {
        /// How many times the backend was asked to speak.
        pub(crate) fn calls(&self) -> u32 {
            self.calls.get()
        }
    }

    impl Tts for CountingTts {
        fn synthesize(&self, _req: &TtsRequest) -> Result<TtsClip, VoiceError> {
            self.calls.set(self.calls.get() + 1);
            Ok(TtsClip {
                pcm: vec![0; 16],
                sample_rate: 16_000,
                duration_ms: 300,
            })
        }
    }

    /// A four-word order on the voice channel.
    fn voice_line() -> Utterance {
        Utterance {
            speaker: 0,
            channel: lm_core::utterance::Channel::Voice,
            addressee: Addressee::Squad,
            act: SpeechAct::Command,
            intent: Intent::Order,
            affect: Affect::Urgent,
            subject: lm_core::utterance::Subject::None,
            prosody: Prosody::Synthesized,
            words: vec![
                Word::new("hold", 0, 10),
                Word::new("the", 10, 20),
                Word::new("north", 20, 30),
                Word::new("beacon", 30, 40),
            ],
        }
    }

    /// A decision of an agent, stamped for tick zero.
    fn stamped_utterance(agent: u16, line: Utterance) -> Stamped {
        Stamped {
            d: Decision {
                apply_tick: 0,
                source: Source::Agent(agent),
                seq: 0,
                kind: Kind::Utterance(line),
                provenance: Provenance::Test,
            },
            arrived: 0,
            tick: 0,
            late_by: 0,
        }
    }

    #[test]
    fn stub_tts_is_deterministic() {
        let tts = StubTts::default();
        let req = TtsRequest::new("hold the north beacon", "default", Affect::Urgent);

        let first = tts.synthesize(&req).expect("the stub answers");
        let second = tts.synthesize(&req).expect("the stub answers");

        assert_eq!(first, second, "the same request, the same clip");
        assert_eq!(first.pcm, second.pcm, "byte for byte");
        // The clip lasts 300 ms a word.
        assert_eq!(first.duration_ms, 4 * 300);
        // And the samples are the clip's length.
        assert_eq!(
            first.pcm.len() as u32,
            first.duration_ms * first.sample_rate / 1000
        );
        // A different tone is a different clip: the bark's tone is the
        // point.
        let calm = tts
            .synthesize(&TtsRequest::new(
                "hold the north beacon",
                "default",
                Affect::Calm,
            ))
            .expect("the stub answers");
        assert_ne!(first.pcm, calm.pcm);
    }

    #[test]
    fn inaudible_line_never_reaches_the_tts() {
        let h = Header::default_round(1, DEFAULT_REFLEX);
        let mut sim = Sim::new(&h).expect("the round starts");
        let speaker = sim.agent(0).expect("agent 0 plays");
        let speaker_place = VoicePlace::new(speaker.pos, speaker.squad);

        let line = voice_line();
        let req = TtsRequest::from_utterance(&line, "default");

        let tts = CountingTts::default();
        let calls = tts.calls.clone();
        let mut service = VoiceService::new(Box::new(tts));

        // The human listens 40 m away: a voice does not carry that far.
        let far = VoicePlace::new(
            speaker_place.pos.offset(40 * i64::from(UNITS_PER_M), 0),
            speaker_place.squad,
        );
        assert!(
            !audible_to_human(&line, speaker_place, far, sim.world(), &h.talk),
            "a voice does not reach 40 m"
        );

        // So the line costs no TTS call.
        let clip = service
            .speak(&req, &line, speaker_place, far, sim.world(), &h.talk)
            .expect("no error");
        assert!(clip.is_none(), "an inaudible line is not spoken");
        assert_eq!(calls.get(), 0, "an inaudible line costs no TTS call");

        // And the utterance still happened in the sim: the audio is for
        // the human, and the sim never depends on it.
        sim.step(&[stamped_utterance(0, line.clone())]);
        assert_eq!(
            sim.talk_records().len(),
            1,
            "the utterance happened in the sim"
        );

        // A human in earshot does get the line: the gate is a gate, not
        // a wall.
        let near = VoicePlace::new(
            speaker_place.pos.offset(5 * i64::from(UNITS_PER_M), 0),
            speaker_place.squad,
        );
        let clip = service
            .speak(&req, &line, speaker_place, near, sim.world(), &h.talk)
            .expect("no error");
        assert!(clip.is_some(), "a line in earshot is spoken");
        assert_eq!(calls.get(), 1, "the line reached the TTS once");
    }

    #[test]
    fn riva_is_not_configured_until_the_spec_sheet() {
        let req = TtsRequest::new("hold", "default", Affect::Urgent);

        // No port: the adapter has nothing to talk to, and says so.
        let tts = RivaTts::new("http://localhost", 0, "default");
        assert!(!tts.is_configured());
        assert!(matches!(
            tts.synthesize(&req),
            Err(VoiceError::NotConfigured)
        ));

        // The port is configuration, and it comes from the configuration.
        let cfg = VoiceConfig {
            riva_base: "http://riva.local".to_owned(),
            riva_tts_port: 5_000,
            ..VoiceConfig::default()
        };
        let tts = RivaTts::from_config(&cfg).expect("the configuration names a host and a port");
        assert_eq!(tts.port, 5_000);
        assert_eq!(tts.base, "http://riva.local");
        assert!(tts.is_configured());

        // The adapter builds the request...
        let built = tts.request(&req);
        assert_eq!(built.text, "hold");
        assert_eq!(built.port, 5_000);
        assert_eq!(built.base, "http://riva.local");
        assert_eq!(built.affect, Affect::Urgent);

        // ... and still refuses to speak until the spec sheet lands.
        assert!(matches!(
            tts.synthesize(&req),
            Err(VoiceError::NotConfigured)
        ));

        // The ASR is the same story.
        let asr = RivaAsr::new("http://riva.local", 0);
        assert!(!asr.is_configured());
        assert!(matches!(
            asr.transcribe(&[0, 0, 0], 16_000),
            Err(VoiceError::NotConfigured)
        ));

        // No port is ever guessed: the default configuration has none,
        // and neither adapter comes out of it.
        let default = VoiceConfig::default();
        assert_eq!(default.riva_tts_port, 0);
        assert_eq!(default.riva_asr_port, 0);
        assert!(RivaTts::from_config(&default).is_none());
        assert!(RivaAsr::from_config(&default).is_none());
    }

    /// The live test: it needs a Riva service, so it is opt-in.
    #[test]
    #[ignore = "the live Riva service is opt-in: it needs a configured service"]
    fn live_riva_tts() {
        let Some(cfg) = riva_config_from_env() else {
            eprintln!("no Riva configuration in the environment: skipping");
            return;
        };
        let Some(tts) = RivaTts::from_config(&cfg) else {
            eprintln!("the Riva configuration has no port: skipping");
            return;
        };
        let req = TtsRequest::new("hold", &cfg.voice, Affect::Calm);
        let clip = match tts.synthesize(&req) {
            Ok(clip) => clip,
            Err(VoiceError::NotConfigured) => panic!(
                "the Riva service is configured but the adapter is still a placeholder: \
                 the spec sheet has landed, so the adapter has to be built"
            ),
            Err(other) => panic!("the live service failed: {other}"),
        };
        assert!(clip.duration_ms > 0, "the live clip is longer than nothing");
    }
}
