//! The learning flow, end to end.
//!
//! One round: play it, measure it, ask a reviewer, seal what it predicted,
//! test the proposal on held-out seeds, and adopt or reject it. Every link
//! of that chain is on disk, and these tests check the chain from the files
//! alone — the log, the ledger, the flow record, the deposit and the
//! harness's git history.
//!
//! Nothing here reaches the network, and nothing here reads or writes the
//! user's own configuration: the reviewer is a stub, and every file lives in
//! a temporary directory of its own.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use lm_core::header::Header;
use lm_core::log::RoundLog;
use lm_learn::flow::{
    metrics_digest, proposal_hash, run_learning_round, write_override, Decision, FlowConfig,
    FlowRecord, StubReviewer,
};
use lm_learn::ledger::{Entry, Ledger};
use lm_learn::metrics::RoundMetrics;

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

/// How many ticks a learning round runs in these tests.
const TICKS: u64 = 1_500;

/// How many held-out seeds the evaluation runs on.
const EVAL_SEEDS: usize = 4;

/// A temporary directory of this test's own.
fn scratch(tag: &str) -> PathBuf {
    let mut n = 0u32;
    loop {
        let path =
            std::env::temp_dir().join(format!("lm-learn-flow-{tag}-{}-{n}", std::process::id()));
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
fn harness_root(dir: &Path, agents: &[u16]) -> PathBuf {
    let root = dir.join("harness");
    for &agent in agents {
        let harness = root.join(format!("a{agent}"));
        fs::create_dir_all(harness.join("reflexes")).expect("the harness is made");
        fs::write(harness.join("reflexes/default.rhai"), DEFAULT_REFLEX)
            .expect("the reflex is written");
        fs::write(harness.join("gates.toml"), "# the agent's gates\n")
            .expect("the gates are written");
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
    }
    root
}

/// The reviewer's answer: the known-good reflex edit, and what it expects.
fn hold_review(agent: u16) -> String {
    let source = serde_json::to_string(HOLD_REFLEX).expect("the reflex is json");
    format!(
        r#"{{"proposal": {{"kind": "EditReflex", "agent": {agent}, "name": "default", "source": {source}}},
            "prediction": {{"metric": "hold_ticks_squad0", "direction": "Up", "min_delta": 1}}}}"#
    )
}

/// The reviewer's answer: a change that cannot move a metric.
fn harmless_review() -> String {
    r#"{"proposal": {"kind": "LexiconEntry", "squad": 0, "text": "the beacon we hold is ours"},
        "prediction": {"metric": "hold_ticks_squad0", "direction": "Up", "min_delta": 1}}"#
        .to_string()
}

/// The flow, set up over a scratch directory.
fn flow(dir: &Path, harness: &Path, seed: u64) -> FlowConfig {
    FlowConfig::new(seed, harness)
        .with_runs_root(dir.join("runs"))
        .with_ticks(TICKS)
        .with_eval_seeds(EVAL_SEEDS)
}

/// The flow record as it is on disk.
fn record_on_disk(dir: &Path, round_id: &str) -> FlowRecord {
    let path = dir
        .join("runs")
        .join("flow")
        .join(format!("{round_id}.json"));
    let text = fs::read_to_string(&path).expect("the flow record is on disk");
    serde_json::from_str(&text).expect("the flow record reads back")
}

/// Round N+1 plays what round N learned: the proposal was tested on
/// held-out seeds, it moved the metric it said it would, and the next
/// round's header pins the commit the change landed in.
#[test]
fn round_n_plus_1_uses_lesson() {
    let dir = scratch("lesson");
    let harness = harness_root(&dir, &[0]);
    let base = git(&harness.join("a0"), &["rev-parse", "HEAD"]);

    let mut reviewer = StubReviewer::new(hold_review(0));
    let record =
        run_learning_round(&flow(&dir, &harness, 7), &mut reviewer).expect("the round runs");

    // The proposal passed the held-out evaluation.
    assert_eq!(record.decision, Decision::Adopt);
    let verdict = record.verdict.clone().expect("the verdict is recorded");
    assert!(verdict.adopted, "the eval did not adopt: {verdict:?}");
    assert_eq!(verdict.deltas.len(), EVAL_SEEDS);
    for (seed, delta) in &verdict.deltas {
        assert!(
            *delta > 0,
            "the lesson did not move the metric on seed {seed}: {delta}"
        );
    }

    // The seeds it was judged on are not the seed it was learned from.
    assert!(!record.eval_seeds.contains(&record.seed));
    for seed in &record.eval_seeds {
        assert!(*seed >= 1_000_000);
    }

    // The change landed in the harness, in a commit of its own.
    let commit = record.commit.clone().expect("the change was committed");
    assert_ne!(commit, base);
    assert_eq!(git(&harness.join("a0"), &["rev-parse", "HEAD"]), commit);
    let source =
        fs::read_to_string(harness.join("a0/reflexes/default.rhai")).expect("the reflex is there");
    assert!(source.contains("nearest_of"), "the new reflex did not land");

    // Round N+1 pins the commit the flow produced.
    assert_eq!(record.next_header_pin, commit);
    let next: Header = serde_json::from_str(
        &fs::read_to_string(&record.next_header_path).expect("the header is there"),
    )
    .expect("the next header reads back");
    let entry = next
        .roster
        .iter()
        .find(|e| e.agent == 0)
        .expect("agent 0 plays");
    assert_eq!(entry.harness_commit, commit);
    assert_eq!(next.seed, record.seed + 1);
}

/// A proposal the held-out evaluation does not back changes nothing: the
/// harness is left exactly as it was.
#[test]
fn rejected_proposal_leaves_harness_unchanged() {
    let dir = scratch("rejected");
    let harness = harness_root(&dir, &[0]);
    let base = git(&harness.join("a0"), &["rev-parse", "HEAD"]);

    let mut reviewer = StubReviewer::new(harmless_review());
    let record =
        run_learning_round(&flow(&dir, &harness, 7), &mut reviewer).expect("the round runs");

    assert_eq!(record.decision, Decision::Reject);
    assert!(record.commit.is_none(), "nothing was committed");
    let verdict = record.verdict.clone().expect("the verdict is recorded");
    assert!(!verdict.adopted);
    for (seed, delta) in &verdict.deltas {
        assert_eq!(*delta, 0, "the change moved the metric on seed {seed}");
    }

    // The harness is untouched: the same bytes, the same commit.
    assert_eq!(
        fs::read_to_string(harness.join("a0/reflexes/default.rhai")).expect("the reflex is there"),
        DEFAULT_REFLEX
    );
    assert_eq!(git(&harness.join("a0"), &["rev-parse", "HEAD"]), base);
    assert!(!harness.join("squad0").exists(), "a lexicon was written");

    // The next round pins the unchanged base.
    assert_eq!(record.next_header_pin, base);

    // The deposit says what was proposed, and that it was not applied.
    let deposit = fs::read_to_string(record.deposit_path.clone().expect("the deposit is there"))
        .expect("the deposit reads back");
    assert!(deposit.contains("a new lexicon entry for squad 0"));
    assert!(deposit.contains("_not applied_"));
}

/// A reviewer that says something that is not a proposal is a rejection, not
/// a crash: the round is recorded, and the harness is left alone.
#[test]
fn garbage_review_is_a_rejection_not_a_crash() {
    let dir = scratch("garbage");
    let harness = harness_root(&dir, &[0]);
    let base = git(&harness.join("a0"), &["rev-parse", "HEAD"]);

    let mut reviewer = StubReviewer::new("Honestly? The round looked fine to me. Carry on.");
    let record = run_learning_round(&flow(&dir, &harness, 7), &mut reviewer)
        .expect("garbage is a rejection");

    assert_eq!(record.decision, Decision::Reject);
    let why = record.rejection.clone().expect("the refusal is named");
    assert!(!why.is_empty());
    assert!(record.proposal.is_none());
    assert!(
        record.seal.is_none(),
        "nothing was sealed: there was no claim"
    );
    assert!(record.verdict.is_none(), "nothing was judged");
    assert!(record.commit.is_none());

    // The harness is untouched, and the round is still on the record.
    assert_eq!(git(&harness.join("a0"), &["rev-parse", "HEAD"]), base);
    let on_disk = record_on_disk(&dir, "r7");
    assert_eq!(on_disk.decision, Decision::Reject);
    assert!(on_disk.rejection.is_some());

    // The deposit asks the human what should change instead.
    let deposit = fs::read_to_string(record.deposit_path.clone().expect("the deposit is there"))
        .expect("the deposit reads back");
    assert!(deposit.contains("What should change?"), "the deposit asks");
    assert!(deposit.contains("_not applied_"), "nothing was applied");
}

/// The chain can be audited from the files alone: the seal is earlier than
/// the verdict, the commit is the harness's, and the next header pins it.
#[test]
fn flow_record_audits_the_chain() {
    let dir = scratch("audit");
    let harness = harness_root(&dir, &[0]);
    let base = git(&harness.join("a0"), &["rev-parse", "HEAD"]);

    let mut reviewer = StubReviewer::new(hold_review(0));
    let record =
        run_learning_round(&flow(&dir, &harness, 7), &mut reviewer).expect("the round runs");
    let commit = record.commit.clone().expect("the change was committed");

    // 1. The log: the round that was played, with the harness it was played
    //    with — the base, not the change.
    let mut log_file = fs::File::open(&record.log_path).expect("the log is there");
    let log = RoundLog::read(&mut log_file).expect("the log reads back");
    assert!(log.verify().is_ok(), "the log proves itself");
    let played = log
        .header
        .roster
        .iter()
        .find(|e| e.agent == 0)
        .expect("agent 0 played");
    assert_eq!(played.harness_commit, base);

    // 2. The record: the metrics digest is the log's own.
    let metrics = RoundMetrics::from_log(&log);
    assert_eq!(record.metrics_digest, metrics_digest(&metrics));

    // 3. The ledger: the seal comes before the verdict, and the seal is for
    //    this proposal.
    let ledger = Ledger::open(&record.ledger_path).expect("the ledger is there");
    let entries: Vec<(usize, &Entry)> = ledger.entries().enumerate().collect();
    let (seal_seq, seal) = entries
        .iter()
        .find(|(_, entry)| matches!(entry, Entry::Seal(_)))
        .expect("the prediction is sealed");
    let (verdict_seq, verdict) = entries
        .iter()
        .find(|(_, entry)| matches!(entry, Entry::Verdict(_)))
        .expect("the verdict is recorded");
    assert!(
        *seal_seq < *verdict_seq,
        "the seal is earlier than the verdict"
    );
    let Entry::Seal(prediction) = seal else {
        panic!("the seal is a seal");
    };
    let proposal = record.proposal.clone().expect("the proposal is recorded");
    assert_eq!(prediction.proposal_hash, proposal_hash(&proposal));
    assert_eq!(prediction.round_id, record.round_id);
    assert_eq!(&prediction.eval_seeds, &record.eval_seeds);
    let Entry::Verdict(measured) = verdict else {
        panic!("the verdict is a verdict");
    };
    assert_eq!(measured.seal, *seal_seq as u64);
    assert_eq!(measured.deltas.len(), EVAL_SEEDS);
    assert!(measured.adopted);

    // 4. The harness's git: the commit is the harness's HEAD, and the round
    //    is tagged on it.
    assert_eq!(git(&harness.join("a0"), &["rev-parse", "HEAD"]), commit);
    let tags = git(&harness.join("a0"), &["tag", "--list"]);
    assert!(
        tags.contains("round-r7"),
        "the round is not tagged: {tags:?}"
    );
    assert_eq!(
        git(&harness.join("a0"), &["rev-list", "-n", "1", "round-r7"]),
        commit
    );

    // 5. The next header: it pins the commit the flow produced.
    let next: Header = serde_json::from_str(
        &fs::read_to_string(&record.next_header_path).expect("the header is there"),
    )
    .expect("the next header reads back");
    let pinned = next
        .roster
        .iter()
        .find(|e| e.agent == 0)
        .expect("agent 0 plays");
    assert_eq!(pinned.harness_commit, commit);

    // 6. The deposit: the change in words, with the commit it landed in.
    let deposit = fs::read_to_string(record.deposit_path.clone().expect("the deposit is there"))
        .expect("the deposit reads back");
    assert!(
        deposit.contains("# Deposit r7"),
        "the deposit names the round"
    );
    assert!(deposit.contains(&commit), "the deposit names the commit");
    assert!(
        deposit.contains("nearest_of"),
        "the deposit names the change"
    );

    // 7. The record on disk is the record that was returned.
    let on_disk = record_on_disk(&dir, "r7");
    assert_eq!(on_disk.commit, Some(commit));
    assert_eq!(on_disk.decision, Decision::Adopt);
    assert_eq!(on_disk.seal, record.seal);
}

/// A human may decide against the evidence; the flow never does that
/// quietly: the override is in the ledger and in the record, and the
/// measurement stays where it was.
#[test]
fn override_is_recorded() {
    let dir = scratch("override");
    let harness = harness_root(&dir, &[0]);

    let mut reviewer = StubReviewer::new(harmless_review());
    let record =
        run_learning_round(&flow(&dir, &harness, 7), &mut reviewer).expect("the round runs");
    assert_eq!(record.decision, Decision::Reject);

    let overridden = write_override(
        &record,
        Decision::Adopt,
        "the phrase is worth keeping even if the metric did not move",
    )
    .expect("the override is written");

    // The decision is the human's; the measurement is still the measurement.
    assert_eq!(overridden.decision, Decision::Adopt);
    assert_eq!(overridden.verdict, record.verdict);
    assert!(
        !overridden
            .verdict
            .clone()
            .expect("the verdict is there")
            .adopted
    );
    assert_eq!(overridden.overrides.len(), 1);
    assert_eq!(overridden.overrides[0].decision, Decision::Adopt);

    // The ledger says so, in the chain, after the verdict it goes against.
    let ledger = Ledger::open(&record.ledger_path).expect("the ledger is there");
    let entries: Vec<(usize, &Entry)> = ledger.entries().enumerate().collect();
    let (verdict_seq, _) = entries
        .iter()
        .find(|(_, entry)| matches!(entry, Entry::Verdict(_)))
        .expect("the verdict is recorded");
    let (override_seq, entry) = entries
        .iter()
        .find(|(_, entry)| matches!(entry, Entry::Override { .. }))
        .expect("the override is recorded");
    assert!(*verdict_seq < *override_seq);
    let Entry::Override {
        seal,
        adopted,
        reason,
    } = entry
    else {
        panic!("the override is an override");
    };
    assert_eq!(*seal, record.seal.expect("the seal is there"));
    assert!(adopted);
    assert!(reason.contains("phrase"));

    // And the record on disk says so too.
    let on_disk = record_on_disk(&dir, "r7");
    assert_eq!(on_disk.decision, Decision::Adopt);
    assert_eq!(on_disk.overrides.len(), 1);
    assert_eq!(on_disk.verdict, record.verdict);
}
