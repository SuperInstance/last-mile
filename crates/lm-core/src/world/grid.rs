//! The coarse navigation grid the world is grown on.
//!
//! One cell is `cell_m` metres (⚙ default 2) on a side, held in a flat
//! row-major array. Walls block movement and sight; cover blocks sight
//! only. Everything is plain ordered data, so a world serializes and
//! hashes canonically on every machine.

use crate::fixed::Pos;
use crate::hash::{Fnv64, StateHash};

/// What occupies a cell of the grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CellKind {
    /// Open ground: blocks nothing.
    Floor,
    /// Full-height wall: blocks movement and sight.
    Wall,
    /// Low cover: blocks sight only, and can be walked through.
    Cover,
}

impl CellKind {
    /// Canonical discriminant used in hashes (declaration order).
    pub const fn tag(self) -> u8 {
        match self {
            CellKind::Floor => 0,
            CellKind::Wall => 1,
            CellKind::Cover => 2,
        }
    }

    /// Whether this cell stops a line of sight.
    pub const fn blocks_sight(self) -> bool {
        matches!(self, CellKind::Wall | CellKind::Cover)
    }

    /// Whether this cell stops movement.
    pub const fn blocks_move(self) -> bool {
        matches!(self, CellKind::Wall)
    }
}

impl StateHash for CellKind {
    fn hash_into(&self, h: &mut Fnv64) {
        h.write(&[self.tag()]);
    }
}

/// A cell coordinate on the grid: `x` east, `y` south.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct CellXY {
    pub x: u16,
    pub y: u16,
}

impl StateHash for CellXY {
    fn hash_into(&self, h: &mut Fnv64) {
        h.u16(self.x);
        h.u16(self.y);
    }
}

/// The grid itself: `w × h` cells in row-major order.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Grid {
    pub w: u16,
    pub h: u16,
    pub cells: Vec<CellKind>,
}

impl Grid {
    /// An all-Floor grid of `w × h` cells.
    pub fn new(w: u16, h: u16) -> Grid {
        Grid {
            w,
            h,
            cells: vec![CellKind::Floor; usize::from(w) * usize::from(h)],
        }
    }

    /// Whether `(x, y)` lies inside the grid.
    pub fn in_bounds(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < i32::from(self.w) && y < i32::from(self.h)
    }

    /// Row-major index of an in-bounds cell.
    pub(crate) fn idx(&self, c: CellXY) -> usize {
        usize::from(c.y) * usize::from(self.w) + usize::from(c.x)
    }

    /// The cell's kind. Out-of-bounds cells read as `Wall` — the map edge
    /// blocks everything — so callers never need a bounds branch.
    pub fn get(&self, c: CellXY) -> CellKind {
        if self.in_bounds(i32::from(c.x), i32::from(c.y)) {
            self.cells[self.idx(c)]
        } else {
            CellKind::Wall
        }
    }

    /// Set an in-bounds cell; out-of-bounds writes are ignored.
    pub(crate) fn set(&mut self, c: CellXY, k: CellKind) {
        if self.in_bounds(i32::from(c.x), i32::from(c.y)) {
            let i = self.idx(c);
            self.cells[i] = k;
        }
    }

    /// Whether an agent may enter the cell (cover blocks sight only).
    pub fn walkable(&self, c: CellXY) -> bool {
        self.get(c) != CellKind::Wall
    }

    /// The 180° rotation of an in-bounds cell about the grid centre.
    pub fn mirror(&self, c: CellXY) -> CellXY {
        if !self.in_bounds(i32::from(c.x), i32::from(c.y)) {
            return c;
        }
        CellXY {
            x: self.w - 1 - c.x,
            y: self.h - 1 - c.y,
        }
    }

    /// The cell containing the position `p`, with `cell_units` fixed-point
    /// units per cell edge. Positions outside the grid saturate to the
    /// border cell.
    pub fn cell_of(&self, p: Pos, cell_units: i32) -> CellXY {
        let cu = i64::from(cell_units.max(1));
        let (ux, uy) = p.to_units();
        let x = ux.div_euclid(cu).clamp(0, (i64::from(self.w) - 1).max(0));
        let y = uy.div_euclid(cu).clamp(0, (i64::from(self.h) - 1).max(0));
        CellXY {
            x: x as u16,
            y: y as u16,
        }
    }

    /// The position at the centre of cell `c`.
    pub fn center(&self, c: CellXY, cell_units: i32) -> Pos {
        let cu = i64::from(cell_units.max(1));
        Pos::from_units(i64::from(c.x) * cu + cu / 2, i64::from(c.y) * cu + cu / 2)
    }
}

impl StateHash for Grid {
    fn hash_into(&self, h: &mut Fnv64) {
        h.u16(self.w);
        h.u16(self.h);
        for k in &self.cells {
            k.hash_into(h);
        }
    }
}
