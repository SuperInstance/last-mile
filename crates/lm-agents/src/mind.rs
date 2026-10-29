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

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use lm_core::decision::{Kind, Provenance, Source};
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

use crate::config::AgentsConfig;
use crate::facts::Facts;
use crate::gates::{answers_to_decisions, GateSet};
use crate::harness::Harness;
use crate::phrasebook::Phrasebook;
use crate::queue::ThinkReq;
use crate::sysone::{Answers, Refusal, SysOne};
use crate::utter::{
    pick_realizer_with, plan_from_gates, presence_gate, utterance_from_text, GateCtx, Realizer,
    GATE_GESTURE, UTTERANCE_GATES,
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tier {
    name: String,
    backend: String,
    quality: u8,
    rtt: RttEstimator,
    per_question: RttEstimator,
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
    pub fn note(&mut self, latency_ms: u32, questions: u16) {
        self.rtt.record(latency_ms);
        self.per_question
            .record(latency_ms / u32::from(questions.max(1)));
    }

    /// The measured p99 of whole calls, in milliseconds.
    pub fn p99_ms(&self) -> u32 {
        self.rtt.p99()
    }

    /// The measured p50 of whole calls, in milliseconds.
    pub fn p50_ms(&self) -> u32 {
        self.rtt.p50()
    }

    /// The p99 a pack of `questions` questions is expected to cost: the
    /// per-question estimate scaled to the pack, falling back to the
    /// whole-call p99, and 0 while nothing has been measured.
    pub fn p99_ms_for(&self, questions: u16) -> u32 {
        if !self.per_question.is_empty() {
            self.per_question
                .p99()
                .saturating_mul(u32::from(questions.max(1)))
        } else if !self.rtt.is_empty() {
            self.rtt.p99()
        } else {
            0
        }
    }

    /// [`Tier::p99_ms_for`] in ticks on a `tick_hz` sim.
    pub fn p99_ticks_for(&self, questions: u16, tick_hz: u16) -> u64 {
        ms_to_ticks_ceil(self.p99_ms_for(questions), tick_hz)
    }

    /// The measured p50 of whole calls, in ticks on a `tick_hz` sim.
    pub fn p50_ticks(&self, tick_hz: u16) -> u64 {
        ms_to_ticks_ceil(self.p50_ms(), tick_hz)
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
    pub fn note(&mut self, name: &str, latency_ms: u32, questions: u16) -> bool {
        match self.tiers.iter_mut().find(|t| t.name == name) {
            Some(tier) => {
                tier.note(latency_ms, questions);
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
    /// The `buy_time` lever: hold or follow the safe branch while a slow
    /// thought is in flight.
    pub buy_time: bool,
    /// A thought expected to take more than this many ticks is "slow".
    pub buy_time_above_ticks: u16,
    /// The `prethink` lever: warm the verdict cache in quiet ticks.
    pub prethink: bool,
    /// The `verdict_cache` lever: cache verdicts by situation key.
    pub verdict_cache: bool,
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
                        rtt: RttEstimator::new(),
                        per_question: RttEstimator::new(),
                    })
                    .collect(),
            },
            threat_horizon_ticks: file.think.threat_horizon_ticks,
            max_think_ticks: file.think.max_think_ticks,
            threat_age_ticks: file.think.threat_age_ticks,
            buy_time: file.levers.buy_time,
            buy_time_above_ticks: file.levers.buy_time_above_ticks,
            prethink: file.levers.prethink,
            verdict_cache: file.levers.verdict_cache,
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
    pub fn pick_tier(&self, urgent: bool, questions: u16, tick_hz: u16) -> Option<&Tier> {
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
            let p99 = tier.p99_ticks_for(questions, tick_hz);
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

/// The `[levers]` section (D56).
#[derive(Deserialize)]
#[serde(default)]
struct LeverSpec {
    buy_time: bool,
    buy_time_above_ticks: u16,
    prethink: bool,
    verdict_cache: bool,
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
}

// ----------------------------------------------------------------- the worker

/// The thinker: the one thread that talks to the models.
///
/// The sim thread pushes [`Ask`]s and drains [`Thought`]s; the worker owns
/// the backends and their persistent connections (D50) and runs every HTTP
/// call, so the simulation never blocks on a model. Dropping the thinker
/// closes its channel and joins the thread.
pub struct Thinker {
    to_worker: Option<mpsc::Sender<Ask>>,
    // Behind a lock, so the thinker can be held by the host's round
    // resource, which is shared between threads.
    from_worker: Mutex<mpsc::Receiver<Thought>>,
    sent: Arc<AtomicUsize>,
    answered: Arc<AtomicUsize>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Thinker {
    /// A thinker asking `backends` (by backend name) on its own thread.
    pub fn new(backends: BTreeMap<String, Box<dyn SysOne + Send>>) -> Thinker {
        let (to_worker, worker_rx) = mpsc::channel::<Ask>();
        let (worker_tx, from_worker) = mpsc::channel::<Thought>();
        let answered = Arc::new(AtomicUsize::new(0));
        let worker_answered = Arc::clone(&answered);
        let handle = thread::Builder::new()
            .name("lm-thinker".to_owned())
            .spawn(move || {
                // One thread, every HTTP call, one persistent connection per
                // backend (D50). The loop ends when the thinker is dropped.
                for ask in worker_rx {
                    let started = Instant::now();
                    let deadline = started + ask.timeout;
                    let result = match backends.get(&ask.backend) {
                        Some(backend) => backend.ask(&ask.state, &ask.questions, deadline),
                        None => Err(Refusal::Malformed(format!(
                            "no backend named {}",
                            ask.backend
                        ))),
                    };
                    let rtt_ms = u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX);
                    if worker_tx
                        .send(Thought {
                            ask,
                            result,
                            rtt_ms,
                        })
                        .is_err()
                    {
                        break;
                    }
                    worker_answered.fetch_add(1, Ordering::SeqCst);
                }
            })
            .expect("the thinker thread starts");
        Thinker {
            to_worker: Some(to_worker),
            from_worker: Mutex::new(from_worker),
            sent: Arc::new(AtomicUsize::new(0)),
            answered,
            handle: Some(handle),
        }
    }

    /// Queue an ask; never blocks and never fails (a dead worker drops the
    /// ask, and the stale window on the sim side cleans up).
    pub fn send(&self, ask: Ask) {
        if let Some(sender) = &self.to_worker {
            if sender.send(ask).is_ok() {
                self.sent.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    /// Every thought that has come home since the last drain.
    pub fn drain(&self) -> Vec<Thought> {
        let from_worker = self
            .from_worker
            .lock()
            .expect("the thinker's inbox is not poisoned");
        let mut out = Vec::new();
        while let Ok(thought) = from_worker.try_recv() {
            out.push(thought);
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
        // Close the channel first, then join: the worker ends its loop once
        // the sender is gone, so the join always returns.
        self.to_worker = None;
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

// -------------------------------------------------------------------- the mind

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
    /// How many questions the pack held.
    ///
    /// The utterance needs the whole talk pack; a pack that was cut short
    /// says nothing about the line, and the agent stays quiet.
    questions_n: u16,
    /// The tick the reply is to be applied on, when it was pre-drafted
    /// (D31): the tick after the line it answers ends.
    pre_draft_at: Option<Tick>,
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
    mode: Mode,
    now: Tick,
    last_ask: Option<Tick>,
    last_asked_key: Option<u64>,
    last_tier_name: Option<String>,
    pending: Option<(Flight, Ask)>,
    in_flight: Option<Flight>,
    cache: BTreeMap<(u64, u64), Answers>,
    ready: Vec<Decision>,
    thoughts: Vec<ThoughtRecord>,
    refusals: Vec<(Tick, Refusal)>,
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
}

impl Mind {
    /// A mind for `agent` playing `role`, thinking with the gates and policy
    /// of `harness` under `cfg`.
    ///
    /// The round's [`Params`] are not part of the harness; set them with
    /// [`Mind::with_params`] when the round starts (a mind built without
    /// them uses the parameter defaults).
    pub fn new(agent: u16, role: Role, harness: &Harness, cfg: &AgentsConfig) -> Mind {
        let gates = GateSet::parse(&harness.gates_toml).unwrap_or_default();
        let phrasebook = Phrasebook::parse(&harness.phrasebook_toml).unwrap_or_default();
        Mind {
            agent,
            role,
            squad: 0,
            cfg: cfg.clone(),
            params: Params::default(),
            gates,
            policy: Policy::for_harness(&harness.policy_toml),
            mode: Mode::parse(&cfg.mode),
            now: 0,
            last_ask: None,
            last_asked_key: None,
            last_tier_name: None,
            pending: None,
            in_flight: None,
            cache: BTreeMap::new(),
            ready: Vec::new(),
            thoughts: Vec::new(),
            refusals: Vec::new(),
            seq_counter: 0,
            roster: Vec::new(),
            talk: TalkParams::default(),
            realizer: pick_realizer_with(cfg, &phrasebook),
            spoken_ticks: 0,
            pre_drafted: None,
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
                let batch_p99 = match self.policy.pick_tier(false, questions, self.params.tick_hz) {
                    Some(tier) => u64::from(tier.p99_ms_for(questions)),
                    // No model fits the quiet budget at all: no batch to
                    // keep in the present tense.
                    None => u64::from(u32::MAX),
                };
                let transit = u64::from(self.params.lead_margin_ms);
                if batch_p99 + transit < period_ms {
                    Mode::Presence
                } else {
                    Mode::Point
                }
            }
        }
    }

    /// Observe the tick and decide whether to think.
    ///
    /// Returns the [`ThinkReq`] for the least-use-first queue when the policy
    /// fires — novelty (a change in the facts key), a fresh threat, the
    /// think rate in point mode; the presence period in presence mode — and
    /// `None` when the moment does not deserve a model (or no model tier
    /// lands in time, and the reflex plays it). The question pack itself
    /// waits in the mind until the queue hands the request back
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
        // One thought at a time: the worker owns this mind's model turn until
        // the answer comes home. While it holds one the only question is
        // whether the wait itself is a move (the buy_time lever).
        if self.in_flight.is_some() {
            return None;
        }
        // D31: a line that is still being spoken and names this agent is
        // answered at once: the reply is drafted now and applied when the
        // line ends.
        let pre_draft = self.pre_draft(view);
        let pre_draft_at = pre_draft.map(|(_, at)| at);
        // The think rate gates everything, facts included: a mind that
        // cannot ask this tick has no use for a situation summary. A
        // pre-drafted reply is the exception: it is already late.
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
        if pre_draft_at.is_none()
            && self
                .last_ask
                .is_some_and(|asked| now.saturating_sub(asked) < period)
        {
            return None;
        }

        let facts = Facts::from_view(view, world, view.squad_id, &self.cfg);
        let fresh_threat = view
            .enemies
            .iter()
            .any(|e| e.age_ticks <= self.policy.threat_age_ticks);
        // Novelty is a change in the facts key (D25): the same situation at
        // a later tick is not news.
        let novel = self.last_asked_key != Some(facts.key);
        let want = match mode {
            // Point mode thinks on policy triggers only.
            Mode::Point | Mode::Auto => novel || fresh_threat,
            // Presence mode samples continuously.
            Mode::Presence => true,
        };
        // A pre-drafted reply is a thought of its own: it is asked now,
        // whatever the novelty and the cache say.
        let want = want || pre_draft_at.is_some();

        // The verdict cache answers a returning situation on the spot.
        if want
            && pre_draft_at.is_none()
            && self.policy.verdict_cache
            && self.cached(facts.key, menu.id)
        {
            let answers = self
                .cache
                .get(&(facts.key, menu.id))
                .cloned()
                .expect("cached said it was there");
            let p99 = self
                .policy
                .tiers
                .tier(&self.last_tier().unwrap_or_default())
                .map_or(0, Tier::p99_ms);
            let lead = lead_ticks(&self.params, p99);
            let mut decisions = answers_to_decisions(
                &self.gates,
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

        // The prethink lever (D56): a quiet tick warms the cache for the
        // likeliest next situation — in M1, the current one, if a refusal
        // left it uncovered.
        let quiet_ask =
            self.policy.prethink && self.policy.verdict_cache && !self.cached(facts.key, menu.id);
        if !want && !quiet_ask {
            return None;
        }

        let questions_n = self.pack_len();
        if questions_n == 0 {
            // No gates to ask: nothing to think about.
            return None;
        }
        let tier = match self
            .policy
            .pick_tier(fresh_threat, questions_n, self.params.tick_hz)
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
        let p99_ticks = tier.p99_ticks_for(questions_n, self.params.tick_hz).max(1);
        let expected_land = now.saturating_add(p99_ticks);
        let req = ThinkReq {
            agent: self.agent,
            squad: view.squad_id,
            asked: now,
            deadline: now.saturating_add(u64::from(self.params.stale_window_ticks)),
            situation_key: facts.key,
        };
        let ask = Ask {
            req: req.clone(),
            backend: tier.backend().to_owned(),
            state: facts.text.clone(),
            questions: self.gates.to_questions_limited(menu, questions_n),
            questions_n,
            timeout: Duration::from_millis(u64::from(self.cfg.request_timeout_ms)),
            pay_priority: self.policy.pay_priority && fresh_threat,
        };
        let flight = Flight {
            req: req.clone(),
            menu: menu.clone(),
            tier: tier.name().to_owned(),
            expected_land,
            facts: facts.clone(),
            questions_n,
            pre_draft_at,
        };
        // The line is drafted now, so it is not drafted again while it is
        // still going (D31).
        if let Some((id, _)) = pre_draft {
            self.pre_drafted = Some(id);
        }
        self.last_tier_name = Some(tier.name().to_owned());
        self.pending = Some((flight, ask));
        self.last_asked_key = Some(facts.key);
        self.last_ask = Some(now);
        Some(req)
    }

    /// Hand the queued request to the worker: the ask leaves with it and the
    /// flight is in the mind's hands until the answer returns.
    ///
    /// `None` when the mind holds no such queued request (it was replaced by
    /// a newer one); the request is then simply never sent.
    pub fn take_flight(&mut self, req: &ThinkReq) -> Option<Ask> {
        let (flight, ask) = self.pending.take()?;
        if flight.req != *req {
            // A newer ask replaced this one before the queue got to it.
            self.pending = Some((flight, ask));
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
        self.in_flight = Some(flight);
        Some(ask)
    }

    /// Take an answer home: note the landing, teach the tier's estimator,
    /// and turn the answer into decisions scheduled `lead` ticks out (the
    /// lead from the tier's measured p99, D44).
    ///
    /// A refusal produces no decision — it is logged, and the reflex keeps
    /// playing (D26). An answer that matches no flight in the mind's hands
    /// is dropped.
    pub fn on_answer(&mut self, req: Ask, result: Result<Answers, Refusal>) -> Vec<Decision> {
        let now = self.now;
        let Some(flight) = self.in_flight.take() else {
            return Vec::new();
        };
        if flight.req.agent != req.req.agent
            || flight.req.asked != req.req.asked
            || flight.req.situation_key != req.req.situation_key
        {
            // The answer belongs to an ask this mind no longer owns; the
            // flight goes back in its hands.
            self.in_flight = Some(flight);
            return Vec::new();
        }
        if let Some(record) =
            self.thoughts.iter_mut().rev().find(|r| {
                r.landed.is_none() && r.asked == flight.req.asked && r.tier == flight.tier
            })
        {
            record.landed = Some(now);
        }
        // Teach the tier: a refusal burned its deadline, so it counts as the
        // timeout it cost (a slow tier must look slow to the next choice).
        let rtt_ms = match &result {
            Ok(answers) => answers.latency_ms,
            Err(_) => u32::try_from(req.timeout.as_millis()).unwrap_or(u32::MAX),
        };
        self.policy
            .tiers
            .note(&flight.tier, rtt_ms, req.questions_n);
        match result {
            Err(refusal) => {
                self.refusals.push((now, refusal));
                Vec::new()
            }
            Ok(answers) => {
                if self.policy.verdict_cache {
                    self.cache
                        .insert((flight.req.situation_key, flight.menu.id), answers.clone());
                }
                let p99 = self.policy.tiers.tier(&flight.tier).map_or(0, Tier::p99_ms);
                let lead = lead_ticks(&self.params, p99);
                let mut decisions = answers_to_decisions(
                    &self.gates,
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
                // (Task 4).
                if let Some(line) = self.utterance(&answers, &flight, now, lead) {
                    decisions.push(line);
                }
                self.restamp(&mut decisions);
                decisions
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
        // A pack that was cut short never asked the talk gates: the line
        // is unknown, and the agent says nothing. The pack is asked in
        // file order, so a gate is in it when its index is under the
        // pack's length.
        let asked = usize::from(flight.questions_n);
        let covered = UTTERANCE_GATES.iter().all(|gate| {
            self.gates.index_of(gate).is_some_and(|index| index < asked)
                && self
                    .gates
                    .index_of(&presence_gate(gate))
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
        let text = match self.realizer.realize(&plan, &flight.facts) {
            Ok(text) => text,
            Err(refusal) => {
                self.refusals.push((now, refusal));
                return None;
            }
        };
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
    pub fn buying_time(&self) -> bool {
        let Some(flight) = &self.in_flight else {
            return false;
        };
        self.policy.buy_time
            && flight.expected_land.saturating_sub(flight.req.asked)
                > u64::from(self.policy.buy_time_above_ticks)
    }

    /// The thought records, for the log and the piano roll (D55).
    pub fn thoughts(&self) -> &[ThoughtRecord] {
        &self.thoughts
    }

    /// The refusals so far, with the tick they landed on.
    pub fn refusals(&self) -> &[(Tick, Refusal)] {
        &self.refusals
    }

    /// Teach a tier's estimator a measured call (used to pre-seed estimates
    /// and by the tests to script them).
    pub fn note_tier(&mut self, tier: &str, latency_ms: u32, questions: u16) {
        self.policy.tiers.note(tier, latency_ms, questions);
    }

    /// The questions this mind asks per pack.
    ///
    /// The pack is the gates the harness has, capped by the policy's or the
    /// configuration's `max_questions` (D56).
    pub fn pack_len(&self) -> u16 {
        u16::try_from(
            self.gates
                .len()
                .min(usize::from(self.policy.max_questions_of(&self.cfg))),
        )
        .unwrap_or(u16::MAX)
    }

    /// Whether the verdict cache holds this situation on this menu.
    fn cached(&self, situation_key: u64, menu_id: u64) -> bool {
        self.cache.contains_key(&(situation_key, menu_id))
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
}

// -------------------------------------------------------------------- the tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AgentsConfig;
    use crate::queue::{Budgets, LeastUseQueue};
    use crate::sysone::{Answer, StubBackend};
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
        }
    }

    /// A member's view standing at (x, y) on the tiny world.
    fn view_at(id: u16, x: u16, y: u16) -> ScoreView {
        ScoreView {
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
            model: "stub".to_owned(),
            input_tokens: 64,
            latency_ms: 0,
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
            let backends: BTreeMap<String, Box<dyn SysOne + Send>> = BTreeMap::from([(
                "stub".to_owned(),
                Box::new(StubBackend::scripted(script)) as Box<dyn SysOne + Send>,
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

            // 1. The minds look; some ask.
            for (&agent, mind) in &mut self.minds {
                let view = self.sim.view(agent);
                let menu = menu(&view);
                if let Some(req) = mind.on_tick(t, &view, self.sim.world(), &menu) {
                    self.queue.push(req);
                }
            }
            self.queue_high = self.queue_high.max(self.queue.len());

            // 2. The queue picks a batch and the worker is sent it.
            let popped = self.queue.pop_batch(t, 16, &mut self.budgets);
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
                    decisions.extend(mind.on_answer(thought.ask, thought.result));
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
        assert_eq!(backends, vec!["reflex", "thor-d1-3b", "jev"]);
        // A missing or broken policy file means the defaults, never a panic.
        assert_eq!(Policy::for_harness(""), p);
        assert_eq!(Policy::for_harness("tiers = ["), p);
    }

    #[test]
    fn a_bad_policy_is_an_error_and_an_empty_one_has_no_tiers() {
        assert!(Policy::parse("tiers = 3\n").is_err());
        let empty = Policy::parse("").expect("an empty file is the defaults");
        assert!(empty.tiers.tiers().is_empty());
        assert!(empty.pick_tier(true, 2, 30).is_none());
    }

    // ------------------------------------------------------ the tier choice

    #[test]
    fn urgent_picks_fast_tier() {
        let mut p = Policy::parse(TIER_POLICY).unwrap();
        p.tiers.note("fast", 30, 3); // 30 ms = 1 tick
        p.tiers.note("deep", 150, 3); // 150 ms = 5 ticks
                                      // A fresh threat gives 3 ticks: the deep tier (5 ticks) does not
                                      // land in time, so the fast one takes the moment.
        assert_eq!(p.pick_tier(true, 3, 30).unwrap().name(), "fast");
        // Nothing lands within the horizon once both slow down: the reflex
        // plays the moment.
        for _ in 0..RTT_WINDOW {
            p.tiers.note("fast", 300, 3);
        }
        assert!(p.pick_tier(true, 3, 30).is_none());
    }

    #[test]
    fn calm_picks_quality_tier() {
        let mut p = Policy::parse(TIER_POLICY).unwrap();
        p.tiers.note("fast", 30, 3); // 1 tick
        p.tiers.note("deep", 150, 3); // 5 ticks, fits the 6-tick budget
                                      // Quiet thinking has 6 ticks: the deep tier fits and out-ranks fast.
        assert_eq!(p.pick_tier(false, 3, 30).unwrap().name(), "deep");
    }

    #[test]
    fn tier_estimates_adapt() {
        // A stub that slows down shifts the choice to the faster tier.
        let mut p = Policy::parse(TIER_POLICY).unwrap();
        p.tiers.note("fast", 30, 3);
        p.tiers.note("deep", 150, 3);
        assert_eq!(p.pick_tier(false, 3, 30).unwrap().name(), "deep");
        for _ in 0..RTT_WINDOW {
            p.tiers.note("deep", 300, 3); // 9 ticks: no longer fits 6
        }
        assert_eq!(p.pick_tier(false, 3, 30).unwrap().name(), "fast");
        // The p50 follows the window too.
        assert_eq!(p.tiers.tier("deep").unwrap().p50_ms(), 300);
    }

    #[test]
    fn smaller_pack_is_faster_tier() {
        // The per-question estimate scales a tier to the pack being asked
        // (D56 max_questions): the same tier fits the quiet budget with a
        // small pack and misses it with a large one.
        let mut p = Policy::parse(TIER_POLICY).unwrap();
        p.tiers.note("fast", 60, 8); // ~2 ticks either way
        for _ in 0..RTT_WINDOW {
            p.tiers.note("deep", 240, 8); // 30 ms per question
        }
        let deep = p.tiers.tier("deep").unwrap();
        assert_eq!(deep.p99_ms_for(8), 240);
        assert_eq!(deep.p99_ms_for(2), 60);
        assert!(deep.p99_ms_for(2) < deep.p99_ms_for(8));
        // Eight questions cost the deep tier 8 ticks: too slow for 6, so the
        // fast tier takes it. Two questions cost 2 ticks: deep is back.
        assert_eq!(p.pick_tier(false, 8, 30).unwrap().name(), "fast");
        assert_eq!(p.pick_tier(false, 2, 30).unwrap().name(), "deep");
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
        m.note_tier("deep", 200, questions);
        assert_eq!(m.effective_mode(), Mode::Point);
        for _ in 0..RTT_WINDOW {
            m.note_tier("deep", 20, questions);
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
        slow.note_tier("deep", 250, questions);
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
        fast.note_tier("deep", 20, 2);
        let req = fast
            .on_tick(0, &view, &world, &view_menu)
            .expect("a novel moment asks");
        fast.take_flight(&req).expect("the ask flies");
        assert!(!fast.buying_time(), "1 expected tick is not slow");
    }

    // ------------------------------------------------------ the prethink lever

    #[test]
    fn prethink_answer_used_when_situation_arrives() {
        let cfg = AgentsConfig::default();
        let world = tiny_world();
        let view_a = view_at(3, 2, 2);
        let view_b = view_at(3, 5, 5);
        let menu_a = menu(&view_a);
        let menu_b = menu(&view_b);
        let key_a = Facts::from_view(&view_a, &world, 0, &cfg).key;

        let mut m = mind(3, &cfg, STUB_POLICY);

        // The first sight of A asks; the answer is cached under its key.
        let req = m
            .on_tick(0, &view_a, &world, &menu_a)
            .expect("a novel situation asks");
        let ask = m.take_flight(&req).expect("the ask flies");
        let decisions = m.on_answer(ask, Ok(lane_answers(&menu_a, 0)));
        assert_eq!(decisions.len(), 1, "the move gate becomes a lane");

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
        assert!(m.on_answer(ask, Err(Refusal::Timeout)).is_empty());
        let req = m
            .on_tick(8, &view_a, &world, &menu_a)
            .expect("prethink asks about the quiet situation");
        assert_eq!(req.situation_key, key_a);
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
            host.queue_high <= host.minds.len(),
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

    /// Ask once and answer, and hand back the decisions.
    fn think_once(
        mind: &mut Mind,
        view: &ScoreView,
        world: &World,
        answers: Answers,
    ) -> Vec<Decision> {
        let menu = menu(view);
        let req = mind
            .on_tick(0, view, world, &menu)
            .expect("a novel situation asks");
        let ask = mind.take_flight(&req).expect("the ask flies");
        mind.on_answer(ask, Ok(answers))
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
    fn bad_realizer_output_is_a_refusal() {
        let cfg = AgentsConfig::default();
        let world = tiny_world();
        let view = view_at(1, 2, 2);
        // The model answers with something that is not the notation.
        let mut m = speaking_mind(1, &cfg).with_realizer(Box::new(LlmRealizer::new(
            Box::new(StubLlm::new("stub", "hold on")),
            Duration::from_millis(50),
        )));

        let decisions = think_once(&mut m, &view, &world, talk_answers(&menu(&view), 0));
        assert!(
            decisions
                .iter()
                .all(|d| !matches!(d.kind, Kind::Utterance(_))),
            "a line that does not compile is not said: {decisions:?}"
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
