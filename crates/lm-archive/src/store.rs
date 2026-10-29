//! The store: what the archive keeps on disk, and how.
//!
//! The layout is flat and boring, on purpose — an operator has to be able to
//! find a round with `ls`:
//!
//! ```text
//! <store_dir>/rounds/<seed>-<unix_secs>.lml   the round's log
//! <store_dir>/reports/<seed>-<unix_secs>.json  the verification report
//! <store_dir>/state.json                       the service's own state
//! ```
//!
//! Every one of those files is written **atomically**: the bytes go into a
//! temporary file next to the target (`<name>.lml.tmp`), they are flushed to
//! the disk, and only then are they renamed into place. A rename is atomic on
//! Linux within a filesystem, so a process that dies half way through a write
//! leaves the temporary behind and **never** a half-written round: the
//! verifier only ever sees a file that was written whole.
//!
//! The archive never rewrites a round. It stores what it was given, it
//! replays it, and it reports what the replay found.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use lm_core::log::RoundLog;
use serde::{Deserialize, Serialize};

use crate::verify::VerifyReport;

/// The name of a stored round: the round's seed and the second it arrived.
///
/// The seed says *which* round it is and the timestamp says *when*, so two
/// rounds of the same seed never collide and the file name sorts into the
/// order the rounds arrived in.
pub fn round_name(seed: u64, unix_secs: u64) -> String {
    format!("{seed}-{unix_secs}")
}

/// The seconds since the epoch, now.
///
/// The clock is only used for the file names; the round itself never reads it
/// (the sim is a pure function of its header and its decisions).
pub fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0)
}

/// The temporary file a write goes through: `<name>.tmp`.
pub fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    path.with_file_name(name)
}

/// Write a file atomically: the bytes go into a temporary file next to the
/// target, and the temporary is renamed over the target when it is whole.
///
/// The rename is atomic on Linux within a filesystem, so the target either
/// does not exist or holds the complete file. There is no state in between
/// that a reader can see.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomic_with(path, |file| file.write_all(bytes))
}

/// Write a file atomically, with the body written by `fill`.
///
/// `fill` writes the file's bytes into the temporary file. If it fails, the
/// temporary is removed and the target is left alone; if the process dies
/// inside it, the temporary stays and the target is still never created —
/// which is the whole point of the temporary-and-rename dance.
pub fn write_atomic_with<F>(path: &Path, fill: F) -> io::Result<()>
where
    F: FnOnce(&mut File) -> io::Result<()>,
{
    let tmp = temp_path(path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let written = (|| -> io::Result<()> {
        let mut file = File::create(&tmp)?;
        fill(&mut file)?;
        file.flush()?;
        // The bytes leave the process before the name does.
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)?;
        Ok(())
    })();

    if written.is_err() {
        // A clean failure leaves nothing behind. A crash inside `fill` never
        // reaches this line, and the temporary survives it — which is what
        // the operator finds on the disk after a `kill -9`.
        let _ = fs::remove_file(&tmp);
    }
    written
}

/// The state the service keeps about itself.
///
/// It is the service's own bookkeeping — what it has taken in, what it has
/// verified — and it is written the same way the rounds are: atomically, so a
/// reader never sees half a state file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveState {
    /// The name of the last round that verified clean.
    pub last_verified_round: Option<String>,
    /// How many rounds have been taken in.
    pub rounds_ingested: u64,
    /// How many of them verified clean.
    pub rounds_verified: u64,
    /// How many of them did not.
    pub rounds_failed: u64,
}

/// The archive's directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// A store at `root`.
    pub fn new(root: impl Into<PathBuf>) -> Store {
        Store { root: root.into() }
    }

    /// The directory the store lives in.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The directory the rounds live in.
    pub fn rounds_dir(&self) -> PathBuf {
        self.root.join("rounds")
    }

    /// The directory the reports live in.
    pub fn reports_dir(&self) -> PathBuf {
        self.root.join("reports")
    }

    /// The service's own state file.
    pub fn state_path(&self) -> PathBuf {
        self.root.join("state.json")
    }

    /// The file a round of this name lives in.
    pub fn round_path(&self, name: &str) -> PathBuf {
        self.rounds_dir().join(format!("{name}.lml"))
    }

    /// The file the report of a round of this name lives in.
    pub fn report_path(&self, name: &str) -> PathBuf {
        self.reports_dir().join(format!("{name}.json"))
    }

    /// The temporary file a round of this name is written through.
    pub fn round_temp_path(&self, name: &str) -> PathBuf {
        temp_path(&self.round_path(name))
    }

    /// Make sure the directories of the store are there.
    pub fn ensure_layout(&self) -> io::Result<()> {
        fs::create_dir_all(self.rounds_dir())?;
        fs::create_dir_all(self.reports_dir())?;
        Ok(())
    }

    /// A name for a round of this seed that nothing has taken yet.
    ///
    /// The archive is written by one worker at a time, so a name that is free
    /// now stays free until the write; the walk is a belt-and-braces guard
    /// against two rounds of the same seed arriving in the same second.
    pub fn free_round_name(&self, seed: u64, unix_secs: u64) -> String {
        let mut secs = unix_secs;
        for _ in 0..1_000 {
            let name = round_name(seed, secs);
            if !self.round_path(&name).exists() {
                return name;
            }
            secs = secs.saturating_add(1);
        }
        round_name(seed, unix_secs)
    }

    /// Store a round: the log is written whole, or not at all.
    pub fn save_round(&self, name: &str, log: &RoundLog) -> io::Result<PathBuf> {
        self.ensure_layout()?;
        let mut bytes = Vec::new();
        log.write(&mut bytes)?;
        let path = self.round_path(name);
        write_atomic(&path, &bytes)?;
        Ok(path)
    }

    /// Write the verification report of a round.
    pub fn save_report(&self, name: &str, report: &VerifyReport) -> io::Result<PathBuf> {
        self.ensure_layout()?;
        let json = serde_json::to_vec_pretty(report)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))?;
        let path = self.report_path(name);
        write_atomic(&path, &json)?;
        Ok(path)
    }

    /// The service's own state.
    ///
    /// A state file that cannot be read is not fatal: the service starts with
    /// an empty state and carries on, because a broken bookkeeping file must
    /// never stop the archive from taking rounds in.
    pub fn read_state(&self) -> ArchiveState {
        let Ok(text) = fs::read_to_string(self.state_path()) else {
            return ArchiveState::default();
        };
        serde_json::from_str(&text).unwrap_or_default()
    }

    /// Write the service's own state, atomically.
    pub fn write_state(&self, state: &ArchiveState) -> io::Result<()> {
        if let Some(parent) = self.state_path().parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_vec_pretty(state)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))?;
        write_atomic(&self.state_path(), &json)
    }

    /// The rounds in the store, oldest first.
    ///
    /// The order is the order the rounds arrived in: the file's own
    /// timestamp, with the timestamp in the name as the tiebreaker for the
    /// rounds that arrived in the same second.
    pub fn rounds(&self) -> io::Result<Vec<(String, PathBuf)>> {
        let mut out: Vec<(String, PathBuf, SystemTime)> = Vec::new();
        let Ok(entries) = fs::read_dir(self.rounds_dir()) else {
            return Ok(Vec::new());
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("lml") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let when = fs::metadata(&path)
                .and_then(|meta| meta.modified())
                .unwrap_or(UNIX_EPOCH);
            out.push((stem.to_owned(), path, when));
        }
        out.sort_by(|a, b| (a.2, round_seconds(&a.0), &a.0).cmp(&(b.2, round_seconds(&b.0), &b.0)));
        Ok(out
            .into_iter()
            .map(|(name, path, _)| (name, path))
            .collect())
    }

    /// Keep only the newest `keep` rounds; the rest go, with their reports.
    ///
    /// `keep == 0` keeps everything: the default is "keep all".
    pub fn prune(&self, keep: usize) -> io::Result<usize> {
        if keep == 0 {
            return Ok(0);
        }
        let rounds = self.rounds()?;
        if rounds.len() <= keep {
            return Ok(0);
        }
        let mut removed = 0;
        for (name, path) in &rounds[..rounds.len() - keep] {
            if fs::remove_file(path).is_ok() {
                removed += 1;
            }
            let _ = fs::remove_file(self.report_path(name));
        }
        Ok(removed)
    }
}

/// The seconds in a round's name, or 0 when the name does not carry one.
fn round_seconds(name: &str) -> u64 {
    name.rsplit('-')
        .next()
        .and_then(|secs| secs.parse::<u64>().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{round, Scratch};

    #[test]
    fn store_layout_is_flat() {
        let scratch = Scratch::new("layout");
        let store = Store::new(scratch.path());
        assert_eq!(store.rounds_dir(), scratch.path().join("rounds"));
        assert_eq!(store.reports_dir(), scratch.path().join("reports"));
        assert_eq!(store.state_path(), scratch.path().join("state.json"));
        assert_eq!(
            store.round_path("1-1700000000"),
            scratch.path().join("rounds").join("1-1700000000.lml")
        );
        assert_eq!(
            store.report_path("1-1700000000"),
            scratch.path().join("reports").join("1-1700000000.json")
        );
        assert_eq!(
            store.round_temp_path("1-1700000000"),
            scratch.path().join("rounds").join("1-1700000000.lml.tmp")
        );
    }

    #[test]
    fn a_round_is_stored_and_reads_back_whole() {
        let scratch = Scratch::new("store-round");
        let store = Store::new(scratch.path());
        let log = round(1, 300);

        let path = store
            .save_round("1-1700000000", &log)
            .expect("the round is stored");
        let mut file = File::open(&path).expect("the round is there");
        let read = RoundLog::read(&mut file).expect("the round reads back");
        assert_eq!(read, log);
    }

    #[test]
    fn atomic_write_leaves_no_partial() {
        let scratch = Scratch::new("atomic");
        let store = Store::new(scratch.path());
        store.ensure_layout().expect("the layout is there");
        let path = store.round_path("1-1700000000");

        // A write that dies half way through: the panic stands in for the
        // `kill -9`, so nothing unwinds and nothing is cleaned up.
        let aborted = std::panic::catch_unwind(|| {
            write_atomic_with(&path, |file| {
                file.write_all(b"half a round").expect("the bytes go in");
                panic!("the process died mid write");
            })
        });
        assert!(aborted.is_err(), "the write was interrupted");

        // The round itself is not there: a reader never sees half a round.
        assert!(!path.exists(), "the round file was never created");
        // The temporary is, and it is the only thing the write left behind.
        assert!(store.round_temp_path("1-1700000000").exists());
        let names: Vec<String> = fs::read_dir(store.rounds_dir())
            .expect("the rounds directory is there")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["1-1700000000.lml.tmp".to_owned()]);
    }

    #[test]
    fn a_clean_failure_leaves_no_temporary_behind() {
        let scratch = Scratch::new("atomic-error");
        let store = Store::new(scratch.path());
        store.ensure_layout().expect("the layout is there");
        let path = store.round_path("1-1700000000");

        let err = write_atomic_with(&path, |file| {
            file.write_all(b"half a round").expect("the bytes go in");
            Err(io::Error::other("the disk said no"))
        })
        .expect_err("the write failed");
        assert_eq!(err.kind(), io::ErrorKind::Other);

        assert!(!path.exists());
        assert!(!store.round_temp_path("1-1700000000").exists());
    }

    #[test]
    fn the_state_is_written_atomically_and_reads_back() {
        let scratch = Scratch::new("state");
        let store = Store::new(scratch.path());
        let state = ArchiveState {
            last_verified_round: Some("1-1700000000".to_owned()),
            rounds_ingested: 3,
            rounds_verified: 2,
            rounds_failed: 1,
        };
        store.write_state(&state).expect("the state is written");
        assert_eq!(store.read_state(), state);
        assert!(store.state_path().exists());
        assert!(!store.state_path().with_extension("json.tmp").exists());
    }

    #[test]
    fn a_broken_state_file_does_not_stop_the_archive() {
        let scratch = Scratch::new("broken-state");
        let store = Store::new(scratch.path());
        store.ensure_layout().expect("the layout is there");
        std::fs::write(store.state_path(), b"{not json").expect("the broken state is written");
        assert_eq!(store.read_state(), ArchiveState::default());
    }

    #[test]
    fn pruning_keeps_the_newest_rounds() {
        let scratch = Scratch::new("prune");
        let store = Store::new(scratch.path());
        let log = round(1, 300);
        for secs in [10u64, 20, 30] {
            store
                .save_round(&round_name(1, secs), &log)
                .expect("the round is stored");
            store
                .save_report(
                    &round_name(1, secs),
                    &crate::verify::VerifyReport::default(),
                )
                .expect("the report is stored");
        }

        let removed = store.prune(2).expect("the store is pruned");
        assert_eq!(removed, 1);
        let left: Vec<String> = store
            .rounds()
            .expect("the rounds are listed")
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(left, vec!["1-20".to_owned(), "1-30".to_owned()]);
        assert!(!store.report_path("1-10").exists());
    }

    #[test]
    fn keeping_every_round_prunes_nothing() {
        let scratch = Scratch::new("prune-none");
        let store = Store::new(scratch.path());
        let log = round(1, 300);
        store
            .save_round(&round_name(1, 10), &log)
            .expect("the round is stored");
        assert_eq!(store.prune(0).expect("the store is pruned"), 0);
        assert_eq!(store.rounds().expect("the rounds are listed").len(), 1);
    }

    #[test]
    fn a_free_name_steps_past_the_taken_one() {
        let scratch = Scratch::new("free-name");
        let store = Store::new(scratch.path());
        let log = round(1, 300);
        store
            .save_round(&round_name(1, 10), &log)
            .expect("the round is stored");
        assert_eq!(store.free_round_name(1, 10), "1-11");
        assert_eq!(store.free_round_name(2, 10), "2-10");
    }
}
