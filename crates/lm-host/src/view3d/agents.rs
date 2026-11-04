//! The agents of the 3D view: one figure per agent of the roster, moved by
//! its `Transform`.
//!
//! Every agent shares **one** figure mesh ([`lm_render::figure::Figure`],
//! Task 5's low-poly body-and-head) and **one material per squad**, so the
//! engine's own batching instanced them on the GPU — there is no `Instance`
//! component to write and no opt-out needed. What makes each figure its own
//! is its `Transform`: set every frame from the lerped position and facing
//! the 2D map draws, so the two views never disagree.
//!
//! The interpolation is [`lm_render::scene::agent_figures`]'s: the same
//! `prev_positions`, the same overstep fraction, the same lerp in `f64` over
//! the unit counts. Positions are never extrapolated and never fed back
//! (D8) — the view reads the round, it does not rule it.

use std::collections::{BTreeMap, BTreeSet};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use lm_render::figure::Figure;
use lm_render::scene::FigureDraw;

use crate::sim_plugin::Round;
use crate::view3d::{View3dMaterials, View3dSettings};

/// The figure body of an agent of the roster. Which agent it is lives in
/// [`FigureEntities`] — the entity map is the view's own bookkeeping, and
/// the component only marks what the entity is.
#[derive(Component)]
pub struct FigureBody;

/// The one figure mesh every agent shares.
#[derive(Resource)]
pub struct FigureMesh(pub Handle<Mesh>);

/// The Bevy entity behind every agent of the roster.
#[derive(Resource, Default)]
pub struct FigureEntities(pub BTreeMap<u16, Entity>);

/// The figure as a Bevy `Mesh`.
///
/// The figure's own coordinates are already the renderer's — metres, origin
/// at the feet, +Z forward — so nothing is remapped; what is added is the
/// flat normal of every face. One vertex per corner of every triangle, so a
/// face owns its corners: the figure is boxes, and a corner shared between
/// faces would smooth them into something that is neither flat nor a box.
pub fn figure_mesh(figure: &Figure) -> Mesh {
    let corners = |triangle: &lm_render::figure::Triangle| {
        let at = |i: usize| &figure.mesh.vertices[i];
        (at(triangle.a), at(triangle.b), at(triangle.c))
    };
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(figure.mesh.triangles.len() * 3);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(figure.mesh.triangles.len() * 3);
    let mut indices: Vec<u32> = Vec::with_capacity(figure.mesh.triangles.len() * 3);
    for (nth, triangle) in figure.mesh.triangles.iter().enumerate() {
        let (a, b, c) = corners(triangle);
        let a = Vec3::new(a.x, a.y, a.z);
        let b = Vec3::new(b.x, b.y, b.z);
        let c = Vec3::new(c.x, c.y, c.z);
        let normal = (b - a).cross(c - a).normalize_or_zero();
        for corner in [a, b, c] {
            positions.push([corner.x, corner.y, corner.z]);
            normals.push([normal.x, normal.y, normal.z]);
        }
        let base = u32::try_from(nth * 3).unwrap_or(0);
        indices.extend_from_slice(&[base, base + 1, base + 2]);
    }
    let mut out = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    // try_insert_attribute, not insert_attribute: this is asset-building
    // code, where the mesh may already have been extracted. The mesh was
    // built this call, so the error the `try` form reports cannot happen.
    let _ = out.try_insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    let _ = out.try_insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    out.insert_indices(Indices::U32(indices));
    out
}

/// Where a figure stands and which way it looks.
///
/// The basis change is the chunks' own — x east, y up, z north — and the
/// yaw turns the figure's +Z forward into the agent's facing: a facing of
/// zero looks east, a quarter turn looks south (which is north's `-z`).
fn figure_transform(figure: &FigureDraw) -> Transform {
    let translation = Vec3::new(figure.pos_m.0, 0.0, -figure.pos_m.1);
    let yaw = std::f32::consts::FRAC_PI_2 + figure.facing_rad;
    Transform::from_translation(translation).with_rotation(Quat::from_rotation_y(yaw))
}

/// The figures of this frame: one entity per agent of the roster, no more,
/// each at its lerped position in its squad's colour.
#[allow(clippy::too_many_arguments)]
pub fn update_figures(
    mut commands: Commands,
    round: Res<Round>,
    settings: Res<View3dSettings>,
    materials: Res<View3dMaterials>,
    figure_mesh: Res<FigureMesh>,
    mut entities: ResMut<FigureEntities>,
    fixed: Res<Time<Fixed>>,
    mut placed: Query<&mut Transform, With<FigureBody>>,
) {
    let Some(sim) = round.sim() else {
        return;
    };
    let figures = lm_render::scene::agent_figures(
        sim,
        round.prev_positions(),
        fixed.overstep_fraction(),
        &settings.palette,
    );

    let mut seen = BTreeSet::new();
    for figure in &figures {
        seen.insert(figure.id);
        let transform = figure_transform(figure);
        match entities.0.get(&figure.id) {
            Some(&entity) => {
                if let Ok(mut placed) = placed.get_mut(entity) {
                    *placed = transform;
                }
            }
            None => {
                // The figure is shared by every agent; the material is the
                // squad's own, and that pair is what the engine instances.
                let squad = usize::from(figure.squad % 4);
                let entity = commands
                    .spawn((
                        Mesh3d(figure_mesh.0.clone()),
                        MeshMaterial3d(materials.squads[squad].clone()),
                        transform,
                        FigureBody,
                    ))
                    .id();
                entities.0.insert(figure.id, entity);
            }
        }
    }

    // The roster does not shrink while a round plays, but the view is not
    // the one to assume it: a figure with no agent is despawned, not kept.
    let gone: Vec<u16> = entities
        .0
        .keys()
        .filter(|id| !seen.contains(*id))
        .copied()
        .collect();
    for id in gone {
        if let Some(entity) = entities.0.remove(&id) {
            commands.entity(entity).despawn();
        }
    }
}
