//! The verifier: replay every round and check every hash.
//!
//! The archive is the part of the M1 loop that does not trust anybody. The
//! ProArt plays a round on x86, pushes the log, and the Orin replays it from
//! the header and the stamped decisions — and compares **every** state hash
//! with the one the ProArt recorded. A difference means the two machines did
//! not play the same game, and it is reported with the **first divergent
//! tick** (spec §7, §8).
//!
//! The replay itself is not written here: `lm_tools::runner::replay` already
//! re-runs the sim over a log's stamped decisions and compares every hash, and
//! `ReplayError::Diverged { tick, expected, got }` already carries the first
//! tick the two rounds parted company on. What this module adds is the
//! **report**: the thing the ProArt's HUD shows and the store keeps.
//!
//! ## No false OK
//!
//! The property that matters most here: every way a round can fail to verify
//! leaves `ok == false` and a **named** reason. A wrong magic, a truncated
//! file, a broken hash chain, a round this build cannot play, a hash that
//! does not match — every one of them is refused by name, and none of them
//! is ever reported as a clean round.

use std::fmt;
use std::path::Path;

use lm_core::decision::Tick;
use lm_core::log::LogError;
use lm_core::rules::SimError;
use lm_tools::runner::{self, ReplayError};
use serde::ser::{SerializeStruct, Serializer};
use serde::Serialize;

/// What a verification found.
///
/// The report is the answer the ProArt's HUD shows and the store keeps: the
/// round's verdict, how many hashes were checked, and — when the round did
/// not verify — the first tick the two rounds disagreed on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct VerifyReport {
    /// True when the replay matched every hash the log recorded.
    pub ok: bool,
    /// How many state hashes were checked.
    pub hashes_checked: usize,
    /// The first tick the replay and the log disagreed on, with the hash the
    /// log recorded and the hash the replay saw.
    pub first_divergence: Option<(Tick, u64, u64)>,
    /// The hash of the round's header: the round's identity.
    pub header_hash: u64,
    /// Why the round did not verify, in words. Always there when `ok` is
    /// false: a refusal without a reason is not a refusal.
    pub reason: Option<String>,
    /// The failure itself, so a caller can match on the kind of damage
    /// instead of on a string.
    pub error: Option<VerifyError>,
}

impl Default for VerifyReport {
    /// A report of a round that was **not** verified.
    ///
    /// The default is the safe one: a report nobody wrote says "no", never
    /// "yes".
    fn default() -> Self {
        VerifyReport {
            ok: false,
            hashes_checked: 0,
            first_divergence: None,
            header_hash: 0,
            reason: Some("the round was not verified".to_owned()),
            error: None,
        }
    }
}

impl VerifyReport {
    /// A report of a round that verified clean.
    pub fn verified(hashes_checked: usize, header_hash: u64) -> VerifyReport {
        VerifyReport {
            ok: true,
            hashes_checked,
            first_divergence: None,
            header_hash,
            reason: None,
            error: None,
        }
    }

    /// A report of a round that did not verify, and why.
    pub fn failed(error: VerifyError, header_hash: u64) -> VerifyReport {
        VerifyReport {
            ok: false,
            hashes_checked: 0,
            first_divergence: None,
            header_hash,
            reason: Some(error.message()),
            error: Some(error),
        }
    }

    /// The summary a human reads: the verdict and the reason.
    pub fn summary(&self) -> String {
        if self.ok {
            return format!("OK: {} hashes checked", self.hashes_checked);
        }
        match self.first_divergence {
            Some((tick, expected, got)) => format!(
                "FAILED: diverged at tick {tick}: the log says {expected:#018x}, the replay saw {got:#018x} ({} hashes checked)",
                self.hashes_checked
            ),
            None => format!(
                "FAILED: {} ({} hashes checked)",
                self.reason.as_deref().unwrap_or("no reason given"),
                self.hashes_checked
            ),
        }
    }
}

/// Why a round did not verify.
///
/// The damage is kept in the shape it has: a log error is a `LogError`, a
/// round this build cannot play is a `SimError`, and a mismatch is a
/// mismatch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VerifyError {
    /// The log itself is damaged: it does not decode, or its hash chain does
    /// not hold.
    Log(LogError),
    /// The round cannot be played by this build: the header names a ruleset
    /// this build does not have, breaks a parameter, or carries a script that
    /// does not compile.
    Setup(SimError),
    /// The replay saw a different world than the log recorded.
    Diverged {
        /// The tick the two rounds first disagreed on.
        tick: Tick,
        /// The hash the log recorded.
        expected: u64,
        /// The hash the replay saw.
        got: u64,
    },
    /// The log could not be read at all.
    Io(String),
}

impl VerifyError {
    /// A short name for the kind of failure, for the report and the log.
    pub fn kind(&self) -> &'static str {
        match self {
            VerifyError::Log(LogError::BadMagic) => "log_bad_magic",
            VerifyError::Log(LogError::Version { .. }) => "log_version",
            VerifyError::Log(LogError::Truncated { .. }) => "log_truncated",
            VerifyError::Log(LogError::ChainBroken { .. }) => "log_chain_broken",
            VerifyError::Log(LogError::Decode { .. }) => "log_decode",
            VerifyError::Setup(_) => "setup",
            VerifyError::Diverged { .. } => "diverged",
            VerifyError::Io(_) => "io",
        }
    }

    /// What went wrong, in words.
    pub fn message(&self) -> String {
        match self {
            VerifyError::Log(err) => err.to_string(),
            VerifyError::Setup(err) => err.to_string(),
            VerifyError::Diverged {
                tick,
                expected,
                got,
            } => format!(
                "diverged at tick {tick}: the log says {expected:#018x}, the replay saw {got:#018x}"
            ),
            VerifyError::Io(message) => format!("the log could not be read: {message}"),
        }
    }
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message())
    }
}

impl std::error::Error for VerifyError {}

impl Serialize for VerifyError {
    /// The error is written as a kind and a message, so the report says what
    /// went wrong without the report itself having to understand `LogError`.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("VerifyError", 2)?;
        state.serialize_field("kind", self.kind())?;
        state.serialize_field("message", &self.message())?;
        state.end()
    }
}

/// Verify a round that is already in memory.
///
/// The round is replayed from its header and its stamped decisions, and every
/// state hash the log recorded is compared with the one the replay produced.
/// The first difference is reported, with the tick it happened on.
///
/// The log's hash chain is **not** checked here: a log that came off the disk
/// is checked by [`verify_file`], which reads it with the reader that
/// verifies the chain before handing it over.
pub fn verify_log(log: &lm_core::log::RoundLog) -> VerifyReport {
    let header_hash = log.header.hash();
    match runner::replay(log) {
        Ok(hashes) => VerifyReport::verified(hashes.len(), header_hash),
        Err(ReplayError::Diverged {
            tick,
            expected,
            got,
        }) => {
            // The hashes that matched before the round parted company.
            let checked = log.hashes().iter().filter(|(at, _)| *at <= tick).count();
            let error = VerifyError::Diverged {
                tick,
                expected,
                got,
            };
            VerifyReport {
                ok: false,
                hashes_checked: checked,
                first_divergence: Some((tick, expected, got)),
                header_hash,
                reason: Some(error.message()),
                error: Some(error),
            }
        }
        Err(ReplayError::Log(err)) => VerifyReport::failed(VerifyError::Log(err), header_hash),
        Err(ReplayError::Setup(err)) => VerifyReport::failed(VerifyError::Setup(err), header_hash),
    }
}

/// Verify a round that is on disk.
///
/// The file is read with `lm_tools::runner::read_log`, which checks the
/// version and the hash chain **before** the log is handed over: a damaged
/// file never reaches the replay. A file that cannot be read, decoded or
/// chained is refused by name, and the report says `ok == false`.
pub fn verify_file(path: &Path) -> VerifyReport {
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(err) => {
            return VerifyReport::failed(VerifyError::Io(format!("{}: {err}", path.display())), 0)
        }
    };
    match runner::read_log(&mut file) {
        Ok(log) => verify_log(&log),
        Err(ReplayError::Log(err)) => VerifyReport::failed(VerifyError::Log(err), 0),
        Err(ReplayError::Setup(err)) => VerifyReport::failed(VerifyError::Setup(err), 0),
        // The reader never diverges: it hands over a log or a reason.
        Err(ReplayError::Diverged {
            tick,
            expected,
            got,
        }) => VerifyReport::failed(
            VerifyError::Diverged {
                tick,
                expected,
                got,
            },
            0,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{round, Scratch};
    use lm_core::decision::{Action, Decision, Kind, Provenance, Source};
    use lm_core::log::MAGIC;
    use lm_core::Header;

    /// A decision of agent `agent`, applied on `tick`.
    fn decision(agent: u16, tick: Tick, seq: u32) -> Decision {
        Decision {
            apply_tick: tick,
            source: Source::Agent(agent),
            seq,
            kind: Kind::ActionOn(Action::Hold),
            provenance: Provenance::Test,
        }
    }

    #[test]
    fn verify_accepts_a_good_log() {
        let log = round(1, 300);
        let report = verify_log(&log);
        assert!(report.ok, "{}", report.summary());
        assert_eq!(report.hashes_checked, log.hashes().len());
        assert_eq!(report.hashes_checked, 10);
        assert_eq!(report.first_divergence, None);
        assert_eq!(report.header_hash, log.header.hash());
        assert_eq!(report.reason, None);
    }

    #[test]
    fn verify_accepts_a_good_log_on_disk() {
        let scratch = Scratch::new("verify-file");
        let log = round(1, 300);
        let path = scratch.path().join("a.lml");
        let mut bytes = Vec::new();
        log.write(&mut bytes).expect("the log writes");
        std::fs::write(&path, &bytes).expect("the log is on disk");

        let report = verify_file(&path);
        assert!(report.ok, "{}", report.summary());
        assert_eq!(report.hashes_checked, log.hashes().len());
    }

    #[test]
    fn verify_catches_a_tampered_hash() {
        let mut log = round(1, 300);
        // One hash is changed: the log now claims a world the round never had.
        let (tick, original) = log.segments[0].hashes[3];
        let tampered = original ^ 0x00ff_ffff_ffff_ffff;
        log.segments[0].hashes[3].1 = tampered;

        let report = verify_log(&log);
        assert!(!report.ok);
        let (at, expected, got) = report
            .first_divergence
            .expect("the round diverged, and the tick is named");
        assert_eq!(at, tick);
        // The two values are the ones the round and the log disagree about:
        // the hash the log claims, and the hash the replay actually saw.
        assert_eq!(expected, tampered);
        assert_eq!(got, original);
        // The hashes before the divergence were checked and matched.
        assert_eq!(report.hashes_checked, 4);
        assert!(report.reason.is_some());
    }

    #[test]
    fn verify_catches_a_tampered_decision() {
        // A round with a decision on tick 5, played for real.
        let header = Header::default_round(1, crate::testing::REFLEX);
        let inject = vec![(5u64, decision(0, 5, 1))];
        let (log, _) = runner::run(header, 300, &inject);
        assert!(
            !log.segments[0].decisions.is_empty(),
            "the decision is in the log"
        );

        // The decision's payload is changed: the round now plays differently.
        let mut tampered = log.clone();
        tampered.segments[0].decisions[0].d.kind =
            Kind::ActionOn(Action::Face(lm_core::fixed::Angle(1_024)));

        let report = verify_log(&tampered);
        assert!(!report.ok, "a changed decision is a changed round");
        let (at, _, _) = report.first_divergence.expect("the divergence is named");
        // The first hash the changed decision reaches is the tick the
        // divergence is reported on: the decision lands on tick 5, and the
        // state is hashed every 30 ticks.
        assert_eq!(
            at, 29,
            "the divergence is on the first hash after the decision"
        );
        assert!(report.reason.is_some());
    }

    #[test]
    fn a_decision_on_a_hash_tick_diverges_on_that_tick() {
        // The same thing, with the decision on a tick the round is hashed on:
        // the divergence is then the tick the decision applies, exactly.
        let header = Header::default_round(1, crate::testing::REFLEX);
        let inject = vec![(29u64, decision(0, 29, 1))];
        let (log, _) = runner::run(header, 300, &inject);
        assert!(!log.segments[0].decisions.is_empty());

        let mut tampered = log.clone();
        tampered.segments[0].decisions[0].d.kind =
            Kind::ActionOn(Action::Face(lm_core::fixed::Angle(1_024)));

        let report = verify_log(&tampered);
        assert!(!report.ok);
        let (at, _, _) = report.first_divergence.expect("the divergence is named");
        assert_eq!(at, 29, "the divergence is the tick the decision applies");
    }

    #[test]
    fn truncated_log_rejected() {
        let scratch = Scratch::new("truncated");
        let log = round(1, 300);
        let mut bytes = Vec::new();
        log.write(&mut bytes).expect("the log writes");
        let path = scratch.path().join("a.lml");
        std::fs::write(&path, &bytes[..bytes.len() / 2]).expect("the log is truncated");

        let report = verify_file(&path);
        assert!(!report.ok);
        assert_eq!(report.hashes_checked, 0);
        assert!(
            matches!(
                report.error,
                Some(VerifyError::Log(LogError::Truncated { .. }))
            ),
            "{:?}",
            report.error
        );
        assert!(report.reason.is_some());
    }

    #[test]
    fn broken_chain_rejected() {
        let scratch = Scratch::new("chain");
        // Two segments, so the second one's link to the first can be broken.
        let log = round(1, 600);
        assert_eq!(log.segments.len(), 2);
        let mut broken = log.clone();
        broken.segments[1].prev ^= 0x01;
        let mut bytes = Vec::new();
        broken.write(&mut bytes).expect("the log writes");
        let path = scratch.path().join("a.lml");
        std::fs::write(&path, &bytes).expect("the log is on disk");

        let report = verify_file(&path);
        assert!(!report.ok);
        assert!(
            matches!(
                report.error,
                Some(VerifyError::Log(LogError::ChainBroken { segment: 2 }))
            ),
            "{:?}",
            report.error
        );
        assert!(report.reason.is_some());
    }

    #[test]
    fn bad_magic_rejected() {
        let scratch = Scratch::new("magic");
        let log = round(1, 300);
        let mut bytes = Vec::new();
        log.write(&mut bytes).expect("the log writes");
        bytes[0] = b'X';
        let path = scratch.path().join("a.lml");
        std::fs::write(&path, &bytes).expect("the log is on disk");

        let report = verify_file(&path);
        assert!(!report.ok);
        assert!(
            matches!(report.error, Some(VerifyError::Log(LogError::BadMagic))),
            "{:?}",
            report.error
        );
        assert!(report.reason.is_some());
    }

    #[test]
    fn a_wrong_version_is_rejected() {
        let scratch = Scratch::new("version");
        let log = round(1, 300);
        let mut bytes = Vec::new();
        log.write(&mut bytes).expect("the log writes");
        let at = MAGIC.len();
        bytes[at..at + 2].copy_from_slice(&99u16.to_le_bytes());
        let path = scratch.path().join("a.lml");
        std::fs::write(&path, &bytes).expect("the log is on disk");

        let report = verify_file(&path);
        assert!(!report.ok);
        assert!(
            matches!(
                report.error,
                Some(VerifyError::Log(LogError::Version { found: 99 }))
            ),
            "{:?}",
            report.error
        );
    }

    #[test]
    fn a_missing_file_is_rejected() {
        let scratch = Scratch::new("missing");
        let report = verify_file(&scratch.path().join("nope.lml"));
        assert!(!report.ok);
        assert!(matches!(report.error, Some(VerifyError::Io(_))));
        assert!(report.reason.is_some());
    }

    #[test]
    fn no_false_ok() {
        // Every way a round can fail to verify has to leave `ok == false` and
        // a reason. This is the property that matters most in this crate.
        let scratch = Scratch::new("no-false-ok");
        let log = round(1, 300);
        let mut bytes = Vec::new();
        log.write(&mut bytes).expect("the log writes");

        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("empty file", Vec::new()),
            ("bad magic", {
                let mut b = bytes.clone();
                b[0] = b'X';
                b
            }),
            ("truncated", bytes[..bytes.len() / 2].to_vec()),
            ("wrong version", {
                let mut b = bytes.clone();
                let at = MAGIC.len();
                b[at..at + 2].copy_from_slice(&99u16.to_le_bytes());
                b
            }),
            ("garbage", vec![0xde, 0xad, 0xbe, 0xef, 0x00, 0x11]),
        ];

        for (name, body) in cases {
            let path = scratch.path().join("case.lml");
            std::fs::write(&path, &body).expect("the case is on disk");
            let report = verify_file(&path);
            assert!(!report.ok, "{name} must not be reported as a clean round");
            assert!(
                report.reason.as_deref().is_some_and(|r| !r.is_empty()),
                "{name} must be refused by name"
            );
            assert!(
                report.error.is_some(),
                "{name} must carry the kind of damage"
            );
        }

        // And a round that is not on disk at all.
        let report = verify_file(&scratch.path().join("nope.lml"));
        assert!(!report.ok);
        assert!(report.reason.is_some());
    }

    #[test]
    fn the_report_serialises_with_its_reason() {
        let scratch = Scratch::new("report-json");
        let log = round(1, 300);
        let mut bytes = Vec::new();
        log.write(&mut bytes).expect("the log writes");
        let path = scratch.path().join("a.lml");
        std::fs::write(&path, &bytes[..10]).expect("the log is broken");

        let report = verify_file(&path);
        let json = serde_json::to_string_pretty(&report).expect("the report serialises");
        assert!(json.contains("\"ok\": false"));
        assert!(json.contains("log_truncated"));
    }

    #[test]
    fn a_round_with_a_decision_replays_bit_exact() {
        // A round with a decision in it must still verify, or the tamper
        // test above proves nothing.
        let header = Header::default_round(1, crate::testing::REFLEX);
        let inject = vec![(5u64, decision(0, 5, 1))];
        let (log, _) = runner::run(header, 300, &inject);
        let report = verify_log(&log);
        assert!(report.ok, "{}", report.summary());
        assert!(!log.segments[0].decisions.is_empty());
    }
}
