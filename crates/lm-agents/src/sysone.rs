//! Task 3: the System One client — the decision models behind one trait.
//!
//! Three backends answer the same question pack: the Thor's local decision
//! models (LAN, unauthenticated), the cloud Jev provider (a bearer key) and a
//! scripted stub for the tests. Every call carries a deadline, every failure
//! is a [`Refusal`], and no failure ever invents an answer (D26).
//!
//! The protocol is the same everywhere:
//!
//! ```text
//! POST {base}/v1/systemone      {"model": …, "state": …, "questions": {…}}
//! -> 200                         {"model": …, "answers": {…}, "usage": {…}}
//! ```
//!
//! Every answer is typed: `noul` (the probability of "yes"), `choice` (the
//! probability of each option) or `score` (a probability-weighted level). A
//! `choice` or a `score` may arrive without a confidence, in which case it is
//! derived from the probabilities as the largest normalised share — never as
//! 1.0 (Thor handoff §5.3).
//!
//! Connections are kept alive: one [`ureq::Agent`] per backend, reused for
//! every call, so a round of thinking does not pay for a TCP handshake per
//! question pack (D50).
//!
//! The Jev key lives in `~/.config/last-mile/secrets.env` and is only ever
//! sent to `api.typesafe.ai`: it is never logged, never printed and never
//! attached to a Thor call.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// The path every backend posts to.
pub const SYSTEMONE_PATH: &str = "/v1/systemone";

/// The cloud provider's base URL. The Jev key only ever goes here.
pub const JEV_BASE: &str = "https://api.typesafe.ai";

/// The model the cloud provider serves by default.
pub const JEV_MODEL: &str = "jev-latest";

/// The confidence floor for a Thor backend, in the spirit of the
/// `thor-d1-2b` default in `AgentsConfig`.
///
/// Every backend the round builds carries its configured floor
/// (`AgentsConfig::min_confidence`); this is only the bare constructor's
/// fallback.
pub const THOR_MIN_CONFIDENCE: f32 = 0.30;

/// The confidence floor for the cloud backend, in the spirit of the `jev`
/// default in `AgentsConfig`.
pub const JEV_MIN_CONFIDENCE: f32 = 0.5;

/// The per-request timeout, in the spirit of `AgentsConfig::request_timeout_ms`.
pub const DEFAULT_REQUEST_TIMEOUT_MS: u64 = 400;

/// The fewest levels a `score` question may hold (the API's rule).
///
/// The API refuses a score question whose criteria is not an array of 2
/// to 10 levels: "questions.intention: \"criteria\" must be an array of 2
/// to 10 levels".
pub const MIN_SCORE_LEVELS: usize = 2;

/// The most levels a `score` question may hold (the API's rule).
pub const MAX_SCORE_LEVELS: usize = 10;

// ---------------------------------------------------------------- the contract

/// A decision model that answers typed questions about a situation.
///
/// The trait is the whole contract between the cognition loop and a model:
/// the loop hands over plain-language facts and a pack of typed questions and
/// gets back typed answers — or a [`Refusal`], which is an outcome and not an
/// error (D26). Nothing here is needed to replay a round: the answers reach
/// the sim as logged decisions.
pub trait SysOne {
    /// Ask the model about `state`.
    ///
    /// `questions` is the pack the gate set built. `deadline` is the instant
    /// after which the answer is worthless: the client never waits past it,
    /// and an answer that arrives later is dropped by the sequencer.
    fn ask(&self, state: &str, questions: &Value, deadline: Instant) -> Result<Answers, Refusal>;

    /// The backend's name: `thor-<model>` for the Thor, `jev` for the cloud.
    ///
    /// It is the key `AgentsConfig` uses for the backend's confidence
    /// threshold, and it is what the log calls this model.
    fn name(&self) -> &str;
}

/// What a model answered.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Answers {
    /// One answer per gate, keyed by the gate id.
    ///
    /// A gate the backend is not sure about is not in here: it is in
    /// [`Answers::unsure`] instead, and it makes no decision of its own
    /// (D26).
    pub per_gate: BTreeMap<String, Answer>,
    /// The gates that were escalated to a second model (D71), by gate id.
    ///
    /// The escalation is a second opinion, not a second answer: the gate's
    /// answer in [`Answers::per_gate`] is the one that was taken, and this
    /// says how it came to be taken.
    #[serde(default)]
    pub escalated: BTreeMap<String, Escalation>,
    /// The gates the backend is not sure about, with their confidence.
    ///
    /// The confidence rule is per gate, not per pack: an unsure gate never
    /// takes another gate's decision with it. A gate that chooses a line
    /// that the model was unsure about silences the line and leaves the
    /// move alone; a gate that chooses a move the model was unsure about
    /// refuses the move and leaves the line alone.
    #[serde(default)]
    pub unsure: BTreeMap<String, f32>,
    /// The model that answered: the id this backend asked for.
    pub model: String,
    /// The tokens the pack cost.
    pub input_tokens: u32,
    /// The round trip, in milliseconds.
    pub latency_ms: u32,
}

/// One gate's answer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Answer {
    /// A choice gate: the probability of every option and the confidence.
    Choice {
        /// The probability of each option, keyed by the option key.
        probs: BTreeMap<String, f32>,
        /// How sure the model was; derived when it did not say.
        confidence: f32,
    },
    /// A yes/no gate: the model's probability that the answer is "yes".
    Noul(f32),
    /// A score gate: the weighted level, the level probabilities and the
    /// confidence.
    Score {
        /// The probability of each level, keyed by the level key.
        probs: BTreeMap<String, f32>,
        /// The probability-weighted level.
        score: f32,
        /// How sure the model was; derived when it did not say.
        confidence: f32,
    },
}

/// One escalation of an unsure gate to a second model (D71).
///
/// The cheap model answers first. A gate it is not sure about is asked of
/// the second model **alone** — one question in one call, never the whole
/// pack — and the second opinion is taken when it is sure (D71). This is
/// the record of that: the gate, both confidences and the latency.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Escalation {
    /// The gate that was escalated.
    pub gate: String,
    /// The cheap model's confidence on the gate.
    pub primary_confidence: f32,
    /// The option the cheap model's own distribution favoured.
    ///
    /// The second opinion may change the answer or keep it, and the two
    /// choices are the difference between the two: an escalation that never
    /// changes the answer buys latency and nothing else.
    #[serde(default)]
    pub primary_choice: Option<String>,
    /// The second model's confidence, when it answered.
    pub second_confidence: Option<f32>,
    /// The option the second model's distribution favoured, when it answered.
    #[serde(default)]
    pub second_choice: Option<String>,
    /// The second model's round trip, in milliseconds.
    pub latency_ms: u32,
    /// What the second opinion came to.
    pub outcome: EscalationOutcome,
}

/// What the second opinion came to (D71).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EscalationOutcome {
    /// The second opinion was taken: the gate has the second model's answer.
    Taken,
    /// The second model was not sure either: the gate stays unsure.
    Unsure,
    /// The second model did not answer in time.
    Timeout,
    /// The second model could not be reached.
    Unavailable,
}

impl fmt::Display for EscalationOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EscalationOutcome::Taken => f.write_str("taken"),
            EscalationOutcome::Unsure => f.write_str("the second model was unsure too"),
            EscalationOutcome::Timeout => f.write_str("the second model did not answer in time"),
            EscalationOutcome::Unavailable => f.write_str("the second model could not be reached"),
        }
    }
}

/// Why there is no answer.
///
/// A refusal is a logged outcome, not a bug: the reflex keeps playing and no
/// default answer is ever invented (D26).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Refusal {
    /// The call did not come back before the deadline.
    Timeout,
    /// The endpoint answered with a status that is not 2xx.
    ///
    /// `0` means the call never reached an HTTP status at all: DNS, connect,
    /// TLS or protocol failure.
    Http(u16),
    /// The answer did not match the schema.
    Malformed(String),
    /// The pack does not fit the API's schema.
    ///
    /// The pack is built by the gates and checked before it is sent, so
    /// this never reaches a model: a question that breaks the API's rules
    /// is a bug in the pack, not a refusal by the model.
    InvalidPack(String),
    /// The answer was less confident than this backend's threshold.
    ///
    /// The rule is per gate: the gate that was unsure makes no decision of
    /// its own, and every unsure gate is a refusal of its own (D26).
    LowConfidence(f32),
    /// The menu changed between the question and the answer.
    MenuChanged,
    /// The agent's budget for this round is spent.
    Budget,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Timeout => f.write_str("the model did not answer before the deadline"),
            Refusal::Http(0) => f.write_str("the call never reached an HTTP status"),
            Refusal::Http(code) => write!(f, "the endpoint answered {code}"),
            Refusal::Malformed(why) => write!(f, "the answer did not match the schema: {why}"),
            Refusal::InvalidPack(why) => {
                write!(f, "the pack does not fit the API's schema: {why}")
            }
            Refusal::LowConfidence(c) => write!(f, "the answer was only {c} confident"),
            Refusal::MenuChanged => f.write_str("the menu changed under the question"),
            Refusal::Budget => f.write_str("the budget is spent"),
        }
    }
}

/// Why a pack does not fit the API's schema.
///
/// The message is the API's own shape — `questions.<gate>: <rule>` — so a
/// pack that breaks a rule reads the same here as it would in a 400 from
/// the Thor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackError {
    /// The gate that broke the rule.
    pub gate: String,
    /// What the rule is.
    pub why: String,
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "questions.{}: {}", self.gate, self.why)
    }
}

impl std::error::Error for PackError {}

/// Check a pack against the System One API's question schema.
///
/// The API refuses the **whole pack** when one question does not fit, so a
/// pack that breaks the rules is caught here, where it was built, instead
/// of turning into a 400 on the Thor and a round with no decisions.
///
/// The rules:
/// - a `noul` question has no criteria;
/// - a `choice` question's criteria is a map of option key -> what the
///   option means;
/// - a `score` question's criteria is an **array of 2 to 10** level
///   descriptions, lowest score first — the API's own words are
///   `"criteria" must be an array of 2 to 10 levels`.
pub fn validate_pack(questions: &Value) -> Result<(), PackError> {
    let Some(gates) = questions.as_object() else {
        return Err(PackError {
            gate: String::new(),
            why: "the pack is not a table of questions".to_string(),
        });
    };
    for (id, question) in gates {
        let kind = question
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("choice");
        match kind {
            "noul" => {
                if question.get("criteria").is_some() {
                    return Err(PackError {
                        gate: id.clone(),
                        why: "a noul question has no criteria".to_string(),
                    });
                }
            }
            "choice" => {
                let Some(criteria) = question.get("criteria").and_then(Value::as_object) else {
                    return Err(PackError {
                        gate: id.clone(),
                        why: "\"criteria\" must be a map of options".to_string(),
                    });
                };
                if criteria.is_empty() {
                    return Err(PackError {
                        gate: id.clone(),
                        why: "\"criteria\" must name at least one option".to_string(),
                    });
                }
            }
            "score" => {
                let Some(criteria) = question.get("criteria").and_then(Value::as_array) else {
                    return Err(PackError {
                        gate: id.clone(),
                        why: "\"criteria\" must be an array of 2 to 10 levels".to_string(),
                    });
                };
                let levels = MIN_SCORE_LEVELS..=MAX_SCORE_LEVELS;
                if !levels.contains(&criteria.len()) {
                    return Err(PackError {
                        gate: id.clone(),
                        why: "\"criteria\" must be an array of 2 to 10 levels".to_string(),
                    });
                }
                if !criteria.iter().all(Value::is_string) {
                    return Err(PackError {
                        gate: id.clone(),
                        why: "\"criteria\" must be an array of 2 to 10 levels".to_string(),
                    });
                }
            }
            other => {
                return Err(PackError {
                    gate: id.clone(),
                    why: format!("unknown question type {other}"),
                });
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- the backends

/// One of the Thor's local decision models.
///
/// The Thor is on the LAN and unauthenticated, so no key is ever attached to
/// these calls: the Jev key never leaves `api.typesafe.ai`.
pub struct ThorBackend {
    agent: ureq::Agent,
    base: String,
    port: u16,
    model: String,
    name: String,
    min_confidence: f32,
    timeout: Duration,
}

impl ThorBackend {
    /// A backend for the model `model` on `base:port`.
    ///
    /// `base` is a scheme and a host (`http://192.168.0.78`); the port is
    /// separate because the Thor serves one model per port.
    pub fn new(base: &str, port: u16, model: &str) -> Self {
        let timeout = Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS);
        Self {
            agent: build_agent(timeout),
            base: base.trim_end_matches('/').to_string(),
            port,
            model: model.to_string(),
            name: format!("thor-{}", model.to_lowercase()),
            min_confidence: THOR_MIN_CONFIDENCE,
            timeout,
        }
    }

    /// Set the confidence floor: an answer below it is refused.
    ///
    /// The default is [`THOR_MIN_CONFIDENCE`]; the configured value belongs
    /// in `AgentsConfig`, so pass that one in.
    pub fn with_min_confidence(mut self, min: f32) -> Self {
        self.min_confidence = min;
        self
    }

    /// Set the per-request timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.agent = build_agent(timeout);
        self.timeout = timeout;
        self
    }

    /// The URL this backend posts to.
    pub fn url(&self) -> String {
        format!("{}:{}{}", self.base, self.port, SYSTEMONE_PATH)
    }

    /// The model this backend asks for.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Ignore the environment's proxy settings.
    ///
    /// The Thor is on the LAN: a proxy in the environment would only break
    /// the route to it.
    #[cfg(test)]
    fn without_proxy(mut self) -> Self {
        self.agent = build_agent_without_proxy(self.timeout);
        self
    }
}

impl fmt::Debug for ThorBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ThorBackend")
            .field("url", &self.url())
            .field("model", &self.model)
            .field("min_confidence", &self.min_confidence)
            .finish()
    }
}

impl SysOne for ThorBackend {
    fn ask(&self, state: &str, questions: &Value, deadline: Instant) -> Result<Answers, Refusal> {
        // The pack is checked before it is sent: the API refuses a whole
        // pack when one question does not fit, so a pack that breaks the
        // rules is named here instead of turning into a 400 on the Thor.
        validate_pack(questions).map_err(|e| Refusal::InvalidPack(e.to_string()))?;
        let body = request_body(&self.model, state, questions);
        let (text, latency_ms) = post_pack(&self.agent, &self.url(), None, &body, deadline)?;
        let mut answers = parse_answers(&text, &self.model, latency_ms)?;
        check_answered(&answers, questions)?;
        split_unsure(&mut answers, self.min_confidence);
        Ok(answers)
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// A backend that asks a second model when the first one is unsure (D71).
///
/// The cheap model answers the pack. Every gate it is not sure about —
/// below `escalate_below` — is then asked of the second model **on its
/// own**, one question in one call, and the second opinion is taken when
/// the second model is sure of it.
///
/// The second model's pack is one question, never the whole pack:
/// clef-flash's batch is 2048 tokens and the full eleven-question pack is
/// 2317 (D71), so a pack that does not fit would be refused outright.
///
/// The escalation is bounded by the deadline of the call it belongs to: a
/// second opinion that cannot arrive in time is no opinion at all. A gate
/// whose second opinion fails stays unsure: the cheap model was not
/// trusted on its own, and the second model did not take its place (D26).
pub struct EscalatingBackend {
    /// The cheap model, which answers first.
    primary: Box<dyn SysOne + Send + Sync>,
    /// The second model: asked when the first one is unsure.
    second: Box<dyn SysOne + Send + Sync>,
    /// Below this confidence the cheap answer is not acted on alone.
    escalate_below: f32,
    /// The gates that may be escalated: `None` is every gate below the
    /// threshold (D71 as it shipped); `Some(set)` is only the named gates,
    /// and an empty set is none of them — the escalation off.
    ///
    /// Which gates are worth a second opinion is a harness question (the
    /// gates are the harness's), so the set comes from `policy.toml`
    /// (`escalate_gates`); the threshold stays with the model's calibration
    /// (`AgentsConfig::escalate_below`).
    gates: Option<BTreeSet<String>>,
    /// The escalations so far, for the telemetry.
    escalations: Mutex<Vec<Escalation>>,
}

impl EscalatingBackend {
    /// A backend that asks `second` when `primary` is not sure.
    ///
    /// The threshold is the cheap model's own: a gate it answers below
    /// `escalate_below` is not acted on alone. Every gate below the
    /// threshold may be escalated; narrow it with [`EscalatingBackend::with_gates`].
    pub fn new(
        primary: Box<dyn SysOne + Send + Sync>,
        second: Box<dyn SysOne + Send + Sync>,
        escalate_below: f32,
    ) -> Self {
        Self {
            primary,
            second,
            escalate_below,
            gates: None,
            escalations: Mutex::new(Vec::new()),
        }
    }

    /// Set the gates that may be escalated.
    ///
    /// `None` is every gate below the threshold; `Some(set)` is only the
    /// named gates, and an empty set turns the escalation off.
    pub fn with_gates(mut self, gates: Option<BTreeSet<String>>) -> Self {
        self.gates = gates;
        self
    }

    /// The threshold below which a gate is escalated.
    pub fn escalate_below(&self) -> f32 {
        self.escalate_below
    }

    /// The escalations so far, oldest first.
    pub fn escalations(&self) -> Vec<Escalation> {
        self.escalations
            .lock()
            .expect("the escalations are not poisoned")
            .clone()
    }

    /// How many gates were escalated.
    pub fn escalation_count(&self) -> usize {
        self.escalations
            .lock()
            .expect("the escalations are not poisoned")
            .len()
    }
}

impl fmt::Debug for EscalatingBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EscalatingBackend")
            .field("primary", &self.primary.name())
            .field("second", &self.second.name())
            .field("escalate_below", &self.escalate_below)
            .finish()
    }
}

impl SysOne for EscalatingBackend {
    fn ask(&self, state: &str, questions: &Value, deadline: Instant) -> Result<Answers, Refusal> {
        let mut answers = self.primary.ask(state, questions, deadline)?;
        // The gates the cheap model was not sure about: their own numbers
        // are in `per_gate` (above its floor) or in `unsure` (below it).
        for gate in gates_below(&answers, self.escalate_below) {
            // Only the gates the harness named: a gate outside the set keeps
            // the cheap model's answer (or its unsure) as it is.
            if let Some(gates) = &self.gates {
                if !gates.contains(&gate) {
                    continue;
                }
            }
            let primary_confidence = confidence_of(&answers, &gate);
            let primary_choice = answers
                .per_gate
                .get(&gate)
                .and_then(|answer| top_option(answer).cloned());
            // The second model is asked this gate **alone**: one question in
            // one call, so its pack always fits.
            let one = one_question(questions, &gate);
            let started = Instant::now();
            let (second_answer, outcome) = match self.second.ask(state, &one, deadline) {
                Ok(second) => {
                    let answer = second.per_gate.get(&gate).cloned();
                    let outcome = if answer.is_some() {
                        EscalationOutcome::Taken
                    } else {
                        EscalationOutcome::Unsure
                    };
                    (answer, outcome)
                }
                Err(Refusal::Timeout) => (None, EscalationOutcome::Timeout),
                Err(_) => (None, EscalationOutcome::Unavailable),
            };
            let latency_ms = ms(started.elapsed());
            let second_confidence = second_answer.as_ref().and_then(answer_confidence);
            let second_choice = second_answer.as_ref().and_then(|a| top_option(a).cloned());
            // The second opinion is the gate's answer, or the gate stays
            // unsure: the cheap model's own answer was not trusted alone.
            match second_answer {
                Some(answer) => {
                    answers.per_gate.insert(gate.clone(), answer);
                    answers.unsure.remove(&gate);
                }
                None => {
                    answers.per_gate.remove(&gate);
                    answers.unsure.insert(gate.clone(), primary_confidence);
                }
            }
            let escalation = Escalation {
                gate: gate.clone(),
                primary_confidence,
                primary_choice,
                second_confidence,
                second_choice,
                latency_ms,
                outcome,
            };
            answers.escalated.insert(gate, escalation.clone());
            self.escalations
                .lock()
                .expect("the escalations are not poisoned")
                .push(escalation);
        }
        Ok(answers)
    }

    fn name(&self) -> &str {
        self.primary.name()
    }
}

/// The confidence of a gate's answer, if it has one.
///
/// A noul has no confidence: its value is the belief itself, and the
/// presence floor (not the confidence floor) decides whether it is
/// silence (D53).
fn answer_confidence(answer: &Answer) -> Option<f32> {
    match answer {
        Answer::Choice { confidence, .. } | Answer::Score { confidence, .. } => Some(*confidence),
        Answer::Noul(_) => None,
    }
}

/// The confidence of a gate in an answer set: whether it was answered or
/// found unsure.
fn confidence_of(answers: &Answers, gate: &str) -> f32 {
    if let Some(answer) = answers.per_gate.get(gate) {
        return answer_confidence(answer).unwrap_or(0.0);
    }
    answers.unsure.get(gate).copied().unwrap_or(0.0)
}

/// The option a distribution favoured: the key of its largest probability.
///
/// A tie goes to the first key in map order, so the same answer always names
/// the same option. A noul has no options: `None`.
fn top_option(answer: &Answer) -> Option<&String> {
    let probs = match answer {
        Answer::Choice { probs, .. } | Answer::Score { probs, .. } => probs,
        Answer::Noul(_) => return None,
    };
    let mut best: Option<(&String, &f32)> = None;
    for (key, p) in probs {
        if best.is_none_or(|(_, bp)| p > bp) {
            best = Some((key, p));
        }
    }
    best.map(|(key, _)| key)
}

/// The gates whose confidence is below `threshold`.
fn gates_below(answers: &Answers, threshold: f32) -> Vec<String> {
    let mut gates: Vec<String> = answers
        .per_gate
        .iter()
        .filter_map(|(id, answer)| {
            let confidence = answer_confidence(answer)?;
            (confidence < threshold).then(|| id.clone())
        })
        .collect();
    gates.extend(
        answers
            .unsure
            .iter()
            .filter(|(_, confidence)| **confidence < threshold)
            .map(|(id, _)| id.clone()),
    );
    gates.sort();
    gates.dedup();
    gates
}

/// The pack of one question: the second model is asked this gate alone.
fn one_question(questions: &Value, gate: &str) -> Value {
    let mut one = Map::new();
    if let Some(gates) = questions.as_object() {
        if let Some(question) = gates.get(gate) {
            one.insert(gate.to_owned(), question.clone());
        }
    }
    Value::Object(one)
}

/// Wrap the backends so that a gate the cheap model is unsure about is
/// asked of the second model (D71).
///
/// `second` builds the second model for a primary backend's name, and is
/// called once per wrapped backend: each gets a connection of its own
/// (D50). A backend that has no second model is left as it is.
///
/// `gates` is the set of gates that may be escalated (the harness's
/// `escalate_gates`): `None` is every gate below the threshold, and an
/// empty set turns the escalation off.
pub fn with_escalation(
    backends: BTreeMap<String, Box<dyn SysOne + Send + Sync>>,
    escalate_below: f32,
    escalate_backend: &str,
    gates: Option<BTreeSet<String>>,
    second: impl Fn(&str) -> Option<Box<dyn SysOne + Send + Sync>>,
) -> BTreeMap<String, Box<dyn SysOne + Send + Sync>> {
    let mut out = BTreeMap::new();
    for (name, backend) in backends {
        if name == escalate_backend {
            // The second model does not escalate to itself.
            out.insert(name, backend);
            continue;
        }
        match second(&name) {
            Some(second) => {
                out.insert(
                    name,
                    Box::new(
                        EscalatingBackend::new(backend, second, escalate_below)
                            .with_gates(gates.clone()),
                    ) as Box<dyn SysOne + Send + Sync>,
                );
            }
            None => {
                out.insert(name, backend);
            }
        }
    }
    out
}

/// The cloud Jev backend (TypeSafe's System One API).
///
/// The key comes from `~/.config/last-mile/secrets.env` and is only ever
/// sent to `api.typesafe.ai`.
pub struct JevBackend {
    agent: ureq::Agent,
    base: String,
    model: String,
    api_key: Secret,
    min_confidence: f32,
    timeout: Duration,
}

impl JevBackend {
    /// A backend that reads its key from `~/.config/last-mile/secrets.env`.
    pub fn from_secrets() -> Result<Self, Refusal> {
        Self::from_secrets_file(default_secrets_path())
    }

    /// A backend that reads its key from `path`.
    ///
    /// The path is a parameter so the tests never touch the real secrets
    /// file.
    pub fn from_secrets_file(path: impl AsRef<Path>) -> Result<Self, Refusal> {
        let api_key = read_api_key(path.as_ref())?;
        Ok(Self {
            agent: build_agent(Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS)),
            base: JEV_BASE.to_string(),
            model: JEV_MODEL.to_string(),
            api_key: Secret(api_key),
            min_confidence: JEV_MIN_CONFIDENCE,
            timeout: Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS),
        })
    }

    /// Set the confidence floor: an answer below it is refused.
    pub fn with_min_confidence(mut self, min: f32) -> Self {
        self.min_confidence = min;
        self
    }

    /// Set the model to ask for.
    pub fn with_model(mut self, model: &str) -> Self {
        self.model = model.to_string();
        self
    }

    /// Set the per-request timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.agent = build_agent(timeout);
        self.timeout = timeout;
        self
    }

    /// The URL this backend posts to.
    pub fn url(&self) -> String {
        format!("{}{}", self.base, SYSTEMONE_PATH)
    }

    /// Point the backend at another base URL.
    ///
    /// Only the tests may do this: the key must only ever travel to
    /// `api.typesafe.ai`.
    #[cfg(test)]
    fn point_at(&mut self, base: &str) {
        self.base = base.trim_end_matches('/').to_string();
    }

    /// Ignore the environment's proxy settings.
    #[cfg(test)]
    fn without_proxy(mut self) -> Self {
        self.agent = build_agent_without_proxy(self.timeout);
        self
    }
}

impl fmt::Debug for JevBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JevBackend")
            .field("url", &self.url())
            .field("model", &self.model)
            .field("api_key", &self.api_key)
            .field("min_confidence", &self.min_confidence)
            .finish()
    }
}

impl SysOne for JevBackend {
    fn ask(&self, state: &str, questions: &Value, deadline: Instant) -> Result<Answers, Refusal> {
        // The pack is checked before it is sent: the API refuses a whole
        // pack when one question does not fit, so a pack that breaks the
        // rules is named here instead of turning into a 400 from the API.
        validate_pack(questions).map_err(|e| Refusal::InvalidPack(e.to_string()))?;
        let body = request_body(&self.model, state, questions);
        let (text, latency_ms) = post_pack(
            &self.agent,
            &self.url(),
            Some(&self.api_key.0),
            &body,
            deadline,
        )?;
        let mut answers = parse_answers(&text, &self.model, latency_ms)?;
        check_answered(&answers, questions)?;
        split_unsure(&mut answers, self.min_confidence);
        Ok(answers)
    }

    fn name(&self) -> &str {
        "jev"
    }
}

/// The scripted backend: the tests' fake model.
///
/// The script is consumed in order; the last scripted outcome repeats once
/// the script runs out, so a one-entry script means "always this".
///
/// A stub can also be built in *menu* mode ([`StubBackend::menu`]): it
/// answers whatever menu it was asked about, with one probability per
/// option. That is the stub a whole round needs, because the menu moves
/// under the question as the world moves — a fixed script would answer a
/// menu that is no longer there, and be refused (D26). The host's
/// `--agents stub` mode and the end-to-end test use it.
pub struct StubBackend {
    script: Mutex<StubScript>,
    calls: AtomicUsize,
    confidence: f32,
}

/// What a [`StubBackend`] answers with.
enum StubScript {
    /// A fixed script, consumed in order.
    Fixed {
        queue: VecDeque<(Duration, Result<Answers, Refusal>)>,
        last: Option<(Duration, Result<Answers, Refusal>)>,
    },
    /// Whatever menu the question carries.
    Menu,
}

impl StubBackend {
    /// A backend that answers with `script`, in order.
    pub fn scripted(script: Vec<(Duration, Result<Answers, Refusal>)>) -> Self {
        Self {
            script: Mutex::new(StubScript::Fixed {
                queue: script.into(),
                last: None,
            }),
            calls: AtomicUsize::new(0),
            confidence: 0.9,
        }
    }

    /// A backend that answers every gate it is asked about.
    ///
    /// The first option is the favourite and the rest share what is left, so
    /// the answer is over exactly the options of the question — never over
    /// a menu that has moved (D26). A noul is answered yes, a score with
    /// the middle of its scale.
    ///
    /// The stub stands in for a model, so it answers the whole pack: the
    /// gates, the policy and the ladder are the real ones, and only the
    /// model is not.
    pub fn menu() -> Self {
        Self {
            script: Mutex::new(StubScript::Menu),
            calls: AtomicUsize::new(0),
            confidence: 0.9,
        }
    }

    /// Set the confidence a menu answer reports.
    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence;
        self
    }

    /// How many times the stub was asked.
    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl fmt::Debug for StubBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StubBackend")
            .field("calls", &self.calls())
            .finish()
    }
}

impl SysOne for StubBackend {
    fn ask(&self, _state: &str, questions: &Value, deadline: Instant) -> Result<Answers, Refusal> {
        self.calls.fetch_add(1, Ordering::SeqCst);

        let entry = {
            let mut script = self
                .script
                .lock()
                .expect("the stub's script is not poisoned");
            match &mut *script {
                StubScript::Menu => {
                    // The menu of the question: the criteria of the move gate,
                    // which is the shape the real models are asked over.
                    let answers = menu_answer(questions, self.confidence)?;
                    drop(script);
                    return Ok(answers);
                }
                StubScript::Fixed { queue, last } => match queue.pop_front() {
                    Some(entry) => {
                        *last = Some(entry.clone());
                        Some(entry)
                    }
                    None => last.clone(),
                },
            }
        };
        let Some((delay, result)) = entry else {
            return Err(Refusal::Malformed(
                "the stub has nothing scripted".to_string(),
            ));
        };

        // A pack that cannot finish before the deadline is a timeout, not a
        // wait: the sim thread never blocks on a model.
        if Instant::now()
            .checked_add(delay)
            .is_none_or(|end| end > deadline)
        {
            return Err(Refusal::Timeout);
        }
        std::thread::sleep(delay);

        match result {
            Ok(mut answers) => {
                // The stub's latency is the latency it simulated.
                answers.latency_ms = ms(delay);
                Ok(answers)
            }
            Err(refusal) => Err(refusal),
        }
    }

    fn name(&self) -> &str {
        "stub"
    }
}

/// The service time of the throttled server, in milliseconds.
///
/// One request at a time and 110 ms for each, so the server answers about
/// nine requests a second: the Thor's d1-2B under load (decision-daw x5).
pub const THROTTLED_SERVICE_MS: u64 = 110;

/// How long the server waits for the client to register a request before it
/// calls the request unpaired.
///
/// The client registers a request as the mind makes it and the worker may
/// reach the server first, so the server waits for the registration. A
/// request that is never registered is a bug in the caller, not a slow
/// model, so the server says so instead of guessing a time.
const ARRIVAL_WAIT: Duration = Duration::from_secs(5);

/// A model server with a capacity: one request at a time.
///
/// The server serves one request at a time and takes [`THROTTLED_SERVICE_MS`]
/// for each, so it answers about nine requests a second — the Thor's d1-2B
/// under load (decision-daw x5). The requests wait in the server's queue in
/// the order they arrived, and the wait counts toward the latency the answer
/// reports: an answer that waited three slots reports three slots of
/// latency, not one.
///
/// The client hands the server each request as the mind makes it
/// ([`ThrottledStub::arrive`]). The thinker takes the requests one at a
/// time, so the server's queue is the queue the agents really waited in.
///
/// This is test support: it puts a round against a server with a capacity,
/// and it changes no shipped behaviour.
#[derive(Clone)]
pub struct ThrottledStub {
    inner: Arc<Inner>,
}

/// The state one throttled server shares between its handles.
struct Inner {
    /// The server: its queue and the instant it is next free.
    server: Mutex<Server>,
    /// How long one request takes to serve.
    service: Duration,
    /// How long an answer is worth after the request was made.
    timeout: Duration,
    /// How many requests the server has been asked.
    calls: AtomicU64,
    /// How many requests the server refused as too late.
    timeouts: AtomicU64,
    /// How many requests the server served without an arrival.
    unpaired: AtomicU64,
    /// The confidence a menu answer reports, as the bits of an `f32`.
    confidence: AtomicU32,
}

/// The server's queue and its clock.
struct Server {
    /// The instant the server is next free.
    free_at: Instant,
    /// The requests that have arrived and are waiting, in order.
    arrivals: VecDeque<Instant>,
}

impl Inner {
    /// The confidence a menu answer reports.
    fn confidence(&self) -> f32 {
        f32::from_bits(self.confidence.load(Ordering::SeqCst))
    }
}

impl ThrottledStub {
    /// A server with the shipped service time (110 ms) and the default
    /// request timeout (400 ms).
    pub fn new() -> Self {
        Self::with_times(
            Duration::from_millis(THROTTLED_SERVICE_MS),
            Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS),
        )
    }

    /// A server with a service time and a request timeout.
    ///
    /// The service time is how long one request takes once the server is
    /// free; the timeout is how long an answer is worth after the request
    /// was made. A request that would be served after its timeout is a
    /// timeout, not a wait: the client gives up and the reflex plays the
    /// moment (D26).
    pub fn with_times(service: Duration, timeout: Duration) -> Self {
        Self {
            inner: Arc::new(Inner {
                server: Mutex::new(Server {
                    free_at: Instant::now(),
                    arrivals: VecDeque::new(),
                }),
                service,
                timeout,
                calls: AtomicU64::new(0),
                timeouts: AtomicU64::new(0),
                unpaired: AtomicU64::new(0),
                confidence: AtomicU32::new(0.9f32.to_bits()),
            }),
        }
    }

    /// Set the confidence a menu answer reports.
    pub fn with_confidence(self, confidence: f32) -> Self {
        self.inner
            .confidence
            .store(confidence.to_bits(), Ordering::SeqCst);
        self
    }

    /// The client hands the server a request: it is in the server's queue
    /// now, and the clock that counts its wait starts now.
    ///
    /// One call per request, in the order the requests were made: the
    /// server serves them in that order too.
    pub fn arrive(&self) {
        let mut server = self
            .inner
            .server
            .lock()
            .expect("the server's queue is not poisoned");
        server.arrivals.push_back(Instant::now());
    }

    /// How many requests the server has been asked.
    pub fn calls(&self) -> u64 {
        self.inner.calls.load(Ordering::SeqCst)
    }

    /// How many requests the server refused as too late.
    pub fn timeouts(&self) -> u64 {
        self.inner.timeouts.load(Ordering::SeqCst)
    }

    /// How many requests the server served without an arrival.
    ///
    /// This is zero when the caller registers every request; a non-zero
    /// count means the queue the server serves is not the queue the caller
    /// meant.
    pub fn unpaired(&self) -> u64 {
        self.inner.unpaired.load(Ordering::SeqCst)
    }

    /// The arrival of the request the server is serving now.
    ///
    /// The worker may reach the server before the client has registered the
    /// request, so the server waits for the registration rather than
    /// guessing when the request joined the queue.
    fn take_arrival(&self) -> Instant {
        let give_up = Instant::now() + ARRIVAL_WAIT;
        loop {
            {
                let mut server = self
                    .inner
                    .server
                    .lock()
                    .expect("the server's queue is not poisoned");
                if let Some(arrival) = server.arrivals.pop_front() {
                    return arrival;
                }
            }
            if Instant::now() >= give_up {
                self.inner.unpaired.fetch_add(1, Ordering::SeqCst);
                return Instant::now();
            }
            std::thread::sleep(Duration::from_micros(100));
        }
    }
}

impl Default for ThrottledStub {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for ThrottledStub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ThrottledStub")
            .field("calls", &self.calls())
            .field("timeouts", &self.timeouts())
            .finish()
    }
}

impl SysOne for ThrottledStub {
    fn ask(&self, _state: &str, questions: &Value, deadline: Instant) -> Result<Answers, Refusal> {
        self.inner.calls.fetch_add(1, Ordering::SeqCst);

        // The request joins the server's queue when the mind made it, and the
        // wait it suffers is counted from there.
        let arrival = self.take_arrival();

        // The server serves one request at a time: this one starts when the
        // server is free and the requests before it are done.
        let finish = {
            let mut server = self
                .inner
                .server
                .lock()
                .expect("the server's queue is not poisoned");
            let start = server.free_at.max(arrival);
            let finish = start + self.inner.service;
            server.free_at = finish;
            finish
        };

        // An answer is worth something to the agent only if it arrives inside
        // the request's timeout — counted from the ask — and inside the
        // caller's own deadline. A request that misses it is a timeout, not
        // a wait: the client gives up and the reflex plays the moment.
        let worth = finish.min(deadline).min(arrival + self.inner.timeout);
        if finish > worth {
            self.inner.timeouts.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(worth.saturating_duration_since(Instant::now()));
            return Err(Refusal::Timeout);
        }
        std::thread::sleep(finish.saturating_duration_since(Instant::now()));

        // The answer is the stub's menu answer, and the latency it reports
        // is the whole wait: the queue and the service.
        let mut answers = menu_answer(questions, self.inner.confidence())?;
        answers.model = self.name().to_owned();
        answers.latency_ms = ms(finish.saturating_duration_since(arrival));
        Ok(answers)
    }

    fn name(&self) -> &str {
        "thor-d1-2b"
    }
}

/// The answer of a stub in menu mode: one probability per menu option.
///
/// The first option is the favourite and the rest share what is left.
fn menu_answer(questions: &Value, confidence: f32) -> Result<Answers, Refusal> {
    let gates = questions
        .as_object()
        .ok_or_else(|| Refusal::Malformed("the pack is not a table of gates".to_string()))?;
    if gates.is_empty() {
        return Err(Refusal::Malformed("the pack has no gates".to_string()));
    }

    let mut per_gate = BTreeMap::new();
    for (id, gate) in gates {
        let kind = gate.get("type").and_then(Value::as_str).unwrap_or("choice");
        let answer = match kind {
            "noul" => Answer::Noul(0.8),
            "score" => {
                // A score question's options are the levels of its scale,
                // counted from zero. The stub stands in for a model: it
                // answers the middle of the scale (idle 0, ordinary 1,
                // urgent 2) and it puts its weight on that level.
                let levels = gate
                    .get("criteria")
                    .and_then(Value::as_array)
                    .ok_or_else(|| Refusal::Malformed(format!("the gate {id} has no levels")))?
                    .len();
                let middle = levels.saturating_sub(1) / 2;
                let rest = levels.saturating_sub(1).max(1) as f32;
                let mut probs = BTreeMap::new();
                for level in 0..levels {
                    let p = if level == middle { 0.7 } else { 0.3 / rest };
                    probs.insert(level.to_string(), p);
                }
                Answer::Score {
                    probs,
                    score: middle as f32,
                    confidence,
                }
            }
            _ => Answer::Choice {
                probs: gate_probs(gate, id)?,
                confidence,
            },
        };
        per_gate.insert(id.clone(), answer);
    }

    Ok(Answers {
        per_gate,
        unsure: BTreeMap::new(),
        escalated: BTreeMap::new(),
        model: "stub".to_owned(),
        input_tokens: 64,
        latency_ms: 0,
    })
}

/// The stub's answer to one gate: the first option is the favourite and
/// the rest share what is left.
fn gate_probs(gate: &Value, id: &str) -> Result<BTreeMap<String, f32>, Refusal> {
    let criteria = gate
        .get("criteria")
        .and_then(Value::as_object)
        .ok_or_else(|| Refusal::Malformed(format!("the gate {id} has no criteria")))?;
    if criteria.is_empty() {
        return Err(Refusal::Malformed(format!("the gate {id} has no options")));
    }
    let count = criteria.len();
    let mut probs = BTreeMap::new();
    for (index, key) in criteria.keys().enumerate() {
        let p = match (index, count) {
            (0, 1) => 1.0,
            (0, _) => 0.7,
            (_, _) => 0.3 / (count - 1) as f32,
        };
        probs.insert(key.clone(), p);
    }
    Ok(probs)
}

// ---------------------------------------------------------------- the plumbing

/// A secret that never prints itself.
struct Secret(String);

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// An HTTP client that keeps its connections alive (D50).
pub(crate) fn build_agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .timeout_per_call(Some(timeout))
        .max_idle_connections_per_host(4)
        .build()
        .into()
}

/// An HTTP client that ignores the environment's proxy settings.
#[cfg(test)]
fn build_agent_without_proxy(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .timeout_per_call(Some(timeout))
        .max_idle_connections_per_host(4)
        .proxy(None)
        .build()
        .into()
}

/// The request body of a System One pack.
fn request_body(model: &str, state: &str, questions: &Value) -> String {
    json!({
        "model": model,
        "state": state,
        "questions": questions,
    })
    .to_string()
}

/// Post a pack and return the body and the round trip in milliseconds.
///
/// The call never outlives the deadline: whatever is left of it becomes the
/// request's timeout.
pub(crate) fn post_pack(
    agent: &ureq::Agent,
    url: &str,
    bearer: Option<&str>,
    body: &str,
    deadline: Instant,
) -> Result<(String, u32), Refusal> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(Refusal::Timeout);
    }

    let started = Instant::now();
    let mut request = agent.post(url).header("Content-Type", "application/json");
    if let Some(token) = bearer {
        request = request.header("Authorization", format!("Bearer {token}"));
    }

    let response = request
        .config()
        .timeout_global(Some(remaining))
        .timeout_per_call(Some(remaining))
        .build()
        .send(body.as_bytes())
        .map_err(map_ureq_error)?;

    let status = response.status().as_u16();
    let text = response
        .into_body()
        .read_to_string()
        .map_err(map_ureq_error)?;
    let latency_ms = ms(started.elapsed());

    if !(200..300).contains(&status) {
        return Err(Refusal::Http(status));
    }
    Ok((text, latency_ms))
}

/// A ureq failure as a refusal.
///
/// A status is a status; a timeout is a timeout; anything else never reached
/// an HTTP status at all.
fn map_ureq_error(error: ureq::Error) -> Refusal {
    match error {
        ureq::Error::StatusCode(code) => Refusal::Http(code),
        ureq::Error::Timeout(_) => Refusal::Timeout,
        ureq::Error::Io(ref e) if e.kind() == std::io::ErrorKind::TimedOut => Refusal::Timeout,
        _ => Refusal::Http(0),
    }
}

/// Parse a System One reply into answers.
///
/// The model is the id we asked for: the Thor's reply carries the path of the
/// weights it happens to serve, which is machine-specific and does not belong
/// in a log.
fn parse_answers(body: &str, model: &str, latency_ms: u32) -> Result<Answers, Refusal> {
    let value: Value = serde_json::from_str(body)
        .map_err(|e| Refusal::Malformed(format!("the answer is not JSON: {e}")))?;

    let raw = value
        .get("answers")
        .ok_or_else(|| Refusal::Malformed("the answer has no answers".to_string()))?;
    let object = raw
        .as_object()
        .ok_or_else(|| Refusal::Malformed("the answers are not a map".to_string()))?;

    let mut per_gate = BTreeMap::new();
    for (gate, answer) in object {
        per_gate.insert(gate.clone(), parse_answer(answer)?);
    }

    let input_tokens = value
        .get("usage")
        .and_then(|usage| usage.get("input_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);

    Ok(Answers {
        per_gate,
        unsure: BTreeMap::new(),
        escalated: BTreeMap::new(),
        model: model.to_string(),
        input_tokens: u32::try_from(input_tokens).unwrap_or(u32::MAX),
        latency_ms,
    })
}

/// Parse one gate's answer.
fn parse_answer(raw: &Value) -> Result<Answer, Refusal> {
    let kind = raw
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| Refusal::Malformed("the answer has no type".to_string()))?;

    match kind {
        "noul" => {
            let value = raw
                .get("noul")
                .and_then(Value::as_f64)
                .ok_or_else(|| Refusal::Malformed("the noul has no value".to_string()))?;
            let value = value as f32;
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(Refusal::Malformed(
                    "the noul is not a probability".to_string(),
                ));
            }
            Ok(Answer::Noul(value))
        }
        "choice" => {
            let probs = parse_probs(raw.get("probabilities"))?;
            let confidence = parse_confidence(raw, &probs)?;
            Ok(Answer::Choice { probs, confidence })
        }
        "score" => {
            let probs = parse_probs(raw.get("probabilities"))?;
            let score = raw
                .get("score")
                .and_then(Value::as_f64)
                .ok_or_else(|| Refusal::Malformed("the score has no value".to_string()))?;
            let score = score as f32;
            if !score.is_finite() {
                return Err(Refusal::Malformed("the score is not a number".to_string()));
            }
            let confidence = parse_confidence(raw, &probs)?;
            Ok(Answer::Score {
                probs,
                score,
                confidence,
            })
        }
        other => Err(Refusal::Malformed(format!("unknown answer type {other}"))),
    }
}

/// The probabilities of a choice or a score answer.
fn parse_probs(raw: Option<&Value>) -> Result<BTreeMap<String, f32>, Refusal> {
    let raw =
        raw.ok_or_else(|| Refusal::Malformed("the answer has no probabilities".to_string()))?;
    let object = raw
        .as_object()
        .ok_or_else(|| Refusal::Malformed("the probabilities are not a map".to_string()))?;

    let mut probs = BTreeMap::new();
    for (option, value) in object {
        let p = value.as_f64().ok_or_else(|| {
            Refusal::Malformed(format!("the probability of {option} is not a number"))
        })?;
        let p = p as f32;
        if !p.is_finite() || p < 0.0 {
            return Err(Refusal::Malformed(format!(
                "the probability of {option} is not a probability"
            )));
        }
        probs.insert(option.clone(), p);
    }
    if probs.is_empty() {
        return Err(Refusal::Malformed(
            "the answer has no probabilities".to_string(),
        ));
    }
    Ok(probs)
}

/// The confidence of an answer: what the model said, or what it means.
///
/// A `choice` or a `score` may arrive without a confidence (Thor handoff
/// §5.3). It is then the largest normalised share of the probabilities: never
/// 1.0, and never a guess.
fn parse_confidence(raw: &Value, probs: &BTreeMap<String, f32>) -> Result<f32, Refusal> {
    match raw.get("confidence") {
        Some(Value::Number(number)) => {
            let confidence = number
                .as_f64()
                .ok_or_else(|| Refusal::Malformed("the confidence is not a number".to_string()))?
                as f32;
            if !confidence.is_finite() || !(0.0..=1.0).contains(&confidence) {
                return Err(Refusal::Malformed(format!(
                    "the confidence {confidence} is not between 0 and 1"
                )));
            }
            Ok(confidence)
        }
        Some(Value::Null) | None => derived_confidence(probs),
        Some(_) => Err(Refusal::Malformed(
            "the confidence is not a number".to_string(),
        )),
    }
}

/// The largest normalised share of a distribution.
///
/// A flat distribution stays flat: the derived confidence of three equal
/// options is a third, not 1.0.
fn derived_confidence(probs: &BTreeMap<String, f32>) -> Result<f32, Refusal> {
    let mut sum = 0.0f32;
    let mut largest = 0.0f32;
    for p in probs.values() {
        sum += *p;
        if *p > largest {
            largest = *p;
        }
    }
    if !sum.is_finite() || sum <= 0.0 {
        return Err(Refusal::Malformed(
            "the probabilities do not sum to a positive number".to_string(),
        ));
    }
    Ok(largest / sum)
}

/// Split the answers into the ones the backend stands behind and the ones it
/// does not.
///
/// The threshold belongs to the backend, never to the answer: two models are
/// never averaged and never share a floor (Thor handoff §3.6). A `noul` has
/// no confidence — its value is the belief itself — so it is never unsure
/// here.
///
/// The rule is **per gate**, and that is the point: the pack-level minimum
/// used to veto the whole pack, so one unsure gate took every other gate's
/// decision with it. The attention gate alone — ten options, so unsure by
/// nature — refused the Thor's every answer. Now a gate the backend is not
/// sure about is moved out of the answers and into
/// [`Answers::unsure`], and it makes no decision of its own (D26).
fn split_unsure(answers: &mut Answers, min_confidence: f32) {
    let unsure: Vec<(String, f32)> = answers
        .per_gate
        .iter()
        .filter_map(|(id, answer)| {
            let confidence = match answer {
                Answer::Choice { confidence, .. } | Answer::Score { confidence, .. } => *confidence,
                Answer::Noul(_) => return None,
            };
            (confidence < min_confidence).then(|| (id.clone(), confidence))
        })
        .collect();
    for (id, _) in &unsure {
        answers.per_gate.remove(id);
    }
    answers.unsure = unsure.into_iter().collect();
}

/// A pack that came back empty is not an answer.
///
/// A model that answers none of the gates it was asked has failed: that is a
/// refusal, not a round with no decisions (D26).
///
/// A score answer has to name a level of the scale that was asked: the API
/// answers a score question with the index of the level it picked, and an
/// index off the end of the scale is an answer to a question that was
/// never asked.
fn check_answered(answers: &Answers, questions: &Value) -> Result<(), Refusal> {
    let asked = questions.as_object().is_some_and(|gates| !gates.is_empty());
    if asked && answers.per_gate.is_empty() {
        return Err(Refusal::Malformed(
            "the model answered none of the gates".to_string(),
        ));
    }
    for (id, answer) in &answers.per_gate {
        let Answer::Score { score, .. } = answer else {
            continue;
        };
        let Some(levels) = score_scale(questions, id) else {
            continue;
        };
        let top = levels.saturating_sub(1) as f32;
        if !score.is_finite() || *score < 0.0 || *score > top {
            return Err(Refusal::Malformed(format!(
                "the gate {id} answered the level {score}, which is not one of its {levels} levels"
            )));
        }
    }
    Ok(())
}

/// How many levels a score question asked over.
fn score_scale(questions: &Value, id: &str) -> Option<usize> {
    let question = questions.get(id)?;
    if question.get("type").and_then(Value::as_str) != Some("score") {
        return None;
    }
    Some(question.get("criteria")?.as_array()?.len())
}

/// The path of the Jev key.
fn default_secrets_path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"));
    home.join(".config").join("last-mile").join("secrets.env")
}

/// Read the Jev key out of a secrets file.
///
/// The key is never logged, never printed and never embedded: it only ever
/// reaches the `Authorization` header of a call to `api.typesafe.ai`.
fn read_api_key(path: &Path) -> Result<String, Refusal> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        Refusal::Malformed(format!(
            "cannot read the secrets file {}: {e}",
            path.display()
        ))
    })?;

    for line in text.lines() {
        let line = line.trim();
        let line = line.strip_prefix("export ").unwrap_or(line);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(value) = line.strip_prefix("TYPESAFE_API_KEY=") else {
            continue;
        };
        let key = value.trim().trim_matches('"').trim_matches('\'');
        if key.is_empty() {
            return Err(Refusal::Malformed(format!(
                "{}: TYPESAFE_API_KEY is empty",
                path.display()
            )));
        }
        return Ok(key.to_string());
    }

    Err(Refusal::Malformed(format!(
        "{}: no TYPESAFE_API_KEY line",
        path.display()
    )))
}

/// A duration in milliseconds, saturated.
fn ms(duration: Duration) -> u32 {
    u32::try_from(duration.as_millis()).unwrap_or(u32::MAX)
}

// ---------------------------------------------------------------- the tests

#[cfg(test)]
mod tests {
    use super::*;
    use lm_core::{Menu, MenuOption};
    use serde_json::Map;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use std::thread::{self, JoinHandle};

    /// The Thor's reply to a facts pack, copied from
    /// `docs/research/poc/thor/e4-d1-3B-facts.json` (scene 7, the "facts"
    /// condition).
    const THOR_FACTS_REPLY: &str = r#"{
      "model": "/home/caseydigennaro/models/d1-3B/d1-3B-Q8_0.gguf",
      "answers": {
        "enemy_visible": { "type": "noul", "noul": 0.31188227287647424 },
        "action": {
          "type": "choice",
          "choice": "advance",
          "probabilities": {
            "hide": 0.11747539647318686,
            "advance": 0.7427279835427973,
            "scout": 0.13979661998401582
          },
          "confidence": 0.614091975314196
        },
        "beacon_bearing": {
          "type": "choice",
          "choice": "south",
          "probabilities": {
            "north": 0.11093725105166598,
            "south": 0.8400110426391334,
            "east": 0.02140614810873595,
            "west": 0.027645558200464583
          },
          "confidence": 0.7866813901855112
        },
        "beacon_reachable": { "type": "noul", "noul": 0.6250886228653652 }
      },
      "usage": { "input_tokens": 530, "output_tokens": 0 }
    }"#;

    /// Jev's reply to the same pack, copied from
    /// `docs/research/poc/jev/e4-log.json` (scene 1, the "gates" condition).
    const JEV_GATES_REPLY: &str = r#"{
      "model": "jev-latest",
      "answers": {
        "enemy_visible": { "type": "noul", "noul": 0.08 },
        "action": {
          "type": "choice",
          "choice": "scout",
          "confidence": 0.69,
          "probabilities": { "hide": 0.05, "advance": 0.16, "scout": 0.79 }
        },
        "beacon_bearing": {
          "type": "choice",
          "choice": "north",
          "confidence": 0.76,
          "probabilities": { "north": 0.82, "south": 0.0, "east": 0.0, "west": 0.18 }
        },
        "beacon_reachable": { "type": "noul", "noul": 0.99 }
      },
      "usage": { "input_tokens": 800, "output_tokens": 124 }
    }"#;

    /// A pack the tests ask with.
    fn questions() -> Value {
        json!({
            "action": {
                "type": "choice",
                "instructions": "What should agent A do next?",
                "criteria": {
                    "hide": "an enemy can see A",
                    "advance": "no enemy is visible and the beacon is close",
                    "scout": "nothing is known"
                }
            }
        })
    }

    /// A plain-language facts pack (D49: the model never sees a grid).
    fn state() -> &'static str {
        "Agent A is at column 3, row 4. The beacon B is at column 7, row 4. \
         A walking path to the beacon exists and is 4 steps long. \
         No enemies are on the map."
    }

    /// The four-option menu of a squad with one beacon.
    fn menu() -> Menu {
        Menu {
            id: 0x5eed_0001,
            options: vec![
                MenuOption::GoToBeacon(0),
                MenuOption::Hold,
                MenuOption::FollowLeader,
                MenuOption::Retreat,
            ],
        }
    }

    /// A reply that answers every gate of a pack.
    ///
    /// The mock is the API's stand-in, so it answers what it is asked: a
    /// noul with a yes, a choice with its first option, and a score with
    /// the middle of its scale.
    fn answer_for(pack: &Value) -> String {
        let mut answers = Map::new();
        for (id, question) in pack.as_object().expect("a pack of gates") {
            let answer = match question
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("choice")
            {
                "noul" => json!({ "type": "noul", "noul": 0.8 }),
                "score" => {
                    let levels = question["criteria"].as_array().map(Vec::len).unwrap_or(2);
                    let probs: Map<String, Value> = (0..levels)
                        .map(|level| (level.to_string(), json!(1.0 / levels as f64)))
                        .collect();
                    json!({
                        "type": "score",
                        "score": (levels.saturating_sub(1)) as f64 / 2.0,
                        "probabilities": probs,
                        "confidence": 0.9
                    })
                }
                _ => {
                    let options = question["criteria"].as_object().expect("the options");
                    let probs: Map<String, Value> = options
                        .keys()
                        .enumerate()
                        .map(|(index, key)| {
                            (key.clone(), json!(if index == 0 { 1.0 } else { 0.0 }))
                        })
                        .collect();
                    json!({
                        "type": "choice",
                        "choice": options.keys().next().expect("an option"),
                        "probabilities": probs,
                        "confidence": 0.9
                    })
                }
            };
            answers.insert(id.clone(), answer);
        }
        json!({ "answers": Value::Object(answers) }).to_string()
    }

    /// A deadline far enough away for a mock answer.
    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(5)
    }

    /// Compare a float with a tolerance: the probabilities come from JSON.
    fn close(value: f32, expected: f64) -> bool {
        (f64::from(value) - expected).abs() < 1e-6
    }

    #[test]
    fn parses_thor_shape() {
        let mock = MockServer::start(200, THOR_FACTS_REPLY.to_string(), Duration::ZERO);
        let backend = ThorBackend::new("http://127.0.0.1", mock.port(), "d1-3B").without_proxy();

        let answers = backend
            .ask(state(), &questions(), deadline())
            .expect("the Thor answered the pack");

        // The model is the id we asked for, not the path the Thor serves.
        assert_eq!(answers.model, "d1-3B");
        assert_eq!(answers.input_tokens, 530);
        assert_eq!(answers.per_gate.len(), 4);

        match answers.per_gate.get("enemy_visible") {
            Some(Answer::Noul(p)) => assert!(close(*p, 0.31188227287647424), "got {p}"),
            other => panic!("enemy_visible is a noul, got {other:?}"),
        }
        match answers.per_gate.get("action") {
            Some(Answer::Choice { probs, confidence }) => {
                assert_eq!(probs.len(), 3);
                assert!(close(probs["hide"], 0.11747539647318686));
                assert!(close(probs["advance"], 0.7427279835427973));
                assert!(close(probs["scout"], 0.13979661998401582));
                assert!(close(*confidence, 0.614091975314196));
            }
            other => panic!("action is a choice, got {other:?}"),
        }
        match answers.per_gate.get("beacon_bearing") {
            Some(Answer::Choice { probs, confidence }) => {
                assert!(close(probs["south"], 0.8400110426391334));
                assert!(close(*confidence, 0.7866813901855112));
            }
            other => panic!("beacon_bearing is a choice, got {other:?}"),
        }
        match answers.per_gate.get("beacon_reachable") {
            Some(Answer::Noul(p)) => assert!(close(*p, 0.6250886228653652), "got {p}"),
            other => panic!("beacon_reachable is a noul, got {other:?}"),
        }

        // The pack really went over the wire.
        assert_eq!(mock.requests(), 1);
        assert!(mock.last_head().contains("POST /v1/systemone"));
        assert!(mock.last_head().contains("content-type: application/json"));
    }

    #[test]
    fn parses_jev_shape() {
        let mock = MockServer::start(200, JEV_GATES_REPLY.to_string(), Duration::ZERO);
        let secrets = write_secrets("jev", "TYPESAFE_API_KEY=test-key-not-a-secret\n");
        let mut backend = JevBackend::from_secrets_file(&secrets).expect("the key was read");
        backend.point_at(&format!("http://127.0.0.1:{}", mock.port()));
        let backend = backend.without_proxy();

        let answers = backend
            .ask(state(), &questions(), deadline())
            .expect("Jev answered the pack");

        assert_eq!(answers.model, "jev-latest");
        assert_eq!(answers.input_tokens, 800);
        assert_eq!(answers.per_gate.len(), 4);

        match answers.per_gate.get("enemy_visible") {
            Some(Answer::Noul(p)) => assert!(close(*p, 0.08), "got {p}"),
            other => panic!("enemy_visible is a noul, got {other:?}"),
        }
        match answers.per_gate.get("action") {
            Some(Answer::Choice { probs, confidence }) => {
                assert!(close(probs["scout"], 0.79));
                assert!(close(*confidence, 0.69));
            }
            other => panic!("action is a choice, got {other:?}"),
        }
        match answers.per_gate.get("beacon_bearing") {
            Some(Answer::Choice { probs, confidence }) => {
                assert!(close(probs["north"], 0.82));
                assert!(close(probs["south"], 0.0));
                assert!(close(*confidence, 0.76));
            }
            other => panic!("beacon_bearing is a choice, got {other:?}"),
        }

        // The bearer token travelled with the call.
        assert!(mock
            .last_head()
            .contains("authorization: Bearer test-key-not-a-secret"));
    }

    #[test]
    fn missing_confidence_derived() {
        let reply = r#"{
          "answers": {
            "action": {
              "type": "choice",
              "choice": "advance",
              "probabilities": { "hide": 0.15, "advance": 0.7, "scout": 0.15 }
            }
          }
        }"#;
        let mock = MockServer::start(200, reply.to_string(), Duration::ZERO);
        let backend = ThorBackend::new("http://127.0.0.1", mock.port(), "d1-3B").without_proxy();

        let answers = backend
            .ask(state(), &questions(), deadline())
            .expect("the model answered the pack");

        match answers.per_gate.get("action") {
            Some(Answer::Choice { probs, confidence }) => {
                assert_eq!(probs.len(), 3);
                // The largest normalised share, not 1.0.
                assert!(close(*confidence, 0.7), "got {confidence}");
                assert!(*confidence < 1.0, "a missing confidence is never 1.0");
            }
            other => panic!("action is a choice, got {other:?}"),
        }

        // A flat distribution stays flat: a third, never 1.0.
        let flat = BTreeMap::from([
            ("hide".to_string(), 1.0f32 / 3.0),
            ("advance".to_string(), 1.0f32 / 3.0),
            ("scout".to_string(), 1.0f32 / 3.0),
        ]);
        let derived = derived_confidence(&flat).expect("the flat distribution has a share");
        assert!(close(derived, 1.0 / 3.0), "got {derived}");
        assert!(derived < 1.0, "a flat distribution is never 1.0 confident");
    }

    #[test]
    fn timeout_is_refusal() {
        let mock = MockServer::start(
            200,
            THOR_FACTS_REPLY.to_string(),
            Duration::from_millis(400),
        );
        let backend = ThorBackend::new("http://127.0.0.1", mock.port(), "d1-3B").without_proxy();

        let deadline = Instant::now() + Duration::from_millis(40);
        let result = backend.ask(state(), &questions(), deadline);

        assert_eq!(result.err(), Some(Refusal::Timeout));
    }

    #[test]
    fn non_2xx_is_refusal() {
        let mock = MockServer::start(500, "{\"error\": \"boom\"}".to_string(), Duration::ZERO);
        let backend = ThorBackend::new("http://127.0.0.1", mock.port(), "d1-3B").without_proxy();

        let result = backend.ask(state(), &questions(), deadline());

        assert_eq!(result.err(), Some(Refusal::Http(500)));
    }

    #[test]
    fn an_unsure_gate_makes_no_decision() {
        // The rule is per gate: the gate the model was unsure about is not
        // an answer any more, and it takes no other gate's decision with
        // it. The pack itself is not refused.
        let reply = r#"{
          "answers": {
            "action": {
              "type": "choice",
              "choice": "advance",
              "probabilities": { "hide": 0.7, "advance": 0.2, "scout": 0.1 },
              "confidence": 0.2
            },
            "speech": {
              "type": "choice",
              "choice": "callout",
              "probabilities": { "callout": 0.9, "statement": 0.1 },
              "confidence": 0.9
            }
          }
        }"#;
        let mock = MockServer::start(200, reply.to_string(), Duration::ZERO);
        let backend = ThorBackend::new("http://127.0.0.1", mock.port(), "d1-3B")
            .without_proxy()
            .with_min_confidence(0.5);

        let answers = backend
            .ask(state(), &questions(), deadline())
            .expect("the pack is not refused");

        assert!(
            !answers.per_gate.contains_key("action"),
            "the unsure gate is not an answer: {:?}",
            answers.per_gate
        );
        assert!(
            answers.per_gate.contains_key("speech"),
            "the sure gate is still an answer: {:?}",
            answers.per_gate
        );
        assert!(
            close(answers.unsure["action"], 0.2),
            "the unsure gate keeps its confidence: {:?}",
            answers.unsure
        );
    }

    #[test]
    fn connection_reused() {
        let mock = MockServer::start(200, THOR_FACTS_REPLY.to_string(), Duration::ZERO);
        let backend = ThorBackend::new("http://127.0.0.1", mock.port(), "d1-3B").without_proxy();

        for _ in 0..5 {
            backend
                .ask(state(), &questions(), deadline())
                .expect("the model answered the pack");
        }

        assert_eq!(mock.requests(), 5);
        assert_eq!(
            mock.connections(),
            1,
            "the five calls shared one connection"
        );
    }

    #[test]
    fn an_empty_answer_is_refused() {
        let mock = MockServer::start(200, "{\"answers\": {}}".to_string(), Duration::ZERO);
        let backend = ThorBackend::new("http://127.0.0.1", mock.port(), "d1-3B").without_proxy();

        let result = backend.ask(state(), &questions(), deadline());

        match result {
            Err(Refusal::Malformed(why)) => {
                assert!(why.contains("none of the gates"), "got {why}");
            }
            other => panic!("expected a malformed refusal, got {other:?}"),
        }
    }

    #[test]
    fn the_default_pack_passes_the_api_schema() {
        // The default harness's pack is what every live ask sends. The
        // API refuses the WHOLE pack when one question does not fit, so
        // the pack has to fit the schema — here and on the Thor.
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/harness/default");
        let harness = crate::harness::Harness::open(&dir).expect("the default harness opens");
        let set = crate::gates::GateSet::parse(&harness.gates_toml).expect("the gates parse");
        let questions = set.to_questions(&menu());

        // The score question carries its levels as an array, lowest score
        // first.
        let criteria = questions["intention"]["criteria"]
            .as_array()
            .expect("the score criteria are an array");
        assert_eq!(criteria.len(), 3);

        // The pack fits the API's schema...
        validate_pack(&questions).expect("the default pack fits the API's schema");

        // ...and the API agrees: the mock enforces the same rule, so a
        // pack that broke it would come back as a 400 and this test
        // would fail.
        let mock = MockServer::start(200, answer_for(&questions), Duration::ZERO);
        let backend = ThorBackend::new("http://127.0.0.1", mock.port(), "d1-3B").without_proxy();

        let answers = backend
            .ask(state(), &questions, deadline())
            .expect("the Thor answered the default pack");

        assert_eq!(mock.requests(), 1);
        assert_eq!(answers.per_gate.len(), 11);
        // The score gate came back on the scale that was asked: the
        // middle of three levels is level 1.
        match answers.per_gate.get("intention") {
            Some(Answer::Score { score, .. }) => assert!(close(*score, 1.0), "got {score}"),
            other => panic!("the intention gate is a score, got {other:?}"),
        }
    }

    #[test]
    fn the_mock_refuses_a_pack_that_breaks_the_api_schema() {
        // The mock is the API's stand-in, and the API refused the whole
        // pack over one score question whose criteria was a map. The
        // mock refuses it the same way, with the API's own words.
        let mock = MockServer::start(200, THOR_FACTS_REPLY.to_string(), Duration::ZERO);
        let body = json!({
            "model": "d1-3B",
            "state": state(),
            "questions": {
                "intention": {
                    "type": "score",
                    "instructions": "How urgent is this moment?",
                    "criteria": {
                        "idle": "the moment is quiet",
                        "ordinary": "the moment is ordinary",
                        "urgent": "the moment is urgent"
                    }
                }
            }
        })
        .to_string();

        let reply = mock.raw_post(&body);

        assert!(reply.starts_with("HTTP/1.1 400"), "got {reply}");
        assert!(
            reply.contains("must be an array of 2 to 10 levels"),
            "got {reply}"
        );
        assert!(reply.contains("invalid_request_error"), "got {reply}");

        // And an array that is too short is refused too: the API takes 2
        // to 10 levels, not one.
        let body = json!({
            "model": "d1-3B",
            "state": state(),
            "questions": {
                "intention": {
                    "type": "score",
                    "instructions": "How urgent is this moment?",
                    "criteria": ["the moment is quiet"]
                }
            }
        })
        .to_string();

        let reply = mock.raw_post(&body);

        assert!(reply.starts_with("HTTP/1.1 400"), "got {reply}");
        assert!(
            reply.contains("must be an array of 2 to 10 levels"),
            "got {reply}"
        );
    }

    #[test]
    fn a_pack_that_breaks_the_api_schema_is_never_sent() {
        // The client checks the pack before it sends it: a broken
        // question is a typed refusal here, and not a 400 on the Thor.
        let mock = MockServer::start(200, THOR_FACTS_REPLY.to_string(), Duration::ZERO);
        let backend = ThorBackend::new("http://127.0.0.1", mock.port(), "d1-3B").without_proxy();
        let broken = json!({
            "intention": {
                "type": "score",
                "instructions": "How urgent is this moment?",
                "criteria": { "idle": "the moment is quiet" }
            }
        });

        let result = backend.ask(state(), &broken, deadline());

        match result {
            Err(Refusal::InvalidPack(why)) => {
                assert!(
                    why.contains("must be an array of 2 to 10 levels"),
                    "got {why}"
                );
            }
            other => panic!("expected an invalid-pack refusal, got {other:?}"),
        }
        // And nothing reached the Thor.
        assert_eq!(mock.requests(), 0);

        // A scale that is too short is refused the same way.
        let short = json!({
            "intention": {
                "type": "score",
                "instructions": "How urgent is this moment?",
                "criteria": ["the moment is quiet"]
            }
        });

        match backend.ask(state(), &short, deadline()) {
            Err(Refusal::InvalidPack(why)) => {
                assert!(
                    why.contains("must be an array of 2 to 10 levels"),
                    "got {why}"
                );
            }
            other => panic!("expected an invalid-pack refusal, got {other:?}"),
        }
        assert_eq!(mock.requests(), 0);
    }

    /// A backend that counts its calls and keeps the packs it was asked, so
    /// a test can see what the escalation asked (D71).
    #[derive(Clone, Debug, Default)]
    struct CountingBackend {
        calls: Arc<AtomicUsize>,
        packs: Arc<Mutex<Vec<Value>>>,
        confidence: f32,
        delay: Duration,
    }

    impl CountingBackend {
        fn new(confidence: f32) -> Self {
            Self {
                calls: Arc::new(AtomicUsize::new(0)),
                packs: Arc::new(Mutex::new(Vec::new())),
                confidence,
                delay: Duration::from_millis(0),
            }
        }

        fn slow(confidence: f32, delay: Duration) -> Self {
            Self {
                calls: Arc::new(AtomicUsize::new(0)),
                packs: Arc::new(Mutex::new(Vec::new())),
                confidence,
                delay,
            }
        }
    }

    impl SysOne for CountingBackend {
        fn ask(
            &self,
            _state: &str,
            questions: &Value,
            deadline: Instant,
        ) -> Result<Answers, Refusal> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.packs
                .lock()
                .expect("the packs are not poisoned")
                .push(questions.clone());
            if Instant::now()
                .checked_add(self.delay)
                .is_none_or(|end| end > deadline)
            {
                return Err(Refusal::Timeout);
            }
            std::thread::sleep(self.delay);
            menu_answer(questions, self.confidence)
        }

        fn name(&self) -> &str {
            "counting"
        }
    }

    /// The pack of one gate, for the escalation tests.
    fn one_gate_pack() -> Value {
        json!({
            "move": {
                "type": "choice",
                "instructions": "Pick one.",
                "criteria": {"opt0": "go", "opt1": "hold"},
            }
        })
    }

    #[test]
    fn an_unsure_gate_is_asked_of_the_second_model() {
        // D71: the cheap model answers below the threshold, so that gate
        // alone is asked of the second model, and the second model's answer
        // is the gate's answer.
        let primary = CountingBackend::new(0.2);
        let second = CountingBackend::new(0.9);
        let primary_calls = Arc::clone(&primary.calls);
        let second_calls = Arc::clone(&second.calls);
        let second_packs = Arc::clone(&second.packs);
        let backend = EscalatingBackend::new(Box::new(primary), Box::new(second), 0.5);
        let answers = backend
            .ask(
                "the facts",
                &one_gate_pack(),
                Instant::now() + Duration::from_secs(5),
            )
            .expect("the answer comes back");
        // The second model was asked once, and it was asked that gate alone.
        assert_eq!(second_calls.load(Ordering::SeqCst), 1);
        let packs = second_packs.lock().expect("the packs are not poisoned");
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].as_object().expect("a table").len(), 1);
        assert!(packs[0].get("move").is_some());
        drop(packs);
        // The second model's answer is the gate's answer.
        match answers.per_gate.get("move") {
            Some(Answer::Choice { confidence, .. }) => {
                assert!((confidence - 0.9).abs() < 0.01, "got {confidence}");
            }
            other => panic!("got {other:?}"),
        }
        // The escalation is logged: the gate, both confidences, the latency
        // and both models' own choices.
        let escalation = answers.escalated.get("move").expect("the escalation");
        assert_eq!(escalation.gate, "move");
        assert!((escalation.primary_confidence - 0.2).abs() < 0.01);
        assert_eq!(escalation.second_confidence, Some(0.9));
        assert_eq!(escalation.primary_choice.as_deref(), Some("opt0"));
        assert_eq!(escalation.second_choice.as_deref(), Some("opt0"));
        assert_eq!(escalation.outcome, EscalationOutcome::Taken);
        assert_eq!(backend.escalation_count(), 1);
        // The cheap model was asked once: the escalation does not ask it
        // again.
        assert_eq!(primary_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_sure_gate_is_not_escalated() {
        // D71: the escalation buys a second opinion only where the cheap
        // model is unsure.
        let primary = CountingBackend::new(0.9);
        let second = CountingBackend::new(0.9);
        let second_calls = Arc::clone(&second.calls);
        let backend = EscalatingBackend::new(Box::new(primary), Box::new(second), 0.5);
        let answers = backend
            .ask(
                "the facts",
                &one_gate_pack(),
                Instant::now() + Duration::from_secs(5),
            )
            .expect("the answer comes back");
        assert_eq!(second_calls.load(Ordering::SeqCst), 0);
        assert!(answers.escalated.is_empty());
        assert_eq!(backend.escalation_count(), 0);
    }

    #[test]
    fn a_slow_second_model_leaves_the_gate_unsure() {
        // D71: the escalation is bounded by the deadline. A second opinion
        // that cannot arrive in time is no opinion, and the gate stays
        // unsure: the cheap model was not trusted on its own.
        let primary = CountingBackend::new(0.2);
        let second = CountingBackend::slow(0.9, Duration::from_millis(500));
        let backend = EscalatingBackend::new(Box::new(primary), Box::new(second), 0.5);
        let answers = backend
            .ask(
                "the facts",
                &one_gate_pack(),
                Instant::now() + Duration::from_millis(50),
            )
            .expect("the pack is answered");
        assert!(answers.per_gate.is_empty(), "the gate decides nothing");
        assert_eq!(answers.unsure.get("move"), Some(&0.2));
        let escalation = answers.escalated.get("move").expect("the escalation");
        assert_eq!(escalation.outcome, EscalationOutcome::Timeout);
        assert_eq!(escalation.second_confidence, None);
    }

    #[test]
    fn a_gate_outside_the_set_is_not_escalated() {
        // The harness names the gates a second opinion is bought for; a gate
        // outside the set keeps the cheap model's answer (or its unsure)
        // even below the threshold. An empty set turns the escalation off.
        let two_gate_pack = json!({
            "move": {
                "type": "choice",
                "instructions": "Pick one.",
                "criteria": {"opt0": "go", "opt1": "hold"},
            },
            "attention": {
                "type": "choice",
                "instructions": "Who for?",
                "criteria": {"squad": "the squad", "all": "everyone"},
            }
        });
        let gates = BTreeSet::from(["move".to_owned()]);
        for gates in [Some(gates), Some(BTreeSet::new())] {
            let primary = CountingBackend::new(0.2);
            let second = CountingBackend::new(0.9);
            let second_calls = Arc::clone(&second.calls);
            let backend = EscalatingBackend::new(Box::new(primary), Box::new(second), 0.5)
                .with_gates(gates.clone());
            let answers = backend
                .ask(
                    "the facts",
                    &two_gate_pack,
                    Instant::now() + Duration::from_secs(5),
                )
                .expect("the answer comes back");
            if gates.as_ref().is_some_and(|g| g.contains("move")) {
                // The named gate was escalated; the unnamed one was not.
                assert_eq!(second_calls.load(Ordering::SeqCst), 1);
                assert!(answers.escalated.contains_key("move"));
                assert!(!answers.escalated.contains_key("attention"));
                // The unnamed gate keeps the cheap model's answer exactly as
                // the cheap model gave it: the second opinion never touches
                // a gate it was not asked about.
                assert!(answers.per_gate.contains_key("attention"));
                assert!(!answers.unsure.contains_key("attention"));
            } else {
                // The empty set: the escalation is off, and the cheap
                // model's answers all stand as the cheap model gave them.
                assert_eq!(second_calls.load(Ordering::SeqCst), 0);
                assert!(answers.escalated.is_empty());
                assert_eq!(answers.per_gate.len(), 2);
            }
        }
    }

    #[test]
    fn the_stub_answers_its_script() {
        let answers = Answers {
            per_gate: BTreeMap::from([(
                "action".to_string(),
                Answer::Choice {
                    probs: BTreeMap::from([("advance".to_string(), 1.0)]),
                    confidence: 1.0,
                },
            )]),
            unsure: BTreeMap::new(),
            escalated: BTreeMap::new(),
            model: "stub".to_string(),
            input_tokens: 0,
            latency_ms: 0,
        };
        let stub = StubBackend::scripted(vec![
            (Duration::from_millis(5), Ok(answers.clone())),
            (Duration::from_millis(5), Err(Refusal::Timeout)),
        ]);

        let first = stub
            .ask(state(), &questions(), deadline())
            .expect("the stub answered");
        assert_eq!(first.latency_ms, 5);
        assert!(matches!(
            first.per_gate.get("action"),
            Some(Answer::Choice { .. })
        ));

        let second = stub.ask(state(), &questions(), deadline());
        assert_eq!(second.err(), Some(Refusal::Timeout));

        // The script ran out: the last outcome repeats.
        let third = stub.ask(state(), &questions(), deadline());
        assert_eq!(third.err(), Some(Refusal::Timeout));
        assert_eq!(stub.calls(), 3);
    }

    #[test]
    fn a_stub_that_cannot_finish_is_a_timeout() {
        let stub = StubBackend::scripted(vec![(
            Duration::from_millis(200),
            Err(Refusal::Malformed("never reached".to_string())),
        )]);

        let deadline = Instant::now() + Duration::from_millis(20);
        let result = stub.ask(state(), &questions(), deadline);

        assert_eq!(result.err(), Some(Refusal::Timeout));
    }

    #[test]
    fn the_key_is_read_and_never_printed() {
        // Its own file name: the tests run in parallel on one temp dir, and
        // two tests writing one shared path race each other's writes.
        let secrets = write_secrets("jev-print", "# the key\nTYPESAFE_API_KEY=sk-test-123456\n");
        let backend = JevBackend::from_secrets_file(&secrets).expect("the key was read");

        let printed = format!("{backend:?}");
        assert!(
            !printed.contains("sk-test-123456"),
            "the key leaked: {printed}"
        );
        assert!(printed.contains("redacted"), "got {printed}");
        assert_eq!(backend.name(), "jev");
    }

    #[test]
    fn a_secrets_file_without_a_key_is_refused() {
        let secrets = write_secrets("empty", "OTHER_KEY=nope\n");
        let result = JevBackend::from_secrets_file(&secrets);

        match result {
            Err(Refusal::Malformed(why)) => assert!(why.contains("TYPESAFE_API_KEY"), "got {why}"),
            other => panic!("expected a malformed refusal, got {other:?}"),
        }
    }

    /// Ask the Thor's d1-3B for real.
    ///
    /// Opt-in: `cargo test -- --ignored live_thor`. It needs the Thor to be
    /// reachable at 192.168.0.78:8093.
    #[test]
    #[ignore]
    fn live_thor() {
        let backend = ThorBackend::new("http://192.168.0.78", 8093, "d1-3B");
        let deadline = Instant::now() + Duration::from_secs(10);

        let answers = match backend.ask(state(), &questions(), deadline) {
            Ok(answers) => answers,
            Err(Refusal::Http(code)) => panic!("the Thor answered {code}, not 200"),
            Err(other) => panic!("the Thor did not answer: {other}"),
        };

        assert!(answers.input_tokens > 0, "the Thor reported no usage");
        match answers.per_gate.get("action") {
            Some(Answer::Choice { .. }) => {}
            other => panic!("the action gate is a choice, got {other:?}"),
        }
    }

    /// Write a secrets file for the tests and hand back its path.
    fn write_secrets(name: &str, content: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lm-agents-sysone-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the temp dir is created");
        let path = dir.join(format!("{name}.env"));
        std::fs::write(&path, content).expect("the secrets file is written");
        path
    }

    /// A tiny HTTP/1.1 server for the tests.
    ///
    /// It speaks keep-alive by hand, counts the connections it accepted and
    /// counts the requests it answered.
    struct MockServer {
        port: u16,
        connections: Arc<AtomicUsize>,
        requests: Arc<AtomicUsize>,
        heads: Arc<Mutex<Vec<String>>>,
        running: Arc<AtomicBool>,
        handle: Option<JoinHandle<()>>,
    }

    impl MockServer {
        /// Start a server that answers every request with `status` and `body`,
        /// after waiting `delay`.
        fn start(status: u16, body: String, delay: Duration) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("the mock server binds");
            listener
                .set_nonblocking(true)
                .expect("the mock server polls");
            let port = listener
                .local_addr()
                .expect("the mock server has an address")
                .port();

            let connections = Arc::new(AtomicUsize::new(0));
            let requests = Arc::new(AtomicUsize::new(0));
            let heads = Arc::new(Mutex::new(Vec::new()));
            let running = Arc::new(AtomicBool::new(true));

            let handle = {
                let connections = Arc::clone(&connections);
                let requests = Arc::clone(&requests);
                let heads = Arc::clone(&heads);
                let running = Arc::clone(&running);
                thread::spawn(move || {
                    while running.load(Ordering::SeqCst) {
                        match listener.accept() {
                            Ok((stream, _)) => {
                                connections.fetch_add(1, Ordering::SeqCst);
                                serve(stream, status, &body, delay, &requests, &heads);
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                thread::sleep(Duration::from_millis(2));
                            }
                            Err(_) => break,
                        }
                    }
                })
            };

            Self {
                port,
                connections,
                requests,
                heads,
                running,
                handle: Some(handle),
            }
        }

        fn port(&self) -> u16 {
            self.port
        }

        fn connections(&self) -> usize {
            self.connections.load(Ordering::SeqCst)
        }

        fn requests(&self) -> usize {
            self.requests.load(Ordering::SeqCst)
        }

        /// The head of the last request the server read.
        fn last_head(&self) -> String {
            self.heads
                .lock()
                .expect("the mock's log is not poisoned")
                .last()
                .cloned()
                .unwrap_or_default()
        }

        /// Post a body to the mock by hand and hand back the reply.
        ///
        /// The client checks a pack before it sends it, so the mock's own
        /// refusal is only ever seen by a raw request: this is how a test
        /// watches what the API answers a pack that breaks the schema.
        fn raw_post(&self, body: &str) -> String {
            let mut stream =
                TcpStream::connect(("127.0.0.1", self.port)).expect("the mock answers");
            let request = format!(
                "POST {SYSTEMONE_PATH} HTTP/1.1\r\n\
                 Host: 127.0.0.1\r\n\
                 Content-Type: application/json\r\n\
                 Content-Length: {len}\r\n\
                 \r\n\
                 {body}",
                len = body.len(),
            );
            stream
                .write_all(request.as_bytes())
                .expect("the request is written");
            stream.flush().expect("the request is flushed");
            // The mock keeps the connection alive; the reply is read once
            // the request is done.
            let _ = stream.shutdown(std::net::Shutdown::Write);
            let mut reply = String::new();
            stream
                .read_to_string(&mut reply)
                .expect("the reply is read");
            reply
        }
    }

    impl Drop for MockServer {
        fn drop(&mut self) {
            self.running.store(false, Ordering::SeqCst);
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    /// Answer the requests on one connection, until it closes.
    fn serve(
        mut stream: TcpStream,
        status: u16,
        body: &str,
        delay: Duration,
        requests: &AtomicUsize,
        heads: &Mutex<Vec<String>>,
    ) {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
        let mut buf = Vec::new();
        loop {
            let Some((head, request)) = read_request(&mut stream, &mut buf) else {
                return;
            };
            requests.fetch_add(1, Ordering::SeqCst);
            heads
                .lock()
                .expect("the mock's log is not poisoned")
                .push(head);

            if !delay.is_zero() {
                thread::sleep(delay);
            }

            // The mock is the API's stand-in, so it enforces the API's
            // question schema: a pack that breaks the rules is refused
            // with the API's own error, and a gate that asks a score
            // question the wrong way fails a test instead of passing one.
            let (status, answer) = match schema_error(&request) {
                Some(why) => (400u16, api_error(&why)),
                None => (status, body.to_string()),
            };

            let response = format!(
                "HTTP/1.1 {status} {reason}\r\n\
                 Content-Type: application/json\r\n\
                 Content-Length: {len}\r\n\
                 \r\n\
                 {answer}",
                reason = if status == 200 { "OK" } else { "Error" },
                len = answer.len(),
            );
            if stream.write_all(response.as_bytes()).is_err() {
                return;
            }
            if stream.flush().is_err() {
                return;
            }
        }
    }

    /// Why a request body breaks the API's question schema, if it does.
    ///
    /// The rules are the API's: a `score` question's criteria is an array
    /// of 2 to 10 levels, a `choice` question's criteria is a map of
    /// options, and a `noul` question has none.
    fn schema_error(body: &[u8]) -> Option<String> {
        let text = std::str::from_utf8(body).ok()?;
        let request: Value = serde_json::from_str(text).ok()?;
        let questions = request.get("questions")?;
        validate_pack(questions).err().map(|e| e.to_string())
    }

    /// The API's refusal of a pack that breaks its schema.
    fn api_error(why: &str) -> String {
        json!({
            "error": {
                "code": 400,
                "message": why,
                "type": "invalid_request_error",
            }
        })
        .to_string()
    }

    /// Read one HTTP request: its head and its body.
    fn read_request(stream: &mut TcpStream, buf: &mut Vec<u8>) -> Option<(String, Vec<u8>)> {
        loop {
            if let Some(head_end) = find(buf, b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                let len = content_length(&head);
                if buf.len() >= head_end + 4 + len {
                    let body = buf[head_end + 4..head_end + 4 + len].to_vec();
                    buf.drain(..head_end + 4 + len);
                    return Some((head, body));
                }
            }
            let mut chunk = [0u8; 4096];
            match stream.read(&mut chunk) {
                Ok(0) => return None,
                Ok(read) => buf.extend_from_slice(&chunk[..read]),
                Err(_) => return None,
            }
        }
    }

    /// Find a byte pattern.
    fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|w| w == needle)
    }

    /// The `Content-Length` of a request head.
    fn content_length(head: &str) -> usize {
        head.lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(name, _)| name.trim().eq_ignore_ascii_case("content-length"))
            .and_then(|(_, value)| value.trim().parse().ok())
            .unwrap_or(0)
    }
}
