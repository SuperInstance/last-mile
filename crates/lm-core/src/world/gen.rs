//! Worldgen L0: the seed-grown, mirrored, connectivity-guaranteed grid.
//!
//! The map is point-symmetric under a 180° rotation, so neither squad can
//! be dealt a luckier board (spec §4 fairness). Walls and cover are grown
//! in the top half from the worldgen RNG stream and mirrored onto the
//! bottom half; spawns and beacons sit at pinned, symmetric positions; and
//! a repair pass carves Floor corridors until every spawn and beacon is
//! reachable from spawn 0, so no seed can wall the round off.

use super::grid::{CellKind, CellXY, Grid};
use super::paint::Paint;
use super::World;
use crate::fixed::UNITS_PER_M;
use crate::params::Params;
use crate::rng::{Domain, Rng};

/// Squad 0 spawns this far from the top-left corner; the plan pins (2,2).
const SPAWN_EDGE: u16 = 2;
/// The smallest wall-block side, in cells.
const WALL_SIDE_MIN: u32 = 2;
/// Wall-block sides span `WALL_SIDE_MIN ..= WALL_SIDE_MIN + WALL_SIDE_SPAN - 1`
/// cells (2..=6, as pinned by the plan).
const WALL_SIDE_SPAN: u32 = 5;

/// Grow the round's world from `seed` under the shape and density `params`.
///
/// The grid is `world_chunks * chunk_m / cell_m` cells per axis (128 × 128
/// by default). Walls and cover fill the top half from the worldgen stream
/// and are mirrored by 180° rotation; spawns and beacons are pinned at
/// symmetric positions with their 3×3 neighbourhoods floored; and every
/// spawn and beacon ends up reachable from spawn 0 by corridor repair.
pub fn generate(seed: u64, p: &Params) -> World {
    let mut rng = Rng::stream(seed, Domain::Worldgen, 0);
    let cell_m = u16::from(p.cell_m).max(1);
    let w = u16::from(p.world_chunks.0) * u16::from(p.chunk_m) / cell_m;
    let h = u16::from(p.world_chunks.1) * u16::from(p.chunk_m) / cell_m;
    let cell_units = i32::from(cell_m) * UNITS_PER_M;
    let mut grid = Grid::new(w, h);
    if w == 0 || h == 0 {
        // A degenerate shape that `Params::validate` refuses; grow nothing.
        return World {
            grid,
            beacons: Vec::new(),
            spawns: Vec::new(),
            cell_units,
            paint: Paint::new(),
        };
    }

    let half = h / 2;
    let half_cells = u32::from(w) * u32::from(half);
    // The fill loops below stop at their density target; the attempt
    // budget is only a termination guard for pathological densities
    // (100% walls over overlapping blocks), never reached at sane ones.
    let attempt_cap = u32::from(w) * u32::from(h);

    // Walls: rectangular blocks of 2..=6 cells per side, placed in the top
    // half until it carries `wall_density_pct` (the mirror doubles the
    // count to the same percentage of the whole grid).
    let wall_target = (u64::from(half_cells) * u64::from(p.wall_density_pct) / 100) as u32; // u64: no overflow at large worlds (audit R1)
    let mut walls = 0u32;
    let mut attempts = 0u32;
    while walls < wall_target && attempts < attempt_cap {
        attempts += 1;
        let sx = WALL_SIDE_MIN + rng.below(WALL_SIDE_SPAN);
        let sy = WALL_SIDE_MIN + rng.below(WALL_SIDE_SPAN);
        let x0 = rng.below(u32::from(w));
        let y0 = rng.below(u32::from(half));
        let x1 = (x0 + sx).min(u32::from(w));
        let y1 = (y0 + sy).min(u32::from(half));
        for y in y0..y1 {
            for x in x0..x1 {
                let c = CellXY {
                    x: x as u16,
                    y: y as u16,
                };
                if grid.get(c) == CellKind::Floor {
                    grid.set(c, CellKind::Wall);
                    walls += 1;
                }
            }
        }
    }

    // Cover: single scattered cells on still-Floor ground, to
    // `cover_density_pct`.
    let cover_target = (u64::from(half_cells) * u64::from(p.cover_density_pct) / 100) as u32;
    let mut covers = 0u32;
    let mut attempts = 0u32;
    while covers < cover_target && attempts < attempt_cap {
        attempts += 1;
        let c = CellXY {
            x: rng.below(u32::from(w)) as u16,
            y: rng.below(u32::from(half)) as u16,
        };
        if grid.get(c) == CellKind::Floor {
            grid.set(c, CellKind::Cover);
            covers += 1;
        }
    }

    // Mirror the top half onto the bottom half by 180° rotation.
    for y in half..h {
        for x in 0..w {
            let c = CellXY { x, y };
            let m = grid.mirror(c);
            grid.set(c, grid.get(m));
        }
    }

    // Spawns: squad 0 at (2,2) and squad 1 at its 180° mirror. Further
    // squads (M1 never uses them) stack in further mirrored pairs.
    let mut spawns = Vec::with_capacity(usize::from(p.squads));
    let top = half.saturating_sub(1);
    for s in 0..p.squads {
        let base = CellXY {
            x: SPAWN_EDGE.min(w - 1),
            y: (SPAWN_EDGE + u16::from(s / 2)).min(top),
        };
        spawns.push(if s % 2 == 0 { base } else { grid.mirror(base) });
    }

    // Beacons: every beacon comes with its 180°-rotated partner, so the
    // beacon set is point-symmetric exactly like the grid. Pair 0
    // straddles the centre — the rotation maps one of its cells onto the
    // other — which keeps a contested middle; the remaining pairs divide
    // the left half of the centre band, the first side pair at w/4. An
    // odd count has no such layout (no cell is its own rotation on an
    // even grid), so `Params::validate` refuses it: the old left-right
    // pairs on the centre row sat one cell closer to squad 1 and biased
    // the round 101:52 over 200 seeds.
    let mut beacons = Vec::with_capacity(usize::from(p.beacons));
    let (cx, cy) = (w / 2, h / 2);
    let pairs = u32::from(p.beacons) / 2;
    for k in 0..pairs {
        let x = if k == 0 {
            u32::from(cx)
        } else {
            u32::from(w) / 2 * k / pairs
        };
        let a = CellXY {
            x: x as u16,
            y: cy.saturating_sub(1),
        };
        beacons.push(a);
        beacons.push(grid.mirror(a));
    }

    // No spawn or beacon may start walled or blinded: force it and its 3×3
    // neighbourhood — and each counterpart's, keeping the point symmetry —
    // to Floor.
    let required: Vec<CellXY> = spawns
        .iter()
        .copied()
        .chain(beacons.iter().copied())
        .collect();
    for c in &required {
        let m = grid.mirror(*c);
        clear_neighbourhood(&mut grid, *c);
        clear_neighbourhood(&mut grid, m);
    }

    // Connectivity repair: flood from spawn 0, and while any required cell
    // is missing, carve a mirrored L-shaped Floor corridor (x leg first,
    // then y) from the first missing cell to the nearest reached cell.
    // Each carve connects its cell, so this ends within `required.len()`
    // passes.
    if !spawns.is_empty() {
        let source = spawns[0];
        loop {
            let reached = flood_from(&grid, source);
            let Some(cell) = required.iter().copied().find(|c| !reached[grid.idx(*c)]) else {
                break;
            };
            carve_to_nearest(&mut grid, cell, &reached);
        }
    }

    World {
        grid,
        beacons,
        spawns,
        cell_units,
        paint: Paint::new(),
    }
}

/// Force `c`'s 3×3 neighbourhood to Floor, clipped at the borders.
fn clear_neighbourhood(grid: &mut Grid, c: CellXY) {
    for dy in -1i32..=1 {
        for dx in -1i32..=1 {
            let (x, y) = (i32::from(c.x) + dx, i32::from(c.y) + dy);
            if grid.in_bounds(x, y) {
                grid.set(
                    CellXY {
                        x: x as u16,
                        y: y as u16,
                    },
                    CellKind::Floor,
                );
            }
        }
    }
}

/// The cells reachable from `start` through 4-connected walkable cells.
fn flood_from(grid: &Grid, start: CellXY) -> Vec<bool> {
    let mut reached = vec![false; grid.cells.len()];
    if !grid.in_bounds(i32::from(start.x), i32::from(start.y)) {
        return reached;
    }
    reached[grid.idx(start)] = true;
    let mut stack = vec![start];
    while let Some(c) = stack.pop() {
        for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            let (x, y) = (i32::from(c.x) + dx, i32::from(c.y) + dy);
            if !grid.in_bounds(x, y) {
                continue;
            }
            let n = CellXY {
                x: x as u16,
                y: y as u16,
            };
            let i = grid.idx(n);
            if !reached[i] && grid.walkable(n) {
                reached[i] = true;
                stack.push(n);
            }
        }
    }
    reached
}

/// Carve a mirrored, L-shaped Floor corridor from `cell` to the nearest
/// reached cell (Manhattan distance, ties to the lowest row-major cell):
/// the x leg runs along `cell.y`, then the y leg at the target's x.
fn carve_to_nearest(grid: &mut Grid, cell: CellXY, reached: &[bool]) {
    let mut nearest: Option<(u32, CellXY)> = None;
    for y in 0..grid.h {
        for x in 0..grid.w {
            let c = CellXY { x, y };
            if !reached[grid.idx(c)] {
                continue;
            }
            let d = u32::from(c.x.abs_diff(cell.x)) + u32::from(c.y.abs_diff(cell.y));
            if nearest.is_none_or(|(bd, _)| d < bd) {
                nearest = Some((d, c));
            }
        }
    }
    let Some((_, target)) = nearest else {
        return;
    };
    let (x0, x1) = if cell.x <= target.x {
        (cell.x, target.x)
    } else {
        (target.x, cell.x)
    };
    for x in x0..=x1 {
        carve_cell(grid, CellXY { x, y: cell.y });
    }
    let (y0, y1) = if cell.y <= target.y {
        (cell.y, target.y)
    } else {
        (target.y, cell.y)
    };
    for y in y0..=y1 {
        carve_cell(grid, CellXY { x: target.x, y });
    }
}

/// Floor a cell and its 180° mirror in one go, preserving point symmetry.
fn carve_cell(grid: &mut Grid, c: CellXY) {
    grid.set(c, CellKind::Floor);
    grid.set(grid.mirror(c), CellKind::Floor);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed::Pos;
    use crate::hash::StateHash;

    /// The plan's `hash_of` helper: the canonical state hash of a world.
    fn hash_of(w: &World) -> u64 {
        w.state_hash()
    }

    /// A 4-connected flood fill over walkable cells, kept independent of
    /// the implementation so the connectivity test cross-checks it.
    struct Flood<'a> {
        grid: &'a Grid,
        reached: Vec<bool>,
    }

    impl Flood<'_> {
        fn contains(&self, c: CellXY) -> bool {
            self.grid.in_bounds(i32::from(c.x), i32::from(c.y)) && self.reached[self.grid.idx(c)]
        }
    }

    fn flood(grid: &Grid, from: CellXY) -> Flood<'_> {
        let mut reached = vec![false; grid.cells.len()];
        let mut frontier = vec![from];
        if grid.in_bounds(i32::from(from.x), i32::from(from.y)) {
            reached[grid.idx(from)] = true;
        }
        while let Some(c) = frontier.pop() {
            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                let (x, y) = (i32::from(c.x) + dx, i32::from(c.y) + dy);
                if !grid.in_bounds(x, y) {
                    continue;
                }
                let n = CellXY {
                    x: x as u16,
                    y: y as u16,
                };
                if !reached[grid.idx(n)] && grid.walkable(n) {
                    reached[grid.idx(n)] = true;
                    frontier.push(n);
                }
            }
        }
        Flood { grid, reached }
    }

    #[test]
    fn deterministic() {
        let p = Params::default();
        assert_eq!(hash_of(&generate(42, &p)), hash_of(&generate(42, &p)));
        assert_ne!(hash_of(&generate(42, &p)), hash_of(&generate(43, &p)));
    }

    #[test]
    fn point_symmetric_for_fairness() {
        let w = generate(7, &Params::default());
        for y in 0..w.grid.h {
            for x in 0..w.grid.w {
                assert_eq!(
                    w.grid.get(CellXY { x, y }),
                    w.grid.get(CellXY {
                        x: w.grid.w - 1 - x,
                        y: w.grid.h - 1 - y
                    }),
                    "cell ({x},{y}) must equal its 180° mirror"
                );
            }
        }
    }

    #[test]
    fn worldgen_always_connected() {
        let p = Params::default();
        for seed in 0..500u64 {
            let w = generate(seed, &p);
            let r = flood(&w.grid, w.spawns[0]);
            assert!(
                w.spawns.iter().chain(&w.beacons).all(|c| r.contains(*c)),
                "seed {seed}"
            );
        }
    }

    #[test]
    fn counts() {
        let w = generate(1, &Params::default());
        assert_eq!(w.beacons.len(), 4);
        assert_eq!(w.spawns.len(), 2);
    }

    #[test]
    fn default_shape() {
        let w = generate(1, &Params::default());
        assert_eq!((w.grid.w, w.grid.h), (128, 128));
        assert_eq!(w.cell_units, 2_048);
        assert_eq!(
            w.spawns,
            vec![CellXY { x: 2, y: 2 }, CellXY { x: 125, y: 125 }]
        );
        // Pair 0 straddles the centre (each cell is the other's 180°
        // rotation); pair 1 sits at w/4 on the centre band, its partner at
        // the rotated cell.
        assert_eq!(
            w.beacons,
            vec![
                CellXY { x: 64, y: 63 },
                CellXY { x: 63, y: 64 },
                CellXY { x: 32, y: 63 },
                CellXY { x: 95, y: 64 },
            ]
        );
    }

    /// The fairness invariant the brief names: the whole world — grid,
    /// beacons and spawns — is invariant under the 180° rotation.
    #[test]
    fn world_is_point_symmetric_including_beacons_and_spawns() {
        let p = Params::default();
        for seed in 0..25u64 {
            let w = generate(seed, &p);
            let mirror = |c: CellXY| w.grid.mirror(c);
            for y in 0..w.grid.h {
                for x in 0..w.grid.w {
                    let c = CellXY { x, y };
                    assert_eq!(
                        w.grid.get(c),
                        w.grid.get(mirror(c)),
                        "seed {seed}: cell ({x},{y}) must equal its rotation"
                    );
                }
            }
            // The beacon multiset maps onto itself under the rotation.
            let mut rotated: Vec<CellXY> = w.beacons.iter().copied().map(mirror).collect();
            rotated.sort();
            let mut beacons = w.beacons.clone();
            beacons.sort();
            assert_eq!(
                beacons, rotated,
                "seed {seed}: the beacon set must be its own 180° rotation"
            );
            // Spawns pair up under the rotation: squad 2k+1's spawn is
            // squad 2k's spawn rotated.
            for s in (0..w.spawns.len()).step_by(2) {
                assert_eq!(
                    mirror(w.spawns[s]),
                    w.spawns[s + 1],
                    "seed {seed}: spawn pair {s} must be rotations"
                );
            }
        }
    }

    /// Even beacon counts beyond the default also pair up exactly.
    #[test]
    fn larger_beacon_counts_stay_paired() {
        let p = Params {
            beacons: 8,
            ..Params::default()
        };
        let w = generate(3, &p);
        let mut rotated: Vec<CellXY> = w.beacons.iter().map(|&b| w.grid.mirror(b)).collect();
        rotated.sort();
        let mut beacons = w.beacons.clone();
        beacons.sort();
        assert_eq!(beacons, rotated);
    }

    #[test]
    fn densities_land_near_their_params() {
        let w = generate(1, &Params::default());
        let total = w.grid.cells.len() as u64;
        let walls = w
            .grid
            .cells
            .iter()
            .filter(|k| **k == CellKind::Wall)
            .count() as u64;
        let cover = w
            .grid
            .cells
            .iter()
            .filter(|k| **k == CellKind::Cover)
            .count() as u64;
        // 18% walls and 6% cover by parameter; spawn/beacon clearing and
        // corridor repair shave a little off both.
        let wall_pct = walls * 100 / total;
        let cover_pct = cover * 100 / total;
        assert!(
            (14..=19).contains(&wall_pct),
            "wall density {wall_pct}% is off the 18% parameter"
        );
        assert!(
            (4..=7).contains(&cover_pct),
            "cover density {cover_pct}% is off the 6% parameter"
        );
    }

    #[test]
    fn cell_center_roundtrip() {
        let w = generate(1, &Params::default());
        let cu = w.cell_units;
        for c in [
            CellXY { x: 0, y: 0 },
            CellXY { x: 5, y: 9 },
            CellXY { x: 64, y: 64 },
            CellXY { x: 127, y: 127 },
        ] {
            assert_eq!(w.grid.cell_of(w.grid.center(c, cu), cu), c);
        }
        // Positions outside the grid saturate to the border cell.
        assert_eq!(
            w.grid.cell_of(Pos::from_units(-1, -1), cu),
            CellXY { x: 0, y: 0 }
        );
        assert_eq!(
            w.grid.cell_of(Pos::from_units(1 << 30, 1 << 30), cu),
            CellXY { x: 127, y: 127 }
        );
    }

    #[test]
    fn kinds_block_what_the_interface_says() {
        assert!(CellKind::Wall.blocks_move());
        assert!(CellKind::Wall.blocks_sight());
        assert!(!CellKind::Cover.blocks_move());
        assert!(CellKind::Cover.blocks_sight());
        assert!(!CellKind::Floor.blocks_move());
        assert!(!CellKind::Floor.blocks_sight());
    }

    #[test]
    fn world_hash_covers_beacons_and_spawns() {
        let mut a = generate(1, &Params::default());
        let b = generate(1, &Params::default());
        assert_eq!(a.state_hash(), b.state_hash());
        a.beacons[0].x = 63;
        assert_ne!(a.state_hash(), b.state_hash());
    }
}
