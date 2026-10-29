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
//!   so the sim keeps its rate whatever the frame rate does, and writes the
//!   round down when it is over or when the window closes;
//! * [`map_plugin`] draws the cells, the fog, the agents and the beacons,
//!   and moves the camera;
//! * [`input_plugin`] turns the keyboard and the mouse into decisions;
//! * [`hud_plugin`] is the bar at the top, [`roll_plugin`] the piano roll at
//!   the bottom.
//!
//! The app plays a round, or it opens one: `--replay FILE` opens a recorded
//! round log and scrubs it instead of playing a new one.
//!
//! ## The agents
//!
//! `--agents thor|stub|off` says who is thinking. `off` is the reflex alone.
//! `stub` is the same gates, the same policy and the same ladder, with a
//! scripted model behind them — the round plays itself, no network needed.
//! `thor` is the real thing: the Thor's d1-2B (D62) on the LAN, and the cloud tier
//! when its key is there.
//!
//! Build and run it with Windows cargo via `scripts/win-build.sh`; this WSL
//! toolchain has none of Bevy's Linux system libraries.

mod audio_plugin;
mod hud_plugin;
mod input_plugin;
mod map_plugin;
mod roll_plugin;
mod sim_plugin;

use std::collections::BTreeMap;
use std::time::Duration;

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
use lm_agents::config::AgentsConfig;
use lm_agents::harness::Harness;
use lm_agents::mind::Policy;
use lm_agents::sysone::{JevBackend, StubBackend, SysOne, ThorBackend};
use lm_agents::Agents;
use lm_core::header::Header;
use lm_host_model::host_loop::HostLoop;
use lm_host_model::input::{InputEvent, InputState};
use lm_host_model::record;
use lm_host_model::scrub::Scrubber;
use std::{num::NonZeroU8, path::PathBuf};

/// The window the app opens with.
const DEFAULT_WINDOW: (u32, u32) = (1280, 720);
/// The seed the round starts from when nobody says otherwise.
const DEFAULT_SEED: u64 = 1;
/// The rate a round ticks at when the round itself cannot say.
const DEFAULT_TICK_HZ: u16 = 30;
/// The harness the agents think with: the shipped one.
const DEFAULT_HARNESS: &str = "assets/harness/default";

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
    /// The seed the round starts from.
    seed: u64,
    /// The size of the window, in pixels.
    window: (u32, u32),
    /// The agent to play as from the first frame, if any.
    possess: Option<u16>,
    /// The round log to replay, if any.
    replay: Option<PathBuf>,
    /// The screenshot to take, if any.
    shot: Option<ShotRequest>,
    /// Who is thinking: the reflex alone, a stub, or the Thor.
    agents: AgentChoice,
    /// Where the harness comes from: the gates and the policy.
    harness: PathBuf,
    /// The agents' configuration, when the command line names one.
    config: Option<PathBuf>,
}

/// Who is thinking.
///
/// `off` is the reflex alone, `stub` is the real pipeline with a scripted
/// model behind it, and `thor` is the real thing: the Thor's decision models
/// on the LAN.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AgentChoice {
    /// No agents: the reflex plays the round.
    Off,
    /// The agents think; the model is a scripted stub.
    Stub,
    /// The agents think; the model is the Thor's.
    Thor,
}

impl AgentChoice {
    /// The choice a command line spells out.
    fn parse(text: &str) -> Result<AgentChoice, String> {
        match text {
            "off" => Ok(AgentChoice::Off),
            "stub" => Ok(AgentChoice::Stub),
            "thor" => Ok(AgentChoice::Thor),
            other => Err(format!("`--agents` wants off, stub or thor, not `{other}`")),
        }
    }

    /// The choice, as it is written on the command line.
    fn as_str(self) -> &'static str {
        match self {
            AgentChoice::Off => "off",
            AgentChoice::Stub => "stub",
            AgentChoice::Thor => "thor",
        }
    }
}

fn main() -> AppExit {
    let options = match parse_args() {
        Ok(options) => options,
        Err(err) => {
            eprintln!("lm-host: {err}");
            eprintln!(
                "usage: lm-host [--seed N] [--window WxH] [--possess AGENT] \
                 [--replay FILE] [--screenshot PATH [--frames N]] \
                 [--agents off|stub|thor] [--harness DIR] [--agents-config FILE]"
            );
            return AppExit::Error(NonZeroU8::MIN);
        }
    };

    // The viewer: watching, unless the command line says to play.
    let mut input = InputState::new();

    // The round: either one being played, or one being replayed out of a
    // log. The two are different animals, so the app is told which one it
    // has before it starts.
    let (round, tick_hz, recording) = match &options.replay {
        Some(path) => {
            if options.possess.is_some() {
                eprintln!("lm-host: a replay cannot be played: `--possess` is ignored");
            }
            match Scrubber::open(path) {
                Ok(scrubber) => {
                    // The replay plays at the rate the round was played at.
                    let tick_hz = scrubber.log().header.params.tick_hz;
                    (
                        sim_plugin::Round::replay(scrubber, path.clone()),
                        tick_hz,
                        sim_plugin::Recording::default(),
                    )
                }
                Err(err) => {
                    // A file that cannot be played is not a crash: the app
                    // opens and says what is wrong with it.
                    eprintln!("lm-host: {} cannot be replayed: {err}", path.display());
                    (
                        sim_plugin::Round::broken(err, path.clone()),
                        DEFAULT_TICK_HZ,
                        sim_plugin::Recording::default(),
                    )
                }
            }
        }
        None => {
            // The round: the standard one, with the shipped reflex. The host
            // plays the same round the headless tools do.
            let header = Header::default_round(options.seed, lm_tools::runner::DEFAULT_REFLEX);
            let tick_hz = header.params.tick_hz;
            let mut host = match HostLoop::new(header.clone()) {
                Ok(host) => host,
                Err(err) => {
                    eprintln!("lm-host: the round does not start: {err}");
                    return AppExit::Error(NonZeroU8::MIN);
                }
            };

            // The agents, when they are asked for. They think on their own
            // thread and hand their decisions to the host, which puts them in
            // the sequencer like every other decision.
            if options.agents != AgentChoice::Off {
                println!(
                    "lm-host: the agents think with the {} model",
                    options.agents.as_str()
                );
                match attach_agents(&options, &header, &mut host) {
                    Ok(()) => {}
                    Err(err) => {
                        eprintln!("lm-host: the agents do not start: {err}");
                        return AppExit::Error(NonZeroU8::MIN);
                    }
                }
            }

            if let Some(agent) = options.possess {
                if let Err(err) = input.apply(InputEvent::Possess(agent), &mut host) {
                    eprintln!("lm-host: {err}");
                    return AppExit::Error(NonZeroU8::MIN);
                }
            }

            // The round is written down when it is over, or when the window
            // closes: one file per round, named after the seed it was played
            // from and the second it was played in.
            let recording = sim_plugin::Recording {
                path: Some(record::path_for(options.seed, unix_secs())),
                written: false,
            };

            (sim_plugin::Round::live(host), tick_hz, recording)
        }
    };

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
    .insert_resource(round)
    .insert_resource(recording)
    .insert_resource(input_plugin::Input(input))
    .init_resource::<FrameCount>()
    .add_plugins((
        sim_plugin::SimPlugin::new(tick_hz),
        map_plugin::MapPlugin,
        input_plugin::InputPlugin,
        audio_plugin::VoicePlugin,
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

/// The wall clock, in seconds since the epoch.
///
/// It names the file a round is recorded in, so two rounds of the same seed
/// never fight over one name.
fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0)
}

/// Build the round's agents and give them to the host.
///
/// The harness is the agents' own: the gates they ask and the policy they
/// think with. The models are behind the tiers of that policy.
fn attach_agents(options: &Options, header: &Header, host: &mut HostLoop) -> Result<(), String> {
    let cfg = match &options.config {
        Some(path) => AgentsConfig::load(path)
            .map_err(|err| format!("the configuration {} cannot be read: {err}", path.display()))?,
        None => AgentsConfig::default(),
    };
    let harness = Harness::open(&options.harness).map_err(|err| {
        format!(
            "the harness {} cannot be read: {err}",
            options.harness.display()
        )
    })?;

    let backends = backends(options.agents, &cfg, &harness);
    if backends.is_empty() {
        return Err("no model is there to ask".to_owned());
    }
    let agents = Agents::new(&cfg, &harness, header, backends);
    host.attach_agents(agents);
    Ok(())
}

/// The models the round's agents think with.
///
/// The names are the ones the policy's tiers ask for, so the harness does not
/// have to know which model is behind them.
fn backends(
    choice: AgentChoice,
    cfg: &AgentsConfig,
    harness: &Harness,
) -> BTreeMap<String, Box<dyn SysOne + Send>> {
    let mut out: BTreeMap<String, Box<dyn SysOne + Send>> = BTreeMap::new();
    match choice {
        AgentChoice::Off => {}
        AgentChoice::Stub => {
            // The stub answers every tier the policy names: the gates, the
            // policy and the ladder are the real ones, only the model is not.
            let policy = Policy::for_harness(&harness.policy_toml);
            for tier in policy.tiers.tiers() {
                if tier.is_reflex() {
                    continue;
                }
                out.entry(tier.backend().to_owned())
                    .or_insert_with(|| Box::new(StubBackend::menu()) as Box<dyn SysOne + Send>);
            }
        }
        AgentChoice::Thor => {
            // The Thor's models: one port each.
            for (model, port) in &cfg.ports {
                let mut backend = ThorBackend::new(&cfg.thor_base, *port, model)
                    .with_timeout(Duration::from_millis(u64::from(cfg.request_timeout_ms)));
                let name = backend.name().to_owned();
                if let Some(min) = cfg.min_confidence.get(&name) {
                    backend = backend.with_min_confidence(*min);
                }
                out.insert(name, Box::new(backend));
            }
            // The cloud tier, when its key is there. That key only ever goes
            // to api.typesafe.ai -- never to the Thor.
            match JevBackend::from_secrets() {
                Ok(jev) => {
                    let name = jev.name().to_owned();
                    out.insert(name, Box::new(jev));
                }
                Err(err) => eprintln!("lm-host: the cloud tier is not available: {err}"),
            }
        }
    }
    out
}

/// Read the command line.
fn parse_args() -> Result<Options, String> {
    let mut seed = DEFAULT_SEED;
    let mut window = DEFAULT_WINDOW;
    let mut possess = None;
    let mut replay = None;
    let mut path = None;
    let mut frames = 30u32;
    let mut agents = AgentChoice::Off;
    let mut harness = PathBuf::from(DEFAULT_HARNESS);
    let mut config = None;
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
            "--replay" => {
                replay = Some(PathBuf::from(args.next().ok_or_else(|| {
                    String::from("`--replay` needs a file, e.g. runs\\1-1764000000.lml")
                })?));
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
            "--agents" => {
                let text = args
                    .next()
                    .ok_or_else(|| String::from("`--agents` needs off, stub or thor"))?;
                agents = AgentChoice::parse(&text)?;
            }
            "--harness" => {
                harness = PathBuf::from(
                    args.next()
                        .ok_or_else(|| String::from("`--harness` needs a directory"))?,
                );
            }
            "--agents-config" => {
                config =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        String::from("`--agents-config` needs a file")
                    })?));
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
        replay,
        shot: path.map(|path| ShotRequest {
            path: PathBuf::from(path),
            frames,
            spawned: false,
        }),
        agents,
        harness,
        config,
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
