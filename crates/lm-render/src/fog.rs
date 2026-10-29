//! The fog of the 3D view: the 2D map's own rule, shared (phase 6 Task 8).
//!
//! The 3D view is a *view* of the same round the 2D map draws, so it may
//! not invent a second fog. This module **is** the 2D map's rule, lifted
//! into the one place both views can call: a spectator sees the round as
//! the authority does (every cell known), and a possessing player sees
//! only what the squad's own [`lm_core::view::ScoreView`] vouches for — a
//! teammate standing in the cell, the beacon it holds, or an enemy sighted
//! in it. The host model's `MapModel::build` sets its per-cell `FogState`
//! through [`Fog`], and the 3D view drops the chunks that hold no known
//! cell through the same [`Fog`]: one rule, two views, and a test that
//! they cannot drift (`fog_matches_the_2d_map`, in the host model, where
//! both sides live).

use std::collections::BTreeSet;

use lm_core::sim::Sim;
use lm_core::world::{CellXY, World};

use crate::greedy::cells_per_chunk;
use crate::mesh::{ChunkCoord, RenderConfig};
use crate::scene;

/// Who is looking at the round — the fog follows them (spec §4, §6).
///
/// The host model's own `Viewer` carries a follow target the fog does not
/// care about, so this is the fog's view of a viewer: watching, or playing
/// as one agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FogViewer {
    /// A spectator: the round as the authority sees it, everything known.
    Spectator,
    /// The player of one agent: the squad's own knowledge, nothing else.
    Possessing(u16),
}

/// How well the viewer knows one cell of the grid — the 2D map's own three
/// states, spelled for the render side of the seam.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellFog {
    /// In sight: only a spectator's cells ever are, between ticks.
    Visible,
    /// Seen before and remembered: the terrain shows, the present does not.
    Explored,
    /// Never seen: not drawn at all by the 3D view.
    Unknown,
}

/// The fog of one frame: what the viewer may know of the grid, built once.
///
/// Built once per frame (or per view change), because the rule is a set of
/// vouched cells and a membership question asked per cell — by the 2D map
/// for its fog overlay, by the 3D view for its chunks. A spectator's set is
/// the whole grid: nothing is fogged.
#[derive(Clone, Debug)]
pub struct Fog {
    /// Whether every cell is known: the spectator's case.
    spectator: bool,
    /// The cells the squad's view vouches for, under possession.
    vouched: BTreeSet<CellXY>,
}

impl Fog {
    /// The fog of one frame: the whole grid for a spectator, the squad's
    /// vouched cells for a possessing player.
    pub fn of(sim: &Sim, viewer: FogViewer) -> Fog {
        match viewer {
            FogViewer::Spectator => Fog {
                spectator: true,
                vouched: BTreeSet::new(),
            },
            FogViewer::Possessing(agent) => Fog::of_view(Some(&sim.view(agent))),
        }
    }

    /// The fog of a view already taken: `None` is the spectator's, and some
    /// squad's `ScoreView` is a possessing player's.
    ///
    /// The 2D map builds its model from a view it takes anyway; this hands
    /// that same view to the shared rule, so the fog costs the map no
    /// second look at the sim.
    pub fn of_view(view: Option<&lm_core::view::ScoreView>) -> Fog {
        let Some(view) = view else {
            return Fog {
                spectator: true,
                vouched: BTreeSet::new(),
            };
        };
        let mut vouched = BTreeSet::new();
        vouched.extend(view.squad.iter().map(|s| s.cell));
        vouched.extend(view.beacons.iter().map(|b| b.cell));
        vouched.extend(view.enemies.iter().map(|e| e.cell));
        Fog {
            spectator: false,
            vouched,
        }
    }

    /// How well the viewer knows one cell.
    pub fn cell(&self, cell: CellXY) -> CellFog {
        if self.spectator {
            CellFog::Visible
        } else if self.vouched.contains(&cell) {
            CellFog::Explored
        } else {
            CellFog::Unknown
        }
    }

    /// Whether the viewer knows the cell at all — anything but
    /// [`CellFog::Unknown`]. The 3D view draws only what this allows.
    pub fn knows(&self, cell: CellXY) -> bool {
        self.cell(cell) != CellFog::Unknown
    }

    /// The chunks the viewer may see: every chunk holding at least one
    /// known cell. A spectator knows them all.
    pub fn known_chunks(&self, world: &World) -> BTreeSet<ChunkCoord> {
        if self.spectator {
            let cells = cells_per_chunk(world).max(1);
            let chunks_w = u32::from(world.grid.w).div_ceil(cells);
            let chunks_h = u32::from(world.grid.h).div_ceil(cells);
            (0..chunks_w)
                .flat_map(|cx| {
                    (0..chunks_h).map(move |cy| ChunkCoord {
                        cx: cx as u8,
                        cy: cy as u8,
                    })
                })
                .collect()
        } else {
            self.vouched
                .iter()
                .map(|&cell| chunk_of_cell(world, cell))
                .collect()
        }
    }
}

/// The chunk a cell belongs to, by the greedy mesher's own arithmetic.
///
/// The one place a cell becomes a chunk tile, so the 2D map's cells and the
/// 3D view's chunks cannot disagree about the tiling.
pub fn chunk_of_cell(world: &World, cell: CellXY) -> ChunkCoord {
    let cells = cells_per_chunk(world).max(1);
    ChunkCoord {
        cx: (u32::from(cell.x) / cells) as u8,
        cy: (u32::from(cell.y) / cells) as u8,
    }
}

/// The chunks the 3D view draws under fog: the geometrically visible set
/// ([`scene::visible_chunks`]) minus the chunks the viewer does not know.
///
/// A chunk outside the viewer's knowledge is **not drawn at all**; a chunk
/// inside it but beyond `fog_far_m` is still drawn, and the world camera's
/// distance fog ramps it into the background (that half lives on the
/// camera, not in the chunk set). The order is `visible_chunks`'s own —
/// nearest first — and the mesh budget holds as before.
pub fn visible_chunks_fogged(
    sim: &Sim,
    focus_m: (f32, f32),
    cfg: &RenderConfig,
    viewer: FogViewer,
) -> Vec<ChunkCoord> {
    let known = Fog::of(sim, viewer).known_chunks(sim.world());
    scene::visible_chunks(sim.world(), focus_m, cfg)
        .into_iter()
        .filter(|chunk| known.contains(chunk))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lm_core::header::Header;

    /// The round of seed 1, unplayed: the world the screenshots draw.
    fn sim() -> Sim {
        Sim::new(&Header::default_round(
            1,
            include_str!("../../../assets/reflexes/default.rhai"),
        ))
        .expect("the round starts")
    }

    /// A configuration that sees the whole world from its centre.
    fn config() -> RenderConfig {
        RenderConfig {
            view_distance_chunks: 7,
            max_chunks: 64,
            ..RenderConfig::default()
        }
    }

    #[test]
    fn spectator_sees_every_chunk() {
        let sim = sim();
        let world = sim.world();

        // A spectator's fog knows every cell, and therefore every chunk.
        let fog = Fog::of(&sim, FogViewer::Spectator);
        for y in 0..world.grid.h {
            for x in 0..world.grid.w {
                let cell = CellXY { x, y };
                assert_eq!(fog.cell(cell), CellFog::Visible, "cell {cell:?}");
                assert!(fog.knows(cell), "a spectator knows cell {cell:?}");
            }
        }
        let chunks = fog.known_chunks(world);
        assert_eq!(chunks.len(), 64, "the 8x8 world, all of it");

        // And the fogged visible set is the unfogged one, whole: nothing is
        // dropped for a spectator, whatever the focus.
        let fogged = visible_chunks_fogged(&sim, (128.0, 128.0), &config(), FogViewer::Spectator);
        let plain = scene::visible_chunks(world, (128.0, 128.0), &config());
        assert_eq!(fogged, plain, "a spectator loses no chunk to the fog");
    }

    #[test]
    fn possessed_view_hides_unseen_chunks() {
        let sim = sim();
        let world = sim.world();
        let me = sim
            .agent_ids()
            .first()
            .copied()
            .expect("a round has agents");

        // The possessed view knows the cells the squad's view vouches for
        // and no others: every known cell holds a teammate, a beacon or a
        // sighted enemy, and the fresh round knows almost none of the map.
        let fog = Fog::of(&sim, FogViewer::Possessing(me));
        let view = sim.view(me);
        let mut vouched = BTreeSet::new();
        vouched.extend(view.squad.iter().map(|s| s.cell));
        vouched.extend(view.beacons.iter().map(|b| b.cell));
        vouched.extend(view.enemies.iter().map(|e| e.cell));
        for y in 0..world.grid.h {
            for x in 0..world.grid.w {
                let cell = CellXY { x, y };
                assert_eq!(
                    fog.cell(cell),
                    if vouched.contains(&cell) {
                        CellFog::Explored
                    } else {
                        CellFog::Unknown
                    },
                    "cell {cell:?} is vouched or unknown, nothing else"
                );
            }
        }
        assert!(
            vouched.len() < (world.grid.w as usize) * (world.grid.h as usize),
            "a fresh squad has not seen the whole map"
        );

        // The 3D view draws fewer chunks than the geometry offers: only the
        // chunks holding a vouched cell, and each of those holds at least
        // one. One's own squad is never fogged, so the chunk the squad
        // stands in is always drawn.
        let fogged =
            visible_chunks_fogged(&sim, (128.0, 128.0), &config(), FogViewer::Possessing(me));
        let plain = scene::visible_chunks(world, (128.0, 128.0), &config());
        assert!(
            fogged.len() < plain.len(),
            "the fogged set is strictly smaller than the plain one"
        );
        assert!(!fogged.is_empty(), "the squad's own chunk is drawn");
        for chunk in &fogged {
            let cells = cells_per_chunk(world);
            let origin = (u32::from(chunk.cx) * cells, u32::from(chunk.cy) * cells);
            let holds_known = (origin.1..origin.1 + cells).any(|y| {
                (origin.0..origin.0 + cells).any(|x| {
                    fog.knows(CellXY {
                        x: x as u16,
                        y: y as u16,
                    })
                })
            });
            assert!(holds_known, "chunk {chunk:?} holds a cell the squad knows");
        }
        for mate in &view.squad {
            assert!(
                fog.knows(mate.cell),
                "teammate {} stands in a known cell: the squad is never fogged",
                mate.id
            );
        }
    }
}
