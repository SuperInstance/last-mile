//! A sweep over the decisiveness parameters: win_score and time_limit_ticks.
//!
//! The sweep measures how the win_score and time_limit_ticks affect the
//! frequency of draws at the time limit. The defaults (win_score 1800,
//! time_limit_ticks 9000) produce 63% draws, but what if we change the
//! parameters? The sweep tests a few combinations to see how the draw rate
//! changes.
//!
//! The 63% is the measured number at the defaults (100 seeds, the shipped
//! reflex): the sweep below prints it. It does not move with the harness's
//! doctrine — the sweep plays the *reflex*, and the reflexes are the same
//! under every variant — so the `advance` doctrine of 2026-10-30 left it
//! where it was.
//!
//! The sweep plays the shipped rounds — `Header::default_round(seed, ...)`
//! with the default reflex agents — headless for the full `time_limit_ticks`.
//!
//! The sweep plays each combination of parameters for 100 seeds, so it is
//! slow: it is marked `#[ignore]` and `scripts/check.sh` runs it in release,
//! where the seeds are spread over the available threads.
//!
//! ## The harness the sweep plays
//!
//! `DECISIVENESS_HARNESS` names a harness directory, relative to the
//! repository root, and the sweep plays **that harness's reflex** instead of
//! the shipped one: `assets/harness/variants/<name>` plays the variant's
//! `reflexes/default.rhai`. The reflex is the harness's own executable
//! doctrine, so the headless sweep of a variant measures the same doctrine
//! the variant's gates ask the model for (D26 — the reflex plays the moment
//! the model does not answer).
//!
//! The sweep also counts what the rounds *did*: the tags that were made, the
//! ticks on which an agent had an enemy in sight, and the gap between the
//! squads' final scores. A draw rate on its own says whether a round was
//! decided; these say whether it was a fight.

use std::path::{Path, PathBuf};

use lm_core::header::Header;
use lm_core::params::Params;
use lm_core::sim::Sim;
use lm_core::world::los;

/// The shipped reflex, the one a standard round runs.
const DEFAULT_REFLEX: &str = include_str!("../../../assets/reflexes/default.rhai");

/// The enemy's own reach, in cells: the facts builder's ENEMY_NEAR_CELLS.
const ENEMY_NEAR_CELLS: i32 = 6;

/// The win scores to test.
const WIN_SCORES: [u32; 4] = [900, 1200, 1500, 1800];

/// The time limit ticks to test.
const TIME_LIMITS: [u32; 2] = [9000, 12000];

/// The number of seeds the sweep plays for each parameter combination.
const SEEDS: u64 = 100;

/// The root of the repository, where the assets live.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The integer square root, by Newton's method.
///
/// The crate does its arithmetic in integers only, and so does the sweep:
/// the fairness bound has to be the same number on every machine (D69's
/// spirit).
fn isqrt(n: u64) -> u64 {
    if n == 0 {
        return 0;
    }
    let mut x = n;
    let mut y = x.div_ceil(2);
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

/// The reflex the sweep plays: the shipped one, or the one of the harness the
/// caller names.
fn reflex() -> String {
    let Ok(name) = std::env::var("DECISIVENESS_HARNESS") else {
        return DEFAULT_REFLEX.to_owned();
    };
    let path = if Path::new(&name).is_absolute() {
        PathBuf::from(&name).join("reflexes").join("default.rhai")
    } else {
        repo_root()
            .join(&name)
            .join("reflexes")
            .join("default.rhai")
    };
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("the harness's reflex reads: {path:?}: {err}"))
}

/// What one round produced.
struct Round {
    /// The seed the round ran on.
    seed: u64,
    /// The winning squad, or `None` when the round was a draw.
    winner: Option<u8>,
    /// The tick the round ended on.
    tick: u64,
    /// The tags the ruleset made.
    tags: u64,
    /// The ticks on which some agent had an enemy in sight.
    sight_ticks: u64,
    /// The squads' final scores.
    scores: Vec<i64>,
    /// The gap between the squads' final scores.
    margin: i64,
}

/// Play one headless round and report what it produced.
fn play(seed: u64, params: &Params, reflex: &str) -> Round {
    let mut h = Header::default_round(seed, reflex);
    // The swept knobs go into the header: the sim reads its params from there.
    h.params.win_score = params.win_score;
    h.params.time_limit_ticks = params.time_limit_ticks;
    h.params.slow_q8 = params.slow_q8;
    h.params.bounce_q8 = params.bounce_q8;
    let mut sim = Sim::new(&h).expect("the round starts");
    let mut tick = 0;
    let mut tags = 0u64;
    let mut sight_ticks = 0u64;
    // A few ticks past the limit: the outcome is declared on the tick after
    // the limit is reached, and a round with no outcome is a bug, not a draw.
    for _ in 0..u64::from(params.time_limit_ticks) + 16 {
        // What every agent can see on this tick, before the rules run.
        let mut seen = false;
        for entry in &h.roster {
            let view = sim.view(entry.agent);
            let me = view.me.cell;
            for enemy in view.enemies.iter() {
                let dist = (i32::from(enemy.cell.x) - i32::from(me.x))
                    .abs()
                    .max((i32::from(enemy.cell.y) - i32::from(me.y)).abs());
                if dist <= ENEMY_NEAR_CELLS && los::clear(&sim.world().grid, me, enemy.cell) {
                    seen = true;
                    break;
                }
            }
            if seen {
                break;
            }
        }
        if seen {
            sight_ticks += 1;
        }
        let step = sim.step(&[]);
        tick += 1;
        for event in &step.events {
            if matches!(event, lm_core::rules::Event::Tagged { .. }) {
                tags += 1;
            }
        }
        if sim.outcome().is_some() {
            break;
        }
    }
    assert!(
        sim.outcome().is_some(),
        "seed {seed}: no outcome after the time limit"
    );
    let outcome = sim.outcome().cloned();
    let scores = sim.scores().to_vec();
    let margin = match (scores.iter().min(), scores.iter().max()) {
        (Some(low), Some(high)) => high - low,
        _ => 0,
    };
    Round {
        seed,

        winner: outcome.as_ref().and_then(|o| o.winner),
        tick,
        tags,
        sight_ticks,
        scores,
        margin,
    }
}

/// Play every seed of the sweep for the given parameters.
///
/// The seeds are independent, so they are spread over the available
/// threads: the sweep plays hundreds of rounds and the wall-clock is what
/// matters.
fn play_seeds(params: &Params, reflex: &str) -> Vec<Round> {
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
                    local.push(play(seed, params, reflex));
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

/// The cells the sweep plays: every combination, or the ones the caller
/// names (`DECISIVENESS_CELL`, a comma-separated list of `win_score x
/// time_limit_ticks`).
fn cells() -> Vec<(u32, u32)> {
    match std::env::var("DECISIVENESS_CELL") {
        Ok(value) => value
            .split(',')
            .filter_map(|cell| {
                let mut parts = cell.split('x');
                let win_score = parts.next()?.trim().parse::<u32>().ok()?;
                let time_limit_ticks = parts.next()?.trim().parse::<u32>().ok()?;
                Some((win_score, time_limit_ticks))
            })
            .collect(),
        Err(_) => WIN_SCORES
            .iter()
            .flat_map(|win_score| {
                TIME_LIMITS
                    .iter()
                    .map(move |time_limit_ticks| (*win_score, *time_limit_ticks))
            })
            .collect(),
    }
}

/// The paint cells the sweep plays: `DECISIVENESS_PAINT`, a comma-separated
/// list of `slow_q8 x bounce_q8`.
///
/// The default is the paint rules off — the round the sweep measured before
/// the paint had a rule of its own — so the sweep without the variable is
/// the sweep it always was.
fn paint_cells() -> Vec<(u16, u16)> {
    match std::env::var("DECISIVENESS_PAINT") {
        Ok(value) => value
            .split(',')
            .filter_map(|cell| {
                let mut parts = cell.split('x');
                let slow_q8 = parts.next()?.trim().parse::<u16>().ok()?;
                let bounce_q8 = parts.next()?.trim().parse::<u16>().ok()?;
                Some((slow_q8, bounce_q8))
            })
            .collect(),
        Err(_) => vec![(0, 0)],
    }
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
    let reflex = reflex();
    println!();
    println!(
        "harness: {}",
        std::env::var("DECISIVENESS_HARNESS").unwrap_or_else(|_| "the shipped reflex".to_owned())
    );
    for (win_score, time_limit_ticks) in cells() {
        for (slow_q8, bounce_q8) in paint_cells() {
            let params = Params {
                win_score,
                time_limit_ticks,
                slow_q8,
                bounce_q8,
                ..Params::default()
            };
            let rounds = play_seeds(&params, &reflex);

            // Count results
            let mut wins0 = 0;
            let mut wins1 = 0;
            let mut draws = 0;
            // Draws that ended before the time limit: both squads reached
            // win_score on the same tick (a mirror match), not a timeout.
            let mut early_draws = 0;
            let mut total_ticks = Vec::new();
            let mut total_tags = 0u64;
            let mut total_sight = 0u64;
            let mut total_margin = 0i64;
            let mut total_score0 = 0i64;
            let mut total_score1 = 0i64;
            let mut lowest_score = i64::MAX;
            // A round where a squad was held off the board: the collapse the
            // engagement must not have.
            let mut collapses = 0u64;
            // The rounds in which a fight happened at all.
            let mut fought = 0u64;

            for round in &rounds {
                total_ticks.push(round.tick);
                total_tags += round.tags;
                total_sight += round.sight_ticks;
                total_margin += round.margin;
                if round.tags > 0 {
                    fought += 1;
                }
                if let (Some(first), Some(second)) = (round.scores.first(), round.scores.get(1)) {
                    total_score0 += *first;
                    total_score1 += *second;
                    let low = (*first).min(*second);
                    lowest_score = lowest_score.min(low);
                    if low < 200 {
                        collapses += 1;
                    }
                }
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
                "{}, {}, {}, {}, {}, {}, {}, {:.1}%, {}, early draws {}",
                win_score,
                time_limit_ticks,
                slow_q8,
                bounce_q8,
                wins0,
                wins1,
                draws,
                draw_percent,
                median_tick,
                early_draws
            );
            // What the rounds did, over the seeds.
            println!(
                "    tags/round {:.2}, sight ticks/round {:.1}, score margin/round {:.1}, mean scores {:.0} / {:.0}, lowest squad score {}, collapsed rounds {}, rounds with a fight {} ({:.0}%)",
                total_tags as f64 / SEEDS as f64,
                total_sight as f64 / SEEDS as f64,
                total_margin as f64 / SEEDS as f64,
                total_score0 as f64 / SEEDS as f64,
                total_score1 as f64 / SEEDS as f64,
                lowest_score,
                collapses,
                fought,
                100.0 * fought as f64 / SEEDS as f64,
            );
            // Fairness: the same bound the fairness guard uses (`tests/fairness.rs`):
            // over the decided rounds the win gap must stay inside 2-sigma, so
            // neither colour may win more than the guard allows. The bound is
            // printed next to the share so a sweep can be read against the guard
            // without running it.
            let decided = wins0 + wins1;
            let bound = 2 * isqrt(decided);
            let gap = wins0.abs_diff(wins1);
            println!(
                "    wins: squad 0 {} ({:.0}%), squad 1 {} ({:.0}%) of {} decided, gap {}, 2-sigma bound {} -- {}",
                wins0,
                100.0 * wins0 as f64 / SEEDS as f64,
                wins1,
                100.0 * wins1 as f64 / SEEDS as f64,
                decided,
                gap,
                bound,
                if gap <= bound { "fair" } else { "UNFAIR" }
            );
        }
    }
}
