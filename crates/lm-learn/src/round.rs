//! Playing a round, headless, with nothing but the sim.
//!
//! The learning flow plays rounds with no injected decisions: every agent is
//! driven by its reflex, so a round is a pure function of its header. That is
//! the same round `lm run` plays, tick for tick — the log is the same log —
//! and `lm-learn` keeps its own player so the crate stays free of the CLI's
//! crate (the CLI is what calls the flow, so the dependency cannot point the
//! other way).

use lm_core::header::Header;
use lm_core::log::{Recorder, RoundLog};
use lm_core::sched::Sequencer;
use lm_core::sim::Sim;

/// Play a round and record it.
///
/// The round runs until `ticks` have been played or the rules end it. The
/// log holds every tick that was played, with the state hash of each, so the
/// round can be replayed and checked later.
pub fn play(header: Header, ticks: u64) -> RoundLog {
    let mut sim = match Sim::new(&header) {
        Ok(sim) => sim,
        Err(err) => panic!("the round does not start: {err}"),
    };

    let start = header.start_tick;
    let n_agents = u16::try_from(header.roster.len()).unwrap_or(u16::MAX);
    let mut sequencer = Sequencer::new(&header.params, n_agents, start);
    let mut recorder = Recorder::new(header.clone(), header.params.segment_ticks);

    let end = start.saturating_add(ticks);
    let mut tick = start;
    while tick < end {
        // The decisions of this tick (none: nothing is injected), then the
        // tick itself, then the hash.
        let applied = sequencer.drain(tick);
        let out = sim.step(&applied);
        recorder.record(tick, &applied, out.hash);
        tick += 1;
        if sim.outcome().is_some() {
            break;
        }
    }

    recorder.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    const REFLEX: &str = include_str!("../../../assets/reflexes/default.rhai");

    #[test]
    fn a_played_round_records_its_ticks() {
        let header = Header::default_round(1, REFLEX);
        let log = play(header, 30);
        assert_eq!(log.segments.len(), 1);
        assert_eq!(log.segments[0].first_tick, 0);
        assert_eq!(log.segments[0].last_tick, 29);
        assert!(log.verify().is_ok());
    }

    #[test]
    fn the_same_header_plays_the_same_round() {
        let a = play(Header::default_round(7, REFLEX), 40);
        let b = play(Header::default_round(7, REFLEX), 40);
        assert_eq!(a.hashes(), b.hashes());
        assert_eq!(a.header, b.header);
    }

    #[test]
    #[should_panic(expected = "the round does not start")]
    fn a_script_that_does_not_compile_stops_the_round() {
        // A reflex with a float in it cannot compile (the sandbox has no
        // floats), so the round never starts.
        let header = Header::default_round(1, "fn reflex(v) { 1.5 }");
        play(header, 10);
    }
}
