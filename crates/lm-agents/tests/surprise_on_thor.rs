//! The surprise signal on the real Thor: where the minds struggled, and
//! whether the top of the table makes sense.
//!
//! The between-round learning flow proposes changes and tests them under a
//! sealed prediction, but nothing told it **where** the agents struggle. The
//! surprise signal is the cheap answer: per situation bucket (a coarse key
//! of the state an ask was made in), how often the minds were unsure
//! (D72), refused (D26) or escalated (D71). A situation the models keep
//! failing in is a situation a gate, a fact or a lexicon entry is missing
//! for (D47: the trainer's output is questions).
//!
//! This is the measurement of that signal on the Thor's own **d1-2B**: the
//! round is the shipped one, played the way the game plays it — the host
//! loop, the agents, the least-use-first queue, the sequencer — and the
//! table is the one the round's own agents counted
//! ([`lm_agents::surprise`]), read once the round is over.
//!
//! ## The two configurations
//!
//! The shipped rounds rarely fight (the squads hold their own beacons and
//! the enemies never meet), so the round is played two ways:
//!
//! - **shipped** (the default): the d1-2B answers both packs — the game as
//!   it ships;
//! - **split** (`SURPRISE_BACKEND=split`): the stub answers the move pack
//!   (the stub's play is what makes the squads meet, so the round has
//!   fights to be surprised about) and the d1-2B the talk pack.
//!
//! The write-up of the numbers is `tests/surprise_on_thor.md`.
//!
//! ## The command
//!
//! ```text
//! cargo test --release -p lm-agents --test surprise_on_thor -- --ignored --nocapture
//! ```

use std::collections::BTreeMap;
use std::time::Duration;

use lm_agents::config::AgentsConfig;
use lm_agents::surprise::{RoundSurprise, SituationCounts, MIN_ASKS};
use lm_agents::sysone::{EscalatingBackend, StubBackend, SysOne, ThorBackend};
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

/// The Thor's d1-2B, with the second opinion the escalation asks for (D71).
fn escalating_thor(cfg: &AgentsConfig) -> Box<dyn SysOne + Send + Sync> {
    let port = cfg.ports["d1-2b"];
    let primary = ThorBackend::new(&cfg.thor_base, port, "d1-2b")
        // The shipped floor (D72): the d1-2B's own calibration, the number
        // the round itself plays with. The surprise table counts what the
        // round's own rule did, not what a zero floor would have shown.
        .with_min_confidence(
            cfg.min_confidence
                .get("thor-d1-2b")
                .copied()
                .unwrap_or(0.30),
        )
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
    Box::new(EscalatingBackend::new(
        Box::new(primary),
        Box::new(second),
        cfg.escalate_below,
    ))
}

/// A backend that answers one pack with one model and the other pack with
/// another: the split configuration's own router (D70 made the packs two
/// requests, so they can be two models).
struct Split {
    /// The model the move pack is asked of.
    move_pack: Box<dyn SysOne + Send + Sync>,
    /// The model the talk pack is asked of.
    talk_pack: Box<dyn SysOne + Send + Sync>,
}

impl SysOne for Split {
    fn ask(
        &self,
        state: &str,
        questions: &Value,
        deadline: std::time::Instant,
    ) -> Result<lm_agents::sysone::Answers, lm_agents::sysone::Refusal> {
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
        deadline: std::time::Instant,
        escalation_deadline: std::time::Instant,
    ) -> Result<lm_agents::sysone::Answers, lm_agents::sysone::Refusal> {
        // The pack is told by the questions it carried (D70): the move pack
        // holds the move gate, the talk pack the gates a line is chosen by.
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

/// The shipped harness, read into memory.
fn default_harness() -> lm_agents::harness::Harness {
    lm_agents::harness::Harness {
        dir: std::path::PathBuf::from("assets/harness/default"),
        gates_toml: DEFAULT_GATES.to_owned(),
        policy_toml: DEFAULT_POLICY.to_owned(),
        phrasebook_toml: DEFAULT_PHRASEBOOK.to_owned(),
        facts_toml: String::new(),
        reflexes: BTreeMap::new(),
        commit: "content:test-harness".to_owned(),
    }
}

/// The configuration the measurement plays with, from the environment:
/// the backend (`shipped` by default, `split` for the fights), the Thor's
/// base URL (`localhost` when the measurement runs on the Thor itself), the
/// ticks and the seeds (a smoke run narrows them).
fn config() -> AgentsConfig {
    let mut cfg = AgentsConfig::default();
    if let Ok(base) = std::env::var("SURPRISE_THOR") {
        cfg.thor_base = base;
    }
    cfg
}

/// The backend of the run, by configuration name.
fn backend(cfg: &AgentsConfig) -> Box<dyn SysOne + Send + Sync> {
    match std::env::var("SURPRISE_BACKEND").as_deref() {
        Ok("split") => Box::new(Split {
            move_pack: Box::new(StubBackend::menu()),
            talk_pack: escalating_thor(cfg),
        }),
        _ => escalating_thor(cfg),
    }
}

/// The backends of the run, one per backend the shipped policy names —
/// the `lm voice` pattern: every non-reflex tier has to find its backend
/// here, or its asks are refused (`no backend named jev` was 48 of the
/// first run's refusals, all of them an artefact of wiring one backend
/// only).
///
/// The `deep` tier names `jev`, the cloud tier; this measurement is
/// Thor-only, so every tier is served by the Thor stack (the shipped
/// configuration) or by the split router (the fights). The substitution is
/// the one thing the numbers are read against.
fn backends(cfg: &AgentsConfig) -> BTreeMap<String, Box<dyn SysOne + Send + Sync>> {
    let policy = lm_agents::mind::Policy::for_harness(DEFAULT_POLICY);
    policy
        .tiers
        .tiers()
        .iter()
        .filter(|tier| !tier.is_reflex())
        .map(|tier| (tier.backend().to_owned(), backend(cfg)))
        .collect()
}

/// The ticks a round is played for, if the caller says otherwise (a smoke
/// run: `SURPRISE_TICKS=120`).
fn ticks() -> u64 {
    std::env::var("SURPRISE_TICKS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(TICKS)
}

/// The seeds the measurement plays, if the caller says otherwise.
fn seeds() -> Vec<u64> {
    match std::env::var("SURPRISE_SEEDS") {
        Ok(value) => value
            .split(',')
            .filter_map(|seed| seed.trim().parse::<u64>().ok())
            .collect(),
        Err(_) => SEEDS.to_vec(),
    }
}

/// What one round did, beside the table.
#[derive(Clone, Debug, Default)]
struct Round {
    /// The lines the agents spoke.
    utterances: u64,
    /// The decisions the agents' answers became.
    decisions: u64,
    /// The questions the minds sent.
    thinks: u64,
    /// The answers that came home.
    answered: u64,
    /// The thoughts the round refused.
    refusals: u64,
    /// The gates that were asked of a second model (D71).
    escalations: u64,
    /// The refusals the minds logged, by their own words.
    refused: BTreeMap<String, u64>,
}

/// Play one round, at the round's own rate, and hand back its numbers and
/// its surprise table.
fn play(seed: u64) -> (Round, RoundSurprise) {
    let header = Header::default_round(seed, DEFAULT_REFLEX);
    let harness = default_harness();
    let cfg = config();

    let backends = backends(&cfg);

    let mut host = HostLoop::new(header.clone()).expect("the round starts");
    host.attach_agents(Agents::new(&cfg, &harness, &header, backends));

    let period = Duration::from_millis(1000 / u64::from(header.params.tick_hz));
    for _ in 0..ticks() {
        std::thread::sleep(period);
        host.tick();
    }

    let metrics = talk_metrics(host.sim().talk_records());
    let stats = host.agent_stats().expect("the round has agents");
    // The minds' own refusal log, by name: the table counts the kinds, and
    // this says the words (a malformed answer's reason is in the message).
    let mut refused: BTreeMap<String, u64> = BTreeMap::new();
    if let Some(agents) = host.agents() {
        for mind in agents.minds().values() {
            for (_, refusal) in mind.refusals() {
                *refused.entry(refusal.to_string()).or_default() += 1;
            }
        }
    }
    let round = Round {
        utterances: metrics.utterances,
        decisions: stats.decisions,
        thinks: stats.thinks,
        answered: stats.answered,
        refusals: stats.refusals,
        escalations: stats.escalations,
        refused,
    };
    let table = host.agent_surprise().expect("the round has agents");
    (round, table)
}

/// The measurement: three rounds on the Thor, and the surprise table the
/// rounds counted.
#[test]
#[ignore = "needs the Thor at 192.168.0.78; run with --release --ignored --nocapture"]
fn surprise_on_thor() {
    let cfg = config();
    let which = std::env::var("SURPRISE_BACKEND").unwrap_or_else(|_| "shipped".to_owned());
    let mut rounds = Vec::new();
    let mut table = RoundSurprise::default();
    for seed in seeds() {
        let (round, one) = play(seed);
        println!(
            "round {seed}: {} utterances, {} decisions, {} thinks, {} answered, \
             {} refusals, {} escalations, {} asks counted",
            round.utterances,
            round.decisions,
            round.thinks,
            round.answered,
            round.refusals,
            round.escalations,
            one.total_asks()
        );
        rounds.push(round);
        table.merge(&one);
    }

    let utterances: u64 = rounds.iter().map(|round| round.utterances).sum();
    let decisions: u64 = rounds.iter().map(|round| round.decisions).sum();
    let thinks: u64 = rounds.iter().map(|round| round.thinks).sum();
    let escalations: u64 = rounds.iter().map(|round| round.escalations).sum();

    println!();
    println!("# the surprise signal on the real Thor ({which})");
    println!();
    println!(
        "- the Thor: {} (d1-2B at :{}, clef-flash at :{})",
        cfg.thor_base, cfg.ports["d1-2b"], cfg.ports["clef-flash"]
    );
    println!("- rounds: {} seeds x {} ticks", rounds.len(), ticks());
    println!("- utterances: {utterances}, decisions: {decisions}, escalations: {escalations}");
    println!();

    // The whole table, in bucket order, so the measurement can say how many
    // buckets the round actually occupied (the design bound is 288).
    let occupied = table.counts.values().filter(|c| c.asks > 0).count();
    let ranked = table.counts.values().filter(|c| c.ranked()).count();
    println!("## the table");
    println!();
    println!(
        "- asks counted: {} of {} sent (an ask is counted when its answer came home)",
        table.total_asks(),
        thinks
    );
    println!(
        "- buckets occupied: {occupied} (of at most 288); with at least {MIN_ASKS} asks: {ranked}"
    );
    println!();
    println!("| bucket | asks | unsure asks | unsure gates (worst) | refused (worst) | escalated (worst) | events/ask x1000 |");
    println!("|---|---|---|---|---|---|---|");
    for counts in table.counts.values().filter(|c| c.asks > 0) {
        println!(
            "| {} | {} | {} | {} ({}) | {} ({}) | {} ({}) | {} |",
            counts.key.describe(),
            counts.asks,
            counts.unsure_asks,
            counts.unsure.values().sum::<u64>(),
            worst(&counts.unsure),
            counts.refused.values().sum::<u64>(),
            worst(&counts.refused),
            counts.escalated.values().sum::<u64>(),
            worst(&counts.escalated),
            counts.surprise_x1000(),
        );
    }
    println!();

    // The top five, as the between-round screen will show them.
    println!("## the top five, as the screen shows them");
    println!();
    for counts in table.top(5) {
        println!("- {}", counts.line());
    }
    println!();

    // The refusals by their own words, so a `malformed` in the table can be
    // read for what it was.
    println!("## the refusals, by name");
    println!();
    let mut refused: BTreeMap<String, u64> = BTreeMap::new();
    for round in &rounds {
        for (name, count) in &round.refused {
            *refused.entry(name.clone()).or_default() += count;
        }
    }
    for (name, count) in &refused {
        println!("- {name}: {count}");
    }
    println!();
}

/// The biggest contributor of a count map, and how big it is.
fn worst(counts: &BTreeMap<String, u64>) -> String {
    counts
        .iter()
        .max_by_key(|(name, count)| (**count, std::cmp::Reverse(name.as_str())))
        .map(|(name, count)| format!("{name} {count}"))
        .unwrap_or_else(|| "-".to_owned())
}

/// The counts type is what the flow record carries: a smoke check that a
/// table entry survives the round trip, without the Thor.
#[test]
fn the_table_entry_round_trips() {
    let mut table = RoundSurprise::default();
    let key = lm_agents::surprise::SituationKey::default();
    let answers = lm_agents::sysone::Answers {
        per_gate: BTreeMap::new(),
        escalated: BTreeMap::new(),
        decomposed: BTreeMap::new(),
        unsure: BTreeMap::from([("attention".to_owned(), 0.2)]),
        model: "d1-2b".to_owned(),
        input_tokens: 0,
        latency_ms: 1,
        questions: BTreeMap::new(),
    };
    for _ in 0..MIN_ASKS {
        table.record(key, &Ok(answers.clone()));
    }
    let top: Vec<SituationCounts> = table.top(5);
    assert_eq!(top.len(), 1, "a ranked bucket with events is kept");
    // The record keeps the top entries, each carrying its own key.
    let text = serde_json::to_string(&top).expect("serialises");
    let back: Vec<SituationCounts> = serde_json::from_str(&text).expect("loads");
    assert_eq!(back, top);
    assert_eq!(back[0].unsure_asks, MIN_ASKS);
}
