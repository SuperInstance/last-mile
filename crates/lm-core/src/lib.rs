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

pub mod fixed;
pub mod hash;
pub mod header;
pub mod lane;
pub mod params;
pub mod rng;
pub mod trig_table;

pub use fixed::{Angle, Pos, CHUNK_UNITS, UNITS_PER_M};
pub use hash::{Fnv64, StateHash};
pub use header::{Header, Role, RosterEntry, PROTO_VERSION};
pub use lane::LaneConditioner;
pub use params::{ParamError, Params};
pub use rng::{Domain, Rng};
pub use trig_table::{SIN_Q14, TRIG_TABLE_HASH};
