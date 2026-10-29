//! Held-out evaluation: baseline vs proposal on disjoint seeds.
//!
//! The evaluation process:
//! 1. Generate a set of held-out seeds disjoint from training seeds
//! 2. For each seed, run both baseline and candidate headers
//! 3. Compute the metric value for the prediction metric
//! 4. Calculate the delta (candidate - baseline)
//! 5. Determine if adoption criteria are met

use crate::ledger::{Direction, Prediction, Verdict};
use crate::metrics::RoundMetrics;
use crate::round::play;
use lm_core::Header;

/// Generate n seeds from a range disjoint from train.
/// Seeds are: 1_000_000 + salt*1000 + i, skipping any that appear in train.
pub fn eval_seeds(train: &[u64], n: usize, salt: u64) -> Vec<u64> {
    let mut seeds = Vec::new();
    let base = 1_000_000 + salt * 1000;
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

/// Evaluate a prediction by comparing baseline vs candidate on held-out seeds.
pub fn evaluate(
    baseline: &dyn Fn(u64) -> Header,
    candidate: &dyn Fn(u64) -> Header,
    ticks: u64,
    pred: &Prediction,
    seal: u64,
) -> Result<Verdict, String> {
    // Get the metric value or return error if unknown
    let metric_name = &pred.metric;
    if metric_value(&RoundMetrics::from_log(&play(baseline(0), 0)), metric_name).is_none() {
        return Err(format!("Unknown metric: {}", metric_name));
    }

    // Run evaluation on each seed
    let mut deltas = Vec::new();

    // We need to run the baseline and candidate for each seed
    for seed in &pred.eval_seeds {
        let baseline_header = baseline(*seed);
        let candidate_header = candidate(*seed);

        let baseline_log = play(baseline_header, ticks);
        let candidate_log = play(candidate_header, ticks);

        let baseline_metrics = RoundMetrics::from_log(&baseline_log);
        let candidate_metrics = RoundMetrics::from_log(&candidate_log);

        let baseline_value = metric_value(&baseline_metrics, metric_name)
            .ok_or_else(|| format!("Missing metric {} in baseline", metric_name))?;
        let candidate_value = metric_value(&candidate_metrics, metric_name)
            .ok_or_else(|| format!("Missing metric {} in candidate", metric_name))?;

        let delta = candidate_value - baseline_value;
        deltas.push((*seed, delta));
    }

    // Check if adoption criteria are met
    // Adopted iff the delta moves in pred.direction by >= pred.min_delta
    // on at least ceil(0.75 * N) seeds
    let threshold = (pred.eval_seeds.len() as f64 * 0.75).ceil() as usize;

    let mut qualified_deltas = Vec::new();

    for &(seed, delta) in &deltas {
        let qualified = match pred.direction {
            Direction::Up => delta >= pred.min_delta,
            Direction::Down => delta <= -pred.min_delta,
        };

        if qualified {
            qualified_deltas.push((seed, delta));
        }
    }

    let adopted = qualified_deltas.len() >= threshold;

    Ok(Verdict {
        seal,
        deltas,
        adopted,
        override_reason: None,
    })
}
