//! The round, end to end: a person plays it, it is recorded, it replays.
//!
//! This is the promise the whole host rests on. A round a person plays on the
//! ProArt is a round like any other: the human's actions become decisions
//! through the same sequencer the agents use, the round is written down, and
//! the file that comes out of it replays bit-exact -- the same world, tick by
//! tick, the same hashes.
//!
//! The test below is that promise, end to end and without a screen:
//!
//! 1. a headless [`HostLoop`] plays a round, with a person in it (possess an
//!    agent, three clicks, release);
//! 2. the round is finished, which seals its log;
//! 3. the log is written to the file the host writes it to;
//! 4. the headless runner replays the log and gets the same hashes;
//! 5. the replay viewer opens the file and shows the round it recorded.
//!
//! If any of that breaks, a round that a person played can no longer be
//! checked, and the host has nothing to show but its word.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use lm_core::decision::{Provenance, Source};
use lm_core::header::Header;
use lm_core::log::RoundLog;
use lm_core::world::CellXY;
use lm_host_model::host_loop::HostLoop;
use lm_host_model::input::{InputEvent, InputState};
use lm_host_model::record;
use lm_host_model::scrub::Scrubber;
use lm_tools::runner::{self, DEFAULT_REFLEX};

/// How many ticks the round lasts.
const TICKS: u64 = 900;

/// A standard round with the shipped reflex.
fn round(seed: u64) -> Header {
    Header::default_round(seed, DEFAULT_REFLEX)
}

/// A path no other test in this process uses.
fn temp_path(tag: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut path = std::env::temp_dir();
    path.push(format!(
        "lm-host-model-{tag}-{}-{n}.lml",
        std::process::id()
    ));
    path
}

/// The decisions of the round that a person made.
fn human_decisions(log: &RoundLog) -> Vec<&lm_core::decision::Stamped> {
    log.segments
        .iter()
        .flat_map(|segment| segment.decisions.iter())
        .filter(|stamped| stamped.d.provenance == Provenance::Human)
        .collect()
}

#[test]
fn host_round_replays_bit_exact() {
    // A person takes over agent 0 before the round starts.
    let mut host = HostLoop::new(round(1)).expect("the round starts");
    let mut input = InputState::new();
    input
        .apply(InputEvent::Possess(0), &mut host)
        .expect("agent 0 is possessed");

    // Three clicks: the agent walks to a cell, then to another, then to a
    // third. The ticks in between are the round being played, not the human
    // hesitating.
    let clicks = [
        CellXY { x: 20, y: 20 },
        CellXY { x: 44, y: 33 },
        CellXY { x: 61, y: 52 },
    ];
    for cell in clicks {
        for _ in 0..10 {
            host.tick();
        }
        input
            .apply(InputEvent::ClickCell(cell), &mut host)
            .expect("the click is an order to the agent");
    }

    // The person lets go of the agent: it goes back to its reflexes.
    for _ in 0..10 {
        host.tick();
    }
    input
        .apply(InputEvent::Release, &mut host)
        .expect("the agent is released");

    // The round plays out.
    while host.sim().tick() < TICKS {
        host.tick();
    }

    // The round is finished, which seals its log.
    let log: RoundLog = host.finish();
    assert_eq!(log.header.start_tick, 0);
    assert_eq!(log.segments.len(), 3, "900 ticks of 300 a segment");

    // The person is in the log, and in it as a person: the same sequencer,
    // the same stamps, and never late.
    let humans = human_decisions(&log);
    assert_eq!(humans.len(), 4, "three clicks and the release");
    for stamped in &humans {
        assert_eq!(stamped.d.source, Source::Human(0));
        assert_eq!(stamped.d.provenance, Provenance::Human);
        assert_eq!(stamped.late_by, 0, "a human decision never waits");
        assert_eq!(
            stamped.arrived, stamped.tick,
            "applied on the tick it arrived"
        );
    }

    // The round replays bit-exact: the same hashes, on the same ticks.
    let hashes = runner::replay(&log).expect("the round replays");
    assert_eq!(hashes, log.hashes());
    assert!(!hashes.is_empty(), "the round took hashes");

    // The file the host writes is the round: written under the name the host
    // gives it, and read back as the very same log.
    let path = temp_path("bit-exact");
    record::write(&log, &path).expect("the round is written");
    let mut file = std::fs::File::open(&path).expect("the file opens");
    let read = runner::read_log(&mut file).expect("the file is a round log");
    assert_eq!(read, log);

    // And the replay viewer opens it and shows the round it recorded.
    let mut scrubber = Scrubber::open(&path).expect("the log opens");
    assert_eq!(scrubber.len(), TICKS);
    assert_eq!(scrubber.last_tick(), TICKS - 1);

    // The world of the last tick is the world the round ended in.
    scrubber.seek(TICKS - 1);
    assert_eq!(scrubber.tick(), Some(TICKS - 1));
    assert_eq!(scrubber.hash(), log.hashes().last().map(|(_, hash)| *hash));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_round_that_was_read_early_is_the_round_that_is_finished() {
    // The host writes the log it has when the round ends. The copy it reads
    // along the way has to be the same log, or the file on disk would not be
    // the round that was played.
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

    assert_eq!(so_far, finished);
}

#[test]
fn a_round_is_recorded_under_the_name_of_its_seed_and_second() {
    // The name says which round it is and when it was played.
    let path = record::path_for(3, 1_700_000_000);
    assert!(path.ends_with("runs/3-1700000000.lml"), "{path:?}");

    // And the directory is made when the file is written, so the first round
    // of a fresh checkout is recorded without anyone making `runs/` first.
    let mut path = std::env::temp_dir();
    path.push(format!("lm-host-model-runs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    path.push("nested");
    path.push("round.lml");

    let (log, _) = runner::run(round(1), 30, &[]);
    record::write(&log, &path).expect("the round is written");
    assert!(path.exists(), "the file is there");

    let _ = std::fs::remove_dir_all(
        path.parent()
            .expect("the directory")
            .parent()
            .expect("the parent"),
    );
}
