use std::collections::BTreeMap;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AffectBucket {
    Calm,
    Urgent,
    Warm,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BarkKey {
    pub text: String,
    pub voice: String,
    pub affect: AffectBucket,
}

impl BarkKey {
    pub fn new(text: &str, voice: &str, affect: AffectBucket) -> BarkKey {
        BarkKey {
            text: normalize(text),
            voice: voice.to_string(),
            affect,
        }
    }
}

pub struct BarkCache {
    capacity: usize,
    cache: BTreeMap<BarkKey, (Vec<u8>, u64)>,
    counter: u64,
    hits: u64,
    misses: u64,
}

impl BarkCache {
    pub fn new(capacity: usize) -> BarkCache {
        BarkCache {
            capacity,
            cache: BTreeMap::new(),
            counter: 0,
            hits: 0,
            misses: 0,
        }
    }

    pub fn get(&mut self, key: &BarkKey) -> Option<&[u8]> {
        if let Some((audio, last_used)) = self.cache.get_mut(key) {
            self.counter += 1;
            *last_used = self.counter;
            self.hits += 1;
            Some(audio)
        } else {
            self.misses += 1;
            None
        }
    }

    pub fn put(&mut self, key: BarkKey, audio: Vec<u8>) {
        if self.capacity == 0 {
            return;
        }

        self.counter += 1;
        let last_used = self.counter;

        // Check if we need to evict
        if self.cache.len() >= self.capacity {
            // Find the least recently used entry
            let lru_key = self
                .cache
                .iter()
                .min_by_key(|(_, (_, last_used))| *last_used)
                .map(|(k, _)| k.clone())
                .unwrap();
            self.cache.remove(&lru_key);
        }

        self.cache.insert(key, (audio, last_used));
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    pub fn hits(&self) -> u64 {
        self.hits
    }

    pub fn misses(&self) -> u64 {
        self.misses
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_collapses_case_space_punctuation() {
        assert_eq!(normalize("  Contact   EAST! "), "contact east");
        assert_eq!(normalize("Hello   World."), "hello world");
        assert_eq!(normalize("Test???"), "test");
        assert_eq!(normalize("  Mixed   Case   !  "), "mixed case");
    }

    #[test]
    fn same_line_different_voice_is_a_different_bark() {
        let key1 = BarkKey::new("test", "voice1", AffectBucket::Calm);
        let key2 = BarkKey::new("test", "voice2", AffectBucket::Calm);
        assert_ne!(key1, key2);
    }

    #[test]
    fn hit_and_miss_are_counted() {
        let mut cache = BarkCache::new(2);
        let key = BarkKey::new("test", "voice", AffectBucket::Calm);
        assert_eq!(cache.hits(), 0);
        assert_eq!(cache.misses(), 0);

        // Miss
        assert_eq!(cache.get(&key), None);
        assert_eq!(cache.hits(), 0);
        assert_eq!(cache.misses(), 1);

        // Put some data
        cache.put(key.clone(), vec![1, 2, 3]);

        // Hit
        assert_eq!(cache.get(&key), Some(&[1, 2, 3][..]));
        assert_eq!(cache.hits(), 1);
        assert_eq!(cache.misses(), 1);
    }

    #[test]
    fn lru_evicts_least_recently_used() {
        let mut cache = BarkCache::new(2);

        let key_a = BarkKey::new("a", "voice", AffectBucket::Calm);
        let key_b = BarkKey::new("b", "voice", AffectBucket::Calm);
        let key_c = BarkKey::new("c", "voice", AffectBucket::Calm);

        cache.put(key_a.clone(), vec![1]);
        cache.put(key_b.clone(), vec![2]);

        // Access key_a to make it most recently used
        cache.get(&key_a);

        // Add a new item - should evict key_b (least recently used)
        cache.put(key_c.clone(), vec![3]);

        // key_a should still be there, key_b should be gone, key_c should be there
        assert_eq!(cache.get(&key_a), Some(&[1][..]));
        assert_eq!(cache.get(&key_b), None);
        assert_eq!(cache.get(&key_c), Some(&[3][..]));
    }

    #[test]
    fn zero_capacity_never_stores() {
        let mut cache = BarkCache::new(0);
        let key = BarkKey::new("test", "voice", AffectBucket::Calm);

        cache.put(key.clone(), vec![1, 2, 3]);
        assert_eq!(cache.get(&key), None);
        assert_eq!(cache.len(), 0);
    }
}
