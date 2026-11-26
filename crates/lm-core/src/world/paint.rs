//! The paint on the ground: what a tag leaves behind.
//!
//! A tag is the readable consequence of a contact. The rule that writes it
//! lives in the rules — `rules::beacons` natively and `assets/rules/beacons.rhai`
//! for the scripted mirror — and this module holds the paint itself: the
//! medium the rule writes into.
//!
//! Three things are true of the paint, and all three are why it lives here:
//!
//! * **It is world state.** The paint is part of the world, so it is hashed
//!   with the world: a replay of a painted round is bit-exact, and a round
//!   that painted and a round that did not are different games. An *empty*
//!   paint layer adds no bytes, the way a branch's lineage adds none when it
//!   is not there: a round that never painted is the world it was, and it
//!   hashes as it did then.
//! * **It is fixed-point.** The wetness is Q8 (255 is fresh, 0 is dry) and
//!   the counts are plain integers. Nothing here is a float.
//! * **It dries.** The medium is not a rule of the round: whatever the rules
//!   say, the paint dries on its own, one step per tick. The rules decide
//!   *whether* a cell is splatted; the medium decides how long the splat
//!   lasts. `Params::dry_ticks` says how long that is.
//!
//! The paint is kept sparsely — only the cells that carry paint are stored —
//! so that the world stays cheap to hash and a round's paint is a short list
//! rather than a grid-sized array.

use crate::hash::{Fnv64, StateHash};
use crate::world::{CellXY, Grid};
use std::collections::BTreeMap;

/// The wetness of a fresh splat, in Q8: the wettest a cell can be.
pub const WET_Q8_MAX: u8 = 255;

/// The paint on one cell of the grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PaintCell {
    /// The squad whose paint it is: the team that splatted the cell.
    ///
    /// The last splat wins — a cell painted by both teams carries the colour
    /// of whoever hit it last, and the splat count keeps both.
    pub owner: u8,
    /// How wet the paint is, in Q8: 255 is fresh, 0 is dry.
    pub wet_q8: u8,
    /// How many splats have landed on this cell, saturated at 255.
    pub splats: u8,
}

impl StateHash for PaintCell {
    fn hash_into(&self, h: &mut Fnv64) {
        h.write(&[self.owner, self.wet_q8, self.splats]);
    }
}

/// The paint layer of the world: the painted cells, and nothing else.
///
/// The cells are held in a `BTreeMap`, so the layer walks in one fixed order
/// on every machine and hashes the same bytes everywhere (D69).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Paint {
    /// The painted cells, keyed by cell.
    cells: BTreeMap<CellXY, PaintCell>,
}

impl Paint {
    /// A world with no paint on it.
    pub fn new() -> Paint {
        Paint {
            cells: BTreeMap::new(),
        }
    }

    /// The paint on `cell`, if there is any.
    pub fn get(&self, cell: CellXY) -> Option<&PaintCell> {
        self.cells.get(&cell)
    }

    /// How many cells carry paint.
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// Whether no cell carries paint.
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// The painted cells, in cell order.
    pub fn cells(&self) -> impl Iterator<Item = (CellXY, &PaintCell)> {
        self.cells.iter().map(|(&cell, paint)| (cell, paint))
    }

    /// Splat `cell` with the paint of `owner`.
    ///
    /// A splat makes the cell as wet as it can be, gives it the colour of
    /// whoever splatted it, and counts the splat. The count saturates: a
    /// cell that has been hit 255 times still counts 255.
    pub fn splat(&mut self, cell: CellXY, owner: u8) {
        let paint = self.cells.entry(cell).or_insert(PaintCell {
            owner,
            wet_q8: WET_Q8_MAX,
            splats: 0,
        });
        paint.owner = owner;
        paint.wet_q8 = WET_Q8_MAX;
        paint.splats = paint.splats.saturating_add(1);
    }

    /// Dry the paint by `step` Q8 units, and drop the cells that are dry.
    ///
    /// A cell that reaches zero wetness is no longer painted: it leaves the
    /// layer, and the world hashes as if it had never been splatted.
    pub fn dry(&mut self, step: u8) {
        if step == 0 {
            return;
        }
        self.cells.retain(|_, paint| {
            paint.wet_q8 = paint.wet_q8.saturating_sub(step);
            paint.wet_q8 > 0
        });
    }
}

impl StateHash for Paint {
    fn hash_into(&self, h: &mut Fnv64) {
        // An empty layer adds no bytes, the way a branch's lineage adds none
        // when it is not there: a round that never painted is the world it
        // was before the paint existed, and it hashes as it did then. A
        // round that painted is a different world, and the paint's bytes are
        // what say so.
        if self.cells.is_empty() {
            return;
        }
        h.write(&[1]);
        h.u32(self.cells.len() as u32);
        for (cell, paint) in &self.cells {
            cell.hash_into(h);
            paint.hash_into(h);
        }
    }
}

/// The wetness a cell loses per tick, given how long a splat lasts.
///
/// The step is `ceil(256 / dry_ticks)`, so a splat is always dry **within**
/// `dry_ticks` ticks, and exactly so whenever `dry_ticks` divides 256. The
/// step is never zero: a splat dries, however slowly.
pub fn dry_step_q8(dry_ticks: u16) -> u8 {
    let ticks = u32::from(dry_ticks.max(1));
    let step = 256u32.div_ceil(ticks);
    u8::try_from(step.clamp(1, u32::from(WET_Q8_MAX))).unwrap_or(WET_Q8_MAX)
}

/// The cells a splat covers: the centre and the cells around it.
///
/// The neighbourhood is a square of Chebyshev radius `radius` — the same
/// metric the grid's sight uses — clipped to the grid, so a splat never
/// lands outside the map. The cells come in row-major order, so the native
/// rules and the script that mirrors them walk them the same way.
pub fn splat_cells(grid: &Grid, centre: CellXY, radius: u8) -> Vec<CellXY> {
    let radius = i32::from(radius);
    let mut cells = Vec::new();
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let (x, y) = (i32::from(centre.x) + dx, i32::from(centre.y) + dy);
            if !grid.in_bounds(x, y) {
                continue;
            }
            cells.push(CellXY {
                x: x as u16,
                y: y as u16,
            });
        }
    }
    cells
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::CellKind;

    /// A flat 8×8 grid.
    fn flat() -> Grid {
        Grid::new(8, 8)
    }

    #[test]
    fn a_splat_is_wet_and_counted() {
        let mut paint = Paint::new();
        let cell = CellXY { x: 3, y: 4 };
        paint.splat(cell, 1);
        let splat = paint.get(cell).expect("the cell carries paint");
        assert_eq!(splat.owner, 1);
        assert_eq!(splat.wet_q8, WET_Q8_MAX);
        assert_eq!(splat.splats, 1);

        // A second splat by the other team: the colour is the last one, and
        // the count keeps both.
        paint.splat(cell, 0);
        let splat = paint.get(cell).expect("the cell carries paint");
        assert_eq!(splat.owner, 0);
        assert_eq!(splat.wet_q8, WET_Q8_MAX);
        assert_eq!(splat.splats, 2);
    }

    #[test]
    fn the_splat_count_saturates() {
        let mut paint = Paint::new();
        let cell = CellXY { x: 0, y: 0 };
        for _ in 0..300 {
            paint.splat(cell, 0);
        }
        assert_eq!(paint.get(cell).expect("painted").splats, 255);
    }

    #[test]
    fn paint_dries_and_leaves_the_layer() {
        let mut paint = Paint::new();
        let cell = CellXY { x: 1, y: 1 };
        paint.splat(cell, 0);
        paint.dry(10);
        assert_eq!(paint.get(cell).expect("still wet").wet_q8, 245);
        paint.dry(245);
        assert!(paint.get(cell).is_none(), "a dry cell leaves the layer");
        assert!(paint.is_empty());
    }

    #[test]
    fn a_dry_step_of_zero_dries_nothing() {
        let mut paint = Paint::new();
        paint.splat(CellXY { x: 1, y: 1 }, 0);
        paint.dry(0);
        assert_eq!(paint.get(CellXY { x: 1, y: 1 }).expect("wet").wet_q8, 255);
    }

    #[test]
    fn the_dry_step_never_outlives_the_dry_ticks() {
        // The paint is dry within `dry_ticks` ticks, for every bound.
        for dry_ticks in [1u16, 2, 7, 10, 32, 60, 100, 128, 150, 256, 300, 600] {
            let step = u32::from(dry_step_q8(dry_ticks));
            let ticks = 255u32.div_ceil(step);
            assert!(
                ticks <= u32::from(dry_ticks),
                "dry_ticks {dry_ticks}: the paint lasts {ticks} ticks"
            );
            assert!(step >= 1, "dry_ticks {dry_ticks}: the paint never dries");
        }
    }

    #[test]
    fn a_splat_covers_the_cell_and_its_neighbours() {
        let grid = flat();
        let cells = splat_cells(&grid, CellXY { x: 4, y: 4 }, 1);
        assert_eq!(cells.len(), 9);
        assert!(cells.contains(&CellXY { x: 4, y: 4 }));
        assert!(cells.contains(&CellXY { x: 5, y: 5 }));
        assert!(cells.contains(&CellXY { x: 3, y: 3 }));

        // A radius of zero is the cell itself.
        let cells = splat_cells(&grid, CellXY { x: 4, y: 4 }, 0);
        assert_eq!(cells, vec![CellXY { x: 4, y: 4 }]);
    }

    #[test]
    fn a_splat_is_clipped_at_the_border() {
        let grid = flat();
        let cells = splat_cells(&grid, CellXY { x: 0, y: 0 }, 1);
        assert_eq!(cells.len(), 4);
        assert!(cells.contains(&CellXY { x: 0, y: 0 }));
        assert!(cells.contains(&CellXY { x: 1, y: 0 }));
        assert!(cells.contains(&CellXY { x: 0, y: 1 }));
        assert!(cells.contains(&CellXY { x: 1, y: 1 }));
    }

    #[test]
    fn the_neighbourhood_is_row_major() {
        let grid = flat();
        let cells = splat_cells(&grid, CellXY { x: 1, y: 1 }, 1);
        assert_eq!(
            cells,
            vec![
                CellXY { x: 0, y: 0 },
                CellXY { x: 1, y: 0 },
                CellXY { x: 2, y: 0 },
                CellXY { x: 0, y: 1 },
                CellXY { x: 1, y: 1 },
                CellXY { x: 2, y: 1 },
                CellXY { x: 0, y: 2 },
                CellXY { x: 1, y: 2 },
                CellXY { x: 2, y: 2 },
            ]
        );
    }

    #[test]
    fn a_wall_can_carry_paint() {
        // The paint is on the ground, not on the floor: a splat lands on the
        // wall the shot hit.
        let mut grid = flat();
        grid.set(CellXY { x: 2, y: 2 }, CellKind::Wall);
        let cells = splat_cells(&grid, CellXY { x: 2, y: 2 }, 1);
        assert!(cells.contains(&CellXY { x: 2, y: 2 }));
    }

    #[test]
    fn the_paint_hash_sees_every_field() {
        let cell = CellXY { x: 1, y: 1 };
        let mut paint = Paint::new();
        paint.splat(cell, 0);
        let base = paint.state_hash();

        let mut other_owner = Paint::new();
        other_owner.splat(cell, 1);
        assert_ne!(base, other_owner.state_hash(), "the owner is hashed");

        let mut wetter = Paint::new();
        wetter.splat(cell, 0);
        wetter.dry(10);
        assert_ne!(base, wetter.state_hash(), "the wetness is hashed");

        let mut counted = Paint::new();
        counted.splat(cell, 0);
        counted.splat(cell, 0);
        assert_ne!(base, counted.state_hash(), "the splat count is hashed");

        assert_ne!(Paint::new().state_hash(), base, "the layer is hashed");
    }

    #[test]
    fn the_paint_hash_is_stable() {
        let mut paint = Paint::new();
        paint.splat(CellXY { x: 2, y: 1 }, 0);
        paint.splat(CellXY { x: 1, y: 2 }, 1);
        assert_eq!(paint.state_hash(), paint.clone().state_hash());
    }

    #[test]
    fn the_world_hash_sees_the_paint() {
        // The paint is world state: a world that carries paint is a
        // different world.
        let p = crate::params::Params::default();
        let mut world = crate::world::generate(1, &p);
        let before = world.state_hash();
        world.paint.splat(CellXY { x: 2, y: 3 }, 0);
        assert_ne!(
            before,
            world.state_hash(),
            "the paint is hashed with the world"
        );
    }
}
