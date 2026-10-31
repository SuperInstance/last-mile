//! The tag moment, in the 3D view: the splat of a hit, the pips over a
//! stunned agent, and the path of the agent being watched.
//!
//! These are the same effects the 2D map draws — one resource, two views —
//! so a hit looks like the same hit in both of them. Here they are pieces
//! of the diorama: the paint lands on the ground, the pips ring the head of
//! the agent that is out of the round, and the path is a line of marks
//! behind the agent being watched.
//!
//! Every one of them is a **pool**: a fixed number of pieces, waiting to be
//! shown. Nothing is spawned or despawned while the round plays, so nothing
//! is compiled mid-round and the frame time does not move.

use bevy::prelude::*;

use crate::input_plugin::Input;
use crate::look::Look;
use crate::look_effects::{Effects, MAX_STUN_PIPS};
use crate::view3d::agents::Figures;
use crate::view3d::View3dSettings;

/// How many dabs of paint the 3D view can show at once.
///
/// A hit is five dabs, so this is a handful of hits: more than a child
/// needs to read, and few enough that the board never turns to mud.
pub const SPLAT_POOL: usize = 40;

/// How many stun pips the 3D view can show at once: a ring of them over
/// every agent of the roster.
pub const PIP_POOL: usize = 96;

/// How many marks of the path the 3D view can show at once.
pub const TRAIL_POOL: usize = 48;

/// How high the dabs of paint sit off the ground, in metres.
const SPLAT_Y_M: f32 = 0.03;
/// How high the marks of the path sit off the ground, in metres.
const TRAIL_Y_M: f32 = 0.02;
/// How high the ring of pips sits over the head of an agent, in metres.
const PIP_Y_M: f32 = 0.42;
/// How wide the ring of pips is, in metres.
const PIP_RING_M: f32 = 0.26;
/// How big a pip is, in metres.
const PIP_SIZE_M: f32 = 0.09;

/// A piece of the effects: a dab, a pip or a mark of the path.
///
/// Which one a piece is, is what the pool it belongs to says: the pools
/// are the pieces' addresses, and the view asks the pool for the piece it
/// wants to show.
#[derive(Component)]
pub struct EffectPiece;

/// The pieces of the effects, and where they are.
///
/// The pools are fixed: the pieces are made at start-up and then only
/// shown and hidden. A frame that spawns and despawns would compile
/// pipelines in the middle of a round, and the frame time would jump.
#[derive(Resource, Default, Debug)]
pub struct EffectPools {
    /// The dabs of paint.
    pub splats: Vec<Entity>,
    /// The pips over the heads.
    pub pips: Vec<Entity>,
    /// The marks of the path.
    pub trail: Vec<Entity>,
}

/// The plugin that draws the effects in the 3D view.
pub struct Effects3dPlugin;

impl Plugin for Effects3dPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EffectPools>()
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                draw.run_if(super::in_round).run_if(super::showing_3d),
            );
    }
}

/// Make the pieces the effects are drawn with.
///
/// The meshes and the materials are made here, at start-up, so the
/// pipelines that draw them exist before the round does: nothing is
/// compiled mid-round.
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    settings: Res<View3dSettings>,
    mut pools: ResMut<EffectPools>,
) {
    // The dabs of paint: a flat disc on the ground, in the paint of the
    // hit. The disc is a cylinder of radius one half, so scaling it by the
    // radius of the dab gives the dab.
    let splat_mesh = meshes.add(Cylinder::new(0.5, 0.02).mesh().build());
    // The pips: little cubes.
    let pip_mesh = meshes.add(
        Cuboid::new(PIP_SIZE_M, PIP_SIZE_M, PIP_SIZE_M)
            .mesh()
            .build(),
    );
    // The marks of the path: small discs on the ground.
    // The marks of the path: small discs on the ground, near enough
    // together to read as a path and far enough apart to read as marks.
    let trail_mesh = meshes.add(Cylinder::new(0.11, 0.02).mesh().build());

    let look = &settings.look.0;
    let transparent = |paint: lm_render::look::Rgb| StandardMaterial {
        base_color: settings.look.bevy(paint),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    };

    for _ in 0..SPLAT_POOL {
        pools.splats.push(
            commands
                .spawn((
                    Mesh3d(splat_mesh.clone()),
                    MeshMaterial3d(materials.add(transparent(look.outline.color))),
                    Transform::from_xyz(0.0, SPLAT_Y_M, 0.0),
                    EffectPiece,
                    Visibility::Hidden,
                ))
                .id(),
        );
    }
    for _ in 0..PIP_POOL {
        pools.pips.push(
            commands
                .spawn((
                    Mesh3d(pip_mesh.clone()),
                    MeshMaterial3d(materials.add(transparent(look.ui.stunned))),
                    Transform::from_xyz(0.0, PIP_Y_M, 0.0),
                    EffectPiece,
                    Visibility::Hidden,
                ))
                .id(),
        );
    }
    for _ in 0..TRAIL_POOL {
        pools.trail.push(
            commands
                .spawn((
                    Mesh3d(trail_mesh.clone()),
                    MeshMaterial3d(materials.add(transparent(look.ui.accent))),
                    Transform::from_xyz(0.0, TRAIL_Y_M, 0.0),
                    EffectPiece,
                    Visibility::Hidden,
                ))
                .id(),
        );
    }
}

/// Draw the effects: the paint of the hits, the pips of the stunned, and
/// the path of the agent being watched.
#[allow(clippy::too_many_arguments)]
fn draw(
    mut pieces: Query<(
        &mut Transform,
        &MeshMaterial3d<StandardMaterial>,
        &mut Visibility,
        &EffectPiece,
    )>,
    mut assets: ResMut<Assets<StandardMaterial>>,
    mut effects: ResMut<Effects>,
    figures: Res<Figures>,
    pools: Res<EffectPools>,
    look: Res<Look>,
    input: Res<Input>,
    settings: Res<View3dSettings>,
) {
    let tokens = &settings.look.0;

    // The paint of the hits: a dab on the ground, spreading and fading as
    // it dries.
    for (slot, splat) in effects.splats.iter().enumerate() {
        let Some(&entity) = pools.splats.get(slot) else {
            break;
        };
        let Ok((mut transform, material, mut visibility, _)) = pieces.get_mut(entity) else {
            continue;
        };
        *visibility = Visibility::Visible;
        // The model says how big the dab is and how strong: the map and
        // the diorama paint the same hit.
        let radius = splat.radius();
        transform.translation = Vec3::new(splat.pos_m.0, SPLAT_Y_M, -splat.pos_m.1);
        transform.scale = Vec3::new(radius * 2.0, 1.0, radius * 2.0);
        if let Some(mut paint) = assets.get_mut(&material.0) {
            paint.base_color = settings.look.bevy(splat.paint).with_alpha(splat.opacity());
        }
    }
    for &entity in pools.splats.iter().skip(effects.splats.len()) {
        if let Ok((_, _, mut visibility, _)) = pieces.get_mut(entity) {
            *visibility = Visibility::Hidden;
        }
    }

    // The pips: a ring of them over the head of every agent that is out of
    // the round, one for each tick of the stun that is left.
    //
    // The ring is in the horizontal plane, so it reads as a row of pips from
    // the side and as a circle of them from above — a mark that says "this
    // one is out" from every angle the camera can be at.
    let mut pip_slot = 0usize;
    for figure in figures.0.iter() {
        let pips = figure.stun_ticks.min(MAX_STUN_PIPS);
        if pips == 0 {
            continue;
        }
        let paint = effects
            .stun_paint_of(figure.id)
            .unwrap_or(tokens.ui.stunned);
        let colour = settings.look.bevy(paint);
        for pip in 0..pips {
            let Some(&entity) = pools.pips.get(pip_slot) else {
                break;
            };
            let Ok((mut transform, material, mut visibility, _)) = pieces.get_mut(entity) else {
                continue;
            };
            pip_slot += 1;
            *visibility = Visibility::Visible;
            // The pips are spread around the head, one for each tick.
            let angle = pip as f32 / pips as f32 * std::f32::consts::TAU;
            let (sin, cos) = angle.sin_cos();
            transform.translation = Vec3::new(
                figure.pos_m.0 + cos * PIP_RING_M,
                PIP_Y_M + tokens.figure.height_m,
                -(figure.pos_m.1 + sin * PIP_RING_M),
            );
            transform.scale = Vec3::ONE;
            if let Some(mut pip) = assets.get_mut(&material.0) {
                pip.base_color = colour;
            }
        }
    }
    for &entity in pools.pips.iter().skip(pip_slot) {
        if let Ok((_, _, mut visibility, _)) = pieces.get_mut(entity) {
            *visibility = Visibility::Hidden;
        }
    }

    // The path of the agent being watched: the places it has been, frame
    // by frame. The host's own bookkeeping — the round is never asked to
    // remember.
    let watched = input.0.viewer.possessing().or(input.0.viewer.following());
    let pos = watched.and_then(|agent| {
        figures
            .0
            .iter()
            .find(|figure| figure.id == agent)
            .map(|figure| figure.pos_m)
    });
    effects.follow(watched, pos);
    let colour = watched
        .and_then(|agent| {
            figures
                .0
                .iter()
                .find(|figure| figure.id == agent)
                .map(|figure| figure.squad)
        })
        .map_or(look.bevy(tokens.ui.accent), |squad| {
            look.bevy(look.0.squad_color(squad))
        });
    for (slot, (x, y)) in effects.trail.iter().enumerate() {
        let Some(&entity) = pools.trail.get(slot) else {
            break;
        };
        let Ok((mut transform, material, mut visibility, _)) = pieces.get_mut(entity) else {
            continue;
        };
        *visibility = Visibility::Visible;
        transform.translation = Vec3::new(*x, TRAIL_Y_M, -*y);
        transform.scale = Vec3::ONE;
        if let Some(mut mark) = assets.get_mut(&material.0) {
            mark.base_color = colour.with_alpha(0.5);
        }
    }
    for &entity in pools.trail.iter().skip(effects.trail.len()) {
        if let Ok((_, _, mut visibility, _)) = pieces.get_mut(entity) {
            *visibility = Visibility::Hidden;
        }
    }
}
