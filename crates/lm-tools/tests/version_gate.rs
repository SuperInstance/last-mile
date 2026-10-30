//! The version gate: a phase-1 log reads under the phase-4 build.
//!
//! The proto bump changed two things: the header gained the talk params and
//! the decision kinds gained the utterance. Both were **appended**, so a
//! phase-1 log still decodes -- and the round it records still replays to
//! the very state hashes it recorded, because the migration touches the
//! header and the chain and never the world.
//!
//! The fixture is the log phase 1 wrote before the bump:
//! `tests/fixtures/phase1-v1-seed1.lml`.

use std::io::Cursor;
use std::path::PathBuf;

use lm_core::decision::Tick;
use lm_core::hash::StateHash;
use lm_core::log::RoundLog;
use lm_core::talk::TalkParams;
use lm_core::PROTO_VERSION;
use lm_tools::runner::replay;

/// The phase-1 log, written by the phase-1 build before the bump.
fn phase1_log() -> Vec<u8> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/phase1-v1-seed1.lml");
    std::fs::read(&path).expect("the phase-1 log fixture is in the repository")
}

/// The state hashes of a log's segments, in tick order.
fn state_hashes(segments: &[lm_core::log::Segment]) -> Vec<(Tick, u64)> {
    segments
        .iter()
        .flat_map(|segment| segment.hashes.clone())
        .collect()
}

/// The same, for the version-2 segments a version-1 log is read into.
///
/// The refusals were added to a segment in version 3, so a version-1 file
/// carries the version-2 segment shape: the one without them.
fn state_hashes_v2(segments: &[lm_core::log::SegmentV2]) -> Vec<(Tick, u64)> {
    segments
        .iter()
        .flat_map(|segment| segment.hashes.clone())
        .collect()
}

#[test]
fn v1_log_replays_under_v2() {
    let bytes = phase1_log();
    let log =
        RoundLog::read(&mut Cursor::new(bytes)).expect("the phase-1 log reads under this build");

    // The log was migrated on the way in: the talk params are there and the
    // version moved.
    assert_eq!(log.header.proto_version, PROTO_VERSION);
    assert_eq!(log.header.talk, TalkParams::default());

    // The round the log records still replays to the hashes it recorded.
    let hashes = log.hashes();
    assert!(!hashes.is_empty(), "the phase-1 log recorded state hashes");
    assert_eq!(replay(&log).expect("the round replays"), hashes);
}

#[test]
fn v1_log_migration_keeps_the_state_hashes() {
    let bytes = phase1_log();
    let v1 = RoundLog::read_v1(&mut Cursor::new(bytes.clone())).expect("the phase-1 log reads");
    let migrated = RoundLog::read(&mut Cursor::new(bytes)).expect("the phase-1 log migrates");

    // The header is a new identity: the version and the talk params joined
    // its hash.
    assert_ne!(v1.header.state_hash(), migrated.header.hash());

    // The chain is re-rooted over the migrated header...
    assert_ne!(v1.segments[0].prev, migrated.segments[0].prev);
    assert_ne!(v1.segments[0].hash, migrated.segments[0].hash);
    assert_eq!(migrated.segments[0].prev, migrated.header.hash());
    for (index, segment) in migrated.segments.iter().enumerate() {
        assert_eq!(segment.hash, segment.compute_hash(), "segment {index}");
    }

    // ...and not one state hash moved: the world the log records did not
    // change.
    assert_eq!(
        state_hashes_v2(&v1.segments),
        state_hashes(&migrated.segments)
    );
    assert_eq!(state_hashes_v2(&v1.segments), migrated.hashes());
}
