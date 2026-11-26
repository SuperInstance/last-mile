//! The fairness guard: over many seeds the two squads must be even, and
//! no round may end in a 0-0 stalemate.
//!
//! The rounds are the shipped ones — `Header::default_round(seed, ...)`
//! with the default parameters and the `beacons` ruleset — played headless
//! for the full `time_limit_ticks`.
//!
//! `FAIRNESS_HARNESS` names a harness directory, relative to the repository
//! root or absolute, and the guard then plays *that* harness's reflex. The
//! guard is about a doctrine, and a doctrine is a harness: the shared
//! target's arms are judged by the same bound as the shipped one, and a
//! harness whose walk favours one side fails the guard the same way.
//!
//! The guard plays hundreds of rounds, so it is slow in debug builds: it is
//! marked `#[ignore]` and `scripts/check.sh` runs it in release, where the
//! seeds are spread over the available threads.

use std::path::Path;
use std::sync::OnceLock;

use lm_core::header::Header;
use lm_core::params::Params;
use lm_core::sim::Sim;

/// The shipped reflex, the one a standard round runs.
const DEFAULT_REFLEX: &str = include_str!("../../../assets/reflexes/default.rhai");

/// The number of seeds the guard plays.
const SEEDS: u64 = 200;

/// The first seed the guard plays, or 1.
///
/// `FAIRNESS_SEED_FROM` moves the block, so the same measurement can be run
/// over a different two hundred seeds: the seeds are a block, and a
/// measurement that only holds for one block is a measurement of that block.
fn first_seed() -> u64 {
    std::env::var("FAIRNESS_SEED_FROM")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(1)
}

/// What one round produced.
struct Round {
    /// The seed the round ran on.
    seed: u64,
    /// The score of every squad, in squad order.
    scores: Vec<i64>,
    /// The winning squad, or `None` when the round was a draw.
    winner: Option<u8>,
}

/// The integer square root, by Newton's method.
///
/// The crate does its arithmetic in integers only, and so does the guard:
/// the binomial bound has to be the same number on every machine.
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

/// The reflex the guard plays: the shipped one, or the one of the harness
/// the caller names.
fn reflex() -> &'static str {
    static REFLEX: OnceLock<String> = OnceLock::new();
    REFLEX.get_or_init(|| {
        let Ok(name) = std::env::var("FAIRNESS_HARNESS") else {
            return DEFAULT_REFLEX.to_owned();
        };
        let path = if Path::new(&name).is_absolute() {
            Path::new(&name).join("reflexes").join("default.rhai")
        } else {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(&name)
                .join("reflexes")
                .join("default.rhai")
        };
        std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("the harness's reflex reads: {path:?}: {err}"))
    })
}

/// Play one headless round and report what it produced.
fn play(seed: u64, ticks: u64, reflex: &str) -> Round {
    let h = Header::default_round(seed, reflex);
    let mut sim = Sim::new(&h).expect("the round starts");
    // The round's window is the time limit **plus a few ticks**: the rules
    // declare the outcome on the tick after the limit is reached, so a round
    // that ends on the clock is only declared a tick later. The headless
    // sweep (`crates/lm-core/tests/decisiveness.rs`) plays the same window,
    // and the two have to agree: the sweep counts a round that ends on the
    // clock with one squad ahead as a **win** for that squad, and a guard
    // that stopped at the limit would call the same round a draw and
    // measure a different thing.
    for _ in 0..ticks + 16 {
        sim.step(&[]);
        if sim.outcome().is_some() {
            break;
        }
    }
    let outcome = sim.outcome().cloned();
    Round {
        seed,
        scores: sim.scores().to_vec(),
        winner: outcome.as_ref().and_then(|o| o.winner),
    }
}

/// Play every seed of the guard.
///
/// The seeds are independent, so they are spread over the available
/// threads: the guard plays hundreds of rounds and the wall-clock is what
/// matters.
fn play_seeds() -> Vec<Round> {
    let ticks = u64::from(Params::default().time_limit_ticks);
    let reflex = reflex();
    let reflex: &'static str = reflex;
    let jobs = std::thread::available_parallelism()
        .map(|n| n.get() as u64)
        .unwrap_or(1)
        .clamp(1, SEEDS);
    let chunk = SEEDS.div_ceil(jobs);
    let block_first = first_seed();
    // The block is SEEDS seeds long, whatever the first one is: the last seed
    // of the block bounds every job, so the block is played exactly once.
    let block_last = block_first + SEEDS - 1;
    let mut rounds: Vec<Round> = Vec::new();
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for job in 0..jobs {
            let first = block_first + job * chunk;
            let last = (first + chunk - 1).min(block_last);
            if first > last {
                continue;
            }
            handles.push(scope.spawn(move || {
                let mut local = Vec::new();
                for seed in first..=last {
                    local.push(play(seed, ticks, reflex));
                }
                local
            }));
        }
        for handle in handles {
            rounds.extend(handle.join().expect("the guard thread finishes"));
        }
    });
    rounds.sort_by_key(|r| r.seed);
    rounds
}

/// The rounds of the guard, played once and shared by the tests.
fn rounds() -> &'static [Round] {
    static ROUNDS: OnceLock<Vec<Round>> = OnceLock::new();
    ROUNDS.get_or_init(play_seeds)
}

/// The two squads must win about as often as each other.
///
/// Over the seeds of the guard the win counts of the two squads have to
/// sit within a 2-sigma binomial bound of each other: with `n` decided
/// rounds every round is a coin flip, so the counts differ by at most
/// `2 * sqrt(n)` in the overwhelming majority of the worlds. A game that
/// is not even — the beacons laid out closer to one squad, or the acting
/// order always handing the first tag to the same squad — breaks it.
#[test]
#[ignore = "plays 200 rounds; run it with --release --ignored"]
fn fairness_over_seeds() {
    let rounds = rounds();
    let wins = |squad: u8| rounds.iter().filter(|r| r.winner == Some(squad)).count() as u64;
    let (wins0, wins1) = (wins(0), wins(1));
    let decided = wins0 + wins1;
    assert!(
        decided >= 20,
        "the guard needs decided rounds to compare: {decided} of {}",
        rounds.len()
    );
    let bound = 2 * isqrt(decided);
    let gap = wins0.abs_diff(wins1);
    println!(
        "the guard: squad 0 won {wins0}, squad 1 won {wins1}, {decided} decided rounds, \
         a gap of {gap} against the 2-sigma bound of {bound} ({} rounds drawn)",
        rounds.len() - decided as usize
    );
    assert!(
        gap <= bound,
        "squad 0 won {wins0}, squad 1 won {wins1}: a gap of {gap} is outside the \
         2-sigma bound of {bound} over {decided} decided rounds"
    );
}

/// No round may end in a 0-0 stalemate.
///
/// A round that ends with both squads on zero points is a standoff: the
/// squads never met and never took a beacon. The motor's pinned leading
/// axis used to lock two mirrored squads into exactly that — they chased
/// each other's jittering column and the leading-axis correction ate the
/// whole tick budget, so the cross-axis gap never closed (seed 5 froze on
/// 0-0 from tick ~2100 to 9000). The motor alternates its leading axis
/// now, and the standoff must not come back.
#[test]
#[ignore = "plays 200 rounds; run it with --release --ignored"]
fn no_round_ends_in_a_stalemate() {
    let rounds = rounds();
    let stalemates: Vec<u64> = rounds
        .iter()
        .filter(|r| r.scores.iter().all(|&s| s == 0))
        .map(|r| r.seed)
        .collect();
    assert!(
        stalemates.is_empty(),
        "these rounds ended 0-0: {stalemates:?}"
    );
}
