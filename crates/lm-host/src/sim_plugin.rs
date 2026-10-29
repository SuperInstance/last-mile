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

use bevy::prelude::*;
use lm_core::decision::Tick;
use lm_core::rules::Event;
use lm_host_model::host_loop::HostLoop;
use lm_host_model::roll::RollModel;
use std::ops::Range;

/// The round being played.
#[derive(Resource)]
pub struct Host(pub HostLoop);

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
}

/// How many ticks the piano roll shows.
pub const ROLL_TICKS: u64 = 300;

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

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        // The fixed timestep is the round's own rate: the sim runs at
        // `Params::tick_hz`, decoupled from the frame rate (the plan's
        // global constraint).
        app.insert_resource(Time::<Fixed>::from_hz(f64::from(self.tick_hz)))
            .init_resource::<LiveEvents>()
            .init_resource::<CurrentRoll>()
            .add_systems(FixedUpdate, step_round);
    }
}

/// Play one tick of the round, and keep what it reported.
fn step_round(
    mut host: ResMut<Host>,
    mut events: ResMut<LiveEvents>,
    mut roll: ResMut<CurrentRoll>,
) {
    let out = host.0.tick();
    for event in out.events {
        events.0.push((out.tick, event));
    }

    // The piano roll is a picture of the last few hundred ticks. It is
    // redrawn when the round moves on -- once a tick -- not once a frame.
    let now = host.0.sim().tick();
    let window = now.saturating_sub(ROLL_TICKS)..now.saturating_add(1);
    roll.model = RollModel::from_log(&host.0.log_so_far(), &events.0, window.clone());
    roll.window = window;
}
