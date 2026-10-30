//! The talk gates on the real Thor: what the model answers, what the gates do
//! with it, and what the second opinion is worth.
//!
//! The first measurement (the first two sections of
//! `tests/talk_on_thor.md`) found why the agents decided fine and said almost
//! nothing: the pack did not fit the deadline, one presence gate was worded
//! as a question about urgency, and the pack-level confidence rule vetoed
//! everything. D70 then split the pack and D71 added the second opinion, and
//! the second measurement found the escalation running at 9.6% of gates
//! rather than D71's 4%, carried by the two ten-option talk gates.
//!
//! This is the third measurement, and it asks the two questions that follow:
//!
//! 1. **Does escalating the ten-option talk gates help?** Three arms play the
//!    same three seeds — the escalation on for every gate, on for the move
//!    gate only, and off — and every gate escalated in the "all" arm is
//!    compared answer by answer: how often the d1-2B's own choice and
//!    clef-flash's differ, and whether the difference would change the line.
//!    An offline judge — the Thor's **d1-3B**, a third model that is neither
//!    party in the dispute, asked the same single question over the same
//!    facts — says which model's choice it agrees with.
//! 2. **What is the d1-3B's own confidence floor?** The same harness plays
//!    the round once with the d1-3B behind the tier and the floor at zero, so
//!    every gate's own number is seen; the floor is read from the model's own
//!    scale the way the d1-2B's 0.30 was.
//!
//! ## What is watched
//!
//! Every ask that comes back is recorded with:
//!
//! - which pack it carried (D70: the move pack or the talk pack);
//! - the five presence nouls (`gesture_present`, `attention_present`,
//!   `intention_present`, `intent_present`, `subject_present`);
//! - the confidence of every gate that has one;
//! - the round trip the model reported;
//! - every escalation (D71): the gate, both confidences, both models' own
//!   choices and the latency — and the question the escalated gate was asked,
//!   so the judge can be asked exactly what the models were asked.
//!
//! The watcher asks the Thor with the floor at zero, so that every gate's
//! own number is seen: the rule that would have been applied to them is
//! derived from them and reported below, per gate and per pack.
//!
//! The write-up of the numbers is `tests/talk_on_thor.md`.
//!
//! ## The command
//!
//! ```text
//! cargo test --release -p lm-agents --test talk_on_thor -- --ignored --nocapture
//! ```
//!
//! The load is kept modest for the Thor's other seats: the round itself holds
//! at most two concurrent requests (one worker per pack), and the judge asks
//! one question at a time after the rounds are done.

use std::collections::{BTreeMap, BTreeSet};
use std::net::ToSocketAddrs;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use lm_agents::config::AgentsConfig;
use lm_agents::gates::Pack;
use lm_agents::harness::Harness;
use lm_agents::sysone::{
    Answer, Answers, EscalatingBackend, Escalation, Refusal, SysOne, ThorBackend,
};
use lm_agents::utter::{presence_gate, UTTERANCE_GATES};
use lm_agents::{Agents, AskTiming};
use lm_core::header::Header;
use lm_core::talk::talk_metrics;
use lm_host_model::HostLoop;
use serde_json::Value;

/// The shipped harness: the gates, the policy and the phrasebook the default
/// round thinks with.
const DEFAULT_GATES: &str = include_str!("../../../assets/harness/default/gates.toml");
const DEFAULT_POLICY: &str = include_str!("../../../assets/harness/default/policy.toml");
const DEFAULT_PHRASEBOOK: &str = include_str!("../../../assets/harness/default/phrasebook.toml");

/// The shipped reflex, compiled into the test binary.
const DEFAULT_REFLEX: &str = include_str!("../../../assets/reflexes/default.rhai");

/// The ticks a round is played for: a hundred seconds at 30 Hz.
const TICKS: u64 = 3_000;

/// The backend the watcher wraps: the Thor's d1-2B.
///
/// It is the only backend the watcher's log and the round's timings are
/// joined on.
const WATCHED_BACKEND: &str = "thor-d1-2b";

/// The seeds the measurement plays.
const SEEDS: [u64; 3] = [1, 2, 3];

/// The seeds to play, if the environment names them.
///
/// The measurement plays three seeds. The pack capture needs more than that
/// to have five hundred packs to compare, and it is the same round, played
/// with more seeds: `LM_SEEDS=1,2,3,4,5,6`.
fn seeds() -> Vec<u64> {
    match std::env::var("LM_SEEDS") {
        Ok(text) => text
            .split(',')
            .filter_map(|part| part.trim().parse::<u64>().ok())
            .collect(),
        Err(_) => SEEDS.to_vec(),
    }
}

/// One escalation and the question the escalated gate was asked, so the
/// judge can be asked exactly what the models were asked.
#[derive(Clone, Debug)]
struct Escalated {
    /// The escalation's own record (both confidences, both choices).
    escalation: Escalation,
    /// The situation the question was asked over: the facts text.
    state: String,
    /// The single question the second model was asked.
    question: Value,
}

/// One ask that came back, and what the gates made of it.
#[derive(Clone, Debug)]
struct Ask {
    /// Which pack the ask carried (D70).
    pack: Pack,
    /// The situation the pack was asked over: the facts text.
    state: String,
    /// The pack as it was asked: the questions, so the same pack can be
    /// asked again of another server and the answers compared.
    questions: Value,
    /// The five presence nouls, by gate id.
    presence: BTreeMap<String, f32>,
    /// The confidence of every gate that has one, by gate id.
    confidence: BTreeMap<String, f32>,
    /// The round trip the model reported, in milliseconds.
    latency_ms: u32,
    /// The escalations this ask paid for (D71), with their questions.
    escalations: Vec<Escalated>,
    /// The wire: the time to open a connection to the Thor, in
    /// milliseconds, measured while the pack was in flight (N1).
    network_ms: u32,
    /// The refusal the call came back with, if it came back with one.
    refusal: Option<String>,
}

/// The escalation arms: which gates a second opinion is bought for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arm {
    /// Every gate below `escalate_below` (D71 as it shipped).
    All,
    /// The move gate only.
    Move,
    /// No gate: the escalation off.
    Off,
}

impl Arm {
    /// The arm's name, for the report.
    fn name(self) -> &'static str {
        match self {
            Arm::All => "all gates",
            Arm::Move => "move only",
            Arm::Off => "off",
        }
    }

    /// The gate set the arm escalates: `None` is every gate.
    fn gates(self) -> Option<BTreeSet<String>> {
        match self {
            Arm::All => None,
            Arm::Move => Some(BTreeSet::from(["move".to_owned()])),
            Arm::Off => Some(BTreeSet::new()),
        }
    }
}

/// The watcher: the Thor behind a log.
struct Watcher {
    inner: Box<dyn SysOne + Send + Sync>,
    log: Arc<Mutex<Vec<Ask>>>,
    /// The refusals the Thor itself produced, by name.
    refused: Arc<Mutex<BTreeMap<String, u64>>>,
    /// Where the Thor is, for the wire probe (N1).
    addr: String,
}

impl Watcher {
    /// A watcher over the Thor's d1-2B under one escalation arm.
    ///
    /// The floor is put at zero, so that every gate's answer is seen: the
    /// measurement reads the model's own numbers, and the rule that would
    /// have been applied to them is derived from them.
    fn d1_2b(cfg: &AgentsConfig, arm: Arm) -> Self {
        let port = cfg.ports["d1-2b"];
        let primary = ThorBackend::new(&cfg.thor_base, port, "d1-2b")
            .with_min_confidence(0.0)
            .with_timeout(Duration::from_millis(u64::from(cfg.request_timeout_ms)));
        let inner = match arm {
            Arm::Off => Box::new(primary) as Box<dyn SysOne + Send + Sync>,
            Arm::All | Arm::Move => {
                // The second model (D71): the Thor's clef-flash, asked one
                // question at a time.
                let second_port = cfg.ports["clef-flash"];
                let second = ThorBackend::new(&cfg.thor_base, second_port, "clef-flash")
                    .with_min_confidence(
                        cfg.min_confidence
                            .get("thor-clef-flash")
                            .copied()
                            .unwrap_or(0.0),
                    )
                    .with_timeout(Duration::from_millis(u64::from(cfg.request_timeout_ms)));
                Box::new(
                    EscalatingBackend::new(Box::new(primary), Box::new(second), cfg.escalate_below)
                        .with_gates(arm.gates()),
                ) as Box<dyn SysOne + Send + Sync>
            }
        };
        Self::over(inner, format!("{}:{}", host_of(&cfg.thor_base), port))
    }

    /// A watcher over the Thor's d1-3B, for the floor calibration: the same
    /// round, the same gates, a different model behind the tier, and the
    /// floor at zero so every gate's own number is seen.
    fn d1_3b(cfg: &AgentsConfig) -> Self {
        let port = cfg.ports["d1-3b"];
        let backend = ThorBackend::new(&cfg.thor_base, port, "d1-3b")
            .with_min_confidence(0.0)
            .with_timeout(Duration::from_millis(u64::from(cfg.request_timeout_ms)));
        Self::over(
            Box::new(backend),
            format!("{}:{}", host_of(&cfg.thor_base), port),
        )
    }

    fn over(inner: Box<dyn SysOne + Send + Sync>, addr: String) -> Self {
        Watcher {
            inner,
            log: Arc::new(Mutex::new(Vec::new())),
            refused: Arc::new(Mutex::new(BTreeMap::new())),
            // Where the model is, for the wire probe (N1).
            addr,
        }
    }

    /// The watcher's log of the answers that came back.
    fn log(&self) -> Arc<Mutex<Vec<Ask>>> {
        Arc::clone(&self.log)
    }

    /// The watcher's log of the refusals the Thor produced.
    fn refused(&self) -> Arc<Mutex<BTreeMap<String, u64>>> {
        Arc::clone(&self.refused)
    }
}

impl SysOne for Watcher {
    fn ask(&self, state: &str, questions: &Value, deadline: Instant) -> Result<Answers, Refusal> {
        self.ask_with_escalation(state, questions, deadline, deadline)
    }

    /// The watcher forwards the escalation's deadline (N3).
    ///
    /// It wraps the escalating backend, so the fence behind it has to
    /// survive the wrap: a wrapper that dropped it would let a second
    /// opinion outlive the decision it serves.
    fn ask_with_escalation(
        &self,
        state: &str,
        questions: &Value,
        deadline: Instant,
        escalation_deadline: Instant,
    ) -> Result<Answers, Refusal> {
        // The wire, measured on its own thread while the pack is in flight
        // (N1): the measurement never delays the thing it measures.
        let probe = thread::spawn({
            let addr = self.addr.clone();
            move || probe_wire(&addr)
        });
        let pack = pack_of(questions);
        let answers =
            match self
                .inner
                .ask_with_escalation(state, questions, deadline, escalation_deadline)
            {
                Ok(answers) => answers,
                Err(refusal) => {
                    *self
                        .refused
                        .lock()
                        .expect("the watcher's refusals")
                        .entry(refusal.to_string())
                        .or_default() += 1;
                    // The refusal is logged as well, so the log and the round's
                    // own timings line up one for one.
                    self.log.lock().expect("the watcher's log").push(Ask {
                        pack,
                        state: state.to_owned(),
                        questions: questions.clone(),
                        presence: BTreeMap::new(),
                        confidence: BTreeMap::new(),
                        latency_ms: 0,
                        escalations: Vec::new(),
                        network_ms: 0,
                        refusal: Some(refusal.to_string()),
                    });
                    drop(probe);
                    return Err(refusal);
                }
            };
        let network_ms = probe.join().unwrap_or(0);

        let mut presence = BTreeMap::new();
        for gate in UTTERANCE_GATES {
            let id = presence_gate(gate);
            if let Some(Answer::Noul(value)) = answers.per_gate.get(&id) {
                presence.insert(id, *value);
            }
        }

        let mut confidence = BTreeMap::new();
        for (id, answer) in &answers.per_gate {
            let value = match answer {
                Answer::Choice { confidence, .. } | Answer::Score { confidence, .. } => *confidence,
                // A noul has no confidence: its value is the belief itself.
                Answer::Noul(_) => continue,
            };
            confidence.insert(id.clone(), value);
        }

        // The escalations keep the question they were asked over, so the
        // judge can be asked exactly what the models were asked.
        let escalations = answers
            .escalated
            .iter()
            .map(|(gate, escalation)| Escalated {
                escalation: escalation.clone(),
                state: state.to_owned(),
                question: questions.get(gate).cloned().unwrap_or(Value::Null),
            })
            .collect();

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
            presence,
            confidence,
            latency_ms: answers.latency_ms,
            escalations,
            network_ms,
            refusal: None,
        });

        Ok(answers)
    }

    fn name(&self) -> &str {
        self.inner.name()
    }
}

/// The host of the Thor's base URL.
///
/// The base is `http://<host>`, and the model ports hang off it.
fn host_of(base: &str) -> String {
    base.trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/')
        .to_owned()
}

/// The pack a set of questions carried (D70).
///
/// The move pack holds the move gate, the talk pack the gates a line is
/// chosen by.
fn pack_of(questions: &Value) -> Pack {
    if questions.get("move").is_some() {
        Pack::Move
    } else {
        Pack::Talk
    }
}

/// The wire to the Thor: the time to open a connection to it, in
/// milliseconds.
///
/// The probe is a TCP handshake on the same route the pack takes, timed on
/// its own thread so the pack is not delayed by the measurement. The
/// handshake is the kernel's, so it is not queued behind the model's own
/// work: the number is the wire, not the model's queue (N1).
fn probe_wire(addr: &str) -> u32 {
    let started = Instant::now();
    let Ok(mut targets) = addr.to_socket_addrs() else {
        return 0;
    };
    let Some(target) = targets.next() else {
        return 0;
    };
    match std::net::TcpStream::connect_timeout(&target, Duration::from_millis(2000)) {
        Ok(stream) => {
            drop(stream);
            u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX)
        }
        Err(_) => 0,
    }
}

/// The shipped harness, read into memory.
fn default_harness() -> Harness {
    Harness {
        dir: std::path::PathBuf::from("assets/harness/default"),
        gates_toml: DEFAULT_GATES.to_owned(),
        policy_toml: DEFAULT_POLICY.to_owned(),
        phrasebook_toml: DEFAULT_PHRASEBOOK.to_owned(),
        reflexes: BTreeMap::new(),
        commit: "content:test-harness".to_owned(),
    }
}

/// What one round did.
#[derive(Clone, Debug, Default)]
struct Round {
    /// The lines the agents spoke.
    utterances: u64,
    /// The orders the round spoke, and the ones the addressee took up.
    orders: u64,
    orders_taken_up: u64,
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
}

/// The configuration the measurement runs with.
///
/// The box to measure. The default is the Thor's own d1-2B; the environment
/// points the round at another server of the same shape (the batched one),
/// so the two can be measured on the same packs. The port override names the
/// d1-2B's own port only: the batched server is a drop-in for :8094, and the
/// other models keep the ports they have.
fn measured_cfg() -> AgentsConfig {
    let mut cfg = AgentsConfig::default();
    if let Ok(base) = std::env::var("LM_THOR_BASE") {
        cfg.thor_base = base;
    }
    if let Ok(port) = std::env::var("LM_THOR_PORT") {
        cfg.ports.insert(
            "d1-2b".to_owned(),
            port.parse().expect("the port is a number"),
        );
    }
    cfg
}

/// Play one round on the Thor, at the round's own rate, and measure it.
///
/// The round's own timings come back with it (N1): where a decision's time
/// went is part of what the round measured.
fn play(watcher: Watcher, seed: u64) -> (Round, Vec<Ask>, BTreeMap<String, u64>, Vec<AskTiming>) {
    let header = Header::default_round(seed, DEFAULT_REFLEX);
    let harness = default_harness();
    let cfg = measured_cfg();

    let log = watcher.log();
    let refused = watcher.refused();
    let backends = BTreeMap::from([(
        // The policy's tiers ask for the fast tier by this name; the watcher
        // behind the name is the model this round measures.
        WATCHED_BACKEND.to_owned(),
        Box::new(watcher) as Box<dyn SysOne + Send + Sync>,
    )]);

    let mut host = HostLoop::new(header.clone()).expect("the round starts");
    host.attach_agents(Agents::new(&cfg, &harness, &header, backends));

    let period = Duration::from_millis(1000 / u64::from(header.params.tick_hz));
    for _ in 0..TICKS {
        std::thread::sleep(period);
        host.tick();
    }

    let metrics = talk_metrics(host.sim().talk_records());
    let stats = host.agent_stats().expect("the round has agents");
    let round = Round {
        utterances: metrics.utterances,
        orders: metrics.orders,
        orders_taken_up: metrics.orders_taken_up,
        decisions: stats.decisions,
        refusals: stats.refusals,
        thinks: stats.thinks,
        answered: stats.answered,
        expired: stats.expired,
        escalations: stats.escalations,
    };
    let log = log.lock().expect("the watcher's log").clone();
    let refused = refused.lock().expect("the watcher's refusals").clone();
    let timings = host
        .agents()
        .expect("the round has agents")
        .timings()
        .to_vec();
    (round, log, refused, timings)
}

/// The split of one pack's thinks, in milliseconds (N1).
///
/// The whole think, from the mind asking to the decision being applied, is
/// made of five parts, and every millisecond of it is in one of them.
#[derive(Default)]
struct Split {
    /// The wait in our own least-use-first queue.
    queue: Vec<u64>,
    /// The wait in the worker's own channel: the call before this one.
    worker: Vec<u64>,
    /// The wire to the Thor.
    network: Vec<u64>,
    /// The model's own time: the pack and its second opinions, less the wire.
    server: Vec<u64>,
    /// The wait from the answer landing to the decision being applied.
    apply: Vec<u64>,
    /// The whole think.
    total: Vec<u64>,
}

/// The split of one pack's thinks.
///
/// The round's own timings and the watcher's log are joined per pack: both
/// are in the order the pack's worker worked through its asks, so the k-th
/// ask of a pack is the k-th of each.
///
/// The join is on the backend as well as the pack. A round may ask more than
/// one backend — the shipped policy's deep tier names the cloud — and only
/// the Thor is watched, so the timings of the asks that went elsewhere are
/// left out. Without that, the k-th of one list would be the (k+1)-th of the
/// other, and every wire time would be read off the wrong ask.
fn split_of(pack: Pack, backend: &str, timings: &[AskTiming], log: &[Ask], tick_hz: u16) -> Split {
    let mut out = Split::default();
    let asked: Vec<&AskTiming> = timings
        .iter()
        .filter(|t| t.pack == pack && t.backend == backend)
        .collect();
    let logged: Vec<&Ask> = log.iter().filter(|a| a.pack == pack).collect();
    for (timing, entry) in asked.iter().zip(logged.iter()) {
        if entry.refusal.is_some() {
            // No answer came back, so there is no model to split out.
            continue;
        }
        let flight = timing.flight_ms();
        let network = u64::from(entry.network_ms);
        let apply = timing.apply_ms(tick_hz);
        out.queue.push(timing.queue_ms());
        out.worker.push(timing.worker_ms());
        out.network.push(network);
        out.server.push(flight.saturating_sub(network));
        out.apply.push(apply);
        out.total
            .push(timing.queue_ms() + timing.worker_ms() + flight + apply);
    }
    out
}

/// The percentile of a sorted slice of milliseconds, by rank.
fn percentile_u64(sorted: &[u64], q: f32) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = ((sorted.len() - 1) as f32 * q).round() as usize;
    sorted[rank]
}

/// The median and the 95th percentile of a set of milliseconds.
fn split_row(values: &[u64]) -> (u64, u64) {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    (percentile_u64(&sorted, 0.50), percentile_u64(&sorted, 0.95))
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

/// The option a distribution favoured: the key of its largest probability.
///
/// The same rule the escalation record uses, so the judge's answer and the
/// two models' answers are compared by the same key.
fn top_option(answer: &Answer) -> Option<String> {
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
    best.map(|(key, _)| key.clone())
}

/// What one arm did, over its rounds.
struct ArmReport {
    arm: Arm,
    /// The seeds the arm played, in order: the report names them.
    seeds: Vec<u64>,
    rounds: Vec<Round>,
    asks: Vec<Ask>,
    refused: BTreeMap<String, u64>,
    /// The round's own timings: where each decision's time went (N1).
    timings: Vec<AskTiming>,
}

/// Play one arm's rounds and print the arm's numbers.
fn run_arm(arm: Arm) -> ArmReport {
    let cfg = measured_cfg();
    println!(
        "# the arm: {} (escalate below {:.2})",
        arm.name(),
        cfg.escalate_below
    );
    let mut rounds = Vec::new();
    let mut asks = Vec::new();
    let mut timings: Vec<AskTiming> = Vec::new();
    let mut refused: BTreeMap<String, u64> = BTreeMap::new();
    let seeds = seeds();
    for &seed in &seeds {
        let watcher = Watcher::d1_2b(&cfg, arm);
        let (round, log, round_refused, seed_timings) = play(watcher, seed);
        println!(
            "round {seed}: {} utterances ({} orders, {} taken up), {} decisions, {} refusals, \
             {} asks sent, {} answered, {} expired, {} escalations, {} answers seen",
            round.utterances,
            round.orders,
            round.orders_taken_up,
            round.decisions,
            round.refusals,
            round.thinks,
            round.answered,
            round.expired,
            round.escalations,
            log.len()
        );
        for (name, count) in round_refused {
            *refused.entry(name).or_default() += count;
        }
        rounds.push(round);
        asks.extend(log);
        timings.extend(seed_timings);
    }

    // The packs that were asked, dumped so they can be asked again of
    // another server and the answers compared: LM_CAPTURE_PACKS=<file>
    // writes one pack per line.
    if let Ok(path) = std::env::var("LM_CAPTURE_PACKS") {
        let mut text = String::new();
        for ask in &asks {
            let line = serde_json::json!({
                "pack": ask.pack.as_str(),
                "state": ask.state,
                "questions": ask.questions,
            });
            text.push_str(&serde_json::to_string(&line).expect("the pack is JSON"));
            text.push('\n');
        }
        std::fs::write(&path, text).expect("the packs are written");
        println!("wrote {} packs to {path}", asks.len());
    }

    ArmReport {
        arm,
        seeds,
        rounds,
        asks,
        refused,
        timings,
    }
}

/// Print one arm's aggregate: the decisions, the lines, the escalation rate
/// and the latencies, next to the other arms'.
fn print_arm(report: &ArmReport) {
    let rounds: Vec<&Round> = report.rounds.iter().collect();
    let sum = |f: fn(&Round) -> u64| rounds.iter().map(|r| f(r)).sum::<u64>();
    let utterances = sum(|r| r.utterances);
    let orders = sum(|r| r.orders);
    let taken_up = sum(|r| r.orders_taken_up);
    let decisions = sum(|r| r.decisions);
    let refusals = sum(|r| r.refusals);
    let asks = report.asks.len() as u64;
    let escalations: Vec<&Escalated> = report
        .asks
        .iter()
        .flat_map(|ask| ask.escalations.iter())
        .collect();
    let gates_answered: u64 = report
        .asks
        .iter()
        .map(|ask| ask.confidence.len() as u64)
        .sum();

    println!("## the arm: {}", report.arm.name());
    println!();
    for (seed, round) in report.seeds.iter().zip(rounds.iter()) {
        println!(
            "- seed {seed}: {} lines ({} orders, {} taken up), {} decisions, {} refusals",
            round.utterances, round.orders, round.orders_taken_up, round.decisions, round.refusals
        );
    }
    println!(
        "- totals: {utterances} lines ({orders} orders, {taken_up} taken up), {decisions} decisions, {refusals} refusals, {asks} asks"
    );
    println!("- escalations: {}", escalations.len());
    if !escalations.is_empty() {
        let mut by_gate: BTreeMap<&str, usize> = BTreeMap::new();
        let mut outcomes: BTreeMap<String, usize> = BTreeMap::new();
        for escalated in &escalations {
            *by_gate
                .entry(escalated.escalation.gate.as_str())
                .or_default() += 1;
            *outcomes
                .entry(escalated.escalation.outcome.to_string())
                .or_default() += 1;
        }
        println!("- by gate: {}", {
            let parts: Vec<String> = by_gate
                .iter()
                .map(|(gate, count)| format!("{gate}: {count}"))
                .collect();
            parts.join(", ")
        });
        println!("- by outcome: {}", {
            let parts: Vec<String> = outcomes
                .iter()
                .map(|(outcome, count)| format!("{outcome}: {count}"))
                .collect();
            parts.join(", ")
        });
        println!(
            "- the rate: {:.1}% of the gates answered, {:.1}% of the asks, {:.1}% of the decisions",
            100.0 * escalations.len() as f64 / gates_answered.max(1) as f64,
            100.0 * escalations.len() as f64 / asks.max(1) as f64,
            100.0 * escalations.len() as f64 / decisions.max(1) as f64
        );
    }

    // The split of a think (N1): the same latency, taken apart.
    //
    // The round trip is one number around the HTTP call. This is the whole
    // latency of a think, part by part: the wait in our own queue, the
    // worker's own wait, the wire, the model, and the wait for the
    // decision to be applied. Every millisecond of the think is in one of
    // them, and the last row is their sum.
    let tick_hz = Header::default_round(SEEDS[0], DEFAULT_REFLEX)
        .params
        .tick_hz;
    let move_split = split_of(
        Pack::Move,
        WATCHED_BACKEND,
        &report.timings,
        &report.asks,
        tick_hz,
    );
    let talk_split = split_of(
        Pack::Talk,
        WATCHED_BACKEND,
        &report.timings,
        &report.asks,
        tick_hz,
    );
    println!("## the split of a think (N1)");
    println!();
    println!("| part | move pack p50 | move pack p95 | talk pack p50 | talk pack p95 |");
    println!("|---|---|---|---|---|");
    let rows: [(&str, &[u64], &[u64]); 6] = [
        (
            "our queue: the least-use-first wait",
            &move_split.queue,
            &talk_split.queue,
        ),
        (
            "the worker's own queue: the call before this one",
            &move_split.worker,
            &talk_split.worker,
        ),
        ("the wire", &move_split.network, &talk_split.network),
        ("the model", &move_split.server, &talk_split.server),
        (
            "the apply: the lead before the decision lands",
            &move_split.apply,
            &talk_split.apply,
        ),
        ("the whole think", &move_split.total, &talk_split.total),
    ];
    for (name, move_vals, talk_vals) in rows {
        let (move_p50, move_p95) = split_row(move_vals);
        let (talk_p50, talk_p95) = split_row(talk_vals);
        println!("| {name} | {move_p50} ms | {move_p95} ms | {talk_p50} ms | {talk_p95} ms |");
    }
    println!();
    let elsewhere = report
        .timings
        .iter()
        .filter(|t| t.backend != WATCHED_BACKEND)
        .count();
    println!(
        "- the split covers {} move pack thinks and {} talk pack thinks, over {} watched asks and {} timings",
        move_split.total.len(),
        talk_split.total.len(),
        report.asks.len(),
        report.timings.len()
    );
    println!(
        "- {} asks went to another backend (the shipped policy's deep tier names the cloud) \
         and are not in the split: they never reached the Thor",
        elsewhere
    );
    println!();

    // The round trip per pack, and the latency the escalation added.
    for (pack, name) in [(Pack::Move, "move pack"), (Pack::Talk, "talk pack")] {
        let mut latencies: Vec<u32> = report
            .asks
            .iter()
            .filter(|ask| ask.pack == pack)
            .map(|ask| ask.latency_ms)
            .collect();
        latencies.sort_unstable();
        if latencies.is_empty() {
            println!("- the {name}: no answer came back");
            continue;
        }
        println!(
            "- the {name}: min {} ms, p50 {} ms, p95 {} ms, max {} ms over {} answers",
            latencies[0],
            percentile_ms(&latencies, 0.50),
            percentile_ms(&latencies, 0.95),
            latencies[latencies.len() - 1],
            latencies.len()
        );
    }
    let mut escalation_latencies: Vec<u32> = escalations
        .iter()
        .map(|e| e.escalation.latency_ms)
        .collect();
    escalation_latencies.sort_unstable();
    if !escalation_latencies.is_empty() {
        println!(
            "- the added latency: min {} ms, p50 {} ms, p95 {} ms, max {} ms",
            escalation_latencies[0],
            percentile_ms(&escalation_latencies, 0.50),
            percentile_ms(&escalation_latencies, 0.95),
            escalation_latencies[escalation_latencies.len() - 1]
        );
    }
    if !report.refused.is_empty() {
        println!("- the Thor refused: {}", {
            let parts: Vec<String> = report
                .refused
                .iter()
                .map(|(name, count)| format!("{name}: {count}"))
                .collect();
            parts.join(", ")
        });
    }
    println!();
}

/// Ask the offline judge — the Thor's d1-3B, neither party in the dispute —
/// every question the "all" arm escalated, and report whose choice it agrees
/// with.
///
/// The judge is asked the same single question over the same facts, one
/// question in one call, exactly what the second model was asked: the facts
/// are the same for both parties, so the only difference between them is the
/// model. The d1 servers are deterministic, so the judge's answer is the
/// same on a re-ask.
fn judge_escalations(escalated: &[Escalated]) {
    let cfg = measured_cfg();
    let judge = ThorBackend::new(&cfg.thor_base, cfg.ports["d1-3b"], "d1-3b")
        .with_min_confidence(0.0)
        .with_timeout(Duration::from_secs(10));

    // The situations are many, the answers are not: every distinct
    // (situation, question) pair counts once, as everywhere in this file.
    let mut distinct: BTreeMap<String, &Escalated> = BTreeMap::new();
    for one in escalated {
        let key = format!("{}\n{}", one.state, one.question);
        distinct.entry(key).or_insert(one);
    }
    let situations: Vec<&Escalated> = distinct.into_values().collect();
    println!(
        "# the judge: d1-3B on the full facts, asked each escalated question alone ({} distinct of {} escalations)",
        situations.len(),
        escalated.len()
    );
    println!();

    #[derive(Default)]
    struct PerGate {
        seen: usize,
        answered: usize,
        differ: usize,
        primary_agrees: usize,
        second_agrees: usize,
        both_differ: usize,
    }
    let mut per_gate: BTreeMap<String, PerGate> = BTreeMap::new();
    let mut judge_refusals: BTreeMap<String, u64> = BTreeMap::new();
    for one in &situations {
        let gate = one.escalation.gate.clone();
        let entry = per_gate.entry(gate.clone()).or_default();
        entry.seen += 1;
        let pack = serde_json::json!({ gate.clone(): one.question.clone() });
        match judge.ask(&one.state, &pack, Instant::now() + Duration::from_secs(10)) {
            Err(refusal) => {
                *judge_refusals.entry(refusal.to_string()).or_default() += 1;
            }
            Ok(answers) => {
                let Some(judge_choice) = answers.per_gate.get(&gate).and_then(top_option) else {
                    *judge_refusals
                        .entry("the judge answered nothing".to_owned())
                        .or_default() += 1;
                    continue;
                };
                entry.answered += 1;
                let primary = one.escalation.primary_choice.as_deref();
                let second = one.escalation.second_choice.as_deref();
                if primary != second {
                    entry.differ += 1;
                }
                if primary == Some(judge_choice.as_str()) {
                    entry.primary_agrees += 1;
                }
                if second == Some(judge_choice.as_str()) {
                    entry.second_agrees += 1;
                }
                if primary != Some(judge_choice.as_str()) && second != Some(judge_choice.as_str()) {
                    entry.both_differ += 1;
                }
            }
        }
    }
    println!("| gate | situations judged | the two models differ | the d1-2B agrees with the judge | clef-flash agrees | neither agrees |");
    println!("|---|---|---|---|---|---|");
    for (gate, counts) in &per_gate {
        println!(
            "| {gate} | {} | {} | {} | {} | {} |",
            counts.answered,
            counts.differ,
            counts.primary_agrees,
            counts.second_agrees,
            counts.both_differ
        );
    }
    if !judge_refusals.is_empty() {
        println!("- the judge refused: {judge_refusals:?}");
    }
    println!();
}

/// The d1-3B's own calibration: play the round with the d1-3B behind the
/// tier and the floor at zero, and read the confidence floor off the model's
/// own scale — the way the d1-2B's 0.30 was read
/// (crates/lm-agents/tests/talk_on_thor.md).
fn calibrate_d1_3b() {
    let cfg = measured_cfg();
    println!("# the d1-3B behind the tier, the floor at zero");
    println!();
    let mut rounds = Vec::new();
    let mut asks = Vec::new();
    for seed in SEEDS {
        let watcher = Watcher::d1_3b(&cfg);
        let (round, log, _refused, _timings) = play(watcher, seed);
        println!(
            "round {seed}: {} utterances, {} decisions, {} refusals, {} asks sent, {} answered, {} expired",
            round.utterances,
            round.decisions,
            round.refusals,
            round.thinks,
            round.answered,
            round.expired
        );
        rounds.push(round);
        asks.extend(log);
    }

    // The distribution is over the distinct situations, as everywhere here.
    let mut distinct: BTreeMap<String, &Ask> = BTreeMap::new();
    for ask in &asks {
        distinct.entry(ask.state.clone()).or_insert(ask);
    }
    let situations: Vec<&Ask> = distinct.into_values().collect();
    println!();
    println!(
        "## the d1-3B's confidences ({} distinct situations)",
        situations.len()
    );
    println!();
    println!("| gate | n | min | p10 | p50 | p90 | max |");
    println!("|---|---|---|---|---|---|---|");
    for gate in &[
        "move",
        "gesture",
        "attention",
        "intention",
        "intent",
        "subject",
    ] {
        let mut values: Vec<f32> = situations
            .iter()
            .filter_map(|ask| ask.confidence.get(*gate).copied())
            .collect();
        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        if values.is_empty() {
            continue;
        }
        println!(
            "| {gate} | {} | {:.4} | {:.4} | {:.4} | {:.4} | {:.4} |",
            values.len(),
            values[0],
            percentile(&values, 0.10),
            percentile(&values, 0.50),
            percentile(&values, 0.90),
            values[values.len() - 1]
        );
    }
    println!();

    // The presence nouls too: the gate wordings are a model's own
    // ("the gate wordings are this model's"), and the d1-3B's presence scale
    // is part of what a harness that puts it behind the tier has to know.
    println!("## the d1-3B's presence nouls");
    println!();
    println!("| gate | n | min | p10 | p50 | p90 | max | below the floor |");
    println!("|---|---|---|---|---|---|---|---|");
    for gate in UTTERANCE_GATES {
        let id = presence_gate(gate);
        let mut values: Vec<f32> = situations
            .iter()
            .filter_map(|ask| ask.presence.get(&id).copied())
            .collect();
        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        if values.is_empty() {
            continue;
        }
        let below = values
            .iter()
            .filter(|v| **v < lm_agents::utter::PRESENCE_FLOOR)
            .count();
        println!(
            "| {id} | {} | {:.4} | {:.4} | {:.4} | {:.4} | {:.4} | {}/{} |",
            values.len(),
            values[0],
            percentile(&values, 0.10),
            percentile(&values, 0.50),
            percentile(&values, 0.90),
            values[values.len() - 1],
            below,
            values.len()
        );
    }
    println!();

    // The per-gate rule at the candidate floors: which gates a floor drops,
    // how many situations go silent (an unsure talk gate) and how many lose
    // the move (an unsure move gate). The candidates run from the d1-2B's
    // own floor (0.30 — where the d1-3B's whole talk pack dies, because its
    // intention gate answers near-flat) down to the second opinion's floor
    // (0.05).
    println!("## the per-gate rule on the d1-3B's own answers");
    println!();
    for floor in [0.05_f32, 0.10, 0.15, 0.20, 0.30] {
        let mut dropped: BTreeMap<&str, u64> = BTreeMap::new();
        let mut silent = 0u64;
        let mut no_move = 0u64;
        for ask in &situations {
            let mut unsure_talk = false;
            let mut unsure_move = false;
            for (gate, confidence) in &ask.confidence {
                if *confidence >= floor {
                    continue;
                }
                if UTTERANCE_GATES.contains(&gate.as_str()) {
                    unsure_talk = true;
                } else if gate == "move" {
                    unsure_move = true;
                }
                *dropped.entry(gate.as_str()).or_default() += 1;
            }
            if unsure_talk {
                silent += 1;
            }
            if unsure_move {
                no_move += 1;
            }
        }
        let dropped_line = if dropped.is_empty() {
            "none".to_owned()
        } else {
            dropped
                .iter()
                .map(|(gate, count)| format!("{gate}: {count}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        println!(
            "- at a floor of {floor:.2}: dropped {dropped_line}; {silent} situations silent, {no_move} without the move"
        );
    }
    println!();
}

/// The measurement: three arms on the Thor, the judge on the escalations,
/// and the d1-3B's own calibration.
#[test]
#[ignore = "needs the Thor at 192.168.0.78; run with --release --ignored --nocapture"]
fn talk_on_thor() {
    // The three arms, in the order the write-up reports them.
    let all = run_arm(Arm::All);
    let moved = run_arm(Arm::Move);
    let off = run_arm(Arm::Off);

    println!();
    println!("# the three arms");
    println!();
    for report in [&all, &moved, &off] {
        print_arm(report);
    }

    // The second opinion, compared answer by answer: every gate the "all"
    // arm escalated, judged offline by the d1-3B.
    let escalated: Vec<Escalated> = all
        .asks
        .iter()
        .flat_map(|ask| ask.escalations.iter().cloned())
        .collect();
    if !escalated.is_empty() {
        judge_escalations(&escalated);
    }

    // The d1-3B's own floor.
    calibrate_d1_3b();
}

/// Only the d1-3B's floor calibration, without the arms: a re-run when the
/// candidate floors change, so the slow arms are not paid for twice.
#[test]
#[ignore = "needs the Thor at 192.168.0.78; run with --release --ignored --nocapture"]
fn d1_3b_floor() {
    calibrate_d1_3b();
}
