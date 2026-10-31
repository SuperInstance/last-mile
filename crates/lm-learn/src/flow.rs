//! `lm-learn::flow`: one learning round, from the played log to a pinned
//! lesson.
//!
//! ```text
//! play a round -> measure it -> show a reviewer -> parse what it said
//!   -> seal the prediction (D32) -> evaluate it on held-out seeds
//!   -> record the verdict -> if it passed: apply it, commit it, tag the round
//!   -> write the deposit -> pin the next round's header (D27)
//! ```
//!
//! One way, and every link of it is on disk: the round's log, the ledger
//! (the sealed prediction and its verdict), the deposit (the change in
//! words), the harness's git history (the commit and the round's tag) and
//! the flow record (all of it in one file). Nothing is applied to the
//! harness before the held-out evaluation has passed, and a proposal that
//! never got a prediction never gets sealed.
//!
//! The harness is a directory of harnesses, one per agent: `a0`, `a1`, …
//! Each of them may be a git repo of its own; that is where a change is
//! committed and where the round's tag goes (D21/D27).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use lm_agents::harness::{self, Harness};
use lm_agents::ledger::QuestionLedger;
use lm_agents::surprise::{RoundSurprise, SituationCounts};
use lm_core::header::Header;
use lm_core::log::RoundLog;
use lm_core::Fnv64;
use lm_router::{ChatReq, Pool, Router, RouterConfig, RouterError};
use serde::{Deserialize, Serialize};

use crate::deposits::{self, Deposit};
use crate::eval;
use crate::ledger::{Direction, Ledger, LedgerError, Prediction, Verdict};
use crate::metrics::RoundMetrics;
use crate::review::{build_review_prompt, parse_answer, Claim, HarnessView, Proposal};
use crate::round::play;
use crate::sign;

/// The reflex an agent runs when no harness is pinned for it.
///
/// The file the repository ships, compiled in: the same reflex every round
/// without a harness plays.
pub const DEFAULT_REFLEX: &str = include_str!("../../../assets/reflexes/default.rhai");

/// How many ticks a learning round may run, when nothing else is said.
pub const DEFAULT_TICKS: u64 = 1_500;

/// How many held-out seeds the evaluation runs on, when nothing else is said.
///
/// Sixteen: at four seeds a verdict turns on luck — four of the seed farm's
/// thirty proposals were wrongly adopted at that count, every one of them a
/// false adopt (crates/lm-learn/SEEDFARM.md). Sixteen is where the false
/// adopt of a change that truly helps ~60% of the seeds falls from ~47% to
/// ~17%, and where the sign test can clear the fairest possible sample.
pub const DEFAULT_EVAL_SEEDS: usize = 16;

/// The horizon from which the claimable metrics exist.
///
/// Below this many ticks every metric the flow can claim is zero on every
/// seed — a squad holds nothing before about tick 1_000 — so a verdict read
/// at a shorter horizon measures nothing at all. The flow still runs and
/// still rejects there; it says so, so the rejection is not read as a
/// measurement (crates/lm-learn/SEEDFARM.md).
pub const MEASURABLE_TICKS: u64 = 1_000;

/// How many of the round's most surprising situations the record keeps: the
/// between-round screen is read by a person, and five lines is a reading
/// list, not a wall.
pub const TOP_SURPRISE: usize = 5;

/// The most tokens a review may use.
pub const DEFAULT_MAX_TOKENS: u32 = 4_000;

/// What the flow decided about a proposal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    /// The held-out evaluation passed: the change is adopted.
    Adopt,
    /// The proposal was refused, or the evaluation did not back it.
    Reject,
}

impl std::fmt::Display for Decision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Decision::Adopt => f.write_str("adopt"),
            Decision::Reject => f.write_str("reject"),
        }
    }
}

/// A human's override of the flow's decision, kept with the record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanOverride {
    /// What the human decided.
    pub decision: Decision,
    /// Why they decided it.
    pub reason: String,
    /// The commit the override produced: the commit the change landed in
    /// (an override to adopt), or the commit that reverted it (an override
    /// to reject). Empty when the override changed nothing.
    #[serde(default)]
    pub commit: Option<String>,
}

/// The record of one learning round: every link of the chain, in one file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FlowRecord {
    /// The round's id (`r7`).
    pub round_id: String,
    /// The seed the round was played with.
    pub seed: u64,
    /// The round's log.
    pub log_path: PathBuf,
    /// The harness root the round was played against: one directory per
    /// agent, `a<agent>`.
    ///
    /// An override is carried out against this harness, so the record says
    /// where it is.
    #[serde(default)]
    pub harness_root: PathBuf,
    /// A digest of the round's metrics, so the record can be checked against
    /// the log without replaying it.
    pub metrics_digest: String,
    /// The reviewer's answer, verbatim.
    pub review_text: String,
    /// The proposal, when the answer held one.
    pub proposal: Option<Proposal>,
    /// Why the proposal was refused, when it was.
    pub rejection: Option<String>,
    /// What the proposal was predicted to change.
    pub claim: Option<Claim>,
    /// The seal of the sealed prediction.
    pub seal: Option<u64>,
    /// The verdict the evaluation reached.
    pub verdict: Option<Verdict>,
    /// The flow's decision.
    pub decision: Decision,
    /// The harness commit the change landed in, when it was adopted.
    pub commit: Option<String>,
    /// The commit the next round's header pins.
    pub next_header_pin: String,
    /// Every commit the next round's header pins, per agent.
    pub next_header_pins: BTreeMap<u16, String>,
    /// Where the next round's header was written.
    pub next_header_path: PathBuf,
    /// Where this record was written.
    pub record_path: PathBuf,
    /// The deposit that records the change in words.
    pub deposit_path: Option<PathBuf>,
    /// The ledger the seal and the verdict went into.
    pub ledger_path: PathBuf,
    /// The held-out seeds the evaluation ran on.
    pub eval_seeds: Vec<u64>,
    /// The horizon the round was judged at: the ticks every seed of the
    /// evaluation was played for.
    ///
    /// A record written before this field existed reads zero, and the
    /// summary then says the horizon was not recorded.
    #[serde(default)]
    pub ticks: u64,
    /// The human overrides, in the order they happened.
    pub overrides: Vec<HumanOverride>,
    /// The round's top surprising situations: where the minds were unsure,
    /// refused or escalated, by situation bucket, most surprising first.
    ///
    /// The table is counted where the asks happen (the round's agents, on
    /// the Thor) and handed to the flow by whoever ran the round with live
    /// minds; a round the flow played with the reflex alone has none, and
    /// the field says so by being empty. Empty for every record written
    /// before the field existed, too — the default is what old records read.
    #[serde(default)]
    pub surprise: Vec<SituationCounts>,
}

impl FlowRecord {
    /// What the round did, in words: the numbers, the decision and where
    /// everything is.
    pub fn summary(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(s, "round {} (seed {})", self.round_id, self.seed);
        let _ = writeln!(s, "  log:            {}", self.log_path.display());
        let _ = writeln!(s, "  metrics digest: {}", self.metrics_digest);
        match &self.proposal {
            Some(p) => {
                let _ = writeln!(s, "  proposal:       {}", describe_proposal(p));
            }
            None => {
                let _ = writeln!(s, "  proposal:       none");
            }
        }
        if let Some(why) = &self.rejection {
            let _ = writeln!(s, "  refused:        {why}");
        }
        if let Some(claim) = &self.claim {
            let _ = writeln!(
                s,
                "  prediction:     {} {} by at least {}",
                claim.metric,
                match claim.direction {
                    Direction::Up => "up",
                    Direction::Down => "down",
                },
                claim.min_delta
            );
        }
        if let Some(seal) = self.seal {
            let _ = writeln!(s, "  seal:           {seal}");
        }
        if let Some(verdict) = &self.verdict {
            let deltas = verdict
                .deltas
                .iter()
                .map(|(seed, delta)| format!("{seed}:{delta:+}"))
                .collect::<Vec<_>>()
                .join(" ");
            let _ = writeln!(s, "  eval:           {deltas}");
        }
        // The sign test, next to the count: the two halves of the pass
        // rule, so a reader can see why the verdict is what it is.
        if let (Some(claim), Some(verdict)) = (&self.claim, &self.verdict) {
            let _ = writeln!(
                s,
                "  sign test:      {}",
                sign_test_text(&verdict.deltas, claim)
            );
        }
        let _ = writeln!(s, "  horizon:        {}", self.horizon_text());
        let _ = writeln!(s, "  decision:       {}", self.decision);
        match &self.commit {
            Some(commit) => {
                let _ = writeln!(
                    s,
                    "  commit:         {commit} (tag round-{})",
                    self.round_id
                );
            }
            None => {
                let _ = writeln!(s, "  commit:         none");
            }
        }
        for over in &self.overrides {
            match &over.commit {
                Some(commit) => {
                    let _ = writeln!(
                        s,
                        "  override:       {} by a human — {} (commit {commit})",
                        over.decision, over.reason
                    );
                }
                None => {
                    let _ = writeln!(
                        s,
                        "  override:       {} by a human — {}",
                        over.decision, over.reason
                    );
                }
            }
        }
        if let Some(path) = &self.deposit_path {
            let _ = writeln!(s, "  deposit:        {}", path.display());
        }
        if !self.surprise.is_empty() {
            let _ = writeln!(s, "  surprise:       the situations the minds struggled in");
            for counts in &self.surprise {
                let _ = writeln!(s, "    - {}", counts.line());
            }
        }
        let _ = writeln!(s, "  next header:    pins {}", self.next_header_pin);
        s
    }

    /// The horizon the round was judged at, and what it can measure.
    ///
    /// A horizon below [`MEASURABLE_TICKS`] measures nothing: every claimable
    /// metric is zero on every seed there, so the verdict that comes out of
    /// it is a rejection of the proposal and not a measurement of it. The
    /// summary says so, so nobody reads the rejection as evidence.
    fn horizon_text(&self) -> String {
        if self.ticks == 0 {
            // A record written before the horizon was kept.
            return "not recorded".to_owned();
        }
        if self.ticks < MEASURABLE_TICKS {
            return format!(
                "{} ticks — too short to measure: every claimable metric is zero before about \
                 tick {MEASURABLE_TICKS}, so a verdict here can only be a rejection",
                self.ticks
            );
        }
        format!("{} ticks", self.ticks)
    }
}

/// The sign test over a judged round's deltas, in words.
///
/// The count and the p-value are both here, because both of them decide:
/// the count says how many seeds met the bar, the p-value says which way
/// the seeds that moved moved.
fn sign_test_text(deltas: &[(u64, i64)], claim: &Claim) -> String {
    let test = eval::sign_test_of(deltas, claim.direction);
    let p = test.p_value_text();
    if test.moved == 0 {
        return format!("no seed moved, so there is nothing to read (p = {p})");
    }
    format!(
        "{} of {} seeds moved {}, p = {p} — {}",
        test.claimed,
        test.moved,
        match claim.direction {
            Direction::Up => "up",
            Direction::Down => "down",
        },
        if sign::passes(test) {
            "below 0.05"
        } else {
            "not below 0.05"
        }
    )
}

/// How one learning round is set up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowConfig {
    /// The seed of the round. The held-out seeds are drawn from far away
    /// from it, so a lesson is never judged on the round it came from.
    pub seed: u64,
    /// The harness root: one directory per agent, `a<agent>`.
    pub harness_root: PathBuf,
    /// Where the round's files go (`runs/`).
    pub runs_root: PathBuf,
    /// How many ticks the round may run.
    pub ticks: u64,
    /// How many held-out seeds the evaluation runs on.
    pub eval_seed_count: usize,
    /// The round's id; derived from the seed when empty.
    pub round_id: Option<String>,
    /// The surprise table of the round the flow is asked about, when the
    /// caller has one (a round played with live minds on the Thor). Its top
    /// entries land in the record, where the between-round screen reads
    /// them; `None` — the flow's own reflex-only rounds — records none.
    pub surprise: Option<RoundSurprise>,
    /// The question ledger of the round the flow is asked about, when the
    /// caller has one.
    ///
    /// The ledger says what each question of the harness cost and what it
    /// answered (ROADMAP-AHEAD §2.5). It is written beside the round's
    /// other files and is never hashed: only decisions travel.
    pub question_ledger: Option<QuestionLedger>,
}

impl FlowConfig {
    /// A flow for one seed and one harness root.
    pub fn new(seed: u64, harness_root: impl Into<PathBuf>) -> FlowConfig {
        FlowConfig {
            seed,
            harness_root: harness_root.into(),
            runs_root: PathBuf::from("runs"),
            ticks: DEFAULT_TICKS,
            eval_seed_count: DEFAULT_EVAL_SEEDS,
            round_id: None,
            surprise: None,
            question_ledger: None,
        }
    }

    /// Hand the flow the round's surprise table, and its top situations go
    /// into the record.
    ///
    /// The table is counted where the asks happen — the round's agents — so
    /// this is the seam between the host that played the round with live
    /// minds and the flow that records it. [`TOP_SURPRISE`] entries are
    /// kept, each a one-line, human-readable description.
    pub fn with_surprise(mut self, surprise: RoundSurprise) -> FlowConfig {
        self.surprise = Some(surprise);
        self
    }

    /// Hand the flow the round's question ledger, and it is written with
    /// the round's other files.
    ///
    /// The ledger is the seam between the host that played the round with
    /// live minds and the flow that records it, in the same way the
    /// surprise table is.
    pub fn with_question_ledger(mut self, ledger: QuestionLedger) -> FlowConfig {
        self.question_ledger = Some(ledger);
        self
    }

    /// Where the round's files go.
    pub fn with_runs_root(mut self, runs_root: impl Into<PathBuf>) -> FlowConfig {
        self.runs_root = runs_root.into();
        self
    }

    /// How many ticks the round may run.
    pub fn with_ticks(mut self, ticks: u64) -> FlowConfig {
        self.ticks = ticks;
        self
    }

    /// How many held-out seeds the evaluation runs on.
    pub fn with_eval_seeds(mut self, count: usize) -> FlowConfig {
        self.eval_seed_count = count;
        self
    }

    /// The round's id.
    pub fn with_round_id(mut self, round_id: impl Into<String>) -> FlowConfig {
        self.round_id = Some(round_id.into());
        self
    }

    /// The round's id: what it was given, or `r<seed>`.
    pub fn round_id(&self) -> String {
        self.round_id
            .clone()
            .unwrap_or_else(|| format!("r{}", self.seed))
    }

    /// The held-out seeds: far from the seed the round was learned from, and
    /// never the same seed.
    pub fn eval_seeds(&self) -> Vec<u64> {
        eval::eval_seeds(&[self.seed], self.eval_seed_count, self.seed)
    }
}

/// Why the reviewer could not answer.
#[derive(Debug)]
pub enum ReviewError {
    /// The reviewer could not be reached.
    Router(RouterError),
    /// The reviewer answered nothing.
    Empty,
}

impl std::fmt::Display for ReviewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReviewError::Router(err) => write!(f, "the reviewer could not answer: {err}"),
            ReviewError::Empty => f.write_str("the reviewer answered nothing"),
        }
    }
}

impl std::error::Error for ReviewError {}

/// Who reviews a round and proposes the next change.
pub trait Reviewer {
    /// Read the prompt and answer with a proposal.
    fn review(&mut self, prompt: &str) -> Result<String, ReviewError>;

    /// Who the reviewer is, for the ledger.
    fn name(&self) -> &str {
        "reviewer"
    }
}

/// A reviewer that always answers the same thing.
///
/// For the tests, and for a dry run of the chain: the same answer, every
/// time, with no network and no key.
#[derive(Clone, Debug)]
pub struct StubReviewer {
    text: String,
}

impl StubReviewer {
    /// A reviewer that answers `text`.
    pub fn new(text: impl Into<String>) -> StubReviewer {
        StubReviewer { text: text.into() }
    }

    /// A reviewer that answers with a change that changes nothing: a dry run
    /// of the chain that never moves a metric.
    pub fn harmless() -> StubReviewer {
        StubReviewer::new(HARMLESS_REVIEW)
    }
}

impl Reviewer for StubReviewer {
    fn review(&mut self, _prompt: &str) -> Result<String, ReviewError> {
        Ok(self.text.clone())
    }

    fn name(&self) -> &str {
        "stub"
    }
}

/// A reviewer that asks a cloud pool (System 2).
///
/// The pool order comes from the config: every pool the config has, in the
/// order the pools are listed in `router.toml`. The router holds the keys;
/// nothing here ever sees one, and nothing here logs one.
pub struct RouterReviewer {
    router: Router,
    pools: Vec<Pool>,
    max_tokens: u32,
    last: Option<String>,
}

impl RouterReviewer {
    /// A reviewer over the pools a config has, in the config's order.
    pub fn from_config(config: RouterConfig) -> Result<RouterReviewer, FlowError> {
        let pools: Vec<Pool> = Pool::ALL
            .iter()
            .copied()
            .filter(|pool| config.pool(*pool).is_some())
            .collect();
        if pools.is_empty() {
            return Err(FlowError::Router(RouterError::NoPools));
        }
        let router = Router::load(config)?;
        Ok(RouterReviewer {
            router,
            pools,
            max_tokens: DEFAULT_MAX_TOKENS,
            last: None,
        })
    }

    /// A reviewer over the pools the user's own config has.
    ///
    /// The config is found where the router finds it, and the keys stay in
    /// the router: this only ever hands the router a question.
    pub fn discover() -> Result<RouterReviewer, FlowError> {
        RouterReviewer::from_config(RouterConfig::discover()?)
    }

    /// A reviewer over the pools given, in the order given.
    pub fn new(router: Router, pools: Vec<Pool>) -> RouterReviewer {
        RouterReviewer {
            router,
            pools,
            max_tokens: DEFAULT_MAX_TOKENS,
            last: None,
        }
    }

    /// How many tokens a review may use.
    pub fn with_max_tokens(mut self, max_tokens: u32) -> RouterReviewer {
        self.max_tokens = max_tokens;
        self
    }
}

impl Reviewer for RouterReviewer {
    fn review(&mut self, prompt: &str) -> Result<String, ReviewError> {
        let req = ChatReq::system_and_user(SYSTEM_PROMPT, prompt).with_max_tokens(self.max_tokens);
        let resp = self
            .router
            .ask(&self.pools, req)
            .map_err(ReviewError::Router)?;
        self.last = Some(format!("router:{}/{}", resp.pool, resp.model));
        if resp.text.trim().is_empty() {
            return Err(ReviewError::Empty);
        }
        Ok(resp.text)
    }

    fn name(&self) -> &str {
        self.last.as_deref().unwrap_or("router")
    }
}

/// The answer of the harmless reviewer: a change that changes nothing, so a
/// dry run of the chain never moves a metric and never touches the harness.
pub const HARMLESS_REVIEW: &str = r#"{"proposal": {"kind": "LexiconEntry", "squad": 0, "text": "hold the beacon you have"},
   "prediction": {"metric": "hold_ticks_squad0", "direction": "Up", "min_delta": 1}}"#;

/// What the reviewer is told before the prompt.
const SYSTEM_PROMPT: &str = "You are the between-round reviewer (System 2) of a last-mile squad. \
Answer with one JSON object and nothing else: a proposal and the prediction that goes with it.";

/// Why an override could not be carried out.
///
/// A human's override is a decision the next round has to live with, so it
/// is carried out before it is recorded. When it cannot be carried out, it
/// is refused with one of these, and nothing is written: the ledger never
/// holds an override that was not carried out, and the harness is left
/// exactly as it was.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OverrideError {
    /// The round holds no proposal: there is nothing to apply, and
    /// nothing to revert.
    NoProposal {
        /// The round that was overridden.
        round_id: String,
    },
    /// The record does not say where the harness is.
    NoHarnessRoot {
        /// The round that was overridden.
        round_id: String,
    },
    /// The record does not say where the round's files go.
    NoRunsRoot {
        /// The round that was overridden.
        round_id: String,
    },
    /// The proposal's harness is not the one the round measured: it was
    /// not there when the round was played, or it has moved on since.
    ///
    /// `expected` is the identity the round recorded (empty when the
    /// harness was not part of the round), `found` is the identity now.
    HarnessMovedOn {
        /// The agent whose harness moved on.
        agent: u16,
        /// The identity the round recorded.
        expected: String,
        /// The identity the harness has now.
        found: String,
    },
    /// The proposal's harness is gone.
    HarnessGone {
        /// The agent whose harness is gone.
        agent: u16,
    },
    /// The change was never committed: the harness is not a git repo of
    /// its own, so there is no commit to revert and no tag to move.
    NoCommit {
        /// The round that was overridden.
        round_id: String,
    },
    /// The round's commit has no parent: there is no base to restore.
    NoBase {
        /// The commit that was to be reverted.
        commit: String,
    },
    /// Git refused the revert.
    RevertRefused {
        /// What git said.
        why: String,
    },
}

impl std::fmt::Display for OverrideError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OverrideError::NoProposal { round_id } => {
                write!(f, "round {round_id} holds no proposal to apply")
            }
            OverrideError::NoHarnessRoot { round_id } => {
                write!(f, "round {round_id} does not say where its harness is")
            }
            OverrideError::NoRunsRoot { round_id } => {
                write!(f, "round {round_id} does not say where its files go")
            }
            OverrideError::HarnessMovedOn {
                agent,
                expected,
                found,
            } => {
                if expected.is_empty() {
                    write!(f, "agent {agent} had no harness when the round was played")
                } else {
                    write!(
                        f,
                        "agent {agent}'s harness moved on: the round measured {expected}, it is {found} now"
                    )
                }
            }
            OverrideError::HarnessGone { agent } => {
                write!(f, "agent {agent}'s harness is gone")
            }
            OverrideError::NoCommit { round_id } => write!(
                f,
                "round {round_id} committed nothing: there is no commit to revert"
            ),
            OverrideError::NoBase { commit } => {
                write!(
                    f,
                    "commit {commit} has no parent: there is no base to restore"
                )
            }
            OverrideError::RevertRefused { why } => write!(f, "git refused the revert: {why}"),
        }
    }
}

impl std::error::Error for OverrideError {}

/// What can go wrong in a learning round.
#[derive(Debug)]
pub enum FlowError {
    /// A file could not be read or written.
    Io(std::io::Error),
    /// The ledger refused.
    Ledger(LedgerError),
    /// The reviewer could not answer.
    Review(ReviewError),
    /// The evaluation could not run.
    Eval(String),
    /// The harness is not what the flow needs.
    Harness(String),
    /// The harness's git refused.
    Git(String),
    /// The router refused.
    Router(RouterError),
    /// A human's override could not be carried out.
    Override(OverrideError),
}

impl std::fmt::Display for FlowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FlowError::Io(err) => write!(f, "io: {err}"),
            FlowError::Ledger(err) => write!(f, "ledger: {err}"),
            FlowError::Review(err) => write!(f, "review: {err}"),
            FlowError::Eval(why) => write!(f, "evaluation: {why}"),
            FlowError::Harness(why) => write!(f, "harness: {why}"),
            FlowError::Git(why) => write!(f, "git: {why}"),
            FlowError::Router(err) => write!(f, "router: {err}"),
            FlowError::Override(err) => write!(f, "override: {err}"),
        }
    }
}

impl std::error::Error for FlowError {}

impl From<std::io::Error> for FlowError {
    fn from(err: std::io::Error) -> FlowError {
        FlowError::Io(err)
    }
}

impl From<LedgerError> for FlowError {
    fn from(err: LedgerError) -> FlowError {
        FlowError::Ledger(err)
    }
}

impl From<ReviewError> for FlowError {
    fn from(err: ReviewError) -> FlowError {
        FlowError::Review(err)
    }
}

impl From<RouterError> for FlowError {
    fn from(err: RouterError) -> FlowError {
        FlowError::Router(err)
    }
}

impl From<OverrideError> for FlowError {
    fn from(err: OverrideError) -> FlowError {
        FlowError::Override(err)
    }
}

/// Run one learning round.
///
/// The chain: play the round, measure it, ask a reviewer, parse what it said,
/// seal the prediction, evaluate it on held-out seeds, record the verdict,
/// apply and commit the change if it passed, write the deposit, and pin the
/// next round's header.
///
/// A proposal that cannot be parsed, or that states nothing about what it
/// expects to change, is a rejection and not a crash: the round is recorded
/// as rejected, and the harness is left exactly as it was.
pub fn run_learning_round(
    cfg: &FlowConfig,
    reviewer: &mut dyn Reviewer,
) -> Result<FlowRecord, FlowError> {
    let round_id = cfg.round_id();
    let flow_dir = cfg.runs_root.join("flow");
    let ledger_path = cfg.runs_root.join("ledger.jsonl");
    fs::create_dir_all(&flow_dir)?;

    // The question ledger of the round, when the caller played it with live
    // minds and handed it over. It is written beside the round's other
    // files: harness-adjacent, and never hashed.
    if let Some(questions) = &cfg.question_ledger {
        let dir = cfg.runs_root.join("questions");
        fs::create_dir_all(&dir)?;
        questions
            .write(&dir.join(format!("{round_id}.toml")))
            .map_err(FlowError::Io)?;
    }

    // 1. The harness, as it is now.
    let harnesses = read_harness_set(&cfg.harness_root)?;
    if harnesses.is_empty() {
        return Err(FlowError::Harness(format!(
            "no agent harness under {}",
            cfg.harness_root.display()
        )));
    }

    // 2. Play the round, headless, and write its log.
    let header = round_header(cfg.seed, &harnesses);
    let log = play(header, cfg.ticks);
    let log_path = flow_dir.join(format!("{round_id}.log"));
    write_log(&log, &log_path)?;

    // 3. What happened.
    let metrics = RoundMetrics::from_log(&log);
    let metrics_digest = metrics_digest(&metrics);

    // 4. The prompt, and the review.
    let views: Vec<HarnessView> = harnesses.values().map(harness_view).collect();
    let prompt = build_review_prompt(&metrics, &metrics.moments, &views);
    let review_text = reviewer.review(&prompt)?;
    let author = reviewer.name().to_string();

    let mut record = FlowRecord {
        round_id: round_id.clone(),
        seed: cfg.seed,
        log_path: log_path.clone(),
        harness_root: cfg.harness_root.clone(),
        metrics_digest: metrics_digest.clone(),
        review_text: review_text.clone(),
        proposal: None,
        rejection: None,
        claim: None,
        seal: None,
        verdict: None,
        decision: Decision::Reject,
        commit: None,
        next_header_pin: String::new(),
        next_header_pins: BTreeMap::new(),
        next_header_path: PathBuf::new(),
        record_path: PathBuf::new(),
        deposit_path: None,
        ledger_path: ledger_path.clone(),
        eval_seeds: Vec::new(),
        ticks: cfg.ticks,
        overrides: Vec::new(),
        surprise: cfg
            .surprise
            .as_ref()
            .map(|s| s.top(TOP_SURPRISE))
            .unwrap_or_default(),
    };

    // 5. What the reviewer said.
    let answer = match parse_answer(&review_text) {
        Ok(answer) => answer,
        Err(rejection) => {
            // A review that is not a proposal is a refusal, not a crash: the
            // round is recorded, the harness is left alone, and the question
            // goes back to a human.
            record.rejection = Some(rejection.to_string());
            record.decision = Decision::Reject;
            let path = write_deposit(
                cfg,
                &record,
                &review_text,
                &format!("no proposal could be read from the review ({rejection})"),
                String::new(),
                vec!["The reviewer's answer held no proposal. What should change?".to_string()],
            )?;
            record.deposit_path = Some(path);
            return finish(cfg, record, &harnesses);
        }
    };

    let claim = answer.claim.clone().unwrap_or_else(Claim::default_claim);
    let claim_author = if answer.claim.is_some() {
        author.clone()
    } else {
        "flow:default".to_string()
    };
    record.proposal = Some(answer.proposal.clone());
    record.claim = Some(claim.clone());

    // 6. Seal the prediction, before the test (D32).
    let eval_seeds = cfg.eval_seeds();
    record.eval_seeds = eval_seeds.clone();
    let mut ledger = Ledger::open(&ledger_path)?;
    let prediction = Prediction {
        round_id: round_id.clone(),
        proposal_hash: proposal_hash(&answer.proposal),
        metric: claim.metric.clone(),
        direction: claim.direction,
        min_delta: claim.min_delta,
        eval_seeds: eval_seeds.clone(),
        author: claim_author.clone(),
        text: prediction_text(&claim, &claim_author, eval_seeds.len()),
    };
    let seal = ledger.seal(prediction.clone())?;
    record.seal = Some(seal);

    // 7. Evaluate it on the held-out seeds: the proposal is applied to a
    // copy of the harness, so the harness itself is untouched until the
    // evidence is in.
    let scratch = scratch_dir(&round_id);
    let candidate_harnesses = match prepare_candidate(&cfg.harness_root, &scratch, &answer.proposal)
    {
        Ok(set) => set,
        Err(err) => {
            let _ = fs::remove_dir_all(&scratch);
            return Err(err);
        }
    };

    let baseline = |seed: u64| round_header(seed, &harnesses);
    let candidate = |seed: u64| round_header(seed, &candidate_harnesses);
    let verdict = match eval::evaluate(&baseline, &candidate, cfg.ticks, &prediction, seal) {
        Ok(verdict) => verdict,
        Err(why) => {
            // The prediction is sealed and the test could not be run: the
            // chain keeps the seal, the round is refused, and nothing is
            // applied. A refusal is an outcome, not a crash.
            let _ = fs::remove_dir_all(&scratch);
            record.rejection = Some(format!("the evaluation could not run: {why}"));
            record.decision = Decision::Reject;
            let path = write_deposit(
                cfg,
                &record,
                &review_text,
                &format!("a proposal was read, but the evaluation could not run ({why})"),
                String::new(),
                vec!["The evaluation could not run. What should change?".to_string()],
            )?;
            record.deposit_path = Some(path);
            return finish(cfg, record, &harnesses);
        }
    };
    let _ = fs::remove_dir_all(&scratch);

    // 8. The verdict, appended to the same chain.
    ledger.verdict(verdict.clone())?;
    record.verdict = Some(verdict.clone());
    record.decision = if verdict.adopted {
        Decision::Adopt
    } else {
        Decision::Reject
    };

    // 9. Apply it, if it passed: the change, the commit, the round's tag.
    let mut diff = String::new();
    if record.decision == Decision::Adopt {
        let proposal = answer.proposal.clone();
        review_apply(&proposal, &cfg.harness_root)?;
        let (commit, the_diff) = commit_harness(&cfg.harness_root, &proposal, &round_id)?;
        record.commit = commit;
        diff = the_diff;
    }

    // 10. The deposit: the change in words, for a human to read.
    let path = write_deposit(
        cfg,
        &record,
        &review_text,
        &inferred_text(&answer.proposal, &claim),
        diff,
        questions(&record),
    )?;
    record.deposit_path = Some(path);

    finish(cfg, record, &harnesses)
}

/// Write the deposit: the change in words, for a human to read.
fn write_deposit(
    cfg: &FlowConfig,
    record: &FlowRecord,
    said: &str,
    inferred: &str,
    diff: String,
    questions: Vec<String>,
) -> Result<PathBuf, FlowError> {
    let deposit = Deposit {
        round_id: record.round_id.clone(),
        said: said.to_string(),
        inferred: inferred.to_string(),
        diff,
        questions,
        harness_commit: record.commit.clone(),
    };
    let dir = cfg.runs_root.join("deposits");
    fs::create_dir_all(&dir)?;
    let path = dir.join(deposits::file_name(&deposit, 0));
    fs::write(&path, deposits::to_markdown(&deposit))?;
    Ok(path)
}

/// The end of the chain: the next round's header, pinned, and the record.
fn finish(
    cfg: &FlowConfig,
    mut record: FlowRecord,
    harnesses: &BTreeMap<u16, Harness>,
) -> Result<FlowRecord, FlowError> {
    // The next round's header pins the harness as it is now: the commit the
    // flow produced, or the unchanged base (D27).
    let next = read_harness_set(&cfg.harness_root)?;
    let next_header = round_header(cfg.seed.saturating_add(1), &next);
    let next_header_path = cfg
        .runs_root
        .join("flow")
        .join(format!("{}-next-header.json", record.round_id));
    fs::write(
        &next_header_path,
        serde_json::to_string_pretty(&next_header)
            .map_err(|err| FlowError::Harness(format!("the next header: {err}")))?,
    )?;

    let (pin, pins) = pins_of(&record, &next_header, harnesses);
    record.next_header_pin = pin;
    record.next_header_pins = pins;
    record.next_header_path = next_header_path;

    let record_path = cfg
        .runs_root
        .join("flow")
        .join(format!("{}.json", record.round_id));
    record.record_path = record_path.clone();
    write_record(&record, &record_path)?;
    Ok(record)
}

/// The commit a harness had before the flow touched it.
fn base_commit(harnesses: &BTreeMap<u16, Harness>, agent: u16) -> String {
    harnesses
        .get(&agent)
        .map(|h| h.commit.clone())
        .unwrap_or_default()
}

/// Write the flow record next to the round it describes.
fn write_record(record: &FlowRecord, path: &Path) -> Result<(), FlowError> {
    let text = serde_json::to_string_pretty(record)
        .map_err(|err| FlowError::Harness(format!("the flow record: {err}")))?;
    fs::write(path, text)?;
    Ok(())
}

/// The pins the next header carries: every agent's, and the round's own.
///
/// The round's pin is the harness the proposal belongs to (a squad's
/// lexicon belongs to no agent, so it takes the first harness there is).
fn pins_of(
    record: &FlowRecord,
    next_header: &Header,
    harnesses: &BTreeMap<u16, Harness>,
) -> (String, BTreeMap<u16, String>) {
    let pins: BTreeMap<u16, String> = next_header
        .roster
        .iter()
        .filter_map(|entry| {
            let commit = entry.harness_commit.clone();
            (!commit.is_empty()).then_some((entry.agent, commit))
        })
        .collect();
    let pin = match record.proposal.as_ref().and_then(proposal_agent) {
        Some(agent) => pins
            .get(&agent)
            .cloned()
            .unwrap_or_else(|| base_commit(harnesses, agent)),
        None => pins.values().next().cloned().unwrap_or_default(),
    };
    (pin, pins)
}

/// Override the flow's decision, by hand.
///
/// The measured verdict stays what it was: the record says the human decided
/// otherwise, and why. Nothing is silent.
///
/// This only changes the record in memory, and the harness is left alone:
/// use [`write_override`] to carry the decision out.
pub fn override_verdict(record: &FlowRecord, decision: Decision) -> FlowRecord {
    override_verdict_reasoned(record, decision, "a human overrode the flow's decision")
}

/// The same, with the human's reason.
pub fn override_verdict_reasoned(
    record: &FlowRecord,
    decision: Decision,
    reason: &str,
) -> FlowRecord {
    let mut next = record.clone();
    next.decision = decision;
    next.overrides.push(HumanOverride {
        decision,
        reason: reason.to_string(),
        commit: None,
    });
    next
}

/// Carry out a human's override, and write it into the files.
///
/// An override is not a note: it is a decision the next round has to live
/// with, so it is carried out before it is recorded.
///
/// * An override to **Adopt** applies the proposal exactly as an adopted
///   one is applied — the same `apply`, the same commit, the same
///   `round-<id>` tag — and rewrites the next round's header so that it
///   pins that commit.
/// * An override to **Reject** of a proposal the flow adopted reverts it:
///   a new commit in the harness repo restores the base (history is never
///   rewritten) and the next header pins that commit.
///
/// The measured verdict stays what it was: the record says the human
/// decided otherwise, and why, and which commit resulted.
///
/// When the proposal can no longer be applied — the harness moved on, or
/// there is nothing to apply — the override is refused with a typed error
/// and nothing is written: the ledger never holds an override that was not
/// carried out, and the harness is left exactly as it was.
///
/// The deposit is left alone: it is the record of the proposal, not of the
/// override.
pub fn write_override(
    record: &FlowRecord,
    decision: Decision,
    reason: &str,
) -> Result<FlowRecord, FlowError> {
    let proposal = record.proposal.clone().ok_or_else(|| {
        FlowError::Override(OverrideError::NoProposal {
            round_id: record.round_id.clone(),
        })
    })?;
    let root = harness_root_of(record)?;
    let harnesses = read_harness_set(&root)?;

    // 1. The harness: the decision, carried out.
    let commit = match decision {
        Decision::Adopt => adopt_override(record, &proposal, &root, &harnesses)?,
        Decision::Reject => reject_override(record, &proposal, &root, &harnesses)?,
    };

    // 2. The ledger: the human's decision, in the chain, after the verdict
    //    it goes against. It is written after the change was carried out,
    //    so the chain never holds an override that was not.
    if let Some(seal) = record.seal {
        let mut ledger = Ledger::open(&record.ledger_path)?;
        ledger.override_verdict(seal, decision == Decision::Adopt, reason)?;
    }

    // 3. The next round's header: it pins the harness as it is now.
    let harnesses = read_harness_set(&root)?;
    let (next_header, next_header_path) = rewrite_next_header(record, &harnesses)?;

    // 4. The record: the human's decision, and the commit it produced.
    let mut next = record.clone();
    next.decision = decision;
    next.overrides.push(HumanOverride {
        decision,
        reason: reason.to_string(),
        commit: commit.clone(),
    });
    if decision == Decision::Adopt {
        next.commit = commit;
    }
    let (pin, pins) = pins_of(&next, &next_header, &harnesses);
    next.next_header_pin = pin;
    next.next_header_pins = pins;
    next.next_header_path = next_header_path;

    let record_path = record_path_of(record);
    next.record_path = record_path.clone();
    write_record(&next, &record_path)?;
    Ok(next)
}

/// Carry out an override to Adopt: the proposal is applied, committed and
/// tagged, exactly as an adopted one is.
fn adopt_override(
    record: &FlowRecord,
    proposal: &Proposal,
    root: &Path,
    harnesses: &BTreeMap<u16, Harness>,
) -> Result<Option<String>, FlowError> {
    // The flow adopted it already: the human agrees with the flow, and the
    // commit is the one the round produced.
    if record.decision == Decision::Adopt {
        return Ok(record.commit.clone());
    }
    // The flow rejected it, so the harness must still be the one the
    // proposal was measured against: a change that can no longer be
    // applied as it was judged is refused, not applied to a state nobody
    // measured.
    check_harness_unchanged(record, harnesses, proposal)?;
    review_apply(proposal, root)?;
    let (commit, _) = commit_harness(root, proposal, &record.round_id)?;
    Ok(commit)
}

/// Carry out an override to Reject: a proposal the flow adopted is
/// reverted, in a commit of its own.
fn reject_override(
    record: &FlowRecord,
    proposal: &Proposal,
    root: &Path,
    harnesses: &BTreeMap<u16, Harness>,
) -> Result<Option<String>, FlowError> {
    // The flow rejected it too: the proposal is not in the harness and the
    // human agrees. There is nothing to revert.
    if record.decision != Decision::Adopt {
        return Ok(None);
    }
    let Some(commit) = record.commit.clone() else {
        return Err(FlowError::Override(OverrideError::NoCommit {
            round_id: record.round_id.clone(),
        }));
    };
    check_harness_unchanged(record, harnesses, proposal)?;
    revert_harness(root, proposal, &record.round_id, &commit).map(Some)
}

/// The harness must be where the round left it.
///
/// The proposal was measured against that harness, so an override is
/// carried out against that state and no other: a harness that has moved
/// on since the round cannot be changed the way the round judged.
fn check_harness_unchanged(
    record: &FlowRecord,
    harnesses: &BTreeMap<u16, Harness>,
    proposal: &Proposal,
) -> Result<(), FlowError> {
    let Some(agent) = proposal_agent(proposal) else {
        // A squad's lexicon is not an agent's harness: there is no commit
        // to compare, and the file itself says whether the change is
        // already there.
        return Ok(());
    };
    let Some(harness) = harnesses.get(&agent) else {
        return Err(FlowError::Override(OverrideError::HarnessGone { agent }));
    };
    let expected = record
        .next_header_pins
        .get(&agent)
        .cloned()
        .unwrap_or_default();
    if expected != harness.commit {
        return Err(FlowError::Override(OverrideError::HarnessMovedOn {
            agent,
            expected,
            found: harness.commit.clone(),
        }));
    }
    Ok(())
}

/// Revert the round's commit: a new commit that restores the base.
///
/// History is never rewritten: the base is restored by a new commit on top
/// of the round's. The round's tag moves to it, because the tag is the
/// frozen state to roll back to (D27), and the frozen state is now the
/// reverted one.
fn revert_harness(
    root: &Path,
    proposal: &Proposal,
    round_id: &str,
    commit: &str,
) -> Result<String, FlowError> {
    let Some(agent) = proposal_agent(proposal) else {
        return Err(FlowError::Override(OverrideError::NoCommit {
            round_id: round_id.to_string(),
        }));
    };
    let dir = root.join(format!("a{agent}"));
    if !dir.join(".git").exists() {
        return Err(FlowError::Override(OverrideError::NoCommit {
            round_id: round_id.to_string(),
        }));
    }
    let base = git(&dir, &["rev-parse", &format!("{commit}^")]).map_err(|_| {
        FlowError::Override(OverrideError::NoBase {
            commit: commit.to_string(),
        })
    })?;
    // The inverse of the round's commit, staged but not committed.
    if let Err(err) = git(&dir, &["revert", "--no-commit", commit]) {
        let _ = git(&dir, &["revert", "--abort"]);
        return Err(FlowError::Override(OverrideError::RevertRefused {
            why: err.to_string(),
        }));
    }
    // And the revert itself, as a commit of its own.
    let message = format!("round {round_id}: a human reverted the change");
    let date = commit_date(round_id);
    let out = Command::new("git")
        .arg("-C")
        .arg(&dir)
        .args([
            "-c",
            "user.name=last-mile flow",
            "-c",
            "user.email=flow@last-mile.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(["commit", "-m", &message])
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
        .output()
        .map_err(|err| FlowError::Git(format!("git commit: {err}")))?;
    if !out.status.success() {
        return Err(FlowError::Git(format!(
            "git commit: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    // The revert must leave the base's tree exactly: that is what
    // "restores the base" means.
    let tree = git(&dir, &["rev-parse", "HEAD^{tree}"])?;
    let base_tree = git(&dir, &["rev-parse", &format!("{base}^{{tree}}")])?;
    if tree != base_tree {
        return Err(FlowError::Override(OverrideError::RevertRefused {
            why: format!("the revert left the tree {tree}, not the base's {base_tree}"),
        }));
    }
    let revert = git(&dir, &["rev-parse", "HEAD"])?;
    git(&dir, &["tag", "-f", &format!("round-{round_id}"), &revert])?;
    Ok(revert)
}

/// Rewrite the next round's header, so it pins the harness as it is now.
fn rewrite_next_header(
    record: &FlowRecord,
    harnesses: &BTreeMap<u16, Harness>,
) -> Result<(Header, PathBuf), FlowError> {
    let path = next_header_path_of(record)?;
    let next_header = round_header(record.seed.saturating_add(1), harnesses);
    fs::write(
        &path,
        serde_json::to_string_pretty(&next_header)
            .map_err(|err| FlowError::Harness(format!("the next header: {err}")))?,
    )?;
    Ok((next_header, path))
}

/// Where the harness the round was played against is.
fn harness_root_of(record: &FlowRecord) -> Result<PathBuf, FlowError> {
    if record.harness_root.as_os_str().is_empty() {
        return Err(FlowError::Override(OverrideError::NoHarnessRoot {
            round_id: record.round_id.clone(),
        }));
    }
    Ok(record.harness_root.clone())
}

/// Where the next round's header is.
fn next_header_path_of(record: &FlowRecord) -> Result<PathBuf, FlowError> {
    if !record.next_header_path.as_os_str().is_empty() {
        return Ok(record.next_header_path.clone());
    }
    record
        .record_path
        .parent()
        .map(|dir| dir.join(format!("{}-next-header.json", record.round_id)))
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| {
            FlowError::Override(OverrideError::NoRunsRoot {
                round_id: record.round_id.clone(),
            })
        })
}

/// Where the flow record is.
fn record_path_of(record: &FlowRecord) -> PathBuf {
    if !record.record_path.as_os_str().is_empty() {
        return record.record_path.clone();
    }
    record
        .next_header_path
        .parent()
        .map(|dir| dir.join(format!("{}.json", record.round_id)))
        .unwrap_or_else(|| PathBuf::from(format!("{}.json", record.round_id)))
}

/// The agent a proposal belongs to, when it belongs to one.
fn proposal_agent(p: &Proposal) -> Option<u16> {
    match p {
        Proposal::AddGate { agent, .. }
        | Proposal::EditReflex { agent, .. }
        | Proposal::PolicyTweak { agent, .. } => Some(*agent),
        Proposal::LexiconEntry { .. } => None,
    }
}

/// The harness directory a proposal belongs to.
fn harness_dir_of(root: &Path, p: &Proposal) -> Option<PathBuf> {
    proposal_agent(p).map(|agent| root.join(format!("a{agent}")))
}

/// Read every agent's harness under the root: the `a<agent>` directories.
fn read_harness_set(root: &Path) -> Result<BTreeMap<u16, Harness>, FlowError> {
    let mut set = BTreeMap::new();
    if !root.is_dir() {
        return Err(FlowError::Harness(format!(
            "no harness at {}",
            root.display()
        )));
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let Some(agent) = agent_id(&name.to_string_lossy()) else {
            continue;
        };
        set.insert(agent, Harness::open(&entry.path())?);
    }
    Ok(set)
}

/// `a3` is agent 3; anything else is not an agent's harness.
fn agent_id(name: &str) -> Option<u16> {
    let rest = name.strip_prefix('a')?;
    if rest.is_empty() {
        return None;
    }
    rest.parse::<u16>().ok()
}

/// The harness as the reviewer sees it.
fn harness_view(h: &Harness) -> HarnessView {
    HarnessView {
        agent: h
            .dir
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(agent_id)
            .unwrap_or_default(),
        commit: h.commit.clone(),
        gates: h.gates_toml.clone(),
        policy: h.policy_toml.clone(),
        phrasebook: h.phrasebook_toml.clone(),
        reflexes: h.reflexes.clone(),
    }
}

/// The header of a round, with the harnesses pinned into it (D27).
fn round_header(seed: u64, harnesses: &BTreeMap<u16, Harness>) -> Header {
    let mut header = Header::default_round(seed, DEFAULT_REFLEX);
    harness::pin_into(&mut header, harnesses);
    header
}

/// Write the round's log.
fn write_log(log: &RoundLog, path: &Path) -> Result<(), FlowError> {
    let mut file = fs::File::create(path)?;
    log.write(&mut file)?;
    Ok(())
}

/// A digest of the round's metrics, so the record can be checked against the
/// log without replaying it.
pub fn metrics_digest(metrics: &RoundMetrics) -> String {
    let body = serde_json::to_vec(metrics).unwrap_or_default();
    let mut h = Fnv64::new();
    h.write(&body);
    format!("{:016x}", h.finish())
}

/// A hash of the proposal, so the verdict is tied to the exact change that
/// was judged.
pub fn proposal_hash(proposal: &Proposal) -> String {
    let body = serde_json::to_vec(proposal).unwrap_or_default();
    let mut h = Fnv64::new();
    h.write(&body);
    format!("{:016x}", h.finish())
}

/// The prediction in words, as it is sealed.
fn prediction_text(claim: &Claim, author: &str, seeds: usize) -> String {
    let direction = match claim.direction {
        Direction::Up => "rise",
        Direction::Down => "fall",
    };
    format!(
        "{} should {direction} by at least {} on {seeds} held-out seed(s) (claimed by {author})",
        claim.metric, claim.min_delta
    )
}

/// What the system understood the change to be.
fn inferred_text(proposal: &Proposal, claim: &Claim) -> String {
    format!(
        "{}; it expects {} to {} by at least {}.",
        describe_proposal(proposal),
        claim.metric,
        match claim.direction {
            Direction::Up => "rise",
            Direction::Down => "fall",
        },
        claim.min_delta
    )
}

/// A proposal in words.
fn describe_proposal(proposal: &Proposal) -> String {
    match proposal {
        Proposal::AddGate { agent, .. } => format!("a new gate for agent {agent}"),
        Proposal::EditReflex { agent, name, .. } => {
            format!("a new `reflexes/{name}.rhai` for agent {agent}")
        }
        Proposal::LexiconEntry { squad, .. } => format!("a new lexicon entry for squad {squad}"),
        Proposal::PolicyTweak { agent, key, .. } => {
            format!("the policy key `{key}` of agent {agent}")
        }
    }
}

/// The open questions the deposit carries back to a human.
fn questions(record: &FlowRecord) -> Vec<String> {
    let metric = record
        .claim
        .as_ref()
        .map(|c| c.metric.clone())
        .unwrap_or_default();
    match (record.decision, record.proposal.is_some()) {
        (Decision::Adopt, true) => vec![format!(
            "The held-out seeds moved {metric} as claimed. Is this the behaviour you want in the next round?"
        )],
        (Decision::Reject, true) => vec![format!(
            "The held-out seeds did not move {metric} as claimed. What would you change instead?"
        )],
        (_, false) => vec!["The reviewer's answer held no proposal. What should change?".to_string()],
    }
}

/// Apply a proposal to the harness root.
fn review_apply(proposal: &Proposal, root: &Path) -> Result<PathBuf, FlowError> {
    crate::review::apply(proposal, root)
        .map_err(|err| FlowError::Harness(format!("the change was refused: {err}")))
}

/// A scratch directory for the candidate harness.
///
/// Every flow gets one of its own: the directory is wiped and copied into,
/// so two flows that shared one would wipe each other's candidate -- and
/// the round id alone cannot keep them apart, because two rounds may well
/// carry the same one.
fn scratch_dir(round_id: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "lm-learn-candidate-{round_id}-{}-{n}",
        std::process::id()
    ))
}

/// Copy the harness root, apply the proposal to the copy, and read it back.
///
/// The evaluation sees the proposal's effect without the harness having it:
/// the change only lands in the real harness once the evidence is in.
fn prepare_candidate(
    root: &Path,
    scratch: &Path,
    proposal: &Proposal,
) -> Result<BTreeMap<u16, Harness>, FlowError> {
    if scratch.exists() {
        fs::remove_dir_all(scratch)?;
    }
    copy_tree(root, scratch)?;
    review_apply(proposal, scratch)?;
    read_harness_set(scratch)
}

/// Copy a directory, without its git history.
fn copy_tree(from: &Path, to: &Path) -> Result<(), FlowError> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let source = entry.path();
        let target = to.join(&name);
        if entry.file_type()?.is_dir() {
            copy_tree(&source, &target)?;
        } else {
            fs::copy(&source, &target)?;
        }
    }
    Ok(())
}

/// Commit the change in the harness it belongs to, and tag the round.
///
/// Returns the commit id (when the harness is a git repo of its own) and the
/// diff that went into it. A harness that is not a repo of its own is
/// identified by its bytes, and the next round pins those bytes: there is
/// nothing to commit, and nothing is pretended.
fn commit_harness(
    root: &Path,
    proposal: &Proposal,
    round_id: &str,
) -> Result<(Option<String>, String), FlowError> {
    let Some(dir) = harness_dir_of(root, proposal) else {
        return Ok((None, String::new()));
    };
    if !dir.join(".git").exists() {
        return Ok((None, String::new()));
    }

    git(&dir, &["add", "-A"])?;
    let diff = git(&dir, &["diff", "HEAD"])?;
    let dirty = !git(&dir, &["status", "--porcelain"])?.trim().is_empty();
    if dirty {
        let message = format!("round {round_id}: {}", describe_proposal(proposal));
        let date = commit_date(round_id);
        let out = Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args([
                "-c",
                "user.name=last-mile flow",
                "-c",
                "user.email=flow@last-mile.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(["commit", "-m", &message])
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .output()
            .map_err(|err| FlowError::Git(format!("git commit: {err}")))?;
        if !out.status.success() {
            return Err(FlowError::Git(format!(
                "git commit: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
    }
    let commit = git(&dir, &["rev-parse", "HEAD"])?;
    git(&dir, &["tag", "-f", &format!("round-{round_id}"), &commit])?;
    Ok((Some(commit), diff))
}

/// The date a commit carries, derived from the round's id.
///
/// The same round therefore produces the same commit id: the chain is
/// reproducible, and a re-run does not look like a different change.
fn commit_date(round_id: &str) -> String {
    let mut h = Fnv64::new();
    h.write(round_id.as_bytes());
    let seconds = 1_600_000_000 + (h.finish() % 100_000_000);
    format!("{seconds} +0000")
}

/// Run git in a directory and hand back what it said.
fn git(dir: &Path, args: &[&str]) -> Result<String, FlowError> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|err| FlowError::Git(format!("git {}: {err}", args.join(" "))))?;
    if !out.status.success() {
        return Err(FlowError::Git(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_flow_is_small() {
        let cfg = FlowConfig::new(7, "/tmp/harness");
        assert_eq!(cfg.round_id(), "r7");
        assert_eq!(cfg.ticks, DEFAULT_TICKS);
        assert_eq!(cfg.eval_seed_count, DEFAULT_EVAL_SEEDS);
    }

    #[test]
    fn the_held_out_seeds_are_far_from_the_round() {
        let cfg = FlowConfig::new(7, "/tmp/harness");
        let seeds = cfg.eval_seeds();
        assert_eq!(seeds.len(), DEFAULT_EVAL_SEEDS);
        assert!(!seeds.contains(&cfg.seed));
        for seed in &seeds {
            assert!(*seed >= 1_000_000);
        }
        // Two rounds never share a held-out seed.
        let other = FlowConfig::new(8, "/tmp/harness").eval_seeds();
        for seed in &seeds {
            assert!(!other.contains(seed));
        }
    }

    #[test]
    fn a_commit_date_is_the_same_every_time() {
        assert_eq!(commit_date("r7"), commit_date("r7"));
        assert_ne!(commit_date("r7"), commit_date("r8"));
    }

    #[test]
    fn an_override_is_kept_with_the_record() {
        let record = FlowRecord {
            round_id: "r7".to_string(),
            seed: 7,
            log_path: PathBuf::from("r7.log"),
            harness_root: PathBuf::from("/tmp/harness"),
            metrics_digest: "0".to_string(),
            review_text: String::new(),
            proposal: None,
            rejection: None,
            claim: None,
            seal: None,
            verdict: None,
            decision: Decision::Reject,
            commit: None,
            next_header_pin: String::new(),
            next_header_pins: BTreeMap::new(),
            next_header_path: PathBuf::from("r7-next-header.json"),
            record_path: PathBuf::from("r7.json"),
            deposit_path: None,
            ledger_path: PathBuf::from("ledger.jsonl"),
            eval_seeds: Vec::new(),
            ticks: 0,
            overrides: Vec::new(),
            surprise: Vec::new(),
        };
        let overridden = override_verdict(&record, Decision::Adopt);
        assert_eq!(overridden.decision, Decision::Adopt);
        assert_eq!(record.decision, Decision::Reject);
        assert_eq!(overridden.overrides.len(), 1);
        assert_eq!(overridden.overrides[0].decision, Decision::Adopt);
    }

    #[test]
    fn an_agent_directory_is_recognised() {
        assert_eq!(agent_id("a0"), Some(0));
        assert_eq!(agent_id("a12"), Some(12));
        assert_eq!(agent_id("squad0"), None);
        assert_eq!(agent_id("a"), None);
        assert_eq!(agent_id("ax"), None);
    }

    #[test]
    fn a_record_carries_its_surprise_and_old_records_load() {
        use lm_agents::surprise::{EnemySpot, Margin, Phase, SituationKey, Standing};
        let key = SituationKey {
            standing: Standing::Enemy,
            enemy: EnemySpot::Seeing,
            stunned: true,
            margin: Margin::Behind,
            phase: Phase::Mid,
        };
        let mut counts = SituationCounts::new(key);
        counts.asks = 14;
        counts.unsure_asks = 9;
        counts.unsure.insert("attention".to_owned(), 9);

        let mut record = record_for_test();
        record.surprise = vec![counts];
        let text = serde_json::to_string(&record).expect("the record serialises");
        let back: FlowRecord = serde_json::from_str(&text).expect("the record loads");
        assert_eq!(back.surprise.len(), 1);
        assert_eq!(back.surprise[0].asks, 14);
        assert!(text.contains("attention"));

        // A record written before the field existed has no `surprise` key at
        // all: it reads back with none, and says so.
        let value: serde_json::Value = serde_json::from_str(&text).expect("json");
        let mut old = value.clone();
        old.as_object_mut()
            .expect("a record is an object")
            .remove("surprise");
        let old_record: FlowRecord = serde_json::from_value(old).expect("the old record loads");
        assert!(old_record.surprise.is_empty());
    }

    /// A minimal record, for the tests that only care about its shape.
    fn record_for_test() -> FlowRecord {
        FlowRecord {
            round_id: "r7".to_string(),
            seed: 7,
            log_path: PathBuf::from("r7.log"),
            harness_root: PathBuf::from("/tmp/harness"),
            metrics_digest: "0".to_string(),
            review_text: String::new(),
            proposal: None,
            rejection: None,
            claim: None,
            seal: None,
            verdict: None,
            decision: Decision::Reject,
            commit: None,
            next_header_pin: String::new(),
            next_header_pins: BTreeMap::new(),
            next_header_path: PathBuf::from("r7-next-header.json"),
            record_path: PathBuf::from("r7.json"),
            deposit_path: None,
            ledger_path: PathBuf::from("ledger.jsonl"),
            eval_seeds: Vec::new(),
            ticks: 0,
            overrides: Vec::new(),
            surprise: Vec::new(),
        }
    }
}
