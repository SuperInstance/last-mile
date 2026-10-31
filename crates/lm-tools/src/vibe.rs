//! The `lm vibe` commands: propose a typed change out of the Vibe
//! Dictionary with a prediction sealed before the test, and read the
//! ledger back with the prediction-trust tally (the sandbox's first slice,
//! M2b).
//!
//! The heavy lifting is [`lm_learn::vibe`]; these are the commands, the
//! output and the exit codes. A refusal is a named line on stderr and a
//! failure — never a clamp, never a silent fallback.

use std::path::PathBuf;
use std::process::ExitCode;

use lm_learn::ledger::Ledger;
use lm_learn::vibe::{self, Ask, Dictionary, LandSite};

/// `lm vibe propose`.
pub struct ProposeArgs {
    /// The dictionary entry (`unit.vision`).
    pub entry: String,
    /// The value proposed.
    pub value: i64,
    /// The prediction as worded (`sight_ticks:up,tags:up`), or `None` for
    /// the entry's default.
    pub predict: Option<String>,
    /// The kid's words as a short label, or `None` for the entry's own
    /// phrase.
    pub say: Option<String>,
    /// How many held-out seeds to judge on.
    pub seeds: usize,
    /// How many ticks each seed plays.
    pub ticks: u64,
    /// Where the round's files go; the ledger is `<runs>/vibe.jsonl`.
    pub runs: PathBuf,
    /// A `vibe.toml` that overrides the shipped one.
    pub file: Option<PathBuf>,
    /// The harness root an adopted change lands in (`lm learn`'s
    /// `--harness`); without it the verdict is recorded but nothing lands.
    pub harness: Option<PathBuf>,
    /// The seed of the next round, whose header a landing writes.
    pub next_seed: u64,
}

/// `lm vibe ledger`.
pub struct LedgerArgs {
    /// Where the round's files go; the ledger is `<runs>/vibe.jsonl`.
    pub runs: PathBuf,
}

/// `lm vibe state`.
pub struct StateArgs {
    /// Where the round's files go; the ledger is `<runs>/vibe.jsonl`.
    pub runs: PathBuf,
    /// A `vibe.toml` that overrides the shipped one, so the entries
    /// resting at their defaults are named by the same dictionary the
    /// proposals were judged against.
    pub file: Option<PathBuf>,
}

/// `lm vibe undo`.
pub struct UndoArgs {
    /// The record to put back: its seal, the ledger line `lm vibe ledger`
    /// prints.
    pub seal: u64,
    /// Where the round's files go; the ledger is `<runs>/vibe.jsonl`.
    pub runs: PathBuf,
    /// A `vibe.toml` that overrides the shipped one.
    pub file: Option<PathBuf>,
    /// The harness root the undo re-lands, when the change landed.
    pub harness: Option<PathBuf>,
    /// The seed of the next round, whose header a re-landing writes.
    pub next_seed: u64,
}

/// Propose a change: seal the prediction, run the test, print the verdict,
/// write the record; when a harness is named, land what the verdict left in
/// force.
pub fn cmd_vibe_propose(args: &ProposeArgs) -> ExitCode {
    let dictionary = match dictionary_of(&args.file) {
        Some(dict) => dict,
        None => return ExitCode::FAILURE,
    };
    let ledger_path = args.runs.join("vibe.jsonl");
    let mut ask = Ask::new(&args.entry, args.value)
        .with_seeds(args.seeds)
        .with_ticks(args.ticks);
    if let Some(prediction) = &args.predict {
        ask = ask.with_prediction(prediction);
    }
    if let Some(label) = &args.say {
        ask = ask.with_label(label);
    }
    if let Some(harness) = &args.harness {
        ask = ask.with_land(LandSite::new(harness.clone(), args.next_seed));
    }
    match vibe::propose(&dictionary, &ledger_path, &ask) {
        Ok(proposed) => {
            print!("{}", proposed.summary());
            println!("  wrote:    {}", ledger_path.display());
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("lm: {err}");
            ExitCode::FAILURE
        }
    }
}

/// List the ledger's records with the prediction-trust tally.
pub fn cmd_vibe_ledger(args: &LedgerArgs) -> ExitCode {
    let ledger_path = args.runs.join("vibe.jsonl");
    match Ledger::open(&ledger_path) {
        Ok(ledger) => {
            print!("{}", vibe::ledger_summary(&ledger));
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("lm: {}: {err}", ledger_path.display());
            ExitCode::FAILURE
        }
    }
}

/// Say what is in force now, from which ledger lines.
pub fn cmd_vibe_state(args: &StateArgs) -> ExitCode {
    let dictionary = match dictionary_of(&args.file) {
        Some(dict) => dict,
        None => return ExitCode::FAILURE,
    };
    let ledger_path = args.runs.join("vibe.jsonl");
    match Ledger::open(&ledger_path) {
        Ok(ledger) => {
            print!("{}", vibe::state_summary(&ledger, &dictionary));
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("lm: {}: {err}", ledger_path.display());
            ExitCode::FAILURE
        }
    }
}

/// Put one adopted record back: a new ledger line that reverts it, and the
/// harness re-landed on what is in force now.
pub fn cmd_vibe_undo(args: &UndoArgs) -> ExitCode {
    let dictionary = match dictionary_of(&args.file) {
        Some(dict) => dict,
        None => return ExitCode::FAILURE,
    };
    let ledger_path = args.runs.join("vibe.jsonl");
    let site = args
        .harness
        .as_ref()
        .map(|harness| LandSite::new(harness.clone(), args.next_seed));
    match vibe::undo(&dictionary, &ledger_path, args.seal, "bench", site.as_ref()) {
        Ok(done) => {
            let undo = &done.undo;
            println!(
                "undone: record {} — {} back to {} (was {}), ledger line {}",
                undo.seal, undo.field, undo.value, undo.from, done.line
            );
            if let Some(landed) = &done.landed {
                println!("re-landed {}", landed.state_path.display());
                println!(
                    "next:   {} (seed {}, {} record(s) in force)",
                    landed.header_path.display(),
                    landed.header.seed,
                    landed.in_force.len()
                );
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("lm: {err}");
            ExitCode::FAILURE
        }
    }
}

/// The dictionary to propose against: the file given, or the one this
/// build ships.
fn dictionary_of(file: &Option<PathBuf>) -> Option<Dictionary> {
    match file {
        Some(path) => match Dictionary::from_file(path) {
            Ok(dict) => Some(dict),
            Err(err) => {
                eprintln!("lm: {}: {err}", path.display());
                None
            }
        },
        None => match Dictionary::default_dictionary() {
            Ok(dict) => Some(dict),
            Err(err) => {
                eprintln!("lm: the shipped dictionary: {err}");
                None
            }
        },
    }
}
