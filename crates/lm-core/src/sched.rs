//! The sequencer: stamp decisions onto ticks, refuse the impossible, and
//! drain every tick in a fair, canonical order.
//!
//! The sim never waits (spec §3). A decision that arrives on time is applied
//! on the tick it asked for; a decision that arrives late is applied on the
//! tick it arrived on, with the lateness logged; a decision that is later than
//! the stale window is dropped. Every decision is either applied or refused
//! with a named reason — never silently lost.
//!
//! The sequencer is pure data: no clock, no randomness, no unordered
//! containers. The same submissions in the same order always produce the same
//! stamped stream, so a replay is bit-exact.

use std::collections::BTreeMap;

use crate::decision::{Decision, Source, Stamped, Tick};
use crate::params::Params;
use serde::{Deserialize, Serialize};

/// Milliseconds in a second: the scale that turns a latency into ticks.
const MS_PER_SEC: u64 = 1_000;

/// Why the sequencer refused a decision.
///
/// Every refusal is named and counted, so a round that loses decisions says
/// why instead of silently diverging.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reject {
    /// The decision asked for a tick that is already gone.
    Stale {
        /// How many ticks too late the decision was.
        late_by: u64,
    },
    /// The source reused a `seq` it had already used.
    DuplicateSeq,
    /// The source is not in the round's roster.
    UnknownSource,
    /// The decision claims to have arrived before the round started.
    BeforeStart,
}

/// How the sequencer is coping.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedStats {
    /// Decisions stamped on the tick they asked for.
    pub on_time: u64,
    /// Decisions stamped on their arrival tick instead.
    pub late: u64,
    /// Decisions dropped for being too late.
    pub stale: u64,
}

/// The sequencer: the authority that decides which tick each decision lands on.
///
/// It holds no clock and no randomness. The host hands it the arrival tick of
/// every decision and drains the tick it is about to simulate; everything else
/// is a pure function of the submissions, so two nodes that see the same
/// submissions see the same stamped stream.
#[derive(Clone, Debug)]
pub struct Sequencer {
    /// How many agents the round has.
    n_agents: u16,
    /// The tick the round starts at.
    start_tick: Tick,
    /// How late a decision may be before it is dropped.
    stale_window_ticks: u16,
    /// The stamped decisions waiting for their tick.
    pending: BTreeMap<Tick, Vec<Stamped>>,
    /// The last `seq` seen from each source.
    last_seq: BTreeMap<Source, u32>,
    /// The counters.
    stats: SchedStats,
}

impl Sequencer {
    /// A sequencer for a round of `n_agents` agents starting at `start_tick`.
    ///
    /// The tunables come from `Params` (`stale_window_ticks`) and are passed
    /// in, so this type holds no defaults of its own.
    pub fn new(p: &Params, n_agents: u16, start_tick: Tick) -> Self {
        Self {
            n_agents,
            start_tick,
            stale_window_ticks: p.stale_window_ticks,
            pending: BTreeMap::new(),
            last_seq: BTreeMap::new(),
            stats: SchedStats::default(),
        }
    }

    /// Take one decision and say which tick it will be applied on.
    ///
    /// `arrived` is the first not-yet-simulated tick: the tick the decision
    /// reached the sequencer.
    ///
    /// - `apply_tick >= arrived`: on time, stamped at `apply_tick`.
    /// - otherwise late by `arrived - apply_tick`: stamped at `arrived`, and
    ///   refused outright when it is later than the stale window.
    ///
    /// The `seq` of a source must strictly increase; a repeated one is
    /// refused. A decision from a source that is not in the roster is refused,
    /// and so is one that claims to have arrived before the round started.
    pub fn submit(&mut self, d: Decision, arrived: Tick) -> Result<Tick, Reject> {
        // An unknown source is never applied: it cannot be attributed to an
        // agent of this round.
        if !self.known_source(&d.source) {
            return Err(Reject::UnknownSource);
        }
        // The `seq` of a source must strictly increase.
        if let Some(&last) = self.last_seq.get(&d.source) {
            if d.seq <= last {
                return Err(Reject::DuplicateSeq);
            }
        }
        // A decision cannot arrive before the round it belongs to.
        if arrived < self.start_tick {
            return Err(Reject::BeforeStart);
        }
        // The decision is well formed from here on, so its `seq` is spent
        // whatever happens to it: a resend of the same `seq` is refused even
        // when the first copy was dropped as stale.
        self.last_seq.insert(d.source, d.seq);

        let (tick, late_by) = if d.apply_tick >= arrived {
            self.stats.on_time += 1;
            (d.apply_tick, 0)
        } else {
            let late_by = arrived - d.apply_tick;
            if late_by > u64::from(self.stale_window_ticks) {
                self.stats.stale += 1;
                return Err(Reject::Stale { late_by });
            }
            self.stats.late += 1;
            (arrived, late_by)
        };

        let stamped = Stamped {
            d,
            arrived,
            tick,
            late_by: u16::try_from(late_by).unwrap_or(u16::MAX),
        };
        self.pending.entry(tick).or_default().push(stamped);
        Ok(tick)
    }

    /// The decisions to apply on `tick`, in the canonical order.
    ///
    /// The order is system first, then the agents in a rotating order, and
    /// inside one rotation slot the human goes before the agent it possesses.
    /// `seq` breaks the remaining ties and is unique per source.
    pub fn drain(&mut self, tick: Tick) -> Vec<Stamped> {
        let mut out = self.pending.remove(&tick).unwrap_or_default();
        out.sort_by_key(|s| self.order_key(s, tick));
        out
    }

    /// The counters so far.
    pub fn stats(&self) -> SchedStats {
        self.stats
    }

    /// Is this source part of the round?
    ///
    /// The system is always there; an agent and the human playing as an
    /// agent are both roster ids, so both have to be inside the round.
    fn known_source(&self, source: &Source) -> bool {
        match source {
            Source::System => true,
            Source::Agent(i) | Source::Human(i) => *i < self.n_agents,
        }
    }

    /// The rotation slot of agent `i` at `tick`: `(i + n - tick % n) % n`.
    ///
    /// The slot rotates every tick, so the agent that goes first at tick `t`
    /// is the one that goes last at `t + 1` (the MIDI Maze lesson: nobody
    /// gets a permanent turn).
    fn rot(&self, i: u16, tick: Tick) -> u64 {
        let n = u64::from(self.n_agents);
        if n == 0 {
            return 0;
        }
        (u64::from(i) + n - tick % n) % n
    }

    /// The canonical order of one decision inside a tick.
    ///
    /// System decisions come first, then the agents in a rotating order, and
    /// inside one rotation slot the human goes before the agent it possesses.
    /// `seq` breaks the remaining ties, and it is unique per source.
    fn order_key(&self, s: &Stamped, tick: Tick) -> (u8, u64, u8, u32) {
        let (group, rot, rank) = match s.d.source {
            Source::System => (0, 0, 0),
            Source::Human(i) => (1, self.rot(i, tick), 0),
            Source::Agent(i) => (1, self.rot(i, tick), 1),
        };
        (group, rot, rank, s.d.seq)
    }
}

/// The decision lead: how many ticks ahead a decision has to be asked for.
///
/// D44: the host measures the one-way p99 of the decision path and adds a
/// margin; the lead is that latency in ticks, clamped to the round's minimum
/// and maximum. The lead only buys the decision time — the sim never waits.
pub fn lead_ticks(p: &Params, p99_one_way_ms: u32) -> u16 {
    let ms = u64::from(p99_one_way_ms) + u64::from(p.lead_margin_ms);
    let ticks = ms.saturating_mul(u64::from(p.tick_hz)).div_ceil(MS_PER_SEC);
    let lead = ticks
        .max(u64::from(p.min_lead_ticks))
        .min(u64::from(p.max_lead_ticks));
    u16::try_from(lead).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision::{Action, Kind, Provenance};

    /// The sequencer the plan's tests use: default params, 8 agents, start 0.
    fn seq() -> Sequencer {
        Sequencer::new(&Params::default(), 8, 0)
    }

    /// A decision from `agent`: hold, applied on `tick`, with `seq`.
    fn dec(agent: u16, tick: Tick, seq: u32) -> Decision {
        Decision {
            apply_tick: tick,
            source: Source::Agent(agent),
            seq,
            kind: Kind::ActionOn(Action::Hold),
            provenance: Provenance::Reflex,
        }
    }

    /// A decision from the human playing as `agent`.
    fn human_dec(agent: u16, tick: Tick, seq: u32) -> Decision {
        Decision {
            apply_tick: tick,
            source: Source::Human(agent),
            seq,
            kind: Kind::ActionOn(Action::Hold),
            provenance: Provenance::Human,
        }
    }

    /// A decision from the simulation itself.
    fn system_dec(tick: Tick, seq: u32) -> Decision {
        Decision {
            apply_tick: tick,
            source: Source::System,
            seq,
            kind: Kind::Marker(0),
            provenance: Provenance::Test,
        }
    }

    #[test]
    fn on_time_keeps_tick() {
        let mut s = seq();
        assert_eq!(s.submit(dec(0, 10, 1), 7), Ok(10));
    }

    #[test]
    fn late_goes_to_arrival() {
        let mut s = seq();
        assert_eq!(s.submit(dec(0, 10, 1), 12), Ok(12));
        assert_eq!(s.drain(12)[0].late_by, 2);
    }

    #[test]
    fn stale_is_dropped() {
        let mut s = seq();
        assert_eq!(
            s.submit(dec(0, 10, 1), 26),
            Err(Reject::Stale { late_by: 16 })
        );
        assert_eq!(s.stats().stale, 1);
    }

    #[test]
    fn rejects_duplicate_seq() {
        let mut s = seq();
        s.submit(dec(0, 10, 5), 1).unwrap();
        assert_eq!(s.submit(dec(0, 11, 5), 1), Err(Reject::DuplicateSeq));
    }

    #[test]
    fn rejects_unknown_source() {
        let mut s = seq();
        assert_eq!(s.submit(dec(99, 10, 1), 1), Err(Reject::UnknownSource));
    }

    #[test]
    fn drain_rotates_fairly() {
        let mut s = Sequencer::new(&Params::default(), 4, 0);
        for agent in 0..4u16 {
            s.submit(dec(agent, 5, 1), 0).unwrap();
            s.submit(dec(agent, 6, 2), 0).unwrap();
        }
        let five = s.drain(5);
        assert_eq!(five.len(), 4);
        assert_eq!(five[0].d.source, Source::Agent(1));
        let six = s.drain(6);
        assert_eq!(six.len(), 4);
        assert_eq!(six[0].d.source, Source::Agent(2));
    }

    #[test]
    fn lead_from_p99() {
        let p = Params::default();
        assert_eq!(lead_ticks(&p, 42), 3);
        assert_eq!(lead_ticks(&p, 110), 4);
        assert_eq!(lead_ticks(&p, 1_000), 9);
    }

    #[test]
    fn the_rotation_moves_every_agent_one_slot_per_tick() {
        let mut s = Sequencer::new(&Params::default(), 4, 0);
        for agent in 0..4u16 {
            s.submit(dec(agent, 5, 1), 0).unwrap();
            s.submit(dec(agent, 6, 2), 0).unwrap();
        }
        let order = |out: &[crate::decision::Stamped]| -> Vec<u16> {
            out.iter()
                .map(|s| s.d.source.agent().unwrap())
                .collect::<Vec<u16>>()
        };
        assert_eq!(order(&s.drain(5)), vec![1, 2, 3, 0]);
        assert_eq!(order(&s.drain(6)), vec![2, 3, 0, 1]);
    }

    #[test]
    fn rejects_before_start() {
        let mut s = Sequencer::new(&Params::default(), 8, 100);
        assert_eq!(s.submit(dec(0, 110, 1), 99), Err(Reject::BeforeStart));
        // The round itself is fine: a decision that arrives on time is kept.
        assert_eq!(s.submit(dec(0, 110, 1), 100), Ok(110));
    }

    #[test]
    fn rejects_an_unknown_human_too() {
        let mut s = seq();
        assert_eq!(s.submit(human_dec(8, 10, 1), 1), Err(Reject::UnknownSource));
    }

    #[test]
    fn seq_must_strictly_increase() {
        let mut s = seq();
        s.submit(dec(0, 10, 5), 1).unwrap();
        assert_eq!(s.submit(dec(0, 11, 4), 1), Err(Reject::DuplicateSeq));
    }

    #[test]
    fn a_stale_decision_spends_its_seq() {
        let mut s = seq();
        assert_eq!(
            s.submit(dec(0, 10, 7), 26),
            Err(Reject::Stale { late_by: 16 })
        );
        assert_eq!(s.submit(dec(0, 30, 7), 1), Err(Reject::DuplicateSeq));
    }

    #[test]
    fn system_sorts_first_and_the_human_before_its_agent() {
        let mut s = seq();
        s.submit(dec(0, 5, 1), 0).unwrap();
        s.submit(human_dec(0, 5, 1), 0).unwrap();
        s.submit(system_dec(5, 1), 0).unwrap();
        let out = s.drain(5);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].d.source, Source::System);
        assert_eq!(out[1].d.source, Source::Human(0));
        assert_eq!(out[2].d.source, Source::Agent(0));
    }

    #[test]
    fn one_source_drains_in_seq_order() {
        let mut s = seq();
        // A source's `seq` only ever grows, so its decisions drain in the
        // order they were submitted.
        s.submit(dec(0, 5, 1), 0).unwrap();
        s.submit(dec(0, 5, 2), 0).unwrap();
        let out = s.drain(5);
        assert_eq!(out[0].d.seq, 1);
        assert_eq!(out[1].d.seq, 2);
    }

    #[test]
    fn a_decision_waits_for_its_tick() {
        let mut s = seq();
        s.submit(dec(0, 10, 1), 5).unwrap();
        assert!(s.drain(9).is_empty());
        assert_eq!(s.drain(10).len(), 1);
        assert!(s.drain(10).is_empty());
    }

    #[test]
    fn counters_track_on_time_late_and_stale() {
        let mut s = seq();
        s.submit(dec(0, 10, 1), 5).unwrap(); // on time
        s.submit(dec(1, 10, 1), 12).unwrap(); // late by 2
        assert_eq!(
            s.submit(dec(2, 10, 1), 30),
            Err(Reject::Stale { late_by: 20 })
        );
        let stats = s.stats();
        assert_eq!((stats.on_time, stats.late, stats.stale), (1, 1, 1));
    }

    #[test]
    fn a_late_decision_is_applied_on_its_arrival_tick() {
        let mut s = seq();
        s.submit(dec(0, 10, 1), 12).unwrap();
        let out = s.drain(12);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].tick, 12);
        assert_eq!(out[0].arrived, 12);
        assert_eq!(out[0].late_by, 2);
        assert_eq!(out[0].d.apply_tick, 10);
    }

    #[test]
    fn a_tick_with_nothing_pending_drains_empty() {
        let mut s = seq();
        assert!(s.drain(7).is_empty());
    }

    #[test]
    fn lead_is_clamped_to_the_round_bounds() {
        let p = Params::default();
        assert_eq!(lead_ticks(&p, 0), p.min_lead_ticks);
        assert_eq!(lead_ticks(&p, u32::MAX), p.max_lead_ticks);
    }
}
