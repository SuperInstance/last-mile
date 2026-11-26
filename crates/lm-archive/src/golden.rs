//! The cross-architecture golden check, run natively on the Orin.
//!
//! The proof that x86 and ARM play the same game is a **golden check**: three
//! pinned rounds, three pinned final hashes, and a fresh 3000-tick run of
//! each on the machine that is asking. The hashes were pinned on the ProArt
//! (x86) in Phase 1, and the Orin (aarch64) has to reach the same numbers —
//! otherwise the two machines did not play the same game, and every round
//! that crosses between them is a round that cannot be compared.
//!
//! `lm golden` already does this check (`cargo run --release --bin lm --
//! golden` prints `OK 3/3` and exits 0). This module is the same check, made
//! a **service** of it: it runs natively on the Orin, and it writes the
//! verdict into the store's report directory so that the ProArt's HUD can
//! show the ARM verdict without anybody having to copy a file around.
//!
//! ## No false OK
//!
//! A check that cannot be run is a check that failed: the report says `ok ==
//! false` and names the reason. A wrong pinned hash is reported **by name**,
//! with both hashes, and the command exits non-zero. That is the property
//! that makes the check worth running at all.

use std::fmt;
use std::path::{Path, PathBuf};

use lm_tools::runner::{self, Golden, GOLDEN_TICKS};
use serde::{Deserialize, Serialize};

use crate::store::Store;

pub use lm_tools::runner::GoldenError;

/// The name of the golden report in the store's report directory.
pub const GOLDEN_REPORT_NAME: &str = "golden.json";

/// A seed whose pinned hash the round did not reach.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoldenMismatch {
    /// The seed the check covers.
    pub seed: u64,
    /// The hash the golden file pins.
    pub pinned: u64,
    /// The hash the round on this machine produced.
    pub computed: u64,
}

impl fmt::Display for GoldenMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "seed {}: golden says {:#018x}, the round says {:#018x}",
            self.seed, self.pinned, self.computed
        )
    }
}

/// What the golden check found.
///
/// The report is what the ProArt's HUD shows: the verdict, how many seeds
/// matched, and — when they did not — which seed, and the two hashes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoldenReport {
    /// True when every pinned hash was reached.
    pub ok: bool,
    /// How many ticks each round ran.
    pub ticks: u64,
    /// How many seeds matched.
    pub matched: usize,
    /// How many seeds the check covers.
    pub total: usize,
    /// The architecture the check ran on: `x86_64` or `aarch64`.
    pub arch: String,
    /// The seeds that did not match.
    pub mismatches: Vec<GoldenMismatch>,
    /// Why the check failed, in words. Always there when `ok` is false.
    pub reason: Option<String>,
}

impl Default for GoldenReport {
    /// A report of a check that was **not** run.
    ///
    /// The default is the safe one: a report nobody wrote says "no", never
    /// "yes".
    fn default() -> Self {
        GoldenReport {
            ok: false,
            ticks: 0,
            matched: 0,
            total: 0,
            arch: String::new(),
            mismatches: Vec::new(),
            reason: Some("the golden check was not run".to_owned()),
        }
    }
}

impl GoldenReport {
    /// The summary a human reads: the verdict, and what did not match.
    ///
    /// The wording is the one `lm golden` uses, so the two checks read the
    /// same: `OK 3/3` when they agree, and the seed and the two hashes when
    /// they do not.
    pub fn summary(&self) -> String {
        if self.ok {
            return format!("OK {}/{}", self.matched, self.total);
        }
        let mut text = format!("{}/{} seeds match", self.matched, self.total);
        for mismatch in &self.mismatches {
            text.push_str(&format!("\n{mismatch}"));
        }
        if let Some(reason) = &self.reason {
            text.push_str(&format!("\n{reason}"));
        }
        text
    }
}

/// The path of the golden file: the workspace's own `tests/golden.toml`.
pub fn default_path() -> PathBuf {
    runner::golden_path()
}

/// The architecture this check is running on.
pub fn arch() -> String {
    std::env::consts::ARCH.to_owned()
}

/// Run the golden check against a golden file.
///
/// The file says how long its rounds run; that is the round to compare. The
/// rounds are played fresh, on this machine, and every pinned hash is
/// compared with the hash the round produced here.
pub fn check(path: &Path) -> Result<GoldenReport, GoldenError> {
    let golden = Golden::read(path)?;
    let ticks = if golden.ticks == 0 {
        GOLDEN_TICKS
    } else {
        golden.ticks
    };

    let mut matched = 0usize;
    let mut mismatches = Vec::new();
    let mut total = 0usize;
    for (seed, computed) in runner::golden_hashes(ticks) {
        total += 1;
        match golden.hash_of(seed) {
            Ok(pinned) if pinned == computed => matched += 1,
            Ok(pinned) => mismatches.push(GoldenMismatch {
                seed,
                pinned,
                computed,
            }),
            // A seed the file does not pin is a mismatch too: the file is
            // the contract, and a missing entry is not a match.
            Err(_err) => mismatches.push(GoldenMismatch {
                seed,
                pinned: 0,
                computed,
            }),
        }
    }

    let ok = mismatches.is_empty() && matched == total && total > 0;
    let reason = if ok {
        None
    } else if total == 0 {
        Some("the golden file covers no seeds".to_owned())
    } else {
        Some(format!(
            "{} of {} seeds did not match",
            mismatches.len(),
            total
        ))
    };
    Ok(GoldenReport {
        ok,
        ticks,
        matched,
        total,
        arch: arch(),
        mismatches,
        reason,
    })
}

/// Write the golden report into the store's report directory.
///
/// The report is written atomically, like every other file of the store, and
/// it lands next to the rounds' own reports: the ProArt reads that
/// directory, so the ARM verdict is where it expects to find it.
pub fn write_report(store: &Store, report: &GoldenReport) -> std::io::Result<PathBuf> {
    store.save_golden_report(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Scratch;

    /// The golden file the workspace ships.
    fn workspace_golden() -> PathBuf {
        default_path()
    }

    #[test]
    fn the_golden_check_passes_on_this_machine() {
        let report = check(&workspace_golden()).expect("the golden file reads");
        assert!(report.ok, "{}", report.summary());
        assert_eq!(report.matched, 3);
        assert_eq!(report.total, 3);
        assert_eq!(report.summary(), "OK 3/3");
        assert!(report.mismatches.is_empty());
        assert_eq!(report.reason, None);
    }

    #[test]
    fn golden_failure_is_reported() {
        // A deliberately wrong pinned hash must produce a named mismatch and
        // a report that says no — never a silent pass. That is the property
        // that makes the check worth running.
        let scratch = Scratch::new("golden-wrong");
        let path = scratch.path().join("golden.toml");
        let golden = Golden {
            ticks: 300,
            golden: [
                ("1".to_owned(), "0x0000000000000001".to_owned()),
                ("2".to_owned(), "0x0000000000000002".to_owned()),
                ("3".to_owned(), "0x0000000000000003".to_owned()),
            ]
            .into_iter()
            .collect(),
        };
        golden.write(&path).expect("the golden file is written");

        let report = check(&path).expect("the golden file reads");
        assert!(!report.ok, "a wrong pinned hash must not pass");
        assert_eq!(report.matched, 0);
        assert_eq!(report.total, 3);
        assert_eq!(report.mismatches.len(), 3);
        // The mismatch is named, with both hashes.
        let first = &report.mismatches[0];
        assert_eq!(first.seed, 1);
        assert_eq!(first.pinned, 1);
        assert_ne!(first.computed, first.pinned);
        assert!(report.summary().contains("seed 1"));
        assert!(report.summary().contains("golden says"));
        assert!(report.reason.is_some());
    }

    #[test]
    fn golden_report_is_recorded() {
        // The Orin's result lands in the store's report directory, where the
        // ProArt can read it.
        let scratch = Scratch::new("golden-report");
        let store = Store::new(scratch.path());
        let report = check(&workspace_golden()).expect("the golden file reads");
        let path = write_report(&store, &report).expect("the report is written");

        assert!(path.exists());
        assert_eq!(path, store.golden_report_path());
        assert!(path.starts_with(store.reports_dir()));

        // And the ProArt can read it back.
        let text = std::fs::read_to_string(&path).expect("the report reads");
        let read: GoldenReport = serde_json::from_str(&text).expect("the report parses");
        assert_eq!(read, report);
        assert!(read.ok);
        assert_eq!(read.arch, arch());
    }

    #[test]
    fn a_check_that_cannot_run_is_not_an_ok() {
        // A golden file that is not there is a failed check, not a pass.
        let err = check(Path::new("/nonexistent/golden.toml")).expect_err("the file is missing");
        assert!(matches!(err, GoldenError::Io(_)));
    }
}
