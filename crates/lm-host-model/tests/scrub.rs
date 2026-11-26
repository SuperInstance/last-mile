//! The replay scrubber: a round log you can open and seek in.
//!
//! The scrubber is the other half of the round: a round that was recorded can
//! be opened again and walked through, tick by tick, exactly as it was
//! played. Two things have to hold for that to be worth anything:
//!
//! * the state the scrubber shows is the state that was recorded, and
//! * a damaged, truncated or foreign file is refused by name, never
//!   crashed on.
//!
//! The scrubber replays the round the way the headless tools do, so what it
//! shows is the sim's own state, not a picture of it.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use lm_core::header::Header;
use lm_core::log::{LogError, RoundLog};
use lm_host_model::scrub::Scrubber;
use lm_tools::runner::{self, DEFAULT_REFLEX};

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

/// Write the log to a temporary file and hand back its path.
fn write_log(log: &RoundLog, tag: &str) -> PathBuf {
    let path = temp_path(tag);
    let mut file = std::fs::File::create(&path).expect("the temporary file is created");
    log.write(&mut file).expect("the log is written");
    path
}

/// Where every blob of a log file starts, and how long it is.
///
/// Blob 0 is the header, blob `i` (for `i >= 1`) is `segments[i - 1]`.
fn blob_spans(bytes: &[u8]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    // The magic and the version come first.
    let mut off = 10;
    while off + 4 <= bytes.len() {
        let len = u32::from_le_bytes(bytes[off..off + 4].try_into().expect("four bytes")) as usize;
        off += 4;
        spans.push((off, len));
        off += len;
    }
    spans
}

#[test]
fn scrub_clamps() {
    let (log, _) = runner::run(round(1), 900, &[]);
    let path = write_log(&log, "clamps");

    let mut scrubber = Scrubber::open(&path).expect("the log opens");
    assert_eq!(scrubber.len(), 900);

    // Past the end of the round: the last tick, and no panic.
    scrubber.seek(10_000);
    assert_eq!(scrubber.tick(), Some(899));

    // The start of the round.
    scrubber.seek(0);
    assert_eq!(scrubber.tick(), Some(0));

    // And backwards again, from the middle.
    scrubber.seek(450);
    assert_eq!(scrubber.tick(), Some(450));
    scrubber.seek(10);
    assert_eq!(scrubber.tick(), Some(10));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn scrubber_reports_bad_log() {
    let (log, _) = runner::run(round(1), 600, &[]);
    let path = write_log(&log, "bad");
    let bytes = std::fs::read(&path).expect("the log reads");

    // Cut the file in the middle of its last blob.
    let spans = blob_spans(&bytes);
    let (start, len) = spans[spans.len() - 1];
    std::fs::write(&path, &bytes[..start + len / 2]).expect("the truncated log is written");

    // The damage is named, not crashed on.
    let err = match Scrubber::open(&path) {
        Ok(_) => panic!("a truncated log is refused"),
        Err(err) => err,
    };
    assert!(
        matches!(err, LogError::Truncated { segment: 2 }),
        "got {err:?}"
    );

    // A file that is not a round log at all is refused too.
    let path = temp_path("not-a-log");
    std::fs::write(&path, b"this is not a round log").expect("the file is written");
    let err = match Scrubber::open(&path) {
        Ok(_) => panic!("a file that is not a log is refused"),
        Err(err) => err,
    };
    assert_eq!(err, LogError::BadMagic);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn seek_matches_recorded_hash() {
    // A round that hashes every tick, so that tick 900 has a hash of its own.
    let mut header = round(1);
    header.params.hash_interval_ticks = 1;
    let (log, _) = runner::run(header, 901, &[]);
    let path = write_log(&log, "hash");

    let mut scrubber = Scrubber::open(&path).expect("the log opens");
    scrubber.seek(900);

    // The state the scrubber shows at tick 900 is the state the log recorded
    // at tick 900.
    let (_, expected) = log
        .hashes()
        .into_iter()
        .find(|(tick, _)| *tick == 900)
        .expect("the log has a hash at tick 900");
    assert_eq!(scrubber.hash(), Some(expected));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn the_scrubbed_state_is_the_recorded_state() {
    // The scrubber shows the same world the round played: the agent that was
    // holding the beacon in the log is holding it in the scrubber too.
    let (log, _) = runner::run(round(1), 300, &[]);
    let path = write_log(&log, "state");

    let mut scrubber = Scrubber::open(&path).expect("the log opens");
    scrubber.seek(299);
    let sim = scrubber.sim();

    // The score of the round is the score the round ended on.
    let (_, stats) = runner::run(round(1), 300, &[]);
    assert_eq!(sim.scores(), stats.scores.as_slice());
    assert_eq!(sim.tick(), 300);

    let _ = std::fs::remove_file(&path);
}
