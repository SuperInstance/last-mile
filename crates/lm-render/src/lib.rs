//! The lm-render crate provides rendering functionality for the lm project.
//! It handles chunk mesh building and palette management without Bevy dependencies.

pub mod figure;
pub mod fog;
pub mod greedy;
pub mod hash;
pub mod mesh;
pub mod palette;
pub mod scene;
pub mod upscale;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_palette_compiles() {
        let _palette = palette::Palette::default();
    }

    #[test]
    fn test_mesh_compiles() {
        let _mesh = mesh::ChunkMesh {
            positions: vec![],
            normals: vec![],
            colors: vec![],
            indices: vec![],
        };
    }
}
