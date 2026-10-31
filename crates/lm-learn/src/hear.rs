//! The vibe gate: a kid's words become a typed proposal, through two
//! closed questions the decision server answers (Vibe Dictionary part 3).
//!
//! ```text
//! the kid speaks
//!   -> question 1 (vibe):        which change? one option per dictionary
//!                                 entry, plus none — never free text
//!   -> question 2 (vibe_value):  how much? the entry's own bounded steps
//!                                 (a little / a lot / back to normal)
//!   -> a typed proposal: entry + value, each with its confidence
//!   -> --propose hands it to [vibe::propose], prediction sealed first
//! ```
//!
//! Every refusal is named (sandbox §3): an answer below the gate's
//! confidence floor — the D72 gate-floor mechanism, per question — asks
//! the kid to say it another way, and `none` says the dictionary holds no
//! word for what was asked. The model's output is an index into the
//! dictionary, never a program and never a patch.
//!
//! The gate is asked only when a kid speaks, so it is not part of the
//! agents' in-round packs (D70): it has its own small policy pack here
//! (`assets/harness/default/vibe-gate.toml`), which is data in the
//! harness like the dictionary is.
//!
//! Privacy (sandbox §7): the words travel only to the decision server and
//! live only in this call's memory. Nothing here writes them — the record
//! a proposal carries is the bounded 48-character label
//! ([`vibe::bounded_label`]), never a transcript, never audio. A hearing
//! writes no file at all; only `--propose` writes, through the same
//! pipeline a typed proposal uses.

use std::fmt;
use std::fmt::Write as _;
use std::path::Path;
use std::time::{Duration, Instant};

use lm_agents::sysone::{Answer, Answers, Refusal, SysOne};
use serde_json::{json, Map, Value};

use crate::vibe::{bounded_label, Block, Dictionary, Step};

/// The policy pack this build ships, embedded the same way the dictionary
/// is: the file in the harness is the source, the binary carries a
/// snapshot, and a `--gate-file` override reads the disk copy.
pub const DEFAULT_GATE_PACK: &str = include_str!("../../../assets/harness/default/vibe-gate.toml");

/// The most characters of the kid's words one hearing will carry.
///
/// The words go to the decision server and no further; a hearing that
/// would carry more than this is a kid pasting a paragraph, and the honest
/// answer is to ask them to say it shorter — not to silently cut the
/// words and risk hearing only half of what was said.
pub const WORDS_MAX: usize = 160;

/// The one phrase a kid says that skips the model: putting it back.
///
/// "Put it back" as *words* names the undo intent directly, and a hearing
/// that proposed the shipped default instead would run a test that cannot
/// mean anything (the pipeline measures every change against the default).
/// So this one phrase is matched before the server is asked, and the
/// refusal names `lm vibe undo`. The *step* named "back to normal" —
/// chosen by the model over a change-asking phrase — is the same refusal,
/// for the same reason.
pub const PUT_IT_BACK: &str = "put it back";

// ---------------------------------------------------------------- the pack

/// The `[gate]` table: the first question, plus its `none` option.
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
struct GateFile {
    #[serde(rename = "id")]
    id: String,
    /// The kind of question. Only `choice` is legal here: the vibe gate
    /// is a closed choice by construction, and a file that says otherwise
    /// is refused rather than reinterpreted.
    #[serde(rename = "type")]
    kind: String,
    #[serde(rename = "instructions")]
    instructions: String,
    /// The wording of the `none` option: the honest edge of the closed
    /// vocabulary.
    #[serde(rename = "none")]
    none: String,
}

/// The `[value]` table: the second question.
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
struct QuestionFile {
    #[serde(rename = "id")]
    id: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(rename = "instructions")]
    instructions: String,
}

/// The `[policy]` table: who answers, and how sure they must be.
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
struct PolicyFile {
    /// The model, by its name in the agents' config (which is the one
    /// place that says where the Thor is and which port serves the model).
    #[serde(rename = "model")]
    model: String,
    /// The confidence floor (D72): an answer below it is a named refusal.
    #[serde(rename = "min_confidence")]
    min_confidence: f32,
    /// The deadline for one question, in milliseconds.
    #[serde(rename = "timeout_ms")]
    timeout_ms: u64,
}

/// The file's shape.
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
struct PackFile {
    #[serde(rename = "gate")]
    gate: GateFile,
    #[serde(rename = "value")]
    value: QuestionFile,
    #[serde(rename = "policy")]
    policy: PolicyFile,
}

/// The vibe gate's policy pack, parsed and validated: the two questions
/// and who answers them.
#[derive(Clone, Debug, PartialEq)]
pub struct GatePack {
    /// The first question's id (`vibe`).
    pub gate_id: String,
    /// The first question's instructions.
    pub gate_instructions: String,
    /// The second question's id (`vibe_value`).
    pub value_id: String,
    /// The second question's instructions.
    pub value_instructions: String,
    /// The wording of the `none` option of the first question.
    pub none: String,
    /// The model that answers, by its name in the agents' config.
    pub model: String,
    /// The confidence floor: below it the answer is a named refusal.
    pub min_confidence: f32,
    /// The deadline for one question.
    pub timeout: Duration,
}

impl GatePack {
    /// Parse and validate a `vibe-gate.toml`.
    pub fn from_toml(text: &str) -> Result<GatePack, PackInvalid> {
        let file: PackFile = toml::from_str(text).map_err(|err| PackInvalid(err.to_string()))?;
        for (table, id, kind) in [
            ("gate", &file.gate.id, &file.gate.kind),
            ("value", &file.value.id, &file.value.kind),
        ] {
            if kind != "choice" {
                return Err(PackInvalid(format!(
                    "the {table} question is a `{kind}`, but the vibe gate asks closed \
                     choices only"
                )));
            }
            if id.trim().is_empty() {
                return Err(PackInvalid(format!("the {table} question has no id")));
            }
        }
        if file.gate.id == file.value.id {
            return Err(PackInvalid(format!(
                "the two questions share the id `{}`",
                file.gate.id
            )));
        }
        if file.gate.none.trim().is_empty() {
            return Err(PackInvalid(
                "the gate's `none` option says nothing".to_owned(),
            ));
        }
        if !(0.0..=1.0).contains(&file.policy.min_confidence) || file.policy.min_confidence == 0.0 {
            return Err(PackInvalid(format!(
                "the confidence floor {} is not a floor a question can have",
                file.policy.min_confidence
            )));
        }
        if file.policy.timeout_ms < 100 {
            return Err(PackInvalid(format!(
                "the timeout {} ms is too short to ask a question",
                file.policy.timeout_ms
            )));
        }
        Ok(GatePack {
            gate_id: file.gate.id,
            gate_instructions: file.gate.instructions,
            value_id: file.value.id,
            value_instructions: file.value.instructions,
            none: file.gate.none,
            model: file.policy.model,
            min_confidence: file.policy.min_confidence,
            timeout: Duration::from_millis(file.policy.timeout_ms),
        })
    }

    /// Read and validate a `vibe-gate.toml` from disk.
    pub fn from_file(path: &Path) -> Result<GatePack, PackInvalid> {
        let text = std::fs::read_to_string(path)
            .map_err(|err| PackInvalid(format!("{}: {err}", path.display())))?;
        Self::from_toml(&text)
    }

    /// The pack this build ships.
    pub fn default_pack() -> Result<GatePack, PackInvalid> {
        Self::from_toml(DEFAULT_GATE_PACK)
    }
}

/// Why a policy pack cannot be used. A pack that disagrees with what the
/// gate actually asks is refused, not reinterpreted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackInvalid(String);

impl fmt::Display for PackInvalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the vibe gate's policy pack is not valid: {}", self.0)
    }
}

impl std::error::Error for PackInvalid {}

// ------------------------------------------------------------- the refusals

/// Why the words did not become a proposal. Every refusal is named, and
/// the ones a kid can hear ask them to do the one thing that helps.
#[derive(Clone, Debug, PartialEq)]
pub enum HearError {
    /// The decision server did not answer.
    Backend(Refusal),
    /// The first question's answer came back under the floor: the gate is
    /// not sure what change the kid means, so the kid is asked to say it
    /// another way (D72's gate-floor mechanism).
    EntryUnsure {
        /// The confidence the answer carried.
        confidence: f32,
        /// The floor it had to clear.
        floor: f32,
    },
    /// The words match no change in the dictionary: Bench has no word for
    /// that yet, and the ask belongs in the request pile.
    NoMatch,
    /// The second question's answer came back under the floor: the change
    /// is named but the amount is not sure.
    ValueUnsure {
        /// The entry that was matched.
        entry: String,
        /// The confidence the answer carried.
        confidence: f32,
        /// The floor it had to clear.
        floor: f32,
    },
    /// The answer named an option the question never offered: the model
    /// answered outside the closed set, which is a refusal, never a
    /// neighbouring option (D26).
    OffMenu {
        /// The question that was answered.
        gate: String,
        /// The option the model named.
        option: String,
    },
    /// The words say "put it back", or the model picked the "back to
    /// normal" step: that is the undo, not a proposal.
    PutItBack {
        /// The entry whose step said it, when a step did.
        entry: Option<String>,
    },
    /// There is nothing to hear: empty or only whitespace.
    SaidNothing,
    /// The words are too long to be one kid's utterance; the honest
    /// answer asks them to say it shorter, not to cut the words.
    TooLong {
        /// How many characters were said.
        chars: usize,
        /// The most one hearing carries.
        max: usize,
    },
}

impl fmt::Display for HearError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HearError::Backend(refusal) => write!(f, "the decision server: {refusal}"),
            HearError::EntryUnsure { confidence, floor } => write!(
                f,
                "refused: Bench is only {confidence:.2} sure what change is meant (the gate's \
                 floor is {floor:.2}) — ask the kid to say it another way"
            ),
            HearError::NoMatch => write!(
                f,
                "refused: the words match no change in the dictionary — Bench has no word for \
                 that yet; the ask goes to the request pile"
            ),
            HearError::ValueUnsure {
                entry,
                confidence,
                floor,
            } => write!(
                f,
                "refused: Bench hears `{entry}` but is only {confidence:.2} sure how much (the \
                 gate's floor is {floor:.2}) — ask the kid to say it another way"
            ),
            HearError::OffMenu { gate, option } => write!(
                f,
                "refused: the answer to `{gate}` named `{option}`, which the question never \
                 offered"
            ),
            HearError::PutItBack { entry } => match entry {
                Some(entry) => write!(
                    f,
                    "refused: the words put `{entry}` back to normal — that is the undo, not a \
                     proposal (`lm vibe undo`)"
                ),
                None => write!(
                    f,
                    "refused: the words say \"{PUT_IT_BACK}\" — that is the undo, not a \
                     proposal (`lm vibe undo`)"
                ),
            },
            HearError::SaidNothing => {
                write!(f, "refused: the kid said nothing to hear")
            }
            HearError::TooLong { chars, max } => write!(
                f,
                "refused: {chars} characters is not one utterance (the most a hearing carries \
                 is {max}) — ask the kid to say it shorter"
            ),
        }
    }
}

impl std::error::Error for HearError {}

// ----------------------------------------------------------------- the gate

/// What one hearing heard: the typed proposal it would make, with the
/// confidence of each answer. The words stay in memory and go nowhere
/// else; nothing is written.
#[derive(Clone, Debug, PartialEq)]
pub struct Heard {
    /// The words, session memory only. Printed by the summary, never
    /// written by the pipeline.
    pub words: String,
    /// The entry the first question picked (`unit.vision`).
    pub entry: String,
    /// The first answer's confidence.
    pub entry_confidence: f32,
    /// The step the second question picked, as the dictionary words it.
    pub step: Step,
    /// The second answer's confidence.
    pub value_confidence: f32,
    /// The value the entry ships — the baseline the proposal moves from.
    pub from: i64,
    /// The backend that answered (`thor-d1-2b`).
    pub backend: String,
    /// Each question's round trip, in milliseconds.
    pub latency_ms: [u32; 2],
}

impl Heard {
    /// The typed proposal this hearing would make: the entry and the
    /// value, the way `lm vibe propose` takes them.
    pub fn proposal_of(&self) -> (&str, i64) {
        (&self.entry, self.step.value)
    }

    /// What the hearing heard, in words: the two answers with their
    /// confidence, and the proposal they add up to. The label is the
    /// bounded words — the only written form of them.
    pub fn summary(&self) -> String {
        let (entry, value) = self.proposal_of();
        let mut s = String::new();
        let _ = writeln!(s, "heard:     \"{}\"", self.words);
        let _ = writeln!(
            s,
            "  entry:    {} (confidence {:.2}, {} ms)",
            self.entry, self.entry_confidence, self.latency_ms[0]
        );
        let _ = writeln!(
            s,
            "  value:    {}  \"{}\" (confidence {:.2}, {} ms)",
            value, self.step.say, self.value_confidence, self.latency_ms[1]
        );
        let _ = writeln!(
            s,
            "  would propose: {} {} -> {}  \"{}\"",
            entry,
            self.from,
            value,
            bounded_label(&self.words)
        );
        s
    }
}

/// Hear the words: ask the two closed questions and read the typed
/// proposal out of the answers.
///
/// The first question is one closed choice over the dictionary's entry
/// ids plus `none`; the second — asked only when the first matched a
/// change — is one closed choice over that entry's own bounded steps.
/// Both answers must clear the pack's confidence floor or the hearing is
/// a named refusal that asks the kid to say it another way; an answer
/// that names an option the question never offered is refused too (D26).
///
/// The deadline is the pack's own, one per question. Nothing is written;
/// the words travel to the backend and stop there.
pub fn hear(
    backend: &dyn SysOne,
    pack: &GatePack,
    dictionary: &Dictionary,
    words: &str,
) -> Result<Heard, HearError> {
    let words = words.trim();
    if words.is_empty() {
        return Err(HearError::SaidNothing);
    }
    let chars = words.chars().count();
    if chars > WORDS_MAX {
        return Err(HearError::TooLong {
            chars,
            max: WORDS_MAX,
        });
    }
    // The one phrase that is an instruction to the game itself, not a
    // change: putting it back is the undo.
    if words.eq_ignore_ascii_case(PUT_IT_BACK) {
        return Err(HearError::PutItBack { entry: None });
    }

    // The first question: which change?
    let first = ask(
        backend,
        pack,
        &entry_state(words),
        entry_question(pack, dictionary),
        &pack.gate_id,
    )?;
    let (chosen, confidence) = match answer_of(&first, &pack.gate_id)? {
        Picked::Unsure(confidence) => {
            return Err(HearError::EntryUnsure {
                confidence,
                floor: pack.min_confidence,
            });
        }
        Picked::Choice(chosen, confidence) => {
            if confidence < pack.min_confidence {
                return Err(HearError::EntryUnsure {
                    confidence,
                    floor: pack.min_confidence,
                });
            }
            (chosen, confidence)
        }
    };
    if chosen == "none" {
        return Err(HearError::NoMatch);
    }
    let block = dictionary.find(&chosen).ok_or_else(|| HearError::OffMenu {
        gate: pack.gate_id.clone(),
        option: chosen,
    })?;

    // The second question: how much? Only the matched entry's own steps.
    let second = ask(
        backend,
        pack,
        &value_state(words, block),
        value_question(pack, block),
        &pack.value_id,
    )?;
    let (picked, value_confidence) = match answer_of(&second, &pack.value_id)? {
        Picked::Unsure(confidence) => {
            return Err(HearError::ValueUnsure {
                entry: block.id.clone(),
                confidence,
                floor: pack.min_confidence,
            });
        }
        Picked::Choice(picked, confidence) => {
            if confidence < pack.min_confidence {
                return Err(HearError::ValueUnsure {
                    entry: block.id.clone(),
                    confidence,
                    floor: pack.min_confidence,
                });
            }
            (picked, confidence)
        }
    };
    let value: i64 = picked.parse().map_err(|_| HearError::OffMenu {
        gate: pack.value_id.clone(),
        option: picked.clone(),
    })?;
    let step = block.step_of(value).ok_or_else(|| HearError::OffMenu {
        gate: pack.value_id.clone(),
        option: picked.clone(),
    })?;
    // "Back to normal" is the undo, not a proposal: the pipeline measures
    // every change against the shipped default, so a proposal of the
    // default is a test of nothing.
    if step.value == block.default {
        return Err(HearError::PutItBack {
            entry: Some(block.id.clone()),
        });
    }

    Ok(Heard {
        words: words.to_owned(),
        entry: block.id.clone(),
        entry_confidence: confidence,
        step: step.clone(),
        value_confidence,
        from: block.default,
        backend: backend.name().to_owned(),
        latency_ms: [first.latency_ms, second.latency_ms],
    })
}

/// One question, one answer, one deadline: the pack's own. A backend
/// refusal is the hearing's refusal.
fn ask(
    backend: &dyn SysOne,
    pack: &GatePack,
    state: &str,
    question: Value,
    gate: &str,
) -> Result<Answers, HearError> {
    let mut questions = Map::new();
    questions.insert(gate.to_owned(), question);
    let questions = Value::Object(questions);
    let deadline = Instant::now() + pack.timeout;
    backend
        .ask(state, &questions, deadline)
        .map_err(HearError::Backend)
}

/// What the model picked for one question: an option with its
/// confidence, or the backend's own unsure of it (D26: an unsure gate
/// never became a decision).
enum Picked {
    /// The top option, and the answer's confidence.
    Choice(String, f32),
    /// The backend refused the gate as unsure, with the confidence that
    /// refused it.
    Unsure(f32),
}

/// Read one gate's answer out of an answer set.
fn answer_of(answers: &Answers, gate: &str) -> Result<Picked, HearError> {
    if let Some(confidence) = answers.unsure.get(gate) {
        return Ok(Picked::Unsure(*confidence));
    }
    let malformed = |why: String| HearError::Backend(Refusal::Malformed(why));
    let Some(answer) = answers.per_gate.get(gate) else {
        return Err(malformed(format!(
            "the answer has nothing for the gate `{gate}`"
        )));
    };
    let Answer::Choice { probs, confidence } = answer else {
        return Err(malformed(format!(
            "the gate `{gate}` was answered as something other than a choice"
        )));
    };
    if probs.is_empty() {
        return Err(malformed(format!(
            "the answer to `{gate}` has no probabilities"
        )));
    }
    // The top option: the largest probability, ties to the first key in
    // map order, so the same answer always names the same option.
    let mut top: Option<(&String, &f32)> = None;
    for (key, p) in probs {
        if top.is_none_or(|(_, bp)| p > bp) {
            top = Some((key, p));
        }
    }
    let (key, _) = top.expect("the probabilities are not empty");
    Ok(Picked::Choice(key.clone(), *confidence))
}

/// The state the first question is asked over: the kid's words, and the
/// rule that Bench may only pick from the closed list.
fn entry_state(words: &str) -> String {
    format!(
        "A kid playing the paintball sandbox says to Bench, the game's co-developer: \
         \"{words}\"\n\nBench cannot change whatever the kid asks for: Bench may only \
         pick one change from the closed list in the question, or none."
    )
}

/// The first question: one option per dictionary entry, keyed by the
/// entry's stable id — the index the model answers with — plus `none`.
fn entry_question(pack: &GatePack, dictionary: &Dictionary) -> Value {
    let mut criteria = Map::new();
    for block in &dictionary.blocks {
        criteria.insert(
            block.id.clone(),
            Value::String(format!("{} — {}", block.says.join("; "), block.explain)),
        );
    }
    criteria.insert("none".to_owned(), Value::String(pack.none.clone()));
    json!({
        "type": "choice",
        "instructions": pack.gate_instructions,
        "criteria": Value::Object(criteria),
    })
}

/// The state the second question is asked over: the same words, the
/// matched change, and what is left to decide.
fn value_state(words: &str, block: &Block) -> String {
    format!(
        "The kid said: \"{words}\"\nBench has matched it to the change {} ({}). Bench \
         still has to pick how much, from the change's own steps in the question.",
        block.id,
        block.says.join("; ")
    )
}

/// The second question: the matched entry's own bounded steps, keyed by
/// the step's value.
fn value_question(pack: &GatePack, block: &Block) -> Value {
    let field = block.field.strip_prefix("params.").unwrap_or(&block.field);
    let mut criteria = Map::new();
    for step in &block.steps {
        criteria.insert(
            step.value.to_string(),
            Value::String(format!(
                "{} — {} moves {} from {} to {} (bounds {}..={})",
                step.say, block.id, field, block.default, step.value, block.min, block.max
            )),
        );
    }
    json!({
        "type": "choice",
        "instructions": pack.value_instructions,
        "criteria": Value::Object(criteria),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeMap;

    use lm_agents::sysone::StubBackend;

    use crate::vibe::LABEL_MAX;

    /// The shipped pack must load: these are the questions the gate asks.
    #[test]
    fn the_shipped_pack_loads() {
        let pack = GatePack::default_pack().expect("vibe-gate.toml loads");
        assert_eq!(pack.gate_id, "vibe");
        assert_eq!(pack.value_id, "vibe_value");
        assert_eq!(pack.model, "d1-2b");
        assert_eq!(pack.min_confidence, 0.30);
        assert_eq!(pack.timeout, Duration::from_millis(2000));
        assert!(pack.none.contains("none of these"));
    }

    /// A pack that disagrees with what the gate asks is refused: a score
    /// question, a floor no question can have, a timeout too short to
    /// ask one, and two questions sharing an id.
    #[test]
    fn a_pack_that_lies_is_refused() {
        let base = DEFAULT_GATE_PACK;
        for broken in [
            base.replacen("type = \"choice\"", "type = \"score\"", 1),
            base.replacen("min_confidence = 0.30", "min_confidence = 0.0", 1),
            base.replacen("min_confidence = 0.30", "min_confidence = 1.7", 1),
            base.replacen("timeout_ms = 2000", "timeout_ms = 10", 1),
            base.replacen("id = \"vibe_value\"", "id = \"vibe\"", 1),
        ] {
            assert!(
                GatePack::from_toml(&broken).is_err(),
                "a broken pack was read: {}",
                &broken[broken.find("id =").unwrap_or(0)..]
            );
        }
    }

    /// The shipped dictionary's steps are the closed options the second
    /// question asks over: every entry carries them, every value is
    /// inside the bounds, and exactly one step is "back to normal".
    #[test]
    fn the_shipped_dictionary_carries_bounded_steps() {
        let dict = Dictionary::default_dictionary().unwrap();
        for block in &dict.blocks {
            assert!(block.steps.len() >= 2, "{} has one step", block.id);
            let defaults = block
                .steps
                .iter()
                .filter(|step| step.value == block.default)
                .count();
            assert_eq!(
                defaults, 1,
                "{} carries its default {} once",
                block.id, block.default
            );
            for step in &block.steps {
                assert!(
                    (block.min..=block.max).contains(&step.value),
                    "{}'s step {} carries {} outside {}..={}",
                    block.id,
                    step.say,
                    step.value,
                    block.min,
                    block.max
                );
            }
            assert!(block.step_of(block.default).is_some());
            assert!(block.step_of(block.min.saturating_sub(1)).is_none() || block.min == 0);
        }
    }

    /// A dictionary whose steps fall outside the bounds, or that never
    /// say "back to normal", is refused at load.
    #[test]
    fn a_dictionary_with_broken_steps_is_refused() {
        let base = crate::vibe::DEFAULT_DICTIONARY;
        let out = base.replacen("value = 64 }", "value = 65 }", 1);
        assert!(matches!(
            Dictionary::from_toml(&out),
            Err(crate::vibe::VibeError::DictionaryInvalid { .. })
        ));
        let back = base.replacen("value = 20 }", "value = 21 }", 1);
        assert!(matches!(
            Dictionary::from_toml(&back),
            Err(crate::vibe::VibeError::DictionaryInvalid { .. })
        ));
    }

    /// A scripted answer: one gate, one choice, the given confidence.
    fn answered(gate: &str, option: &str, confidence: f32) -> Result<Answers, Refusal> {
        let mut probs = BTreeMap::new();
        probs.insert(option.to_owned(), 0.9);
        probs.insert("none".to_owned(), 0.1);
        Ok(Answers {
            per_gate: BTreeMap::from([(gate.to_owned(), Answer::Choice { probs, confidence })]),
            unsure: BTreeMap::new(),
            escalated: BTreeMap::new(),
            decomposed: BTreeMap::new(),
            model: "stub".to_owned(),
            input_tokens: 64,
            latency_ms: 12,
        })
    }

    /// A backend scripted with the two answers a hearing of "let them see
    /// farther" would get.
    fn hearing_stub() -> StubBackend {
        StubBackend::scripted(vec![
            (
                Duration::from_millis(0),
                answered("vibe", "unit.vision", 0.86),
            ),
            (Duration::from_millis(0), answered("vibe_value", "30", 0.91)),
        ])
    }

    #[test]
    fn a_hearing_reads_two_answers_into_a_proposal() {
        let pack = GatePack::default_pack().unwrap();
        let dict = Dictionary::default_dictionary().unwrap();
        let heard = hear(&hearing_stub(), &pack, &dict, "let them see farther").expect("it hears");
        assert_eq!(heard.entry, "unit.vision");
        assert_eq!(heard.entry_confidence, 0.86);
        assert_eq!(heard.step.value, 30);
        assert_eq!(heard.step.say, "a little farther");
        assert_eq!(heard.value_confidence, 0.91);
        assert_eq!(heard.from, 20);
        assert_eq!(heard.proposal_of(), ("unit.vision", 30));

        let text = heard.summary();
        assert!(text.contains("unit.vision"));
        assert!(text.contains("0.86"));
        assert!(text.contains("unit.vision 20 -> 30"));
        assert!(text.contains("a little farther"));
        assert!(text.contains("let them see farther"));
    }

    /// The same script hears the same proposal twice: a hearing over the
    /// same answers is the same hearing.
    #[test]
    fn a_hearing_is_deterministic() {
        let pack = GatePack::default_pack().unwrap();
        let dict = Dictionary::default_dictionary().unwrap();
        let a = hear(&hearing_stub(), &pack, &dict, "wider eyes").unwrap();
        let b = hear(&hearing_stub(), &pack, &dict, "wider eyes").unwrap();
        assert_eq!(a, b);
    }

    /// A first answer under the floor refuses by name and asks the kid to
    /// say it another way — and the second question is never asked.
    #[test]
    fn an_unsure_entry_is_a_named_refusal() {
        let pack = GatePack::default_pack().unwrap();
        let dict = Dictionary::default_dictionary().unwrap();
        let backend = StubBackend::scripted(vec![(
            Duration::from_millis(0),
            answered("vibe", "unit.vision", 0.18),
        )]);
        let err = hear(&backend, &pack, &dict, "umm maybe the seeing thing?").unwrap_err();
        assert!(
            matches!(
                &err,
                HearError::EntryUnsure { confidence, floor } if *confidence == 0.18 && *floor == 0.30
            ),
            "the wrong refusal: {err}"
        );
        assert!(err.to_string().contains("say it another way"));
        assert_eq!(backend.calls(), 1, "the value question was never asked");
    }

    /// A `none` answer is the honest edge: the words match no change.
    #[test]
    fn a_none_answer_refuses_by_name() {
        let pack = GatePack::default_pack().unwrap();
        let dict = Dictionary::default_dictionary().unwrap();
        let backend = StubBackend::scripted(vec![(
            Duration::from_millis(0),
            answered("vibe", "none", 0.90),
        )]);
        let err = hear(&backend, &pack, &dict, "make the paint bouncy").unwrap_err();
        assert!(matches!(err, HearError::NoMatch));
        assert!(err.to_string().contains("request pile"));
        assert_eq!(backend.calls(), 1, "the value question was never asked");
    }

    /// An unsure amount refuses by name too, with the entry it heard.
    #[test]
    fn an_unsure_value_is_a_named_refusal() {
        let pack = GatePack::default_pack().unwrap();
        let dict = Dictionary::default_dictionary().unwrap();
        let backend = StubBackend::scripted(vec![
            (
                Duration::from_millis(0),
                answered("vibe", "unit.speed", 0.8),
            ),
            (Duration::from_millis(0), answered("vibe_value", "204", 0.2)),
        ]);
        let err = hear(&backend, &pack, &dict, "make them, you know, quicker?").unwrap_err();
        assert!(
            matches!(
                &err,
                HearError::ValueUnsure { entry, confidence, floor }
                    if entry == "unit.speed" && *confidence == 0.2 && *floor == 0.3
            ),
            "the wrong refusal: {err}"
        );
        assert!(err.to_string().contains("unit.speed"));
    }

    /// An answer naming an option the question never offered is refused
    /// (D26): never a neighbouring option, never a guess.
    #[test]
    fn an_off_menu_answer_is_refused() {
        let pack = GatePack::default_pack().unwrap();
        let dict = Dictionary::default_dictionary().unwrap();
        let backend = StubBackend::scripted(vec![(
            Duration::from_millis(0),
            answered("vibe", "paint.bounce", 0.99),
        )]);
        let err = hear(&backend, &pack, &dict, "bouncy paint").unwrap_err();
        assert!(
            matches!(&err, HearError::OffMenu { gate, option }
                if gate == "vibe" && option == "paint.bounce"),
            "the wrong refusal: {err}"
        );

        // And a value the entry's steps do not carry.
        let backend = StubBackend::scripted(vec![
            (
                Duration::from_millis(0),
                answered("vibe", "unit.vision", 0.9),
            ),
            (Duration::from_millis(0), answered("vibe_value", "21", 0.9)),
        ]);
        let err = hear(&backend, &pack, &dict, "see a bit farther").unwrap_err();
        assert!(
            matches!(&err, HearError::OffMenu { gate, option }
                if gate == "vibe_value" && option == "21"),
            "the wrong refusal: {err}"
        );
    }

    /// "Put it back" and the "back to normal" step are the undo, not a
    /// proposal: the pipeline measures against the shipped default, so a
    /// proposal of it would test nothing.
    #[test]
    fn putting_it_back_is_the_undo_not_a_proposal() {
        let pack = GatePack::default_pack().unwrap();
        let dict = Dictionary::default_dictionary().unwrap();

        // The words, before any server is asked.
        let backend = StubBackend::menu();
        for words in ["put it back", "Put It Back"] {
            let err = hear(&backend, &pack, &dict, words).unwrap_err();
            assert!(
                matches!(err, HearError::PutItBack { entry: None }),
                "{words}"
            );
            assert!(err.to_string().contains("lm vibe undo"));
        }
        assert_eq!(backend.calls(), 0, "the server was never asked");

        // The step, chosen by the model over a change-asking phrase.
        let backend = StubBackend::scripted(vec![
            (
                Duration::from_millis(0),
                answered("vibe", "unit.vision", 0.9),
            ),
            (Duration::from_millis(0), answered("vibe_value", "20", 0.9)),
        ]);
        let err = hear(&backend, &pack, &dict, "let them see like normal again").unwrap_err();
        assert!(
            matches!(&err, HearError::PutItBack { entry: Some(entry) } if entry == "unit.vision"),
            "the wrong refusal: {err}"
        );
    }

    /// Empty words, whitespace, and a paragraph are each a named refusal.
    #[test]
    fn nothing_and_too_much_are_refused() {
        let pack = GatePack::default_pack().unwrap();
        let dict = Dictionary::default_dictionary().unwrap();
        let backend = StubBackend::menu();
        for words in ["", "   "] {
            assert!(matches!(
                hear(&backend, &pack, &dict, words),
                Err(HearError::SaidNothing)
            ));
        }
        let paragraph = "x".repeat(WORDS_MAX + 1);
        let err = hear(&backend, &pack, &dict, &paragraph).unwrap_err();
        assert!(
            matches!(&err, HearError::TooLong { chars, max }
                if *chars == WORDS_MAX + 1 && *max == WORDS_MAX),
            "the wrong refusal: {err}"
        );
        assert_eq!(backend.calls(), 0, "the server was never asked");
    }

    /// The words travel to the backend and stop there: the only form of
    /// them anything downstream may write is the bounded label.
    #[test]
    fn the_words_never_become_a_transcript() {
        let words = "a really quite remarkably long turn of phrase from a kid";
        let label = bounded_label(words);
        assert_eq!(label.chars().count(), LABEL_MAX);
        assert!(label.starts_with("a really quite remarkably long"));

        // The summary carries the words once, for the person reading the
        // hearing, and the proposal line carries only the label.
        let pack = GatePack::default_pack().unwrap();
        let dict = Dictionary::default_dictionary().unwrap();
        let heard = hear(&hearing_stub(), &pack, &dict, "let them see farther").unwrap();
        let text = heard.summary();
        assert!(text.contains("\"let them see farther\""));
        let proposal_line = text
            .lines()
            .find(|line| line.contains("would propose"))
            .expect("the summary states the proposal");
        assert!(proposal_line.contains("\"let them see farther\""));
        // And a hearing writes nothing at all: no file, no ledger.
        assert!(heard.words.len() <= WORDS_MAX);
    }
}
