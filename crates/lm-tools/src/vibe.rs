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
use lm_learn::vibe::{self, Ask, Dictionary};

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
}

/// `lm vibe ledger`.
pub struct LedgerArgs {
    /// Where the round's files go; the ledger is `<runs>/vibe.jsonl`.
    pub runs: PathBuf,
}

/// Propose a change: seal the prediction, run the test, print the verdict,
/// write the record.
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
