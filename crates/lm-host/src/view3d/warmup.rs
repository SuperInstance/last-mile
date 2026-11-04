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

use bevy::prelude::*;

use lm_core::fixed::UNITS_PER_M;
use lm_render::palette::MaterialClass;

use crate::sim_plugin::Round;
use crate::view3d::agents::{self, FigureMesh};
use crate::view3d::chunks::ChunkStore;
use crate::view3d::{View3dMaterials, View3dSettings, ViewStage};

/// How many frames the card is drawn before the round starts.
///
/// The pipelines compile inside the first drawn frame (synchronously); the
/// rest is the mesh and material assets settling on the GPU. At 60 fps the
/// whole card is a blink.
const CARD_FRAMES: u32 = 8;

/// How far the spectator's overview sits from the point it looks at, in
/// metres. Close enough that a 1.5 m figure is a readable body, far enough
/// that the view holds a good slice of the world.
const OVERVIEW_DISTANCE_M: f32 = 100.0;
/// The overview's pitch above the ground: about 41 degrees — steep enough
/// that a wall shows its sides, shallow enough that the world reads in
/// depth.
const OVERVIEW_PITCH_RAD: f32 = 0.72;

/// The weak ambient light of the round: enough that a lit figure's own
/// shadowed side is not a black hole, no more.
const AMBIENT_BRIGHTNESS: f32 = 40.0;

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
pub fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    settings: Res<View3dSettings>,
    round: Res<Round>,
    mut time: ResMut<Time<Virtual>>,
) {
    // The round is held while the card is up. `Time<Virtual>` paused means
    // `FixedUpdate` does not run: the sim does not tick behind a loading
    // screen.
    time.pause();

    let world = world_m(&round);
    let centre = bevy_xz((world.0 * 0.5, world.1 * 0.5));

    // The camera, with the scene's weak ambient on it, looking at the card
    // from the south and a little above.
    commands.spawn((
        Camera3d::default(),
        AmbientLight {
            brightness: AMBIENT_BRIGHTNESS,
            ..default()
        },
        Transform::from_translation(centre + Vec3::new(0.0, 7.0, 12.0)).looking_at(centre, Vec3::Y),
        ViewCamera,
    ));

    // The sun: flat unlit terrain ignores it, the lit figures read as solid
    // for it.
    commands.spawn((
        DirectionalLight::default(),
        Transform::from_rotation(Quat::from_euler(
            EulerRot::XYZ,
            -std::f32::consts::PI * 0.35,
            std::f32::consts::PI * 0.25,
            0.0,
        )),
    ));

    // The materials, fixed for the whole round: one white unlit chunk
    // material (the vertex colour carries every surface's identity), one lit
    // material per squad.
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
    commands.insert_resource(View3dMaterials {
        chunk: chunk.clone(),
        squads: squads.clone(),
    });

    // The one figure mesh every agent shares, instanced by the engine.
    let figure = meshes.add(agents::figure_mesh(&lm_render::figure::Figure::new()));
    commands.insert_resource(FigureMesh(figure.clone()));

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
    mut cameras: Query<&mut Transform, With<ViewCamera>>,
    cards: Query<Entity, With<WarmupCard>>,
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

    // The camera takes the spectator's overview: above and south of the
    // home squad, looking at it.
    let world = world_m(&round);
    let focus = focus_m(&round, world);
    let centre = bevy_xz(focus);
    store.focus_m = focus;
    let offset = Vec3::new(
        0.0,
        OVERVIEW_DISTANCE_M * OVERVIEW_PITCH_RAD.sin(),
        -OVERVIEW_DISTANCE_M * OVERVIEW_PITCH_RAD.cos(),
    );
    for mut transform in &mut cameras {
        *transform = Transform::from_translation(centre + offset).looking_at(centre, Vec3::Y);
    }

    // The round runs.
    time.unpause();
    *stage = ViewStage::Round;
    println!("lm-host: the 3D view is warm; the round starts");
}
