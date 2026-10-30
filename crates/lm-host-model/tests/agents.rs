//! Task 7: the agents, wired into the host loop.
//!
//! This file is the proof that phase 3 is done. A round is played by the
//! host — the sequencer, the sim and the recorder of `lm-host-model` — with
//! a cognition loop attached. The agents think (on a scripted stub, never on
//! a network), their answers become decisions, and those decisions are logged
//! like any other decision.
//!
//! And the round still replays bit-exact: the log alone is enough to replay
//! it, with no model in sight (D10, D26).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use lm_agents::config::AgentsConfig;
use lm_agents::harness::Harness;
use lm_agents::mind::Policy;
use lm_agents::sysone::{StubBackend, SysOne};
use lm_agents::Agents;
use lm_core::decision::{Provenance, Source};
use lm_core::header::Header;
use lm_core::log::RoundLog;
use lm_core::refusal::RefusalKind;
use lm_host_model::score::{AttentionTrace, ScoreModel};
use lm_host_model::HostLoop;
use lm_tools::runner;

/// The shipped harness: the gates and the policy the default round thinks
/// with. The agents of this test are the ones the game would use; only the
/// model behind them is a stub.
const DEFAULT_GATES: &str = include_str!("../../../assets/harness/default/gates.toml");
const DEFAULT_POLICY: &str = include_str!("../../../assets/harness/default/policy.toml");
const DEFAULT_PHRASEBOOK: &str = include_str!("../../../assets/harness/default/phrasebook.toml");

/// A standard round with the shipped reflex.
fn a_round(seed: u64) -> Header {
    Header::default_round(seed, runner::DEFAULT_REFLEX)
}

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

/// A stub behind every tier the policy names.
///
/// The tiers stay the ones the game would use — the same gates, the same
/// policy, the same ladder — and only the model behind them changes.
fn stub_backends(harness: &Harness) -> BTreeMap<String, Box<dyn SysOne + Send + Sync>> {
    let policy = Policy::for_harness(&harness.policy_toml);
    policy
        .tiers
        .tiers()
        .iter()
        .filter(|tier| !tier.is_reflex())
        .map(|tier| {
            (
                tier.backend().to_owned(),
                Box::new(StubBackend::menu()) as Box<dyn SysOne + Send + Sync>,
            )
        })
        .collect()
}

/// The round's agents, thinking on the stub.
fn stub_agents(header: &Header, harness: &Harness) -> Agents {
    Agents::new(
        &AgentsConfig::default(),
        harness,
        header,
        stub_backends(harness),
    )
}

/// Give the thinker thread a chance to answer before the next tick.
///
/// The host never waits for a model — that is the whole point of the loop —
/// but a test that plays ticks as fast as the CPU allows would outrun the
/// worker and lose the answers at the end of the round. The test waits here,
/// not the host.
fn settle(host: &mut HostLoop) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while host.agents_pending() > 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Play ticks until the round's ledger closes: every ask that was made has
/// ended, and nothing is with the models.
///
/// A test that reads the counters right after a settle can read a sum that
/// does not add up — not because a question vanished, but because its answer
/// came home during the settle and sits in the channel, waiting for the next
/// tick to drain it. The round is played until it is quiet, and then the
/// ledger has to hold.
fn play_until_quiet(host: &mut HostLoop) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        host.tick();
        settle(host);
        let stats = host.agent_stats().expect("the round has agents");
        if stats.asks_add_up() && host.agents_pending() == 0 {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "the round never went quiet: {stats:?}"
        );
    }
}

/// The decisions the agents made, as they were logged.
fn agent_decisions(log: &RoundLog) -> Vec<(u64, u16)> {
    log.segments
        .iter()
        .flat_map(|segment| segment.decisions.iter())
        .filter(|stamped| matches!(stamped.d.source, Source::Agent(_)))
        .map(|stamped| (stamped.tick, stamped.d.source.agent().unwrap_or_default()))
        .collect()
}

#[test]
fn stub_round_end_to_end() {
    let header = a_round(7);
    let harness = default_harness();
    let agents = stub_agents(&header, &harness);

    let mut host = HostLoop::new(header).expect("the round starts");
    host.attach_agents(agents);

    for _ in 0..900 {
        host.tick();
        settle(&mut host);
    }

    let log = host.log_so_far();

    // The agents thought.
    let stats = host.agent_stats().expect("the round has agents");
    assert!(stats.thinks > 0, "the agents asked questions: {stats:?}");
    assert!(
        stats.decisions > 0,
        "the answers became decisions: {stats:?}"
    );

    // And their decisions are in the log, with System One provenance.
    let system1: Vec<_> = log
        .segments
        .iter()
        .flat_map(|segment| segment.decisions.iter())
        .filter(|stamped| matches!(stamped.d.provenance, Provenance::System1 { .. }))
        .collect();
    assert!(
        !system1.is_empty(),
        "the agents' decisions are in the log, with System One provenance"
    );
    for stamped in &system1 {
        assert!(
            matches!(stamped.d.source, Source::Agent(_)),
            "a System One decision comes from an agent: {:?}",
            stamped.d.source
        );
    }

    // The decisions really were applied: the agents moved the game on.
    assert!(
        !agent_decisions(&log).is_empty(),
        "the agents' decisions are logged"
    );

    // And the round replays bit-exact, with no model in sight.
    let replayed = runner::replay(&log).expect("the log replays");
    assert_eq!(replayed, log.hashes());
}

#[test]
fn a_round_without_agents_plays_the_same_way() {
    // The reflex alone: no agents, no decisions from a model, and the round
    // still replays.
    let mut host = HostLoop::new(a_round(7)).expect("the round starts");
    for _ in 0..120 {
        host.tick();
    }

    assert!(host.agent_stats().is_none(), "the round has no agents");
    assert!(host.agent_thoughts(0).is_empty());

    let log = host.log_so_far();
    assert_eq!(runner::replay(&log).expect("the log replays"), log.hashes());
}

#[test]
fn the_agents_refuse_rather_than_guess() {
    // A round whose policy names a backend nobody serves: the agents hear a
    // refusal and the reflex keeps playing. Nothing is invented (D26).
    let header = a_round(7);
    let harness = default_harness();
    let agents = Agents::new(&AgentsConfig::default(), &harness, &header, BTreeMap::new());

    let mut host = HostLoop::new(header).expect("the round starts");
    host.attach_agents(agents);

    for _ in 0..120 {
        host.tick();
        settle(&mut host);
    }

    let log = host.log_so_far();
    let system1 = log
        .segments
        .iter()
        .flat_map(|segment| segment.decisions.iter())
        .filter(|stamped| matches!(stamped.d.provenance, Provenance::System1 { .. }))
        .count();
    assert_eq!(system1, 0, "a model that is not there decides nothing");

    let stats = host.agent_stats().expect("the round has agents");
    assert!(stats.refusals > 0, "the refusals are counted: {stats:?}");
    assert_eq!(stats.decisions, 0, "a refusal is not a decision");
}

#[test]
fn every_question_ends_in_exactly_one_place() {
    // The ledger of the round (N4): every question the minds asked is taken,
    // late or refused, and it is exactly one of the three. A question that
    // vanishes on the way would break the sum, and the sum is the assertion
    // that says so.
    let header = a_round(7);
    let harness = default_harness();
    let agents = stub_agents(&header, &harness);

    let mut host = HostLoop::new(header).expect("the round starts");
    host.attach_agents(agents);

    for _ in 0..900 {
        host.tick();
        settle(&mut host);
    }
    // The ledger is asserted when the round is quiet, so an ask whose answer
    // is still in the channel cannot read as a vanished question.
    play_until_quiet(&mut host);

    let stats = host.agent_stats().expect("the round has agents");
    assert!(stats.asked > 0, "the minds asked: {stats:?}");
    assert!(stats.asks_add_up(), "the ledger does not add up: {stats:?}");
    assert_eq!(
        stats.asked,
        stats.accepted + stats.late + stats.refused,
        "every question ends in exactly one place"
    );
}

#[test]
fn the_refusals_are_in_the_log_by_name() {
    // N2: every refusal the round saw is in the log, with the name of the
    // rule that failed, and the log's own counts agree with the round's.
    let header = a_round(7);
    let harness = default_harness();
    // A round whose policy names a backend nobody serves: every question is
    // refused, so there is something to record.
    let agents = Agents::new(&AgentsConfig::default(), &harness, &header, BTreeMap::new());

    let mut host = HostLoop::new(header).expect("the round starts");
    host.attach_agents(agents);

    for _ in 0..120 {
        host.tick();
        settle(&mut host);
    }

    let log = host.log_so_far();
    let stats = host.agent_stats().expect("the round has agents");

    // The refusals are in the record, and the record is sealed with them.
    let refusals = log.refusals();
    assert!(!refusals.is_empty(), "the refusals are in the log");
    assert!(log.verify().is_ok(), "the log still chains");

    // The kinds are apart: the record says which rule failed, not just how
    // many things failed.
    let counts = log.refusal_counts();
    assert_eq!(counts.total(), refusals.len() as u64);
    assert_eq!(
        counts.total(),
        stats.refusal_counts.total(),
        "the record and the counters agree"
    );
    assert_eq!(counts.total(), stats.refusals);
}

#[test]
fn a_replay_draws_the_hollow_marks() {
    // E7: the score view reads the refusals from the log, so a replay draws
    // the same marks a live round did.
    let header = a_round(7);
    let harness = default_harness();
    let agents = Agents::new(&AgentsConfig::default(), &harness, &header, BTreeMap::new());

    let mut host = HostLoop::new(header).expect("the round starts");
    host.attach_agents(agents);

    for _ in 0..120 {
        host.tick();
        settle(&mut host);
    }

    let log = host.log_so_far();
    let refusals = log.refusals();
    assert!(!refusals.is_empty(), "the round refused something");

    let window = 0..u64::MAX;
    let score = ScoreModel::from_log(&log, &AttentionTrace::default(), &BTreeMap::new(), window);
    let marks: Vec<_> = score
        .systems
        .iter()
        .flat_map(|system| system.hollow.iter())
        .collect();
    assert_eq!(
        marks.len(),
        refusals.len(),
        "every refusal in the record is a mark on the staff"
    );
}

#[test]
fn a_crashed_backend_ends_its_question_in_the_log() {
    // N2/N4: a backend that panics is a bug, but its question still ends —
    // named `panicked` in the record, and the round's ledger still adds up.
    // The worker catches the crash instead of dying with the ask in its
    // hands, so the lane keeps serving the next question too.
    struct Panicking;
    impl SysOne for Panicking {
        fn ask(
            &self,
            _state: &str,
            _questions: &serde_json::Value,
            _deadline: Instant,
        ) -> Result<lm_agents::sysone::Answers, lm_agents::sysone::Refusal> {
            panic!("the model is on fire");
        }
        fn name(&self) -> &str {
            "panicking"
        }
    }

    let header = a_round(7);
    let harness = default_harness();
    let policy = Policy::for_harness(&harness.policy_toml);
    let backends: BTreeMap<String, Box<dyn SysOne + Send + Sync>> = policy
        .tiers
        .tiers()
        .iter()
        .filter(|tier| !tier.is_reflex())
        .map(|tier| {
            (
                tier.backend().to_owned(),
                Box::new(Panicking) as Box<dyn SysOne + Send + Sync>,
            )
        })
        .collect();
    let agents = Agents::new(&AgentsConfig::default(), &harness, &header, backends);

    let mut host = HostLoop::new(header).expect("the round starts");
    host.attach_agents(agents);

    // The crash is the worker's to catch. The message it would print is
    // silenced for the thinker's own threads only — every other panic of
    // the process keeps its words — and the refusal carries the why.
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thinker = std::thread::current()
            .name()
            .is_some_and(|name| name.starts_with("lm-thinker-"));
        if !thinker {
            previous(info);
        }
    }));
    for _ in 0..200 {
        host.tick();
        settle(&mut host);
    }
    play_until_quiet(&mut host);

    let stats = host.agent_stats().expect("the round has agents");
    assert!(stats.asked > 0, "the minds asked: {stats:?}");
    assert!(
        stats.asks_add_up(),
        "a crashed backend is a refusal, not a lost question: {stats:?}"
    );
    assert!(
        stats.refusal_counts.get(RefusalKind::Panicked) > 0,
        "the crash is counted by its own name: {stats:?}"
    );

    // And the record says it too: the log and the counters agree.
    let log = host.log_so_far();
    let counts = log.refusal_counts();
    assert_eq!(
        counts.get(RefusalKind::Panicked),
        stats.refusal_counts.get(RefusalKind::Panicked),
        "the record and the counters agree"
    );
    assert!(log.verify().is_ok(), "the log still chains");
}

/// The live round: the Thor's own d1-2B (the default fast tier since D62), on the LAN.
///
/// It is ignored by default: it needs the Thor to be up and it talks to a
/// real model. Run it with `cargo test -p lm-host-model -- --ignored`.
///
/// The round plays at its own rate — 300 ticks at 30 Hz is ten seconds — so
/// the models get the time a model needs. A round played faster than the
/// clock would let the answers land many ticks late, which is not how the
/// host plays.
///
/// The confidence floor is pinned low on purpose. The test proves the
/// *pipeline* — the question, the answer, the decision, the log — and
/// leaves the calibration where it belongs: a decision about the floor is
/// Casey's, not the test's. The pack's own confidence is measured
/// separately, in `tests/gates_tuning.rs` and written up in
/// docs/research/poc/thor/GATES-TUNING.md: the "what should you do now?"
/// pack scored 0.20-0.57 (p10-p50), and the pack that states the squad's
/// doctrine as a rule scores 0.40-0.91 with a median of 0.87.
#[test]
#[ignore = "needs the Thor on the LAN (192.168.0.78:8093)"]
fn live_round_on_thor() {
    use lm_agents::sysone::ThorBackend;

    /// The floor this test pins, below the Thor's own confidence.
    const FLOOR: f32 = 0.15;

    let header = a_round(7);
    let tick_hz = header.params.tick_hz;
    let harness = default_harness();
    let cfg = AgentsConfig::default();

    let port = cfg.ports["d1-2b"];
    let backend = ThorBackend::new(&cfg.thor_base, port, "d1-2b")
        .with_min_confidence(FLOOR)
        .with_timeout(Duration::from_millis(u64::from(cfg.request_timeout_ms)));
    let backends = BTreeMap::from([(
        "thor-d1-2b".to_owned(),
        Box::new(backend) as Box<dyn SysOne + Send + Sync>,
    )]);

    let agents = Agents::new(&cfg, &harness, &header, backends);
    let mut host = HostLoop::new(header).expect("the round starts");
    host.attach_agents(agents);

    // 300 ticks of the round, at the round's own rate.
    let period = Duration::from_millis(1000 / u64::from(tick_hz));
    for _ in 0..300 {
        std::thread::sleep(period);
        host.tick();
    }

    // The thoughts that were in flight are given time to land, and the ticks
    // that apply them are played.
    let deadline = Instant::now() + Duration::from_secs(20);
    while host.agents_pending() > 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    for _ in 0..15 {
        std::thread::sleep(period);
        host.tick();
    }

    let log = host.log_so_far();
    let system1 = log
        .segments
        .iter()
        .flat_map(|segment| segment.decisions.iter())
        .filter(|stamped| matches!(stamped.d.provenance, Provenance::System1 { .. }))
        .count();
    println!(
        "the round: {:?}",
        host.agent_stats().expect("the round has agents")
    );
    assert!(
        system1 > 0,
        "the Thor's decisions are applied: {} in the log",
        agent_decisions(&log).len()
    );
}
