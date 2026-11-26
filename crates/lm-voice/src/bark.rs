//! The bark cache: the same line, in the same voice, with the same affect
//! is never spoken twice.
//!
//! A bark is a short line an agent says over and over -- "hold", "contact
//! east", "fall back". The lines repeat, so the clips repeat, and a clip
//! that is already made is a clip nobody pays for twice.
//!
//! The key of a bark is the **normalized** line, the **voice** and the
//! **affect bucket**:
//!
//! - the text is normalized (lower-cased, trimmed, the internal
//!   whitespace collapsed, the trailing `.!?` stripped), so "Hold." and
//!   "hold" are one line and one clip;
//! - the voice is part of the key, because another voice is another clip;
//! - the affect is part of the key, because the bark's *tone* is the
//!   point: the same words calm and the same words pressed are two
//!   different sounds, and a cache that forgot that would speak every
//!   order in the same flat voice.
//!
//! The cache evicts the **least recently used** bark when it is full, so
//! the lines that are being said now are the ones that stay.

use crate::{Tts, TtsClip, TtsRequest, VoiceError};
use lm_core::hash::{Fnv64, StateHash};
use lm_core::utterance::Affect;
use std::collections::BTreeMap;

/// How many barks the cache holds by default.
pub const DEFAULT_BARK_CAP: usize = 128;

/// The normalized text of a line: lower-cased, trimmed, the internal
/// whitespace collapsed and the trailing `.!?` stripped.
///
/// The line's *meaning* is in the score, not in the string; this is only
/// the shape of the text that identifies a clip.
pub fn normalize(text: &str) -> String {
    text.trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join(" ")
        .trim_end_matches(['.', '!', '?'])
        .trim_end()
        .to_string()
}

/// The cache key of a line: the normalized text, the voice and the affect
/// bucket.
///
/// The key is a canonical hash (FNV-1a 64 over explicit little-endian
/// bytes), so it is the same on every machine and never depends on the
/// iteration order of a hash map.
pub fn bark_key(text: &str, voice: &str, affect: Affect) -> u64 {
    let normalized = normalize(text);
    let mut h = Fnv64::new();
    h.u32(u32::try_from(normalized.len()).unwrap_or(u32::MAX));
    h.write(normalized.as_bytes());
    h.u32(u32::try_from(voice.len()).unwrap_or(u32::MAX));
    h.write(voice.as_bytes());
    // The affect is a bucket, and its canonical hash is the bucket's
    // identity: the same declaration order as the score's.
    h.u64(affect.state_hash());
    h.finish()
}

/// One cached clip, and when it was last used.
struct Entry {
    /// The clip itself.
    clip: TtsClip,
    /// The clock reading of its last use: the least recent one is evicted.
    used: u64,
}

/// The bark cache: the clips of the lines already spoken.
pub struct BarkCache {
    /// How many barks the cache holds.
    cap: usize,
    /// The clips, by key.
    entries: BTreeMap<u64, Entry>,
    /// The clock that says which bark was used last.
    clock: u64,
    /// How many requests the cache answered from itself.
    hits: u64,
    /// How many requests the cache had to synthesize.
    misses: u64,
}

impl BarkCache {
    /// A cache that holds `cap` barks.
    pub fn new(cap: usize) -> BarkCache {
        BarkCache {
            cap,
            entries: BTreeMap::new(),
            clock: 0,
            hits: 0,
            misses: 0,
        }
    }

    /// How many barks the cache holds.
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// The clip for a key, if the cache has it.
    ///
    /// A hit marks the bark as recently used, so the least recently used
    /// one is the next to go.
    pub fn get(&mut self, key: u64) -> Option<&TtsClip> {
        self.clock += 1;
        let clock = self.clock;
        match self.entries.get_mut(&key) {
            Some(entry) => {
                entry.used = clock;
                self.hits += 1;
                Some(&entry.clip)
            }
            None => {
                self.misses += 1;
                None
            }
        }
    }

    /// Put a clip in the cache.
    ///
    /// A cache that is full evicts its least recently used bark to make
    /// room; a cache with no room at all (a cap of zero) keeps nothing.
    pub fn insert(&mut self, key: u64, clip: TtsClip) {
        if self.cap == 0 {
            return;
        }
        self.clock += 1;
        let clock = self.clock;
        if !self.entries.contains_key(&key) && self.entries.len() >= self.cap {
            if let Some(lru) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| *key)
            {
                self.entries.remove(&lru);
            }
        }
        self.entries.insert(key, Entry { clip, used: clock });
    }

    /// The clip for a request: the cached one when there is one, else a
    /// new one from the TTS.
    ///
    /// This is the whole point of the cache: the same line, in the same
    /// voice, with the same affect never reaches the TTS twice.
    pub fn synthesize(&mut self, tts: &dyn Tts, req: &TtsRequest) -> Result<TtsClip, VoiceError> {
        let key = bark_key(&req.text, &req.voice, req.affect);
        if let Some(clip) = self.get(key) {
            return Ok(clip.clone());
        }
        let clip = tts.synthesize(req)?;
        self.insert(key, clip.clone());
        Ok(clip)
    }

    /// How many barks the cache holds.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// How many requests the cache answered from itself.
    pub fn hits(&self) -> u64 {
        self.hits
    }

    /// How many requests the cache had to synthesize.
    pub fn misses(&self) -> u64 {
        self.misses
    }
}

impl Default for BarkCache {
    /// The cache of the default size.
    fn default() -> Self {
        BarkCache::new(DEFAULT_BARK_CAP)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::CountingTts;
    use crate::{TtsClip, TtsRequest};

    /// A request for a line, in a voice, with an affect.
    fn request(text: &str, voice: &str, affect: Affect) -> TtsRequest {
        TtsRequest::new(text, voice, affect)
    }

    /// A clip that says which one it is.
    fn clip(marker: i16) -> TtsClip {
        TtsClip {
            pcm: vec![marker],
            sample_rate: 16_000,
            duration_ms: 300,
        }
    }

    #[test]
    fn the_normalized_text_is_the_line_without_its_case_and_its_dust() {
        assert_eq!(normalize("  Contact   EAST! "), "contact east");
        assert_eq!(normalize("Hello   World."), "hello world");
        assert_eq!(normalize("Test???"), "test");
        assert_eq!(normalize("  HOLD  "), "hold");
    }

    #[test]
    fn the_bark_key_covers_the_affect() {
        let calm = bark_key("Hold.", "default", Affect::Calm);
        let urgent = bark_key("Hold.", "default", Affect::Urgent);
        let uncertain = bark_key("Hold.", "default", Affect::Uncertain);

        // The tone is the point: the same words calm and the same words
        // pressed are two different barks.
        assert_ne!(calm, urgent);
        assert_ne!(calm, uncertain);
        assert_ne!(urgent, uncertain);

        // The same text and the same affect are the same bark.
        assert_eq!(calm, bark_key("Hold.", "default", Affect::Calm));
        assert_eq!(urgent, bark_key("Hold.", "default", Affect::Urgent));

        // And the voice is part of the key too: another voice is another
        // bark.
        assert_ne!(calm, bark_key("Hold.", "other", Affect::Calm));
    }

    #[test]
    fn the_bark_key_normalizes_the_text() {
        let key = bark_key("Hold.", "default", Affect::Calm);
        assert_eq!(key, bark_key("hold", "default", Affect::Calm));
        assert_eq!(key, bark_key("  HOLD  ", "default", Affect::Calm));
        // A different line is a different bark.
        assert_ne!(key, bark_key("Hold the north", "default", Affect::Calm));
    }

    #[test]
    fn the_bark_cache_hits_on_the_second_call() {
        let mut cache = BarkCache::new(8);
        let tts = CountingTts::default();
        let req = request("hold the north", "default", Affect::Urgent);

        let first = cache.synthesize(&tts, &req).expect("the stub answers");
        let second = cache.synthesize(&tts, &req).expect("the stub answers");

        assert_eq!(tts.calls(), 1, "the line is spoken once, not twice");
        assert_eq!(first, second, "the same clip comes back");
        assert_eq!(cache.hits(), 1);
        assert_eq!(cache.misses(), 1);
    }

    #[test]
    fn the_bark_cache_evicts_at_the_cap() {
        let mut cache = BarkCache::new(2);
        let a = bark_key("a", "default", Affect::Calm);
        let b = bark_key("b", "default", Affect::Calm);
        let c = bark_key("c", "default", Affect::Calm);

        cache.insert(a, clip(1));
        cache.insert(b, clip(2));
        assert_eq!(cache.len(), 2);

        // `a` is used, so `b` is now the least recently used bark.
        assert!(cache.get(a).is_some());
        cache.insert(c, clip(3));

        assert_eq!(cache.len(), 2, "the cache holds its cap, no more");
        assert!(cache.get(a).is_some(), "the recently used bark stays");
        assert!(
            cache.get(b).is_none(),
            "the least recently used bark is gone"
        );
        assert!(cache.get(c).is_some(), "the new bark is here");
    }

    #[test]
    fn the_default_cache_holds_a_hundred_and_twenty_eight_barks() {
        let cache = BarkCache::default();
        assert_eq!(cache.cap(), 128);
        assert!(cache.is_empty());
        assert_eq!(cache.hits(), 0);
        assert_eq!(cache.misses(), 0);
    }

    #[test]
    fn a_cache_with_no_room_keeps_nothing() {
        let mut cache = BarkCache::new(0);
        let key = bark_key("hold", "default", Affect::Calm);
        cache.insert(key, clip(1));
        assert!(cache.get(key).is_none());
        assert!(cache.is_empty());
    }
}
