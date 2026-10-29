//! `lm-host-model`: the mind of the host, without a screen.
//!
//! The host is the client a person plays on. It owns the round: the
//! sequencer that stamps every decision onto a tick, the simulation that
//! plays those ticks and the recorder that writes them down. This crate is
//! that ownership, as plain logic, with no window and no clock.
//!
//! The rule the whole crate is built around: **the host never decides game
//! outcomes** (spec §6, D10 lesson 10). A person playing the game does not
//! reach into the simulation and move an agent; every human action becomes a
//! [`lm_core::decision::Decision`] with `source: Human(agent)` and travels
//! through the same [`lm_core::sched::Sequencer`] the agents use. The host
//! and the agents stand on the same ground, and the log cannot tell them
//! apart except by their provenance.
//!
//! Two more rules shape the code here:
//!
//! * **Human decisions never wait.** They are submitted with `apply_tick` and
//!   `arrived` both set to the tick that is about to be simulated, so they
//!   land on the very next tick instead of sitting in a queue.
//! * **Floats are host-only.** Positions become `f32` metres inside the map
//!   model and inside the Bevy app, and never travel back into the core.
//!
//! The crate is deliberately free of Bevy: everything here is testable in
//! WSL, and `lm-host` only wires it to a screen.

pub mod host_loop;
pub mod turn;

pub use host_loop::{catch_up, HostLoop, TickBudget, TickOut};

// Declared up front so parallel phase-2 tasks (3: input, 4: map, 5: roll + scrub)
// each fill their own file and never collide here.
pub mod audio;
pub mod barks;
pub mod between;
pub mod input;
pub mod map;
pub mod record;
pub mod roll;
// The conductor's score: the roll grown into one system of staves per
// agent (Vox Score §12, E7).
pub mod score;
pub mod scrub;
// The human's talk: the voice backend, the bark cache and the human's ear.
pub mod talk;
// The human's voice: the commands, the tutor ladder and the score.
pub mod voice;
// Which view of the round the host draws, and the runtime switch between
// them (phase 6 Task 8); and the bottom panel's own switch between the
// piano roll and the conductor's score (Vox Score §12, E7).
pub mod view;

// Re-exported at the end of the file, so the parallel phase-2 tasks (4: map,
// 5: roll + scrub) can add their own without touching these lines.
pub use input::{InputError, InputEvent, InputState, Viewer};
pub use map::{AgentDraw, BeaconDraw, FogState, MapModel, SpeechTick};
pub use talk::{Listener, TalkClip, TalkConfig, TalkCounters};
pub use turn::{TurnDetector, TurnRules, MAX_TALK_MS, TURN_END_MS, VOICE_STOP_MS};
pub use view::{BottomMode, BottomSwitch};
pub use voice::{
    CommandOutcome, CommandParser, CommandRefusal, HumanVoice, TutorLadder, TutorMode, VoiceRefusal,
};
