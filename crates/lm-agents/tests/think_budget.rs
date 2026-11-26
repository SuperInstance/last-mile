//! The think budget, measured in our own code (X5 follow-up).
//!
//! decision-daw x5 measured the Thor: d1-2B serves about nine requests a
//! second, serialized (median 141 ms alone, about 755 ms with sixteen in
//! flight). The shipped defaults ask for far more — `think_hz_member = 4`
//! and `think_hz_leader = 1` — so eight agents ask for about thirty
//! requests a second. This sweep plays whole rounds against a server with
//! that capacity and says what the agents actually do under it.
//!
//! ## What is played
//!
//! The rounds are played through the host's own path: the [`HostLoop`], the
//! [`Agents`], the least-use-first queue, the sequencer and the recorder.
//! Nothing here re-implements the cognition loop; the only thing that is
//! not the shipped code is the model behind the tiers, which is a
//! [`ThrottledStub`]: a server that serves one request at a time, 110 ms
//! each, so it answers about nine requests a second. Every tier the policy
//! names is served by that one server, so the round's whole demand meets one
//! capacity — the ledger the brief asks for.
//!
//! ## The clock
//!
//! The sim runs at `Params::tick_hz` and the host keeps that rate against
//! the wall clock, so one tick is worth one tick of the model server's
//! time. A round of 3,000 ticks is a hundred seconds of play, and an answer
//! that took four slots to be served arrives four slots late: the model's
//! latency has to be read in the sim's own ticks, or it is not measured at
//! all.
//!
//! ## The command
//!
//! ```text
//! cargo test --release -p lm-agents --test think_budget -- --ignored --nocapture
//! ```

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use lm_agents::config::AgentsConfig;
use lm_agents::harness::Harness;
use lm_agents::mind::Policy;
use lm_agents::sysone::{SysOne, ThrottledStub, THROTTLED_SERVICE_MS};
use lm_agents::Agents;
use lm_core::decision::Source;
use lm_core::header::Header;
use lm_core::log::RoundLog;
use lm_host_model::HostLoop;
use lm_learn::metrics::RoundMetrics;

/// The shipped harness: the gates, the policy and the phrasebook the default
/// round thinks with.
const DEFAULT_GATES: &str = include_str!("../../../assets/harness/default/gates.toml");
const DEFAULT_POLICY: &str = include_str!("../../../assets/harness/default/policy.toml");
const DEFAULT_PHRASEBOOK: &str = include_str!("../../../assets/harness/default/phrasebook.toml");

/// The shipped reflex, compiled into the test binary.
const DEFAULT_REFLEX: &str = include_str!("../../../assets/reflexes/default.rhai");

/// The cells of the sweep: the think rate of a member, in Hz, and the
/// request timeout, in milliseconds.
const CELLS: [(u16, u32); 6] = [
    (4, 400),
    (4, 1_200),
    (2, 400),
    (2, 1_200),
    (1, 400),
    (1, 1_200),
];

/// The seeds every cell is played over.
const SEEDS: [u64; 10] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

/// The ticks a round is played for: a hundred seconds at 30 Hz.
const TICKS: u64 = 3_000;

/// The ticks a round may play on to let the answers still in flight land.
const DRAIN_TICKS: u64 = 60;

/// The shipped harness, read into memory.
fn default_harness() -> Harness {
    Harness {
        dir: PathBuf::from("assets/harness/default"),
        gates_toml: DEFAULT_GATES.to_owned(),
        policy_toml: DEFAULT_POLICY.to_owned(),
        phrasebook_toml: DEFAULT_PHRASEBOOK.to_owned(),
        facts_toml: String::new(),
        reflexes: BTreeMap::new(),
        commit: "content:test-harness".to_owned(),
    }
}

/// One server behind every tier the policy names.
///
/// The tiers stay the ones the game would use — the same gates, the same
/// policy, the same ladder — and only the model behind them changes: one
/// server with a capacity, so the round's whole demand meets the same
/// nine requests a second.
fn throttled_backends(
    harness: &Harness,
    server: &ThrottledStub,
) -> BTreeMap<String, Box<dyn SysOne + Send + Sync>> {
    let policy = Policy::for_harness(&harness.policy_toml);
    policy
        .tiers
        .tiers()
        .iter()
        .filter(|tier| !tier.is_reflex())
        .map(|tier| {
            (
                tier.backend().to_owned(),
                Box::new(server.clone()) as Box<dyn SysOne + Send + Sync>,
            )
        })
        .collect()
}

/// What one round did, measured.
#[derive(Clone, Debug, Default)]
struct Round {
    /// The ticks the round played.
    ticks: u64,
    /// The agent decisions that were applied.
    applied: u64,
    /// The sequencer's own counts.
    on_time: u64,
    late: u64,
    stale: u64,
    /// The requests the server refused as too late.
    timeouts: u64,
    /// The requests the round made.
    asks: u64,
    /// The dial's model-driven ticks.
    model_ticks: u64,
    /// The dial's reflex-driven ticks.
    reflex_ticks: u64,
    /// The final score of every squad.
    scores: Vec<i64>,
}

/// Play one round and measure it.
///
/// The round is played at the sim's own rate against the wall clock, and the
/// client hands the server every request the round made: the server's queue
/// is then the queue the agents really waited in, and the wait counts
/// toward the latency the answers report.
fn play_round(seed: u64, cfg: &AgentsConfig) -> Round {
    let header = Header::default_round(seed, DEFAULT_REFLEX);
    let harness = default_harness();
    let server = ThrottledStub::with_times(
        Duration::from_millis(THROTTLED_SERVICE_MS),
        Duration::from_millis(u64::from(cfg.request_timeout_ms)),
    );
    let agents = Agents::new(
        cfg,
        &harness,
        &header,
        throttled_backends(&harness, &server),
    );

    let mut host = HostLoop::new(header).expect("the round starts");
    host.attach_agents(agents);

    let tick_hz = u64::from(host.header().params.tick_hz);
    let period = Duration::from_nanos(1_000_000_000 / tick_hz);
    let start = Instant::now();
    let mut sent = 0u64;
    let mut played = 0u64;

    for tick in 0..TICKS {
        wait_for(start, period, tick);
        host.tick();
        played += 1;
        hand_over(&server, &host, &mut sent);
        if host.sim().outcome().is_some() {
            break;
        }
    }
    // The answers still in the air land on the ticks after the round's: the
    // host never waits for a model, so the round is measured after they
    // have landed and the host has picked them up.
    for _ in 0..DRAIN_TICKS {
        let idle = host.agents_pending() == 0;
        wait_for(start, period, played);
        host.tick();
        played += 1;
        hand_over(&server, &host, &mut sent);
        if idle {
            break;
        }
    }

    let log = host.log_so_far();
    let stats = host.sched_stats();
    let dial = RoundMetrics::from_log(&log).dial;
    let mut round = Round {
        ticks: played,
        applied: agent_decisions(&log),
        on_time: stats.on_time,
        late: stats.late,
        stale: stats.stale,
        timeouts: server.timeouts(),
        asks: host.agent_stats().map_or(0, |stats| stats.thinks),
        model_ticks: dial.values().map(|dial| dial.model_ticks).sum(),
        reflex_ticks: dial.values().map(|dial| dial.reflex_ticks).sum(),
        scores: host.sim().scores().to_vec(),
    };
    assert_eq!(
        server.unpaired(),
        0,
        "every request the round made reached the server"
    );
    assert!(round.ticks > 0, "the round played ticks");
    round.scores.sort_unstable();
    round
}

/// Wait until the tick is due.
///
/// The host keeps the sim's rate against the wall clock, and the leftover is
/// carried, so no tick is skipped and no tick is played twice.
fn wait_for(start: Instant, period: Duration, tick: u64) {
    let ticks = u32::try_from(tick).unwrap_or(u32::MAX);
    let due = start + period * ticks;
    let now = Instant::now();
    if due > now {
        std::thread::sleep(due - now);
    }
}

/// The client hands the server every request the round made.
///
/// The count is the agents' own counter of asks sent, so the server's queue
/// is the queue the round really waited in.
fn hand_over(server: &ThrottledStub, host: &HostLoop, sent: &mut u64) {
    let now = host.agent_stats().map_or(0, |stats| stats.thinks);
    for _ in *sent..now {
        server.arrive();
    }
    *sent = now;
}

/// The agent decisions that were applied, as the log recorded them.
fn agent_decisions(log: &RoundLog) -> u64 {
    log.segments
        .iter()
        .flat_map(|segment| segment.decisions.iter())
        .filter(|stamped| matches!(stamped.d.source, Source::Agent(_)))
        .count() as u64
}

/// The six numbers of one cell, over its seeds.
#[derive(Clone, Copy, Debug, Default)]
struct Cell {
    /// The ticks the seeds played, over all of them.
    ticks: u64,
    /// The agent decisions that were applied, over all the seeds.
    applied: u64,
    /// The sequencer's own counts, over all the seeds.
    on_time: u64,
    late: u64,
    stale: u64,
    /// The requests the server refused as too late, over all the seeds.
    timeouts: u64,
    /// The requests the rounds made, over all the seeds.
    asks: u64,
    /// The dial's ticks, over all the seeds.
    model_ticks: u64,
    reflex_ticks: u64,
    /// The final scores, over all the seeds and squads.
    score_sum: i64,
    score_count: u64,
}

impl Cell {
    /// Fold a round into the cell.
    fn add(&mut self, round: &Round) {
        self.ticks += round.ticks;
        self.applied += round.applied;
        self.on_time += round.on_time;
        self.late += round.late;
        self.stale += round.stale;
        self.timeouts += round.timeouts;
        self.asks += round.asks;
        self.model_ticks += round.model_ticks;
        self.reflex_ticks += round.reflex_ticks;
        for score in &round.scores {
            self.score_sum += *score;
            self.score_count += 1;
        }
    }

    /// The decisions applied per agent per second.
    ///
    /// The round's play is what the sim's rate makes it: the ticks the round
    /// played, over the sim's rate, is the time the round took.
    fn per_agent_per_second(&self, agents: u64, tick_hz: u64) -> f64 {
        let seconds = self.ticks as f64 / tick_hz as f64;
        if seconds <= 0.0 {
            return 0.0;
        }
        self.applied as f64 / (seconds * agents as f64)
    }

    /// The share of the answers that were on time, late or stale.
    fn shares(&self) -> (f64, f64, f64) {
        let total = (self.on_time + self.late + self.stale) as f64;
        if total <= 0.0 {
            return (0.0, 0.0, 0.0);
        }
        (
            self.on_time as f64 / total,
            self.late as f64 / total,
            self.stale as f64 / total,
        )
    }

    /// The reflex share of the dial, in per-mille.
    fn reflex_permille(&self) -> u64 {
        let total = self.model_ticks + self.reflex_ticks;
        if total == 0 {
            return 0;
        }
        self.reflex_ticks * 1_000 / total
    }

    /// The mean final score.
    fn mean_score(&self) -> f64 {
        if self.score_count == 0 {
            return 0.0;
        }
        self.score_sum as f64 / self.score_count as f64
    }

    /// The requests the rounds made per second: the demand the server met.
    fn asks_per_second(&self, tick_hz: u64) -> f64 {
        let seconds = self.ticks as f64 / tick_hz as f64;
        if seconds <= 0.0 {
            return 0.0;
        }
        self.asks as f64 / seconds
    }
}

/// Play every seed of a cell, each on its own thread.
///
/// Each round has its own server, so the rounds do not share a capacity and
/// do not wait on each other: the seeds are the sample, and the sample is
/// taken at the same time.
fn play_cell(think_hz_member: u16, request_timeout_ms: u32) -> Cell {
    let handles: Vec<_> = SEEDS
        .iter()
        .map(|seed| {
            std::thread::spawn(move || {
                let cfg = AgentsConfig {
                    think_hz_member,
                    request_timeout_ms,
                    ..AgentsConfig::default()
                };
                play_round(*seed, &cfg)
            })
        })
        .collect();

    let mut cell = Cell::default();
    for handle in handles {
        let round = handle.join().expect("the round finishes");
        cell.add(&round);
    }
    cell
}

/// The think budget, swept.
///
/// The test is `#[ignore]`d because it is a measurement and not a gate: it
/// plays sixty rounds of a hundred seconds each.
#[test]
#[ignore = "a measurement, not a gate: sixty rounds of a hundred seconds"]
fn think_budget_sweep() {
    let tick_hz = u64::from(Header::default_round(1, DEFAULT_REFLEX).params.tick_hz);
    let agents = u64::try_from(Header::default_round(1, DEFAULT_REFLEX).roster.len()).unwrap_or(0);

    println!();
    println!("# the think budget, measured");
    println!();
    println!(
        "command: cargo test --release -p lm-agents --test think_budget -- --ignored --nocapture"
    );
    println!();
    println!(
        "| think_hz_member | request_timeout_ms | asks/s | decisions/agent/s | on time | late | \
         stale | timeouts | reflex share | mean score |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|");

    for (think_hz_member, request_timeout_ms) in CELLS {
        let cell = play_cell(think_hz_member, request_timeout_ms);
        let (on_time, late, stale) = cell.shares();
        println!(
            "| {} | {} | {:.2} | {:.3} | {:.1}% | {:.1}% | {:.1}% | {} | {:.1}% | {:.1} |",
            think_hz_member,
            request_timeout_ms,
            cell.asks_per_second(tick_hz),
            cell.per_agent_per_second(agents, tick_hz),
            on_time * 100.0,
            late * 100.0,
            stale * 100.0,
            cell.timeouts,
            cell.reflex_permille() as f64 / 10.0,
            cell.mean_score(),
        );
    }
    println!();
}
