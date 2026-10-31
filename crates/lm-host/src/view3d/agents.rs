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
///
/// The body is the **parent** of the piece: the outline shell, the base it
/// stands on and the leader's hat are its children, so a frame's transform
/// is written once and the whole piece moves with it.
#[derive(Component)]
pub struct FigureBody;

/// The outline shell of a figure: the same mesh, a shade larger, drawn with
/// its back faces only.
///
/// The figure covers the middle of the shell, so only the rim shows: the
/// outline is the figure's own silhouette, drawn a little larger — a piece
/// on a board, separated from the board by a dark edge.
#[derive(Component)]
pub struct FigureOutline;

/// The base a figure stands on: a flat plate on the ground, in the ink of
/// the picture. It is the piece's contact shadow and its footing at once.
#[derive(Component)]
pub struct FigureBase;

/// The leader's hat: the mark that says which figure leads its squad.
#[derive(Component)]
pub struct LeaderHat;

/// The one figure mesh every agent shares.
#[derive(Resource)]
pub struct FigureMesh(pub Handle<Mesh>);

/// The Bevy entity behind every agent of the roster.
#[derive(Resource, Default)]
pub struct FigureEntities(pub BTreeMap<u16, Entity>);

/// The one marker mesh every beacon shares (Task 8's reviewer finding 4).
#[derive(Resource)]
pub struct BeaconMesh(pub Handle<Mesh>);

/// A beacon's marker: a slim pylon in the beacon class's colour — the
/// palette's own `beacon_free` or `beacon_held`, the same colours the 2D
/// map's diamonds wear.
#[derive(Component)]
pub struct BeaconMarker;

/// The Bevy entities behind the round's beacons, in world order.
#[derive(Resource, Default)]
pub struct BeaconEntities(pub Vec<Entity>);

/// Draw the round's beacons: one marker each, in the class colour of its
/// holder, at its cell (Task 8's reviewer finding 4: 3D shows the same
/// game as 2D, beacons included).
///
/// The beacons are the round's common knowledge — pinned in the header,
/// symmetric for both squads — so every viewer sees every one of them,
/// exactly as the 2D map does; only the *colour* follows the holder, and a
/// change of hands swaps a material, never a mesh, so nothing compiles
/// mid-round.
pub fn update_beacons(
    mut commands: Commands,
    round: Res<Round>,
    materials: Res<View3dMaterials>,
    mesh: Res<BeaconMesh>,
    mut entities: ResMut<BeaconEntities>,
    mut markers: Query<(&mut Transform, &mut MeshMaterial3d<StandardMaterial>), With<BeaconMarker>>,
) {
    let Some(sim) = round.sim() else {
        return;
    };
    let beacons = lm_render::scene::beacons(sim);

    // The roster never changes; the markers are made once and only move or
    // change colour after that.
    while entities.0.len() < beacons.len() {
        let entity = commands
            .spawn((
                Mesh3d(mesh.0.clone()),
                MeshMaterial3d(materials.beacon_free.clone()),
                BeaconMarker,
            ))
            .id();
        entities.0.push(entity);
    }
    for &extra in entities.0.iter().skip(beacons.len()) {
        commands.entity(extra).despawn();
    }
    entities.0.truncate(beacons.len());

    for (index, beacon) in beacons.iter().enumerate() {
        let Some(&entity) = entities.0.get(index) else {
            continue;
        };
        let Ok((mut transform, mut material)) = markers.get_mut(entity) else {
            continue;
        };
        // A marker stands at the centre of its cell, on the ground: a slim
        // tower, taller than the walls, half of it either side of its own
        // middle.
        transform.translation = Vec3::new(beacon.pos_m.0, 3.6, -beacon.pos_m.1);
        // The colour follows the holder: the held class for a squad, the
        // free class for nobody.
        let wanted = match beacon.holder {
            Some(_) => materials.beacon_held.clone(),
            None => materials.beacon_free.clone(),
        };
        if material.0 != wanted {
            *material = MeshMaterial3d(wanted);
        }
    }
}

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
/// The scale is the look's: a figure is a game piece, and how big a game
/// piece is a thing the look file says.
fn figure_transform(figure: &FigureDraw, scale: f32) -> Transform {
    let translation = Vec3::new(figure.pos_m.0, 0.0, -figure.pos_m.1);
    let yaw = std::f32::consts::FRAC_PI_2 + figure.facing_rad;
    Transform::from_translation(translation)
        .with_rotation(Quat::from_rotation_y(yaw))
        .with_scale(Vec3::splat(scale))
}

/// The height of the figure mesh, in the mesh's own units.
///
/// The mesh is a body and a head, 1.5 units tall; the look says how tall a
/// figure stands in metres, and the scale is the ratio.
pub const FIGURE_MESH_HEIGHT: f32 = 1.5;

/// The radius of the leader's hat, in the mesh's own units.
///
/// The hat is a brim of this radius, and the look says how wide it should
/// be in metres; the two are the ratio the hat is scaled by.
pub const HAT_BRIM_MESH_RADIUS: f32 = 0.5;

/// The figures of the last frame: the pieces, as the view drew them.
///
/// The view draws them, and the effects read them: the pips go over the
/// head of the agent that is out of the round, and the path follows the
/// agent being watched. One list, so the two cannot disagree about where
/// a piece is standing.
#[derive(Resource, Default, Debug, Clone)]
pub struct Figures(pub Vec<lm_render::scene::FigureDraw>);

/// The figures of this frame: one piece per agent of the roster, no more,
/// each at its lerped position in its squad's colour.
///
/// The piece is four entities: the body, the outline shell around it, the
/// base it stands on, and — for the leader — the hat. The body is the
/// parent, so a frame's transform is written once and the whole piece
/// follows it.
#[allow(clippy::too_many_arguments)]
pub fn update_figures(
    mut commands: Commands,
    round: Res<Round>,
    settings: Res<View3dSettings>,
    materials: Res<View3dMaterials>,
    figure_mesh: Res<FigureMesh>,
    mut entities: ResMut<FigureEntities>,
    mut drawn: ResMut<Figures>,
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
    // The effects read the same list the pieces are drawn from.
    drawn.0 = figures.clone();

    let look = &settings.look.0;
    let scale = look.figure.height_m / FIGURE_MESH_HEIGHT;
    // The shell stands off the figure by the look's outline width, in
    // metres: the scale says how big the figure is, so the width has to be
    // read in the same units the figure is measured in.
    let shell_scale = 1.0 + look.outline.width_m / scale;

    let mut seen = BTreeSet::new();
    for figure in &figures {
        seen.insert(figure.id);
        let transform = figure_transform(figure, scale);
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
                let body = commands
                    .spawn((
                        Mesh3d(figure_mesh.0.clone()),
                        MeshMaterial3d(materials.squads[squad].clone()),
                        transform,
                        FigureBody,
                    ))
                    .id();
                // The outline: the same mesh, a shade larger, drawn with its
                // back faces only, so the figure covers the middle of it and
                // only the rim shows.
                commands.entity(body).with_children(|parent| {
                    parent.spawn((
                        Mesh3d(figure_mesh.0.clone()),
                        MeshMaterial3d(materials.outline.clone()),
                        Transform::from_scale(Vec3::splat(shell_scale)),
                        FigureOutline,
                    ));
                    // The base: the plate the piece stands on, and the
                    // shadow it drops.
                    parent.spawn((
                        Mesh3d(materials.base.clone()),
                        MeshMaterial3d(materials.shadow.clone()),
                        Transform::from_translation(Vec3::new(0.0, 0.01, 0.0)),
                        FigureBase,
                    ));
                    // The hat: the leader's mark. A leader you can only
                    // tell by its colour is not a leader you can tell apart
                    // at a glance.
                    //
                    // The hat is a brim: it sits on the head and its width
                    // is the look's, so the leader's mark is a parameter
                    // rather than a constant.
                    if figure.leader && look.figure.leader_hat {
                        let brim = look.figure.leader_hat_m / (HAT_BRIM_MESH_RADIUS * scale);
                        parent.spawn((
                            Mesh3d(materials.hat.clone()),
                            MeshMaterial3d(materials.leader_hat.clone()),
                            Transform::from_translation(Vec3::new(0.0, FIGURE_MESH_HEIGHT, 0.0))
                                .with_scale(Vec3::new(brim, 1.0, brim)),
                            LeaderHat,
                        ));
                    }
                });
                entities.0.insert(figure.id, body);
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
