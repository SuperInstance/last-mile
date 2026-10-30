//! The escalation's fence (N3): a second opinion that misses the decision's
//! apply tick is no opinion at all.
//!
//! A decision has two deadlines: the request's own deadline and the tick the
//! decision is applied on. D71's second opinion was bounded by the first one
//! only, so it could come home after the decision it was supposed to serve —
//! into a round that had already moved on. The gate was then neither sure
//! nor unsure: the move was lost, and nothing said why.
//!
//! So the escalation is bounded by the decision's apply tick as well. An
//! opinion that would land after it is refused by name (`escalation_late`),
//! the gate stays unsure, and the safe branch plays: the reflex moves the
//! agent and the round says why it did not.
//!
//! ## What is played
//!
//! The round is played through the shipped path — the [`Agents`], the
//! least-use-first queue, the thinker, the sequencer and the sim — with one
//! change only: the model behind the tiers is a pair of stubs. The cheap one
//! is not sure about the move, so the gate is escalated; the second one is
//! either too slow to answer before the fence (late) or fast enough (on
//! time).
//!
//! ## The fence the test needs
//!
//! The fence is the decision's apply tick, read as wall-clock time through
//! the round's tick rate. The shipped lead is three to nine ticks, so on the
//! shipped parameters the fence is about 130 ms out — narrower than the
//! longest pause a loaded test machine can take between two instructions of
//! the same thread. The test therefore widens the round's maximum lead, so
//! the fence is about 1.3 s out and only the model's own lateness can cross
//! it. The fence's own arithmetic is tested where it is deterministic, in
//! `lm_agents::sysone`'s unit tests.
//!
//! ## The command
//!
//! ```text
//! cargo test --release -p lm-agents --test escalation_fence -- --nocapture
//! ```

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use lm_agents::config::AgentsConfig;
use lm_agents::harness::Harness;
use lm_agents::sink::ChannelSink;
use lm_agents::sysone::{
    Answers, EscalatingBackend, EscalationOutcome, Refusal, StubBackend, SysOne,
};
use lm_agents::Agents;
use lm_core::decision::{Decision, Kind};
use lm_core::header::Header;
use lm_core::sched::Sequencer;
use lm_core::sim::Sim;

/// The shipped gates: the questions the move pack carries.
const DEFAULT_GATES: &str = include_str!("../../../assets/harness/default/gates.toml");

/// The shipped phrasebook: the wording of the lines.
const DEFAULT_PHRASEBOOK: &str = include_str!("../../../assets/harness/default/phrasebook.toml");

/// The shipped reflex, compiled into the test binary.
const DEFAULT_REFLEX: &str = include_str!("../../../assets/reflexes/default.rhai");

/// A policy over one stub backend, and a talk pack that is never asked.
///
/// The fence is about the **move**: it is the decision that moves the game,
/// so the move pack is the one the test watches. The talk pack is switched
/// off here so the test says one thing.
const FENCE_POLICY: &str = r#"
    [[tiers]]
    name = "reflex"
    backend = "reflex"
    quality = 0

    [[tiers]]
    name = "fast"
    backend = "stub"
    quality = 1

    [think]
    threat_horizon_ticks = 9
    max_think_ticks = 9
    threat_age_ticks = 2

    [talk]
    ask_on_sight_change = false
    ask_on_order = false
    ask_on_threat = false
    ask_on_timer = false
"#;

/// The ticks the round is played for.
const TICKS: u64 = 400;

/// How long the test waits for the workers to answer, in seconds.
const WAIT_SECS: u64 = 10;

/// The shipped harness with the fence policy.
fn fence_harness() -> Harness {
    Harness {
        dir: PathBuf::from("assets/harness/default"),
        gates_toml: DEFAULT_GATES.to_owned(),
        policy_toml: FENCE_POLICY.to_owned(),
        phrasebook_toml: DEFAULT_PHRASEBOOK.to_owned(),
        reflexes: BTreeMap::new(),
        commit: "content:test-harness".to_owned(),
    }
}

/// The cheap model: it is not sure about the move, so the gate is escalated.
///
/// It answers every gate of the pack it is asked, at 0.2 confidence — below
/// the 0.5 the escalation fires at.
fn unsure_primary() -> StubBackend {
    StubBackend::menu().with_confidence(0.2)
}

/// The second model, for the case where it answers in time.
fn quick_second() -> StubBackend {
    StubBackend::menu().with_confidence(0.9)
}

/// The second model, for the case where it cannot answer in time.
///
/// The scripted stub checks the delay against the deadline it is given, and
/// refuses the call outright when the delay does not fit — so the opinion
/// misses the fence without the test ever waiting for it.
fn slow_second() -> StubBackend {
    StubBackend::scripted(vec![(
        Duration::from_secs(30),
        Ok(Answers {
            per_gate: BTreeMap::new(),
            unsure: BTreeMap::new(),
            escalated: BTreeMap::new(),
            model: "stub".to_owned(),
            input_tokens: 0,
            latency_ms: 0,
        }),
    )])
}

/// What one round did.
struct Played {
    /// Every decision the minds made.
    decisions: Vec<Decision>,
    /// Every refusal the minds recorded, by name.
    refusals: Vec<String>,
    /// The escalations the minds recorded, by gate and outcome.
    escalations: Vec<String>,
    /// The decisions the sequencer accepted on time.
    on_time: u64,
}

/// Play a round with the given second model, and report what it did.
///
/// The round is played through the shipped path: the agents ask, the
/// least-use-first queue picks, the thinker's worker asks the model, the
/// answers become decisions, and the sequencer stamps them. The sim is
/// stepped with what the sequencer hands it, so a decision that is taken
/// really moves the agent.
fn play(second: StubBackend) -> Played {
    let mut header = Header::default_round(7, DEFAULT_REFLEX);
    // The fence the test needs: a lead wide enough that only the model's
    // own lateness can cross it (see the module's note).
    header.params.max_lead_ticks = 40;
    let cfg = AgentsConfig::default();
    let backend = EscalatingBackend::new(Box::new(unsure_primary()), Box::new(second), 0.5);
    let backends: BTreeMap<String, Box<dyn SysOne + Send + Sync>> = BTreeMap::from([(
        "stub".to_owned(),
        Box::new(backend) as Box<dyn SysOne + Send + Sync>,
    )]);
    let (sink, mut inbox) = ChannelSink::new();
    let mut agents =
        Agents::new(&cfg, &fence_harness(), &header, backends).with_sink(Box::new(sink));
    let mut sim = Sim::new(&header).expect("the round starts");
    let mut seq = Sequencer::new(
        &header.params,
        header.roster.len() as u16,
        header.start_tick,
    );

    // The round is played until the workers have answered everything they
    // were asked: the sim never waits for a model, so the test does.
    let stop = Instant::now() + Duration::from_secs(WAIT_SECS);
    let mut tick = 0;
    let mut decisions = Vec::new();
    while tick < TICKS && Instant::now() < stop {
        agents.on_tick(tick, &sim);
        for (decision, arrived) in inbox.drain() {
            decisions.push(decision.clone());
            seq.submit(decision, arrived)
                .expect("the sequencer takes the mind's decision");
        }
        let stamped = seq.drain(tick);
        sim.step(&stamped);
        tick += 1;
        // The workers need a moment to answer; the sim ticks on.
        std::thread::sleep(Duration::from_millis(1));
    }

    let mut refusals = Vec::new();
    let mut escalations = Vec::new();
    for mind in agents.minds().values() {
        for (_, refusal) in mind.refusals() {
            refusals.push(refusal.to_string());
        }
        for (_, escalation) in mind.escalations() {
            escalations.push(format!(
                "{}:{}",
                escalation.gate,
                match escalation.outcome {
                    EscalationOutcome::Taken => "taken",
                    EscalationOutcome::Unsure => "unsure",
                    EscalationOutcome::Timeout => "timeout",
                    EscalationOutcome::Unavailable => "unavailable",
                    EscalationOutcome::Late => "late",
                }
            ));
        }
    }
    Played {
        decisions,
        refusals,
        escalations,
        on_time: seq.stats().on_time,
    }
}

/// A second opinion that misses the decision's apply tick takes the safe
/// branch, and the round says why.
#[test]
fn a_late_escalation_takes_the_safe_branch() {
    let played = play(slow_second());

    // The gate was escalated, and the second opinion missed the fence.
    assert!(
        played.escalations.iter().any(|e| e == "move:late"),
        "the move was escalated and the opinion was late: {:?}",
        played.escalations
    );
    // The refusal is named: the round knows why the move was lost.
    assert!(
        played
            .refusals
            .iter()
            .any(|r| r == &Refusal::EscalationLate.to_string()),
        "the round recorded the late escalation: {:?}",
        played.refusals
    );
    // And the move was not taken: the safe branch plays.
    assert!(
        !played
            .decisions
            .iter()
            .any(|d| matches!(d.kind, Kind::Lane(_))),
        "the model decided no lane: {:?}",
        played.decisions
    );
    assert_eq!(played.on_time, 0, "no decision was made at all");
}

/// A second opinion that is in hand before the decision's apply tick is
/// taken, exactly as it was before the fence existed.
#[test]
fn an_escalation_on_time_is_taken() {
    let played = play(quick_second());

    // The gate was escalated, and the second opinion was in time.
    assert!(
        played.escalations.iter().any(|e| e == "move:taken"),
        "the move was escalated and the opinion was taken: {:?}",
        played.escalations
    );
    // The fence did not fire.
    assert!(
        !played
            .refusals
            .iter()
            .any(|r| r == &Refusal::EscalationLate.to_string()),
        "the fence did not fire: {:?}",
        played.refusals
    );
    // And the move was taken: the model decided the lane, and the sequencer
    // took it on time.
    assert!(
        played
            .decisions
            .iter()
            .any(|d| matches!(d.kind, Kind::Lane(_))),
        "the model decided a lane: {:?}",
        played.decisions
    );
    assert!(played.on_time >= 1, "the lane decision was on time");
}
