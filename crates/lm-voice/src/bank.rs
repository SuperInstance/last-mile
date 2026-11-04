//! The voice bank: the lines that were already rendered, on disk, by
//! their own content.
//!
//! A bark repeats — "hold", "contact east", "fall back" — and a clip
//! that is already made is a clip nobody renders twice. The bark cache
//! (in memory, per round) says so for one round; the **bank** says so
//! for every round this machine has played: the WAV of every line an
//! agent has ever said here, filed under the one thing that identifies
//! it, so a lookup costs a file stat and never a synthesis.
//!
//! The key is the **content** of the ask: the text, the voice, the style
//! and the engine id, hashed with the house FNV-1a 64. Two files hold an
//! entry: `<key>.wav`, the audio as the service answered it, and
//! `<key>.json`, the sidecar that says what it is and what it cost — the
//! duration, the voice, the render latency, the engine. A key whose two
//! files are not both there is not there: a store that was interrupted
//! mid-write leaves no half-entry a lookup could mistake for a clip.
//!
//! The bank never decides anything. It is a cache of the audio for the
//! human's ear (the sim never sees it, and the round's hash never covers
//! it), and it is written ahead of time by the prefetch worker, so a
//! lookup in the game's hot path is a read of a small JSON file — hit,
//! pending or miss, and never a wait.

use std::fmt;
use std::fs;
use std::path::PathBuf;

use lm_core::hash::Fnv64;
use serde::{Deserialize, Serialize};

use crate::skald::{wav_duration_ms, wav_pcm16, Rendered, SpeechLine};
use crate::{TtsClip, VoiceError};

/// The bank's key: the hash of the text, the voice, the style and the
/// engine id.
///
/// The four fields are hashed length-first, in this order, so the same
/// ask is the same key on every machine and a different ask — another
/// voice, another style, or the same words on another engine — is
/// another key, and therefore another clip.
pub fn bank_key(text: &str, voice: &str, style: &str, engine: &str) -> u64 {
    let mut h = Fnv64::new();
    for field in [text, voice, style, engine] {
        h.u32(u32::try_from(field.len()).unwrap_or(u32::MAX));
        h.write(field.as_bytes());
    }
    h.finish()
}

/// The sidecar of a bank entry: what the clip is, and what it cost.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BankMeta {
    /// The line, as it was spoken.
    pub text: String,
    /// The voice the render asked for (the map's own spelling).
    pub voice: String,
    /// The canonical voice id the service was asked for.
    pub canonical_voice: String,
    /// The emotion style.
    pub style: String,
    /// The engine id the clip was rendered on.
    pub engine: String,
    /// The sample rate of the WAV.
    pub sample_rate_hz: u32,
    /// How long the clip lasts, in milliseconds.
    pub duration_ms: u64,
    /// How long the render took, in milliseconds.
    pub latency_ms: u64,
    /// How many bytes of WAV the entry holds.
    pub bytes: u64,
}

/// Why a bank operation could not be done.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BankError {
    /// The filesystem refused: the disk is full, the path is not a
    /// directory, the permissions are wrong.
    Io(String),
}

impl fmt::Display for BankError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BankError::Io(why) => write!(f, "the voice bank: {why}"),
        }
    }
}

impl std::error::Error for BankError {}

impl From<BankError> for VoiceError {
    fn from(err: BankError) -> VoiceError {
        VoiceError::Malformed(err.to_string())
    }
}

/// The bank: the rendered lines of a directory tree.
///
/// The bank is a plain directory (`<root>/<xy>/<key>.{wav,json}` — the
/// first byte of the key names one of 256 shards, so a long-lived bank is
/// never one enormous directory). It holds no locks and no state beyond
/// its root: any number of readers may look up while the prefetch worker
/// stores, because a store lands by rename and a lookup only ever sees
/// files that are whole. It is `Clone` because it *is* its path, and the
/// prefetch worker keeps a copy of its own.
#[derive(Clone)]
pub struct VoiceBank {
    root: PathBuf,
}

impl VoiceBank {
    /// The bank rooted at `root`, created if it is not there yet.
    pub fn open(root: impl Into<PathBuf>) -> Result<VoiceBank, BankError> {
        let root = root.into();
        fs::create_dir_all(&root)
            .map_err(|err| BankError::Io(format!("{}: {err}", root.display())))?;
        Ok(VoiceBank { root })
    }

    /// The bank's root directory.
    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    /// The key of a line under this bank's engine: the content-addressed
    /// id the line is filed by.
    pub fn key(&self, line: &SpeechLine, engine: &str) -> u64 {
        bank_key(&line.text, &line.voice, &line.style, engine)
    }

    /// The sidecar of a key, if the bank holds the whole entry.
    ///
    /// A key whose WAV is there but whose sidecar is not (a store that
    /// was interrupted) is a miss: the entry is re-rendered rather than
    /// believed.
    pub fn lookup(&self, key: u64) -> Option<BankMeta> {
        let sidecar = fs::read(self.sidecar_path(key)).ok()?;
        let meta: BankMeta = serde_json::from_slice(&sidecar).ok()?;
        if !self.wav_path(key).is_file() {
            return None;
        }
        Some(meta)
    }

    /// File a rendered line under its key, with the engine of the ask in
    /// the sidecar.
    ///
    /// Both files are written whole and moved into place by rename, so a
    /// concurrent lookup sees either the old entry or the new one, and
    /// never a half of either. The sidecar lands first: the WAV's
    /// arrival is the entry's arrival.
    pub fn store(
        &self,
        key: u64,
        line: &SpeechLine,
        engine: &str,
        rendered: &Rendered,
    ) -> Result<(), BankError> {
        let meta = BankMeta {
            text: line.text.clone(),
            voice: line.voice.clone(),
            canonical_voice: rendered.voice.clone(),
            style: line.style.clone(),
            engine: engine.to_owned(),
            sample_rate_hz: rendered.sample_rate,
            duration_ms: rendered.duration_ms,
            latency_ms: rendered.latency_ms,
            bytes: rendered.wav.len() as u64,
        };
        self.store_meta(key, meta, &rendered.wav)
    }

    /// Write the two files of an entry: the sidecar, then the WAV, each
    /// through a temporary and a rename.
    fn store_meta(&self, key: u64, meta: BankMeta, wav: &[u8]) -> Result<(), BankError> {
        let dir = self.shard_dir(key);
        fs::create_dir_all(&dir)
            .map_err(|err| BankError::Io(format!("{}: {err}", dir.display())))?;
        let sidecar = self.sidecar_path(key);
        let wav_path = self.wav_path(key);
        write_whole(
            &dir,
            &sidecar,
            serde_json::to_vec_pretty(&meta)
                .map_err(|err| BankError::Io(format!("the sidecar does not encode: {err}")))?
                .as_slice(),
        )?;
        write_whole(&dir, &wav_path, wav)?;
        Ok(())
    }

    /// The WAV of a key, as it was answered.
    pub fn load_wav(&self, key: u64) -> Option<Vec<u8>> {
        fs::read(self.wav_path(key)).ok()
    }

    /// The clip of a key, decoded for the host's player.
    ///
    /// `None` when the entry is not there or its WAV is not the 16-bit
    /// mono the host plays: the caller falls back to the tone rather than
    /// resampling a stranger's file.
    pub fn clip(&self, key: u64) -> Option<TtsClip> {
        let wav = self.load_wav(key)?;
        let (pcm, sample_rate) = wav_pcm16(&wav)?;
        Some(TtsClip {
            duration_ms: u32::try_from(wav_duration_ms(&wav).ok()?).unwrap_or(u32::MAX),
            pcm,
            sample_rate,
        })
    }

    /// How many entries the bank holds, counted by walking its shards.
    ///
    /// This is a report's question, not a lookup's: it walks the tree.
    pub fn len(&self) -> usize {
        let mut n = 0usize;
        let Ok(shards) = fs::read_dir(&self.root) else {
            return 0;
        };
        for shard in shards.flatten() {
            for entry in fs::read_dir(shard.path()).into_iter().flatten().flatten() {
                if entry.path().extension().is_some_and(|x| x == "wav") {
                    n += 1;
                }
            }
        }
        n
    }

    /// Whether the bank holds no entries at all.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The shard directory of a key: `<root>/<xy>`.
    fn shard_dir(&self, key: u64) -> PathBuf {
        self.root.join(format!("{key:02x}"))
    }

    /// Where a key's WAV lives.
    pub fn wav_path(&self, key: u64) -> PathBuf {
        self.shard_dir(key).join(format!("{key:016x}.wav"))
    }

    /// Where a key's sidecar lives.
    pub fn sidecar_path(&self, key: u64) -> PathBuf {
        self.shard_dir(key).join(format!("{key:016x}.json"))
    }
}

/// Write `bytes` to `path` through a temporary in the same directory, so
/// the rename that lands it is atomic and a reader never sees a part.
fn write_whole(
    dir: &std::path::Path,
    path: &std::path::Path,
    bytes: &[u8],
) -> Result<(), BankError> {
    let tmp = dir.join(format!(
        ".{}.tmp",
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    fs::write(&tmp, bytes).map_err(|err| BankError::Io(format!("{}: {err}", tmp.display())))?;
    fs::rename(&tmp, path).map_err(|err| BankError::Io(format!("{}: {err}", path.display())))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skald::SpeechLine;

    /// A fresh bank under a temp directory of this test's own.
    fn bank(tag: &str) -> VoiceBank {
        let dir =
            std::env::temp_dir().join(format!("lm-voice-bank-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        VoiceBank::open(&dir).expect("the bank opens")
    }

    /// A rendered line, as the mock daemon would have answered it.
    fn rendered(wav: Vec<u8>, latency_ms: u64) -> Rendered {
        let duration = wav_duration_ms(&wav).expect("the wav parses");
        Rendered {
            sample_rate: 22_050,
            duration_ms: duration,
            latency_ms,
            voice: "Magpie-Multilingual.EN-US.Mia.Neutral".to_owned(),
            wav,
        }
    }

    #[test]
    fn the_key_covers_every_field_of_the_ask() {
        let base = bank_key("hold", "mia", "Neutral", "engine-a");
        // The same ask is the same key.
        assert_eq!(base, bank_key("hold", "mia", "Neutral", "engine-a"));
        // And every field of the ask is a different clip.
        assert_ne!(base, bank_key("holds", "mia", "Neutral", "engine-a"));
        assert_ne!(base, bank_key("hold", "jason", "Neutral", "engine-a"));
        assert_ne!(base, bank_key("hold", "mia", "Angry", "engine-a"));
        assert_ne!(base, bank_key("hold", "mia", "Neutral", "engine-b"));
    }

    #[test]
    fn a_stored_line_is_found_and_lasts() {
        let bank = bank("store");
        let line = SpeechLine::new("hold the north beacon", "mia", "Neutral");
        let wav = crate::prosody::wav_bytes(&vec![100i16; 22_050], 22_050);
        let key = bank.key(&line, "engine-a");
        assert!(bank.lookup(key).is_none(), "nothing is there yet");

        bank.store(key, &line, "engine-a", &rendered(wav.clone(), 712))
            .expect("the store lands");
        let meta = bank.lookup(key).expect("the entry is there");
        assert_eq!(meta.text, "hold the north beacon");
        assert_eq!(meta.engine, "engine-a");
        assert_eq!(meta.latency_ms, 712);
        assert_eq!(meta.duration_ms, 1000);
        assert_eq!(meta.bytes, wav.len() as u64);

        // The clip decodes for the host's player.
        let clip = bank.clip(key).expect("the clip decodes");
        assert_eq!(clip.sample_rate, 22_050);
        assert_eq!(clip.pcm.len(), 22_050);

        // The bank keeps its word across instances: the entry is a file,
        // not a memory.
        let root = bank.root().to_owned();
        drop(bank);
        let again = VoiceBank::open(&root).expect("the bank reopens");
        assert!(again.lookup(key).is_some(), "the entry survives");
        assert_eq!(again.len(), 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_interrupted_store_is_a_miss_not_a_half_entry() {
        let bank = bank("interrupted");
        let line = SpeechLine::new("hold", "mia", "Neutral");
        let key = bank.key(&line, "engine-a");
        // A sidecar without its WAV — the state a crash between the two
        // renames would leave — is not an entry.
        let meta = BankMeta {
            text: "hold".to_owned(),
            voice: "mia".to_owned(),
            canonical_voice: "Magpie-Multilingual.EN-US.Mia".to_owned(),
            style: "Neutral".to_owned(),
            engine: "engine-a".to_owned(),
            sample_rate_hz: 22_050,
            duration_ms: 300,
            latency_ms: 700,
            bytes: 3,
        };
        fs::create_dir_all(bank.shard_dir(key)).expect("the shard is made");
        fs::write(bank.sidecar_path(key), serde_json::to_vec(&meta).unwrap()).expect("written");
        assert!(bank.lookup(key).is_none(), "a sidecar alone is a miss");
        let _ = fs::remove_dir_all(bank.root());
    }

    #[test]
    fn a_store_leaves_no_temporaries_behind() {
        let bank = bank("tmp");
        let line = SpeechLine::new("hold", "mia", "Neutral");
        let key = bank.key(&line, "engine-a");
        let wav = crate::prosody::wav_bytes(&[1, 2, 3], 22_050);
        bank.store(key, &line, "engine-a", &rendered(wav, 5))
            .expect("the store lands");
        for entry in fs::read_dir(bank.shard_dir(key))
            .expect("the shard is there")
            .flatten()
        {
            let name = entry.file_name().to_string_lossy().into_owned();
            assert!(!name.starts_with('.'), "a temporary was left: {name}");
        }
        let _ = fs::remove_dir_all(bank.root());
    }
}
