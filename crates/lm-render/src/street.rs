//! The streets of the city: which of the floor cells are road.
//!
//! The world has three kinds of cell and no notion of a street: floor, wall
//! and cover. A city, though, is blocks with roads between them, and the
//! road is what makes a block a block — so the renderer derives it.
//!
//! A floor cell is a **street** when it touches a building: when one of the
//! four cells around it is a wall. Everything else that is floor is the open
//! ground the blocks stand on.
//!
//! The rule is the pavement of a city: a block of buildings wears a street
//! around it, and where two blocks stand close the two streets meet and
//! make a road between them. On the world the generator grows — blocks of
//! two to six cells, scattered — it marks about a fifth of the open ground,
//! which is enough to read as a city and little enough to leave the ground
//! calm.
//!
//! The rule looks at the cells around a cell, and those cells may be in the
//! next chunk: so the chunk's structure hash carries a one-cell halo
//! (D46's cache is content-addressed, and a mesh that depended on cells the
//! hash did not cover would be the wrong mesh).

use lm_core::world::{CellKind, CellXY, World};

/// Whether a cell is a street: a floor cell that touches a building.
///
/// The rule is a pure function of the cells around it, so the 2D map and
/// the 3D view mark the same cells.
pub fn is_street(world: &World, cell: CellXY) -> bool {
    if world.grid.get(cell) != CellKind::Floor {
        return false;
    }
    let wall = |dx: i32, dy: i32| -> bool {
        let x = i32::from(cell.x) + dx;
        let y = i32::from(cell.y) + dy;
        if !world.grid.in_bounds(x, y) {
            // The edge of the map is the edge of the board, not the side of
            // a building.
            return false;
        }
        world.grid.get(CellXY {
            x: u16::try_from(x).unwrap_or(0),
            y: u16::try_from(y).unwrap_or(0),
        }) == CellKind::Wall
    };
    wall(0, -1) || wall(0, 1) || wall(-1, 0) || wall(1, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lm_core::params::Params;
    use lm_core::world::generate;

    /// A world of `w × h` cells, every cell the kind its character says:
    /// `.` floor, `#` wall, `c` cover.
    fn world_from(rows: &[&str]) -> World {
        let h = rows.len() as u16;
        let w = rows.first().map_or(0, |r| r.len()) as u16;
        let cells = rows
            .iter()
            .flat_map(|row| row.chars())
            .map(|c| match c {
                '#' => CellKind::Wall,
                'c' => CellKind::Cover,
                _ => CellKind::Floor,
            })
            .collect();
        World {
            grid: lm_core::world::Grid { w, h, cells },
            beacons: Vec::new(),
            spawns: Vec::new(),
            cell_units: 2048,
            paint: lm_core::world::Paint::new(),
        }
    }

    #[test]
    fn the_ground_beside_a_building_is_a_street() {
        // One block of buildings, and the ground it wears around it.
        let world = world_from(&["...", ".#.", "..."]);
        let street = |x: u16, y: u16| is_street(&world, CellXY { x, y });

        // The four cells that touch the block are street.
        assert!(street(1, 0), "the ground above the block");
        assert!(street(0, 1), "the ground beside the block");
        assert!(street(2, 1), "the ground beside the block");
        assert!(street(1, 2), "the ground below the block");
        // The corners and the open ground are not.
        assert!(!street(0, 0), "the corner is open ground");
        assert!(!street(2, 2), "the corner is open ground");
    }

    #[test]
    fn two_blocks_make_a_road_between_them() {
        // Two blocks with a lane between them: the lane is street on both
        // sides, so it reads as a road.
        let world = world_from(&["#.#", "#.#", "...", "..."]);
        let street = |x: u16, y: u16| is_street(&world, CellXY { x, y });
        assert!(street(1, 0), "the lane between the blocks");
        assert!(street(1, 1), "the lane between the blocks");
        assert!(!street(2, 3), "open ground is not a street");
    }

    #[test]
    fn the_edge_of_the_map_is_not_a_street() {
        // The edge of the board is not the side of a building: a cell at
        // the edge is not touching a block, because the block would be off
        // the map.
        let world = world_from(&["..#", "...", "..#"]);
        assert!(!is_street(&world, CellXY { x: 0, y: 1 }));
    }

    #[test]
    fn a_wall_or_a_cover_is_never_a_street() {
        let world = world_from(&["#c."]);
        assert!(!is_street(&world, CellXY { x: 0, y: 0 }), "a wall");
        assert!(!is_street(&world, CellXY { x: 1, y: 0 }), "cover");
    }

    #[test]
    fn a_city_has_streets_in_it() {
        // The rule has to say something about a real world: a city with no
        // streets is a city with no roads, and the look's road colour would
        // never be seen.
        let world = generate(1, &Params::default());
        let mut streets = 0usize;
        let mut floor = 0usize;
        for y in 0..world.grid.h {
            for x in 0..world.grid.w {
                let cell = CellXY { x, y };
                if world.grid.get(cell) == CellKind::Floor {
                    floor += 1;
                    if is_street(&world, cell) {
                        streets += 1;
                    }
                }
            }
        }
        // The rule has to say something about a real world: a city with no
        // streets is a city with no roads, and the look's road colour would
        // never be seen.
        assert!(floor > 100, "the world has open ground to classify");
        assert!(
            streets > 20,
            "the city has streets: {streets} of {floor} floor cells"
        );
        // And not too many: the streets are the pavement around the blocks,
        // not the ground itself. A rule that painted most of the board road
        // would leave nothing for the ground colour to say.
        assert!(
            streets * 3 < floor,
            "the streets are a pavement, not the ground: {streets} of {floor}"
        );
    }
}
