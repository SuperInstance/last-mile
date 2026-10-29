//! The headless runner: the host loop that plays a whole round without a
//! screen.
//!
//! A run is four steps per tick, in this order:
//!
//! 1. the decisions that have arrived are handed to the sequencer, which
//!    stamps each one on the tick it belongs to — or refuses it, by name;
//! 2. the sequencer is drained for this tick, which yields the decisions in
//!    the canonical order (system first, then the agents in a rotating order);
//! 3. the sim steps the tick with those decisions;
//! 4. the recorder writes the tick into the round log.
//!
//! Nothing here reads a clock, an environment variable or a random source:
//! the round is a pure function of its header and its decisions, so the same
//! inputs always produce the same log, byte for byte.
//!
//! ## Replay
//!
//! [`replay`] re-runs a round from the header in its log, feeding it the
//! decisions the log recorded, and checks every state hash on the way. A log
//! that does not describe the round it claims to describe is caught, not
//! believed — that is what makes a round reproducible instead of merely
//! repeatable.

use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};

use lm_core::decision::{Decision, Tick};
use lm_core::header::Header;
use lm_core::log::{LogError, Recorder, RoundLog};
use lm_core::rules::{Outcome, SimError};
use lm_core::sched::Sequencer;
use lm_core::sim::Sim;
use serde::{Deserialize, Serialize};

/// The reflex every agent of a standard round runs.
///
/// The file is compiled into the binary, so `lm run` and the tests always
/// play the same reflex the repository ships.
pub const DEFAULT_REFLEX: &str = include_str!("../../../assets/reflexes/default.rhai");

/// The seeds the golden rounds cover.
pub const GOLDEN_SEEDS: [u64; 3] = [1, 2, 3];

/// The ticks each golden round runs.
pub const GOLDEN_TICKS: u64 = 3_000;

/// What a run did.
///
/// The counters are the story of the round: how many ticks it ran, how many
/// hashes it produced, and what happened to every decision that was handed to
/// the sequencer. A decision is either applied or refused with a named
/// reason — never silently lost.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunStats {
    /// The ticks the round ran.
    pub ticks: u64,
    /// The state hashes the round produced.
    pub hashes: u64,
    /// Decisions stamped on the tick they asked for.
    pub on_time: u64,
    /// Decisions stamped on their arrival tick instead.
    pub late: u64,
    /// Decisions dropped for being later than the stale window.
    pub stale: u64,
    /// Decisions the sequencer refused, for any reason.
    pub refused: u64,
    /// Effects the ruleset refused to apply, for any reason.
    ///
    /// A ruleset that applies a script checks every effect the script asks
    /// for. An effect the round will not have is refused and never applied,
    /// and the refusal is counted here.
    pub ruleset_refused: u64,
    /// True when the round ended before the last tick it was given.
    pub round_over: bool,
    /// How the round ended, if it ended.
    pub outcome: Option<Outcome>,
    /// The score of every squad, in squad order.
    pub scores: Vec<i64>,
}

/// Why a replay did not finish.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplayError {
    /// The log itself is damaged: it does not decode, or its hash chain does
    /// not hold.
    Log(LogError),
    /// The log decodes, but the round it describes cannot be played: the
    /// header names a ruleset this build does not have, breaks a parameter,
    /// or carries a script that does not compile.
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
}

impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReplayError::Log(err) => write!(f, "{err}"),
            ReplayError::Setup(err) => write!(f, "{err}"),
            ReplayError::Diverged {
                tick,
                expected,
                got,
            } => write!(
                f,
                "diverged at tick {tick}: the log says {expected:#018x}, the replay saw {got:#018x}"
            ),
        }
    }
}

impl std::error::Error for ReplayError {}

impl From<LogError> for ReplayError {
    fn from(err: LogError) -> Self {
        ReplayError::Log(err)
    }
}

/// Play a round and record it.
///
/// `h` is the header of the round, `ticks` how many ticks it may run, and
/// `inject` the decisions that arrive during it. Each injection is the tick
/// the decision arrived on and the decision itself; the sequencer decides
/// which tick it is applied on, and refuses it — by name — when it cannot be
/// applied at all.
///
/// The round stops early when the rules end it. The returned log holds every
/// tick that was played, and the returned stats say what happened to every
/// decision.
pub fn run(h: Header, ticks: u64, inject: &[(Tick, Decision)]) -> (RoundLog, RunStats) {
    let mut sim = Sim::new(&h).unwrap_or_else(|err| panic!("the round does not start: {err}"));

    // The header is the round's identity; the rest of the run follows from it.
    let start = h.start_tick;
    let n_agents = u16::try_from(h.roster.len()).unwrap_or(u16::MAX);
    let mut sequencer = Sequencer::new(&h.params, n_agents, start);
    let mut recorder = Recorder::new(h.clone(), h.params.segment_ticks);

    // The injections, in the order they arrive. The sort is stable, so two
    // decisions that arrive on the same tick keep the order they were given.
    let mut arrivals: Vec<(Tick, &Decision)> = inject.iter().map(|(at, d)| (*at, d)).collect();
    arrivals.sort_by_key(|(at, _)| *at);

    let mut stats = RunStats::default();
    let mut next = 0usize;
    let end = start.saturating_add(ticks);
    let mut tick = start;

    while tick < end {
        // Hand the sequencer every decision that has arrived by this tick.
        while next < arrivals.len() && arrivals[next].0 <= tick {
            let (arrived, decision) = arrivals[next];
            next += 1;
            if sequencer.submit(decision.clone(), arrived).is_err() {
                // Refused, and named in the sequencer's own counters.
                stats.refused += 1;
            }
        }

        // The decisions of this tick, in the canonical order.
        let applied = sequencer.drain(tick);
        let out = sim.step(&applied);
        recorder.record(tick, &applied, out.hash);

        stats.ticks += 1;
        if out.hash.is_some() {
            stats.hashes += 1;
        }
        tick += 1;

        if sim.outcome().is_some() {
            stats.round_over = true;
            break;
        }
    }

    let sched = sequencer.stats();
    stats.on_time = sched.on_time;
    stats.late = sched.late;
    stats.stale = sched.stale;
    stats.outcome = sim.outcome().cloned();
    stats.scores = sim.scores().to_vec();
    // The refusals of the ruleset: an effect the rules will not apply is
    // refused and named, never applied and never lost.
    stats.ruleset_refused = sim.stats().ruleset_refused;

    (recorder.finish(), stats)
}

/// Replay a round and check every hash it recorded.
///
/// The round is re-run from the header in the log, fed with the decisions the
/// log recorded, and every state hash is compared with the one in the log.
/// The hashes of the replay are returned when they all match.
pub fn replay(log: &RoundLog) -> Result<Vec<(Tick, u64)>, ReplayError> {
    let mut sim = Sim::new(&log.header).map_err(ReplayError::Setup)?;

    let mut got = Vec::new();
    for segment in &log.segments {
        // Every tick of a segment is replayed, so the replay ends in the same
        // state the round ended in.
        for tick in segment.first_tick..=segment.last_tick {
            let out = sim.step(log.decisions_for(tick));
            if let Some(hash) = out.hash {
                got.push((tick, hash));
            }
        }
    }

    compare(&log.hashes(), &got)?;
    Ok(got)
}

/// Compare two lists of hashes and name the first disagreement.
fn compare(expected: &[(Tick, u64)], got: &[(Tick, u64)]) -> Result<(), ReplayError> {
    for (left, right) in expected.iter().zip(got.iter()) {
        if left.0 != right.0 || left.1 != right.1 {
            return Err(ReplayError::Diverged {
                tick: left.0.min(right.0),
                expected: left.1,
                got: right.1,
            });
        }
    }
    // One list ran out before the other: the first hash the longer one has
    // is where the two rounds parted company.
    if let Some((tick, hash)) = expected.get(got.len()) {
        return Err(ReplayError::Diverged {
            tick: *tick,
            expected: *hash,
            got: 0,
        });
    }
    if let Some((tick, hash)) = got.get(expected.len()) {
        return Err(ReplayError::Diverged {
            tick: *tick,
            expected: 0,
            got: *hash,
        });
    }
    Ok(())
}

/// Read a round log, naming the damage if there is any.
pub fn read_log(r: &mut impl Read) -> Result<RoundLog, ReplayError> {
    RoundLog::read(r).map_err(ReplayError::Log)
}

/// The first tick whose hashes differ, with the two hashes.
///
/// `None` means the two logs recorded the same hashes: the same world, tick
/// by tick.
pub fn diff(a: &RoundLog, b: &RoundLog) -> Option<(Tick, u64, u64)> {
    let left = a.hashes();
    let right = b.hashes();
    for (x, y) in left.iter().zip(right.iter()) {
        if x.0 != y.0 || x.1 != y.1 {
            return Some((x.0.min(y.0), x.1, y.1));
        }
    }
    if let Some((tick, hash)) = left.get(right.len()) {
        return Some((*tick, *hash, 0));
    }
    if let Some((tick, hash)) = right.get(left.len()) {
        return Some((*tick, 0, *hash));
    }
    None
}

/// Why an injection file could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InjectError {
    /// A line of the file is not a decision.
    Line {
        /// The line the trouble is on, counting from 1.
        line: usize,
        /// What is wrong with it.
        message: String,
    },
    /// The file itself could not be read.
    Io(String),
}

impl fmt::Display for InjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InjectError::Line { line, message } => write!(f, "injection line {line}: {message}"),
            InjectError::Io(message) => write!(f, "injection file: {message}"),
        }
    }
}

impl std::error::Error for InjectError {}

/// One line of an injection file.
#[derive(Clone, Debug, Deserialize)]
struct InjectionLine {
    /// The tick the decision arrives on.
    arrived: Tick,
    /// The decision itself.
    decision: Decision,
}

/// Read the injections of a JSON Lines file.
///
/// Every line is one decision and the tick it arrives on:
///
/// ```json
/// {"arrived": 120, "decision": {"apply_tick": 120, "source": {"Agent": 0},
///   "seq": 1, "kind": {"ActionOn": "Hold"}, "provenance": "Test"}}
/// ```
///
/// Blank lines and lines starting with `#` are skipped, so a file can be
/// annotated.
pub fn parse_injections(text: &str) -> Result<Vec<(Tick, Decision)>, InjectError> {
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parsed: InjectionLine =
            serde_json::from_str(line).map_err(|err| InjectError::Line {
                line: index + 1,
                message: err.to_string(),
            })?;
        out.push((parsed.arrived, parsed.decision));
    }
    Ok(out)
}

/// Why the golden file could not be read or written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GoldenError {
    /// The file could not be read or written.
    Io(String),
    /// The file is not the golden file this build understands.
    Parse(String),
    /// The file does not pin the seed.
    Missing(u64),
    /// A pinned hash is not a number.
    Format(String),
}

impl fmt::Display for GoldenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GoldenError::Io(message) => write!(f, "golden file: {message}"),
            GoldenError::Parse(message) => write!(f, "golden file: {message}"),
            GoldenError::Missing(seed) => write!(f, "golden file: seed {seed} is not pinned"),
            GoldenError::Format(message) => write!(f, "golden file: {message}"),
        }
    }
}

impl std::error::Error for GoldenError {}

/// The pinned golden hashes of the phase-1 rounds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Golden {
    /// The ticks each golden round runs.
    pub ticks: u64,
    /// The final hash of every golden seed, by seed.
    pub golden: BTreeMap<String, String>,
}

impl Golden {
    /// Read the golden file.
    pub fn read(path: &Path) -> Result<Golden, GoldenError> {
        let text = std::fs::read_to_string(path)
            .map_err(|err| GoldenError::Io(format!("{}: {err}", path.display())))?;
        toml::from_str(&text).map_err(|err| GoldenError::Parse(err.to_string()))
    }

    /// Write the golden file, with the header that says what it is.
    ///
    /// The directory is created if it is not there yet, so the first golden
    /// file can be written into a fresh checkout.
    pub fn write(&self, path: &Path) -> Result<(), GoldenError> {
        let body =
            toml::to_string_pretty(self).map_err(|err| GoldenError::Parse(err.to_string()))?;
        let text = format!("{GOLDEN_HEADER}\n{body}");
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|err| GoldenError::Io(format!("{}: {err}", dir.display())))?;
        }
        std::fs::write(path, text)
            .map_err(|err| GoldenError::Io(format!("{}: {err}", path.display())))
    }

    /// The pinned hash of a seed.
    pub fn hash_of(&self, seed: u64) -> Result<u64, GoldenError> {
        let text = self
            .golden
            .get(&seed.to_string())
            .ok_or(GoldenError::Missing(seed))?;
        parse_hash(text)
    }
}

/// The comment every golden file starts with.
const GOLDEN_HEADER: &str = "\
# The pinned golden hashes of the headless phase-1 rounds (spec section 10).
#
# One entry per seed: the last state hash of a round of `ticks` ticks started
# with `Header::default_round(seed, DEFAULT_REFLEX)` -- the shipped reflex, the
# default parameters and the `beacons` ruleset.
#
# These numbers are the contract. A change to the simulation moves them, and
# the commit that touches this file has to say why the world changed.
#
# Regenerate with:
#     cargo run --release --bin lm -- golden --write
";

/// Read a hash as it is written in the golden file: `0x` and 16 digits.
fn parse_hash(text: &str) -> Result<u64, GoldenError> {
    let trimmed = text.trim();
    let digits = trimmed.strip_prefix("0x").unwrap_or(trimmed);
    u64::from_str_radix(digits, 16).map_err(|err| GoldenError::Format(format!("{trimmed}: {err}")))
}

/// The path of the golden file: `tests/golden.toml` of the workspace.
pub fn golden_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden.toml")
}

/// Read the golden file of the workspace.
pub fn read_golden() -> Result<Golden, GoldenError> {
    Golden::read(&golden_path())
}

/// The golden rounds: the final hash of each seed, at the golden tick count.
pub fn golden_hashes(ticks: u64) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    for seed in GOLDEN_SEEDS {
        let (log, _) = run(Header::default_round(seed, DEFAULT_REFLEX), ticks, &[]);
        let last = log.hashes().last().map(|(_, hash)| *hash).unwrap_or(0);
        out.push((seed, last));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use lm_core::decision::{Action, Kind, Provenance, Source};

    /// The reflex the tests run: an empty one is enough for the plumbing.
    const REFLEX: &str = "fn reflex(v){ #{ hold: true } }";

    /// A decision of `agent`, applied on `tick`.
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
    fn a_run_records_the_ticks_it_played() {
        let (log, stats) = run(Header::default_round(1, REFLEX), 300, &[]);
        assert_eq!(stats.ticks, 300);
        assert_eq!(stats.hashes, 10);
        assert_eq!(log.hashes().len(), 10);
        assert_eq!(log.segments.len(), 1);
        assert!(!stats.round_over);
    }

    #[test]
    fn a_run_replays_bit_exact() {
        let (log, _) = run(Header::default_round(1, REFLEX), 300, &[]);
        assert_eq!(replay(&log).unwrap(), log.hashes());
    }

    #[test]
    fn a_stale_decision_is_refused_and_named() {
        let inject = vec![(200, decision(0, 10, 1))];
        let (log, stats) = run(Header::default_round(1, REFLEX), 300, &inject);
        assert_eq!(stats.stale, 1);
        assert_eq!(stats.refused, 1);
        assert_eq!(
            log.segments
                .iter()
                .map(|s| s.decisions.len())
                .sum::<usize>(),
            0
        );
    }

    #[test]
    fn an_unknown_source_is_refused() {
        let inject = vec![(10, decision(99, 10, 1))];
        let (_, stats) = run(Header::default_round(1, REFLEX), 300, &inject);
        assert_eq!(stats.refused, 1);
        assert_eq!(stats.stale, 0);
    }

    #[test]
    fn a_late_decision_is_applied_on_its_arrival_tick() {
        let inject = vec![(20, decision(0, 18, 1))];
        let (log, stats) = run(Header::default_round(1, REFLEX), 300, &inject);
        assert_eq!(stats.late, 1);
        assert_eq!(log.decisions_for(20).len(), 1);
    }

    #[test]
    fn a_diff_of_a_round_with_itself_finds_nothing() {
        let (log, _) = run(Header::default_round(1, REFLEX), 300, &[]);
        assert_eq!(diff(&log, &log), None);
    }

    #[test]
    fn a_diff_names_the_first_tick_that_moves() {
        let (a, _) = run(Header::default_round(1, REFLEX), 300, &[]);
        let (b, _) = run(Header::default_round(2, REFLEX), 300, &[]);
        let (tick, left, right) = diff(&a, &b).expect("the two rounds differ");
        assert_eq!(tick, 29);
        assert_ne!(left, right);
    }

    /// The refusals of a scripted ruleset reach the host: a script that asks
    /// for an effect the round will not have is refused every tick, and the
    /// round goes on.
    #[test]
    fn a_ruleset_refusal_is_counted_and_the_round_goes_on() {
        let mut h = Header::default_round(1, REFLEX);
        h.ruleset = "rhai:bad".to_owned();
        h.scripts.insert(
            "bad".to_owned(),
            r#"fn rules(v) { [ #{ kind: "tag", by: 0, "target": 999 } ] }"#.to_owned(),
        );
        let (log, stats) = run(h, 60, &[]);
        assert_eq!(stats.ruleset_refused, 60);
        assert_eq!(stats.ticks, 60);
        assert!(!log.hashes().is_empty());
    }

    #[test]
    fn injections_are_read_from_json_lines() {
        let text = r#"
# a comment
{"arrived": 120, "decision": {"apply_tick": 120, "source": {"Agent": 0}, "seq": 1, "kind": {"ActionOn": "Hold"}, "provenance": "Test"}}
"#;
        let injections = parse_injections(text).expect("the file reads");
        assert_eq!(injections.len(), 1);
        assert_eq!(injections[0].0, 120);
        assert_eq!(injections[0].1.source, Source::Agent(0));
        assert_eq!(injections[0].1.kind, Kind::ActionOn(Action::Hold));
    }

    #[test]
    fn a_broken_injection_line_is_named() {
        let err = parse_injections("{\"arrived\": 1}").unwrap_err();
        assert!(matches!(err, InjectError::Line { line: 1, .. }));
    }

    #[test]
    fn a_hash_is_read_in_hex() {
        assert_eq!(parse_hash("0x00000000000000ff").unwrap(), 255);
        assert_eq!(parse_hash("ff").unwrap(), 255);
        assert!(parse_hash("nonsense").is_err());
    }
}
