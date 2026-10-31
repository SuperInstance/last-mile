//! The prediction ledger: honest learning needs predictions written down
//! *before* the test (D32; phase-5 Task 3).
//!
//! Every between-round proposal (a new gate, a reflex edit, a lexicon
//! entry…) must say what it expects to change, by how much, and on which
//! held-out seeds, and that claim is **sealed** here before any evaluation
//! runs. The evaluation's verdict is appended afterwards to the same chain.
//! Because the chain is hash-linked, nobody (a model or a human) can quietly
//! edit a prediction to match the result. This is the dungeon-ml-zoo
//! "sealed predictions" ritual from the fleet research, as a library.
//!
//! The file is JSON lines. Each line is
//! `{"seq":N,"prev":"<16 hex>","hash":"<16 hex>","entry":{...}}`, where
//! `hash = FNV-1a 64(prev bytes LE ++ canonical JSON of entry)`, the first
//! line's `prev` is 0, and the canonical JSON is `serde_json`'s output for
//! the entry types below (fields in declaration order, no maps with
//! unordered keys).

use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use lm_core::Fnv64;
use serde::{Deserialize, Serialize};

/// Which way a metric is predicted to move.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    /// The metric should rise.
    Up,
    /// The metric should fall.
    Down,
}

/// A claim about a proposal's effect, sealed before it's tested.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prediction {
    /// The round the lesson came from.
    pub round_id: String,
    /// Hash of the proposal text being judged, so the verdict is tied to it.
    pub proposal_hash: String,
    /// Which metric (a `RoundMetrics` field name).
    pub metric: String,
    /// Predicted direction.
    pub direction: Direction,
    /// Smallest change that counts, in the metric's integer unit (x1000 for rates).
    pub min_delta: i64,
    /// The held-out seeds it will be judged on (never the seeds it was learned from).
    pub eval_seeds: Vec<u64>,
    /// Who made the prediction (e.g. `"router:z.ai/glm"`, `"human"`).
    pub author: String,
    /// The prediction in words, as the author wrote it.
    pub text: String,
}

/// The measured outcome of a sealed prediction.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verdict {
    /// The seal this answers.
    pub seal: u64,
    /// Per-seed change in the predicted metric (proposal minus baseline).
    pub deltas: Vec<(u64, i64)>,
    /// Whether the proposal was adopted.
    pub adopted: bool,
    /// Set when a human adopted against the evidence, with their reason
    /// (Review Focus 2: allowed, but logged).
    pub override_reason: Option<String>,
}

/// One ledger line's payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Entry {
    /// A prediction, sealed.
    Seal(Prediction),
    /// The verdict on an earlier seal.
    Verdict(Verdict),
    /// A human's override of a verdict: they decided against the evidence,
    /// and said why (Review Focus 2: allowed, but logged).
    Override {
        /// The seal the override answers.
        seal: u64,
        /// Whether the human adopted the proposal anyway.
        adopted: bool,
        /// The human's reason.
        reason: String,
    },
    /// A Vibe proposal: one typed change out of the closed dictionary, its
    /// prediction sealed before the test (the sandbox, M2b).
    VibeProposal(VibeProposal),
    /// The measured outcome of an earlier Vibe proposal.
    VibeOutcome(VibeOutcome),
}

/// One measure a Vibe prediction claims will move, sealed before the test.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VibeClaim {
    /// The metric, a `RoundMetrics` measure name (`sight_ticks`, `tags`, …).
    pub metric: String,
    /// Predicted direction.
    pub direction: Direction,
    /// Smallest change that counts, in the metric's integer unit.
    pub min_delta: i64,
}

/// A typed change out of the Vibe dictionary, sealed before it is tested.
///
/// The prediction is part of the seal: once this line is in the chain, the
/// claims it makes cannot be edited to match the result any more than a
/// [`Prediction`] can. The only words it holds are a short label — the
/// kid's phrasing of the change, bounded — never a transcript, never audio
/// (the sandbox privacy rules, §7).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VibeProposal {
    /// The dictionary entry (`unit.vision`): what Bench names out loud.
    pub block: String,
    /// The one field the entry writes (`params.view_range_cells`).
    pub field: String,
    /// The value now: the baseline the test runs against.
    pub from: i64,
    /// The value proposed.
    pub value: i64,
    /// The kid's words as a short label, bounded at seal time; never a
    /// transcript.
    pub label: String,
    /// Who proposed (`bench`; later `kid`, `bolt`).
    pub author: String,
    /// The prediction: what the change should move, one claim per measure.
    pub claims: Vec<VibeClaim>,
    /// The held-out seeds the prediction is judged on.
    pub seeds: Vec<u64>,
    /// The horizon every seed is played for.
    pub ticks: u64,
}

/// One measure's measured outcome.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeasureOutcome {
    /// The claim this answers.
    pub claim: VibeClaim,
    /// Per-seed change (candidate minus baseline).
    pub deltas: Vec<(u64, i64)>,
    /// How many seeds met the sealed bar.
    pub qualified: u64,
    /// How many seeds moved at all.
    pub moved: u64,
    /// Of the seeds that moved, how many moved the claimed way.
    pub claimed: u64,
    /// The sign test's p-value, rendered; the decision itself is integer.
    pub p: String,
    /// Did this measure's prediction hold?
    pub met: bool,
}

/// The outcome of a Vibe proposal, appended after its test.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VibeOutcome {
    /// The seal (line number) of the proposal this answers.
    pub seal: u64,
    /// The chain hash of the sealed proposal's line. The outcome names the
    /// exact bytes it was said from, so a prediction edited after the test
    /// fails this check even before the chain itself breaks.
    pub seal_hash: String,
    /// Per-measure outcomes, in the prediction's order.
    pub measures: Vec<MeasureOutcome>,
    /// Whether the change is adopted: every measure held.
    pub adopted: bool,
}

/// One line on disk.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Line {
    seq: u64,
    prev: String,
    hash: String,
    entry: Entry,
}

/// What can go wrong.
#[derive(Debug)]
pub enum LedgerError {
    /// Reading or writing the file failed.
    Io(io::Error),
    /// A line isn't valid JSON for a ledger line.
    Decode { line: u64, why: String },
    /// A line's hash or sequence doesn't follow from the one before: the
    /// file was edited, reordered or truncated mid-line.
    ChainBroken { line: u64 },
    /// A verdict names a seal that doesn't exist.
    NoSuchSeal(u64),
    /// A seal already has its verdict; a second one would rewrite history.
    AlreadyJudged(u64),
    /// An outcome names a seal hash that is not the hash of the proposal it
    /// claims to answer: the prediction changed after the test.
    SealMismatch(u64),
}

impl std::fmt::Display for LedgerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LedgerError::Io(err) => write!(f, "the ledger file: {err}"),
            LedgerError::Decode { line, why } => {
                write!(f, "line {line} is not a ledger line: {why}")
            }
            LedgerError::ChainBroken { line } => {
                write!(
                    f,
                    "line {line} does not follow the one before it: the file was edited"
                )
            }
            LedgerError::NoSuchSeal(seal) => write!(f, "there is no seal {seal} in this ledger"),
            LedgerError::AlreadyJudged(seal) => {
                write!(f, "seal {seal} already has its verdict")
            }
            LedgerError::SealMismatch(seal) => write!(
                f,
                "the outcome does not answer the bytes that were sealed at {seal}: \
                 the prediction changed after the test"
            ),
        }
    }
}

impl std::error::Error for LedgerError {}

impl From<io::Error> for LedgerError {
    fn from(e: io::Error) -> Self {
        LedgerError::Io(e)
    }
}

/// The ledger, backed by an append-only file.
#[derive(Debug)]
pub struct Ledger {
    path: PathBuf,
    lines: Vec<Line>,
}

fn hex(v: u64) -> String {
    format!("{v:016x}")
}

/// The chain hash of an entry given the previous hash.
fn chain_hash(prev: u64, entry: &Entry) -> Result<u64, LedgerError> {
    let body = serde_json::to_vec(entry).map_err(|e| LedgerError::Decode {
        line: 0,
        why: e.to_string(),
    })?;
    let mut h = Fnv64::new();
    h.u64(prev);
    h.write(&body);
    Ok(h.finish())
}

impl Ledger {
    /// Open (or create) a ledger file and verify its whole chain.
    pub fn open(path: &Path) -> Result<Ledger, LedgerError> {
        let mut lines = Vec::new();
        if path.exists() {
            let f = fs::File::open(path)?;
            let mut prev = 0u64;
            for (i, raw) in io::BufReader::new(f).lines().enumerate() {
                let n = i as u64;
                let raw = raw?;
                if raw.trim().is_empty() {
                    continue;
                }
                let line: Line = serde_json::from_str(&raw).map_err(|e| LedgerError::Decode {
                    line: n,
                    why: e.to_string(),
                })?;
                let expect = chain_hash(prev, &line.entry)?;
                if line.seq != n || line.prev != hex(prev) || line.hash != hex(expect) {
                    return Err(LedgerError::ChainBroken { line: n });
                }
                prev = expect;
                lines.push(line);
            }
        }
        Ok(Ledger {
            path: path.to_path_buf(),
            lines,
        })
    }

    fn head(&self) -> u64 {
        self.lines
            .last()
            .and_then(|l| u64::from_str_radix(&l.hash, 16).ok())
            .unwrap_or(0)
    }

    fn append(&mut self, entry: Entry) -> Result<u64, LedgerError> {
        let prev = self.head();
        let seq = self.lines.len() as u64;
        let line = Line {
            seq,
            prev: hex(prev),
            hash: hex(chain_hash(prev, &entry)?),
            entry,
        };
        let mut text = serde_json::to_string(&line).map_err(|e| LedgerError::Decode {
            line: seq,
            why: e.to_string(),
        })?;
        text.push('\n');
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        f.write_all(text.as_bytes())?;
        f.sync_data()?;
        self.lines.push(line);
        Ok(seq)
    }

    /// Seal a prediction. Returns its seal id (its line number).
    pub fn seal(&mut self, p: Prediction) -> Result<u64, LedgerError> {
        self.append(Entry::Seal(p))
    }

    /// Record the verdict for a seal. A seal can be judged exactly once.
    pub fn verdict(&mut self, v: Verdict) -> Result<u64, LedgerError> {
        match self.lines.get(v.seal as usize).map(|l| &l.entry) {
            Some(Entry::Seal(_)) => {}
            _ => return Err(LedgerError::NoSuchSeal(v.seal)),
        }
        let judged = self
            .lines
            .iter()
            .any(|l| matches!(&l.entry, Entry::Verdict(old) if old.seal == v.seal));
        if judged {
            return Err(LedgerError::AlreadyJudged(v.seal));
        }
        self.append(Entry::Verdict(v))
    }

    /// The sealed prediction for `seal`, if any.
    pub fn prediction(&self, seal: u64) -> Option<&Prediction> {
        match self.lines.get(seal as usize).map(|l| &l.entry) {
            Some(Entry::Seal(p)) => Some(p),
            _ => None,
        }
    }

    /// Record a human's override of a verdict.
    ///
    /// The verdict stays what the measurement said; the override is a line of
    /// its own, so a reader of the chain sees both the evidence and the
    /// decision that went against it. A seal can be overridden as often as a
    /// human wants to say so out loud.
    pub fn override_verdict(
        &mut self,
        seal: u64,
        adopted: bool,
        reason: &str,
    ) -> Result<u64, LedgerError> {
        match self.lines.get(seal as usize).map(|l| &l.entry) {
            Some(Entry::Seal(_)) => {}
            _ => return Err(LedgerError::NoSuchSeal(seal)),
        }
        self.append(Entry::Override {
            seal,
            adopted,
            reason: reason.to_string(),
        })
    }

    /// Every entry, in order.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.lines.iter().map(|l| &l.entry)
    }

    /// Seal a Vibe proposal: append it, and hand back its seal (line
    /// number) with the chain hash of its line.
    ///
    /// The hash is what the outcome later has to name, so a prediction
    /// edited between the seal and the test fails the outcome's check even
    /// if the editor also fixed up the chain.
    pub fn seal_vibe(&mut self, p: VibeProposal) -> Result<(u64, String), LedgerError> {
        let seal = self.append(Entry::VibeProposal(p))?;
        let hash = self.lines[seal as usize].hash.clone();
        Ok((seal, hash))
    }

    /// Record the outcome of a Vibe proposal. A proposal is judged exactly
    /// once, and its outcome has to name the bytes that were sealed.
    pub fn vibe_outcome(&mut self, o: VibeOutcome) -> Result<u64, LedgerError> {
        let sealed = match self.lines.get(o.seal as usize).map(|l| (&l.entry, &l.hash)) {
            Some((Entry::VibeProposal(_), hash)) => hash,
            _ => return Err(LedgerError::NoSuchSeal(o.seal)),
        };
        let judged = self
            .lines
            .iter()
            .any(|l| matches!(&l.entry, Entry::VibeOutcome(old) if old.seal == o.seal));
        if judged {
            return Err(LedgerError::AlreadyJudged(o.seal));
        }
        if *sealed != o.seal_hash {
            return Err(LedgerError::SealMismatch(o.seal));
        }
        self.append(Entry::VibeOutcome(o))
    }

    /// The sealed Vibe proposal of a seal, if any.
    pub fn vibe_proposal(&self, seal: u64) -> Option<&VibeProposal> {
        match self.lines.get(seal as usize).map(|l| &l.entry) {
            Some(Entry::VibeProposal(p)) => Some(p),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("lm-ledger-{tag}-{}.jsonl", std::process::id()));
        let _ = fs::remove_file(&p);
        p
    }

    fn pred() -> Prediction {
        Prediction {
            round_id: "r7".into(),
            proposal_hash: "00ff00ff00ff00ff".into(),
            metric: "beacon_hold_ticks_squad0".into(),
            direction: Direction::Up,
            min_delta: 50,
            eval_seeds: vec![1001, 1002, 1003, 1004],
            author: "router:test".into(),
            text: "Holding with two agents should add 50+ hold ticks".into(),
        }
    }

    #[test]
    fn sealed_before_eval_then_judged() {
        let path = tmp("ok");
        let mut l = Ledger::open(&path).unwrap();
        let seal = l.seal(pred()).unwrap();
        l.verdict(Verdict {
            seal,
            deltas: vec![(1001, 80)],
            adopted: true,
            override_reason: None,
        })
        .unwrap();
        let reopened = Ledger::open(&path).unwrap();
        assert_eq!(reopened.entries().count(), 2);
        assert_eq!(reopened.prediction(seal), Some(&pred()));
    }

    #[test]
    fn verdict_requires_seal() {
        let mut l = Ledger::open(&tmp("noseal")).unwrap();
        let err = l
            .verdict(Verdict {
                seal: 0,
                deltas: vec![],
                adopted: false,
                override_reason: None,
            })
            .unwrap_err();
        assert!(matches!(err, LedgerError::NoSuchSeal(0)));
    }

    #[test]
    fn no_double_verdict() {
        let mut l = Ledger::open(&tmp("double")).unwrap();
        let seal = l.seal(pred()).unwrap();
        let v = Verdict {
            seal,
            deltas: vec![],
            adopted: false,
            override_reason: None,
        };
        l.verdict(v.clone()).unwrap();
        assert!(matches!(l.verdict(v), Err(LedgerError::AlreadyJudged(_))));
    }

    #[test]
    fn tamper_detected() {
        let path = tmp("tamper");
        let mut l = Ledger::open(&path).unwrap();
        l.seal(pred()).unwrap();
        l.seal(pred()).unwrap();
        // Quietly "improve" the first prediction after the fact.
        let text =
            fs::read_to_string(&path)
                .unwrap()
                .replacen("\"min_delta\":50", "\"min_delta\":5", 1);
        fs::write(&path, text).unwrap();
        assert!(matches!(
            Ledger::open(&path),
            Err(LedgerError::ChainBroken { line: 0 })
        ));
    }

    #[test]
    fn reordered_lines_detected() {
        let path = tmp("reorder");
        let mut l = Ledger::open(&path).unwrap();
        l.seal(pred()).unwrap();
        let mut p2 = pred();
        p2.metric = "tags_for_squad0".into();
        l.seal(p2).unwrap();
        let mut lines: Vec<String> = fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(String::from)
            .collect();
        lines.swap(0, 1);
        fs::write(&path, lines.join("\n") + "\n").unwrap();
        assert!(matches!(
            Ledger::open(&path),
            Err(LedgerError::ChainBroken { .. })
        ));
    }

    #[test]
    fn human_override_is_recorded() {
        let mut l = Ledger::open(&tmp("override")).unwrap();
        let seal = l.seal(pred()).unwrap();
        l.verdict(Verdict {
            seal,
            deltas: vec![(1001, -3)],
            adopted: true,
            override_reason: Some("I like how it plays".into()),
        })
        .unwrap();
        let last = l.entries().last().unwrap();
        assert!(matches!(last, Entry::Verdict(v) if v.override_reason.is_some()));
    }

    fn vibe_proposal() -> VibeProposal {
        VibeProposal {
            block: "unit.vision".into(),
            field: "params.view_range_cells".into(),
            from: 20,
            value: 32,
            label: "let them see farther".into(),
            author: "bench".into(),
            claims: vec![VibeClaim {
                metric: "sight_ticks".into(),
                direction: Direction::Up,
                min_delta: 1,
            }],
            seeds: vec![2_000_001, 2_000_002],
            ticks: 1_500,
        }
    }

    #[test]
    fn a_vibe_proposal_is_judged_once_and_by_its_own_bytes() {
        let mut l = Ledger::open(&tmp("vibe")).unwrap();
        let (seal, hash) = l.seal_vibe(vibe_proposal()).unwrap();
        l.vibe_outcome(VibeOutcome {
            seal,
            seal_hash: hash.clone(),
            measures: vec![MeasureOutcome {
                claim: VibeClaim {
                    metric: "sight_ticks".into(),
                    direction: Direction::Up,
                    min_delta: 1,
                },
                deltas: vec![(2_000_001, 4), (2_000_002, 2)],
                qualified: 2,
                moved: 2,
                claimed: 2,
                p: "0.2500".into(),
                met: false,
            }],
            adopted: false,
        })
        .unwrap();

        // Judged exactly once.
        let again = VibeOutcome {
            seal,
            seal_hash: hash.clone(),
            measures: Vec::new(),
            adopted: false,
        };
        assert!(matches!(
            l.vibe_outcome(again),
            Err(LedgerError::AlreadyJudged(_))
        ));

        // An outcome that does not name the sealed bytes — a prediction
        // changed after the test — is refused by the seal check.
        let mut other = vibe_proposal();
        other.value = 48;
        let (other_seal, _) = l.seal_vibe(other).unwrap();
        let mismatched = VibeOutcome {
            seal: other_seal,
            seal_hash: hash,
            measures: Vec::new(),
            adopted: false,
        };
        assert!(matches!(
            l.vibe_outcome(mismatched),
            Err(LedgerError::SealMismatch(_))
        ));

        // And a proposal line edited after the fact breaks the chain, the
        // same way any other edit does.
        let path = tmp("vibe-tamper");
        let mut l = Ledger::open(&path).unwrap();
        l.seal_vibe(vibe_proposal()).unwrap();
        let text =
            fs::read_to_string(&path)
                .unwrap()
                .replacen("\"min_delta\":1", "\"min_delta\":9", 1);
        fs::write(&path, text).unwrap();
        assert!(matches!(
            Ledger::open(&path),
            Err(LedgerError::ChainBroken { line: 0 })
        ));
    }
}
