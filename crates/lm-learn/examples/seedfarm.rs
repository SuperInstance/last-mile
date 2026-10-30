//! The seed farm's bench.
//!
//! Two measurements and one experiment, all on this machine:
//!
//! * `timing <ticks>` — how long the held-out evaluation takes at 4, 16,
//!   64, 256 and 1_024 seeds, in parallel and in serial.
//! * `serial <ticks> <seeds>` — one serial timing, for the counts that are
//!   too slow to time twice.
//! * `verdicts <ticks>` — thirty proposals the flow would make, each
//!   judged on 4 seeds and on 256: how many verdicts flip, which way they
//!   flip, and what the statistics say about it.
//! * `probe <seeds>` — which metrics a tick horizon can see at all, and
//!   the deltas of a few candidates there. A metric that is zero on every
//!   seed at some horizon cannot be moved by any proposal, so the verdict
//!   experiment is only meaningful where the probe says the metric lives
//!   (at the default 1_500 ticks, not at 300).
//!
//! Nothing here writes into a flow record or a ledger. These are
//! measurements, not evidence: the chain keeps no wall-clock value, and
//! this bench keeps none either. Every number it prints can be read
//! again by running it twice.
//!
//! ```text
//! cargo run --release -p lm-learn --example seedfarm -- timing 300
//! cargo run --release -p lm-learn --example seedfarm -- verdicts 1500
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use lm_core::header::Header;
use lm_learn::eval::metric_value;
use lm_learn::eval::{eval_seeds, evaluate, evaluate_serial, worker_count};
use lm_learn::flow::{run_learning_round, FlowConfig, StubReviewer, DEFAULT_REFLEX, DEFAULT_TICKS};
use lm_learn::ledger::Direction;
use lm_learn::metrics::RoundMetrics;
use lm_learn::round::play;

/// The counts the farm measures.
const COUNTS: [usize; 5] = [4, 16, 64, 256, 1024];

/// The counts the verdict experiment judges at.
const FEW: usize = 4;
const MANY: usize = 256;

/// The scratch directory everything is written under.
fn scratch() -> PathBuf {
    let dir = PathBuf::from("runs/seedfarm");
    fs::create_dir_all(&dir).expect("the scratch directory is made");
    dir
}

/// A harness root: one agent, with the shipped reflex in it.
fn harness_root(dir: &Path) -> PathBuf {
    let root = dir.join("harness");
    let agent = root.join("a0");
    fs::create_dir_all(agent.join("reflexes")).expect("the harness is made");
    fs::write(agent.join("reflexes/default.rhai"), DEFAULT_REFLEX).expect("the reflex is written");
    fs::write(agent.join("gates.toml"), "# the agent's gates\n").expect("the gates are written");
    fs::write(agent.join("policy.toml"), "# the agent's policy\n").expect("the policy is written");
    root
}

/// The baseline of the timing: the shipped reflex.
fn baseline(seed: u64) -> Header {
    Header::default_round(seed, DEFAULT_REFLEX)
}

/// The candidate of the timing: the reflex that camps on the beacon.
fn candidate(seed: u64) -> Header {
    Header::default_round(seed, &camp_reflex(Camp::Always))
}

/// A sealed prediction over `n` held-out seeds.
fn prediction(n: usize) -> lm_learn::ledger::Prediction {
    lm_learn::ledger::Prediction {
        round_id: "r1".into(),
        proposal_hash: "0123456789abcdef".into(),
        metric: "hold_ticks_squad0".into(),
        direction: Direction::Up,
        min_delta: 1,
        eval_seeds: eval_seeds(&[], n, 1),
        author: "seedfarm".into(),
        text: "the squad holds the beacon for longer".into(),
    }
}

/// How long the evaluation takes at each count.
///
/// The parallel run is the one the flow uses. The serial run is the same
/// measurement, one seed after the other, and is timed only where it is
/// cheap enough to time twice.
fn timing(ticks: u64) {
    println!("## timing at {ticks} ticks");
    println!("cores the farm can use: {}", worker_count(1_024));
    println!();
    println!("| seeds | parallel | serial | speed-up |");
    println!("|---:|---:|---:|---:|");
    for n in COUNTS {
        let pred = prediction(n);
        let start = Instant::now();
        let verdict =
            evaluate(&baseline, &candidate, ticks, &pred, 0).expect("the evaluation runs");
        let parallel = start.elapsed();
        let serial = if n <= 64 {
            let start = Instant::now();
            let other = evaluate_serial(&baseline, &candidate, ticks, &pred, 0)
                .expect("the serial evaluation runs");
            assert_eq!(
                verdict, other,
                "the parallel verdict differs from the serial one"
            );
            Some(start.elapsed())
        } else {
            None
        };
        match serial {
            Some(serial) => println!(
                "| {n} | {:.3} s | {:.3} s | {:.1}x |",
                parallel.as_secs_f64(),
                serial.as_secs_f64(),
                serial.as_secs_f64() / parallel.as_secs_f64().max(1e-9)
            ),
            None => println!("| {n} | {:.3} s | not timed | - |", parallel.as_secs_f64()),
        }
    }
    println!();
}

/// One serial timing, for the counts that are too slow to time twice.
fn serial(ticks: u64, seeds: usize) {
    let pred = prediction(seeds);
    let start = Instant::now();
    let verdict =
        evaluate_serial(&baseline, &candidate, ticks, &pred, 0).expect("the evaluation runs");
    let serial = start.elapsed();
    let start = Instant::now();
    let parallel_verdict =
        evaluate(&baseline, &candidate, ticks, &pred, 0).expect("the evaluation runs");
    let parallel = start.elapsed();
    assert_eq!(verdict, parallel_verdict, "the two runs differ");
    println!("## serial at {ticks} ticks, {seeds} seeds");
    println!("serial:   {:.3} s", serial.as_secs_f64());
    println!("parallel: {:.3} s", parallel.as_secs_f64());
    println!(
        "speed-up: {:.1}x",
        serial.as_secs_f64() / parallel.as_secs_f64().max(1e-9)
    );
    println!();
}

/// The change a proposal asks for.
enum Change {
    /// A new reflex for an agent.
    Reflex { agent: u16, source: String },
    /// A word for a squad.
    Lexicon { squad: u8, text: String },
    /// A gate for an agent.
    Gate { agent: u16, toml: String },
    /// A policy key for an agent.
    Policy { agent: u16, key: String, value: i64 },
}

/// One proposal the flow could make, and the claim it comes with.
#[derive(Clone)]
struct Proposed {
    /// A short name, for the report.
    name: String,
    /// The reviewer's words: the flow parses these.
    review: String,
    /// The metric the claim is about.
    metric: String,
    /// Which way the claim says the metric moves.
    direction: Direction,
    /// The bar the claim sets.
    min_delta: i64,
}

/// A proposal, with the claim the reviewer sealed with it.
///
/// The claim is in the reviewer's words, because that is where the flow
/// reads it from: the report must say what was sealed, not what was
/// meant.
fn proposed(
    name: String,
    change: Change,
    metric: &str,
    direction: Direction,
    min_delta: i64,
) -> Proposed {
    let claim = format!(
        r#"{{"metric": "{metric}", "direction": "{}", "min_delta": {min_delta}}}"#,
        match direction {
            Direction::Up => "Up",
            Direction::Down => "Down",
        }
    );
    let proposal = match change {
        Change::Reflex { agent, source } => {
            let source = serde_json::to_string(&source).expect("the source is json");
            format!(
                r#"{{"kind": "EditReflex", "agent": {agent}, "name": "default", "source": {source}}}"#
            )
        }
        Change::Lexicon { squad, text } => {
            let text = serde_json::to_string(&text).expect("the text is json");
            format!(r#"{{"kind": "LexiconEntry", "squad": {squad}, "text": {text}}}"#)
        }
        Change::Gate { agent, toml } => {
            let toml = serde_json::to_string(&toml).expect("the toml is json");
            format!(r#"{{"kind": "AddGate", "agent": {agent}, "toml": {toml}}}"#)
        }
        Change::Policy { agent, key, value } => {
            format!(
                r#"{{"kind": "PolicyTweak", "agent": {agent}, "key": "{key}", "value": {value}}}"#
            )
        }
    };
    Proposed {
        name,
        review: format!(r#"{{"proposal": {proposal}, "prediction": {claim}}}"#),
        metric: metric.to_string(),
        direction,
        min_delta,
    }
}

/// The thirty proposals the experiment judges.
///
/// They are the flow's own kinds — a reflex edit, a lexicon entry, a gate,
/// a policy tweak — with the claims a reviewer would make about them. The
/// reflex edits are the interesting ones: they camp on the beacon under
/// different conditions, so they help on some seeds and not on others,
/// which is where a verdict can turn on luck.
fn catalogue() -> Vec<Proposed> {
    let mut out: Vec<Proposed> = Vec::new();

    // Camp on the beacon the squad holds, once the round is under way.
    for after in [30u64, 60, 90, 120, 150, 180] {
        out.push(proposed(
            format!("camp after {after} ticks"),
            Change::Reflex {
                agent: 0,
                source: camp_reflex(Camp::After(after)),
            },
            "hold_ticks_squad0",
            Direction::Up,
            1,
        ));
    }

    // Camp on every third tick, and the like: a partial change.
    for every in [2u64, 3, 4, 5, 6, 8] {
        out.push(proposed(
            format!("camp every {every} ticks"),
            Change::Reflex {
                agent: 0,
                source: camp_reflex(Camp::Every(every)),
            },
            "hold_ticks_squad0",
            Direction::Up,
            1,
        ));
    }

    // Stand still: from `after` on, the agent never holds anything again,
    // so the metric falls by every hold tick it would have had. The claim
    // says down, and the change means it: the family gives the experiment
    // its true-helps cases in the other direction.
    for after in [30u64, 60, 120, 240] {
        out.push(proposed(
            format!("stand still after {after} ticks"),
            Change::Reflex {
                agent: 0,
                source: idle_reflex(after),
            },
            "hold_ticks_squad0",
            Direction::Down,
            1,
        ));
    }

    // The same change, asked for by different agents: the leader's version
    // matters more than a member's.
    for agent in [0u16, 1, 2, 3] {
        out.push(proposed(
            format!("agent {agent} camps on the beacon"),
            Change::Reflex {
                agent,
                source: camp_reflex(Camp::Always),
            },
            "hold_ticks_squad0",
            Direction::Up,
            1,
        ));
    }

    // The same change, with a bar that sweeps the pass rule's boundary: at
    // the default 1_500 ticks the camp reflex moves hold_ticks_squad0 by
    // roughly 35..110 ticks on a seed, so the rungs below take the
    // qualified fraction from every seed down to a handful of them. A
    // verdict can only flip near the bar, so the ladder is where the
    // experiment looks.
    for min_delta in [5i64, 20, 35, 45, 55, 70, 100] {
        out.push(proposed(
            format!("camp on the beacon, at least {min_delta} more ticks"),
            Change::Reflex {
                agent: 0,
                source: camp_reflex(Camp::Always),
            },
            "hold_ticks_squad0",
            Direction::Up,
            min_delta,
        ));
    }

    // The changes that cannot move a metric at all: a word, a gate, a
    // policy key.
    out.push(proposed(
        "a lexicon entry for squad 0".to_string(),
        Change::Lexicon {
            squad: 0,
            text: "the beacon we hold is ours".to_string(),
        },
        "hold_ticks_squad0",
        Direction::Up,
        1,
    ));
    out.push(proposed(
        "a gate for agent 0".to_string(),
        Change::Gate {
            agent: 0,
            toml: "[[gate]]\nname = \"may_hold_beacon\"\n".to_string(),
        },
        "hold_ticks_squad0",
        Direction::Up,
        1,
    ));
    out.push(proposed(
        "the policy of agent 0: hold_beacon = 1".to_string(),
        Change::Policy {
            agent: 0,
            key: "hold_beacon".to_string(),
            value: 1,
        },
        "hold_ticks_squad0",
        Direction::Up,
        1,
    ));

    out
}

/// When the reflex camps on the beacon the squad holds.
enum Camp {
    /// Always.
    Always,
    /// Once the round is under way.
    After(u64),
    /// Every n-th tick.
    Every(u64),
}

/// The reflex that camps on the beacon the squad holds.
///
/// It is the shipped reflex with one rule changed: the beacon the squad
/// holds is worth more than the next one.
fn camp_reflex(camp: Camp) -> String {
    let mut s = String::new();
    s.push_str(PREAMBLE);
    s.push_str("fn reflex(v) {\n");
    s.push_str("    let me = v.me.cell;\n");
    s.push_str("    let t = threats(v, me);\n");
    s.push_str("    if t[0] > 0 {\n");
    s.push_str("        let target = v.enemies[t[1]].cell;\n");
    s.push_str("        if allies_near(v, me) + 1 >= t[0] {\n");
    s.push_str("            return #{ \"goto\": [target[0], target[1]] };\n");
    s.push_str("        }\n");
    s.push_str("        return flee(me, target);\n");
    s.push_str("    }\n");
    match camp {
        Camp::Always => s.push_str(CAMP),
        Camp::After(after) => {
            s.push_str(&format!("    if v.tick > {after} {{\n"));
            s.push_str(CAMP);
            s.push_str("    }\n");
        }
        Camp::Every(every) => {
            s.push_str(&format!("    if v.tick % {every} == 0 {{\n"));
            s.push_str(CAMP);
            s.push_str("    }\n");
        }
    }
    s.push_str(DEFAULT_STEP);
    s.push_str("}\n");
    s
}

/// The reflex that stands still once the round is under way.
fn idle_reflex(after: u64) -> String {
    let mut s = String::new();
    s.push_str(PREAMBLE);
    s.push_str("fn reflex(v) {\n");
    s.push_str("    let me = v.me.cell;\n");
    s.push_str(&format!("    if v.tick > {after} {{\n"));
    // The hold has to be returned, not just built: a map on its own line
    // inside the block is a statement the round throws away, and the agent
    // would walk the default step after all.
    s.push_str("        return #{ hold: true };\n");
    s.push_str("    }\n");
    s.push_str("    let t = threats(v, me);\n");
    s.push_str("    if t[0] > 0 {\n");
    s.push_str("        let target = v.enemies[t[1]].cell;\n");
    s.push_str("        if allies_near(v, me) + 1 >= t[0] {\n");
    s.push_str("            return #{ \"goto\": [target[0], target[1]] };\n");
    s.push_str("        }\n");
    s.push_str("        return flee(me, target);\n");
    s.push_str("    }\n");
    s.push_str(DEFAULT_STEP);
    s.push_str("}\n");
    s
}

/// The helpers every variant shares.
const PREAMBLE: &str = r#"
fn chebyshev(a, b) {
    let dx = if a[0] > b[0] { a[0] - b[0] } else { b[0] - a[0] };
    let dy = if a[1] > b[1] { a[1] - b[1] } else { b[1] - a[1] };
    if dx > dy { dx } else { dy }
}

fn step_away(d) {
    if d > 0 { 4 } else if d < 0 { -4 } else { 0 }
}

fn clamp_min(v) {
    if v < 0 { 0 } else { v }
}

fn flee(me, at) {
    let dx = me[0] - at[0];
    let dy = me[1] - at[1];
    if dx == 0 && dy == 0 {
        return #{ hold: true };
    }
    let x = clamp_min(me[0] + step_away(dx));
    let y = clamp_min(me[1] + step_away(dy));
    #{ "goto": [x, y] }
}

fn nearest_of(v, me, want_ours) {
    let best = -1;
    let best_distance = 0;
    for i in 0..v.beacons.len() {
        let beacon = v.beacons[i];
        let ours = beacon.holder == v.me.squad;
        if want_ours == 1 && !ours { continue; }
        if want_ours == 0 && ours { continue; }
        let distance = chebyshev(me, beacon.cell);
        if best < 0 || distance < best_distance {
            best = i;
            best_distance = distance;
        }
    }
    best
}

fn free_beacon(v, me) {
    let best = -1;
    let best_distance = 0;
    for i in 0..v.beacons.len() {
        let beacon = v.beacons[i];
        if beacon.holder < 0 || beacon.holder != v.me.squad {
            let distance = chebyshev(me, beacon.cell);
            if best < 0 || distance < best_distance {
                best = i;
                best_distance = distance;
            }
        }
    }
    best
}

fn allies_near(v, me) {
    let n = 0;
    for mate in v.squad {
        if mate.id != v.me.id && !mate.stunned && chebyshev(me, mate.cell) <= 4 {
            n = n + 1;
        }
    }
    n
}

fn threats(v, me) {
    let count = 0;
    let nearest = -1;
    let nearest_d = 0;
    for i in 0..v.enemies.len() {
        let e = v.enemies[i];
        if e.age_ticks <= 6 {
            let d = chebyshev(me, e.cell);
            if d <= 6 {
                count = count + 1;
                if nearest < 0 || d < nearest_d {
                    nearest = i;
                    nearest_d = d;
                }
            }
        }
    }
    [count, nearest, nearest_d]
}
"#;

/// Camp on the beacon the squad holds, and take the next one if it holds
/// none.
const CAMP: &str = r#"    let mine = nearest_of(v, me, 1);
    if mine >= 0 {
        return #{ "goto": [v.beacons[mine].cell[0], v.beacons[mine].cell[1]] };
    }
"#;

/// The shipped reflex's last step: take the nearest beacon nobody holds.
const DEFAULT_STEP: &str = r#"    let target = free_beacon(v, me);
    if target >= 0 {
        return #{ "goto": [v.beacons[target].cell[0], v.beacons[target].cell[1]] };
    }
    #{ hold: true }
"#;

/// What one proposal's two verdicts came to.
struct Judged {
    proposal: Proposed,
    /// The verdict on four seeds.
    few: lm_learn::ledger::Verdict,
    /// The verdict on 256 seeds.
    many: lm_learn::ledger::Verdict,
}

/// What the deltas look like, at a few tick counts.
///
/// The probe answers one question: how long a round has to run before a
/// change shows up in the metric at all.
fn probe(seeds: usize) {
    println!("## probe: the deltas of a few candidates");
    for ticks in [300u64, 600, 900, 1_500] {
        // Which metrics the horizon can even see: a metric that is zero on
        // every seed at these ticks cannot be moved by any proposal, and
        // no seed count will ever adopt a claim on it.
        let base = baseline_average(ticks, seeds);
        println!(
            "{ticks:>5} ticks: baseline hold {}  tags {}  margin {}",
            base[0], base[1], base[2]
        );
        for (name, source) in [
            ("camp", camp_reflex(Camp::Always)),
            ("camp after 120", camp_reflex(Camp::After(120))),
            ("camp every 3", camp_reflex(Camp::Every(3))),
            ("idle", idle_reflex(0)),
        ] {
            let pred = prediction(seeds);
            let candidate = move |seed: u64| Header::default_round(seed, &source);
            let verdict =
                evaluate(&baseline, &candidate, ticks, &pred, 0).expect("the evaluation runs");
            let deltas: Vec<i64> = verdict.deltas.iter().map(|(_, delta)| *delta).collect();
            let moved = deltas.iter().filter(|delta| **delta != 0).count();
            println!(
                "{ticks:>5} ticks  {name:<16} baseline {:>6}  moved {moved}/{seeds}  {deltas:?}",
                base[0]
            );
        }
    }
    println!();
}

/// The baseline's own averages of the metrics a claim can name, at a tick
/// count: hold ticks, tags, and the score margin.
fn baseline_average(ticks: u64, seeds: usize) -> Vec<i64> {
    const NAMES: [&str; 3] = ["hold_ticks_squad0", "tags_for_squad0", "margin_squad0"];
    let mut totals = vec![0i64; NAMES.len()];
    for i in 0..seeds {
        let log = play(baseline(1_000_000 + i as u64), ticks);
        let metrics = RoundMetrics::from_log(&log);
        for (j, name) in NAMES.iter().enumerate() {
            totals[j] += metric_value(&metrics, name).unwrap_or(0);
        }
    }
    totals.iter().map(|t| t / seeds.max(1) as i64).collect()
}

/// Judge every proposal on four seeds and on 256.
///
/// The flow does the work: the proposal is made by the flow's own
/// reviewer, applied by the flow's own applier, and judged by the flow's
/// own evaluator. The only thing that changes between the two runs is the
/// number of held-out seeds.
fn verdicts(ticks: u64, only: Option<usize>) {
    let dir = scratch();
    let catalogue = catalogue();
    let catalogue = match only {
        Some(n) => &catalogue[..n.min(catalogue.len())],
        None => &catalogue[..],
    };
    let mut judged: Vec<Judged> = Vec::new();

    for (i, proposal) in catalogue.iter().enumerate() {
        let few = run_one(&dir, i, FEW, proposal, ticks);
        let many = run_one(&dir, i, MANY, proposal, ticks);

        // The four seeds are the first four of the 256: the same seeds, the
        // same rounds, the same deltas.
        assert_eq!(
            few.deltas,
            many.deltas[..FEW].to_vec(),
            "the {} seeds of the 4-seed run are not the first 4 of the 256-seed run",
            proposal.name
        );

        judged.push(Judged {
            proposal: proposal.clone(),
            few,
            many,
        });
        println!("judged {:2}: {}", i + 1, proposal.name);
    }

    report(&judged);
}

/// Run the flow once, over `seeds` held-out seeds, and hand back the verdict.
fn run_one(
    dir: &Path,
    i: usize,
    seeds: usize,
    proposal: &Proposed,
    ticks: u64,
) -> lm_learn::ledger::Verdict {
    let work = dir.join(format!("p{i}-{seeds}"));
    let harness = harness_root(&work);
    let cfg = FlowConfig::new(1_000 + i as u64, harness)
        .with_runs_root(work.join("runs"))
        .with_ticks(ticks)
        .with_eval_seeds(seeds)
        .with_round_id(format!("r{i}-{seeds}"));
    let mut reviewer = StubReviewer::new(proposal.review.clone());
    let record = run_learning_round(&cfg, &mut reviewer).expect("the round runs");
    record.verdict.expect("the round judged the prediction")
}

/// What the experiment found.
///
/// The truth about a proposal is read off the 256-seed run: the change
/// really helps when the qualified fraction clears the bar *and* the sign
/// test says the metric moved that way (the rule SEEDFARM.md proposes).
/// A verdict that flips is called by that truth:
///
/// * the four seeds adopted it and the truth says it does not work — a
///   **false adopt**;
/// * the four seeds rejected it and the truth says it does work — a
///   **false reject**;
/// * the four seeds rejected it and the truth is not sure either — the
///   many seeds adopted on the bar alone, and the sign test did not back
///   it.
fn report(judged: &[Judged]) {
    let mut flips = 0;
    let mut false_adopts = 0;
    let mut false_rejects = 0;
    let mut not_sure = 0;

    println!();
    println!("## the verdicts");
    println!();
    println!(
        "| # | proposal | claim | 4 seeds | 256 seeds | qualified | 95% of it | sign test p | flip |"
    );
    println!("|---:|---|---|---|---|---:|---|---:|---|");

    for (i, j) in judged.iter().enumerate() {
        let claim = &j.proposal;
        let qualified = qualified_count(&j.many.deltas, claim.direction, claim.min_delta);
        let fraction = qualified as f64 / j.many.deltas.len() as f64;
        let (low, high) = bootstrap_interval(&j.many.deltas, claim.direction, claim.min_delta);
        let p = sign_test(&j.many.deltas, claim.direction);
        let backed = p < 0.05 && fraction >= 0.75;
        let flip = j.few.adopted != j.many.adopted;
        let kind = if !flip {
            "-".to_string()
        } else {
            flips += 1;
            if j.few.adopted {
                false_adopts += 1;
                "false adopt".to_string()
            } else if backed {
                false_rejects += 1;
                "false reject".to_string()
            } else {
                not_sure += 1;
                "not sure".to_string()
            }
        };
        println!(
            "| {} | {} | {} {} by {} | {} | {} | {:.0}% | [{:.0}%, {:.0}%] | {:.4} | {} |",
            i + 1,
            claim.name,
            claim.metric,
            match claim.direction {
                Direction::Up => "up",
                Direction::Down => "down",
            },
            claim.min_delta,
            if j.few.adopted { "adopt" } else { "reject" },
            if j.many.adopted { "adopt" } else { "reject" },
            fraction * 100.0,
            low * 100.0,
            high * 100.0,
            p,
            kind
        );
    }

    println!();
    println!("## what it means");
    println!();
    println!("proposals: {}", judged.len());
    println!("verdicts that flip: {flips}");
    println!("  of those, false adopts: {false_adopts}");
    println!("  of those, false rejects: {false_rejects}");
    println!("  of those, the many seeds are not sure either: {not_sure}");
    println!();

    // What the rule would have to be to be safe: how often a change that
    // truly helps the bar q is adopted, and how often one that does not is.
    println!("## the rule, and what it costs");
    println!();
    println!("A change that helps on a fraction q of the seeds is adopted by the");
    println!("current rule (three seeds in four) with this probability:");
    println!();
    println!("| true fraction q | 4 seeds | 16 seeds | 64 seeds | 256 seeds |");
    println!("|---:|---:|---:|---:|---:|");
    for q in [0.5, 0.6, 0.7, 0.75, 0.8, 0.9] {
        println!(
            "| {q:.2} | {:.3} | {:.3} | {:.3} | {:.3} |",
            binomial_at_least(4, 3, q),
            binomial_at_least(16, 12, q),
            binomial_at_least(64, 48, q),
            binomial_at_least(256, 192, q)
        );
    }
    println!();
}

/// How many seeds met the claim's bar.
fn qualified_count(deltas: &[(u64, i64)], direction: Direction, min_delta: i64) -> usize {
    deltas
        .iter()
        .filter(|(_, delta)| qualified_delta(direction, min_delta, *delta))
        .count()
}

/// Whether a seed's delta meets the claim's bar.
fn qualified_delta(direction: Direction, min_delta: i64, delta: i64) -> bool {
    match direction {
        Direction::Up => delta >= min_delta,
        Direction::Down => delta <= -min_delta,
    }
}

/// The sign test on the per-seed deltas.
///
/// Under the null ("the change does nothing") each seed's delta is as
/// likely to be up as down, so the number of seeds that moved the way the
/// claim says is a coin toss count. The p-value is how unlikely that
/// count is under the null, in the direction the claim says.
fn sign_test(deltas: &[(u64, i64)], direction: Direction) -> f64 {
    let (up, down) = deltas
        .iter()
        .fold((0usize, 0usize), |(up, down), (_, delta)| match delta {
            d if *d > 0 => (up + 1, down),
            d if *d < 0 => (up, down + 1),
            _ => (up, down),
        });
    let n = up + down;
    if n == 0 {
        return 1.0;
    }
    match direction {
        Direction::Up => binomial_upper_tail(n, up),
        Direction::Down => binomial_upper_tail(n, down),
    }
}

/// P(X >= k) for X ~ Binomial(n, 1/2).
///
/// The tail is summed from the far end: the terms are the masses of
/// X = n, n-1, …, k, each one the last times (n-j)/(j+1), and none of them
/// is bigger than the mass at the middle of the distribution (under 0.06),
/// so the sum stays exact to the double and never overflows. The near half
/// of the distribution is read by symmetry: P(X >= k) with k at or below
/// the middle is 1 - P(X >= n-k+1).
fn binomial_upper_tail(n: usize, k: usize) -> f64 {
    if k == 0 || n == 0 {
        return 1.0;
    }
    if k > n {
        return 0.0;
    }
    if 2 * k <= n {
        return 1.0 - binomial_upper_tail(n, n - k + 1);
    }
    // The mass of X = n, then the masses below it, down to X = k.
    let mut p = 2f64.powi(-(n as i32));
    let mut mass = p;
    for j in 0..(n - k) {
        mass *= (n - j) as f64 / (j + 1) as f64;
        p += mass;
    }
    p.min(1.0)
}

/// P(X >= threshold) for X ~ Binomial(n, q): how often a change that helps
/// on a fraction q of the seeds is adopted.
fn binomial_at_least(n: usize, threshold: usize, q: f64) -> f64 {
    if threshold > n {
        return 0.0;
    }
    if q >= 1.0 {
        return 1.0;
    }
    if q <= 0.0 {
        return if threshold == 0 { 1.0 } else { 0.0 };
    }
    let mut p = 0.0f64;
    // The mass at 0, then the masses up to n.
    let mut mass = (1.0 - q).powi(n as i32);
    for i in 0..=n {
        if i >= threshold {
            p += mass;
        }
        if i < n {
            mass *= q / (1.0 - q) * ((n - i) as f64 / (i + 1) as f64);
        }
    }
    p.min(1.0)
}

/// The bootstrap: how sure is the qualified fraction.
///
/// The seeds are resampled with replacement, and the interval is the 95%
/// per cent of the fractions that come out. The generator is seeded, so the
/// interval is the same every time.
fn bootstrap_interval(deltas: &[(u64, i64)], direction: Direction, min_delta: i64) -> (f64, f64) {
    const ROUNDS: usize = 2_000;
    let mut rng = Rng(0x5EED_FA2E_0000_0001);
    let mut fractions: Vec<f64> = Vec::with_capacity(ROUNDS);
    for _ in 0..ROUNDS {
        let mut qualified = 0usize;
        for _ in 0..deltas.len() {
            let (_, delta) = deltas[rng.below(deltas.len())];
            if qualified_delta(direction, min_delta, delta) {
                qualified += 1;
            }
        }
        fractions.push(qualified as f64 / deltas.len() as f64);
    }
    fractions.sort_by(|a, b| a.partial_cmp(b).expect("the fractions are numbers"));
    // The 2.5th and the 97.5th per cent of the resamples.
    let low = fractions[ROUNDS * 25 / 1000];
    let high = fractions[ROUNDS * 975 / 1000];
    (low, high)
}

/// A small deterministic generator, so the bootstrap says the same thing
/// every time it runs.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let what = args.first().map(String::as_str).unwrap_or("all");
    match what {
        "timing" => {
            let ticks = args
                .get(1)
                .and_then(|t| t.parse::<u64>().ok())
                .unwrap_or(300);
            timing(ticks);
        }
        "serial" => {
            let ticks = args
                .get(1)
                .and_then(|t| t.parse::<u64>().ok())
                .unwrap_or(300);
            let seeds = args
                .get(2)
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(1024);
            serial(ticks, seeds);
        }
        "verdicts" => {
            let ticks = args
                .get(1)
                .and_then(|t| t.parse::<u64>().ok())
                .unwrap_or(300);
            let only = args.get(2).and_then(|n| n.parse::<usize>().ok());
            verdicts(ticks, only);
        }
        "probe" => {
            let seeds = args
                .get(1)
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(8);
            probe(seeds);
        }
        "all" => {
            timing(300);
            timing(DEFAULT_TICKS);
            verdicts(300, None);
        }
        other => {
            eprintln!("seedfarm: unknown job '{other}'");
            eprintln!(
                "seedfarm: 'timing <ticks>', 'serial <ticks> <seeds>', 'verdicts <ticks>' or 'probe <seeds>'"
            );
            std::process::exit(2);
        }
    }
}
