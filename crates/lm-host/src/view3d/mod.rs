//! The 3D view: the world of `lm-render` drawn by Bevy (phase 6 Tasks 6-8).
//!
//! The view is a *view*, not a second source of truth: it renders the same
//! [`crate::sim_plugin::Round`] the 2D map renders, and every decision about
//! *what* to draw — which chunks, which figures, which colour, whether the
//! round may start — is made by `lm-render`'s scene model, in plain data,
//! off the GPU. This module is the mapping: scene data in, entities out.
//! Any disagreement between the 2D map and the 3D view is a bug here.
//!
//! Two rules hold the whole thing together:
//!
//! * **Nothing is compiled mid-round.** The materials are fixed — one white
//!   unlit material for every chunk quad (the vertex colour carries every
//!   surface's identity, spec §6's clean proxy-friendly output), one lit
//!   material per squad — and they are warmed on a card the camera looks at
//!   *before* the round starts. See [`warmup`].
//! * **The entities are stable.** A chunk is one `Mesh3d` entity, added when
//!   its chunk enters the view and despawned when it leaves — never re-added
//!   every frame, or the pipeline cache churns. The agents are one entity
//!   each, moved by their `Transform` and instanced by the engine's own
//!   batching (there is no `Instance` component to write).
//!
//! Task 7's half is [`upscale`]: the world renders at a low internal
//! resolution and is drawn to the window at an integer, nearest-neighbour
//! scale. Task 8's half is here and in [`chunks`]: the fog of war is the
//! 2D map's own rule (`lm_render::fog`, one shared implementation), the
//! camera answers the same verbs the 2D map's does (WASD, drag, wheel,
//! follow), and `--view both` lays the HUD and the piano roll over this
//! view while `B` flips between it and the 2D map without touching the
//! round.

pub mod agents;
pub mod chunks;
pub mod effects;
pub mod upscale;
pub mod warmup;

use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::input::mouse::{MouseButton, MouseWheel};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use lm_core::fixed::UNITS_PER_M;
use lm_core::world::CellXY;
use lm_host_model::input::{InputEvent, InputState, Viewer};
use lm_render::palette::RenderConfig;
use lm_render::palette::{Palette, UpscaleMode};

use crate::keys::{FOLLOW, LET_GO, LET_GO_ESCAPE, PAN_BACK, PAN_FORWARD, PAN_LEFT, PAN_RIGHT};
use crate::sim_plugin::Round;

/// The stage of the 3D view: the warmup card, then the round.
///
/// A plain resource rather than a `States` machine — the crate needs exactly
/// one transition, and `bevy_state` is not among bevy's default features.
/// While the card is up, virtual time is paused: the round held behind it
/// starts from its first tick, after the pipelines exist.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Resource, Default)]
pub enum ViewStage {
    /// The warmup card is on screen and the pipelines are compiling.
    #[default]
    Warming,
    /// The round: the chunks and the figures.
    Round,
}

/// Whether the round is on — the run condition of the round's systems.
fn in_round(stage: Res<ViewStage>) -> bool {
    *stage == ViewStage::Round
}

/// Whether the 3D view is the one drawing.
///
/// `main` holds the view switch in its [`crate::Views`] wrapper when the
/// layout can flip (`--view both`); no wrapper means no flipping — the 3D
/// view is simply on. Every per-frame system of the view runs under this
/// condition, so flipping the switch pauses the 3D side without touching
/// the round.
pub fn showing_3d(views: Option<Res<crate::Views>>) -> bool {
    views.map(|views| views.0.showing_3d()).unwrap_or(true)
}

/// The settings of the 3D view: everything tunable, loaded once (D19).
#[derive(Clone, Debug, Resource)]
pub struct View3dSettings {
    /// The render configuration, from `render.toml` or its defaults.
    pub render: RenderConfig,
    /// The palette the configuration names.
    pub palette: Palette,
    /// The look: the colours, sizes and shadows every view of the game
    /// reads. The 3D view takes its sky, its light and its figures from it,
    /// so the diorama and the 2D map are the same world drawn twice.
    pub look: crate::look::Look,
}

impl View3dSettings {
    /// The settings from a `render.toml`, or the plan's defaults when there
    /// is no such file.
    ///
    /// A configuration the view cannot honour — DLSS without its project
    /// id, DLSS in a build without the feature — is an error here, at
    /// start-up, not a panic at the first frame.
    pub fn load(path: &std::path::Path, look: crate::look::Look) -> Result<Self, String> {
        let render = RenderConfig::load(path)?;
        lm_render::upscale::validate(&render)?;
        if render.upscale == UpscaleMode::Dlss && !cfg!(feature = "dlss") {
            return Err(String::from(
                "upscale = \"dlss\" needs a build with the dlss feature: \
                 rebuild with `--features lm-host/dlss` (RTX only), \
                 or set upscale to \"integer\" or \"off\"",
            ));
        }
        Ok(View3dSettings {
            palette: look.0.palette(),
            render,
            look,
        })
    }

    /// The sky: the world camera's clear colour, and the colour its
    /// distance fog ramps the far ground into.
    ///
    /// The sky is the look's own, so the far ground melts into a colour the
    /// file chose — not into a constant in the code.
    pub fn sky(&self) -> Color {
        self.look.bevy(self.look.0.world.sky)
    }
}

/// The materials of the 3D view: fixed, and few, on purpose.
///
/// Chunk quads are `unlit` and white — every surface's identity rides in the
/// mesh's vertex colours, so a future neural renderer can read the same
/// buffer and no texture is ever sampled. The figures are lit, in the
/// squads' own colours, so they read as solid bodies. The beacons are the
/// palette's own two beacon classes, unlit and flat like the terrain (Task
/// 8's reviewer finding 4: 3D shows the same game as 2D, beacons included).
/// Nothing here is created after startup, so nothing is compiled mid-round.
#[derive(Resource)]
pub struct View3dMaterials {
    /// The material every chunk quad wears: white, unlit.
    pub chunk: Handle<StandardMaterial>,
    /// The figure material of each squad.
    pub squads: [Handle<StandardMaterial>; 4],
    /// The marker of a beacon nobody holds: the palette's `beacon_free`.
    pub beacon_free: Handle<StandardMaterial>,
    /// The marker of a held beacon: the palette's `beacon_held`.
    pub beacon_held: Handle<StandardMaterial>,
    /// The dark shell that draws the outline around a figure.
    ///
    /// It is drawn with its back faces only, so the figure itself covers
    /// the middle of the shell and only the rim shows: the outline is the
    /// figure's own silhouette, drawn a little larger.
    pub outline: Handle<StandardMaterial>,
    /// The hat of the leader: the mark that tells a leader apart from the
    /// rest of the squad at a glance.
    pub leader_hat: Handle<StandardMaterial>,
    /// The shadow a figure drops on the ground.
    pub shadow: Handle<StandardMaterial>,
    /// The plate a figure stands on.
    pub base: Handle<Mesh>,
    /// The hat of the leader.
    pub hat: Handle<Mesh>,
}

/// The 3D camera rig: where it looks, from how far, at what angle, and
/// which agent it follows (Task 8).
///
/// The camera's `Transform` is derived from the rig every frame
/// ([`rig_transform`]), so the rig is the one place the camera is said.
/// The verbs are the 2D map's own, so the human does not learn two
/// schemes: WASD pans, the drag pans, the wheel zooms, `F` follows the
/// agent under the pointer, and possessing an agent follows it.
#[derive(Clone, Debug, Resource)]
pub struct CameraRig3d {
    /// The point of the world the camera looks at, in metres (x east,
    /// y south — the scene model's own coordinates).
    pub focus_m: (f32, f32),
    /// How far the camera sits from its focus, in metres.
    pub distance_m: f32,
    /// The camera's pitch above the ground, in radians.
    pub pitch_rad: f32,
    /// The camera's yaw around the focus, in radians (0 sits north of the
    /// focus, looking south).
    pub yaw_rad: f32,
    /// The agent the camera follows, when it follows one. Possession
    /// overrides it while an agent is played.
    pub follow: Option<u16>,
    /// Where the pointer went down, and whether it has become a drag.
    pub pointer: Pointer3d,
}

/// The pointer, as the 3D camera sees it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Pointer3d {
    /// Where the button went down, in window pixels, if it is down.
    pub pressed_at: Option<Vec2>,
    /// Whether the pointer has travelled far enough to be a drag.
    pub dragged: bool,
    /// Where the pointer was the last time it was seen.
    pub last: Option<Vec2>,
}

impl Default for CameraRig3d {
    fn default() -> Self {
        CameraRig3d {
            focus_m: (128.0, 128.0),
            distance_m: 120.0,
            pitch_rad: warmup::OVERVIEW_PITCH_RAD,
            yaw_rad: 0.0,
            follow: None,
            pointer: Pointer3d::default(),
        }
    }
}

/// How far a drag of one window pixel pans the ground, in metres.
///
/// Proportional to the camera's distance: a drag covers the same slice of
/// the world at any zoom.
const DRAG_METRES_PER_PX: f32 = 0.0016;
/// How fast the keys pan, as a fraction of the camera's distance a second.
const PAN_PER_DISTANCE: f32 = 0.9;
/// How much one notch of the wheel zooms.
const ZOOM_STEP: f32 = 1.15;
/// The closest and furthest the camera may sit, in metres.
const MIN_DISTANCE_M: f32 = 8.0;
const MAX_DISTANCE_M: f32 = 600.0;
/// How far a click may sit from an agent's feet and still pick it, in metres.
const PICK_RADIUS_M: f32 = 2.0;
/// How far the pointer may travel between the press and the release and
/// still count as a click, in pixels.
const CLICK_SLOP: f32 = 5.0;

/// A scene point as a Bevy point: x east, y up, z north.
pub fn bevy_xz(pos_m: (f32, f32)) -> Vec3 {
    Vec3::new(pos_m.0, 0.0, -pos_m.1)
}

/// Where the camera sits for the rig it holds.
/// Where the camera sits for the rig it holds.
///
/// Yaw 0 sits **north** of the focus (over the world's edge, when the
/// focus is near one), looking south — into the world. The sign matters
/// more than it looks: the home squad spawns at a corner, and a camera
/// south of it looks *across* the corner into the void, which Task 6's
/// navy-over-navy sky hid and the violet one exposed.
pub fn rig_transform(rig: &CameraRig3d) -> Transform {
    let focus = bevy_xz(rig.focus_m);
    let ground = rig.distance_m * rig.pitch_rad.cos();
    let offset = Vec3::new(
        ground * rig.yaw_rad.sin(),
        rig.distance_m * rig.pitch_rad.sin(),
        ground * rig.yaw_rad.cos(),
    );
    Transform::from_translation(focus + offset).looking_at(focus, Vec3::Y)
}

/// The fog viewer of this frame: whoever the host's input says is looking,
/// with a replay watched, never played.
pub fn fog_viewer(input: &InputState, round: &Round) -> lm_render::fog::FogViewer {
    if round.is_replay() {
        return lm_render::fog::FogViewer::Spectator;
    }
    match input.viewer {
        Viewer::Spectator { .. } => lm_render::fog::FogViewer::Spectator,
        Viewer::Possessing(agent) => lm_render::fog::FogViewer::Possessing(agent),
    }
}

/// The 3D view plugin: the warmup, the chunks, the figures, the upscale,
/// the camera.
pub struct View3dPlugin {
    /// The settings, loaded by `main` from `render.toml` (or its defaults).
    pub settings: View3dSettings,
}

impl Plugin for View3dPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.settings.clone())
            .init_resource::<ViewStage>()
            .init_resource::<warmup::WarmupFrames>()
            .insert_resource(warmup::WarmupModel(lm_render::scene::Warmup::new()))
            .init_resource::<chunks::ChunkStore>()
            .init_resource::<agents::FigureEntities>()
            .init_resource::<agents::BeaconEntities>()
            .init_resource::<agents::Figures>()
            .init_resource::<CameraRig3d>()
            // The tag moment: the paint of the hits, the pips of the
            // stunned, and the path of the agent being watched.
            .add_plugins(effects::Effects3dPlugin)
            // The retro-modern look starts in the sky's colour; the fog
            // ramps the far ground into it, and the letterbox shows it.
            .insert_resource(ClearColor(self.settings.sky()))
            .add_systems(
                Startup,
                // The upscale makes the picture before the camera that
                // renders into it exists.
                (upscale::setup, warmup::setup).chain(),
            )
            .add_systems(
                Update,
                (
                    // The warmup advances only while it holds; the round's
                    // systems run only once it has let go, and only while
                    // the 3D view is the view being drawn. The chain is the
                    // handover: the card clears in the same frame the stage
                    // flips, and the first round frame draws a warm world.
                    warmup::advance,
                    update_camera
                        .run_if(in_round)
                        .run_if(showing_3d)
                        .after(warmup::advance),
                    upscale::update,
                    chunks::sync.run_if(in_round).run_if(showing_3d),
                    agents::update_figures.run_if(in_round).run_if(showing_3d),
                    agents::update_beacons.run_if(in_round).run_if(showing_3d),
                ),
            );

        // DLSS, when this build has it (an optional cargo feature, off by
        // default: the SDK crate is fetched from crates.io, not the local
        // registry). The project id must exist before the init plugin, or
        // it panics; unsupported hardware degrades to off inside the
        // plugin, never fails the app.
        #[cfg(feature = "dlss")]
        if self.settings.render.upscale == UpscaleMode::Dlss {
            use dlss::{DlssInitPlugin, DlssPlugin, DlssProjectId};
            app.insert_resource(WantsDlss)
                .insert_resource(DlssProjectId(
                    self.settings
                        .render
                        .dlss_project_id
                        .parse()
                        .unwrap_or_default(),
                ))
                .add_plugins((DlssInitPlugin, DlssPlugin));
        }
    }
}

/// Whether the configuration asked for DLSS. The flag itself is
/// unconditional (so the warmup can take it as a plain system parameter);
/// only the DLSS work behind it is feature-gated.
#[derive(Resource)]
pub struct WantsDlss;

/// The DLSS wiring, compiled only in a build with the `dlss` feature.
#[cfg(feature = "dlss")]
mod dlss {
    pub use bevy::anti_alias::dlss::{DlssInitPlugin, DlssPlugin, DlssProjectId};
}

/// Drive the 3D camera: follow who is being played or watched, and answer
/// the 2D map's own verbs — WASD pans, the drag pans, the wheel zooms, `F`
/// follows the agent under the pointer, a click on the ground orders the
/// agent being played to walk there.
///
/// The camera's transform is the rig, derived, never free-floating, so
/// nothing else moves it; and the rig's focus feeds the visible-chunk set,
/// so the world grows around wherever the camera looks.
#[allow(clippy::too_many_arguments)]
pub fn update_camera(
    mut rig: ResMut<CameraRig3d>,
    mut store: ResMut<chunks::ChunkStore>,
    mut round: ResMut<Round>,
    settings: Res<View3dSettings>,
    mut input: ResMut<crate::input_plugin::Input>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<MouseWheel>,
    fixed: Res<Time<Fixed>>,
    time: Res<Time>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<warmup::ViewCamera>>,
    mut transforms: Query<&mut Transform, With<warmup::ViewCamera>>,
    mut fogs: Query<&mut DistanceFog, With<warmup::ViewCamera>>,
) {
    let Some(window) = windows.single().ok() else {
        return;
    };
    let cursor = window.cursor_position();

    // The pointer: a press that travels is a drag, a press that does not is
    // a click on the world.
    if mouse.just_pressed(MouseButton::Left) {
        rig.pointer.pressed_at = cursor;
        rig.pointer.dragged = false;
    }
    if mouse.pressed(MouseButton::Left) {
        if let (Some(start), Some(now)) = (rig.pointer.pressed_at, cursor) {
            if (now - start).length() > CLICK_SLOP {
                rig.pointer.dragged = true;
            }
        }
    }
    let mut clicked = false;
    if mouse.just_released(MouseButton::Left) {
        clicked = !rig.pointer.dragged;
        rig.pointer.pressed_at = None;
        rig.pointer.dragged = false;
    }

    // The point of the world under the cursor, when the cursor is over the
    // picture: through the blit and into the world camera's own viewport.
    let under_cursor = cursor.and_then(|cursor| {
        let (camera, transform) = cameras.single().ok()?;
        let (_, viewport) = upscale::cursor_blit(
            window,
            cursor,
            (
                settings.render.internal_width,
                settings.render.internal_height,
            ),
        )?;
        let ray = camera.viewport_to_world(transform, viewport).ok()?;
        let hit = ray.plane_intersection_point(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y))?;
        Some((hit.x, -hit.z))
    });

    // Everything the round is asked for, under the one borrow of it: the
    // world's extents, the agent `F` picks, the follow. The click's order
    // below needs the round *mutably*, so the borrow ends here.
    let mut followed_focus = None;
    let click_cell;
    let world_m;
    {
        let Some(sim) = round.sim() else {
            return;
        };
        let world = sim.world();
        let cell_m = world.cell_units as f32 / UNITS_PER_M as f32;
        world_m = (
            f32::from(world.grid.w) * cell_m,
            f32::from(world.grid.h) * cell_m,
        );

        // Who the camera follows: the agent being played or watched, when
        // the host's input knows one, and otherwise whoever `F` last
        // picked.
        let mut follow = rig.follow;
        if let Some(agent) = input.0.viewer.possessing().or(input.0.viewer.following()) {
            follow = Some(agent);
        }
        if keys.just_pressed(LET_GO_ESCAPE.key) || keys.just_pressed(LET_GO.key) {
            follow = None;
            rig.follow = None;
        }

        // `F` follows the agent under the pointer, the same key as the map.
        if keys.just_pressed(FOLLOW.key) {
            if let Some(under) = under_cursor {
                let picked = sim
                    .agent_ids()
                    .iter()
                    .filter_map(|&id| Some((id, sim.agent(id)?)))
                    .map(|(id, agent)| {
                        let (x, y) = agent.pos.to_units();
                        let (mx, my) =
                            (x as f32 / UNITS_PER_M as f32, y as f32 / UNITS_PER_M as f32);
                        (
                            id,
                            (mx - under.0) * (mx - under.0) + (my - under.1) * (my - under.1),
                        )
                    })
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                if let Some((id, square)) = picked {
                    if square <= PICK_RADIUS_M * PICK_RADIUS_M {
                        rig.follow = Some(id);
                        follow = Some(id);
                        // When the host's input runs (the `both` layout),
                        // the follow belongs to the viewer, so the HUD
                        // says it too.
                        input.0.viewer = Viewer::Spectator { follow: Some(id) };
                    }
                }
            }
        }

        // The cell a click lands on, if it lands on one.
        click_cell =
            under_cursor.and_then(|under| cell_of(under, cell_m, world.grid.w, world.grid.h));

        // Follow: the camera sits on the agent, lerped exactly as its
        // figure is.
        if let Some((id, pos)) = follow
            .filter(|&id| sim.agent(id).is_some())
            .map(|id| (id, sim.agent(id).expect("just checked").pos))
        {
            let from = round
                .prev_positions()
                .iter()
                .find(|&&(seen, _)| seen == id)
                .map_or(pos, |&(_, p)| p);
            followed_focus = Some(lm_render::scene::pos_m(
                from,
                pos,
                fixed.overstep_fraction(),
            ));
        }
    }

    // A click on the ground orders the agent being played to walk there —
    // the same order the 2D map's click makes, through the same sequencer.
    // Nobody being played means no order: the click is just a click.
    if clicked && click_cell.is_some() && input.0.viewer.possessing().is_some() {
        if let (Some(cell), Some(host)) = (click_cell, round.live.as_mut()) {
            if let Err(err) = input.0.apply(InputEvent::ClickCell(cell), host) {
                eprintln!("lm-host: {err}");
            }
        }
    }

    // Follow: the camera sits on the agent. Free: WASD, the drag, the wheel.
    match followed_focus {
        Some(focus) => {
            rig.focus_m = focus;
            rig.distance_m = rig.distance_m.clamp(MIN_DISTANCE_M, MAX_DISTANCE_M);
        }
        None => {
            // The pan directions: forward is into the screen on the
            // ground (the way the camera faces), right is across it.
            let forward = Vec2::new(-rig.yaw_rad.sin(), rig.yaw_rad.cos());
            let right = Vec2::new(-forward.y, forward.x);

            // The keys pan.
            let mut pan = Vec2::ZERO;
            if keys.pressed(PAN_FORWARD.key) {
                pan += forward;
            }
            if keys.pressed(PAN_BACK.key) {
                pan -= forward;
            }
            if keys.pressed(PAN_RIGHT.key) {
                pan += right;
            }
            if keys.pressed(PAN_LEFT.key) {
                pan -= right;
            }
            let speed = rig.distance_m * PAN_PER_DISTANCE;
            if pan != Vec2::ZERO {
                rig.focus_m.0 += pan.x * speed * time.delta_secs();
                rig.focus_m.1 += pan.y * speed * time.delta_secs();
            }

            // The drag pans, the world following the pointer.
            if rig.pointer.dragged {
                if let (Some(now), Some(previous)) = (cursor, rig.pointer.last) {
                    let delta = now - previous;
                    let metres = rig.distance_m * DRAG_METRES_PER_PX;
                    rig.focus_m.0 += (-right.x * delta.x + forward.x * delta.y) * metres;
                    rig.focus_m.1 += (-right.y * delta.x + forward.y * delta.y) * metres;
                }
            }
            rig.pointer.last = cursor;

            // The wheel zooms.
            let scroll: f32 = wheel.read().map(|wheel| wheel.y).sum();
            if scroll != 0.0 {
                rig.distance_m = (rig.distance_m * ZOOM_STEP.powf(-scroll))
                    .clamp(MIN_DISTANCE_M, MAX_DISTANCE_M);
            }
        }
    }

    // The focus stays inside the world, and the visible set grows around
    // wherever it is.
    rig.focus_m.0 = rig.focus_m.0.clamp(0.0, world_m.0);
    rig.focus_m.1 = rig.focus_m.1.clamp(0.0, world_m.1);
    store.focus_m = rig.focus_m;

    for mut transform in &mut transforms {
        *transform = rig_transform(&rig);
    }

    // The fog ramps over what the camera is looking at: the ground at the
    // focus stays clear, and the fog's own near/far are measured **past the
    // camera's distance**. The configuration's 40/120 m are ground-level
    // numbers — how far a figure sees — and a spectator's camera sitting
    // 100 m and more back would otherwise fog the whole board into the sky
    // before a single floor tile shows (the first Task 8 screenshots did
    // exactly that). This way the chunk set's far edge melts into the
    // background wherever the camera stands, which is what the soft edge
    // is for.
    for mut fog in &mut fogs {
        fog.falloff = FogFalloff::Linear {
            start: rig.distance_m + settings.render.fog_near_m,
            end: rig.distance_m + settings.render.fog_far_m,
        };
    }
}

/// The cell a point of the world sits in, when it sits in one.
fn cell_of(point_m: (f32, f32), cell_m: f32, w: u16, h: u16) -> Option<CellXY> {
    let x = point_m.0 / cell_m;
    let y = point_m.1 / cell_m;
    if x < 0.0 || y < 0.0 || x >= f32::from(w) || y >= f32::from(h) {
        return None;
    }
    Some(CellXY {
        x: x as u16,
        y: y as u16,
    })
}
