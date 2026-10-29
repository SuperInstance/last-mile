//! Bresenham line of sight on the coarse grid.
//!
//! Sight runs along the classic integer Bresenham cell walk, always drawn
//! from the lexicographically smaller endpoint so the answer is a function
//! of the unordered pair of cells alone (`clear(a, b) == clear(b, a)`).
//! Walls and cover block; the two endpoints are exempt, so an agent
//! standing in cover can still see and be seen through it.

use super::grid::{CellXY, Grid};

/// Whether the straight line from `a` to `b` is unobstructed.
pub fn clear(grid: &Grid, a: CellXY, b: CellXY) -> bool {
    // Canonical direction: the cell sequence — and so the answer — is a
    // function of the unordered pair {a, b} only.
    let (p, q) = if (a.x, a.y) <= (b.x, b.y) {
        (a, b)
    } else {
        (b, a)
    };
    let line = line_cells(p, q);
    // Interior cells only; the endpoints are exempt.
    line.iter()
        .skip(1)
        .take(line.len().saturating_sub(2))
        .all(|c| !grid.get(*c).blocks_sight())
}

/// The Bresenham cell walk from `p` to `q`, both endpoints included.
fn line_cells(p: CellXY, q: CellXY) -> Vec<CellXY> {
    let (x0, y0) = (i32::from(p.x), i32::from(p.y));
    let (x1, y1) = (i32::from(q.x), i32::from(q.y));
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    let (mut x, mut y) = (x0, y0);
    let mut cells = Vec::with_capacity((dx - dy + 1) as usize);
    loop {
        cells.push(CellXY {
            x: x as u16,
            y: y as u16,
        });
        if x == x1 && y == y1 {
            return cells;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::clear;
    use crate::params::Params;
    use crate::rng::{Domain, Rng};
    use crate::world::gen::generate;
    use crate::world::grid::{CellKind, CellXY, Grid};

    #[test]
    fn los_blocked_by_wall_and_cover() {
        // A 5x1 corridor: sight passes over the three interior cells.
        let mut grid = Grid::new(5, 1);
        let (a, b) = (CellXY { x: 0, y: 0 }, CellXY { x: 4, y: 0 });
        assert!(clear(&grid, a, b));
        grid.set(CellXY { x: 2, y: 0 }, CellKind::Wall);
        assert!(!clear(&grid, a, b));
        grid.set(CellXY { x: 2, y: 0 }, CellKind::Cover);
        assert!(!clear(&grid, a, b));
        // The endpoints themselves are excluded: cover at the eye or at
        // the target does not blind either end.
        grid.set(CellXY { x: 2, y: 0 }, CellKind::Floor);
        grid.set(b, CellKind::Cover);
        assert!(clear(&grid, a, b));
    }

    #[test]
    fn los_symmetric() {
        let world = generate(3, &Params::default());
        // An independent stream for drawing test pairs (worldgen itself
        // uses id 0).
        let mut rng = Rng::stream(3, Domain::Worldgen, 1);
        for _ in 0..1000 {
            let a = CellXY {
                x: rng.below(u32::from(world.grid.w)) as u16,
                y: rng.below(u32::from(world.grid.h)) as u16,
            };
            let b = CellXY {
                x: rng.below(u32::from(world.grid.w)) as u16,
                y: rng.below(u32::from(world.grid.h)) as u16,
            };
            assert_eq!(
                clear(&world.grid, a, b),
                clear(&world.grid, b, a),
                "clear must not depend on argument order: {a:?} {b:?}"
            );
        }
    }
}
