//! The sim: the round, stepped in `FixedUpdate` at the round's own rate.
//!
//! The round is a [`HostLoop`] -- the sequencer, the simulation and the
//! recorder of `lm-host-model` -- and it lives in a resource. It is stepped
//! in `FixedUpdate` at `Params::tick_hz`, which is the whole point: the sim
//! keeps its rate whatever the frame rate does. A frame that takes longer
//! than a tick is answered by several ticks (the fixed-timestep runner drains
//! the ones it owes), and a frame that takes less leaves a remainder the next
//! frame carries. No tick is skipped and none is played twice.
//!
//! What the host does *not* do here is decide anything. The loop is stepped,
//! the events it reports are kept for the piano roll, and the human's
//! decisions reach the very same sequencer the agents use (spec §6, D10
//! lesson 10). The host draws; it never rules.
//!
//! ## The round, live or replayed
//!
//! The app shows one of two things, and both are a [`Round`]: a round being
//! played, and a round being replayed out of a log. The live one is stepped
//! here; the replayed one is scrubbed, and the scrubber replays the log to
//! show the tick the viewer asks for. A file that cannot be played is a
//! third thing: the [`LogError`] it came back with, shown in the UI.
//!
//! ## Recording
//!
//! A round that is played is written down. [`HostLoop::finish`] seals the
//! log, and the log goes to `runs/<seed>-<unix_secs>.lml`: the seed says
//! which round it is, the second says when it was played. It is written when
//! the round ends -- by outcome or by the time limit -- and when the window
//! closes, so whatever was played is on disk either way, and the path is
//! printed. The file is the round itself: the headless tools replay it, and
//! the viewer opens it.

use bevy::app::OnAppExitSystems;
use bevy::prelude::*;
use lm_agents::mind::ThoughtRecord;
use lm_core::decision::Tick;
use lm_core::fixed::Pos;
use lm_core::header::Header;
use lm_core::log::LogError;
use lm_core::rules::{Event, Outcome};
use lm_core::sim::Sim;
use lm_host_model::host_loop::HostLoop;
use lm_host_model::record;
use lm_host_model::roll::RollModel;
use lm_host_model::score::{hollows_of, note_attention, AttentionTrace, Hollow, ScoreModel};
use lm_host_model::scrub::Scrubber;
use std::collections::BTreeMap;
use std::ops::Range;
use std::path::PathBuf;

/// The round the app is showing.
///
/// A live round and a replayed round are different animals: the live one is
/// stepped by the app and can be played, the replayed one is only looked at
/// and is scrubbed. They share what a renderer needs -- the world, the
/// header, the tick -- so the plugins ask the round, not the mode.
#[derive(Resource, Default)]
pub struct Round {
    /// The live round, when the app is playing one.
    pub live: Option<HostLoop>,
    /// The round being replayed, when the app is opening a log.
    pub replay: Option<Scrubber>,
    /// Why a file could not be opened, when one was asked for and failed.
    pub error: Option<LogError>,
    /// The file the round came from, when it came from one.
    pub source: Option<PathBuf>,
}

impl Round {
    /// A round the app is playing.
    pub fn live(host: HostLoop) -> Round {
        Round {
            live: Some(host),
            replay: None,
            error: None,
            source: None,
        }
    }

    /// A round the app is replaying out of a file.
    pub fn replay(scrubber: Scrubber, source: PathBuf) -> Round {
        Round {
            live: None,
            replay: Some(scrubber),
            error: None,
            source: Some(source),
        }
    }

    /// A file that could not be opened: the error the app shows.
    pub fn broken(err: LogError, source: PathBuf) -> Round {
        Round {
            live: None,
            replay: None,
            error: Some(err),
            source: Some(source),
        }
    }

    /// The world as it stands, if there is one.
    ///
    /// A live round shows its own world; a replayed one shows the world of
    /// the tick the scrubber is on.
    pub fn sim(&self) -> Option<&Sim> {
        match (&self.live, &self.replay) {
            (Some(host), _) => Some(host.sim()),
            (_, Some(scrubber)) => Some(scrubber.sim()),
            _ => None,
        }
    }

    /// The header of the round: its parameters, its roster and its rules.
    pub fn header(&self) -> Option<&Header> {
        match (&self.live, &self.replay) {
            (Some(host), _) => Some(host.header()),
            (_, Some(scrubber)) => Some(&scrubber.log().header),
            _ => None,
        }
    }

    /// Where every agent stood before the last tick.
    ///
    /// Only a live round keeps them: the renderer interpolates between them
    /// and the positions the agents are in now. A replayed round shows the
    /// states the log recorded, one tick at a time, and there is nothing to
    /// interpolate between.
    pub fn prev_positions(&self) -> &[(u16, Pos)] {
        self.live.as_ref().map_or(&[], |host| host.prev_positions())
    }

    /// The tick of the round being shown.
    ///
    /// A live round is on the tick it is about to simulate; a replayed one is
    /// on the tick the scrubber is showing.
    pub fn tick(&self) -> Tick {
        match (&self.live, &self.replay) {
            (Some(host), _) => host.sim().tick(),
            (_, Some(scrubber)) => scrubber.tick().unwrap_or(scrubber.log().header.start_tick),
            _ => 0,
        }
    }

    /// The score of every squad, in squad order.
    pub fn scores(&self) -> &[i64] {
        self.sim().map_or(&[], |sim| sim.scores())
    }

    /// How the round ended, if it has.
    pub fn outcome(&self) -> Option<&Outcome> {
        self.sim().and_then(Sim::outcome)
    }

    /// Whether the app is showing a round that is being replayed.
    pub fn is_replay(&self) -> bool {
        self.replay.is_some()
    }
}

/// Where the round is written down.
///
/// A round is recorded once: when it ends, or when the window closes.
#[derive(Resource, Default)]
pub struct Recording {
    /// The file the round is written to, if it is going to be written.
    pub path: Option<PathBuf>,
    /// Whether the round has been written already.
    pub written: bool,
}

/// The replay controls: where the viewer is in the round, and whether it plays.
///
/// The slider of the bottom panel moves `tick`; the play button moves
/// `playing`. The tick is the one thing the viewer asks for: the scrubber is
/// moved to it, and the round on screen is the round of that tick.
#[derive(Resource, Default)]
pub struct Replay {
    /// Whether the round plays itself back.
    pub playing: bool,
    /// The tick the viewer is asking for.
    pub tick: u64,
}

/// The events of the round so far, with the tick they happened on.
///
/// These are what the piano roll draws its spans from: a tag and a respawn
/// make a stun, a beacon taken and lost make a hold.
#[derive(Resource, Default)]
pub struct LiveEvents(pub Vec<(Tick, Event)>);

/// The piano roll of the round so far.
#[derive(Resource, Default)]
pub struct CurrentRoll {
    /// The roll itself: one lane per agent.
    pub model: RollModel,
    /// The ticks it covers.
    pub window: Range<Tick>,
    /// The agents' thoughts: what was asked, and when the answer landed.
    ///
    /// A thought is not a decision — it is the question behind it, and how
    /// long the answer took. The roll draws them as bars under the marks.
    pub thoughts: BTreeMap<u16, Vec<ThoughtRecord>>,
}

/// How many ticks the piano roll shows while a round is being played.
pub const ROLL_TICKS: u64 = 300;

/// How many seconds of the round the conductor's score draws, unless the
/// command line says otherwise (`--score-seconds`).
pub const SCORE_SECONDS: u32 = 20;

/// The score's zoom: how many seconds of the round it draws.
///
/// A score is read at a glance, so it is a window on the last moments of
/// the round — the whole round is what the replay scrubber is for, and the
/// two share one clock.
#[derive(Resource)]
pub struct ScoreZoom(pub u32);

impl Default for ScoreZoom {
    fn default() -> Self {
        ScoreZoom(SCORE_SECONDS)
    }
}

/// The conductor's score of the round so far (E7).
///
/// It is built exactly as the roll is built — from the log and, live, from
/// what the host knows that the log does not carry — with one thing more:
/// the agents' attention, sampled once per played tick. A replayed round
/// carries the same samples, taken by the scrubber's opening replay, so
/// both modes of the round draw the same score.
#[derive(Resource, Default)]
pub struct CurrentScore {
    /// The score itself: one system of staves per agent.
    pub model: ScoreModel,
    /// The ticks it covers.
    pub window: Range<Tick>,
    /// The attention trace it is built from: per agent, the tick each run
    /// of attention began on.
    pub attention: AttentionTrace,
    /// The hollow marks so far: the refusals and unsure gates of every
    /// mind that is playing the round.
    ///
    /// A log carries no refusals (D26 records them with the mind), so a
    /// round opened straight from a file has none; a round the app played
    /// itself keeps the ones it collected while playing, through the
    /// replay it is reopened as.
    pub hollow: BTreeMap<u16, Vec<Hollow>>,
    /// How many refusals of each mind have been taken already, so each
    /// tick appends only the new ones.
    taken: BTreeMap<u16, usize>,
}

impl CurrentScore {
    /// Take the refusals that arrived since the last look.
    ///
    /// The minds keep their whole refusal history; the score keeps its own
    /// copy as hollow marks, appending the delta each tick.
    fn take_new_refusals(&mut self, host: &HostLoop) {
        let Some(agents) = host.agents() else {
            return;
        };
        for (&agent, mind) in agents.minds() {
            let refusals = mind.refusals();
            let taken = *self.taken.entry(agent).or_insert(0);
            if taken >= refusals.len() {
                continue;
            }
            let fresh = hollows_of(&refusals[taken..]);
            self.hollow.entry(agent).or_default().extend(fresh);
            self.taken.insert(agent, refusals.len());
        }
    }
}

/// The ticks the score is drawn over: the last `span` ticks up to `now`.
///
/// While the round is younger than the span the window stays anchored at
/// the round's start (so an early tick still sees a full window instead of
/// a sliver), and a round with an end never draws past it.
fn score_window(start: Tick, span: u64, now: Tick, end: Option<Tick>) -> Range<Tick> {
    let mut last = now.saturating_add(1).max(start.saturating_add(span));
    if let Some(end) = end {
        last = last.min(end.saturating_add(1));
    }
    last.saturating_sub(span)..last
}

/// The sim plugin: the round, and the schedule it runs on.
pub struct SimPlugin {
    /// The rate the round ticks at: `Params::tick_hz`.
    tick_hz: u16,
}

impl SimPlugin {
    /// A plugin that steps the round at `tick_hz` ticks a second.
    pub fn new(tick_hz: u16) -> Self {
        SimPlugin { tick_hz }
    }
}

/// The systems of the round, as one set.
///
/// The set is there so that what has to happen *before* the round ticks can
/// say so: the human's ear moves before the round asks for the clips it can
/// hear.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum SimSet {
    /// The round ticks.
    Step,
}

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        // The fixed timestep is the round's own rate: the sim runs at
        // `Params::tick_hz`, decoupled from the frame rate (the plan's
        // global constraint).
        app.insert_resource(Time::<Fixed>::from_hz(f64::from(self.tick_hz)))
            .init_resource::<LiveEvents>()
            .init_resource::<CurrentRoll>()
            .init_resource::<CurrentScore>()
            .init_resource::<ScoreZoom>()
            .init_resource::<Recording>()
            .init_resource::<Replay>()
            // The round ticks, and a replay plays itself back at the very
            // same rate: one tick of the log per fixed update.
            .add_systems(FixedUpdate, (step_round, step_replay).in_set(SimSet::Step))
            // The round is written down when it is over, and the replay is
            // kept in step with the tick the viewer is asking for.
            .add_systems(
                Update,
                (
                    write_when_over,
                    apply_seek,
                    update_replay_roll,
                    update_replay_score,
                ),
            )
            // And when the window closes, whatever was played is written down.
            .add_systems(
                Last,
                write_on_exit
                    .in_set(OnAppExitSystems)
                    .run_if(|messages: Res<Messages<AppExit>>| !messages.is_empty()),
            );
    }
}

/// Play one tick of the round, and keep what it reported.
fn step_round(
    mut round: ResMut<Round>,
    mut events: ResMut<LiveEvents>,
    mut roll: ResMut<CurrentRoll>,
    mut score: ResMut<CurrentScore>,
    zoom: Res<ScoreZoom>,
) {
    let Some(host) = round.live.as_mut() else {
        return;
    };

    // A round that is over is not played any further: it has been written
    // down, and what is left to do with it is to look at it.
    if host.sim().outcome().is_some() {
        return;
    }

    let out = host.tick();
    for event in out.events {
        events.0.push((out.tick, event));
    }

    // The agents' attention as their own state now holds it: the sample the
    // score's gaze and tier staves read, and the refusal delta of its
    // hollow marks. Both are read after the tick they belong to -- the one
    // the sim has just moved past.
    let played = host.sim().tick().saturating_sub(1);
    note_attention(host.sim(), &mut score.attention, played);
    score.take_new_refusals(host);
    let log = host.log_so_far();

    // The piano roll is a picture of the last few hundred ticks. It is
    // redrawn when the round moves on -- once a tick -- not once a frame.
    let now = host.sim().tick();
    let window = now.saturating_sub(ROLL_TICKS)..now.saturating_add(1);
    roll.model = RollModel::from_log(&log, &events.0, window.clone());
    roll.window = window;
    // The thoughts of the agents: the questions they asked, and how long the
    // answers took.
    roll.thoughts = host.agent_thoughts_all();

    // The score is a picture of the last few seconds, drawn over the same
    // log on the same tick.
    let span = score_span(&zoom, host.header().params.tick_hz);
    let window = score_window(log.header.start_tick, span, now, None);
    score.model = ScoreModel::from_log(&log, &score.attention, &score.hollow, window.clone());
    score.window = window;
}

/// The score's span in ticks: its seconds on the round's own clock.
fn score_span(zoom: &ScoreZoom, tick_hz: u16) -> u64 {
    u64::from(zoom.0.max(1)) * u64::from(tick_hz.max(1))
}

/// Play the replay forward: one tick of the log per fixed update.
///
/// The fixed update runs at the round's own rate, so a replay plays at the
/// rate the round was played at.
fn step_replay(round: Res<Round>, mut replay: ResMut<Replay>) {
    if !replay.playing {
        return;
    }
    let Some(scrubber) = round.replay.as_ref() else {
        return;
    };

    // The end of the round is the end of the replay.
    if replay.tick >= scrubber.last_tick() {
        replay.playing = false;
        return;
    }
    replay.tick += 1;
}

/// Show the tick the viewer is asking for.
fn apply_seek(mut round: ResMut<Round>, replay: Res<Replay>) {
    let Some(scrubber) = round.replay.as_mut() else {
        return;
    };
    if scrubber.tick() == Some(replay.tick) {
        return;
    }
    scrubber.seek(replay.tick);
}

/// Keep the piano roll in step with the round being replayed.
///
/// A replay shows the whole round: the lanes are the whole round's lanes,
/// and the slider of the bottom panel is on the same scale.
fn update_replay_roll(mut roll: ResMut<CurrentRoll>, round: Res<Round>) {
    let Some(scrubber) = round.replay.as_ref() else {
        return;
    };
    let window = replay_window(scrubber);
    if roll.window == window {
        return;
    }
    roll.model = RollModel::from_log(scrubber.log(), scrubber.events(), window.clone());
    roll.window = window;
}

/// Keep the score in step with the round being replayed.
///
/// The scrubber says which tick the round is on, and the score draws the
/// last few seconds up to that tick -- so the scrubber's slider pans the
/// score exactly as the round's own ticking scrolls it while it is played.
/// The hollow marks are the ones the app collected while the round was
/// being played; a log opened from a file carries none (the log holds no
/// refusals), and the score says so with its empty staves rather than by
/// inventing marks.
fn update_replay_score(mut score: ResMut<CurrentScore>, round: Res<Round>, zoom: Res<ScoreZoom>) {
    let Some(scrubber) = round.replay.as_ref() else {
        return;
    };
    let now = scrubber
        .tick()
        .unwrap_or_else(|| scrubber.log().header.start_tick);
    let span = score_span(&zoom, scrubber.log().header.params.tick_hz);
    let window = score_window(
        scrubber.log().header.start_tick,
        span,
        now,
        Some(scrubber.last_tick()),
    );
    if score.window == window {
        return;
    }
    score.model = ScoreModel::from_log(
        scrubber.log(),
        scrubber.attention(),
        &score.hollow,
        window.clone(),
    );
    score.window = window;
}

/// The ticks a replayed round is shown over: the whole round.
fn replay_window(scrubber: &Scrubber) -> Range<Tick> {
    let start = scrubber.log().header.start_tick;
    start..scrubber.last_tick().saturating_add(1)
}

/// Write the round down when it is over.
///
/// The round ends by outcome or by the time limit; either way it is written
/// down, and the app opens the file it just wrote so that the last state of
/// the round stays on screen and can be walked back through.
fn write_when_over(world: &mut World) {
    let over = world
        .get_resource::<Round>()
        .and_then(|round| round.live.as_ref())
        .is_some_and(|host| host.sim().outcome().is_some());
    if over {
        write_round(world, true);
    }
}

/// Write the round down when the window closes.
///
/// This is the `OnAppExitSystems` hook: it runs after the window plugin has
/// said the app is closing, so whatever was played is on disk.
fn write_on_exit(world: &mut World) {
    write_round(world, false);
}

/// Write the log of the live round to its file, and say where it went.
///
/// The round is finished with `HostLoop::finish`, which seals the log: the
/// file is the round, exactly as the headless tools write it. It is written
/// once -- a round that is over and a window that closes in the same frame
/// are the same round, not two.
fn write_round(world: &mut World, reopen: bool) {
    // Already written? Then this is a second chance, not a second file.
    if world.resource::<Recording>().written {
        return;
    }
    let Some(path) = world.resource::<Recording>().path.clone() else {
        return;
    };

    // The live round is finished, so its log is sealed.
    let log = {
        let mut round = world.resource_mut::<Round>();
        let Some(host) = round.live.take() else {
            return;
        };
        host.finish()
    };

    // The file.
    if let Err(err) = record::write(&log, &path) {
        eprintln!(
            "lm-host: the round could not be written to {}: {err}",
            path.display()
        );
        return;
    }
    println!("lm-host: recorded {}", path.display());
    world.resource_mut::<Recording>().written = true;

    // The round that was just played is now a round that can be scrubbed:
    // the app opens its own log, so the last state of the round stays on
    // screen and the person can walk back through what they just played.
    if reopen {
        match Scrubber::open(&path) {
            Ok(scrubber) => {
                let last = scrubber.last_tick();
                let mut round = world.resource_mut::<Round>();
                round.replay = Some(scrubber);
                round.source = Some(path.clone());
                world.resource_mut::<Replay>().tick = last;
            }
            Err(err) => {
                eprintln!("lm-host: the round that was recorded does not replay: {err}");
                world.resource_mut::<Round>().error = Some(err);
            }
        }
    }
}
