//! The intelligibility gate: machine ears before human ears.
//!
//! skald's hardest-learned rule (docs/features/asr-gate.md, research
//! 2026-10-29a→b): audio "works" only when a speech recognizer transcribes
//! what the engine was asked to say. A TTS deployment can return correct
//! durations and clean spectra and still speak fluent, confident **babble**
//! — that exact failure was committed as "MAGPIE SPEAKS" once — so a
//! round-trip through ASR is the gate every "it works" has to pass, and no
//! claim of working audio is made without it.
//!
//! ## Where the ears are on this box (probed 2026-10-29)
//!
//! - the skald daemon (`:8000`) proxies TTS only: its transcriptions path
//!   answers 404;
//! - Riva's own HTTP surface (`:50000`) *has* `/v1/audio/transcriptions`,
//!   but this deployment carries TTS models only — it answers
//!   "Model not found" for every language spelling (`en`, `en-US`,
//!   `en_US`, `English`);
//! - the ears that hear are **faster-whisper** in the venv skald's own
//!   smoke test uses (`/tmp/whisperenv`, tiny.en, CPU int8), which
//!   transcribed a rendered line back at homophone-level divergence.
//!
//! So the gate tries Riva first (a future deployment may carry ASR models
//! and then it is the better ear), and falls through to the whisper venv.
//! When neither path answers, [`Gate::unavailable`] says so plainly: the
//! audio's intelligibility is **not** verified, and nothing in this crate
//! or the `lm voice` report claims otherwise.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::voicemap::AsrConfig;
use crate::VoiceError;

/// The path Riva's HTTP surface answers transcriptions on.
pub const TRANSCRIPTIONS_PATH: &str = "/v1/audio/transcriptions";

/// How long one transcription may take, end to end.
const TRANSCRIBE_TIMEOUT: Duration = Duration::from_secs(300);

/// The script the whisper venv runs: a WAV in, one JSON line out.
///
/// It is skald's own smoke test's recipe (`daemon/server/test_smoke.py`):
/// tiny.en on the CPU at int8 — the floor skald's gate calls "enough to
/// catch babble" — reading the WAV through `wave` and `numpy` exactly as
/// the smoke test does.
const WHISPER_SCRIPT: &str = r#"
import json, sys, wave
import numpy as np
from faster_whisper import WhisperModel
w = wave.open(sys.argv[1])
audio = np.frombuffer(w.readframes(w.getnframes()), dtype=np.int16).astype(np.float32) / 32768.0
w.close()
m = WhisperModel("tiny.en", device="cpu", compute_type="int8")
segs, _ = m.transcribe(audio)
print(json.dumps({"text": " ".join(s.text.strip() for s in segs)}))
"#;

/// One heard line: what was asked for, what came back, and how closely
/// the two agree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heard {
    /// The line the engine was asked to say.
    pub expected: String,
    /// What the recognizer heard.
    pub heard: String,
    /// The word accuracy, in per-mille of the expected words (1000 is
    /// every word, 0 is nothing).
    pub accuracy_permille: u32,
}

/// The word accuracy of a transcript: how many of the expected words
/// survived the round trip, in per-mille.
///
/// The words are compared as the recognizer sees them — lower-cased, the
/// punctuation stripped — by word-level edit distance, so a homophone
/// ("hold" heard as "hope") costs exactly one word and a hallucination
/// costs the line.
pub fn word_accuracy(expected: &str, heard: &str) -> u32 {
    let want = words(expected);
    let got = words(heard);
    if want.is_empty() && got.is_empty() {
        return 1000;
    }
    let distance = edit_distance(&want, &got);
    let longest = want.len().max(got.len()) as u32;
    ((longest.saturating_sub(distance as u32)) * 1000) / longest
}

/// The words of a line as the recognizer sees them: lower-cased, split on
/// whitespace, the punctuation stripped, the empties dropped.
fn words(line: &str) -> Vec<String> {
    line.to_lowercase()
        .split_whitespace()
        .map(|word| {
            word.trim_matches(|c: char| !c.is_ascii_alphanumeric())
                .to_owned()
        })
        .filter(|word| !word.is_empty())
        .collect()
}

/// The edit distance of two word lists, the classic way.
fn edit_distance(a: &[String], b: &[String]) -> usize {
    let mut prior: Vec<usize> = (0..=b.len()).collect();
    let mut row = vec![0usize; b.len() + 1];
    for (i, wa) in a.iter().enumerate() {
        row[0] = i + 1;
        for (j, wb) in b.iter().enumerate() {
            let sub = prior[j] + usize::from(wa != wb);
            row[j + 1] = (row[j] + 1).min(prior[j + 1] + 1).min(sub);
        }
        std::mem::swap(&mut prior, &mut row);
    }
    prior[b.len()]
}

/// The gate's ears, tried in order until one of them hears.
pub struct Gate {
    config: AsrConfig,
}

/// What the gate found: the ears that answered, or the plain statement
/// that none did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GateOutcome {
    /// Every line was heard by this ear (named for the report).
    Heard(String, Vec<Heard>),
    /// No ear answered. The reasons are kept: the report says them out
    /// loud, and the audio's intelligibility stays unverified.
    Unavailable(Vec<String>),
}

impl Gate {
    /// The gate over the voice map's `[asr]` section.
    pub fn new(config: AsrConfig) -> Gate {
        Gate { config }
    }

    /// Transcribe one WAV: Riva's transcriptions first, the whisper venv
    /// second — the ear that answers a probe is the ear the gate uses.
    pub fn transcribe(&self, wav_path: &Path) -> Result<(String, String), Vec<String>> {
        let mut reasons = Vec::new();
        match self.transcribe_riva(wav_path) {
            Ok(text) => return Ok(("riva-http".to_owned(), text)),
            Err(err) => reasons.push(format!("riva {:?}: {err}", self.riva_url())),
        }
        match self.transcribe_whisper(wav_path) {
            Ok(text) => return Ok(("whisper-tiny.en".to_owned(), text)),
            Err(err) => reasons.push(format!("whisper {:?}: {err}", self.config.whisper_python)),
        }
        Err(reasons)
    }

    /// Hear a sample of lines and judge them.
    ///
    /// Every line that neither ear could hear ends the gate as
    /// [`GateOutcome::Unavailable`] with the reasons: a gate that cannot
    /// run is not a gate that passed.
    pub fn hear(&self, sample: &[(PathBuf, String)]) -> GateOutcome {
        let mut heard = Vec::new();
        let mut ear = String::new();
        for (wav, expected) in sample {
            match self.transcribe(wav) {
                Ok((line_ear, text)) => {
                    if ear.is_empty() {
                        ear = line_ear;
                    }
                    heard.push(Heard {
                        expected: expected.clone(),
                        accuracy_permille: word_accuracy(expected, &text),
                        heard: text,
                    });
                }
                Err(reasons) => return GateOutcome::Unavailable(reasons),
            }
        }
        GateOutcome::Heard(ear, heard)
    }

    /// Riva's transcriptions URL.
    fn riva_url(&self) -> String {
        format!(
            "{}:{}{}",
            self.config.riva_base, self.config.riva_port, TRANSCRIPTIONS_PATH
        )
    }

    /// Ask Riva's HTTP surface what it hears in a WAV.
    ///
    /// This deployment carries TTS models only, so this usually answers
    /// HTTP 400 "Model not found for language …" — which is precisely the
    /// probe: the day Riva carries ASR models, this ear is the better one
    /// and the gate takes it without a code change.
    fn transcribe_riva(&self, wav_path: &Path) -> Result<String, VoiceError> {
        let boundary = "lm-voice-gate-7381";
        let wav = std::fs::read(wav_path)
            .map_err(|err| VoiceError::Malformed(format!("{}: {err}", wav_path.display())))?;
        let mut body = Vec::new();
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            b"Content-Disposition: form-data; name=\"file\"; filename=\"line.wav\"\r\n\
               Content-Type: audio/wav\r\n\r\n",
        );
        body.extend_from_slice(&wav);
        body.extend_from_slice(format!("\r\n--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            b"Content-Disposition: form-data; name=\"language\"\r\n\r\nen-US\r\n",
        );
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(120)))
            .proxy(None)
            .build()
            .into();
        let response = agent
            .post(&self.riva_url())
            .header(
                "Content-Type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .send(&body)
            .map_err(|err| match err {
                ureq::Error::StatusCode(code) => VoiceError::Http(code),
                ureq::Error::Timeout(_) => VoiceError::Timeout,
                other => VoiceError::Malformed(format!("{other}")),
            })?;
        let status = response.status().as_u16();
        let mut text = String::new();
        response
            .into_body()
            .into_reader()
            .take(1024 * 1024)
            .read_to_string(&mut text)
            .map_err(|err| VoiceError::Malformed(format!("the answer did not arrive: {err}")))?;
        if !(200..300).contains(&status) {
            return Err(VoiceError::Http(status));
        }
        #[derive(Deserialize)]
        struct Answer {
            #[serde(default)]
            text: String,
        }
        let answer: Answer = serde_json::from_str(&text)
            .map_err(|err| VoiceError::Malformed(format!("the answer is not JSON: {err}")))?;
        Ok(answer.text)
    }

    /// Ask the whisper venv what it hears in a WAV.
    ///
    /// The venv is a subprocess with a deadline: a python that wedges is
    /// killed and named, not waited on.
    fn transcribe_whisper(&self, wav_path: &Path) -> Result<String, VoiceError> {
        let python = PathBuf::from(&self.config.whisper_python);
        if !python.is_file() {
            return Err(VoiceError::NotConfigured);
        }
        let mut child = Command::new(&python)
            .arg("-c")
            .arg(WHISPER_SCRIPT)
            .arg(wav_path)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|err| VoiceError::Malformed(format!("the venv would not run: {err}")))?;
        let deadline = Instant::now() + TRANSCRIBE_TIMEOUT;
        loop {
            match child.try_wait() {
                Ok(Some(_status)) => break,
                Ok(None) => {
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(VoiceError::Timeout);
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(err) => {
                    return Err(VoiceError::Malformed(format!("the venv broke: {err}")));
                }
            }
        }
        let output = child.wait_with_output().map_err(|err| {
            VoiceError::Malformed(format!("the venv's answer did not arrive: {err}"))
        })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(VoiceError::Malformed(format!(
                "the venv failed: {}",
                stderr.trim()
            )));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        #[derive(Deserialize)]
        struct Answer {
            text: String,
        }
        let answer: Answer = serde_json::from_str(stdout.trim()).map_err(|err| {
            VoiceError::Malformed(format!("the venv's answer is not JSON: {err}"))
        })?;
        Ok(answer.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_perfect_round_trip_is_every_word() {
        assert_eq!(word_accuracy("hold the line", "hold the line"), 1000);
        assert_eq!(word_accuracy("", ""), 1000);
    }

    #[test]
    fn a_homophone_costs_one_word() {
        // skald's own verification heard exactly this class of drift.
        assert_eq!(
            word_accuracy("hold the north beacon", "hope the north beacon"),
            750
        );
    }

    #[test]
    fn punctuation_and_case_are_not_words() {
        assert_eq!(
            word_accuracy("Hold the north beacon.", "hold the North Beacon"),
            1000
        );
    }

    #[test]
    fn babble_is_nothing() {
        // Six words asked, four "the"s heard back: five edits, one word
        // of agreement — the score says babble, whatever it sounds like.
        assert_eq!(
            word_accuracy("fall back to the rally point", "the the the the"),
            166
        );
        assert_eq!(word_accuracy("fall back", ""), 0);
    }

    #[test]
    fn an_insertion_costs_like_a_deletion() {
        assert_eq!(word_accuracy("hold", "hold the line"), 333);
        assert_eq!(word_accuracy("hold the line", "hold"), 333);
    }

    /// The live gate: render a line on the real daemon and hear it back.
    ///
    /// It is opt-in like every live test, and it is the one test whose
    /// whole point is the claim "the audio works": if the ears are not on
    /// this box, the test fails loudly rather than passing quietly.
    #[test]
    #[ignore = "the live gate needs the skald daemon (:8000) and the whisper venv"]
    fn live_skald_round_trip_is_intelligible() {
        use crate::skald::{SpeechLine, VoiceClient};
        use crate::voicemap::VoiceMap;

        let map = VoiceMap::shipped();
        let client = VoiceClient::new(&map.skald.base, map.skald.port, map.skald.sample_rate_hz);
        let line = SpeechLine::new("Hold the north beacon. Contact east.", "mia", "Neutral");
        let rendered = client
            .render(&line, Instant::now() + Duration::from_secs(10))
            .expect("the live daemon speaks");

        let dir = std::env::temp_dir().join(format!("lm-voice-gate-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the scratch dir is made");
        let wav = dir.join("line.wav");
        std::fs::write(&wav, &rendered.wav).expect("the wav is written");

        let gate = Gate::new(map.asr.clone());
        let (ear, heard) = gate
            .transcribe(&wav)
            .expect("an ear on this box hears — without one, do not claim the audio works");
        let accuracy = word_accuracy(&line.text, &heard);
        assert!(
            accuracy >= 700,
            "the {ear} heard {heard:?} for {:?}: {accuracy} per-mille",
            line.text
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
