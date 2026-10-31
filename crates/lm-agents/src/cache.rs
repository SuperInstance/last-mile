//! The answer cache: a question's answer, remembered.
//!
//! The doctrine is the one scrap-spark already runs in production
//! (docs/research/lucineer-ecosystem.md §13): *model call heavy at first,
//! then more and more canned responses*. The first ask of a question is
//! paid for; the next ask of the **same question over the same context**
//! is free.
//!
//! What makes it safe is the key. The key is
//! `SHA-256(question id | version | context digest)`:
//!
//! - the **id** says which question it is, and survives a rewording;
//! - the **version** says which revision of that question, so a reworded
//!   gate is never answered out of an old gate's cache;
//! - the **context digest** says what the question was asked about — the
//!   facts the gate saw, normalized, and the options it was really asked
//!   about. The same question over a different situation is a different
//!   key, so it is asked again (D26: no answer is ever invented).
//!
//! A hit is served only when the **backend** and its **floor** match: an
//! answer one model gave under one confidence floor is not an answer
//! another model would give, and a stricter floor would have refused it.
//!
//! The cache is **off by default** (the `answer_cache` lever in
//! `policy.toml`), it is in memory only, and it never touches the round's
//! hashed state: a cached answer is an answer the model would have given,
//! and the round's own record is unchanged by it.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::{Map, Value};

use crate::question::{cache_key, context_digest, stamp_of, QuestionStamp};
use crate::sysone::{Answer, Answers, Refusal, SysOne};

/// One remembered answer, and the conditions it was given under.
#[derive(Clone, Debug)]
struct Entry {
    /// The gate's answer.
    answer: Answer,
    /// The backend that gave it: a hit is only served to the same backend.
    backend: String,
    /// The confidence floor it was given under, as its bits.
    ///
    /// The bits, not the float: the floor is a threshold, and two floors
    /// that differ in the last bit are two floors.
    floor: u32,
    /// The ask counter when it was stored: the age of the entry.
    stored_at: u64,
}

/// What the cache holds, in numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    /// The answers served from the cache.
    pub hits: u64,
    /// The questions that had to be asked.
    pub misses: u64,
    /// The answers stored.
    pub stores: u64,
    /// The answers held now.
    pub entries: usize,
    /// The mean age of the held answers, in asks.
    pub age_mean: u64,
    /// The age of the oldest held answer, in asks.
    pub age_max: u64,
    /// The age of the last answer served, in asks.
    pub age_last: u64,
}

/// The cache itself.
///
/// The cache is shared: one cache for the round, so an answer one agent
/// paid for is an answer every agent may have. It is behind a lock
/// because the thinker's worker threads ask in parallel.
#[derive(Debug, Default)]
pub struct AnswerCache {
    inner: Mutex<Inner>,
}

/// The cache's state.
#[derive(Debug, Default)]
struct Inner {
    entries: BTreeMap<String, Entry>,
    hits: u64,
    misses: u64,
    stores: u64,
    /// The ask counter: the clock the ages are counted in.
    clock: u64,
    /// The age of the last answer served, in asks.
    age_last: u64,
}

/// An answer the cache served.
#[derive(Clone, Debug)]
pub struct CachedAnswer {
    /// The gate's answer.
    pub answer: Answer,
    /// How many asks old the answer was.
    pub age_asks: u64,
}

impl AnswerCache {
    /// An empty cache.
    pub fn new() -> AnswerCache {
        AnswerCache::default()
    }

    /// Count one ask: the clock the ages are counted in moves on.
    pub fn tick(&self) {
        let mut inner = self.lock();
        inner.clock = inner.clock.saturating_add(1);
    }

    /// The answer for this key, if the cache holds one that may be served.
    ///
    /// The answer is served only when the backend and the floor are the
    /// ones it was given under: another model, or another floor, is a
    /// different question as far as the cache is concerned.
    pub fn get(&self, key: &str, backend: &str, floor: f32) -> Option<CachedAnswer> {
        let mut inner = self.lock();
        let clock = inner.clock;
        let entry = inner.entries.get(key).cloned()?;
        if entry.backend != backend || entry.floor != floor.to_bits() {
            // A different model, or a different floor: the answer was not
            // given under these conditions, so it is not served.
            return None;
        }
        let age_asks = clock.saturating_sub(entry.stored_at);
        inner.hits = inner.hits.saturating_add(1);
        inner.age_last = age_asks;
        Some(CachedAnswer {
            answer: entry.answer.clone(),
            age_asks,
        })
    }

    /// Remember an answer.
    pub fn put(&self, key: &str, answer: Answer, backend: &str, floor: f32) {
        let mut inner = self.lock();
        let clock = inner.clock;
        inner.stores = inner.stores.saturating_add(1);
        inner.entries.insert(
            key.to_string(),
            Entry {
                answer,
                backend: backend.to_string(),
                floor: floor.to_bits(),
                stored_at: clock,
            },
        );
    }

    /// Count a question that had to be asked.
    pub fn count_miss(&self) {
        let mut inner = self.lock();
        inner.misses = inner.misses.saturating_add(1);
    }

    /// What the cache holds, in numbers.
    pub fn stats(&self) -> CacheStats {
        let inner = self.lock();
        let clock = inner.clock;
        let mut total_age = 0u64;
        let mut age_max = 0u64;
        for entry in inner.entries.values() {
            let age = clock.saturating_sub(entry.stored_at);
            total_age = total_age.saturating_add(age);
            age_max = age_max.max(age);
        }
        let entries = inner.entries.len();
        let age_mean = if entries == 0 {
            0
        } else {
            total_age / entries as u64
        };
        CacheStats {
            hits: inner.hits,
            misses: inner.misses,
            stores: inner.stores,
            entries,
            age_mean,
            age_max,
            age_last: inner.age_last,
        }
    }

    /// Forget everything: the cache is empty again.
    pub fn clear(&self) {
        let mut inner = self.lock();
        inner.entries.clear();
    }

    /// The lock, poisoned only by a panic in a holder: the cache is not
    /// state that a panic may corrupt, so the poison is taken back.
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

/// The pack, split into the questions the cache holds and the ones it does
/// not.
struct Split {
    /// The answers the cache held.
    held: BTreeMap<String, Answer>,
    /// The names of the held questions, with the cache's mark on them.
    stamps: BTreeMap<String, QuestionStamp>,
    /// The questions the model has to be asked.
    ask: Map<String, Value>,
    /// The key every question was asked with.
    keys: BTreeMap<String, String>,
}

/// A backend with the answer cache in front of it.
///
/// The cache is a wrapper and not a change to the backends: a backend is
/// still `Box<dyn SysOne + Send + Sync>`, and the cache can be put in
/// front of any of them.
pub struct CachedBackend {
    /// The backend that answers the questions the cache does not hold.
    inner: Box<dyn SysOne + Send + Sync>,
    /// The cache, shared with the round's other backends.
    cache: Arc<AnswerCache>,
}

impl CachedBackend {
    /// The cache in front of `inner`.
    pub fn new(inner: Box<dyn SysOne + Send + Sync>, cache: Arc<AnswerCache>) -> CachedBackend {
        CachedBackend { inner, cache }
    }

    /// The cache behind this backend.
    pub fn cache(&self) -> &Arc<AnswerCache> {
        &self.cache
    }
}

impl CachedBackend {
    /// The pack, split into the questions the cache holds and the ones it
    /// does not.
    ///
    /// The held answers, the questions the model has to be asked, the
    /// stamps of the held questions and the key every question was asked
    /// with.
    fn split(&self, state: &str, questions: &Value) -> Split {
        let backend = self.inner.name();
        let floor = self.inner.confidence_floor();
        let mut held: BTreeMap<String, Answer> = BTreeMap::new();
        let mut stamps: BTreeMap<String, QuestionStamp> = BTreeMap::new();
        let mut ask: Map<String, Value> = Map::new();
        let mut keys: BTreeMap<String, String> = BTreeMap::new();
        let Some(gates) = questions.as_object() else {
            return Split {
                held,
                stamps,
                ask,
                keys,
            };
        };
        for (slot, question) in gates {
            let stamp = stamp_of(slot, question);
            let context = context_digest(state, question);
            let key = cache_key(&stamp.id, &stamp.version, &context);
            keys.insert(slot.clone(), key.clone());
            match self.cache.get(&key, backend, floor) {
                Some(hit) => {
                    let mut stamp = stamp;
                    stamp.cached = true;
                    stamp.age_asks = hit.age_asks;
                    held.insert(slot.clone(), hit.answer);
                    stamps.insert(slot.clone(), stamp);
                }
                None => {
                    self.cache.count_miss();
                    ask.insert(slot.clone(), question.clone());
                }
            }
        }
        Split {
            held,
            stamps,
            ask,
            keys,
        }
    }

    /// Remember the fresh answers, under the key they were asked with.
    ///
    /// An answer the model was not sure about is not remembered: it made
    /// no decision, and the next ask of that question is a question the
    /// model has to answer again.
    fn store(&self, fresh: &Answers, keys: &BTreeMap<String, String>) {
        let backend = self.inner.name();
        let floor = self.inner.confidence_floor();
        for (slot, key) in keys {
            if let Some(answer) = fresh.per_gate.get(slot) {
                self.cache.put(key, answer.clone(), backend, floor);
            }
        }
    }

    /// The answer is the fresh one and the held one, together.
    ///
    /// The pack is the authority on the questions' names: the answer says
    /// which question it answers, whether that answer came out of the
    /// cache, and how old it is.
    fn merge(
        &self,
        fresh: Answers,
        held: BTreeMap<String, Answer>,
        stamps: BTreeMap<String, QuestionStamp>,
        pack: &Value,
    ) -> Answers {
        let mut per_gate = fresh.per_gate.clone();
        for (slot, answer) in held {
            per_gate.insert(slot, answer);
        }
        let mut questions = BTreeMap::new();
        if let Some(gates) = pack.as_object() {
            for (slot, question) in gates {
                questions.insert(slot.clone(), stamp_of(slot, question));
            }
        }
        for (slot, stamp) in stamps {
            questions.insert(slot, stamp);
        }
        Answers {
            per_gate,
            escalated: fresh.escalated,
            unsure: fresh.unsure,
            model: fresh.model,
            input_tokens: fresh.input_tokens,
            latency_ms: fresh.latency_ms,
            questions,
        }
    }

    /// The answer when every question was held: the model is not asked at
    /// all, and the round pays nothing for it.
    fn all_held(
        &self,
        held: BTreeMap<String, Answer>,
        stamps: BTreeMap<String, QuestionStamp>,
    ) -> Answers {
        Answers {
            per_gate: held,
            escalated: BTreeMap::new(),
            unsure: BTreeMap::new(),
            model: self.inner.name().to_string(),
            input_tokens: 0,
            latency_ms: 0,
            questions: stamps,
        }
    }
}

impl SysOne for CachedBackend {
    fn ask(&self, state: &str, questions: &Value, deadline: Instant) -> Result<Answers, Refusal> {
        self.cache.tick();
        let Split {
            held,
            stamps,
            ask,
            keys,
        } = self.split(state, questions);
        if ask.is_empty() {
            return Ok(self.all_held(held, stamps));
        }
        // The rest is asked, and only that: a pack of one question is a
        // pack that fits, and the model is asked about what it does not
        // know.
        let fresh = self.inner.ask(state, &Value::Object(ask), deadline)?;
        self.store(&fresh, &keys);
        Ok(self.merge(fresh, held, stamps, questions))
    }

    fn ask_with_escalation(
        &self,
        state: &str,
        questions: &Value,
        deadline: Instant,
        escalation_deadline: Instant,
    ) -> Result<Answers, Refusal> {
        self.cache.tick();
        let Split {
            held,
            stamps,
            ask,
            keys,
        } = self.split(state, questions);
        if ask.is_empty() {
            return Ok(self.all_held(held, stamps));
        }
        let fresh = self.inner.ask_with_escalation(
            state,
            &Value::Object(ask),
            deadline,
            escalation_deadline,
        )?;
        self.store(&fresh, &keys);
        Ok(self.merge(fresh, held, stamps, questions))
    }

    fn name(&self) -> &str {
        self.inner.name()
    }

    fn confidence_floor(&self) -> f32 {
        self.inner.confidence_floor()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A backend that counts its calls and answers every gate.
    struct Counting {
        calls: Mutex<usize>,
        answer: Answer,
        floor: f32,
        name: String,
    }

    impl Counting {
        fn new(floor: f32) -> Counting {
            Counting {
                calls: Mutex::new(0),
                answer: Answer::Choice {
                    probs: BTreeMap::from([("opt0".to_string(), 0.9)]),
                    confidence: 0.9,
                },
                floor,
                name: "counting".to_string(),
            }
        }

        fn calls(&self) -> usize {
            *self.calls.lock().unwrap()
        }
    }

    impl SysOne for Counting {
        fn ask(
            &self,
            _state: &str,
            questions: &Value,
            _deadline: Instant,
        ) -> Result<Answers, Refusal> {
            *self.calls.lock().unwrap() += 1;
            Ok(Answers {
                per_gate: questions
                    .as_object()
                    .map(|gates| {
                        gates
                            .keys()
                            .map(|slot| (slot.clone(), self.answer.clone()))
                            .collect()
                    })
                    .unwrap_or_default(),
                escalated: BTreeMap::new(),
                unsure: BTreeMap::new(),
                model: self.name.clone(),
                input_tokens: 0,
                latency_ms: 1,
                questions: BTreeMap::new(),
            })
        }

        fn name(&self) -> &str {
            &self.name
        }

        fn confidence_floor(&self) -> f32 {
            self.floor
        }
    }

    fn pack() -> Value {
        json!({"move": {"type": "choice", "instructions": "What do you do?", "criteria": {"opt0": "go"}}})
    }

    fn other_pack() -> Value {
        json!({"move": {"type": "choice", "instructions": "What do you do now?", "criteria": {"opt0": "go"}}})
    }

    #[test]
    fn a_second_ask_of_the_same_question_is_a_hit() {
        let cache = Arc::new(AnswerCache::new());
        let counting = Arc::new(Counting::new(0.3));
        let backend = CachedBackend::new(
            Box::new(CountingProxy(Arc::clone(&counting))),
            Arc::clone(&cache),
        );
        let deadline = Instant::now() + std::time::Duration::from_secs(1);
        let first = backend.ask("the same facts", &pack(), deadline).unwrap();
        let second = backend.ask("the same facts", &pack(), deadline).unwrap();
        assert_eq!(counting.calls(), 1, "the model is asked once");
        assert!(!first.questions["move"].cached);
        assert!(second.questions["move"].cached);
        assert_eq!(second.per_gate, first.per_gate);
        let stats = cache.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.stores, 1);
    }

    #[test]
    fn a_different_context_is_a_miss() {
        let cache = Arc::new(AnswerCache::new());
        let counting = Arc::new(Counting::new(0.3));
        let backend = CachedBackend::new(
            Box::new(CountingProxy(Arc::clone(&counting))),
            Arc::clone(&cache),
        );
        let deadline = Instant::now() + std::time::Duration::from_secs(1);
        backend.ask("one situation", &pack(), deadline).unwrap();
        backend.ask("another situation", &pack(), deadline).unwrap();
        assert_eq!(counting.calls(), 2);
    }

    #[test]
    fn a_reworded_question_is_a_miss() {
        let cache = Arc::new(AnswerCache::new());
        let counting = Arc::new(Counting::new(0.3));
        let backend = CachedBackend::new(
            Box::new(CountingProxy(Arc::clone(&counting))),
            Arc::clone(&cache),
        );
        let deadline = Instant::now() + std::time::Duration::from_secs(1);
        backend.ask("the same facts", &pack(), deadline).unwrap();
        backend
            .ask("the same facts", &other_pack(), deadline)
            .unwrap();
        assert_eq!(counting.calls(), 2, "a new version is a new question");
    }

    #[test]
    fn another_backend_does_not_get_the_answer() {
        let cache = Arc::new(AnswerCache::new());
        let counting = Arc::new(Counting::new(0.3));
        let deadline = Instant::now() + std::time::Duration::from_secs(1);
        let first = CachedBackend::new(
            Box::new(CountingProxy(Arc::clone(&counting))),
            Arc::clone(&cache),
        );
        first.ask("the same facts", &pack(), deadline).unwrap();
        // A second backend with the same cache and another name.
        let other = CachedBackend::new(
            Box::new(OtherBackend(CountingProxy(Arc::clone(&counting)))),
            Arc::clone(&cache),
        );
        other.ask("the same facts", &pack(), deadline).unwrap();
        assert_eq!(counting.calls(), 2, "another model is asked again");
    }

    #[test]
    fn another_floor_does_not_get_the_answer() {
        let cache = Arc::new(AnswerCache::new());
        let counting = Arc::new(Counting::new(0.3));
        let deadline = Instant::now() + std::time::Duration::from_secs(1);
        let lenient = CachedBackend::new(
            Box::new(CountingProxy(Arc::clone(&counting))),
            Arc::clone(&cache),
        );
        lenient.ask("the same facts", &pack(), deadline).unwrap();
        let strict = CachedBackend::new(
            Box::new(StrictBackend(CountingProxy(Arc::clone(&counting)))),
            Arc::clone(&cache),
        );
        strict.ask("the same facts", &pack(), deadline).unwrap();
        assert_eq!(counting.calls(), 2, "another floor is asked again");
    }

    #[test]
    fn only_the_missing_questions_are_asked() {
        let cache = Arc::new(AnswerCache::new());
        let counting = Arc::new(Counting::new(0.3));
        let deadline = Instant::now() + std::time::Duration::from_secs(1);
        let backend = CachedBackend::new(
            Box::new(CountingProxy(Arc::clone(&counting))),
            Arc::clone(&cache),
        );
        let one = json!({"move": {"type": "choice", "instructions": "What do you do?", "criteria": {"opt0": "go"}}});
        backend.ask("the same facts", &one, deadline).unwrap();
        let two = json!({
            "move": {"type": "choice", "instructions": "What do you do?", "criteria": {"opt0": "go"}},
            "talk": {"type": "noul", "instructions": "Is there something to say?"},
        });
        let answers = backend.ask("the same facts", &two, deadline).unwrap();
        assert_eq!(counting.calls(), 2, "the held question is not asked again");
        assert!(answers.questions["move"].cached);
        assert!(!answers.questions["talk"].cached);
        assert_eq!(answers.per_gate.len(), 2);
    }

    #[test]
    fn the_age_of_a_hit_is_counted_in_asks() {
        let cache = Arc::new(AnswerCache::new());
        let counting = Arc::new(Counting::new(0.3));
        let backend = CachedBackend::new(
            Box::new(CountingProxy(Arc::clone(&counting))),
            Arc::clone(&cache),
        );
        let deadline = Instant::now() + std::time::Duration::from_secs(1);
        backend.ask("the same facts", &pack(), deadline).unwrap();
        backend.ask("other facts", &pack(), deadline).unwrap();
        let answers = backend.ask("the same facts", &pack(), deadline).unwrap();
        assert_eq!(answers.questions["move"].age_asks, 2, "two asks old");
    }

    /// A proxy so the test can count the calls of a backend behind a
    /// `Box<dyn SysOne>`.
    struct CountingProxy(Arc<Counting>);

    impl SysOne for CountingProxy {
        fn ask(
            &self,
            state: &str,
            questions: &Value,
            deadline: Instant,
        ) -> Result<Answers, Refusal> {
            self.0.ask(state, questions, deadline)
        }
        fn name(&self) -> &str {
            self.0.name()
        }
        fn confidence_floor(&self) -> f32 {
            self.0.confidence_floor()
        }
    }

    /// Another backend under another name.
    struct OtherBackend(CountingProxy);

    impl SysOne for OtherBackend {
        fn ask(
            &self,
            state: &str,
            questions: &Value,
            deadline: Instant,
        ) -> Result<Answers, Refusal> {
            self.0.ask(state, questions, deadline)
        }
        fn name(&self) -> &str {
            "other"
        }
        fn confidence_floor(&self) -> f32 {
            self.0.confidence_floor()
        }
    }

    /// The same backend under another floor.
    struct StrictBackend(CountingProxy);

    impl SysOne for StrictBackend {
        fn ask(
            &self,
            state: &str,
            questions: &Value,
            deadline: Instant,
        ) -> Result<Answers, Refusal> {
            self.0.ask(state, questions, deadline)
        }
        fn name(&self) -> &str {
            self.0.name()
        }
        fn confidence_floor(&self) -> f32 {
            0.9
        }
    }
}
