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
//!
//! ## The A/B arm
//!
//! The wording model is configuration (`[utter]` `llm_model` / `llm_port`),
//! so the same harness can word with any model on the LAN:
//! `UTTER_LLM_MODEL` and `UTTER_LLM_PORT` override the `[utter]` defaults
//! for the whole measurement, everything else -- the prompt, the deadline,
//! the seeds, the ticks -- staying the shipped ones. `UTTER_DUMP` names a
//! file every spoken line is written to, so two arms' wordings can be
//! paired over the same plans afterwards (the A/B section of
//! `tests/realizer_on_thor.md`).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lm_agents::config::AgentsConfig;
use lm_agents::harness::Harness;
use lm_agents::sysone::{Refusal, SysOne, ThorBackend};
use lm_agents::utter::{
    other_target, AutoRealizer, LlmRealizer, TemplateRealizer, ThorLlm, UtterPlan, WordSource,
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
            LlmRealizer::new(
                Box::new(ThorLlm::new(
                    &cfg.thor_base,
                    cfg.utter.llm_port,
                    &cfg.utter.llm_model,
                    deadline,
                )),
                deadline,
            ),
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

/// The wording arm of an A/B: the shipped config, with the `[utter]` model
/// and port overridden from the environment when the measurement names
/// them (`UTTER_LLM_MODEL`, `UTTER_LLM_PORT`) and the defaults otherwise.
///
/// Everything else is the shipped round's own, so the two arms of an A/B
/// differ in the wording model and nothing else.
fn arm_config() -> AgentsConfig {
    let mut cfg = AgentsConfig::default();
    if let Ok(model) = std::env::var("UTTER_LLM_MODEL") {
        cfg.utter.llm_model = model;
    }
    if let Ok(port) = std::env::var("UTTER_LLM_PORT") {
        cfg.utter.llm_port = port.parse().expect("UTTER_LLM_PORT is a port");
    }
    cfg
}

/// A plan in plain words, the same words in the samples and the dump.
fn describe(plan: &UtterPlan) -> String {
    format!(
        "{} to {}, {}, {}, about {}",
        lm_agents::phrasebook::act_name(plan.act),
        match plan.addressee {
            lm_core::utterance::Addressee::Squad => "the squad".to_owned(),
            lm_core::utterance::Addressee::All => "everybody".to_owned(),
            lm_core::utterance::Addressee::Agent(agent) => format!("agent {agent}"),
        },
        lm_agents::phrasebook::intent_name(plan.intent),
        lm_agents::phrasebook::affect_name(plan.affect),
        match plan.subject {
            Subject::Beacon(beacon) => format!("beacon {beacon}"),
            Subject::Enemy(enemy) => format!("enemy {enemy}"),
            Subject::Direction(direction) => format!("direction {direction}"),
            Subject::None => "nothing".to_owned(),
        },
    )
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
fn play(seed: u64) -> (Round, Vec<Line>, Vec<u64>) {
    let header = Header::default_round(seed, DEFAULT_REFLEX);
    let harness = default_harness();
    let cfg = arm_config();

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
    let mut latencies: Vec<u64> = banks
        .lock()
        .expect("the banks")
        .iter()
        .flat_map(|bank| bank.latencies())
        .collect();
    latencies.sort_unstable();
    (round, log, latencies)
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
    let mut rounds = Vec::new();
    let mut lines: Vec<Line> = Vec::new();
    let mut latencies: Vec<u64> = Vec::new();
    for seed in SEEDS {
        let (round, log, measured) = play(seed);
        latencies.extend(measured);
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
    let arm = arm_config();
    println!(
        "- the wording model: {} on :{}",
        arm.utter.llm_model, arm.utter.llm_port
    );
    println!("- rounds: {}", rounds.len());
    println!("- lines: {utterances}");
    println!("- words llm {llm} / phrasebook {phrasebook} / late {late}");
    println!("- the share that fell back: {share:.1}%");
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
        println!("- plan: {}", describe(&line.plan));
        println!();
    }

    // The record of every line, as JSON, for the pairing of two arms'
    // wordings over the same plans: the plan, where the words came from,
    // the deadline the model met or missed, the wording's own latency,
    // the line, and the line the phrasebook would have worded.
    if let Ok(path) = std::env::var("UTTER_DUMP") {
        let path = std::path::Path::new(&path);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).expect("the dump's directory is made");
        }
        let mut records = String::new();
        for line in &lines {
            let record = json!({
                "plan": describe(&line.plan),
                "plan_debug": format!("{:?}", line.plan),
                "source": match line.source {
                    WordSource::Llm => "llm",
                    WordSource::Phrasebook => "phrasebook",
                },
                "late": line.late,
                "micros": line.micros,
                "text": line.text,
                "phrasebook": line.phrasebook,
            });
            records.push_str(&record.to_string());
            records.push('\n');
        }
        std::fs::write(path, records).expect("the dump is written");
        println!();
        println!("## the dump: {} lines to {}", lines.len(), path.display());
    }

    // The acceptance: the model was asked, and the rounds say so.
    assert!(utterances > 0, "the agents spoke");
    assert!(llm > 0, "the model on the Thor worded some lines");
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
