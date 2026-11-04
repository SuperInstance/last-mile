//! The urgency gate on the real Thor: what the model answers, and what the
//! game does with it.
//!
//! The agents never speak urgently. The E7 score view found that no line was
//! ever delivered urgent in 83 logged rounds and four Thor rounds: the
//! `intention` gate answers "calm" throughout, even in a tag fight. This is
//! the measurement that says why, and the measurement the fix is read
//! against.
//!
//! ## What is measured
//!
//! Every ask that comes back from the Thor's **d1-2B** is recorded with:
//!
//! - the `intention` gate's answer: the score (the level's index), the
//!   probability of every level, and the confidence;
//! - the **game state at the moment the question was asked**: how many
//!   enemies the agent can see, whether a tag fight is on, the score margin,
//!   the time left in the round;
//! - the **facts text the question was asked over**, which is what the
//!   situation is read from: an enemy with a clear line of sight, a stun, an
//!   ally's stun, a beacon the enemy holds;
//! - the line the gate's answer produced, and the affect it was delivered
//!   with.
//!
//! The round is the shipped one, played the way the game plays it: the host
//! loop, the agents, the least-use-first queue, the sequencer, the recorder.
//! The only thing that is not the shipped code is the model behind the tier:
//! the Thor's own **d1-2B**, on the LAN. Three seeds, 3000 ticks each, at
//! the round's own rate (30 Hz).
//!
//! The watcher asks the Thor with the floor at zero, so every gate's own
//! number is seen: the rule that would have been applied to them is derived
//! from them and reported below.
//!
//! ## The three configurations
//!
//! The game does not always have a fight, and a measurement of urgency
//! needs one, so the round is played three ways:
//!
//! - **shipped** (the default): the d1-2B answers both packs — the game as
//!   it ships;
//! - **stub** (`URGENCY_BACKEND=stub`): the stub answers both packs — the
//!   control, the same rounds with a model that answers at once;
//! - **split** (`URGENCY_BACKEND=split`): the stub answers the move pack
//!   and the d1-2B the talk pack — the fight, with the real model's
//!   urgency answers.
//!
//! The write-up of the numbers is `tests/urgency_on_thor.md`.
//!
//! ## The command
//!
//! ```text
//! cargo test --release -p lm-agents --test urgency_on_thor -- --ignored --nocapture
//! ```

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lm_agents::config::AgentsConfig;
use lm_agents::gates::Pack;
use lm_agents::harness::Harness;
use lm_agents::sysone::{Answer, Answers, EscalatingBackend, Refusal, SysOne, ThorBackend};
use lm_agents::Agents;
use lm_core::header::Header;
use lm_core::talk::TalkRecord;
use lm_core::view::ScoreView;
use lm_core::world::los;
use lm_host_model::HostLoop;
use serde_json::Value;

/// The shipped harness: the gates, the policy and the phrasebook the default
/// round thinks with.
const DEFAULT_GATES: &str = include_str!("../../../assets/harness/default/gates.toml");
const DEFAULT_POLICY: &str = include_str!("../../../assets/harness/default/policy.toml");
const DEFAULT_PHRASEBOOK: &str = include_str!("../../../assets/harness/default/phrasebook.toml");

/// The shipped reflex, compiled into the test binary.
const DEFAULT_REFLEX: &str = include_str!("../../../assets/reflexes/default.rhai");

/// The shipped policy, for the stub control run.
fn policy_toml() -> String {
    DEFAULT_POLICY.to_owned()
}

/// The ticks a round is played for: a hundred seconds at 30 Hz.
const TICKS: u64 = 3_000;

/// The seeds the measurement plays.
const SEEDS: [u64; 3] = [1, 2, 3];

/// How long a tag fight counts as a fight, in ticks: three seconds at 30 Hz.
const FIGHT_WINDOW: u64 = 90;

/// The enemy's own reach, in cells: the facts builder's ENEMY_NEAR_CELLS.
const ENEMY_NEAR_CELLS: i32 = 6;

/// The ticks a round is played for, if the caller says otherwise (a smoke
/// run: `URGENCY_TICKS=120`).
fn ticks() -> u64 {
    std::env::var("URGENCY_TICKS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(TICKS)
}

/// The seeds the measurement plays, if the caller says otherwise.
fn seeds() -> Vec<u64> {
    match std::env::var("URGENCY_SEEDS") {
        Ok(value) => value
            .split(',')
            .filter_map(|seed| seed.trim().parse::<u64>().ok())
            .collect(),
        Err(_) => SEEDS.to_vec(),
    }
}

/// What one agent could see on one tick.
#[derive(Clone, Copy, Debug, Default)]
struct Sight {
    /// How many fresh enemies the agent has within reach.
    near: i64,
    /// How many of them can see it.
    seeing: i64,
    /// Whether the agent is stunned (a tag landed on it).
    #[allow(dead_code)]
    stunned: bool,
}

/// The state of the round on one tick.
#[derive(Clone, Debug, Default)]
struct Moment {
    /// The tick.
    tick: u64,
    /// The round's scores, in squad order.
    scores: Vec<i64>,
    /// The last tick a tag was made on, if one was.
    last_tag: Option<u64>,
    /// What each agent could see, by agent id.
    sight: BTreeMap<u16, Sight>,
}

impl Moment {
    /// Whether a tag fight is on at this moment.
    fn in_fight(&self) -> bool {
        self.last_tag
            .is_some_and(|tag| self.tick.saturating_sub(tag) <= FIGHT_WINDOW)
    }

    /// The score margin of the round: the gap between the best and the worst
    /// squad, in points.
    fn margin(&self) -> i64 {
        match (self.scores.iter().min(), self.scores.iter().max()) {
            (Some(low), Some(high)) => high - low,
            _ => 0,
        }
    }

    /// The most any agent is threatened on this tick.
    fn worst_sight(&self) -> Sight {
        self.sight
            .values()
            .copied()
            .max_by_key(|sight| (sight.seeing, sight.near))
            .unwrap_or_default()
    }
}

/// The game situation a moment belongs to: the rows of the confusion table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Situation {
    /// A tag was made in the last [`FIGHT_WINDOW`] ticks: the squads are in
    /// contact.
    Fight,
    /// No tag fight, but an enemy can see an agent.
    Threat,
    /// No tag fight and no enemy in sight.
    Quiet,
}

impl Situation {
    /// The situation of the round for one agent: a fight is a fight whoever is
    /// speaking, and the threat is the speaker's own.
    fn of_agent(moment: &Moment, agent: u16) -> Self {
        if moment.in_fight() {
            Situation::Fight
        } else {
            let seeing = moment
                .sight
                .get(&agent)
                .map(|sight| sight.seeing)
                .unwrap_or(0);
            if seeing > 0 {
                Situation::Threat
            } else {
                Situation::Quiet
            }
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Situation::Fight => "fight",
            Situation::Threat => "threat",
            Situation::Quiet => "quiet",
        }
    }
}

/// The situation as the **facts the gate was asked over** state it.
///
/// The round's own state (a tag, an enemy in sight) is the situation of
/// the tick; this is the situation the model was shown, which is the one
/// the gate's answer is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum FactsSituation {
    /// An enemy can see the agent, or the agent (or an ally next to it)
    /// has been hit: the squad is under fire.
    UnderFire,
    /// Nothing is on the agent but the enemy holds a beacon.
    Contest,
    /// Nothing at all.
    Quiet,
}

impl FactsSituation {
    /// Read the situation off the facts text, the way the model reads it.
    fn of(state: &str) -> Self {
        let watching = state.contains("can see you (clear line of sight)");
        let hit = state.contains("You are stunned.") || state.contains(" and is stunned");
        if watching || hit {
            FactsSituation::UnderFire
        } else if state.contains("held by enemy squad") {
            FactsSituation::Contest
        } else {
            FactsSituation::Quiet
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            FactsSituation::UnderFire => "under fire",
            FactsSituation::Contest => "contest",
            FactsSituation::Quiet => "quiet",
        }
    }
}

/// One ask that came back, and what the gates made of it.
#[derive(Clone, Debug)]
struct Ask {
    /// Which pack the ask carried (D70).
    pack: Pack,
    /// The agent that asked: the facts text names it.
    agent: u16,
    /// The facts the question was asked over.
    state: String,
    /// The game state at the moment of the ask.
    moment: Moment,
    /// The intention gate's answer: the score, the level probabilities and
    /// the confidence.
    intention: Option<(f32, BTreeMap<String, f32>, f32)>,
    /// The round trip the model reported, in milliseconds.
    latency_ms: u32,
}

/// The watcher: the Thor behind a log.
struct Watcher {
    inner: Box<dyn SysOne + Send + Sync>,
    log: Arc<Mutex<Vec<Ask>>>,
    /// The moments of the round, by tick: the main loop writes them.
    moments: Arc<Mutex<BTreeMap<u64, Moment>>>,
}

impl Watcher {
    /// A watcher over the Thor's d1-2B, with the floor at zero so that every
    /// gate's answer is seen.
    fn new(cfg: &AgentsConfig) -> Self {
        // The stub answers at once, so a round is a second rather than a
        // hundred: the control run, the same round on the same harness with
        // the model behind it replaced by a stub. It says whether the game
        // reaches a fight at all with this model's moves.
        if std::env::var("URGENCY_BACKEND").as_deref() == Ok("stub") {
            let policy = lm_agents::mind::Policy::for_harness(&policy_toml());
            let mut backends: BTreeMap<String, Box<dyn SysOne + Send + Sync>> = BTreeMap::new();
            for tier in policy.tiers.tiers().iter().filter(|tier| !tier.is_reflex()) {
                backends.insert(
                    tier.backend().to_owned(),
                    Box::new(lm_agents::sysone::StubBackend::menu())
                        as Box<dyn SysOne + Send + Sync>,
                );
            }
            let inner = backends
                .into_values()
                .next()
                .expect("the policy names a model tier");
            return Watcher {
                inner,
                log: Arc::new(Mutex::new(Vec::new())),
                moments: Arc::new(Mutex::new(BTreeMap::new())),
            };
        }
        let port = cfg.ports["d1-2b"];
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
        // The split run: the **move** pack on the stub (the stub's play is
        // what makes the squads meet, so the round has a fight to be urgent
        // about) and the **talk** pack on the real d1-2B, so the urgency
        // answers and the delivered lines are the model's own.
        if std::env::var("URGENCY_BACKEND").as_deref() == Ok("split") {
            let inner: Box<dyn SysOne + Send + Sync> = Box::new(Split {
                move_pack: Box::new(lm_agents::sysone::StubBackend::menu()),
                talk_pack: Box::new(escalating),
            });
            return Watcher {
                inner,
                log: Arc::new(Mutex::new(Vec::new())),
                moments: Arc::new(Mutex::new(BTreeMap::new())),
            };
        }
        Watcher {
            inner: Box::new(escalating),
            log: Arc::new(Mutex::new(Vec::new())),
            moments: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    fn log(&self) -> Arc<Mutex<Vec<Ask>>> {
        Arc::clone(&self.log)
    }

    fn moments(&self) -> Arc<Mutex<BTreeMap<u64, Moment>>> {
        Arc::clone(&self.moments)
    }
}

impl SysOne for Watcher {
    fn ask(&self, state: &str, questions: &Value, deadline: Instant) -> Result<Answers, Refusal> {
        // The game state at the moment of the **question**: the main loop
        // writes the moments of the round, and the newest one is the state
        // the question was asked over. It is read before the ask, not after:
        // the pack takes a second to answer, and the moment the answer
        // lands in is a second later than the moment that was asked about.
        let moment = {
            let moments = self.moments.lock().expect("the watcher's moments");
            moments
                .range(..=u64::MAX)
                .next_back()
                .map(|(_, moment)| moment.clone())
                .unwrap_or_default()
        };

        let answers = self.inner.ask(state, questions, deadline)?;

        let intention = match answers.per_gate.get("intention") {
            Some(Answer::Score {
                score,
                probs,
                confidence,
            }) => Some((*score, probs.clone(), *confidence)),
            _ => None,
        };

        // The pack is told by the questions it carried (D70): the move pack
        // holds the move gate, the talk pack the gates a line is chosen by.
        let pack = if questions.get("move").is_some() {
            Pack::Move
        } else {
            Pack::Talk
        };

        self.log.lock().expect("the watcher's log").push(Ask {
            pack,
            agent: agent_of(state),
            state: state.to_owned(),
            moment,
            intention,
            latency_ms: answers.latency_ms,
        });

        Ok(answers)
    }

    fn name(&self) -> &str {
        self.inner.name()
    }
}

/// The split backend: the move pack on the stub, the talk pack on the Thor.
///
/// The stub's play is what makes the two squads meet — it walks every agent
/// at the first beacon in the menu, so the squads converge on the same one
/// and fight — while the Thor's d1-2B answers the talk pack, so the urgency
/// answers and the lines they become are the model's own.
struct Split {
    move_pack: Box<dyn SysOne + Send + Sync>,
    talk_pack: Box<dyn SysOne + Send + Sync>,
}

impl SysOne for Split {
    fn ask(&self, state: &str, questions: &Value, deadline: Instant) -> Result<Answers, Refusal> {
        if questions.get("move").is_some() {
            self.move_pack.ask(state, questions, deadline)
        } else {
            self.talk_pack.ask(state, questions, deadline)
        }
    }

    fn name(&self) -> &str {
        "split"
    }
}

/// The agent a facts text is about: "You are agent 3, the leader, ...".
fn agent_of(state: &str) -> u16 {
    let marker = "You are agent ";
    let Some(rest) = state.split(marker).nth(1) else {
        return 0;
    };
    rest.split(|c: char| !c.is_ascii_digit())
        .next()
        .and_then(|digits| digits.parse::<u16>().ok())
        .unwrap_or(0)
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

/// Play one round on the Thor, at the round's own rate, and measure it.
fn play(seed: u64) -> (Vec<Ask>, Vec<TalkRecord>, BTreeMap<u64, Moment>) {
    let header = Header::default_round(seed, DEFAULT_REFLEX);
    let harness = Harness {
        dir: std::path::PathBuf::from("assets/harness/default"),
        gates_toml: DEFAULT_GATES.to_owned(),
        policy_toml: DEFAULT_POLICY.to_owned(),
        phrasebook_toml: DEFAULT_PHRASEBOOK.to_owned(),
        reflexes: BTreeMap::new(),
        commit: "content:test-harness".to_owned(),
    };
    let cfg = AgentsConfig::default();

    let watcher = Watcher::new(&cfg);
    let log = watcher.log();
    let moments = watcher.moments();
    let backends = BTreeMap::from([(
        "thor-d1-2b".to_owned(),
        Box::new(watcher) as Box<dyn SysOne + Send + Sync>,
    )]);

    let mut host = HostLoop::new(header.clone()).expect("the round starts");
    host.attach_agents(Agents::new(&cfg, &harness, &header, backends));

    let period = Duration::from_millis(1000 / u64::from(header.params.tick_hz));
    let mut last_tag: Option<u64> = None;
    for _ in 0..ticks() {
        std::thread::sleep(period);

        // The state of the round on this tick, for the agents that are
        // thinking on it: what every agent can see, and the score.
        let tick = host.sim().tick();
        let scores = host.sim().scores().to_vec();
        let mut sight = BTreeMap::new();
        for entry in &header.roster {
            let view = host.sim().view(entry.agent);
            let (near, seeing) = threat_of(&view, host.sim().world(), cfg.stale_enemy_ticks);
            sight.insert(
                entry.agent,
                Sight {
                    near,
                    seeing,
                    stunned: view.me.stunned,
                },
            );
        }
        moments.lock().expect("the watcher's moments").insert(
            tick,
            Moment {
                tick,
                scores: scores.clone(),
                last_tag,
                sight,
            },
        );

        let out = host.tick();
        for event in &out.events {
            if matches!(event, lm_core::rules::Event::Tagged { .. }) {
                last_tag = Some(out.tick);
            }
        }
    }

    let log = log.lock().expect("the watcher's log").clone();
    let moments = moments.lock().expect("the watcher's moments").clone();
    let records = host.sim().talk_records().to_vec();
    (log, records, moments)
}

/// The percentile of a sorted slice, by rank.
fn percentile(sorted: &[f32], q: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = ((sorted.len() - 1) as f32 * q).round() as usize;
    sorted[rank]
}

/// The measurement: three rounds on the Thor, and what the urgency gate did.
#[test]
#[ignore = "needs the Thor at 192.168.0.78; run with --release --ignored --nocapture"]
fn urgency_on_thor() {
    let mut asks: Vec<Ask> = Vec::new();
    let mut lines: Vec<TalkRecord> = Vec::new();
    let mut moments: BTreeMap<u64, Moment> = BTreeMap::new();

    for seed in seeds() {
        let (seed_asks, seed_lines, seed_moments) = play(seed);
        let scores = seed_moments
            .values()
            .last()
            .map(|moment| moment.scores.clone())
            .unwrap_or_default();
        println!(
            "round {seed}: {} asks answered, {} lines delivered, {} ticks of game, scores {scores:?}",
            seed_asks.len(),
            seed_lines.len(),
            seed_moments.len()
        );
        asks.extend(seed_asks);
        lines.extend(seed_lines);
        moments.extend(seed_moments);
    }

    // The raw record, for the write-up and for anybody who wants to look at
    // the states themselves.
    if let Ok(path) = std::env::var("URGENCY_DUMP") {
        let mut json = Vec::new();
        for ask in &asks {
            if ask.pack != Pack::Talk {
                continue;
            }
            let (score, probs, confidence) = match &ask.intention {
                Some((score, probs, confidence)) => {
                    (Some(*score), Some(probs.clone()), Some(*confidence))
                }
                None => (None, None, None),
            };
            json.push(serde_json::json!({
                "agent": ask.agent,
                "tick": ask.moment.tick,
                "score": score,
                "probs": probs,
                "confidence": confidence,
                "latency_ms": ask.latency_ms,
                "fight": ask.moment.in_fight(),
                "margin": ask.moment.margin(),
                "seeing": ask.moment.sight.get(&ask.agent).map(|s| s.seeing),
                "near": ask.moment.sight.get(&ask.agent).map(|s| s.near),
                "state": ask.state,
            }));
        }
        if let Err(err) = std::fs::write(&path, serde_json::to_string_pretty(&json).unwrap()) {
            eprintln!("the dump did not write: {err}");
        } else {
            println!("the raw record is at {path}");
        }
    }

    println!();
    println!("# the urgency gate on the real Thor");
    println!();

    // ---------------------------------------------------------------- gate
    let talk_asks: Vec<&Ask> = asks
        .iter()
        .filter(|ask| ask.pack == Pack::Talk && ask.intention.is_some())
        .collect();

    println!("## the intention gate's answer");
    println!();
    println!("- talk asks answered: {}", talk_asks.len());
    println!(
        "- move asks answered: {}",
        asks.iter().filter(|ask| ask.pack == Pack::Move).count()
    );

    let mut scores: Vec<f32> = Vec::new();
    let mut by_level: BTreeMap<String, u64> = BTreeMap::new();
    for ask in &talk_asks {
        let (score, _, _) = ask.intention.as_ref().expect("the intention gate answered");
        scores.push(*score);
        *by_level.entry(format!("{score}")).or_default() += 1;
    }
    scores.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if !scores.is_empty() {
        println!(
            "- the score: min {:.0}, p10 {:.0}, p50 {:.0}, p90 {:.0}, max {:.0}",
            scores[0],
            percentile(&scores, 0.10),
            percentile(&scores, 0.50),
            percentile(&scores, 0.90),
            scores[scores.len() - 1]
        );
    }
    println!("- the levels it picked (0 idle, 1 ordinary, 2 urgent):");
    for (level, count) in &by_level {
        println!("  - {level}: {count}");
    }
    println!();

    // The confidence the gate answered with.
    let mut confidences: Vec<f32> = talk_asks
        .iter()
        .filter_map(|ask| ask.intention.as_ref().map(|(_, _, confidence)| *confidence))
        .collect();
    confidences.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if !confidences.is_empty() {
        println!(
            "- the gate's confidence: min {:.4}, p50 {:.4}, max {:.4}",
            confidences[0],
            percentile(&confidences, 0.50),
            confidences[confidences.len() - 1]
        );
    }
    println!();

    // The gate's answer against the game situation: the confusion table.
    println!("## the gate's answer against the game situation");
    println!();
    println!("### the situation the facts state");
    println!();
    println!("| situation | asks | idle (0) | ordinary (1) | urgent (2) |");
    println!("|---|---|---|---|---|");
    let mut situation_asks: BTreeMap<FactsSituation, Vec<&Ask>> = BTreeMap::new();
    for ask in &talk_asks {
        situation_asks
            .entry(FactsSituation::of(&ask.state))
            .or_default()
            .push(ask);
    }
    for (situation, group) in &situation_asks {
        let mut idle = 0;
        let mut ordinary = 0;
        let mut urgent = 0;
        for ask in group {
            let (score, _, _) = ask.intention.as_ref().expect("the intention gate answered");
            match score.round() as i32 {
                0 => idle += 1,
                1 => ordinary += 1,
                _ => urgent += 1,
            }
        }
        println!(
            "| {} | {} | {idle} | {ordinary} | {urgent} |",
            situation.as_str(),
            group.len()
        );
    }
    println!();

    println!("### the situation of the round");
    println!();
    println!("| situation | asks | idle (0) | ordinary (1) | urgent (2) |");
    println!("|---|---|---|---|---|");
    let mut round_asks: BTreeMap<Situation, Vec<&Ask>> = BTreeMap::new();
    for ask in &talk_asks {
        round_asks
            .entry(Situation::of_agent(&ask.moment, ask.agent))
            .or_default()
            .push(ask);
    }
    for (situation, group) in &round_asks {
        let mut idle = 0;
        let mut ordinary = 0;
        let mut urgent = 0;
        for ask in group {
            let (score, _, _) = ask.intention.as_ref().expect("the intention gate answered");
            match score.round() as i32 {
                0 => idle += 1,
                1 => ordinary += 1,
                _ => urgent += 1,
            }
        }
        println!(
            "| {} | {} | {idle} | {ordinary} | {urgent} |",
            situation.as_str(),
            group.len()
        );
    }
    println!();

    // The probability the gate gave to each level, per situation: the
    // question the brief asks ("is urgent ever above 0.2 in a fight?").
    println!("## the probability of the urgent level, per situation");
    println!();
    println!("| situation | asks | p(urgent): min | p10 | p50 | p90 | max | above 0.2 |");
    println!("|---|---|---|---|---|---|---|---|");
    for (situation, group) in &situation_asks {
        let mut values: Vec<f32> = Vec::new();
        for ask in group {
            let (_, probs, _) = ask.intention.as_ref().expect("the intention gate answered");
            let total: f32 = probs.values().sum();
            let urgent = probs.get("2").copied().unwrap_or(0.0);
            let p = if total > 0.0 { urgent / total } else { 0.0 };
            values.push(p);
        }
        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let above = values.iter().filter(|p| **p > 0.2).count();
        println!(
            "| {} | {} | {:.4} | {:.4} | {:.4} | {:.4} | {:.4} | {above}/{} |",
            situation.as_str(),
            group.len(),
            values[0],
            percentile(&values, 0.10),
            percentile(&values, 0.50),
            percentile(&values, 0.90),
            values[values.len() - 1],
            group.len()
        );
    }
    println!();

    // ------------------------------------------------------------ delivered
    println!("## the delivered lines");
    println!();
    let mut by_affect: BTreeMap<String, u64> = BTreeMap::new();
    for line in &lines {
        *by_affect.entry(format!("{:?}", line.affect)).or_default() += 1;
    }
    println!("- lines delivered: {}", lines.len());
    for (affect, count) in &by_affect {
        println!("  - {affect}: {count}");
    }
    println!();

    // The confusion table the brief asks for: the game situation a line was
    // delivered in, against the urgency it was delivered with.
    println!("### the confusion table: situation against delivered urgency");
    println!();
    println!("| situation | lines | calm | ordinary | urgent |");
    println!("|---|---|---|---|---|");
    let mut table: BTreeMap<Situation, BTreeMap<String, u64>> = BTreeMap::new();
    for line in &lines {
        let moment = moments
            .range(..=line.at_tick)
            .next_back()
            .map(|(_, moment)| moment.clone())
            .unwrap_or_default();
        let situation = Situation::of_agent(&moment, line.speaker);
        *table
            .entry(situation)
            .or_default()
            .entry(format!("{:?}", line.affect))
            .or_default() += 1;
    }
    for (situation, counts) in &table {
        let calm = counts.get("Calm").copied().unwrap_or(0);
        let ordinary = counts.get("Uncertain").copied().unwrap_or(0);
        let urgent = counts.get("Urgent").copied().unwrap_or(0);
        println!(
            "| {} | {} | {calm} | {ordinary} | {urgent} |",
            situation.as_str(),
            calm + ordinary + urgent
        );
    }
    println!();

    // The rounds' own shape: how many tags, how many threats.
    let fights = moments.values().filter(|moment| moment.in_fight()).count();
    let threats = moments
        .values()
        .filter(|moment| moment.worst_sight().seeing > 0)
        .count();
    println!("## the rounds");
    println!();
    println!("- ticks of game: {}", moments.len());
    println!("- ticks with a tag in the last {FIGHT_WINDOW}: {fights}");
    println!("- ticks with an enemy in sight: {threats}");
}
