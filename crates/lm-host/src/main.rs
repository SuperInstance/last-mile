//! `lm-host`: the Windows-only Bevy app that runs the sim and draws it.
//!
//! This is the Task 1 spike: a window, a 2D camera, one coloured square, an
//! egui side panel, and a `--screenshot PATH --frames N` mode that renders N
//! frames, saves a PNG and exits — how reviewers verify visuals without a
//! screen. Every API it relies on is recorded in `docs/notes/bevy-0.20.md`;
//! later tasks extend this app from the notes, not from older Bevy APIs.
//!
//! Build and run it with Windows cargo via `scripts/win-build.sh`; this WSL
//! toolchain has none of Bevy's Linux system libraries.

use bevy::{
    app::AppExit,
    prelude::*,
    render::{
        settings::{PowerPreference, RenderCreation, WgpuSettings},
        view::screenshot::{save_to_disk, Screenshot, ScreenshotCaptured},
        RenderPlugin,
    },
};
use bevy_egui::{egui, EguiContexts, EguiPlugin, EguiPrimaryContextPass};
use std::{num::NonZeroU8, path::PathBuf};

/// The spike's one coloured square.
const SQUARE_COLOR: Color = Color::srgb(0.85, 0.30, 0.19);
/// The square's edge in world units (pixels at the default 2D camera zoom).
const SQUARE_SIZE: f32 = 128.0;
/// The fixed-timestep rate the sim will run at. `Params::tick_hz` (Task 2)
/// replaces this constant; the spike pins the same value to prove the
/// fixed-timestep plumbing end to end.
const TICK_HZ: f64 = 64.0;

/// How many times `FixedUpdate` has run: shown in the panel so a screenshot
/// proves the schedule ticks decoupled from the frame rate.
#[derive(Resource, Default)]
struct FixedTicks(u64);

/// How many frames have rendered.
#[derive(Resource, Default)]
struct FrameCount(u32);

/// `--screenshot PATH --frames N`: after N frames, capture the window to PATH
/// (a PNG) and exit. `spawned` guards against requesting twice.
#[derive(Resource)]
struct ShotRequest {
    path: PathBuf,
    frames: u32,
    spawned: bool,
}

fn main() -> AppExit {
    let shot = match parse_args() {
        Ok(shot) => shot,
        Err(err) => {
            eprintln!("lm-host: {err}");
            eprintln!("usage: lm-host [--screenshot PATH [--frames N]]");
            return AppExit::Error(NonZeroU8::MIN);
        }
    };

    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "last-mile".to_owned(),
                    resolution: (1280, 720).into(),
                    ..default()
                }),
                ..default()
            })
            // Ask for the RTX 4050, not the iGPU (spec §6). WgpuSettings
            // already defaults to HighPerformance, but only unless the
            // WGPU_POWER_PREF env var says otherwise — this pins it.
            .set(RenderPlugin {
                render_creation: RenderCreation::Automatic(Box::new(WgpuSettings {
                    power_preference: PowerPreference::HighPerformance,
                    ..default()
                })),
                ..default()
            }),
        EguiPlugin::default(),
    ))
    // The sim will tick here at `Params::tick_hz`, decoupled from the frame
    // rate; rendering will interpolate with the overstep fraction (Task 2).
    .insert_resource(Time::<Fixed>::from_hz(TICK_HZ))
    .init_resource::<FixedTicks>()
    .init_resource::<FrameCount>()
    .add_systems(Startup, setup)
    .add_systems(FixedUpdate, count_fixed_ticks)
    .add_systems(Update, count_frames)
    // Egui UI systems belong in this egui-aware schedule, not `Update`:
    // it reruns them for egui's extra passes.
    .add_systems(EguiPrimaryContextPass, hello_panel);

    if let Some(request) = shot {
        app.insert_resource(request).add_systems(
            Update,
            shoot_and_exit.run_if(resource_exists::<ShotRequest>),
        );
    }

    app.run()
}

fn parse_args() -> Result<Option<ShotRequest>, String> {
    let mut path = None;
    let mut frames = 30u32;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--screenshot" => {
                path = Some(args.next().ok_or_else(|| {
                    String::from(
                        "`--screenshot` needs a PATH, e.g. C:\\Users\\casey\\src\\shot.png",
                    )
                })?);
            }
            "--frames" => {
                frames = args
                    .next()
                    .ok_or_else(|| String::from("`--frames` needs a count"))?
                    .parse()
                    .map_err(|_| String::from("`--frames` needs a whole number"))?;
            }
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    if path.is_some() && frames == 0 {
        return Err(String::from("`--frames` must be at least 1"));
    }
    Ok(path.map(|path| ShotRequest {
        path: PathBuf::from(path),
        frames,
        spawned: false,
    }))
}

fn setup(mut commands: Commands) {
    // `Camera2d` is a marker: it carries its `Camera`, orthographic
    // projection and frustum as required components. bevy_egui attaches its
    // context to the first camera, so this must exist before the panel.
    commands.spawn(Camera2d);
    // `Sprite` carries `Transform`, `Visibility` and `Anchor` as required
    // components, so colour plus size is a whole square.
    commands.spawn(Sprite::from_color(SQUARE_COLOR, Vec2::splat(SQUARE_SIZE)));
}

fn count_fixed_ticks(mut ticks: ResMut<FixedTicks>) {
    ticks.0 += 1;
}

fn count_frames(mut frame: ResMut<FrameCount>) {
    frame.0 += 1;
}

fn hello_panel(
    mut contexts: EguiContexts,
    ticks: Res<FixedTicks>,
    frame: Res<FrameCount>,
    fixed: Res<Time<Fixed>>,
) -> Result {
    let ctx = contexts.ctx_mut()?.clone();
    // egui 0.36 panels show into a `&mut Ui`, not a `&Context`: build the
    // background ui the panel docks against.
    let mut root = egui::Ui::new(
        ctx.clone(),
        "viewport".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    egui::Panel::left("spike_side")
        .resizable(true)
        .default_size(220.0)
        .show(&mut root, |ui| {
            ui.heading("last-mile");
            ui.label("hello");
            ui.separator();
            ui.label(format!("fixed ticks: {}", ticks.0));
            ui.label(format!("frames: {}", frame.0));
            ui.label(format!("overstep: {:.3}", fixed.overstep_fraction()));
        });
    Ok(())
}

/// When the requested frame count is reached, capture the window and exit.
fn shoot_and_exit(
    mut commands: Commands,
    mut request: ResMut<ShotRequest>,
    frame: Res<FrameCount>,
) {
    if request.spawned || frame.0 < request.frames {
        return;
    }
    request.spawned = true;
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(request.path.clone()))
        .observe(quit_after_capture);
}

/// The capture is asynchronous: this fires when the PNG bytes are ready, so
/// the app never exits before the file is written.
fn quit_after_capture(_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>) {
    exit.write(AppExit::Success);
}
