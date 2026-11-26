//! Jump Point Search on the same strict no-corner-cutting 8-connected grid
//! as `astar`, so both are optimal and their costs must match exactly.
//!
//! Pruning is adapted to the strict movement model (classic JPS assumes a
//! diagonal step only needs its target walkable; here it needs BOTH orthogonal
//! cells walkable, which moves every "forced" turn one cell further along):
//!
//! * Straight scan in direction (dx,0) stops at cell c iff some side s has the
//!   cell beside the PREVIOUS step walled and the cell beside c open
//!   (`!walk(c-(dx,0)+(0,s)) && walk(c+(0,s))`). That is the classic rule
//!   shifted by one cell: under strict rules the legal turn around such a wall
//!   happens at c, not at the cell before it.
//! * A node with straight incoming direction expands the natural straight
//!   direction plus, for each forced side s, the straight aside (0,s) and the
//!   diagonal (dx,s) — both legal from c exactly when the side is open.
//! * A node with diagonal incoming direction expands the three natural
//!   directions only. The classic forced diagonals are illegal as direct
//!   steps under strict rules; the detour they represent is found by the
//!   straight natural jumps, which stop via the shifted rule above.
//! * Diagonal scans stop where a straight sub-scan finds a stop (classic).

use crate::grid::{octile_h, reconstruct_path, GridMap, SearchResult, DIRS};
use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// Does cell (x, y), while scanning straight along (dx,0) or (0,dy), have a
/// forced turn around a wall beside the previous cell?
fn straight_forced(map: &GridMap, x: i32, y: i32, dx: i32, dy: i32) -> bool {
    debug_assert!(dx == 0 || dy == 0);
    if dy == 0 {
        (!map.walkable(x - dx, y + 1) && map.walkable(x, y + 1))
            || (!map.walkable(x - dx, y - 1) && map.walkable(x, y - 1))
    } else {
        (!map.walkable(x + 1, y - dy) && map.walkable(x + 1, y))
            || (!map.walkable(x - 1, y - dy) && map.walkable(x - 1, y))
    }
}

/// Scan from (x, y) in direction (dx, dy) with g-cost `g` at (x, y).
/// Returns the first jump point (or the goal) with its accumulated g-cost,
/// or None if the ray is blocked before anything interesting.
fn jump(map: &GridMap, goal: (i32, i32), x: i32, y: i32, dx: i32, dy: i32, g: u32) -> Option<(i32, i32, u32)> {
    if !map.step_legal(x, y, dx, dy) {
        return None;
    }
    let mut cx = x + dx;
    let mut cy = y + dy;
    let mut cg = g + if dx != 0 && dy != 0 { 14 } else { 10 };
    if dx != 0 && dy != 0 {
        loop {
            if (cx, cy) == goal {
                return Some((cx, cy, cg));
            }
            // classic rule: a stop on either straight sub-ray makes this a jump point
            if jump(map, goal, cx, cy, dx, 0, cg).is_some()
                || jump(map, goal, cx, cy, 0, dy, cg).is_some()
            {
                return Some((cx, cy, cg));
            }
            if !map.step_legal(cx, cy, dx, dy) {
                return None;
            }
            cx += dx;
            cy += dy;
            cg += 14;
        }
    } else {
        loop {
            if (cx, cy) == goal {
                return Some((cx, cy, cg));
            }
            if straight_forced(map, cx, cy, dx, dy) {
                return Some((cx, cy, cg));
            }
            if !map.step_legal(cx, cy, dx, dy) {
                return None;
            }
            cx += dx;
            cy += dy;
            cg += 10;
        }
    }
}

/// Direction set to expand at a node reached with the given incoming direction.
fn successor_dirs(map: &GridMap, x: i32, y: i32, incoming: Option<(i32, i32)>, out: &mut [(i32, i32); 8]) -> usize {
    let Some((dx, dy)) = incoming else {
        out[..8].copy_from_slice(&DIRS);
        return 8;
    };
    let mut n = 0;
    if dx != 0 && dy != 0 {
        out[n] = (dx, 0);
        n += 1;
        out[n] = (0, dy);
        n += 1;
        out[n] = (dx, dy);
        n += 1;
    } else if dx != 0 {
        out[n] = (dx, 0);
        n += 1;
        for s in [1i32, -1] {
            if !map.walkable(x - dx, y + s) && map.walkable(x, y + s) {
                out[n] = (0, s);
                n += 1;
                out[n] = (dx, s);
                n += 1;
            }
        }
    } else {
        out[n] = (0, dy);
        n += 1;
        for s in [1i32, -1] {
            if !map.walkable(x + s, y - dy) && map.walkable(x + s, y) {
                out[n] = (s, 0);
                n += 1;
                out[n] = (s, dy);
                n += 1;
            }
        }
    }
    n
}

pub fn jps(map: &GridMap, start: (i32, i32), goal: (i32, i32)) -> Option<SearchResult> {
    if !map.walkable(start.0, start.1) || !map.walkable(goal.0, goal.1) {
        return None;
    }
    let n = (map.w * map.h) as usize;
    let idx = |x: i32, y: i32| (y * map.w + x) as usize;
    let s = idx(start.0, start.1);
    let t = idx(goal.0, goal.1);
    if s == t {
        return Some(SearchResult { cost: 0, expansions: 0, path: vec![start] });
    }

    let mut g = vec![u32::MAX; n];
    let mut parent = vec![u32::MAX; n];
    let mut closed = vec![false; n];
    let mut heap = BinaryHeap::new();

    g[s] = 0;
    heap.push(Reverse((octile_h(start.0, start.1, goal.0, goal.1), Reverse(0u32), s as u32)));

    let mut expansions = 0u64;
    let mut dirs = [(0i32, 0i32); 8];
    while let Some(Reverse((_, _, ci))) = heap.pop() {
        let ci = ci as usize;
        if closed[ci] {
            continue;
        }
        closed[ci] = true;
        expansions += 1;
        if ci == t {
            let path = reconstruct_path(map.w, &parent, s, t);
            return Some(SearchResult { cost: g[ci], expansions, path });
        }
        let cx = (ci as i32) % map.w;
        let cy = (ci as i32) / map.w;
        let incoming = if parent[ci] == u32::MAX {
            None
        } else {
            let p = parent[ci] as usize;
            let px = (p as i32) % map.w;
            let py = (p as i32) / map.w;
            Some(((cx - px).signum(), (cy - py).signum()))
        };
        let nd = successor_dirs(map, cx, cy, incoming, &mut dirs);
        for &(dx, dy) in &dirs[..nd] {
            if let Some((jx, jy, jg)) = jump(map, goal, cx, cy, dx, dy, g[ci]) {
                let ji = idx(jx, jy);
                if jg < g[ji] {
                    g[ji] = jg;
                    parent[ji] = ci as u32;
                    heap.push(Reverse((jg + octile_h(jx, jy, goal.0, goal.1), Reverse(jg), ji as u32)));
                }
            }
        }
    }
    None
}
