//! The Vibe pipeline: a typed change, a sealed prediction, a held-out test,
//! a verdict — the sandbox's first slice (M2b).
//!
//! ```text
//! words -> a dictionary entry and a value (bounds checked; refusals named)
//!      -> a prediction on two or three measures, sealed BEFORE the test
//!      -> a headless sweep: the D78 seeds, baseline vs the change
//!      -> a verdict per measure (D78's rule), adopt iff every measure held
//! ```
//!
//! The dictionary is data (`assets/harness/default/vibe.toml`), and its
//! bounds are enforced **before** anything runs: an out-of-bounds value or
//! an unknown entry is a refusal that names itself, never a clamp. The
//! prediction is sealed in the [ledger](crate::ledger)'s hash chain before
//! the first seed is played, so it cannot be edited to match the result
//! afterwards — the outcome has to name the bytes that were sealed.
//!
//! Privacy (sandbox §7): the record holds the entry, the value, the
//! prediction, the seal, the results and the verdict. The only words it
//! carries are a short label — the kid's phrasing of the change, bounded at
//! [LABEL_MAX] characters — never a transcript, never audio.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;

use lm_core::header::Header;
use lm_core::params::{Params, BRANCH_FIELDS};
use lm_core::Fnv64;

use crate::eval;
use crate::flow::{DEFAULT_EVAL_SEEDS, DEFAULT_REFLEX, MEASURABLE_TICKS};
use crate::ledger::{
    Direction, Ledger, LedgerError, MeasureOutcome, Prediction, VibeClaim, VibeOutcome,
    VibeProposal,
};
use crate::metrics::RoundMetrics;
use crate::round::play;

/// The dictionary this build ships, embedded the same way the default
/// reflex is: the file in the harness is the source, the binary carries a
/// snapshot, and `--file` overrides it with the disk copy.
pub const DEFAULT_DICTIONARY: &str = include_str!("../../../assets/harness/default/vibe.toml");

/// How many held-out seeds a test runs on when nothing else is said: the
/// D78 count (16, at 1500 ticks).
pub const DEFAULT_SEEDS: usize = DEFAULT_EVAL_SEEDS;

/// How many ticks each seed is played for when nothing else is said.
pub const DEFAULT_TICKS: u64 = crate::flow::DEFAULT_TICKS;

/// The most measures one prediction may claim.
pub const MAX_MEASURES: usize = 4;

/// The most characters a label may carry. The kid's words arrive as a
/// short label or not at all; a transcript is never stored.
pub const LABEL_MAX: usize = 48;

/// Why a proposal never happened, or the pipeline could not run.
///
/// The refusals are the point of the dictionary: each one names itself, and
/// none of them clamps a bad value into a good one.
#[derive(Debug)]
pub enum VibeError {
    /// A file could not be read or written.
    Io(std::io::Error),
    /// The ledger refused.
    Ledger(LedgerError),
    /// The dictionary itself is not valid: its bounds, defaults or fields
    /// disagree with the code that runs them.
    DictionaryInvalid { why: String },
    /// The entry asked for is not in the dictionary.
    UnknownEntry {
        /// What was asked for.
        asked: String,
        /// What the dictionary holds.
        known: Vec<String>,
    },
    /// The value is outside the entry's bounds. Refused, never clamped.
    OutOfBounds {
        /// The entry.
        block: String,
        /// The value asked for.
        value: i64,
        /// The bounds, inclusive.
        min: i64,
        max: i64,
    },
    /// A predicted measure no metric answers to.
    UnknownMeasure {
        /// What was asked for.
        asked: String,
    },
    /// The prediction text could not be read.
    BadPrediction {
        /// The text as it was given.
        text: String,
        /// Why it could not be read.
        why: String,
    },
    /// The entry carries no default prediction (`expect = "either"`), so
    /// the proposal has to state one.
    NeedsPrediction {
        /// The entry.
        block: String,
    },
    /// The entry has not passed the kid-safety review (`age_ok = false`).
    NotAgeOk {
        /// The entry.
        block: String,
    },
}

impl std::fmt::Display for VibeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VibeError::Io(err) => write!(f, "io: {err}"),
            VibeError::Ledger(err) => write!(f, "ledger: {err}"),
            VibeError::DictionaryInvalid { why } => write!(f, "the dictionary is not valid: {why}"),
            VibeError::UnknownEntry { asked, known } => write!(
                f,
                "refused: `{asked}` is not in the Vibe dictionary (known: {})",
                known.join(", ")
            ),
            VibeError::OutOfBounds {
                block,
                value,
                min,
                max,
            } => write!(
                f,
                "refused: {value} is outside `{block}`'s bounds {min}..={max}"
            ),
            VibeError::UnknownMeasure { asked } => {
                write!(f, "refused: no measure answers to `{asked}`")
            }
            VibeError::BadPrediction { text, why } => {
                write!(
                    f,
                    "refused: the prediction `{text}` could not be read: {why}"
                )
            }
            VibeError::NeedsPrediction { block } => write!(
                f,
                "refused: `{block}` predicts no direction of its own; say one with --predict"
            ),
            VibeError::NotAgeOk { block } => write!(
                f,
                "refused: `{block}` has not passed the kid-safety review (age_ok = false)"
            ),
        }
    }
}

impl std::error::Error for VibeError {}

impl From<std::io::Error> for VibeError {
    fn from(err: std::io::Error) -> VibeError {
        VibeError::Io(err)
    }
}

impl From<LedgerError> for VibeError {
    fn from(err: LedgerError) -> VibeError {
        VibeError::Ledger(err)
    }
}

/// A dictionary tier: **A** is locked (a thumbs-up can never widen it),
/// **B** is learnable (sandbox §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Tier {
    /// Locked: bounds and behaviour may not be changed by play.
    A,
    /// Learnable: a block the flow may grow into the dictionary.
    B,
}

/// Which way an entry expects its measure to move.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Expect {
    /// Up.
    Up,
    /// Down.
    Down,
    /// Either way: the entry states no default direction, so a proposal
    /// has to bring its own prediction.
    Either,
}

impl Expect {
    /// The direction of the entry's default prediction, when it has one.
    fn direction(self) -> Option<Direction> {
        match self {
            Expect::Up => Some(Direction::Up),
            Expect::Down => Some(Direction::Down),
            Expect::Either => None,
        }
    }
}

/// One entry of the Vibe dictionary: a typed change, with bounds.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Block {
    /// The stable id, what Bench names out loud (`unit.vision`).
    pub id: String,
    /// The closed vocabulary the entry's gate options are built from; the
    /// first is the label a proposal carries when the kid says no more.
    pub says: Vec<String>,
    /// The one field the entry writes (`params.view_range_cells`). Must be
    /// a branchable parameter: a change may retune the round, never reshape
    /// it.
    pub field: String,
    /// The bound's floor, inclusive.
    pub min: i64,
    /// The bound's ceiling, inclusive.
    pub max: i64,
    /// The value now; must equal the field's value in `Params::default`,
    /// so the baseline the tests run against is the value the game ships.
    pub default: i64,
    /// The measure the entry's default prediction watches.
    pub metric: String,
    /// The direction the entry expects that measure to move.
    pub expect: Expect,
    /// The tier: A locked, B learnable.
    pub tier: Tier,
    /// What Bench says the change does, in plain words.
    pub explain: String,
    /// Kid-safe vocabulary review passed.
    pub age_ok: bool,
}

/// The parsed dictionary.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Dictionary {
    /// The entries, in file order. The table key is `block` (one
    /// `[[block]]` per entry), the roadmap §2.1 shape.
    #[serde(rename = "block")]
    pub blocks: Vec<Block>,
}

impl Dictionary {
    /// Parse and validate a `vibe.toml`.
    ///
    /// Validation is the whole point of the dictionary being data: the
    /// bounds, the fields and the defaults are checked against the code
    /// that runs them **here**, before any proposal touches them.
    pub fn from_toml(text: &str) -> Result<Dictionary, VibeError> {
        let dict: Dictionary =
            toml::from_str(text).map_err(|err| VibeError::DictionaryInvalid {
                why: err.to_string(),
            })?;
        let mut seen: Vec<String> = Vec::new();
        for block in &dict.blocks {
            if block.id.trim().is_empty() {
                return Err(VibeError::DictionaryInvalid {
                    why: "an entry has no id".to_owned(),
                });
            }
            if seen.contains(&block.id) {
                return Err(VibeError::DictionaryInvalid {
                    why: format!("two entries share the id `{}`", block.id),
                });
            }
            seen.push(block.id.clone());
            block.validate()?;
        }
        if dict.blocks.is_empty() {
            return Err(VibeError::DictionaryInvalid {
                why: "the dictionary holds no entries".to_owned(),
            });
        }
        Ok(dict)
    }

    /// Read and validate a `vibe.toml` from disk.
    pub fn from_file(path: &Path) -> Result<Dictionary, VibeError> {
        let text = fs::read_to_string(path)?;
        Self::from_toml(&text)
    }

    /// The dictionary this build ships.
    pub fn default_dictionary() -> Result<Dictionary, VibeError> {
        Self::from_toml(DEFAULT_DICTIONARY)
    }

    /// The ids, in file order, for refusals that name what is known.
    pub fn ids(&self) -> Vec<String> {
        self.blocks.iter().map(|b| b.id.clone()).collect()
    }

    /// The entry of `id`, when the dictionary holds it.
    pub fn find(&self, id: &str) -> Option<&Block> {
        self.blocks.iter().find(|b| b.id == id)
    }
}

impl Block {
    /// Check the entry against the code that runs it.
    fn validate(&self) -> Result<(), VibeError> {
        let bad = |why: String| VibeError::DictionaryInvalid { why };
        if self.says.is_empty() {
            return Err(bad(format!("`{}` says nothing: `says` is empty", self.id)));
        }
        let Some(field) = self.field.strip_prefix("params.") else {
            return Err(bad(format!(
                "`{}` writes `{}`, which is not a sim parameter",
                self.id, self.field
            )));
        };
        if !BRANCH_FIELDS.contains(&field) {
            return Err(bad(format!(
                "`{}` writes `{field}`, which is not a branchable parameter (a Vibe \
                 change may retune a round, never reshape it)",
                self.id
            )));
        }
        if self.min > self.max {
            return Err(bad(format!(
                "`{}`'s bounds are upside down: {}..{}",
                self.id, self.min, self.max
            )));
        }
        if !(self.min..=self.max).contains(&self.default) {
            return Err(bad(format!(
                "`{}`'s default {} is outside its own bounds {}..={}",
                self.id, self.default, self.min, self.max
            )));
        }
        let Some(current) = params_current(field) else {
            return Err(bad(format!(
                "`{}` names a parameter this build has no value for",
                self.id
            )));
        };
        if current != self.default {
            return Err(bad(format!(
                "`{}`'s default is {}, but the parameter ships {}; the dictionary and \
                 the code disagree on what \"now\" is",
                self.id, self.default, current
            )));
        }
        if !eval::is_known_metric(&self.metric) {
            return Err(bad(format!(
                "`{}` watches `{}`, which no measure answers to",
                self.id, self.metric
            )));
        }
        Ok(())
    }

    /// The entry's default prediction, when it states a direction.
    fn default_claim(&self) -> Option<VibeClaim> {
        self.expect.direction().map(|direction| VibeClaim {
            metric: self.metric.clone(),
            direction,
            min_delta: 1,
        })
    }
}

/// The value a branchable parameter ships with, by field name.
///
/// The closed getter that pairs with [`Params::set_branchable`]: the
/// dictionary's `default` is checked against it, so "the current values"
/// the tests run against are the values the game ships.
fn params_current(field: &str) -> Option<i64> {
    let p = Params::default();
    match field {
        "speed_upt" => Some(i64::from(p.speed_upt)),
        "view_range_cells" => Some(i64::from(p.view_range_cells)),
        "tag_range_units" => Some(i64::from(p.tag_range_units)),
        "stun_ticks" => Some(i64::from(p.stun_ticks)),
        "beacon_radius_units" => Some(i64::from(p.beacon_radius_units)),
        "lane_alpha_shift" => Some(i64::from(p.lane_alpha_shift)),
        "lane_deadband_q16" => Some(i64::from(p.lane_deadband_q16)),
        "script_fuel" => i64::try_from(p.script_fuel).ok(),
        _ => None,
    }
}

/// What a kid (through Bench) asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ask {
    /// The dictionary entry (`unit.vision`).
    pub entry: String,
    /// The value proposed.
    pub value: i64,
    /// The prediction as it was worded (`sight_ticks:up,tags:up`), parsed
    /// by [`parse_prediction`]. `None` uses the entry's default.
    pub prediction: Option<String>,
    /// The kid's words as a short label; `None` uses the entry's first
    /// `says` phrase. Bounded at [`LABEL_MAX`], never a transcript.
    pub label: Option<String>,
    /// Who proposed. `"bench"` for this slice; `kid` and `bolt` come later.
    pub author: String,
    /// How many held-out seeds to judge on (the D78 count by default).
    pub seeds: usize,
    /// How many ticks each seed plays.
    pub ticks: u64,
}

impl Ask {
    /// An ask for `entry` = `value`, with the defaults of the flow.
    pub fn new(entry: impl Into<String>, value: i64) -> Ask {
        Ask {
            entry: entry.into(),
            value,
            prediction: None,
            label: None,
            author: "bench".to_owned(),
            seeds: DEFAULT_SEEDS,
            ticks: DEFAULT_TICKS,
        }
    }

    /// The prediction, as it was worded.
    pub fn with_prediction(mut self, prediction: impl Into<String>) -> Ask {
        self.prediction = Some(prediction.into());
        self
    }

    /// The kid's words as a short label.
    pub fn with_label(mut self, label: impl Into<String>) -> Ask {
        self.label = Some(label.into());
        self
    }

    /// How many seeds to judge on.
    pub fn with_seeds(mut self, seeds: usize) -> Ask {
        self.seeds = seeds;
        self
    }

    /// How many ticks each seed plays.
    pub fn with_ticks(mut self, ticks: u64) -> Ask {
        self.ticks = ticks;
        self
    }
}

/// Read a prediction as it is worded: `sight_ticks:up,tags:up`, each claim
/// `measure:direction` with an optional `:min_delta` (`tags:up:3`).
pub fn parse_prediction(text: &str) -> Result<Vec<VibeClaim>, VibeError> {
    let mut claims: Vec<VibeClaim> = Vec::new();
    for part in text.split(',') {
        let part = part.trim();
        let bad = |why: &str| VibeError::BadPrediction {
            text: text.to_owned(),
            why: why.to_owned(),
        };
        if part.is_empty() {
            return Err(bad("an empty claim"));
        }
        let mut bits = part.split(':');
        let name = bits.next().unwrap_or_default().trim();
        let direction = bits.next().unwrap_or_default().trim();
        let min_raw = bits.next();
        if bits.next().is_some() {
            return Err(bad("a claim is measure:direction[:min_delta], no more"));
        }
        let direction = match direction {
            "up" => Direction::Up,
            "down" => Direction::Down,
            "" => return Err(bad("a claim needs a direction: `up` or `down`")),
            other => return Err(bad(&format!("`{other}` is not a direction"))),
        };
        let min_delta = match min_raw {
            None => 1,
            Some(raw) => {
                let value: i64 = raw
                    .trim()
                    .parse()
                    .map_err(|_| bad(&format!("`{raw}` is not a whole number")))?;
                if value < 1 {
                    return Err(bad("the smallest change that counts is at least 1"));
                }
                value
            }
        };
        if !eval::is_known_metric(name) {
            return Err(VibeError::UnknownMeasure {
                asked: name.to_owned(),
            });
        }
        if claims.iter().any(|claim| claim.metric == name) {
            return Err(bad(&format!("`{name}` is claimed twice")));
        }
        claims.push(VibeClaim {
            metric: name.to_owned(),
            direction,
            min_delta,
        });
    }
    if claims.is_empty() {
        return Err(VibeError::BadPrediction {
            text: text.to_owned(),
            why: "no claims".to_owned(),
        });
    }
    if claims.len() > MAX_MEASURES {
        return Err(VibeError::BadPrediction {
            text: text.to_owned(),
            why: format!("a prediction may claim at most {MAX_MEASURES} measures"),
        });
    }
    Ok(claims)
}

/// The label a proposal carries: the kid's words, bounded.
///
/// More than [`LABEL_MAX`] characters is kept as the first `LABEL_MAX`; the
/// words are a label, and a label that grew into a transcript would be a
/// transcript.
pub fn bounded_label(text: &str) -> String {
    text.chars().take(LABEL_MAX).collect()
}

/// The seeds a proposal is judged on, derived from the change itself.
///
/// The same change is always judged on the same seeds, whatever ran before
/// it; two different changes are judged on different ones. The salt is kept
/// small so the seed arithmetic stays far from any overflow.
fn seed_set(block: &str, value: i64, n: usize) -> Vec<u64> {
    let mut h = Fnv64::new();
    h.write(block.as_bytes());
    h.write(&value.to_le_bytes());
    let salt = h.finish() % 65_536;
    eval::eval_seeds(&[], n, salt)
}

/// What one proposal produced: the seal, and the outcome the test reached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proposed {
    /// The sealed proposal, exactly as the chain holds it.
    pub proposal: VibeProposal,
    /// The seal (the proposal's line number in the ledger).
    pub seal: u64,
    /// The outcome the test reached.
    pub outcome: VibeOutcome,
    /// The ledger the seal and the outcome went into.
    pub ledger_path: PathBuf,
}

/// One proposal through the whole pipeline: bounds, seal, test, verdict.
///
/// The prediction is sealed **before** the first seed is played, and the
/// outcome has to name the sealed bytes, so nothing between the seal and
/// the verdict can quietly rewrite what was claimed.
pub fn propose(
    dictionary: &Dictionary,
    ledger_path: &Path,
    ask: &Ask,
) -> Result<Proposed, VibeError> {
    let block = dictionary
        .find(&ask.entry)
        .ok_or_else(|| VibeError::UnknownEntry {
            asked: ask.entry.clone(),
            known: dictionary.ids(),
        })?;
    if !block.age_ok {
        return Err(VibeError::NotAgeOk {
            block: block.id.clone(),
        });
    }
    if !(block.min..=block.max).contains(&ask.value) {
        return Err(VibeError::OutOfBounds {
            block: block.id.clone(),
            value: ask.value,
            min: block.min,
            max: block.max,
        });
    }
    let claims = match &ask.prediction {
        Some(text) => parse_prediction(text)?,
        None => vec![block
            .default_claim()
            .ok_or_else(|| VibeError::NeedsPrediction {
                block: block.id.clone(),
            })?],
    };
    let label = bounded_label(
        ask.label
            .as_deref()
            .unwrap_or(block.says.first().map(String::as_str).unwrap_or_default()),
    );
    let field = block
        .field
        .strip_prefix("params.")
        .expect("the dictionary's load check refused anything else");
    let seeds = seed_set(&block.id, ask.value, ask.seeds);

    // The headers: the baseline is the shipped defaults, the candidate is
    // the same round with the one field changed. The candidate is proven
    // buildable before anything is sealed, so the test can never die half
    // way through a sealed prediction.
    let candidate_header = |seed: u64| -> Result<Header, VibeError> {
        let mut header = Header::default_round(seed, DEFAULT_REFLEX);
        match header.params.set_branchable(field, ask.value) {
            Ok(true) => Ok(header),
            Ok(false) => Err(VibeError::DictionaryInvalid {
                why: format!(
                    "`{}` writes `{}`, which is not a parameter",
                    block.id, field
                ),
            }),
            Err(err) => Err(VibeError::DictionaryInvalid {
                why: format!(
                    "`{}`'s value {} does not apply to `{}`: {}",
                    block.id, ask.value, err.field, err.reason
                ),
            }),
        }
    };
    candidate_header(0)?;

    // Seal first: the prediction is in the chain before any test runs.
    if let Some(dir) = ledger_path.parent() {
        if !dir.as_os_str().is_empty() {
            fs::create_dir_all(dir)?;
        }
    }
    let mut ledger = Ledger::open(ledger_path)?;
    let proposal = VibeProposal {
        block: block.id.clone(),
        field: block.field.clone(),
        from: block.default,
        value: ask.value,
        label,
        author: ask.author.clone(),
        claims: claims.clone(),
        seeds: seeds.clone(),
        ticks: ask.ticks,
    };
    let (seal, seal_hash) = ledger.seal_vibe(proposal.clone())?;

    // The test: every seed, both rounds, every measure at once.
    let metrics: Vec<String> = claims.iter().map(|claim| claim.metric.clone()).collect();
    let baseline = |seed: u64| Ok::<Header, VibeError>(Header::default_round(seed, DEFAULT_REFLEX));
    let measured = sweep(&baseline, &candidate_header, ask.ticks, &seeds, &metrics)?;

    // The verdict, per measure, on the sealed claims: D78's rule.
    let measures: Vec<MeasureOutcome> = claims
        .iter()
        .enumerate()
        .map(|(i, claim)| {
            let deltas: Vec<(u64, i64)> = measured
                .iter()
                .map(|one| (one.seed, one.candidate[i] - one.baseline[i]))
                .collect();
            measure_outcome(claim, deltas)
        })
        .collect();
    let adopted = !measures.is_empty() && measures.iter().all(|measure| measure.met);
    let outcome = VibeOutcome {
        seal,
        seal_hash,
        measures,
        adopted,
    };
    ledger.vibe_outcome(outcome.clone())?;
    Ok(Proposed {
        proposal,
        seal,
        outcome,
        ledger_path: ledger_path.to_path_buf(),
    })
}

/// One seed's two rounds, every measure of the prediction read from both.
struct Measured {
    seed: u64,
    baseline: Vec<i64>,
    candidate: Vec<i64>,
}

/// The header a seed is played with, or why it cannot be.
type HeaderFor<'a> = dyn Fn(u64) -> Result<Header, VibeError> + Send + Sync + 'a;

/// Play every seed's two rounds and read every measure from both.
///
/// The seeds are judged on the machine's cores at once, the same way
/// [`eval::judge_seeds`] judges its own: every seed is played on its own,
/// and the results are put back in seed order, so the sweep is the same
/// measurement a serial run would reach, whatever the machine did in
/// between.
fn sweep(
    baseline: &HeaderFor<'_>,
    candidate: &HeaderFor<'_>,
    ticks: u64,
    seeds: &[u64],
    metrics: &[String],
) -> Result<Vec<Measured>, VibeError> {
    let count = eval::worker_count(seeds.len());
    if count < 2 {
        return sweep_serial(baseline, candidate, ticks, seeds, metrics);
    }
    let mut parts: Vec<Result<Vec<Measured>, VibeError>> = Vec::new();
    thread::scope(|scope| {
        let mut workers = Vec::new();
        for worker in 0..count {
            let from = worker * seeds.len() / count;
            let to = (worker + 1) * seeds.len() / count;
            if from == to {
                continue;
            }
            let part = &seeds[from..to];
            workers
                .push(scope.spawn(move || sweep_serial(baseline, candidate, ticks, part, metrics)));
        }
        for worker in workers {
            match worker.join() {
                Ok(part) => parts.push(part),
                // A worker that panicked is the same panic a serial run
                // would have raised: it is resumed, not swallowed.
                Err(payload) => std::panic::resume_unwind(payload),
            }
        }
    });
    let mut measured = Vec::with_capacity(seeds.len());
    for part in parts {
        measured.extend(part?);
    }
    Ok(measured)
}

/// The sweep's serial reference: one seed at a time, in order.
fn sweep_serial(
    baseline: &HeaderFor<'_>,
    candidate: &HeaderFor<'_>,
    ticks: u64,
    seeds: &[u64],
    metrics: &[String],
) -> Result<Vec<Measured>, VibeError> {
    let read = |log: &lm_core::log::RoundLog| -> Result<Vec<i64>, VibeError> {
        let m = RoundMetrics::from_log(log);
        metrics
            .iter()
            .map(|name| {
                eval::metric_value(&m, name).ok_or_else(|| VibeError::UnknownMeasure {
                    asked: name.clone(),
                })
            })
            .collect()
    };
    let mut measured = Vec::with_capacity(seeds.len());
    for &seed in seeds {
        let baseline_log = play(baseline(seed)?, ticks);
        let candidate_log = play(candidate(seed)?, ticks);
        measured.push(Measured {
            seed,
            baseline: read(&baseline_log)?,
            candidate: read(&candidate_log)?,
        });
    }
    Ok(measured)
}

/// One measure's outcome: the deltas, the two halves of D78's rule, and
/// whether the claim held.
fn measure_outcome(claim: &VibeClaim, deltas: Vec<(u64, i64)>) -> MeasureOutcome {
    let pred = Prediction {
        round_id: String::new(),
        proposal_hash: String::new(),
        metric: claim.metric.clone(),
        direction: claim.direction,
        min_delta: claim.min_delta,
        eval_seeds: Vec::new(),
        author: String::new(),
        text: String::new(),
    };
    let qualified = deltas
        .iter()
        .filter(|(_, delta)| eval::qualified_delta(*delta, &pred))
        .count() as u64;
    let test = eval::sign_test_of(&deltas, claim.direction);
    MeasureOutcome {
        met: eval::passes(&deltas, &pred),
        claim: claim.clone(),
        deltas,
        qualified,
        moved: test.moved as u64,
        claimed: test.claimed as u64,
        p: test.p_value_text(),
    }
}

/// A proposal and its outcome, as the ledger holds them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VibeRecord {
    /// The seal (the proposal's line number).
    pub seal: u64,
    /// The sealed proposal.
    pub proposal: VibeProposal,
    /// The outcome, when the test has run and been appended.
    pub outcome: Option<VibeOutcome>,
}

/// Every Vibe record in a ledger, in seal order.
pub fn records(ledger: &Ledger) -> Vec<VibeRecord> {
    use crate::ledger::Entry;
    let mut out: Vec<VibeRecord> = Vec::new();
    for (line, entry) in ledger.entries().enumerate() {
        match entry {
            Entry::VibeProposal(proposal) => out.push(VibeRecord {
                seal: line as u64,
                proposal: proposal.clone(),
                outcome: None,
            }),
            Entry::VibeOutcome(outcome) => {
                if let Some(record) = out.iter_mut().find(|record| record.seal == outcome.seal) {
                    record.outcome = Some(outcome.clone());
                }
            }
            _ => {}
        }
    }
    out
}

/// The prediction-trust tally: how many measures held, of how many were
/// claimed and judged.
///
/// This is the sandbox's own instrument (the prior art's L3 change): a
/// prediction that holds is trust spent well, and a rate that falls is a
/// fault in the predictor, not in the kid.
pub fn trust(records: &[VibeRecord]) -> (u64, u64) {
    let (mut held, mut of) = (0u64, 0u64);
    for record in records {
        if let Some(outcome) = &record.outcome {
            for measure in &outcome.measures {
                of += 1;
                held += u64::from(measure.met);
            }
        }
    }
    (held, of)
}

impl Proposed {
    /// What the proposal did, in words: the change, the seal, the test and
    /// the verdict, the way the between-round screen will read them.
    pub fn summary(&self) -> String {
        let mut s = String::new();
        let p = &self.proposal;
        let _ = writeln!(s, "{} {} -> {}  \"{}\"", p.block, p.from, p.value, p.label);
        let _ = writeln!(
            s,
            "  sealed:   seal {} in {} (before the test)",
            self.seal,
            self.ledger_path.display()
        );
        for claim in &p.claims {
            let _ = writeln!(
                s,
                "  predict:  {} {} by at least {}",
                claim.metric,
                match claim.direction {
                    Direction::Up => "up",
                    Direction::Down => "down",
                },
                claim.min_delta
            );
        }
        let _ = writeln!(
            s,
            "  test:     {} seeds x {} ticks, {} vs {}",
            p.seeds.len(),
            p.ticks,
            p.from,
            p.value
        );
        if p.ticks > 0 && p.ticks < MEASURABLE_TICKS {
            let _ = writeln!(
                s,
                "  (a horizon under {MEASURABLE_TICKS} ticks measures little: most metrics \
                 are zero there, so a not-met verdict is a refusal, not a reading)"
            );
        }
        let held = self.outcome.measures.iter().filter(|m| m.met).count();
        for measure in &self.outcome.measures {
            let _ = writeln!(
                s,
                "  {:<12} {} {}/{} seeds qualified, {}/{} moved the claimed way, p = {} — {}",
                measure.claim.metric,
                match measure.claim.direction {
                    Direction::Up => "up  ",
                    Direction::Down => "down",
                },
                measure.qualified,
                measure.deltas.len(),
                measure.claimed,
                measure.moved,
                measure.p,
                if measure.met { "MET" } else { "NOT MET" },
            );
        }
        let _ = writeln!(
            s,
            "  verdict:  {} ({} of {} predictions held)",
            if self.outcome.adopted {
                "ADOPTED"
            } else {
                "NOT ADOPTED"
            },
            held,
            self.outcome.measures.len()
        );
        s
    }
}

/// The ledger's records and the prediction-trust tally, in words.
///
/// This is `lm vibe ledger`: every record on one line, then how often the
/// predictions held.
pub fn ledger_summary(ledger: &Ledger) -> String {
    let mut s = String::new();
    let all = records(ledger);
    if all.is_empty() {
        let _ = writeln!(s, "no Vibe records yet");
        return s;
    }
    for record in &all {
        let p = &record.proposal;
        let _ = write!(
            s,
            "seal {}  {} {} -> {}  \"{}\"",
            record.seal, p.block, p.from, p.value, p.label
        );
        match &record.outcome {
            Some(outcome) => {
                let words: Vec<String> = outcome
                    .measures
                    .iter()
                    .map(|measure| {
                        format!(
                            "{}:{}",
                            measure.claim.metric,
                            if measure.met { "MET" } else { "not-met" }
                        )
                    })
                    .collect();
                let _ = write!(
                    s,
                    "  [{}] — {}",
                    words.join(" "),
                    if outcome.adopted {
                        "adopted"
                    } else {
                        "not adopted"
                    }
                );
            }
            None => {
                let _ = write!(s, "  sealed, not yet tested");
            }
        }
        s.push('\n');
    }
    let (held, of) = trust(&all);
    // The percentage is integer maths, floor: no float near a tally.
    match (held * 100).checked_div(of) {
        None => {
            let _ = writeln!(s, "prediction trust: nothing judged yet");
        }
        Some(pct) => {
            let _ = writeln!(
                s,
                "prediction trust: {held}/{of} measures held ({pct}%) across {} proposals",
                all.iter().filter(|record| record.outcome.is_some()).count()
            );
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ledger file of this test's own.
    fn tmp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "lm-vibe-{tag}-{}-{}.jsonl",
            std::process::id(),
            line!()
        ));
        let _ = fs::remove_file(&p);
        p
    }

    /// The shipped dictionary must load: this is the one entry the slice
    /// promises.
    #[test]
    fn the_shipped_dictionary_loads() {
        let dict = Dictionary::default_dictionary().expect("vibe.toml loads");
        let block = dict.find("unit.vision").expect("the entry is there");
        assert_eq!(block.min, 4);
        assert_eq!(block.max, 64);
        assert_eq!(block.default, 20);
        assert_eq!(
            block.default,
            params_current("view_range_cells").unwrap(),
            "the default must be the value the sim ships"
        );
        assert_eq!(block.tier, Tier::A);
        assert!(block.age_ok);
    }

    #[test]
    fn a_dictionary_that_disagrees_with_the_code_is_refused() {
        // A default the parameter does not ship with: the dictionary and
        // the code disagree on what "now" is.
        let text = r#"
[[block]]
id = "unit.vision"
says = ["let them see farther"]
field = "params.view_range_cells"
min = 4
max = 64
default = 21
metric = "sight_ticks"
expect = "up"
tier = "A"
explain = "x"
age_ok = true
"#;
        assert!(matches!(
            Dictionary::from_toml(text),
            Err(VibeError::DictionaryInvalid { .. })
        ));

        // A field that is not a parameter a change may retune.
        let text = text.replacen("params.view_range_cells", "params.beacons", 1);
        assert!(matches!(
            Dictionary::from_toml(&text),
            Err(VibeError::DictionaryInvalid { .. })
        ));

        // A field that is not a parameter at all.
        let text = text.replacen("params.beacons", "rules.paint.bounce_q8", 1);
        assert!(matches!(
            Dictionary::from_toml(&text),
            Err(VibeError::DictionaryInvalid { .. })
        ));

        // A measure nothing answers to.
        let text = text.replacen("sight_ticks", "vibes", 1);
        assert!(matches!(
            Dictionary::from_toml(&text),
            Err(VibeError::DictionaryInvalid { .. })
        ));
    }

    #[test]
    fn an_unknown_entry_is_a_named_refusal() {
        let dict = Dictionary::default_dictionary().unwrap();
        let err = propose(
            &dict,
            &tmp("unknown"),
            &Ask::new("unit.speed", 200).with_seeds(1).with_ticks(30),
        )
        .unwrap_err();
        let VibeError::UnknownEntry { asked, known } = &err else {
            panic!("the wrong refusal: {err}");
        };
        assert_eq!(asked, "unit.speed");
        assert_eq!(known, &vec!["unit.vision".to_owned()]);
        assert!(err.to_string().contains("refused"));
    }

    #[test]
    fn an_out_of_bounds_value_is_a_named_refusal_never_a_clamp() {
        let dict = Dictionary::default_dictionary().unwrap();
        for value in [3, 65, -1, 1_000_000] {
            let err = propose(
                &dict,
                &tmp("bounds"),
                &Ask::new("unit.vision", value).with_seeds(1).with_ticks(30),
            )
            .unwrap_err();
            let VibeError::OutOfBounds {
                value: refused,
                min,
                max,
                ..
            } = &err
            else {
                panic!("the wrong refusal for {value}: {err}");
            };
            assert_eq!(*refused, value, "the value was clamped or renamed: {err}");
            assert_eq!((*min, *max), (4, 64));
            assert!(err.to_string().contains("refused"));
            // And nothing was written: a refusal is not a record.
            assert!(!tmp("bounds").exists());
        }
    }

    #[test]
    fn predictions_are_read_as_worded() {
        let claims = parse_prediction("sight_ticks:up,tags:up").unwrap();
        assert_eq!(
            claims,
            vec![
                VibeClaim {
                    metric: "sight_ticks".into(),
                    direction: Direction::Up,
                    min_delta: 1,
                },
                VibeClaim {
                    metric: "tags".into(),
                    direction: Direction::Up,
                    min_delta: 1,
                },
            ]
        );
        let claims = parse_prediction(" tags:down:3 ").unwrap();
        assert_eq!(
            claims,
            vec![VibeClaim {
                metric: "tags".into(),
                direction: Direction::Down,
                min_delta: 3,
            }]
        );

        for bad in [
            "",
            "sight_ticks",
            "sight_ticks:sideways",
            "sight_ticks:up:0",
            "sight_ticks:up,tight_pants:up",
            "a:up,b:up,c:up,d:up,e:up",
            "sight_ticks:up:1:2",
        ] {
            assert!(parse_prediction(bad).is_err(), "`{bad}` was read");
        }
    }

    #[test]
    fn a_label_is_bounded_never_a_transcript() {
        let long = "x".repeat(LABEL_MAX + 40);
        let bounded = bounded_label(&long);
        assert_eq!(bounded.chars().count(), LABEL_MAX);
        assert_eq!(bounded_label("short"), "short");
    }

    /// The seeds are the same every time for the same change, and different
    /// for a different one.
    #[test]
    fn the_seeds_follow_the_change() {
        let a = seed_set("unit.vision", 32, 4);
        let again = seed_set("unit.vision", 32, 4);
        let other = seed_set("unit.vision", 48, 4);
        assert_eq!(a, again);
        assert_ne!(a, other);
        assert!(!a.is_empty());
    }

    /// One full pass, small enough to run anywhere: the seal lands before
    /// the test, the outcome answers it, and the ledger reads back.
    #[test]
    fn a_proposal_runs_the_whole_pipeline() {
        let dict = Dictionary::default_dictionary().unwrap();
        let path = tmp("pipeline");
        let asked = Ask::new("unit.vision", 64)
            .with_prediction("sight_ticks:up")
            .with_seeds(2)
            .with_ticks(200);
        let proposed = propose(&dict, &path, &asked).expect("the proposal runs");

        assert_eq!(proposed.proposal.from, 20);
        assert_eq!(proposed.proposal.value, 64);
        assert_eq!(proposed.seal, 0);
        assert_eq!(proposed.outcome.seal, 0);
        assert_eq!(proposed.outcome.measures.len(), 1);
        assert_eq!(proposed.outcome.measures[0].claim.metric, "sight_ticks");
        // The outcome names the bytes that were sealed.
        assert_eq!(proposed.outcome.seal_hash.len(), 16);

        // The ledger reads back as the same records, in order.
        let ledger = Ledger::open(&path).unwrap();
        let all = records(&ledger);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].proposal, proposed.proposal);
        assert_eq!(all[0].outcome.as_ref().unwrap(), &proposed.outcome);
        let (held, of) = trust(&all);
        assert_eq!(of, 1);
        assert_eq!(held, u64::from(proposed.outcome.measures[0].met));
        assert!(ledger_summary(&ledger).contains("prediction trust"));
    }

    /// A second proposal joins the first in the same chain, and the trust
    /// tally counts both.
    #[test]
    fn the_ledger_round_trips_and_tallies() {
        let dict = Dictionary::default_dictionary().unwrap();
        let path = tmp("roundtrip");
        for (value, prediction) in [(32, "sight_ticks:up"), (8, "sight_ticks:down")] {
            let asked = Ask::new("unit.vision", value)
                .with_prediction(prediction)
                .with_seeds(2)
                .with_ticks(200);
            propose(&dict, &path, &asked).expect("the proposal runs");
        }
        let ledger = Ledger::open(&path).unwrap();
        let all = records(&ledger);
        assert_eq!(all.len(), 2, "both proposals are in the chain");
        assert_ne!(all[0].proposal.value, all[1].proposal.value);
        let (held, of) = trust(&all);
        assert_eq!(of, 2, "one measure claimed per proposal");
        assert!(held <= 2);
        let text = ledger_summary(&ledger);
        // The seal is the proposal's line number in the chain, so the
        // second record sits at seal 2 (after the first proposal and its
        // outcome).
        assert!(text.contains("seal 0"));
        assert!(text.contains("seal 2"));
        assert!(text.contains("prediction trust"));
    }

    /// The headline rule of the slice: the prediction is sealed before the
    /// test, and editing it after the fact breaks the seal check.
    #[test]
    fn a_sealed_prediction_cannot_be_edited_after_the_test() {
        let dict = Dictionary::default_dictionary().unwrap();
        let path = tmp("sealed");
        let asked = Ask::new("unit.vision", 48)
            .with_prediction("sight_ticks:up")
            .with_seeds(2)
            .with_ticks(200);
        propose(&dict, &path, &asked).expect("the proposal runs");

        // Quietly "improve" the sealed direction after the test ran.
        let text = fs::read_to_string(&path).unwrap();
        let doctored = text.replacen("\"direction\":\"Up\"", "\"direction\":\"Down\"", 1);
        assert_ne!(doctored, text, "the seal held nothing to edit");
        fs::write(&path, doctored).unwrap();
        assert!(
            Ledger::open(&path).is_err(),
            "an edited prediction still opened: the seal does not check itself"
        );
    }

    /// The default prediction comes from the entry when none is worded.
    #[test]
    fn the_entry_s_default_prediction_is_used() {
        let dict = Dictionary::default_dictionary().unwrap();
        let path = tmp("default-prediction");
        let asked = Ask::new("unit.vision", 32).with_seeds(1).with_ticks(100);
        let proposed = propose(&dict, &path, &asked).expect("the proposal runs");
        assert_eq!(
            proposed.proposal.claims,
            vec![VibeClaim {
                metric: "sight_ticks".into(),
                direction: Direction::Up,
                min_delta: 1,
            }]
        );
    }

    /// An entry that expects "either" states no direction of its own, so a
    /// proposal has to bring one.
    #[test]
    fn an_either_entry_demands_a_prediction() {
        let text = DEFAULT_DICTIONARY.replacen("expect = \"up\"", "expect = \"either\"", 1);
        let dict = Dictionary::from_toml(&text).unwrap();
        let err = propose(
            &dict,
            &tmp("either"),
            &Ask::new("unit.vision", 32).with_seeds(1).with_ticks(30),
        )
        .unwrap_err();
        assert!(matches!(err, VibeError::NeedsPrediction { .. }));
        assert!(err.to_string().contains("--predict"));
    }

    /// A prediction naming a measure nothing answers to is a refusal.
    #[test]
    fn an_unknown_measure_is_a_named_refusal() {
        let dict = Dictionary::default_dictionary().unwrap();
        let err = propose(
            &dict,
            &tmp("measure"),
            &Ask::new("unit.vision", 32)
                .with_prediction("vibes:up")
                .with_seeds(1)
                .with_ticks(30),
        )
        .unwrap_err();
        let VibeError::UnknownMeasure { asked } = &err else {
            panic!("the wrong refusal: {err}");
        };
        assert_eq!(asked, "vibes");
    }

    /// The same ask always measures the same thing: the pipeline is
    /// deterministic, seed for seed, delta for delta.
    #[test]
    fn the_sweep_is_deterministic() {
        let dict = Dictionary::default_dictionary().unwrap();
        let asked = Ask::new("unit.vision", 48)
            .with_prediction("sight_ticks:up")
            .with_seeds(3)
            .with_ticks(240);
        let a = propose(&dict, &tmp("det-a"), &asked).unwrap();
        let b = propose(&dict, &tmp("det-b"), &asked).unwrap();
        assert_eq!(a.outcome, b.outcome);
    }
}
