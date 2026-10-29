//! A sweep over the decisiveness parameters: win_score and time_limit_ticks.
//!
//! The sweep measures how the win_score and time_limit_ticks affect the
//! frequency of draws at the time limit. The defaults (win_score 1800,
//! time_limit_ticks 9000) produce 67% draws, but what if we change the
//! parameters? The sweep tests a few combinations to see how the draw rate
//! changes.
//!
//! The sweep plays the shipped rounds — `Header::default_round(seed, ...)`
//! with the default reflex agents — headless for the full `time_limit_ticks`.
//!
//! The sweep plays each combination of parameters for 100 seeds, so it is
//! slow: it is marked `#[ignore]` and `scripts/check.sh` runs it in release,
//! where the seeds are spread over the available threads.

use lm_core::header::Header;
use lm_core::params::Params;
use lm_core::sim::Sim;

/// The shipped reflex, the one a standard round runs.
const DEFAULT_REFLEX: &str = include_str!("../../../assets/reflexes/default.rhai");

/// The win scores to test.
const WIN_SCORES: [u32; 4] = [900, 1200, 1500, 1800];

/// The time limit ticks to test.
const TIME_LIMITS: [u32; 2] = [9000, 12000];

/// The number of seeds the sweep plays for each parameter combination.
const SEEDS: u64 = 100;

/// What one round produced.
struct Round {
    /// The seed the round ran on.
    seed: u64,
    /// The winning squad, or `None` when the round was a draw.
    winner: Option<u8>,
    /// The tick the round ended on.
    tick: u64,
}

/// Play one headless round and report what it produced.
fn play(seed: u64, params: &Params) -> Round {
    let mut h = Header::default_round(seed, DEFAULT_REFLEX);
    // The swept knobs go into the header: the sim reads its params from there.
    h.params.win_score = params.win_score;
    h.params.time_limit_ticks = params.time_limit_ticks;
    let mut sim = Sim::new(&h).expect("the round starts");
    let mut tick = 0;
    // A few ticks past the limit: the outcome is declared on the tick after
    // the limit is reached, and a round with no outcome is a bug, not a draw.
    for _ in 0..u64::from(params.time_limit_ticks) + 16 {
        sim.step(&[]);
        tick += 1;
        if sim.outcome().is_some() {
            break;
        }
    }
    assert!(sim.outcome().is_some(), "seed {seed}: no outcome after the time limit");
    let outcome = sim.outcome().cloned();
    Round {
        seed,

        winner: outcome.as_ref().and_then(|o| o.winner),
        tick,
    }
}

/// Play every seed of the sweep for the given parameters.
///
/// The seeds are independent, so they are spread over the available
/// threads: the sweep plays hundreds of rounds and the wall-clock is what
/// matters.
fn play_seeds(params: &Params) -> Vec<Round> {
    let jobs = std::thread::available_parallelism()
        .map(|n| n.get() as u64)
        .unwrap_or(1)
        .clamp(1, SEEDS);
    let chunk = SEEDS.div_ceil(jobs);
    let mut rounds: Vec<Round> = Vec::new();
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for job in 0..jobs {
            let first = job * chunk + 1;
            let last = ((job + 1) * chunk).min(SEEDS);
            if first > last {
                continue;
            }
            handles.push(scope.spawn(move || {
                let mut local = Vec::new();
                for seed in first..=last {
                    local.push(play(seed, params));
                }
                local
            }));
        }
        for handle in handles {
            rounds.extend(handle.join().expect("the sweep thread finishes"));
        }
    });
    rounds.sort_by_key(|r| r.seed);
    rounds
}

/// The sweep test.
///
/// This test is slow and ignored by default. To run it, use:
/// ```bash
/// cargo test --release -p lm-core --test decisiveness -- --ignored --nocapture
/// ```
#[test]
#[ignore = "sweep; run with --release --ignored --nocapture"]
fn decisiveness_sweep() {
    for &win_score in &WIN_SCORES {
        for &time_limit_ticks in &TIME_LIMITS {
            let params = Params {
                win_score,
                time_limit_ticks,
                ..Params::default()
            };
            let rounds = play_seeds(&params);

            // Count results
            let mut wins0 = 0;
            let mut wins1 = 0;
            let mut draws = 0;
            // Draws that ended before the time limit: both squads reached
            // win_score on the same tick (a mirror match), not a timeout.
            let mut early_draws = 0;
            let mut total_ticks = Vec::new();

            for round in &rounds {
                total_ticks.push(round.tick);
                match round.winner {
                    Some(0) => wins0 += 1,
                    Some(1) => wins1 += 1,
                    Some(_) => panic!("Unexpected winner value: {:?}", round.winner),
                    None => {
                        draws += 1;
                        if round.tick < u64::from(time_limit_ticks) {
                            early_draws += 1;
                        }
                    }
                }
            }

            // Calculate median tick
            total_ticks.sort();
            let median_tick = if total_ticks.is_empty() {
                0
            } else {
                let len = total_ticks.len();
                if len % 2 == 0 {
                    (total_ticks[len / 2 - 1] + total_ticks[len / 2]) / 2
                } else {
                    total_ticks[len / 2]
                }
            };

            // Verify assertions
            assert_eq!(
                wins0 + wins1 + draws,
                SEEDS,
                "Every round must end with a win or draw"
            );

            // Print results for this parameter combination
            let draw_percent = (draws as f64 / SEEDS as f64) * 100.0;
            println!(
                "{}, {}, {}, {}, {}, {:.1}%, {}, early draws {}",
                win_score, time_limit_ticks, wins0, wins1, draws, draw_percent, median_tick, early_draws
            );
        }
    }
}
