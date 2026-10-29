//! The end-to-end proof of phase 1: a headless round replays bit-exact.
//!
//! These are the tests the whole milestone rests on. A round of two squads,
//! played by reflex-only agents, is recorded in a log; the log is written to
//! bytes and read back; and the round is replayed from its header and its
//! decisions, tick by tick, until every state hash matches. If any of that
//! drifts, the sim is no longer deterministic, and this file says so.
//!
//! The golden hashes live in `tests/golden.toml`, never in Rust: a change to
//! the simulation has to be argued for in the file, not hidden in a test.

use std::io::Cursor;

use lm_core::decision::{Action, Decision, Intent, Kind, Lane, Provenance, Roll, Source, Tick};
use lm_core::header::Header;
use lm_core::log::{LogError, RoundLog};
use lm_core::sim::Sim;
use lm_core::world::CellXY;
use lm_tools::branch::{branch, BranchError, Change};
use lm_tools::runner::{read_golden, read_log, replay, run, ReplayError};

/// The shipped reflex, compiled into the test binary.
const DEFAULT_REFLEX: &str = include_str!("../../../assets/reflexes/default.rhai");

/// A decision of `agent`, applied on `tick`.
fn decision(agent: u16, tick: Tick, seq: u32, kind: Kind) -> Decision {
    Decision {
        apply_tick: tick,
        source: Source::Agent(agent),
        seq,
        kind,
        provenance: Provenance::Test,
    }
}

/// The number of decisions in a log.
fn decisions_in(log: &RoundLog) -> usize {
    log.segments.iter().map(|s| s.decisions.len()).sum()
}

#[test]
fn two_squad_round_replays_bit_exact() {
    let (log, stats) = run(Header::default_round(1, DEFAULT_REFLEX), 3_000, &[]);

    // Two squads of four agents: the round the phase-1 spec asks for.
    assert_eq!(log.header.roster.len(), 8);
    assert_eq!(stats.ticks, 3_000);
    // A state hash every 30 ticks.
    assert_eq!(stats.hashes, 100);
    assert_eq!(log.hashes().len(), 100);

    // The replay sees the same world, tick by tick.
    assert_eq!(replay(&log).unwrap(), log.hashes());
}

#[test]
fn injected_decisions_replay() {
    let inject = vec![
        (
            100,
            decision(
                0,
                100,
                1,
                Kind::Intent(Intent {
                    ttl_ticks: 30,
                    plan: vec![CellXY { x: 3, y: 4 }],
                    safe: vec![],
                }),
            ),
        ),
        (
            120,
            decision(
                5,
                120,
                1,
                Kind::Lane(Lane {
                    gate: 1,
                    menu_id: 7,
                    values: vec![0, 1],
                }),
            ),
        ),
        (
            140,
            decision(
                2,
                140,
                1,
                Kind::Roll(Roll {
                    menu_id: 3,
                    p: vec![1, 2],
                }),
            ),
        ),
        // Arrived long after the tick it asks for: dropped as stale.
        (200, decision(6, 10, 1, Kind::ActionOn(Action::Hold))),
    ];

    let (log, stats) = run(Header::default_round(1, DEFAULT_REFLEX), 600, &inject);

    // The stale decision is dropped and named; the other three are applied.
    assert_eq!(stats.stale, 1);
    assert_eq!(stats.refused, 1);
    assert_eq!(decisions_in(&log), 3);

    // And the round with the injections replays bit-exact too.
    assert_eq!(replay(&log).unwrap(), log.hashes());
}

#[test]
fn written_log_replays() {
    let (log, _) = run(Header::default_round(1, DEFAULT_REFLEX), 600, &[]);

    let mut bytes = Vec::new();
    log.write(&mut bytes).expect("the log writes");
    let mut cursor = Cursor::new(bytes);
    let read = RoundLog::read(&mut cursor).expect("the log reads back");
    assert_eq!(read, log);

    assert_eq!(replay(&read).unwrap(), read.hashes());
}

#[test]
fn kill_and_resume() {
    let h = Header::default_round(1, DEFAULT_REFLEX);
    let (full, _) = run(h.clone(), 3_000, &[]);
    let (prefix, _) = run(h.clone(), 1_500, &[]);

    // The round is killed at tick 1500 and resumed from the prefix's
    // decisions, then continues with the full round's decisions.
    let mut sim = Sim::new(&h).expect("the round starts");
    let mut hashes = Vec::new();
    for tick in 0..1_500 {
        let out = sim.step(prefix.decisions_for(tick));
        if let Some(hash) = out.hash {
            hashes.push((tick, hash));
        }
    }
    for tick in 1_500..3_000 {
        let out = sim.step(full.decisions_for(tick));
        if let Some(hash) = out.hash {
            hashes.push((tick, hash));
        }
    }

    // The resumed round ends where the uninterrupted one ended.
    assert_eq!(hashes, full.hashes());
    assert_eq!(
        hashes.last().expect("the round has hashes").1,
        full.hashes().last().expect("the round has hashes").1
    );
}

#[test]
fn golden_hashes() {
    let golden = read_golden().expect("tests/golden.toml is readable");
    for seed in [1u64, 2, 3] {
        let (log, _) = run(Header::default_round(seed, DEFAULT_REFLEX), 3_000, &[]);
        let last = log.hashes().last().expect("the round has hashes").1;
        assert_eq!(
            last,
            golden.hash_of(seed).expect("the seed is pinned"),
            "seed {seed}"
        );
    }
}

#[test]
fn tamper_is_detected() {
    let inject = vec![(10, decision(0, 10, 1, Kind::ActionOn(Action::Hold)))];
    let (mut log, _) = run(Header::default_round(1, DEFAULT_REFLEX), 600, &inject);
    assert_eq!(decisions_in(&log), 1);

    // Someone edits the log: the decision now asks for another tick.
    log.segments[0].decisions[0].d.apply_tick = 11;

    let mut bytes = Vec::new();
    log.write(&mut bytes).expect("the log writes");
    let mut cursor = Cursor::new(bytes);
    let err = read_log(&mut cursor).expect_err("the tampered log is refused");
    assert!(
        matches!(
            err,
            ReplayError::Log(LogError::ChainBroken { .. } | LogError::Decode { .. })
        ),
        "got {err:?}"
    );
}

// --- `lm branch`: watch, change one thing, re-run, feel it -----------------
//
// The permanent test of the whole design: a round is played, one rule is
// changed at a chosen tick, and the re-run round shows exactly the intended
// difference -- nothing before the tick, everything after it.

/// A reflex that never moves: the "change one thing" of the branch tests.
const HOLD_REFLEX: &str = "fn reflex(v){ #{ hold: true } }";

/// A round whose squad 0 runs a reflex of its own name.
///
/// The source is the shipped one, so the round plays the standard game, but
/// squad 0's roster entries name the script `squad0` -- so a branch can
/// change squad 0 alone by swapping that one script.
fn squad0_round(seed: u64, ticks: u64) -> RoundLog {
    let mut h = Header::default_round(seed, DEFAULT_REFLEX);
    h.scripts
        .insert("squad0".to_owned(), DEFAULT_REFLEX.to_owned());
    for entry in &mut h.roster {
        if entry.squad == 0 {
            entry.reflex = "squad0".to_owned();
        }
    }
    let (log, _) = run(h, ticks, &[]);
    log
}

/// The change every branch test makes: squad 0 always holds.
fn hold_squad0() -> Change {
    Change::Reflex {
        name: "squad0".to_owned(),
        src: HOLD_REFLEX.to_owned(),
    }
}

/// The hashes of a log up to and including a tick.
fn hashes_upto(log: &RoundLog, tick: Tick) -> Vec<(Tick, u64)> {
    log.hashes()
        .into_iter()
        .filter(|(t, _)| *t <= tick)
        .collect()
}

#[test]
fn branch_prefix_is_identical() {
    let parent = squad0_round(1, 3_000);
    let (child, _) = branch(&parent, 1_500, &[hold_squad0()]).expect("the branch runs");

    // Every hash up to the branch tick is the parent's, one for one: the
    // branch changed nothing before tick 1500. A hash every 30 ticks means
    // 50 of them by tick 1500.
    assert_eq!(hashes_upto(&parent, 1_500).len(), 50);
    assert_eq!(hashes_upto(&parent, 1_500), hashes_upto(&child, 1_500));
}

#[test]
fn branch_changes_behaviour_as_intended() {
    // Seed 3, pinned like a golden hash: the parent's squad 0 plays the
    // beacons and banks 1216 points; frozen on the spot from tick 1500 it
    // banks nothing. (Freezing is not always worse -- on seed 2 a frozen
    // agent happens to sit on a beacon and keeps scoring -- so the seed is
    // part of the assertion.)
    let parent = squad0_round(3, 3_000);
    let (_, report) = branch(&parent, 1_500, &[hold_squad0()]).expect("the branch runs");

    // Squad 0 stands still from the branch tick on: it cannot score what
    // the parent's squad 0 scored by keeping after the beacons.
    assert!(
        report.branch_scores[0] <= report.parent_scores[0],
        "branch {:?} scored more than parent {:?}",
        report.branch_scores,
        report.parent_scores
    );
    assert_eq!(report.parent_scores[0], 1_216);
    assert_eq!(report.branch_scores[0], 0);
    // The rounds are not the same round, and they part company after the
    // branch tick, never before it.
    match report.first_divergent_tick {
        Some(t) => assert!(t > 1_500, "diverged at tick {t}, before the branch tick"),
        None => panic!("the branch played exactly the parent's round"),
    }
}

#[test]
fn branch_is_replayable() {
    let parent = squad0_round(3, 900);
    let (child, _) = branch(&parent, 300, &[hold_squad0()]).expect("the branch runs");

    // The branch log replays from its own header, like any round log: the
    // changes it carries in its lineage fire on their ticks again.
    assert_eq!(replay(&child).unwrap(), child.hashes());
}

#[test]
fn branch_never_mutates_parent() {
    let parent = squad0_round(4, 600);
    let mut before = Vec::new();
    parent.write(&mut before).expect("the parent writes");

    let _ = branch(&parent, 300, &[hold_squad0()]).expect("the branch runs");

    let mut after = Vec::new();
    parent.write(&mut after).expect("the parent writes");
    assert_eq!(before, after);
}

#[test]
fn branch_of_branch_lineage() {
    let parent = squad0_round(5, 900);
    let (child, _) = branch(
        &parent,
        300,
        &[Change::Param {
            field: "speed_upt".to_owned(),
            value: 64,
        }],
    )
    .expect("the first branch runs");
    let (grandchild, _) = branch(&child, 600, &[hold_squad0()]).expect("the second branch runs");

    // The grandchild's lineage names its parent (the child) by header hash
    // and head segment hash, and -- through the chain, in `prior` -- the
    // child's own lineage names the root round: both ancestors are recorded.
    let gl = grandchild
        .header
        .lineage
        .as_ref()
        .expect("the grandchild has a lineage");
    assert_eq!(gl.parent_header, child.header.hash());
    assert_eq!(
        gl.parent_head,
        child.segments.last().expect("the child has segments").hash
    );
    let prior = gl
        .prior
        .as_deref()
        .expect("the chain carries the child's lineage");
    assert_eq!(prior.parent_header, parent.header.hash());
    assert_eq!(
        prior.parent_head,
        parent
            .segments
            .last()
            .expect("the parent has segments")
            .hash
    );
    assert_eq!(
        child
            .header
            .lineage
            .as_ref()
            .expect("the child has a lineage")
            .parent_header,
        parent.header.hash()
    );

    // Every log of the chain replays on its own.
    assert_eq!(replay(&child).unwrap(), child.hashes());
    assert_eq!(replay(&grandchild).unwrap(), grandchild.hashes());
}

#[test]
fn param_change_outside_whitelist_refused() {
    let parent = squad0_round(6, 300);
    let err = branch(
        &parent,
        100,
        &[Change::Param {
            field: "tick_hz".to_owned(),
            value: 60,
        }],
    )
    .unwrap_err();
    assert_eq!(err, BranchError::ParamNotBranchable("tick_hz".to_owned()));
}

#[test]
fn branch_records_the_swap_as_a_marker() {
    let parent = squad0_round(7, 300);
    let (child, _) = branch(&parent, 100, &[hold_squad0()]).expect("the branch runs");

    // The swap itself is in the log: a marker decision of the system, on the
    // branch tick, so a dump of the log says where the round changed.
    let at = child.decisions_for(100);
    assert_eq!(at.len(), 1);
    assert_eq!(at[0].d.source, Source::System);
    assert!(matches!(at[0].d.kind, Kind::Marker(_)));
}
