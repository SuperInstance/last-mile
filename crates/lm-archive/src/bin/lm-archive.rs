//! `lm-archive`: the archive's command.
//!
//! Three things, and no more:
//!
//! - `verify <log>` — replay a round log and say whether every hash matched.
//! - `ingest <log>` — take a round log into the store, verify it and write
//!   the report, exactly as a push over the wire would.
//! - `paths` — say where the store keeps its rounds, its reports and its
//!   state.
//!
//! The service itself — the accept loop, the yielding, the priority — is the
//! next task of the phase; this command is the part of the archive that can
//! already be run by hand.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use lm_archive::protocol::ingest_round;
use lm_archive::store::Store;
use lm_archive::{verify_file, ArchiveConfig};
use lm_tools::runner::read_log;

/// What the command was asked to do.
enum Command {
    /// Replay a round log and report the verdict.
    Verify(PathBuf),
    /// Take a round log into the store and report the verdict.
    Ingest(PathBuf),
    /// Print the layout of the store.
    Paths,
}

/// How the command ended.
const OK: u8 = 0;
/// The round did not verify, or the command could not do its job.
const FAILED: u8 = 1;
/// The command was used wrongly.
const USAGE: u8 = 2;

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("lm-archive: {message}");
            ExitCode::from(FAILED)
        }
    }
}

/// Run the command.
fn run() -> Result<u8, String> {
    let mut args = std::env::args().skip(1);
    let mut config_path: Option<PathBuf> = None;
    let mut store_dir: Option<PathBuf> = None;

    // The options come before the command, the way the other tools take them.
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                let value = args.next().ok_or("--config needs a path")?;
                config_path = Some(PathBuf::from(value));
            }
            "--store" => {
                let value = args.next().ok_or("--store needs a path")?;
                store_dir = Some(PathBuf::from(value));
            }
            "--help" | "-h" => {
                print_help();
                return Ok(OK);
            }
            other => {
                let command = match other {
                    "verify" => {
                        Command::Verify(args.next().ok_or("verify needs a log file")?.into())
                    }
                    "ingest" => {
                        Command::Ingest(args.next().ok_or("ingest needs a log file")?.into())
                    }
                    "paths" => Command::Paths,
                    other => return Err(format!("unknown command {other:?}")),
                };
                return dispatch(command, config_path, store_dir);
            }
        }
    }
    print_help();
    Ok(USAGE)
}

/// Do the command.
fn dispatch(
    command: Command,
    config_path: Option<PathBuf>,
    store_dir: Option<PathBuf>,
) -> Result<u8, String> {
    let mut cfg = match &config_path {
        Some(path) => ArchiveConfig::load(path).map_err(|err| err.to_string())?,
        None => ArchiveConfig::default(),
    };
    if let Some(dir) = store_dir {
        cfg.store_dir = dir;
    }

    match command {
        Command::Verify(path) => {
            let report = verify_file(&path);
            println!("{}", report.summary());
            Ok(if report.ok { OK } else { FAILED })
        }
        Command::Ingest(path) => {
            let log = read_log_file(&path)?;
            let store = cfg.store();
            let name = store.free_round_name(log.header.seed, lm_archive::store::now_unix_secs());
            let report = ingest_round(&store, &log, &name, cfg.verify_on_arrival)
                .map_err(|err| err.to_string())?;
            // The store keeps as many rounds as the config says; 0 keeps them
            // all.
            if cfg.retain_rounds > 0 {
                let _ = store.prune(cfg.retain_rounds);
            }
            println!("stored {}", report.path.display());
            match &report.verified {
                Some(verified) => {
                    println!("{}", verified.summary());
                    Ok(if verified.ok { OK } else { FAILED })
                }
                None => {
                    println!("the round was stored; the service does not verify on arrival");
                    Ok(OK)
                }
            }
        }
        Command::Paths => {
            let store = cfg.store();
            print_paths(&store);
            Ok(OK)
        }
    }
}

/// Read a round log off the disk, naming the damage if there is any.
fn read_log_file(path: &Path) -> Result<lm_core::log::RoundLog, String> {
    let mut file = std::fs::File::open(path).map_err(|err| format!("{}: {err}", path.display()))?;
    read_log(&mut file).map_err(|err| format!("{}: {err}", path.display()))
}

/// Say where the store keeps its things.
fn print_paths(store: &Store) {
    println!("store:      {}", store.root().display());
    println!("rounds:     {}", store.rounds_dir().display());
    println!("reports:    {}", store.reports_dir().display());
    println!("state:      {}", store.state_path().display());
}

/// What the command can do.
fn print_help() {
    println!("lm-archive — the Orin archive: store round logs and verify every hash");
    println!();
    println!("usage:");
    println!("  lm-archive verify <log.lml>       replay a round and report the verdict");
    println!("  lm-archive ingest <log.lml>       store a round, verify it, write the report");
    println!("  lm-archive paths                  say where the store keeps its files");
    println!();
    println!("options:");
    println!("  --config <archive.toml>           read the config from a file");
    println!("  --store <dir>                     use this store directory");
}
