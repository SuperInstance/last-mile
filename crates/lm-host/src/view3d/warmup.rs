//! The warmup: the card that carries the pipelines (the `night-street`
//! lesson).
//!
//! `fleet/06-code-generated-3d.md` measured 35–40 s to the first frame on an
//! RTX 4060 — shader compilation, all of it. The 3D view turns that into a
//! loading screen: *before* the round starts, one box per material class and
//! one figure per squad stand where the camera looks, and the renderer
//! compiles everything they need. The round is held while they do — virtual
//! time is paused, so the round that follows starts from its first tick —
//! and starts only when the model says every material is compiled.
//!
//! The pipelines compile **synchronously**: `main` sets `RenderPlugin`'s
//! `synchronous_pipeline_compilation` for the 3D view, so the frame that
//! first draws the card blocks until its pipelines exist (`bevy_render`
//! exposes the state as `PipelineCache::get_render_pipeline_state`, a
//! `CachedPipelineState` that ends in `Ok(..)`). That drawn frame is the
//! proof of warmth: the materials are fixed — one white unlit chunk
//! material, one lit material per squad — so the round asks for no pipeline
//! the card did not already compile, and nothing stalls mid-round.
//!
//! The card itself doubles as a palette test: each class box carries its
//! class's colour as vertex colour through the chunk material — exactly how
//! chunk quads render — and each figure wears its squad's material, exactly
//! how the round's agents render.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use lm_core::fixed::UNITS_PER_M;
use lm_render::palette::MaterialClass;

use crate::sim_plugin::Round;
use crate::view3d::agents::{self, FigureMesh};
use crate::view3d::chunks::ChunkStore;
use crate::view3d::upscale;
use crate::view3d::{View3dMaterials, View3dSettings, ViewStage};

/// How many frames the card is drawn before the round starts.
///
/// The pipelines compile inside the first drawn frame (synchronously); the
/// rest is the mesh and material assets settling on the GPU. At 60 fps the
/// whole card is a blink.
const CARD_FRAMES: u32 = 8;

/// The overview's pitch above the ground: about 20 degrees.
///
/// Task 6's reviewer finding 2 — "walls read as flat tiles" — was in the
/// end the mesh, not the angle: the wall sides were being drawn a
/// millimetre tall, so no pitch could have shown them (the sides render
/// at their full height now, and they carry 62% of their colour besides,
/// so a wall reads as a box from any angle). What the pitch *is* for is
/// the top of the frame: the round's beacons stand a hundred metres and
/// more from the squads, which puts them a few degrees below the horizon,
/// and a 45°-field camera pitched 20° puts the horizon at its top edge —
/// the frame holds the ground, the fog that melts into the sky, and the
/// beacon towers over both, with no empty sky to waste (Task 8's
/// acceptance asks for a beacon tower in the opening frame).
pub const OVERVIEW_PITCH_RAD: f32 = 0.35;

/// How far the void ground spreads past the world, in metres: far enough
/// that a camera looking from beyond the world's edge sees ground below the
/// horizon rather than sky, near enough to stay inside the camera's far
/// plane.
const VOID_SPREAD_M: f32 = 400.0;

/// How far below the floor the void ground sits, in metres. Below the depth
/// buffer's reach at the world's far edge, above z-fighting with it.
const VOID_DROP_M: f32 = 0.2;

/// The ground the world floats on: one dark quad under everything, out past
/// the world's edges (Task 8's review: "add a ground plane").
///
/// It wears the chunk material and carries its colour as vertex colour —
/// the same pipeline the floor quads render through, so nothing is compiled
/// for it — and the distance fog ramps it into the sky like any ground, so
/// the world still melts into its background at the far edge. It carries no
/// information about the round: the fog of war rules the chunks, and the
/// void is merely not-sky.
#[derive(Component)]
pub struct VoidGround;

/// How many frames the card has been on screen.
#[derive(Resource, Default)]
pub struct WarmupFrames(pub u32);
/// The scene model's warmup: which materials are compiled, and whether the
/// round may start.
#[derive(Resource)]
pub struct WarmupModel(pub lm_render::scene::Warmup);

/// Something on the warmup card.
#[derive(Component)]
pub struct WarmupCard;

/// The 3D camera: the card and the round share it, and it moves between
/// them.
#[derive(Component)]
pub struct ViewCamera;

/// The world's extents in metres, or nothing when there is no world to draw.
fn world_m(round: &Round) -> (f32, f32) {
    round.sim().map_or((0.0, 0.0), |sim| {
        let world = sim.world();
        let cell_m = world.cell_units as f32 / UNITS_PER_M as f32;
        (
            f32::from(world.grid.w) * cell_m,
            f32::from(world.grid.h) * cell_m,
        )
    })
}

/// A world position in Bevy's coordinates: x east, y up, z north.
fn bevy_xz(pos_m: (f32, f32)) -> Vec3 {
    Vec3::new(pos_m.0, 0.0, -pos_m.1)
}

/// Where the view looks when the round starts: the middle of the **home
/// squad** — squad 0, the one the human plays by default — clamped into the
/// world.
///
/// The round's squads start at opposite corners of the map and take a while
/// to close, so the middle of *all* the agents is the empty middle of the
/// world: an overview centred there shows terrain and no bodies. Centred on
/// the home squad it shows figures, readable, from the first frame. A round
/// with no agents looks at the middle of the world.
fn focus_m(round: &Round, world: (f32, f32)) -> (f32, f32) {
    let centre = (world.0 * 0.5, world.1 * 0.5);
    let Some(sim) = round.sim() else {
        return centre;
    };
    let ids = sim.agent_ids();
    let Some(home) = ids
        .iter()
        .filter_map(|&id| sim.agent(id))
        .map(|agent| agent.squad)
        .min()
    else {
        return centre;
    };
    let mut x = 0i64;
    let mut y = 0i64;
    let mut count = 0i64;
    for id in &ids {
        let Some(agent) = sim.agent(*id) else {
            continue;
        };
        if agent.squad != home {
            continue;
        }
        let (px, py) = agent.pos.to_units();
        x += px;
        y += py;
        count += 1;
    }
    if count == 0 {
        return centre;
    }
    let metres = |units: i64| (units as f32 / count as f32) / UNITS_PER_M as f32;
    (metres(x).clamp(0.0, world.0), metres(y).clamp(0.0, world.1))
}

/// Set the view up: the held round, the camera, the lights, the materials,
/// the figure, and the card.
#[allow(clippy::too_many_arguments)]
pub fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    settings: Res<View3dSettings>,
    round: Res<Round>,
    targets: Res<upscale::UpscaleTargets>,
    wants: Option<Res<crate::view3d::WantsDlss>>,
    mut time: ResMut<Time<Virtual>>,
) {
    // The round is held while the card is up. `Time<Virtual>` paused means
    // `FixedUpdate` does not run: the sim does not tick behind a loading
    // screen.
    time.pause();

    let world = world_m(&round);
    let centre = bevy_xz((world.0 * 0.5, world.1 * 0.5));

    // The camera, with the scene's weak ambient on it, looking at the card
    // from the south and a little above. It renders into the upscale's
    // offscreen picture when the integer upscale is on (Task 7), below
    // every camera that draws to the window; and it carries the distance
    // fog (Task 8), which ramps the far chunks into the sky's own colour —
    // the fog colour may match the sky, the floor may not.
    let mut camera = commands.spawn((
        Camera3d::default(),
        Camera {
            order: -1,
            ..default()
        },
        // The render target is its own component in Bevy 0.20: the
        // upscale's offscreen picture, or the window by default.
        upscale::world_target(&targets),
        DistanceFog {
            color: settings.sky(),
            falloff: FogFalloff::Linear {
                start: settings.render.fog_near_m,
                end: settings.render.fog_far_m,
            },
            ..default()
        },
        // The weak ambient light of the round: enough that a lit figure's
        // own shadowed side is not a black hole, no more. How weak it is
        // is the look's business: the strength is in
        // `assets/look/look.toml`, under `[light]`.
        AmbientLight {
            brightness: settings.look.0.light.ambient_brightness,
            ..default()
        },
        Transform::from_translation(centre + Vec3::new(0.0, 7.0, 12.0)).looking_at(centre, Vec3::Y),
        ViewCamera,
    ));
    // DLSS rides the world camera, in a build that has it and a
    // configuration that asks for it; the flag is set at plugin build.
    if wants.is_some() {
        #[cfg(feature = "dlss")]
        camera.insert(bevy::anti_alias::dlss::Dlss::default());
        #[cfg(not(feature = "dlss"))]
        let _ = &mut camera;
    }

    // The sun: flat unlit terrain ignores it, the lit figures read as solid
    // for it. Its colour, its strength and where it stands are the look's.
    let light = settings.look.0.light;
    let sun_color = settings.look.bevy(light.sun_color);
    let rotation = light.sun_rotation;
    commands.spawn((
        DirectionalLight {
            color: sun_color,
            illuminance: light.sun_illuminance,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(
            EulerRot::XYZ,
            rotation[0],
            rotation[1],
            rotation[2],
        )),
    ));

    // The materials, fixed for the whole round: one white unlit chunk
    // material (the vertex colour carries every surface's identity), one lit
    // material per squad, and the palette's own two beacon classes for the
    // markers (Task 8's reviewer finding 4).
    let chunk = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
        ..default()
    });
    let squads = [0u8, 1, 2, 3].map(|squad| {
        let color = settings.palette.squad_color(squad);
        materials.add(StandardMaterial {
            base_color: Color::linear_rgba(color.r, color.g, color.b, color.a),
            ..default()
        })
    });
    let mut beacon_of = |class: lm_render::palette::MaterialClass| {
        let color = settings.palette.class_color(class);
        materials.add(StandardMaterial {
            base_color: Color::linear_rgba(color.r, color.g, color.b, color.a),
            unlit: true,
            // A beacon is the round's common knowledge — the 2D map draws
            // every one of them for every viewer — so the 3D view does not
            // fog its markers: a tower seen across the field, the way the
            // map's diamond is. (Task 8's acceptance asks for a beacon
            // tower in the opening frame, and the round's beacons stand a
            // hundred metres and more from the squads.)
            fog_enabled: false,
            ..default()
        })
    };
    let beacon_free = beacon_of(lm_render::palette::MaterialClass::BeaconFree);
    let beacon_held = beacon_of(lm_render::palette::MaterialClass::BeaconHeld);
    // The plate a piece stands on, and the hat the leader wears. Both are
    // made here, at start-up, so the pipelines that draw them exist before
    // the round does: nothing is compiled mid-round.
    let base = meshes.add(Cuboid::new(1.5, 0.03, 1.5).mesh().build());
    // The leader's hat: a brim, of the radius the look says, sitting on
    // the head. Its own height is small: the mark is the width, not the
    // height.
    let hat = meshes.add(
        Cylinder::new(agents::HAT_BRIM_MESH_RADIUS, 0.08)
            .mesh()
            .build(),
    );
    // The outline: the figure's own mesh, a shade larger, drawn with its
    // back faces only. The figure covers the middle of the shell, so only
    // the rim shows — a dark edge that separates a piece from the board it
    // stands on. It is unlit, so the edge is the same dark everywhere.
    let outline = materials.add(StandardMaterial {
        base_color: settings.look.bevy(settings.look.0.outline.color),
        unlit: true,
        // The back faces only: the figure covers the middle of the shell,
        // so only the rim of it shows.
        cull_mode: Some(bevy::render::render_resource::Face::Front),
        ..default()
    });
    // The leader's hat: the mark that tells a leader apart from the rest of
    // the squad at a glance, in the look's own leader colour.
    let leader_hat = materials.add(StandardMaterial {
        base_color: settings.look.bevy(settings.look.0.ui.leader),
        unlit: true,
        ..default()
    });
    // The shadow under a figure: a flat disc on the ground, in the outline's
    // ink, at the look's own strength.
    let shadow = materials.add(StandardMaterial {
        base_color: settings.look.bevy(
            settings
                .look
                .0
                .outline
                .color
                .alpha(settings.look.0.light.shadow_alpha),
        ),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    commands.insert_resource(View3dMaterials {
        chunk: chunk.clone(),
        squads: squads.clone(),
        beacon_free: beacon_free.clone(),
        beacon_held: beacon_held.clone(),
        outline: outline.clone(),
        leader_hat: leader_hat.clone(),
        shadow: shadow.clone(),
        base: base.clone(),
        hat: hat.clone(),
    });

    // The one figure mesh every agent shares, instanced by the engine, and
    // the one marker mesh every beacon shares: a slim tower, taller than
    // the walls, in the beacon class's colour — a beacon is the thing the
    // round plays toward, and it reads from across the field.
    let figure = meshes.add(agents::figure_mesh(&lm_render::figure::Figure::new()));
    commands.insert_resource(FigureMesh(figure.clone()));
    let marker = meshes.add(Cuboid::new(1.2, 7.2, 1.2).mesh().build());
    let void_ground = settings.look.0.world.void.to_linear();
    commands.insert_resource(agents::BeaconMesh(marker.clone()));

    // The ground under the world, out past its edges: the world floats on
    // the void, and the camera never sees sky below the horizon.
    commands.spawn((
        Mesh3d(meshes.add(void_ground_mesh(world, void_ground))),
        MeshMaterial3d(chunk.clone()),
        Transform::from_translation(centre + Vec3::new(0.0, -VOID_DROP_M, 0.0)),
        VoidGround,
    ));

    // The card: every material the round draws with, on it. The class boxes
    // wear the chunk material and carry their class's colour as vertex
    // colour — the same pipeline the chunks render through — and the figures
    // wear their squad's material, the same pipeline the agents render
    // through.
    let classes = [
        MaterialClass::Floor,
        MaterialClass::Wall,
        MaterialClass::Cover,
        MaterialClass::BeaconHeld,
        MaterialClass::BeaconFree,
    ];
    for (index, class) in classes.into_iter().enumerate() {
        let color = settings.palette.class_color(class);
        let mut card = Cuboid::new(1.4, 1.4, 1.4).mesh().build();
        let vertices = card.count_vertices();
        // The mesh was built this line; it cannot have been extracted yet,
        // so the error the `try` form reports cannot happen.
        let _ = card.try_insert_attribute(
            Mesh::ATTRIBUTE_COLOR,
            vec![[color.r, color.g, color.b, color.a]; vertices],
        );
        let east = (index as f32 - 2.0) * 2.2;
        commands.spawn((
            Mesh3d(meshes.add(card)),
            MeshMaterial3d(chunk.clone()),
            Transform::from_translation(centre + Vec3::new(east, 0.7, 0.0)),
            WarmupCard,
        ));
    }
    for (squad, material) in squads.iter().enumerate() {
        let east = (squad as f32 - 1.5) * 2.2;
        commands.spawn((
            Mesh3d(figure.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(centre + Vec3::new(east, 0.0, -3.0)),
            WarmupCard,
        ));
    }

    // The beacons' own materials ride the card too, so a beacon changing
    // hands mid-round compiles nothing.
    for (index, material) in [beacon_free.clone(), beacon_held.clone()]
        .into_iter()
        .enumerate()
    {
        let east = (index as f32 - 0.5) * 2.2;
        commands.spawn((
            Mesh3d(marker.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(centre + Vec3::new(east, 3.6, -6.0)),
            WarmupCard,
        ));
    }
}

/// The void ground as a Bevy `Mesh`: one quad, centred on the world, in the
/// void colour, facing up.
///
/// Built like a chunk's quads are — flat normal, one flat colour on every
/// corner — so it renders through the chunk material and its pipeline
/// without a thing of its own.
fn void_ground_mesh(world_m: (f32, f32), color: lm_render::palette::Color) -> Mesh {
    let half = VOID_SPREAD_M + world_m.0.max(world_m.1) * 0.5;
    let corners = [
        (-half, 0.0, half),
        (half, 0.0, half),
        (half, 0.0, -half),
        (-half, 0.0, -half),
    ];
    let mut out = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    // The mesh is built this call; it cannot have been extracted yet, so
    // the error the `try` form reports cannot happen.
    let _ = out.try_insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        corners.iter().map(|p| [p.0, p.1, p.2]).collect::<Vec<_>>(),
    );
    let _ = out.try_insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; corners.len()]);
    let _ = out.try_insert_attribute(
        Mesh::ATTRIBUTE_COLOR,
        vec![[color.r, color.g, color.b, color.a]; corners.len()],
    );
    out.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    out
}

/// Wait out the card, then hand the view to the round: the card goes, the
/// camera takes the spectator's overview, and the round runs — from its
/// first tick, with every pipeline it will ask for already compiled.
#[allow(clippy::too_many_arguments)]
pub fn advance(
    mut commands: Commands,
    mut stage: ResMut<ViewStage>,
    mut frames: ResMut<WarmupFrames>,
    mut warmup: ResMut<WarmupModel>,
    mut time: ResMut<Time<Virtual>>,
    round: Res<Round>,
    mut store: ResMut<ChunkStore>,
    mut rig: ResMut<crate::view3d::CameraRig3d>,
    mut cameras: Query<&mut Transform, With<ViewCamera>>,
    cards: Query<Entity, With<WarmupCard>>,
    settings: Res<View3dSettings>,
) {
    if *stage != ViewStage::Warming {
        return;
    }
    frames.0 += 1;
    if frames.0 < CARD_FRAMES {
        return;
    }

    // The card has been drawn, and its frames compiled synchronously: every
    // material on it is compiled, and the card holds every material the
    // round draws with. The model says so, and the round may start.
    warmup.0.mark_all_compiled();
    debug_assert!(
        warmup.0.ready(),
        "the card holds every material the round draws with"
    );

    // The card goes.
    for entity in &cards {
        commands.entity(entity).despawn();
    }

    // The rig takes the opening overview: the middle of the **home
    // squad**, far enough back that every one of its figures is in frame
    // (Task 6's reviewer finding 3), yawed so the camera looks past the
    // squad **into** the world — behind the squad from the nearest beacon,
    // so the opening frame is the squad and the tower it plays toward
    // (Task 8's review: a beacon tower in frame, and no camera looking
    // across a corner into the void). The camera system holds the camera
    // to the rig from here on; setting it once here keeps the handover
    // frame from being the one frame behind.
    let world = world_m(&round);
    let focus = focus_m(&round, world);
    let home = round.sim().and_then(|sim| {
        sim.agent_ids()
            .iter()
            .filter_map(|&id| sim.agent(id))
            .map(|agent| agent.squad)
            .min()
    });
    let radius = home
        .and_then(|squad| lm_render::scene::squad_radius_m(round.sim()?, squad))
        .unwrap_or(0.0);
    let beacons = round
        .sim()
        .map(lm_render::scene::beacons)
        .unwrap_or_default();
    rig.focus_m = focus;
    rig.distance_m = lm_render::scene::overview_distance(radius);
    // The tilt is the look's: a diorama is looked at from above, and how
    // far above is a thing a file says, not a thing the code knows.
    rig.pitch_rad = settings.look.0.camera.pitch_rad;
    rig.yaw_rad = lm_render::scene::opening_yaw(focus, &beacons, world);
    store.focus_m = focus;
    for mut transform in &mut cameras {
        *transform = crate::view3d::rig_transform(&rig);
    }

    // The round runs.
    time.unpause();
    *stage = ViewStage::Round;
    println!("lm-host: the 3D view is warm; the round starts");
}
