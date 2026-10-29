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
