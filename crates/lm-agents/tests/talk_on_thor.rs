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
use lm_agents::utter::{presence_gate, PRESENCE_FLOOR, UTTERANCE_GATES};
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

/// The confidence floor the soak pins on the Thor's d1-2B.
const SOAK_FLOOR: f32 = 0.15;

/// One ask that came back, and what the gates made of it.
#[derive(Clone, Debug)]
struct Ask {
    /// Which pack the ask carried (D70).
    pack: Pack,
    /// The situation the pack was asked over: the facts text.
    state: String,
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
    /// The wire: the time to open a connection to the Thor, in
    /// milliseconds, measured while the pack was in flight (N1).
    network_ms: u32,
    /// The refusal the call came back with, if it came back with one.
    refusal: Option<String>,
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
            addr: format!("{}:{}", host_of(&cfg.thor_base), port),
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
                        presence: BTreeMap::new(),
                        confidence: BTreeMap::new(),
                        min_confidence: 0.0,
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

        self.log.lock().expect("the watcher's log").push(Ask {
            pack,
            state: state.to_owned(),
            presence,
            confidence,
            min_confidence,
            latency_ms: answers.latency_ms,
            escalations: answers.escalated.values().cloned().collect(),
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
fn play(seed: u64) -> (Round, Vec<Ask>, BTreeMap<String, u64>, Vec<AskTiming>) {
    let header = Header::default_round(seed, DEFAULT_REFLEX);
    let harness = default_harness();
    let cfg = AgentsConfig::default();

    let watcher = Watcher::new(&cfg);
    let log = watcher.log();
    let refused = watcher.refused();
    let backends = BTreeMap::from([(
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
    let mut timings: Vec<AskTiming> = Vec::new();
    let mut thor_refusals: BTreeMap<String, u64> = BTreeMap::new();
    for seed in SEEDS {
        let (round, log, refused, seed_timings) = play(seed);
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
        timings.extend(seed_timings);
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
    let move_split = split_of(Pack::Move, WATCHED_BACKEND, &timings, &asks, tick_hz);
    let talk_split = split_of(Pack::Talk, WATCHED_BACKEND, &timings, &asks, tick_hz);
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
    let elsewhere = timings
        .iter()
        .filter(|t| t.backend != WATCHED_BACKEND)
        .count();
    println!(
        "- the split covers {} move pack thinks and {} talk pack thinks, over {} watched asks and {} timings",
        move_split.total.len(),
        talk_split.total.len(),
        asks.len(),
        timings.len()
    );
    println!(
        "- {} asks went to another backend (the shipped policy's deep tier names the cloud) \
         and are not in the split: they never reached the Thor",
        elsewhere
    );
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
