//! The `lm vibe` commands: propose a typed change out of the Vibe
//! Dictionary with a prediction sealed before the test, and read the
//! ledger back with the prediction-trust tally (the sandbox's first slice,
//! M2b). `lm vibe hear` is the vibe gate: a kid's words become that typed
//! proposal through two closed questions the decision server answers
//! (part 3).
//!
//! The heavy lifting is [`lm_learn::vibe`] and [`lm_learn::hear`]; these
//! are the commands, the output and the exit codes. A refusal is a named
//! line on stderr and a failure — never a clamp, never a silent fallback.

use std::path::PathBuf;
use std::process::ExitCode;

use lm_agents::config::AgentsConfig;
use lm_agents::sysone::ThorBackend;
use lm_learn::hear::{self, GatePack};
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

/// `lm vibe hear`: the vibe gate — a kid's words become a typed proposal
/// through two closed questions.
pub struct HearArgs {
    /// The kid's words. They travel to the decision server and stop
    /// there; the record a `--propose` writes carries only the bounded
    /// 48-character label, never a transcript.
    pub words: String,
    /// Hand the heard proposal to the propose path: the prediction is
    /// sealed before the test, as today.
    pub propose: bool,
    /// The prediction as worded (`sight_ticks:up,tags:up`), or `None` for
    /// the entry's default.
    pub predict: Option<String>,
    /// How many held-out seeds to judge on.
    pub seeds: usize,
    /// How many ticks each seed plays.
    pub ticks: u64,
    /// Where the round's files go; the ledger is `<runs>/vibe.jsonl`.
    pub runs: PathBuf,
    /// A `vibe.toml` that overrides the shipped one.
    pub file: Option<PathBuf>,
    /// A `vibe-gate.toml` that overrides the shipped one.
    pub gate_file: Option<PathBuf>,
    /// The harness root an adopted change lands in.
    pub harness: Option<PathBuf>,
    /// The seed of the next round, whose header a landing writes.
    pub next_seed: u64,
    /// The decision server's base URL; the default is the agents' config,
    /// the one place that says where the Thor is.
    pub base: Option<String>,
}

/// Hear the kid's words: run the two gates against the decision server,
/// print the typed proposal it would make with the confidence of each
/// answer, and — with `--propose` — hand it to the pipeline.
pub fn cmd_vibe_hear(args: &HearArgs) -> ExitCode {
    let Some(dictionary) = dictionary_of(&args.file) else {
        return ExitCode::FAILURE;
    };
    let Some(pack) = pack_of(&args.gate_file) else {
        return ExitCode::FAILURE;
    };
    let Some(backend) = backend_of(&pack, args.base.as_deref()) else {
        return ExitCode::FAILURE;
    };
    let heard = match hear::hear(&backend, &pack, &dictionary, &args.words) {
        Ok(heard) => heard,
        Err(err) => {
            eprintln!("lm: {err}");
            return ExitCode::FAILURE;
        }
    };
    print!("{}", heard.summary());
    if !args.propose {
        println!("  (a hearing writes nothing; say --propose to seal and test it)");
        return ExitCode::SUCCESS;
    }

    // The proposal: the entry and the value the gate heard, labelled with
    // the bounded words (the label is the only written form of them), and
    // everything else exactly as a typed proposal takes it.
    let (entry, value) = heard.proposal_of();
    let ledger_path = args.runs.join("vibe.jsonl");
    let mut ask = Ask::new(entry, value)
        .with_label(&heard.words)
        .with_seeds(args.seeds)
        .with_ticks(args.ticks);
    if let Some(prediction) = &args.predict {
        ask = ask.with_prediction(prediction);
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

/// The policy pack to hear against: the file given, or the one this build
/// ships.
fn pack_of(file: &Option<PathBuf>) -> Option<GatePack> {
    match file {
        Some(path) => match GatePack::from_file(path) {
            Ok(pack) => Some(pack),
            Err(err) => {
                eprintln!("lm: {}: {err}", path.display());
                None
            }
        },
        None => match GatePack::default_pack() {
            Ok(pack) => Some(pack),
            Err(err) => {
                eprintln!("lm: the shipped policy pack: {err}");
                None
            }
        },
    }
}

/// The decision server the gate asks: the pack names the model, the
/// agents' config says where it is (the base and the port), and an
/// explicit `--base` overrides the base only.
fn backend_of(pack: &GatePack, base: Option<&str>) -> Option<ThorBackend> {
    let config = AgentsConfig::default();
    let Some(port) = config.ports.get(&pack.model) else {
        eprintln!(
            "lm: the agents' config has no port for the model `{}`, which the vibe gate names",
            pack.model
        );
        return None;
    };
    let base = base.unwrap_or(&config.thor_base);
    Some(
        ThorBackend::new(base, *port, &pack.model)
            .with_min_confidence(pack.min_confidence)
            .with_timeout(pack.timeout),
    )
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
