//! The held-out evaluation: judged on many cores, and judged the same way.
//!
//! The evaluation runs its seeds in parallel. The claim it has to keep is
//! that this changes the schedule and nothing else: the rounds, the
//! deltas, their order and the verdict are the ones the serial run
//! reaches, bit for bit. These tests hold the two to each other, and check
//! the deltas against rounds played on their own.

use lm_core::header::Header;
use lm_learn::eval::{
    eval_seeds, evaluate, evaluate_serial, judge_seeds, judge_seeds_serial, metric_value,
    worker_count,
};
use lm_learn::ledger::{Direction, Prediction};
use lm_learn::metrics::RoundMetrics;
use lm_learn::round::play;

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

/// How many ticks the evaluation plays in these tests.
///
/// The rounds are short: what is being held to is that the parallel path
/// plays the same rounds as the serial one, not that the metric moves.
const TICKS: u64 = 40;

/// The baseline: the shipped reflex.
fn baseline(seed: u64) -> Header {
    Header::default_round(seed, DEFAULT_REFLEX)
}

/// The candidate: the reflex that keeps the beacon.
fn candidate(seed: u64) -> Header {
    Header::default_round(seed, HOLD_REFLEX)
}

/// A sealed prediction over `n` held-out seeds.
fn prediction(n: usize) -> Prediction {
    Prediction {
        round_id: "r1".into(),
        proposal_hash: "deadbeefdeadbeef".into(),
        metric: "hold_ticks_squad0".into(),
        direction: Direction::Up,
        min_delta: 1,
        eval_seeds: eval_seeds(&[], n, 1),
        author: "test".into(),
        text: "the squad holds the beacon for longer".into(),
    }
}

/// The parallel evaluation and the serial one are the same measurement.
#[test]
fn the_parallel_evaluation_is_the_serial_evaluation() {
    // The counts that matter: the default, the counts around it, and a
    // count that does not divide by the number of cores.
    for n in [1usize, 2, 3, 4, 5, 7, 8, 16, 33] {
        let pred = prediction(n);
        let parallel = evaluate(&baseline, &candidate, TICKS, &pred, 42)
            .expect("the parallel evaluation runs");
        let serial = evaluate_serial(&baseline, &candidate, TICKS, &pred, 42)
            .expect("the serial evaluation runs");

        assert_eq!(
            parallel, serial,
            "the parallel verdict differs from the serial one at {n} seeds"
        );
        assert_eq!(parallel.deltas.len(), n);
        assert_eq!(parallel.seal, 42);
    }
}

/// The rounds the parallel path played are the rounds the serial path
/// played.
///
/// This is the strong form of the claim: not only the deltas, but the
/// hashes of every tick of both rounds.
#[test]
fn the_parallel_evaluation_plays_the_same_rounds() {
    for n in [1usize, 2, 3, 4, 7, 8, 16, 33] {
        let seeds = eval_seeds(&[], n, 1);
        let parallel = judge_seeds(&baseline, &candidate, TICKS, &seeds, "hold_ticks_squad0")
            .expect("the parallel judgement runs");
        let serial = judge_seeds_serial(&baseline, &candidate, TICKS, &seeds, "hold_ticks_squad0")
            .expect("the serial judgement runs");

        assert_eq!(parallel, serial, "the rounds differ at {n} seeds");
        assert_eq!(parallel.len(), n);
        for judgement in &parallel {
            assert!(
                !judgement.baseline_hashes.is_empty(),
                "the round has no chain"
            );
            assert!(
                !judgement.candidate_hashes.is_empty(),
                "the round has no chain"
            );
            assert_eq!(
                judgement.baseline_hashes.len(),
                judgement.candidate_hashes.len()
            );
        }
    }
}

/// The comparison is not vacuous: every seed is its own round.
#[test]
fn every_seed_is_its_own_round() {
    let seeds = eval_seeds(&[], 8, 1);
    let judged = judge_seeds_serial(&baseline, &candidate, TICKS, &seeds, "hold_ticks_squad0")
        .expect("the judgement runs");
    for (i, one) in judged.iter().enumerate() {
        for other in judged.iter().skip(i + 1) {
            assert_ne!(
                one.baseline_hashes, other.baseline_hashes,
                "two seeds played the same round"
            );
        }
    }
}

/// The deltas are the ones the rounds gave, in seed order.
#[test]
fn the_deltas_are_the_ones_the_rounds_gave() {
    let seeds = eval_seeds(&[], 12, 1);
    let judged = judge_seeds(&baseline, &candidate, TICKS, &seeds, "hold_ticks_squad0")
        .expect("the judgement runs");

    // Every delta, measured on its own, one round after the other.
    for judgement in &judged {
        let seed = judgement.seed;
        let baseline_metrics = RoundMetrics::from_log(&play(baseline(seed), TICKS));
        let candidate_metrics = RoundMetrics::from_log(&play(candidate(seed), TICKS));
        let before = metric_value(&baseline_metrics, "hold_ticks_squad0").expect("the metric");
        let after = metric_value(&candidate_metrics, "hold_ticks_squad0").expect("the metric");
        assert_eq!(after - before, judgement.delta, "the delta of seed {seed}");
    }
}

/// The seeds are judged on the machine's cores, and the count is never more
/// than the seeds there are.
#[test]
fn the_seeds_are_judged_on_the_cores() {
    assert!(worker_count(0) >= 1);
    assert_eq!(worker_count(1), 1);
    assert!(worker_count(1_000) >= 1);
    assert!(worker_count(1_000) <= 1_000);
}

/// A prediction with no seeds has no evidence, and no evidence adopts
/// nothing.
#[test]
fn a_prediction_with_no_seeds_is_not_adopted() {
    let pred = prediction(0);
    let verdict = evaluate(&baseline, &candidate, TICKS, &pred, 3).expect("the evaluation runs");
    assert!(verdict.deltas.is_empty());
    assert!(
        !verdict.adopted,
        "a prediction judged on no seeds was adopted"
    );
}

/// Two rounds never judge the same seed, at any count.
#[test]
fn two_rounds_never_share_a_held_out_seed() {
    for n in [1usize, 4, 999, 1_000, 1_024, 2_048] {
        let one = eval_seeds(&[], n, 1);
        let other = eval_seeds(&[], n, 2);
        assert_eq!(one.len(), n);
        for seed in &one {
            assert!(
                !other.contains(seed),
                "the rounds of seeds 1 and 2 share {seed} at {n} seeds"
            );
        }
    }
}

/// The default count of seeds is what it has always been.
#[test]
fn the_default_seeds_are_the_seeds_the_flow_has_always_judged() {
    assert_eq!(lm_learn::flow::DEFAULT_EVAL_SEEDS, 4);
    let seeds = eval_seeds(&[], lm_learn::flow::DEFAULT_EVAL_SEEDS, 1);
    assert_eq!(seeds, vec![1_001_000, 1_001_001, 1_001_002, 1_001_003]);
}
