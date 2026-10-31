//! The map: the world as the viewer is allowed to see it, drawn.
//!
//! Three layers, each with one job:
//!
//! * the **tile layer** -- one sprite per cell of the grid, drawn once when
//!   the round starts. The terrain is in it; the fog is a second sprite over
//!   it, and only the cells whose fog changed are repainted.
//! * the **bodies** -- the agents as coloured discs and the beacons as
//!   diamonds, moved and recoloured every frame.
//! * the **camera** -- it follows the agent being played or watched, and it
//!   is free (WASD, drag, wheel) when nobody is being followed.
//!
//! Positions come from [`MapModel`], which is the only place besides this
//! crate where a position becomes a float, and they never travel back into
//! the simulation (D8). The host draws; it never rules.

use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::PrimaryWindow;
use bevy::{asset::RenderAssetUsages, image::ImageSampler};
use lm_core::fixed::UNITS_PER_M;
use lm_core::header::Role;
use lm_core::utterance::Channel;
use lm_core::world::{CellKind, CellXY};
use lm_host_model::input::Viewer;
use lm_host_model::map::{FogState, MapModel};

use crate::input_plugin::Input;
use crate::look::Look;
use crate::look_effects::{Effects, MAX_STUN_PIPS};
use crate::sim_plugin::Round;

// The colours of the world are the look's, not the map's: the map and the
// 3D view are the same world drawn twice, and they take their colours from
// the same file (`assets/look/look.toml`).

/// The colour of a squad.
pub fn squad_color(look: &Look, squad: u8) -> Color {
    look.bevy(look.squad_color(squad))
}

/// The colour of the terrain of a cell.
///
/// A cell that lies between two buildings is a street, and a street is
/// painted in the road's colour: the city reads as blocks with roads
/// between them, the same way it reads in the 3D view.
pub fn cell_color(look: &Look, kind: CellKind, street: bool) -> Color {
    let color = match kind {
        CellKind::Floor => {
            if street {
                look.0.world.road
            } else {
                look.0.world.ground
            }
        }
        CellKind::Wall => look.0.world.building,
        CellKind::Cover => look.0.world.cover,
    };
    look.bevy(color)
}

/// The colour of the fog over a cell.
pub fn fog_color(look: &Look, state: FogState) -> Color {
    let color = match state {
        FogState::Visible => lm_render::look::Rgb::new(0.0, 0.0, 0.0).alpha(0.0),
        FogState::Explored => look.0.world.fog_explored,
        FogState::Unknown => look.0.world.fog_unknown,
    };
    look.bevy(color)
}

/// The colour of a word that is spoken on each channel.
///
/// The channel is the shape of the line, so it is the colour of the tick:
/// a shout is loud and bright, a whisper is quiet and dim, and the radio is
/// the squad's own colour.
const CHANNEL_SHOUT: Color = Color::srgb(1.0, 0.78, 0.32);
const CHANNEL_VOICE: Color = Color::srgb(0.95, 0.95, 0.98);
const CHANNEL_WHISPER: Color = Color::srgb(0.76, 0.84, 1.0);
const CHANNEL_RADIO: Color = Color::srgb(0.42, 0.92, 0.62);

/// The colour of a word that has not been said yet.
///
/// It is dim, but not gone: a word the speaker has not reached yet is
/// still a word of the line.
const UNSPOKEN_ALPHA: f32 = 0.45;

/// A colour of the map as an egui colour: the panels speak in the colours of
/// the world.
pub fn egui_color(color: Color) -> bevy_egui::egui::Color32 {
    let srgba = color.to_srgba();
    bevy_egui::egui::Color32::from_rgba_unmultiplied(
        (srgba.red * 255.0).round() as u8,
        (srgba.green * 255.0).round() as u8,
        (srgba.blue * 255.0).round() as u8,
        (srgba.alpha * 255.0).round() as u8,
    )
}

/// How much larger the dark edge around a body is than the body itself.
///
/// The edge is a disc of its own behind the body, so the body covers the
/// middle of it and only the rim shows: a piece on a board, separated from
/// the board by a dark edge.
const EDGE_SCALE: f32 = 1.22;

/// How much larger the leader's halo is than the body itself.
const HALO_SCALE: f32 = 1.62;

/// How big an agent is drawn, in metres.
const AGENT_RADIUS_M: f32 = 1.1;
/// Where a splat is drawn: above the terrain and the fog, below the
/// bodies, so a hit is seen and never covers an agent.
const SPLAT_Z: f32 = 0.08;
/// How big a beacon is drawn, in metres.
const BEACON_RADIUS_M: f32 = 1.7;
/// The smallest an agent is drawn, in pixels: a body has to stay readable
/// when the whole 256 m world is on the screen. It is a piece on a board,
/// and it is drawn bigger than its cell so that it can be found.
const MIN_AGENT_PX: f32 = 12.0;
/// The smallest a beacon is drawn, in pixels.
const MIN_BEACON_PX: f32 = 13.0;
/// How long a speech tick is, in metres.
const TICK_LENGTH_M: f32 = 2.2;
/// How far above the body the speech ticks start, in metres.
const TICK_GAP_M: f32 = 0.6;
/// How far apart two words of a line are drawn, in metres.
const TICK_SPACING_M: f32 = 0.9;
/// The smallest a speech tick is drawn, in pixels: a word has to stay
/// readable when the whole world is on the screen.
const MIN_TICK_PX: f32 = 12.0;
/// How closely the camera follows an agent, in pixels per metre.
const FOLLOW_PX_PER_M: f32 = 9.0;
/// The closest the camera may come, in pixels per metre.
const MIN_PX_PER_M: f32 = 0.2;
/// The furthest the camera may go, in pixels per metre.
const MAX_PX_PER_M: f32 = 40.0;

/// The layers of the map, from the ground up.
const TERRAIN_Z: f32 = 0.0;
const FOG_Z: f32 = 0.05;
const AGENT_Z: f32 = 0.10;
const BEACON_Z: f32 = 0.15;

/// The map as the last frame drew it.
#[derive(Resource, Default)]
pub struct CurrentMap(pub Option<MapModel>);

/// The tile layer: the terrain, the fog over it, and the fog as drawn.
#[derive(Resource, Default)]
pub struct TileLayer {
    /// The fog of every cell as it was last drawn.
    pub drawn: Vec<FogState>,
    /// The width of the grid, in cells.
    pub grid_w: u16,
    /// The height of the grid, in cells.
    pub grid_h: u16,
    /// The size of a cell, in metres.
    pub cell_m: f32,
    /// The size of the world, in metres.
    pub world_m: Vec2,
}

/// What a body on the map is.
///
/// Every sprite the map draws carries one of these, so that the drawing is a
/// single query over the bodies rather than three that would fight over the
/// same component.
#[derive(Component)]
enum Body {
    /// The fog over a cell of the grid, in row-major order.
    Fog(usize),
    /// An agent, by its id.
    Agent(u16),
    /// The dark edge around an agent, by its id.
    ///
    /// The edge is a disc of its own, a shade larger than the body and
    /// behind it: the body covers the middle of it and only the rim shows.
    /// A piece on a board, separated from the board by a dark edge.
    AgentEdge(u16),
    /// The halo around the leader of a squad, by its id.
    AgentHalo(u16),
    /// A beacon, by its order in the round.
    Beacon(u8),
}

/// One blob of a splat: a dab of paint where an agent was hit.
///
/// The blob is a marker only: what it holds lives in the round's effects,
/// which the 2D map and the 3D view both read. A dab that the map knew
/// about and the 3D view did not would be two different hits.
#[derive(Component, Clone, Copy, Debug)]
pub struct SplatBlob;

/// How many dabs of paint the map can show at once.
///
/// A hit is five dabs, so this is a handful of hits: more than a child
/// needs to read, and few enough that the board never turns to mud.
pub const SPLAT_POOL: usize = 40;

/// The layout of the window: what the panels take and what is left for the
/// map.
#[derive(Resource, Clone, Copy, Debug)]
pub struct Layout {
    /// The size of the window, in pixels.
    pub size: Vec2,
    /// The height of the top bar, in pixels.
    ///
    /// It is the whole of the top: the bar with the round's own numbers and
    /// the row under it with the talk's.
    pub top: f32,
    /// The height of the bar with the round's numbers, in pixels.
    pub bar: f32,
    /// The height of the row with the talk's numbers, in pixels.
    pub talk: f32,
    /// The height of the bottom panel, in pixels.
    pub bottom: f32,
}

impl Default for Layout {
    fn default() -> Self {
        Layout {
            size: Vec2::new(1280.0, 720.0),
            bar: top_height(1280.0),
            talk: talk_height(1280.0),
            top: top_height(1280.0) + talk_height(1280.0),
            bottom: bottom_height(720.0),
        }
    }
}

impl Layout {
    /// The size of the area the map is drawn in.
    pub fn map_size(&self) -> Vec2 {
        Vec2::new(self.size.x, (self.size.y - self.top - self.bottom).max(1.0))
    }
}

/// The height of the top bar: a narrow window gets two rows of it.
fn top_height(width: f32) -> f32 {
    if width < 900.0 {
        46.0
    } else {
        28.0
    }
}

/// The height of the row with the talk's numbers.
///
/// The talk's counters are their own row: they are the numbers of the
/// round's talk, and they are read together, not mixed in with the round's
/// own.
fn talk_height(width: f32) -> f32 {
    if width < 900.0 {
        40.0
    } else {
        22.0
    }
}

/// The height of the piano roll: a quarter of the window, within reason.
fn bottom_height(height: f32) -> f32 {
    (height * 0.25).clamp(70.0, 150.0)
}

/// The height of the conductor's score: more of the window, because it
/// stacks four staves per agent where the roll draws one lane, and the
/// words and marks on the voice staff have to stay readable.
fn score_height(height: f32) -> f32 {
    (height * 0.40).clamp(150.0, 300.0)
}

/// The camera: where it looks, and how closely.
#[derive(Resource, Debug)]
pub struct CameraRig {
    /// The point of the world it looks at, in Bevy coordinates.
    pub focus: Vec2,
    /// The zoom the person asked for, in pixels per metre. `None` means the
    /// default for the mode: close on the agent being watched, or the whole
    /// world when the camera is free.
    pub zoom: Option<f32>,
    /// The agent the camera follows.
    pub follow: Option<u16>,
    /// The zoom in force, in pixels per metre.
    pub px_per_m: f32,
}

impl Default for CameraRig {
    fn default() -> Self {
        CameraRig {
            focus: Vec2::ZERO,
            zoom: None,
            follow: None,
            px_per_m: 1.0,
        }
    }
}

/// The systems of the map, as one set.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum MapSet {
    /// The map is built, drawn and looked at.
    Map,
}

/// The map's own camera — the one this plugin moves.
///
/// The marker matters the moment a second view shares the app (`--view
/// both`): the 3D view's world camera and its blit camera are cameras too,
/// and this plugin's controller must never touch them. The first `both`
/// layout dragged the 3D camera to the ground plane every frame — its
/// queries said `With<Camera>` and every camera answered.
#[derive(Component)]
pub struct MapCamera;

/// The bottom panel's mode, as a resource the model's own switch rides in
/// (the model crate knows no Bevy, so the host wraps it — the same shape
/// as the view switch's wrapper).
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct Bottom(pub lm_host_model::view::BottomSwitch);

impl Bottom {
    /// The mode the panel is in.
    pub fn mode(&self) -> lm_host_model::view::BottomMode {
        self.0.mode
    }
}

/// The map plugin: the world, the bodies on it, and the camera.
pub struct MapPlugin;

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CurrentMap>()
            .init_resource::<TileLayer>()
            .init_resource::<Layout>()
            .init_resource::<CameraRig>()
            // The bottom panel's mode sizes it (the score is the taller of
            // the two), so the map keeps its share of the window in step
            // with whichever is drawing.
            .init_resource::<Bottom>()
            .add_systems(Startup, spawn_world)
            .add_systems(
                Update,
                (update_layout, build_map, update_camera, draw_map)
                    .chain()
                    .in_set(MapSet::Map),
            );
    }
}

/// Spawn the camera, the tile layer and the bodies.
fn spawn_world(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    round: Res<Round>,
    look: Res<Look>,
    mut layer: ResMut<TileLayer>,
    mut rig: ResMut<CameraRig>,
) {
    // The camera: bevy_egui hangs its context on the first camera, so it has
    // to exist before the panels are drawn -- even when the file that was
    // asked for could not be opened and there is no world to draw.
    commands.spawn((Camera2d, MapCamera));

    let Some(sim) = round.sim() else {
        return;
    };
    let world = sim.world();
    let grid = &world.grid;
    let cell_m = world.cell_units as f32 / UNITS_PER_M as f32;

    // The discs the agents are drawn with: one white disc, tinted per
    // agent — and the splats, which are dabs of the same disc.
    let disc = images.add(disc_image());

    // The tile layer: the terrain, and the fog over it.
    let cells = usize::from(grid.w) * usize::from(grid.h);
    for y in 0..grid.h {
        for x in 0..grid.w {
            let cell = CellXY { x, y };
            let centre = cell_centre(cell, cell_m);
            let index = usize::from(y) * usize::from(grid.w) + usize::from(x);
            let kind = grid.get(cell);
            commands.spawn((
                Sprite::from_color(
                    cell_color(&look, kind, lm_render::street::is_street(world, cell)),
                    Vec2::splat(cell_m),
                ),
                Transform::from_translation(centre),
            ));
            commands.spawn((
                Sprite::from_color(fog_color(&look, FogState::Visible), Vec2::splat(cell_m)),
                Transform::from_translation(centre + Vec3::new(0.0, 0.0, FOG_Z)),
                Body::Fog(index),
            ));
        }
    }

    // The agents: a disc each, with the dark edge around it and — for the
    // leader — the halo that says which one leads.
    for id in sim.agent_ids() {
        let agent = sim.agent(id);
        let squad = agent.map_or(0, |agent| agent.squad);
        let leader = agent.is_some_and(|agent| agent.role == Role::Leader);
        commands.spawn((
            Sprite {
                image: disc.clone(),
                custom_size: Some(Vec2::splat(AGENT_RADIUS_M * 2.0)),
                color: squad_color(&look, squad),
                ..default()
            },
            Transform::from_translation(Vec3::ZERO),
            Body::Agent(id),
        ));
        // The edge: the same disc, larger, in the ink of the picture.
        commands.spawn((
            Sprite {
                image: disc.clone(),
                custom_size: Some(Vec2::splat(AGENT_RADIUS_M * 2.0 * EDGE_SCALE)),
                color: look.bevy(look.0.outline.color),
                ..default()
            },
            Transform::from_translation(Vec3::ZERO),
            Body::AgentEdge(id),
        ));
        // The halo: the leader's mark, a disc again, in the look's leader
        // colour. A leader you can only tell by a ring is a leader you have
        // to look for.
        if leader {
            commands.spawn((
                Sprite {
                    image: disc.clone(),
                    custom_size: Some(Vec2::splat(AGENT_RADIUS_M * 2.0 * HALO_SCALE)),
                    color: look.bevy(look.0.ui.leader),
                    ..default()
                },
                Transform::from_translation(Vec3::ZERO),
                Body::AgentHalo(id),
            ));
        }
    }

    // The beacons: a diamond each (a square, turned on its corner).
    for index in 0..world.beacons.len() {
        commands.spawn((
            Sprite::from_color(
                look.bevy(look.0.world.beacon_free),
                Vec2::splat(BEACON_RADIUS_M * 2.0),
            ),
            Transform {
                rotation: Quat::from_rotation_z(std::f32::consts::FRAC_PI_4),
                ..default()
            },
            Body::Beacon(index as u8),
        ));
    }

    // The dabs of paint: a pool, waiting for hits. They are hidden until a
    // hit lands, and then the effects say which of them is showing what.
    for _ in 0..SPLAT_POOL {
        commands.spawn((
            Sprite {
                image: disc.clone(),
                custom_size: Some(Vec2::ZERO),
                color: Color::WHITE,
                ..default()
            },
            Transform::from_translation(Vec3::ZERO),
            SplatBlob,
            Visibility::Hidden,
        ));
    }

    layer.drawn = vec![FogState::Visible; cells];
    layer.grid_w = grid.w;
    layer.grid_h = grid.h;
    layer.cell_m = cell_m;
    layer.world_m = Vec2::new(f32::from(grid.w) * cell_m, f32::from(grid.h) * cell_m);

    // The camera starts on the middle of the world.
    rig.focus = Vec2::new(layer.world_m.x * 0.5, -layer.world_m.y * 0.5);
}

/// The disc the agents are drawn with: a white circle with a soft edge, so
/// that a tint keeps the shape of a body.
fn disc_image() -> Image {
    const SIZE: u32 = 64;
    let mut data = vec![0u8; (SIZE * SIZE * 4) as usize];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = (x as f32 + 0.5) / SIZE as f32 - 0.5;
            let dy = (y as f32 + 0.5) / SIZE as f32 - 0.5;
            let distance = (dx * dx + dy * dy).sqrt();
            let alpha = ((0.5 - distance) * 2.0).clamp(0.0, 1.0);
            let pixel = ((y * SIZE + x) * 4) as usize;
            data[pixel] = 255;
            data[pixel + 1] = 255;
            data[pixel + 2] = 255;
            data[pixel + 3] = (alpha * 255.0).round() as u8;
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::linear();
    image
}

/// The centre of a cell, in Bevy coordinates: east is +x and south is -y.
fn cell_centre(cell: CellXY, cell_m: f32) -> Vec3 {
    Vec3::new(
        (f32::from(cell.x) + 0.5) * cell_m,
        -(f32::from(cell.y) + 0.5) * cell_m,
        TERRAIN_Z,
    )
}

/// A point of the world, in Bevy coordinates.
fn bevy_point(pos_m: (f32, f32)) -> Vec2 {
    Vec2::new(pos_m.0, -pos_m.1)
}

/// The layout of the window, as it is now.
fn update_layout(
    mut layout: ResMut<Layout>,
    windows: Query<&Window, With<PrimaryWindow>>,
    bottom: Res<Bottom>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let size = Vec2::new(window.width(), window.height());
    layout.size = size;
    layout.bar = top_height(size.x);
    layout.talk = talk_height(size.x);
    layout.top = layout.bar + layout.talk;
    // The score stacks four staves per agent where the roll draws one lane,
    // so it takes the larger share of the window while it is the one
    // drawing. The map above shrinks by the same amount, and the click
    // routing follows, because everything reads this one layout.
    layout.bottom = match bottom.mode() {
        lm_host_model::view::BottomMode::Roll => bottom_height(size.y),
        lm_host_model::view::BottomMode::Score => score_height(size.y),
    };
}

/// Build the map of this frame: the world as the viewer may see it, with
/// every agent lerped between the last two ticks.
fn build_map(
    mut map: ResMut<CurrentMap>,
    round: Res<Round>,
    input: Res<Input>,
    fixed: Res<Time<Fixed>>,
) {
    let Some(sim) = round.sim() else {
        return;
    };

    // A round being replayed is watched, not played: the viewer sees the
    // whole round, the way the authority's own view sees it.
    let viewer = if round.is_replay() {
        Viewer::Spectator { follow: None }
    } else {
        input.0.viewer
    };

    let model = MapModel::build(
        sim,
        round.prev_positions(),
        &viewer,
        fixed.overstep_fraction(),
    );
    map.0 = Some(model);
}

/// Move and zoom the camera.
///
/// It looks at the agent being played, or at the one being watched; when
/// nobody is being watched it stays where it was left, and the person drives
/// it with WASD, the drag and the wheel.
///
/// Only the map's own camera ([`MapCamera`]) — never any other view's: in
/// the `both` layout the 3D view's cameras live in the same app, and a
/// controller that reached for every `Camera` would drag them along.
fn update_camera(
    mut cameras: Query<&mut Transform, With<MapCamera>>,
    mut projections: Query<&mut Projection, With<MapCamera>>,
    mut rig: ResMut<CameraRig>,
    map: Res<CurrentMap>,
    input: Res<Input>,
    layout: Res<Layout>,
    layer: Res<TileLayer>,
) {
    // Who the camera follows: the agent being played, or the one watched.
    let follow = input.0.viewer.possessing().or(input.0.viewer.following());
    rig.follow = follow;

    // The focus: the place of the agent, when there is one.
    if let Some(id) = follow {
        if let Some(draw) = map
            .0
            .as_ref()
            .and_then(|m| m.agents.iter().find(|a| a.id == id))
        {
            rig.focus = bevy_point(draw.pos_m);
        }
    }

    // The zoom: the person's own, or the one the mode asks for.
    let px_per_m = match rig.zoom {
        Some(zoom) => zoom,
        None if follow.is_some() => FOLLOW_PX_PER_M,
        None => fit_px_per_m(&layout, layer.world_m),
    }
    .clamp(MIN_PX_PER_M, MAX_PX_PER_M);
    rig.px_per_m = px_per_m;
    let scale = 1.0 / px_per_m;

    // Where the camera sits, so that the focus is in the middle of the area
    // the panels leave free.
    let translation = camera_translation(rig.focus, &layout, scale);
    for mut transform in &mut cameras {
        transform.translation = Vec3::new(translation.x, translation.y, 0.0);
    }
    for mut projection in &mut projections {
        if let Projection::Orthographic(orthographic) = &mut *projection {
            orthographic.scale = scale;
        }
    }
}

/// Draw the map: the fog over the tiles, the agents, the beacons.
#[allow(clippy::too_many_arguments)]
fn draw_map(
    mut bodies: Query<(&mut Sprite, &mut Transform, &mut Visibility, &Body), Without<SplatBlob>>,
    mut blobs: Query<(&mut Sprite, &mut Transform, &mut Visibility, &SplatBlob), With<SplatBlob>>,
    map: Res<CurrentMap>,
    look: Res<Look>,
    mut effects: ResMut<Effects>,
    input: Res<Input>,
    mut layer: ResMut<TileLayer>,
    rig: Res<CameraRig>,
    mut gizmos: Gizmos,
) {
    let Some(model) = map.0.as_ref() else {
        return;
    };
    let px_per_m = rig.px_per_m;
    // The words being spoken: one tick per word, above the body that is
    // saying them. The ticks stand clear of the body — the agent's own
    // disc, and the leader's ring around it, would swallow a tick drawn on
    // top of them, and a word nobody can see is not said.
    let tick_length = TICK_LENGTH_M.max(MIN_TICK_PX / px_per_m);
    let tick_spacing = TICK_SPACING_M.max(MIN_TICK_PX / px_per_m);
    let tick_gap = AGENT_RADIUS_M.max(MIN_AGENT_PX * 0.5 / px_per_m) * 1.6 + TICK_GAP_M;

    for (mut sprite, mut transform, mut visibility, body) in &mut bodies {
        match body {
            // The fog: only the cells whose fog changed are repainted.
            Body::Fog(index) => {
                let Some(&(_, _, state)) = model.cells.get(*index) else {
                    continue;
                };
                if layer.drawn.get(*index) == Some(&state) {
                    continue;
                }
                sprite.color = fog_color(&look, state);
                if let Some(slot) = layer.drawn.get_mut(*index) {
                    *slot = state;
                }
            }

            // An agent: a disc in the colour of the squad, grey while it is
            // stunned.
            Body::Agent(id) => {
                let Some(draw) = model.agents.iter().find(|a| a.id == *id) else {
                    *visibility = Visibility::Hidden;
                    continue;
                };
                *visibility = Visibility::Visible;
                let radius = AGENT_RADIUS_M.max(MIN_AGENT_PX * 0.5 / px_per_m);
                sprite.custom_size = Some(Vec2::splat(radius * 2.0));
                sprite.color = if draw.stunned {
                    look.bevy(look.0.ui.stunned)
                } else {
                    squad_color(&look, draw.squad)
                };
                let point = bevy_point(draw.pos_m);
                transform.translation = Vec3::new(point.x, point.y, AGENT_Z);

                // Everybody shows which way they face: the nose is the ink
                // of the picture, so it reads on any of the squads' colours.
                let facing = Vec2::new(draw.facing_rad.cos(), -draw.facing_rad.sin());
                gizmos.line_2d(
                    point,
                    point + facing * (radius * 1.9),
                    look.bevy(look.0.outline.color),
                );

                // The words this agent is saying: a tick per word, above
                // the body. A word that has been said is bright; a word the
                // speaker has not reached yet is dim.
                for tick in model.speech.iter().filter(|tick| tick.speaker == *id) {
                    let base = Vec2::new(
                        point.x + f32::from(tick.index) * tick_spacing,
                        point.y + tick_gap,
                    );
                    let mut color = channel_color(tick.channel);
                    if tick.at > model.now_tick {
                        color.set_alpha(UNSPOKEN_ALPHA);
                    }
                    gizmos.line_2d(base, base + Vec2::new(0.0, tick_length), color);
                }
            }

            // The dark edge around a body: the same place, a shade larger.
            Body::AgentEdge(id) => {
                let Some(draw) = model.agents.iter().find(|a| a.id == *id) else {
                    *visibility = Visibility::Hidden;
                    continue;
                };
                *visibility = Visibility::Visible;
                let radius = AGENT_RADIUS_M.max(MIN_AGENT_PX * 0.5 / px_per_m);
                sprite.custom_size = Some(Vec2::splat(radius * 2.0 * EDGE_SCALE));
                let point = bevy_point(draw.pos_m);
                transform.translation = Vec3::new(point.x, point.y, AGENT_Z - 0.01);
            }

            // The leader's halo: the mark that says which one leads.
            Body::AgentHalo(id) => {
                let Some(draw) = model.agents.iter().find(|a| a.id == *id) else {
                    *visibility = Visibility::Hidden;
                    continue;
                };
                *visibility = Visibility::Visible;
                let radius = AGENT_RADIUS_M.max(MIN_AGENT_PX * 0.5 / px_per_m);
                sprite.custom_size = Some(Vec2::splat(radius * 2.0 * HALO_SCALE));
                let point = bevy_point(draw.pos_m);
                transform.translation = Vec3::new(point.x, point.y, AGENT_Z - 0.02);
            }

            // A beacon: a diamond in the colour of the squad holding it.
            Body::Beacon(index) => {
                let Some(draw) = model.beacons.get(usize::from(*index)) else {
                    *visibility = Visibility::Hidden;
                    continue;
                };
                *visibility = Visibility::Visible;
                let radius = BEACON_RADIUS_M.max(MIN_BEACON_PX * 0.5 / px_per_m);
                sprite.custom_size = Some(Vec2::splat(radius * 2.0));
                sprite.color = match draw.holder {
                    Some(squad) => squad_color(&look, squad),
                    None => look.bevy(look.0.world.beacon_free),
                };
                let point = bevy_point(draw.pos_m);
                transform.translation = Vec3::new(point.x, point.y, BEACON_Z);
            }
        }
    }

    // The tag moment: the hits of this round, and the path of the agent
    // being watched.
    draw_effects(
        &mut blobs,
        &mut gizmos,
        &mut effects,
        &look,
        &input,
        model,
        px_per_m,
    );
}

/// Draw the effects: the splat of a hit, the pips over a stunned agent, and
/// the path of the agent being watched.
///
/// The effects are the round's, not the map's: they live in one resource
/// that the 3D view reads too, so a hit looks like the same hit in both
/// views. The map only draws them.
fn draw_effects(
    blobs: &mut Query<(&mut Sprite, &mut Transform, &mut Visibility, &SplatBlob), With<SplatBlob>>,
    gizmos: &mut Gizmos,
    effects: &mut Effects,
    look: &Look,
    input: &Input,
    model: &MapModel,
    px_per_m: f32,
) {
    // The dabs of paint: the pool is filled from the effects, and the dabs
    // that are left over are put away.
    let mut pool = blobs.iter_mut();
    for splat in effects.splats.iter() {
        let Some((mut sprite, mut transform, mut visibility, _)) = pool.next() else {
            break;
        };
        *visibility = Visibility::Visible;
        // The paint spreads as it dries, and it fades as it goes: the
        // model says how big and how strong, so the map and the 3D view
        // paint the same dab.
        let radius = splat.radius();
        sprite.custom_size = Some(Vec2::splat(radius * 2.0));
        sprite.color = look.bevy(splat.paint).with_alpha(splat.opacity());
        let point = bevy_point(splat.pos_m);
        transform.translation = Vec3::new(point.x, point.y, SPLAT_Z);
    }
    for (_, _, mut visibility, _) in pool {
        *visibility = Visibility::Hidden;
    }

    // The stun pips: one for each tick of the stun that is left, above the
    // head of the agent that is out of the round.
    let pip_h = (MIN_AGENT_PX * 1.4).max(10.0) / px_per_m;
    let pip_gap = (MIN_AGENT_PX * 2.0).max(16.0) / px_per_m;
    let pip_w = pip_h * 0.5;
    for draw in model.agents.iter().filter(|draw| draw.stunned) {
        let pips = draw.stun_ticks.min(MAX_STUN_PIPS);
        if pips == 0 {
            effects.forget(draw.id);
            continue;
        }
        let color = effects
            .stun_paint_of(draw.id)
            .map_or(look.bevy(look.0.ui.stunned), |paint| look.bevy(paint));
        let point = bevy_point(draw.pos_m);
        let total = pips as f32 * pip_w;
        for pip in 0..pips {
            let x = point.x - total * 0.5 + pip as f32 * pip_w + pip_w * 0.5;
            let base = Vec2::new(x, point.y + pip_gap);
            gizmos.line_2d(base, base + Vec2::new(0.0, pip_h), color);
        }
    }

    // The path of the agent being watched: the host's own bookkeeping, the
    // places the agent has been, frame by frame.
    let watched = input.0.viewer.possessing().or(input.0.viewer.following());
    let pos = watched.and_then(|agent| {
        model
            .agents
            .iter()
            .find(|draw| draw.id == agent)
            .map(|draw| draw.pos_m)
    });
    effects.follow(watched, pos);
    if effects.trail.len() > 1 {
        let color = watched
            .and_then(|agent| {
                model
                    .agents
                    .iter()
                    .find(|draw| draw.id == agent)
                    .map(|draw| draw.squad)
            })
            .map_or(look.bevy(look.0.ui.accent), |squad| {
                squad_color(look, squad)
            });
        let points: Vec<Vec2> = effects
            .trail
            .iter()
            .map(|&(x, y)| bevy_point((x, y)))
            .collect();
        for window in points.windows(2) {
            gizmos.line_2d(window[0], window[1], color.with_alpha(0.35));
        }
        for point in &points {
            gizmos.circle_2d(*point, 0.18, color);
        }
    }
}

/// The colour of a word spoken on a channel.
///
/// The panels speak in the colours of the world, so the roll's speaking
/// spans and the map's speech ticks are the same colour for the same
/// channel.
pub fn channel_color(channel: Channel) -> Color {
    match channel {
        Channel::Shout => CHANNEL_SHOUT,
        Channel::Voice => CHANNEL_VOICE,
        Channel::Whisper => CHANNEL_WHISPER,
        Channel::Radio => CHANNEL_RADIO,
    }
}

/// The zoom that fits the whole world into the area the panels leave free.
fn fit_px_per_m(layout: &Layout, world_m: Vec2) -> f32 {
    if world_m.x <= 0.0 || world_m.y <= 0.0 {
        return 1.0;
    }
    let map = layout.map_size();
    (map.x / world_m.x).min(map.y / world_m.y)
}

/// Where the camera has to sit for the focus to be in the middle of the
/// area the panels leave free.
pub fn camera_translation(focus: Vec2, layout: &Layout, scale: f32) -> Vec2 {
    focus + Vec2::new(0.0, (layout.top - layout.bottom) * 0.5 * scale)
}

/// The cell of the map under the cursor, if the cursor is over the map.
///
/// The point under the cursor is in the world's own metres -- x east, y
/// south -- and the cell is the one that holds it.
pub fn cursor_cell(
    rig: &CameraRig,
    layout: &Layout,
    layer: &TileLayer,
    cursor: Vec2,
) -> Option<CellXY> {
    let world = cursor_world(rig, layout, cursor);
    let x = world.x / layer.cell_m;
    let y = world.y / layer.cell_m;
    if x < 0.0 || y < 0.0 {
        return None;
    }
    let cell = CellXY {
        x: x as u16,
        y: y as u16,
    };
    if cell.x >= layer.grid_w || cell.y >= layer.grid_h {
        return None;
    }
    Some(cell)
}

/// The point of the world under the cursor, in the world's own metres.
pub fn cursor_world(rig: &CameraRig, layout: &Layout, cursor: Vec2) -> Vec2 {
    let scale = 1.0 / rig.px_per_m;
    let offset = cursor - layout.size * 0.5;
    let translation = camera_translation(rig.focus, layout, scale);
    let bevy = translation + Vec2::new(offset.x * scale, -offset.y * scale);
    Vec2::new(bevy.x, -bevy.y)
}

/// The agent under the cursor, if the cursor is over one.
pub fn hovered_agent(
    rig: &CameraRig,
    layout: &Layout,
    map: &CurrentMap,
    cursor: Vec2,
) -> Option<u16> {
    let world = cursor_world(rig, layout, cursor);
    let model = map.0.as_ref()?;
    let radius = (AGENT_RADIUS_M).max(MIN_AGENT_PX * 0.5 / rig.px_per_m);
    model
        .agents
        .iter()
        .find(|draw| {
            let point = bevy_point(draw.pos_m);
            (point - world).length() <= radius * 1.5
        })
        .map(|draw| draw.id)
}
