//! The between-round screen: what it shows, and what a human's answer does.
//!
//! These tests run the real learning flow -- a round, a reviewer, a sealed
//! prediction, a held-out evaluation -- over a scratch directory of their own,
//! and then look at the screen the host would show and at the files a human's
//! answer would touch. Nothing here reaches the network: the reviewer is a
//! stub and every file lives in a temporary directory.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use lm_host_model::between::{Action, BetweenModel, PredictionView, Section};
use lm_learn::deposits;
use lm_learn::flow::{run_learning_round, Decision, FlowConfig, FlowRecord, StubReviewer};
use lm_learn::ledger::{Direction, Entry, Ledger};

/// The reflex the repository ships: what an agent runs with no harness.
const DEFAULT_REFLEX: &str = include_str!("../../../assets/reflexes/default.rhai");

/// The reflex of D60: keep the beacon the squad holds instead of leaving it
/// to chase or flee the next one.
const HOLD_REFLEX: &str = r#"
fn chebyshev(a, b) {
    let dx = if a[0] > b[0] { a[0] - b[0] } else { b[0] - a[0] };
    let dy = if a[1] > b[1] { a[1] - b[1] } else { b[1] - a[1] };
    if dx > dy { dx } else { dy }
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

fn reflex(v) {
    let me = v.me.cell;
    let mine = nearest_of(v, me, 1);
    if mine >= 0 {
        return #{ "goto": [v.beacons[mine].cell[0], v.beacons[mine].cell[1]] };
    }
    let target = nearest_of(v, me, 0);
    if target >= 0 {
        return #{ "goto": [v.beacons[target].cell[0], v.beacons[target].cell[1]] };
    }
    #{ hold: true }
}
"#;

/// A temporary directory of this test's own.
fn scratch(tag: &str) -> PathBuf {
    let mut n = 0u32;
    loop {
        let path =
            std::env::temp_dir().join(format!("lm-host-between-{tag}-{}-{n}", std::process::id()));
        if !path.exists() {
            fs::create_dir_all(&path).expect("the scratch directory is made");
            return path;
        }
        n += 1;
    }
}

/// Run git in a directory and hand back what it said.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A harness root: one directory per agent, each its own git repo.
fn harness_root(dir: &Path) -> PathBuf {
    let root = dir.join("harness");
    let harness = root.join("a0");
    fs::create_dir_all(harness.join("reflexes")).expect("the harness is made");
    fs::write(harness.join("reflexes/default.rhai"), DEFAULT_REFLEX)
        .expect("the reflex is written");
    fs::write(harness.join("gates.toml"), "# the agent's gates\n").expect("the gates are written");
    fs::write(harness.join("policy.toml"), "# the agent's policy\n")
        .expect("the policy is written");
    git(&harness, &["init", "-q"]);
    git(&harness, &["add", "-A"]);
    git(
        &harness,
        &[
            "-c",
            "user.name=the harness",
            "-c",
            "user.email=harness@last-mile.invalid",
            "commit",
            "-q",
            "-m",
            "the harness as it was",
        ],
    );
    root
}

/// The reviewer's answer: a change that cannot move a metric.
fn harmless_review() -> String {
    r#"{"proposal": {"kind": "LexiconEntry", "squad": 0, "text": "the beacon we hold is ours"},
        "prediction": {"metric": "hold_ticks_squad0", "direction": "Up", "min_delta": 1}}"#
        .to_string()
}

/// The reviewer's answer: the known-good reflex edit, and what it expects.
fn hold_review(agent: u16) -> String {
    let source = serde_json::to_string(HOLD_REFLEX).expect("the reflex is json");
    format!(
        r#"{{"proposal": {{"kind": "EditReflex", "agent": {agent}, "name": "default", "source": {source}}},
            "prediction": {{"metric": "hold_ticks_squad0", "direction": "Up", "min_delta": 1}}}}"#
    )
}

/// The flow, set up over a scratch directory.
fn flow(dir: &Path, harness: &Path, seed: u64, ticks: u64, eval_seeds: usize) -> FlowConfig {
    FlowConfig::new(seed, harness)
        .with_runs_root(dir.join("runs"))
        .with_ticks(ticks)
        .with_eval_seeds(eval_seeds)
}

/// The screen the host would show for this record.
fn screen(record: &FlowRecord) -> BetweenModel {
    BetweenModel::measured(record, None)
}

/// The held-out seeds fit on the screen.
///
/// The flow judges a proposal on sixteen held-out seeds now, and sixteen
/// numbers do not fit on a line as a list: a run of consecutive seeds is
/// written as a range, and the count is always there, because a reader has
/// to know how many seeds the verdict was read from.
#[test]
fn the_held_out_seeds_are_one_line() {
    let prediction = PredictionView {
        seal: 1,
        metric: "hold_ticks_squad0".into(),
        direction: Direction::Up,
        min_delta: 1,
        eval_seeds: (1_001_000..1_001_016).collect(),
        author: "test".into(),
        text: String::new(),
    };
    assert_eq!(
        prediction.seeds_text(),
        "16 seeds (1001000..1001015)",
        "sixteen seeds do not fit on the screen as a list"
    );

    // Seeds that are not a run are written as the list they are.
    let scattered = PredictionView {
        eval_seeds: vec![1_001_000, 1_001_002, 1_001_007],
        ..prediction.clone()
    };
    assert_eq!(
        scattered.seeds_text(),
        "3 seeds (1001000, 1001002, 1001007)"
    );

    // And a round judged on no seeds says so.
    let none = PredictionView {
        eval_seeds: Vec::new(),
        ..prediction
    };
    assert_eq!(none.seeds_text(), "none");
}

/// The promise comes before the measurement that judges it: the prediction was
/// sealed before the test (D32), so a reader has to see it first.
#[test]
fn screen_shows_the_sealed_prediction_before_the_verdict() {
    let dir = scratch("order");
    let harness = harness_root(&dir);
    let mut reviewer = StubReviewer::new(harmless_review());
    let record = run_learning_round(&flow(&dir, &harness, 7, 300, 2), &mut reviewer)
        .expect("the round runs");

    let model = screen(&record);

    // The promise is on the screen, and it carries its seal.
    let prediction = model.prediction.as_ref().expect("the prediction is shown");
    assert_eq!(
        prediction.seal,
        record.seal.expect("the prediction was sealed"),
        "the screen shows another seal than the record"
    );
    assert_eq!(prediction.metric, "hold_ticks_squad0");
    assert_eq!(prediction.direction, Direction::Up);
    assert_eq!(prediction.min_delta, 1);
    assert_eq!(
        prediction.eval_seeds, record.eval_seeds,
        "the held-out seeds are the ones the evaluation ran on"
    );

    // And the measurement that judges it.
    let verdict = model.verdict.as_ref().expect("the verdict is shown");
    assert_eq!(verdict.seal, prediction.seal);
    assert_eq!(verdict.seeds, verdict.deltas.len());

    // The promise comes first.
    let sections = model.sections();
    let at = |section: Section| {
        sections
            .iter()
            .position(|s| *s == section)
            .unwrap_or_else(|| panic!("the section is there: {sections:?}"))
    };
    assert!(
        at(Section::Prediction) < at(Section::Verdict),
        "the prediction must come before the verdict: {sections:?}"
    );
}

/// A human may go against the measurement, and the chain says so: the ledger
/// gets a line of its own, with the reason (D32, Review Focus 2).
#[test]
fn override_goes_through_the_ledger() {
    let dir = scratch("override");
    let harness = harness_root(&dir);
    let mut reviewer = StubReviewer::new(harmless_review());
    let record = run_learning_round(&flow(&dir, &harness, 7, 300, 2), &mut reviewer)
        .expect("the round runs");
    assert_eq!(
        record.decision,
        Decision::Reject,
        "the evaluation did not back the proposal"
    );

    let model = screen(&record);
    let reason = "the squad should say it, whatever the numbers say";
    let next = model
        .apply(Action::Adopt, reason)
        .expect("the override is written");

    // The record says what the human decided, and why.
    assert_eq!(next.decision, Decision::Adopt);
    assert_eq!(next.overrides.len(), 1);
    assert_eq!(next.overrides[0].decision, Decision::Adopt);
    assert_eq!(next.overrides[0].reason, reason);

    // The ledger has a line of its own: the evidence, and the decision that
    // went against it.
    let ledger = Ledger::open(&record.ledger_path).expect("the ledger is there");
    let overrides: Vec<&Entry> = ledger
        .entries()
        .filter(|entry| matches!(entry, Entry::Override { .. }))
        .collect();
    assert_eq!(overrides.len(), 1, "the override is on the ledger");
    match overrides[0] {
        Entry::Override {
            seal,
            adopted,
            reason: why,
        } => {
            assert_eq!(*seal, record.seal.expect("the seal"));
            assert!(adopted, "the human adopted it anyway");
            assert_eq!(why, reason);
        }
        other => panic!("the ledger holds an override: {other:?}"),
    }

    // The measurement stays where it was: the human did not change it.
    let verdict = next.verdict.as_ref().expect("the verdict is still there");
    assert!(!verdict.adopted);
}

/// The request box: what a human types becomes a deposit, with their words
/// kept verbatim (D33).
#[test]
fn request_box_writes_a_deposit() {
    let dir = scratch("request");
    let harness = harness_root(&dir);
    let mut reviewer = StubReviewer::new(harmless_review());
    let record = run_learning_round(&flow(&dir, &harness, 7, 300, 2), &mut reviewer)
        .expect("the round runs");

    let mut model = screen(&record);
    let said = "the squads should hold the beacon they already have";
    let path = model.write_request(said).expect("the deposit is written");

    assert!(path.exists(), "the deposit is on disk: {}", path.display());
    let text = fs::read_to_string(&path).expect("the deposit reads back");
    let deposit = deposits::from_markdown(&text).expect("the deposit parses");
    assert_eq!(deposit.said, said, "the human's words are kept verbatim");
    assert_eq!(deposit.round_id, record.round_id);
    assert!(
        deposit.harness_commit.is_none(),
        "a request is not a change: nothing was applied"
    );

    // The flow's own deposit is 0 and the human's is 1: they never fight over
    // one file.
    let flow_deposit = record
        .deposit_path
        .clone()
        .expect("the flow wrote a deposit");
    assert_ne!(path, flow_deposit);

    // And the deposit is what the reviewer is asked about next.
    let input = model.reviewer_input(said);
    assert!(
        input.contains(said),
        "the request is the reviewer's next input: {input}"
    );
}

/// Refusing a proposal does not move the next round's pin: the header the
/// flow wrote stays as it is (D27).
#[test]
fn reject_override_repins_the_next_round_to_a_revert() {
    let dir = scratch("pin");
    let harness = harness_root(&dir);
    let mut reviewer = StubReviewer::new(hold_review(0));
    // Sixteen held-out seeds: the count the flow ships with. At four the
    // sign test refuses everything — the fairest possible sample at four
    // seeds is p = 0.0625 — so the round this test needs (a proposal the
    // evaluation backs) cannot happen there.
    let record = run_learning_round(&flow(&dir, &harness, 7, 1_500, 16), &mut reviewer)
        .expect("the round runs");
    assert_eq!(
        record.decision,
        Decision::Adopt,
        "the evaluation backed the proposal"
    );

    // The change landed in a commit, and the next round pins it.
    let commit = record.commit.clone().expect("the change was committed");
    assert_eq!(record.next_header_pin, commit);
    let header_before = fs::read(&record.next_header_path).expect("the next header is there");

    // The human refuses it, against the measurement.
    let model = screen(&record);
    let next = model
        .apply(Action::Reject, "not this time")
        .expect("the override is written");
    assert_eq!(next.decision, Decision::Reject);

    // A refusal against the measurement is carried out: the change is
    // reverted in a new commit, and the next round pins that revert, not the
    // commit the flow adopted.
    assert_ne!(
        next.next_header_pin, commit,
        "the refusal left the next round on the adopted change"
    );
    assert_ne!(
        fs::read(&record.next_header_path).expect("the next header is there"),
        header_before,
        "the next header was not repinned"
    );
}

/// The surprise the round's agents counted travels the whole chain: handed
/// to the flow, kept by the record (its top entries only), and shown by the
/// screen between the metrics and the proposal. A round without a table —
/// a reflex-only round, which is what the flow plays alone — shows nothing.
#[test]
fn the_screen_shows_where_the_minds_struggled() {
    use lm_agents::surprise::{EnemySpot, Margin, Phase, RoundSurprise, SituationKey, Standing};
    use lm_agents::sysone::{Answers, Refusal};

    let fight = SituationKey {
        standing: Standing::Enemy,
        enemy: EnemySpot::Seeing,
        stunned: true,
        margin: Margin::Behind,
        phase: Phase::Mid,
    };
    let quiet = SituationKey {
        standing: Standing::Off,
        enemy: EnemySpot::None,
        stunned: false,
        margin: Margin::Level,
        phase: Phase::Early,
    };
    let answered = Answers {
        per_gate: Default::default(),
        escalated: Default::default(),
        decomposed: BTreeMap::new(),
        unsure: [("attention".to_owned(), 0.2)].into_iter().collect(),
        model: "d1-2b".to_owned(),
        input_tokens: 0,
        latency_ms: 1,
        questions: Default::default(),
    };
    let mut table = RoundSurprise::default();
    for _ in 0..9 {
        table.record(fight, &Ok(answered.clone()));
    }
    table.record(fight, &Err(Refusal::Timeout));
    // The quiet bucket never reaches the minimum-asks floor: counted, but
    // not ranked.
    table.record(quiet, &Ok(answered.clone()));

    let dir = scratch("surprise");
    let harness = harness_root(&dir);
    let mut reviewer = StubReviewer::new(harmless_review());
    let cfg = flow(&dir, &harness, 7, 300, 2).with_surprise(table);
    let record = run_learning_round(&cfg, &mut reviewer).expect("the round runs");
    assert_eq!(
        record.surprise.len(),
        1,
        "only the ranked bucket is kept: {:?}",
        record.surprise
    );

    let model = screen(&record);
    assert_eq!(model.surprise.len(), 1);
    let line = &model.surprise[0];
    assert_eq!(line.asks, 10);
    assert_eq!(line.unsure_asks, 9);
    assert_eq!(line.refused, 1);
    assert_eq!(line.escalated, 0);
    assert_eq!(
        line.text,
        "mid round, behind on score, an enemy in sight, stunned, on an enemy beacon: \
         9 of 10 asks unsure (attention: 9)"
    );

    // The surprise sits between the metrics and the proposal: it is the
    // round's own answer to "where did they struggle", and the proposal is
    // read against it.
    let sections = model.sections();
    let at = |section: Section| sections.iter().position(|s| *s == section).expect("there");
    assert!(at(Section::Metrics) < at(Section::Surprise));
    assert!(at(Section::Surprise) < at(Section::Proposal));

    // The record on disk carries the counts the screen read: the files agree.
    let text = fs::read_to_string(&record.record_path).expect("the record is on disk");
    let on_disk: FlowRecord = serde_json::from_str(&text).expect("the record loads back");
    assert_eq!(on_disk.surprise, record.surprise);
    assert_eq!(on_disk.surprise[0].asks, 10);

    // And a round the flow played alone — reflex only, no asks — shows no
    // surprise section at all, and says nothing.
    let dir = scratch("surprise-none");
    let harness = harness_root(&dir);
    let mut reviewer = StubReviewer::new(harmless_review());
    let record = run_learning_round(&flow(&dir, &harness, 7, 300, 2), &mut reviewer)
        .expect("the round runs");
    assert!(record.surprise.is_empty());
    assert!(!screen(&record).sections().contains(&Section::Surprise));
}
