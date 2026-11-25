//! The question ledger: what each question of a round cost, and what it
//! answered.
//!
//! The ledger is the seed of ROADMAP-AHEAD §2.5: a per-question record of
//! provenance, calibration and cost. It is **harness-adjacent and never
//! hashed**: it describes the questions, not the world, so it sits beside
//! the round's other records and never enters the round log (D69: only
//! decisions travel).
//!
//! One row per question, keyed by the question's id and its version, with:
//!
//! - **asks**: how many times the question was put to a model;
//! - **unsure**: how many times the model was not sure of it (below the
//!   floor), so the gate made no decision of its own (D26);
//! - **refused**: how many times the question was asked and no answer came
//!   back at all — the call timed out, the endpoint failed, the pack did
//!   not fit;
//! - **escalated**: how many times a second model was asked about it (D71);
//! - **mean confidence per model**: the model's own number, in permille,
//!   over the answers it gave directly;
//! - **ms cost**: the model's own time on the packs that carried it;
//! - **cache hits**: how many of the answers came out of the answer cache
//!   instead of out of a model.
//!
//! The numbers are integers: the confidence is in permille, so a ledger is
//! the same file on every machine, and a reader never has to wonder what
//! a float meant.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::cache::CacheStats;
use crate::question::{normalized_form, options_of, QuestionIdentity};
use crate::sysone::{Answer, Answers, Refusal};

/// One model's confidence in the questions it answered directly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ModelConfidence {
    /// The sum of the model's confidences, in permille.
    pub sum_permille: u64,
    /// How many answers the sum covers.
    pub n: u64,
}

impl ModelConfidence {
    /// The mean confidence, in permille.
    pub fn mean_permille(&self) -> u64 {
        self.sum_permille.checked_div(self.n).unwrap_or(0)
    }

    /// One more answer, with its confidence.
    fn add(&mut self, permille: u64) {
        self.sum_permille = self.sum_permille.saturating_add(permille);
        self.n = self.n.saturating_add(1);
    }
}

/// One question's row.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct LedgerRow {
    /// The question's id.
    pub id: String,
    /// The question's version: a rewording is a new one.
    pub version: String,
    /// The gate's slot: the name the answer comes back under.
    pub slot: String,
    /// The question's kind: `choice`, `noul` or `score`.
    pub kind: String,
    /// The question's normalized form.
    pub normalized: String,
    /// How many times the question was put to a model.
    pub asks: u64,
    /// How many answers the model gave and the round took.
    pub accepted: u64,
    /// How many times the model was not sure of the question.
    pub unsure: u64,
    /// How many times the question was asked and no answer came back.
    pub refused: u64,
    /// How many times a second model was asked about it.
    pub escalated: u64,
    /// How many answers came out of the cache.
    pub cache_hits: u64,
    /// The model's time on the packs that carried the question, in
    /// milliseconds.
    pub ms_cost: u64,
    /// The confidence per model, in permille.
    pub confidence: BTreeMap<String, ModelConfidence>,
}

/// The ledger of a round.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuestionLedger {
    /// One row per (id, version).
    rows: BTreeMap<(String, String), LedgerRow>,
    /// The harness's own identity per slot: the normalized form of the
    /// question as the harness wrote it.
    identities: BTreeMap<String, QuestionIdentity>,
    /// The cache's numbers, for the round's header.
    cache: CacheStats,
}

impl QuestionLedger {
    /// An empty ledger.
    pub fn new() -> QuestionLedger {
        QuestionLedger::default()
    }

    /// Tell the ledger which questions the harness asks, by slot.
    ///
    /// The harness's identity is the better source of a question's
    /// normalized form than the pack is: the pack carries the question as
    /// it was asked, with the menu's options expanded, while the harness
    /// carries the question as it was written.
    pub fn set_slot_identities(&mut self, identities: &BTreeMap<String, QuestionIdentity>) {
        self.identities = identities.clone();
    }

    /// Record one ask and its outcome.
    ///
    /// `pack` is the question pack the model was asked, `result` is what
    /// came back, and `latency_ms` is the model's own round trip.
    pub fn record(&mut self, pack: &Value, result: &Result<Answers, Refusal>, latency_ms: u32) {
        let Some(gates) = pack.as_object() else {
            return;
        };
        for (slot, question) in gates {
            let row = self.row_for(slot, question);
            row.asks = row.asks.saturating_add(1);
            row.ms_cost = row.ms_cost.saturating_add(u64::from(latency_ms));
        }
        match result {
            // The question was asked and no answer came back: the refusal
            // is the row's own, and it is counted for every gate the pack
            // carried.
            Err(_) => {
                for (slot, question) in gates {
                    let row = self.row_for(slot, question);
                    row.refused = row.refused.saturating_add(1);
                }
            }
            Ok(answers) => {
                for gate in answers.unsure.keys() {
                    if let Some(row) = self.row_mut(gate, gates) {
                        row.unsure = row.unsure.saturating_add(1);
                    }
                }
                for (gate, escalation) in &answers.escalated {
                    if let Some(row) = self.row_mut(gate, gates) {
                        row.escalated = row.escalated.saturating_add(1);
                        // The cheap model's own confidence is the cheap
                        // model's calibration, so it is counted under the
                        // cheap model's name.
                        let model = answers.model.clone();
                        row.confidence
                            .entry(model)
                            .or_default()
                            .add(permille(escalation.primary_confidence));
                    }
                }
                for (gate, answer) in &answers.per_gate {
                    let Some(row) = self.row_mut(gate, gates) else {
                        continue;
                    };
                    row.accepted = row.accepted.saturating_add(1);
                    // An escalated gate's answer is the **second** model's,
                    // and the second model's name is not in the answer: the
                    // cheap model's own confidence was already counted
                    // above, so counting this one too would charge the
                    // cheap model for the second model's number.
                    if answers.escalated.contains_key(gate) {
                        continue;
                    }
                    if let Some(confidence) = answer_confidence(answer) {
                        let model = answers.model.clone();
                        row.confidence
                            .entry(model)
                            .or_default()
                            .add(permille(confidence));
                    }
                }
                // The answers that came out of the cache: they cost the
                // round nothing, and the row says so.
                for (gate, stamp) in &answers.questions {
                    if !stamp.cached {
                        continue;
                    }
                    if let Some(row) = self.row_mut(gate, gates) {
                        row.cache_hits = row.cache_hits.saturating_add(1);
                    }
                }
            }
        }
    }

    /// The cache's numbers, for the ledger's header.
    pub fn record_cache(&mut self, stats: CacheStats) {
        self.cache = stats;
    }

    /// The rows, by question.
    pub fn rows(&self) -> impl Iterator<Item = &LedgerRow> {
        self.rows.values()
    }

    /// The row of one question.
    pub fn row(&self, id: &str, version: &str) -> Option<&LedgerRow> {
        self.rows.get(&(id.to_string(), version.to_string()))
    }

    /// The cache's numbers.
    pub fn cache(&self) -> CacheStats {
        self.cache
    }

    /// The ledger as TOML: the shape ROADMAP-AHEAD §2.5 asks for.
    pub fn to_toml(&self) -> String {
        let mut out = String::new();
        out.push_str(
            "# The question ledger of a round.\n\
             #\n\
             # One row per question: the questions are the harness's\n\
             # (assets/harness/default/gates.toml), and this file records\n\
             # what they cost and what they answered. It is not hashed: the\n\
             # round's log carries the decisions, never the ledger.\n\
             #\n\
             # The confidence is in permille (0-1000) and is the model's\n\
             # own number over the answers it gave directly.\n\n",
        );
        out.push_str("[round]\n");
        out.push_str(&format!("questions = {}\n", self.rows.len()));
        out.push_str(&format!("asks = {}\n", self.total_asks()));
        out.push_str(&format!("cache_hits = {}\n", self.cache.hits));
        out.push_str(&format!("cache_misses = {}\n", self.cache.misses));
        out.push_str(&format!("cache_entries = {}\n", self.cache.entries));
        out.push_str(&format!("cache_age_mean_asks = {}\n", self.cache.age_mean));
        out.push_str(&format!("cache_age_max_asks = {}\n", self.cache.age_max));
        out.push('\n');
        for row in self.rows.values() {
            out.push_str("[[question]]\n");
            out.push_str(&format!("id = {}\n", quoted(&row.id)));
            out.push_str(&format!("version = {}\n", quoted(&row.version)));
            out.push_str(&format!("slot = {}\n", quoted(&row.slot)));
            out.push_str(&format!("kind = {}\n", quoted(&row.kind)));
            out.push_str(&format!("normalized = {}\n", quoted(&row.normalized)));
            out.push_str(&format!("asks = {}\n", row.asks));
            out.push_str(&format!("accepted = {}\n", row.accepted));
            out.push_str(&format!("unsure = {}\n", row.unsure));
            out.push_str(&format!("refused = {}\n", row.refused));
            out.push_str(&format!("escalated = {}\n", row.escalated));
            out.push_str(&format!("cache_hits = {}\n", row.cache_hits));
            out.push_str(&format!("ms_cost = {}\n", row.ms_cost));
            for (model, confidence) in &row.confidence {
                out.push_str("\n[[question.confidence]]\n");
                out.push_str(&format!("model = {}\n", quoted(model)));
                out.push_str(&format!("mean_permille = {}\n", confidence.mean_permille()));
                out.push_str(&format!("n = {}\n", confidence.n));
            }
            out.push('\n');
        }
        out
    }

    /// Write the ledger to a file.
    ///
    /// The file is the round's record of its questions; it is written
    /// beside the round's other files and never enters the hashed log.
    pub fn write(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(path, self.to_toml())
    }

    /// The round's total asks.
    fn total_asks(&self) -> u64 {
        self.rows.values().map(|row| row.asks).sum()
    }

    /// The row of a question, made if it is not there yet.
    ///
    /// The row's fields are worked out once, when the row is made: the
    /// question's name does not change, and the round asks the same
    /// question over and over.
    fn row_for(&mut self, slot: &str, question: &Value) -> &mut LedgerRow {
        let id = question
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_default();
        let version = question
            .get("version")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_default();
        let key = (id.clone(), version.clone());
        if !self.rows.contains_key(&key) {
            let kind = question
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string();
            // The harness's own normalized form, when the ledger knows the
            // question: the pack carries the question as it was asked, with
            // the menu's options expanded, and the harness carries the
            // question as it was written.
            let normalized = match self.identities.get(slot) {
                Some(identity) => identity.normalized.clone(),
                None => {
                    let instructions = question
                        .get("instructions")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    normalized_form(&kind, instructions, &options_of(question))
                }
            };
            self.rows.insert(
                key.clone(),
                LedgerRow {
                    id,
                    version,
                    slot: slot.to_string(),
                    kind,
                    normalized,
                    ..LedgerRow::default()
                },
            );
        }
        self.rows.get_mut(&key).expect("the row was just made")
    }

    /// The row of a gate of the pack, if the pack carried it.
    fn row_mut(
        &mut self,
        gate: &str,
        gates: &serde_json::Map<String, Value>,
    ) -> Option<&mut LedgerRow> {
        let question = gates.get(gate)?;
        let id = question.get("id").and_then(Value::as_str)?.to_string();
        let version = question.get("version").and_then(Value::as_str)?.to_string();
        self.rows.get_mut(&(id, version))
    }
}

/// The confidence of an answer, if it has one.
fn answer_confidence(answer: &Answer) -> Option<f32> {
    match answer {
        Answer::Choice { confidence, .. } | Answer::Score { confidence, .. } => Some(*confidence),
        Answer::Noul(_) => None,
    }
}

/// A confidence in permille.
fn permille(confidence: f32) -> u64 {
    if !confidence.is_finite() {
        return 0;
    }
    let value = (confidence.clamp(0.0, 1.0) * 1000.0).round();
    u64::try_from(value as i64).unwrap_or(0)
}

/// A TOML string, quoted and escaped.
fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pack() -> Value {
        json!({
            "move": {
                "type": "choice",
                "instructions": "What do you do?",
                "criteria": {"opt0": "go", "opt1": "hold"},
                "id": "q_0000000000000001",
                "version": "v_0000000000000001",
            }
        })
    }

    fn answers(confidence: f32) -> Answers {
        Answers {
            decomposed: BTreeMap::new(),
            per_gate: BTreeMap::from([(
                "move".to_string(),
                Answer::Choice {
                    probs: BTreeMap::from([("opt0".to_string(), 0.7)]),
                    confidence,
                },
            )]),
            unsure: BTreeMap::new(),
            escalated: BTreeMap::new(),
            model: "thor-d1-2b".to_string(),
            input_tokens: 10,
            latency_ms: 5,
            questions: BTreeMap::new(),
        }
    }

    #[test]
    fn a_asked_question_gets_a_row() {
        let mut ledger = QuestionLedger::new();
        ledger.record(&pack(), &Ok(answers(0.6)), 120);
        let rows: Vec<&LedgerRow> = ledger.rows().collect();
        assert_eq!(rows.len(), 1);
        let row = rows[0];
        assert_eq!(row.id, "q_0000000000000001");
        assert_eq!(row.version, "v_0000000000000001");
        assert_eq!(row.slot, "move");
        assert_eq!(row.kind, "choice");
        assert_eq!(row.asks, 1);
        assert_eq!(row.accepted, 1);
        assert_eq!(row.ms_cost, 120);
        assert_eq!(row.confidence["thor-d1-2b"].mean_permille(), 600);
        assert_eq!(row.confidence["thor-d1-2b"].n, 1);
    }

    #[test]
    fn an_unsure_gate_is_counted_apart() {
        let mut ledger = QuestionLedger::new();
        let mut answer = answers(0.6);
        answer.unsure.insert("move".to_string(), 0.2);
        answer.per_gate.clear();
        ledger.record(&pack(), &Ok(answer), 10);
        let row = ledger.rows().next().unwrap();
        assert_eq!(row.unsure, 1);
        assert_eq!(row.accepted, 0);
        assert_eq!(row.asks, 1);
    }

    #[test]
    fn a_refusal_is_counted_for_every_gate() {
        let mut ledger = QuestionLedger::new();
        ledger.record(&pack(), &Err(Refusal::Timeout), 400);
        let row = ledger.rows().next().unwrap();
        assert_eq!(row.refused, 1);
        assert_eq!(row.asks, 1);
        assert_eq!(row.accepted, 0);
    }

    #[test]
    fn an_escalated_gate_is_counted_and_the_cheap_model_is_credited() {
        let mut ledger = QuestionLedger::new();
        let mut answer = answers(0.6);
        answer.escalated.insert(
            "move".to_string(),
            crate::sysone::Escalation {
                primary_probs: BTreeMap::new(),
                second_probs: BTreeMap::new(),
                gate: "move".to_string(),
                primary_confidence: 0.4,
                primary_choice: Some("opt0".to_string()),
                second_confidence: Some(0.8),
                second_choice: Some("opt1".to_string()),
                latency_ms: 900,
                outcome: crate::sysone::EscalationOutcome::Taken,
                refusal: None,
            },
        );
        ledger.record(&pack(), &Ok(answer), 120);
        let row = ledger.rows().next().unwrap();
        assert_eq!(row.escalated, 1);
        assert_eq!(row.confidence["thor-d1-2b"].mean_permille(), 400);
    }

    #[test]
    fn a_cached_answer_is_counted_as_a_hit() {
        let mut ledger = QuestionLedger::new();
        let mut answer = answers(0.6);
        answer.questions.insert(
            "move".to_string(),
            crate::question::QuestionStamp {
                id: "q_0000000000000001".to_string(),
                version: "v_0000000000000001".to_string(),
                normalized: String::new(),
                cached: true,
                age_asks: 3,
            },
        );
        ledger.record(&pack(), &Ok(answer), 0);
        let row = ledger.rows().next().unwrap();
        assert_eq!(row.cache_hits, 1);
        assert_eq!(row.ms_cost, 0, "a cached answer cost the round nothing");
    }

    #[test]
    fn the_ledger_is_toml_and_says_what_it_holds() {
        let mut ledger = QuestionLedger::new();
        ledger.record(&pack(), &Ok(answers(0.6)), 120);
        let text = ledger.to_toml();
        assert!(text.contains("[[question]]"));
        assert!(text.contains("id = \"q_0000000000000001\""));
        assert!(text.contains("slot = \"move\""));
        assert!(text.contains("mean_permille = 600"));
        // The file parses as TOML.
        let parsed: toml::Value = toml::from_str(&text).expect("the ledger is TOML");
        let questions = parsed["question"].as_array().expect("the rows");
        assert_eq!(questions.len(), 1);
        assert_eq!(questions[0]["asks"].as_integer(), Some(1));
    }

    #[test]
    fn two_versions_of_one_question_are_two_rows() {
        let mut ledger = QuestionLedger::new();
        ledger.record(&pack(), &Ok(answers(0.6)), 10);
        let mut reworded = pack();
        reworded["move"]["version"] = json!("v_0000000000000002");
        ledger.record(&reworded, &Ok(answers(0.6)), 10);
        assert_eq!(ledger.rows().count(), 2);
    }
}
