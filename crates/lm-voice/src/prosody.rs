//! The prosody sidecar: `communicator` as a service on the ProArt.
//!
//! The prosody capture is a **service**, not a port (D53). `communicator`
//! is Python with a heavy dependency set (`typesafe_sdk`, numpy, silero
//! ONNX), it lives outside this Rust workspace, and its science *is* the
//! measured tracks. A Rust port would freeze one snapshot of that science
//! into a second implementation and drift from it -- exactly what D53 says
//! not to do.
//!
//! So the sidecar runs **here**, on the ProArt: the microphone audio is
//! 16 kHz mono and never leaves the machine, and the judged tracks can be
//! made locally (tev1, fleet/08) -- no cloud key and no LAN hop. The
//! measured DSP is the same wherever it runs.
//!
//! The service is **optional**. When it is absent, or when it answers with
//! an error, the capture still gives its words and the score is built
//! without prosody ([`lm_core::utterance::Prosody::Absent`]). The round
//! never waits for it.
//!
//! ## The contract
//!
//! The sidecar answers `POST /score` with the capture as WAV (16-bit
//! mono), and answers with the measured tracks as JSON:
//!
//! ```json
//! {
//!   "pitch_hz": [[0, 14417920], [10, 14417920]],
//!   "energy": [65536, 65536],
//!   "tempo_bpm": 200,
//!   "pauses": [[300, 450]],
//!   "act": "command",
//!   "affect": "calm"
//! }
//! ```
//!
//! The pitch is in Q16 hertz and the energy in Q16, because the tracks
//! become the score's lanes and the score is integer-only.

use std::time::Duration;

use serde::Deserialize;

use lm_core::utterance::{Affect, SpeechAct};

use crate::{MeasuredProsody, ProsodyPort, VoiceError};

/// The path the sidecar answers on.
pub const SCORE_PATH: &str = "/score";

/// How long a call to the sidecar may take, in milliseconds.
///
/// The call is made outside the round's tick, so the round never waits for
/// it; the timeout is only here so a wedged sidecar cannot hold the host.
pub const DEFAULT_TIMEOUT_MS: u32 = 500;

/// The client of the local prosody sidecar.
///
/// The client is a plain HTTP call: the capture goes in as WAV and the
/// measured tracks come out as JSON. There is no key and no cloud: the
/// sidecar is on the ProArt, and the audio never leaves the machine.
pub struct ProsodyService {
    agent: ureq::Agent,
    base: String,
    port: u16,
}

impl ProsodyService {
    /// The client for a sidecar on `base:port`.
    pub fn new(base: &str, port: u16) -> ProsodyService {
        ProsodyService {
            agent: build_agent(Duration::from_millis(u64::from(DEFAULT_TIMEOUT_MS))),
            base: base.trim_end_matches('/').to_owned(),
            port,
        }
    }

    /// The client with another timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> ProsodyService {
        self.agent = build_agent(timeout);
        self
    }

    /// Whether the client has a host and a port to talk to.
    pub fn is_configured(&self) -> bool {
        !self.base.trim().is_empty() && self.port != 0
    }

    /// The URL the client posts to.
    pub fn url(&self) -> String {
        format!("{}:{}{}", self.base, self.port, SCORE_PATH)
    }
}

impl std::fmt::Debug for ProsodyService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProsodyService")
            .field("url", &self.url())
            .finish()
    }
}

impl ProsodyPort for ProsodyService {
    fn measure(&self, pcm: &[i16], sample_rate: u32) -> Result<MeasuredProsody, VoiceError> {
        if !self.is_configured() {
            return Err(VoiceError::NotConfigured);
        }
        let wav = wav_bytes(pcm, sample_rate);
        let response = self
            .agent
            .post(&self.url())
            .header("Content-Type", "audio/wav")
            .send(&wav)
            .map_err(map_ureq_error)?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(VoiceError::Http(status));
        }
        let text = response
            .into_body()
            .read_to_string()
            .map_err(map_ureq_error)?;
        parse_tracks(&text)
    }
}

/// The HTTP client: a blocking one, with the timeout on every call.
fn build_agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .timeout_per_call(Some(timeout))
        .max_idle_connections_per_host(4)
        .build()
        .into()
}

/// What the sidecar answers with.
///
/// Every field is optional: a sidecar that measures only the tempo is a
/// sidecar that answers with the tempo alone.
#[derive(Clone, Debug, Default, Deserialize)]
struct Tracks {
    /// The pitch track: the frame and the pitch, in Q16 hertz.
    #[serde(default)]
    pitch_hz: Vec<(u32, u32)>,
    /// The energy track, in Q16.
    #[serde(default)]
    energy: Vec<u32>,
    /// The tempo, in beats per minute.
    #[serde(default)]
    tempo_bpm: u16,
    /// The pauses: the millisecond it starts and the millisecond it ends.
    #[serde(default)]
    pauses: Vec<(u32, u32)>,
    /// The gesture the service heard.
    #[serde(default)]
    act: Option<String>,
    /// The tone the service heard.
    #[serde(default)]
    affect: Option<String>,
}

/// Read the measured tracks out of the sidecar's answer.
fn parse_tracks(text: &str) -> Result<MeasuredProsody, VoiceError> {
    let tracks: Tracks = serde_json::from_str(text)
        .map_err(|err| VoiceError::Malformed(format!("the tracks do not parse: {err}")))?;
    Ok(MeasuredProsody {
        pitch_hz: tracks.pitch_hz,
        energy: tracks.energy,
        tempo_bpm: tracks.tempo_bpm,
        pauses: tracks.pauses,
        act: tracks.act.as_deref().and_then(act_of),
        affect: tracks
            .affect
            .as_deref()
            .and_then(affect_of)
            .unwrap_or(Affect::Calm),
    })
}

/// The gesture a name stands for.
fn act_of(name: &str) -> Option<SpeechAct> {
    match name.trim().to_ascii_lowercase().as_str() {
        "command" => Some(SpeechAct::Command),
        "question" => Some(SpeechAct::Question),
        "statement" => Some(SpeechAct::Statement),
        "callout" => Some(SpeechAct::Callout),
        _ => None,
    }
}

/// The tone a name stands for.
fn affect_of(name: &str) -> Option<Affect> {
    match name.trim().to_ascii_lowercase().as_str() {
        "calm" => Some(Affect::Calm),
        "urgent" => Some(Affect::Urgent),
        "uncertain" => Some(Affect::Uncertain),
        _ => None,
    }
}

/// The capture as a WAV file: 16-bit mono, at the rate it was taken.
///
/// The sidecar takes WAV because that is what `communicator` reads; the
/// header is written by hand so the voice crate stays free of a sound
/// library.
pub fn wav_bytes(pcm: &[i16], sample_rate: u32) -> Vec<u8> {
    let data_len = u32::try_from(pcm.len() * 2).unwrap_or(u32::MAX);
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&36u32.saturating_add(data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&sample_rate.saturating_mul(2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in pcm {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
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

    #[test]
    fn the_capture_is_written_as_a_wav_file() {
        let wav = wav_bytes(&[1, -1, 2, -2], 16_000);

        // The RIFF header: the magic, the size, the format and the data.
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[36..40], b"data");
        // The sizes: 36 bytes of header, then two bytes a sample.
        assert_eq!(u32::from_le_bytes(wav[4..8].try_into().unwrap()), 36 + 8);
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 8);
        // And the samples themselves, little-endian.
        assert_eq!(&wav[44..46], &[1, 0]);
        assert_eq!(&wav[46..48], &[0xFF, 0xFF]);
    }

    #[test]
    fn the_tracks_are_read_out_of_the_answer() {
        let measured = parse_tracks(
            r#"{"pitch_hz": [[0, 14417920]], "energy": [65536], "tempo_bpm": 200,
                "pauses": [[300, 450]], "act": "command", "affect": "urgent"}"#,
        )
        .expect("the answer parses");

        assert_eq!(measured.pitch_hz, vec![(0, 14_417_920)]);
        assert_eq!(measured.energy, vec![65_536]);
        assert_eq!(measured.tempo_bpm, 200);
        assert_eq!(measured.pauses, vec![(300, 450)]);
        assert_eq!(measured.act, Some(SpeechAct::Command));
        assert_eq!(measured.affect, Affect::Urgent);
    }

    #[test]
    fn an_answer_that_makes_no_sense_is_refused() {
        assert!(matches!(
            parse_tracks("not json at all"),
            Err(VoiceError::Malformed(_))
        ));
    }

    #[test]
    fn a_sidecar_that_is_not_configured_says_so() {
        let service = ProsodyService::new("http://127.0.0.1", 0);
        assert!(!service.is_configured());
        assert_eq!(service.url(), "http://127.0.0.1:0/score");
        assert!(matches!(
            service.measure(&[0, 1], 16_000),
            Err(VoiceError::NotConfigured)
        ));
    }
}
