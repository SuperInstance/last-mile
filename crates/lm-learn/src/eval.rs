//! Held-out evaluation: baseline vs proposal on disjoint seeds.
//!
//! The evaluation process:
//! 1. Generate a set of held-out seeds disjoint from training seeds
//! 2. For each seed, run both baseline and candidate headers
//! 3. Compute the metric value for the prediction metric
//! 4. Calculate the delta (candidate - baseline)
//! 5. Determine if adoption criteria are met: the count of the seeds whose
//!    delta met the sealed bar, and the exact binomial sign test over the
//!    deltas' direction (see [`crate::sign`])
//!
//! The seeds are judged **in parallel**, across the machine's cores: a
//! proposal judged on 256 seeds is 512 rounds, and one core would spend
//! seconds on it while the host waits for the round to finish. The parallel
//! run is the same measurement as the serial one, bit for bit: the sim is
//! deterministic and bit-exact (D69), every seed is judged on its own, and
//! the deltas are put back together in seed order. [`evaluate_serial`] is
//! that serial reference, and the tests hold the two to each other.

use std::thread;

use crate::ledger::{Direction, Prediction, Verdict};
use crate::metrics::RoundMetrics;
use crate::round::play;
use crate::sign;
use lm_core::log::RoundLog;
use lm_core::Header;

/// The header a seed is played with.
///
/// The bound is `Send + Sync` because the seeds are judged on more than one
/// thread: every worker calls it, so it has to be safe to share.
pub type HeaderFor<'a> = dyn Fn(u64) -> Header + Send + Sync + 'a;

/// How many seeds one round's block of the seed space holds.
///
/// A round judges its seeds in a block of its own, so two rounds never
/// judge the same seed. The block is 1_000 wide, which is what the flow
/// has always used (the round of seed 1 judges 1_001_000..).
const BLOCK: u64 = 1_000;

/// Generate n seeds from a range disjoint from train.
///
/// Seeds are: 1_000_000 + salt*stride + i, skipping any that appear in
/// train.
///
/// The stride is the width of the block the count needs, so a round that
/// judges more seeds than a block holds still gets a block of its own, and
/// never lands on the seeds of the round after it.
pub fn eval_seeds(train: &[u64], n: usize, salt: u64) -> Vec<u64> {
    let (stride, offset) = block_of(n);
    let base = 1_000_000 + salt * stride + offset;
    let mut seeds = Vec::new();
    let mut i = 0u64;

    while seeds.len() < n {
        let seed = base + i;
        if !train.contains(&seed) {
            seeds.push(seed);
        }
        i += 1;
    }

    seeds
}

/// The block of the seed space a count of seeds lives in.
///
/// The block is `(stride, offset)`: the seeds of a round are
/// `1_000_000 + salt*stride + offset ..`, `stride` wide.
///
/// A count of a thousand seeds or fewer keeps the block it has always had,
/// so the round of seed 1 still judges 1_001_000..1_001_003. A count that
/// needs more is given a wider block of its own, in the part of the seed
/// space above the first, so that two rounds that judge different numbers
/// of seeds never share one.
fn block_of(n: usize) -> (u64, u64) {
    let stride = (n as u64).div_ceil(BLOCK).max(1) * BLOCK;
    if stride == BLOCK {
        (BLOCK, 0)
    } else {
        // The count itself names the part of the space, so two counts above
        // the block size land in different places.
        (stride, (n as u64) * 1_000_000_000)
    }
}

/// Get the value of a metric by name from RoundMetrics.
pub fn metric_value(m: &RoundMetrics, name: &str) -> Option<i64> {
    match name {
        "score_squad0" => m.scores.first().copied(),
        "score_squad1" => m.scores.get(1).copied(),
        "hold_ticks_squad0" => m.beacon_hold_ticks.first().copied().map(|x| x as i64),
        "hold_ticks_squad1" => m.beacon_hold_ticks.get(1).copied().map(|x| x as i64),
        "tags_for_squad0" => m.tags_for.first().copied().map(|x| x as i64),
        "tags_for_squad1" => m.tags_for.get(1).copied().map(|x| x as i64),
        "margin_squad0" => {
            // score0 - score1
            match (m.scores.first(), m.scores.get(1)) {
                (Some(score0), Some(score1)) => Some(score0 - score1),
                _ => None,
            }
        }
        _ => None,
    }
}

/// One seed's judgement: the two rounds it was measured from, and the
/// change the metric saw.
///
/// The hashes are the rounds themselves, tick for tick: whatever the
/// verdict says, these are the rounds it was said from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Judgement {
    /// The seed the rounds were played with.
    pub seed: u64,
    /// The chain of segment hashes of the baseline's round: the round's own
    /// identity, the one the golden pins.
    pub baseline_hashes: Vec<u64>,
    /// The chain of segment hashes of the candidate's round.
    pub candidate_hashes: Vec<u64>,
    /// The metric's change: the candidate minus the baseline.
    pub delta: i64,
}

/// Evaluate a prediction by comparing baseline vs candidate on held-out seeds.
///
/// The seeds are judged on the machine's cores at once. The verdict is the
/// same one the serial run reaches, bit for bit.
pub fn evaluate(
    baseline: &HeaderFor<'_>,
    candidate: &HeaderFor<'_>,
    ticks: u64,
    pred: &Prediction,
    seal: u64,
) -> Result<Verdict, String> {
    let metric_name = &pred.metric;
    if metric_value(&RoundMetrics::from_log(&play(baseline(0), 0)), metric_name).is_none() {
        return Err(format!("Unknown metric: {}", metric_name));
    }

    let judged = judge_seeds(baseline, candidate, ticks, &pred.eval_seeds, metric_name)?;
    let deltas = judged
        .into_iter()
        .map(|judgement| (judgement.seed, judgement.delta))
        .collect();

    Ok(verdict_of(deltas, pred, seal))
}

/// The same evaluation, one seed after the other.
///
/// This is the reference the parallel path is held to: the same seeds must
/// give the same rounds, the same deltas, in the same order, and the same
/// verdict.
pub fn evaluate_serial(
    baseline: &HeaderFor<'_>,
    candidate: &HeaderFor<'_>,
    ticks: u64,
    pred: &Prediction,
    seal: u64,
) -> Result<Verdict, String> {
    let metric_name = &pred.metric;
    if metric_value(&RoundMetrics::from_log(&play(baseline(0), 0)), metric_name).is_none() {
        return Err(format!("Unknown metric: {}", metric_name));
    }

    let judged = judge_seeds_serial(baseline, candidate, ticks, &pred.eval_seeds, metric_name)?;
    let deltas = judged
        .into_iter()
        .map(|judgement| (judgement.seed, judgement.delta))
        .collect();

    Ok(verdict_of(deltas, pred, seal))
}

/// Judge the seeds, across the machine's cores, and hand back every
/// judgement.
///
/// The seeds are split into contiguous blocks, one per worker, and the
/// blocks are put back together in seed order: the judgements are the list
/// the serial run produces, in the same order, whatever the machine did in
/// between.
pub fn judge_seeds(
    baseline: &HeaderFor<'_>,
    candidate: &HeaderFor<'_>,
    ticks: u64,
    seeds: &[u64],
    metric: &str,
) -> Result<Vec<Judgement>, String> {
    let count = worker_count(seeds.len());
    if count < 2 {
        return judge_seeds_serial(baseline, candidate, ticks, seeds, metric);
    }

    let mut parts: Vec<Result<Vec<Judgement>, String>> = Vec::new();

    thread::scope(|scope| {
        let mut workers = Vec::new();
        // The seeds are split evenly: every worker gets the same number of
        // seeds, or one more than its neighbour. The split is by index, so
        // the blocks stay in seed order.
        for worker in 0..count {
            let from = worker * seeds.len() / count;
            let to = (worker + 1) * seeds.len() / count;
            if from == to {
                continue;
            }
            let part = &seeds[from..to];
            workers.push(
                scope.spawn(move || judge_seeds_serial(baseline, candidate, ticks, part, metric)),
            );
        }
        for worker in workers {
            match worker.join() {
                Ok(part) => parts.push(part),
                // A worker that panicked (a round that does not start) is
                // the same panic the serial run would have raised: it is
                // resumed, not swallowed.
                Err(payload) => std::panic::resume_unwind(payload),
            }
        }
    });

    let mut judged = Vec::with_capacity(seeds.len());
    for part in parts {
        judged.extend(part?);
    }
    Ok(judged)
}

/// Judge the seeds, one after the other, and hand back every judgement.
pub fn judge_seeds_serial(
    baseline: &HeaderFor<'_>,
    candidate: &HeaderFor<'_>,
    ticks: u64,
    seeds: &[u64],
    metric: &str,
) -> Result<Vec<Judgement>, String> {
    let mut judged = Vec::with_capacity(seeds.len());
    for &seed in seeds {
        judged.push(judge_one(baseline, candidate, ticks, seed, metric)?);
    }
    Ok(judged)
}

/// Judge one seed: the two rounds, and the change the metric saw.
fn judge_one(
    baseline: &HeaderFor<'_>,
    candidate: &HeaderFor<'_>,
    ticks: u64,
    seed: u64,
    metric: &str,
) -> Result<Judgement, String> {
    // We need to run the baseline and candidate for each seed
    let baseline_header = baseline(seed);
    let candidate_header = candidate(seed);

    let baseline_log = play(baseline_header, ticks);
    let candidate_log = play(candidate_header, ticks);

    let baseline_metrics = RoundMetrics::from_log(&baseline_log);
    let candidate_metrics = RoundMetrics::from_log(&candidate_log);

    let baseline_value = metric_value(&baseline_metrics, metric)
        .ok_or_else(|| format!("Missing metric {} in baseline", metric))?;
    let candidate_value = metric_value(&candidate_metrics, metric)
        .ok_or_else(|| format!("Missing metric {} in candidate", metric))?;

    Ok(Judgement {
        seed,
        baseline_hashes: chain_of(&baseline_log),
        candidate_hashes: chain_of(&candidate_log),
        delta: candidate_value - baseline_value,
    })
}

/// The chain of segment hashes of a round: its identity, tick for tick.
///
/// The chain is what the golden pins and what `verify` checks, so two
/// rounds with the same chain are the same round.
fn chain_of(log: &RoundLog) -> Vec<u64> {
    log.segments.iter().map(|segment| segment.hash).collect()
}

/// The verdict the deltas reach.
///
/// Adopted iff the delta meets the sealed bar on at least ceil(0.75 * N)
/// seeds **and** the sign test on the deltas' direction has p < 0.05. The
/// count carries "how often"; the sign test carries "which way" (the seed
/// farm's rule, crates/lm-learn/SEEDFARM.md).
///
/// A prediction judged on no seeds has no evidence behind it, and is not
/// adopted: three seeds in four of nothing is nothing.
fn verdict_of(deltas: Vec<(u64, i64)>, pred: &Prediction, seal: u64) -> Verdict {
    let adopted = passes(&deltas, pred);

    Verdict {
        seal,
        deltas,
        adopted,
        override_reason: None,
    }
}

/// The pass rule, over the deltas of a judged round.
///
/// Adopted iff
///
/// * the delta meets the sealed bar (`pred.min_delta`, in `pred.direction`)
///   on at least `ceil(3N/4)` of the N seeds — the threshold is whole-number
///   maths: three seeds in four, rounded up; and
/// * the sign test over the deltas' direction has p < 0.05 — the ties
///   (the seeds that did not move) are excluded, and the p-value is an
///   exact binomial tail, decided in integers.
///
/// This is the whole rule, over hand-made deltas: the verdict the flow
/// records is this function's answer, and the tests read it on deltas they
/// made up.
pub fn passes(deltas: &[(u64, i64)], pred: &Prediction) -> bool {
    if deltas.is_empty() {
        return false;
    }
    let threshold = (deltas.len() * 3).div_ceil(4);
    let qualified = deltas
        .iter()
        .filter(|(_, delta)| qualified_delta(*delta, pred))
        .count();
    if qualified < threshold {
        return false;
    }
    let values: Vec<i64> = deltas.iter().map(|(_, delta)| *delta).collect();
    sign::passes(sign::sign_test(&values, pred.direction))
}

/// Does this delta meet the sealed bar?
fn qualified_delta(delta: i64, pred: &Prediction) -> bool {
    match pred.direction {
        Direction::Up => delta >= pred.min_delta,
        Direction::Down => delta <= -pred.min_delta,
    }
}

/// The sign test over a judged round's deltas: how many seeds moved, and how
/// many of them moved the claimed way.
pub fn sign_test_of(deltas: &[(u64, i64)], direction: Direction) -> sign::SignTest {
    let values: Vec<i64> = deltas.iter().map(|(_, delta)| *delta).collect();
    sign::sign_test(&values, direction)
}

/// How many workers judge the seeds.
///
/// One per core the machine has, and never more than there are seeds to
/// judge.
pub fn worker_count(seeds: usize) -> usize {
    if seeds < 2 {
        return 1;
    }
    let cores = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    cores.min(seeds)
}
