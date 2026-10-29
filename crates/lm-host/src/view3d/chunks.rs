//! The chunks of the 3D view: one `Mesh3d` entity per visible chunk.
//!
//! Which chunks are visible is [`lm_render::scene::visible_chunks`]'s to say
//! — the world's own set, never the screen's — and the mesh behind a chunk
//! comes from [`lm_render::hash::ChunkCache`], content-addressed by the
//! chunk's structure (D46): the same structure is the same mesh asset
//! wherever it sits in the world, and a camera that pans builds nothing.
//!
//! What happens per frame is a diff, and only a diff: a chunk that enters
//! the view is spawned once, a chunk that leaves is despawned once, and a
//! chunk that stays is untouched. Entities are never added and removed every
//! frame — that churns the pipeline cache (the plan's review focus 3).

use std::collections::BTreeMap;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use lm_render::mesh::{ChunkCoord, ChunkMesh};
use lm_render::scene::ChunkEntities;

use crate::sim_plugin::Round;
use crate::view3d::{View3dMaterials, View3dSettings};

/// A chunk drawn by the 3D view. Which world tile it is lives in
/// [`ChunkStore`] — the entity map is the view's own bookkeeping, and the
/// component only marks what the entity is.
#[derive(Component)]
pub struct ChunkTile;

/// The chunk side of the view: the scene model's entity set, the
/// content-addressed mesh cache (D46), and the Bevy assets and entities
/// behind them.
#[derive(Resource, Default)]
pub struct ChunkStore {
    /// The model's chunk set: what is spawned, and its sync.
    pub entities: ChunkEntities,
    /// The mesh cache the sync builds through, keyed by structure hash.
    pub cache: lm_render::hash::ChunkCache,
    /// The Bevy mesh asset of every built structure: content-addressed, so
    /// two chunks of the same structure share one asset.
    pub meshes: BTreeMap<u64, Handle<Mesh>>,
    /// The Bevy entity of every spawned chunk.
    pub spawned: BTreeMap<ChunkCoord, Entity>,
    /// Where the view looks, in world metres. The visible set grows around
    /// it; for now it is the world's centre, set when the round starts.
    pub focus_m: (f32, f32),
}

/// A chunk's mesh data as a Bevy `Mesh`.
///
/// The basis change happens here, once, at the render boundary: the scene's
/// lattice counts x east, y south and z up, and Bevy's world counts x east,
/// y up and z south (north), so `(x, y, z)` lands as `(x, z, -y)`. This is
/// the one place a chunk's numbers change hands.
///
/// `try_insert_attribute`, not `insert_attribute`: the mesh is built inside
/// asset-building code, where a mesh may already have been extracted to the
/// render world and `insert_attribute` panics.
pub fn chunk_mesh(mesh: &ChunkMesh) -> Mesh {
    let positions: Vec<[f32; 3]> = mesh.positions.iter().map(|p| [p[0], p[2], -p[1]]).collect();
    let normals: Vec<[f32; 3]> = mesh.normals.iter().map(|n| [n[0], n[2], -n[1]]).collect();
    let mut out = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    // The mesh was built this call, so the error the `try` form reports —
    // extraction to the render world — cannot happen.
    let _ = out.try_insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    let _ = out.try_insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    let _ = out.try_insert_attribute(Mesh::ATTRIBUTE_COLOR, mesh.colors.clone());
    out.insert_indices(Indices::U32(mesh.indices.clone()));
    out
}

/// Bring the drawn chunks to the visible set: spawn what entered, despawn
/// what left, touch nothing that stayed.
pub fn sync(
    mut commands: Commands,
    round: Res<Round>,
    settings: Res<View3dSettings>,
    materials: Res<View3dMaterials>,
    mut assets: ResMut<Assets<Mesh>>,
    mut store: ResMut<ChunkStore>,
) {
    let Some(sim) = round.sim() else {
        return;
    };
    // One reborrow of the store, so its halves can be lent to the sync at
    // once: the entities to update, the cache to build through.
    let store = &mut *store;
    let want = lm_render::scene::visible_chunks(sim.world(), store.focus_m, &settings.render);
    let (added, removed) = store.entities.sync(
        &want,
        sim.world(),
        &settings.render,
        &settings.palette,
        &mut store.cache,
    );

    for (chunk, handle) in added {
        let Some(data) = store.cache.asset(handle).cloned() else {
            continue; // the sync just built or found it; not happening
        };
        let mesh = match store.meshes.get(&handle.id()) {
            Some(mesh) => mesh.clone(),
            None => {
                let mesh = assets.add(chunk_mesh(&data));
                store.meshes.insert(handle.id(), mesh.clone());
                mesh
            }
        };
        let entity = commands
            .spawn((
                Mesh3d(mesh),
                MeshMaterial3d(materials.chunk.clone()),
                ChunkTile,
            ))
            .id();
        store.spawned.insert(chunk, entity);
    }

    for chunk in removed {
        if let Some(entity) = store.spawned.remove(&chunk) {
            commands.entity(entity).despawn();
        }
    }
}
