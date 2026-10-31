//! The Vibe pipeline: a typed change, a sealed prediction, a held-out test,
//! a verdict, a landing — the sandbox's first slice (M2b).
//!
//! ```text
//! words -> a dictionary entry and a value (bounds checked; refusals named)
//!      -> a prediction on two or three measures, sealed BEFORE the test
//!      -> a headless sweep: the D78 seeds, baseline vs the change
//!      -> a verdict per measure (D78's rule), adopt iff every measure held
//!      -> a landing: the adopted values join the harness the next round
//!         is played with, and the next round's header names the records
//!      -> an undo: "put it back" is a new ledger line, never an edit
//! ```
//!
//! The dictionary is data (`assets/harness/default/vibe.toml`), and its
//! bounds are enforced **before** anything runs: an out-of-bounds value or
//! an unknown entry is a refusal that names itself, never a clamp. The
//! prediction is sealed in the [ledger](crate::ledger)'s hash chain before
//! the first seed is played, so it cannot be edited to match the result
//! afterwards — the outcome has to name the bytes that were sealed.
//!
//! The landing reuses the one path into the harness that `lm learn`
//! already commits through ([`crate::flow`]): the in-force values are
//! written as data into the harness root and committed with the same
//! discipline, and the next round's header carries both the values (in its
//! params, which have always been hashed) and the records that put them
//! there ([`lm_core::VibeCarried`], hashed when present — a round with no
//! records in force hashes exactly as it always did). The undo is a new
//! `VibeUndo` line in the same append-only chain.
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
use lm_core::{Fnv64, VibeCarried};

use crate::eval;
use crate::flow::{DEFAULT_EVAL_SEEDS, DEFAULT_REFLEX, MEASURABLE_TICKS};
use crate::ledger::{
    Direction, Ledger, LedgerError, MeasureOutcome, Prediction, VibeClaim, VibeOutcome,
    VibeProposal, VibeUndo,
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
    /// The record named is not the one in force for its entry: a later
    /// record already moved the same field, and undoing a buried record
    /// would pretend history is simpler than it is.
    NotInForce {
        /// The record asked for.
        seal: u64,
        /// The entry.
        block: String,
        /// The record whose value is in force.
        in_force: u64,
    },
    /// A record in force names an entry the dictionary no longer holds.
    NotInDictionary {
        /// The entry that has gone.
        block: String,
    },
    /// The harness's Vibe state disagrees with the ledger: the file was
    /// changed by something other than the pipeline.
    StateDrift {
        /// The file.
        path: PathBuf,
        /// Why it was refused.
        why: String,
    },
    /// The harness (or its git) refused the landing.
    Flow(crate::flow::FlowError),
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
            VibeError::NotInForce {
                seal,
                block,
                in_force,
            } => write!(
                f,
                "refused: record {seal} is not in force for `{block}`; record {in_force} moved \
                 the same field later — undo that one first"
            ),
            VibeError::NotInDictionary { block } => write!(
                f,
                "refused: a record in force names `{block}`, which the dictionary no longer \
                 holds; the state cannot land until it is undone"
            ),
            VibeError::StateDrift { path, why } => write!(
                f,
                "refused: {} disagrees with the ledger: {why}",
                path.display()
            ),
            VibeError::Flow(err) => write!(f, "the landing: {err}"),
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

impl From<crate::flow::FlowError> for VibeError {
    fn from(err: crate::flow::FlowError) -> VibeError {
        VibeError::Flow(err)
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

/// Where an adopted change lands: the harness the next round is played
/// with, and the seed that round will run on.
///
/// This is the same place `lm learn` commits its adopted proposals — the
/// harness root, one directory per agent — and the landing reuses the same
/// commit discipline ([`crate::flow`]'s), so there is one path into the
/// harness, not two.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LandSite {
    /// The harness root, as `lm learn --harness` takes it.
    pub harness_root: PathBuf,
    /// The seed of the next round, whose header the landing writes.
    pub next_seed: u64,
}

impl LandSite {
    /// A landing site for a harness root and a next-round seed.
    pub fn new(harness_root: impl Into<PathBuf>, next_seed: u64) -> LandSite {
        LandSite {
            harness_root: harness_root.into(),
            next_seed,
        }
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
    /// Where an adopted change lands, and the next round it is played in.
    /// `None` runs the pipeline without touching any harness: the verdict
    /// is still sealed, tested and recorded.
    pub land: Option<LandSite>,
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
            land: None,
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

    /// Land an adopted change in this harness, for this next-round seed.
    pub fn with_land(mut self, site: LandSite) -> Ask {
        self.land = Some(site);
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
    /// The landing the verdict produced, when the ask named a harness and
    /// the change was adopted.
    pub landed: Option<Landed>,
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

    // The landing: when the ask named a harness, the verdict reaches it —
    // an adopted change becomes part of the harness the next round is
    // played with, the same way `lm learn` lands an adopted proposal. A
    // refused verdict lands nothing new, but the projection is re-taken,
    // so an adopted record that never landed (a run with no harness named)
    // is healed by the next one that does.
    let landed = match &ask.land {
        Some(site) => Some(land(dictionary, ledger_path, site)?),
        None => None,
    };
    Ok(Proposed {
        proposal,
        seal,
        outcome,
        ledger_path: ledger_path.to_path_buf(),
        landed,
    })
}

/// One adopted change in force, and the ledger line that put it there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InForce {
    /// The ledger line (the seal) of the adopted record.
    pub seal: u64,
    /// The dictionary entry (`unit.vision`).
    pub block: String,
    /// The one field the record writes (`params.view_range_cells`).
    pub field: String,
    /// The value in force.
    pub value: i64,
}

/// What the ledger has in force now: for every entry, the latest record
/// that was adopted and not undone, in seal order.
///
/// The ledger is the truth — [state](state_summary) reads it live — and
/// everything else (the state file in the harness, the records in the next
/// round's header) is a projection of this walk.
pub fn in_force(ledger: &Ledger) -> Vec<InForce> {
    let undone: Vec<u64> = ledger
        .entries()
        .filter_map(|entry| match entry {
            crate::ledger::Entry::VibeUndo(undo) => Some(undo.seal),
            _ => None,
        })
        .collect();
    let mut per_block: std::collections::BTreeMap<String, InForce> =
        std::collections::BTreeMap::new();
    for record in records(ledger) {
        let Some(outcome) = &record.outcome else {
            continue;
        };
        if !outcome.adopted || undone.contains(&record.seal) {
            continue;
        }
        per_block.insert(
            record.proposal.block.clone(),
            InForce {
                seal: record.seal,
                block: record.proposal.block.clone(),
                field: record.proposal.field.clone(),
                value: record.proposal.value,
            },
        );
    }
    let mut out: Vec<InForce> = per_block.into_values().collect();
    out.sort_by_key(|record| record.seal);
    out
}

/// The name of the state file inside the harness root.
pub const STATE_FILE: &str = "vibe-state.toml";

/// One entry of the state file on disk.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StateEntry {
    /// The ledger line (the seal) that adopted the change.
    pub seal: u64,
    /// The dictionary entry (`unit.vision`).
    pub block: String,
    /// The one field the entry writes (`params.view_range_cells`).
    pub field: String,
    /// The value in force.
    pub value: i64,
}

/// The projection on disk: `<harness_root>/vibe-state.toml`.
///
/// Data, not code: the file says what is in force and which ledger lines
/// put it there, so the harness the next round is played with carries an
/// adopted change the way it carries a reflex or a gate. It holds no words
/// beyond the entry's stable id — no transcript, no audio, ever.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StateFile {
    /// The entries in force, in seal order.
    #[serde(default, rename = "in_force")]
    pub entries: Vec<StateEntry>,
}

/// Write the state file: the projection, as TOML, with the provenance in a
/// header the next reader can follow.
fn write_state(path: &Path, state: &StateFile) -> Result<(), VibeError> {
    let body = toml::to_string_pretty(state).map_err(|err| {
        VibeError::Io(std::io::Error::other(format!(
            "the Vibe state does not serialise: {err}"
        )))
    })?;
    let text = format!(
        "# The Vibe state: the adopted changes in force, projected from the\n\
         # ledger ({STATE_FILE} is written by `lm vibe propose/undo --harness`).\n\
         # The ledger is the truth; this file is the copy the next round is\n\
         # played with. It holds entry ids and values — never words, never\n\
         # audio.\n\n{body}"
    );
    fs::write(path, text)?;
    Ok(())
}

/// Is this state file one the ledger could have written?
///
/// A stale projection is fine — the ledger moved, and the landing
/// re-projects. A file holding a change no ledger line ever adopted is a
/// doctored file, and a doctored harness is refused, not healed.
fn explained_by(state: &StateFile, ledger: &Ledger) -> bool {
    let all = records(ledger);
    state.entries.iter().all(|entry| {
        all.iter().any(|record| {
            record.seal == entry.seal
                && record.proposal.block == entry.block
                && record.proposal.field == entry.field
                && record.proposal.value == entry.value
                && record
                    .outcome
                    .as_ref()
                    .is_some_and(|outcome| outcome.adopted)
        })
    })
}

/// What a landing produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Landed {
    /// The state file the harness now carries.
    pub state_path: PathBuf,
    /// The commit the landing made, when the harness root is a git repo of
    /// its own; `None` means the harness is identified by its bytes, and
    /// nothing is pretended.
    pub commit: Option<String>,
    /// The next round's header: the shipped round, the harness pinned, the
    /// in-force values applied and their records named.
    pub header: Header,
    /// Where the header was written.
    pub header_path: PathBuf,
    /// The records in force — what the header carries.
    pub in_force: Vec<InForce>,
}

/// Land the ledger's adopted changes in the harness — the same path `lm
/// learn` lands an adopted proposal through, not a second one.
///
/// * the in-force values are written as data into the harness
///   (`vibe-state.toml`), committed with the same discipline (a date
///   derived from the landing's id, so the same state always produces the
///   same commit, and a `round-<id>` tag);
/// * the next round's header is written: the shipped round, the harnesses
///   pinned (D27), every in-force value applied to the params, and the
///   records named in [`Header::vibe`], so a replay knows where each value
///   came from.
///
/// A record whose entry left the dictionary, or whose value left its
/// bounds, is a named refusal: the state never lands what the dictionary
/// no longer vouches for.
pub fn land(
    dictionary: &Dictionary,
    ledger_path: &Path,
    site: &LandSite,
) -> Result<Landed, VibeError> {
    let ledger = Ledger::open(ledger_path)?;
    let force = in_force(&ledger);

    for record in &force {
        let block = dictionary
            .find(&record.block)
            .ok_or_else(|| VibeError::NotInDictionary {
                block: record.block.clone(),
            })?;
        if !(block.min..=block.max).contains(&record.value) {
            return Err(VibeError::OutOfBounds {
                block: block.id.clone(),
                value: record.value,
                min: block.min,
                max: block.max,
            });
        }
    }

    // The harness, the same requirement `lm learn` makes of its root.
    let harnesses = crate::flow::read_harness_set(&site.harness_root)?;
    if harnesses.is_empty() {
        return Err(VibeError::Flow(crate::flow::FlowError::Harness(format!(
            "no agent harness under {}",
            site.harness_root.display()
        ))));
    }

    // The state file: the projection, written only when it moved. A file
    // the ledger cannot explain is refused, not healed.
    let state_path = site.harness_root.join(STATE_FILE);
    let projection = StateFile {
        entries: force
            .iter()
            .map(|record| StateEntry {
                seal: record.seal,
                block: record.block.clone(),
                field: record.field.clone(),
                value: record.value,
            })
            .collect(),
    };
    match fs::read_to_string(&state_path) {
        Ok(text) => {
            let current: StateFile =
                toml::from_str(&text).map_err(|err| VibeError::StateDrift {
                    path: state_path.clone(),
                    why: format!("it does not read: {err}"),
                })?;
            if current != projection {
                if !explained_by(&current, &ledger) {
                    return Err(VibeError::StateDrift {
                        path: state_path.clone(),
                        why: "it holds a change no ledger line adopted".to_owned(),
                    });
                }
                write_state(&state_path, &projection)?;
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            write_state(&state_path, &projection)?;
        }
        Err(err) => return Err(err.into()),
    }

    // The commit: one discipline with `lm learn`. The id is the latest
    // seal in force — the state as of that record — so the same state
    // always produces the same commit and the same tag.
    let id = force
        .last()
        .map(|record| format!("vibe-{}", record.seal))
        .unwrap_or_else(|| "vibe-none".to_owned());
    let said: Vec<String> = force
        .iter()
        .map(|record| format!("{} = {}", record.block, record.value))
        .collect();
    let message = if said.is_empty() {
        "vibe: nothing in force; back to the shipped defaults".to_owned()
    } else {
        format!("vibe: {}", said.join(", "))
    };
    let (commit, _diff) = crate::flow::commit_tree(&site.harness_root, &id, &message)?;

    // The next round's header: the shipped round, the harness pinned, the
    // values applied, the records named.
    let mut header = crate::flow::round_header(site.next_seed, &harnesses);
    for record in &force {
        let field = record
            .field
            .strip_prefix("params.")
            .expect("the dictionary's load check refused anything else");
        match header.params.set_branchable(field, record.value) {
            Ok(true) => {}
            Ok(false) => {
                return Err(VibeError::DictionaryInvalid {
                    why: format!(
                        "`{}` writes `{field}`, which is not a parameter",
                        record.block
                    ),
                });
            }
            Err(err) => {
                return Err(VibeError::DictionaryInvalid {
                    why: format!(
                        "`{}`'s value {} no longer applies to `{}`: {}",
                        record.block, record.value, err.field, err.reason
                    ),
                });
            }
        }
    }
    header.vibe = force
        .iter()
        .map(|record| VibeCarried {
            seal: record.seal,
            block: record.block.clone(),
            value: record.value,
        })
        .collect();
    let header_path = ledger_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("vibe")
        .join("next-header.json");
    if let Some(dir) = header_path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(
        &header_path,
        serde_json::to_string_pretty(&header).map_err(|err| {
            VibeError::Io(std::io::Error::other(format!(
                "the next header does not serialise: {err}"
            )))
        })?,
    )?;
    Ok(Landed {
        state_path,
        commit,
        header,
        header_path,
        in_force: force,
    })
}

/// What an undo did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Undone {
    /// The undo record, exactly as the chain holds it.
    pub undo: VibeUndo,
    /// Its line number in the ledger.
    pub line: u64,
    /// The landing the undo re-projected, when a harness was named.
    pub landed: Option<Landed>,
}

/// Undo one adopted record: append the inverse to the ledger — the chain is
/// append-only, so "put it back" is a new line, not an edit — and re-land
/// the harness on what is in force now.
///
/// Only the record whose value is in force for its entry can be undone:
/// each entry's value always comes from exactly one record, and undoing a
/// buried one would pretend history is simpler than it is. The field goes
/// back to the previous record in force for the entry, when there is one,
/// else to the entry's shipped default.
pub fn undo(
    dictionary: &Dictionary,
    ledger_path: &Path,
    seal: u64,
    author: &str,
    site: Option<&LandSite>,
) -> Result<Undone, VibeError> {
    let mut ledger = Ledger::open(ledger_path)?;
    let all = records(&ledger);
    let record = all
        .iter()
        .find(|record| record.seal == seal)
        .ok_or(VibeError::Ledger(LedgerError::NoSuchSeal(seal)))?;
    let block = record.proposal.block.clone();

    // The refusals name themselves in the order a reader meets them: a
    // record that was never adopted has nothing to undo (and one already
    // undone would rewrite history), and a record that is not the one in
    // force for its entry answers to the later one.
    if record
        .outcome
        .as_ref()
        .is_none_or(|outcome| !outcome.adopted)
    {
        let undone = ledger.entries().any(
            |entry| matches!(entry, crate::ledger::Entry::VibeUndo(undo) if undo.seal == seal),
        );
        return Err(VibeError::Ledger(if undone {
            LedgerError::AlreadyUndone(seal)
        } else {
            LedgerError::NotAdopted(seal)
        }));
    }
    if let Some(current) = in_force(&ledger)
        .iter()
        .find(|record| record.block == block)
    {
        if current.seal != seal {
            return Err(VibeError::NotInForce {
                seal,
                block,
                in_force: current.seal,
            });
        }
    }

    // What the field goes back to: the previous record in force for the
    // entry — adopted, not undone, earlier than this one — else the
    // dictionary's shipped default.
    let undone_seals: Vec<u64> = ledger
        .entries()
        .filter_map(|entry| match entry {
            crate::ledger::Entry::VibeUndo(undo) => Some(undo.seal),
            _ => None,
        })
        .collect();
    let restored = all
        .iter()
        .filter(|other| {
            other.seal < seal
                && other.proposal.block == block
                && !undone_seals.contains(&other.seal)
                && other
                    .outcome
                    .as_ref()
                    .is_some_and(|outcome| outcome.adopted)
        })
        .max_by_key(|other| other.seal)
        .map(|other| other.proposal.value)
        .unwrap_or_else(|| {
            dictionary
                .find(&block)
                .map(|entry| entry.default)
                .unwrap_or_default()
        });

    let undo = VibeUndo {
        seal,
        seal_hash: record
            .outcome
            .as_ref()
            .expect("a record in force has an outcome")
            .seal_hash
            .clone(),
        field: record.proposal.field.clone(),
        from: record.proposal.value,
        value: restored,
        author: author.to_owned(),
    };
    let line = ledger.undo_vibe(undo.clone())?;

    let landed = match site {
        Some(site) => Some(land(dictionary, ledger_path, site)?),
        None => None,
    };
    Ok(Undone { undo, line, landed })
}

/// What is in force now, in words — `lm vibe state`.
///
/// One line per record in force, naming the ledger line that adopted it;
/// the entries resting at their shipped default are one compact line
/// beneath. A record whose entry left the dictionary is still named: it is
/// in force whether the dictionary still words it or not (the landing will
/// refuse it until it is undone).
pub fn state_summary(ledger: &Ledger, dictionary: &Dictionary) -> String {
    let mut s = String::new();
    let force = in_force(ledger);
    if force.is_empty() {
        let _ = writeln!(s, "nothing in force: every entry ships its default");
    } else {
        let _ = writeln!(s, "in force:");
        for record in &force {
            let _ = writeln!(
                s,
                "  {:<16} = {:<6} ({} , ledger line {})",
                record.block, record.value, record.field, record.seal
            );
        }
    }
    let resting: Vec<String> = dictionary
        .blocks
        .iter()
        .filter(|block| !force.iter().any(|record| record.block == block.id))
        .map(|block| format!("{} = {}", block.id, block.default))
        .collect();
    if !resting.is_empty() {
        let _ = writeln!(s, "at their shipped default: {}", resting.join(", "));
    }
    s
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
    /// The undo, when one has been filed.
    pub undo: Option<VibeUndo>,
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
                undo: None,
            }),
            Entry::VibeOutcome(outcome) => {
                if let Some(record) = out.iter_mut().find(|record| record.seal == outcome.seal) {
                    record.outcome = Some(outcome.clone());
                }
            }
            Entry::VibeUndo(undo) => {
                if let Some(record) = out.iter_mut().find(|record| record.seal == undo.seal) {
                    record.undo = Some(undo.clone());
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
        if let Some(landed) = &self.landed {
            match &landed.commit {
                Some(commit) => {
                    let _ = writeln!(
                        s,
                        "  landed:   {} (commit {commit})",
                        landed.state_path.display()
                    );
                }
                None => {
                    let _ = writeln!(s, "  landed:   {}", landed.state_path.display());
                }
            }
            let _ = writeln!(
                s,
                "  next:     {} (seed {}, {} record(s) in force)",
                landed.header_path.display(),
                landed.header.seed,
                landed.in_force.len()
            );
        } else if self.outcome.adopted {
            let _ = writeln!(
                s,
                "  landed:   nothing — no harness was named, so the change waits in the ledger"
            );
        }
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
        if let Some(undo) = &record.undo {
            let _ = write!(s, ", undone (back to {} by {})", undo.value, undo.author);
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

    /// The shipped dictionary must load: these are the entries the slice
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

    /// The three Params-backed entries of this slice: their bounds (and
    /// where each bound comes from), their defaults (the values the sim
    /// ships), and that both extremes of every bound apply through the same
    /// path a round applies it — `set_branchable` + `validate`.
    #[test]
    fn the_three_params_backed_entries_carry_stated_bounds() {
        let dict = Dictionary::default_dictionary().unwrap();
        // (id, field, min, max, default, where the bounds come from)
        let entries = [
            (
                "unit.speed",
                "speed_upt",
                0,
                2048,
                136,
                "Params::validate: at most one cell per tick (cell_m 2 x 1024)",
            ),
            (
                "unit.tag_range",
                "tag_range_units",
                1024,
                8192,
                2048,
                "the sandbox's own bounds; validate states none",
            ),
            (
                "unit.stun_time",
                "stun_ticks",
                0,
                300,
                90,
                "the sandbox's own bounds; validate states none",
            ),
        ];
        for (id, field, min, max, default, _why) in entries {
            let block = dict.find(id).unwrap_or_else(|| panic!("{id} is missing"));
            assert_eq!(block.field, format!("params.{field}"), "{id}");
            assert_eq!(
                (block.min, block.max, block.default),
                (min, max, default),
                "{id}"
            );
            assert_eq!(
                block.default,
                params_current(field).unwrap(),
                "{id}'s default must be the value the sim ships"
            );
            assert!(block.age_ok, "{id}");

            // Both extremes apply and validate: a bound that the round
            // itself would refuse is a lie in the file.
            for value in [min, max] {
                let mut params = Params::default();
                assert!(
                    params.set_branchable(field, value).unwrap_or(false),
                    "{id}: {value} does not apply"
                );
                assert!(params.validate().is_ok(), "{id}: {value} does not validate");
            }
        }
    }

    /// Each of the new entries refuses a value outside its bounds, by
    /// name, and writes nothing.
    #[test]
    fn the_three_entries_refuse_out_of_bounds() {
        let dict = Dictionary::default_dictionary().unwrap();
        let cases = [
            ("unit.speed", -1),
            ("unit.speed", 2049),
            ("unit.tag_range", 1023),
            ("unit.tag_range", 8193),
            ("unit.stun_time", -1),
            ("unit.stun_time", 301),
        ];
        for (entry, value) in cases {
            let path = tmp("bounds-three");
            let err = propose(
                &dict,
                &path,
                &Ask::new(entry, value).with_seeds(1).with_ticks(30),
            )
            .unwrap_err();
            let VibeError::OutOfBounds {
                block,
                value: refused,
                ..
            } = &err
            else {
                panic!("the wrong refusal for {entry}={value}: {err}");
            };
            assert_eq!(block, entry);
            assert_eq!(*refused, value, "{entry}={value} was clamped or renamed");
            assert!(err.to_string().contains("refused"));
            assert!(
                !path.exists(),
                "a refusal wrote a record for {entry}={value}"
            );
        }
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
            &Ask::new("unit.mood", 200).with_seeds(1).with_ticks(30),
        )
        .unwrap_err();
        let VibeError::UnknownEntry { asked, known } = &err else {
            panic!("the wrong refusal: {err}");
        };
        assert_eq!(asked, "unit.mood");
        assert_eq!(
            known,
            &vec![
                "unit.vision".to_owned(),
                "unit.speed".to_owned(),
                "unit.tag_range".to_owned(),
                "unit.stun_time".to_owned(),
            ]
        );
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

    // The landing: an adopted value joins the harness the next round is
    // played with, the next round's header names the records, and the undo
    // puts it back. These tests build adopted outcomes by hand — the
    // landing mechanics are what is under test, not the sweep, and a real
    // adoption costs a full D78 sweep.

    /// A fresh harness root, the way `lm learn` likes it: one agent's
    /// harness with the shipped reflex in it.
    fn harness(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "lm-vibe-harness-{tag}-{}-{}",
            std::process::id(),
            line!()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("a0").join("reflexes")).unwrap();
        fs::write(
            root.join("a0").join("reflexes").join("default.rhai"),
            DEFAULT_REFLEX,
        )
        .unwrap();
        root
    }

    /// Seal one proposal for `value` and adopt it, without running a
    /// sweep. Returns its seal.
    fn adopt(path: &Path, value: i64) -> u64 {
        let mut ledger = Ledger::open(path).unwrap();
        let (seal, seal_hash) = ledger
            .seal_vibe(VibeProposal {
                block: "unit.vision".into(),
                field: "params.view_range_cells".into(),
                from: 20,
                value,
                label: "let them see farther".into(),
                author: "bench".into(),
                claims: vec![VibeClaim {
                    metric: "sight_ticks".into(),
                    direction: Direction::Up,
                    min_delta: 1,
                }],
                seeds: vec![2_000_001],
                ticks: 1_500,
            })
            .unwrap();
        ledger
            .vibe_outcome(VibeOutcome {
                seal,
                seal_hash,
                measures: Vec::new(),
                adopted: true,
            })
            .unwrap();
        seal
    }

    /// An adopted change lands: the state file carries it, the next round's
    /// header plays with it and names the record, the round replays, and a
    /// header with no records in force hashes exactly as the shipped round
    /// always did.
    #[test]
    fn an_adopted_change_lands_in_the_next_round_s_header_and_replays() {
        let dict = Dictionary::default_dictionary().unwrap();
        let ledger_path = tmp("land");
        let harness_root = harness("land");
        adopt(&ledger_path, 32);

        let landed = land(&dict, &ledger_path, &LandSite::new(&harness_root, 41)).unwrap();

        // The state file: the projection, on disk, in the harness.
        let text = fs::read_to_string(harness_root.join(STATE_FILE)).unwrap();
        assert!(text.contains("unit.vision"));
        assert!(text.contains("value = 32"));
        assert!(text.contains("seal = 0"));

        // The next round's header: the value applied, the record named.
        assert_eq!(landed.header.seed, 41);
        assert_eq!(landed.header.params.view_range_cells, 32);
        assert_eq!(
            landed.header.vibe,
            vec![VibeCarried {
                seal: 0,
                block: "unit.vision".to_owned(),
                value: 32,
            }]
        );
        assert!(landed.header.params.validate().is_ok());
        assert!(landed.header_path.exists());
        // The harness is pinned the same way `lm learn` pins it.
        assert_ne!(landed.header.roster[0].harness_commit, "none");

        // The round plays, its log verifies, and it is a different round
        // from the shipped one (the change did something).
        let log = play(landed.header.clone(), 240);
        assert!(log.verify().is_ok());
        let plain_log = play(Header::default_round(41, DEFAULT_REFLEX), 240);
        assert_ne!(
            log.hashes().last().map(|(_, h)| *h),
            plain_log.hashes().last().map(|(_, h)| *h)
        );

        // The replay reproduces it: the metrics re-run the round from the
        // log's own header and recorded decisions, and the same header
        // played twice is the same round twice.
        let metrics = RoundMetrics::from_log(&log);
        assert_eq!(metrics.ticks, 240);
        let again = play(landed.header.clone(), 240);
        assert_eq!(log.hashes(), again.hashes());

        // The one promise that keeps the golden honest: with the record
        // taken out, the round plays exactly the round the shipped params
        // always played — the state hashes agree tick for tick (the header
        // hash itself moves with the protocol version; the state never
        // does).
        let mut undone = landed.header.clone();
        undone.vibe.clear();
        undone
            .params
            .set_branchable("view_range_cells", 20)
            .unwrap();
        assert_eq!(play(undone, 240).hashes(), plain_log.hashes());
    }

    /// The undo restores the previous value — an earlier record's when
    /// there is one, the shipped default when there is not — and the
    /// re-landed header is the shipped round again.
    #[test]
    fn undo_restores_the_previous_value() {
        let dict = Dictionary::default_dictionary().unwrap();
        let ledger_path = tmp("undo");
        let harness_root = harness("undo");
        let first = adopt(&ledger_path, 32);
        let second = adopt(&ledger_path, 48);

        // Two adopted records: the later one is in force.
        let landed = land(&dict, &ledger_path, &LandSite::new(&harness_root, 7)).unwrap();
        assert_eq!(landed.header.params.view_range_cells, 48);
        assert_eq!(landed.header.vibe.len(), 1);
        assert_eq!(landed.header.vibe[0].seal, second);
        assert_eq!(landed.in_force[0].value, 48);

        // Undoing the buried one is a named refusal.
        let err = undo(&dict, &ledger_path, first, "bench", None).unwrap_err();
        assert!(matches!(err, VibeError::NotInForce { .. }));

        // Undo the one in force: back to the first record's value.
        let done = undo(
            &dict,
            &ledger_path,
            second,
            "bench",
            Some(&LandSite::new(&harness_root, 7)),
        )
        .unwrap();
        assert_eq!(done.undo.from, 48);
        assert_eq!(done.undo.value, 32, "the first record is in force again");
        let landed = done.landed.unwrap();
        assert_eq!(landed.header.params.view_range_cells, 32);
        assert_eq!(landed.header.vibe[0].seal, first);

        // Undo that too: back to the shipped default, and the header is
        // exactly the round the flow would pin — no records, no values.
        let done = undo(
            &dict,
            &ledger_path,
            first,
            "bench",
            Some(&LandSite::new(&harness_root, 7)),
        )
        .unwrap();
        assert_eq!(done.undo.value, 20);
        let landed = done.landed.unwrap();
        assert_eq!(landed.header.params.view_range_cells, 20);
        assert!(landed.header.vibe.is_empty());
        let harnesses = crate::flow::read_harness_set(&harness_root).unwrap();
        assert_eq!(
            landed.header,
            crate::flow::round_header(7, &harnesses),
            "with nothing in force the landing pins exactly the round the flow pins"
        );

        // A second undo of the same record is a named refusal, and so is
        // an undo of a seal that never was.
        assert!(matches!(
            undo(&dict, &ledger_path, first, "bench", None),
            Err(VibeError::Ledger(LedgerError::AlreadyUndone(_)))
        ));
        assert!(matches!(
            undo(&dict, &ledger_path, 99, "bench", None),
            Err(VibeError::Ledger(LedgerError::NoSuchSeal(99)))
        ));

        // The state says so, and the summary reads from the ledger.
        let ledger = Ledger::open(&ledger_path).unwrap();
        assert!(in_force(&ledger).is_empty());
        let state = state_summary(&ledger, &dict);
        assert!(state.contains("nothing in force"));
        assert!(state.contains("unit.vision = 20"));
        // And the ledger's own summary names the undos.
        let summary = ledger_summary(&ledger);
        assert!(summary.contains("undone (back to 32"));
        assert!(summary.contains("undone (back to 20"));
    }

    /// A refused proposal leaves the state unchanged: nothing is written,
    /// anywhere.
    #[test]
    fn a_refused_proposal_leaves_the_state_unchanged() {
        let dict = Dictionary::default_dictionary().unwrap();
        let ledger_path = tmp("refused");
        let harness_root = harness("refused");
        let site = LandSite::new(&harness_root, 3);

        // Out of bounds: a named refusal, and not one file moved.
        let err = propose(
            &dict,
            &ledger_path,
            &Ask::new("unit.vision", 65)
                .with_prediction("sight_ticks:up")
                .with_seeds(1)
                .with_ticks(30)
                .with_land(site.clone()),
        )
        .unwrap_err();
        assert!(matches!(err, VibeError::OutOfBounds { .. }));
        assert!(!ledger_path.exists());
        assert!(!harness_root.join(STATE_FILE).exists());

        // And a proposal that does not clear its test lands nothing new:
        // the state stays empty, the header stays the round the flow pins.
        // (One seed at 30 ticks can never meet D78; every metric is zero
        // there.)
        let proposed = propose(
            &dict,
            &ledger_path,
            &Ask::new("unit.vision", 32)
                .with_prediction("sight_ticks:up")
                .with_seeds(1)
                .with_ticks(30)
                .with_land(site.clone()),
        )
        .unwrap();
        assert!(!proposed.outcome.adopted);
        let landed = proposed.landed.expect("the harness was named");
        assert!(landed.in_force.is_empty());
        assert_eq!(landed.header.params.view_range_cells, 20);
        assert!(landed.header.vibe.is_empty());
        let harnesses = crate::flow::read_harness_set(&harness_root).unwrap();
        assert_eq!(landed.header, crate::flow::round_header(3, &harnesses));
    }

    /// The landing commits with `lm learn`'s discipline: the same state in
    /// two harness repos produces the same commit, because the date comes
    /// from the landing's id, not the clock.
    #[test]
    fn the_landing_commits_the_same_state_to_the_same_commit() {
        let dict = Dictionary::default_dictionary().unwrap();
        let ledger_path = tmp("commit");
        adopt(&ledger_path, 32);

        let mut commits = Vec::new();
        for tag in ["git-a", "git-b"] {
            let root = harness(tag);
            let out = std::process::Command::new("git")
                .args(["init", "-q"])
                .arg(&root)
                .output()
                .unwrap();
            assert!(out.status.success());
            let landed = land(&dict, &ledger_path, &LandSite::new(&root, 5)).unwrap();
            commits.push(landed.commit.expect("the harness root is a git repo"));
        }
        assert_eq!(
            commits[0], commits[1],
            "the same state must produce the same commit, whatever the clock says"
        );

        // A harness that is not a repo of its own commits nothing, and
        // nothing is pretended.
        let plain = harness("git-none");
        let landed = land(&dict, &ledger_path, &LandSite::new(&plain, 5)).unwrap();
        assert_eq!(landed.commit, None);
    }

    /// A state file the ledger cannot explain is refused, not healed.
    #[test]
    fn a_doctored_state_file_is_refused() {
        let dict = Dictionary::default_dictionary().unwrap();
        let ledger_path = tmp("doctor");
        let harness_root = harness("doctor");
        adopt(&ledger_path, 32);
        land(&dict, &ledger_path, &LandSite::new(&harness_root, 5)).unwrap();

        // A value no ledger line adopted.
        let doctored = harness_root.join(STATE_FILE);
        fs::write(
            &doctored,
            fs::read_to_string(&doctored)
                .unwrap()
                .replace("value = 32", "value = 64"),
        )
        .unwrap();
        let err = land(&dict, &ledger_path, &LandSite::new(&harness_root, 5)).unwrap_err();
        assert!(matches!(err, VibeError::StateDrift { .. }));
        assert!(err.to_string().contains("refused"));

        // And a file that does not parse at all.
        fs::write(&doctored, "not a toml file at all {{{{").unwrap();
        assert!(matches!(
            land(&dict, &ledger_path, &LandSite::new(&harness_root, 5)),
            Err(VibeError::StateDrift { .. })
        ));
    }

    /// `lm vibe state`'s summary names the in-force line and the ledger
    /// line it came from, and the rest rest at their defaults.
    #[test]
    fn the_state_summary_names_the_ledger_lines() {
        let dict = Dictionary::default_dictionary().unwrap();
        let ledger_path = tmp("state");
        let seal = adopt(&ledger_path, 32);
        let ledger = Ledger::open(&ledger_path).unwrap();
        let text = state_summary(&ledger, &dict);
        assert!(text.contains("in force:"));
        assert!(text.contains("unit.vision"));
        assert!(text.contains("= 32"));
        assert!(text.contains(&format!("ledger line {seal}")));
        assert!(text.contains("unit.speed = 136"));
        assert!(text.contains("unit.tag_range = 2048"));
        assert!(text.contains("unit.stun_time = 90"));
    }
}
