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
pub mod lane;
pub mod log;
pub mod params;
pub mod rng;
pub mod rules;
pub mod sched;
pub mod trig_table;
pub mod view;
pub mod world;

pub use agent::{ActiveIntent, Agent, MotorCmd};
pub use decision::{Action, Decision, Intent, Kind, Lane, Provenance, Roll, Source, Stamped, Tick};
pub use fixed::{Angle, Pos, CHUNK_UNITS, UNITS_PER_M};
pub use hash::{Fnv64, StateHash};
pub use header::{Header, Role, RosterEntry, PROTO_VERSION};
pub use lane::LaneConditioner;
pub use log::{LogError, Recorder, RoundLog, Segment, MAGIC};
pub use params::{ParamError, Params};
pub use rng::{Domain, Rng};
pub use rules::{Beacons, Event, Outcome, Ruleset, SimError};
pub use sched::{lead_ticks, Reject, SchedStats, Sequencer};
pub use trig_table::{SIN_Q14, TRIG_TABLE_HASH};
pub use view::{
    menu, option_cmd, AgentSnap, BeaconSnap, EnemySeen, Menu, MenuOption, ScoreView, SquadKnowledge,
};
pub use world::CellXY;
