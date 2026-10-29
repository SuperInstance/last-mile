//! Recording a round: where the log goes, and how it gets there.
//!
//! A round the host plays is written down: one file per round, under `runs/`,
//! named `<seed>-<unix_secs>.lml`. The name says which round it is -- the seed
//! it was played from -- and when it was played -- the second it was recorded
//! -- so two rounds of the same seed never fight over one name.
//!
//! The file is the log of the round, sealed the way [`crate::HostLoop::finish`]
//! seals it: the magic, the version, the header and every segment. It is the
//! same file the headless tools write, so `lm replay` reads it and the replay
//! viewer opens it.
//!
//! The directory is made when the file is written, so a round can be recorded
//! into a `runs/` that does not exist yet.

use std::io;
use std::path::{Path, PathBuf};

use lm_core::log::RoundLog;

/// The directory the rounds are recorded in.
pub const RUNS_DIR: &str = "runs";

/// The path a round is recorded to: `runs/<seed>-<unix_secs>.lml`.
///
/// The seed is the one the round was played from, and the seconds are the
/// ones the round was recorded at -- the wall clock, as a count of seconds
/// since the epoch.
pub fn path_for(seed: u64, unix_secs: u64) -> PathBuf {
    let mut path = PathBuf::from(RUNS_DIR);
    path.push(format!("{seed}-{unix_secs}.lml"));
    path
}

/// Write a round's log to its file, making the directory if it is not there.
///
/// The file is written whole or not at all: it is created, and the log is
/// written into it. A file that cannot be written is an error the caller can
/// name, never a half-written round left behind.
pub fn write(log: &RoundLog, path: &Path) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    let mut file = std::fs::File::create(path)?;
    log.write(&mut file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lm_core::header::Header;
    use lm_tools::runner::DEFAULT_REFLEX;

    /// The name of a round: the seed, then the second it was recorded at.
    #[test]
    fn the_name_of_a_round_says_what_it_is() {
        assert_eq!(
            path_for(7, 1_700_000_000),
            PathBuf::from("runs/7-1700000000.lml")
        );
    }

    /// A round written to its file is the round the tools read back.
    #[test]
    fn a_written_round_is_the_round() {
        let header = Header::default_round(1, DEFAULT_REFLEX);
        let (log, _) = lm_tools::runner::run(header, 60, &[]);

        let mut path = std::env::temp_dir();
        path.push(format!("lm-host-model-record-{}.lml", std::process::id()));
        write(&log, &path).expect("the round is written");

        let mut file = std::fs::File::open(&path).expect("the file opens");
        let read = lm_tools::runner::read_log(&mut file).expect("the file is a round log");
        assert_eq!(read, log);

        let _ = std::fs::remove_file(&path);
    }
}
