//! `lm-archive`: the archive's command.
//!
//! Five things, and no more:
//!
//! - `serve` — run the service: the accept loop, the verification worker and
//!   the yielding to the desktop.
//! - `verify <log>` — replay a round log and say whether every hash matched.
//! - `ingest <log>` — take a round log into the store, verify it and write
//!   the report, exactly as a push over the wire would.
//! - `push <log> <addr>` — the ProArt's side of the wire: push a round to an
//!   archive and print the verdict it sends back.
//! - `golden` — run the cross-architecture golden check natively, and record
//!   the verdict in the store's report directory.
//! - `paths` — say where the store keeps its rounds, its reports and its
//!   state.
//!
//! The service is the part of the archive that runs on the Orin; the rest is
//! the part that can already be run by hand.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use lm_archive::golden;
use lm_archive::protocol::ingest_round;
use lm_archive::server::Server;
use lm_archive::store::Store;
use lm_archive::{priority, push_round, verify_file, ArchiveConfig};
use lm_proto::Msg;
use lm_tools::runner::read_log;

/// What the command was asked to do.
enum Command {
    /// Run the service.
    Serve,
    /// Replay a round log and report the verdict.
    Verify(PathBuf),
    /// Take a round log into the store and report the verdict.
    Ingest(PathBuf),
    /// Push a round log to an archive and report the verdict.
    Push(PathBuf, String),
    /// Run the golden check and record the verdict.
    Golden,
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
    let mut golden_path: Option<PathBuf> = None;
    let mut command: Option<Command> = None;

    // The options may come before the command or after it: an operator
    // should not have to remember which way round, especially for the
    // options that say where the store is.
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
            "--golden" => {
                let value = args.next().ok_or("--golden needs a path")?;
                golden_path = Some(PathBuf::from(value));
            }
            "--help" | "-h" => {
                print_help();
                return Ok(OK);
            }
            other => {
                if command.is_some() {
                    return Err(format!("unexpected argument {other:?}"));
                }
                command = Some(match other {
                    "serve" => Command::Serve,
                    "verify" => {
                        Command::Verify(positional(&mut args, "verify needs a log file")?.into())
                    }
                    "ingest" => {
                        Command::Ingest(positional(&mut args, "ingest needs a log file")?.into())
                    }
                    "push" => {
                        let path = positional(&mut args, "push needs a log file")?;
                        let addr = positional(&mut args, "push needs an address")?;
                        Command::Push(PathBuf::from(path), addr)
                    }
                    "golden" => Command::Golden,
                    "paths" => Command::Paths,
                    other => return Err(format!("unknown command {other:?}")),
                });
            }
        }
    }

    match command {
        Some(command) => dispatch(command, config_path, store_dir, golden_path),
        None => {
            print_help();
            Ok(USAGE)
        }
    }
}

/// The next argument, which has to be a value and not another option.
fn positional(args: &mut impl Iterator<Item = String>, complaint: &str) -> Result<String, String> {
    let value = args.next().ok_or(complaint.to_owned())?;
    if value.starts_with("--") {
        return Err(format!("{complaint}, not the option {value}"));
    }
    Ok(value)
}

/// Do the command.
fn dispatch(
    command: Command,
    config_path: Option<PathBuf>,
    store_dir: Option<PathBuf>,
    golden_path: Option<PathBuf>,
) -> Result<u8, String> {
    let mut cfg = match &config_path {
        Some(path) => ArchiveConfig::load(path).map_err(|err| err.to_string())?,
        None => ArchiveConfig::default(),
    };
    if let Some(dir) = store_dir {
        cfg.store_dir = dir;
    }

    match command {
        Command::Serve => serve(cfg),
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
        Command::Push(path, addr) => {
            let log = read_log_file(&path)?;
            match push_round(&addr, &log) {
                Ok(Msg::StateHash { tick, hash }) => {
                    println!(
                        "verified: the archive replayed the round to tick {tick} (hash {hash:#018x})"
                    );
                    Ok(OK)
                }
                Ok(other) => {
                    println!("the archive answered {other:?}");
                    Ok(OK)
                }
                Err(err) => {
                    eprintln!("lm-archive: {err}");
                    Ok(FAILED)
                }
            }
        }
        Command::Golden => {
            let path = golden_path.unwrap_or_else(golden::default_path);
            let report = golden::check(&path).map_err(|err| err.to_string())?;
            // The verdict lands in the store's report directory: the ProArt
            // reads that directory, and this is how it learns what the
            // Orin's machine says.
            let store = cfg.store();
            let written = golden::write_report(&store, &report).map_err(|err| err.to_string())?;
            println!("{}", report.summary());
            println!("wrote {}", written.display());
            Ok(if report.ok { OK } else { FAILED })
        }
        Command::Paths => {
            let store = cfg.store();
            print_paths(&store);
            Ok(OK)
        }
    }
}

/// Run the service.
///
/// The priority is applied **first**, by re-executing the process under
/// `nice` and `ionice`: the service comes back as itself, at the priority
/// it asked for, and only then opens the listener. A box that cannot lower
/// the priority is not a box that must not archive rounds: the service says
/// so and carries on.
fn serve(cfg: ArchiveConfig) -> Result<u8, String> {
    if !priority::already_applied() {
        match priority::apply_to_self(&cfg) {
            Ok(()) => {}
            Err(err) => {
                eprintln!("lm-archive: the priority did not take: {err}");
                eprintln!("lm-archive: the service runs at the foreground priority");
            }
        }
    }

    let server = Server::bind(cfg).map_err(|err| err.to_string())?;
    let addr = server.local_addr().map_err(|err| err.to_string())?;
    // The store's directories are made now, so a store that cannot be
    // written is a startup error and not a surprise at the first push.
    let store = server.config().store();
    store
        .ensure_layout()
        .map_err(|err| format!("the store at {}: {err}", store.root().display()))?;
    println!("lm-archive: listening on {addr}");
    println!("lm-archive: store {}", store.root().display());
    println!(
        "lm-archive: nice {}, io class {}",
        server.config().nice,
        server.config().ionice_class
    );
    server.serve().map_err(|err| err.to_string())?;
    Ok(OK)
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
    println!("golden:     {}", store.golden_report_path().display());
    println!("state:      {}", store.state_path().display());
}

/// What the command can do.
fn print_help() {
    println!("lm-archive — the Orin archive: store round logs and verify every hash");
    println!();
    println!("usage:");
    println!("  lm-archive serve                  run the service (the Orin)");
    println!("  lm-archive verify <log.lml>       replay a round and report the verdict");
    println!("  lm-archive ingest <log.lml>       store a round, verify it, write the report");
    println!("  lm-archive push <log.lml> <addr>  push a round to an archive and report");
    println!("  lm-archive golden                 run the cross-arch golden check");
    println!("  lm-archive paths                  say where the store keeps its files");
    println!();
    println!("options:");
    println!("  --config <archive.toml>           read the config from a file");
    println!("  --store <dir>                     use this store directory");
    println!("  --golden <golden.toml>            use this golden file");
}
