//! The chunk mesh: the world's cells as plain vertex data.
//!
//! The mesh is Bevy-free on purpose: this crate's half of the 3D view has
//! to be testable in WSL, with no GPU and no window. What comes out is
//! plain data — positions in metres, flat normals, vertex colours and
//! triangle indices — and the host turns it into a Bevy `Mesh`.
//!
//! Every face carries **one flat colour**: all four of its vertices hold
//! the same colour, the colour of the face's material class. That is what
//! makes the greedy mesh and the unmerged mesh the same picture: a merged
//! quad has four vertices where the unmerged quads had many, so a colour
//! that varied inside a face would be interpolated differently by the two
//! meshes. Flat classes and vertex colours are also what the proxy-friendly
//! output asks for: no texture is ever sampled.

use lm_core::fixed::UNITS_PER_M;
use lm_core::world::World;

use crate::greedy::{
    chunk_quads, greedy_merge, RawQuad, MATERIAL_COVER, MATERIAL_ROAD, MATERIAL_WALL,
};
use crate::palette::{Color, MaterialClass, Palette};

pub use crate::palette::RenderConfig;

/// Coordinate for a chunk in the world
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChunkCoord {
    pub cx: u8,
    pub cy: u8,
}

/// A mesh representation for a single chunk
#[derive(Debug, Clone)]
pub struct ChunkMesh {
    /// Vertex positions (x, y, z)
    pub positions: Vec<[f32; 3]>,
    /// Vertex normals (x, y, z)
    pub normals: Vec<[f32; 3]>,
    /// Vertex colors (r, g, b, a)
    pub colors: Vec<[f32; 4]>,
    /// Triangle indices
    pub indices: Vec<u32>,
}

impl ChunkMesh {
    /// Get the number of quads in the mesh
    pub fn quad_count(&self) -> usize {
        self.indices.len() / 6
    }

    /// Get the number of triangles in the mesh
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
}

/// Build a mesh for a chunk from the world data.
///
/// The quads of the chunk are merged when `cfg.greedy` is on, and left
/// alone when it is off. The two meshes are the same picture: the merge
/// changes how many quads there are, never what they cover.
pub fn build_chunk(
    world: &World,
    chunk: ChunkCoord,
    cfg: &RenderConfig,
    palette: &Palette,
) -> ChunkMesh {
    let quads = chunk_quads(world, chunk, cfg);
    let quads = if cfg.greedy {
        greedy_merge(&quads)
    } else {
        quads
    };
    mesh_of(&quads, world, palette)
}

/// The mesh of a set of quads: four vertices and two triangles per face.
fn mesh_of(quads: &[RawQuad], world: &World, palette: &Palette) -> ChunkMesh {
    let cell_m = world.cell_units as f32 / UNITS_PER_M as f32;
    let mut mesh = ChunkMesh {
        positions: Vec::with_capacity(quads.len() * 4),
        normals: Vec::with_capacity(quads.len() * 4),
        colors: Vec::with_capacity(quads.len() * 4),
        indices: Vec::with_capacity(quads.len() * 6),
    };
    for (face, quad) in quads.iter().enumerate() {
        let normal = quad.normal().unwrap_or([0, 0, 1]);
        let color = shaded(
            class_color(palette, quad.material),
            face_shade(&normal, palette.side_shade),
        );
        // The corners step by the quad's own lengths: a merged quad is
        // `u_len × v_len` cells (or millimetres) on its axes, and every
        // corner of the face sits that far from the origin. Stepping by one
        // instead would draw a merged floor as one cell and a wall's side as
        // a millimetre sliver — the first Task 8 screenshots did exactly
        // that.
        for (u, v) in [
            (0, 0),
            (quad.u_len, 0),
            (quad.u_len, quad.v_len),
            (0, quad.v_len),
        ] {
            mesh.positions.push(corner(quad, u, v, cell_m));
            mesh.normals
                .push([normal[0] as f32, normal[1] as f32, normal[2] as f32]);
            mesh.colors.push([color.r, color.g, color.b, color.a]);
        }
        let base = u32::try_from(face).unwrap_or(0) * 4;
        mesh.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    mesh
}

/// How much light a face's own colour carries, by the direction it faces.
///
/// A **flat** shade by face direction — the tops full, the sides dimmed —
/// is what makes an unlit box read as a box: its sides are darker than its
/// top, so a wall shows its height (phase 6 Task 8's reviewer finding 2).
/// The factor is constant per direction, so a merged quad carries the same
/// shade as the cells it was merged from: the greedy mesh and the unmerged
/// one stay the same picture. (The scene's lattice counts z up, so a top
/// is `normal[2] == 1`, not `normal[1]`.)
///
/// How dark a side is is the look's business, not the mesher's: it comes
/// from the palette, so a look file can say how the toy is lit.
fn face_shade(normal: &[i32; 3], side_shade: f32) -> f32 {
    if normal[2] > 0 {
        1.0 // a top: the ground, a wall's roof, a cover's lid
    } else {
        side_shade // a side: a wall's or a cover's faces of height
    }
}

/// A colour under a shade factor.
fn shaded(color: Color, shade: f32) -> Color {
    Color {
        r: color.r * shade,
        g: color.g * shade,
        b: color.b * shade,
        a: color.a,
    }
}

/// The corner of a face, in metres.
///
/// The horizontal axes are cells on the world's grid and the vertical axis
/// is millimetres, so the two are scaled differently on the way out. The
/// conversion happens once, here at the render boundary: the geometry
/// itself stays integer.
fn corner(quad: &RawQuad, du: i32, dv: i32, cell_m: f32) -> [f32; 3] {
    let mut position = [0.0f32; 3];
    for (axis, slot) in position.iter_mut().enumerate() {
        let steps = (quad.u_axis[axis] * du + quad.v_axis[axis] * dv) as f32;
        let value = quad.origin[axis] as f32 + steps;
        *slot = if axis == 2 {
            value * 0.001
        } else {
            value * cell_m
        };
    }
    position
}

/// The colour of a material class.
fn class_color(palette: &Palette, material: u8) -> Color {
    let class = match material {
        MATERIAL_WALL => MaterialClass::Wall,
        MATERIAL_COVER => MaterialClass::Cover,
        MATERIAL_ROAD => MaterialClass::Road,
        _ => MaterialClass::Floor,
    };
    palette.class_color(class)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::greedy::{chunk_quads, greedy_merge, MATERIAL_COVER, MATERIAL_WALL};
    use lm_core::world::{CellKind, Grid};

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

    /// The render configuration the tests use: real heights, greedy on.
    fn render_config() -> RenderConfig {
        RenderConfig {
            wall_height_m: 2.6,
            cover_height_m: 1.1,
            greedy: true,
            ..RenderConfig::default()
        }
    }

    /// The extent of a mesh along one axis, in metres.
    fn extent(mesh: &ChunkMesh, axis: usize) -> (f32, f32) {
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        for position in &mesh.positions {
            min = min.min(position[axis]);
            max = max.max(position[axis]);
        }
        (min, max)
    }

    #[test]
    fn a_merged_floor_covers_the_whole_chunk() {
        // A 16x16 chunk of open floor merges into one quad — and the mesh of
        // that quad has to span the chunk, all 16 cells of it. (The first
        // Task 8 screenshots showed a violet world with a few dark squares:
        // every merged quad was being drawn one cell wide, so the floor was
        // one tile per run and the walls had no sides at all.)
        let world = world_from(&["................"; 16]);
        let cfg = render_config();
        let palette = Palette::default();

        let quads = greedy_merge(&chunk_quads(&world, ChunkCoord { cx: 0, cy: 0 }, &cfg));
        assert_eq!(quads.len(), 1, "the open floor is one merged quad");
        assert_eq!((quads[0].u_len, quads[0].v_len), (16, 16));

        let mesh = build_chunk(&world, ChunkCoord { cx: 0, cy: 0 }, &cfg, &palette);
        let cell_m = 2.0;
        for axis in 0..2 {
            let (min, max) = extent(&mesh, axis);
            assert_eq!(
                (min, max),
                (0.0, 16.0 * cell_m),
                "axis {axis}: the merged floor spans the chunk, not one cell"
            );
        }
        let (z_min, z_max) = extent(&mesh, 2);
        assert_eq!((z_min, z_max), (0.0, 0.0), "the floor lies on the ground");
    }

    #[test]
    fn a_wall_side_reaches_the_wall_height() {
        // One wall block in open floor: its sides have real height on screen,
        // which is what makes an unlit box read as a box (Task 8's finding 2).
        let mut rows = vec!["................"; 16];
        rows[8] = "........#.......";
        let rows: Vec<String> = rows.into_iter().map(String::from).collect();
        let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
        let world = world_from(&refs);
        let cfg = render_config();
        let palette = Palette::default();

        let quads = chunk_quads(&world, ChunkCoord { cx: 0, cy: 0 }, &cfg);
        assert!(
            quads
                .iter()
                .any(|q| q.material == MATERIAL_WALL && q.v_axis == [0, 0, 1]),
            "the wall block has side quads"
        );

        let mesh = build_chunk(&world, ChunkCoord { cx: 0, cy: 0 }, &cfg, &palette);
        let (_, z_max) = extent(&mesh, 2);
        assert!(
            (z_max - cfg.wall_height_m).abs() < 0.02,
            "the mesh reaches the wall height {z_max} m, not a sliver"
        );
        // The sides stand on the ground and reach the height: a face of the
        // mesh spans the wall's height corner to corner. (The first Task 8
        // screenshots had walls whose sides were one millimetre tall — flat
        // squares floating on the sky.)
        let sides_span = mesh.positions.as_chunks::<4>().0.iter().any(|face| {
            let z: Vec<f32> = face.iter().map(|p| p[2]).collect();
            let Some(min) = z.iter().cloned().reduce(f32::min) else {
                return false;
            };
            let Some(max) = z.iter().cloned().reduce(f32::max) else {
                return false;
            };
            (min - 0.0).abs() < 0.02 && (max - cfg.wall_height_m).abs() < 0.02
        });
        assert!(
            sides_span,
            "a side face reaches from the ground to the wall height"
        );
        // And the side's vertices carry the side shade, the top's the full
        // colour: the sides are visibly darker than the tops.
        let top = class_color(&palette, MATERIAL_WALL);
        let side = shaded(top, palette.side_shade);
        let colors: Vec<[f32; 4]> = mesh.colors.clone();
        assert!(
            colors.iter().any(|c| (c[0] - top.r).abs() < 1e-6),
            "a wall top is the class colour"
        );
        assert!(
            colors
                .iter()
                .any(|c| (c[0] - side.r).abs() < 1e-3 && (c[1] - side.g).abs() < 1e-3),
            "a wall side is the shaded colour"
        );
    }

    #[test]
    fn greedy_and_plain_meshes_span_the_same_extent() {
        // The merge may change how many quads there are, never what they
        // cover: the two meshes' bounding boxes agree, axis by axis.
        let mut rows = vec!["........cc......"; 16];
        rows[2] = "....##..........";
        rows[3] = "....##..c.......";
        let rows: Vec<String> = rows.into_iter().map(String::from).collect();
        let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
        let world = world_from(&refs);
        let palette = Palette::default();

        let mut greedy_cfg = render_config();
        greedy_cfg.greedy = true;
        let mut plain_cfg = render_config();
        plain_cfg.greedy = false;

        let greedy_mesh = build_chunk(&world, ChunkCoord { cx: 0, cy: 0 }, &greedy_cfg, &palette);
        let plain_mesh = build_chunk(&world, ChunkCoord { cx: 0, cy: 0 }, &plain_cfg, &palette);
        for axis in 0..3 {
            let greedy_extent = extent(&greedy_mesh, axis);
            let plain_extent = extent(&plain_mesh, axis);
            assert_eq!(
                greedy_extent, plain_extent,
                "axis {axis}: the merged mesh covers what the unmerged one covers"
            );
        }
        assert!(
            greedy_mesh.quad_count() < plain_mesh.quad_count(),
            "the merge merged something"
        );
        let _ = MATERIAL_COVER;
    }

    #[test]
    fn the_same_structure_is_the_same_local_mesh() {
        // The mesh cache is content-addressed (D46): two chunks of the same
        // structure share one mesh asset, so that asset may not know where
        // it sits — it is built in chunk-local coordinates and the host
        // places it by its world tile. (The first Task 8 review round drew
        // every identical chunk at the first one's position: the floor was
        // a few patches in a void.)
        let world = world_from(&["................................"; 32]);
        let cfg = render_config();
        let palette = Palette::default();

        let a = build_chunk(&world, ChunkCoord { cx: 0, cy: 0 }, &cfg, &palette);
        let b = build_chunk(&world, ChunkCoord { cx: 1, cy: 1 }, &cfg, &palette);
        assert_eq!(a.positions, b.positions, "the mesh is chunk-local");
        assert_eq!(a.indices, b.indices);
        let (x0, x1) = extent(&a, 0);
        let (y0, y1) = extent(&a, 1);
        assert!(
            x0 >= 0.0 && x1 <= 32.0 && y0 >= 0.0 && y1 <= 32.0,
            "the mesh never leaves its own chunk ({x0}..{x1}, {y0}..{y1})"
        );
    }
}
