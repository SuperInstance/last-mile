//! `schedproto` — a throwaway prototype that de-risks phase 1 of the
//! last-mile scheduler.
//!
//! It models the decision schedule of the last mile at 30 ticks/s:
//!
//! * [`seq::Sequencer`] stamps submissions (on time / late / stale) and
//!   defines the canonical order inside a tick;
//! * [`lane::LaneConditioner`] picks a lane per agent per gate with an
//!   exponential smoother and a deadband;
//! * [`world::World`] runs the agents, including intents with a mandatory
//!   safe branch;
//! * [`log`] is the binary decision log, [`replay`] re-runs it and must
//!   reproduce the hash sequence bit for bit.
//!
//! std only — no external crates.

pub mod driver;
pub mod hash;
pub mod lane;
pub mod log;
pub mod replay;
pub mod rng;
pub mod seq;
pub mod types;
pub mod world;

pub use driver::Driver;
pub use hash::{fnv1a64, Fnv1a64};
pub use lane::{pct_to_q16, LaneConditioner, ALPHA_SHIFT, DEADBAND_Q16, Q16_ONE};
pub use log::{
    decode_log, decode_records, encode_log, encode_record, record_count, record_offsets, split_log,
    LogError, MAGIC, RECORD_HEADER_BYTES,
};
pub use replay::{replay_bytes, replay_records, ReplayResult, Replayer};
pub use rng::XorShift64Star;
pub use seq::{
    canonical_key, canonical_sort, canonical_sort_slice, rotation_slot, SeqStats, Sequencer,
    DEFAULT_STALE_WINDOW,
};
pub use types::{Decision, Kind, Stamped};
pub use world::{step_dir, Agent, IntentState, Mode, World, HASH_EVERY};
