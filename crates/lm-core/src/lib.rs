//! `lm-core`: the deterministic simulation core for last-mile M1.
//!
//! The crate is pure integer math: no floats, no clock, no environment
//! reads, no unordered containers and no networking. Everything the world,
//! the agents and the rules need is computed from the round header, so two
//! machines that run the same seed and the same decisions produce the same
//! bytes.

#![deny(
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::cast_precision_loss
)]
#![forbid(unsafe_code)]

pub mod agent;
pub mod decision;
pub mod fixed;
pub mod hash;
pub mod header;
pub mod hearing;
pub mod lane;
pub mod log;
pub mod opal;
pub mod params;
pub mod rng;
pub mod rules;
pub mod sched;
pub mod script;
pub mod sim;
pub mod talk;
pub mod trig_table;
pub mod utterance;
pub mod view;
pub mod world;

pub use agent::{ActiveIntent, Agent, MotorCmd};
pub use decision::{Action, Decision, Intent, Kind, Lane, Provenance, Roll, Source, Stamped, Tick};
pub use fixed::{Angle, Pos, CHUNK_UNITS, UNITS_PER_M};
pub use hash::{Fnv64, StateHash};
pub use header::{Header, HeaderV1, Lineage, LineageChange, Role, RosterEntry, PROTO_VERSION};
pub use hearing::{
    audible_to, audible_to_in_world, clarity, clarity_between, degrade, degrade_at, dist_units,
    range_units, DEFAULT_CELL_UNITS,
};
pub use lane::LaneConditioner;
pub use log::{LogError, Recorder, RoundLog, RoundLogV1, Segment, MAGIC, MIN_READABLE_VERSION};
pub use opal::{OpalError, TalkContext};
pub use params::{ParamError, Params, BRANCH_FIELDS};
pub use rng::{Domain, Rng};
pub use rules::{Beacons, Event, Outcome, Refusal, RefusalKind, RhaiRuleset, Ruleset, SimError};
pub use sched::{lead_ticks, Reject, SchedStats, Sequencer};
pub use script::{ReflexOut, RhaiRuntime, ScriptError, ScriptRuntime};
pub use sim::{Sim, SimStats, StepOut};
pub use talk::{ListenerRecord, TalkParams, TalkRecord};
pub use trig_table::{SIN_Q14, TRIG_TABLE_HASH};
// The speech intent is `TalkIntent` at the crate root: `Intent` already
// names the decision's plan (the thing an agent intends to do), and the
// speech intent is the thing a line is *for*.
pub use utterance::{
    Addressee, Affect, Channel, Heard, Intent as TalkIntent, Prosody, SpeechAct, Subject,
    Utterance, Word,
};
pub use view::{
    menu, option_cmd, AgentSnap, BeaconSnap, EnemySeen, Menu, MenuOption, ScoreView, SquadKnowledge,
};
pub use world::{walls_between, CellXY};
