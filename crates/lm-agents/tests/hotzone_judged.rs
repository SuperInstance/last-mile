//! The hot zone, judged: the `shared-ghost` variant against the shipped
//! doctrine, on the Thor's real decision model, under the rule D78 fixes.
//!
//! ## What this is
//!
//! `engagement.md` measured the hot zone's *promise* — that both squads are
//! pulled onto the same beacon and therefore meet — on three seeds at 3000
//! ticks, and the write-up was careful about what it could not say: three
//! rounds are not a verdict, and a round that ends in a draw is not a game
//! that was won.
//!
//! This is the measurement that turns the promise into a verdict:
//!
//! - **16 held-out seeds**, the seed farm's own block (`1_000_000..`), the
//!   count a proposal is judged on;
//! - **1500 ticks** a round, D78's horizon: the ticks at which the
//!   claimable metrics live;
//! - the **same seed in both arms**, so the only thing that differs between
//!   the two rounds of a seed is the doctrine;
//! - the **exact sign test** over the per-seed deltas, and the count of the
//!   seeds that moved the claimed way;
//! - the **asks and the refusals** of both arms, because a doctrine that
//!   wins by spending the whole ask budget has not won.
//!
//! ## The rule, written down before the run
//!
//! **What is measured, per round:**
//!
//! - `tags`: the `Tagged` events the ruleset made;
//! - `sight_ticks`: the ticks on which some agent had a fresh enemy within
//!   reach and a clear line of sight to it;
//! - `scores`: the squads' points when the round ended;
//! - `asks` / `refusals`: the questions the round sent to the models, and
//!   the answers that never came, by kind.
//!
//! **The score — the primary metric — of a round:**
//!
//! ```text
//! score = 100 * tags + sight_ticks + 1000 * (min(score0, score1) >= 200)
//! ```
//!
//! - the tags are the game's atomic fight event, weighted so that one
//!   fight is worth a hundred ticks of looking;
//! - the sight ticks are the sustained engagement;
//! - the last term is the round's *both squads in the game* test: the same
//!   200-point collapse rule the headless sweep uses. A round where one
//!   squad was held off the board scores nothing for it, so a win bought
//!   with a rout cannot buy the verdict.
//!
//! **"Better", per seed:** the hot zone's score is strictly greater than the
//! default's. The bar is therefore one point: any rise counts.
//!
//! **The verdict (D78):** the hot zone qualifies only when
//!
//! 1. it is better on at least `ceil(3N/4)` = 12 of the 16 seeds, **and**
//! 2. the exact sign test over the 16 deltas (ties excluded) gives
//!    `p < 0.05`.
//!
//! The one-sided sign test is D78's own. The two-sided one is reported
//! beside it, and it is the stricter of the two: at 12 of 16 seeds the
//! one-sided p-value is 0.0384 and the two-sided is 0.0768, so the
//! two-sided test needs 13 of 16 to pass where the one-sided needs 12.
//!
//! **What cannot be measured at 1500 ticks:** a round cannot be decided
//! before tick 1800 — a squad scores at most one point a tick and the
//! round's win score is 1800 — so every round here is a draw, and the win
//! share cannot be read from this run at all. The draws, the win share and
//! the fairness of the hot zone are read from the headless sweep at the
//! round's own limit instead.
//!
//! **And what cannot be measured at 1500 ticks is the hot zone itself:**
//! the first enemy in sight lands between tick 1740 and 2237, so at D78's
//! horizon the squads have not met in any round at all — 0 tags and 0 sight
//! ticks in all 32 rounds of the 1500-tick run. The same 16 seeds at 3000
//! ticks (`HOTZONE_TICKS=3000`) are where the fight shows up: the hot zone
//! makes 78 tags against the default's 5, and leaves a squad on zero points
//! in 8 rounds of 16. The write-up is in
//! `crates/lm-agents/tests/engagement.md`, under "The hot zone, judged".
//!
//! ## The command
//!
//! ```text
//! cargo test --release -p lm-agents --test hotzone_judged -- --ignored --nocapture
//! ```
//!
//! `HOTZONE_PORT` names the decision server (8104, the batched one, when it
//! is up; 8094, the d1-2B, otherwise), `HOTZONE_TICKS` the horizon,
//! `HOTZONE_SEEDS` the seed count and `HOTZONE_WORKERS` how many rounds are
//! played at once.

use std::collections::BTreeMap;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lm_agents::config::AgentsConfig;
use lm_agents::harness::Harness;
use lm_agents::sysone::{Answers, EscalatingBackend, Refusal, SysOne, ThorBackend};
use lm_agents::Agents;
use lm_core::header::Header;
use lm_core::view::ScoreView;
use lm_core::world::los;
use lm_host_model::HostLoop;
use lm_learn::eval::eval_seeds;
use lm_learn::ledger::Direction;
use lm_learn::sign::{passes, sign_test, two_sided_passes};
use serde_json::{json, Value};

/// The shipped reflex, compiled into the test binary.
const DEFAULT_REFLEX: &str = include_str!("../../../assets/reflexes/default.rhai");

/// The ticks a round is played for: D78's horizon.
const TICKS: u64 = 1_500;

/// The seeds the verdict is judged on: the seed farm's count.
const SEED_COUNT: usize = 16;

/// The points a squad has to score to count as in the game.
///
/// The same 200 the headless sweep uses for a collapsed round.
const COLLAPSE_POINTS: i64 = 200;

/// The enemy's own reach, in cells: the facts builder's ENEMY_NEAR_CELLS.
const ENEMY_NEAR_CELLS: i32 = 6;

/// The two arms: the shipped doctrine, and the hot zone.
const ARMS: [(&str, &str); 2] = [
    ("default", "assets/harness/default"),
    ("shared-ghost", "assets/harness/variants/shared-ghost"),
];

/// The root of the repository, where the assets live.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The ticks a round is played for, if the caller says otherwise.
fn ticks() -> u64 {
    std::env::var("HOTZONE_TICKS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(TICKS)
}

/// The seeds the verdict is judged on.
///
/// The seed farm's own block: `eval_seeds(&[], 16, 0)` is
/// `1_000_000..1_000_015`, which is held out from every round the seed farm
/// has ever trained on.
fn seeds() -> Vec<u64> {
    match std::env::var("HOTZONE_SEEDS") {
        Ok(value) => value
            .split(',')
            .filter_map(|seed| seed.trim().parse::<u64>().ok())
            .collect(),
        Err(_) => eval_seeds(&[], SEED_COUNT, 0),
    }
}

/// How many rounds are played at once.
///
/// One, by default: the decision server is the scarce thing here, and a
/// round asks it eight times a second. Two rounds at once ask it sixteen
/// times a second, which it cannot serve — the queue then refuses, and a
/// refused answer plays the reflex, so the run would measure the server's
/// queue instead of the doctrine. The refusals of every round are reported
/// so a starved run is visible.
fn workers() -> usize {
    std::env::var("HOTZONE_WORKERS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|count| *count > 0)
        .unwrap_or(1)
}

/// Is something listening on this port?
fn port_is_open(host: &str, port: u16) -> bool {
    let addrs: Vec<SocketAddr> = match (host, port).to_socket_addrs() {
        Ok(iter) => iter.collect(),
        Err(_) => return false,
    };
    addrs
        .iter()
        .any(|addr| TcpStream::connect_timeout(addr, Duration::from_millis(750)).is_ok())
}

/// The decision server the run plays against.
///
/// The batched server (8104) first, because the brief prefers it and it
/// serves a round's asks better than the single-worker one; the d1-2B
/// (8094) when the batched one is not up. The two answer the same packs
/// the same way (`crates/lm-agents/tests/batched_sysone.md`), so the choice
/// does not change what the doctrine is measured against.
fn decision_port(base: &str) -> u16 {
    if let Ok(value) = std::env::var("HOTZONE_PORT") {
        if let Ok(port) = value.parse::<u16>() {
            return port;
        }
    }
    let host = base.trim_start_matches("http://");
    let host = host.split(':').next().unwrap_or(host);
    if port_is_open(host, 8104) {
        8104
    } else {
        8094
    }
}

/// One ask the round made, as the watcher saw it.
#[derive(Clone, Debug)]
struct Ask {
    /// The pack: the move gate's or the talk gate's.
    talk: bool,
    /// How long the answer took, in milliseconds.
    latency_ms: u32,
}

/// The watcher: the decision server behind a log.
struct Watcher {
    inner: Box<dyn SysOne + Send + Sync>,
    log: Arc<Mutex<Vec<Ask>>>,
}

impl Watcher {
    /// A watcher over the Thor's decision server, with the floor at zero so
    /// that every gate's answer is seen.
    fn new(cfg: &AgentsConfig, harness: &Harness, port: u16) -> Self {
        let policy = lm_agents::mind::Policy::for_harness(&harness.policy_toml);
        let _ = policy;
        let primary = ThorBackend::new(&cfg.thor_base, port, "d1-2b")
            .with_min_confidence(0.0)
            .with_timeout(Duration::from_millis(u64::from(cfg.request_timeout_ms)));
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
        }
    }

    /// The log of the asks, shared with the round that is being played.
    fn log(&self) -> Arc<Mutex<Vec<Ask>>> {
        Arc::clone(&self.log)
    }
}

impl SysOne for Watcher {
    fn ask(&self, state: &str, questions: &Value, deadline: Instant) -> Result<Answers, Refusal> {
        self.ask_with_escalation(state, questions, deadline, deadline)
    }

    fn ask_with_escalation(
        &self,
        state: &str,
        questions: &Value,
        deadline: Instant,
        escalation_deadline: Instant,
    ) -> Result<Answers, Refusal> {
        let answers =
            self.inner
                .ask_with_escalation(state, questions, deadline, escalation_deadline)?;
        let talk = questions.get("move").is_none();
        self.log.lock().expect("the watcher's log").push(Ask {
            talk,
            latency_ms: answers.latency_ms,
        });
        Ok(answers)
    }

    fn name(&self) -> &str {
        self.inner.name()
    }
}

/// What one round produced.
#[derive(Clone, Debug)]
struct Round {
    /// The arm the round was played in.
    arm: &'static str,
    /// The seed the round ran on.
    seed: u64,
    /// The ticks the round played.
    ticks: u64,
    /// The tags the ruleset made.
    tags: u64,
    /// The ticks on which some agent had an enemy in sight.
    sight_ticks: u64,
    /// The tick at which some agent first had an enemy in sight.
    ///
    /// `None` when the squads never saw each other at all: the round in
    /// which they do not is the round in which the hot zone's promise is
    /// still in the future.
    first_sight_tick: Option<u64>,
    /// The round's final scores, in squad order.
    scores: Vec<i64>,
    /// The gap between the squads' scores.
    margin: i64,
    /// Whether the round ended in a draw.
    draw: bool,
    /// The squad that won the round, when one did.
    ///
    /// `None` is a draw, and also a round that has not ended yet: at 1500
    /// ticks no round can have reached the win score, so nothing is
    /// decided and nothing can be.
    winner: Option<u8>,
    /// The tick the round ended on, when it ended.
    end_tick: Option<u64>,
    /// The decisions the agents' answers became.
    decisions: u64,
    /// The questions the round sent to the models.
    asks: u64,
    /// The answers that came home.
    answered: u64,
    /// The refusals the round saw, of every kind together.
    refusals: u64,
    /// The refusals, by kind: the kinds that happened, and how many.
    refusal_kinds: Vec<(String, u64)>,
    /// The gates that were asked of a second model.
    escalations: u64,
    /// The requests dropped because their deadline passed while queued.
    expired: u64,
    /// The model calls per agent.
    usage: Vec<(u16, u64)>,
    /// The asks that went to the move gate.
    move_asks: u64,
    /// The asks that went to the talk gate.
    talk_asks: u64,
    /// How long the answers took, in milliseconds.
    latencies: Vec<u32>,
}

impl Round {
    /// The pre-registered score of the round (see the header).
    fn score(&self) -> i64 {
        let both_in_the_game = self.scores.iter().copied().min().unwrap_or(0) >= COLLAPSE_POINTS;
        100 * self.tags as i64 + self.sight_ticks as i64 + i64::from(both_in_the_game) * 1000
    }

    /// The **secondary** score: the contact, and nothing else.
    ///
    /// This is reported beside the primary score, never instead of it. It
    /// asks one question — did the squads meet — and it does not care what
    /// they scored for it. The 200-point term of the primary score is a
    /// full-round rule: at 1500 ticks a squad that is still walking to a
    /// beacon that moves scores nothing, so the term says "in transit"
    /// where it was written to say "held off the board".
    fn contact_score(&self) -> i64 {
        100 * self.tags as i64 + self.sight_ticks as i64
    }

    /// The median answer latency, in milliseconds, and the slowest.
    fn latency(&self) -> (u64, u64) {
        if self.latencies.is_empty() {
            return (0, 0);
        }
        let mut sorted = self.latencies.clone();
        sorted.sort_unstable();
        let middle = sorted.len() / 2;
        let median = if sorted.len().is_multiple_of(2) {
            (sorted[middle - 1] as u64 + sorted[middle] as u64) / 2
        } else {
            sorted[middle] as u64
        };
        (median, *sorted.last().unwrap_or(&0) as u64)
    }
}

/// Play one round on the Thor, at the round's own rate, and measure it.
fn play(arm: &'static str, seed: u64, harness: &Harness, reflex: &str, port: u16) -> Round {
    let header = Header::default_round(seed, reflex);
    let cfg = AgentsConfig::default();

    let watcher = Watcher::new(&cfg, harness, port);
    let log = watcher.log();
    let backends = BTreeMap::from([(
        "thor-d1-2b".to_owned(),
        Box::new(watcher) as Box<dyn SysOne + Send + Sync>,
    )]);

    let mut host = HostLoop::new(header.clone()).expect("the round starts");
    host.attach_agents(Agents::new(&cfg, harness, &header, backends));

    let period = Duration::from_millis(1000 / u64::from(header.params.tick_hz));
    let mut tags = 0u64;
    let mut sight_ticks = 0u64;
    let mut first_sight_tick: Option<u64> = None;

    for _ in 0..ticks() {
        std::thread::sleep(period);

        let _tick = host.sim().tick();

        // What every agent can see on this tick.
        let mut seen_by_any = false;
        for entry in &header.roster {
            let view = host.sim().view(entry.agent);
            let (_, seeing) = threat_of(&view, host.sim().world(), cfg.stale_enemy_ticks);
            if seeing > 0 {
                seen_by_any = true;
            }
        }
        if seen_by_any {
            sight_ticks += 1;
            if first_sight_tick.is_none() {
                first_sight_tick = Some(host.sim().tick());
            }
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

    let scores = host.sim().scores().to_vec();
    let margin = match (scores.iter().min(), scores.iter().max()) {
        (Some(low), Some(high)) => high - low,
        _ => 0,
    };
    let stats = host.agent_stats();
    let outcome = host.sim().outcome();

    let asks_log = {
        let guard = log.lock().expect("the watcher's log");
        guard.clone()
    };
    let latencies: Vec<u32> = asks_log.iter().map(|ask| ask.latency_ms).collect();
    let move_asks = asks_log.iter().filter(|ask| !ask.talk).count() as u64;
    let talk_asks = asks_log.iter().filter(|ask| ask.talk).count() as u64;

    Round {
        arm,
        seed,
        ticks: host.sim().tick(),
        tags,
        sight_ticks,
        first_sight_tick,
        scores,
        margin,
        draw: outcome.is_some_and(|outcome| outcome.winner.is_none()),
        winner: outcome.and_then(|outcome| outcome.winner),
        end_tick: outcome.map(|outcome| outcome.tick),
        decisions: stats.map_or(0, |stats| stats.decisions),
        asks: stats.map_or(0, |stats| stats.thinks),
        answered: stats.map_or(0, |stats| stats.answered),
        refusals: stats.map_or(0, |stats| stats.refusals),
        refusal_kinds: stats.map_or_else(Vec::new, |stats| {
            lm_core::refusal::RefusalKind::ALL
                .iter()
                .filter_map(|kind| {
                    let count = stats.refusal_counts.get(*kind);
                    (count > 0).then(|| (kind.to_string(), count))
                })
                .collect()
        }),
        escalations: stats.map_or(0, |stats| stats.escalations),
        expired: stats.map_or(0, |stats| stats.expired),
        usage: stats.map_or_else(Vec::new, |stats| {
            stats
                .usage
                .iter()
                .map(|(agent, calls)| (*agent, *calls))
                .collect()
        }),
        move_asks,
        talk_asks,
        latencies,
    }
}

/// How many fresh enemies the agent has within reach, and how many of them
/// can see it.
fn threat_of(view: &ScoreView, world: &lm_core::world::World, stale_after: u32) -> (i64, i64) {
    let me = view.me.cell;
    let mut near = 0;
    let mut seeing = 0;
    for enemy in view.enemies.iter().filter(|e| e.age_ticks <= stale_after) {
        let dist = (i32::from(enemy.cell.x) - i32::from(me.x))
            .abs()
            .max((i32::from(enemy.cell.y) - i32::from(me.y)).abs());
        if dist > ENEMY_NEAR_CELLS {
            continue;
        }
        near += 1;
        if los::clear(&world.grid, me, enemy.cell) {
            seeing += 1;
        }
    }
    (near, seeing)
}

/// The measurement: the hot zone against the shipped doctrine, and the
/// verdict D78 asks for.
#[test]
#[ignore = "needs the Thor's decision server; run with --release --ignored --nocapture"]
fn the_hot_zone_judged() {
    let seeds = seeds();
    let cfg = AgentsConfig::default();
    let port = decision_port(&cfg.thor_base);

    println!();
    println!(
        "hot zone judged: {} seeds, {} ticks a round, decision server :{}, {} worker(s)",
        seeds.len(),
        ticks(),
        port,
        workers()
    );
    println!(
        "arms: {} (the shipped doctrine) against {} (the hot zone)",
        ARMS[0].0, ARMS[1].0
    );

    // The harnesses, opened once: each arm's own harness, with its own
    // reflex as the fallback when the model refuses.
    let mut harnesses: Vec<(&'static str, Harness, String)> = Vec::new();
    for (arm, dir) in ARMS {
        let harness = Harness::open(&repo_root().join(dir))
            .unwrap_or_else(|err| panic!("the {arm} harness opens: {err}"));
        let reflex = harness
            .reflexes
            .get("default")
            .cloned()
            .unwrap_or_else(|| DEFAULT_REFLEX.to_owned());
        harnesses.push((arm, harness, reflex));
    }

    // The rounds: every seed in both arms. The arms alternate inside a seed
    // (the seed's own index decides which goes first), so that a slow patch
    // of the server does not fall on one arm every time.
    let mut rounds: Vec<Round> = Vec::new();
    let started = Instant::now();
    let chunk = seeds.len().div_ceil(workers());
    let seeds_ref = &seeds;
    let harnesses_ref = &harnesses;
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for job in 0..workers() {
            let first = job * chunk;
            let last = ((job + 1) * chunk).min(seeds.len());
            if first >= last {
                continue;
            }
            handles.push(scope.spawn(move || {
                let mut local: Vec<Round> = Vec::new();
                for (index, seed) in seeds_ref[first..last].iter().enumerate() {
                    let order: Vec<usize> = if (first + index).is_multiple_of(2) {
                        vec![0, 1]
                    } else {
                        vec![1, 0]
                    };
                    for arm_index in order {
                        let (arm, harness, reflex) = &harnesses_ref[arm_index];
                        let round = play(arm, *seed, harness, reflex, port);
                        let (p50, p95) = round.latency();
                        println!(
                            "round {arm} seed {seed}: {} ticks, {} tags, {} sight ticks \
                             (first at {:?}), scores {:?} (margin {}), score {}, {} decisions, {} asks \
                             ({} move, {} talk; {} answered), {} refusals {:?}, \
                             {} escalations, {} expired, latency p50 {p50} ms p95 {p95} ms",
                            round.ticks,
                            round.tags,
                            round.sight_ticks,
                            round.first_sight_tick,
                            round.scores,
                            round.margin,
                            round.score(),
                            round.decisions,
                            round.asks,
                            round.move_asks,
                            round.talk_asks,
                            round.answered,
                            round.refusals,
                            round.refusal_kinds,
                            round.escalations,
                            round.expired,
                        );
                        local.push(round);
                    }
                }
                local
            }));
        }
        for handle in handles {
            rounds.extend(handle.join().expect("the round thread finishes"));
        }
    });
    rounds.sort_by_key(|round| (round.seed, round.arm));

    // The per-seed deltas, on the pre-registered score.
    let mut deltas: Vec<i64> = Vec::new();
    let mut contact_deltas: Vec<i64> = Vec::new();
    let mut better = 0usize;
    let mut worse = 0usize;
    let mut ties = 0usize;
    let mut contact_better = 0usize;
    let mut contact_worse = 0usize;
    let mut contact_ties = 0usize;
    println!();
    println!(
        "| seed | default score | hot zone score | delta | default tags | hot zone tags | \
         default sight | hot zone sight |"
    );
    println!("| --- | --- | --- | --- | --- | --- | --- | --- |");
    for seed in &seeds {
        let default = rounds
            .iter()
            .find(|round| round.seed == *seed && round.arm == ARMS[0].0);
        let hot = rounds
            .iter()
            .find(|round| round.seed == *seed && round.arm == ARMS[1].0);
        let (Some(default), Some(hot)) = (default, hot) else {
            println!("| {seed} | (missing) | | | | | | |");
            continue;
        };
        let delta = hot.score() - default.score();
        let contact_delta = hot.contact_score() - default.contact_score();
        deltas.push(delta);
        contact_deltas.push(contact_delta);
        match delta.cmp(&0) {
            std::cmp::Ordering::Greater => better += 1,
            std::cmp::Ordering::Less => worse += 1,
            std::cmp::Ordering::Equal => ties += 1,
        }
        match contact_delta.cmp(&0) {
            std::cmp::Ordering::Greater => contact_better += 1,
            std::cmp::Ordering::Less => contact_worse += 1,
            std::cmp::Ordering::Equal => contact_ties += 1,
        }
        println!(
            "| {seed} | {} | {} | {delta:+} | {} | {} | {} | {} |",
            default.score(),
            hot.score(),
            default.tags,
            hot.tags,
            default.sight_ticks,
            hot.sight_ticks
        );
    }

    // D78: the count of the seeds that moved the claimed way, and the exact
    // sign test over the deltas.
    let n = deltas.len();
    let bar = n * 3 / 4 + usize::from(!n.is_multiple_of(4)); // ceil(3N/4), in integers
    let test = sign_test(&deltas, Direction::Up);
    let two_sided = test.two_sided_p_value_text();
    let one_sided = test.p_value_text();
    let one_sided_passes = passes(test);
    let two_sided_passed = two_sided_passes(test);

    println!();
    println!("the verdict (D78), on the pre-registered score:");
    println!("  seeds where the hot zone was better: {better} of {n} (the bar is {bar})");
    println!(
        "  the sign test: {} seeds moved, {} of them the claimed way, p = {one_sided} \
         (one-sided, D78's own) -- {}",
        test.moved,
        test.claimed,
        if one_sided_passes {
            "passes"
        } else {
            "does not pass"
        }
    );
    println!(
        "  the two-sided sign test: p = {two_sided} -- {}",
        if two_sided_passed {
            "passes"
        } else {
            "does not pass"
        }
    );
    println!("  worse on {worse} seeds, tied on {ties}");
    let qualifies = better >= bar && one_sided_passes;
    println!(
        "  VERDICT: the hot zone {}.",
        if qualifies {
            "qualifies"
        } else {
            "does not qualify"
        }
    );

    // The secondary reading: the contact, and nothing else.
    let contact_test = sign_test(&contact_deltas, Direction::Up);
    println!();
    println!("the secondary reading (contact only: 100 * tags + sight ticks):");
    println!(
        "  seeds where the hot zone had the contact: {contact_better} of {n} \
         (worse on {contact_worse}, tied on {contact_ties})"
    );
    println!(
        "  the sign test: {} seeds moved, {} of them the claimed way, p = {} (one-sided), \
         p = {} (two-sided) -- the two-sided test {}",
        contact_test.moved,
        contact_test.claimed,
        contact_test.p_value_text(),
        contact_test.two_sided_p_value_text(),
        if two_sided_passes(contact_test) {
            "passes"
        } else {
            "does not pass"
        }
    );

    // The cost: what the arms asked, and what they were refused.
    for (arm, _, _) in &harnesses {
        let arm_rounds: Vec<&Round> = rounds.iter().filter(|round| round.arm == *arm).collect();
        if arm_rounds.is_empty() {
            continue;
        }
        let asks: u64 = arm_rounds.iter().map(|round| round.asks).sum();
        let move_asks: u64 = arm_rounds.iter().map(|round| round.move_asks).sum();
        let talk_asks: u64 = arm_rounds.iter().map(|round| round.talk_asks).sum();
        let refusals: u64 = arm_rounds.iter().map(|round| round.refusals).sum();
        let escalations: u64 = arm_rounds.iter().map(|round| round.escalations).sum();
        let expired: u64 = arm_rounds.iter().map(|round| round.expired).sum();
        let decisions: u64 = arm_rounds.iter().map(|round| round.decisions).sum();
        let mut kinds: BTreeMap<String, u64> = BTreeMap::new();
        for round in &arm_rounds {
            for (kind, count) in &round.refusal_kinds {
                *kinds.entry(kind.clone()).or_insert(0) += count;
            }
        }
        let mut latencies: Vec<u32> = arm_rounds
            .iter()
            .flat_map(|round| round.latencies.clone())
            .collect();
        latencies.sort_unstable();
        let p50 = latencies.get(latencies.len() / 2).copied().unwrap_or(0);
        let p95 = latencies
            .get(latencies.len() * 95 / 100)
            .copied()
            .unwrap_or(0);
        let tags: u64 = arm_rounds.iter().map(|round| round.tags).sum();
        let sight: u64 = arm_rounds.iter().map(|round| round.sight_ticks).sum();
        let collapsed = arm_rounds
            .iter()
            .filter(|round| round.scores.iter().copied().min().unwrap_or(0) < COLLAPSE_POINTS)
            .count();
        println!();
        println!("the {arm} arm:");
        println!(
            "  {asks} asks over {} rounds ({:.1} a round: {move_asks} move, {talk_asks} talk), \
             {decisions} decisions",
            arm_rounds.len(),
            asks as f64 / arm_rounds.len() as f64
        );
        println!("  {refusals} refusals, {escalations} escalations, {expired} expired");
        println!("  the refusals by kind: {kinds:?}");
        println!("  the answer latency: p50 {p50} ms, p95 {p95} ms");
        println!(
            "  the model calls per agent: {:?}",
            arm_rounds
                .iter()
                .flat_map(|round| round.usage.iter().copied())
                .fold(
                    BTreeMap::new(),
                    |mut usage: BTreeMap<u16, u64>, (agent, calls)| {
                        *usage.entry(agent).or_insert(0) += calls;
                        usage
                    }
                )
        );
        let decided = arm_rounds
            .iter()
            .filter(|round| round.winner.is_some())
            .count();
        let wins0 = arm_rounds
            .iter()
            .filter(|round| round.winner == Some(0))
            .count();
        let wins1 = arm_rounds
            .iter()
            .filter(|round| round.winner == Some(1))
            .count();
        println!(
            "  {tags} tags over the rounds, {sight} sight ticks, {collapsed} rounds with a \
             squad held off the board"
        );
        println!(
            "  the rounds decided: {decided} of {} (squad 0 {wins0}, squad 1 {wins1})",
            arm_rounds.len()
        );
    }
    println!();
    println!("the run took {:.0} s", started.elapsed().as_secs_f64());

    // The raw record, if the caller wants it.
    if let Ok(path) = std::env::var("HOTZONE_DUMP") {
        let rounds_json: Vec<Value> = rounds
            .iter()
            .map(|round| {
                json!({
                    "arm": round.arm,
                    "seed": round.seed,
                    "ticks": round.ticks,
                    "tags": round.tags,
                    "sight_ticks": round.sight_ticks,
                    "scores": round.scores,
                    "margin": round.margin,
                    "draw": round.draw,
                    "winner": round.winner,
                    "end_tick": round.end_tick,
                    "first_sight_tick": round.first_sight_tick,
                    "score": round.score(),
                    "decisions": round.decisions,
                    "asks": round.asks,
                    "answered": round.answered,
                    "refusals": round.refusals,
                    "refusal_kinds": round.refusal_kinds,
                    "escalations": round.escalations,
                    "expired": round.expired,
                    "usage": round.usage,
                })
            })
            .collect();
        let text = serde_json::to_string_pretty(&json!({
            "seeds": seeds,
            "ticks": ticks(),
            "port": port,
            "deltas": deltas,
            "rounds": rounds_json,
        }))
        .expect("the dump is json");
        std::fs::write(&path, text).unwrap_or_else(|err| panic!("the dump writes: {err}"));
        println!("the raw record: {path}");
    }
}
