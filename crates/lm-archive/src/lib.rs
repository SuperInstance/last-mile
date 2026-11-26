//! `lm-archive`: the Orin's round-log archive and verifier.
//!
//! The archive is the third node of the M1 loop. The ProArt plays a round and
//! pushes the log; the archive stores it, replays it headless and checks
//! **every** state hash; the ProArt's HUD shows the verdict. Spec §7.
//!
//! Three properties shape the whole crate:
//!
//! - **The archive never writes to a round.** It stores, replays and reports.
//!   A hash that does not match is *reported*, with the first divergent tick;
//!   it is never "fixed", and a log is never rewritten.
//! - **The archive believes nothing.** A damaged log — a wrong magic, a
//!   truncated file, a broken hash chain — is refused **by name**, and a
//!   refusal never becomes an `ok`. The property that matters most here is
//!   that there is no false OK.
//! - **The archive uses no floats at all.** It is a verifier, so it inherits
//!   `lm-core`'s discipline: the whole crate is integer arithmetic, exactly
//!   like the core it checks.
//!
//! ## The modules
//!
//! - [`store`] — the on-disk layout: the rounds, the reports and the service's
//!   own state. Every file is written **atomically** (a temporary file, then a
//!   rename), so a half-written round is never visible to the verifier.
//! - [`protocol`] — the wire: framed `lm_proto::Msg` over a TCP stream, and
//!   the ingest that turns a pushed round into a stored, verified round.
//! - [`verify`] — the verifier: the headless replay and the report it writes
//!   for the ProArt's HUD.
//! - [`server`] — the service: the accept loop, a thread per connection, and
//!   the verification worker that keeps a slow replay off the accept loop.
//! - [`priority`] — the yielding: the CPU and I/O priority the service runs
//!   at, applied by re-executing itself under `nice` and `ionice`.
//! - [`golden`] — the cross-architecture golden check, run natively on the
//!   Orin so that the ProArt's HUD can show the ARM verdict.

#![deny(
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::cast_precision_loss
)]
#![forbid(unsafe_code)]

pub mod golden;
pub mod priority;
pub mod protocol;
pub mod server;
pub mod store;
pub mod verify;

use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use golden::{GoldenMismatch, GoldenReport};
pub use priority::{apply_to_self, is_low_priority};
pub use protocol::{handle_connection, push_round, IngestError, IngestReport, PushError};
pub use server::{Server, Stats, Verifier};
pub use store::{ArchiveState, Store};
pub use verify::{verify_file, verify_log, VerifyError, VerifyReport};

/// The address the service listens on, unless the config says otherwise.
pub const DEFAULT_LISTEN: &str = "0.0.0.0:8790";

/// The directory the rounds live in, unless the config says otherwise.
pub const DEFAULT_STORE_DIR: &str = "/var/lib/last-mile/archive";

/// The biggest log the service accepts, unless the config says otherwise:
/// 256 MiB.
pub const DEFAULT_MAX_LOG_BYTES: u64 = 256 * 1024 * 1024;

/// How long a connection may stay silent before the service drops it,
/// unless the config says otherwise: 30 seconds.
pub const DEFAULT_READ_TIMEOUT_S: u64 = 30;

/// How many rounds may wait for the verification worker at once, unless the
/// config says otherwise.
pub const DEFAULT_QUEUE_CAPACITY: usize = 8;

/// The configuration of the archive service.
///
/// Every value has a default, and every value can be overridden by an
/// `archive.toml` (D19: if more than one option works, it is a parameter).
/// The file is optional: a field the file does not mention keeps its default.
///
/// ```toml
/// listen = "0.0.0.0:8790"
/// store_dir = "/var/lib/last-mile/archive"
/// max_log_bytes = 268435456
/// verify_on_arrival = true
/// retain_rounds = 0
/// nice = 10
/// ionice_class = 3
/// workers = 1
/// read_timeout_s = 30
/// queue_capacity = 8
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ArchiveConfig {
    /// The address the service listens on.
    pub listen: String,
    /// The directory the rounds, the reports and the state live in.
    pub store_dir: PathBuf,
    /// The biggest a pushed round may be, in bytes. A frame that claims to be
    /// bigger is refused **before** it is buffered.
    pub max_log_bytes: u64,
    /// Whether a round is verified as soon as it arrives.
    pub verify_on_arrival: bool,
    /// How many rounds to keep; 0 keeps every round. The oldest go first.
    pub retain_rounds: usize,
    /// The niceness the service runs at: higher means less CPU.
    pub nice: i32,
    /// The I/O scheduling class the service runs at: 3 is idle.
    pub ionice_class: u8,
    /// How many verification workers the service runs.
    pub workers: usize,
    /// How long a connection may stay silent before the service drops it.
    ///
    /// A peer that opens a socket and says nothing must not pin a thread of
    /// the service forever.
    pub read_timeout_s: u64,
    /// How many rounds may wait for the verification worker at once.
    ///
    /// The queue is the archive's buffer cap: a burst of pushes is held, not
    /// swallowed, and a full queue is backpressure on the push rather than
    /// on the accept loop.
    pub queue_capacity: usize,
}

impl Default for ArchiveConfig {
    fn default() -> Self {
        ArchiveConfig {
            listen: DEFAULT_LISTEN.to_owned(),
            store_dir: PathBuf::from(DEFAULT_STORE_DIR),
            max_log_bytes: DEFAULT_MAX_LOG_BYTES,
            verify_on_arrival: true,
            retain_rounds: 0,
            nice: priority::DEFAULT_NICE,
            ionice_class: priority::DEFAULT_IO_CLASS,
            workers: 1,
            read_timeout_s: DEFAULT_READ_TIMEOUT_S,
            queue_capacity: DEFAULT_QUEUE_CAPACITY,
        }
    }
}

impl ArchiveConfig {
    /// Read the config from a TOML file.
    ///
    /// A field the file does not mention keeps its default, so a file with a
    /// single line in it is a valid config.
    pub fn from_toml(path: &Path) -> Result<ArchiveConfig, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|err| ConfigError::Read {
            path: path.to_path_buf(),
            message: err.to_string(),
        })?;
        toml::from_str(&text).map_err(|err| ConfigError::Parse {
            path: path.to_path_buf(),
            message: err.to_string(),
        })
    }

    /// Read the config from a TOML file, or fall back to the defaults when
    /// the file is not there.
    ///
    /// The config file is optional by design: the service runs with the
    /// defaults on a box that has never been configured.
    pub fn load(path: &Path) -> Result<ArchiveConfig, ConfigError> {
        if !path.exists() {
            return Ok(ArchiveConfig::default());
        }
        ArchiveConfig::from_toml(path)
    }

    /// The store this config points at.
    pub fn store(&self) -> Store {
        Store::new(self.store_dir.clone())
    }
}

/// Why a config file could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigError {
    /// The file could not be read.
    Read {
        /// The file that could not be read.
        path: PathBuf,
        /// What went wrong.
        message: String,
    },
    /// The file is not a config this build understands.
    Parse {
        /// The file that could not be parsed.
        path: PathBuf,
        /// What went wrong.
        message: String,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Read { path, message } => {
                write!(f, "config file {}: {message}", path.display())
            }
            ConfigError::Parse { path, message } => {
                write!(f, "config file {}: {message}", path.display())
            }
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
pub(crate) mod testing {
    //! The bits the tests share: a scratch directory that cleans up after
    //! itself, and a round to push.

    use std::fs;
    use std::path::{Path, PathBuf};

    use lm_core::Header;

    /// A directory of its own for one test, removed when the test ends.
    pub struct Scratch {
        path: PathBuf,
    }

    impl Scratch {
        /// A scratch directory named after the test.
        ///
        /// The process id keeps two runs of the suite apart, and the name
        /// keeps the tests of one run apart.
        pub fn new(name: &str) -> Scratch {
            let path =
                std::env::temp_dir().join(format!("lm-archive-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("the scratch directory is created");
            Scratch { path }
        }

        /// The directory itself.
        pub fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    /// The reflex the archive's tests replay: an empty one is enough for the
    /// plumbing.
    pub const REFLEX: &str = "fn reflex(v){ #{ hold: true } }";

    /// A round of `ticks` ticks with no injected decisions.
    ///
    /// The tick count is a multiple of the hash interval, so the round's last
    /// tick is a tick that carries a hash: a push of this round reconstructs
    /// into exactly this log.
    pub fn round(seed: u64, ticks: u64) -> lm_core::log::RoundLog {
        let (log, _) = lm_tools::runner::run(Header::default_round(seed, REFLEX), ticks, &[]);
        log
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_ones_the_table_names() {
        let cfg = ArchiveConfig::default();
        assert_eq!(cfg.listen, "0.0.0.0:8790");
        assert_eq!(cfg.store_dir, PathBuf::from("/var/lib/last-mile/archive"));
        assert_eq!(cfg.max_log_bytes, 256 * 1024 * 1024);
        assert!(cfg.verify_on_arrival);
        assert_eq!(cfg.retain_rounds, 0);
        assert_eq!(cfg.nice, 10);
        assert_eq!(cfg.ionice_class, 3);
        assert_eq!(cfg.workers, 1);
        assert_eq!(cfg.read_timeout_s, 30);
        assert_eq!(cfg.queue_capacity, 8);
    }

    #[test]
    fn a_partial_config_file_keeps_the_other_defaults() {
        let scratch = testing::Scratch::new("partial-config");
        let path = scratch.path().join("archive.toml");
        std::fs::write(&path, "listen = \"127.0.0.1:9999\"\n").expect("the config is written");

        let cfg = ArchiveConfig::from_toml(&path).expect("the config reads");
        assert_eq!(cfg.listen, "127.0.0.1:9999");
        assert_eq!(cfg.max_log_bytes, ArchiveConfig::default().max_log_bytes);
        assert_eq!(cfg.store_dir, ArchiveConfig::default().store_dir);
        assert!(cfg.verify_on_arrival);
    }

    #[test]
    fn a_missing_config_file_falls_back_to_the_defaults() {
        let scratch = testing::Scratch::new("missing-config");
        let cfg = ArchiveConfig::load(&scratch.path().join("archive.toml"))
            .expect("a missing file is not an error");
        assert_eq!(cfg, ArchiveConfig::default());
    }

    #[test]
    fn a_broken_config_file_is_named() {
        let scratch = testing::Scratch::new("broken-config");
        let path = scratch.path().join("archive.toml");
        std::fs::write(&path, "listen = 7\n").expect("the config is written");

        let err = ArchiveConfig::from_toml(&path).expect_err("a broken config is refused");
        assert!(matches!(err, ConfigError::Parse { .. }));
        assert!(err.to_string().contains("archive.toml"));
    }

    #[test]
    fn the_config_points_at_a_store() {
        let cfg = ArchiveConfig::default();
        assert_eq!(cfg.store().root(), cfg.store_dir.as_path());
    }
}
