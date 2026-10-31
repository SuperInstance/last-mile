//! `lm-tools`: the headless host and the `lm` CLI.
//!
//! The crate is a library *and* a binary. The library holds the runner that
//! plays a round without a screen, so the phase-2 host can drive the same
//! code the tests drive; the binary is the `lm` command that runs, replays,
//! diffs and golden-checks rounds.
//!
//! The runner holds no clock, no randomness and no environment reads: the
//! same header and the same decisions always produce the same log, byte for
//! byte. That is the whole point of the crate.

pub mod branch;
pub mod runner;
pub mod vibe;
pub mod voice;
