//! `lm-host`: the Windows-only Bevy app that runs the sim and draws it.
//!
//! The app is a thin shell around `lm-host-model`: the round is a
//! [`HostLoop`], the models build the map and the piano roll, and this
//! crate only wires them to a window. The rules of the game live in
//! `lm-core`, and the host obeys them like everybody else -- every human
//! action becomes a decision that travels through the same sequencer the
//! agents use (spec §6, D10 lesson 10).
//!
//! The pieces:
//!
//! * [`sim_plugin`] steps the round in `FixedUpdate` at `Params::tick_hz`,
//!   so the sim keeps its rate whatever the frame rate does;
//! * [`map_plugin`] draws the cells, the fog, the agents and the beacons,
//!   and moves the camera;
//! * [`input_plugin`] turns the keyboard and the mouse into decisions;
//! * [`hud_plugin`] is the bar at the top, [`roll_plugin`] the piano roll at
//!   the bottom.
//!
//! Build and run it with Windows cargo via `scripts/win-build.sh`; this WSL
//! toolchain has none of Bevy's Linux system libraries.

mod hud_plugin;
mod input_plugin;
mod map_plugin;
mod roll_plugin;
mod sim_plugin;

use bevy::{
    app::AppExit,
    prelude::*,
    render::{
        settings::{PowerPreference, RenderCreation, WgpuSettings},
        view::screenshot::{save_to_disk, Screenshot, ScreenshotCaptured},
        RenderPlugin,
    },
};
use bevy_egui::EguiPlugin;
use lm_core::header::Header;
use lm_host_model::host_loop::HostLoop;
use lm_host_model::input::{InputEvent, InputState};
use std::{num::NonZeroU8, path::PathBuf};

/// The window the app opens with.
const DEFAULT_WINDOW: (u32, u32) = (1280, 720);
/// The seed the round starts from when nobody says otherwise.
const DEFAULT_SEED: u64 = 1;

/// `--screenshot PATH --frames N`: after N frames, capture the window to
/// PATH (a PNG) and exit. `spawned` guards against requesting twice.
#[derive(Resource)]
struct ShotRequest {
    path: PathBuf,
    frames: u32,
    spawned: bool,
}

/// What the command line asks for.
struct Options {
    /// The seed of the round.
    seed: u64,
    /// The size of the window, in pixels.
    window: (u32, u32),
    /// The agent to play as from the first frame, if any.
    possess: Option<u16>,
    /// The screenshot to take, if any.
    shot: Option<ShotRequest>,
}

fn main() -> AppExit {
    let options = match parse_args() {
        Ok(options) => options,
        Err(err) => {
            eprintln!("lm-host: {err}");
            eprintln!(
                "usage: lm-host [--seed N] [--window WxH] [--possess AGENT] \
                 [--screenshot PATH [--frames N]]"
            );
            return AppExit::Error(NonZeroU8::MIN);
        }
    };

    // The round: the standard one, with the shipped reflex. The host plays
    // the same round the headless tools do.
    let header = Header::default_round(options.seed, lm_tools::runner::DEFAULT_REFLEX);
    let tick_hz = header.params.tick_hz;
    let mut host = match HostLoop::new(header) {
        Ok(host) => host,
        Err(err) => {
            eprintln!("lm-host: the round does not start: {err}");
            return AppExit::Error(NonZeroU8::MIN);
        }
    };

    // The viewer: watching, unless the command line says to play.
    let mut input = InputState::new();
    if let Some(agent) = options.possess {
        if let Err(err) = input.apply(InputEvent::Possess(agent), &mut host) {
            eprintln!("lm-host: {err}");
            return AppExit::Error(NonZeroU8::MIN);
        }
    }

    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "last-mile".to_owned(),
                    resolution: options.window.into(),
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
    .insert_resource(sim_plugin::Host(host))
    .insert_resource(input_plugin::Input(input))
    .init_resource::<FrameCount>()
    .add_plugins((
        sim_plugin::SimPlugin::new(tick_hz),
        map_plugin::MapPlugin,
        input_plugin::InputPlugin,
        hud_plugin::HudPlugin,
        roll_plugin::RollPlugin,
    ))
    // What the person did is read before the map is drawn, so the frame
    // shows the order that was just given.
    .configure_sets(
        Update,
        (input_plugin::InputSet::Read, map_plugin::MapSet::Map).chain(),
    )
    .add_systems(Update, count_frames);

    if let Some(request) = options.shot {
        app.insert_resource(request).add_systems(
            Update,
            shoot_and_exit.run_if(resource_exists::<ShotRequest>),
        );
    }

    app.run()
}

/// Read the command line.
fn parse_args() -> Result<Options, String> {
    let mut seed = DEFAULT_SEED;
    let mut window = DEFAULT_WINDOW;
    let mut possess = None;
    let mut path = None;
    let mut frames = 30u32;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--seed" => {
                seed = args
                    .next()
                    .ok_or_else(|| String::from("`--seed` needs a number"))?
                    .parse()
                    .map_err(|_| String::from("`--seed` needs a whole number"))?;
            }
            "--window" => {
                let text = args
                    .next()
                    .ok_or_else(|| String::from("`--window` needs a size, e.g. 640x360"))?;
                window = parse_window(&text)?;
            }
            "--possess" => {
                possess = Some(
                    args.next()
                        .ok_or_else(|| String::from("`--possess` needs an agent"))?
                        .parse()
                        .map_err(|_| String::from("`--possess` needs an agent number"))?,
                );
            }
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
    Ok(Options {
        seed,
        window,
        possess,
        shot: path.map(|path| ShotRequest {
            path: PathBuf::from(path),
            frames,
            spawned: false,
        }),
    })
}

/// A window size, written `640x360`.
fn parse_window(text: &str) -> Result<(u32, u32), String> {
    let (width, height) = text
        .split_once(['x', 'X'])
        .ok_or_else(|| format!("`--window` wants a size like 640x360, not `{text}`"))?;
    let width: u32 = width
        .trim()
        .parse()
        .map_err(|_| format!("`--window` wants a width in pixels, not `{width}`"))?;
    let height: u32 = height
        .trim()
        .parse()
        .map_err(|_| format!("`--window` wants a height in pixels, not `{height}`"))?;
    if width == 0 || height == 0 {
        return Err(String::from(
            "`--window` wants a size of at least one pixel",
        ));
    }
    Ok((width, height))
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

/// How many frames have rendered.
#[derive(Resource, Default)]
struct FrameCount(u32);

/// Count the frames: the screenshot mode waits for them.
fn count_frames(mut frame: ResMut<FrameCount>) {
    frame.0 += 1;
}
