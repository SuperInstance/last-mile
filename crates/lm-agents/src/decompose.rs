//! Asking a gate in a different shape.
//!
//! Two shapes, both behind a parameter and both **off** unless a
//! parameter asks for them:
//!
//! - the **tree**: one gate asked as two smaller questions, the second
//!   taken from the first's answer ([`DecomposingBackend`]);
//! - the **ghost**: one gate asked with the options that rarely win left
//!   out ([`PruningBackend`]).
//!
//! Both keep the gate's own answer. What the rest of the code sees is the
//! gate's answer, in the gate's own options, so nothing downstream has to
//! know the gate was asked in another shape. What changed is *logged*: the
//! tree's path in [`Answers::decomposed`], the ghost's ask in
//! [`PruningBackend::asks`].
//!
//! The shapes are the harness's own questions. The tree is written in
//! `gates.toml` next to the gate it decomposes, because a question
//! belongs to the harness: the questions we know to ask are our experience
//! (docs/design/questions-as-experience.md).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Mutex;
use std::time::Instant;

use serde_json::{json, Map, Value};

use crate::config::AgentsConfig;
use crate::gates::{Decomposition, GateSet};
use crate::sysone::{
    Answer, Answers, DecompositionRecord, DecompositionStep, EscalatingBackend, Escalation,
    EscalationOutcome, Refusal, SysOne,
};

// ----------------------------------------------------------------- the shape

/// Why a shape could not be built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShapeError {
    /// The gate named as a tree has no tree in the harness.
    ///
    /// The tree is the harness's own, so a parameter that names a gate the
    /// harness has no tree for is a mistake, not a silent fallback.
    NoTree(String),
    /// Two shapes were named at once.
    ///
    /// A gate is asked in one shape: a tree or a ghost, never both.
    BothShapes,
}

impl fmt::Display for ShapeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShapeError::NoTree(gate) => {
                write!(f, "the harness has no tree for the gate {gate}")
            }
            ShapeError::BothShapes => {
                f.write_str("a gate is asked in one shape: a tree or a ghost, never both")
            }
        }
    }
}

impl std::error::Error for ShapeError {}

/// What the log says a gate chose, per situation.
///
/// This is the ghost table's memory of the model: the gate's winners, as
/// the round's log recorded them. A round that has a log hands it over and
/// the gate is asked with the options that have been winning, instead of
/// being asked blind until it finds out by itself.
#[derive(Clone, Debug, Default)]
pub struct GhostLog {
    /// The winner of the gate, per situation, with the options it was
    /// asked between.
    entries: Vec<(String, Vec<String>, Option<String>)>,
}

impl GhostLog {
    /// A log with nothing in it.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record what the gate chose in a situation.
    ///
    /// The situation is the facts text, exactly as the table keys it
    /// itself, so a recorded answer and an asked answer land on the same
    /// site.
    pub fn record(&mut self, state: &str, options: &[String], winner: Option<&str>) {
        self.entries.push((
            state.to_owned(),
            options.to_vec(),
            winner.map(str::to_owned),
        ));
    }

    /// How many answers the log holds.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the log is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The entries, for the table.
    fn entries(&self) -> &[(String, Vec<String>, Option<String>)] {
        &self.entries
    }
}

/// Build the backend a round asks, in the shape the parameters ask for.
///
/// The shape is **off** unless a parameter names it, so the default
/// build is the backend the shipped round uses: the cheap model, with a
/// second opinion bought for the gates the parameters name.
///
/// The second model is built by `second` rather than handed over, because
/// the tree and the escalation may each want one: they are the same model
/// on the same port, and each gets its own client of it. A backend with
/// no second model is left as it is: nothing is escalated, and the shape
/// (if one is named) is still applied.
///
/// The ghost log is the round's own log, when there is one: the ghosted
/// gate is asked with the options that have been winning in the
/// situations the log covers. Without a log the gate is asked cold and
/// learns by itself.
///
/// * a **tree** gate is asked by the tree, which escalates its own last
///   step when it is told to; the escalation around it is left alone for
///   that gate, so the second opinion is bought once and only once;
/// * a **ghost** gate is asked with fewer options, and the escalation is
///   **inside** the ghost, so the second model is asked the ghosted
///   question too — a second opinion of the full question would undo the
///   ghosting.
///
/// `escalate_gates` is the harness's: the gates a second opinion is bought
/// for, read from `policy.toml` (`escalate_gates`, D75). It is passed in
/// rather than read from the agents file, so that there is one setting and
/// not two.
pub fn shape_backend(
    gates: &GateSet,
    cfg: &AgentsConfig,
    escalate_gates: Option<BTreeSet<String>>,
    primary: Box<dyn SysOne + Send + Sync>,
    second: &dyn Fn() -> Option<Box<dyn SysOne + Send + Sync>>,
    ghost: Option<&GhostLog>,
) -> Result<Box<dyn SysOne + Send + Sync>, ShapeError> {
    let shape = &cfg.shape;
    if shape.tree_gate.is_some() && shape.prune_gate.is_some() {
        return Err(ShapeError::BothShapes);
    }

    // The gates the escalation around the shape may touch: the tree's own
    // gate is the tree's business. The exclusion is kept apart from the
    // filter, because "every gate but this one" cannot be written as a
    // list of gates — and that is exactly what no filter at all means.
    let exclude_gates: BTreeSet<String> = shape.tree_gate.iter().cloned().collect();
    let escalation = |inner: Box<dyn SysOne + Send + Sync>| -> Box<dyn SysOne + Send + Sync> {
        // No second model: there is no second opinion to buy, and the
        // cheap model's own answer is the answer.
        let Some(second) = second() else {
            return inner;
        };
        let mut backend = EscalatingBackend::new(inner, second, cfg.escalate_below)
            .with_gates(escalate_gates.clone());
        if !exclude_gates.is_empty() {
            backend = backend.with_exclude_gates(exclude_gates.clone());
        }
        Box::new(backend)
    };

    if let Some(gate) = &shape.tree_gate {
        let tree = gates
            .decomposition_of(gate)
            .ok_or_else(|| ShapeError::NoTree(gate.clone()))?;
        let tree_second = if shape.escalate_tree_step {
            second()
        } else {
            None
        };
        let inner = Box::new(DecomposingBackend::new(
            primary,
            tree_second,
            cfg.escalate_below,
            gate.clone(),
            tree.clone(),
        ));
        return Ok(escalation(inner));
    }

    if let Some(gate) = &shape.prune_gate {
        // The escalation goes **inside** the ghost: the second model is
        // asked the question as it was asked, with the ghosted options
        // left out. A second opinion of the full question would undo the
        // ghosting, and the arm would not be measuring what it claims.
        let pruning = PruningBackend::new(
            escalation(primary),
            gate.clone(),
            usize::from(shape.prune_k),
        );
        if let Some(log) = ghost {
            pruning.learn(log);
        }
        return Ok(Box::new(pruning));
    }

    Ok(escalation(primary))
}

/// Build every backend of a round in the shape the parameters ask for.
///
/// This is the one place a round is shaped: the game and the measurements
/// both go through it, so an arm of the measurement is the shipped code
/// and not a re-implementation of it.
///
/// The backend that *is* the second model is left alone: it does not
/// escalate to itself.
///
/// `escalate_gates` is the harness's set of gates a second opinion is
/// bought for, read from `policy.toml` (D75) and passed in by the caller.
pub fn shape_backends(
    gates: &GateSet,
    cfg: &AgentsConfig,
    escalate_gates: Option<BTreeSet<String>>,
    backends: BTreeMap<String, Box<dyn SysOne + Send + Sync>>,
    second: impl Fn(&str) -> Option<Box<dyn SysOne + Send + Sync>>,
) -> Result<BTreeMap<String, Box<dyn SysOne + Send + Sync>>, ShapeError> {
    shape_backends_with(gates, cfg, escalate_gates, backends, second, None)
}

/// Build every backend of a round in the shape the parameters ask for,
/// with the ghosted gate taught by a log.
///
/// The log is the round's own record of what the gate chose: the ghosted
/// gate is asked with the options that have been winning in the
/// situations the log covers. Without a log the gate is asked cold and
/// learns by itself.
pub fn shape_backends_with(
    gates: &GateSet,
    cfg: &AgentsConfig,
    escalate_gates: Option<BTreeSet<String>>,
    backends: BTreeMap<String, Box<dyn SysOne + Send + Sync>>,
    second: impl Fn(&str) -> Option<Box<dyn SysOne + Send + Sync>>,
    ghost: Option<&GhostLog>,
) -> Result<BTreeMap<String, Box<dyn SysOne + Send + Sync>>, ShapeError> {
    let mut out = BTreeMap::new();
    for (name, backend) in backends {
        if name == cfg.escalate_backend {
            out.insert(name, backend);
            continue;
        }
        // The second model is the one the configuration names, and **not**
        // the backend being shaped: a model asked to second-guess itself is
        // no second opinion at all. The name goes to the caller's resolver
        // so that the caller decides which model that is.
        let shaped = shape_backend(
            gates,
            cfg,
            escalate_gates.clone(),
            backend,
            &|| second(&cfg.escalate_backend),
            ghost,
        )?;
        out.insert(name, shaped);
    }
    Ok(out)
}

// ------------------------------------------------------------------ the tree

/// A backend that asks one gate as a **two-step tree**.
///
/// The gate is asked in two questions. The first asks the coarse
/// partition; the second asks the choice inside the partition the first
/// picked, and its options come from that answer.
///
/// The first question rides **in the pack**: the rest of the pack is asked
/// with it, so the tree costs one extra round trip only when the first
/// answer leaves more than one option to choose from. A partition with a
/// single option is already the answer, so nothing is asked twice.
///
/// The gate's answer is the tree's answer, synthesised over the gate's
/// own options: the options of the chosen partition share the
/// probability the first step gave that partition, and the options of the
/// other partitions are left at zero — the first step ruled them out.
/// The confidence is the deciding step's own confidence.
///
/// When the second model is given, the tree's **last** step is escalated
/// to it when that step is unsure: the step that decides the answer is
/// the one worth a second opinion.
pub struct DecomposingBackend {
    /// The fast model that answers the tree.
    primary: Box<dyn SysOne + Send + Sync>,
    /// The second model, when the tree's last step is escalated.
    second: Option<Box<dyn SysOne + Send + Sync>>,
    /// Below this confidence the last step is escalated.
    escalate_below: f32,
    /// The gate the tree is for.
    gate: String,
    /// The tree, as the harness wrote it.
    tree: Decomposition,
}

impl DecomposingBackend {
    /// A backend that asks `gate` as `tree`.
    ///
    /// The tree is the harness's own; the second model is optional, and
    /// without it the tree's last step is never escalated.
    pub fn new(
        primary: Box<dyn SysOne + Send + Sync>,
        second: Option<Box<dyn SysOne + Send + Sync>>,
        escalate_below: f32,
        gate: String,
        tree: Decomposition,
    ) -> Self {
        Self {
            primary,
            second,
            escalate_below,
            gate,
            tree,
        }
    }

    /// The gate this tree is for.
    pub fn gate(&self) -> &str {
        &self.gate
    }

    /// The first question: the coarse partition.
    fn step_one(&self) -> Value {
        let mut criteria = Map::new();
        for partition in &self.tree.partitions {
            criteria.insert(
                partition.key.clone(),
                Value::String(partition.description.clone()),
            );
        }
        json!({
            "type": "choice",
            "instructions": self.tree.instructions.clone(),
            "criteria": Value::Object(criteria),
        })
    }

    /// The second question: the choice inside one partition.
    fn step_two(&self, partition: &crate::gates::Partition) -> Value {
        let mut criteria = Map::new();
        for leaf in &partition.options {
            criteria.insert(leaf.option.clone(), Value::String(leaf.description.clone()));
        }
        json!({
            "type": "choice",
            "instructions": partition.instructions.clone(),
            "criteria": Value::Object(criteria),
        })
    }
}

impl std::fmt::Debug for DecomposingBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecomposingBackend")
            .field("primary", &self.primary.name())
            .field("second", &self.second.as_ref().map(|s| s.name()))
            .field("gate", &self.gate)
            .field("partitions", &self.tree.partitions.len())
            .finish()
    }
}

impl SysOne for DecomposingBackend {
    fn ask(&self, state: &str, questions: &Value, deadline: Instant) -> Result<Answers, Refusal> {
        // Only the pack that asks the gate is shaped. The other packs go
        // straight through.
        if questions.get(&self.gate).is_none() {
            return self.primary.ask(state, questions, deadline);
        }

        // Step 1 rides in the pack: the other questions are asked with it.
        let mut pack = questions.as_object().cloned().unwrap_or_default();
        pack.insert(self.gate.clone(), self.step_one());
        let mut answers = self.primary.ask(state, &Value::Object(pack), deadline)?;
        let pack_latency = answers.latency_ms;
        let mut steps: Vec<DecompositionStep> = Vec::new();

        // Step 1: the partition.
        let (partition_probs, partition_confidence) = match answers.per_gate.get(&self.gate) {
            Some(Answer::Choice { probs, confidence }) => (probs.clone(), *confidence),
            Some(other) => {
                return Err(Refusal::Malformed(format!(
                    "the tree's first step answered a {other:?}, not a choice"
                )))
            }
            None => {
                // The first step was unsure: the tree decides nothing.
                let confidence = answers.unsure.get(&self.gate).copied().unwrap_or(0.0);
                steps.push(DecompositionStep {
                    step: 1,
                    partition: String::new(),
                    probs: BTreeMap::new(),
                    confidence,
                    chosen: String::new(),
                    escalated: false,
                    latency_ms: pack_latency,
                });
                answers
                    .decomposed
                    .insert(self.gate.clone(), DecompositionRecord { steps });
                return Ok(answers);
            }
        };

        let partition = self.partition_of(&partition_probs).ok_or_else(|| {
            Refusal::Malformed("the tree's first step chose no partition".to_string())
        })?;
        steps.push(DecompositionStep {
            step: 1,
            partition: partition.key.clone(),
            probs: partition_probs.clone(),
            confidence: partition_confidence,
            chosen: partition.key.clone(),
            escalated: false,
            latency_ms: pack_latency,
        });

        // The share of the probability the first step gave this partition.
        let share = share_of(&partition_probs, &partition.key);

        // A partition with one option is already the answer: nothing is
        // asked twice.
        if partition.options.len() == 1 {
            let leaf = &partition.options[0];
            let mut probs = BTreeMap::new();
            probs.insert(leaf.option.clone(), share);
            answers.per_gate.insert(
                self.gate.clone(),
                Answer::Choice {
                    probs,
                    confidence: partition_confidence,
                },
            );
            answers.unsure.remove(&self.gate);
            answers
                .decomposed
                .insert(self.gate.clone(), DecompositionRecord { steps });
            return Ok(answers);
        }

        // Step 2: the choice inside the partition, asked alone.
        let question = self.step_two(partition);
        let mut one = Map::new();
        one.insert(self.gate.clone(), question.clone());
        let one = Value::Object(one);

        let step_started = Instant::now();
        // The step's own round trip, on the cheap model. The second model's
        // time is kept apart: it is the escalation's own, and it is the
        // escalation's record that carries it.
        let (mut choice_probs, mut choice_confidence, mut chosen, step_latency) =
            match self.primary.ask(state, &one, deadline) {
                Ok(second) => {
                    let latency = second.latency_ms;
                    match second.per_gate.get(&self.gate) {
                        Some(Answer::Choice { probs, confidence }) => {
                            (probs.clone(), *confidence, best_of(probs), latency)
                        }
                        _ => (BTreeMap::new(), 0.0, None, latency),
                    }
                }
                Err(Refusal::Timeout) => {
                    // The step ran out of time: the gate decides nothing.
                    // The time it took is still time the round spent.
                    (BTreeMap::new(), 0.0, None, elapsed_ms(step_started))
                }
                Err(refusal) => return Err(refusal),
            };

        // The last step is escalated when it is unsure.
        let mut escalated = false;
        let unsure = chosen.is_none() || choice_confidence < self.escalate_below;
        if unsure {
            if let Some(second) = &self.second {
                let started = Instant::now();
                let outcome = second.ask(state, &one, deadline);
                let latency = elapsed_ms(started);
                let (second_answer, refusal) = match &outcome {
                    Ok(answers) => match answers.per_gate.get(&self.gate) {
                        Some(answer) => (Some(answer.clone()), None),
                        None => (
                            None,
                            Some(Refusal::Malformed(
                                "the second model did not choose".to_string(),
                            )),
                        ),
                    },
                    Err(refusal) => (None, Some(refusal.clone())),
                };
                let mut record = Escalation {
                    gate: self.gate.clone(),
                    primary_confidence: choice_confidence,
                    primary_choice: chosen.clone(),
                    primary_probs: choice_probs.clone(),
                    second_confidence: None,
                    second_choice: None,
                    second_probs: BTreeMap::new(),
                    latency_ms: latency,
                    outcome: EscalationOutcome::Unsure,
                    refusal: None,
                };
                match second_answer {
                    Some(Answer::Choice { probs, confidence }) => {
                        let picked = best_of(&probs);
                        record.second_confidence = Some(confidence);
                        record.second_choice = picked.clone();
                        record.second_probs = probs.clone();
                        record.outcome = if picked.is_some() {
                            EscalationOutcome::Taken
                        } else {
                            EscalationOutcome::Unsure
                        };
                        choice_probs = probs;
                        choice_confidence = confidence;
                        chosen = picked;
                        escalated = true;
                    }
                    Some(_) | None => {
                        record.refusal = refusal.clone();
                        record.outcome = match refusal {
                            Some(Refusal::Timeout) => EscalationOutcome::Timeout,
                            Some(_) => EscalationOutcome::Unavailable,
                            None => EscalationOutcome::Unsure,
                        };
                    }
                }
                answers.escalated.insert(self.gate.clone(), record);
            }
        }

        steps.push(DecompositionStep {
            step: 2,
            partition: partition.key.clone(),
            probs: choice_probs.clone(),
            confidence: choice_confidence,
            chosen: chosen.clone().unwrap_or_default(),
            escalated,
            latency_ms: step_latency,
        });

        // The gate's answer: the chosen partition's options share the
        // probability the first step gave that partition; the other
        // partitions are ruled out.
        match chosen {
            Some(_) => {
                let mut probs = BTreeMap::new();
                for leaf in &partition.options {
                    let p = share * share_of(&choice_probs, &leaf.option);
                    probs.insert(leaf.option.clone(), p);
                }
                answers.per_gate.insert(
                    self.gate.clone(),
                    Answer::Choice {
                        probs,
                        confidence: choice_confidence,
                    },
                );
                answers.unsure.remove(&self.gate);
            }
            None => {
                // The last step was unsure and nobody seconded it: the
                // gate decides nothing.
                answers.per_gate.remove(&self.gate);
                answers.unsure.insert(self.gate.clone(), choice_confidence);
            }
        }

        // The pack took the tree's round trips as well as its own: both
        // are the cheap model's, so the tier reads what a call of this
        // shape costs. The second model's time is not carried here; it is
        // the escalation's own, and the escalation record has it.
        answers.latency_ms = answers.latency_ms.saturating_add(step_latency);
        answers
            .decomposed
            .insert(self.gate.clone(), DecompositionRecord { steps });
        Ok(answers)
    }

    fn name(&self) -> &str {
        self.primary.name()
    }
}

impl DecomposingBackend {
    /// The partition the first step chose.
    ///
    /// The largest probability wins and a tie goes to the first partition
    /// in key order, so the same answer always takes the same path.
    fn partition_of(&self, probs: &BTreeMap<String, f32>) -> Option<&crate::gates::Partition> {
        let mut best: Option<(&str, f32)> = None;
        for partition in &self.tree.partitions {
            let Some(p) = probs.get(&partition.key) else {
                continue;
            };
            if best.is_none_or(|(_, best)| *p > best) {
                best = Some((partition.key.as_str(), *p));
            }
        }
        let (key, _) = best?;
        self.tree.partitions.iter().find(|p| p.key == key)
    }
}

/// The share of a distribution one option holds.
///
/// The share is the option's probability over the sum of them all, so it
/// does not care how the distribution was scaled.
fn share_of(probs: &BTreeMap<String, f32>, key: &str) -> f32 {
    let total: f32 = probs.values().sum();
    if !total.is_finite() || total <= 0.0 {
        return 0.0;
    }
    probs.get(key).copied().unwrap_or(0.0) / total
}

/// The option with the largest probability, the first in key order on a tie.
fn best_of(probs: &BTreeMap<String, f32>) -> Option<String> {
    let mut best: Option<(&String, f32)> = None;
    for (key, probability) in probs {
        if best.is_none_or(|(_, best)| *probability > best) {
            best = Some((key, *probability));
        }
    }
    best.map(|(key, _)| key.clone())
}

/// The milliseconds a stretch of time took.
fn elapsed_ms(started: Instant) -> u32 {
    u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX)
}

// ------------------------------------------------------------------ the ghost

/// A backend that **ghosts** the options that rarely win.
///
/// The gate is still asked as one question, but only with the options
/// that have been winning are asked at all: the table learns, per
/// situation, which candidates the model actually picks, and the rest are
/// left out.
///
/// An option that is left out is not gone. It decays, and it is asked
/// again on a turn of its own, so an option that starts winning comes
/// back: the table is a memory of what the model does, not a decision
/// about what it may say.
pub struct PruningBackend {
    /// The fast model that answers the gate.
    primary: Box<dyn SysOne + Send + Sync>,
    /// The gate to ghost.
    gate: String,
    /// How many options the gate is asked with.
    k: usize,
    /// What the table has learned.
    table: Mutex<GhostTable>,
    /// The asks so far, for the telemetry.
    asks: Mutex<Vec<PrunedAsk>>,
}

/// One ask of a ghosted gate: what was asked and what was won.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrunedAsk {
    /// The situation the ask was for.
    pub site: u64,
    /// The options the gate was asked with.
    pub asked: Vec<String>,
    /// The option the model chose, when it chose one.
    pub winner: Option<String>,
}

/// What the ghost table remembers.
///
/// The scores are whole numbers so the table is exact: the same asks
/// always give the same scores, whatever the order they arrive in.
#[derive(Clone, Debug, Default)]
struct GhostTable {
    /// The score of each option, per situation.
    per_site: BTreeMap<u64, BTreeMap<String, i64>>,
    /// The score of each option, over every situation.
    global: BTreeMap<String, i64>,
    /// How many times each situation was asked: whose turn the probe is on.
    asks: BTreeMap<u64, u64>,
}

/// How much a score decays on every ask.
const DECAY_NUM: i64 = 7;
/// The denominator of the decay.
const DECAY_DEN: i64 = 8;
/// What a win is worth.
const WIN: i64 = 16;

impl GhostTable {
    /// The options of a situation, best first.
    ///
    /// The best score wins and a tie goes to the first option in key
    /// order, so the same table always asks the same options.
    fn ranking(&self, site: u64, options: &[String]) -> Vec<String> {
        let mut scored: Vec<(String, i64)> = options
            .iter()
            .map(|option| {
                let score = self
                    .per_site
                    .get(&site)
                    .and_then(|site| site.get(option))
                    .copied()
                    .or_else(|| self.global.get(option).copied())
                    .unwrap_or(0);
                (option.clone(), score)
            })
            .collect();
        // A stable sort keeps the key order of the ties.
        scored.sort_by_key(|(_, score)| std::cmp::Reverse(*score));
        scored.into_iter().map(|(option, _)| option).collect()
    }

    /// The options to ask: the best `k` less one, and one option of the
    /// table's own choosing.
    ///
    /// The last seat turns around: every option gets asked on a turn of
    /// its own, so an option that starts winning is asked often enough
    /// to be seen winning.
    fn ask_set(&self, site: u64, options: &[String], k: usize) -> Vec<String> {
        if options.len() <= k {
            return options.to_vec();
        }
        let ranking = self.ranking(site, options);
        let turn = self.asks.get(&site).copied().unwrap_or(0) as usize;
        let probe = ranking[turn % ranking.len()].clone();
        // The probe is always asked; the rest of the seats go to the
        // best options the table knows.
        let mut asked = vec![probe];
        for option in &ranking {
            if asked.len() >= k {
                break;
            }
            if !asked.contains(option) {
                asked.push(option.clone());
            }
        }
        asked
    }

    /// What the table learned from one ask.
    fn record(&mut self, site: u64, options: &[String], winner: Option<&str>) {
        let entry = self.per_site.entry(site).or_default();
        for option in options {
            let score = entry.get(option).copied().unwrap_or(0);
            entry.insert(option.clone(), score * DECAY_NUM / DECAY_DEN);
        }
        for option in options {
            let score = self.global.get(option).copied().unwrap_or(0);
            self.global
                .insert(option.clone(), score * DECAY_NUM / DECAY_DEN);
        }
        if let Some(winner) = winner {
            *entry.entry(winner.to_string()).or_insert(0) += WIN;
            *self.global.entry(winner.to_string()).or_insert(0) += WIN;
        }
        *self.asks.entry(site).or_insert(0) += 1;
    }
}

impl PruningBackend {
    /// A backend that asks `gate` with `k` options.
    pub fn new(primary: Box<dyn SysOne + Send + Sync>, gate: String, k: usize) -> Self {
        Self {
            primary,
            gate,
            k: k.max(1),
            table: Mutex::new(GhostTable::default()),
            asks: Mutex::new(Vec::new()),
        }
    }

    /// The gate this backend ghosts.
    pub fn gate(&self) -> &str {
        &self.gate
    }

    /// How many options the gate is asked with.
    pub fn k(&self) -> usize {
        self.k
    }

    /// The asks so far, oldest first.
    pub fn asks(&self) -> Vec<PrunedAsk> {
        self.asks.lock().expect("the asks are not poisoned").clone()
    }

    /// Teach the table what the model chose in a situation.
    ///
    /// The table learns from the **logged answers**: a round that recorded
    /// which option won a gate can hand that record to the table, and the
    /// next ask leaves out the options that never win. This is how the
    /// table is warmed before it is asked, instead of being asked blind
    /// until it finds out by itself.
    ///
    /// The situation is the facts text, exactly as the table keys it
    /// itself, so a remembered answer and an asked answer land on the
    /// same site.
    pub fn remember(&self, state: &str, options: &[String], winner: Option<&str>) {
        let mut table = self.table.lock().expect("the table is not poisoned");
        table.record(site_of(state), options, winner);
    }

    /// Teach the table everything the log holds.
    ///
    /// The log is the round's own record of what the gate chose, per
    /// situation: the table learns it all at once, so the gate is asked
    /// with the options that have been winning from the first ask.
    pub fn learn(&self, log: &GhostLog) {
        let mut table = self.table.lock().expect("the table is not poisoned");
        for (state, options, winner) in log.entries() {
            table.record(site_of(state), options, winner.as_deref());
        }
    }
}

impl std::fmt::Debug for PruningBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PruningBackend")
            .field("primary", &self.primary.name())
            .field("gate", &self.gate)
            .field("k", &self.k)
            .finish()
    }
}

impl SysOne for PruningBackend {
    fn ask(&self, state: &str, questions: &Value, deadline: Instant) -> Result<Answers, Refusal> {
        let Some(question) = questions.get(&self.gate).cloned() else {
            return self.primary.ask(state, questions, deadline);
        };
        let Some(criteria) = question.get("criteria").and_then(Value::as_object) else {
            return self.primary.ask(state, questions, deadline);
        };
        let options: Vec<String> = criteria.keys().cloned().collect();
        let site = site_of(state);

        let asked = {
            let table = self.table.lock().expect("the table is not poisoned");
            table.ask_set(site, &options, self.k)
        };

        // The question, with the ghosted options left out.
        let mut pruned_criteria = Map::new();
        for option in &asked {
            if let Some(text) = criteria.get(option) {
                pruned_criteria.insert(option.clone(), text.clone());
            }
        }
        let mut pruned = question.clone();
        pruned["criteria"] = Value::Object(pruned_criteria);
        let mut pack = questions.as_object().cloned().unwrap_or_default();
        pack.insert(self.gate.clone(), pruned);
        let answers = self.primary.ask(state, &Value::Object(pack), deadline)?;

        let winner = answers
            .per_gate
            .get(&self.gate)
            .and_then(|answer| match answer {
                Answer::Choice { probs, .. } => best_of(probs),
                _ => None,
            });
        {
            let mut table = self.table.lock().expect("the table is not poisoned");
            table.record(site, &options, winner.as_deref());
        }
        self.asks
            .lock()
            .expect("the asks are not poisoned")
            .push(PrunedAsk {
                site,
                asked: asked.clone(),
                winner,
            });
        Ok(answers)
    }

    fn name(&self) -> &str {
        self.primary.name()
    }
}

/// The situation a question was asked in.
///
/// The facts text is the situation, so the table learns per situation.
/// The hash is FNV-1a: it is whole-number exact and the same text always
/// gives the same number.
fn site_of(state: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in state.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ShapeConfig;
    use crate::gates::{Decomposition, Leaf, Partition};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// A backend that answers a named option and records what it was
    /// asked.
    #[derive(Clone)]
    struct Recorder {
        /// The option it picks for every choice question.
        pick: String,
        /// The confidence it reports.
        confidence: f32,
        /// The packs it was asked.
        packs: Arc<Mutex<Vec<Value>>>,
        calls: Arc<AtomicUsize>,
        /// How long it pretends to take, in milliseconds.
        ///
        /// Zero for most recorders; a slow one is what shows that the
        /// second model's time is not charged to the pack.
        sleep_ms: u64,
    }

    impl Recorder {
        fn new(pick: &str, confidence: f32) -> Self {
            Self {
                pick: pick.to_string(),
                confidence,
                packs: Arc::new(Mutex::new(Vec::new())),
                calls: Arc::new(AtomicUsize::new(0)),
                sleep_ms: 0,
            }
        }

        /// A recorder that takes its time before it answers.
        fn slow(pick: &str, confidence: f32, sleep_ms: u64) -> Self {
            Self {
                sleep_ms,
                ..Self::new(pick, confidence)
            }
        }

        /// A recorder whose calls are counted in a shared counter.
        fn counted(pick: &str, confidence: f32, calls: Arc<AtomicUsize>) -> Self {
            Self {
                pick: pick.to_string(),
                confidence,
                packs: Arc::new(Mutex::new(Vec::new())),
                calls,
                sleep_ms: 0,
            }
        }

        /// A recorder whose calls and packs are both shared.
        fn shared(
            pick: &str,
            confidence: f32,
            calls: Arc<AtomicUsize>,
            packs: Arc<Mutex<Vec<Value>>>,
        ) -> Self {
            Self {
                pick: pick.to_string(),
                confidence,
                packs,
                calls,
                sleep_ms: 0,
            }
        }
    }

    impl SysOne for Recorder {
        fn ask(
            &self,
            _state: &str,
            questions: &Value,
            _deadline: Instant,
        ) -> Result<Answers, Refusal> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.sleep_ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(self.sleep_ms));
            }
            self.packs
                .lock()
                .expect("the packs are not poisoned")
                .push(questions.clone());
            let mut per_gate = BTreeMap::new();
            for (id, gate) in questions.as_object().expect("a table") {
                let kind = gate.get("type").and_then(Value::as_str).unwrap_or("choice");
                let answer = match kind {
                    "noul" => Answer::Noul(0.8),
                    _ => {
                        let options: Vec<&String> = gate
                            .get("criteria")
                            .and_then(Value::as_object)
                            .expect("options")
                            .keys()
                            .collect();
                        let top = 0.5 + 0.5 * self.confidence;
                        let rest = options.len().saturating_sub(1).max(1) as f32;
                        let mut probs = BTreeMap::new();
                        for option in options {
                            let p = if option == &self.pick {
                                top
                            } else {
                                (1.0 - top) / rest
                            };
                            probs.insert(option.clone(), p);
                        }
                        Answer::Choice {
                            probs,
                            confidence: self.confidence,
                        }
                    }
                };
                per_gate.insert(id.clone(), answer);
            }
            Ok(Answers {
                questions: BTreeMap::new(),
                per_gate,
                unsure: BTreeMap::new(),
                escalated: BTreeMap::new(),
                decomposed: BTreeMap::new(),
                model: "recorder".to_string(),
                input_tokens: 64,
                latency_ms: 10,
            })
        }

        fn name(&self) -> &str {
            "recorder"
        }
    }

    /// A tree over three options: a dead end, a pair and a single.
    fn tree() -> Decomposition {
        Decomposition {
            instructions: "Who is it for, in coarse terms?".to_string(),
            partitions: vec![
                Partition {
                    key: "squad".to_string(),
                    description: "your squad as a whole".to_string(),
                    instructions: "Your squad as a whole?".to_string(),
                    options: vec![Leaf {
                        option: "squad".to_string(),
                        description: "your squad".to_string(),
                    }],
                },
                Partition {
                    key: "own".to_string(),
                    description: "one agent of your own squad".to_string(),
                    instructions: "Which agent of your own squad?".to_string(),
                    options: vec![
                        Leaf {
                            option: "agent0".to_string(),
                            description: "agent 0 in particular".to_string(),
                        },
                        Leaf {
                            option: "agent1".to_string(),
                            description: "agent 1 in particular".to_string(),
                        },
                    ],
                },
                Partition {
                    key: "all".to_string(),
                    description: "everybody who can hear".to_string(),
                    instructions: "Everybody?".to_string(),
                    options: vec![Leaf {
                        option: "all".to_string(),
                        description: "everybody".to_string(),
                    }],
                },
            ],
        }
    }

    /// The pack of one gate with three options.
    fn pack() -> Value {
        json!({
            "attention": {
                "type": "choice",
                "instructions": "Who the line is for?",
                "criteria": {"squad": "your squad", "all": "everybody", "agent0": "agent 0", "agent1": "agent 1"},
            }
        })
    }

    #[test]
    fn the_tree_asks_the_partition_then_the_choice() {
        // The first question rides in the pack and asks the partitions;
        // the second asks the choice inside the partition that was picked.
        let primary = Recorder::new("own", 0.8);
        let packs = Arc::clone(&primary.packs);
        let backend = DecomposingBackend::new(
            Box::new(primary),
            None,
            0.5,
            "attention".to_string(),
            tree(),
        );
        let answers = backend
            .ask(
                "the facts",
                &pack(),
                Instant::now() + std::time::Duration::from_secs(5),
            )
            .expect("the tree answers");
        assert!(
            answers.decomposed.contains_key("attention"),
            "the tree logged"
        );

        let packs = packs.lock().expect("the packs are not poisoned");
        assert_eq!(packs.len(), 2, "the tree asked twice");
        // The first ask carried the partitions.
        let first = packs[0].get("attention").expect("the gate");
        let criteria = first
            .get("criteria")
            .and_then(Value::as_object)
            .expect("criteria");
        assert_eq!(criteria.len(), 3, "the first step asks the partitions");
        assert!(criteria.contains_key("own"));
        // The second ask carried the choice inside the partition.
        let second = packs[1].get("attention").expect("the gate");
        let criteria = second
            .get("criteria")
            .and_then(Value::as_object)
            .expect("criteria");
        assert_eq!(
            criteria.len(),
            2,
            "the second step asks inside the partition"
        );
        assert!(criteria.contains_key("agent0"));
        assert!(criteria.contains_key("agent1"));
        assert!(!criteria.contains_key("squad"));
    }

    #[test]
    fn the_tree_answers_the_gate_in_the_gates_own_options() {
        // The gate's answer names the leaf the tree picked, and the
        // options the tree ruled out are left at zero.
        let primary = Recorder::new("own", 0.8);
        let backend = DecomposingBackend::new(
            Box::new(primary),
            None,
            0.5,
            "attention".to_string(),
            tree(),
        );
        let answers = backend
            .ask(
                "the facts",
                &pack(),
                Instant::now() + std::time::Duration::from_secs(5),
            )
            .expect("the tree answers");
        let Some(Answer::Choice { probs, confidence }) = answers.per_gate.get("attention") else {
            panic!("the gate chose");
        };
        assert!(probs.contains_key("agent0") && probs.contains_key("agent1"));
        assert!(
            !probs.contains_key("squad"),
            "the other partitions are ruled out"
        );
        assert!((confidence - 0.8).abs() < 0.01, "got {confidence}");
    }

    #[test]
    fn a_dead_end_partition_asks_once() {
        // A partition with one option is already the answer: nothing is
        // asked twice.
        let primary = Recorder::new("squad", 0.8);
        let calls = Arc::clone(&primary.calls);
        let backend = DecomposingBackend::new(
            Box::new(primary),
            None,
            0.5,
            "attention".to_string(),
            tree(),
        );
        let answers = backend
            .ask(
                "the facts",
                &pack(),
                Instant::now() + std::time::Duration::from_secs(5),
            )
            .expect("the tree answers");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let Some(Answer::Choice { probs, .. }) = answers.per_gate.get("attention") else {
            panic!("the gate chose");
        };
        assert!(probs.contains_key("squad"));
    }

    #[test]
    fn the_tree_logs_both_answers() {
        // Both answers are logged, and the log says which path was taken.
        let primary = Recorder::new("own", 0.8);
        let backend = DecomposingBackend::new(
            Box::new(primary),
            None,
            0.5,
            "attention".to_string(),
            tree(),
        );
        let answers = backend
            .ask(
                "the facts",
                &pack(),
                Instant::now() + std::time::Duration::from_secs(5),
            )
            .expect("the tree answers");
        let record = answers.decomposed.get("attention").expect("the record");
        assert_eq!(record.steps.len(), 2);
        assert_eq!(record.steps[0].step, 1);
        assert_eq!(record.steps[0].chosen, "own");
        assert_eq!(record.steps[1].step, 2);
        assert_eq!(record.steps[1].partition, "own");
        assert!(!record.steps[1].escalated);
    }

    #[test]
    fn the_tree_escalates_its_last_step() {
        // The step that decides the answer is the one worth a second
        // opinion.
        let primary = Recorder::new("own", 0.2);
        let second = Recorder::new("agent1", 0.9);
        let second_calls = Arc::clone(&second.calls);
        let backend = DecomposingBackend::new(
            Box::new(primary),
            Some(Box::new(second)),
            0.5,
            "attention".to_string(),
            tree(),
        );
        let answers = backend
            .ask(
                "the facts",
                &pack(),
                Instant::now() + std::time::Duration::from_secs(5),
            )
            .expect("the tree answers");
        assert_eq!(second_calls.load(Ordering::SeqCst), 1);
        let record = answers.decomposed.get("attention").expect("the record");
        assert!(record.steps[1].escalated);
        let escalation = answers.escalated.get("attention").expect("the escalation");
        assert_eq!(escalation.outcome, EscalationOutcome::Taken);
        // The cheap model chose inside the partition; the second model
        // chose something else.
        assert_eq!(escalation.primary_choice.as_deref(), Some("agent0"));
        assert_eq!(escalation.second_choice.as_deref(), Some("agent1"));
    }

    #[test]
    fn a_pack_without_the_gate_goes_straight_through() {
        let primary = Recorder::new("own", 0.8);
        let calls = Arc::clone(&primary.calls);
        let backend = DecomposingBackend::new(
            Box::new(primary),
            None,
            0.5,
            "attention".to_string(),
            tree(),
        );
        let other = json!({
            "move": {
                "type": "choice",
                "instructions": "What now?",
                "criteria": {"a": "a", "b": "b"},
            }
        });
        backend
            .ask(
                "the facts",
                &other,
                Instant::now() + std::time::Duration::from_secs(5),
            )
            .expect("the pack is answered");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn the_ghost_asks_only_k_options() {
        // The gate is still one question; it is just asked with fewer
        // options.
        let primary = Recorder::new("agent0", 0.8);
        let packs = Arc::clone(&primary.packs);
        let backend = PruningBackend::new(Box::new(primary), "attention".to_string(), 2);
        backend
            .ask(
                "the facts",
                &pack(),
                Instant::now() + std::time::Duration::from_secs(5),
            )
            .expect("the pack is answered");
        let packs = packs.lock().expect("the packs are not poisoned");
        let criteria = packs[0]
            .get("attention")
            .and_then(|q| q.get("criteria"))
            .and_then(Value::as_object)
            .expect("criteria");
        assert_eq!(criteria.len(), 2, "the gate is asked with k options");
    }

    #[test]
    fn the_ghost_learns_the_winner_and_keeps_it() {
        // The table learns what the model picks: the winner stays in the
        // ask, and the options that never win are left out.
        let mut table = GhostTable::default();
        let options: Vec<String> = ["a", "b", "c", "d"].iter().map(|s| s.to_string()).collect();
        for _ in 0..20 {
            table.record(7, &options, Some("c"));
        }
        let ranking = table.ranking(7, &options);
        assert_eq!(ranking[0], "c", "the winner is first");
        let asked = table.ask_set(7, &options, 2);
        assert!(
            asked.contains(&"c".to_string()),
            "the winner is asked: {asked:?}"
        );
        assert_eq!(asked.len(), 2);
    }

    #[test]
    fn a_ghosted_option_comes_back_when_it_starts_winning() {
        // An option that is left out decays, but it is asked again on a
        // turn of its own: a table that forgets is not a table.
        let mut table = GhostTable::default();
        let options: Vec<String> = ["a", "b", "c", "d"].iter().map(|s| s.to_string()).collect();
        for _ in 0..20 {
            table.record(7, &options, Some("a"));
        }
        let before = table.ask_set(7, &options, 2);
        assert!(
            !before.contains(&"d".to_string()),
            "d is ghosted: {before:?}"
        );
        // d wins when it is asked.
        table.record(7, &options, Some("d"));
        let after = table.ask_set(7, &options, 2);
        assert!(after.contains(&"d".to_string()), "d is back: {after:?}");
    }

    /// The harness's gates with the tree of the attention gate.
    const HARNESS_GATES: &str = r#"
        [[gates]]
        id = "attention"
        type = "choice"
        options_from = "fixed"
        instructions = "Who the line is for?"

        [gates.criteria]
        squad = "your squad"
        all = "everybody"
        agent0 = "agent 0 in particular"
        agent1 = "agent 1 in particular"

        [gates.decompose]
        instructions = "Who is it for, in coarse terms?"

        [[gates.decompose.partitions]]
        key = "squad"
        description = "your squad as a whole"
        instructions = "Your squad as a whole?"

        [[gates.decompose.partitions.options]]
        option = "squad"
        description = "your squad"

        [[gates.decompose.partitions]]
        key = "one"
        description = "one agent in particular"
        instructions = "Which agent in particular?"

        [[gates.decompose.partitions.options]]
        option = "agent0"
        description = "agent 0 in particular"

        [[gates.decompose.partitions.options]]
        option = "agent1"
        description = "agent 1 in particular"

        [[gates.decompose.partitions]]
        key = "all"
        description = "everybody who can hear"
        instructions = "Everybody?"

        [[gates.decompose.partitions.options]]
        option = "all"
        description = "everybody"
    "#;

    #[test]
    fn the_shape_builds_a_plain_backend() {
        // Nothing named: the gate is asked as it is written, and the
        // second opinion is bought for the gates the parameters name.
        let gates = GateSet::parse(HARNESS_GATES).expect("the gates parse");
        let cfg = AgentsConfig::default();
        let escalate_gates = Some(BTreeSet::from(["attention".to_string()]));
        let second_calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&second_calls);
        let backend = shape_backend(
            &gates,
            &cfg,
            escalate_gates,
            Box::new(Recorder::new("agent0", 0.2)),
            &move || {
                Some(Box::new(Recorder::counted(
                    "agent1",
                    0.9,
                    Arc::clone(&counter),
                )))
            },
            None,
        )
        .expect("the backend is built");
        let answers = backend
            .ask(
                "the facts",
                &pack(),
                Instant::now() + std::time::Duration::from_secs(5),
            )
            .expect("the pack is answered");
        assert!(answers.decomposed.is_empty(), "the gate was not a tree");
        assert!(
            answers.escalated.contains_key("attention"),
            "the gate was escalated"
        );
        assert_eq!(
            second_calls.load(Ordering::SeqCst),
            1,
            "the second model was asked"
        );
    }

    #[test]
    fn the_shape_builds_a_tree() {
        // The gate named as a tree is asked as one, and the second
        // opinion is not bought for it: the tree escalates its own last
        // step when it is asked to.
        let gates = GateSet::parse(HARNESS_GATES).expect("the gates parse");
        let cfg = AgentsConfig {
            shape: ShapeConfig {
                tree_gate: Some("attention".to_string()),
                ..ShapeConfig::default()
            },
            ..AgentsConfig::default()
        };
        let escalate_gates = Some(BTreeSet::from(["attention".to_string()]));
        let second_calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&second_calls);
        let backend = shape_backend(
            &gates,
            &cfg,
            escalate_gates,
            Box::new(Recorder::new("one", 0.8)),
            &move || {
                Some(Box::new(Recorder::counted(
                    "agent1",
                    0.9,
                    Arc::clone(&counter),
                )))
            },
            None,
        )
        .expect("the backend is built");
        let answers = backend
            .ask(
                "the facts",
                &pack(),
                Instant::now() + std::time::Duration::from_secs(5),
            )
            .expect("the pack is answered");
        assert!(
            answers.decomposed.contains_key("attention"),
            "the gate was a tree"
        );
        assert!(
            !answers.escalated.contains_key("attention"),
            "the tree did not escalate: the last step was sure"
        );
        assert_eq!(
            second_calls.load(Ordering::SeqCst),
            0,
            "the second model was not asked"
        );
    }

    #[test]
    fn the_shape_builds_a_ghost() {
        // The gate named as a ghost is asked with fewer options, and the
        // second opinion is bought for it as it is for any other gate.
        let gates = GateSet::parse(HARNESS_GATES).expect("the gates parse");
        let mut cfg = AgentsConfig::default();
        cfg.shape.prune_gate = Some("attention".to_string());
        cfg.shape.prune_k = 2;
        let escalate_gates = Some(BTreeSet::from(["attention".to_string()]));
        let backend = shape_backend(
            &gates,
            &cfg,
            escalate_gates,
            Box::new(Recorder::new("agent0", 0.2)),
            &|| Some(Box::new(Recorder::new("agent1", 0.9))),
            None,
        )
        .expect("the backend is built");
        let answers = backend
            .ask(
                "the facts",
                &pack(),
                Instant::now() + std::time::Duration::from_secs(5),
            )
            .expect("the pack is answered");
        assert!(answers.decomposed.is_empty(), "the gate was not a tree");
        assert!(answers.escalated.contains_key("attention"));
    }

    #[test]
    fn a_tree_gate_is_bought_once() {
        // The tree escalates its own last step, so the escalation around
        // the tree must leave that gate alone: the second opinion is
        // bought once and only once. This holds even when the filter
        // names **every** gate, which is what no filter means.
        let gates = GateSet::parse(HARNESS_GATES).expect("the gates parse");
        let cfg = AgentsConfig {
            shape: ShapeConfig {
                tree_gate: Some("attention".to_string()),
                escalate_tree_step: true,
                ..ShapeConfig::default()
            },
            ..AgentsConfig::default()
        };
        // No filter: every gate would be escalated.
        let escalate_gates = None;
        let second_calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&second_calls);
        let backend = shape_backend(
            &gates,
            &cfg,
            escalate_gates,
            // Unsure: the tree's last step is escalated.
            Box::new(Recorder::new("one", 0.1)),
            &move || {
                Some(Box::new(Recorder::counted(
                    "agent1",
                    0.1,
                    Arc::clone(&counter),
                )))
            },
            None,
        )
        .expect("the backend is built");
        let answers = backend
            .ask(
                "the facts",
                &pack(),
                Instant::now() + std::time::Duration::from_secs(5),
            )
            .expect("the pack is answered");
        assert!(
            answers.escalated.contains_key("attention"),
            "the tree escalated its last step"
        );
        assert_eq!(
            second_calls.load(Ordering::SeqCst),
            1,
            "the second model was asked once, by the tree"
        );
    }

    #[test]
    fn the_second_opinion_is_another_model() {
        // The second opinion has to come from **another** model. A model
        // asked to second-guess itself agrees with itself, and the
        // measurement then reads that as the second opinion buying
        // nothing — which is a fact about the wiring, not about the
        // models.
        let gates = GateSet::parse(HARNESS_GATES).expect("the gates parse");
        let cfg = AgentsConfig {
            escalate_backend: "second".to_string(),
            ..AgentsConfig::default()
        };
        let escalate_gates = Some(BTreeSet::from(["attention".to_string()]));
        // The name the resolver was asked for, so the test can see which
        // model the shape reached for.
        let asked_for: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let mut backends = BTreeMap::new();
        backends.insert(
            "cheap".to_string(),
            Box::new(Recorder::new("squad", 0.2)) as Box<dyn SysOne + Send + Sync>,
        );
        backends.insert(
            "second".to_string(),
            Box::new(Recorder::new("agent0", 0.9)) as Box<dyn SysOne + Send + Sync>,
        );
        let names = Arc::clone(&asked_for);
        let shaped = shape_backends_with(
            &gates,
            &cfg,
            escalate_gates,
            backends,
            move |name| {
                names
                    .lock()
                    .expect("the names are not poisoned")
                    .push(name.to_owned());
                Some(Box::new(Recorder::new("agent0", 0.9)) as Box<dyn SysOne + Send + Sync>)
            },
            None,
        )
        .expect("the backends are built");
        let cheap = shaped.get("cheap").expect("the cheap backend is there");
        let answers = cheap
            .ask(
                "the facts",
                &pack(),
                Instant::now() + std::time::Duration::from_secs(5),
            )
            .expect("the pack is answered");
        let escalation = answers
            .escalated
            .get("attention")
            .expect("the gate was escalated");
        assert_eq!(
            escalation.second_choice.as_deref(),
            Some("agent0"),
            "the second opinion came from the other model"
        );
        assert!(
            asked_for
                .lock()
                .expect("the names are not poisoned")
                .iter()
                .all(|name| name == "second"),
            "the shape asked for the escalate backend, not the one it was shaping"
        );
    }

    #[test]
    fn the_round_is_shaped_through_one_builder() {
        // The game builds its backends through `shape_backends`, the same
        // way the measurement does: one builder, so an arm of the
        // measurement is the shipped code and not a re-implementation of
        // it. The gates a second opinion is bought for are the harness's
        // and are passed in (D75).
        let gates = GateSet::parse(HARNESS_GATES).expect("the gates parse");
        let cfg = AgentsConfig {
            escalate_backend: "second".to_string(),
            ..AgentsConfig::default()
        };
        let escalate_gates = Some(BTreeSet::from(["attention".to_string()]));
        let mut backends = BTreeMap::new();
        backends.insert(
            "cheap".to_string(),
            Box::new(Recorder::new("agent0", 0.2)) as Box<dyn SysOne + Send + Sync>,
        );
        backends.insert(
            "second".to_string(),
            Box::new(Recorder::new("agent1", 0.9)) as Box<dyn SysOne + Send + Sync>,
        );
        let shaped = shape_backends(&gates, &cfg, escalate_gates, backends, |_| {
            Some(Box::new(Recorder::new("agent0", 0.9)) as Box<dyn SysOne + Send + Sync>)
        })
        .expect("the backends are built");
        assert_eq!(shaped.len(), 2, "every backend is there");
        let cheap = shaped.get("cheap").expect("the cheap backend is there");
        let answers = cheap
            .ask(
                "the facts",
                &pack(),
                Instant::now() + std::time::Duration::from_secs(5),
            )
            .expect("the pack is answered");
        // The gate the harness named is the one that was escalated.
        assert!(
            answers.escalated.contains_key("attention"),
            "the gate the harness named was escalated"
        );
        assert_eq!(answers.escalated.len(), 1, "no other gate was escalated");
    }

    #[test]
    fn the_tree_does_not_charge_the_pack_for_the_second_model() {
        // The pack's latency is what a call of this shape costs, so the
        // tier can budget for it. The second model's time is the
        // escalation's own, and it is not part of what the cheap model
        // costs: the escalation record carries it instead.
        let primary = Recorder::new("own", 0.8);
        let backend = DecomposingBackend::new(
            Box::new(primary),
            // The second model is slow, so its time would show if the
            // pack were charged for it.
            Some(Box::new(Recorder::slow("agent0", 0.9, 60))),
            // The cheap model is unsure in the tree's last step, so the
            // last step is escalated.
            0.9,
            "attention".to_string(),
            tree(),
        );
        let answers = backend
            .ask(
                "the facts",
                &pack(),
                Instant::now() + std::time::Duration::from_secs(5),
            )
            .expect("the tree answers");
        let escalation = answers
            .escalated
            .get("attention")
            .expect("the tree escalated its last step");
        assert!(
            escalation.latency_ms >= 50,
            "the second model's time was {} ms",
            escalation.latency_ms
        );
        // The pack's own round trip (10 ms) and the tree's second step
        // (10 ms): both the cheap model's, and no more.
        assert_eq!(
            answers.latency_ms, 20,
            "the pack carried the second model's time"
        );
    }

    #[test]
    fn the_ghost_escalates_the_ghosted_question() {
        // The second model is asked the question as it was asked: the
        // ghosted one, with the ghosted options left out. A second
        // opinion of the full question would undo the ghosting, and the
        // arm would not be measuring what it says it measures.
        let gates = GateSet::parse(HARNESS_GATES).expect("the gates parse");
        let mut cfg = AgentsConfig::default();
        cfg.shape.prune_gate = Some("attention".to_string());
        cfg.shape.prune_k = 2;
        let escalate_gates = Some(BTreeSet::from(["attention".to_string()]));
        let second_calls = Arc::new(AtomicUsize::new(0));
        let second_packs = Arc::new(Mutex::new(Vec::new()));
        let calls = Arc::clone(&second_calls);
        let packs = Arc::clone(&second_packs);
        let backend = shape_backend(
            &gates,
            &cfg,
            escalate_gates,
            // Unsure: the gate is escalated.
            Box::new(Recorder::new("agent0", 0.2)),
            &move || {
                Some(Box::new(Recorder::shared(
                    "agent1",
                    0.9,
                    Arc::clone(&calls),
                    Arc::clone(&packs),
                )))
            },
            None,
        )
        .expect("the backend is built");
        backend
            .ask(
                "the facts",
                &pack(),
                Instant::now() + std::time::Duration::from_secs(5),
            )
            .expect("the pack is answered");
        assert_eq!(
            second_calls.load(Ordering::SeqCst),
            1,
            "the gate was escalated"
        );
        let packs = second_packs.lock().expect("the packs are not poisoned");
        let criteria = packs[0]
            .get("attention")
            .and_then(|question| question.get("criteria"))
            .and_then(Value::as_object)
            .expect("criteria");
        assert_eq!(
            criteria.len(),
            2,
            "the second model was asked the ghosted question"
        );
    }

    #[test]
    fn a_gate_named_as_a_tree_without_one_is_refused() {
        // The tree is the harness's: a parameter that names a gate the
        // harness has no tree for is a mistake, not a silent fallback.
        let gates = GateSet::parse(HARNESS_GATES).expect("the gates parse");
        let mut cfg = AgentsConfig::default();
        cfg.shape.tree_gate = Some("gesture".to_string());
        let error = match shape_backend(
            &gates,
            &cfg,
            None,
            Box::new(Recorder::new("one", 0.8)),
            &|| Some(Box::new(Recorder::new("agent1", 0.9))),
            None,
        ) {
            Ok(_) => panic!("the tree is refused"),
            Err(error) => error,
        };
        assert_eq!(error, ShapeError::NoTree("gesture".to_string()));
    }

    #[test]
    fn the_probe_asks_every_option_in_turn() {
        // Every option gets asked on a turn of its own, so an option that
        // starts winning is asked often enough to be seen winning.
        let mut table = GhostTable::default();
        let options: Vec<String> = ["a", "b", "c", "d"].iter().map(|s| s.to_string()).collect();
        let mut seen: Vec<String> = Vec::new();
        for _ in 0..8 {
            let asked = table.ask_set(7, &options, 2);
            assert_eq!(asked.len(), 2);
            for option in asked {
                if !seen.contains(&option) {
                    seen.push(option);
                }
            }
            table.record(7, &options, Some("a"));
        }
        assert_eq!(seen.len(), 4, "every option was asked: {seen:?}");
    }
}
