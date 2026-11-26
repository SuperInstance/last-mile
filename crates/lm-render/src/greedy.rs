//! Greedy meshing: coplanar, same-material quads merged into the fewest
//! possible larger quads.
//!
//! The geometry lives in **integer cell space**, never in floats, so a merge
//! is exact and can never accumulate a rounding error. The horizontal axes
//! are the grid's own cell lattice; the vertical axis is in millimetres,
//! because the wall and cover heights are metre-valued parameters that need
//! not be a whole number of cells and the merge still has to be exact.
//!
//! The pass runs **per face direction** (the six axis directions), so a
//! merged quad is always planar, and **per material class**, so a merged
//! quad never spans two colours. What is left is the classic greedy mesher
//! of Mikola Lysenko ("Greedy Meshing", 0fps, 2012): in each direction's
//! plane the faces of one material are drawn into a mask, and the mask is
//! cut into the fewest rectangles that cover it.
//!
//! The merge may change *how many* quads there are, never *what* they
//! cover: the greedy mesh and the unmerged mesh paint exactly the same
//! surface, and [`same_surface`] is the check that says so.

use std::collections::BTreeMap;

use lm_core::fixed::CHUNK_UNITS;
use lm_core::world::{CellKind, CellXY, World};

use crate::mesh::ChunkCoord;
use crate::palette::RenderConfig;

/// The material class of a floor face: the cell kind's canonical tag.
pub const MATERIAL_FLOOR: u8 = CellKind::Floor.tag();
/// The material class of a wall face.
pub const MATERIAL_WALL: u8 = CellKind::Wall.tag();
/// The material class of a cover face.
pub const MATERIAL_COVER: u8 = CellKind::Cover.tag();
/// The material class of a street face: the open floor that lies between two
/// buildings.
///
/// It is a material class of its own because a street is a different colour
/// from the ground it is cut out of — and because the greedy pass merges
/// per class, so a street keeps its own long runs instead of being cut into
/// the ground's quads.
pub const MATERIAL_ROAD: u8 = 3;

/// A face in integer cell space.
///
/// `origin` is the corner of the face: `x` and `y` are cell indices on the
/// world's grid and `z` is millimetres above the floor. `u_axis` and
/// `v_axis` are the unit steps along the face's two axes; the face's
/// outward normal is their cross product, so a face says which way it
/// looks without carrying a normal of its own.
///
/// `u_len` and `v_len` count the steps along those axes: cells on a
/// horizontal axis, millimetres on the vertical one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RawQuad {
    /// The corner of the face: cells on `x` and `y`, millimetres on `z`.
    pub origin: [i32; 3],
    /// The unit step along the face's `u` axis.
    pub u_axis: [i32; 3],
    /// The unit step along the face's `v` axis.
    pub v_axis: [i32; 3],
    /// How many `u` steps the face spans.
    pub u_len: i32,
    /// How many `v` steps the face spans.
    pub v_len: i32,
    /// The material class of the face.
    pub material: u8,
}

impl RawQuad {
    /// The face's outward normal, if its axes are two different unit axes.
    pub fn normal(&self) -> Option<[i32; 3]> {
        let a = axis_of(self.u_axis)?;
        let b = axis_of(self.v_axis)?;
        if a == b {
            return None;
        }
        Some(cross(self.u_axis, self.v_axis))
    }

    /// The face's area in its own lattice units.
    ///
    /// The units of the two axes differ (cells across, millimetres up), so
    /// the area only means something inside one direction's plane — which
    /// is exactly where the greedy pass works.
    pub fn area(&self) -> i64 {
        i64::from(self.u_len) * i64::from(self.v_len)
    }
}

/// The six axis-aligned face directions.
///
/// The `u` and `v` axes of a direction are chosen so that `u × v` is the
/// outward normal: that makes a face's direction readable from its axes
/// alone, and gives every direction one lattice to merge in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Frame {
    /// The outward normal of the direction.
    normal: [i32; 3],
    /// The direction's `u` axis.
    u: [i32; 3],
    /// The direction's `v` axis.
    v: [i32; 3],
}

const FRAMES: [Frame; 6] = [
    Frame {
        normal: [1, 0, 0],
        u: [0, 1, 0],
        v: [0, 0, 1],
    },
    Frame {
        normal: [-1, 0, 0],
        u: [0, 0, 1],
        v: [0, 1, 0],
    },
    Frame {
        normal: [0, 1, 0],
        u: [0, 0, 1],
        v: [1, 0, 0],
    },
    Frame {
        normal: [0, -1, 0],
        u: [1, 0, 0],
        v: [0, 0, 1],
    },
    Frame {
        normal: [0, 0, 1],
        u: [1, 0, 0],
        v: [0, 1, 0],
    },
    Frame {
        normal: [0, 0, -1],
        u: [0, 1, 0],
        v: [1, 0, 0],
    },
];

/// A rectangle in a direction's lattice: a half-open range on each of its
/// two tangent axes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rect {
    u0: i32,
    u1: i32,
    v0: i32,
    v1: i32,
}

/// The number of cells along one axis of a chunk.
///
/// A chunk is `CHUNK_UNITS` (32 m) on a side — the same chunk the core's
/// `Pos` counts — and a cell is `world.cell_units`, so the chunk's cell
/// count follows from the world.
pub fn cells_per_chunk(world: &World) -> u32 {
    let cell_units = world.cell_units.max(1);
    let cells = CHUNK_UNITS / cell_units;
    u32::try_from(cells).unwrap_or(1).max(1)
}

/// The first cell of a chunk, on the world's grid.
pub fn chunk_origin(chunk: ChunkCoord, cells: u32) -> (i32, i32) {
    let cells = i32::try_from(cells).unwrap_or(1);
    (i32::from(chunk.cx) * cells, i32::from(chunk.cy) * cells)
}

/// The quads of a chunk, in integer cell space, before any merging.
///
/// The quads are **chunk-local**: a chunk's own `(0, 0)` corner is the
/// origin of every face it emits, so the mesh of a structure is the same
/// mesh wherever the chunk sits in the world — which is what the
/// content-addressed cache (D46) builds on: two chunks of the same
/// structure share one mesh asset, placed by the host at each chunk's own
/// world tile. (A mesh baked with absolute world coordinates would draw
/// every identical chunk at the first one's position, and the floor would
/// be patches in a void.)
///
/// A `Floor` cell is one quad on the ground. A `Wall` cell is a box: a top
/// quad and a side quad for every neighbour that does not hide it. A
/// `Cover` cell is the same box at cover height, and a cover's side is
/// hidden by a wall as well as by another cover. The neighbours are read
/// at the cell's **world** place — a chunk's edge faces the world outside
/// it — while the faces themselves are emitted in the chunk's own frame.
pub fn chunk_quads(world: &World, chunk: ChunkCoord, cfg: &RenderConfig) -> Vec<RawQuad> {
    let cells = cells_per_chunk(world);
    let (x0, y0) = chunk_origin(chunk, cells);
    let wall = height_mm(cfg.wall_height_m);
    let cover = height_mm(cfg.cover_height_m);
    let mut quads = Vec::new();
    let side = i32::try_from(cells).unwrap_or(0);
    for y in y0..y0 + side {
        for x in x0..x0 + side {
            let local = (x - x0, y - y0);
            match world.grid.get(cell_of(x, y)) {
                CellKind::Floor => {
                    // A floor cell between two buildings is a street, and a
                    // street is drawn in its own colour: the city reads as
                    // blocks with roads between them.
                    let material = if crate::street::is_street(world, cell_of(x, y)) {
                        MATERIAL_ROAD
                    } else {
                        MATERIAL_FLOOR
                    };
                    quads.push(floor_quad(local.0, local.1, material));
                }
                CellKind::Wall => box_quads(&mut quads, world, x, y, local, wall, MATERIAL_WALL),
                CellKind::Cover => box_quads(&mut quads, world, x, y, local, cover, MATERIAL_COVER),
            }
        }
    }
    quads
}

/// Merge coplanar, same-material quads into the fewest possible larger quads.
///
/// The pass runs per face direction and per material class, so a merged
/// quad is always planar and never spans two colours. Quads whose axes are
/// not two different unit axes cannot be merged and are passed through
/// untouched.
pub fn greedy_merge(quads: &[RawQuad]) -> Vec<RawQuad> {
    let mut groups: BTreeMap<(usize, u8, i32), Vec<Rect>> = BTreeMap::new();
    let mut loose: Vec<RawQuad> = Vec::new();
    for quad in quads {
        match place(quad) {
            Some((frame, slice, rect)) if rect.u1 > rect.u0 && rect.v1 > rect.v0 => {
                groups
                    .entry((frame, quad.material, slice))
                    .or_default()
                    .push(rect);
            }
            _ => loose.push(*quad),
        }
    }

    let mut merged = Vec::new();
    for ((frame, material, slice), rects) in groups {
        for rect in extract(&rects) {
            merged.push(quad_of(&FRAMES[frame], slice, rect, material));
        }
    }
    merged.extend(loose);
    merged
}

/// Whether two sets of quads cover exactly the same surface.
///
/// This is the headless stand-in for a renderer: the quads are rasterised
/// into elementary rectangles on the finest grid both sets describe, and the
/// two pictures are compared. It is what says that the greedy mesh and the
/// unmerged mesh are the same view of the world.
pub fn same_surface(a: &[RawQuad], b: &[RawQuad]) -> bool {
    let first = groups_of(a);
    let second = groups_of(b);
    for (key, rects) in &first {
        let other = second.get(key).map(Vec::as_slice).unwrap_or(&[]);
        if !group_matches(rects, other) {
            return false;
        }
    }
    for (key, rects) in &second {
        if !first.contains_key(key) && !group_matches(&[], rects) {
            return false;
        }
    }
    true
}

/// The quads of a set, grouped by direction, material and plane.
fn groups_of(quads: &[RawQuad]) -> BTreeMap<(usize, u8, i32), Vec<Rect>> {
    let mut groups: BTreeMap<(usize, u8, i32), Vec<Rect>> = BTreeMap::new();
    for quad in quads {
        if let Some((frame, slice, rect)) = place(quad) {
            if rect.u1 > rect.u0 && rect.v1 > rect.v0 {
                groups
                    .entry((frame, quad.material, slice))
                    .or_default()
                    .push(rect);
            }
        }
    }
    groups
}

/// Whether two sets of rectangles in one plane cover the same area.
fn group_matches(a: &[Rect], b: &[Rect]) -> bool {
    let mut us: Vec<i32> = a.iter().chain(b.iter()).map(|r| r.u0).collect();
    us.extend(a.iter().chain(b.iter()).map(|r| r.u1));
    us.sort_unstable();
    us.dedup();
    let mut vs: Vec<i32> = a.iter().chain(b.iter()).map(|r| r.v0).collect();
    vs.extend(a.iter().chain(b.iter()).map(|r| r.v1));
    vs.sort_unstable();
    vs.dedup();
    if us.len() < 2 || vs.len() < 2 {
        return a.is_empty() && b.is_empty();
    }
    let nu = us.len() - 1;
    let nv = vs.len() - 1;
    let mut first = vec![false; nu * nv];
    let mut second = vec![false; nu * nv];
    fill(&mut first, a, &us, &vs);
    fill(&mut second, b, &us, &vs);
    first == second
}

/// Mark the elementary cells a set of rectangles covers.
fn fill(mask: &mut [bool], rects: &[Rect], us: &[i32], vs: &[i32]) {
    let nv = vs.len() - 1;
    for rect in rects {
        for (i, w) in us.windows(2).enumerate() {
            if w[0] < rect.u0 || w[1] > rect.u1 {
                continue;
            }
            for (j, h) in vs.windows(2).enumerate() {
                if h[0] < rect.v0 || h[1] > rect.v1 {
                    continue;
                }
                mask[i * nv + j] = true;
            }
        }
    }
}

/// The direction a face belongs to, the plane it sits in, and the rectangle
/// it covers in that direction's lattice.
fn place(quad: &RawQuad) -> Option<(usize, i32, Rect)> {
    if quad.u_len < 0 || quad.v_len < 0 {
        return None;
    }
    let normal = quad.normal()?;
    let frame = FRAMES.iter().position(|f| f.normal == normal)?;
    let u_axis = axis_of(FRAMES[frame].u)?;
    let v_axis = axis_of(FRAMES[frame].v)?;
    let (u0, u1) = range_of(quad, u_axis)?;
    let (v0, v1) = range_of(quad, v_axis)?;
    let slice = quad.origin[axis_of(FRAMES[frame].normal)?];
    Some((frame, slice, Rect { u0, u1, v0, v1 }))
}

/// The range of cells a face covers along one axis.
fn range_of(quad: &RawQuad, axis: usize) -> Option<(i32, i32)> {
    let u = quad.u_axis[axis];
    let v = quad.v_axis[axis];
    if u != 0 {
        Some(span(quad.origin[axis], u, quad.u_len))
    } else if v != 0 {
        Some(span(quad.origin[axis], v, quad.v_len))
    } else {
        None
    }
}

/// A half-open range of `len` steps from `origin` along a unit step.
fn span(origin: i32, step: i32, len: i32) -> (i32, i32) {
    if step > 0 {
        (origin, origin + len)
    } else {
        (origin - len + 1, origin + 1)
    }
}

/// Cut a plane's mask into the fewest rectangles that cover it.
///
/// This is the greedy mesher proper: the first free cell of a row grows
/// sideways for as long as the mask allows, and the band it found grows
/// downwards for as long as every cell of the band is free.
fn extract(rects: &[Rect]) -> Vec<Rect> {
    let min_u = rects.iter().map(|r| r.u0).min().unwrap_or(0);
    let max_u = rects.iter().map(|r| r.u1).max().unwrap_or(0);
    let min_v = rects.iter().map(|r| r.v0).min().unwrap_or(0);
    let max_v = rects.iter().map(|r| r.v1).max().unwrap_or(0);
    let nu = usize::try_from(max_u - min_u).unwrap_or(0);
    let nv = usize::try_from(max_v - min_v).unwrap_or(0);
    if nu == 0 || nv == 0 {
        return Vec::new();
    }

    let mut mask = vec![false; nu * nv];
    for rect in rects {
        for u in rect.u0..rect.u1 {
            for v in rect.v0..rect.v1 {
                let i = (usize::try_from(u - min_u).unwrap_or(0)) * nv
                    + usize::try_from(v - min_v).unwrap_or(0);
                if let Some(cell) = mask.get_mut(i) {
                    *cell = true;
                }
            }
        }
    }

    let mut used = vec![false; nu * nv];
    let mut out = Vec::new();
    for v in 0..nv {
        for u in 0..nu {
            let index = u * nv + v;
            if !mask[index] || used[index] {
                continue;
            }
            // Grow sideways along the row for as long as the mask allows.
            let mut width = 1;
            while u + width < nu {
                let i = (u + width) * nv + v;
                if !mask[i] || used[i] {
                    break;
                }
                width += 1;
            }
            // Then grow downwards, while the whole band is still free.
            let mut height = 1;
            while v + height < nv {
                let mut free = true;
                for k in 0..width {
                    let i = (u + k) * nv + (v + height);
                    if !mask[i] || used[i] {
                        free = false;
                        break;
                    }
                }
                if !free {
                    break;
                }
                height += 1;
            }
            for k in 0..width {
                for l in 0..height {
                    used[(u + k) * nv + (v + l)] = true;
                }
            }
            out.push(Rect {
                u0: min_u + i32::try_from(u).unwrap_or(0),
                u1: min_u + i32::try_from(u + width).unwrap_or(0),
                v0: min_v + i32::try_from(v).unwrap_or(0),
                v1: min_v + i32::try_from(v + height).unwrap_or(0),
            });
        }
    }
    out
}

/// The face of a rectangle in a direction's lattice.
fn quad_of(frame: &Frame, slice: i32, rect: Rect, material: u8) -> RawQuad {
    let mut origin = [0; 3];
    origin[axis_of(frame.normal).unwrap_or(0)] = slice;
    origin[axis_of(frame.u).unwrap_or(1)] = rect.u0;
    origin[axis_of(frame.v).unwrap_or(2)] = rect.v0;
    RawQuad {
        origin,
        u_axis: frame.u,
        v_axis: frame.v,
        u_len: rect.u1 - rect.u0,
        v_len: rect.v1 - rect.v0,
        material,
    }
}

/// The floor quad of a cell: the ground, or a street.
fn floor_quad(x: i32, y: i32, material: u8) -> RawQuad {
    RawQuad {
        origin: [x, y, 0],
        u_axis: [1, 0, 0],
        v_axis: [0, 1, 0],
        u_len: 1,
        v_len: 1,
        material,
    }
}

/// The quads of the box standing on a cell: its top and the sides that no
/// neighbour hides. The neighbours are read at the cell's world place
/// `(x, y)`; the faces are emitted at the cell's chunk-local `local`.
fn box_quads(
    quads: &mut Vec<RawQuad>,
    world: &World,
    x: i32,
    y: i32,
    local: (i32, i32),
    height: i32,
    material: u8,
) {
    if height <= 0 {
        // A box with no height is not drawn at all.
        return;
    }
    let (lx, ly) = local;
    quads.push(RawQuad {
        origin: [lx, ly, height],
        u_axis: [1, 0, 0],
        v_axis: [0, 1, 0],
        u_len: 1,
        v_len: 1,
        material,
    });
    // The +x side, at the cell's east edge.
    if side_visible(world, x + 1, y, material) {
        quads.push(RawQuad {
            origin: [lx + 1, ly, 0],
            u_axis: [0, 1, 0],
            v_axis: [0, 0, 1],
            u_len: 1,
            v_len: height,
            material,
        });
    }
    // The -x side, at the cell's west edge.
    if side_visible(world, x - 1, y, material) {
        quads.push(RawQuad {
            origin: [lx, ly, 0],
            u_axis: [0, 0, 1],
            v_axis: [0, 1, 0],
            u_len: height,
            v_len: 1,
            material,
        });
    }
    // The +y side, at the cell's south edge.
    if side_visible(world, x, y + 1, material) {
        quads.push(RawQuad {
            origin: [lx, ly + 1, 0],
            u_axis: [0, 0, 1],
            v_axis: [1, 0, 0],
            u_len: height,
            v_len: 1,
            material,
        });
    }
    // The -y side, at the cell's north edge.
    if side_visible(world, x, y - 1, material) {
        quads.push(RawQuad {
            origin: [lx, ly, 0],
            u_axis: [1, 0, 0],
            v_axis: [0, 0, 1],
            u_len: 1,
            v_len: height,
            material,
        });
    }
}

/// Whether a side of a box is visible from outside.
///
/// A wall hides the side of anything next to it, a box of the same kind
/// hides its own shared side, and the edge of the world is open air: the
/// map's border is a wall the player sees from outside.
fn side_visible(world: &World, x: i32, y: i32, material: u8) -> bool {
    if !world.grid.in_bounds(x, y) {
        return true;
    }
    let kind = world.grid.get(cell_of(x, y));
    kind != CellKind::Wall && kind.tag() != material
}

/// A cell coordinate on the world's grid.
fn cell_of(x: i32, y: i32) -> CellXY {
    CellXY {
        x: u16::try_from(x).unwrap_or(0),
        y: u16::try_from(y).unwrap_or(0),
    }
}

/// A height in metres as whole millimetres.
fn height_mm(metres: f32) -> i32 {
    if !metres.is_finite() || metres <= 0.0 {
        return 0;
    }
    let mm = metres * 1000.0;
    if mm >= i32::MAX as f32 {
        return i32::MAX;
    }
    mm.round() as i32
}

/// The axis a unit vector points along, if it is one.
fn axis_of(vector: [i32; 3]) -> Option<usize> {
    match vector {
        [1, 0, 0] | [-1, 0, 0] => Some(0),
        [0, 1, 0] | [0, -1, 0] => Some(1),
        [0, 0, 1] | [0, 0, -1] => Some(2),
        _ => None,
    }
}

/// The cross product of two vectors.
fn cross(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::{build_chunk, ChunkMesh};
    use crate::palette::Palette;
    use lm_core::world::Grid;

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
            grid: Grid { w, h, cells },
            beacons: Vec::new(),
            spawns: Vec::new(),
            cell_units: 2048,
            paint: lm_core::world::Paint::new(),
        }
    }

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

    /// The render configuration the tests use: real heights, greedy on.
    fn render_config() -> RenderConfig {
        RenderConfig {
            wall_height_m: 2.6,
            cover_height_m: 1.1,
            greedy: true,
            ..RenderConfig::default()
        }
    }

    /// One chunk of the world's corner.
    const CHUNK: ChunkCoord = ChunkCoord { cx: 0, cy: 0 };

    /// A chunk with a wall block, a cover and open floor around them.
    fn mixed_world() -> World {
        let mut rows: Vec<Vec<char>> = (0..16).map(|_| vec!['.'; 16]).collect();
        for (dy, row) in rows.iter_mut().enumerate() {
            for (dx, cell) in row.iter_mut().enumerate() {
                if (2..4).contains(&dx) && (2..4).contains(&dy) {
                    *cell = '#';
                }
                if dx == 5 && dy == 5 {
                    *cell = 'c';
                }
            }
        }
        let rows: Vec<String> = rows.iter().map(|row| row.iter().collect()).collect();
        let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
        world_from(&refs)
    }

    /// The area a set of quads covers, per direction, plane and material.
    fn covered_area(quads: &[RawQuad]) -> BTreeMap<(usize, u8, i32), i64> {
        let mut area: BTreeMap<(usize, u8, i32), i64> = BTreeMap::new();
        for quad in quads {
            if let Some((frame, slice, rect)) = place(quad) {
                let entry = area.entry((frame, quad.material, slice)).or_insert(0);
                *entry += i64::from(rect.u1 - rect.u0) * i64::from(rect.v1 - rect.v0);
            }
        }
        area
    }

    /// Whether two faces share any area.
    fn overlaps(a: &RawQuad, b: &RawQuad) -> bool {
        let (Some((fa, sa, ra)), Some((fb, sb, rb))) = (place(a), place(b)) else {
            return false;
        };
        fa == fb && sa == sb && ra.u0 < rb.u1 && rb.u0 < ra.u1 && ra.v0 < rb.v1 && rb.v0 < ra.v1
    }

    /// Whether every face of a mesh carries one flat colour.
    fn faces_are_flat(mesh: &ChunkMesh) -> bool {
        mesh.indices.len().is_multiple_of(6)
            && mesh.positions.len().is_multiple_of(4)
            && mesh.colors.len() == mesh.positions.len()
            && mesh.normals.len() == mesh.positions.len()
            && {
                for face in 0..mesh.indices.len() / 6 {
                    let base = face * 4;
                    let colour = mesh.colors[base];
                    for corner in 1..4 {
                        if mesh.colors[base + corner] != colour {
                            return false;
                        }
                    }
                }
                true
            }
    }

    #[test]
    fn greedy_reduces_quad_count() {
        let world = floor_world(16, 16);
        let cfg = render_config();
        let quads = chunk_quads(&world, CHUNK, &cfg);
        assert_eq!(quads.len(), 256, "an open floor is one quad per cell");
        assert!(quads.iter().all(|q| q.material == MATERIAL_FLOOR));

        let merged = greedy_merge(&quads);
        assert_eq!(merged.len(), 1, "a 16x16 open floor is one quad, not 256");
        assert_eq!((merged[0].u_len, merged[0].v_len), (16, 16));
        assert_eq!(merged[0].material, MATERIAL_FLOOR);
    }

    #[test]
    fn greedy_preserves_geometry() {
        let world = mixed_world();
        let cfg = render_config();
        let quads = chunk_quads(&world, CHUNK, &cfg);
        let merged = greedy_merge(&quads);
        assert!(
            merged.len() < quads.len(),
            "the merge has to merge something"
        );

        // The same total area, per direction, plane and material...
        assert_eq!(covered_area(&merged), covered_area(&quads));
        // ...and the same covered cell footprints, not merely the same area.
        assert!(
            same_surface(&merged, &quads),
            "the merged quads cover exactly the unmerged footprints"
        );
    }

    #[test]
    fn greedy_never_merges_across_materials() {
        let world = mixed_world();
        let cfg = render_config();
        let quads = chunk_quads(&world, CHUNK, &cfg);
        let merged = greedy_merge(&quads);

        for quad in &merged {
            for other in &quads {
                if overlaps(quad, other) {
                    assert_eq!(
                        quad.material, other.material,
                        "a merged quad spans two material classes"
                    );
                }
            }
        }
        // The chunk really does carry more than one material class.
        let mut materials: Vec<u8> = quads.iter().map(|q| q.material).collect();
        materials.sort_unstable();
        materials.dedup();
        assert!(
            materials.len() > 1,
            "the test needs a chunk with two classes"
        );
    }

    #[test]
    fn greedy_is_deterministic() {
        let world = mixed_world();
        let cfg = render_config();
        let quads = chunk_quads(&world, CHUNK, &cfg);

        let first = greedy_merge(&quads);
        let second = greedy_merge(&quads);
        assert_eq!(first, second, "the same quads merge the same way");

        // The order the quads arrive in does not change the result either:
        // the pass groups by direction and material before it merges.
        let mut reversed = quads.clone();
        reversed.reverse();
        assert_eq!(greedy_merge(&reversed), first);
    }

    #[test]
    fn greedy_matches_unmerged_visually() {
        let world = mixed_world();
        let palette = Palette::default();
        let mut greedy_cfg = render_config();
        greedy_cfg.greedy = true;
        let mut plain_cfg = render_config();
        plain_cfg.greedy = false;

        let greedy_mesh = build_chunk(&world, CHUNK, &greedy_cfg, &palette);
        let plain_mesh = build_chunk(&world, CHUNK, &plain_cfg, &palette);
        assert!(
            greedy_mesh.quad_count() < plain_mesh.quad_count(),
            "the greedy mesh has fewer quads"
        );

        // The headless rasteriser: the two meshes cover the same surface.
        let greedy_quads = greedy_merge(&chunk_quads(&world, CHUNK, &greedy_cfg));
        let plain_quads = chunk_quads(&world, CHUNK, &plain_cfg);
        assert!(same_surface(&greedy_quads, &plain_quads));

        // Both meshes paint every face in one flat colour, so the two
        // pictures are the same picture and not merely the same shape.
        assert!(faces_are_flat(&greedy_mesh));
        assert!(faces_are_flat(&plain_mesh));
        let mut greedy_colours: Vec<[f32; 4]> = greedy_mesh.colors.clone();
        let mut plain_colours: Vec<[f32; 4]> = plain_mesh.colors.clone();
        greedy_colours.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        plain_colours.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        greedy_colours.dedup();
        plain_colours.dedup();
        assert_eq!(
            greedy_colours, plain_colours,
            "the same classes, the same colours"
        );
    }
}
