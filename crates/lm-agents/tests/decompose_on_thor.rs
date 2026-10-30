//! The decomposition arms on the real Thor: the same attention gate asked
//! four ways, and a neutral judge to say which answer was better.
//!
//! The question under test is the one the harness's own notes ask: the
//! attention gate is ten options wide, the d1-2B is unsure on it (0.29-0.64
//! over a round), and an unsure gate was answered with a second opinion
//! from clef-flash. D75 says clef-flash is usually **worse** on the
//! ten-option talk gates, so the second opinion may be buying a worse
//! answer and a second round trip.
//!
//! The other way out is to ask the same choice differently: as a
//! two-step tree, where the first question picks a coarse partition and
//! the second picks inside it. The tree is the harness's own
//! (`gates.toml`), and it is off unless a parameter asks for it.
//!
//! ## The arms
//!
//! - **A, escalate**: the ten-option gate, escalated to clef-flash below
//!   0.5. This is the arm the note called "today"; since D75 the shipped
//!   harness escalates the move gate alone, so it is the attention gate
//!   escalated, not the attention gate as the harness runs it.
//! - **B, tree**: the same gate as a two-step tree, on the d1-2B alone.
//! - **C, tree + escalate**: B, with the tree's last step escalated.
//! - **D, ghost**: the ten-option gate with its rarely-chosen options
//!   ghosted: only the options that have been winning are asked at all.
//!
//! The arms differ **only** in the shape of the attention gate. Every
//! other gate, and the rest of the round, is the shipped code.
//!
//! ## What is measured
//!
//! The arms are asked over the **same situations**: the distinct facts
//! texts the shipped round asked the attention gate in. Each situation is
//! asked by every arm, and by the judge, so the answers can be compared
//! one against another.
//!
//! The judge is the Thor's **d1-3B**, asked the full ten-option question
//! with the same facts, as esctune did: it is a third model, not one of
//! the two under test, and it never answers the round.
//!
//! ## The command
//!
//! ```text
//! cargo test --release -p lm-agents --test decompose_on_thor -- --ignored --nocapture
//! ```

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lm_agents::config::{AgentsConfig, ShapeConfig};
use lm_agents::decompose::{shape_backends_with, GhostLog};
use lm_agents::gates::{GateSet, Pack};
use lm_agents::harness::Harness;
use lm_agents::hodge::{decompose as decompose_preferences, Part, View};
use lm_agents::mind::Policy;
use lm_agents::sysone::{
    Answer, Answers, DecompositionStep, Escalation, Refusal, SysOne, ThorBackend,
};
use lm_agents::Agents;
use lm_core::header::Header;
use lm_core::talk::talk_metrics;
use lm_host_model::HostLoop;
use serde_json::{json, Value};

/// The shipped harness: the gates, the policy and the phrasebook the
/// default round thinks with.
const DEFAULT_GATES: &str = include_str!("../../../assets/harness/default/gates.toml");
const DEFAULT_POLICY: &str = include_str!("../../../assets/harness/default/policy.toml");
const DEFAULT_PHRASEBOOK: &str = include_str!("../../../assets/harness/default/phrasebook.toml");

/// The shipped reflex, compiled into the test binary.
const DEFAULT_REFLEX: &str = include_str!("../../../assets/reflexes/default.rhai");

/// The ticks a round is played for: a hundred seconds at 30 Hz.
const TICKS: u64 = 3_000;

/// The seeds the measurement plays.
const SEEDS: [u64; 3] = [1, 2, 3];

/// The ticks a round is played for, if the environment names them.
///
/// The measurement plays a hundred seconds; a smoke run plays a few.
fn ticks() -> u64 {
    match std::env::var("LM_TICKS") {
        Ok(text) => text.trim().parse().unwrap_or(TICKS),
        Err(_) => TICKS,
    }
}

/// The seeds to play, if the environment names them.
fn seeds() -> Vec<u64> {
    match std::env::var("LM_SEEDS") {
        Ok(text) => text
            .split(',')
            .filter_map(|part| part.trim().parse::<u64>().ok())
            .collect(),
        Err(_) => SEEDS.to_vec(),
    }
}

/// The gate the arms are about.
const GATE: &str = "attention";

/// How many options the ghosted gate is asked with.
const PRUNE_K: u16 = 3;

/// How many requests may be in flight at once.
///
/// The measurement does not load the Thor: two at a time, so the latency
/// a decision is charged is its own.
const CONCURRENCY: usize = 2;

/// The flow on a pair has to be at least this decisive to be an edge.
///
/// The flow is the relative margin, so it is a share of the pair's own
/// mass: a fifth of it is a fifth of the way from "the same" to "the
/// opposite".
const FLOW_THRESHOLD: f64 = 0.20;

/// The arms of the measurement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Arm {
    /// Today: the ten-option gate, escalated to clef-flash when unsure.
    Today,
    /// The same gate as a two-step tree, on the d1-2B alone.
    Tree,
    /// The tree, with its last step escalated to clef-flash.
    TreeEscalated,
    /// The ten-option gate with its rarely-chosen options ghosted.
    Ghost,
}

impl Arm {
    /// Every arm, in the order they are reported.
    const ALL: [Arm; 4] = [Arm::Today, Arm::Tree, Arm::TreeEscalated, Arm::Ghost];

    /// The name the arm is reported under.
    fn name(self) -> &'static str {
        match self {
            Arm::Today => "A today",
            Arm::Tree => "B tree",
            Arm::TreeEscalated => "C tree+escalate",
            Arm::Ghost => "D ghost",
        }
    }

    /// The shape the arm asks the gate in.
    fn shape(self) -> ShapeConfig {
        match self {
            Arm::Today => ShapeConfig::default(),
            Arm::Tree => ShapeConfig {
                tree_gate: Some(GATE.to_owned()),
                escalate_tree_step: false,
                ..ShapeConfig::default()
            },
            Arm::TreeEscalated => ShapeConfig {
                tree_gate: Some(GATE.to_owned()),
                escalate_tree_step: true,
                ..ShapeConfig::default()
            },
            Arm::Ghost => ShapeConfig {
                prune_gate: Some(GATE.to_owned()),
                prune_k: PRUNE_K,
                ..ShapeConfig::default()
            },
        }
    }
}

/// One situation the attention gate was asked in.
#[derive(Clone, Debug)]
struct Situation {
    /// The facts text: the situation, the same text for every arm.
    state: String,
    /// The question as the harness asked it.
    question: Value,
    /// The options of the question, in key order.
    options: Vec<String>,
}

/// One ask that came back.
#[derive(Clone, Debug)]
struct Ask {
    /// Which pack it carried.
    pack: Pack,
    /// The facts text.
    state: String,
    /// The questions as asked.
    questions: Value,
    /// The answers.
    answers: Answers,
}

/// The Thor behind a log.
struct Watcher {
    inner: Box<dyn SysOne + Send + Sync>,
    log: Arc<Mutex<Vec<Ask>>>,
}

impl Watcher {
    /// A watcher over a backend.
    fn new(inner: Box<dyn SysOne + Send + Sync>) -> Self {
        Self {
            inner,
            log: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// The watcher's log.
    fn log(&self) -> Arc<Mutex<Vec<Ask>>> {
        Arc::clone(&self.log)
    }
}

impl SysOne for Watcher {
    fn ask(&self, state: &str, questions: &Value, deadline: Instant) -> Result<Answers, Refusal> {
        let answers = self.inner.ask(state, questions, deadline)?;
        // The pack is told by the questions it carried (D70): the move pack
        // holds the move gate, the talk pack the gates a line is chosen by.
        let pack = if questions.get("move").is_some() {
            Pack::Move
        } else {
            Pack::Talk
        };
        self.log.lock().expect("the watcher's log").push(Ask {
            pack,
            state: state.to_owned(),
            questions: questions.clone(),
            answers: answers.clone(),
        });
        Ok(answers)
    }

    fn name(&self) -> &str {
        self.inner.name()
    }
}

/// What one arm said about one situation.
#[derive(Clone, Debug, Default)]
struct ArmAnswer {
    /// The option the arm chose.
    choice: Option<String>,
    /// How sure the arm was.
    confidence: f32,
    /// How many options the model was asked between, over the calls the
    /// decision took.
    options_asked: usize,
    /// The wall clock the decision took.
    wall_ms: u32,
    /// The round trip the model reported.
    model_ms: u32,
    /// The round trip the second opinion took, when one was bought.
    ///
    /// The pack's own latency is the cheap model's: the tier reads it to
    /// know what a call costs, and it must not carry the second model's
    /// time as well. The escalation's own time is in its record, and
    /// this is where the measurement reads it.
    second_ms: u32,
    /// Whether a second opinion was bought.
    escalated: bool,
    /// The probability of every option, as the arm took it.
    probs: BTreeMap<String, f32>,
    /// The cheap model's probability of every option, from the escalation
    /// record where there was one.
    primary_probs: BTreeMap<String, f32>,
    /// The second model's probability of every option, from the escalation
    /// record where there was one.
    second_probs: BTreeMap<String, f32>,
    /// The tree's steps, when the gate was asked as a tree.
    ///
    /// Empty for the arms that ask the gate as it is written.
    steps: Vec<DecompositionStep>,
}

/// One situation, and what every arm and the judge said about it.
#[derive(Clone, Debug)]
struct SituationResult {
    /// The options the question was asked between.
    options: Vec<String>,
    /// The arms' answers.
    answers: BTreeMap<Arm, ArmAnswer>,
    /// What the judge chose.
    judge: Option<String>,
    /// How sure the judge was.
    judge_confidence: f32,
    /// What the cheap model answered to the **same** question with the
    /// options in the order the harness declares, instead of the order
    /// they are sent in.
    ordered_choice: Option<String>,
}

/// What one round did.
#[derive(Clone, Debug, Default)]
struct Round {
    /// The lines the agents spoke.
    utterances: u64,
    /// The decisions the agents' answers became.
    decisions: u64,
    /// The thoughts the model refused.
    refusals: u64,
    /// The questions the minds sent.
    thinks: u64,
    /// The answers that came home.
    answered: u64,
    /// The requests dropped because their deadline passed while queued.
    expired: u64,
    /// The gates that were asked of a second model (D71).
    escalations: u64,
    /// The attention decisions the round made.
    attention: u64,
    /// The attention decisions that were unsure.
    attention_unsure: u64,
    /// The attention decisions a second opinion was bought for.
    attention_escalated: u64,
    /// The attention escalations where the two models chose **differently**.
    ///
    /// The escalations that changed nothing are the ones that cost time
    /// and bought no answer.
    attention_escalation_changed: u64,
    /// How long the round took, in seconds.
    seconds: u64,
}

/// The configuration the measurement runs with.
///
/// The Thor is on this machine, so the base is localhost. The floor is
/// put at zero on every model, so that the measurement reads the models'
/// own numbers and derives the rule that would have been applied to them.
fn config() -> AgentsConfig {
    // The escalation is scoped to the gate under test, so that the arms
    // differ only in the shape of that gate. The setting is the harness's
    // (D75) and lives in the policy — see `measurement_policy`.
    AgentsConfig {
        thor_base: std::env::var("LM_THOR_BASE").unwrap_or_else(|_| "http://127.0.0.1".to_owned()),
        ..AgentsConfig::default()
    }
}

/// The policy the measurement runs with: the shipped one, with the gates a
/// second opinion is bought for scoped to the gate under test.
///
/// The setting is the harness's (D75) and lives in `policy.toml` — there is
/// one of it, and the arms take it from there like the game does.
fn measurement_policy() -> String {
    let shipped = "gates = [\"move\"]";
    let measured = format!("gates = [\"{GATE}\"]");
    assert!(
        DEFAULT_POLICY.contains(shipped),
        "the shipped policy still names the gates it escalates"
    );
    DEFAULT_POLICY.replace(shipped, &measured)
}

/// The gates a second opinion is bought for in the measurement: the gate
/// under test, read from the measurement's policy.
fn measurement_escalate_gates() -> Option<std::collections::BTreeSet<String>> {
    Policy::parse(&measurement_policy())
        .expect("the measurement's policy parses")
        .escalate_gates
}

/// The shipped harness, read into memory.
fn default_harness() -> Harness {
    Harness {
        dir: std::path::PathBuf::from("assets/harness/default"),
        gates_toml: DEFAULT_GATES.to_owned(),
        policy_toml: measurement_policy(),
        phrasebook_toml: DEFAULT_PHRASEBOOK.to_owned(),
        facts_toml: String::new(),
        reflexes: BTreeMap::new(),
        commit: "content:test-harness".to_owned(),
    }
}

/// A client of one of the Thor's models.
///
/// The floor is put at zero: the measurement reads the model's own
/// numbers, and the rule that would have been applied to them is derived
/// from them.
fn thor(cfg: &AgentsConfig, model: &str) -> Option<Box<dyn SysOne + Send + Sync>> {
    let port = *cfg.ports.get(model)?;
    let backend = ThorBackend::new(&cfg.thor_base, port, model)
        .with_min_confidence(0.0)
        .with_timeout(Duration::from_millis(u64::from(cfg.request_timeout_ms)));
    Some(Box::new(backend))
}

/// A client of one of the Thor's models, by the name the policy uses.
fn thor_by_policy_name(cfg: &AgentsConfig, name: &str) -> Option<Box<dyn SysOne + Send + Sync>> {
    let model = name.strip_prefix("thor-").unwrap_or(name);
    thor(cfg, model)
}

/// The backends of one arm: the cheap model and the second opinion.
///
/// The arm's shape is in the configuration, so the arm is the shipped
/// code and not a re-implementation of it.
fn arm_backends(
    gates: &GateSet,
    cfg: &AgentsConfig,
    arm: Arm,
    ghost: Option<&GhostLog>,
) -> Result<BTreeMap<String, Box<dyn SysOne + Send + Sync>>, String> {
    let mut arm_cfg = cfg.clone();
    arm_cfg.shape = arm.shape();
    let mut backends = BTreeMap::new();
    for (name, model) in [("thor-d1-2b", "d1-2b"), ("thor-clef-flash", "clef-flash")] {
        let backend = thor(cfg, model).ok_or_else(|| format!("the {model} is not there"))?;
        backends.insert(name.to_owned(), backend);
    }
    shape_backends_with(
        gates,
        &arm_cfg,
        measurement_escalate_gates(),
        backends,
        |name| thor_by_policy_name(cfg, name),
        ghost,
    )
    .map_err(|err| format!("the arm's backends are not built: {err}"))
}

/// The judge: the Thor's d1-3B, asked the full ten-option question.
///
/// The judge is not an arm and never answers the round: it is a third
/// model, asked the same facts, so that the arms' answers can be
/// compared against something that is neither of them.
fn judge(cfg: &AgentsConfig) -> Option<Box<dyn SysOne + Send + Sync>> {
    thor(cfg, "d1-3b")
}

/// The attention question of a pack, with its options.
fn attention_of(ask: &Ask) -> Option<(Value, Vec<String>)> {
    let question = ask.questions.get(GATE)?.clone();
    let criteria = question.get("criteria")?.as_object()?;
    let options = criteria.keys().cloned().collect();
    Some((question, options))
}

/// The choice an answer names.
///
/// The tie goes to the **first** option in order, the way the shipped
/// code breaks it: the measurement reads the answer the round acted on,
/// not one of its own.
fn choice_of(answer: &Answer) -> Option<String> {
    match answer {
        Answer::Choice { probs, .. } | Answer::Score { probs, .. } => top_of(probs),
        Answer::Noul(_) => None,
    }
}

/// The probability of every option of an answer.
fn probs_of(answer: &Answer) -> BTreeMap<String, f32> {
    match answer {
        Answer::Choice { probs, .. } | Answer::Score { probs, .. } => probs.clone(),
        Answer::Noul(_) => BTreeMap::new(),
    }
}

/// The confidence of an answer.
fn confidence_of(answer: &Answer) -> f32 {
    match answer {
        Answer::Choice { confidence, .. } | Answer::Score { confidence, .. } => *confidence,
        Answer::Noul(_) => 0.0,
    }
}

/// What an arm said, read out of the answers it brought back.
fn read_arm(answers: &Answers, wall_ms: u32) -> ArmAnswer {
    let escalation = answers.escalated.get(GATE);
    let answer = answers.per_gate.get(GATE);

    // How many options the model was asked between: the tree's steps when
    // the gate was a tree, the gate's own options otherwise, plus the
    // second opinion where one was bought.
    let mut options_asked = 0;
    if let Some(record) = answers.decomposed.get(GATE) {
        for step in &record.steps {
            options_asked += step.probs.len();
        }
    } else if let Some(answer) = answer {
        options_asked += probs_of(answer).len();
    }
    if let Some(escalation) = escalation {
        options_asked += escalation.second_probs.len();
    }

    let probs = answer.map(probs_of).unwrap_or_default();
    let primary_probs = match escalation {
        Some(escalation) if !escalation.primary_probs.is_empty() => {
            escalation.primary_probs.clone()
        }
        _ => probs.clone(),
    };

    ArmAnswer {
        choice: answer.and_then(choice_of),
        confidence: answer.map(confidence_of).unwrap_or(0.0),
        options_asked,
        wall_ms,
        model_ms: answers.latency_ms,
        second_ms: escalation
            .map(|escalation| escalation.latency_ms)
            .unwrap_or(0),
        escalated: escalation.is_some(),
        probs,
        primary_probs,
        second_probs: escalation
            .map(|escalation| escalation.second_probs.clone())
            .unwrap_or_default(),
        steps: answers
            .decomposed
            .get(GATE)
            .map(|record| record.steps.clone())
            .unwrap_or_default(),
    }
}

/// Play one round with one arm, and watch every ask that reaches the
/// model.
fn play_round(
    seed: u64,
    arm: Arm,
    cfg: &AgentsConfig,
    gates: &GateSet,
    ghost: Option<&GhostLog>,
) -> (Round, Vec<Ask>) {
    let header = Header::default_round(seed, DEFAULT_REFLEX);
    let harness = default_harness();

    let mut backends = arm_backends(gates, cfg, arm, ghost).expect("the arm's backends");
    // The watcher is outside the shape, so it sees the answers the round
    // actually took.
    let cheap = backends
        .remove("thor-d1-2b")
        .expect("the cheap model is there");
    let watcher = Watcher::new(cheap);
    let log = watcher.log();
    let mut map = BTreeMap::new();
    map.insert(
        "thor-d1-2b".to_owned(),
        Box::new(watcher) as Box<dyn SysOne + Send + Sync>,
    );
    for (name, backend) in backends {
        map.insert(name, backend);
    }

    let mut host = HostLoop::new(header.clone()).expect("the round starts");
    host.attach_agents(Agents::new(cfg, &harness, &header, map));

    let period = Duration::from_millis(1000 / u64::from(header.params.tick_hz));
    let started = Instant::now();
    for _ in 0..ticks() {
        std::thread::sleep(period);
        host.tick();
    }
    let elapsed = started.elapsed().as_secs();

    let metrics = talk_metrics(host.sim().talk_records());
    let stats = host.agent_stats().expect("the round has agents");
    let log = log.lock().expect("the watcher's log").clone();

    let mut round = Round {
        seconds: elapsed,
        utterances: metrics.utterances,
        decisions: stats.decisions,
        refusals: stats.refusals,
        thinks: stats.thinks,
        answered: stats.answered,
        expired: stats.expired,
        escalations: stats.escalations,
        ..Round::default()
    };
    for ask in &log {
        if ask.pack != Pack::Talk {
            continue;
        }
        if !ask.answers.per_gate.contains_key(GATE) && !ask.answers.unsure.contains_key(GATE) {
            continue;
        }
        round.attention += 1;
        if ask.answers.unsure.contains_key(GATE) {
            round.attention_unsure += 1;
        }
        if let Some(escalation) = ask.answers.escalated.get(GATE) {
            round.attention_escalated += 1;
            if escalation.primary_choice != escalation.second_choice {
                round.attention_escalation_changed += 1;
            }
        }
    }
    (round, log)
}

/// The distinct situations the shipped round asked the attention gate in.
///
/// The distinct facts texts, so that every arm is asked the same
/// situations and the answers can be compared one against another.
fn situations(asks: &[Ask]) -> Vec<Situation> {
    let mut out: Vec<Situation> = Vec::new();
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for ask in asks {
        if ask.pack != Pack::Talk {
            continue;
        }
        let Some((question, options)) = attention_of(ask) else {
            continue;
        };
        if seen.contains_key(&ask.state) {
            continue;
        }
        seen.insert(ask.state.clone(), out.len());
        out.push(Situation {
            state: ask.state.clone(),
            question,
            options,
        });
    }
    out
}

/// The log of what the attention gate chose, per situation.
///
/// This is the ghost table's memory: the ten-option gate's winners, as the
/// round's log recorded them.
fn ghost_log(asks: &[Ask]) -> GhostLog {
    let mut log = GhostLog::new();
    for ask in asks {
        if ask.pack != Pack::Talk {
            continue;
        }
        let Some((_, options)) = attention_of(ask) else {
            continue;
        };
        let winner = ask
            .answers
            .per_gate
            .get(GATE)
            .and_then(choice_of)
            .or_else(|| {
                ask.answers
                    .escalated
                    .get(GATE)
                    .and_then(|escalation: &Escalation| escalation.second_choice.clone())
            });
        log.record(&ask.state, &options, winner.as_deref());
    }
    log
}

/// Ask every arm, and the judge, about one situation.
fn ask_one(
    situation: &Situation,
    arms: &BTreeMap<Arm, Box<dyn SysOne + Send + Sync>>,
    the_judge: &(dyn SysOne + Send + Sync),
    cfg: &AgentsConfig,
) -> SituationResult {
    // The attention gate alone: the question under test, with the same
    // facts every arm and the judge are given.
    let question = json!({ GATE: situation.question.clone() });
    // Every ask gets its own deadline, so that one arm's slow answer does
    // not eat the next arm's budget. The deadline is three times the
    // request timeout: a decision that does not fit the game's own
    // deadline is a different question, and this one is about what the
    // decision costs.
    let budget = Duration::from_millis(u64::from(cfg.request_timeout_ms) * 3);

    let mut answers = BTreeMap::new();
    for (arm, backend) in arms {
        let started = Instant::now();
        let deadline = started + budget;
        let outcome = backend.ask(&situation.state, &question, deadline);
        let wall_ms = u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX);
        answers.insert(
            *arm,
            match outcome {
                Ok(answers) => read_arm(&answers, wall_ms),
                Err(_) => ArmAnswer {
                    wall_ms,
                    ..ArmAnswer::default()
                },
            },
        );
    }

    let judge = match the_judge.ask(&situation.state, &question, Instant::now() + budget) {
        Ok(answers) => answers.per_gate.get(GATE).cloned(),
        Err(_) => None,
    };
    let judge_choice = judge.as_ref().and_then(choice_of);
    let judge_confidence = judge.as_ref().map(confidence_of).unwrap_or(0.0);

    // The same question with the options in the order the harness
    // **declares**: the doctrine's own order, the squad and the field
    // first. The harness cannot send them in that order — its options
    // live in a sorted map — so the body is written out by hand.
    let ordered_choice = ordered_answer(cfg, situation);

    SituationResult {
        options: situation.options.clone(),
        answers,
        judge: judge_choice,
        judge_confidence,
        ordered_choice,
    }
}

/// Ask every arm, and the judge, about every situation.
///
/// At most `CONCURRENCY` requests are in flight at a time.
fn sweep(
    situations: &[Situation],
    arms: &BTreeMap<Arm, Box<dyn SysOne + Send + Sync>>,
    the_judge: &(dyn SysOne + Send + Sync),
    cfg: &AgentsConfig,
) -> Vec<SituationResult> {
    let next = AtomicUsize::new(0);
    let slots: Mutex<Vec<Option<SituationResult>>> =
        Mutex::new((0..situations.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..CONCURRENCY {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                if index >= situations.len() {
                    break;
                }
                let result = ask_one(&situations[index], arms, the_judge, cfg);
                slots.lock().expect("the slots")[index] = Some(result);
            });
        }
    });
    slots
        .into_inner()
        .expect("the slots")
        .into_iter()
        .map(|slot| slot.expect("every situation was asked"))
        .collect()
}

/// The percentile of a sorted slice, by rank.
fn percentile(sorted: &[f32], q: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = ((sorted.len() - 1) as f32 * q).round() as usize;
    sorted[rank]
}

/// The percentile of a sorted slice of milliseconds, by rank.
fn percentile_ms(sorted: &[u32], q: f32) -> u32 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = ((sorted.len() - 1) as f32 * q).round() as usize;
    sorted[rank]
}

/// The mean of a slice.
fn mean(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f32>() / values.len() as f32
}

/// The measurement: the four arms on the real Thor, and what they cost.
#[test]
#[ignore = "needs the Thor on this machine; run with --release --ignored --nocapture"]
fn decompose_on_thor() {
    let cfg = config();
    let gates = GateSet::parse(&default_harness().gates_toml).expect("the gates parse");

    // ---------------------------------------------------------------- the rounds
    //
    // Arm A is today's round, played three times: it is the shipped
    // behaviour and the source of the situations every arm is asked about.
    println!("== the rounds ==");
    let mut rounds: BTreeMap<Arm, Vec<(u64, Round)>> = BTreeMap::new();
    let mut asks: Vec<Ask> = Vec::new();
    let mut per_seed_log: BTreeMap<u64, Vec<Ask>> = BTreeMap::new();
    for seed in seeds() {
        let (round, log) = play_round(seed, Arm::Today, &cfg, &gates, None);
        println!(
            "A today  seed {seed}: {} utterances, {} decisions, {} refusals, {} asks sent, \
             {} answered, {} expired, {} escalations, {} attention decisions ({} unsure, \
             {} escalated, {} of them changed the answer) [{} s]",
            round.utterances,
            round.decisions,
            round.refusals,
            round.thinks,
            round.answered,
            round.expired,
            round.escalations,
            round.attention,
            round.attention_unsure,
            round.attention_escalated,
            round.attention_escalation_changed,
            round.seconds,
        );
        rounds.entry(Arm::Today).or_default().push((seed, round));
        asks.extend(log.clone());
        per_seed_log.insert(seed, log);
    }

    // The other three arms, each round warmed with the log of the same
    // seed's round of today, so that the arms differ only in the shape of
    // the attention gate and not in what they knew going in.
    for arm in [Arm::Tree, Arm::TreeEscalated, Arm::Ghost] {
        for seed in seeds() {
            let ghost = ghost_log(per_seed_log.get(&seed).expect("the seed was played"));
            let (round, _log) = play_round(seed, arm, &cfg, &gates, Some(&ghost));
            println!(
                "{}  seed {seed}: {} utterances, {} decisions, {} refusals, {} asks sent, \
                 {} answered, {} expired, {} escalations, {} attention decisions ({} unsure, \
                 {} escalated, {} of them changed the answer) [{} s]",
                arm.name(),
                round.utterances,
                round.decisions,
                round.refusals,
                round.thinks,
                round.answered,
                round.expired,
                round.escalations,
                round.attention,
                round.attention_unsure,
                round.attention_escalated,
                round.attention_escalation_changed,
                round.seconds,
            );
            rounds.entry(arm).or_default().push((seed, round));
        }
    }

    // ---------------------------------------------------------------- the situations
    let situations = situations(&asks);
    println!(
        "\n== the situations ==\n{} distinct facts texts, from {} talk packs",
        situations.len(),
        asks.iter().filter(|ask| ask.pack == Pack::Talk).count()
    );

    // ---------------------------------------------------------------- the arms
    let mut arms: BTreeMap<Arm, Box<dyn SysOne + Send + Sync>> = BTreeMap::new();
    for arm in Arm::ALL {
        let mut backends = arm_backends(&gates, &cfg, arm, None).expect("the arm's backends");
        arms.insert(
            arm,
            backends
                .remove("thor-d1-2b")
                .expect("the cheap model is there"),
        );
    }
    let the_judge = judge(&cfg).expect("the judge is there");

    // The ghost table is taught by the log of today's round, so that the
    // ghosted arm is asked with the options that have been winning.
    let log = ghost_log(&asks);
    {
        let mut backends =
            arm_backends(&gates, &cfg, Arm::Ghost, Some(&log)).expect("the arm's backends");
        arms.insert(
            Arm::Ghost,
            backends
                .remove("thor-d1-2b")
                .expect("the cheap model is there"),
        );
    }

    let started = Instant::now();
    let results = sweep(&situations, &arms, the_judge.as_ref(), &cfg);
    println!("the sweep took {} s", started.elapsed().as_secs());

    report_arms(&results);
    report_agreement(&results);
    report_esctune(&results);
    report_partitions(&results, &gates);
    report_shape(&results);
    report_hodge(&results);

    // The evidence: one line per situation, with what every arm said.
    // LM_DUMP=<file> writes it.
    if let Ok(path) = std::env::var("LM_DUMP") {
        let mut text = String::new();
        for (index, result) in results.iter().enumerate() {
            let mut line = serde_json::json!({
                "situation": index,
                "state": situations[index].state,
                "options": result.options,
                "judge": result.judge,
                "judge_confidence": result.judge_confidence,
                "arms": {},
            });
            let arms = line["arms"].as_object_mut().expect("the arms are there");
            for arm in Arm::ALL {
                let Some(answer) = result.answers.get(&arm) else {
                    continue;
                };
                arms.insert(
                    arm.name().to_owned(),
                    serde_json::json!({
                        "choice": answer.choice,
                        "confidence": answer.confidence,
                        "options_asked": answer.options_asked,
                        "wall_ms": answer.wall_ms,
                        "model_ms": answer.model_ms,
                        "second_ms": answer.second_ms,
                        "escalated": answer.escalated,
                        "probs": answer.probs,
                        "primary_probs": answer.primary_probs,
                        "second_probs": answer.second_probs,
                        "steps": answer.steps,
                    }),
                );
            }
            // The Hodge's own numbers for this question, so the evidence
            // carries the curl the write-up sorts the questions by.
            if let Some(today) = result.answers.get(&Arm::Today) {
                let mut options = result.options.clone();
                options.sort();
                if options.len() >= 2 {
                    let mut views = vec![View::new(today.primary_probs.clone())];
                    if !today.second_probs.is_empty() {
                        views.push(View::new(today.second_probs.clone()));
                    }
                    let hodge = decompose_preferences(&options, &views, FLOW_THRESHOLD);
                    let (gradient, curl, harmonic) = hodge.fractions();
                    line["gradient"] = serde_json::json!(gradient);
                    line["curl"] = serde_json::json!(curl);
                    line["harmonic"] = serde_json::json!(harmonic);
                    line["hodge_edges"] = serde_json::json!(hodge.edges);
                }
            }
            text.push_str(&serde_json::to_string(&line).expect("the line is JSON"));
            text.push('\n');
        }
        std::fs::write(&path, text).expect("the dump is written");
        println!("\nthe evidence is in {path}");
    }
}

/// The answer of the cheap model to the same question with its options in
/// the order the harness **declares**.
///
/// The harness cannot send the options in that order: they live in a
/// sorted map, so the order they arrive in is the alphabetical one. This
/// writes the body out by hand, so the order the harness declares can be
/// measured against the order it sends.
///
/// The answer is `None` when the question could not be asked.
fn ordered_answer(cfg: &AgentsConfig, situation: &Situation) -> Option<String> {
    let source = situation
        .question
        .get("criteria")
        .and_then(Value::as_object)?;
    // The order the harness declares: the squad and the field first, then
    // the agents in number.
    let mut keys: Vec<String> = Vec::new();
    for key in ["squad", "all"] {
        if source.contains_key(key) {
            keys.push(key.to_owned());
        }
    }
    let mut agents: Vec<String> = source
        .keys()
        .filter(|key| key.starts_with("agent"))
        .cloned()
        .collect();
    agents.sort();
    keys.extend(agents);
    let criteria: Vec<(String, String)> = keys
        .iter()
        .filter_map(|key| {
            source
                .get(key)
                .and_then(Value::as_str)
                .map(|text| (key.clone(), text.to_owned()))
        })
        .collect();

    let instructions = situation
        .question
        .get("instructions")
        .and_then(Value::as_str)
        .unwrap_or_default();

    // The body, by hand: the options in the order they are given.
    let mut body = String::from("{\"model\":\"d1-2b\",\"state\":");
    body.push_str(&json_string(&situation.state));
    body.push_str(",\"questions\":{\"attention\":{\"type\":\"choice\",\"instructions\":");
    body.push_str(&json_string(instructions));
    body.push_str(",\"criteria\":{");
    for (index, (key, text)) in criteria.iter().enumerate() {
        if index > 0 {
            body.push(',');
        }
        body.push_str(&json_string(key));
        body.push(':');
        body.push_str(&json_string(text));
    }
    body.push_str("}}}}");

    let port = *cfg.ports.get("d1-2b")?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(12)))
        .build()
        .into();
    let response = agent
        .post(format!("http://127.0.0.1:{port}/v1/systemone"))
        .header("Content-Type", "application/json")
        .send(body.as_bytes())
        .ok()?;
    let text = response.into_body().read_to_string().ok()?;
    let parsed: Value = serde_json::from_str(&text).ok()?;
    let answer = parsed.get("answers")?.get(GATE)?;
    Some(answer.get("choice")?.as_str()?.to_owned())
}

/// A JSON string, with the quotes and the escapes.
fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// What each arm said, and what it cost.
fn report_arms(results: &[SituationResult]) {
    println!("\n== the arms, over the same situations ==");
    println!(
        "{:<16} {:>8} {:>8} {:>8} {:>9} {:>8} {:>8} {:>8} {:>10} {:>7} {:>9} {:>9}",
        "arm",
        "decided",
        "conf p50",
        "conf p95",
        "unsure",
        "ms p50",
        "ms p95",
        "model p50",
        "ms/decide",
        "opts",
        "vs judge",
        "p(judge)"
    );
    for arm in Arm::ALL {
        let mut confidences: Vec<f32> = Vec::new();
        let mut latencies: Vec<u32> = Vec::new();
        let mut model_latencies: Vec<u32> = Vec::new();
        let mut decided = 0usize;
        let mut unsure = 0usize;
        let mut escalated = 0usize;
        let mut options = 0usize;
        let mut total_ms = 0u64;
        let mut agree = 0usize;
        let mut judged = 0usize;
        let mut judge_probability = 0.0f32;
        let mut second_latencies: Vec<u32> = Vec::new();
        for result in results {
            let Some(answer) = result.answers.get(&arm) else {
                continue;
            };
            decided += 1;
            confidences.push(answer.confidence);
            latencies.push(answer.wall_ms);
            model_latencies.push(answer.model_ms);
            total_ms += u64::from(answer.wall_ms);
            options += answer.options_asked;
            if answer.confidence < 0.30 {
                unsure += 1;
            }
            if answer.escalated {
                escalated += 1;
                second_latencies.push(answer.second_ms);
            }
            if let Some(judge) = &result.judge {
                judged += 1;
                if answer.choice.as_ref() == Some(judge) {
                    agree += 1;
                }
                // How much the arm gave the judge's choice, even when it
                // did not pick it.
                judge_probability += answer.probs.get(judge).copied().unwrap_or(0.0);
            }
        }
        confidences.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        latencies.sort_unstable();
        model_latencies.sort_unstable();
        println!(
            "{:<16} {:>8} {:>8.3} {:>8.3} {:>9} {:>8} {:>8} {:>8} {:>10} {:>7} {:>9} {:>9}",
            arm.name(),
            decided,
            percentile(&confidences, 0.5),
            percentile(&confidences, 0.95),
            format!("{unsure}/{}", decided),
            percentile_ms(&latencies, 0.5),
            percentile_ms(&latencies, 0.95),
            percentile_ms(&model_latencies, 0.5),
            total_ms.checked_div(decided as u64).unwrap_or(0),
            options.checked_div(decided).unwrap_or(0),
            if judged == 0 {
                "n/a".to_owned()
            } else {
                format!("{:.1}%", 100.0 * agree as f64 / judged as f64)
            },
            if judged == 0 {
                "n/a".to_owned()
            } else {
                format!("{:.3}", f64::from(judge_probability) / judged as f64)
            },
        );
        second_latencies.sort_unstable();
        println!(
            "                 {} escalated of {}, mean confidence {:.3}, the second opinions \
             cost {} ms at the median",
            escalated,
            decided,
            mean(&confidences),
            percentile_ms(&second_latencies, 0.5),
        );
        // The confidence distribution, not just its middle: a p50 and a p95
        // do not say how the arm's confidence is spread.
        if !confidences.is_empty() {
            println!(
                "                 confidence: min {:.3}, p25 {:.3}, p50 {:.3}, p75 {:.3}, p95 {:.3}, max {:.3}",
                confidences.first().copied().unwrap_or(0.0),
                percentile(&confidences, 0.25),
                percentile(&confidences, 0.5),
                percentile(&confidences, 0.75),
                percentile(&confidences, 0.95),
                confidences.last().copied().unwrap_or(0.0),
            );
        }
    }
}

/// How often the arms agree with each other.
fn report_agreement(results: &[SituationResult]) {
    let mut judge_confidences: Vec<f32> = results.iter().map(|r| r.judge_confidence).collect();
    judge_confidences.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    println!("\n== the agreement between the arms ==");
    println!(
        "the judge answered {} of {} situations, mean confidence {:.3}, p50 {:.3}, p95 {:.3}",
        results.iter().filter(|r| r.judge.is_some()).count(),
        results.len(),
        mean(&judge_confidences),
        percentile(&judge_confidences, 0.5),
        percentile(&judge_confidences, 0.95),
    );
    println!("{:<16} {:>16}", "pair", "same answer");
    for i in 0..Arm::ALL.len() {
        for j in (i + 1)..Arm::ALL.len() {
            let (a, b) = (Arm::ALL[i], Arm::ALL[j]);
            let mut same = 0usize;
            let mut both = 0usize;
            for result in results {
                let (Some(x), Some(y)) = (result.answers.get(&a), result.answers.get(&b)) else {
                    continue;
                };
                if x.choice.is_none() && y.choice.is_none() {
                    continue;
                }
                both += 1;
                if x.choice == y.choice {
                    same += 1;
                }
            }
            println!(
                "{:<16} {:>16}",
                format!("{} / {}", a.name(), b.name()),
                if both == 0 {
                    "n/a".to_owned()
                } else {
                    format!("{:.1}% ({same}/{both})", 100.0 * same as f64 / both as f64)
                }
            );
        }
    }
}

/// Who the judge agrees with, model by model.
///
/// This is the esctune question: the d1-3B is asked the ten-option
/// question with the same facts, and the answer it gives is the verdict
/// on the answers of the models under test. The d1-2B and clef-flash are
/// both answering the **same** question here, so the count says which of
/// the two the judge prefers -- and the tree and the ghost are counted
/// against the same verdict.
fn report_esctune(results: &[SituationResult]) {
    println!("\n== the judge, model by model ==");
    let mut cheap = 0usize;
    let mut second = 0usize;
    let mut tree = 0usize;
    let mut ghost = 0usize;
    let mut judged = 0usize;
    let mut order_same = 0usize;
    let mut order_total = 0usize;
    for result in results {
        let Some(judge) = &result.judge else {
            continue;
        };
        judged += 1;
        let today = result.answers.get(&Arm::Today);
        if let Some(today) = today {
            if top_of(&today.primary_probs).as_ref() == Some(judge) {
                cheap += 1;
            }
            if top_of(&today.second_probs).as_ref() == Some(judge) {
                second += 1;
            }
        }
        if let Some(answer) = result.answers.get(&Arm::Tree) {
            if answer.choice.as_ref() == Some(judge) {
                tree += 1;
            }
        }
        if let Some(answer) = result.answers.get(&Arm::Ghost) {
            if answer.choice.as_ref() == Some(judge) {
                ghost += 1;
            }
        }
        if let Some(today) = today {
            order_total += 1;
            if top_of(&today.primary_probs) == result.ordered_choice {
                order_same += 1;
            }
        }
    }
    println!(
        "the judge answered {judged} of {} situations",
        results.len()
    );
    println!(
        "  the d1-2B's flat answer agrees with it:  {cheap}/{judged}\n  \
         the second model's answer agrees with it: {second}/{judged}\n  \
         the tree's answer agrees with it:          {tree}/{judged}\n  \
         the ghost's answer agrees with it:         {ghost}/{judged}"
    );
    println!(
        "\nthe same question, the options in the order the harness sends them and in the\n\
         order the harness declares them: the same answer {order_same} of {order_total} times"
    );

    // The ghosted arm: the options it left out, and whether the judge
    // wanted one of them.
    let mut ghosted = 0usize;
    let mut ghosted_right = 0usize;
    let mut asked_right = 0usize;
    for result in results {
        let Some(judge) = &result.judge else {
            continue;
        };
        let Some(answer) = result.answers.get(&Arm::Ghost) else {
            continue;
        };
        // The ghosted gate is asked with fewer options, so the answer's
        // own options are the ones it was asked between.
        if answer.choice.as_ref() == Some(judge) {
            asked_right += 1;
        }
        if !answer.probs.contains_key(judge) {
            ghosted += 1;
            ghosted_right += 1;
        }
    }
    println!(
        "\nthe ghost left out the judge's answer in {ghosted_right} of the {ghosted} questions \
         where it left out something; it answered with the judge's answer {asked_right} times"
    );
}

/// Where the tree's question and the flat question part company.
///
/// The judge is asked the **flat** question, so an arm that answers a
/// different question is measured against a verdict about this one. That
/// bias is real, and this says how much of it there is: the flat
/// question's own probabilities are projected onto the tree's partitions
/// (every option of the gate falls under exactly one of them), and the
/// tree's first step is read against that projection.
///
/// Three questions come out of it:
/// - does the flat question put its mass on the partition the tree took?
///   (did the coarse question move the model?)
/// - does the judge's answer fall under the partition the tree took?
///   (was the judge even in the tree's branch?)
/// - and where it did, did the tree then disagree with the judge anyway?
fn report_partitions(results: &[SituationResult], gates: &GateSet) {
    println!("\n== the tree's partitions, against the flat question ==");
    let Some(tree) = gates.decomposition_of(GATE) else {
        println!("the gate has no tree, so there are no partitions to compare");
        return;
    };
    // The partition each option falls under.
    let mut of_option: BTreeMap<String, String> = BTreeMap::new();
    for partition in &tree.partitions {
        for leaf in &partition.options {
            of_option.insert(leaf.option.clone(), partition.key.clone());
        }
    }

    let mut judged = 0usize;
    let mut same_reading = 0usize;
    let mut judge_inside = 0usize;
    let mut judge_outside = 0usize;
    let mut disagreed_inside = 0usize;
    let mut flat_picks: BTreeMap<String, usize> = BTreeMap::new();
    let mut tree_picks: BTreeMap<String, usize> = BTreeMap::new();
    for result in results {
        let Some(today) = result.answers.get(&Arm::Today) else {
            continue;
        };
        let Some(tree_answer) = result.answers.get(&Arm::Tree) else {
            continue;
        };
        let Some(partition) = tree_answer
            .steps
            .iter()
            .find(|step| step.step == 1)
            .map(|step| step.partition.clone())
        else {
            continue;
        };
        *tree_picks.entry(partition.clone()).or_default() += 1;

        // The flat question's own mass, projected onto the partitions.
        let mut masses: BTreeMap<String, f32> = BTreeMap::new();
        for (option, probability) in &today.primary_probs {
            let Some(key) = of_option.get(option) else {
                continue;
            };
            *masses.entry(key.clone()).or_default() += probability;
        }
        let flat_partition = top_of(&masses);
        if let Some(flat_partition) = &flat_partition {
            *flat_picks.entry(flat_partition.clone()).or_default() += 1;
            if flat_partition == &partition {
                same_reading += 1;
            }
        }

        let Some(judge) = &result.judge else {
            continue;
        };
        judged += 1;
        let Some(judge_partition) = of_option.get(judge) else {
            continue;
        };
        if judge_partition == &partition {
            judge_inside += 1;
            if tree_answer.choice.as_ref() != Some(judge) {
                disagreed_inside += 1;
            }
        } else {
            judge_outside += 1;
        }
    }

    println!(
        "the flat question's own mass and the tree's first step fell on the same partition in \
         {same_reading} of {} situations",
        results.len()
    );
    println!(
        "the judge's answer fell under the partition the tree took in {judge_inside} of {judged} \
         situations, and under another one in {judge_outside}"
    );
    if judge_inside > 0 {
        println!(
            "inside the tree's own branch, the tree still disagreed with the judge in \
             {disagreed_inside} of {judge_inside}"
        );
    }
    println!("the flat question's mass: {flat_picks:?}");
    println!("the tree's first step:    {tree_picks:?}");
}

/// The option with the largest probability.
///
/// The tie goes to the **first** option in order, the way the shipped
/// code breaks it.
fn top_of(probs: &BTreeMap<String, f32>) -> Option<String> {
    let mut best: Option<(&String, &f32)> = None;
    for (key, value) in probs {
        let better = match best {
            None => true,
            Some((_, top)) => *value > *top,
        };
        if better {
            best = Some((key, value));
        }
    }
    best.map(|(key, _)| key.clone())
}

/// What the tree answers, against what the flat question answers.
///
/// The tree asks the coarse question first, so it cannot answer "the
/// squad": it either names the partition it picked and the agent inside
/// it, or it says "everybody". The flat question can answer "the squad",
/// and does. This is the shape of the difference, in the arms' own words.
fn report_shape(results: &[SituationResult]) {
    println!("\n== what the tree answers, and what the flat question answers ==");
    let mut tree_agent = 0usize;
    let mut tree_squad = 0usize;
    let mut tree_everybody = 0usize;
    let mut flat_squad = 0usize;
    let mut both = 0usize;
    for result in results {
        let (Some(tree), Some(flat)) = (
            result.answers.get(&Arm::Tree),
            result.answers.get(&Arm::Today),
        ) else {
            continue;
        };
        let (Some(tree_choice), Some(flat_choice)) = (&tree.choice, &flat.choice) else {
            continue;
        };
        both += 1;
        if flat_choice == "squad" {
            flat_squad += 1;
        }
        if tree_choice.starts_with("agent") {
            tree_agent += 1;
        } else if tree_choice == "everybody" || tree_choice == "all" {
            tree_everybody += 1;
        } else {
            tree_squad += 1;
        }
    }
    if both == 0 {
        println!("no situation was answered by both the tree and the flat question");
        return;
    }
    println!(
        "in {both} situations both the tree and the flat question answered:\n  \
         the tree named an individual agent: {tree_agent}\n  \
         the tree said everybody:          {tree_everybody}\n  \
         the tree said the squad:          {tree_squad}\n  \
         the flat question said the squad: {flat_squad}"
    );
}

/// The Hodge decomposition of the attention gate's answers, and what it
/// says about the arms.
fn report_hodge(results: &[SituationResult]) {
    println!("\n== the Hodge decomposition of the attention gate's answers ==");
    let mut rows: Vec<(f64, f64, f64, usize, i32, usize)> = Vec::new();
    let mut tree_only = 0usize;
    let mut today_only = 0usize;
    let mut both = 0usize;
    let mut neither = 0usize;
    for result in results {
        let Some(today) = result.answers.get(&Arm::Today) else {
            continue;
        };
        let mut options = result.options.clone();
        options.sort();
        if options.len() < 2 {
            continue;
        }
        let mut views = vec![View::new(today.primary_probs.clone())];
        if !today.second_probs.is_empty() {
            views.push(View::new(today.second_probs.clone()));
        }
        let hodge = decompose_preferences(&options, &views, FLOW_THRESHOLD);
        let (gradient, curl, harmonic) = hodge.fractions();

        // Did decomposing beat escalating on this question?
        let mut delta = 0i32;
        if let Some(judge) = &result.judge {
            let today_match = today.choice.as_ref() == Some(judge);
            let tree_match = result
                .answers
                .get(&Arm::Tree)
                .and_then(|tree| tree.choice.as_ref())
                == Some(judge);
            delta = i32::from(tree_match) - i32::from(today_match);
        }
        // The paired count against the judge: the questions where the two
        // answers differ and one of them is the judge's.
        match delta {
            1 => tree_only += 1,
            -1 => today_only += 1,
            0 => {
                if today.choice.as_ref() == result.judge.as_ref() {
                    both += 1;
                } else {
                    neither += 1;
                }
            }
            _ => {}
        }
        rows.push((
            gradient,
            curl,
            harmonic,
            hodge.components,
            delta,
            hodge.edges,
        ));
    }
    if rows.is_empty() {
        println!("no situation had a flow to decompose");
        return;
    }

    let gradients: Vec<f32> = rows.iter().map(|r| r.0 as f32).collect();
    let curls: Vec<f32> = rows.iter().map(|r| r.1 as f32).collect();
    let harmonics: Vec<f32> = rows.iter().map(|r| r.2 as f32).collect();
    let edges: Vec<usize> = rows.iter().map(|r| r.5).collect();
    let mut sorted_curls = curls.clone();
    sorted_curls.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median_curl = f64::from(percentile(&sorted_curls, 0.5));

    let mut dominant = [0usize; 3];
    for row in &rows {
        let part = if row.1 >= row.0 && row.1 >= row.2 {
            Part::Curl
        } else if row.2 >= row.0 && row.2 > row.1 {
            Part::Harmonic
        } else {
            Part::Gradient
        };
        match part {
            Part::Gradient => dominant[0] += 1,
            Part::Curl => dominant[1] += 1,
            Part::Harmonic => dominant[2] += 1,
        }
    }

    println!(
        "mean gradient {:.4}, mean curl {:.4}, mean harmonic {:.4} (threshold {FLOW_THRESHOLD})",
        mean(&gradients),
        mean(&curls),
        mean(&harmonics)
    );
    println!(
        "the graph had {} edges on average over {} options",
        edges.iter().sum::<usize>() / edges.len().max(1),
        results.first().map(|r| r.options.len()).unwrap_or(0)
    );
    println!(
        "the dominant part: {} gradient, {} curl, {} harmonic",
        dominant[0], dominant[1], dominant[2]
    );

    // The question the brief asks: are the questions where the options do
    // not lie on one scale the questions where decomposing beats
    // escalating?
    let high: Vec<i32> = rows
        .iter()
        .filter(|r| r.1 >= median_curl)
        .map(|r| r.4)
        .collect();
    let low: Vec<i32> = rows
        .iter()
        .filter(|r| r.1 < median_curl)
        .map(|r| r.4)
        .collect();
    let mean_delta = |values: &[i32]| -> f64 {
        if values.is_empty() {
            return 0.0;
        }
        values.iter().sum::<i32>() as f64 / values.len() as f64
    };
    // The paired counts, so the mean is read with its own counts and not
    // on its own.
    let counts = |values: &[i32]| -> (usize, usize, usize) {
        let tree_only = values.iter().filter(|d| **d == 1).count();
        let today_only = values.iter().filter(|d| **d == -1).count();
        let both = values.iter().filter(|d| **d == 0).count();
        (tree_only, today_only, both)
    };
    let (high_tree, high_today, high_both) = counts(&high);
    let (low_tree, low_today, low_both) = counts(&low);
    println!(
        "\ndecompose minus escalate, by the question's curl (median {median_curl:.4}):\n  \
         high curl ({} questions): {:+.3} per question — the tree right {high_tree}, today's arm right {high_today}, both right {high_both}\n  \
         low curl ({} questions): {:+.3} per question — the tree right {low_tree}, today's arm right {low_today}, both right {low_both}",
        high.len(),
        mean_delta(&high),
        low.len(),
        mean_delta(&low)
    );

    println!(
        "\npaired against the judge: the tree right and today's arm wrong {tree_only}, \
         today's arm right and the tree wrong {today_only}, both right {both}, \
         neither right {neither}"
    );
}
