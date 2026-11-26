//! The chunk structure hash (D46) and the world-anchored chunk cache.
//!
//! A chunk's mesh depends on the chunk's **structure** and on nothing else:
//! the cells it covers, the heights the boxes are drawn at, and the palette
//! the classes are coloured with. Nothing that moves through the round —
//! the tick, the agents, the squads — is part of it. So the hash of a
//! chunk's structure is the same wherever that structure sits in the
//! world, and two chunks with the same structure are the same mesh.
//!
//! That is what makes the cache **content-addressed**, and the cache key
//! **world-anchored** (D46): the key holds the world tile the chunk sits
//! on and the hash of its structure, never a screen tile. E1 measured what
//! happens otherwise: a follow camera whose keys are screen tiles re-keys
//! 68% of the tiles every frame, while the same camera on world-anchored
//! keys re-keys 4.8% and hits 95.6% of the time. The camera moving must
//! not invalidate the world.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::hash::Hash;
use std::marker::PhantomData;

use lm_core::hash::Fnv64;
use lm_core::world::{CellXY, World};

use crate::greedy::{cells_per_chunk, chunk_origin};
use crate::mesh::{build_chunk, ChunkCoord, ChunkMesh};
use crate::palette::{Palette, RenderConfig};

/// The tag of a cell that is outside the world.
///
/// The grid reads out of bounds as a wall, but the street rule does not: a
/// cell at the edge of the map is not between two buildings, it is at the
/// edge of the board. So the hash needs a tag of its own for "outside", or
/// the hash of a chunk at the edge would not be the hash of its mesh.
const TAG_OUTSIDE: u8 = 3;

/// The hash of a chunk's structure: everything that can change its mesh.
///
/// It is an FNV-1a-64 over the `CellKind` tag of every cell of the chunk
/// in canonical row-major order, then the wall and cover heights, then the
/// palette's own colours and shade. It carries no positions, no ticks and
/// nothing else that changes from round to round, so the same structure
/// always hashes the same — wherever it stands in the world.
///
/// The hash covers the chunk **and the ring of cells around it**: a cell on
/// the chunk's edge may be a street because of a wall that lies in the next
/// chunk, so the halo is part of the structure the mesh is built from. A
/// hash that left the halo out would give two different meshes the same
/// key, and the content-addressed cache (D46) would hand out the wrong one.
///
/// The hash covers the chunk, not the world it sits in: the world's cell
/// size belongs to the world the cache belongs to, and a cache is never
/// shared between two worlds.
pub fn structure_hash(
    world: &World,
    chunk: ChunkCoord,
    cfg: &RenderConfig,
    palette: &Palette,
) -> u64 {
    let cells = cells_per_chunk(world);
    let (x0, y0) = chunk_origin(chunk, cells);
    let side = i32::try_from(cells).unwrap_or(0);
    let mut h = Fnv64::new();
    h.u32(cells);
    h.u32(cells);
    for y in y0 - 1..y0 + side + 1 {
        for x in x0 - 1..x0 + side + 1 {
            h.write(&[cell_tag(world, x, y)]);
        }
    }
    // The heights the boxes are drawn at, bit for bit: a float that differs
    // in its last bit is a different mesh.
    h.u32(cfg.wall_height_m.to_bits());
    h.u32(cfg.cover_height_m.to_bits());
    // The palette: a new palette is a new mesh, even on the same cells.
    // Its colours and its shade are hashed as they are, so the hash says
    // what this docstring says it says.
    h.u32(palette.side_shade.to_bits());
    for color in [
        palette.material_colors.floor,
        palette.material_colors.road,
        palette.material_colors.wall,
        palette.material_colors.cover,
        palette.material_colors.beacon_held,
        palette.material_colors.beacon_free,
    ] {
        h.u32(color.r.to_bits());
        h.u32(color.g.to_bits());
        h.u32(color.b.to_bits());
        h.u32(color.a.to_bits());
    }
    h.u32(cfg.palette.version());
    h.finish()
}

/// The tag of the cell at `(x, y)`, or [`TAG_OUTSIDE`] when it is off the
/// board.
fn cell_tag(world: &World, x: i32, y: i32) -> u8 {
    if !world.grid.in_bounds(x, y) {
        return TAG_OUTSIDE;
    }
    world
        .grid
        .get(CellXY {
            x: u16::try_from(x).unwrap_or(0),
            y: u16::try_from(y).unwrap_or(0),
        })
        .tag()
}

/// The world-anchored cache key of a chunk (D46).
///
/// The key holds the **world tile** the chunk sits on and the hash of its
/// structure. It never holds a screen tile: the camera's view of the world
/// is not part of the world's identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChunkKey {
    /// The chunk's column in the world.
    pub cx: u8,
    /// The chunk's row in the world.
    pub cy: u8,
    /// The hash of the chunk's structure.
    pub structure: u64,
}

/// An opaque handle to a mesh asset.
///
/// This crate is Bevy-free, so the handle is the asset's id and the host
/// maps it to a real asset; in Bevy the same role is played by
/// `Handle<Mesh>`. The id of a chunk's handle is the hash of its
/// structure, which is what makes the cache content-addressed.
#[derive(Debug)]
pub struct Handle<T> {
    id: u64,
    _asset: PhantomData<fn() -> T>,
}

// The handle is a plain id: it copies and compares whatever the asset is,
// so the derives' `T: Clone` bounds would say nothing true.
impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Handle<T> {}

impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T> Eq for Handle<T> {}

impl<T> PartialOrd for Handle<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Handle<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.id.cmp(&other.id)
    }
}

impl<T> Hash for Handle<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl<T> Handle<T> {
    /// The handle of the asset with that id.
    pub const fn from_id(id: u64) -> Self {
        Self {
            id,
            _asset: PhantomData,
        }
    }

    /// The asset's id.
    pub const fn id(self) -> u64 {
        self.id
    }
}

/// The chunk mesh cache, keyed by world tile and structure (D46).
///
/// The cache holds two things: the key of every chunk it has seen, and the
/// meshes it has built, keyed by their structure hash. A chunk whose
/// structure has been built before gets the same handle wherever it stands
/// in the world, so the same mesh asset is shared by every chunk that
/// looks the same — and a pan of the camera builds nothing.
#[derive(Debug, Default)]
pub struct ChunkCache {
    /// The mesh handle of every chunk the cache has keyed.
    pub map: HashMap<ChunkKey, Handle<ChunkMesh>>,
    /// The meshes the cache has built, keyed by their structure hash.
    assets: HashMap<u64, ChunkMesh>,
    /// How many meshes the cache has built. A rebuild is a miss.
    pub builds: u64,
}

impl ChunkCache {
    /// A new, empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// The handle of a chunk's mesh, building it if the cache has not seen
    /// the chunk's structure before.
    ///
    /// The key is the world tile and the structure hash, so the same chunk
    /// always gets the same key, and a chunk that moves on the screen does
    /// not get a new one.
    pub fn get_or_build(
        &mut self,
        world: &World,
        chunk: ChunkCoord,
        cfg: &RenderConfig,
        palette: &Palette,
    ) -> Handle<ChunkMesh> {
        let structure = structure_hash(world, chunk, cfg, palette);
        let key = ChunkKey {
            cx: chunk.cx,
            cy: chunk.cy,
            structure,
        };
        if let Some(handle) = self.map.get(&key) {
            return *handle;
        }
        let handle = Handle::from_id(structure);
        let built = match self.assets.entry(structure) {
            Entry::Occupied(_) => false,
            Entry::Vacant(slot) => {
                slot.insert(build_chunk(world, chunk, cfg, palette));
                true
            }
        };
        if built {
            self.builds += 1;
        }
        self.map.insert(key, handle);
        handle
    }

    /// The mesh behind a handle, if the cache has built it.
    pub fn asset(&self, handle: Handle<ChunkMesh>) -> Option<&ChunkMesh> {
        self.assets.get(&handle.id())
    }

    /// The number of chunks the cache has keyed.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Whether the cache has keyed no chunk at all.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lm_core::header::Header;
    use lm_core::params::Params;
    use lm_core::sim::Sim;
    use lm_core::world::{generate, CellKind, Grid};

    /// The reflex every agent of the test round runs.
    const REFLEX: &str = include_str!("../../../assets/reflexes/default.rhai");

    /// A world of `w × h` cells, all floor.
    fn floor_world(w: u16, h: u16) -> World {
        World {
            grid: Grid {
                w,
                h,
                cells: vec![CellKind::Floor; usize::from(w) * usize::from(h)],
            },
            beacons: Vec::new(),
            spawns: Vec::new(),
            cell_units: 2048,
            paint: lm_core::world::Paint::new(),
        }
    }

    /// The render configuration the tests use.
    fn render_config() -> RenderConfig {
        RenderConfig {
            wall_height_m: 2.6,
            cover_height_m: 1.1,
            greedy: true,
            ..RenderConfig::default()
        }
    }

    /// The cells of a chunk, in row-major order.
    fn chunk_cells(world: &World, chunk: ChunkCoord) -> Vec<CellKind> {
        let cells = cells_per_chunk(world);
        let (x0, y0) = chunk_origin(chunk, cells);
        let side = i32::try_from(cells).unwrap_or(0);
        let mut out = Vec::new();
        for y in y0..y0 + side {
            for x in x0..x0 + side {
                out.push(world.grid.get(CellXY {
                    x: u16::try_from(x).unwrap_or(0),
                    y: u16::try_from(y).unwrap_or(0),
                }));
            }
        }
        out
    }

    #[test]
    fn hash_is_deterministic() {
        // Two chunks of the same structure, side by side.
        let world = floor_world(32, 16);
        let cfg = render_config();
        let palette = Palette::default();
        let chunk = ChunkCoord { cx: 0, cy: 0 };

        let first = structure_hash(&world, chunk, &cfg, &palette);
        let second = structure_hash(&world, chunk, &cfg, &palette);
        assert_eq!(first, second, "the same chunk hashes the same");

        // The hash is of the structure, not of the place: the same cells at
        // another world position hash the same — as long as the ring of
        // cells around them is the same too. (The ring is part of the
        // structure: a cell on the chunk's edge may be a street because of
        // a building that stands in the next chunk, so the halo is part of
        // the mesh and has to be part of the hash.)
        let mut interior = floor_world(64, 32);
        interior.grid.cells.fill(CellKind::Floor);
        let middle = structure_hash(&interior, ChunkCoord { cx: 1, cy: 0 }, &cfg, &palette);
        let other = structure_hash(&interior, ChunkCoord { cx: 2, cy: 0 }, &cfg, &palette);
        assert_eq!(middle, other, "the hash is content-addressed");
    }

    #[test]
    fn the_hash_covers_the_ring_around_the_chunk() {
        // A building just outside the chunk changes the chunk's mesh — the
        // ground beside it is a street — so it has to change the chunk's
        // hash too. A hash that left the ring out would give two different
        // meshes the same key.
        let mut world = floor_world(64, 32);
        let cfg = render_config();
        let palette = Palette::default();
        let chunk = ChunkCoord { cx: 2, cy: 0 };

        let before = structure_hash(&world, chunk, &cfg, &palette);
        // The cell just to the left of the chunk.
        world.grid.cells[31] = CellKind::Wall;
        let after = structure_hash(&world, chunk, &cfg, &palette);
        assert_ne!(
            before, after,
            "the ring around the chunk is part of its structure"
        );
    }

    #[test]
    fn hash_changes_with_structure() {
        let mut world = floor_world(16, 16);
        let cfg = render_config();
        let palette = Palette::default();
        let chunk = ChunkCoord { cx: 0, cy: 0 };

        let before = structure_hash(&world, chunk, &cfg, &palette);
        world.grid.cells[0] = CellKind::Wall;
        let after = structure_hash(&world, chunk, &cfg, &palette);
        assert_ne!(before, after, "one wall cell changes the structure");

        // And so does the height the boxes are drawn at.
        let mut taller = render_config();
        taller.wall_height_m = 3.0;
        assert_ne!(
            before,
            structure_hash(&world, chunk, &taller, &palette),
            "the wall height is part of the mesh"
        );
    }

    #[test]
    fn hash_ignores_non_structure() {
        let cfg = render_config();
        let palette = Palette::default();
        let chunk = ChunkCoord { cx: 0, cy: 0 };

        // A round that is played: the tick moves on and the agents walk.
        let mut sim = Sim::new(&Header::default_round(1, REFLEX)).expect("the round starts");
        let before = structure_hash(sim.world(), chunk, &cfg, &palette);
        let start: Vec<_> = sim
            .agent_ids()
            .iter()
            .map(|id| sim.agent(*id).expect("an agent of the round").pos)
            .collect();
        for _ in 0..60 {
            sim.step(&[]);
        }
        let after = structure_hash(sim.world(), chunk, &cfg, &palette);
        assert_eq!(before, after, "the tick and the agents are not structure");
        let end: Vec<_> = sim
            .agent_ids()
            .iter()
            .map(|id| sim.agent(*id).expect("an agent of the round").pos)
            .collect();
        assert_ne!(start, end, "the agents really did move");

        // The squad count is not structure either: two rounds that differ
        // only in their squads hash alike wherever their cells agree.
        let two = generate(1, &Params::default());
        let four = generate(
            1,
            &Params {
                squads: 4,
                ..Params::default()
            },
        );
        let mut compared = 0;
        for cx in 0..8u8 {
            for cy in 0..8u8 {
                let chunk = ChunkCoord { cx, cy };
                if chunk_cells(&two, chunk) == chunk_cells(&four, chunk) {
                    assert_eq!(
                        structure_hash(&two, chunk, &cfg, &palette),
                        structure_hash(&four, chunk, &cfg, &palette),
                        "the squad count is not part of the structure"
                    );
                    compared += 1;
                }
            }
        }
        assert!(compared > 0, "the test compared nothing");
    }

    #[test]
    fn chunk_cache_is_world_anchored() {
        // A world of two chunks with the same structure.
        let world = floor_world(32, 16);
        let cfg = render_config();
        let palette = Palette::default();
        let mut cache = ChunkCache::new();

        // Two chunks in the middle of the world, where the ring around them
        // is floor as well: the same structure, the same ring, so the same
        // mesh asset.
        let mut interior = floor_world(64, 32);
        interior.grid.cells.fill(CellKind::Floor);
        let left = cache.get_or_build(&interior, ChunkCoord { cx: 1, cy: 0 }, &cfg, &palette);
        let right = cache.get_or_build(&interior, ChunkCoord { cx: 2, cy: 0 }, &cfg, &palette);
        assert_eq!(left, right, "the same structure is the same mesh asset");
        assert_eq!(cache.builds, 1, "the mesh was built once");
        assert_eq!(cache.map.len(), 2, "both world tiles are keyed");

        // The key holds the world tile and the structure, never a screen
        // position.
        let structure = structure_hash(&interior, ChunkCoord { cx: 2, cy: 0 }, &cfg, &palette);
        let key = ChunkKey {
            cx: 1,
            cy: 0,
            structure,
        };
        assert_eq!(cache.map.get(&key), Some(&right));
        assert!(cache.asset(left).is_some(), "the mesh is behind the handle");

        // A chunk with a different structure is a different asset.
        let mut other = world.clone();
        other.grid.cells[0] = CellKind::Wall;
        let changed = cache.get_or_build(&other, ChunkCoord { cx: 0, cy: 0 }, &cfg, &palette);
        assert_ne!(left, changed, "a different structure is a different mesh");
        assert_eq!(cache.builds, 2);
    }

    #[test]
    fn cache_survives_camera_movement() {
        // The world the camera pans across: eight chunks across.
        let world = generate(1, &Params::default());
        let cfg = render_config();
        let palette = Palette::default();
        let mut cache = ChunkCache::new();

        let mut seen: HashMap<ChunkCoord, Handle<ChunkMesh>> = HashMap::new();
        for pan in 0..50u32 {
            // The camera slides east across the world, a chunk per pan.
            let cx = u8::try_from(pan % 8).unwrap_or(0);
            for cy in 0..2u8 {
                let chunk = ChunkCoord { cx, cy };
                let handle = cache.get_or_build(&world, chunk, &cfg, &palette);
                match seen.get(&chunk) {
                    Some(previous) => assert_eq!(
                        *previous, handle,
                        "a chunk keeps its handle while the camera moves"
                    ),
                    None => {
                        seen.insert(chunk, handle);
                    }
                }
            }
        }

        assert_eq!(
            cache.map.len(),
            seen.len(),
            "the camera re-keyed no chunk: one key per world tile"
        );
        assert!(
            cache.builds <= cache.map.len() as u64,
            "no chunk was built twice: {} builds for {} chunks",
            cache.builds,
            cache.map.len()
        );
        // The regression this is here for: a screen-anchored key would have
        // produced a new key on every one of the fifty pans.
        assert!(
            cache.map.len() < 50,
            "fifty pans produced {} keys, not one per pan",
            cache.map.len()
        );
    }
}
