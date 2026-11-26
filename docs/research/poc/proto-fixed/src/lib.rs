//! fixproto — throwaway prototype to de-risk phase 1: fixed-point core +
//! Jump Point Search, std-only, no floats on any runtime path.

pub mod angle;
pub mod astar;
pub mod fixedmath;
pub mod fnv;
pub mod grid;
pub mod jps;
pub mod position;

pub use position::Position;

/// Golden constant for the 10,000-move trajectory hash (recorded from the
/// deterministic run; see the `golden_ten_thousand_moves` test).
pub const GOLDEN_TEN_K_MOVES: u64 = 0xc188_5dc5_4066_8e0a;

#[cfg(test)]
mod tests {
    use crate::astar::astar;
    use crate::fnv::hash_positions;
    use crate::grid::{path_cost, pick_pair, GridMap, Xorshift64Star};
    use crate::jps::jps;
    use crate::Position;

    fn compare(map: &GridMap, s: (i32, i32), g: (i32, i32)) {
        let a = astar(map, s, g);
        let j = jps(map, s, g);
        assert_eq!(
            a.is_some(),
            j.is_some(),
            "status mismatch\n{}",
            map.dump(s, g)
        );
        if let (Some(a), Some(j)) = (&a, &j) {
            assert_eq!(a.cost, j.cost, "cost mismatch A*={} JPS={}\n{}", a.cost, j.cost, map.dump(s, g));
            assert_eq!(a.path.first(), Some(&s));
            assert_eq!(a.path.last(), Some(&g));
            assert_eq!(j.path.first(), Some(&s));
            assert_eq!(j.path.last(), Some(&g));
            assert_eq!(path_cost(map, &a.path), Some(a.cost), "A* path illegal");
            assert_eq!(path_cost(map, &j.path), Some(j.cost), "JPS path illegal");
        }
    }

    // ------------------------------------------------------------------
    // Golden vector: 10,000 deterministic random moves, final state hash.
    // ------------------------------------------------------------------

    #[test]
    fn golden_ten_thousand_moves() {
        let mut rng = Xorshift64Star::new(0x5EED_1234_ABCD_0001);
        let mut pos = Position::origin();
        let mut trajectory = vec![pos];
        for _ in 0..10_000 {
            let angle = rng.next_u64() as u16;
            let speed = 64 + (rng.next_u64() % 576) as i32;
            pos = pos.step(angle, speed);
            trajectory.push(pos);
        }
        let h = hash_positions(&trajectory);
        println!("golden 10k-move trajectory hash = {h:#018x}");
        assert_eq!(h, crate::GOLDEN_TEN_K_MOVES, "golden hash drifted");
    }

    // ------------------------------------------------------------------
    // JPS vs A* on 200 seeded 64x64 maps (the brief's comparison).
    // ------------------------------------------------------------------

    #[test]
    fn jps_matches_astar_on_200_seeded_maps() {
        let (mut exp_a, mut exp_j, mut solved) = (0u64, 0u64, 0usize);
        for seed in 1..=200u64 {
            let (map, mut rng) =
                GridMap::generate(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1, 25, 64, 64);
            let (s, g) = pick_pair(&map, &mut rng, 24);
            let a = astar(&map, s, g);
            let j = jps(&map, s, g);
            assert_eq!(a.is_some(), j.is_some(), "status mismatch seed {seed}");
            if let (Some(a), Some(j)) = (&a, &j) {
                assert_eq!(a.cost, j.cost, "cost mismatch seed {seed}\n{}", map.dump(s, g));
                assert_eq!(path_cost(&map, &a.path), Some(a.cost));
                assert_eq!(path_cost(&map, &j.path), Some(j.cost));
                exp_a += a.expansions;
                exp_j += j.expansions;
                solved += 1;
            }
        }
        println!(
            "200 seeded maps: {solved} solvable; mean expansions A* = {:.1}, JPS = {:.1} (JPS/A* = {:.3})",
            exp_a as f64 / 200.0,
            exp_j as f64 / 200.0,
            exp_j as f64 / exp_a as f64
        );
        assert!(solved >= 150, "too few solvable maps ({solved}) for a meaningful comparison");
    }

    // ------------------------------------------------------------------
    // Extra differential confidence for the strict-model JPS rules.
    // ------------------------------------------------------------------

    #[test]
    fn jps_exhaustive_4x4() {
        // every wall configuration on a 4x4 grid, several start/goal combos
        let combos = [((0, 0), (3, 3)), ((0, 0), (3, 0)), ((0, 0), (0, 3)), ((1, 1), (2, 2))];
        for (s, g) in combos {
            for bits in 0u32..(1 << 14) {
                let mut walls = vec![false; 16];
                let mut b = bits;
                for y in 0..4i32 {
                    for x in 0..4i32 {
                        if (x, y) == s || (x, y) == g {
                            continue;
                        }
                        walls[(y * 4 + x) as usize] = b & 1 == 1;
                        b >>= 1;
                    }
                }
                let m = GridMap::from_walls(4, 4, walls);
                compare(&m, s, g);
            }
        }
    }

    #[test]
    fn jps_random_stress_3000() {
        let mut rng = Xorshift64Star::new(0xDEAD_BEEF_CAFE_F00D);
        for _ in 0..3_000 {
            let w = 8 + rng.next_below(32) as i32;
            let h = 8 + rng.next_below(32) as i32;
            let pct = 5 + rng.next_below(45);
            let (m, mut r2) = GridMap::generate(rng.next_u64() | 1, pct, w, h);
            let (s, g) = pick_pair(&m, &mut r2, 4);
            compare(&m, s, g);
        }
    }

    #[test]
    fn jps_adversarial_two_wide_corridor_with_offset_wall() {
        // rows 0-1 open except one wall; rows 2-3 solid. The offset wall
        // forces the strict-rule forced-diagonal successor; classic rules
        // (corner-cutting) would give an illegal shortcut here.
        let mut walls = vec![false; 7 * 4];
        for y in 2..4 {
            for x in 0..7 {
                walls[y * 7 + x] = true;
            }
        }
        walls[0 * 7 + 3] = true;
        let m = GridMap::from_walls(7, 4, walls);
        let (s, g) = ((0, 0), (6, 0));
        let a = astar(&m, s, g).expect("A* must solve");
        let j = jps(&m, s, g).expect("JPS must solve");
        // optimal: 4 straight (40) + 2 diagonals (28) = 68
        assert_eq!(a.cost, 68);
        assert_eq!(j.cost, a.cost);
        compare(&m, s, g);
    }

    #[test]
    fn jps_enclosed_start_is_unsolvable() {
        let mut walls = vec![false; 9];
        walls[1] = true; // (1,0)
        walls[3] = true; // (0,1)
        let m = GridMap::from_walls(3, 3, walls);
        assert_eq!(astar(&m, (0, 0), (2, 2)), None);
        assert_eq!(jps(&m, (0, 0), (2, 2)), None);
    }

    #[test]
    fn jps_open_grid_octile_cost() {
        let (m, _) = GridMap::generate(0x1234_5678_9ABC_DEF1, 0, 64, 64);
        let (s, g) = ((0, 0), (63, 63));
        let a = astar(&m, s, g).unwrap();
        let j = jps(&m, s, g).unwrap();
        assert_eq!(a.cost, 63 * 14);
        assert_eq!(j.cost, a.cost);
        assert!(j.expansions <= a.expansions + 1); // open grid: near-straight diagonal chain
    }

    // ------------------------------------------------------------------
    // Per-op timings (printed; run with -- --nocapture to see them).
    // ------------------------------------------------------------------

    #[test]
    fn perf_step_and_searches() {
        use std::hint::black_box;
        use std::time::Instant;

        // movement step
        let mut rng = Xorshift64Star::new(0xC0FF_EE12_3456_7890);
        let n = 1_000_000usize;
        let angles: Vec<u16> = (0..n).map(|_| rng.next_u64() as u16).collect();
        let speeds: Vec<i32> = (0..n).map(|_| 64 + (rng.next_u64() % 576) as i32).collect();
        let mut pos = Position::origin();
        let t0 = Instant::now();
        for i in 0..n {
            pos = pos.step(angles[i], speeds[i]);
        }
        let step_dt = t0.elapsed();
        black_box(pos);

        // searches over the same 200 seeded maps
        let mut maps = Vec::new();
        for seed in 1..=200u64 {
            let (m, mut r) = GridMap::generate(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1, 25, 64, 64);
            let (s, g) = pick_pair(&m, &mut r, 24);
            maps.push((m, s, g));
        }
        let t1 = Instant::now();
        let mut ea = 0u64;
        for (m, s, g) in &maps {
            if let Some(r) = astar(m, *s, *g) {
                ea += r.expansions;
            }
        }
        let astar_dt = t1.elapsed();
        let t2 = Instant::now();
        let mut ej = 0u64;
        for (m, s, g) in &maps {
            if let Some(r) = jps(m, *s, *g) {
                ej += r.expansions;
            }
        }
        let jps_dt = t2.elapsed();

        println!("step:     {n} moves in {step_dt:?} = {:.1} ns/op", step_dt.as_nanos() as f64 / n as f64);
        println!(
            "A*:       200 maps in {astar_dt:?} = {:.1} us/map, mean expansions {:.1}",
            astar_dt.as_secs_f64() * 5e3,
            ea as f64 / 200.0
        );
        println!(
            "JPS:      200 maps in {jps_dt:?} = {:.1} us/map, mean expansions {:.1}",
            jps_dt.as_secs_f64() * 5e3,
            ej as f64 / 200.0
        );
    }
}
