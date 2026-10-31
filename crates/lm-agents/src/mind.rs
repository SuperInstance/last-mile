//! Task 5: the cognition loop — one agent's mind, its tiers and its fallbacks.
//!
//! A [`Mind`] is one agent's loop between the score and the model:
//!
//! ```text
//! ScoreView --facts--> plain-language facts (novelty is a change in the key)
//!          --policy--> does this moment deserve a thought? on which tier?
//!          --queue-->  a ThinkReq for the least-use-first queue (Task 4)
//!          --worker--> the HTTP call runs on the thinker thread, never here
//!          --answer--> scheduled Lane decisions, or a logged refusal (D26)
//! ```
//!
//! **Thinking time is gameplay (D55).** The mind keeps a [`TierTable`]: every
//! backend it can ask has a measured latency (its own [`RttEstimator`]) and a
//! configured quality rank, and the harness's `policy.toml` maps the moment
//! to a tier — a fresh threat wants the fastest tier whose p99 lands within
//! the threat's horizon; a quiet moment with a beacon plan wants the
//! best-quality tier whose p99 fits `max_think_ticks`. When no model tier
//! fits, the reflex tier plays the moment: a refusal is an outcome, never a
//! guessed answer (D26).
//!
//! **The levers (D56)** come from the same file: `buy_time` (hold or follow
//! the safe branch while a slow thought is in flight), `prethink` (ask about
//! the likeliest next situation in quiet ticks and cache it),
//! `max_questions` (a smaller pack is a faster call), `verdict_cache` and
//! `pay_priority`.
//!
//! **The sim thread never waits.** [`Mind::on_tick`] builds a request,
//! [`Mind::on_answer`] turns an answer into decisions, and everything in
//! between — the HTTP call — happens on the thinker thread ([`Thinker`]),
//! connected by channels. Nothing here is needed to replay a round: every
//! model answer reaches the simulation only as a logged decision, so a round
//! replays bit-exact with no backend at all.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use lm_core::decision::{Kind, Provenance, Source};
use lm_core::hash::Fnv64;
use lm_core::header::{Role, RosterEntry};
use lm_core::opal::TalkContext;
use lm_core::params::Params;
use lm_core::sched::lead_ticks;
use lm_core::talk::TalkParams;
use lm_core::utterance::Addressee;
use lm_core::view::{Menu, ScoreView};
use lm_core::world::World;
use lm_core::{Decision, Tick};
use serde::Deserialize;
use serde_json::Value;

use crate::agents::WordCounts;
use crate::config::AgentsConfig;
use crate::facts::Facts;
use crate::gates::{answers_to_decisions, GateSet};
use crate::question::QuestionIdentity;
use crate::surprise::{RoundSurprise, SituationKey};

/// The pack a thought carries (D70): the move pack or the talk pack.
pub use crate::gates::Pack;
use crate::harness::Harness;
use crate::phrasebook::Phrasebook;
use crate::queue::ThinkReq;
use crate::shared::SharedConfig;
use crate::sysone::{Answers, Escalation, EscalationOutcome, Refusal, SysOne};
use crate::utter::{
    pick_realizer_with, plan_from_gates, presence_gate, utterance_from_text, GateCtx, Realizer,
    WordSource, GATE_GESTURE, UTTERANCE_GATES,
};

/// The shipped default policy, the single source of `Policy`'s defaults.
const DEFAULT_POLICY_TOML: &str = include_str!("../../../assets/harness/default/policy.toml");

/// The reflex tier's backend name: asking it means "don't ask a model".
const REFLEX_BACKEND: &str = "reflex";

// ------------------------------------------------------------ tick arithmetic

/// Ticks between asks at `rate_hz` on a `tick_hz` sim: ⌈hz/rate⌉, at least 1.
pub fn period_ticks(tick_hz: u16, rate_hz: u16) -> u64 {
    u64::from(tick_hz).div_ceil(u64::from(rate_hz.max(1)))
}

/// A latency in milliseconds rounded up to ticks on a `tick_hz` sim.
pub fn ms_to_ticks_ceil(ms: u32, tick_hz: u16) -> u64 {
    u64::from(ms)
        .saturating_mul(u64::from(tick_hz))
        .div_ceil(1_000)
}

// -------------------------------------------------------------- the estimator

/// How many samples the round-trip estimator keeps (D44: N = 32).
pub const RTT_WINDOW: usize = 32;

/// A windowed round-trip estimate in milliseconds (D44: N = 32, per tier).
///
/// Percentiles are nearest-rank over the window, so `p99` of a full window
/// is its largest sample. An empty estimator has learned nothing and reports
/// zero — an unmeasured tier is assumed fast until the measurements arrive.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RttEstimator {
    samples: VecDeque<u32>,
}

impl RttEstimator {
    /// An estimator with an empty window.
    pub fn new() -> Self {
        Self::default()
    }

    /// Note one round trip, in milliseconds. The window keeps the newest
    /// [`RTT_WINDOW`] samples.
    pub fn record(&mut self, sample_ms: u32) {
        if self.samples.len() == RTT_WINDOW {
            self.samples.pop_front();
        }
        self.samples.push_back(sample_ms);
    }

    /// How many samples the window holds.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Whether nothing has been measured yet.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Forget every sample.
    pub fn clear(&mut self) {
        self.samples.clear();
    }

    /// The median sample (nearest rank); 0 when nothing was measured.
    pub fn p50(&self) -> u32 {
        self.percentile(50)
    }

    /// The 99th-percentile sample (nearest rank); 0 when nothing was
    /// measured.
    pub fn p99(&self) -> u32 {
        self.percentile(99)
    }

    /// The nearest-rank percentile: the sample at rank ⌈p·n/100⌉ of the
    /// sorted window, so `p99` of a full window is its largest sample.
    fn percentile(&self, p: u64) -> u32 {
        let n = self.samples.len();
        if n == 0 {
            return 0;
        }
        let mut sorted: Vec<u32> = self.samples.iter().copied().collect();
        sorted.sort_unstable();
        let rank = (p * n as u64).div_ceil(100).clamp(1, n as u64) as usize;
        sorted[rank - 1]
    }
}

// --------------------------------------------------------------------- modes

/// How the mind samples its gates (D38).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Ask only when the policy fires: a novel situation, a fresh threat,
    /// within the think rate.
    Point,
    /// Sample continuously at `presence_hz` and emit lane decisions.
    Presence,
    /// Presence while the measured batch p99 + transit fits the presence
    /// period, point mode otherwise (the default).
    Auto,
}

impl Mode {
    /// Parse the config word (`point`, `presence`, `auto`); anything else is
    /// `auto`, the documented default.
    pub fn parse(word: &str) -> Mode {
        match word {
            "point" => Mode::Point,
            "presence" => Mode::Presence,
            _ => Mode::Auto,
        }
    }

    /// The config word.
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Point => "point",
            Mode::Presence => "presence",
            Mode::Auto => "auto",
        }
    }
}

// --------------------------------------------------------------- the tier table

/// One tier of thinking: a backend, its quality rank, and its measured
/// latency (D55).
///
/// Two estimators live here: `rtt` remembers whole-call latencies (the lead
/// calculation's input), and `per_question` remembers the cost per question
/// so a tier's estimate can scale to the pack being asked — a smaller pack
/// is a faster call (D56 `max_questions`).
///
/// Both are kept **per pack** (D70): the move pack is one question and the
/// talk pack is ten, so one estimator over both would price a ten-question
/// pack as ten times a one-question call, which is not what the model
/// costs. The lead of a thought is the p99 of the pack it carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tier {
    name: String,
    backend: String,
    quality: u8,
    rtt: BTreeMap<Pack, RttEstimator>,
    per_question: BTreeMap<Pack, RttEstimator>,
}

impl Tier {
    /// The tier's name, as `policy.toml` and the thought records spell it.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The backend the tier asks (`"reflex"` asks nothing).
    pub fn backend(&self) -> &str {
        &self.backend
    }

    /// The configured quality rank: higher is better thinking.
    pub fn quality(&self) -> u8 {
        self.quality
    }

    /// Whether this is the reflex tier — the one that means "don't ask".
    pub fn is_reflex(&self) -> bool {
        self.backend == REFLEX_BACKEND
    }

    /// Note one measured call of `latency_ms` over `questions` questions.
    pub fn note(&mut self, pack: Pack, latency_ms: u32, questions: u16) {
        self.rtt.entry(pack).or_default().record(latency_ms);
        self.per_question
            .entry(pack)
            .or_default()
            .record(latency_ms / u32::from(questions.max(1)));
    }

    /// The measured p99 of whole calls of this pack, in milliseconds.
    pub fn p99_ms(&self, pack: Pack) -> u32 {
        self.rtt.get(&pack).map_or(0, RttEstimator::p99)
    }

    /// The measured p50 of whole calls of this pack, in milliseconds.
    pub fn p50_ms(&self, pack: Pack) -> u32 {
        self.rtt.get(&pack).map_or(0, RttEstimator::p50)
    }

    /// The p99 a pack of `questions` questions is expected to cost: the
    /// per-question estimate scaled to the pack, falling back to the
    /// whole-call p99, and 0 while nothing has been measured.
    pub fn p99_ms_for(&self, pack: Pack, questions: u16) -> u32 {
        if let Some(per_question) = self.per_question.get(&pack) {
            if !per_question.is_empty() {
                return per_question
                    .p99()
                    .saturating_mul(u32::from(questions.max(1)));
            }
        }
        if let Some(rtt) = self.rtt.get(&pack) {
            if !rtt.is_empty() {
                return rtt.p99();
            }
        }
        0
    }

    /// [`Tier::p99_ms_for`] in ticks on a `tick_hz` sim.
    pub fn p99_ticks_for(&self, pack: Pack, questions: u16, tick_hz: u16) -> u64 {
        ms_to_ticks_ceil(self.p99_ms_for(pack, questions), tick_hz)
    }

    /// The measured p50 of whole calls of this pack, in ticks.
    pub fn p50_ticks(&self, pack: Pack, tick_hz: u16) -> u64 {
        ms_to_ticks_ceil(self.p50_ms(pack), tick_hz)
    }
}

/// The tiers a mind can think on, in `policy.toml` order (D55).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TierTable {
    tiers: Vec<Tier>,
}

impl TierTable {
    /// Every tier, in file order.
    pub fn tiers(&self) -> &[Tier] {
        &self.tiers
    }

    /// The tier with that name.
    pub fn tier(&self, name: &str) -> Option<&Tier> {
        self.tiers.iter().find(|t| t.name == name)
    }

    /// Note a measured call against a tier. `false` when no tier has that
    /// name.
    pub fn note(&mut self, name: &str, pack: Pack, latency_ms: u32, questions: u16) -> bool {
        match self.tiers.iter_mut().find(|t| t.name == name) {
            Some(tier) => {
                tier.note(pack, latency_ms, questions);
                true
            }
            None => false,
        }
    }
}

// -------------------------------------------------------------------- policy

/// Why a `policy.toml` does not parse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyError {
    /// What went wrong.
    pub why: String,
}

impl std::fmt::Display for PolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the policy is not valid TOML for a mind: {}", self.why)
    }
}

impl std::error::Error for PolicyError {}

/// When the mind asks the talk pack (D70).
///
/// The talk pack is ten questions and most of a thought's cost, so it is
/// asked only when the agent has something to say: a change in what it
/// sees, an order it heard, a threat, or the slow timer. Every trigger is a
/// switch and the timer is a period, so a harness decides when its agents
/// speak — and the move pack, which moves the game, is asked on every think
/// either way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TalkPolicy {
    /// Ask when the world the agent sees has changed.
    pub ask_on_sight_change: bool,
    /// Ask when a line addressed to this agent is being spoken.
    pub ask_on_order: bool,
    /// Ask when a fresh threat is visible.
    pub ask_on_threat: bool,
    /// Ask when the slow timer fires.
    pub ask_on_timer: bool,
    /// The slow timer's period, in ticks.
    pub timer_period_ticks: u16,
}

impl Default for TalkPolicy {
    fn default() -> Self {
        Self {
            ask_on_sight_change: true,
            ask_on_order: true,
            ask_on_threat: true,
            ask_on_timer: true,
            timer_period_ticks: 300,
        }
    }
}

/// The thinking policy of one agent's harness (D55/D56): its tiers, the
/// moment-to-tier thresholds, and the levers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    /// The tiers a thought can run on.
    pub tiers: TierTable,
    /// A fresh threat gives the mind this many ticks.
    pub threat_horizon_ticks: u16,
    /// Quiet thinking may take up to this many ticks.
    pub max_think_ticks: u16,
    /// A sighting younger than this many ticks is a fresh threat.
    pub threat_age_ticks: u32,
    /// When the talk pack is asked (D70).
    pub talk: TalkPolicy,
    /// The gates a second opinion may be bought for (D71).
    ///
    /// `None` is every gate below `AgentsConfig::escalate_below`; `Some(set)`
    /// is only the named gates, and an empty set is none of them — the
    /// escalation off. Which gates are worth a second opinion is a harness
    /// question (the gates are the harness's); the threshold is the model's
    /// calibration and stays in the config.
    pub escalate_gates: Option<BTreeSet<String>>,
    /// The `buy_time` lever: hold or follow the safe branch while a slow
    /// thought is in flight.
    pub buy_time: bool,
    /// A thought expected to take more than this many ticks is "slow".
    pub buy_time_above_ticks: u16,
    /// The `prethink` lever: warm the verdict cache in quiet ticks.
    pub prethink: bool,
    /// The `verdict_cache` lever: cache verdicts by situation key.
    pub verdict_cache: bool,
    /// The `answer_cache` lever: answer a repeated question from the cache.
    ///
    /// Off by default. The cache is keyed by the question's id, its
    /// version and the digest of the context it was asked over, so a hit is
    /// only served when the same question is asked about the same
    /// situation — and only to the same backend, under the same confidence
    /// floor. What it buys and what it costs is measured on the pack
    /// corpus (`lm qcache`).
    pub answer_cache: bool,
    /// The `pay_priority` lever: spend extra budget to jump the queue when
    /// urgent.
    pub pay_priority: bool,
    /// The `max_questions` lever; `None` defers to `AgentsConfig`.
    pub max_questions: Option<u16>,
}

impl Default for Policy {
    fn default() -> Self {
        Policy::parse(DEFAULT_POLICY_TOML).expect("the shipped default policy parses")
    }
}

impl Policy {
    /// Parse a `policy.toml`. An empty file is the defaults (no tiers); a
    /// file that names a section leaves every unmentioned field at its
    /// default.
    pub fn parse(text: &str) -> Result<Policy, PolicyError> {
        let file: PolicyFile =
            toml::from_str(text).map_err(|e| PolicyError { why: e.to_string() })?;
        Ok(Policy {
            tiers: TierTable {
                tiers: file
                    .tiers
                    .into_iter()
                    .map(|t| Tier {
                        name: t.name,
                        backend: t.backend,
                        quality: t.quality,
                        rtt: BTreeMap::new(),
                        per_question: BTreeMap::new(),
                    })
                    .collect(),
            },
            threat_horizon_ticks: file.think.threat_horizon_ticks,
            max_think_ticks: file.think.max_think_ticks,
            threat_age_ticks: file.think.threat_age_ticks,
            talk: TalkPolicy {
                ask_on_sight_change: file.talk.ask_on_sight_change,
                ask_on_order: file.talk.ask_on_order,
                ask_on_threat: file.talk.ask_on_threat,
                ask_on_timer: file.talk.ask_on_timer,
                timer_period_ticks: file.talk.timer_period_ticks,
            },
            escalate_gates: file.escalate.gates.map(|gates| gates.into_iter().collect()),
            buy_time: file.levers.buy_time,
            buy_time_above_ticks: file.levers.buy_time_above_ticks,
            prethink: file.levers.prethink,
            verdict_cache: file.levers.verdict_cache,
            answer_cache: file.levers.answer_cache,
            pay_priority: file.levers.pay_priority,
            max_questions: file.levers.max_questions,
        })
    }

    /// The policy of a harness: the defaults when the file is missing or
    /// unparsable (a broken policy never stops the reflex from playing).
    pub fn for_harness(text: &str) -> Policy {
        if text.trim().is_empty() {
            Policy::default()
        } else {
            Policy::parse(text).unwrap_or_default()
        }
    }

    /// The questions-per-ask cap in force: the lever, or the config's.
    pub fn max_questions_of(&self, cfg: &AgentsConfig) -> u16 {
        self.max_questions.unwrap_or(cfg.max_questions)
    }

    /// The tier this moment thinks on (D55): among the model tiers whose
    /// estimated p99 lands within the budget — the threat's horizon when
    /// `urgent`, `max_think_ticks` when quiet — the fastest when urgent, the
    /// best-quality when calm. `None` means no model fits: the reflex plays.
    ///
    /// The estimate is the pack's own: the move pack is one question and the
    /// talk pack is ten, and they are priced apart (D70).
    pub fn pick_tier(
        &self,
        urgent: bool,
        pack: Pack,
        questions: u16,
        tick_hz: u16,
    ) -> Option<&Tier> {
        let budget = u64::from(if urgent {
            self.threat_horizon_ticks
        } else {
            self.max_think_ticks
        });
        let mut best: Option<(&Tier, u64)> = None;
        for tier in self.tiers.tiers() {
            if tier.is_reflex() {
                continue;
            }
            let p99 = tier.p99_ticks_for(pack, questions, tick_hz);
            if p99 > budget {
                continue;
            }
            let better = match best {
                None => true,
                Some((b, bp)) => {
                    if urgent {
                        // The fastest tier that lands within the horizon;
                        // an equal pace goes to the better thinker.
                        p99 < bp || (p99 == bp && tier.quality > b.quality)
                    } else {
                        // The best-quality tier that fits the quiet budget;
                        // an equal rank goes to the faster one.
                        tier.quality > b.quality || (tier.quality == b.quality && p99 < bp)
                    }
                }
            };
            if better {
                best = Some((tier, p99));
            }
        }
        best.map(|(tier, _)| tier)
    }
}

/// The `policy.toml` schema, parsed by `toml` and mapped into [`Policy`].
///
/// Every section defaults, so a file may name only what it changes.
#[derive(Deserialize, Default)]
#[serde(default)]
struct PolicyFile {
    tiers: Vec<TierSpec>,
    think: ThinkSpec,
    levers: LeverSpec,
    talk: TalkSpec,
    escalate: EscalateSpec,
}

/// One `[[tiers]]` entry.
#[derive(Deserialize)]
struct TierSpec {
    name: String,
    backend: String,
    quality: u8,
}

/// The `[think]` section: the moment-to-tier thresholds.
#[derive(Deserialize)]
#[serde(default)]
struct ThinkSpec {
    threat_horizon_ticks: u16,
    max_think_ticks: u16,
    threat_age_ticks: u32,
}

impl Default for ThinkSpec {
    fn default() -> Self {
        ThinkSpec {
            threat_horizon_ticks: 3,
            max_think_ticks: 6,
            threat_age_ticks: 2,
        }
    }
}

/// The `[talk]` section: when the talk pack is asked (D70).
#[derive(Deserialize)]
#[serde(default)]
struct TalkSpec {
    ask_on_sight_change: bool,
    ask_on_order: bool,
    ask_on_threat: bool,
    ask_on_timer: bool,
    timer_period_ticks: u16,
}

impl Default for TalkSpec {
    fn default() -> Self {
        let talk = TalkPolicy::default();
        TalkSpec {
            ask_on_sight_change: talk.ask_on_sight_change,
            ask_on_order: talk.ask_on_order,
            ask_on_threat: talk.ask_on_threat,
            ask_on_timer: talk.ask_on_timer,
            timer_period_ticks: talk.timer_period_ticks,
        }
    }
}

/// The `[escalate]` section (D71): which gates may be escalated.
///
/// `gates` absent is every gate below `AgentsConfig::escalate_below` (the
/// escalation as it shipped); `gates = []` is none of them (the escalation
/// off); `gates = ["move"]` is the named ones only.
#[derive(Deserialize, Default)]
#[serde(default)]
struct EscalateSpec {
    gates: Option<Vec<String>>,
}

/// The `[levers]` section (D56).
#[derive(Deserialize)]
#[serde(default)]
struct LeverSpec {
    buy_time: bool,
    buy_time_above_ticks: u16,
    prethink: bool,
    verdict_cache: bool,
    answer_cache: bool,
    pay_priority: bool,
    max_questions: Option<u16>,
}

impl Default for LeverSpec {
    fn default() -> Self {
        LeverSpec {
            buy_time: true,
            buy_time_above_ticks: 4,
            prethink: true,
            verdict_cache: true,
            answer_cache: false,
            pay_priority: false,
            max_questions: None,
        }
    }
}

// ----------------------------------------------------------- thought records

/// One thought, for the log and the piano roll (D55): which tier ran it,
/// when it was asked, when it was expected to land, and — once the answer
/// returns — when it actually landed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThoughtRecord {
    /// The tier that ran the thought (`"cache"` for an answered cache hit).
    pub tier: String,
    /// The tick the question was asked.
    pub asked: Tick,
    /// The tick the answer was expected to land on.
    pub expected_land: Tick,
    /// The tick the answer landed on; `None` until it does.
    pub landed: Option<Tick>,
}

// ------------------------------------------------------------ worker protocol

/// One question pack on its way to a backend, carrying everything the
/// thinker thread needs — the sim thread never waits for the answer.
#[derive(Clone, Debug, PartialEq)]
pub struct Ask {
    /// The queue's view of the request (agent, deadlines, situation key).
    pub req: ThinkReq,
    /// Which pack this ask carries (D70).
    pub pack: Pack,
    /// The backend to ask (a tier's backend name).
    pub backend: String,
    /// The plain-language facts (D49).
    pub state: String,
    /// The typed question pack (D37).
    pub questions: Value,
    /// How many questions the pack holds (a smaller pack is a faster call).
    pub questions_n: u16,
    /// The HTTP deadline: the call never outlives it.
    pub timeout: Duration,
    /// The `pay_priority` lever's verdict for this ask.
    pub pay_priority: bool,
    /// When the mind asked, on the monotonic clock (N1).
    ///
    /// The stamp is outside the sim: it is telemetry for the split, and it
    /// never enters the sim's state — the sim only ever sees the decisions.
    pub asked_at: Instant,
    /// When the ask left the least-use-first queue (N1).
    pub sent_at: Option<Instant>,
    /// The tick this ask's decision is scheduled for (N3).
    ///
    /// The mind commits to it when it asks: the tick the answer lands on
    /// when the model is on time, plus the decision's lead. A second opinion
    /// that would land after it is no opinion at all.
    pub apply_tick: Tick,
    /// The decision's apply tick, as a wall-clock instant (N3).
    ///
    /// This is the escalation's deadline: the second opinion must be in
    /// hand by then, or the gate takes the safe branch and the round records
    /// `escalation_late`.
    pub apply_deadline: Instant,
}

/// An ask that came home: the ask, the outcome, and the measured round trip.
#[derive(Debug)]
pub struct Thought {
    /// The ask this answers.
    pub ask: Ask,
    /// The backend's answer, or its refusal.
    pub result: Result<Answers, Refusal>,
    /// The wall-clock round trip the thinker measured, in milliseconds.
    pub rtt_ms: u32,
    /// When the worker took the ask off its channel (N1).
    ///
    /// The time between the ask leaving the queue and the worker picking it
    /// up is the worker's own wait: the worker was still busy with the
    /// previous call.
    pub picked_up: Instant,
    /// When the answer came home (N1).
    pub answered_at: Instant,
}

/// How an ask ended.
///
/// Every ask ends in exactly one of these three, and the round counts them
/// apart: a question that was asked must not vanish between the mind and the
/// log. A question that was refused before it was sent, a question whose
/// answer never came back and a question that came back too late are three
/// different problems, and one counter cannot tell them apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AskOutcome {
    /// The answer came back and was taken.
    Accepted,
    /// The answer came back, but later than the tick the tier said it would
    /// land on.
    Late,
    /// The ask ended in a refusal: the call did not come back, the pack did
    /// not fit, the budget was spent, or the answer did not match.
    Refused,
}

// ----------------------------------------------------------------- the worker

/// The thinker: the threads that talk to the models.
///
/// The sim thread pushes [`Ask`]s and drains [`Thought`]s; the workers own
/// the backends and their persistent connections (D50) and run every HTTP
/// call, so the simulation never blocks on a model. Dropping the thinker
/// closes its channels and joins the threads.
///
/// There is a worker **per pack** (D70): the move pack and the talk pack
/// are separate requests, and a talk pack in flight — ten questions, up to
/// a second and a half on the Thor — must never hold the move pack back.
/// Two workers and the model's own parallel slots keep the two apart.
pub struct Thinker {
    lanes: BTreeMap<Pack, Lane>,
    sent: Arc<AtomicUsize>,
    answered: Arc<AtomicUsize>,
}

/// One worker's channel and thread.
struct Lane {
    to_worker: Option<mpsc::Sender<Ask>>,
    // Behind a lock, so the thinker can be held by the host's round
    // resource, which is shared between threads.
    from_worker: Mutex<mpsc::Receiver<Thought>>,
    handle: Option<thread::JoinHandle<()>>,
}

/// What a panic said, as text.
///
/// A payload is `&'static str` for `panic!("…")` and `String` for
/// `panic!("{}", x)`; anything else says only that it panicked. The message
/// is a *why*, not a decision: it goes into the refusal's words, where a
/// reader of the log can see it.
fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(text) = payload.downcast_ref::<&'static str>() {
        (*text).to_owned()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "the backend panicked".to_owned()
    }
}

impl Thinker {
    /// A thinker asking `backends` (by backend name) on its own threads.
    pub fn new(backends: BTreeMap<String, Box<dyn SysOne + Send + Sync>>) -> Thinker {
        let sent = Arc::new(AtomicUsize::new(0));
        let answered = Arc::new(AtomicUsize::new(0));
        let backends = Arc::new(backends);
        let mut lanes = BTreeMap::new();
        for pack in [Pack::Move, Pack::Talk] {
            let (to_worker, worker_rx) = mpsc::channel::<Ask>();
            let (worker_tx, from_worker) = mpsc::channel::<Thought>();
            let worker_answered = Arc::clone(&answered);
            let worker_backends = Arc::clone(&backends);
            let handle = thread::Builder::new()
                .name(format!("lm-thinker-{}", pack.as_str()))
                .spawn(move || {
                    // One thread per pack, every HTTP call, one persistent
                    // connection per backend (D50). The loop ends when the
                    // thinker is dropped.
                    for ask in worker_rx {
                        // When the worker takes the ask off its channel: the
                        // wait before this is the worker's own queue (N1).
                        let picked_up = Instant::now();
                        let started = picked_up;
                        let deadline = started + ask.timeout;
                        let result = match worker_backends.get(&ask.backend) {
                            Some(backend) => {
                                // A panic is a bug in the backend, but the
                                // question still has to end somewhere (N4):
                                // caught here, it becomes a named refusal,
                                // and the worker keeps serving the lane. Left
                                // alone it would kill the thread with the
                                // ask still in its hands — a question that
                                // vanished without a word.
                                //
                                // The ask is asked with the decision's own
                                // apply tick as the escalation's deadline
                                // (N3): a second opinion that would land
                                // after the decision is no opinion at all.
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    backend.ask_with_escalation(
                                        &ask.state,
                                        &ask.questions,
                                        deadline,
                                        ask.apply_deadline,
                                    )
                                }))
                                .unwrap_or_else(|payload| {
                                    Err(Refusal::Panicked(panic_message(&payload)))
                                })
                            }
                            None => Err(Refusal::Malformed(format!(
                                "no backend named {}",
                                ask.backend
                            ))),
                        };
                        let rtt_ms =
                            u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX);
                        // When the answer came home (N1).
                        let answered_at = Instant::now();
                        if worker_tx
                            .send(Thought {
                                ask,
                                result,
                                rtt_ms,
                                picked_up,
                                answered_at,
                            })
                            .is_err()
                        {
                            break;
                        }
                        worker_answered.fetch_add(1, Ordering::SeqCst);
                    }
                })
                .expect("the thinker thread starts");
            lanes.insert(
                pack,
                Lane {
                    to_worker: Some(to_worker),
                    from_worker: Mutex::new(from_worker),
                    handle: Some(handle),
                },
            );
        }
        Thinker {
            lanes,
            sent,
            answered,
        }
    }

    /// Send an ask to the worker that owns its pack.
    ///
    /// `false` means the ask never reached a worker: the lane is gone, or the
    /// worker is not listening any more. The question ends there, and the
    /// caller has to say so — a question that was asked and never sent must
    /// not disappear without a word.
    ///
    /// The ask goes to its pack's worker (D70), so a talk pack in flight
    /// never delays the move pack.
    ///
    /// The ask is stamped here: this is the moment it leaves the
    /// least-use-first queue, and the wait before it is the queue's own
    /// (N1).
    pub fn send(&self, mut ask: Ask) -> bool {
        ask.sent_at = Some(Instant::now());
        if let Some(lane) = self.lanes.get(&ask.pack) {
            if let Some(sender) = &lane.to_worker {
                if sender.send(ask).is_ok() {
                    self.sent.fetch_add(1, Ordering::SeqCst);
                    return true;
                }
            }
        }
        false
    }

    /// Every thought that has come home since the last drain.
    pub fn drain(&self) -> Vec<Thought> {
        let mut out = Vec::new();
        for lane in self.lanes.values() {
            let from_worker = lane
                .from_worker
                .lock()
                .expect("the thinker's inbox is not poisoned");
            while let Ok(thought) = from_worker.try_recv() {
                out.push(thought);
            }
        }
        out
    }

    /// How many asks were sent.
    pub fn sent(&self) -> usize {
        self.sent.load(Ordering::SeqCst)
    }

    /// How many asks were answered.
    pub fn answered(&self) -> usize {
        self.answered.load(Ordering::SeqCst)
    }

    /// Asks sent but not yet answered.
    pub fn pending(&self) -> usize {
        self.sent().saturating_sub(self.answered())
    }
}

impl Drop for Thinker {
    fn drop(&mut self) {
        // Close the channels first, then join: the workers end their loops
        // once the senders are gone, so the joins always return.
        for lane in self.lanes.values_mut() {
            lane.to_worker = None;
        }
        for lane in self.lanes.values_mut() {
            if let Some(handle) = lane.handle.take() {
                let _ = handle.join();
            }
        }
    }
}

// -------------------------------------------------------------------- the mind

/// The key of what the agent sees (D70).
///
/// The enemies it knows of, the allies in range, the beacon holders and
/// whether it is stunned — *who* and *what*, never *where*: the agent's
/// own walk is not a change of sight. It changes where the agent is, not
/// what is there, so a mind that only walked has nothing new to say and
/// the talk pack is not asked again.
fn sight_key(view: &ScoreView, cfg: &AgentsConfig) -> u64 {
    let mut h = Fnv64::new();
    for enemy in &view.enemies {
        h.i64(i64::from(enemy.id));
    }
    for ally in &view.squad {
        if ally.id == view.me.id {
            continue;
        }
        if cells_between(view.me.cell, ally.cell) > i64::from(cfg.ally_range_cells) {
            continue;
        }
        h.i64(i64::from(ally.id));
    }
    for beacon in &view.beacons {
        h.i64(beacon.holder.map_or(-1, i64::from));
    }
    h.i64(i64::from(view.me.stunned));
    h.finish()
}

/// The distance between two cells, in cells (Chebyshev).
fn cells_between(a: lm_core::world::CellXY, b: lm_core::world::CellXY) -> i64 {
    let dx = (i32::from(a.x) - i32::from(b.x)).abs();
    let dy = (i32::from(a.y) - i32::from(b.y)).abs();
    i64::from(dx.max(dy))
}

/// One thought in the mind's hands: queued (not yet sent) or in flight.
#[derive(Clone, Debug)]
struct Flight {
    /// The queue request this flight answers.
    req: ThinkReq,
    /// The menu the question was asked over — the answer is mapped onto
    /// exactly this menu, never the current one.
    menu: Menu,
    /// The tier that runs it.
    tier: String,
    /// The tick the answer was expected to land on (D55).
    expected_land: Tick,
    /// The facts the question was asked over.
    ///
    /// The wording of a line is written over the same situation the
    /// choice was made over, so the line and the choice agree.
    facts: Facts,
    /// The coarse bucket of the same situation (the surprise signal): the
    /// answer is counted under it when it comes home.
    situation: SituationKey,
    /// How many questions the pack held.
    ///
    /// The utterance needs the whole talk pack; a pack that was cut short
    /// says nothing about the line, and the agent stays quiet.
    questions_n: u16,
    /// The tick the reply is to be applied on, when it was pre-drafted
    /// (D31): the tick after the line it answers ends.
    pre_draft_at: Option<Tick>,
}

/// One pack's lane in the mind: the request waiting for the queue and the
/// flight the worker holds (D70).
///
/// The two packs are two lanes because they are two requests: a talk pack
/// in flight never holds the move pack back, and the move pack never holds
/// the talk pack back.
#[derive(Clone, Debug, Default)]
struct PackLane {
    /// The request waiting for the queue, with the ask that goes with it.
    pending: Option<(Flight, Ask)>,
    /// The flight the worker is holding.
    in_flight: Option<Flight>,
}

/// One agent's cognition loop.
pub struct Mind {
    agent: u16,
    role: Role,
    squad: u8,
    cfg: AgentsConfig,
    params: Params,
    gates: GateSet,
    policy: Policy,
    /// The shared target the facts builder states (off by default).
    shared: SharedConfig,
    mode: Mode,
    now: Tick,
    last_ask: Option<Tick>,
    last_asked_key: Option<u64>,
    last_tier_name: Option<String>,
    /// The move pack's lane (D70).
    move_lane: PackLane,
    /// The talk pack's lane (D70).
    talk_lane: PackLane,
    /// The tick the talk pack was last asked on.
    last_talk_ask: Option<Tick>,
    /// The sight key of the last talk ask: what the agent saw then (D70).
    last_sight_key: Option<u64>,
    /// The verdicts the cache holds, by pack, situation key and menu.
    cache: BTreeMap<(Pack, u64, u64), Answers>,
    ready: Vec<Decision>,
    thoughts: Vec<ThoughtRecord>,
    refusals: Vec<(Tick, Refusal)>,
    /// The escalations of this round (D71): the gate, both confidences and
    /// the latency of every second opinion that was asked for.
    escalations: Vec<(Tick, Escalation)>,
    /// The surprise table of this round: every answer that came home,
    /// counted under the coarse bucket of the situation it was asked in.
    surprise: RoundSurprise,
    seq_counter: u32,
    /// The round's roster: who exists, so a line may not name an agent
    /// that is not in the round (D37).
    roster: Vec<RosterEntry>,
    /// The round's talk rules: the airtime budget every agent shares.
    talk: TalkParams,
    /// How the lines are worded (Task 4).
    realizer: Box<dyn Realizer>,
    /// The airtime this mind has committed, in ticks.
    ///
    /// The mind checks the budget before it submits a line, so the sim's
    /// backstop is rarely needed. The sim is still the authority: it
    /// counts what it applied, and it refuses what does not fit.
    spoken_ticks: u32,
    /// The line that was pre-drafted for a reply (D31), by its id.
    pre_drafted: Option<u64>,
    /// What the wording did, in numbers: how many lines the model worded,
    /// how many the phrasebook worded, and how many the model missed its
    /// deadline on.
    words: WordCounts,
}

impl Mind {
    /// A mind for `agent` playing `role`, thinking with the gates and policy
    /// of `harness` under `cfg`.
    ///
    /// The round's [`Params`] are not part of the harness; set them with
    /// [`Mind::with_params`] when the round starts (a mind built without
    /// them uses the parameter defaults).
    pub fn new(agent: u16, role: Role, harness: &Harness, cfg: &AgentsConfig) -> Mind {
        // A harness whose gates do not load is a refusal the round can
        // see, not a silent agent that never thinks: the gates are the
        // questions, and a question that does not fit the API's schema
        // is refused by the model anyway (D26).
        let (gates, mut refusals) = match GateSet::parse(&harness.gates_toml) {
            Ok(gates) => (gates, Vec::new()),
            Err(error) => (
                GateSet::default(),
                vec![(
                    0,
                    Refusal::Malformed(format!("the harness gates do not load: {error}")),
                )],
            ),
        };
        let phrasebook = Phrasebook::parse(&harness.phrasebook_toml).unwrap_or_default();
        // The shared target is a harness parameter like the gates are: a
        // harness whose facts file does not load, or asks for a target the
        // facts cannot state, is a refusal the round can see — not a silent
        // agent that states the wrong target (D26).
        let (shared, shared_refusal) = match SharedConfig::parse(&harness.facts_toml)
            .map_err(|error| format!("the harness facts do not load: {error}"))
            .and_then(|shared| {
                shared
                    .validate()
                    .map_err(|error| format!("the harness facts cannot be stated: {error}"))?;
                Ok(shared)
            }) {
            Ok(shared) => (shared, None),
            Err(error) => (SharedConfig::default(), Some(Refusal::Malformed(error))),
        };
        if let Some(refusal) = shared_refusal {
            refusals.push((0, refusal));
        }
        Mind {
            agent,
            role,
            squad: 0,
            cfg: cfg.clone(),
            params: Params::default(),
            gates,
            policy: Policy::for_harness(&harness.policy_toml),
            shared,
            mode: Mode::parse(&cfg.mode),
            now: 0,
            last_ask: None,
            last_asked_key: None,
            last_tier_name: None,
            move_lane: PackLane::default(),
            talk_lane: PackLane::default(),
            last_talk_ask: None,
            last_sight_key: None,
            cache: BTreeMap::new(),
            ready: Vec::new(),
            thoughts: Vec::new(),
            refusals,
            escalations: Vec::new(),
            surprise: RoundSurprise::default(),
            seq_counter: 0,
            roster: Vec::new(),
            talk: TalkParams::default(),
            realizer: pick_realizer_with(cfg, &phrasebook),
            spoken_ticks: 0,
            pre_drafted: None,
            words: WordCounts::default(),
        }
    }

    /// Pin the round's parameters (tick rate, lead bounds, stale window).
    pub fn with_params(mut self, params: &Params) -> Self {
        self.params = *params;
        self
    }

    /// Pin the round's roster: who is in the round.
    ///
    /// A line may not name an agent that is not in the round, so the mind
    /// needs to know who exists (D37).
    pub fn with_roster(mut self, roster: &[RosterEntry]) -> Self {
        self.roster = roster.to_vec();
        self
    }

    /// Pin the round's talk rules: the airtime budget every agent shares.
    pub fn with_talk(mut self, talk: &TalkParams) -> Self {
        self.talk = talk.clone();
        self
    }

    /// Pin the realizer: how this mind's lines are worded.
    ///
    /// The harness's phrasebook and the configuration's `utter.realizer`
    /// choose one by default; this is the seam for a host that has its own
    /// wording, and for the tests, which must not reach the network.
    pub fn with_realizer(mut self, realizer: Box<dyn Realizer>) -> Self {
        self.realizer = realizer;
        self
    }

    /// Set the realizer: how this mind's lines are worded.
    ///
    /// [`Mind::with_realizer`] is the builder's version of this; this one
    /// replaces the realizer of a mind that is already in a round, which
    /// is what a caller that watches the wording needs.
    pub fn set_realizer(&mut self, realizer: Box<dyn Realizer>) {
        self.realizer = realizer;
    }

    /// The tick a reply to a line now being spoken should be applied on
    /// (D31): the tick after the line ends.
    ///
    /// The reply is drafted while the line is still being spoken, so it is
    /// on the runway when the line ends instead of a full thought later.
    /// A line is drafted once: while the same line is still going, no
    /// second reply is drafted for it.
    fn pre_draft(&self, view: &ScoreView) -> Option<(u64, Tick)> {
        let heard = view
            .heard
            .iter()
            .filter(|heard| heard.ongoing && heard.addressee == Addressee::Agent(self.agent))
            .min_by_key(|heard| heard.ended_tick)?;
        if self.pre_drafted == Some(heard.id) {
            return None;
        }
        Some((heard.id, heard.ended_tick.saturating_add(1)))
    }

    /// The agent this mind thinks for.
    pub fn agent(&self) -> u16 {
        self.agent
    }

    /// The agent's role.
    pub fn role(&self) -> Role {
        self.role
    }

    /// The configured mode (D38).
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The mode in force right now: a fixed mode is itself; `auto` resolves
    /// to presence while the batch p99 of the tier it would pick plus the
    /// transit margin fits the presence period, and to point mode otherwise
    /// (D38).
    pub fn effective_mode(&self) -> Mode {
        match self.mode {
            Mode::Point | Mode::Presence => self.mode,
            Mode::Auto => {
                // D38: presence while the measured batch p99 plus the
                // transit margin fits the presence period, else point.
                let period_ms = 1_000 / u64::from(self.cfg.presence_hz.max(1));
                let questions = self.pack_len();
                let batch_p99 =
                    match self
                        .policy
                        .pick_tier(false, Pack::Move, questions, self.params.tick_hz)
                    {
                        Some(tier) => u64::from(tier.p99_ms_for(Pack::Move, questions)),
                        // No model fits the quiet budget at all: no batch to
                        // keep in the present tense.
                        None => u64::from(u32::MAX),
                    };
                let transit = u64::from(self.params.lead_margin_ms);
                // The presence mode asks every mind at `presence_hz`, and
                // the minds share one model: the mode is only affordable
                // when the whole match's demand fits the model's second.
                // One mind alone would pass the test above — the move pack
                // is that cheap now (D70) — and eight of them would not.
                let minds = u64::try_from(self.roster.len().max(1)).unwrap_or(u64::MAX);
                let demand = batch_p99
                    .saturating_mul(minds)
                    .saturating_mul(u64::from(self.cfg.presence_hz.max(1)));
                if batch_p99 + transit < period_ms && demand <= 1_000 {
                    Mode::Presence
                } else {
                    Mode::Point
                }
            }
        }
    }

    /// Observe the tick and decide whether to think — the **move pack**
    /// (D70).
    ///
    /// The move pack holds the questions whose answer is the agent's move,
    /// and it is asked on every think: the think rate is what limits it,
    /// and the verdict cache answers a situation the model has already seen
    /// without asking again (D56). The talk pack is asked on its own
    /// triggers ([`Mind::on_tick_talk`]).
    ///
    /// Returns the [`ThinkReq`] for the least-use-first queue when the
    /// policy fires and `None` when the moment does not deserve a model (or
    /// no model tier lands in time, and the reflex plays it). The question
    /// pack itself waits in the mind until the queue hands the request back
    /// ([`Mind::take_flight`]).
    pub fn on_tick(
        &mut self,
        now: Tick,
        view: &ScoreView,
        world: &World,
        menu: &Menu,
    ) -> Option<ThinkReq> {
        self.now = now;
        self.squad = view.squad_id;
        // One thought at a time on this lane: the worker owns this mind's
        // model turn until the answer comes home.
        if self.move_lane.in_flight.is_some() {
            return None;
        }
        // The think rate gates everything, facts included: a mind that
        // cannot ask this tick has no use for a situation summary.
        let mode = self.effective_mode();
        let rate_hz = match mode {
            Mode::Presence => self.cfg.presence_hz,
            Mode::Point | Mode::Auto => {
                if self.role == Role::Leader {
                    self.cfg.think_hz_leader
                } else {
                    self.cfg.think_hz_member
                }
            }
        };
        let period = period_ticks(self.params.tick_hz, rate_hz);
        if self
            .last_ask
            .is_some_and(|asked| now.saturating_sub(asked) < period)
        {
            return None;
        }

        let facts = Facts::from_view(view, world, view.squad_id, &self.cfg, &self.shared);
        let situation = SituationKey::of(view, &facts, &self.params);
        let fresh_threat = view
            .enemies
            .iter()
            .any(|e| e.age_ticks <= self.policy.threat_age_ticks);
        let questions_n = self.gates.move_len();
        if questions_n == 0 {
            // No move gates to ask: nothing to think about.
            return None;
        }

        // The verdict cache answers a returning situation on the spot.
        if self.policy.verdict_cache && self.cached(Pack::Move, facts.key, menu.id) {
            let answers = self
                .cache
                .get(&(Pack::Move, facts.key, menu.id))
                .cloned()
                .expect("cached said it was there");
            let p99 = self
                .policy
                .tiers
                .tier(&self.last_tier().unwrap_or_default())
                .map_or(0, |tier| tier.p99_ms(Pack::Move));
            let lead = lead_ticks(&self.params, p99);
            let mut decisions = answers_to_decisions(
                &self.gates,
                Pack::Move,
                &answers,
                menu,
                self.agent,
                now,
                lead,
                facts.key,
            )
            .unwrap_or_else(|refusal| {
                self.refusals.push((now, refusal));
                Vec::new()
            });
            self.restamp(&mut decisions);
            self.ready.extend(decisions);
            self.thoughts.push(ThoughtRecord {
                tier: "cache".to_owned(),
                asked: now,
                expected_land: now,
                landed: Some(now),
            });
            self.last_asked_key = Some(facts.key);
            self.last_ask = Some(now);
            return None;
        }

        let tier =
            match self
                .policy
                .pick_tier(fresh_threat, Pack::Move, questions_n, self.params.tick_hz)
            {
                Some(tier) => tier,
                None => {
                    // No model lands in time: the reflex plays the moment, and
                    // the situation is spent (D26 — never a guessed answer).
                    self.last_asked_key = Some(facts.key);
                    self.last_ask = Some(now);
                    return None;
                }
            };
        let p99_ticks = tier
            .p99_ticks_for(Pack::Move, questions_n, self.params.tick_hz)
            .max(1);
        let expected_land = now.saturating_add(p99_ticks);
        let req = ThinkReq {
            agent: self.agent,
            squad: view.squad_id,
            pack: Pack::Move,
            asked: now,
            deadline: now.saturating_add(u64::from(self.params.stale_window_ticks)),
            situation_key: facts.key,
        };
        let asked_at = Instant::now();
        let (apply_tick, apply_deadline) =
            self.commit_apply(now, expected_land, tier.p99_ms(Pack::Move), asked_at);
        let ask = Ask {
            req: req.clone(),
            pack: Pack::Move,
            backend: tier.backend().to_owned(),
            state: facts.text.clone(),
            questions: self.gates.move_questions(menu, questions_n),
            questions_n,
            timeout: Duration::from_millis(u64::from(self.cfg.request_timeout_ms)),
            pay_priority: self.policy.pay_priority && fresh_threat,
            asked_at,
            sent_at: None,
            apply_tick,
            apply_deadline,
        };
        let flight = Flight {
            req: req.clone(),
            menu: menu.clone(),
            tier: tier.name().to_owned(),
            expected_land,
            facts: facts.clone(),
            situation,
            questions_n,
            pre_draft_at: None,
        };
        self.last_tier_name = Some(tier.name().to_owned());
        self.move_lane.pending = Some((flight, ask));
        self.last_asked_key = Some(facts.key);
        self.last_ask = Some(now);
        Some(req)
    }

    /// Observe the tick and decide whether to ask the **talk pack** (D70).
    ///
    /// The talk pack holds the gates a line is chosen by, and it is most of
    /// a thought's cost, so it is asked only when the agent has something to
    /// say: a change in what it sees, an order it heard, a threat, or the
    /// slow timer. Every trigger is a switch in `policy.toml`, and the timer
    /// is a period (D70).
    ///
    /// The talk pack is a request of its own: it never waits behind the move
    /// pack, and the move pack never waits behind it.
    pub fn on_tick_talk(
        &mut self,
        now: Tick,
        view: &ScoreView,
        world: &World,
        menu: &Menu,
    ) -> Option<ThinkReq> {
        self.now = now;
        self.squad = view.squad_id;
        // One thought at a time on this lane.
        if self.talk_lane.in_flight.is_some() {
            return None;
        }
        let questions_n = self.gates.talk_len();
        if questions_n == 0 {
            // No talk gates to ask: this mind has nothing to say.
            return None;
        }
        // D31: a line that is still being spoken and names this agent is
        // answered at once: the reply is drafted now and applied when the
        // line ends.
        let pre_draft = self.pre_draft(view);
        let pre_draft_at = pre_draft.map(|(_, at)| at);
        let talk = self.policy.talk;
        let fresh_threat = view
            .enemies
            .iter()
            .any(|e| e.age_ticks <= self.policy.threat_age_ticks);
        // The slow timer: the backstop that has the agent look again even
        // when nothing changed.
        let timer_fired = talk.ask_on_timer
            && self.last_talk_ask.is_none_or(|asked| {
                now.saturating_sub(asked) >= u64::from(talk.timer_period_ticks)
            });
        // A change in what the agent sees.
        let sight_changed =
            talk.ask_on_sight_change && self.last_sight_key != Some(sight_key(view, &self.cfg));
        let want = timer_fired
            || (talk.ask_on_order && pre_draft_at.is_some())
            || (talk.ask_on_threat && fresh_threat)
            || sight_changed;
        if !want {
            return None;
        }

        let facts = Facts::from_view(view, world, view.squad_id, &self.cfg, &self.shared);
        let situation = SituationKey::of(view, &facts, &self.params);
        let tier =
            match self
                .policy
                .pick_tier(fresh_threat, Pack::Talk, questions_n, self.params.tick_hz)
            {
                Some(tier) => tier,
                None => {
                    // No model lands in time for a ten-question pack: the
                    // reflex plays the moment (D26 — never a guessed answer).
                    self.last_talk_ask = Some(now);
                    return None;
                }
            };
        let p99_ticks = tier
            .p99_ticks_for(Pack::Talk, questions_n, self.params.tick_hz)
            .max(1);
        let expected_land = now.saturating_add(p99_ticks);
        let req = ThinkReq {
            agent: self.agent,
            squad: view.squad_id,
            pack: Pack::Talk,
            asked: now,
            deadline: now.saturating_add(u64::from(self.params.stale_window_ticks)),
            situation_key: facts.key,
        };
        let asked_at = Instant::now();
        let (apply_tick, apply_deadline) =
            self.commit_apply(now, expected_land, tier.p99_ms(Pack::Talk), asked_at);
        let ask = Ask {
            req: req.clone(),
            pack: Pack::Talk,
            backend: tier.backend().to_owned(),
            state: facts.text.clone(),
            questions: self.gates.talk_questions(menu, questions_n),
            questions_n,
            timeout: Duration::from_millis(u64::from(self.cfg.request_timeout_ms)),
            pay_priority: self.policy.pay_priority && fresh_threat,
            asked_at,
            sent_at: None,
            apply_tick,
            apply_deadline,
        };
        let flight = Flight {
            req: req.clone(),
            menu: menu.clone(),
            tier: tier.name().to_owned(),
            expected_land,
            facts: facts.clone(),
            situation,
            questions_n,
            pre_draft_at,
        };
        // The line is drafted now, so it is not drafted again while it is
        // still going (D31).
        if let Some((id, _)) = pre_draft {
            self.pre_drafted = Some(id);
        }
        self.last_tier_name = Some(tier.name().to_owned());
        self.talk_lane.pending = Some((flight, ask));
        self.last_talk_ask = Some(now);
        self.last_sight_key = Some(sight_key(view, &self.cfg));
        Some(req)
    }

    /// Hand the queued request to the worker: the ask leaves with it and the
    /// flight is in the mind's hands until the answer returns.
    ///
    /// `None` when the mind holds no such queued request (it was replaced by
    /// a newer one); the request is then simply never sent.
    pub fn take_flight(&mut self, req: &ThinkReq) -> Option<Ask> {
        let lane = match req.pack {
            Pack::Move => &mut self.move_lane,
            Pack::Talk => &mut self.talk_lane,
        };
        let (flight, ask) = lane.pending.take()?;
        if flight.req != *req {
            // A newer ask replaced this one before the queue got to it.
            lane.pending = Some((flight, ask));
            return None;
        }
        // D55: every request records its tier, its ask tick and its expected
        // landing; the landing tick is filled in when the answer returns.
        self.thoughts.push(ThoughtRecord {
            tier: flight.tier.clone(),
            asked: flight.req.asked,
            expected_land: flight.expected_land,
            landed: None,
        });
        lane.in_flight = Some(flight);
        Some(ask)
    }

    /// Take an answer home: note the landing, teach the tier's estimator,
    /// and turn the answer into decisions scheduled `lead` ticks out (the
    /// lead from the tier's measured p99, D44).
    ///
    /// A refusal produces no decision — it is logged, and the reflex keeps
    /// playing (D26). An answer that matches no flight in the mind's hands
    /// is dropped, and the drop is a refusal with a name.
    ///
    /// The outcome says how the ask ended, so the round's counters can add
    /// up: every ask ends in exactly one of accepted, late or refused.
    pub fn on_answer(
        &mut self,
        req: Ask,
        result: Result<Answers, Refusal>,
    ) -> (Vec<Decision>, AskOutcome) {
        let now = self.now;
        let pack = req.pack;
        let lane = match pack {
            Pack::Move => &mut self.move_lane,
            Pack::Talk => &mut self.talk_lane,
        };
        let Some(flight) = lane.in_flight.take() else {
            // Nothing was in flight for this pack: the answer belongs to an
            // ask this mind no longer owns. The ask ends here, and it ends
            // with a name — a lost answer is a refusal, not a silence.
            self.refusals.push((now, Refusal::Superseded));
            return (Vec::new(), AskOutcome::Refused);
        };
        if flight.req.agent != req.req.agent
            || flight.req.asked != req.req.asked
            || flight.req.situation_key != req.req.situation_key
        {
            // The answer belongs to an ask this mind no longer owns; the
            // flight goes back in its hands. The ask ends here, and it ends
            // with a name.
            lane.in_flight = Some(flight);
            self.refusals.push((now, Refusal::Superseded));
            return (Vec::new(), AskOutcome::Refused);
        }
        // The answer came back after the tick the tier said it would land on:
        // the ask ended late, and the round counts it apart.
        let late = now > flight.expected_land;
        if let Some(record) =
            self.thoughts.iter_mut().rev().find(|r| {
                r.landed.is_none() && r.asked == flight.req.asked && r.tier == flight.tier
            })
        {
            record.landed = Some(now);
        }
        // The surprise signal: the answer is counted under the coarse bucket
        // of the situation it was asked in, from the answer alone — no clock,
        // no latency, nothing the same round would not say again.
        self.surprise.record(flight.situation, &result);
        // Teach the tier: a refusal burned its deadline, so it counts as the
        // timeout it cost (a slow tier must look slow to the next choice).
        let rtt_ms = match &result {
            Ok(answers) => answers.latency_ms,
            Err(_) => u32::try_from(req.timeout.as_millis()).unwrap_or(u32::MAX),
        };
        self.policy
            .tiers
            .note(&flight.tier, pack, rtt_ms, req.questions_n);
        match result {
            Err(refusal) => {
                self.refusals.push((now, refusal));
                (Vec::new(), AskOutcome::Refused)
            }
            Ok(answers) => {
                // The confidence rule, per gate: a gate the backend was not
                // sure about makes no decision of its own. The gate is not
                // in the answers any more, so a gate that chose a line
                // silences the line and leaves the move alone, and a gate
                // that chose a move refuses the move and leaves the line
                // alone (D26/D49). Every one of them is a refusal, and a
                // refusal is logged.
                //
                // The refusal is named after the rule that actually failed.
                // A gate the second opinion could not answer is not a gate
                // the backend was unsure about: it is a gate whose second
                // opinion timed out, or whose second model could not be
                // reached, and those are different problems with different
                // fixes (D71).
                for (gate, confidence) in &answers.unsure {
                    let refusal = match answers.escalated.get(gate) {
                        // The second opinion missed the decision's apply tick
                        // (N3): the gate took the safe branch, and the round
                        // says why, by the name N2 reserved.
                        Some(escalation) => match escalation.outcome {
                            EscalationOutcome::Late => Refusal::EscalationLate,
                            _ => match escalation.refusal.clone() {
                                // The second opinion failed, and it says how:
                                // a timeout is a timeout, and an endpoint
                                // that answered an error is not a call that
                                // never reached one.
                                Some(refusal) => refusal,
                                None => match escalation.outcome {
                                    EscalationOutcome::Timeout => Refusal::Timeout,
                                    _ => Refusal::LowConfidence(*confidence),
                                },
                            },
                        },
                        // No second opinion was asked: the backend was
                        // simply unsure, and that is what failed.
                        None => Refusal::LowConfidence(*confidence),
                    };
                    self.refusals.push((now, refusal));
                }
                // The escalations (D71): the gate, both confidences and the
                // latency of every second opinion that was asked for.
                for escalation in answers.escalated.values() {
                    self.escalations.push((now, escalation.clone()));
                }
                // The verdict cache holds the move's verdict: the talk pack
                // is asked on its own triggers, and a line is not repeated
                // out of a cache.
                if self.policy.verdict_cache && pack == Pack::Move {
                    self.cache.insert(
                        (pack, flight.req.situation_key, flight.menu.id),
                        answers.clone(),
                    );
                }
                let p99 = self
                    .policy
                    .tiers
                    .tier(&flight.tier)
                    .map_or(0, |tier| tier.p99_ms(pack));
                let lead = lead_ticks(&self.params, p99);
                let mut decisions = answers_to_decisions(
                    &self.gates,
                    pack,
                    &answers,
                    &flight.menu,
                    self.agent,
                    now,
                    lead,
                    flight.req.situation_key,
                )
                .unwrap_or_else(|refusal| {
                    self.refusals.push((now, refusal));
                    Vec::new()
                });
                // The line, if the gates chose one and the wording held
                // (Task 4). Only the talk pack chooses a line: the move
                // pack has no line gates in it (D70).
                if pack == Pack::Talk {
                    if let Some(line) = self.utterance(&answers, &flight, now, lead) {
                        decisions.push(line);
                    }
                }
                self.restamp(&mut decisions);
                (
                    decisions,
                    if late {
                        AskOutcome::Late
                    } else {
                        AskOutcome::Accepted
                    },
                )
            }
        }
    }

    /// The line this mind says, if it says one (Task 4).
    ///
    /// The gates chose it and the realizer worded it; the score is the
    /// line the simulation will play. Everything that can go wrong is a
    /// refusal with a name, and a refusal is silence: no decision, no
    /// airtime spent, and the round's hash untouched (D26).
    ///
    /// * a pack that did not cover the talk gates says nothing about the
    ///   line: the agent stays quiet (no refusal: nothing was refused);
    /// * a choice the gates did not make, or made badly, is a refusal;
    /// * a presence gate of about zero is silence (D53);
    /// * a wording that does not compile is a refusal (D26);
    /// * a line that does not fit the airtime budget is not said.
    fn utterance(
        &mut self,
        answers: &Answers,
        flight: &Flight,
        now: Tick,
        lead: u16,
    ) -> Option<Decision> {
        // A harness without the talk gates has nothing to say.
        self.gates.gate(GATE_GESTURE)?;
        // A talk pack that was cut short never asked every talk gate: the
        // line is unknown, and the agent says nothing. The pack is asked in
        // file order, so a gate is in it when its index in the pack is under
        // the pack's length (D70).
        let asked = usize::from(flight.questions_n);
        let covered = UTTERANCE_GATES.iter().all(|gate| {
            self.gates
                .pack_index_of(Pack::Talk, gate)
                .is_some_and(|index| index < asked)
                && self
                    .gates
                    .pack_index_of(Pack::Talk, &presence_gate(gate))
                    .is_some_and(|index| index < asked)
        });
        if !covered {
            return None;
        }
        // A gate the model did not answer produces no line: nothing is
        // invented to fill the gap (D26), and — as in phase 3 — an
        // unanswered gate is not a refusal either.
        let answered = UTTERANCE_GATES.iter().all(|gate| {
            answers.per_gate.contains_key(*gate)
                && answers.per_gate.contains_key(&presence_gate(gate))
        });
        if !answered {
            return None;
        }
        let ctx = GateCtx {
            agent: self.agent,
            roster: &self.roster,
        };
        let plan = match plan_from_gates(answers, &flight.menu, &ctx) {
            Ok(plan) => plan,
            Err(refusal) => {
                self.refusals.push((now, refusal));
                return None;
            }
        };
        // Silence is an outcome (D53): the presence gate said this is not
        // even here, and nothing is said at all.
        if !plan.should_speak {
            return None;
        }
        // The airtime budget, checked before the line is submitted (D31).
        // The sim is the authority and refuses what does not fit; the mind
        // checks first so the refusal is rare.
        let budget = self.airtime_budget_ticks();
        if u64::from(self.spoken_ticks) >= budget {
            return None;
        }
        let worded = match self.realizer.realize(&plan, &flight.facts) {
            Ok(worded) => worded,
            Err(refusal) => {
                self.refusals.push((now, refusal));
                return None;
            }
        };
        // The wording is counted where it happens: the round's numbers say
        // which realizer wrote the lines, and how many lines the model
        // missed its deadline on.
        match worded.source {
            WordSource::Llm => self.words.llm += 1,
            WordSource::Phrasebook => self.words.phrasebook += 1,
        }
        if worded.late {
            self.words.late += 1;
        }
        let text = worded.text;
        let talk_ctx = TalkContext {
            speaker: self.agent,
            roster: &self.roster,
        };
        let utterance = match utterance_from_text(&talk_ctx, &plan, &text) {
            Ok(utterance) => utterance,
            Err(refusal) => {
                self.refusals.push((now, refusal));
                return None;
            }
        };
        // The line has to fit the budget as well: a line that would take
        // the speaker over it is not said.
        let duration = utterance.duration_ticks();
        if u64::from(self.spoken_ticks) + u64::from(duration) > budget {
            return None;
        }
        self.spoken_ticks = self.spoken_ticks.saturating_add(duration);
        Some(Decision {
            // A pre-drafted reply lands right after the line it answers
            // ends (D31); every other line lands with the rest of the
            // thought.
            apply_tick: flight
                .pre_draft_at
                .unwrap_or(now.saturating_add(u64::from(lead))),
            source: Source::Agent(self.agent),
            seq: 0,
            kind: Kind::Utterance(utterance),
            provenance: Provenance::System1 {
                model: answers.model.clone(),
                tokens: answers.input_tokens,
                latency_ms: answers.latency_ms,
                situation_key: flight.req.situation_key,
            },
        })
    }

    /// The airtime one agent may spend in a round, in ticks (D31).
    ///
    /// The budget is the round's and every agent shares it, so the mind
    /// reads it from the round's talk rules.
    fn airtime_budget_ticks(&self) -> u64 {
        u64::from(self.talk.airtime_s) * u64::from(self.params.tick_hz)
    }

    /// Decisions made without a model call this tick (verdict-cache hits),
    /// handed over once.
    pub fn take_decisions(&mut self) -> Vec<Decision> {
        std::mem::take(&mut self.ready)
    }

    /// The `buy_time` lever's verdict (D56): a thought slower than
    /// `buy_time_above_ticks` is in flight, so the agent holds or follows
    /// the safe branch — in M1 that is the reflex.
    ///
    /// The move pack's flight is the one that matters: the talk pack is a
    /// line, and the agent does not walk differently for it (D70).
    pub fn buying_time(&self) -> bool {
        let Some(flight) = &self.move_lane.in_flight else {
            return false;
        };
        self.policy.buy_time
            && flight.expected_land.saturating_sub(flight.req.asked)
                > u64::from(self.policy.buy_time_above_ticks)
    }

    /// The identity of every gate this mind asks, by slot.
    ///
    /// The identities are the harness's own: derived from the file's
    /// bytes, so the round's ledger names the questions the way the
    /// harness wrote them.
    pub fn gate_identities(&self) -> BTreeMap<String, QuestionIdentity> {
        self.gates.identities()
    }

    /// The thought records, for the log and the piano roll (D55).
    pub fn thoughts(&self) -> &[ThoughtRecord] {
        &self.thoughts
    }

    /// The refusals so far, with the tick they landed on.
    pub fn refusals(&self) -> &[(Tick, Refusal)] {
        &self.refusals
    }

    /// What this mind's wording did, in numbers.
    pub fn words(&self) -> WordCounts {
        self.words
    }

    /// Teach a tier's estimator a measured call (used to pre-seed estimates
    /// and by the tests to script them).
    pub fn note_tier(&mut self, tier: &str, pack: Pack, latency_ms: u32, questions: u16) {
        self.policy.tiers.note(tier, pack, latency_ms, questions);
    }

    /// The questions the move pack asks (D70).
    ///
    /// The pack is the gates the harness has, capped by the policy's or the
    /// configuration's `max_questions` (D56).
    pub fn pack_len(&self) -> u16 {
        self.gates
            .move_len()
            .min(self.policy.max_questions_of(&self.cfg))
    }

    /// The questions the talk pack asks (D70).
    pub fn talk_len(&self) -> u16 {
        self.gates
            .talk_len()
            .min(self.policy.max_questions_of(&self.cfg))
    }

    /// The escalations this mind paid for (D71): the gate, both confidences
    /// and the latency of every second opinion.
    pub fn escalations(&self) -> &[(Tick, Escalation)] {
        &self.escalations
    }

    /// The surprise table of this mind's round: every answer that came
    /// home, counted under its situation bucket.
    pub fn surprise(&self) -> &RoundSurprise {
        &self.surprise
    }

    /// Whether the verdict cache holds this situation on this menu.
    fn cached(&self, pack: Pack, situation_key: u64, menu_id: u64) -> bool {
        self.cache.contains_key(&(pack, situation_key, menu_id))
    }

    /// The tier of the last ask, for cache hits that follow refusals.
    fn last_tier(&self) -> Option<String> {
        self.last_tier_name.clone()
    }

    /// Re-stamp a batch of decisions with the mind's own strictly increasing
    /// sequence numbers (two calls on one tick must never share a `seq`).
    fn restamp(&mut self, decisions: &mut [Decision]) {
        for d in decisions.iter_mut() {
            self.seq_counter = self.seq_counter.saturating_add(1);
            d.seq = self.seq_counter;
        }
    }

    /// The tick this ask's decision is scheduled for, and the wall-clock
    /// instant it falls on (N1/N3).
    ///
    /// The mind commits to the tick when it asks: the tick the answer lands
    /// on when the model is on time (`expected_land`), plus the decision's
    /// lead. The second opinion that may serve that decision is fenced by
    /// it (N3), so a wait in the queue eats the second opinion's budget
    /// instead of the round's.
    fn commit_apply(
        &self,
        asked: Tick,
        expected_land: Tick,
        p99_ms: u32,
        asked_at: Instant,
    ) -> (Tick, Instant) {
        let lead = u64::from(lead_ticks(&self.params, p99_ms));
        let apply_tick = expected_land.saturating_add(lead);
        // The budget between the ask and the tick the decision is applied on,
        // in wall-clock milliseconds: the sim's ticks are wall-clock ticks.
        let budget_ms = apply_tick.saturating_sub(asked) * 1000 / u64::from(self.params.tick_hz);
        (apply_tick, asked_at + Duration::from_millis(budget_ms))
    }
}

// -------------------------------------------------------------------- the tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AgentsConfig;
    use crate::queue::{Budgets, LeastUseQueue};
    use crate::sysone::{Answer, Escalation, EscalationOutcome, StubBackend};
    use crate::utter::{AutoRealizer, LlmRealizer, StubLlm, TemplateRealizer};
    use lm_core::decision::{Kind, Source};
    use lm_core::log::{Recorder, RoundLog};
    use lm_core::sched::Sequencer;
    use lm_core::sim::Sim;
    use lm_core::utterance::Heard;
    use lm_core::view::menu;
    use lm_core::world::{CellKind, CellXY, Grid, World};
    use lm_core::{Header, MenuOption, RosterEntry};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    /// The shipped gates, the same file the default harness serves.
    const DEFAULT_GATES: &str = include_str!("../../../assets/harness/default/gates.toml");

    /// The shipped phrasebook: the wording of the lines.
    const DEFAULT_PHRASEBOOK: &str =
        include_str!("../../../assets/harness/default/phrasebook.toml");

    /// The shipped reflex, so host rounds move by reflex.
    const DEFAULT_REFLEX: &str = include_str!("../../../assets/reflexes/default.rhai");

    /// A policy over stub backends for the mind-level tests: one fast tier
    /// and one deep tier, quiet thinking up to 9 ticks, every lever on.
    const STUB_POLICY: &str = r#"
        [[tiers]]
        name = "reflex"
        backend = "reflex"
        quality = 0

        [[tiers]]
        name = "fast"
        backend = "stub"
        quality = 1

        [[tiers]]
        name = "deep"
        backend = "stub"
        quality = 2

        [think]
        threat_horizon_ticks = 3
        max_think_ticks = 9
        threat_age_ticks = 2

        [levers]
        buy_time = true
        buy_time_above_ticks = 4
        prethink = true
        verdict_cache = true
    "#;

    /// A policy for the tier-choice tests: quiet thinking up to 6 ticks.
    const TIER_POLICY: &str = r#"
        [[tiers]]
        name = "reflex"
        backend = "reflex"
        quality = 0

        [[tiers]]
        name = "fast"
        backend = "stub"
        quality = 1

        [[tiers]]
        name = "deep"
        backend = "stub"
        quality = 2

        [think]
        threat_horizon_ticks = 3
        max_think_ticks = 6
    "#;

    // ------------------------------------------------------------ the helpers

    /// A harness that serves the shipped gates, phrasebook and `policy`.
    fn harness(policy: &str) -> Harness {
        Harness {
            dir: PathBuf::from("test-harness"),
            gates_toml: DEFAULT_GATES.to_owned(),
            policy_toml: policy.to_owned(),
            phrasebook_toml: DEFAULT_PHRASEBOOK.to_owned(),
            facts_toml: String::new(),
            reflexes: BTreeMap::new(),
            commit: "test".to_owned(),
        }
    }

    /// A member mind over the stub policy.
    fn mind(agent: u16, cfg: &AgentsConfig, policy: &str) -> Mind {
        Mind::new(agent, Role::Member, &harness(policy), cfg).with_params(&Params::default())
    }

    /// A small open world: 12×12 cells, one beacon, two spawns.
    fn tiny_world() -> World {
        World {
            grid: Grid {
                w: 12,
                h: 12,
                cells: vec![CellKind::Floor; 12 * 12],
            },
            beacons: vec![CellXY { x: 9, y: 2 }],
            spawns: vec![CellXY { x: 2, y: 2 }, CellXY { x: 9, y: 9 }],
            cell_units: 1024,
            paint: lm_core::world::Paint::new(),
        }
    }

    /// A member's view standing at (x, y) on the tiny world.
    fn view_at(id: u16, x: u16, y: u16) -> ScoreView {
        ScoreView {
            paint: Vec::new(),
            heard: Vec::new(),
            tick: 0,
            me: lm_core::view::AgentSnap {
                id,
                cell: CellXY { x, y },
                role: Role::Member,
                stunned: false,
                airtime_ticks: 0,
            },
            squad: vec![lm_core::view::AgentSnap {
                id,
                cell: CellXY { x, y },
                role: Role::Member,
                stunned: false,
                airtime_ticks: 0,
            }],
            enemies: vec![],
            beacons: vec![lm_core::view::BeaconSnap {
                cell: CellXY { x: 9, y: 2 },
                holder: None,
            }],
            score: vec![0, 0],
            squad_id: 0,
        }
    }

    /// A model answer that favors option `best` of `menu`.
    fn lane_answers(menu: &Menu, best: usize) -> Answers {
        let n = menu.options.len();
        let rest = 0.3f32 / (n - 1) as f32;
        let probs: BTreeMap<String, f32> = (0..n)
            .map(|i| (format!("opt{i}"), if i == best { 0.7 } else { rest }))
            .collect();
        Answers {
            per_gate: BTreeMap::from([(
                "move".to_owned(),
                Answer::Choice {
                    probs,
                    confidence: 0.9,
                },
            )]),
            unsure: BTreeMap::new(),
            escalated: BTreeMap::new(),
            model: "stub".to_owned(),
            input_tokens: 64,
            latency_ms: 0,
            questions: BTreeMap::new(),
        }
    }

    /// The answers of a mind that says something: the move, the five talk
    /// gates and their presence gates.
    ///
    /// The presence gates are high, so the line is spoken; the gesture is
    /// a command to the squad and the subject is the beacon the menu's
    /// first option names.
    fn talk_answers(menu: &Menu, best: usize) -> Answers {
        let mut answers = lane_answers(menu, best);
        // The subject gate is asked over the same menu, so it is answered
        // over it too: the line is about the option the model picked.
        let probs = match answers.per_gate.get("move") {
            Some(Answer::Choice { probs, .. }) => probs.clone(),
            _ => BTreeMap::new(),
        };
        answers.per_gate.insert(
            "subject".to_owned(),
            Answer::Choice {
                probs,
                confidence: 0.9,
            },
        );
        answers.per_gate.insert(
            "gesture".to_owned(),
            Answer::Choice {
                probs: BTreeMap::from([
                    ("command".to_owned(), 0.8),
                    ("question".to_owned(), 0.1),
                    ("statement".to_owned(), 0.05),
                    ("callout".to_owned(), 0.05),
                ]),
                confidence: 0.9,
            },
        );
        answers.per_gate.insert(
            "attention".to_owned(),
            Answer::Choice {
                probs: BTreeMap::from([("squad".to_owned(), 0.9), ("all".to_owned(), 0.1)]),
                confidence: 0.9,
            },
        );
        answers.per_gate.insert(
            "intention".to_owned(),
            Answer::Score {
                probs: BTreeMap::from([("urgent".to_owned(), 0.8)]),
                score: 1.5,
                confidence: 0.9,
            },
        );
        answers.per_gate.insert(
            "intent".to_owned(),
            Answer::Choice {
                probs: BTreeMap::from([
                    ("order".to_owned(), 0.8),
                    ("ack".to_owned(), 0.1),
                    ("warn".to_owned(), 0.05),
                    ("query".to_owned(), 0.03),
                    ("report".to_owned(), 0.02),
                ]),
                confidence: 0.9,
            },
        );
        for gate in ["gesture", "attention", "intention", "intent", "subject"] {
            answers
                .per_gate
                .insert(format!("{gate}_present"), Answer::Noul(0.9));
        }
        answers
    }

    /// A small round: `squads` × `size` agents on a 2×2-chunk world.
    fn header(seed: u64, squads: u8, size: u8) -> Header {
        let mut h = Header::default_round(seed, DEFAULT_REFLEX);
        h.params.squads = squads;
        h.params.squad_size = size;
        h.params.world_chunks = (2, 2);
        h.params.beacons = 2;
        h.roster = (0..squads)
            .flat_map(|s| {
                (0..size).map(move |i| RosterEntry {
                    agent: u16::from(s * size + i),
                    squad: s,
                    role: if i == 0 { Role::Leader } else { Role::Member },
                    voice: "default".to_owned(),
                    harness_commit: "none".to_owned(),
                    reflex: "default".to_owned(),
                })
            })
            .collect();
        h
    }

    /// A mind-wired host: one mind per roster agent, a least-use-first
    /// queue, budgets, a sequencer, a recorder and one stub backend.
    struct Host {
        header: Header,
        sim: Sim,
        seq: Sequencer,
        minds: BTreeMap<u16, Mind>,
        queue: LeastUseQueue,
        budgets: Budgets,
        thinker: Thinker,
        recorder: Recorder,
        submitted: Vec<Decision>,
        queue_high: usize,
    }

    impl Host {
        fn new(
            header: Header,
            cfg: &AgentsConfig,
            script: Vec<(Duration, Result<Answers, Refusal>)>,
        ) -> Host {
            let sim = Sim::new(&header).expect("the round starts");
            let minds = header
                .roster
                .iter()
                .map(|entry| {
                    let mind = Mind::new(entry.agent, entry.role, &harness(STUB_POLICY), cfg)
                        .with_params(&header.params)
                        .with_roster(&header.roster)
                        .with_talk(&header.talk);
                    (entry.agent, mind)
                })
                .collect();
            let backends: BTreeMap<String, Box<dyn SysOne + Send + Sync>> = BTreeMap::from([(
                "stub".to_owned(),
                Box::new(StubBackend::scripted(script)) as Box<dyn SysOne + Send + Sync>,
            )]);
            Host {
                seq: Sequencer::new(
                    &header.params,
                    header.roster.len() as u16,
                    header.start_tick,
                ),
                recorder: Recorder::new(header.clone(), 300),
                minds,
                queue: LeastUseQueue::new(500),
                budgets: Budgets::uniform(sim.agent_ids(), cfg.budget_calls_per_round),
                thinker: Thinker::new(backends),
                sim,
                submitted: Vec::new(),
                queue_high: 0,
                header,
            }
        }

        /// One tick: minds look, the queue picks, the worker works, the
        /// answers become decisions, the sim steps.
        fn tick(&mut self) -> lm_core::sim::StepOut {
            let t = self.sim.tick();

            // 1. The minds look; some ask. The move pack and the talk pack
            //    are asked apart (D70).
            for (&agent, mind) in &mut self.minds {
                let view = self.sim.view(agent);
                let menu = menu(&view);
                if let Some(req) = mind.on_tick(t, &view, self.sim.world(), &menu) {
                    self.queue.push(req);
                }
                if let Some(req) = mind.on_tick_talk(t, &view, self.sim.world(), &menu) {
                    self.queue.push(req);
                }
            }
            self.queue_high = self.queue_high.max(self.queue.len());

            // 2. The queue picks a batch and the worker is sent it.
            let popped = self.queue.pop_batch(t, 32, &mut self.budgets);
            for req in popped.send {
                if let Some(ask) = self
                    .minds
                    .get_mut(&req.agent)
                    .and_then(|m| m.take_flight(&req))
                {
                    self.thinker.send(ask);
                }
            }

            // 3. Answers come home and become decisions. The rig waits
            //    for its worker here so an answer lands on a deterministic
            //    tick; a real host simply finds it on a later one.
            let stop = Instant::now() + Duration::from_secs(10);
            while self.thinker.pending() > 0 && Instant::now() < stop {
                thread::sleep(Duration::from_millis(1));
            }
            self.pump(t);

            // 4. The sequencer stamps; the sim steps; the recorder records.
            let stamped = self.seq.drain(t);
            let out = self.sim.step(&stamped);
            self.recorder.record(t, &stamped, out.hash);
            out
        }

        /// Drain the thinker and every mind's ready decisions into the
        /// sequencer, all arriving on tick `t`.
        fn pump(&mut self, t: Tick) {
            let mut decisions = Vec::new();
            for thought in self.thinker.drain() {
                if let Some(mind) = self.minds.get_mut(&thought.ask.req.agent) {
                    // A model answers the gates it was asked (D70): the
                    // scripted stub answers everything, so its answers are
                    // cut down to the pack the ask carried.
                    let result = thought
                        .result
                        .map(|answers| answers_asked(&thought.ask, &answers));
                    decisions.extend(mind.on_answer(thought.ask, result).0);
                }
            }
            for mind in self.minds.values_mut() {
                decisions.extend(mind.take_decisions());
            }
            for d in decisions {
                self.seq
                    .submit(d.clone(), t)
                    .expect("the sequencer takes the mind's decision");
                self.submitted.push(d);
            }
        }

        /// Play `ticks` ticks, then let the worker finish what is in flight.
        fn run(&mut self, ticks: u64) -> RoundLog {
            for _ in 0..ticks {
                self.tick();
            }
            // Pump before checking: a thought can already sit in the
            // channel while `pending` reads zero, so the exit needs a drain
            // that found nothing behind a zero pending.
            let stop = Instant::now() + Duration::from_secs(20);
            loop {
                self.pump(self.sim.tick());
                if self.thinker.pending() == 0 || Instant::now() > stop {
                    break;
                }
                thread::sleep(Duration::from_millis(2));
            }
            let recorder =
                std::mem::replace(&mut self.recorder, Recorder::new(self.header.clone(), 0));
            recorder.finish()
        }
    }

    // -------------------------------------------------- the tick arithmetic

    #[test]
    fn period_and_tick_math() {
        assert_eq!(period_ticks(30, 4), 8, "a 4 Hz think rate on 30 Hz");
        assert_eq!(period_ticks(30, 10), 3, "presence at 10 Hz");
        assert_eq!(period_ticks(30, 1), 30, "a leader's 1 Hz");
        assert_eq!(period_ticks(30, 60), 1, "faster than the sim is every tick");
        assert_eq!(ms_to_ticks_ceil(20, 30), 1);
        assert_eq!(ms_to_ticks_ceil(200, 30), 6);
        assert_eq!(ms_to_ticks_ceil(1, 30), 1, "any latency costs a tick");
    }

    // ------------------------------------------------------- the estimator

    #[test]
    fn rtt_estimator_tracks_a_window() {
        let mut r = RttEstimator::new();
        assert!(r.is_empty());
        assert_eq!((r.p50(), r.p99()), (0, 0), "nothing measured is zero");
        for i in 1..=40u32 {
            r.record(i);
        }
        // The window keeps the newest 32: 9..=40.
        assert_eq!(r.len(), RTT_WINDOW);
        assert_eq!(r.p50(), 24, "nearest-rank median of 9..=40");
        assert_eq!(r.p99(), 40, "nearest-rank p99 of a 32-window is its max");
        r.clear();
        assert!(r.is_empty());
    }

    // ----------------------------------------------------------- the policy

    #[test]
    fn policy_parses_tiers_and_thresholds() {
        let p = Policy::parse(STUB_POLICY).expect("the stub policy parses");
        let names: Vec<&str> = p.tiers.tiers().iter().map(|t| t.name()).collect();
        assert_eq!(names, vec!["reflex", "fast", "deep"]);
        assert_eq!(p.tiers.tier("deep").unwrap().backend(), "stub");
        assert_eq!(p.tiers.tier("deep").unwrap().quality(), 2);
        assert!(p.tiers.tier("reflex").unwrap().is_reflex());
        assert_eq!(
            (
                p.threat_horizon_ticks,
                p.max_think_ticks,
                p.threat_age_ticks
            ),
            (3, 9, 2)
        );
        assert!(p.buy_time && p.prethink && p.verdict_cache);
        assert_eq!(p.buy_time_above_ticks, 4);
        assert!(!p.pay_priority);
        assert_eq!(p.max_questions, None);
        assert_eq!(p.max_questions_of(&AgentsConfig::default()), 11);
    }

    #[test]
    fn default_policy_is_the_shipped_one() {
        let p = Policy::default();
        assert_eq!(
            p,
            Policy::parse(DEFAULT_POLICY_TOML).expect("the shipped policy parses")
        );
        // The shipped backends: reflex plus the two model tiers of D55.
        let backends: Vec<&str> = p.tiers.tiers().iter().map(|t| t.backend()).collect();
        assert_eq!(backends, vec!["reflex", "thor-d1-2b", "jev"]);
        // The shipped escalation set: the move gate alone (D71's accuracy
        // was measured on the move gate; the talk gates' second opinion did
        // not pay for its latency).
        assert_eq!(p.escalate_gates, Some(BTreeSet::from(["move".to_owned()])));
        // A missing or broken policy file means the defaults, never a panic.
        assert_eq!(Policy::for_harness(""), p);
        assert_eq!(Policy::for_harness("tiers = ["), p);
    }

    #[test]
    fn the_escalation_set_is_the_harness_s() {
        // A file that names the gates escalates only those; an empty list is
        // the escalation off; a file without the section escalates every
        // gate below the threshold (D71 as it shipped).
        let named = Policy::parse("[escalate]\ngates = [\"move\", \"subject\"]\n")
            .expect("the named gates parse");
        assert_eq!(
            named.escalate_gates,
            Some(BTreeSet::from(["move".to_owned(), "subject".to_owned()]))
        );
        let off = Policy::parse("[escalate]\ngates = []\n").expect("the empty set parses");
        assert_eq!(off.escalate_gates, Some(BTreeSet::new()));
        let absent = Policy::parse("").expect("the empty file parses");
        assert_eq!(absent.escalate_gates, None);
    }

    #[test]
    fn a_bad_policy_is_an_error_and_an_empty_one_has_no_tiers() {
        assert!(Policy::parse("tiers = 3\n").is_err());
        let empty = Policy::parse("").expect("an empty file is the defaults");
        assert!(empty.tiers.tiers().is_empty());
        assert!(empty.pick_tier(true, Pack::Move, 2, 30).is_none());
    }

    // ------------------------------------------------------ the tier choice

    #[test]
    fn urgent_picks_fast_tier() {
        let mut p = Policy::parse(TIER_POLICY).unwrap();
        p.tiers.note("fast", Pack::Move, 30, 3); // 30 ms = 1 tick
        p.tiers.note("deep", Pack::Move, 150, 3); // 150 ms = 5 ticks
                                                  // A fresh threat gives 3 ticks: the deep tier (5 ticks) does not
                                                  // land in time, so the fast one takes the moment.
        assert_eq!(p.pick_tier(true, Pack::Move, 3, 30).unwrap().name(), "fast");
        // Nothing lands within the horizon once both slow down: the reflex
        // plays the moment.
        for _ in 0..RTT_WINDOW {
            p.tiers.note("fast", Pack::Move, 300, 3);
        }
        assert!(p.pick_tier(true, Pack::Move, 3, 30).is_none());
    }

    #[test]
    fn calm_picks_quality_tier() {
        let mut p = Policy::parse(TIER_POLICY).unwrap();
        p.tiers.note("fast", Pack::Move, 30, 3); // 1 tick
        p.tiers.note("deep", Pack::Move, 150, 3); // 5 ticks, fits the 6-tick budget
                                                  // Quiet thinking has 6 ticks: the deep tier fits and out-ranks fast.
        assert_eq!(
            p.pick_tier(false, Pack::Move, 3, 30).unwrap().name(),
            "deep"
        );
    }

    #[test]
    fn tier_estimates_adapt() {
        // A stub that slows down shifts the choice to the faster tier.
        let mut p = Policy::parse(TIER_POLICY).unwrap();
        p.tiers.note("fast", Pack::Move, 30, 3);
        p.tiers.note("deep", Pack::Move, 150, 3);
        assert_eq!(
            p.pick_tier(false, Pack::Move, 3, 30).unwrap().name(),
            "deep"
        );
        for _ in 0..RTT_WINDOW {
            p.tiers.note("deep", Pack::Move, 300, 3); // 9 ticks: no longer fits 6
        }
        assert_eq!(
            p.pick_tier(false, Pack::Move, 3, 30).unwrap().name(),
            "fast"
        );
        // The p50 follows the window too.
        assert_eq!(p.tiers.tier("deep").unwrap().p50_ms(Pack::Move), 300);
    }

    #[test]
    fn smaller_pack_is_faster_tier() {
        // The per-question estimate scales a tier to the pack being asked
        // (D56 max_questions): the same tier fits the quiet budget with a
        // small pack and misses it with a large one.
        let mut p = Policy::parse(TIER_POLICY).unwrap();
        p.tiers.note("fast", Pack::Move, 60, 8); // ~2 ticks either way
        for _ in 0..RTT_WINDOW {
            p.tiers.note("deep", Pack::Move, 240, 8); // 30 ms per question
        }
        let deep = p.tiers.tier("deep").unwrap();
        assert_eq!(deep.p99_ms_for(Pack::Move, 8), 240);
        assert_eq!(deep.p99_ms_for(Pack::Move, 2), 60);
        assert!(deep.p99_ms_for(Pack::Move, 2) < deep.p99_ms_for(Pack::Move, 8));
        // Eight questions cost the deep tier 8 ticks: too slow for 6, so the
        // fast tier takes it. Two questions cost 2 ticks: deep is back.
        assert_eq!(
            p.pick_tier(false, Pack::Move, 8, 30).unwrap().name(),
            "fast"
        );
        assert_eq!(
            p.pick_tier(false, Pack::Move, 2, 30).unwrap().name(),
            "deep"
        );
    }

    // ------------------------------------------------------------ the modes

    #[test]
    fn auto_switches_modes() {
        // Presence at 10 Hz buys 100 ms. A 200 ms batch p99 plus the 10 ms
        // transit margin does not fit: point mode. A 20 ms one does.
        let cfg = AgentsConfig {
            mode: "auto".to_owned(),
            presence_hz: 10,
            ..AgentsConfig::default()
        };
        let mut m = mind(7, &cfg, STUB_POLICY);
        // The deep tier is the one a calm moment picks (it out-ranks fast),
        // so its estimate is the batch p99 auto resolves on.
        let questions = m.pack_len();
        m.note_tier("deep", Pack::Move, 200, questions);
        assert_eq!(m.effective_mode(), Mode::Point);
        for _ in 0..RTT_WINDOW {
            m.note_tier("deep", Pack::Move, 20, questions);
        }
        assert_eq!(m.effective_mode(), Mode::Presence);

        // A fixed mode never resolves.
        let point = mind(
            7,
            &AgentsConfig {
                mode: "point".to_owned(),
                ..AgentsConfig::default()
            },
            STUB_POLICY,
        );
        assert_eq!(point.effective_mode(), Mode::Point);
        let presence = mind(
            7,
            &AgentsConfig {
                mode: "presence".to_owned(),
                ..AgentsConfig::default()
            },
            STUB_POLICY,
        );
        assert_eq!(presence.effective_mode(), Mode::Presence);
        // An unknown config word is the documented default.
        assert_eq!(Mode::parse("nonsense"), Mode::Auto);
        assert_eq!(Mode::parse("point").as_str(), "point");
    }

    // -------------------------------------------------------- the buy lever

    #[test]
    fn buys_time_for_slow_tier() {
        let cfg = AgentsConfig::default();
        let world = tiny_world();
        let view = view_at(4, 2, 2);
        let view_menu = menu(&view);

        // A slow tier (250 ms = 8 ticks, above the 4-tick lever): while its
        // thought is in flight the mind buys time and asks nothing new. The
        // deep tier is the one a calm moment picks.
        let mut slow = mind(4, &cfg, STUB_POLICY);
        let questions = slow.pack_len();
        slow.note_tier("deep", Pack::Move, 250, questions);
        let req = slow
            .on_tick(0, &view, &world, &view_menu)
            .expect("a novel moment asks");
        slow.take_flight(&req).expect("the ask flies");
        assert!(slow.buying_time(), "8 expected ticks > the 4-tick lever");
        let elsewhere = view_at(5, 9, 9);
        let elsewhere_menu = menu(&elsewhere);
        assert!(
            slow.on_tick(1, &elsewhere, &world, &elsewhere_menu)
                .is_none(),
            "a mind buying time asks nothing"
        );

        // A fast tier (20 ms = 1 tick) does not trip the lever.
        let mut fast = mind(4, &cfg, STUB_POLICY);
        fast.note_tier("deep", Pack::Move, 20, 2);
        let req = fast
            .on_tick(0, &view, &world, &view_menu)
            .expect("a novel moment asks");
        fast.take_flight(&req).expect("the ask flies");
        assert!(!fast.buying_time(), "1 expected tick is not slow");
    }

    // ------------------------------------------------------ the prethink lever

    #[test]
    fn an_unsure_gate_is_named_after_the_rule_that_failed() {
        // D72 says an unsure gate makes no decision of its own. Which rule
        // failed is a different question, and the answer differs: a gate the
        // backend was unsure about is a low-confidence refusal, a gate whose
        // second opinion timed out is a timeout, and a gate whose second
        // model answered an error is that error. They are three different
        // problems with three different fixes.
        let cfg = AgentsConfig::default();
        let world = tiny_world();
        let view_a = view_at(3, 2, 2);
        let menu_a = menu(&view_a);

        /// One mind, asked once, and answered with `answers`.
        fn answered(
            cfg: &AgentsConfig,
            world: &World,
            view: &ScoreView,
            menu: &Menu,
            answers: Answers,
        ) -> Mind {
            let mut m = mind(3, cfg, STUB_POLICY);
            let req = m
                .on_tick(0, view, world, menu)
                .expect("a novel situation asks");
            let ask = m.take_flight(&req).expect("the ask flies");
            m.on_answer(ask, Ok(answers));
            m
        }

        /// The answers with one unsure gate, and the escalation behind it.
        fn unsure_with(menu: &Menu, escalation: Option<Escalation>) -> Answers {
            let mut answers = lane_answers(menu, 0);
            answers.unsure.insert("move".to_owned(), 0.2);
            if let Some(escalation) = escalation {
                answers.escalated.insert("move".to_owned(), escalation);
            }
            answers
        }

        /// A second opinion that came to nothing, in the way it came to it.
        fn escalation(outcome: EscalationOutcome, refusal: Option<Refusal>) -> Escalation {
            Escalation {
                gate: "move".to_owned(),
                primary_confidence: 0.2,
                primary_choice: None,
                second_confidence: None,
                second_choice: None,
                latency_ms: 10,
                outcome,
                refusal,
            }
        }

        // No second opinion: the backend was unsure, and that is what failed.
        let plain = answered(&cfg, &world, &view_a, &menu_a, unsure_with(&menu_a, None));
        let (_, refusal) = plain.refusals()[0].clone();
        assert!(
            matches!(refusal, Refusal::LowConfidence(_)),
            "an unsure gate is a low-confidence refusal"
        );

        // The second opinion timed out.
        let timed_out = answered(
            &cfg,
            &world,
            &view_a,
            &menu_a,
            unsure_with(
                &menu_a,
                Some(escalation(
                    EscalationOutcome::Timeout,
                    Some(Refusal::Timeout),
                )),
            ),
        );
        let (_, refusal) = timed_out.refusals()[0].clone();
        assert!(
            matches!(refusal, Refusal::Timeout),
            "a second opinion that timed out is a timeout"
        );

        // The second model answered an error: that is the error, not a call
        // that never reached one.
        let errored = answered(
            &cfg,
            &world,
            &view_a,
            &menu_a,
            unsure_with(
                &menu_a,
                Some(escalation(
                    EscalationOutcome::Unavailable,
                    Some(Refusal::Http(503)),
                )),
            ),
        );
        let (_, refusal) = errored.refusals()[0].clone();
        assert!(
            matches!(refusal, Refusal::Http(503)),
            "a second opinion that answered 503 is that error"
        );
    }

    #[test]
    fn a_backend_that_panics_does_not_take_the_question_with_it() {
        // N4: a panic in a backend is a bug, but the question it was asked
        // still has to end somewhere. The worker catches the crash, answers
        // with a refusal of its own name, and keeps serving — the ask does
        // not vanish with the thread, and the lane does not die with it.
        struct Panicking;
        impl SysOne for Panicking {
            fn ask(
                &self,
                _state: &str,
                _questions: &Value,
                _deadline: Instant,
            ) -> Result<Answers, Refusal> {
                panic!("the model is on fire");
            }
            fn name(&self) -> &str {
                "panicking"
            }
        }

        fn ask_of(agent: u16, asked: Tick) -> Ask {
            let now = Instant::now();
            Ask {
                req: crate::queue::ThinkReq {
                    agent,
                    squad: 0,
                    pack: Pack::Move,
                    asked,
                    deadline: asked + 30,
                    situation_key: 1,
                },
                pack: Pack::Move,
                backend: "panicking".to_owned(),
                state: String::new(),
                questions: serde_json::json!({}),
                questions_n: 0,
                timeout: Duration::from_secs(1),
                pay_priority: false,
                asked_at: now,
                sent_at: None,
                apply_tick: asked,
                apply_deadline: now + Duration::from_secs(1),
            }
        }

        let thinker = Thinker::new(BTreeMap::from([(
            "panicking".to_owned(),
            Box::new(Panicking) as Box<dyn SysOne + Send + Sync>,
        )]));

        // The panic is the worker's to catch. The message it would print is
        // silenced for the thinker's own threads only — every other panic of
        // the process keeps its words — and the refusal carries the why.
        let stop = Instant::now() + Duration::from_secs(5);
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let thinker = std::thread::current()
                .name()
                .is_some_and(|name| name.starts_with("lm-thinker-"));
            if !thinker {
                previous(info);
            }
        }));
        assert!(thinker.send(ask_of(3, 10)));
        let first = loop {
            let drained = thinker.drain();
            if let Some(thought) = drained.into_iter().next() {
                break thought;
            }
            assert!(Instant::now() < stop, "the crashed ask never came home");
            std::thread::sleep(Duration::from_millis(1));
        };
        assert!(
            matches!(&first.result, Err(Refusal::Panicked(why)) if why.contains("on fire")),
            "a crashed backend is a refusal that says so, not a lost ask: {:?}",
            first.result
        );

        // The worker survived: a second ask comes home the same way, and
        // nothing is left pending.
        assert!(thinker.send(ask_of(3, 40)));
        loop {
            if !thinker.drain().is_empty() {
                break;
            }
            assert!(Instant::now() < stop, "the second ask never came home");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(thinker.pending(), 0, "the lane kept serving");
    }

    #[test]
    fn prethink_answer_used_when_situation_arrives() {
        let cfg = AgentsConfig::default();
        let world = tiny_world();
        let view_a = view_at(3, 2, 2);
        let view_b = view_at(3, 5, 5);
        let menu_a = menu(&view_a);
        let menu_b = menu(&view_b);
        let key_a = Facts::from_view(&view_a, &world, 0, &cfg, &SharedConfig::default()).key;

        let mut m = mind(3, &cfg, STUB_POLICY);

        // The first sight of A asks; the answer is cached under its key.
        let req = m
            .on_tick(0, &view_a, &world, &menu_a)
            .expect("a novel situation asks");
        let ask = m.take_flight(&req).expect("the ask flies");
        let (decisions, outcome) = m.on_answer(ask, Ok(lane_answers(&menu_a, 0)));
        assert_eq!(decisions.len(), 1, "the move gate becomes a lane");
        assert_eq!(outcome, AskOutcome::Accepted);

        // A quiet tick on the same situation asks nothing.
        assert!(m.on_tick(1, &view_a, &world, &menu_a).is_none());

        // B arrives and is asked and answered.
        let req = m
            .on_tick(8, &view_b, &world, &menu_b)
            .expect("a new situation asks");
        let ask = m.take_flight(&req).expect("the ask flies");
        m.on_answer(ask, Ok(lane_answers(&menu_b, 1)));

        // A arrives again: the pre-thought verdict answers it — no ask, the
        // decisions are ready on the spot.
        assert!(
            m.on_tick(16, &view_a, &world, &menu_a).is_none(),
            "a cached situation does not re-ask"
        );
        let cached = m.take_decisions();
        assert_eq!(cached.len(), 1);
        match &cached[0].provenance {
            lm_core::Provenance::System1 { situation_key, .. } => {
                assert_eq!(*situation_key, key_a, "the cached decision names A")
            }
            other => panic!("the cached decision is System 1, got {other:?}"),
        }
        assert_eq!(cached[0].apply_tick, 16 + 3, "applied a minimum lead out");

        // A refused first ask caches nothing, so a quiet tick pre-thinks the
        // situation instead of leaving it uncovered.
        let mut m = mind(3, &cfg, STUB_POLICY);
        let req = m
            .on_tick(0, &view_a, &world, &menu_a)
            .expect("a novel situation asks");
        let ask = m.take_flight(&req).expect("the ask flies");
        let (decisions, outcome) = m.on_answer(ask, Err(Refusal::Timeout));
        assert!(decisions.is_empty());
        assert_eq!(outcome, AskOutcome::Refused);
        let req = m
            .on_tick(8, &view_a, &world, &menu_a)
            .expect("prethink asks about the quiet situation");
        assert_eq!(req.situation_key, key_a);
    }

    // ---------------------------------------------------------- the fence (N3)

    /// The move gate's answer, with the second opinion's outcome.
    ///
    /// The second opinion is the one the fence is about: `Late` means it
    /// would have landed after the decision's apply tick, so it is no
    /// opinion at all and the gate stays unsure.
    fn escalated_move(menu: &Menu, outcome: EscalationOutcome) -> Answers {
        let mut answers = lane_answers(menu, 0);
        let second = match outcome {
            EscalationOutcome::Taken => {
                // The second model answered, and its answer is the gate's.
                let probs = match answers.per_gate.get("move") {
                    Some(Answer::Choice { probs, .. }) => probs.clone(),
                    _ => BTreeMap::new(),
                };
                answers.per_gate.insert(
                    "move".to_owned(),
                    Answer::Choice {
                        probs,
                        confidence: 0.9,
                    },
                );
                Some(0.9)
            }
            _ => {
                // No opinion: the gate is unsure, and the cheap model's own
                // number is what is left of it.
                answers.per_gate.remove("move");
                answers.unsure.insert("move".to_owned(), 0.2);
                None
            }
        };
        answers.escalated.insert(
            "move".to_owned(),
            Escalation {
                gate: "move".to_owned(),
                primary_confidence: 0.2,
                primary_choice: None,
                second_confidence: second,
                second_choice: None,
                latency_ms: 500,
                outcome,
                // The mechanism behind the outcome (N2): the second opinion
                // was fenced out, so there is no call that failed — the
                // outcome says which rule it was.
                refusal: None,
            },
        );
        answers
    }

    #[test]
    fn a_late_escalation_takes_the_safe_branch() {
        // N3: a second opinion that missed the decision's apply tick is no
        // opinion at all. The move is not taken — the safe branch plays, the
        // reflex moves the agent — and the round records the refusal by
        // name, so the loss is visible.
        let cfg = AgentsConfig::default();
        let world = tiny_world();
        let view = view_at(1, 2, 2);
        let menu = menu(&view);
        let mut m = speaking_mind(1, &cfg);

        let req = m.on_tick(0, &view, &world, &menu).expect("the mind asks");
        let ask = m.take_flight(&req).expect("the ask flies");
        let answers = escalated_move(&menu, EscalationOutcome::Late);
        let (decisions, outcome) = m.on_answer(ask, Ok(answers));

        // The move was not taken.
        assert!(
            !decisions.iter().any(|d| matches!(d.kind, Kind::Lane(_))),
            "the safe branch plays: {decisions:?}"
        );
        // The ask ended in exactly one state (N4): the answer came home in
        // time, so the *question* was answered. The gate it carried took
        // the safe branch, and that refusal is a separate ledger — named
        // below.
        assert_eq!(outcome, AskOutcome::Accepted);
        // And the round says why, by name.
        assert!(
            m.refusals()
                .iter()
                .any(|(_, r)| *r == Refusal::EscalationLate),
            "the round named the late escalation: {:?}",
            m.refusals()
        );
        // And the escalation itself is on the record, with its outcome.
        assert!(
            m.escalations()
                .iter()
                .any(|(_, e)| e.outcome == EscalationOutcome::Late),
            "the escalation is logged: {:?}",
            m.escalations()
        );
    }

    #[test]
    fn an_escalation_on_time_is_taken() {
        // N3: the fence is a deadline, not a veto. A second opinion that is
        // in hand before the decision's apply tick is taken, and the move is
        // made on the second model's word.
        let cfg = AgentsConfig::default();
        let world = tiny_world();
        let view = view_at(1, 2, 2);
        let menu = menu(&view);
        let mut m = speaking_mind(1, &cfg);

        let req = m.on_tick(0, &view, &world, &menu).expect("the mind asks");
        let ask = m.take_flight(&req).expect("the ask flies");
        let answers = escalated_move(&menu, EscalationOutcome::Taken);
        let (decisions, outcome) = m.on_answer(ask, Ok(answers));

        // The move was taken.
        assert!(
            decisions.iter().any(|d| matches!(d.kind, Kind::Lane(_))),
            "the model decided the move: {decisions:?}"
        );
        // And the ask ended in exactly one state: taken (N4).
        assert_eq!(outcome, AskOutcome::Accepted);
        // And the fence did not fire.
        assert!(
            !m.refusals()
                .iter()
                .any(|(_, r)| *r == Refusal::EscalationLate),
            "the fence did not fire: {:?}",
            m.refusals()
        );
    }

    // ------------------------------------------------------------- the host

    #[test]
    fn fast_model_produces_lane_decisions() {
        let h = header(3, 1, 2);
        let cfg = AgentsConfig::default();
        // The stub answers the menu of agent 1's first view, favoring
        // beacon 0, at 20 ms.
        let probe = Sim::new(&h).expect("the round starts");
        let answers = lane_answers(&menu(&probe.view(1)), 0);

        let mut host = Host::new(h, &cfg, vec![(Duration::from_millis(20), Ok(answers))]);
        host.run(90);

        // The decisions arrived, are lanes, and were stamped on time.
        assert!(!host.submitted.is_empty(), "the stub produced decisions");
        for d in &host.submitted {
            assert_eq!(d.source, Source::Agent(d.source.agent().unwrap()));
            assert!(matches!(d.kind, Kind::Lane(_)), "got {:?}", d.kind);
            assert!(matches!(d.provenance, lm_core::Provenance::System1 { .. }));
        }
        let stats = host.seq.stats();
        assert!(stats.on_time >= 1, "at least one decision was on time");
        assert_eq!(stats.late, 0, "no decision was late");
        assert_eq!(stats.stale, 0, "no decision was stale");

        // The lane command changed: agent 1 walks to beacon 0 on the
        // model's word, not the reflex's.
        assert_eq!(
            host.sim.agent(1).and_then(|a| a.lane_cmd),
            Some(MenuOption::GoToBeacon(0))
        );
        // The thought records landed (D55).
        let thoughts = host.minds[&1].thoughts();
        assert!(!thoughts.is_empty());
        assert!(thoughts.iter().all(|t| t.landed.is_some()));
    }

    #[test]
    fn thor_down_falls_back() {
        let h = header(5, 2, 4);
        // Every call outlives its deadline: the stub refuses instantly.
        let cfg = AgentsConfig {
            request_timeout_ms: 60,
            ..AgentsConfig::default()
        };
        let probe = Sim::new(&h).expect("the round starts");
        let answers = lane_answers(&menu(&probe.view(1)), 0);
        let mut host = Host::new(
            h.clone(),
            &cfg,
            vec![(Duration::from_secs(10), Ok(answers))],
        );
        host.run(240);

        // Refusals logged, zero decisions.
        assert!(host.submitted.is_empty(), "a dead model decides nothing");
        for mind in host.minds.values() {
            assert!(!mind.refusals().is_empty(), "every mind refused");
            assert!(
                mind.refusals().iter().all(|(_, r)| *r == Refusal::Timeout),
                "every refusal is a timeout"
            );
        }

        // The agents still moved by reflex: the round is bit-identical to a
        // reflex-only round on the same header.
        let mut twin = Sim::new(&h).expect("the round starts");
        for _ in 0..240 {
            twin.step(&[]);
        }
        assert_eq!(
            host.sim.state_hash(),
            twin.state_hash(),
            "the round completed on reflexes alone"
        );
    }

    #[test]
    fn no_backlog_after_outage() {
        let h = header(6, 2, 4);
        let cfg = AgentsConfig {
            request_timeout_ms: 60,
            ..AgentsConfig::default()
        };
        let probe = Sim::new(&h).expect("the round starts");
        let answers = lane_answers(&menu(&probe.view(1)), 0);
        // A long outage (every call times out), then recovery at 15 ms.
        let mut script: Vec<(Duration, Result<Answers, Refusal>)> = (0..100)
            .map(|_| (Duration::from_secs(10), Ok(answers.clone())))
            .collect();
        script.push((Duration::from_millis(15), Ok(answers)));

        let mut host = Host::new(h, &cfg, script);
        host.run(300);

        // The queue never held more than one request per agent.
        assert!(
            host.queue_high <= host.minds.len() * 2,
            "queue reached {} for {} agents",
            host.queue_high,
            host.minds.len()
        );
        // The outage refused, and the recovery decided.
        assert!(
            host.minds
                .values()
                .any(|m| m.refusals().iter().any(|(_, r)| *r == Refusal::Timeout)),
            "the outage left refusals"
        );
        assert!(
            !host.submitted.is_empty(),
            "the recovery produced decisions"
        );
    }

    // ------------------------------------------------------------ the lines

    /// A roster of two agents, for the line tests.
    fn two_roster() -> Vec<RosterEntry> {
        vec![
            RosterEntry {
                agent: 0,
                squad: 0,
                role: Role::Leader,
                voice: "default".to_owned(),
                harness_commit: "test".to_owned(),
                reflex: "default".to_owned(),
            },
            RosterEntry {
                agent: 1,
                squad: 0,
                role: Role::Member,
                voice: "default".to_owned(),
                harness_commit: "test".to_owned(),
                reflex: "default".to_owned(),
            },
        ]
    }

    /// A mind that speaks: the shipped gates and phrasebook, the stub
    /// policy, and the round's roster and talk rules.
    fn speaking_mind(agent: u16, cfg: &AgentsConfig) -> Mind {
        Mind::new(agent, Role::Member, &harness(STUB_POLICY), cfg)
            .with_params(&Params::default())
            .with_roster(&two_roster())
            .with_talk(&TalkParams::default())
    }

    /// The answers of the gates that were asked.
    ///
    /// A model answers the gates it is asked and no others (D70): the
    /// scripted stub answers everything, so its answers are cut down to
    /// the pack the ask carried.
    fn answers_asked(ask: &Ask, answers: &Answers) -> Answers {
        let Some(gates) = ask.questions.as_object() else {
            return answers.clone();
        };
        Answers {
            per_gate: answers
                .per_gate
                .iter()
                .filter(|(id, _)| gates.contains_key(*id))
                .map(|(id, answer)| (id.clone(), answer.clone()))
                .collect(),
            unsure: answers
                .unsure
                .iter()
                .filter(|(id, _)| gates.contains_key(*id))
                .map(|(id, confidence)| (id.clone(), *confidence))
                .collect(),
            escalated: answers.escalated.clone(),
            model: answers.model.clone(),
            input_tokens: answers.input_tokens,
            questions: answers.questions.clone(),
            latency_ms: answers.latency_ms,
        }
    }

    /// Ask once and answer, and hand back the decisions.
    ///
    /// Both packs are asked (D70): the move pack decides where the agent
    /// walks, the talk pack what it says.
    fn think_once(
        mind: &mut Mind,
        view: &ScoreView,
        world: &World,
        answers: Answers,
    ) -> Vec<Decision> {
        let menu = menu(view);
        let mut decisions = Vec::new();
        if let Some(req) = mind.on_tick(0, view, world, &menu) {
            let ask = mind.take_flight(&req).expect("the ask flies");
            let answers = answers_asked(&ask, &answers);
            decisions.extend(mind.on_answer(ask, Ok(answers)).0);
        }
        if let Some(req) = mind.on_tick_talk(0, view, world, &menu) {
            let ask = mind.take_flight(&req).expect("the ask flies");
            let answers = answers_asked(&ask, &answers);
            decisions.extend(mind.on_answer(ask, Ok(answers)).0);
        }
        decisions
    }

    #[test]
    fn the_utterance_is_a_logged_decision() {
        let h = header(3, 1, 2);
        let cfg = AgentsConfig::default();
        let probe = Sim::new(&h).expect("the round starts");
        let answers = talk_answers(&menu(&probe.view(1)), 0);
        let mut host = Host::new(h, &cfg, vec![(Duration::from_millis(20), Ok(answers))]);
        let log = host.run(300);

        // The line is in the log, with the provenance of the thought that
        // chose it.
        let spoken: Vec<_> = log
            .segments
            .iter()
            .flat_map(|s| &s.decisions)
            .filter(|s| matches!(s.d.kind, Kind::Utterance(_)))
            .collect();
        assert!(!spoken.is_empty(), "the round spoke");
        for stamped in &spoken {
            match &stamped.d.provenance {
                lm_core::Provenance::System1 {
                    model,
                    tokens,
                    latency_ms,
                    situation_key,
                } => {
                    assert_eq!(model, "stub");
                    assert_eq!(*tokens, 64);
                    assert_eq!(*latency_ms, 20);
                    assert_ne!(*situation_key, 0);
                }
                other => panic!("the line's provenance is {other:?}"),
            }
            // And the line is a score: a gesture, an addressee, a point and
            // its words.
            let Kind::Utterance(utterance) = &stamped.d.kind else {
                panic!("the decision is not a line");
            };
            assert!(!utterance.words.is_empty(), "the line has words");
        }

        // And the round replays bit-exact with the lines in it.
        assert!(log.verify().is_ok());
        let mut replay = Sim::new(&log.header).expect("the replay starts");
        let mut hashes = Vec::new();
        for t in 0..300 {
            let out = replay.step(log.decisions_for(t));
            if let Some(hash) = out.hash {
                hashes.push(hash);
            }
        }
        let recorded: Vec<u64> = log.hashes().into_iter().map(|(_, h)| h).collect();
        assert_eq!(hashes, recorded, "the replay reproduced every hash");
    }

    #[test]
    fn the_presence_gate_silences_the_agent() {
        let cfg = AgentsConfig::default();
        let world = tiny_world();
        let view = view_at(1, 2, 2);
        let mut m = speaking_mind(1, &cfg);

        // Every presence gate says the line is not there.
        let mut answers = talk_answers(&menu(&view), 0);
        for gate in ["gesture", "attention", "intention", "intent", "subject"] {
            answers
                .per_gate
                .insert(format!("{gate}_present"), Answer::Noul(0.0));
        }
        let decisions = think_once(&mut m, &view, &world, answers);
        assert!(
            decisions
                .iter()
                .all(|d| !matches!(d.kind, Kind::Utterance(_))),
            "a line that is not there is not said: {decisions:?}"
        );
        assert!(m.refusals().is_empty(), "silence is not a refusal");
    }

    #[test]
    fn a_harness_whose_gates_do_not_load_is_a_refusal() {
        // The gates are the questions: a harness whose gates do not fit
        // the API's schema is a refusal the round can see, and not a
        // silent agent that never thinks.
        let mut h = harness(STUB_POLICY);
        h.gates_toml = r#"
            [[gates]]
            id = "intention"
            type = "score"
            options_from = "fixed"
            instructions = "How urgent is this moment?"

            [gates.criteria]
            idle = "the moment is quiet"
        "#
        .to_owned();

        let m = Mind::new(1, Role::Member, &h, &AgentsConfig::default());

        assert_eq!(m.refusals().len(), 1);
        assert!(
            m.refusals()[0]
                .1
                .to_string()
                .contains("the harness gates do not load"),
            "got {:?}",
            m.refusals()[0].1
        );
    }

    #[test]
    fn a_harness_whose_facts_cannot_be_stated_is_a_refusal() {
        // The shared target is a fact the builder says out loud, and a
        // repulsor cannot be one: the beacon would be named as the target
        // and described as pushing away from it. So the round sees the
        // refusal rather than a silent agent with the wrong target (D26).
        let mut h = harness(STUB_POLICY);
        h.facts_toml = r#"
            [shared]
            rule = "ghost"

            [shared.ghost]
            kind = "repulsor"
        "#
        .to_owned();

        let m = Mind::new(1, Role::Member, &h, &AgentsConfig::default());

        assert_eq!(m.refusals().len(), 1);
        assert!(
            m.refusals()[0]
                .1
                .to_string()
                .contains("the harness facts cannot be stated"),
            "got {:?}",
            m.refusals()[0].1
        );
    }

    #[test]
    fn bad_realizer_output_is_a_refusal() {
        let cfg = AgentsConfig::default();
        let world = tiny_world();
        let view = view_at(1, 2, 2);
        // The model writes no words at all: there is no line to say.
        let mut m = speaking_mind(1, &cfg).with_realizer(Box::new(LlmRealizer::new(
            Box::new(StubLlm::new("stub", "   ")),
            Duration::from_millis(50),
        )));

        let decisions = think_once(&mut m, &view, &world, talk_answers(&menu(&view), 0));
        assert!(
            decisions
                .iter()
                .all(|d| !matches!(d.kind, Kind::Utterance(_))),
            "a line with no words is not said: {decisions:?}"
        );
        assert!(
            m.refusals()
                .iter()
                .any(|(_, refusal)| matches!(refusal, Refusal::Malformed(_))),
            "the refusal is logged: {:?}",
            m.refusals()
        );
    }

    #[test]
    fn the_wording_is_counted_where_it_happened() {
        let cfg = AgentsConfig::default();
        let world = tiny_world();
        let view = view_at(1, 2, 2);

        // The model words the line: the count says so.
        let mut m = speaking_mind(1, &cfg).with_realizer(Box::new(LlmRealizer::new(
            Box::new(StubLlm::new("stub", "hold on")),
            Duration::from_millis(50),
        )));
        think_once(&mut m, &view, &world, talk_answers(&menu(&view), 0));
        assert_eq!(m.words().llm, 1, "the model worded the line");
        assert_eq!(m.words().phrasebook, 0);
        assert_eq!(m.words().late, 0);

        // The model is late: the phrasebook words the line, and the line is
        // counted late.
        let mut m = speaking_mind(1, &cfg).with_realizer(Box::new(AutoRealizer::new(
            TemplateRealizer::shipped(),
            LlmRealizer::new(
                Box::new(StubLlm::refusing("stub", Refusal::Timeout)),
                Duration::from_millis(50),
            ),
        )));
        think_once(&mut m, &view, &world, talk_answers(&menu(&view), 0));
        assert_eq!(m.words().llm, 0);
        assert_eq!(m.words().phrasebook, 1, "the phrasebook worded the line");
        assert_eq!(m.words().late, 1, "the model missed the deadline");
    }

    #[test]
    fn llm_down_falls_back_to_the_template() {
        let cfg = AgentsConfig::default();
        let world = tiny_world();
        let view = view_at(1, 2, 2);

        // The model is down and the phrasebook has the line: the automatic
        // realizer words it from the phrasebook.
        let mut m = speaking_mind(1, &cfg).with_realizer(Box::new(AutoRealizer::new(
            TemplateRealizer::shipped(),
            LlmRealizer::new(
                Box::new(StubLlm::refusing("stub", Refusal::Timeout)),
                Duration::from_millis(50),
            ),
        )));
        let decisions = think_once(&mut m, &view, &world, talk_answers(&menu(&view), 0));
        assert!(
            decisions
                .iter()
                .any(|d| matches!(d.kind, Kind::Utterance(_))),
            "the phrasebook worded the line: {decisions:?}"
        );

        // The model alone, with the model down: nothing is said, and the
        // refusal is logged.
        let mut m = speaking_mind(1, &cfg).with_realizer(Box::new(LlmRealizer::new(
            Box::new(StubLlm::refusing("stub", Refusal::Timeout)),
            Duration::from_millis(50),
        )));
        let decisions = think_once(&mut m, &view, &world, talk_answers(&menu(&view), 0));
        assert!(
            decisions
                .iter()
                .all(|d| !matches!(d.kind, Kind::Utterance(_))),
            "a model that is down says nothing: {decisions:?}"
        );
        assert!(
            m.refusals()
                .iter()
                .any(|(_, refusal)| matches!(refusal, Refusal::Timeout)),
            "the refusal is logged: {:?}",
            m.refusals()
        );
    }

    #[test]
    fn a_pre_drafted_reply_lands_after_the_utterance() {
        let cfg = AgentsConfig::default();
        let world = tiny_world();
        let mut view = view_at(1, 2, 2);
        // A line that is still being spoken, addressed to this agent.
        view.heard = vec![Heard {
            id: 7,
            speaker: 0,
            channel: lm_core::utterance::Channel::Voice,
            addressee: Addressee::Agent(1),
            act: Some(lm_core::utterance::SpeechAct::Command),
            intent: Some(lm_core::utterance::Intent::Order),
            affect: lm_core::utterance::Affect::Calm,
            subject: lm_core::utterance::Subject::None,
            clarity_q16: 65_536,
            words: Vec::new(),
            spoken_words: 2,
            started_tick: 0,
            ended_tick: 9,
            ongoing: true,
        }];
        let mut m = speaking_mind(1, &cfg);

        let decisions = think_once(&mut m, &view, &world, talk_answers(&menu(&view), 0));
        let line = decisions
            .iter()
            .find(|d| matches!(d.kind, Kind::Utterance(_)))
            .expect("the reply is drafted");
        assert_eq!(
            line.apply_tick, 10,
            "the reply lands just after the line ends"
        );

        // While the same line is still going, no second reply is drafted.
        let again = m.on_tick(1, &view, &world, &menu(&view));
        assert!(again.is_none(), "the line is not drafted twice");
    }

    #[test]
    fn round_replays_without_models() {
        let h = header(9, 1, 2);
        let cfg = AgentsConfig::default();
        let probe = Sim::new(&h).expect("the round starts");
        let answers = lane_answers(&menu(&probe.view(1)), 0);
        let mut host = Host::new(h, &cfg, vec![(Duration::from_millis(3), Ok(answers))]);
        let log = host.run(300);
        assert!(log.verify().is_ok());

        // The round thought with a model…
        let thought = log
            .segments
            .iter()
            .flat_map(|s| &s.decisions)
            .any(|s| matches!(s.d.provenance, lm_core::Provenance::System1 { .. }));
        assert!(thought, "the round logged System 1 decisions");

        // …and replays bit-exact with no backend at all: just the header and
        // the stamped decisions (this is `lm replay`'s path).
        let mut replay = Sim::new(&log.header).expect("the replay starts");
        let mut hashes = Vec::new();
        for t in 0..300 {
            let out = replay.step(log.decisions_for(t));
            if let Some(hash) = out.hash {
                hashes.push(hash);
            }
        }
        let recorded: Vec<u64> = log.hashes().into_iter().map(|(_, h)| h).collect();
        assert_eq!(hashes, recorded, "the replay reproduced every hash");
    }
}
