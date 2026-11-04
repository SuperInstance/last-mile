//! The talk gates on the real Thor: what the model answers, and what the
//! gates do with it.
//!
//! The agents used to decide fine on the Thor (57-85 decisions per 3000-tick
//! round) and say almost nothing (0-1 lines per round). This is the
//! measurement that says why, and the measurement the fix is read against:
//! it plays the round the way the game plays it — the host loop, the agents,
//! the least-use-first queue, the sequencer — and watches every ask that
//! reaches the model.
//!
//! ## What is watched
//!
//! Every ask that comes back is recorded with:
//!
//! - which pack it carried (D70: the move pack or the talk pack);
//! - the five presence nouls (`gesture_present`, `attention_present`,
//!   `intention_present`, `intent_present`, `subject_present`);
//! - the confidence of every gate that has one (the move gate, the five talk
//!   gates and the intention score);
//! - the lowest of those confidences, which is the number the old pack-level
//!   rule folded the whole pack into;
//! - which presence gate blocked the line, if one did;
//! - the round trip the model reported.
//!
//! The escalations (D71) are watched too: the gate, both confidences and the
//! latency the second opinion cost.
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

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lm_agents::config::AgentsConfig;
use lm_agents::gates::Pack;
use lm_agents::harness::Harness;
use lm_agents::sysone::{
    Answer, Answers, EscalatingBackend, Escalation, Refusal, SysOne, ThorBackend,
};
use lm_agents::utter::{presence_gate, PRESENCE_FLOOR, UTTERANCE_GATES};
use lm_agents::Agents;
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

/// The confidence floor the soak pins on the Thor's d1-2B.
const SOAK_FLOOR: f32 = 0.15;

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
    /// The lowest of those confidences.
    min_confidence: f32,
    /// The round trip the model reported, in milliseconds.
    latency_ms: u32,
    /// The escalations this ask paid for (D71).
    escalations: Vec<Escalation>,
}

/// The watcher: the Thor behind a log.
struct Watcher {
    inner: Box<dyn SysOne + Send + Sync>,
    log: Arc<Mutex<Vec<Ask>>>,
    /// The refusals the Thor itself produced, by name.
    refused: Arc<Mutex<BTreeMap<String, u64>>>,
}

impl Watcher {
    /// A watcher over the Thor's d1-2B, with the floor the soak pins and
    /// the second opinion the escalation asks (D71).
    ///
    /// The log is shared: the watcher is moved into the agents, and the
    /// measurement reads the log after the round.
    fn new(cfg: &AgentsConfig) -> Self {
        let port = cfg.ports["d1-2b"];
        let primary = ThorBackend::new(&cfg.thor_base, port, "d1-2b")
            // The floor is put at zero, so that every gate's answer is seen:
            // the measurement reads the model's own numbers, and the rule
            // that would have been applied to them is derived from them.
            .with_min_confidence(0.0)
            .with_timeout(Duration::from_millis(u64::from(cfg.request_timeout_ms)));
        // The second model (D71): the Thor's clef-flash, asked one question
        // at a time.
        let second_port = cfg.ports["clef-flash"];
        let second = ThorBackend::new(&cfg.thor_base, second_port, "clef-flash")
            .with_min_confidence(
                cfg.min_confidence
                    .get("thor-clef-flash")
                    .copied()
                    .unwrap_or(0.0),
            )
            .with_timeout(Duration::from_millis(u64::from(cfg.request_timeout_ms)));
        let escalating =
            EscalatingBackend::new(Box::new(primary), Box::new(second), cfg.escalate_below);
        Watcher {
            inner: Box::new(escalating),
            log: Arc::new(Mutex::new(Vec::new())),
            refused: Arc::new(Mutex::new(BTreeMap::new())),
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
        let answers = match self.inner.ask(state, questions, deadline) {
            Ok(answers) => answers,
            Err(refusal) => {
                *self
                    .refused
                    .lock()
                    .expect("the watcher's refusals")
                    .entry(refusal.to_string())
                    .or_default() += 1;
                return Err(refusal);
            }
        };

        let mut presence = BTreeMap::new();
        for gate in UTTERANCE_GATES {
            let id = presence_gate(gate);
            if let Some(Answer::Noul(value)) = answers.per_gate.get(&id) {
                presence.insert(id, *value);
            }
        }

        let mut confidence = BTreeMap::new();
        let mut min_confidence = f32::INFINITY;
        for (id, answer) in &answers.per_gate {
            let value = match answer {
                Answer::Choice { confidence, .. } | Answer::Score { confidence, .. } => *confidence,
                // A noul has no confidence: its value is the belief itself.
                Answer::Noul(_) => continue,
            };
            confidence.insert(id.clone(), value);
            min_confidence = min_confidence.min(value);
        }
        if !min_confidence.is_finite() {
            min_confidence = 0.0;
        }

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
            min_confidence,
            latency_ms: answers.latency_ms,
            escalations: answers.escalated.values().cloned().collect(),
        });

        Ok(answers)
    }

    fn name(&self) -> &str {
        self.inner.name()
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
    /// The minds in the round.
    agents: u64,
}

/// Play one round on the Thor, at the round's own rate, and measure it.
fn play(seed: u64) -> (Round, Vec<Ask>, BTreeMap<String, u64>) {
    let header = Header::default_round(seed, DEFAULT_REFLEX);
    let harness = default_harness();
    let mut cfg = AgentsConfig::default();
    // The box to measure. The default is the Thor's own d1-2B; the
    // environment points the round at another server of the same shape (the
    // batched one), so the two can be measured on the same packs.
    if let Ok(base) = std::env::var("LM_THOR_BASE") {
        cfg.thor_base = base;
    }
    if let Ok(port) = std::env::var("LM_THOR_PORT") {
        cfg.ports.insert(
            "d1-2b".to_owned(),
            port.parse().expect("the port is a number"),
        );
    }

    let watcher = Watcher::new(&cfg);
    let log = watcher.log();
    let refused = watcher.refused();
    let backends = BTreeMap::from([(
        "thor-d1-2b".to_owned(),
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
        decisions: stats.decisions,
        refusals: stats.refusals,
        thinks: stats.thinks,
        answered: stats.answered,
        expired: stats.expired,
        escalations: stats.escalations,
        agents: header.roster.len() as u64,
    };
    let log = log.lock().expect("the watcher's log").clone();
    let refused = refused.lock().expect("the watcher's refusals").clone();
    (round, log, refused)
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

/// The presence gate that blocked the line, if one did.
///
/// The gates are read in the order `plan_from_gates` reads them: the first
/// presence gate below the floor decides, and the rest are not read.
fn blocker(ask: &Ask) -> Option<String> {
    UTTERANCE_GATES
        .iter()
        .map(|gate| presence_gate(gate))
        .find(|id| {
            ask.presence
                .get(id)
                .is_some_and(|value| *value < PRESENCE_FLOOR)
        })
}

/// The measurement: three rounds on the Thor, and what the gates did.
#[test]
#[ignore = "needs the Thor at 192.168.0.78; run with --release --ignored --nocapture"]
fn talk_on_thor() {
    let mut rounds = Vec::new();
    let mut asks = Vec::new();
    let mut thor_refusals: BTreeMap<String, u64> = BTreeMap::new();
    for seed in seeds() {
        let (round, log, refused) = play(seed);
        println!(
            "round {seed}: {} utterances, {} decisions, {} refusals, {} asks sent, \
             {} answered, {} expired, {} escalations, {} answers seen",
            round.utterances,
            round.decisions,
            round.refusals,
            round.thinks,
            round.answered,
            round.expired,
            round.escalations,
            log.len()
        );
        for (name, count) in refused {
            *thor_refusals.entry(name).or_default() += count;
        }
        rounds.push(round);
        asks.extend(log);
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

    let utterances: u64 = rounds.iter().map(|round| round.utterances).sum();
    let decisions: u64 = rounds.iter().map(|round| round.decisions).sum();
    let refusals: u64 = rounds.iter().map(|round| round.refusals).sum();
    let thinks: u64 = rounds.iter().map(|round| round.thinks).sum();
    let escalations: u64 = rounds.iter().map(|round| round.escalations).sum();

    // The situations are many, the answers are not: the Thor answers the same
    // pack over the same facts the same way. The distribution is therefore
    // over the situations, and every distinct situation counts once.
    let mut distinct: BTreeMap<String, &Ask> = BTreeMap::new();
    for ask in &asks {
        distinct.entry(ask.state.clone()).or_insert(ask);
    }
    let situations: Vec<&Ask> = distinct.into_values().collect();

    println!();
    println!("# the talk gates on the real Thor");
    println!();
    println!("- rounds: {}", rounds.len());
    println!("- asks answered: {}", asks.len());
    println!("- distinct situations asked: {}", situations.len());
    println!("- utterances: {utterances}, decisions: {decisions}, refusals: {refusals}");
    println!();

    // The packs (D70): how much of the thinking each pack carries, and how
    // fast it came back.
    let move_asks: Vec<&Ask> = asks.iter().filter(|ask| ask.pack == Pack::Move).collect();
    let talk_asks: Vec<&Ask> = asks.iter().filter(|ask| ask.pack == Pack::Talk).collect();
    println!("## the two packs (D70)");
    println!();
    println!("- move pack asks: {}", move_asks.len());
    println!("- talk pack asks: {}", talk_asks.len());
    // The think rate per agent: the questions asked over the round's
    // seconds, per mind.
    let agents = rounds.first().map_or(0, |round| round.agents);
    let seconds = TICKS as f64 / f64::from(30);
    let rounds_n = rounds.len().max(1) as f64;
    println!(
        "- thoughts per second per agent: {:.2} ({} minds, {:.0} s)",
        thinks as f64 / (rounds_n * agents as f64 * seconds),
        agents,
        seconds
    );
    println!(
        "- decisions per second per agent: {:.2}",
        decisions as f64 / (rounds_n * agents as f64 * seconds)
    );
    println!();

    // The round trip per pack.
    let mut move_latencies: Vec<u32> = move_asks.iter().map(|ask| ask.latency_ms).collect();
    move_latencies.sort_unstable();
    let mut talk_latencies: Vec<u32> = talk_asks.iter().map(|ask| ask.latency_ms).collect();
    talk_latencies.sort_unstable();
    println!("## the round trip per pack");
    println!();
    if !move_latencies.is_empty() {
        println!(
            "- the move pack: min {} ms, p50 {} ms, p95 {} ms, max {} ms over {} answers",
            move_latencies[0],
            percentile_ms(&move_latencies, 0.50),
            percentile_ms(&move_latencies, 0.95),
            move_latencies[move_latencies.len() - 1],
            move_latencies.len()
        );
    }
    if !talk_latencies.is_empty() {
        println!(
            "- the talk pack: min {} ms, p50 {} ms, p95 {} ms, max {} ms over {} answers",
            talk_latencies[0],
            percentile_ms(&talk_latencies, 0.50),
            percentile_ms(&talk_latencies, 0.95),
            talk_latencies[talk_latencies.len() - 1],
            talk_latencies.len()
        );
    }
    println!();

    // The escalations (D71).
    let mut escalated: Vec<&Escalation> = Vec::new();
    for ask in &asks {
        for escalation in &ask.escalations {
            escalated.push(escalation);
        }
    }
    let mut escalation_latencies: Vec<u32> = escalated.iter().map(|e| e.latency_ms).collect();
    escalation_latencies.sort_unstable();
    let mut gates_asked = 0u64;
    for ask in &asks {
        gates_asked += ask.confidence.len() as u64;
    }
    println!("## the escalation (D71)");
    println!();
    println!("- escalations: {}", escalated.len());
    println!("- gates answered: {gates_asked}");
    if !escalated.is_empty() {
        println!(
            "- the rate per gate: {:.1}%",
            100.0 * escalated.len() as f64 / gates_asked as f64
        );
        println!(
            "- the rate per ask: {:.1}%",
            100.0 * escalated.len() as f64 / asks.len() as f64
        );
        println!(
            "- the rate per decision: {:.1}% (D71 measured 4% of the decisions)",
            100.0 * escalated.len() as f64 / decisions.max(1) as f64
        );
        println!(
            "- the added latency: min {} ms, p50 {} ms, p95 {} ms, max {} ms",
            escalation_latencies[0],
            percentile_ms(&escalation_latencies, 0.50),
            percentile_ms(&escalation_latencies, 0.95),
            escalation_latencies[escalation_latencies.len() - 1]
        );
        let mut by_gate: BTreeMap<String, u64> = BTreeMap::new();
        let mut outcomes: BTreeMap<String, u64> = BTreeMap::new();
        let mut primary_confidences: Vec<f32> = Vec::new();
        let mut second_confidences: Vec<f32> = Vec::new();
        for escalation in &escalated {
            *by_gate.entry(escalation.gate.clone()).or_default() += 1;
            *outcomes.entry(escalation.outcome.to_string()).or_default() += 1;
            primary_confidences.push(escalation.primary_confidence);
            if let Some(confidence) = escalation.second_confidence {
                second_confidences.push(confidence);
            }
        }
        primary_confidences.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        second_confidences.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        println!("- by gate:");
        for (gate, count) in &by_gate {
            println!("  - {gate}: {count}");
        }
        println!("- by outcome:");
        for (outcome, count) in &outcomes {
            println!("  - {outcome}: {count}");
        }
        println!(
            "- the first model's confidence: min {:.4}, p50 {:.4}, max {:.4}",
            primary_confidences[0],
            percentile(&primary_confidences, 0.50),
            primary_confidences[primary_confidences.len() - 1]
        );
        if !second_confidences.is_empty() {
            println!(
                "- the second model's confidence: min {:.4}, p50 {:.4}, max {:.4}",
                second_confidences[0],
                percentile(&second_confidences, 0.50),
                second_confidences[second_confidences.len() - 1]
            );
        }
    }
    println!();

    // The presence nouls, per gate.
    println!("## the presence nouls");
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
        let below = values.iter().filter(|v| **v < PRESENCE_FLOOR).count();
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

    // Which gate blocked the line.
    let mut blockers: BTreeMap<String, u64> = BTreeMap::new();
    for ask in &situations {
        let name = blocker(ask).unwrap_or_else(|| "nothing".to_owned());
        *blockers.entry(name).or_default() += 1;
    }
    println!("## what blocked the line");
    println!();
    println!("| blocker | situations |");
    println!("|---|---|");
    for (name, count) in &blockers {
        println!("| {name} | {count} |");
    }
    println!();

    // The confidences, per gate, and the pack-level fold.
    println!("## the confidences");
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

    let mut folds: Vec<f32> = situations.iter().map(|ask| ask.min_confidence).collect();
    folds.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    println!("## the pack-level fold (the lowest confidence in the pack)");
    println!();
    println!(
        "- min {:.4}, p10 {:.4}, p50 {:.4}, p90 {:.4}, max {:.4}",
        folds[0],
        percentile(&folds, 0.10),
        percentile(&folds, 0.50),
        percentile(&folds, 0.90),
        folds[folds.len() - 1]
    );
    for floor in [0.15_f32, 0.30, 0.55, 0.60] {
        let below = folds.iter().filter(|value| **value < floor).count();
        println!(
            "- the old pack-level rule at a floor of {floor:.2} would refuse {below}/{} situations ({:.1}%)",
            folds.len(),
            100.0 * below as f64 / folds.len() as f64
        );
    }
    println!();

    // The rule that is shipped now: per gate. A gate below the floor makes
    // no decision of its own, so a talk gate's uncertainty silences the
    // line and a move gate's uncertainty refuses the move.
    println!("## the per-gate rule");
    println!();
    for floor in [0.15_f32, 0.30] {
        let mut dropped: BTreeMap<String, u64> = BTreeMap::new();
        let mut silent = 0u64;
        let mut moved = 0u64;
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
                *dropped.entry(gate.clone()).or_default() += 1;
            }
            if unsure_talk {
                silent += 1;
            }
            if unsure_move {
                moved += 1;
            }
        }
        println!("- at a floor of {floor:.2}:");
        for (gate, count) in &dropped {
            println!("  - the gate {gate} would be dropped in {count} situations");
        }
        println!("  - {silent} situations would be silent and {moved} would lose the move");
    }
    println!();

    // The lines the gates allowed, against the lines the round spoke.
    let allowed = situations
        .iter()
        .filter(|ask| {
            blocker(ask).is_none()
                && ask
                    .confidence
                    .iter()
                    .all(|(_, confidence)| *confidence >= SOAK_FLOOR)
        })
        .count();
    println!("## the lines");
    println!();
    println!(
        "- the gates allow {allowed}/{} distinct situations to speak",
        situations.len()
    );
    println!("- the rounds spoke {utterances} lines and made {decisions} decisions");
    println!();

    // The round trip the Thor reported, and what it refused.
    let mut latencies: Vec<u32> = asks.iter().map(|ask| ask.latency_ms).collect();
    latencies.sort_unstable();
    println!("## the round trip");
    println!();
    if latencies.is_empty() {
        println!("- no answer came back");
    } else {
        println!(
            "- min {} ms, p50 {} ms, p90 {} ms, max {} ms over {} answers",
            latencies[0],
            latencies[latencies.len() / 2],
            latencies[latencies.len() * 9 / 10],
            latencies[latencies.len() - 1],
            latencies.len()
        );
    }
    println!("- the Thor refused:");
    for (name, count) in &thor_refusals {
        println!("  - {name}: {count}");
    }
    println!();

    // The round's own numbers.
    println!("## the rounds");
    println!();
    println!("- the rounds asked {thinks} questions, {escalations} of them paid for an escalation");
}
