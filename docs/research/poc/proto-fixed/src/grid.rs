//! Seeded RNG, wall-map grid, and shared search plumbing.
//! Movement model everywhere: 8-connected, strict no-corner-cutting
//! (a diagonal step requires BOTH orthogonal cells walkable).
//! Step costs: straight 10, diagonal 14.

/// xorshift64* — state must be nonzero (enforced at construction).
pub struct Xorshift64Star {
    state: u64,
}

impl Xorshift64Star {
    pub fn new(seed: u64) -> Self {
        assert!(seed != 0, "xorshift64* state must be nonzero");
        Xorshift64Star { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform-ish draw below n (slight modulo bias is fine for a prototype).
    pub fn next_below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }
}

pub struct GridMap {
    pub w: i32,
    pub h: i32,
    pub walls: Vec<bool>,
}

impl GridMap {
    pub fn walkable(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.w && y < self.h && !self.walls[(y * self.w + x) as usize]
    }

    /// Legality of a unit step from (x, y) along (dx, dy) under the strict
    /// no-corner-cutting rule.
    pub fn step_legal(&self, x: i32, y: i32, dx: i32, dy: i32) -> bool {
        if dx != 0 && dy != 0 {
            self.walkable(x + dx, y + dy) && self.walkable(x + dx, y) && self.walkable(x, y + dy)
        } else {
            self.walkable(x + dx, y + dy)
        }
    }

    /// Deterministic map from a nonzero seed; returns the map and the RNG
    /// stream positioned right after map generation (for start/goal picks).
    pub fn generate(seed: u64, wall_pct: u64, w: i32, h: i32) -> (GridMap, Xorshift64Star) {
        let mut rng = Xorshift64Star::new(seed);
        let mut walls = Vec::with_capacity((w * h) as usize);
        for _ in 0..(w * h) {
            walls.push(rng.next_below(100) < wall_pct);
        }
        (GridMap { w, h, walls }, rng)
    }

    pub fn from_walls(w: i32, h: i32, walls: Vec<bool>) -> Self {
        assert_eq!(walls.len(), (w * h) as usize);
        GridMap { w, h, walls }
    }

    /// ASCII dump used in test failure messages.
    pub fn dump(&self, s: (i32, i32), g: (i32, i32)) -> String {
        let mut out = String::new();
        for y in 0..self.h {
            for x in 0..self.w {
                let c = if (x, y) == s {
                    'S'
                } else if (x, y) == g {
                    'G'
                } else if self.walls[(y * self.w + x) as usize] {
                    '#'
                } else {
                    '.'
                };
                out.push(c);
            }
            out.push('\n');
        }
        out
    }
}

/// Pick a start/goal pair of walkable cells, preferring Chebyshev distance
/// >= min_cheb; relaxes after many attempts. Deterministic per RNG stream.
pub fn pick_pair(map: &GridMap, rng: &mut Xorshift64Star, min_cheb: i32) -> ((i32, i32), (i32, i32)) {
    let mut best: Option<((i32, i32), (i32, i32), i32)> = None;
    for attempt in 0..4_000 {
        let s = (rng.next_below(map.w as u64) as i32, rng.next_below(map.h as u64) as i32);
        let g = (rng.next_below(map.w as u64) as i32, rng.next_below(map.h as u64) as i32);
        if !map.walkable(s.0, s.1) || !map.walkable(g.0, g.1) || s == g {
            continue;
        }
        let d = (s.0 - g.0).abs().max((s.1 - g.1).abs());
        if d >= min_cheb {
            return (s, g);
        }
        if best.map_or(true, |(_, _, bd)| d > bd) {
            best = Some((s, g, d));
        }
        if attempt == 3_999 {
            break;
        }
    }
    best.map(|(s, g, _)| (s, g)).expect("no walkable start/goal pair found")
}

/// All 8 directions.
pub const DIRS: [(i32, i32); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];

/// Octile heuristic in cost units (10 straight / 14 diagonal): admissible and
/// consistent even under the no-corner-cutting restriction.
pub fn octile_h(x: i32, y: i32, gx: i32, gy: i32) -> u32 {
    let dx = (gx - x).unsigned_abs();
    let dy = (gy - y).unsigned_abs();
    if dx >= dy {
        10 * dx + 4 * dy
    } else {
        10 * dy + 4 * dx
    }
}

#[derive(Debug, PartialEq)]
pub struct SearchResult {
    pub cost: u32,
    pub expansions: u64,
    pub path: Vec<(i32, i32)>,
}

/// Rebuild the cell path from parent indices. Every parent hop lies on a
/// straight or diagonal line (true for both A* unit steps and JPS jump lines).
pub fn reconstruct_path(w: i32, parent: &[u32], start: usize, goal: usize) -> Vec<(i32, i32)> {
    let cell = |i: usize| ((i as i32) % w, (i as i32) / w);
    let mut path = Vec::new();
    let mut cur = goal;
    let (mut x, mut y) = cell(goal);
    path.push((x, y));
    while cur != start {
        let p = parent[cur] as usize;
        let (px, py) = cell(p);
        let dx = (px - x).signum();
        let dy = (py - y).signum();
        let steps = (px - x).abs().max((py - y).abs());
        for k in 1..=steps {
            path.push((x + dx * k, y + dy * k));
        }
        x = px;
        y = py;
        cur = p;
    }
    path.reverse();
    path
}

/// Recompute path cost from the cells, rejecting any illegal step.
/// Returns None if a step is not a legal 8-connected no-corner-cut move.
pub fn path_cost(map: &GridMap, path: &[(i32, i32)]) -> Option<u32> {
    if path.is_empty() {
        return None;
    }
    let mut total = 0u32;
    for w in path.windows(2) {
        let dx = w[1].0 - w[0].0;
        let dy = w[1].1 - w[0].1;
        if !((-1..=1).contains(&dx) && (-1..=1).contains(&dy) && (dx != 0 || dy != 0)) {
            return None;
        }
        if !map.step_legal(w[0].0, w[0].1, dx, dy) {
            return None;
        }
        total += if dx != 0 && dy != 0 { 14 } else { 10 };
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xorshift_stream_is_deterministic_and_nonzero() {
        let mut a = Xorshift64Star::new(42);
        let mut b = Xorshift64Star::new(42);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        let mut r = Xorshift64Star::new(7);
        assert_ne!(r.next_u64(), 0);
    }

    #[test]
    fn map_generation_is_seeded() {
        let (m1, _) = GridMap::generate(0xAAAA_0000_0000_0001, 25, 64, 64);
        let (m2, _) = GridMap::generate(0xAAAA_0000_0000_0001, 25, 64, 64);
        assert_eq!(m1.walls, m2.walls);
        let (m3, _) = GridMap::generate(0xAAAA_0000_0000_0002, 25, 64, 64);
        assert_ne!(m1.walls, m3.walls);
        let walls = m1.walls.iter().filter(|&&w| w).count();
        let pct = walls * 100 / m1.walls.len();
        assert!((10..=40).contains(&pct), "wall density off: {pct}%");
    }

    #[test]
    fn no_corner_cutting_enforced() {
        // walls east and north of (0,0): the diagonal to (1,1) is illegal
        let m = GridMap::from_walls(3, 3, vec![
            false, true, false,
            true, false, false,
            false, false, false,
        ]);
        assert!(m.walkable(0, 0) && m.walkable(1, 1));
        assert!(!m.step_legal(0, 0, 1, 1));
        assert!(m.step_legal(0, 0, 0, 0 - 1) == false); // off-grid is blocked
        assert!(!m.step_legal(1, 1, -1, -1)); // same corner from the other side
    }
}
