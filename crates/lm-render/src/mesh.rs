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

use crate::greedy::{chunk_quads, greedy_merge, RawQuad, MATERIAL_COVER, MATERIAL_WALL};
use crate::palette::{Color, MaterialClass, Palette};

pub use crate::palette::RenderConfig;

/// Coordinate for a chunk in the world
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
        let color = class_color(palette, quad.material);
        for (du, dv) in [(0, 0), (1, 0), (1, 1), (0, 1)] {
            mesh.positions.push(corner(quad, du, dv, cell_m));
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
        _ => MaterialClass::Floor,
    };
    palette.class_color(class)
}
