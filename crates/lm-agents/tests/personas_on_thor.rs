//! The personas A/B: the default squad against a squad that thinks with a
//! personality, measured on the real Thor.
//!
//! ## What is measured
//!
//! Every round is played the way the game plays it: the host loop, the
//! agents, the least-use-first queue, the sequencer. The only thing that is
//! not the shipped code is the model behind the tier — the Thor's own
//! **d1-2B**, on the LAN — and the persona on one of the squads.
//!
//! The round's own two squads are the A/B: **squad 0 thinks with the
//! shipped doctrine and squad 1 thinks with the persona**, on the same map,
//! on the same seed, at the same time. The baseline is the same round with
//! no persona at all, so the yardstick is known.
//!
//! For every round:
//!
//! - **the score**: the two squads' scores, and which side won;
//! - **contact**: the ticks on which some agent had an enemy in sight, and
//!   the tags the ruleset made — by squad;
//! - **the lines**: how many were spoken, by squad, and how **distinct**
//!   they are — the words themselves, so the question "do the personas
//!   sound different?" is asked of the words and not of the plans;
//! - **refusals**: every refusal of the round, by kind;
//! - **the patch**: how many biases the persona applied, and how many it
//!   could not.
//!
//! ## The load
//!
//! One round is in flight at a time: the rounds are played one after
//! another, never two at once, so the Thor is asked for one round's worth
//! of traffic and no more. The load on the box is printed with the
//! results — a number measured on a busy Thor is worth nothing.
//!
//! ## The command
//!
//! ```text
//! cargo test --release -p lm-agents --test personas_on_thor -- --ignored --nocapture
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lm_agents::config::AgentsConfig;
use lm_agents::facts::Facts;
use lm_agents::harness::Harness;
use lm_agents::persona::Persona;
use lm_agents::sysone::{Answers, EscalatingBackend, Refusal, SysOne, ThorBackend};
use lm_agents::utter::{LlmRealizer, Realizer, ThorLlm, UtterPlan};
use lm_agents::Agents;
use lm_core::header::Header;
use lm_core::talk::TalkRecord;
use lm_core::utterance::{Addressee, Affect, Intent, SpeechAct, Subject};
use lm_core::view::ScoreView;
use lm_core::world::los;
use lm_host_model::HostLoop;
use serde_json::Value;

/// The shipped reflex, compiled into the test binary.
const DEFAULT_REFLEX: &str = include_str!("../../../assets/reflexes/default.rhai");

/// The ticks a round is played for: a hundred seconds at 30 Hz.
const TICKS: u64 = 3_000;

/// The seeds the measurement plays.
const SEEDS: [u64; 3] = [1, 2, 3];

/// The personas the measurement plays, one squad each.
const PERSONAS: [&str; 6] = [
    "anxious-grunt",
    "chatterbox",
    "literal-minded-rule-follower",
    "middle-manager-leader",
    "overconfident-sprinter",
    "quiet-sharpshooter",
];

/// The squad that thinks with the persona: squad 1, agents 4 to 7.
const PERSONA_SQUAD: u8 = 1;

/// The enemy's own reach, in cells: the facts builder's ENEMY_NEAR_CELLS.
const ENEMY_NEAR_CELLS: i32 = 6;

/// The root of the repository, where the assets live.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The harness directory the measurement plays.
fn harness_dir() -> PathBuf {
    let name =
        std::env::var("PERSONAS_HARNESS").unwrap_or_else(|_| "assets/harness/default".to_owned());
    repo_root().join(name)
}

/// The persona library the measurement plays.
fn personas_dir() -> PathBuf {
    repo_root().join("assets/harness/personas")
}

/// The ticks a round is played for, if the caller says otherwise.
fn ticks() -> u64 {
    std::env::var("PERSONAS_TICKS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(TICKS)
}

/// The seeds the measurement plays, if the caller says otherwise.
fn seeds() -> Vec<u64> {
    match std::env::var("PERSONAS_SEEDS") {
        Ok(value) => value
            .split(',')
            .filter_map(|seed| seed.trim().parse::<u64>().ok())
            .collect(),
        Err(_) => SEEDS.to_vec(),
    }
}

/// The personas the measurement plays, if the caller says otherwise.
fn personas() -> Vec<String> {
    match std::env::var("PERSONAS_LIST") {
        Ok(value) => value
            .split(',')
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty())
            .collect(),
        Err(_) => PERSONAS.iter().map(|name| (*name).to_owned()).collect(),
    }
}

/// One line, as the words it was made of.
///
/// The wording is the notation: a header line and one line per word. The
/// words are what a voice is, so the words are what is compared.
fn words_of(text: &str) -> String {
    let mut words = Vec::new();
    for line in text.lines().skip(1) {
        let trimmed = line.trim();
        let Some(word) = trimmed.split_whitespace().next() else {
            continue;
        };
        words.push(word.to_lowercase());
    }
    words.join(" ")
}

/// The watcher: the Thor behind a log.
struct Watcher {
    inner: Box<dyn SysOne + Send + Sync>,
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
        self.inner
            .ask_with_escalation(state, questions, deadline, escalation_deadline)
    }

    fn name(&self) -> &str {
        self.inner.name()
    }
}

/// What one round did, measured.
#[derive(Clone, Debug, Default)]
struct Round {
    /// The seed the round ran on.
    seed: u64,
    /// The ticks the round played.
    ticks: u64,
    /// The persona squad's persona, if it had one.
    persona: Option<String>,
    /// The round's final scores, in squad order.
    scores: Vec<i64>,
    /// The gap between the squads' scores.
    margin: i64,
    /// Whether the round ended in a draw.
    draw: bool,
    /// The ticks on which some agent had an enemy in sight, by squad.
    sight_ticks: [u64; 2],
    /// The tags the ruleset made, by the squad of the agent that made them.
    tags: [u64; 2],
    /// The lines that were spoken, by squad.
    lines: [u64; 2],
    /// The distinct lines of each squad: the words, lowercased.
    distinct: [BTreeSet<String>; 2],
    /// The lines themselves, by squad, so a reader can hear them.
    said: [Vec<String>; 2],
    /// The lines of each squad by what they were for: the intent mix.
    intents: [BTreeMap<String, u64>; 2],
    /// The lines of each squad by what they were: the gesture mix.
    acts: [BTreeMap<String, u64>; 2],
    /// The refusals the round saw, by kind.
    refusal_kinds: Vec<(String, u64)>,
    /// The decisions the agents' answers became.
    decisions: u64,
    /// The questions the round asked.
    asks: u64,
    /// The biases the persona applied.
    biases: u64,
    /// The biases the persona could not apply.
    skipped: u64,
    /// Where every agent stood at the end of the round: the agent, its
    /// squad, and its cell.
    positions: Vec<(u16, u8, i64, i64)>,
}

impl Round {
    /// The persona squad's score.
    fn persona_score(&self) -> i64 {
        self.scores
            .get(usize::from(PERSONA_SQUAD))
            .copied()
            .unwrap_or_default()
    }

    /// The default squad's score.
    fn default_score(&self) -> i64 {
        self.scores
            .get(usize::from(1 - PERSONA_SQUAD))
            .copied()
            .unwrap_or_default()
    }

    /// Did the persona squad win the round?
    fn persona_won(&self) -> bool {
        self.persona_score() > self.default_score()
    }

    /// How many refusals the round saw, of every kind.
    fn refusals(&self) -> u64 {
        self.refusal_kinds.iter().map(|(_, count)| count).sum()
    }
}

/// Play one round on the Thor, with `persona` on the persona squad, and
/// measure it.
fn play(seed: u64, harness: &Harness, reflex: &str, persona: Option<&str>) -> Round {
    let header = Header::default_round(seed, reflex);
    // The persona library is where the assets are, and the persona is the
    // squad's: every agent of the persona squad thinks with it.
    let mut cfg = AgentsConfig {
        personas_dir: personas_dir().display().to_string(),
        ..AgentsConfig::default()
    };
    if let Some(persona) = persona {
        for entry in &header.roster {
            if entry.squad == PERSONA_SQUAD {
                cfg.personas
                    .insert(entry.agent.to_string(), persona.to_owned());
            }
        }
    }

    let watcher = Watcher {
        inner: Box::new(thor_backends(&cfg)),
    };
    let backends = BTreeMap::from([(
        "thor-d1-2b".to_owned(),
        Box::new(watcher) as Box<dyn SysOne + Send + Sync>,
    )]);

    let mut host = HostLoop::new(header.clone()).expect("the round starts");
    let mut agents = Agents::new(&cfg, harness, &header, backends);

    // The words, as they are worded: the line is telemetry, and the
    // measurement of the voice reads it.
    let words: Arc<Mutex<Vec<(u16, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&words);
    agents.watch_lines(Arc::new(move |agent, worded| {
        let words = words_of(&worded.text);
        if words.is_empty() {
            return;
        }
        sink.lock().expect("the words").push((agent, words));
    }));

    host.attach_agents(agents);

    let period = Duration::from_millis(1000 / u64::from(header.params.tick_hz));
    let mut sight_ticks = [0u64; 2];
    let mut tags = [0u64; 2];

    for _ in 0..ticks() {
        std::thread::sleep(period);

        let _tick = host.sim().tick();

        // What every agent can see on this tick, by squad.
        for entry in &header.roster {
            let view = host.sim().view(entry.agent);
            let (near, seeing) = threat_of(&view, host.sim().world(), cfg.stale_enemy_ticks);
            if seeing > 0 {
                let squad = usize::from(entry.squad);
                if squad < sight_ticks.len() {
                    sight_ticks[squad] += 1;
                }
            }
            let _ = near;
        }

        let out = host.tick();
        for event in &out.events {
            if let lm_core::rules::Event::Tagged { by, .. } = event {
                if let Some(entry) = header.roster.iter().find(|entry| entry.agent == *by) {
                    let squad = usize::from(entry.squad);
                    if squad < tags.len() {
                        tags[squad] += 1;
                    }
                }
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
    let positions = header
        .roster
        .iter()
        .map(|entry| {
            let view = host.sim().view(entry.agent);
            (
                entry.agent,
                entry.squad,
                i64::from(view.me.cell.x),
                i64::from(view.me.cell.y),
            )
        })
        .collect();
    let records: Vec<TalkRecord> = host.sim().talk_records().to_vec();
    let stats = host.agent_stats();

    // The lines, by squad, from the records the sim kept.
    let mut lines = [0u64; 2];
    for record in &records {
        let squad = usize::from(record.squad);
        if squad < lines.len() {
            lines[squad] += 1;
        }
    }
    // What the lines were for and what they were: the persona's bias moves
    // the gates, so the mix of intents and gestures is where the personality
    // shows even when the same model words both squads.
    let mut intents = [BTreeMap::new(), BTreeMap::new()];
    let mut acts = [BTreeMap::new(), BTreeMap::new()];
    for record in &records {
        let squad = usize::from(record.squad);
        if squad >= intents.len() {
            continue;
        }
        *intents[squad]
            .entry(format!("{:?}", record.intent))
            .or_default() += 1;
        *acts[squad].entry(format!("{:?}", record.act)).or_default() += 1;
    }

    // The distinct words, by squad, from the lines the minds worded.
    let mut distinct = [BTreeSet::new(), BTreeSet::new()];
    let mut said: [Vec<String>; 2] = [Vec::new(), Vec::new()];
    {
        let guard = words.lock().expect("the words");
        for (agent, words) in guard.iter() {
            let Some(entry) = header.roster.iter().find(|entry| entry.agent == *agent) else {
                continue;
            };
            let squad = usize::from(entry.squad);
            if squad < distinct.len() {
                distinct[squad].insert(words.clone());
                said[squad].push(words.clone());
            }
        }
    }

    let persona_log = host
        .agents()
        .map(|agents| agents.persona_log())
        .unwrap_or_default();
    let persona_skipped = host
        .agents()
        .map(|agents| agents.persona_skipped())
        .unwrap_or_default();
    let biases = persona_log.values().map(Vec::len).sum::<usize>() as u64;
    let skipped = persona_skipped.values().map(Vec::len).sum::<usize>() as u64;

    Round {
        seed,
        ticks: host.sim().tick(),
        persona: persona.map(str::to_owned),
        scores,
        margin,
        draw: host
            .sim()
            .outcome()
            .is_some_and(|outcome| outcome.winner.is_none()),
        sight_ticks,
        tags,
        lines,
        distinct,
        said,
        intents,
        acts,
        refusal_kinds: stats.map_or_else(Vec::new, |stats| {
            lm_core::refusal::RefusalKind::ALL
                .iter()
                .filter_map(|kind| {
                    let count = stats.refusal_counts.get(*kind);
                    (count > 0).then(|| (kind.to_string(), count))
                })
                .collect()
        }),
        decisions: stats.map_or(0, |stats| stats.decisions),
        asks: stats.map_or(0, |stats| stats.thinks),
        biases,
        skipped,
        positions,
    }
}

/// The Thor's own models behind the tiers, with the escalation fence.
fn thor_backends(cfg: &AgentsConfig) -> EscalatingBackend {
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
    EscalatingBackend::new(Box::new(primary), Box::new(second), cfg.escalate_below)
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

/// How many of the persona squad's lines the default squad also said.
///
/// A number near one means the two squads sound the same; a number near
/// zero means the persona has its own voice.
fn overlap(round: &Round) -> f64 {
    let persona = &round.distinct[usize::from(PERSONA_SQUAD)];
    let default = &round.distinct[usize::from(1 - PERSONA_SQUAD)];
    if persona.is_empty() || default.is_empty() {
        return 0.0;
    }
    let shared = persona.intersection(default).count();
    shared as f64 / persona.len() as f64
}

/// The measurement: the rounds, and what the personas did in them.
#[test]
#[ignore = "needs the Thor at 192.168.0.78; run with --release --ignored --nocapture"]
fn personas_on_thor() {
    let dir = harness_dir();
    let harness = Harness::open(&dir).unwrap_or_else(|err| panic!("the harness opens: {err}"));
    let reflex = harness
        .reflexes
        .get("default")
        .cloned()
        .unwrap_or_else(|| DEFAULT_REFLEX.to_owned());

    // The baseline: the same rounds with no persona at all.
    let mut baseline: Vec<Round> = Vec::new();
    for seed in seeds() {
        let round = play(seed, &harness, &reflex, None);
        println!(
            "baseline {seed}: {} ticks, scores {:?} (margin {}), tags {:?}, sight {:?}, lines {:?}, {} distinct lines, {} decisions, {} refusals {:?}",
            round.ticks,
            round.scores,
            round.margin,
            round.tags,
            round.sight_ticks,
            round.lines,
            round.distinct[0].len() + round.distinct[1].len(),
            round.decisions,
            round.refusals(),
            round.refusal_kinds,
        );
        baseline.push(round);
    }

    let mut rounds: Vec<Round> = Vec::new();
    for persona in personas() {
        for seed in seeds() {
            let round = play(seed, &harness, &reflex, Some(&persona));
            println!(
                "{persona} {seed}: scores {:?} (margin {}, draw {}), tags {:?}, sight {:?}, lines {:?}, {} distinct lines, overlap {:.2}, {} asks, {} decisions, {} biases applied, {} skipped, {} refusals {:?}",
                round.scores,
                round.margin,
                round.draw,
                round.tags,
                round.sight_ticks,
                round.lines,
                round.distinct[usize::from(PERSONA_SQUAD)].len(),
                overlap(&round),
                round.asks,
                round.decisions,
                round.biases,
                round.skipped,
                round.refusals(),
                round.refusal_kinds,
            );
            if std::env::var("PERSONAS_SHOW_LINES").is_ok() {
                for squad in 0..2 {
                    println!("  squad {squad} said:");
                    for line in &round.said[squad] {
                        println!("    {line}");
                    }
                }
            }
            rounds.push(round);
        }
    }

    println!();
    println!("# the personas, measured");
    println!();
    println!("harness: {}", dir.display());
    println!("personas: {}", personas_dir().display());
    println!(
        "backend: {}",
        std::env::var("PERSONAS_BACKEND").unwrap_or_else(|_| "thor".to_owned())
    );
    println!(
        "load: {}",
        std::fs::read_to_string("/proc/loadavg").unwrap_or_default()
    );
    println!();

    // ------------------------------------------------------------- baseline
    println!("## the baseline: two squads with no persona");
    println!();
    println!("| seed | scores | margin | draw | tags | sight ticks | lines | distinct lines |");
    println!("|---|---|---|---|---|---|---|---|");
    for round in &baseline {
        println!(
            "| {} | {:?} | {} | {} | {:?} | {:?} | {:?} | {} |",
            round.seed,
            round.scores,
            round.margin,
            round.draw,
            round.tags,
            round.sight_ticks,
            round.lines,
            round.distinct[0].len() + round.distinct[1].len(),
        );
    }
    let baseline_wins = baseline
        .iter()
        .filter(|round| round.persona_score() > round.default_score())
        .count();
    println!();
    println!(
        "- squad 1 took {baseline_wins} of {} baseline rounds (a draw is a draw)",
        baseline.len()
    );
    println!();

    // -------------------------------------------------------------- personas
    println!("## the personas against the default squad");
    println!();
    println!("| persona | rounds | won | drew | lost | score (persona/default) | tags (persona/default) | sight ticks | lines (persona/default) | distinct lines | overlap | biases | refusals |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    for persona in personas() {
        let rounds: Vec<&Round> = rounds
            .iter()
            .filter(|round| round.persona.as_deref() == Some(persona.as_str()))
            .collect();
        if rounds.is_empty() {
            continue;
        }
        let won = rounds.iter().filter(|round| round.persona_won()).count();
        let lost = rounds
            .iter()
            .filter(|round| round.persona_score() < round.default_score())
            .count();
        let drew = rounds.len() - won - lost;
        let persona_score: i64 = rounds.iter().map(|round| round.persona_score()).sum();
        let default_score: i64 = rounds.iter().map(|round| round.default_score()).sum();
        let persona_tags: u64 = rounds
            .iter()
            .map(|round| round.tags[usize::from(PERSONA_SQUAD)])
            .sum();
        let default_tags: u64 = rounds
            .iter()
            .map(|round| round.tags[usize::from(1 - PERSONA_SQUAD)])
            .sum();
        let persona_sight: u64 = rounds
            .iter()
            .map(|round| round.sight_ticks[usize::from(PERSONA_SQUAD)])
            .sum();
        let default_sight: u64 = rounds
            .iter()
            .map(|round| round.sight_ticks[usize::from(1 - PERSONA_SQUAD)])
            .sum();
        let persona_lines: u64 = rounds
            .iter()
            .map(|round| round.lines[usize::from(PERSONA_SQUAD)])
            .sum();
        let default_lines: u64 = rounds
            .iter()
            .map(|round| round.lines[usize::from(1 - PERSONA_SQUAD)])
            .sum();
        let distinct: usize = rounds
            .iter()
            .map(|round| round.distinct[usize::from(PERSONA_SQUAD)].len())
            .sum();
        let overlaps: f64 =
            rounds.iter().map(|round| overlap(round)).sum::<f64>() / rounds.len() as f64;
        let biases: u64 = rounds.iter().map(|round| round.biases).sum();
        let refusals: u64 = rounds.iter().map(|round| round.refusals()).sum();
        println!(
            "| {persona} | {} | {won} | {drew} | {lost} | {persona_score}/{default_score} | {persona_tags}/{default_tags} | {persona_sight}/{default_sight} | {persona_lines}/{default_lines} | {distinct} | {overlaps:.2} | {biases} | {refusals} |",
            rounds.len(),
        );
    }
    println!();

    // ------------------------------------------------------------- collapse
    println!("## what the personas say: the intent mix of the lines");
    println!();
    println!("The intent is what the line is for, and it is the gate the persona");
    println!("biases most directly. The persona squad is squad 1.");
    println!();
    for persona in personas() {
        let rounds: Vec<&Round> = rounds
            .iter()
            .filter(|round| round.persona.as_deref() == Some(persona.as_str()))
            .collect();
        if rounds.is_empty() {
            continue;
        }
        let mut persona_intents: BTreeMap<String, u64> = BTreeMap::new();
        let mut default_intents: BTreeMap<String, u64> = BTreeMap::new();
        let mut persona_acts: BTreeMap<String, u64> = BTreeMap::new();
        let mut default_acts: BTreeMap<String, u64> = BTreeMap::new();
        for round in &rounds {
            for (intent, count) in &round.intents[usize::from(PERSONA_SQUAD)] {
                *persona_intents.entry(intent.clone()).or_default() += count;
            }
            for (intent, count) in &round.intents[usize::from(1 - PERSONA_SQUAD)] {
                *default_intents.entry(intent.clone()).or_default() += count;
            }
            for (act, count) in &round.acts[usize::from(PERSONA_SQUAD)] {
                *persona_acts.entry(act.clone()).or_default() += count;
            }
            for (act, count) in &round.acts[usize::from(1 - PERSONA_SQUAD)] {
                *default_acts.entry(act.clone()).or_default() += count;
            }
        }
        println!("### {persona}");
        println!();
        println!("| intent | persona squad | default squad |");
        println!("|---|---|---|");
        let mut keys: BTreeSet<&String> = persona_intents.keys().collect();
        keys.extend(default_intents.keys());
        for key in keys {
            println!(
                "| {key} | {} | {} |",
                persona_intents.get(key).copied().unwrap_or(0),
                default_intents.get(key).copied().unwrap_or(0)
            );
        }
        println!();
        println!("| gesture | persona squad | default squad |");
        println!("|---|---|---|");
        let mut acts: BTreeSet<&String> = persona_acts.keys().collect();
        acts.extend(default_acts.keys());
        for key in acts {
            println!(
                "| {key} | {} | {} |",
                persona_acts.get(key).copied().unwrap_or(0),
                default_acts.get(key).copied().unwrap_or(0)
            );
        }
        println!();
    }

    // ------------------------------------------------------------- collapse
    println!("## did any persona collapse its squad?");
    println!("| persona | rounds | tags | sight ticks | lines | score | where they ended |");
    println!("|---|---|---|---|---|---|---|");
    for persona in personas() {
        let rounds: Vec<&Round> = rounds
            .iter()
            .filter(|round| round.persona.as_deref() == Some(persona.as_str()))
            .collect();
        if rounds.is_empty() {
            continue;
        }
        let tags: u64 = rounds
            .iter()
            .map(|round| round.tags[usize::from(PERSONA_SQUAD)])
            .sum();
        let sight: u64 = rounds
            .iter()
            .map(|round| round.sight_ticks[usize::from(PERSONA_SQUAD)])
            .sum();
        let lines: u64 = rounds
            .iter()
            .map(|round| round.lines[usize::from(PERSONA_SQUAD)])
            .sum();
        let score: i64 = rounds.iter().map(|round| round.persona_score()).sum();
        // Where the persona squad stood at the end: a squad that never left
        // its spawn is a squad that did not play.
        let mut cells: BTreeSet<(i64, i64)> = BTreeSet::new();
        for round in &rounds {
            for (_, squad, x, y) in &round.positions {
                if *squad == PERSONA_SQUAD {
                    cells.insert((*x, *y));
                }
            }
        }
        println!(
            "| {persona} | {} | {tags} | {sight} | {lines} | {score} | {} distinct cells |",
            rounds.len(),
            cells.len()
        );
    }
    println!();
}

/// The persona's effect on the **voice**, measured on its own.
///
/// The game's talk is too sparse to answer "do the personas sound
/// different?" — a hundred seconds of play is a handful of lines, and
/// under load it is none at all. So the wording is measured where it is
/// deterministic: the same plan, worded by the same model, with and
/// without the persona's marks.
///
/// This is the same path the game takes: the marks are in the prompt, and
/// the model writes the line.
#[test]
#[ignore = "needs the Thor's wording model; run with --release --ignored --nocapture"]
fn the_personas_voice_on_thor() {
    let cfg = AgentsConfig::default();
    let dir = personas_dir();
    let plan = UtterPlan {
        act: SpeechAct::Callout,
        addressee: Addressee::Squad,
        intent: Intent::Report,
        affect: Affect::Calm,
        subject: Subject::Beacon(2),
        should_speak: true,
    };
    // The wording model does not read the facts -- the plan is the whole
    // of it -- but the realizer's shape asks for them.
    let facts = Facts {
        text: "i am at 0 0. beacon 2 is held by nobody.".to_owned(),
        key: 0,
        fields: BTreeMap::new(),
    };

    let llm = |marks: Vec<String>| -> Box<dyn Realizer> {
        Box::new(
            LlmRealizer::new(
                Box::new(ThorLlm::new(
                    &cfg.thor_base,
                    cfg.utter.llm_port,
                    &cfg.utter.llm_model,
                    Duration::from_millis(u64::from(cfg.utter.deadline_ms) * 4),
                )),
                Duration::from_millis(u64::from(cfg.utter.deadline_ms) * 4),
            )
            .with_marks(marks),
        )
    };

    println!("# the personas' voices: the same plan, worded with and without the marks\n");
    let plain = llm(Vec::new());
    let default_line = plain
        .realize(&plan, &facts)
        .map(|worded| words_of(&worded.text))
        .unwrap_or_else(|error| format!("(the model did not answer: {error})"));
    println!("the shipped voice: {default_line}\n");

    let mut same = 0;
    let mut total = 0;
    for persona in personas() {
        let Ok(persona) = Persona::load(&dir, &persona) else {
            continue;
        };
        let realizer = llm(persona.marks.clone());
        let line = realizer
            .realize(&plan, &facts)
            .map(|worded| words_of(&worded.text))
            .unwrap_or_else(|error| format!("(the model did not answer: {error})"));
        total += 1;
        if line == default_line {
            same += 1;
        }
        println!("### {} ({:?})\n", persona.name, persona.marks);
        println!("- shipped: {default_line}");
        println!("- {}: {line}", persona.name);
        println!("- barks: {}", persona.barks.join(" / "));
        println!();
    }
    println!("{same} of {total} personas worded the line exactly like the shipped voice.");
}
