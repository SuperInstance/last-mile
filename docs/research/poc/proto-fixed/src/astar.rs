//! Plain A* baseline: 8-connected, no corner cutting, octile heuristic.
//! Deterministic tie-breaking: lowest f, then highest g, then lowest index.

use crate::grid::{octile_h, reconstruct_path, GridMap, SearchResult, DIRS};
use std::cmp::Reverse;
use std::collections::BinaryHeap;

pub fn astar(map: &GridMap, start: (i32, i32), goal: (i32, i32)) -> Option<SearchResult> {
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
        for &(dx, dy) in &DIRS {
            if !map.step_legal(cx, cy, dx, dy) {
                continue;
            }
            let nx = cx + dx;
            let ny = cy + dy;
            let ni = idx(nx, ny);
            let ng = g[ci] + if dx != 0 && dy != 0 { 14 } else { 10 };
            if ng < g[ni] {
                g[ni] = ng;
                parent[ni] = ci as u32;
                heap.push(Reverse((ng + octile_h(nx, ny, goal.0, goal.1), Reverse(ng), ni as u32)));
            }
        }
    }
    None
}
