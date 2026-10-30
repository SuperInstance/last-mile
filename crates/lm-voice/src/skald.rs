//! The skald client: one line, one HTTP call, one WAV — with a deadline.
//!
//! skald is the voice service on the Thor (the D67 sidecar): a daemon on
//! `:8000` fronting NVIDIA's TRT-LLM Magpie TTS. Its contract is the
//! OpenAI speech shape: `POST /v1/audio/speech` with `{"input", "voice"}`
//! answers a WAV at 22 050 Hz, mono, 16-bit.
//!
//! What the service accepts was **probed, not assumed** (2026-10-29):
//!
//! - the aliases `mia`, `jason`, `aria` and the canonical ids
//!   `Magpie-Multilingual.EN-US.<Speaker>[.<Style>]` work; the short form
//!   `Mia.Calm` answers HTTP 400;
//! - the emotion style is **not a parameter of its own** — it is the
//!   suffix of the voice id. The styles are `Default`, `Neutral`, `Calm`,
//!   `Angry`, `Happy` and `Sad`; anything else is refused;
//! - there is no rate, pitch or emphasis parameter: the daemon passes
//!   nothing synthetic to Riva, and the pace is the client's playback
//!   concern.
//!
//! So the score's prosody reaches skald as the **choice of voice id** and
//! nothing else: the speaker comes from the voice map, the style from the
//! tone, and [`canonical_voice_id`] folds the two into the one string the
//! service knows.
//!
//! A render takes about 0.7 s on an idle Thor and about 2.3 s for a 2.7 s
//! line on a busy one, which is why nothing in the game's hot path ever
//! calls this client: the prefetch worker does, ahead of time, through
//! the bank.

use std::io::Read;
use std::time::{Duration, Instant};

use serde_json::json;

use crate::voicemap::VoiceMap;
use crate::{VoiceError, ENV_SKALD_BASE};

/// The path the daemon answers speech on.
pub const SPEECH_PATH: &str = "/v1/audio/speech";

/// The voice daemon's base URL: the environment's, when it names one,
/// and the voice map's otherwise.
///
/// The daemon is on the Thor and the live lane runs from another box
/// (the ProArt), so the environment says where the daemon is. The map's
/// "localhost" is the default for a run on the daemon's own box.
pub fn skald_base(map: &VoiceMap) -> String {
    std::env::var(ENV_SKALD_BASE)
        .ok()
        .filter(|base| !base.trim().is_empty())
        .unwrap_or_else(|| map.skald.base.clone())
}

/// The prefix of every Magpie voice on the Thor.
const MAGPIE_PREFIX: &str = "Magpie-Multilingual.EN-US";

/// The emotion styles the Magpie voices accept, as the daemon's voice ids
/// spell them.
///
/// `Default` is the absence of a suffix — the plain speaker — and the
/// other five are the styles the engine ships with. Two probes matter:
/// the daemon answers HTTP 400 for any other spelling (`Mia.Calm` was
/// refused, and so was a `style` field of its own), and **the palette is
/// per speaker** — Jason, for one, carries no `Sad`, so a voice map
/// sticks to the styles its speakers actually have (the daemon's
/// `/v1/voices` is the authority).
pub const STYLES: [&str; 6] = ["Default", "Neutral", "Calm", "Angry", "Happy", "Sad"];

/// One line to render: the words, the speaker and the style.
///
/// `voice` is the speaker as the voice map names it — an alias (`mia`) or
/// a canonical id (`Magpie-Multilingual.EN-US.Mia`). `style` is one of
/// [`STYLES`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpeechLine {
    /// The line, as it is spoken.
    pub text: String,
    /// The speaker: an alias or a canonical id.
    pub voice: String,
    /// The emotion style, one of [`STYLES`].
    pub style: String,
}

impl SpeechLine {
    /// A line for `text`, spoken by `voice` in `style`.
    pub fn new(
        text: impl Into<String>,
        voice: impl Into<String>,
        style: impl Into<String>,
    ) -> Self {
        SpeechLine {
            text: text.into(),
            voice: voice.into(),
            style: style.into(),
        }
    }
}

/// The voice id the daemon is asked for: the speaker and the style folded
/// into the one string it knows.
///
/// An alias becomes `Magpie-Multilingual.EN-US.<Speaker>[.<Style>]` — the
/// speaker capitalized, `Default` leaving the suffix off. A voice that is
/// already canonical is passed through **as the map spelled it**, so a
/// harness that names a styled id outright (`...Mia.Calm`) keeps that
/// style whatever the tone table says, and one that names the plain id
/// gets the style appended.
pub fn canonical_voice_id(voice: &str, style: &str) -> String {
    if voice.starts_with(MAGPIE_PREFIX) {
        // Already canonical: the map's own spelling is the authority.
        return voice.to_owned();
    }
    let speaker = voice.trim();
    let mut speaker = speaker.to_owned();
    // The canonical ids capitalize the speaker: `mia` -> `Mia`.
    if let Some(first) = speaker.get_mut(0..1) {
        let upper: String = first.to_uppercase();
        speaker.replace_range(0..1, &upper);
    }
    if style.is_empty() || style == "Default" {
        format!("{MAGPIE_PREFIX}.{speaker}")
    } else {
        format!("{MAGPIE_PREFIX}.{speaker}.{style}")
    }
}

/// One rendered line: the WAV as it came back, and what was measured of
/// the call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rendered {
    /// The WAV the service answered: 22 050 Hz, mono, 16-bit.
    pub wav: Vec<u8>,
    /// The sample rate the WAV carries.
    pub sample_rate: u32,
    /// How long the clip lasts, in milliseconds.
    pub duration_ms: u64,
    /// How long the render took, in milliseconds.
    pub latency_ms: u64,
    /// The canonical voice id the service was asked for.
    pub voice: String,
}

/// The client of the skald daemon.
///
/// The client is a plain blocking HTTP call — the prefetch worker's thread
/// is the only caller in the game, so blocking is exactly right there.
/// Every call carries its own deadline: the worker hands one in, and a
/// service that cannot answer inside it answers
/// [`VoiceError::Timeout`] instead of holding the worker.
pub struct VoiceClient {
    agent: ureq::Agent,
    base: String,
    port: u16,
    sample_rate_hz: u32,
}

impl VoiceClient {
    /// The client for a daemon on `base:port`, asking for WAVs at
    /// `sample_rate_hz`.
    pub fn new(base: &str, port: u16, sample_rate_hz: u32) -> VoiceClient {
        // The agent's own timeout is a backstop: the per-call deadline is
        // the contract, and the backstop only stops a client whose
        // deadline was forgotten from hanging forever.
        let backstop = Duration::from_secs(60);
        VoiceClient {
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(backstop))
                .timeout_per_call(Some(backstop))
                .max_idle_connections_per_host(4)
                .proxy(None)
                .build()
                .into(),
            base: base.trim_end_matches('/').to_owned(),
            port,
            sample_rate_hz,
        }
    }

    /// The URL the client posts speech to.
    pub fn url(&self) -> String {
        format!("{}:{}{}", self.base, self.port, SPEECH_PATH)
    }

    /// Render one line to a WAV, before `deadline`.
    ///
    /// The deadline is the whole call's budget, end to end; a service that
    /// cannot answer inside it yields [`VoiceError::Timeout`], and a
    /// service that answers with an error code yields [`VoiceError::Http`].
    pub fn render(&self, line: &SpeechLine, deadline: Instant) -> Result<Rendered, VoiceError> {
        let started = Instant::now();
        let remaining = deadline.saturating_duration_since(started);
        if remaining.is_zero() {
            return Err(VoiceError::Timeout);
        }
        let voice = canonical_voice_id(&line.voice, &line.style);
        let body = json!({
            "input": line.text,
            "voice": voice,
            "sample_rate_hz": self.sample_rate_hz,
            "response_format": "wav",
        })
        .to_string();
        let response = self
            .agent
            .post(&self.url())
            .config()
            .timeout_global(Some(remaining))
            .build()
            .header("Content-Type", "application/json")
            .send(&body)
            .map_err(map_ureq_error)?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(VoiceError::Http(status));
        }
        let mut wav = Vec::new();
        response
            .into_body()
            .into_reader()
            .take(64 * 1024 * 1024)
            .read_to_end(&mut wav)
            .map_err(|err| VoiceError::Malformed(format!("the WAV did not arrive: {err}")))?;
        let latency = started.elapsed();
        let (_, _, rate, _) = wav_shape(&wav)?;
        let duration_ms = wav_duration_ms(&wav)?;
        Ok(Rendered {
            wav,
            duration_ms,
            sample_rate: rate,
            latency_ms: latency.as_millis() as u64,
            voice,
        })
    }
}

/// The facts of a WAV file: its channels, its bits, its rate and its
/// sample data.
///
/// The parser walks the RIFF chunks instead of trusting fixed offsets, and
/// a file that is not a PCM WAV is a [`VoiceError::Malformed`], never a
/// guessed duration.
pub fn wav_shape(wav: &[u8]) -> Result<(u16, u16, u32, Range), VoiceError> {
    if wav.len() < 12 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return Err(VoiceError::Malformed("not a RIFF/WAVE file".to_owned()));
    }
    let mut at = 12usize;
    let mut channels = 0u16;
    let mut bits = 0u16;
    let mut rate = 0u32;
    let mut data = Range { at: 0, len: 0 };
    while at + 8 <= wav.len() {
        let id = &wav[at..at + 4];
        let size = u32::from_le_bytes(wav[at + 4..at + 8].try_into().unwrap_or([0; 4])) as usize;
        let body = at + 8;
        if body + size > wav.len() {
            return Err(VoiceError::Malformed(format!(
                "the `{}` chunk runs past the file",
                String::from_utf8_lossy(id)
            )));
        }
        if id == b"fmt " {
            if size < 16 {
                return Err(VoiceError::Malformed("the fmt chunk is short".to_owned()));
            }
            channels = u16::from_le_bytes(wav[body + 2..body + 4].try_into().unwrap_or([0; 2]));
            rate = u32::from_le_bytes(wav[body + 4..body + 8].try_into().unwrap_or([0; 4]));
            bits = u16::from_le_bytes(wav[body + 14..body + 16].try_into().unwrap_or([0; 2]));
        } else if id == b"data" {
            data = Range {
                at: body,
                len: size,
            };
        }
        // Chunks are word-aligned: an odd size carries one pad byte.
        at = body + size + (size & 1);
    }
    if rate == 0 || channels == 0 || bits == 0 || data.len == 0 {
        return Err(VoiceError::Malformed(
            "the WAV has no fmt or no data".to_owned(),
        ));
    }
    Ok((channels, bits, rate, data))
}

/// Where a WAV's samples live: the offset and length of its data chunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    /// The byte the samples start at.
    pub at: usize,
    /// How many bytes of samples there are.
    pub len: usize,
}

/// How long a WAV lasts, in milliseconds.
pub fn wav_duration_ms(wav: &[u8]) -> Result<u64, VoiceError> {
    let (channels, bits, rate, data) = wav_shape(wav)?;
    let bytes_per_frame = u64::from(channels) * u64::from(bits) / 8;
    if bytes_per_frame == 0 {
        return Err(VoiceError::Malformed("a frame of no bytes".to_owned()));
    }
    Ok(data.len as u64 * 1000 / (u64::from(rate) * bytes_per_frame))
}

/// The samples of a 16-bit WAV, as the host plays them.
///
/// `None` when the file is not 16-bit PCM: the caller falls back to the
/// tone rather than guessing at a resample.
pub fn wav_pcm16(wav: &[u8]) -> Option<(Vec<i16>, u32)> {
    let (channels, bits, rate, data) = wav_shape(wav).ok()?;
    if bits != 16 {
        return None;
    }
    let samples: Vec<i16> = wav[data.at..data.at + data.len]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| i16::from_le_bytes(*pair))
        .collect();
    if channels != 1 {
        // The daemon is asked for mono; anything else is not ours.
        return None;
    }
    Some((samples, rate))
}

/// What an HTTP failure is, in the voice service's words.
fn map_ureq_error(err: ureq::Error) -> VoiceError {
    match err {
        ureq::Error::StatusCode(code) => VoiceError::Http(code),
        ureq::Error::Timeout(_) => VoiceError::Timeout,
        ureq::Error::Io(ref e) if e.kind() == std::io::ErrorKind::TimedOut => VoiceError::Timeout,
        other => VoiceError::Malformed(format!("the service is not there: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mockhttp::{MockSkald, Plan};
    use crate::prosody::wav_bytes;

    /// A WAV of silence lasting about `ms`, at the daemon's rate.
    fn silence(ms: usize) -> Vec<u8> {
        let samples = 22_050 * ms / 1000;
        wav_bytes(&vec![0i16; samples], 22_050)
    }

    #[test]
    fn the_style_is_the_suffix_of_the_voice_id() {
        // An alias plus a style folds into the canonical id.
        assert_eq!(
            canonical_voice_id("mia", "Calm"),
            "Magpie-Multilingual.EN-US.Mia.Calm"
        );
        // Default is the absence of a suffix.
        assert_eq!(
            canonical_voice_id("jason", "Default"),
            "Magpie-Multilingual.EN-US.Jason"
        );
        // A canonical id is passed through as the map spelled it.
        assert_eq!(
            canonical_voice_id("Magpie-Multilingual.EN-US.Aria.Sad", "Angry"),
            "Magpie-Multilingual.EN-US.Aria.Sad"
        );
    }

    #[test]
    fn a_line_renders_to_a_wav_with_a_latency() {
        let mock = MockSkald::start(Plan::Wav(silence(1500)));
        let client = VoiceClient::new(&mock.base(), mock.port(), 22_050);
        let line = SpeechLine::new("hold the north beacon", "mia", "Calm");
        let rendered = client
            .render(&line, Instant::now() + Duration::from_secs(5))
            .expect("the mock answers");
        assert_eq!(rendered.sample_rate, 22_050);
        assert_eq!(rendered.duration_ms, 1500);
        assert_eq!(
            rendered.voice, "Magpie-Multilingual.EN-US.Mia.Calm",
            "the style rides in the voice id"
        );
        // And the mock saw exactly that id in the body it was posted.
        let seen = mock.requests();
        assert_eq!(seen.len(), 1);
        let body = &seen[0];
        assert!(
            body.contains("Magpie-Multilingual.EN-US.Mia.Calm"),
            "{body}"
        );
        assert!(body.contains("hold the north beacon"));
        mock.join();
    }

    #[test]
    fn a_service_that_holds_the_answer_past_the_deadline_times_out() {
        // The mock holds its answer at the door; the deadline is 100 ms.
        let mock = MockSkald::start(Plan::Wav(silence(300)));
        mock.hold();
        let client = VoiceClient::new(&mock.base(), mock.port(), 22_050);
        let line = SpeechLine::new("hold", "mia", "Neutral");
        let answer = client.render(&line, Instant::now() + Duration::from_millis(100));
        assert!(matches!(answer, Err(VoiceError::Timeout)), "{answer:?}");
        // A deadline already spent is a timeout without a call at all.
        let spent = client.render(&line, Instant::now() - Duration::from_secs(1));
        assert!(matches!(spent, Err(VoiceError::Timeout)), "{spent:?}");
        mock.join();
    }

    #[test]
    fn an_error_status_is_named() {
        let mock = MockSkald::start(Plan::Status(400, "unknown voice".to_owned()));
        let client = VoiceClient::new(&mock.base(), mock.port(), 22_050);
        let line = SpeechLine::new("hold", "Mia.Calm", "Neutral");
        let answer = client.render(&line, Instant::now() + Duration::from_secs(2));
        assert!(matches!(answer, Err(VoiceError::Http(400))), "{answer:?}");
        mock.join();
    }

    #[test]
    fn nonsense_back_is_refused() {
        let mock = MockSkald::start(Plan::Wav(b"not a wav at all".to_vec()));
        let client = VoiceClient::new(&mock.base(), mock.port(), 22_050);
        let line = SpeechLine::new("hold", "mia", "Neutral");
        let answer = client.render(&line, Instant::now() + Duration::from_secs(2));
        assert!(
            matches!(answer, Err(VoiceError::Malformed(_))),
            "{answer:?}"
        );
        mock.join();
    }

    #[test]
    fn a_wavs_duration_comes_from_its_own_header() {
        // 22050 samples at 22050 Hz is one second, whatever the file says
        // it is called.
        assert_eq!(wav_duration_ms(&silence(1000)).unwrap(), 1000);
        assert_eq!(wav_duration_ms(&silence(1500)).unwrap(), 1500);
        assert!(wav_duration_ms(b"RIFFxxxxWAVEjunk").is_err());
    }

    #[test]
    fn the_samples_of_a_wav_read_back_as_they_went_in() {
        let wav = wav_bytes(&[0, -1, 2, -2], 16_000);
        let (pcm, rate) = wav_pcm16(&wav).expect("16-bit mono reads");
        assert_eq!(rate, 16_000);
        assert_eq!(pcm, vec![0, -1, 2, -2]);
    }

    /// The live test: the skald daemon on this Thor, opt-in like every
    /// live test (scripts/live.sh's pattern). It renders one line and
    /// checks the WAV the service answered with.
    ///
    /// The daemon's address is the environment's when it names one, so
    /// the lane can run this from another box.
    #[test]
    #[ignore = "the live skald daemon is opt-in: it must be running on :8000"]
    fn live_skald_speaks() {
        let map = VoiceMap::shipped();
        let client = VoiceClient::new(&skald_base(&map), map.skald.port, map.skald.sample_rate_hz);
        let line = SpeechLine::new("Hold the north beacon.", "mia", "Neutral");
        let rendered = match client.render(&line, Instant::now() + Duration::from_secs(10)) {
            Ok(rendered) => rendered,
            Err(err) => panic!("the live daemon failed: {err}"),
        };
        assert_eq!(rendered.sample_rate, 22_050);
        assert!(rendered.duration_ms > 500, "a spoken line is not empty");
        assert!(rendered.latency_ms > 0);
    }
}
