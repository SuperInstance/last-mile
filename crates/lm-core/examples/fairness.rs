//! The fairness probe: who wins the default round, over many seeds?
//!
//! A test-only measurement tool (BRIEF: fairness and stalemate
//! investigation). It plays `Header::default_round(seed, DEFAULT_REFLEX)`
//! rounds headless and reports, per squad: wins, draws, 0-0 stalemates, the
//! mean score margin and the tag counts. The table is printed seed by seed.
//!
//! The table takes three positional arguments — the last seed, the ticks a
//! round may run and the number of threads — and the seeds start at 1.
//!
//! ```text
//! cargo run -p lm-core --release --example fairness -- 200 9000
//! ```
//!
//! These modes sit next to the table:
//!
//! * `--swap` plays the same seeds with the roster's squad ids swapped.
//!   The swap relabels the squads and moves the bodies to the other
//!   spawn, so a fair game gives the mirrored scores.
//! * `--symmetry` checks the generated worlds for exact 180° point
//!   symmetry, beacons and spawns included.
//! * `--race` prints the distance from each spawn to each beacon: two
//!   squads with the same profile are dealt the same race.
//! * `--firsttag` reports which squad lands the first tag of each round
//!   and at which phase of the eight-tick acting order.
//! * `--relabel SEED [TICKS]` runs the normal and the swapped round of one
//!   seed side by side and reports the first tick where the mirrored
//!   state differs.
//! * `--trace SEED [TICKS] [EVERY]` traces one round tick by tick: where
//!   the agents stand, where they are headed and who holds what.
//!
//! ```text
//! cargo run -p lm-core --release --example fairness -- 200 9000 --swap
//! cargo run -p lm-core --release --example fairness -- --symmetry
//! cargo run -p lm-core --release --example fairness -- --race
//! cargo run -p lm-core --release --example fairness -- --firsttag
//! cargo run -p lm-core --release --example fairness -- --relabel 5 9000
//! cargo run -p lm-core --release --example fairness -- --trace 5 9000 300
//! ```

use lm_core::header::Header;
use lm_core::params::Params;
use lm_core::rules::Event;
use lm_core::sim::Sim;
use lm_core::world::{generate, CellXY, World};

/// The shipped reflex, the one a standard round runs.
const DEFAULT_REFLEX: &str = include_str!("../../../assets/reflexes/default.rhai");

/// What one round produced.
struct Round {
    seed: u64,
    /// The score of every squad, in squad order.
    scores: Vec<i64>,
    /// The winning squad, or `None` for a draw.
    winner: Option<u8>,
    /// The tick the round ended on.
    over_tick: Option<u64>,
    /// How many tags each squad landed.
    tags: [u64; 2],
}

/// The header of a default round, with the roster's squad ids swapped when
/// `swap` is set.
///
/// The swap relabels the squads and changes nothing else: worldgen puts
/// squad 0 on spawn 0 and squad 1 on spawn 1 whatever the roster says, and
/// the roster is walked in agent-id order, so the agents keep their cells.
/// A label-neutral simulation therefore plays the same physical round.
fn header(seed: u64, swap: bool) -> Header {
    let mut h = Header::default_round(seed, DEFAULT_REFLEX);
    if swap {
        for entry in &mut h.roster {
            entry.squad ^= 1;
        }
    }
    h
}

/// Play one headless round of `ticks` ticks and summarize it.
fn play(seed: u64, ticks: u64, swap: bool) -> Round {
    let h = header(seed, swap);
    let mut sim = Sim::new(&h).expect("the round starts");
    let mut tags = [0u64; 2];
    for _ in 0..ticks {
        let out = sim.step(&[]);
        for event in &out.events {
            if let Event::Tagged { by, .. } = event {
                let squad = usize::from(sim.agent(*by).map(|a| a.squad).unwrap_or(0));
                tags[squad.min(1)] += 1;
            }
        }
        if sim.outcome().is_some() {
            break;
        }
    }
    let outcome = sim.outcome().cloned();
    Round {
        seed,
        scores: sim.scores().to_vec(),
        winner: outcome.as_ref().and_then(|o| o.winner),
        over_tick: outcome.as_ref().map(|o| o.tick),
        tags,
    }
}

/// The cells the agents of a round stand on, in agent-id order.
fn cells(sim: &Sim) -> Vec<CellXY> {
    let world = sim.world();
    sim.agent_ids()
        .into_iter()
        .map(|id| {
            let a = sim.agent(id).expect("roster id");
            world.grid.cell_of(a.pos, world.cell_units)
        })
        .collect()
}

/// The stun of every agent of a round, in agent-id order.
fn stuns(sim: &Sim) -> Vec<Option<u64>> {
    sim.agent_ids()
        .into_iter()
        .map(|id| sim.agent(id).expect("roster id").stunned_until)
        .collect()
}

/// Run the normal and the swapped round of one seed side by side and report
/// the first tick where the mirrored state differs.
///
/// The swap puts every agent on the 180° rotation of the cell it stood on
/// in the normal round: squad 0's spawn becomes squad 1's, and worldgen
/// mirrors the odd squads' offsets to match. So a mirror-symmetric
/// simulation plays the same round with every cell rotated, the same agent
/// stunned and the same score — mirrored. A difference names a rule that
/// reads the squad id or the absolute cell.
fn relabel(seed: u64, ticks: u64) {
    let mut normal = Sim::new(&header(seed, false)).expect("normal starts");
    let mut swapped = Sim::new(&header(seed, true)).expect("swapped starts");
    let (w, h) = (normal.world().grid.w, normal.world().grid.h);
    let mirror = |c: CellXY| CellXY {
        x: w - 1 - c.x,
        y: h - 1 - c.y,
    };

    for t in 0..ticks {
        let before_n = cells(&normal);
        let before_s = cells(&swapped);
        let out_n = normal.step(&[]);
        let out_s = swapped.step(&[]);
        let after_n = cells(&normal);
        let after_s = cells(&swapped);
        let mirrored: Vec<CellXY> = after_n.iter().copied().map(mirror).collect();
        if mirrored != after_s || stuns(&normal) != stuns(&swapped) {
            println!("seed {seed}: the mirrored state first differs at tick {t}");
            println!("  normal cells before: {before_n:?}");
            println!("  swapped cells before: {before_s:?}");
            println!("  normal cells after : {after_n:?}");
            println!("  rotated normal     : {mirrored:?}");
            println!("  swapped cells after: {after_s:?}");
            println!("  normal stuns : {:?}", stuns(&normal));
            println!("  swapped stuns: {:?}", stuns(&swapped));
            println!("  normal events: {:?}", out_n.events);
            println!("  swapped events: {:?}", out_s.events);
            println!(
                "  scores: normal {:?} swapped {:?}",
                normal.scores(),
                swapped.scores()
            );
            return;
        }
        if normal.outcome().is_some() || swapped.outcome().is_some() {
            println!(
                "seed {seed}: mirrored to the end (tick {t}); scores normal {:?} swapped {:?}",
                normal.scores(),
                swapped.scores()
            );
            return;
        }
    }
    println!("seed {seed}: mirrored through {ticks} ticks");
}

/// Print the fairness table of the rounds and the summary under it.
fn report(rounds: &[Round], ticks: u64, swap: bool) {
    let mode = if swap { " (squads swapped)" } else { "" };
    println!(
        "=== fairness over {} seeds, {ticks} ticks{mode} ===",
        rounds.len()
    );
    println!(
        "{:>5} {:>7} {:>7} {:>7} {:>7} {:>6} {:>6} {:>6}",
        "seed", "s0", "s1", "winner", "tick", "tag0", "tag1", "stale"
    );
    for r in rounds {
        let winner = match r.winner {
            Some(0) => "s0",
            Some(_) => "s1",
            None => "draw",
        };
        let stale = if r.scores.iter().all(|&s| s == 0) {
            "0-0"
        } else {
            ""
        };
        let over = r
            .over_tick
            .map_or_else(|| "-".to_owned(), |t| t.to_string());
        println!(
            "{:>5} {:>7} {:>7} {:>7} {:>7} {:>6} {:>6} {:>6}",
            r.seed,
            r.scores.first().copied().unwrap_or(0),
            r.scores.get(1).copied().unwrap_or(0),
            winner,
            over,
            r.tags[0],
            r.tags[1],
            stale
        );
    }

    let wins = |squad: u8| rounds.iter().filter(|r| r.winner == Some(squad)).count();
    let draws = rounds.iter().filter(|r| r.winner.is_none()).count();
    let stalemates = rounds
        .iter()
        .filter(|r| r.scores.iter().all(|&s| s == 0))
        .count();
    let decided = rounds.iter().filter(|r| r.winner.is_some()).count();
    // Fixed-point means, in tenths: the core never touches a float and the
    // probe keeps the habit.
    let margin_sum: i128 = rounds
        .iter()
        .map(|r| {
            (r.scores.first().copied().unwrap_or(0) - r.scores.get(1).copied().unwrap_or(0)) as i128
        })
        .sum();
    let abs_margin_sum: i128 = rounds
        .iter()
        .map(|r| {
            (r.scores.first().copied().unwrap_or(0) - r.scores.get(1).copied().unwrap_or(0)).abs()
                as i128
        })
        .sum();
    let n = i128::from(rounds.len().max(1) as u64);
    let tenths = |v: i128| format!("{}.{}", v / 10, (v % 10).abs());
    println!("---");
    println!("wins squad 0: {}", wins(0));
    println!("wins squad 1: {}", wins(1));
    println!("draws: {draws} (decided: {decided}, 0-0 stalemates: {stalemates})");
    println!("mean margin (s0 - s1): {}", tenths(margin_sum * 10 / n));
    println!("mean |margin|: {}", tenths(abs_margin_sum * 10 / n));
}

/// Trace one round tick by tick: where the agents stand, where they are
/// headed and who holds what.
fn trace(seed: u64, ticks: u64, every: u64) {
    let h = header(seed, false);
    let mut sim = Sim::new(&h).expect("the round starts");
    let world = sim.world().clone();
    println!(
        "=== trace seed {seed}: beacons {:?} spawns {:?}",
        world.beacons, world.spawns
    );
    for t in 0..ticks {
        let out = sim.step(&[]);
        if t % every == 0 || t + 1 == ticks {
            let mut line = format!("t{t:>5} scores {:?}", sim.scores());
            for id in sim.agent_ids() {
                let a = sim.agent(id).expect("roster id");
                let cell = world.grid.cell_of(a.pos, world.cell_units);
                let (px, py) = a.pos.to_units();
                let goal = a.route.as_ref().map(|(g, _, _)| *g);
                let next = a.route.as_ref().and_then(|(_, path, _)| {
                    path.iter()
                        .position(|c| *c == cell)
                        .and_then(|i| path.get(i + 1).copied())
                });
                let stunned = if a.stunned_until.is_some_and(|e| t < e) {
                    "*"
                } else {
                    ""
                };
                line.push_str(&format!(
                    " {id}:({},{})@({px},{py})->{goal:?}/{next:?}{stunned}",
                    cell.x, cell.y
                ));
            }
            println!("{line}");
        }
        if !out.events.is_empty() {
            println!("  t{t} events {:?}", out.events);
        }
        if sim.outcome().is_some() {
            println!("t{t}: OVER {:?}", sim.outcome());
            return;
        }
    }
    println!("t{ticks}: no outcome, final scores {:?}", sim.scores());
}

/// Report which squad lands the first tag of each round, and at which
/// phase of the eight-tick acting order it happens.
///
/// The acting order rotates by one agent every tick, so the squad that
/// acts first alternates. The phase of the first tag decides the round,
/// and a phase that is not spread evenly is a bias.
fn first_tag(max_seed: u64, ticks: u64) {
    let mut by_squad = [0u64; 2];
    let mut by_phase = [[0u64; 2]; 8];
    let mut no_tag = 0u64;
    // The squad of the tagger and the squad of its target: which pairs
    // meet first.
    let mut pairs: std::collections::BTreeMap<(u8, u8), u64> = std::collections::BTreeMap::new();
    let jobs: u64 = std::thread::available_parallelism()
        .map(|n| n.get() as u64)
        .unwrap_or(1)
        .clamp(1, max_seed);
    let chunk = max_seed.div_ceil(jobs);
    let mut firsts: Vec<Option<(u64, u8)>> = Vec::new();

    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for job in 0..jobs {
            let first = job * chunk + 1;
            let last = ((job + 1) * chunk).min(max_seed);
            if first > last {
                continue;
            }
            handles.push(scope.spawn(move || {
                let mut local = Vec::new();
                let mut counts: std::collections::BTreeMap<(u8, u8), u64> =
                    std::collections::BTreeMap::new();
                for seed in first..=last {
                    let mut sim = Sim::new(&header(seed, false)).expect("the round starts");
                    let mut first: Option<(u64, u8)> = None;
                    for t in 0..ticks {
                        let out = sim.step(&[]);
                        for event in &out.events {
                            if let Event::Tagged { by, target } = event {
                                let squad = sim.agent(*by).map(|a| a.squad).unwrap_or(0);
                                if first.is_none() {
                                    first = Some((t, squad));
                                    let target_squad =
                                        sim.agent(*target).map(|a| a.squad).unwrap_or(0);
                                    *counts.entry((squad, target_squad)).or_insert(0) += 1;
                                }
                            }
                        }
                        if sim.outcome().is_some() {
                            break;
                        }
                    }
                    local.push(first);
                }
                (local, counts)
            }));
        }
        for handle in handles {
            let (local, counts) = handle.join().expect("the probe thread finishes");
            firsts.extend(local);
            for (pair, count) in counts {
                *pairs.entry(pair).or_insert(0) += count;
            }
        }
    });
    for first in firsts {
        match first {
            Some((t, squad)) => {
                by_squad[usize::from(squad)] += 1;
                by_phase[(t % 8) as usize][usize::from(squad)] += 1;
            }
            None => no_tag += 1,
        }
    }
    println!("=== first tag of {max_seed} rounds ===");
    println!("squad 0 lands it: {}", by_squad[0]);
    println!("squad 1 lands it: {}", by_squad[1]);
    println!("no tag at all: {no_tag}");
    println!("phase (tick mod 8): squad 0 / squad 1");
    for (phase, counts) in by_phase.iter().enumerate() {
        println!("  {phase}: {:>4} / {:>4}", counts[0], counts[1]);
    }
    println!("tagger squad -> target squad: rounds");
    for ((tagger, target), count) in pairs {
        println!("  {tagger} -> {target}: {count}");
    }
}

/// Report the point symmetry of the generated worlds.
fn symmetry() {
    println!("=== point symmetry of the default world, seeds 1..=50 ===");
    let p = Params::default();
    let mut bad = 0;
    for seed in 1..=50u64 {
        let w = generate(seed, &p);
        let mirror = |c: CellXY| w.grid.mirror(c);
        let mut grid_ok = true;
        for y in 0..w.grid.h {
            for x in 0..w.grid.w {
                let c = CellXY { x, y };
                if w.grid.get(c) != w.grid.get(mirror(c)) {
                    grid_ok = false;
                }
            }
        }
        let mut rotated: Vec<CellXY> = w.beacons.iter().copied().map(mirror).collect();
        rotated.sort();
        let mut beacons = w.beacons.clone();
        beacons.sort();
        let beacons_ok = beacons == rotated;
        // The spawns pair up: squad 2k+1's spawn is squad 2k's rotated.
        let spawns_ok = w.spawns.len().is_multiple_of(2)
            && w.spawns
                .iter()
                .enumerate()
                .all(|(i, &s)| mirror(s) == w.spawns[w.spawns.len() - 1 - i]);
        if !(grid_ok && beacons_ok && spawns_ok) {
            bad += 1;
        }
        println!(
            "seed {seed:>2}: grid {} beacons {} spawns {} | beacons {:?} rotated {:?}",
            if grid_ok { "OK" } else { "ASYMMETRIC" },
            if beacons_ok { "OK" } else { "ASYMMETRIC" },
            if spawns_ok { "OK" } else { "ASYMMETRIC" },
            w.beacons,
            w.beacons.iter().copied().map(mirror).collect::<Vec<_>>(),
        );
    }
    println!("---");
    println!("asymmetric worlds: {bad} of 50");
}

/// The distances from each squad's spawn to every beacon, sorted.
///
/// Two squads with the same distance profile are dealt the same race; a
/// different profile is a bias in the layout.
fn race_profile(w: &World) -> (Vec<u32>, Vec<u32>) {
    let cheb =
        |a: CellXY, b: CellXY| u32::from(a.x.abs_diff(b.x)).max(u32::from(a.y.abs_diff(b.y)));
    let mut s0: Vec<u32> = w.beacons.iter().map(|&b| cheb(w.spawns[0], b)).collect();
    let mut s1: Vec<u32> = w.beacons.iter().map(|&b| cheb(w.spawns[1], b)).collect();
    s0.sort_unstable();
    s1.sort_unstable();
    (s0, s1)
}

/// Report the race profile of the default world.
fn race() {
    println!("=== distance from each spawn to each beacon, seeds 1..=8 ===");
    let p = Params::default();
    for seed in 1..=8u64 {
        let w = generate(seed, &p);
        let (s0, s1) = race_profile(&w);
        println!(
            "seed {seed:>2}: beacons {:?} | squad 0 {s0:?} | squad 1 {s1:?} | {}",
            w.beacons,
            if s0 == s1 { "even" } else { "UNEVEN" }
        );
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--symmetry") {
        symmetry();
        return;
    }
    if args.iter().any(|a| a == "--race") {
        race();
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--relabel") {
        let seed: u64 = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(1);
        let ticks: u64 = args
            .get(i + 2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(9_000);
        relabel(seed, ticks);
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--firsttag") {
        let seed: u64 = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(200);
        let ticks: u64 = args
            .get(i + 2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(9_000);
        first_tag(seed, ticks);
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--trace") {
        let seed: u64 = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(5);
        let ticks: u64 = args
            .get(i + 2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(9_000);
        let every: u64 = args.get(i + 3).and_then(|s| s.parse().ok()).unwrap_or(300);
        trace(seed, ticks, every);
        return;
    }
    let swap = args.iter().any(|a| a == "--swap");
    let plain = || args.iter().filter(|a| !a.starts_with("--"));
    let max_seed: u64 = plain().next().and_then(|s| s.parse().ok()).unwrap_or(200);
    let ticks: u64 = plain().nth(1).and_then(|s| s.parse().ok()).unwrap_or(9_000);
    let jobs: u64 = plain()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get() as u64)
                .unwrap_or(1)
        })
        .clamp(1, max_seed);
    let from: u64 = plain().nth(3).and_then(|s| s.parse().ok()).unwrap_or(1);

    let rounds = play_all(from, max_seed, ticks, swap, jobs);
    report(&rounds, ticks, swap);
}

/// Play every seed of `from..=max_seed` and collect the results.
///
/// The seeds are independent, so they are spread over `jobs` threads: the
/// probe plays hundreds of rounds and the wall-clock is what matters.
fn play_all(from: u64, max_seed: u64, ticks: u64, swap: bool, jobs: u64) -> Vec<Round> {
    let span = max_seed.saturating_sub(from).saturating_add(1);
    let chunk = span.div_ceil(jobs);
    let mut rounds: Vec<Round> = Vec::new();
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for job in 0..jobs {
            let first = from + job * chunk;
            let last = (first + chunk - 1).min(max_seed);
            if first > last {
                continue;
            }
            handles.push(scope.spawn(move || {
                let mut local = Vec::new();
                for seed in first..=last {
                    local.push(play(seed, ticks, swap));
                }
                local
            }));
        }
        for handle in handles {
            rounds.extend(handle.join().expect("the probe thread finishes"));
        }
    });
    rounds.sort_by_key(|r| r.seed);
    rounds
}
