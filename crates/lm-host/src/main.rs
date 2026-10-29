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
//! * [`view3d`] is the other view of the same round: the low-poly,
//!   vertex-coloured 3D world of `lm-render`, with its pipelines warmed
//!   before the round starts (`--view 3d`);
//! * [`input_plugin`] turns the keyboard and the mouse into decisions;
//! * [`hud_plugin`] is the bar at the top, [`roll_plugin`] the piano roll at
//!   the bottom;
//! * [`between_plugin`] runs the learning flow between the rounds -- on its
//!   own thread, never on the frame -- and shows what it decided.
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
//! ## The rounds between rounds
//!
//! When a round is over, the learning flow runs: it measures the round, asks
//! its reviewer what should change, seals the prediction of what that change
//! will do and tests it on held-out seeds. `--reviewer stub|router` says who
//! reviews: the stub needs no network and no key, the router asks the cloud.
//! The flow works on `--harness-root` (one directory per agent) and writes to
//! `--runs`. When it answers, a window says what it decided, and the human
//! may adopt it, refuse it, go against the measurement, or ask for a change.
//! "Next round" then plays round N+1 with the header the flow wrote.
//!
//! ## The voice
//!
//! `--talk off|stub|riva` says who speaks. `off` is no audio: the round still
//! talks, because the round's talk is decisions, and the person hears
//! nothing. `stub` is the deterministic backend — no network, no clock, the
//! same line always the same clip. `riva` is the real voice service, and it
//! is only used when the environment names it: `LM_RIVA_BASE` and
//! `LM_RIVA_TTS_PORT`. A port is never guessed (spec §12).
//!
//! Build and run it with Windows cargo via `scripts/win-build.sh`; this WSL
//! toolchain has none of Bevy's Linux system libraries.

mod audio_plugin;
mod between_plugin;
mod hud_plugin;
mod input_plugin;
mod keys;
mod map_plugin;
mod roll_plugin;
mod score_plugin;
mod sim_plugin;
mod view3d;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use bevy::{
    app::AppExit,
    camera::visibility::RenderLayers,
    prelude::*,
    render::{
        settings::{PowerPreference, RenderCreation, WgpuSettings},
        view::screenshot::{save_to_disk, Screenshot, ScreenshotCaptured},
        RenderPlugin,
    },
};
use bevy_egui::{EguiContext, EguiGlobalSettings, EguiPlugin, PrimaryEguiContext};
use lm_agents::config::AgentsConfig;
use lm_agents::harness::Harness;
use lm_agents::mind::Policy;
use lm_agents::sysone::{with_escalation, JevBackend, StubBackend, SysOne, ThorBackend};
use lm_agents::Agents;
use lm_core::header::Header;
use lm_host_model::host_loop::HostLoop;
use lm_host_model::input::{InputEvent, InputState};
use lm_host_model::record;
use lm_host_model::scrub::Scrubber;
use lm_host_model::talk::TalkConfig;
use lm_host_model::view::{BottomMode, BottomSwitch, ViewMode, ViewSwitch};
use lm_voice::{RivaTts, StubTts, Tts};
use std::{num::NonZeroU8, path::PathBuf};

/// The window the app opens with.
const DEFAULT_WINDOW: (u32, u32) = (1280, 720);
/// The seed the round starts from when nobody says otherwise.
const DEFAULT_SEED: u64 = 1;
/// The rate a round ticks at when the round itself cannot say.
const DEFAULT_TICK_HZ: u16 = 30;
/// The harness the agents think with: the shipped one.
const DEFAULT_HARNESS: &str = "assets/harness/default";
/// The render configuration of the 3D view, when there is one.
const RENDER_TOML: &str = "render.toml";
/// Where the flow's harnesses are: one directory per agent.
const DEFAULT_HARNESS_ROOT: &str = "runs/harness";
/// Where the flow writes: the record, the ledger and the deposits.
const DEFAULT_RUNS: &str = "runs";

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
    /// Which view of the round to draw.
    view: ViewMode,
    /// The agent to play as from the first frame, if any.
    possess: Option<u16>,
    /// The round log to replay, if any.
    replay: Option<PathBuf>,
    /// The screenshot to take, if any.
    shot: Option<ShotRequest>,
    /// Who is thinking: the reflex alone, a stub, or the Thor.
    agents: AgentChoice,
    /// Which voice backend the host talks through.
    talk: TalkChoice,
    /// Where the harness comes from: the gates and the policy.
    harness: PathBuf,
    /// The agents' configuration, when the command line names one.
    config: Option<PathBuf>,
    /// Who reviews the round between rounds.
    reviewer: between_plugin::ReviewerChoice,
    /// Where the flow's harnesses are: one directory per agent.
    harness_root: PathBuf,
    /// Where the flow writes.
    runs: PathBuf,
    /// How many ticks the learning round plays.
    learn_ticks: u64,
    /// How long the round is, in ticks, when the command line says.
    ticks: Option<u32>,
    /// Which mode the bottom panel opens in: the roll or the score.
    bottom: BottomMode,
    /// How many seconds of the round the score draws.
    score_seconds: u32,
    /// The tick a replay opens on, when the command line names one.
    scrub: Option<u64>,
}

/// The host's view switch, as a resource: which view draws, and the `B`
/// that flips it (phase 6 Task 8).
///
/// The switch is data — the round, its tick and its recorder are never
/// touched by a flip, which is `view_mode_switches_without_a_restart`'s
/// whole property, held by the model's type and stated by its test.
#[derive(Resource)]
struct Views(ViewSwitch);

/// The camera the egui panels ride (the `both` layout): the app's first
/// camera, above every view, drawing nothing of its own — the HUD and the
/// piano roll pass through it whatever view is underneath.
///
/// bevy_egui hangs its context on the first camera it sees, so this one is
/// spawned into the world before the app runs, before any view's camera.
#[derive(Component)]
struct PanelCarrier;

/// The render layer the panels ride on, apart from every view's own.
const PANEL_LAYER: usize = 2;

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

/// Which voice backend the host talks through.
///
/// `off` is no audio at all — the round still talks, because the round's
/// talk is decisions and not sound, and the person simply hears nothing.
/// `stub` is the deterministic backend: no network, no clock, the same line
/// always the same clip. `riva` is the real service, and it is only used
/// when the environment names it: a port is never guessed (spec §12).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TalkChoice {
    /// No audio: the round still talks, in silence.
    Off,
    /// The deterministic backend: no network, no clock.
    Stub,
    /// The real voice service, when the environment names it.
    Riva,
}

impl TalkChoice {
    /// The choice a command line spells out.
    fn parse(text: &str) -> Result<TalkChoice, String> {
        match text {
            "off" => Ok(TalkChoice::Off),
            "stub" => Ok(TalkChoice::Stub),
            "riva" => Ok(TalkChoice::Riva),
            other => Err(format!("`--talk` wants off, stub or riva, not `{other}`")),
        }
    }

    /// The choice, as it is written on the command line.
    fn as_str(self) -> &'static str {
        match self {
            TalkChoice::Off => "off",
            TalkChoice::Stub => "stub",
            TalkChoice::Riva => "riva",
        }
    }
}

fn main() -> AppExit {
    let options = match parse_args() {
        Ok(options) => options,
        Err(err) => {
            eprintln!("lm-host: {err}");
            eprintln!(
                "usage: lm-host [--seed N] [--window WxH] [--view 2d|3d|both] \
                 [--possess AGENT] [--replay FILE] [--screenshot PATH [--frames N]] \
                 [--agents off|stub|thor] [--talk off|stub|riva] [--harness DIR] \
                 [--agents-config FILE] [--reviewer stub|router] [--harness-root DIR] \
                 [--runs DIR] [--learn-ticks N] [--ticks N] \
                 [--bottom roll|score] [--score-seconds N] [--scrub N]"
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
            if options.scrub.is_some() {
                eprintln!("lm-host: a live round cannot open on a tick: `--scrub` is ignored");
            }
            let mut header = Header::default_round(options.seed, lm_tools::runner::DEFAULT_REFLEX);
            if let Some(ticks) = options.ticks {
                header.params.time_limit_ticks = ticks;
            }
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
                match attach_agents(
                    options.agents,
                    options.config.as_deref(),
                    &options.harness,
                    &header,
                    &mut host,
                ) {
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

            // The flow needs a harness of its own: one directory per agent.
            // When the root has none, the host seeds it from the round's
            // harness, so the flow has something to change.
            if let Err(err) =
                between_plugin::seed_harness_root(&options.harness_root, &options.harness)
            {
                eprintln!(
                    "lm-host: the harness root {} cannot be prepared: {err}",
                    options.harness_root.display()
                );
            }
            println!(
                "lm-host: between rounds the {} reviewer looks at what changed",
                options.reviewer.as_str()
            );

            // The voice. A backend that is not there is not a crash: the
            // round still talks, in silence, and the fallback is logged.
            if let Err(err) = attach_talk(&options, &mut host) {
                eprintln!("lm-host: {err}");
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

    // The flow, and what the next round needs to start.
    let flow_settings = between_plugin::FlowSettings {
        harness_root: options.harness_root.clone(),
        runs_root: options.runs.clone(),
        reviewer: options.reviewer,
        ticks: options.learn_ticks,
        stamp: unix_secs(),
    };
    let next_round = between_plugin::NextRound {
        agents: options.agents,
        config: options.config.clone(),
        harness: options.harness.clone(),
        harness_root: options.harness_root.clone(),
    };
    let mut app = App::new();
    app.add_plugins(
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
            //
            // Every layout with a 3D view in it also compiles its
            // pipelines synchronously: the frame that first draws the
            // warmup card blocks until they exist, so shader compilation
            // is a loading screen, never a mid-round stall (the
            // `night-street` lesson; see `view3d::warmup`).
            .set(RenderPlugin {
                render_creation: RenderCreation::Automatic(Box::new(WgpuSettings {
                    power_preference: PowerPreference::HighPerformance,
                    ..default()
                })),
                synchronous_pipeline_compilation: options.view != ViewMode::Map2d,
                ..default()
            }),
    );
    // The panels are egui's: the 2D view's own companions, and the `both`
    // layout's overlay over the 3D view.
    if options.view != ViewMode::View3d {
        app.add_plugins(EguiPlugin::default());
    }
    app.insert_resource(round)
        .insert_resource(recording)
        .insert_resource(input_plugin::Input(input))
        .init_resource::<FrameCount>()
        // The bottom panel opens in the mode the command line names (`E`
        // walks between them at runtime), the score draws as many seconds
        // as it asks for, and a replay can open on a named tick.
        .insert_resource(map_plugin::Bottom(BottomSwitch::new(options.bottom)))
        .insert_resource(sim_plugin::ScoreZoom(options.score_seconds))
        .add_plugins(sim_plugin::SimPlugin::new(tick_hz));
    if let Some(tick) = options.scrub {
        app.insert_resource(sim_plugin::Replay {
            playing: false,
            tick,
        });
    }

    match options.view {
        ViewMode::Map2d => {
            app.add_plugins((
                map_plugin::MapPlugin,
                input_plugin::InputPlugin,
                audio_plugin::VoicePlugin,
                hud_plugin::HudPlugin,
                roll_plugin::RollPlugin,
                // The between-round screen is egui, and it rides whatever
                // egui context the layout has: this branch's own, or the
                // `both` layout's panel carrier.
                between_plugin::BetweenPlugin::new(flow_settings, next_round),
            ))
            // What the person did is read before the map is drawn, so the
            // frame shows the order that was just given.
            .configure_sets(
                Update,
                (input_plugin::InputSet::Read, map_plugin::MapSet::Map).chain(),
            );
        }
        ViewMode::View3d => {
            // The 3D view's settings: `render.toml`, or the defaults the
            // plan pins. A file that cannot be read is a start-up error, not
            // a silent default.
            let settings = match view3d::View3dSettings::load(std::path::Path::new(RENDER_TOML)) {
                Ok(settings) => settings,
                Err(err) => {
                    eprintln!("lm-host: {err}");
                    return AppExit::Error(NonZeroU8::MIN);
                }
            };
            app.add_plugins(view3d::View3dPlugin { settings });
        }
        ViewMode::Both => {
            // The debugger's layout: the 3D view, with the egui HUD and
            // the piano roll over it (the piano roll is the M1 debugger
            // and keeps working in 3D), and the 2D map one `B` away. Both
            // views are installed; the switch says which draws.
            let settings = match view3d::View3dSettings::load(std::path::Path::new(RENDER_TOML)) {
                Ok(settings) => settings,
                Err(err) => {
                    eprintln!("lm-host: {err}");
                    return AppExit::Error(NonZeroU8::MIN);
                }
            };
            app.insert_resource(Views(ViewSwitch::new(ViewMode::Both)))
                .add_plugins((
                    view3d::View3dPlugin { settings },
                    map_plugin::MapPlugin,
                    input_plugin::InputPlugin,
                    audio_plugin::VoicePlugin,
                    hud_plugin::HudPlugin,
                    roll_plugin::RollPlugin,
                    // The between-round screen rides the panel carrier's
                    // egui context, the same one the HUD and the piano roll
                    // use, so it works in this layout too.
                    between_plugin::BetweenPlugin::new(flow_settings, next_round),
                ))
                .configure_sets(
                    Update,
                    (input_plugin::InputSet::Read, map_plugin::MapSet::Map).chain(),
                )
                // The panel carrier is the app's first camera, before any
                // view's own: bevy_egui hangs its context on the first
                // camera it sees, and the panels must ride above whatever
                // view is drawing, not inside one of them.
                //
                // The context is attached here, by hand, and the plugin's
                // own auto-attach is off: `setup_primary_egui_context_system`
                // runs in PreStartup *and* PreUpdate, each instance with its
                // own "already have one" flag, so with four cameras spawning
                // across both schedules it could pin a *second* primary
                // context onto one of the views' cameras — whichever the
                // query happened to yield first — and every panel system
                // that asks for "the" context would find two. One carrier,
                // one context, attached where it is meant to live.
                .add_systems(Update, enforce_view);
            app.insert_resource(EguiGlobalSettings {
                auto_create_primary_context: false,
                ..Default::default()
            });
            app.world_mut().spawn((
                Camera2d,
                Camera {
                    order: 10,
                    clear_color: ClearColorConfig::None,
                    ..default()
                },
                RenderLayers::layer(PANEL_LAYER),
                EguiContext::default(),
                PrimaryEguiContext,
                PanelCarrier,
            ));
        }
    }

    app.add_systems(Update, count_frames);

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
fn attach_agents(
    choice: AgentChoice,
    config: Option<&std::path::Path>,
    harness_dir: &std::path::Path,
    header: &Header,
    host: &mut HostLoop,
) -> Result<(), String> {
    let cfg = match config {
        Some(path) => AgentsConfig::load(path)
            .map_err(|err| format!("the configuration {} cannot be read: {err}", path.display()))?,
        None => AgentsConfig::default(),
    };
    let harness = Harness::open(harness_dir).map_err(|err| {
        format!(
            "the harness {} cannot be read: {err}",
            harness_dir.display()
        )
    })?;

    let backends = backends(choice, &cfg, &harness);
    if backends.is_empty() {
        return Err("no model is there to ask".to_owned());
    }
    let agents = Agents::new(&cfg, &harness, header, backends);
    host.attach_agents(agents);
    Ok(())
}

/// The voice backend, attached to the host.
///
/// `off` attaches nothing: the round still talks — the round's talk is
/// decisions, not sound — and the person hears nothing. `stub` attaches the
/// deterministic backend: no network, no clock, the same line always the
/// same clip. `riva` attaches the real service, but only when the
/// environment names it: a port is never guessed (spec §12).
///
/// A backend that is not there is not a crash. The round keeps talking, in
/// silence, and the fallback is logged.
fn attach_talk(options: &Options, host: &mut HostLoop) -> Result<(), String> {
    let tts: Arc<dyn Tts> = match options.talk {
        TalkChoice::Off => return Ok(()),
        TalkChoice::Stub => Arc::new(StubTts::default()),
        TalkChoice::Riva => {
            let Some(cfg) = lm_voice::riva_config_from_env() else {
                return Err(String::from(
                    "the voice service is not configured: LM_RIVA_BASE and LM_RIVA_TTS_PORT are not set; the round still talks, in silence",
                ));
            };
            let Some(tts) = RivaTts::from_config(&cfg) else {
                return Err(String::from(
                    "the voice service has no TTS port; the round still talks, in silence",
                ));
            };
            Arc::new(tts)
        }
    };
    println!(
        "lm-host: the round speaks with the {} voice",
        options.talk.as_str()
    );
    host.attach_talk(TalkConfig::default().with_tts(tts));
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
) -> BTreeMap<String, Box<dyn SysOne + Send + Sync>> {
    let mut out: BTreeMap<String, Box<dyn SysOne + Send + Sync>> = BTreeMap::new();
    match choice {
        AgentChoice::Off => {}
        AgentChoice::Stub => {
            // The stub answers every tier the policy names: the gates, the
            // policy and the ladder are the real ones, only the model is not.
            let policy = Policy::for_harness(&harness.policy_toml);
            let mut out: BTreeMap<String, Box<dyn SysOne + Send + Sync>> = BTreeMap::new();
            for tier in policy.tiers.tiers() {
                if tier.is_reflex() {
                    continue;
                }
                out.entry(tier.backend().to_owned()).or_insert_with(|| {
                    Box::new(StubBackend::menu()) as Box<dyn SysOne + Send + Sync>
                });
            }
            // The escalation (D71) is modelled by the stub too, so the gate
            // tests cover it without the Thor.
            with_escalation(out, cfg.escalate_below, &cfg.escalate_backend, |_| {
                Some(Box::new(StubBackend::menu()) as Box<dyn SysOne + Send + Sync>)
            })
        }
        AgentChoice::Thor => {
            // The Thor's models: one port each.
            let mut out: BTreeMap<String, Box<dyn SysOne + Send + Sync>> = BTreeMap::new();
            for (model, port) in &cfg.ports {
                let mut backend = ThorBackend::new(&cfg.thor_base, *port, model)
                    .with_timeout(Duration::from_millis(u64::from(cfg.request_timeout_ms)));
                let name = backend.name().to_owned();
                if let Some(min) = cfg.min_confidence.get(&name) {
                    backend = backend.with_min_confidence(*min);
                }
                out.insert(name, Box::new(backend));
            }
            // The escalation (D71): a gate the cheap model is unsure about
            // is asked of the second model, that one question alone.
            let thor = with_escalation(out, cfg.escalate_below, &cfg.escalate_backend, |_| {
                thor_backend(cfg, &cfg.escalate_backend)
            });
            out = thor;
            // The cloud tier, when its key is there. That key only ever goes
            // to api.typesafe.ai -- never to the Thor.
            match JevBackend::from_secrets() {
                Ok(jev) => {
                    let name = jev.name().to_owned();
                    out.insert(name, Box::new(jev));
                }
                Err(err) => eprintln!("lm-host: the cloud tier is not available: {err}"),
            }
            out
        }
    }
    out
}

/// The Thor backend a backend name stands for: `"thor-clef-flash"` is the
/// model `clef-flash` on its port.
fn thor_backend(cfg: &AgentsConfig, name: &str) -> Option<Box<dyn SysOne + Send + Sync>> {
    let model = name.strip_prefix("thor-")?;
    let port = *cfg.ports.get(model)?;
    let mut backend = ThorBackend::new(&cfg.thor_base, port, model)
        .with_timeout(Duration::from_millis(u64::from(cfg.request_timeout_ms)));
    let backend_name = backend.name().to_owned();
    if let Some(min) = cfg.min_confidence.get(&backend_name) {
        backend = backend.with_min_confidence(*min);
    }
    Some(Box::new(backend))
}

/// Read the command line.
fn parse_args() -> Result<Options, String> {
    let mut seed = DEFAULT_SEED;
    let mut window = DEFAULT_WINDOW;
    let mut view = ViewMode::Map2d;
    let mut possess = None;
    let mut replay = None;
    let mut path = None;
    let mut frames = 30u32;
    let mut agents = AgentChoice::Off;
    let mut talk = TalkChoice::Off;
    let mut harness = PathBuf::from(DEFAULT_HARNESS);
    let mut config = None;
    let mut reviewer = between_plugin::ReviewerChoice::Stub;
    let mut harness_root = PathBuf::from(DEFAULT_HARNESS_ROOT);
    let mut runs = PathBuf::from(DEFAULT_RUNS);
    let mut learn_ticks = lm_learn::flow::DEFAULT_TICKS;
    let mut ticks = None;
    let mut bottom = BottomMode::Roll;
    let mut score_seconds = sim_plugin::SCORE_SECONDS;
    let mut scrub = None;
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
            "--view" => {
                let text = args
                    .next()
                    .ok_or_else(|| String::from("`--view` needs 2d, 3d or both"))?;
                view = ViewMode::parse(&text)?;
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
            "--talk" => {
                let text = args
                    .next()
                    .ok_or_else(|| String::from("`--talk` needs off, stub or riva"))?;
                talk = TalkChoice::parse(&text)?;
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
            "--reviewer" => {
                let text = args
                    .next()
                    .ok_or_else(|| String::from("`--reviewer` needs stub or router"))?;
                reviewer = between_plugin::ReviewerChoice::parse(&text)?;
            }
            "--harness-root" => {
                harness_root = PathBuf::from(
                    args.next()
                        .ok_or_else(|| String::from("`--harness-root` needs a directory"))?,
                );
            }
            "--runs" => {
                runs = PathBuf::from(
                    args.next()
                        .ok_or_else(|| String::from("`--runs` needs a directory"))?,
                );
            }
            "--learn-ticks" => {
                learn_ticks = args
                    .next()
                    .ok_or_else(|| String::from("`--learn-ticks` needs a number"))?
                    .parse()
                    .map_err(|_| String::from("`--learn-ticks` needs a whole number"))?;
            }
            "--ticks" => {
                ticks = Some(
                    args.next()
                        .ok_or_else(|| String::from("`--ticks` needs a number"))?
                        .parse()
                        .map_err(|_| String::from("`--ticks` needs a whole number"))?,
                );
            }
            "--bottom" => {
                let text = args
                    .next()
                    .ok_or_else(|| String::from("`--bottom` needs roll or score"))?;
                bottom = BottomMode::parse(&text)?;
            }
            "--score-seconds" => {
                score_seconds = args
                    .next()
                    .ok_or_else(|| String::from("`--score-seconds` needs a number"))?
                    .parse()
                    .map_err(|_| String::from("`--score-seconds` needs a whole number"))?;
                if score_seconds == 0 {
                    return Err(String::from("`--score-seconds` must be at least 1"));
                }
            }
            "--scrub" => {
                scrub = Some(
                    args.next()
                        .ok_or_else(|| String::from("`--scrub` needs a tick"))?
                        .parse()
                        .map_err(|_| String::from("`--scrub` needs a whole number"))?,
                );
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
        view,
        possess,
        replay,
        shot: path.map(|path| ShotRequest {
            path: PathBuf::from(path),
            frames,
            spawned: false,
        }),
        agents,
        talk,
        harness,
        config,
        reviewer,
        harness_root,
        runs,
        learn_ticks,
        ticks,
        bottom,
        score_seconds,
        scrub,
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

/// Keep the cameras with the switch: `B` flips between the 2D map and the
/// 3D view, and the cameras follow — the map's camera or the 3D view's two,
/// one set on at a time, the panel carrier always. The round never knows.
#[allow(clippy::type_complexity)]
fn enforce_view(
    keys: Res<ButtonInput<KeyCode>>,
    mut views: ResMut<Views>,
    // The three camera queries are made provably disjoint (`Without` on the
    // other two's markers), or Bevy refuses the system (B0001). The map's
    // own marker names its camera outright.
    mut map_cameras: Query<
        &mut Camera,
        (
            With<map_plugin::MapCamera>,
            Without<view3d::upscale::BlitCamera>,
            Without<view3d::warmup::ViewCamera>,
        ),
    >,
    mut world_cameras: Query<
        &mut Camera,
        (
            With<view3d::warmup::ViewCamera>,
            Without<view3d::upscale::BlitCamera>,
            Without<Camera2d>,
        ),
    >,
    mut blit_cameras: Query<
        &mut Camera,
        (
            With<view3d::upscale::BlitCamera>,
            Without<view3d::warmup::ViewCamera>,
            Without<Camera2d>,
        ),
    >,
) {
    if keys.just_pressed(crate::keys::VIEW_FLIP.key) {
        views.0.toggle();
        println!(
            "lm-host: the {} view draws (B flips it)",
            if views.0.showing_3d() { "3D" } else { "2D map" }
        );
    }
    let show3d = views.0.showing_3d();
    for mut camera in &mut map_cameras {
        camera.is_active = !show3d;
    }
    for mut camera in &mut world_cameras {
        camera.is_active = show3d;
    }
    for mut camera in &mut blit_cameras {
        camera.is_active = show3d;
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The flip key flips the views, and the talk key does not — held
    /// against the real system and the real binding table, not a copy of
    /// the key. `V` once flipped the view too, and holding it to speak in
    /// `--view both` turned the world under the person; this is the test
    /// that says it cannot happen again.
    #[test]
    fn the_view_flips_on_its_key_and_not_on_the_talk_key() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(Views(ViewSwitch::new(ViewMode::Both)))
            .add_systems(Update, enforce_view);
        let map = app
            .world_mut()
            .spawn((Camera::default(), map_plugin::MapCamera))
            .id();
        let world = app
            .world_mut()
            .spawn((Camera::default(), view3d::warmup::ViewCamera))
            .id();
        let blit = app
            .world_mut()
            .spawn((Camera::default(), view3d::upscale::BlitCamera))
            .id();

        let is_active = |app: &mut App, entity| {
            app.world_mut()
                .get::<Camera>(entity)
                .expect("the camera exists")
                .is_active
        };
        let showing_3d = |app: &mut App| app.world().resource::<Views>().0.showing_3d();

        // The layout opens on the 3D view.
        app.update();
        assert!(showing_3d(&mut app));
        assert!(!is_active(&mut app, map));
        assert!(is_active(&mut app, world) && is_active(&mut app, blit));

        // The flip key turns the whole thing over to the 2D map.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(keys::VIEW_FLIP.key);
        app.update();
        assert!(
            !showing_3d(&mut app),
            "{} flips the views",
            keys::VIEW_FLIP.job
        );
        assert!(is_active(&mut app, map));
        assert!(!is_active(&mut app, world) && !is_active(&mut app, blit));

        // The talk key does nothing to the views: pressing it is speaking,
        // not flipping — even though it is the key that used to do both.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(keys::PUSH_TO_TALK.key);
        app.update();
        assert!(
            !showing_3d(&mut app),
            "{} must not flip the views",
            keys::PUSH_TO_TALK.job
        );
        assert!(is_active(&mut app, map));
        assert!(!is_active(&mut app, world) && !is_active(&mut app, blit));
    }
}
