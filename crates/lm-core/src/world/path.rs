//! Integer Jump Point Search over the coarse grid.
//!
//! Movement model (shared with the A* reference in the tests): 8-connected
//! with strict no corner cutting — a diagonal step needs its target and both
//! orthogonal neighbours walkable — at step costs 10 orthogonal / 14
//! diagonal. Cover blocks sight only and stays walkable.
//!
//! The pruning rules are the Harabor–Grastien rules corrected for the strict
//! movement model, where every "forced" turn sits one cell further along
//! than in the classic rules (verified against A* on hundreds of random
//! maps, here and in the phase-0 prototype):
//!
//! * A straight scan in direction (dx, 0) stops at cell c iff a wall beside
//!   the PREVIOUS step opens up beside c: `!walk(c-(dx,0)+(0,s)) &&
//!   walk(c+(0,s))` for some side s. Under strict rules the legal turn
//!   around such a wall happens at c, not at the cell before it.
//! * A node with straight incoming direction expands the natural straight
//!   direction plus, for each forced side s, the straight aside (0, s) and
//!   the diagonal (dx, s) — both legal from c exactly when the side is open.
//! * A node with diagonal incoming direction expands the three natural
//!   directions only; the classic forced diagonals are illegal as direct
//!   steps under strict rules, and the detours they stand for are found by
//!   the straight natural jumps stopping via the shifted rule above.
//! * Diagonal scans stop where a straight sub-scan finds a stop (classic).
//!
//! The open list orders by (f, h, y, x) — cost, heuristic, then row-major
//! cell — a total, platform-independent tie-break that prefers the deeper
//! node at equal cost, so the same map always yields the same path.

use super::grid::{CellKind, CellXY, Grid};
use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// Cost of one orthogonal step.
const COST_STRAIGHT: u32 = 10;
/// Cost of one diagonal step.
const COST_DIAG: u32 = 14;

/// The eight directions, in the fixed order that keeps successor expansion
/// (and so parent choice among equal-cost nodes) deterministic.
const DIRS: [(i32, i32); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];

/// Whether (x, y) is inside the grid and not a Wall (cover is walkable).
fn walkable_at(grid: &Grid, x: i32, y: i32) -> bool {
    grid.in_bounds(x, y)
        && grid.get(CellXY {
            x: x as u16,
            y: y as u16,
        }) != CellKind::Wall
}

/// Whether the unit step from (x, y) along (dx, dy) is legal: a diagonal
/// needs both orthogonal neighbours walkable (strict no corner cutting).
fn step_legal(grid: &Grid, x: i32, y: i32, dx: i32, dy: i32) -> bool {
    if dx != 0 && dy != 0 {
        walkable_at(grid, x + dx, y + dy)
            && walkable_at(grid, x + dx, y)
            && walkable_at(grid, x, y + dy)
    } else {
        walkable_at(grid, x + dx, y + dy)
    }
}

/// Octile distance in cost units: `10*(dx+dy) - 6*min(dx,dy)`. Admissible
/// and consistent even under the no-corner-cutting restriction.
fn octile(x: i32, y: i32, gx: i32, gy: i32) -> u32 {
    let dx = (gx - x).unsigned_abs();
    let dy = (gy - y).unsigned_abs();
    10 * (dx + dy) - 6 * dx.min(dy)
}

/// Whether the straight scan through (x, y) along (dx, 0) — or (0, dy) —
/// has a forced turn at (x, y): a wall beside the PREVIOUS step opens up
/// beside (x, y). That is the classic Harabor–Grastien stop rule shifted
/// one cell along, as the strict movement model requires.
fn straight_forced(grid: &Grid, x: i32, y: i32, dx: i32, dy: i32) -> bool {
    debug_assert!(dx == 0 || dy == 0);
    if dy == 0 {
        (!walkable_at(grid, x - dx, y + 1) && walkable_at(grid, x, y + 1))
            || (!walkable_at(grid, x - dx, y - 1) && walkable_at(grid, x, y - 1))
    } else {
        (!walkable_at(grid, x + 1, y - dy) && walkable_at(grid, x + 1, y))
            || (!walkable_at(grid, x - 1, y - dy) && walkable_at(grid, x - 1, y))
    }
}

/// Scan from (x, y) in direction (dx, dy), with g-cost `g` at (x, y).
/// Returns the first jump point — or the goal — with its accumulated cost,
/// or `None` when the ray is blocked before anything interesting. Straight
/// scans loop; a diagonal scan stops where either straight sub-scan stops.
fn jump(
    grid: &Grid,
    goal: CellXY,
    x: i32,
    y: i32,
    dx: i32,
    dy: i32,
    g: u32,
) -> Option<(i32, i32, u32)> {
    if !step_legal(grid, x, y, dx, dy) {
        return None;
    }
    let (gx, gy) = (i32::from(goal.x), i32::from(goal.y));
    let mut cx = x + dx;
    let mut cy = y + dy;
    let mut cg = g + if dx != 0 && dy != 0 {
        COST_DIAG
    } else {
        COST_STRAIGHT
    };
    if dx != 0 && dy != 0 {
        loop {
            if (cx, cy) == (gx, gy) {
                return Some((cx, cy, cg));
            }
            if jump(grid, goal, cx, cy, dx, 0, cg).is_some()
                || jump(grid, goal, cx, cy, 0, dy, cg).is_some()
            {
                return Some((cx, cy, cg));
            }
            if !step_legal(grid, cx, cy, dx, dy) {
                return None;
            }
            cx += dx;
            cy += dy;
            cg += COST_DIAG;
        }
    } else {
        loop {
            if (cx, cy) == (gx, gy) {
                return Some((cx, cy, cg));
            }
            if straight_forced(grid, cx, cy, dx, dy) {
                return Some((cx, cy, cg));
            }
            if !step_legal(grid, cx, cy, dx, dy) {
                return None;
            }
            cx += dx;
            cy += dy;
            cg += COST_STRAIGHT;
        }
    }
}

/// Fill `out` with the directions to expand at (x, y) given the incoming
/// direction (parent -> node), in a fixed order; returns how many.
fn successor_dirs(
    grid: &Grid,
    x: i32,
    y: i32,
    incoming: Option<(i32, i32)>,
    out: &mut [(i32, i32); 8],
) -> usize {
    let Some((dx, dy)) = incoming else {
        out.copy_from_slice(&DIRS);
        return 8;
    };
    let mut n = 0;
    if dx != 0 && dy != 0 {
        // Diagonal incoming: the three natural directions only.
        out[n] = (dx, 0);
        out[n + 1] = (0, dy);
        out[n + 2] = (dx, dy);
        n += 3;
    } else if dx != 0 {
        out[n] = (dx, 0);
        n += 1;
        for s in [1, -1] {
            if !walkable_at(grid, x - dx, y + s) && walkable_at(grid, x, y + s) {
                out[n] = (0, s);
                out[n + 1] = (dx, s);
                n += 2;
            }
        }
    } else {
        out[n] = (0, dy);
        n += 1;
        for s in [1, -1] {
            if !walkable_at(grid, x + s, y - dy) && walkable_at(grid, x + s, y) {
                out[n] = (s, 0);
                out[n + 1] = (s, dy);
                n += 2;
            }
        }
    }
    n
}

/// Walk the parent chain from goal to start, expanding each jump segment
/// (a straight or diagonal line) into its unit steps. The result includes
/// both endpoints, start first.
fn reconstruct(w: u16, parent: &[Option<u32>], start: usize, goal: usize) -> Vec<CellXY> {
    let mut path = vec![reconstruct_cell(w, goal)];
    let mut cur = goal;
    let (mut x, mut y) = (
        i32::from(reconstruct_cell(w, goal).x),
        i32::from(reconstruct_cell(w, goal).y),
    );
    while cur != start {
        let p = parent[cur].expect("every open node has a parent") as usize;
        let pc = reconstruct_cell(w, p);
        let (px, py) = (i32::from(pc.x), i32::from(pc.y));
        let (dx, dy) = ((px - x).signum(), (py - y).signum());
        let steps = (px - x).abs().max((py - y).abs());
        for k in 1..=steps {
            path.push(CellXY {
                x: (x + dx * k) as u16,
                y: (y + dy * k) as u16,
            });
        }
        x = px;
        y = py;
        cur = p;
    }
    path.reverse();
    path
}

/// The cheapest path from `start` to `goal` under the strict movement
/// model, expanded to unit steps and including both endpoints, or `None`
/// when no legal path exists. `start == goal` (both walkable) is `[start]`.
pub fn jps(grid: &Grid, start: CellXY, goal: CellXY) -> Option<Vec<CellXY>> {
    if !grid.walkable(start) || !grid.walkable(goal) {
        return None;
    }
    let s = grid.idx(start);
    let t = grid.idx(goal);
    if s == t {
        return Some(vec![start]);
    }
    let (sx, sy) = (i32::from(start.x), i32::from(start.y));
    let (gx, gy) = (i32::from(goal.x), i32::from(goal.y));

    let mut g = vec![u32::MAX; grid.cells.len()];
    let mut parent = vec![None; grid.cells.len()];
    let mut closed = vec![false; grid.cells.len()];
    let mut heap: BinaryHeap<Reverse<(u32, u32, u16, u16)>> = BinaryHeap::new();

    let h0 = octile(sx, sy, gx, gy);
    g[s] = 0;
    heap.push(Reverse((h0, h0, start.y, start.x)));

    let mut dirs = [(0, 0); 8];
    while let Some(Reverse((_, _, py, px))) = heap.pop() {
        let ci = usize::from(py) * usize::from(grid.w) + usize::from(px);
        if closed[ci] {
            continue;
        }
        closed[ci] = true;
        if ci == t {
            return Some(reconstruct(grid.w, &parent, s, t));
        }
        let (cx, cy) = (i32::from(px), i32::from(py));
        let incoming = parent[ci].map(|p| {
            let pc = reconstruct_cell(grid.w, p as usize);
            (
                (cx - i32::from(pc.x)).signum(),
                (cy - i32::from(pc.y)).signum(),
            )
        });
        let n = successor_dirs(grid, cx, cy, incoming, &mut dirs);
        for &(dx, dy) in &dirs[..n] {
            if let Some((jx, jy, jg)) = jump(grid, goal, cx, cy, dx, dy, g[ci]) {
                let ji = usize::from(jy as u16) * usize::from(grid.w) + usize::from(jx as u16);
                if jg < g[ji] {
                    g[ji] = jg;
                    parent[ji] = Some(ci as u32);
                    let jh = octile(jx, jy, gx, gy);
                    heap.push(Reverse((jg + jh, jh, jy as u16, jx as u16)));
                }
            }
        }
    }
    None
}

/// The cell at row-major index `i` on a grid `w` cells wide.
fn reconstruct_cell(w: u16, i: usize) -> CellXY {
    CellXY {
        x: ((i as u32) % u32::from(w)) as u16,
        y: ((i as u32) / u32::from(w)) as u16,
    }
}

/// The cost of an adjacent-cell path: 10 per orthogonal step, 14 per
/// diagonal. Paths of fewer than two cells cost 0.
pub fn path_cost(path: &[CellXY]) -> u32 {
    let mut total = 0;
    for step in path.windows(2) {
        let dx = step[1].x.abs_diff(step[0].x);
        let dy = step[1].y.abs_diff(step[0].y);
        debug_assert!(
            (dx == 0 && dy == 1) || (dx == 1 && dy == 0) || (dx == 1 && dy == 1),
            "path_cost expects unit steps, got ({dx},{dy})"
        );
        total += if dx != 0 && dy != 0 {
            COST_DIAG
        } else {
            COST_STRAIGHT
        };
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::Params;
    use crate::rng::{Domain, Rng};
    use crate::world::gen::generate;

    /// Plain A* reference: the same movement model, costs, heuristic and
    /// (f, h, y, x) tie-break as `jps`, but expanding single unit steps.
    /// Returns the optimal cost only; `jps` must agree with it exactly.
    fn astar_cost(grid: &Grid, start: CellXY, goal: CellXY) -> Option<u32> {
        if !grid.walkable(start) || !grid.walkable(goal) {
            return None;
        }
        let s = grid.idx(start);
        let t = grid.idx(goal);
        if s == t {
            return Some(0);
        }
        let mut g = vec![u32::MAX; grid.cells.len()];
        let mut closed = vec![false; grid.cells.len()];
        let mut heap: BinaryHeap<Reverse<(u32, u32, u16, u16)>> = BinaryHeap::new();
        let (gx, gy) = (i32::from(goal.x), i32::from(goal.y));
        let h0 = octile(i32::from(start.x), i32::from(start.y), gx, gy);
        g[s] = 0;
        heap.push(Reverse((h0, h0, start.y, start.x)));
        while let Some(Reverse((_, _, y, x))) = heap.pop() {
            let ci = usize::from(y) * usize::from(grid.w) + usize::from(x);
            if closed[ci] {
                continue;
            }
            closed[ci] = true;
            if ci == t {
                return Some(g[ci]);
            }
            for &(dx, dy) in &DIRS {
                if !step_legal(grid, i32::from(x), i32::from(y), dx, dy) {
                    continue;
                }
                let (nx, ny) = (i32::from(x) + dx, i32::from(y) + dy);
                let ni = usize::from(ny as u16) * usize::from(grid.w) + usize::from(nx as u16);
                let ng = g[ci]
                    + if dx != 0 && dy != 0 {
                        COST_DIAG
                    } else {
                        COST_STRAIGHT
                    };
                if ng < g[ni] {
                    g[ni] = ng;
                    let nh = octile(nx, ny, gx, gy);
                    heap.push(Reverse((ng + nh, nh, ny as u16, nx as u16)));
                }
            }
        }
        None
    }

    /// A random walkable cell from `rng` (rejection sampling; three quarters
    /// of a 25%-wall map is walkable, so this settles within a few draws).
    fn draw_walkable(grid: &Grid, rng: &mut Rng) -> CellXY {
        for _ in 0..10_000 {
            let c = CellXY {
                x: rng.below(u32::from(grid.w)) as u16,
                y: rng.below(u32::from(grid.h)) as u16,
            };
            if grid.walkable(c) {
                return c;
            }
        }
        panic!("no walkable cell in 10000 draws");
    }

    /// A random distinct walkable (start, goal) pair from `rng`.
    fn pick_pair(grid: &Grid, rng: &mut Rng) -> (CellXY, CellXY) {
        let start = draw_walkable(grid, rng);
        for _ in 0..10_000 {
            let goal = draw_walkable(grid, rng);
            if goal != start {
                return (start, goal);
            }
        }
        panic!("no distinct walkable pair in 10000 draws");
    }

    /// `path` runs from `start` to `goal` and every step is a legal unit
    /// move (no corner cutting).
    fn path_legal(grid: &Grid, start: CellXY, goal: CellXY, path: &[CellXY]) -> bool {
        path.first() == Some(&start)
            && path.last() == Some(&goal)
            && path.windows(2).all(|w| {
                let dx = i32::from(w[1].x) - i32::from(w[0].x);
                let dy = i32::from(w[1].y) - i32::from(w[0].y);
                dx.abs() <= 1
                    && dy.abs() <= 1
                    && (dx != 0 || dy != 0)
                    && step_legal(grid, i32::from(w[0].x), i32::from(w[0].y), dx, dy)
            })
    }

    #[test]
    fn jps_matches_astar_cost() {
        for seed in 0..200u64 {
            let mut rng = Rng::stream(seed, Domain::Worldgen, 0);
            let mut grid = Grid::new(64, 64);
            for y in 0..u32::from(grid.h) {
                for x in 0..u32::from(grid.w) {
                    if rng.below(100) < 25 {
                        grid.set(
                            CellXY {
                                x: x as u16,
                                y: y as u16,
                            },
                            CellKind::Wall,
                        );
                    }
                }
            }
            let (start, goal) = pick_pair(&grid, &mut rng);
            let jp = jps(&grid, start, goal);
            let ac = astar_cost(&grid, start, goal);
            match (jp, ac) {
                (Some(path), Some(cost)) => {
                    assert_eq!(path_cost(&path), cost, "seed {seed}");
                    assert!(
                        path_legal(&grid, start, goal, &path),
                        "seed {seed}: illegal path {path:?}"
                    );
                }
                (None, None) => {}
                other => panic!("seed {seed}: jps and astar disagree: {other:?}"),
            }
        }
    }

    #[test]
    fn jps_no_corner_cutting() {
        // Walls east and north of (0,0): the diagonal (0,0)->(1,1) cuts
        // between two walls and must never appear in a path. From the
        // boxed-in (0,0) nothing is reachable at all — a corner-cutting
        // pathfinder would return the diagonal.
        let mut grid = Grid::new(3, 3);
        grid.set(CellXY { x: 1, y: 0 }, CellKind::Wall);
        grid.set(CellXY { x: 0, y: 1 }, CellKind::Wall);
        let origin = CellXY { x: 0, y: 0 };
        for goal in [
            CellXY { x: 1, y: 1 },
            CellXY { x: 2, y: 0 },
            CellXY { x: 2, y: 2 },
        ] {
            // Any path to these goals would have to open with the diagonal
            // (0,0)->(1,1), so `None` here is exactly the "no such step"
            // assertion.
            assert!(
                jps(&grid, origin, goal).is_none(),
                "a path from {origin:?} to {goal:?} exists only by cutting the corner"
            );
        }
        // The same corner from the other side is also refused.
        assert!(jps(&grid, CellXY { x: 1, y: 1 }, origin).is_none());
        // A diagonal whose orthogonal cells are open is still taken.
        assert_eq!(
            jps(&grid, CellXY { x: 2, y: 2 }, CellXY { x: 1, y: 1 }),
            Some(vec![CellXY { x: 2, y: 2 }, CellXY { x: 1, y: 1 }])
        );
        // Cover blocks sight, not movement: a Floor-Cover-Floor corridor
        // paths straight through.
        let mut corridor = Grid::new(3, 1);
        corridor.set(CellXY { x: 1, y: 0 }, CellKind::Cover);
        assert_eq!(
            jps(&corridor, CellXY { x: 0, y: 0 }, CellXY { x: 2, y: 0 }),
            Some(vec![
                CellXY { x: 0, y: 0 },
                CellXY { x: 1, y: 0 },
                CellXY { x: 2, y: 0 }
            ])
        );
    }

    #[test]
    fn jps_deterministic_tiebreak() {
        // An open 20x20: many equal-cost paths exist, so the returned one
        // pins the (f, h, y, x) tie-break. The recorded path is this
        // implementation's output, pinned as a regression constant.
        let grid = Grid::new(20, 20);
        let (start, goal) = (CellXY { x: 0, y: 0 }, CellXY { x: 19, y: 13 });
        let a = jps(&grid, start, goal);
        let b = jps(&grid, start, goal);
        let c = jps(&grid, start, goal);
        assert_eq!(a, b);
        assert_eq!(b, c);
        // Recorded: 13 diagonals to (13,13), then straight east to the goal.
        assert_eq!(
            a,
            Some(vec![
                CellXY { x: 0, y: 0 },
                CellXY { x: 1, y: 1 },
                CellXY { x: 2, y: 2 },
                CellXY { x: 3, y: 3 },
                CellXY { x: 4, y: 4 },
                CellXY { x: 5, y: 5 },
                CellXY { x: 6, y: 6 },
                CellXY { x: 7, y: 7 },
                CellXY { x: 8, y: 8 },
                CellXY { x: 9, y: 9 },
                CellXY { x: 10, y: 10 },
                CellXY { x: 11, y: 11 },
                CellXY { x: 12, y: 12 },
                CellXY { x: 13, y: 13 },
                CellXY { x: 14, y: 13 },
                CellXY { x: 15, y: 13 },
                CellXY { x: 16, y: 13 },
                CellXY { x: 17, y: 13 },
                CellXY { x: 18, y: 13 },
                CellXY { x: 19, y: 13 },
            ])
        );
        // Grounding: on open ground the optimal cost is 13 diagonals plus
        // 6 straight steps, whatever the tie-break.
        assert_eq!(a.as_ref().map(|p| path_cost(p)), Some(14 * 13 + 10 * 6));
    }

    /// Reachability sanity used by neither plan test but keeping `generate`
    /// in scope honest: worldgen maps are JPS-reachable spawn to beacon.
    #[test]
    fn jps_worldgen_spawn_reaches_beacons() {
        let world = generate(3, &Params::default());
        for b in &world.beacons {
            assert!(
                jps(&world.grid, world.spawns[0], *b).is_some(),
                "beacon {b:?} unreachable from spawn"
            );
        }
    }
}
