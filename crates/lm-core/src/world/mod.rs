//! The world a round is played on: the grid plus the pinned, symmetric
//! beacon and spawn cells, and the paint the round leaves on them.

pub mod gen;
pub mod grid;
pub mod los;
pub mod paint;
pub mod path;

pub use gen::generate;
pub use grid::{CellKind, CellXY, Grid};
pub use los::{clear, walls_between};
pub use paint::{dry_step_q8, splat_cells, Paint, PaintCell};
pub use path::{jps, path_cost};

use crate::hash::{Fnv64, StateHash};

/// A generated world: the grid, the beacons and the spawns, all grown from
/// the round seed, plus the fixed-point units per cell edge.
///
/// The paint is part of the world: a tag writes a splat into it, the paint
/// dries on its own, and the whole layer is hashed with the world — so a
/// painted round is a different round, and a replay of it is bit-exact.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct World {
    pub grid: Grid,
    pub beacons: Vec<CellXY>,
    pub spawns: Vec<CellXY>,
    pub cell_units: i32,
    /// The paint on the ground: what the tags of this round have left.
    pub paint: Paint,
}

impl StateHash for World {
    fn hash_into(&self, h: &mut Fnv64) {
        self.grid.hash_into(h);
        h.u32(self.beacons.len() as u32);
        for b in &self.beacons {
            b.hash_into(h);
        }
        h.u32(self.spawns.len() as u32);
        for s in &self.spawns {
            s.hash_into(h);
        }
        h.i32(self.cell_units);
        self.paint.hash_into(h);
    }
}
