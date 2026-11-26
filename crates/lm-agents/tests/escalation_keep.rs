//! Does the move gate's escalation earn its keep? (D75's open question)
//!
//! D75 kept the second opinion for the move gate alone, and its own
//! measurement then argued against it: on 21 escalated situations the
//! d1-3B judge sided with the d1-2B 4 times and clef-flash 0. A later
//! 75-situation sample read the same way but was small and taken under
//! heavy load. This is the pre-registered measurement that settles it
//! (`tests/talk_on_thor.md`, "Does escalation earn its keep"); the
//! registration was written before any of this file's data was collected.
//!
//! Two measurements, two tests:
//!
//! 1. **`collect_escalations`**: real rounds on the Thor, the shipped
//!    harness and the shipped policy (`escalate_gates = ["move"]`), seeds
//!    spread across rounds. Every escalated move situation — the d1-2B's
//!    move-gate confidence below 0.50, exactly the set the escalation
//!    touches — is recorded with the seed and the tick it landed on, and
//!    then asked offline of all three models: the d1-2B itself (its own
//!    answer, the servers are deterministic), clef-flash (the answer the
//!    escalation takes), and the d1-3B (the judge, neither party in the
//!    dispute). Each model's round trip is kept. The record is
//!    `evidence/escjudge/situations.jsonl`; one line per round goes to
//!    `collect_rounds.jsonl`.
//!
//! 2. **`escalation_arms`**: the same 16 seeds twice, 3000 ticks each,
//!    once with the shipped escalation and once with it off
//!    (`escalate_gates = []`), identical otherwise. Per seed: tags, enemy
//!    sight ticks, draws (score margin 0 at the cutoff), refusals, and the
//!    escalation's own count and cost. The D78 sign test over the per-seed
//!    deltas is computed offline, in `evidence/escjudge/`.
//!
//! ## The command
//!
//! ```text
//! cargo test --release -p lm-agents --test escalation_keep -- --ignored --nocapture
//! ```
//!
//! The knobs, all optional: `ESC_SEEDS` (the collection seeds, comma
//! separated), `ESC_ARM_SEEDS` (the arms' seeds), `ESC_TICKS` (the ticks a
//! round plays), `ESC_OUT` (the evidence directory), and `LM_THOR_BASE` /
//! `LM_THOR_PORT` as in `talk_on_thor`. The collection merges into what is
//! already in `situations.jsonl`, so it can be run in batches until the
//! pre-registered 200 distinct situations are in hand; the arms append one
//! line per round played.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lm_agents::config::AgentsConfig;
use lm_agents::gates::Pack;
use lm_agents::harness::Harness;
use lm_agents::sysone::{
    Answer, Answers, EscalatingBackend, Escalation, EscalationOutcome, Refusal, SysOne, ThorBackend,
};
use lm_agents::{Agents, AskTiming};
use lm_core::header::Header;
use lm_core::talk::talk_metrics;
use lm_core::world::los;
use lm_host_model::HostLoop;
use serde_json::{json, Value};

/// The shipped harness: the gates, the policy and the phrasebook the default
/// round thinks with.
const DEFAULT_GATES: &str = include_str!("../../../assets/harness/default/gates.toml");
const DEFAULT_POLICY: &str = include_str!("../../../assets/harness/default/policy.toml");
const DEFAULT_PHRASEBOOK: &str = include_str!("../../../assets/harness/default/phrasebook.toml");

/// The shipped reflex, compiled into the test binary.
const DEFAULT_REFLEX: &str = include_str!("../../../assets/reflexes/default.rhai");

/// The ticks a round is played for: a hundred seconds at 30 Hz.
const TICKS: u64 = 3_000;

/// The backend the watcher wraps: the Thor's d1-2B, the tier the shipped
/// policy names.
const WATCHED_BACKEND: &str = "thor-d1-2b";

/// The enemy's own reach, in cells: the facts builder's ENEMY_NEAR_CELLS.
const ENEMY_NEAR_CELLS: i32 = 6;

/// The seeds the collection plays when the environment names none: twenty
/// rounds' worth, to be extended until the pre-registered 200 distinct
/// situations are in hand.
const COLLECT_SEEDS: [u64; 20] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
];

/// The seeds the arms play when the environment names none: sixteen, the
/// D78 count, paired across both arms.
const ARM_SEEDS: [u64; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];

/// The root of the repository, where the assets and the evidence live.
///
/// Cargo runs a test binary with the package directory as its working
/// directory, so the assets are found from the manifest, not the cwd.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The evidence directory the measurement writes to
/// (`ESC_OUT`, or `evidence/escjudge` under the repository root).
fn out_dir() -> PathBuf {
    match std::env::var("ESC_OUT") {
        Ok(name) => PathBuf::from(name),
        Err(_) => repo_root().join("evidence/escjudge"),
    }
}

/// The ticks a round plays (`ESC_TICKS`, or the default).
fn ticks() -> u64 {
    std::env::var("ESC_TICKS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(TICKS)
}

/// The seeds a knob names, or its default.
fn seeds_of(name: &str, default: &[u64]) -> Vec<u64> {
    match std::env::var(name) {
        Ok(text) => text
            .split(',')
            .filter_map(|part| part.trim().parse::<u64>().ok())
            .collect(),
        Err(_) => default.to_vec(),
    }
}

/// The box to measure, as in `talk_on_thor`: the Thor's own d1-2B by
/// default, the batched server a drop-in when `LM_THOR_PORT` names it.
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

/// The load on the box, as `/proc/loadavg` states it, with a note of when.
///
/// Latency numbers move with the box's load (the talk_on_thor
/// measurements), so every phase of this measurement states the load it
/// ran under. The read is telemetry outside the sim.
fn load_average() -> String {
    std::fs::read_to_string("/proc/loadavg")
        .unwrap_or_else(|_| "unknown".to_owned())
        .lines()
        .next()
        .unwrap_or("unknown")
        .to_owned()
}

/// The shipped harness, read into memory.
fn default_harness() -> Harness {
    Harness {
        dir: std::path::PathBuf::from("assets/harness/default"),
        gates_toml: DEFAULT_GATES.to_owned(),
        policy_toml: DEFAULT_POLICY.to_owned(),
        phrasebook_toml: DEFAULT_PHRASEBOOK.to_owned(),
        facts_toml: String::new(),
        reflexes: BTreeMap::new(),
        commit: "content:test-harness".to_owned(),
    }
}

/// The option a distribution favoured: the key of its largest probability.
///
/// The same rule the escalation record uses, so every model's answer in
/// this measurement is read the same way.
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

/// One ask that came back, and what the escalation did with it.
#[derive(Clone, Debug)]
struct Ask {
    /// Which pack the ask carried (D70).
    pack: Pack,
    /// The situation the question was asked over: the facts text.
    state: String,
    /// The pack as it was asked, so the same question can be asked again
    /// of every model offline.
    questions: Value,
    /// The escalations this ask paid for (D71), if any.
    escalations: Vec<Escalation>,
    /// The refusal the call came back with, if it came back with one.
    refusal: Option<String>,
}

/// The watcher: the shipped backends behind a log.
struct Watcher {
    inner: Box<dyn SysOne + Send + Sync>,
    log: Arc<Mutex<Vec<Ask>>>,
}

impl Watcher {
    /// The `on` arm: the shipped escalation, the d1-2B behind the tier and
    /// clef-flash as the second opinion, the move gate alone below 0.50 —
    /// exactly what `policy.toml`'s `escalate_gates = ["move"]` buys, with
    /// the floors the config calibrates.
    fn on(cfg: &AgentsConfig) -> Self {
        let primary = ThorBackend::new(&cfg.thor_base, cfg.ports["d1-2b"], "d1-2b")
            .with_min_confidence(cfg.min_confidence["thor-d1-2b"])
            .with_timeout(Duration::from_millis(u64::from(cfg.request_timeout_ms)));
        let second = ThorBackend::new(&cfg.thor_base, cfg.ports["clef-flash"], "clef-flash")
            .with_min_confidence(cfg.min_confidence["thor-clef-flash"])
            .with_timeout(Duration::from_millis(u64::from(cfg.request_timeout_ms)));
        let inner = EscalatingBackend::new(Box::new(primary), Box::new(second), cfg.escalate_below)
            .with_gates(Some(BTreeSet::from(["move".to_owned()])));
        Self::over(Box::new(inner))
    }

    /// The `off` arm: the d1-2B alone, the floors and the deadline as they
    /// ship — what `escalate_gates = []` plays.
    fn off(cfg: &AgentsConfig) -> Self {
        let inner = ThorBackend::new(&cfg.thor_base, cfg.ports["d1-2b"], "d1-2b")
            .with_min_confidence(cfg.min_confidence["thor-d1-2b"])
            .with_timeout(Duration::from_millis(u64::from(cfg.request_timeout_ms)));
        Self::over(Box::new(inner))
    }

    fn over(inner: Box<dyn SysOne + Send + Sync>) -> Self {
        Watcher {
            inner,
            log: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// The watcher's log of the answers that came back.
    fn log(&self) -> Arc<Mutex<Vec<Ask>>> {
        Arc::clone(&self.log)
    }
}

impl SysOne for Watcher {
    fn ask(&self, state: &str, questions: &Value, deadline: Instant) -> Result<Answers, Refusal> {
        self.ask_with_escalation(state, questions, deadline, deadline)
    }

    /// The watcher forwards the escalation's deadline (N3).
    ///
    /// It wraps the escalating backend, so the fence behind it has to
    /// survive the wrap.
    fn ask_with_escalation(
        &self,
        state: &str,
        questions: &Value,
        deadline: Instant,
        escalation_deadline: Instant,
    ) -> Result<Answers, Refusal> {
        let answers =
            match self
                .inner
                .ask_with_escalation(state, questions, deadline, escalation_deadline)
            {
                Ok(answers) => answers,
                Err(refusal) => {
                    self.log.lock().expect("the watcher's log").push(Ask {
                        pack: pack_of(questions),
                        state: state.to_owned(),
                        questions: questions.clone(),
                        escalations: Vec::new(),
                        refusal: Some(refusal.to_string()),
                    });
                    return Err(refusal);
                }
            };
        let escalations: Vec<Escalation> = answers.escalated.values().cloned().collect();
        self.log.lock().expect("the watcher's log").push(Ask {
            pack: pack_of(questions),
            state: state.to_owned(),
            questions: questions.clone(),
            escalations,
            refusal: None,
        });
        Ok(answers)
    }

    fn name(&self) -> &str {
        self.inner.name()
    }
}

/// The pack a set of questions carried (D70): the move pack holds the move
/// gate, the talk pack the gates a line is chosen by.
fn pack_of(questions: &Value) -> Pack {
    if questions.get("move").is_some() {
        Pack::Move
    } else {
        Pack::Talk
    }
}

/// What one round did, measured.
#[derive(Clone, Debug, Default)]
struct Round {
    /// The seed the round ran on.
    seed: u64,
    /// The ticks the round played.
    ticks: u64,
    /// The tags the ruleset made.
    tags: u64,
    /// The ticks on which some agent had a fresh enemy in sight, a clear
    /// line to it, within the enemy's reach.
    sight_ticks: u64,
    /// The squads' final scores.
    scores: Vec<i64>,
    /// The gap between the squads' scores: 0 is the pre-registered draw.
    margin: i64,
    /// The winner the round declared, if it declared one by the cutoff.
    winner: Option<u8>,
    /// The lines the agents spoke.
    utterances: u64,
    /// The decisions the agents' answers became.
    decisions: u64,
    /// The questions the minds sent.
    asks: u64,
    /// The asks the call itself refused: no answer came back, so no
    /// escalation either.
    asks_refused: u64,
    /// The thoughts the model refused, of every kind together.
    refusals: u64,
    /// The refusals by name, the kinds that happened.
    refusal_kinds: Vec<(String, u64)>,
    /// The gates asked of a second model.
    escalations: u64,
    /// The escalations by outcome, the outcomes that happened.
    escalation_outcomes: Vec<(String, u64)>,
    /// The escalations' round trips, in milliseconds, every one of them.
    escalation_latencies_ms: Vec<u32>,
}

impl Round {
    /// The round as one JSON line for the evidence.
    fn to_json(&self, arm: &str, load: &str) -> Value {
        json!({
            "arm": arm,
            "seed": self.seed,
            "ticks": self.ticks,
            "tags": self.tags,
            "sight_ticks": self.sight_ticks,
            "scores": self.scores,
            "margin": self.margin,
            "winner": self.winner,
            "utterances": self.utterances,
            "decisions": self.decisions,
            "asks": self.asks,
            "asks_refused": self.asks_refused,
            "refusals": self.refusals,
            "refusal_kinds": self.refusal_kinds.iter()
                .map(|(kind, count)| json!({"kind": kind, "count": count}))
                .collect::<Vec<_>>(),
            "escalations": self.escalations,
            "escalation_outcomes": self.escalation_outcomes.iter()
                .map(|(outcome, count)| json!({"outcome": outcome, "count": count}))
                .collect::<Vec<_>>(),
            "escalation_latencies_ms": self.escalation_latencies_ms,
            "escalation_cost_ms": self.escalation_latencies_ms.iter().map(|ms| u64::from(*ms)).sum::<u64>(),
            "load": load,
        })
    }
}

/// Play one round on the Thor, at the round's own rate, and measure it.
///
/// The round is the shipped path throughout — the host loop, the agents,
/// the least-use-first queue, the thinker, the sequencer — with the
/// watcher's backends behind the tier the policy names.
fn play(watcher: Watcher, seed: u64) -> (Round, Vec<Ask>, Vec<AskTiming>) {
    let header = Header::default_round(seed, DEFAULT_REFLEX);
    let harness = default_harness();
    let cfg = measured_cfg();

    let log = watcher.log();
    let backends = BTreeMap::from([(
        WATCHED_BACKEND.to_owned(),
        Box::new(watcher) as Box<dyn SysOne + Send + Sync>,
    )]);

    let mut host = HostLoop::new(header.clone()).expect("the round starts");
    host.attach_agents(Agents::new(&cfg, &harness, &header, backends));

    let period = Duration::from_millis(1000 / u64::from(header.params.tick_hz));
    let mut tags = 0u64;
    let mut sight_ticks = 0u64;
    for _ in 0..ticks() {
        std::thread::sleep(period);

        // What every agent can see on this tick, before the rules run: the
        // engagement measurement's definition of an enemy-sight tick.
        let mut seen_by_any = false;
        for entry in &header.roster {
            let view = host.sim().view(entry.agent);
            if enemy_in_sight(&view, host.sim().world(), cfg.stale_enemy_ticks) {
                seen_by_any = true;
                break;
            }
        }
        if seen_by_any {
            sight_ticks += 1;
        }

        let out = host.tick();
        for event in &out.events {
            if matches!(event, lm_core::rules::Event::Tagged { .. }) {
                tags += 1;
            }
        }
        if host.sim().outcome().is_some() {
            break;
        }
    }

    let metrics = talk_metrics(host.sim().talk_records());
    let stats = host.agent_stats().expect("the round has agents");
    let asks = {
        let guard = log.lock().expect("the watcher's log");
        guard.clone()
    };
    let escalations: Vec<&Escalation> =
        asks.iter().flat_map(|ask| ask.escalations.iter()).collect();
    let mut escalation_outcomes: BTreeMap<String, u64> = BTreeMap::new();
    for escalation in &escalations {
        *escalation_outcomes
            .entry(outcome_name(escalation.outcome).to_owned())
            .or_default() += 1;
    }
    let mut refusal_kinds = Vec::new();
    for kind in lm_core::refusal::RefusalKind::ALL {
        let count = stats.refusal_counts.get(kind);
        if count > 0 {
            refusal_kinds.push((kind.to_string(), count));
        }
    }
    let scores = host.sim().scores().to_vec();
    let margin = match (scores.iter().min(), scores.iter().max()) {
        (Some(low), Some(high)) => high - low,
        _ => 0,
    };
    let round = Round {
        seed,
        ticks: host.sim().tick(),
        tags,
        sight_ticks,
        margin,
        winner: host.sim().outcome().and_then(|outcome| outcome.winner),
        utterances: metrics.utterances,
        decisions: stats.decisions,
        asks: stats.thinks,
        asks_refused: asks.iter().filter(|ask| ask.refusal.is_some()).count() as u64,
        refusals: stats.refusals,
        refusal_kinds,
        escalations: escalations.len() as u64,
        escalation_outcomes: escalation_outcomes.into_iter().collect(),
        escalation_latencies_ms: escalations.iter().map(|e| e.latency_ms).collect(),
        scores,
    };
    let timings = host
        .agents()
        .expect("the round has agents")
        .timings()
        .to_vec();
    (round, asks, timings)
}

/// The outcome's name, as the evidence writes it.
fn outcome_name(outcome: EscalationOutcome) -> &'static str {
    match outcome {
        EscalationOutcome::Taken => "taken",
        EscalationOutcome::Unsure => "unsure",
        EscalationOutcome::Timeout => "timeout",
        EscalationOutcome::Unavailable => "unavailable",
        EscalationOutcome::Late => "late",
    }
}

/// Whether the agent has a fresh enemy within the enemy's reach and a
/// clear line of sight to it: the engagement measurement's sight.
fn enemy_in_sight(
    view: &lm_core::view::ScoreView,
    world: &lm_core::world::World,
    stale_after: u32,
) -> bool {
    let me = view.me.cell;
    for enemy in view.enemies.iter().filter(|e| e.age_ticks <= stale_after) {
        let dist = (i32::from(enemy.cell.x) - i32::from(me.x))
            .abs()
            .max((i32::from(enemy.cell.y) - i32::from(me.y)).abs());
        if dist <= ENEMY_NEAR_CELLS && los::clear(&world.grid, me, enemy.cell) {
            return true;
        }
    }
    false
}

/// One offline answer: what a model said when asked the escalated
/// question alone, and what the call cost.
#[derive(Clone, Debug, Default)]
struct Offline {
    /// The option the model's own distribution favoured.
    choice: Option<String>,
    /// The confidence of that answer.
    confidence: Option<f32>,
    /// The round trip the model reported, in milliseconds.
    latency_ms: u32,
    /// Why the model refused, when it refused every try.
    refusal: Option<String>,
}

/// Ask one model one question, retrying a little: the box is shared, and a
/// busy moment must not cost the measurement a situation.
fn ask_offline(backend: &dyn SysOne, state: &str, question: &Value, tries: u32) -> Offline {
    let mut last = Offline::default();
    for _ in 0..tries {
        let pack = json!({ "move": question });
        match backend.ask(state, &pack, Instant::now() + Duration::from_secs(10)) {
            Ok(answers) => {
                let answer = answers.per_gate.get("move");
                return Offline {
                    choice: answer.and_then(top_option),
                    confidence: answer.and_then(|a| match a {
                        Answer::Choice { confidence, .. } | Answer::Score { confidence, .. } => {
                            Some(*confidence)
                        }
                        Answer::Noul(_) => None,
                    }),
                    latency_ms: answers.latency_ms,
                    refusal: None,
                };
            }
            Err(refusal) => {
                last = Offline {
                    refusal: Some(refusal.to_string()),
                    ..Offline::default()
                };
            }
        }
    }
    last
}

/// The collection: play the rounds, keep every escalated move situation,
/// and ask all three models each one alone.
///
/// The rounds are the shipped game — the harness, the policy and the
/// escalation exactly as they ship — so the situations are the situations
/// the escalation actually touched. The offline asks then put the three
/// models on the same footing: the same single question over the same
/// facts, one question in one call, each model asked alone, the floors at
/// zero so each model's own number is seen. The d1 servers are
/// deterministic, so the d1-2B's offline answer is the answer it gave in
/// the round; the cross-check is printed.
#[test]
#[ignore = "needs the Thor at 192.168.0.78; run with --release --ignored --nocapture"]
fn collect_escalations() {
    let cfg = measured_cfg();
    let out = out_dir();
    std::fs::create_dir_all(&out).expect("the evidence directory is made");
    let situations_path = out.join("situations.jsonl");
    let rounds_path = out.join("collect_rounds.jsonl");

    // What is already in hand, so the collection can run in batches: the
    // key is the situation itself, the facts and the question together.
    let mut known: BTreeSet<String> = BTreeSet::new();
    let mut situations: Vec<Value> = Vec::new();
    if let Ok(text) = std::fs::read_to_string(&situations_path) {
        for line in text.lines() {
            let Ok(value) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let key = situation_key(
                value.get("state").and_then(Value::as_str).unwrap_or(""),
                value.get("question").unwrap_or(&Value::Null),
            );
            if known.insert(key) {
                situations.push(value);
            }
        }
    }
    println!(
        "# the collection: {} situations already in hand, {} more rounds to play (load {})",
        situations.len(),
        seeds_of("ESC_SEEDS", &COLLECT_SEEDS).len(),
        load_average()
    );

    // One offline backend per model, the floors at zero, a generous
    // deadline: the offline asks price the models, not the game's queue.
    let d12b = ThorBackend::new(&cfg.thor_base, cfg.ports["d1-2b"], "d1-2b")
        .with_min_confidence(0.0)
        .with_timeout(Duration::from_secs(10));
    let clef = ThorBackend::new(&cfg.thor_base, cfg.ports["clef-flash"], "clef-flash")
        .with_min_confidence(0.0)
        .with_timeout(Duration::from_secs(10));
    let judge = ThorBackend::new(&cfg.thor_base, cfg.ports["d1-3b"], "d1-3b")
        .with_min_confidence(0.0)
        .with_timeout(Duration::from_secs(10));

    for seed in seeds_of("ESC_SEEDS", &COLLECT_SEEDS) {
        let load_start = load_average();
        let watcher = Watcher::on(&cfg);
        let (round, asks, timings) = play(watcher, seed);

        // The tick each move ask landed on: the round's own timings and
        // the watcher's log are both in the order the move worker worked
        // through them, so the k-th of one is the k-th of the other (the
        // join `talk_on_thor`'s split uses).
        let landed: Vec<u64> = timings
            .iter()
            .filter(|t| t.pack == Pack::Move && t.backend == WATCHED_BACKEND)
            .map(|t| t.landed)
            .collect();
        let mut move_asks = 0usize;

        for ask in &asks {
            if ask.pack != Pack::Move {
                continue;
            }
            let tick = landed.get(move_asks).copied().unwrap_or(0);
            move_asks += 1;
            for escalation in &ask.escalations {
                if escalation.gate != "move" {
                    continue;
                }
                let question = ask.questions.get("move").cloned().unwrap_or(Value::Null);
                let key = situation_key(&ask.state, &question);
                if !known.insert(key) {
                    continue;
                }
                situations.push(json!({
                    "seed": seed,
                    "tick": tick,
                    "state": ask.state,
                    "question": question,
                    "primary_confidence": escalation.primary_confidence,
                    "in_round": {
                        "outcome": outcome_name(escalation.outcome),
                        "primary_choice": escalation.primary_choice,
                        "primary_confidence": escalation.primary_confidence,
                        "second_choice": escalation.second_choice,
                        "second_confidence": escalation.second_confidence,
                        "latency_ms": escalation.latency_ms,
                    },
                }));
            }
        }

        // The offline asks, every situation that does not have them yet.
        for situation in situations.iter_mut() {
            if situation.get("judge").is_some() {
                continue;
            }
            let state = situation
                .get("state")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let question = situation.get("question").cloned().unwrap_or(Value::Null);
            let d12b_answer = ask_offline(&d12b, &state, &question, 3);
            let clef_answer = ask_offline(&clef, &state, &question, 3);
            let judge_answer = ask_offline(&judge, &state, &question, 3);
            situation["d1_2b"] = offline_json(&d12b_answer);
            situation["clef_flash"] = offline_json(&clef_answer);
            situation["judge"] = offline_json(&judge_answer);
        }

        // The record is written as the rounds go, not at the end: a batch
        // that stops early keeps what it collected.
        let mut text = String::new();
        for situation in &situations {
            text.push_str(&situation.to_string());
            text.push('\n');
        }
        std::fs::write(&situations_path, text).expect("the situations are written");

        let round_line = round
            .to_json(
                "collect",
                &format!("start {load_start}; end {}", load_average()),
            )
            .to_string();
        let mut rounds = String::new();
        if let Ok(existing) = std::fs::read_to_string(&rounds_path) {
            rounds = existing;
            if !rounds.ends_with('\n') && !rounds.is_empty() {
                rounds.push('\n');
            }
        }
        rounds.push_str(&round_line);
        rounds.push('\n');
        std::fs::write(&rounds_path, rounds).expect("the round lines are written");

        println!(
            "round {seed}: {} ticks, {} tags, {} sight ticks, margin {}, {} decisions, {} refusals, {} escalations ({:?}), {} situations in hand",
            round.ticks,
            round.tags,
            round.sight_ticks,
            round.margin,
            round.decisions,
            round.refusals,
            round.escalations,
            round.escalation_outcomes,
            situations.len()
        );
        // The refusals the calls themselves returned, spelled out: a round
        // that refuses more than it decides is a measurement that needs
        // saying, not smoothing over.
        let mut spelled: BTreeMap<String, u64> = BTreeMap::new();
        for ask in &asks {
            if let Some(refusal) = &ask.refusal {
                *spelled.entry(refusal.clone()).or_default() += 1;
            }
        }
        if !spelled.is_empty() {
            println!("  the calls refused: {spelled:?}");
        }
    }

    // The cross-check the registration promises: the d1 servers are
    // deterministic, so the offline answer is the round's own answer
    // wherever the round's record carries one. A sub-floor situation's
    // record carries no choice of its own (the shipped floor withholds
    // it), so those are counted apart, not read as mismatches.
    let mut checked = 0usize;
    let mut mismatched = 0usize;
    let mut sub_floor = 0usize;
    for situation in &situations {
        let in_round = situation
            .get("in_round")
            .and_then(|v| v.get("primary_choice"))
            .filter(|value| !value.is_null());
        let Some(in_round) = in_round else {
            sub_floor += 1;
            continue;
        };
        let Some(offline) = situation.get("d1_2b").and_then(|v| v.get("choice")) else {
            continue;
        };
        checked += 1;
        if in_round != offline {
            mismatched += 1;
        }
    }
    println!(
        "# {} distinct situations in {situations_path:?}; the determinism cross-check: {} of {} comparable, {} mismatched, {} sub-floor without a round choice (load {})",
        situations.len(),
        checked - mismatched,
        checked,
        mismatched,
        sub_floor,
        load_average()
    );
}

/// An offline answer as JSON.
fn offline_json(answer: &Offline) -> Value {
    json!({
        "choice": answer.choice,
        "confidence": answer.confidence,
        "latency_ms": answer.latency_ms,
        "refusal": answer.refusal,
    })
}

/// The key a situation is known by: the facts and the question together.
fn situation_key(state: &str, question: &Value) -> String {
    format!("{state}\u{1f}{}", question_to_string(question))
}

/// The question as a stable string, whatever the JSON's key order.
fn question_to_string(question: &Value) -> String {
    if let Ok(text) = serde_json::to_string(question) {
        return text;
    }
    question.to_string()
}

/// The arms: the same seeds twice, the escalation on and off, and what the
/// difference did to the rounds.
///
/// The pre-registration: 16 seeds x 3000 ticks, paired per seed, the `on`
/// arm the shipped `escalate_gates = ["move"]` and the `off` arm
/// `escalate_gates = []`, identical otherwise. The per-seed deltas go
/// through D78's exact sign test offline, in `evidence/escjudge/`.
#[test]
#[ignore = "needs the Thor at 192.168.0.78; run with --release --ignored --nocapture"]
fn escalation_arms() {
    let cfg = measured_cfg();
    let out = out_dir();
    std::fs::create_dir_all(&out).expect("the evidence directory is made");

    println!(
        "# the arms: {} seeds x {} ticks each, paired per seed (load {})",
        seeds_of("ESC_ARM_SEEDS", &ARM_SEEDS).len(),
        ticks(),
        load_average()
    );

    for seed in seeds_of("ESC_ARM_SEEDS", &ARM_SEEDS) {
        for (arm, watcher) in [("on", Watcher::on(&cfg)), ("off", Watcher::off(&cfg))] {
            let (round, _asks, _timings) = play(watcher, seed);
            println!(
                "arm {arm} seed {seed}: {} ticks, {} tags, {} sight ticks, margin {} ({}), {} decisions, {} refusals, {} escalations ({} ms), load {}",
                round.ticks,
                round.tags,
                round.sight_ticks,
                round.margin,
                if round.margin == 0 { "draw" } else { "decided" },
                round.decisions,
                round.refusals,
                round.escalations,
                round
                    .escalation_latencies_ms
                    .iter()
                    .map(|ms| u64::from(*ms))
                    .sum::<u64>(),
                load_average()
            );
            let path = out.join(format!("arms_{arm}.jsonl"));
            let mut existing = String::new();
            if let Ok(text) = std::fs::read_to_string(&path) {
                // A re-run replaces its own seeds' lines: the last line a
                // seed wrote is the line that counts.
                existing = text
                    .lines()
                    .filter(|line| {
                        serde_json::from_str::<Value>(line)
                            .ok()
                            .and_then(|value| value.get("seed").and_then(Value::as_u64))
                            .is_some_and(|s| s != seed)
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if !existing.is_empty() {
                    existing.push('\n');
                }
            }
            existing.push_str(&round.to_json(arm, &load_average()).to_string());
            existing.push('\n');
            std::fs::write(&path, existing).expect("the arm's lines are written");
        }
    }

    println!("# the arms are in {:?} (load {})", out, load_average());
}
