//! The realizer on the real Thor: what the model words, how long it takes,
//! and how often the phrasebook has to step in.
//!
//! The realizer had no model to talk to: nothing served `:8092`, so `auto`
//! fell back to the phrasebook and said nothing about it. This is the
//! measurement of the other side of that: the model on the Thor, the
//! wording it writes, and the lines it misses its deadline on.
//!
//! ## What is measured
//!
//! Three rounds of three thousand ticks -- a hundred seconds at 30 Hz --
//! are played the way the game plays them: the host loop, the agents, the
//! least-use-first queue, the sequencer. The decision tier is the Thor's
//! d1-2B on `:8094` and the wording is the Thor's LFM2-1.2B on `:8092`.
//!
//! Every line the agents speak is recorded with:
//!
//! - the plan it was made from (the gesture, the addressee, the intent,
//!   the affect, the subject);
//! - where the words came from: the model or the phrasebook;
//! - whether the model missed the line's deadline;
//! - how long the wording took;
//! - the line the phrasebook would have worded for the same plan.
//!
//! ## The arm
//!
//! The **arm** -- which model words the lines, and where it lives -- comes
//! from the environment, so the A/B is the same harness four times over
//! and only the door changes:
//!
//! ```text
//! UTTER_LLM_MODEL  the model's name, as the server knows it
//! UTTER_LLM_PORT   the port it serves on
//! UTTER_LLM_BASE   the base URL of the server (default: the Thor's)
//! UTTER_DUMP       where the report is written, as JSON
//! ```
//!
//! With none of them set the arm is the Thor's own: LFM2-1.2B on `:8092`.
//!
//! ## What is checked
//!
//! - the model's line stays within the plan: the same intent, the same
//!   recipient, the same urgency, and the same target;
//! - the model never names a target the gates did not choose (D37).
//!
//! ## The command
//!
//! ```text
//! cargo test --release -p lm-agents --test realizer_on_thor -- --ignored --nocapture
//! ```
//!
//! The write-up of the numbers is `tests/realizer_on_thor.md`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lm_agents::config::AgentsConfig;
use lm_agents::harness::Harness;
use lm_agents::sysone::{Refusal, SysOne, ThorBackend};
use lm_agents::utter::{
    other_target, wording_llm, AutoRealizer, LlmRealizer, TemplateRealizer, UtterPlan, WordSource,
    Worded,
};
use lm_agents::Agents;
use lm_core::header::Header;
use lm_core::opal::{self, TalkContext};
use lm_core::utterance::Subject;
use lm_host_model::HostLoop;
use serde_json::json;

/// The shipped harness: the gates, the policy and the phrasebook the default
/// round thinks with.
const DEFAULT_GATES: &str = include_str!("../../../assets/harness/default/gates.toml");
const DEFAULT_POLICY: &str = include_str!("../../../assets/harness/default/policy.toml");
const DEFAULT_PHRASEBOOK: &str = include_str!("../../../assets/harness/default/phrasebook.toml");

/// The shipped reflex, compiled into the test binary.
const DEFAULT_REFLEX: &str = include_str!("../../../assets/reflexes/default.rhai");

/// The ticks a round is played for: a hundred seconds at 30 Hz.
const TICKS: u64 = 3_000;

/// The seeds the measurement plays.
const SEEDS: [u64; 3] = [1, 2, 3];

/// How many lines the report shows side by side.
const SAMPLES: usize = 30;

/// The arm the measurement plays: the model, and the door it lives behind.
#[derive(Clone, Debug)]
struct Arm {
    /// The model's name, as the server knows it.
    model: String,
    /// The port it serves on.
    port: u16,
    /// The base URL of the server.
    base: String,
    /// The model the ladder falls through to, if the environment names
    /// one.
    fallback: Option<Fallback>,
    /// Where the report is written, if the environment says where.
    dump: Option<PathBuf>,
}

/// The ladder's second model, as the environment spells it.
#[derive(Clone, Debug)]
struct Fallback {
    model: String,
    port: u16,
    base: Option<String>,
}

impl Arm {
    /// The arm the environment asks for.
    ///
    /// With nothing set it is the Thor's own arm: LFM2-1.2B on `:8092`,
    /// the door the realizer has always lived behind. A variable that is
    /// set but **empty** is unset: the sweep exports every arm's
    /// variables to every arm, and the plain arms' fallback spells out
    /// to nothing.
    fn from_env() -> Arm {
        let default = AgentsConfig::default();
        let named = |name: &str| {
            std::env::var(name)
                .ok()
                .filter(|text| !text.trim().is_empty())
        };
        let fallback = named("UTTER_FALLBACK_MODEL").map(|model| Fallback {
            port: named("UTTER_FALLBACK_PORT")
                .and_then(|text| text.parse::<u16>().ok())
                .unwrap_or(default.utter.fallback_port),
            base: named("UTTER_FALLBACK_BASE"),
            model,
        });
        Arm {
            model: named("UTTER_LLM_MODEL").unwrap_or(default.utter.llm_model.clone()),
            port: named("UTTER_LLM_PORT")
                .and_then(|text| text.parse::<u16>().ok())
                .unwrap_or(default.utter.llm_port),
            base: named("UTTER_LLM_BASE").unwrap_or(default.thor_base.clone()),
            fallback,
            dump: std::env::var_os("UTTER_DUMP").map(PathBuf::from),
        }
    }

    /// The configuration this arm plays with.
    fn config(&self) -> AgentsConfig {
        let mut cfg = AgentsConfig::default();
        cfg.utter.llm_model = self.model.clone();
        cfg.utter.llm_port = self.port;
        cfg.utter.llm_base = Some(self.base.clone());
        if let Some(fallback) = &self.fallback {
            cfg.utter.fallback_model = Some(fallback.model.clone());
            cfg.utter.fallback_port = fallback.port;
            cfg.utter.fallback_base = fallback.base.clone();
        }
        cfg
    }

    /// The arm's name, for the report: the model, and the ladder if there
    /// is one.
    fn name(&self) -> String {
        match &self.fallback {
            None => format!("{} on {}:{}", self.model, self.base, self.port),
            Some(fallback) => format!(
                "{} on {}:{} then {} on {}:{}",
                self.model,
                self.base,
                self.port,
                fallback.model,
                fallback.base.as_deref().unwrap_or("http://127.0.0.1"),
                fallback.port
            ),
        }
    }
}

/// One line the agents spoke, and what it cost.
#[derive(Clone, Debug)]
struct Line {
    /// The plan the line was made from.
    plan: UtterPlan,
    /// Where the words came from.
    source: WordSource,
    /// Whether the model missed the line's deadline.
    late: bool,
    /// How long the wording took, in microseconds.
    micros: u64,
    /// The line, in the notation.
    text: String,
    /// The line the phrasebook would have worded for the same plan.
    phrasebook: Option<String>,
}

/// The realizer the measurement watches.
///
/// It is the shipped `auto` realizer -- the model first, the phrasebook for
/// the lines the model misses -- with a log around it.
struct Watcher {
    inner: Box<dyn lm_agents::utter::Realizer>,
    auto: Arc<AutoRealizer>,
    template: TemplateRealizer,
    log: Arc<Mutex<Vec<Line>>>,
}

impl Watcher {
    /// A watcher over the shipped realizer, with a log of its own.
    fn new(cfg: &AgentsConfig, template: TemplateRealizer, log: Arc<Mutex<Vec<Line>>>) -> Watcher {
        let deadline = Duration::from_millis(u64::from(cfg.utter.deadline_ms));
        let auto = Arc::new(AutoRealizer::new(
            template.clone(),
            LlmRealizer::new(wording_llm(cfg, deadline), deadline),
        ));
        Watcher {
            inner: Box::new(AutoRealizerProxy(Arc::clone(&auto))),
            auto,
            template,
            log,
        }
    }
}

/// The realizer behind the watcher.
///
/// `AutoRealizer` is not `Clone`, so the watcher holds it in an `Arc` and
/// this is the handle the trait is implemented on.
struct AutoRealizerProxy(Arc<AutoRealizer>);

impl lm_agents::utter::Realizer for AutoRealizerProxy {
    fn realize(
        &self,
        plan: &UtterPlan,
        facts: &lm_agents::facts::Facts,
    ) -> Result<Worded, Refusal> {
        self.0.realize(plan, facts)
    }
}

impl lm_agents::utter::Realizer for Watcher {
    fn realize(
        &self,
        plan: &UtterPlan,
        facts: &lm_agents::facts::Facts,
    ) -> Result<Worded, Refusal> {
        let started = Instant::now();
        let worded = self.inner.realize(plan, facts)?;
        let micros = started.elapsed().as_micros() as u64;
        self.log.lock().expect("the watcher's log").push(Line {
            plan: *plan,
            source: worded.source,
            late: worded.late,
            micros,
            text: worded.text.clone(),
            phrasebook: self.template.realize(plan, facts).ok().map(|w| w.text),
        });
        Ok(worded)
    }
}

/// The shipped harness, read into memory.
fn default_harness() -> Harness {
    Harness {
        dir: std::path::PathBuf::from("assets/harness/default"),
        gates_toml: DEFAULT_GATES.to_owned(),
        policy_toml: DEFAULT_POLICY.to_owned(),
        phrasebook_toml: DEFAULT_PHRASEBOOK.to_owned(),
        reflexes: BTreeMap::new(),
        commit: "content:test-harness".to_owned(),
    }
}

/// What one round did.
#[derive(Clone, Debug, Default)]
struct Round {
    /// The lines the agents spoke.
    utterances: u64,
    /// The decisions the agents' answers became.
    decisions: u64,
    /// The thoughts the model refused.
    refusals: u64,
    /// The lines the model on the Thor worded.
    llm: u64,
    /// The lines the phrasebook worded.
    phrasebook: u64,
    /// The lines the model missed its deadline on.
    late: u64,
}

/// Play one round on the Thor, at the round's own rate, and measure it.
///
/// The round is the same whatever the arm is: the same seed, the same
/// harness, the same decision tier on the Thor. Only the door the wording
/// comes through is the arm's.
fn play(seed: u64, arm: &Arm) -> (Round, Vec<Line>, Vec<u64>, u64) {
    let header = Header::default_round(seed, DEFAULT_REFLEX);
    let harness = default_harness();
    let cfg = arm.config();

    let backend = ThorBackend::new(&cfg.thor_base, cfg.ports["d1-2b"], "d1-2b")
        .with_min_confidence(0.30)
        .with_timeout(Duration::from_millis(u64::from(cfg.request_timeout_ms)));
    let backends = BTreeMap::from([(
        "thor-d1-2b".to_owned(),
        Box::new(backend) as Box<dyn SysOne + Send + Sync>,
    )]);

    let mut agents = Agents::new(&cfg, &harness, &header, backends);
    // The watcher: the shipped `auto` realizer, with a log around it.
    let log: Arc<Mutex<Vec<Line>>> = Arc::new(Mutex::new(Vec::new()));
    let banks: Arc<Mutex<Vec<Arc<AutoRealizer>>>> = Arc::new(Mutex::new(Vec::new()));
    {
        let log = Arc::clone(&log);
        let cfg = cfg.clone();
        let banks = Arc::clone(&banks);
        let template = TemplateRealizer::shipped();
        agents.with_realizer(move |_agent| {
            let watcher = Watcher::new(&cfg, template.clone(), Arc::clone(&log));
            banks
                .lock()
                .expect("the banks")
                .push(Arc::clone(&watcher.auto));
            Box::new(watcher)
        });
    }

    let mut host = HostLoop::new(header.clone()).expect("the round starts");
    host.attach_agents(agents);

    let period = Duration::from_millis(1000 / u64::from(header.params.tick_hz));
    for _ in 0..TICKS {
        std::thread::sleep(period);
        host.tick();
    }

    let stats = host.agent_stats().expect("the round has agents");
    let round = Round {
        utterances: stats.words.llm + stats.words.phrasebook,
        decisions: stats.decisions,
        refusals: stats.refusals,
        llm: stats.words.llm,
        phrasebook: stats.words.phrasebook,
        late: stats.words.late,
    };
    let log = log.lock().expect("the watcher's log").clone();
    let banks = banks.lock().expect("the banks").clone();
    let mut latencies: Vec<u64> = banks.iter().flat_map(|bank| bank.latencies()).collect();
    latencies.sort_unstable();
    let misses: u64 = banks.iter().map(|bank| bank.misses()).sum();
    (round, log, latencies, misses)
}

/// The percentile of a sorted slice, by rank.
fn percentile(sorted: &[u64], q: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = ((sorted.len() - 1) as f64 * q).round() as usize;
    sorted[rank]
}

/// The measurement: three rounds on the Thor, and what the wording did.
#[test]
#[ignore = "needs the Thor at 192.168.0.78; run with --release --ignored --nocapture"]
fn realizer_on_thor() {
    let arm = Arm::from_env();
    let mut rounds = Vec::new();
    let mut lines: Vec<Line> = Vec::new();
    let mut latencies: Vec<u64> = Vec::new();
    let mut misses: u64 = 0;
    for seed in SEEDS {
        let (round, log, measured, missed) = play(seed, &arm);
        latencies.extend(measured);
        misses += missed;
        println!(
            "round {seed}: {} lines, {} decisions, {} refusals, {} by the model, \
             {} by the phrasebook, {} late",
            round.utterances,
            round.decisions,
            round.refusals,
            round.llm,
            round.phrasebook,
            round.late
        );
        lines.extend(log);
        rounds.push(round);
    }

    let utterances: u64 = rounds.iter().map(|r| r.utterances).sum();
    let llm: u64 = rounds.iter().map(|r| r.llm).sum();
    let phrasebook: u64 = rounds.iter().map(|r| r.phrasebook).sum();
    let late: u64 = rounds.iter().map(|r| r.late).sum();

    // The latency of a line's wording: the model's own round trips, as
    // the wording thread measured them. The game waits for none of it.
    let mut llm_micros = latencies;
    llm_micros.sort_unstable();

    // The share that fell back: the lines the phrasebook worded, over all
    // the lines that were worded.
    let share = if utterances == 0 {
        0.0
    } else {
        100.0 * phrasebook as f64 / utterances as f64
    };

    println!();
    println!("# the realizer on the real Thor");
    println!();
    println!("- the arm: {}", arm.name());
    println!("- rounds: {}", rounds.len());
    println!("- lines: {utterances}");
    println!("- words llm {llm} / phrasebook {phrasebook} / late {late}");
    println!("- the share that fell back: {share:.1}%");
    println!("- the wordings that never came: {misses}");
    println!(
        "- the wording's latency: p50 {} ms, p95 {} ms (n={})",
        percentile(&llm_micros, 0.50) as f64 / 1000.0,
        percentile(&llm_micros, 0.95) as f64 / 1000.0,
        llm_micros.len()
    );
    println!(
        "- the wording's latency: min {} ms, max {} ms",
        llm_micros.first().copied().unwrap_or(0) as f64 / 1000.0,
        llm_micros.last().copied().unwrap_or(0) as f64 / 1000.0
    );

    // The check: the model's line stays within the plan.
    let mut checked = 0;
    let mut off_plan = Vec::new();
    let mut wrong_target = Vec::new();
    for line in lines.iter().filter(|l| l.source == WordSource::Llm) {
        checked += 1;
        let ctx = TalkContext {
            speaker: 0,
            roster: &[],
        };
        let utterance = match opal::parse(&ctx, &line.text) {
            Ok(utterance) => utterance,
            Err(_) => {
                off_plan.push(line.text.clone());
                continue;
            }
        };
        if utterance.intent != line.plan.intent
            || utterance.addressee != line.plan.addressee
            || utterance.affect != line.plan.affect
            || utterance.act != line.plan.act
            || utterance.subject != line.plan.subject
        {
            off_plan.push(line.text.clone());
        }
        let words: Vec<String> = utterance
            .words
            .iter()
            .map(|word| word.text.clone())
            .collect();
        if let Some(other) = other_target(&words.join(" "), line.plan.subject) {
            wrong_target.push(format!("{other} in {}", line.text.replace('\n', " / ")));
        }
    }
    // The on-plan rate: the share of the model's own lines that stayed on
    // the plan -- the tags it was given, and the target it did not name
    // against the plan.
    let on_plan = checked - off_plan.len() - wrong_target.len();
    let on_plan_rate = if checked == 0 {
        0.0
    } else {
        100.0 * on_plan as f64 / checked as f64
    };
    // The game's own wait for the wording: the bank makes it zero.
    let waits: Vec<u64> = lines.iter().map(|line| line.micros).collect();
    println!(
        "- the game's wait for a line's wording: max {} us over {} lines",
        waits.iter().copied().max().unwrap_or(0),
        waits.len()
    );
    // The bank is keyed by the plan, so the number of distinct plans is
    // the number of wordings the model was asked for.
    let mut plans: Vec<String> = lines
        .iter()
        .map(|line| format!("{:?}", line.plan))
        .collect();
    plans.sort();
    plans.dedup();
    println!("- the distinct plans the rounds used: {}", plans.len());
    println!("- lines checked against their plan: {checked}");
    println!("- lines off their plan: {}", off_plan.len());
    println!("- lines naming another target: {}", wrong_target.len());
    println!("- the on-plan rate: {on_plan_rate:.1}%");
    // The lines the phrasebook worded that were not late: the model
    // answered in time, and what it wrote was refused.
    let wrong = lines
        .iter()
        .filter(|line| line.source == WordSource::Phrasebook && !line.late)
        .count();
    println!("- lines the model answered in time and got wrong: {wrong}");
    for line in off_plan.iter().take(5) {
        println!("  off plan: {}", line.replace('\n', " / "));
    }
    for line in wrong_target.iter().take(5) {
        println!("  wrong target: {line}");
    }

    // The samples: the model and the phrasebook, side by side, for the
    // same plans.
    let samples: Vec<&Line> = lines
        .iter()
        .filter(|line| line.source == WordSource::Llm && line.phrasebook.is_some())
        .take(SAMPLES)
        .collect();
    println!();
    println!("## the samples");
    println!();
    for (index, line) in samples.iter().enumerate() {
        println!("### {index}");
        println!();
        println!("```text");
        println!("model:");
        println!("{}", line.text);
        println!("phrasebook:");
        println!("{}", line.phrasebook.as_deref().unwrap_or("(none)"));
        println!("```");
        println!();
        println!(
            "- plan: {} to {}, {}, {}, about {}",
            lm_agents::phrasebook::act_name(line.plan.act),
            match line.plan.addressee {
                lm_core::utterance::Addressee::Squad => "the squad".to_owned(),
                lm_core::utterance::Addressee::All => "everybody".to_owned(),
                lm_core::utterance::Addressee::Agent(agent) => format!("agent {agent}"),
            },
            lm_agents::phrasebook::intent_name(line.plan.intent),
            lm_agents::phrasebook::affect_name(line.plan.affect),
            match line.plan.subject {
                Subject::Beacon(beacon) => format!("beacon {beacon}"),
                Subject::Enemy(enemy) => format!("enemy {enemy}"),
                Subject::Direction(direction) => format!("direction {direction}"),
                Subject::None => "nothing".to_owned(),
            },
        );
        println!();
    }

    // The dump: the arm's numbers, as JSON, for the sweep that runs the
    // arms one after another.
    if let Some(path) = arm.dump.as_ref() {
        let dump = json!({
            "arm": {
                "model": arm.model,
                "base": arm.base,
                "port": arm.port,
                "ladder": arm.name(),
            },
            "rounds": rounds.iter().map(|round| json!({
                "utterances": round.utterances,
                "decisions": round.decisions,
                "refusals": round.refusals,
                "llm": round.llm,
                "phrasebook": round.phrasebook,
                "late": round.late,
            })).collect::<Vec<_>>(),
            "lines": utterances,
            "llm": llm,
            "phrasebook": phrasebook,
            "late": late,
            "misses": misses,
            "fallback_pct": share,
            "latency": {
                "n": llm_micros.len(),
                "p50_ms": percentile(&llm_micros, 0.50) as f64 / 1000.0,
                "p95_ms": percentile(&llm_micros, 0.95) as f64 / 1000.0,
                "min_ms": llm_micros.first().copied().unwrap_or(0) as f64 / 1000.0,
                "max_ms": llm_micros.last().copied().unwrap_or(0) as f64 / 1000.0,
            },
            "on_plan": {
                "checked": checked,
                "off_plan": off_plan.len(),
                "wrong_target": wrong_target.len(),
                "rate_pct": on_plan_rate,
            },
            "wait_max_us": waits.iter().copied().max().unwrap_or(0),
            "distinct_plans": plans.len(),
            "wrong_in_time": wrong,
            "samples": samples.iter().map(|line| json!({
                "plan": format!("{:?}", line.plan),
                "model": line.text,
                "phrasebook": line.phrasebook,
            })).collect::<Vec<_>>(),
        });
        std::fs::write(path, dump.to_string()).expect("the dump is written");
        println!("- the dump: {}", path.display());
    }

    // The acceptance: the model was asked, and the rounds say so.
    assert!(utterances > 0, "the agents spoke");
    assert!(llm > 0, "the model worded some lines");
    assert_eq!(llm + phrasebook, utterances, "every line was worded");
    assert!(late <= phrasebook, "a late line is a phrasebook line");
    assert!(
        off_plan.is_empty(),
        "the model's lines stayed within their plan"
    );
    assert!(
        wrong_target.is_empty(),
        "the model never named a target the gates did not choose"
    );
}
