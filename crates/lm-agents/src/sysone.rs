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

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The path every backend posts to.
pub const SYSTEMONE_PATH: &str = "/v1/systemone";

/// The cloud provider's base URL. The Jev key only ever goes here.
pub const JEV_BASE: &str = "https://api.typesafe.ai";

/// The model the cloud provider serves by default.
pub const JEV_MODEL: &str = "jev-latest";

/// The confidence floor for a Thor backend, in the spirit of the
/// `thor-d1-3b` default in `AgentsConfig`.
pub const THOR_MIN_CONFIDENCE: f32 = 0.55;

/// The confidence floor for the cloud backend, in the spirit of the `jev`
/// default in `AgentsConfig`.
pub const JEV_MIN_CONFIDENCE: f32 = 0.5;

/// The per-request timeout, in the spirit of `AgentsConfig::request_timeout_ms`.
pub const DEFAULT_REQUEST_TIMEOUT_MS: u64 = 400;

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
    pub per_gate: BTreeMap<String, Answer>,
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
    /// The answer was less confident than this backend's threshold.
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
            Refusal::LowConfidence(c) => write!(f, "the answer was only {c} confident"),
            Refusal::MenuChanged => f.write_str("the menu changed under the question"),
            Refusal::Budget => f.write_str("the budget is spent"),
        }
    }
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
        let body = request_body(&self.model, state, questions);
        let (text, latency_ms) = post_pack(&self.agent, &self.url(), None, &body, deadline)?;
        let answers = parse_answers(&text, &self.model, latency_ms)?;
        check_answered(&answers, questions)?;
        check_confidence(&answers, self.min_confidence)?;
        Ok(answers)
    }

    fn name(&self) -> &str {
        &self.name
    }
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
        let body = request_body(&self.model, state, questions);
        let (text, latency_ms) = post_pack(
            &self.agent,
            &self.url(),
            Some(&self.api_key.0),
            &body,
            deadline,
        )?;
        let answers = parse_answers(&text, &self.model, latency_ms)?;
        check_answered(&answers, questions)?;
        check_confidence(&answers, self.min_confidence)?;
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
pub struct StubBackend {
    script: Mutex<StubScript>,
    calls: AtomicUsize,
}

/// The script of a [`StubBackend`]: what is left, and what to repeat.
struct StubScript {
    queue: VecDeque<(Duration, Result<Answers, Refusal>)>,
    last: Option<(Duration, Result<Answers, Refusal>)>,
}

impl StubBackend {
    /// A backend that answers with `script`, in order.
    pub fn scripted(script: Vec<(Duration, Result<Answers, Refusal>)>) -> Self {
        Self {
            script: Mutex::new(StubScript {
                queue: script.into(),
                last: None,
            }),
            calls: AtomicUsize::new(0),
        }
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
    fn ask(&self, _state: &str, _questions: &Value, deadline: Instant) -> Result<Answers, Refusal> {
        self.calls.fetch_add(1, Ordering::SeqCst);

        let entry = {
            let mut script = self
                .script
                .lock()
                .expect("the stub's script is not poisoned");
            match script.queue.pop_front() {
                Some(entry) => {
                    script.last = Some(entry.clone());
                    Some(entry)
                }
                None => script.last.clone(),
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
fn build_agent(timeout: Duration) -> ureq::Agent {
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
fn post_pack(
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

/// Refuse an answer that is less confident than the backend's threshold.
///
/// The threshold belongs to the backend, never to the answer: two models are
/// never averaged and never share a floor (Thor handoff §3.6). A `noul` has
/// no confidence — its value is the belief itself — so it is never refused
/// here.
fn check_confidence(answers: &Answers, min_confidence: f32) -> Result<(), Refusal> {
    let lowest = answers
        .per_gate
        .values()
        .filter_map(|answer| match answer {
            Answer::Choice { confidence, .. } | Answer::Score { confidence, .. } => {
                Some(*confidence)
            }
            Answer::Noul(_) => None,
        })
        .fold(f32::INFINITY, f32::min);

    if lowest < min_confidence {
        return Err(Refusal::LowConfidence(lowest));
    }
    Ok(())
}

/// A pack that came back empty is not an answer.
///
/// A model that answers none of the gates it was asked has failed: that is a
/// refusal, not a round with no decisions (D26).
fn check_answered(answers: &Answers, questions: &Value) -> Result<(), Refusal> {
    let asked = questions.as_object().is_some_and(|gates| !gates.is_empty());
    if asked && answers.per_gate.is_empty() {
        return Err(Refusal::Malformed(
            "the model answered none of the gates".to_string(),
        ));
    }
    Ok(())
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
    fn low_confidence_is_refusal() {
        let reply = r#"{
          "answers": {
            "action": {
              "type": "choice",
              "choice": "advance",
              "probabilities": { "hide": 0.7, "advance": 0.2, "scout": 0.1 },
              "confidence": 0.2
            }
          }
        }"#;
        let mock = MockServer::start(200, reply.to_string(), Duration::ZERO);
        let backend = ThorBackend::new("http://127.0.0.1", mock.port(), "d1-3B").without_proxy();

        let result = backend.ask(state(), &questions(), deadline());

        match result {
            Err(Refusal::LowConfidence(confidence)) => {
                assert!(close(confidence, 0.2), "got {confidence}");
            }
            other => panic!("expected a low-confidence refusal, got {other:?}"),
        }
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
    fn the_stub_answers_its_script() {
        let answers = Answers {
            per_gate: BTreeMap::from([(
                "action".to_string(),
                Answer::Choice {
                    probs: BTreeMap::from([("advance".to_string(), 1.0)]),
                    confidence: 1.0,
                },
            )]),
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
        let secrets = write_secrets("jev", "# the key\nTYPESAFE_API_KEY=sk-test-123456\n");
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
            let Some((head, _body)) = read_request(&mut stream, &mut buf) else {
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

            let response = format!(
                "HTTP/1.1 {status} {reason}\r\n\
                 Content-Type: application/json\r\n\
                 Content-Length: {len}\r\n\
                 \r\n\
                 {body}",
                reason = if status == 200 { "OK" } else { "Error" },
                len = body.len(),
            );
            if stream.write_all(response.as_bytes()).is_err() {
                return;
            }
            if stream.flush().is_err() {
                return;
            }
        }
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
