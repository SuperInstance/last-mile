//! The world a round is played on: the grid plus the pinned, symmetric
//! beacon and spawn cells.

pub mod gen;
pub mod grid;

pub use gen::generate;
pub use grid::{CellKind, CellXY, Grid};

use crate::hash::{Fnv64, StateHash};

/// A generated world: the grid, the beacons and the spawns, all grown from
/// the round seed, plus the fixed-point units per cell edge.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct World {
    pub grid: Grid,
    pub beacons: Vec<CellXY>,
    pub spawns: Vec<CellXY>,
    pub cell_units: i32,
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
    }
}
