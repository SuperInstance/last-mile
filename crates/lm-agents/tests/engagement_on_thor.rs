//! The engagement: what the shipped doctrine does to a round, measured on the
//! real Thor.
//!
//! The shipped doctrine ends every round in a draw, with no tag and no enemy
//! ever in sight. This is the measurement that says **why**, and the yardstick
//! the doctrine variants in `assets/harness/variants/` are read against.
//!
//! ## What is measured
//!
//! Every round is played the way the game plays it: the host loop, the
//! agents, the least-use-first queue, the sequencer, the recorder. The only
//! thing that is not the shipped code is the model behind the tier — the
//! Thor's own **d1-2B**, on the LAN — and the harness, which the caller picks.
//!
//! For every round:
//!
//! - **tags**: the `Tagged` events the ruleset made;
//! - **enemy-sight ticks**: the ticks on which some agent had a fresh enemy
//!   within reach and a clear line of sight to it;
//! - **draw**: the round's outcome, and the **score margin** between the
//!   squads;
//! - **decisions**: the decisions the agents' answers became;
//! - **urgent lines**: the lines the **urgency gate** (`intention`) made
//!   urgent, and the lines that were delivered with that affect.
//!
//! And, for every move-gate answer, the two things the write-up turns on:
//!
//! - the **branch** of the doctrine that the facts the model was shown select
//!   (retreat / hold-on-the-beacon / go-to-the-nearest-unheld-beacon /
//!   hold-because-there-is-nothing-to-go-to), derived from the facts text the
//!   same way the doctrine reads it;
//! - the **option the model chose**, and how sure it was.
//!
//! When the branch and the choice agree, the hold is the **rule** the model
//! applied; when they disagree, the wording of the criteria is what moved it.
//!
//! ## The configurations
//!
//! - **thor** (the default): the d1-2B answers both packs — the game as it
//!   ships;
//! - **stub** (`ENGAGEMENT_BACKEND=stub`): the stub answers both packs — the
//!   control, the same rounds with a model that answers at once and always
//!   takes the first option of the menu;
//! - **split** (`ENGAGEMENT_BACKEND=split`): the stub answers the move pack
//!   and the d1-2B the talk pack — the fight, with the real model's urgency
//!   answers.
//!
//! ## The harness
//!
//! `ENGAGEMENT_HARNESS` names the harness directory, relative to the repo
//! root: `assets/harness/default` (the shipped one) or
//! `assets/harness/variants/<name>` (a variant). The harness's own reflex is
//! the fallback when the model refuses, so the headless and the on-Thor
//! measurements of a variant are the same doctrine.
//!
//! ## The command
//!
//! ```text
//! cargo test --release -p lm-agents --test engagement_on_thor -- --ignored --nocapture
//! ```

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lm_agents::config::AgentsConfig;
use lm_agents::gates::Pack;
use lm_agents::harness::Harness;
use lm_agents::sysone::{Answer, Answers, EscalatingBackend, Refusal, SysOne, ThorBackend};
use lm_agents::Agents;
use lm_core::header::Header;
use lm_core::talk::TalkRecord;
use lm_core::utterance::Affect;
use lm_core::view::ScoreView;
use lm_core::world::los;
use lm_host_model::HostLoop;
use serde_json::{json, Value};

/// The shipped reflex, compiled into the test binary.
const DEFAULT_REFLEX: &str = include_str!("../../../assets/reflexes/default.rhai");

/// The ticks a round is played for: a hundred seconds at 30 Hz.
const TICKS: u64 = 3_000;

/// The seeds the measurement plays.
const SEEDS: [u64; 3] = [1, 2, 3];

/// The enemy's own reach, in cells: the facts builder's ENEMY_NEAR_CELLS.
const ENEMY_NEAR_CELLS: i32 = 6;

/// The root of the repository, where the assets live.
///
/// Cargo runs a test binary with the package directory as its working
/// directory, so the assets are found from the manifest, not from the
/// current directory.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The harness directory the measurement plays, if the caller says otherwise.
///
/// The name is relative to the repository root: `assets/harness/default` (the
/// shipped one) or `assets/harness/variants/<name>` (a variant).
fn harness_dir() -> PathBuf {
    let name =
        std::env::var("ENGAGEMENT_HARNESS").unwrap_or_else(|_| "assets/harness/default".to_owned());
    repo_root().join(name)
}

/// The ticks a round is played for, if the caller says otherwise.
fn ticks() -> u64 {
    std::env::var("ENGAGEMENT_TICKS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(TICKS)
}

/// The seeds the measurement plays, if the caller says otherwise.
fn seeds() -> Vec<u64> {
    match std::env::var("ENGAGEMENT_SEEDS") {
        Ok(value) => value
            .split(',')
            .filter_map(|seed| seed.trim().parse::<u64>().ok())
            .collect(),
        Err(_) => SEEDS.to_vec(),
    }
}

/// The branch of the doctrine that the facts select.
///
/// The doctrine is a rule over the facts the builder states, so the branch is
/// read off the facts text the same way the model reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Branch {
    /// (1) an enemy can see you and you are outnumbered: retreat.
    Retreat,
    /// (2) you are standing on a beacon your squad holds and no enemy is
    /// near: hold.
    HoldOnBeacon,
    /// (3) go to the nearest beacon your squad does not hold.
    GoNearestUnheld,
    /// (3) there is no such beacon: hold.
    HoldNoBeacon,
}

impl Branch {
    fn as_str(self) -> &'static str {
        match self {
            Branch::Retreat => "retreat",
            Branch::HoldOnBeacon => "hold (on the beacon)",
            Branch::GoNearestUnheld => "go to the nearest unheld beacon",
            Branch::HoldNoBeacon => "hold (nothing to go to)",
        }
    }

    /// The option the doctrine's branch selects, by menu name.
    fn option(self, beacon: Option<u8>) -> String {
        match self {
            Branch::Retreat => "retreat".to_owned(),
            Branch::HoldOnBeacon | Branch::HoldNoBeacon => "hold".to_owned(),
            Branch::GoNearestUnheld => {
                format!("go to beacon {}", beacon.unwrap_or_default())
            }
        }
    }
}

/// The facts the doctrine is read off, as the facts text states them.
#[derive(Clone, Debug, Default)]
struct Facts {
    /// How many fresh enemies are within the enemy's reach.
    near: i64,
    /// How many of them can see the agent.
    seeing: i64,
    /// How many allies are within the ally's reach.
    allies: i64,
    /// Whether the agent stands on a beacon its own squad holds.
    on_held_beacon: bool,
    /// The beacon the facts name as the nearest one the squad does not hold.
    nearest_unheld: Option<u8>,
}

impl Facts {
    /// Read the facts off the text the builder wrote.
    ///
    /// The builder writes the facts as one paragraph of sentences, so the
    /// text is read by the phrases it carries, not by lines.
    fn of(state: &str) -> Facts {
        let mut facts = Facts::default();

        // The enemy count line.
        if state.contains("No enemy is within 6 cells of you.") {
            facts.near = 0;
            facts.seeing = 0;
        } else if let Some(index) = state.find(" enemies are within 6 cells of you") {
            facts.near = number_before(state, " enemies are within 6 cells of you").unwrap_or(0);
            // The tail of the sentence says how many of them can see the
            // agent.
            let tail = &state[index..];
            let sentence = tail.split('.').next().unwrap_or("");
            facts.seeing = if sentence.contains("all of them can see you") {
                facts.near
            } else if sentence.contains("none of them can see you") {
                0
            } else if let Some(seen) = number_before(sentence, " of them can see you") {
                seen
            } else if sentence.contains("it can see you") {
                1
            } else {
                0
            };
        } else if state.contains("1 enemy is within 6 cells of you") {
            facts.near = 1;
            facts.seeing = i64::from(state.contains("and it can see you"));
        }

        // The ally count line.
        if let Some(index) = state.find(" allies within 4 cells of you") {
            facts.allies = number_before(state, " allies within 4 cells of you").unwrap_or(0);
            let _ = index;
        }

        // Where the agent stands.
        if let Some(index) = state.find("You are standing on beacon ") {
            let tail = &state[index..];
            let sentence = tail.split('.').next().unwrap_or("");
            facts.on_held_beacon = sentence.contains("which your squad holds");
        }

        // The beacon the facts name as the doctrine's target: the beacon
        // named just before the marker.
        let marker = "it is the nearest beacon your squad does not hold";
        if let Some(index) = state.find(marker) {
            let head = &state[..index];
            if let Some(start) = head.rfind("Beacon ") {
                facts.nearest_unheld = number_after(&head[start..], "Beacon ").map(|n| n as u8);
            }
        }

        facts
    }

    /// The branch of the doctrine these facts select.
    fn branch(&self) -> Branch {
        if self.seeing > 0 && self.allies < self.near {
            Branch::Retreat
        } else if self.on_held_beacon && self.near == 0 {
            Branch::HoldOnBeacon
        } else if self.nearest_unheld.is_some() {
            Branch::GoNearestUnheld
        } else {
            Branch::HoldNoBeacon
        }
    }
}

/// The number in front of `marker` in a line.
fn number_before(line: &str, marker: &str) -> Option<i64> {
    let index = line.find(marker)?;
    let head = &line[..index];
    let digits: String = head
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return None;
    }
    digits.chars().rev().collect::<String>().parse::<i64>().ok()
}

/// The number after `marker` in a line.
fn number_after(line: &str, marker: &str) -> Option<i64> {
    let index = line.find(marker)? + marker.len();
    let tail = &line[index..];
    let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<i64>().ok()
}

/// One ask that came back.
#[derive(Clone, Debug)]
struct Ask {
    /// Which pack the ask carried (D70).
    pack: Pack,
    /// The agent that asked.
    agent: u16,
    /// The facts the question was asked over.
    state: String,
    /// The menu the move gate's options came from, by option key.
    options: BTreeMap<String, String>,
    /// The model's probability for every option of the move gate.
    probs: BTreeMap<String, f32>,
    /// The urgency gate's score, when the talk pack asked it.
    intention: Option<f32>,
    /// The round trip the model reported, in milliseconds.
    latency_ms: u32,
}

impl Ask {
    /// The option the model chose: the one it gave the most probability.
    fn choice(&self) -> Option<String> {
        let mut best: Option<(&String, &f32)> = None;
        for (key, prob) in &self.probs {
            if best.is_none_or(|(_, top)| prob > top) {
                best = Some((key, prob));
            }
        }
        let (key, _) = best?;
        self.options.get(key).cloned()
    }

    /// How sure the model was of the option it chose.
    fn confidence(&self) -> f32 {
        let mut best: Option<&f32> = None;
        for prob in self.probs.values() {
            if best.is_none_or(|top| prob > top) {
                best = Some(prob);
            }
        }
        best.copied().unwrap_or(0.0)
    }
}

/// The watcher: the Thor behind a log.
struct Watcher {
    inner: Box<dyn SysOne + Send + Sync>,
    log: Arc<Mutex<Vec<Ask>>>,
}

impl Watcher {
    /// A watcher over the Thor's d1-2B, with the floor at zero so that every
    /// gate's answer is seen.
    fn new(cfg: &AgentsConfig, harness: &Harness) -> Self {
        let policy = lm_agents::mind::Policy::for_harness(&harness.policy_toml);
        if std::env::var("ENGAGEMENT_BACKEND").as_deref() == Ok("stub") {
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
        if std::env::var("ENGAGEMENT_BACKEND").as_deref() == Ok("split") {
            let inner: Box<dyn SysOne + Send + Sync> = Box::new(Split {
                move_pack: Box::new(lm_agents::sysone::StubBackend::menu()),
                talk_pack: Box::new(escalating),
            });
            return Watcher {
                inner,
                log: Arc::new(Mutex::new(Vec::new())),
            };
        }
        Watcher {
            inner: Box::new(escalating),
            log: Arc::new(Mutex::new(Vec::new())),
        }
    }

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
    /// survive the wrap: a wrapper that dropped it would let a second
    /// opinion outlive the decision it serves.
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

        let pack = if questions.get("move").is_some() {
            Pack::Move
        } else {
            Pack::Talk
        };

        // The menu the move gate's options came from, read off the criteria
        // the question carried: each criterion says which move it is.
        let mut options = BTreeMap::new();
        if let Some(criteria) = questions
            .get("move")
            .and_then(|move_gate| move_gate.get("criteria"))
            .and_then(|criteria| criteria.as_object())
        {
            for (key, text) in criteria {
                if let Some(text) = text.as_str() {
                    options.insert(key.clone(), option_name(text));
                }
            }
        }

        let mut probs = BTreeMap::new();
        let mut intention = None;
        for (gate, answer) in &answers.per_gate {
            match answer {
                Answer::Choice { probs: p, .. } => {
                    if gate == "move" {
                        probs = p.clone();
                    }
                }
                Answer::Score { score, .. } => {
                    if gate == "intention" {
                        intention = Some(*score);
                    }
                }
                Answer::Noul(_) => {}
            }
        }

        self.log.lock().expect("the watcher's log").push(Ask {
            pack,
            agent: agent_of(state),
            state: state.to_owned(),
            options,
            probs,
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
struct Split {
    move_pack: Box<dyn SysOne + Send + Sync>,
    talk_pack: Box<dyn SysOne + Send + Sync>,
}

impl SysOne for Split {
    fn ask(&self, state: &str, questions: &Value, deadline: Instant) -> Result<Answers, Refusal> {
        self.ask_with_escalation(state, questions, deadline, deadline)
    }

    /// The split forwards the escalation's deadline (N3).
    ///
    /// The talk pack's backend escalates, so the fence has to survive the
    /// split: a router that dropped it would let a second opinion outlive
    /// the decision it serves.
    fn ask_with_escalation(
        &self,
        state: &str,
        questions: &Value,
        deadline: Instant,
        escalation_deadline: Instant,
    ) -> Result<Answers, Refusal> {
        if questions.get("move").is_some() {
            self.move_pack
                .ask_with_escalation(state, questions, deadline, escalation_deadline)
        } else {
            self.talk_pack
                .ask_with_escalation(state, questions, deadline, escalation_deadline)
        }
    }

    fn name(&self) -> &str {
        "split"
    }
}

/// The name of the move a criterion describes.
fn option_name(text: &str) -> String {
    if let Some(rest) = text.strip_prefix("go to beacon ") {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            return format!("go to beacon {digits}");
        }
    }
    if text.starts_with("hold position") {
        return "hold".to_owned();
    }
    if text.starts_with("follow the squad leader") {
        return "follow".to_owned();
    }
    if text.starts_with("retreat to the squad's spawn") {
        return "retreat".to_owned();
    }
    text.to_owned()
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

/// What one round did, measured.
#[derive(Clone, Debug, Default)]
struct Round {
    /// The seed the round ran on.
    seed: u64,
    /// The ticks the round played.
    ticks: u64,
    /// The tags the ruleset made.
    tags: u64,
    /// The ticks on which some agent had an enemy in sight.
    sight_ticks: u64,
    /// The ticks on which some agent had an enemy in sight, per agent.
    agent_sight_ticks: u64,
    /// The round's final scores, in squad order.
    scores: Vec<i64>,
    /// The gap between the squads' scores.
    margin: i64,
    /// Whether the round ended in a draw.
    draw: bool,
    /// The tick the round ended on.
    end_tick: u64,
    /// The decisions the agents' answers became.
    decisions: u64,
    /// The questions the round asked.
    asks: u64,
    /// The lines that were spoken.
    lines: u64,
    /// The lines that were delivered urgent.
    urgent_lines: u64,
    /// Where every agent stood at the end of the round.
    positions: Vec<(u16, i64, i64)>,
}

/// Play one round on the Thor, at the round's own rate, and measure it.
fn play(seed: u64, harness: &Harness, reflex: &str) -> (Round, Vec<Ask>, Vec<TalkRecord>) {
    let header = Header::default_round(seed, reflex);
    let cfg = AgentsConfig::default();

    let watcher = Watcher::new(&cfg, harness);
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
    let mut agent_sight_ticks = 0u64;
    let mut end_tick = 0u64;

    for _ in 0..ticks() {
        std::thread::sleep(period);

        let _tick = host.sim().tick();

        // What every agent can see on this tick.
        let mut seen_by_any = false;
        for entry in &header.roster {
            let view = host.sim().view(entry.agent);
            let (near, seeing) = threat_of(&view, host.sim().world(), cfg.stale_enemy_ticks);
            if seeing > 0 {
                seen_by_any = true;
                agent_sight_ticks += 1;
            }
            let _ = near;
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
        if let Some(outcome) = host.sim().outcome() {
            end_tick = out.tick;
            let _ = outcome;
            break;
        }
    }

    let scores = host.sim().scores().to_vec();
    let margin = match (scores.iter().min(), scores.iter().max()) {
        (Some(low), Some(high)) => high - low,
        _ => 0,
    };
    let positions = header
        .roster
        .iter()
        .map(|entry| {
            let view = host.sim().view(entry.agent);
            (
                entry.agent,
                i64::from(view.me.cell.x),
                i64::from(view.me.cell.y),
            )
        })
        .collect();
    let records = host.sim().talk_records().to_vec();
    let urgent_lines = records
        .iter()
        .filter(|record| record.affect == Affect::Urgent)
        .count() as u64;
    let stats = host.agent_stats();

    let round = Round {
        seed,
        ticks: host.sim().tick(),
        tags,
        sight_ticks,
        agent_sight_ticks,
        scores,
        margin,
        draw: host
            .sim()
            .outcome()
            .is_some_and(|outcome| outcome.winner.is_none()),
        end_tick,
        decisions: stats.map_or(0, |stats| stats.decisions),
        asks: stats.map_or(0, |stats| stats.thinks),
        lines: records.len() as u64,
        urgent_lines,
        positions,
    };

    let asks = {
        let guard = log.lock().expect("the watcher's log");
        guard.clone()
    };
    (round, asks, records)
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

/// The measurement: the rounds, and what the doctrine did in them.
#[test]
#[ignore = "needs the Thor at 192.168.0.78; run with --release --ignored --nocapture"]
fn engagement_on_thor() {
    let dir = harness_dir();
    let harness = Harness::open(&dir).unwrap_or_else(|err| panic!("the harness opens: {err}"));
    let reflex = harness
        .reflexes
        .get("default")
        .cloned()
        .unwrap_or_else(|| DEFAULT_REFLEX.to_owned());

    let mut rounds: Vec<Round> = Vec::new();
    let mut asks: Vec<Ask> = Vec::new();

    for seed in seeds() {
        let (round, seed_asks, _records) = play(seed, &harness, &reflex);
        println!(
            "round {seed}: {} ticks, {} tags, {} sight ticks, scores {:?} (margin {}), {} decisions, {} asks, {} lines ({} urgent)",
            round.ticks,
            round.tags,
            round.sight_ticks,
            round.scores,
            round.margin,
            round.decisions,
            round.asks,
            round.lines,
            round.urgent_lines,
        );
        println!("  positions: {:?}", round.positions);
        rounds.push(round);
        asks.extend(seed_asks);
    }

    // The raw record: one line per ask, for the write-up.
    if let Ok(path) = std::env::var("ENGAGEMENT_DUMP") {
        let mut text = String::new();
        for ask in &asks {
            let facts = Facts::of(&ask.state);
            let branch = facts.branch();
            let choice = ask.choice();
            let agrees = choice.as_deref() == Some(branch.option(facts.nearest_unheld).as_str());
            let line = json!({
                "agent": ask.agent,
                "pack": if ask.pack == Pack::Move { "move" } else { "talk" },
                "branch": branch.as_str(),
                "doctrine_option": branch.option(facts.nearest_unheld),
                "model_option": choice,
                "agrees": agrees,
                "confidence": ask.confidence(),
                "probs": ask.probs,
                "intention": ask.intention,
                "latency_ms": ask.latency_ms,
                "facts": ask.state,
            });
            text.push_str(&line.to_string());
            text.push('\n');
        }
        if let Err(err) = std::fs::write(&path, text) {
            eprintln!("the dump did not write: {err}");
        } else {
            println!("the raw record is at {path}");
        }
    }

    println!();
    println!("# the engagement, measured");
    println!();
    println!("harness: {}", dir.display());
    println!(
        "backend: {}",
        std::env::var("ENGAGEMENT_BACKEND").unwrap_or_else(|_| "thor".to_owned())
    );
    println!();

    // ---------------------------------------------------------------- rounds
    println!("## the rounds");
    println!();
    println!("| seed | ticks | end tick | tags | sight ticks | agent sight ticks | scores | margin | draw | decisions | asks | lines | urgent |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    for round in &rounds {
        println!(
            "| {} | {} | {} | {} | {} | {} | {:?} | {} | {} | {} | {} | {} | {} |",
            round.seed,
            round.ticks,
            round.end_tick,
            round.tags,
            round.sight_ticks,
            round.agent_sight_ticks,
            round.scores,
            round.margin,
            round.draw,
            round.decisions,
            round.asks,
            round.lines,
            round.urgent_lines,
        );
    }
    println!();

    // ----------------------------------------------------------------- gate
    let move_asks: Vec<&Ask> = asks
        .iter()
        .filter(|ask| ask.pack == Pack::Move && !ask.probs.is_empty())
        .collect();

    println!("## the move gate: the doctrine's branch against the model's choice");
    println!();
    println!("- move asks answered: {}", move_asks.len());

    let mut by_branch: BTreeMap<Branch, (u64, u64)> = BTreeMap::new();
    for ask in &move_asks {
        let facts = Facts::of(&ask.state);
        let branch = facts.branch();
        let choice = ask.choice();
        let agrees = choice.as_deref() == Some(branch.option(facts.nearest_unheld).as_str());
        let entry = by_branch.entry(branch).or_default();
        entry.0 += 1;
        if agrees {
            entry.1 += 1;
        }
    }
    println!();
    println!("| branch | asks | the model took the branch's option |");
    println!("|---|---|---|");
    for (branch, (total, agrees)) in &by_branch {
        println!("| {} | {total} | {agrees} |", branch.as_str());
    }
    let total: u64 = by_branch.values().map(|(total, _)| total).sum();
    let agrees: u64 = by_branch.values().map(|(_, agrees)| agrees).sum();
    if total > 0 {
        println!();
        println!(
            "- the model followed the doctrine's branch on {} of {total} move asks ({:.1}%)",
            agrees,
            100.0 * agrees as f64 / total as f64
        );
    }
    println!();

    // What the model chose, by option.
    let mut by_option: BTreeMap<String, u64> = BTreeMap::new();
    for ask in &move_asks {
        if let Some(choice) = ask.choice() {
            *by_option.entry(choice).or_default() += 1;
        }
    }
    println!("## what the model chose");
    println!();
    println!("| option | times |");
    println!("|---|---|");
    for (option, count) in &by_option {
        println!("| {option} | {count} |");
    }
    println!();

    // -------------------------------------------------------------- urgency
    let talk_asks: Vec<&Ask> = asks
        .iter()
        .filter(|ask| ask.pack == Pack::Talk && ask.intention.is_some())
        .collect();
    let urgent_scores = talk_asks
        .iter()
        .filter(|ask| ask.intention.is_some_and(|score| score.round() >= 2.0))
        .count();
    println!("## the urgency gate");
    println!();
    println!("- talk asks answered: {}", talk_asks.len());
    println!("- the gate said urgent on {urgent_scores} of them",);
    let urgent_lines: u64 = rounds.iter().map(|round| round.urgent_lines).sum();
    let lines: u64 = rounds.iter().map(|round| round.lines).sum();
    println!("- lines delivered: {lines}, of them urgent: {urgent_lines}");
    println!();
}

/// Which tier the shipped policy picks for a calm moment, and which backend
/// it names.
///
/// The shipped policy names two model tiers: the fast one, on the Thor's
/// d1-2B, and a deep one, on a backend called `jev`. A tier that has never
/// been measured costs zero ticks, so it always "fits" the budget — and it
/// is the better-quality tier, so it is the one picked. The ask then finds no
/// backend by that name and is refused, and the refusal is counted as the
/// timeout it cost (D26 — a refusal is an outcome, never a guess). The next
/// pick is the fast tier, which answers.
#[test]
fn which_tier_answers() {
    let harness = Harness::open(&harness_dir()).expect("the harness opens");
    let policy = lm_agents::mind::Policy::for_harness(&harness.policy_toml);
    for tier in policy.tiers.tiers() {
        println!(
            "tier {} backend {} quality {}",
            tier.name(),
            tier.backend(),
            tier.quality()
        );
    }
    println!(
        "calm move pick: {:?}",
        policy
            .pick_tier(false, Pack::Move, 1, 30)
            .map(|tier| tier.name())
    );
    println!(
        "urgent move pick: {:?}",
        policy
            .pick_tier(true, Pack::Move, 1, 30)
            .map(|tier| tier.name())
    );
}

/// The facts parser reads the facts the builder writes, and the branch it
/// selects is the branch of the shipped doctrine.
///
/// The texts are the facts of real rounds (the Thor's own), so a change in
/// the builder's wording shows up here.
#[test]
fn the_facts_parser_reads_the_builders_words() {
    // A squad-0 agent at its spawn: nothing held, no enemy in sight, three
    // allies near. The doctrine's third branch: go to the nearest beacon the
    // squad does not hold, which the facts name as beacon 2.
    let at_spawn = "You are agent 0, the leader, of squad 0 at column 2, row 2 (row 0 is north). \
Beacon 0 at column 64, row 63 is unheld; it is not the nearest beacon your squad does not hold; \
a walking path from you is 74 steps long to the east. \
Beacon 1 at column 63, row 64 is unheld; it is not the nearest beacon your squad does not hold; \
a walking path from you is 75 steps long to the south. \
Beacon 2 at column 32, row 63 is unheld; it is the nearest beacon your squad does not hold; \
a walking path from you is 61 steps long to the south. \
Beacon 3 at column 95, row 64 is unheld; it is not the nearest beacon your squad does not hold; \
a walking path from you is 95 steps long to the east. \
You are not standing on a beacon. You know of no fresh enemies. \
No enemy is within 6 cells of you. Ally 1 (member) is 2 cells away at column 4, row 2. \
Ally 2 (member) is 2 cells away at column 2, row 4. Ally 3 (member) is 2 cells away at column 4, row 4. \
You have 3 allies within 4 cells of you. You are not stunned.";
    let facts = Facts::of(at_spawn);
    assert_eq!(facts.near, 0);
    assert_eq!(facts.seeing, 0);
    assert_eq!(facts.allies, 3);
    assert!(!facts.on_held_beacon);
    assert_eq!(facts.nearest_unheld, Some(2));
    assert_eq!(facts.branch(), Branch::GoNearestUnheld);
    assert_eq!(
        facts.branch().option(facts.nearest_unheld),
        "go to beacon 2"
    );

    // A squad-1 agent standing on the beacon its squad holds, with no enemy
    // near. The doctrine's second branch: hold.
    let on_own_beacon = "You are agent 4, the leader, of squad 1 at column 95, row 64 (row 0 is north). \
Beacon 0 at column 64, row 63 is unheld; it is the nearest beacon your squad does not hold; \
a walking path from you is 31 steps long to the west. \
Beacon 1 at column 63, row 64 is unheld; it is not the nearest beacon your squad does not hold; \
a walking path from you is 32 steps long to the west. \
Beacon 2 at column 32, row 63 is held by enemy squad 0; it is not the nearest beacon your squad does not hold; \
a walking path from you is 63 steps long to the west. \
Beacon 3 at column 95, row 64 is held by your squad; you are standing on it. \
You are standing on beacon 3, which your squad holds. You know of no fresh enemies. \
No enemy is within 6 cells of you. Ally 5 (member) is 0 cells away at column 95, row 64. \
Ally 7 (member) is 0 cells away at column 95, row 64. \
You have 2 allies within 4 cells of you. You are not stunned.";
    let facts = Facts::of(on_own_beacon);
    assert!(facts.on_held_beacon);
    assert_eq!(facts.nearest_unheld, Some(0));
    assert_eq!(facts.branch(), Branch::HoldOnBeacon);
    assert_eq!(facts.branch().option(facts.nearest_unheld), "hold");

    // An agent that sees two enemies and has one ally: outnumbered, so the
    // doctrine's first branch: retreat.
    let outnumbered = "You are agent 2, the member, of squad 0 at column 60, row 60 (row 0 is north). \
Beacon 0 at column 64, row 63 is unheld; it is the nearest beacon your squad does not hold; \
a walking path from you is 4 steps long to the east. \
Beacon 1 at column 63, row 64 is unheld; it is not the nearest beacon your squad does not hold; \
a walking path from you is 5 steps long to the east. \
Beacon 2 at column 32, row 63 is held by your squad; it is not the nearest beacon your squad does not hold; \
a walking path from you is 28 steps long to the west. \
Beacon 3 at column 95, row 64 is held by enemy squad 1; it is not the nearest beacon your squad does not hold; \
a walking path from you is 35 steps long to the east. \
You are not standing on a beacon. \
Enemy 5, last seen 1 tick ago at column 61, row 61, is 1 cells away and can see you (clear line of sight). \
Enemy 6, last seen 1 tick ago at column 59, row 59, is 2 cells away and can see you (clear line of sight). \
2 enemies are within 6 cells of you, and all of them can see you. \
Ally 0 (leader) is 2 cells away at column 58, row 60. \
You have 1 allies within 4 cells of you. You are not stunned.";
    let facts = Facts::of(outnumbered);
    assert_eq!(facts.near, 2);
    assert_eq!(facts.seeing, 2);
    assert_eq!(facts.allies, 1);
    assert_eq!(facts.branch(), Branch::Retreat);
    assert_eq!(facts.branch().option(facts.nearest_unheld), "retreat");
}
