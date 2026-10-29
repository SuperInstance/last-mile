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
        reflexes: BTreeMap::new(),
        commit: "content:test-harness".to_owned(),
    }
}

/// A stub behind every tier the policy names.
///
/// The tiers stay the ones the game would use — the same gates, the same
/// policy, the same ladder — and only the model behind them changes.
fn stub_backends(harness: &Harness) -> BTreeMap<String, Box<dyn SysOne + Send>> {
    let policy = Policy::for_harness(&harness.policy_toml);
    policy
        .tiers
        .tiers()
        .iter()
        .filter(|tier| !tier.is_reflex())
        .map(|tier| {
            (
                tier.backend().to_owned(),
                Box::new(StubBackend::menu()) as Box<dyn SysOne + Send>,
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
        Box::new(backend) as Box<dyn SysOne + Send>,
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
