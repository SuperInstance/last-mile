//! The between-round screen: what the flow did, and what a human may do about it.
//!
//! The flow (`lm-learn`) plays a round, asks the reviewer what to change, seals
//! the prediction of what that change will do, tests it on held-out seeds and
//! writes the whole chain down. This module is the screen that shows that
//! chain and takes a human's answer back:
//!
//! ```text
//! metrics   what the round measured, and the change against the round before
//! proposal  what the reviewer proposed, in words and in a diff
//! prediction what it promised, and the seal it was written under
//! verdict   what the held-out evaluation measured
//! actions   adopt, reject, override
//! ```
//!
//! The order matters: the prediction comes before the verdict, because the
//! prediction was sealed *before* the test (D32) and a reader has to see the
//! promise before the measurement that judges it.
//!
//! Two things a human can do are not a decision but a request, and they are
//! kept apart from the verdict on purpose:
//!
//! * an **override** goes against the measured verdict. It is allowed, and it
//!   is logged: the evidence stays where it was and the record says the human
//!   decided otherwise, and why (D32, Review Focus 2);
//! * a **request** (the request box) is a change the human asks for. It
//!   becomes a deposit (D33) -- the words kept verbatim -- and the deposit is
//!   what the reviewer is asked about next.
//!
//! The model itself is plain logic: it holds what the screen shows and what
//! the buttons would call. The two calls that touch the disk are `lm_learn`'s
//! own (`flow::write_override` and the deposit file), and they are named as
//! such.

use std::fmt::Write;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use lm_agents::surprise::SituationCounts;
use lm_learn::deposits::{self, Deposit};
use lm_learn::eval::metric_value;
use lm_learn::flow::{Decision, FlowError, FlowRecord, HumanOverride};
use lm_learn::ledger::{Direction, Ledger};
use lm_learn::metrics::RoundMetrics;
use lm_learn::review::{Claim, Proposal, KNOWN_METRICS};

/// The reason a human gives when they go against the measurement and say
/// nothing else: the flow's own words for it.
pub const DEFAULT_REASON: &str = "a human overrode the flow's decision";

/// One number on the screen: what it is now, what it was, and the change.
///
/// The change is against the round before this one. A round that has no
/// predecessor -- the first one -- shows the number and no change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetricLine {
    /// The metric's name (`hold_ticks_squad0`).
    pub name: String,
    /// What it measured this round.
    pub value: Option<i64>,
    /// What it measured the round before.
    pub previous: Option<i64>,
    /// The change against the round before.
    pub delta: Option<i64>,
}

/// What the reviewer proposed, in words and in a diff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProposalView {
    /// One line: what the change is.
    pub summary: String,
    /// The exact change, as it was written.
    pub diff: String,
}

/// One situation the minds struggled in, as the screen shows it.
///
/// The line is the whole entry — `early round, level on score, an enemy in
/// sight, stunned, on your beacon: 9 of 14 asks unsure (attention: 9)` —
/// and the numbers sit beside it for whoever wants more than the line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurpriseLine {
    /// The one-line, human-readable description of the situation and its
    /// dominant failure.
    pub text: String,
    /// The surprise score: struggle events per ask, x1000.
    pub score_x1000: u64,
    /// The asks that came home in the bucket.
    pub asks: u64,
    /// The asks that came home with at least one unsure gate.
    pub unsure_asks: u64,
    /// The refused asks, every kind together.
    pub refused: u64,
    /// The gates asked of a second model (D71), every gate together.
    pub escalated: u64,
}

/// The prediction that was sealed before the change was tested.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PredictionView {
    /// The seal: the ledger line the prediction sits on.
    pub seal: u64,
    /// The metric the change was predicted to move.
    pub metric: String,
    /// Which way it was predicted to move.
    pub direction: Direction,
    /// The smallest move that counts, in the metric's own unit.
    pub min_delta: i64,
    /// The held-out seeds it was judged on: never the seeds it was learned
    /// from.
    pub eval_seeds: Vec<u64>,
    /// Who made the prediction.
    pub author: String,
    /// The prediction in the author's own words.
    pub text: String,
}

impl PredictionView {
    /// The held-out seeds, as one line of text.
    ///
    /// The flow judges a proposal on sixteen held-out seeds now, and
    /// sixteen numbers do not fit on a line as a list. A run of consecutive
    /// seeds is written as a range ("1001000..1001015") and anything else
    /// as the list it is. The count is always there: a reader has to know
    /// how many seeds the verdict was read from.
    pub fn seeds_text(&self) -> String {
        let count = self.eval_seeds.len();
        if count == 0 {
            return "none".to_owned();
        }
        let noun = if count == 1 { "seed" } else { "seeds" };
        let consecutive = self
            .eval_seeds
            .windows(2)
            .all(|pair| pair[1] == pair[0] + 1);
        if consecutive && count > 2 {
            return format!(
                "{count} {noun} ({}..{})",
                self.eval_seeds[0],
                self.eval_seeds[count - 1]
            );
        }
        let list = self
            .eval_seeds
            .iter()
            .map(|seed| seed.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        format!("{count} {noun} ({list})")
    }
}

/// The verdict, as it was measured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerdictView {
    /// The seal this answers.
    pub seal: u64,
    /// Whether the held-out evaluation adopted the change.
    pub adopted: bool,
    /// The change per held-out seed.
    pub deltas: Vec<(u64, i64)>,
    /// How many of the seeds moved the way the prediction said.
    pub moved: usize,
    /// How many seeds were judged.
    pub seeds: usize,
    /// The human's reason, when a human went against the measurement.
    pub override_reason: Option<String>,
}

/// What a human may do on the between-round screen.
///
/// The flow has already decided, and the screen shows that decision next to
/// the measurement that backed it. These three buttons are what a person can
/// do about it:
///
/// * **Adopt** -- take the change. When the flow adopted it too, this
///   confirms the flow's own decision and nothing is called; when the flow
///   refused it, the human is going against the evidence and the override is
///   written.
/// * **Reject** -- leave the change out. The mirror of Adopt.
/// * **Override** -- go against the measured verdict, in the direction the
///   flow did not go, and say why. It is the explicit form of the other two:
///   it always asks for a reason and it is always recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Take the change.
    Adopt,
    /// Leave the change out.
    Reject,
    /// Go against the measurement, and say why.
    Override,
}

impl Action {
    /// The button, as it is written on the screen.
    pub fn as_str(self) -> &'static str {
        match self {
            Action::Adopt => "Adopt",
            Action::Reject => "Reject",
            Action::Override => "Override",
        }
    }
}

/// The call into `lm_learn` an action makes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Call {
    /// The human confirmed the flow's own decision: nothing is called.
    Confirm,
    /// The human went against the evidence: `lm_learn::flow::override_verdict`,
    /// with the ledger write that makes it visible to a reader of the chain.
    Override {
        /// The decision the human made.
        decision: Decision,
        /// Why they made it.
        reason: String,
    },
}

/// A part of the between-round screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    /// What the round measured.
    Metrics,
    /// Where the minds struggled: the top surprising situations.
    Surprise,
    /// What the reviewer proposed.
    Proposal,
    /// What it predicted, and the seal it was written under.
    Prediction,
    /// What the held-out evaluation measured.
    Verdict,
    /// What a human may do about it.
    Actions,
}

/// The screen between two rounds.
///
/// Built from the flow's record: the record says what the round did, the
/// metrics say what it measured, and the ledger says what was promised and
/// what was measured.
#[derive(Clone, Debug)]
pub struct BetweenModel {
    /// The round the screen is about.
    pub round_id: String,
    /// The seed the round was played with.
    pub seed: u64,
    /// The metrics of this round, with the change against the round before.
    pub metrics: Vec<MetricLine>,
    /// The top situations the minds struggled in (the surprise signal):
    /// where they were unsure, refused or escalated, most surprising first.
    pub surprise: Vec<SurpriseLine>,
    /// The proposal, when the reviewer made one.
    pub proposal: Option<ProposalView>,
    /// The prediction that was sealed, when there was one.
    pub prediction: Option<PredictionView>,
    /// The verdict, as it was measured.
    pub verdict: Option<VerdictView>,
    /// The three buttons.
    pub actions: Vec<Action>,
    /// The decision the flow reached.
    pub decision: Decision,
    /// Why the proposal was refused, when it was.
    pub rejection: Option<String>,
    /// The commit the next round's header pins.
    pub next_header_pin: String,
    /// Where the next round's header is.
    pub next_header_path: PathBuf,
    /// The human overrides so far, in the order they happened.
    pub overrides: Vec<HumanOverride>,

    /// The record the screen is built from.
    record: FlowRecord,
    /// Where the deposits are written.
    deposits_dir: PathBuf,
    /// The number the last request got.
    requests: u32,
}

impl BetweenModel {
    /// The screen for one round.
    ///
    /// `current` is this round's metrics and `previous` the round before's,
    /// when the caller has them: the deltas are against the round before, and
    /// the first round has none.
    pub fn new(
        record: &FlowRecord,
        current: Option<&RoundMetrics>,
        previous: Option<&RoundMetrics>,
    ) -> BetweenModel {
        BetweenModel {
            round_id: record.round_id.clone(),
            seed: record.seed,
            metrics: metric_lines(current, previous),
            surprise: surprise_of(record),
            proposal: proposal_of(record),
            prediction: prediction_of(record),
            verdict: verdict_of(record),
            actions: vec![Action::Adopt, Action::Reject, Action::Override],
            decision: record.decision,
            rejection: record.rejection.clone(),
            next_header_pin: record.next_header_pin.clone(),
            next_header_path: record.next_header_path.clone(),
            overrides: record.overrides.clone(),
            record: record.clone(),
            deposits_dir: deposits_dir_of(record),
            requests: 0,
        }
    }

    /// The screen for one round, with this round's metrics measured from the
    /// log the record points at.
    pub fn measured(record: &FlowRecord, previous: Option<&RoundMetrics>) -> BetweenModel {
        let current = metrics_of(record);
        BetweenModel::new(record, current.as_ref(), previous)
    }

    /// The same screen, for a record that has changed under it.
    ///
    /// The round is the same round: only what was decided about it moved, so
    /// the metrics stay where they are.
    pub fn replaced(&self, record: &FlowRecord) -> BetweenModel {
        let mut next = BetweenModel::new(record, None, None);
        next.metrics = self.metrics.clone();
        next.requests = self.requests;
        next
    }

    /// The record the screen is built from.
    pub fn record(&self) -> &FlowRecord {
        &self.record
    }

    /// The parts of the screen, in the order they are shown.
    ///
    /// The prediction comes before the verdict: the promise was sealed
    /// before the test, so it is read first. The surprise comes right after
    /// the metrics: it is the round's own answer to "where did they
    /// struggle", and the proposal is read against it.
    pub fn sections(&self) -> Vec<Section> {
        let mut out = Vec::new();
        if !self.metrics.is_empty() {
            out.push(Section::Metrics);
        }
        if !self.surprise.is_empty() {
            out.push(Section::Surprise);
        }
        if self.proposal.is_some() {
            out.push(Section::Proposal);
        }
        if self.prediction.is_some() {
            out.push(Section::Prediction);
        }
        if self.verdict.is_some() {
            out.push(Section::Verdict);
        }
        out.push(Section::Actions);
        out
    }

    /// The decision an action makes.
    ///
    /// Adopt is Adopt and Reject is Reject. Override is the decision the
    /// flow did *not* make: the human goes against the measurement.
    pub fn decision_of(&self, action: Action) -> Decision {
        match action {
            Action::Adopt => Decision::Adopt,
            Action::Reject => Decision::Reject,
            Action::Override => self.other_decision(),
        }
    }

    /// What an action calls in `lm_learn`.
    ///
    /// Confirming the flow's own decision calls nothing: it is already what
    /// the record says. Going against it is an override, and the reason is
    /// the human's -- the flow's own words when they gave none.
    pub fn call(&self, action: Action, reason: &str) -> Call {
        let decision = self.decision_of(action);
        if decision == self.decision {
            return Call::Confirm;
        }
        Call::Override {
            decision,
            reason: reason_of(reason),
        }
    }

    /// Make the call: the record with the human's decision in it.
    ///
    /// A confirmation changes nothing. An override is written into the
    /// ledger and the record -- the evidence stays where it was, and the
    /// chain says the human decided otherwise, and why.
    pub fn apply(&self, action: Action, reason: &str) -> Result<FlowRecord, FlowError> {
        match self.call(action, reason) {
            Call::Confirm => Ok(self.record.clone()),
            Call::Override { decision, reason } => {
                lm_learn::flow::write_override(&self.record, decision, &reason)
            }
        }
    }

    /// The deposit a human's request becomes.
    ///
    /// The words are kept verbatim (D33): what the human said is what the
    /// reviewer is asked about. The diff is empty -- nothing has been
    /// changed yet -- and the deposit is not applied to any harness.
    pub fn request_deposit(&self, text: &str) -> Deposit {
        Deposit {
            round_id: self.round_id.clone(),
            said: text.to_string(),
            inferred: format!("a human asked for a change after round {}", self.round_id),
            diff: String::new(),
            questions: Vec::new(),
            harness_commit: None,
        }
    }

    /// Write the human's request as a deposit, and say where it went.
    ///
    /// The deposits are numbered: the flow's own deposit is 0, so the human's
    /// first request is 1 and the two never fight over one file.
    pub fn write_request(&mut self, text: &str) -> io::Result<PathBuf> {
        let deposit = self.request_deposit(text);
        let seq = self.next_request_seq();
        let path = self.deposits_dir.join(deposits::file_name(&deposit, seq));
        fs::create_dir_all(&self.deposits_dir)?;
        fs::write(&path, deposits::to_markdown(&deposit))?;
        self.requests = seq.saturating_add(1);
        Ok(path)
    }

    /// The number the next request gets: after the ones that are already there.
    ///
    /// The flow's own deposit is 0, so the human's first request is 1. A
    /// request written before the screen was opened still counts: the
    /// numbers are read off the files, not off a counter that starts fresh.
    fn next_request_seq(&self) -> u32 {
        let mut seq = self.requests.max(1);
        for path in deposits_in(&self.deposits_dir, &self.round_id) {
            if let Some(n) = seq_of(&path) {
                seq = seq.max(n.saturating_add(1));
            }
        }
        seq
    }

    /// The reviewer's next input: the human's request, in the shape the
    /// reviewer is asked in.
    pub fn reviewer_input(&self, text: &str) -> String {
        let mut s = String::new();
        let _ = writeln!(
            s,
            "A human asked for a change after round {}.",
            self.round_id
        );
        let _ = writeln!(s);
        let _ = writeln!(s, "{text}");
        s
    }

    /// The decision the flow did not make.
    fn other_decision(&self) -> Decision {
        match self.decision {
            Decision::Adopt => Decision::Reject,
            Decision::Reject => Decision::Adopt,
        }
    }
}

/// The metrics of a round, one line per known metric.
fn metric_lines(
    current: Option<&RoundMetrics>,
    previous: Option<&RoundMetrics>,
) -> Vec<MetricLine> {
    KNOWN_METRICS
        .iter()
        .map(|name| {
            let value = current.and_then(|m| metric_value(m, name));
            let before = previous.and_then(|m| metric_value(m, name));
            let delta = match (value, before) {
                (Some(now), Some(was)) => Some(now - was),
                _ => None,
            };
            MetricLine {
                name: (*name).to_string(),
                value,
                previous: before,
                delta,
            }
        })
        .collect()
}

/// The situations the minds struggled in, as the screen shows them.
///
/// The record carries them already ranked (the flow kept the top of the
/// round's table); the screen turns each into the one line a person reads
/// and keeps the numbers beside it. A round with no table — a reflex-only
/// round, or a record from before the signal existed — shows nothing, and
/// says nothing.
fn surprise_of(record: &FlowRecord) -> Vec<SurpriseLine> {
    record.surprise.iter().map(surprise_line).collect()
}

/// One bucket of the table, as one line plus its numbers.
fn surprise_line(counts: &SituationCounts) -> SurpriseLine {
    SurpriseLine {
        text: counts.line(),
        score_x1000: counts.surprise_x1000(),
        asks: counts.asks,
        unsure_asks: counts.unsure_asks,
        refused: counts.refused.values().sum(),
        escalated: counts.escalated.values().sum(),
    }
}

/// What the reviewer proposed, in words and in a diff.
///
/// The diff is the one the flow wrote down: the deposit holds the change
/// exactly as it landed, and the deposit is the record of it.
fn proposal_of(record: &FlowRecord) -> Option<ProposalView> {
    let proposal = record.proposal.as_ref()?;
    Some(ProposalView {
        summary: summary_of(proposal),
        diff: diff_of(record),
    })
}

/// One line: what the change is.
fn summary_of(proposal: &Proposal) -> String {
    match proposal {
        Proposal::AddGate { agent, .. } => format!("a new gate for agent {agent}"),
        Proposal::EditReflex { agent, name, .. } => {
            format!("a new reflex `{name}` for agent {agent}")
        }
        Proposal::LexiconEntry { squad, .. } => format!("a new lexicon entry for squad {squad}"),
        Proposal::PolicyTweak { agent, key, value } => {
            format!("the policy key `{key}` of agent {agent}, set to {value}")
        }
    }
}

/// The exact change, as the deposit holds it.
fn diff_of(record: &FlowRecord) -> String {
    let Some(path) = record.deposit_path.as_ref() else {
        return String::new();
    };
    let Ok(text) = fs::read_to_string(path) else {
        return String::new();
    };
    match deposits::from_markdown(&text) {
        Ok(deposit) => deposit.diff,
        Err(_) => String::new(),
    }
}

/// The prediction that was sealed, as the screen shows it.
///
/// The ledger holds the promise as it was written; the record holds the claim
/// it was made from. The ledger wins when both are there.
fn prediction_of(record: &FlowRecord) -> Option<PredictionView> {
    let seal = record.seal?;
    let ledger = Ledger::open(&record.ledger_path).ok();
    let sealed = ledger.as_ref().and_then(|l| l.prediction(seal));
    let claim: Option<&Claim> = record.claim.as_ref();

    let metric = sealed
        .map(|p| p.metric.clone())
        .or_else(|| claim.map(|c| c.metric.clone()))?;
    let direction = sealed
        .map(|p| p.direction)
        .or_else(|| claim.map(|c| c.direction))?;
    let min_delta = sealed
        .map(|p| p.min_delta)
        .or_else(|| claim.map(|c| c.min_delta))?;
    let eval_seeds = if record.eval_seeds.is_empty() {
        sealed.map(|p| p.eval_seeds.clone()).unwrap_or_default()
    } else {
        record.eval_seeds.clone()
    };

    Some(PredictionView {
        seal,
        metric,
        direction,
        min_delta,
        eval_seeds,
        author: sealed.map(|p| p.author.clone()).unwrap_or_default(),
        text: sealed.map(|p| p.text.clone()).unwrap_or_default(),
    })
}

/// The verdict, as it was measured.
fn verdict_of(record: &FlowRecord) -> Option<VerdictView> {
    let verdict = record.verdict.as_ref()?;
    let moved = match record.claim.as_ref() {
        Some(claim) => verdict
            .deltas
            .iter()
            .filter(|(_, delta)| moved_as(claim, *delta))
            .count(),
        None => 0,
    };
    Some(VerdictView {
        seal: verdict.seal,
        adopted: verdict.adopted,
        deltas: verdict.deltas.clone(),
        moved,
        seeds: verdict.deltas.len(),
        override_reason: verdict.override_reason.clone(),
    })
}

/// Did the change move the metric the way the prediction said?
fn moved_as(claim: &Claim, delta: i64) -> bool {
    match claim.direction {
        Direction::Up => delta >= claim.min_delta,
        Direction::Down => delta <= claim.min_delta.saturating_neg(),
    }
}

/// The reason a human gave, or the flow's own words for it.
fn reason_of(reason: &str) -> String {
    let trimmed = reason.trim();
    if trimmed.is_empty() {
        DEFAULT_REASON.to_string()
    } else {
        trimmed.to_string()
    }
}

/// Where the deposits are written.
fn deposits_dir_of(record: &FlowRecord) -> PathBuf {
    if let Some(dir) = record.deposit_path.as_ref().and_then(|p| p.parent()) {
        return dir.to_path_buf();
    }
    record
        .ledger_path
        .parent()
        .map(|dir| dir.join("deposits"))
        .unwrap_or_else(|| PathBuf::from("deposits"))
}

/// The metrics of the round the record points at, measured from its log.
///
/// A log that cannot be read has no metrics: the screen shows the numbers it
/// has, and says nothing about the ones it does not.
pub fn metrics_of(record: &FlowRecord) -> Option<RoundMetrics> {
    metrics_of_path(&record.log_path)
}

/// The metrics of a round, measured from its log.
pub fn metrics_of_path(path: &Path) -> Option<RoundMetrics> {
    let mut file = fs::File::open(path).ok()?;
    let log = lm_tools::runner::read_log(&mut file).ok()?;
    Some(RoundMetrics::from_log(&log))
}

/// The header the flow wrote for the next round.
///
/// The flow writes it as JSON next to the record; the next round is played
/// with it, harness pins and all (D27).
pub fn next_header(record: &FlowRecord) -> Result<lm_core::header::Header, FlowError> {
    let text = fs::read_to_string(&record.next_header_path)
        .map_err(|err| FlowError::Harness(format!("the next header: {err}")))?;
    serde_json::from_str(&text).map_err(|err| FlowError::Harness(format!("the next header: {err}")))
}

/// The deposits of a round, in the order they were written.
pub fn deposits_in(dir: &Path, round_id: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with(&format!("deposit-{round_id}-")) && name.ends_with(".md") {
            out.push(path);
        }
    }
    out.sort();
    out
}

/// The number a deposit file carries: `deposit-r7-003.md` is 3.
fn seq_of(path: &Path) -> Option<u32> {
    let name = path.file_name()?.to_str()?;
    let number = name.strip_suffix(".md")?.rsplit('-').next()?;
    number.parse().ok()
}
