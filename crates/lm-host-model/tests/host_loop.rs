//! The host loop over the phase-1 runner.
//!
//! The host is the client a person plays on, and it must not be a second
//! simulation: the same header and the same decisions have to produce the
//! same world, whether the round is played by the headless runner or by a
//! person clicking on a map. These tests are the proof of that.
//!
//! They also pin the two rules that make the host honest: a human action
//! becomes a decision through the same sequencer the agents use (so it is
//! logged, hashed and replayable like any other), and it never waits (it
//! lands on the very next simulated tick).

use lm_core::decision::{Action, Kind, Provenance, Source};
use lm_core::header::Header;
use lm_core::log::RoundLog;
use lm_host_model::{catch_up, HostLoop, TickBudget};
use lm_tools::runner::{self, DEFAULT_REFLEX};

/// A standard round with the shipped reflex.
fn round(seed: u64) -> Header {
    Header::default_round(seed, DEFAULT_REFLEX)
}

/// The hashes a host loop produced, with their ticks.
fn hashes_of(host: &mut HostLoop, ticks: u64) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    for _ in 0..ticks {
        let tick = host.tick();
        if let Some(hash) = tick.hash {
            out.push((tick.tick, hash));
        }
    }
    out
}

#[test]
fn host_loop_matches_runner() {
    // 600 ticks of the same round, played two ways.
    let (log, _) = runner::run(round(1), 600, &[]);
    let mut host = HostLoop::new(round(1)).expect("the round starts");

    assert_eq!(hashes_of(&mut host, 600), log.hashes());
}

#[test]
fn human_decision_applies_next_tick() {
    let mut host = HostLoop::new(round(1)).expect("the round starts");

    // Ten ticks of quiet play: the tick about to be simulated is 10.
    for _ in 0..10 {
        host.tick();
    }

    // The human holds agent 0. It is stamped on tick 10, the tick that is
    // about to be simulated: a human decision never waits.
    let applied_at = host
        .submit_human(0, Kind::ActionOn(Action::Hold))
        .expect("the decision is accepted");
    assert_eq!(applied_at, 10);

    let out = host.tick();
    assert_eq!(out.tick, 10);
    assert_eq!(
        host.sim().agent(0).expect("agent 0").held,
        Some(Action::Hold)
    );
}

#[test]
fn host_loop_tick_count_matches_elapsed() {
    // 100 ms at 30 Hz is three ticks.
    assert_eq!(catch_up(100, 30), 3);
    // And the remainder is carried, so no tick is skipped or doubled.
    let mut budget = TickBudget::new();
    assert_eq!(budget.catch_up(16, 30), 0);
    assert_eq!(budget.catch_up(16, 30), 0);
    assert_eq!(budget.catch_up(16, 30), 1);

    // 160 ms is 4.8 ticks: four whole ticks, and the fifth is still owed.
    let mut budget = TickBudget::new();
    let mut ticks = 0;
    for _ in 0..10 {
        ticks += budget.catch_up(16, 30);
    }
    assert_eq!(ticks, 4);
}

#[test]
fn log_so_far_replays() {
    let mut host = HostLoop::new(round(1)).expect("the round starts");
    for _ in 0..300 {
        host.tick();
    }

    let log = host.log_so_far();
    assert_eq!(runner::replay(&log).expect("the log replays"), log.hashes());
}

#[test]
fn a_human_decision_is_logged_as_a_human_decision() {
    let mut host = HostLoop::new(round(1)).expect("the round starts");
    for _ in 0..10 {
        host.tick();
    }
    host.submit_human(0, Kind::ActionOn(Action::Hold))
        .expect("the decision is accepted");
    host.tick();

    let log = host.log_so_far();
    let applied = log.decisions_for(10);
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].d.source, Source::Human(0));
    assert_eq!(applied[0].d.provenance, Provenance::Human);
    assert_eq!(applied[0].d.apply_tick, 10);
    assert_eq!(applied[0].arrived, 10);
    assert_eq!(applied[0].late_by, 0);

    // And the round with a human in it still replays bit-exact.
    assert_eq!(runner::replay(&log).expect("the log replays"), log.hashes());
}

#[test]
fn finishing_the_round_gives_the_same_log_as_reading_it_early() {
    let mut host = HostLoop::new(round(1)).expect("the round starts");
    for _ in 0..300 {
        host.tick();
    }
    let so_far = host.log_so_far();

    let mut host = HostLoop::new(round(1)).expect("the round starts");
    for _ in 0..300 {
        host.tick();
    }
    let finished: RoundLog = host.finish();

    assert_eq!(so_far.hashes(), finished.hashes());
    assert_eq!(so_far.header, finished.header);
    assert!(finished.verify().is_ok());
}

#[test]
fn the_positions_before_the_last_tick_are_kept_for_interpolation() {
    let mut host = HostLoop::new(round(1)).expect("the round starts");

    host.tick();
    // One entry per agent, in agent-id order.
    assert_eq!(host.prev_positions().len(), 8);
    assert_eq!(
        host.prev_positions()
            .iter()
            .map(|(id, _)| *id)
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3, 4, 5, 6, 7]
    );

    // After the next tick, the positions kept are the ones the agents had
    // before it: that is what the renderer interpolates between.
    let after_first = host.sim().agent(0).expect("agent 0").pos;
    host.tick();
    assert_eq!(host.prev_positions()[0].1, after_first);
}
