//! The sequencer: stamping, lateness accounting and canonical ordering.

use crate::types::{Decision, Stamped};

/// Decisions that arrive more than this many ticks late are dropped.
pub const DEFAULT_STALE_WINDOW: u64 = 15;

/// Counters kept by the sequencer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SeqStats {
    pub accepted: u64,
    pub late: u64,
    pub dropped: u64,
    pub lateness_total: u64,
    pub lateness_max: u32,
}

/// Stamps submitted decisions and enforces the stale window.
///
/// Stamping rules (arrival tick `a`, requested apply tick `t`):
/// * `a <= t` — on time, the decision keeps its requested tick.
/// * `a > t` and `a - t <= stale_window` — late: the decision is pushed to the
///   next tick after it arrived (`a + 1`), because a late decision can never be
///   applied retroactively, and the lateness is counted.
/// * `a - t > stale_window` — stale: the decision is dropped.
#[derive(Clone, Debug)]
pub struct Sequencer {
    stale_window: u64,
    pub stats: SeqStats,
}

impl Default for Sequencer {
    fn default() -> Self {
        Self::new()
    }
}

impl Sequencer {
    pub fn new() -> Self {
        Sequencer::with_stale_window(DEFAULT_STALE_WINDOW)
    }

    pub fn with_stale_window(stale_window: u64) -> Self {
        Sequencer { stale_window, stats: SeqStats::default() }
    }

    pub fn stale_window(&self) -> u64 {
        self.stale_window
    }

    /// Stamp a decision that arrived at `arrival_tick`.
    /// Returns `None` when the decision is stale and gets dropped.
    pub fn submit(&mut self, d: Decision, arrival_tick: u64) -> Option<Stamped> {
        if arrival_tick <= d.apply_tick {
            self.stats.accepted += 1;
            return Some(Stamped {
                apply_tick: d.apply_tick,
                source: d.source,
                seq: d.seq,
                kind: d.kind,
                arrival_tick,
                late: false,
                lateness: 0,
            });
        }

        let lateness = arrival_tick - d.apply_tick;
        if lateness > self.stale_window {
            self.stats.dropped += 1;
            return None;
        }

        self.stats.accepted += 1;
        self.stats.late += 1;
        self.stats.lateness_total += lateness;
        if lateness > self.stats.lateness_max as u64 {
            self.stats.lateness_max = lateness as u32;
        }
        Some(Stamped {
            apply_tick: arrival_tick + 1,
            source: d.source,
            seq: d.seq,
            kind: d.kind,
            arrival_tick,
            late: true,
            lateness: lateness as u32,
        })
    }
}

/// Fairness slot of a source inside a tick.
///
/// The source id is rotated by `tick % n_agents`, so every agent lands on a
/// different slot on every tick and no agent can monopolise a tick.
pub fn rotation_slot(source: u16, n_agents: usize, tick: u64) -> usize {
    if n_agents == 0 {
        return 0;
    }
    let s = source as usize % n_agents;
    (s + (tick as usize % n_agents)) % n_agents
}

/// Canonical ordering key: `(apply_tick, rotated source, seq, source)`.
pub fn canonical_key(s: &Stamped, n_agents: usize) -> (u64, usize, u32, u16) {
    (
        s.apply_tick,
        rotation_slot(s.source, n_agents, s.apply_tick),
        s.seq,
        s.source,
    )
}

/// Sort a tick's worth of stamped decisions into canonical order.
pub fn canonical_sort(items: &mut Vec<Stamped>, n_agents: usize) {
    items.sort_by(|a, b| canonical_key(a, n_agents).cmp(&canonical_key(b, n_agents)));
}

/// Canonical sort of any slice-like collection.
pub fn canonical_sort_slice(items: &mut [Stamped], n_agents: usize) {
    items.sort_by(|a, b| canonical_key(a, n_agents).cmp(&canonical_key(b, n_agents)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Kind;

    fn dec(tick: u64, source: u16, seq: u32) -> Decision {
        Decision { apply_tick: tick, source, seq, kind: Kind::Marker(seq) }
    }

    #[test]
    fn on_time_keeps_apply_tick() {
        let mut s = Sequencer::new();
        let st = s.submit(dec(100, 1, 0), 100).unwrap();
        assert_eq!(st.apply_tick, 100);
        assert!(!st.late);
        assert_eq!(st.lateness, 0);
        assert_eq!(s.stats.late, 0);
        assert_eq!(s.stats.accepted, 1);
    }

    #[test]
    fn early_arrival_keeps_apply_tick() {
        let mut s = Sequencer::new();
        let st = s.submit(dec(100, 1, 0), 40).unwrap();
        assert_eq!(st.apply_tick, 100);
        assert!(!st.late);
    }

    #[test]
    fn late_goes_to_next_tick_and_counts_lateness() {
        let mut s = Sequencer::new();
        let st = s.submit(dec(100, 1, 0), 105).unwrap();
        assert!(st.late);
        assert_eq!(st.lateness, 5);
        assert_eq!(st.apply_tick, 106, "late decisions land on the tick after arrival");
        assert_eq!(s.stats.late, 1);
        assert_eq!(s.stats.lateness_total, 5);
        assert_eq!(s.stats.lateness_max, 5);
    }

    #[test]
    fn stale_window_edge_is_inclusive() {
        let mut s = Sequencer::new();
        // exactly at the window: accepted, late
        let st = s.submit(dec(100, 1, 0), 115).unwrap();
        assert!(st.late);
        assert_eq!(st.lateness, 15);
        // one tick beyond the window: dropped
        assert!(s.submit(dec(200, 1, 1), 216).is_none());
        assert_eq!(s.stats.dropped, 1);
        assert_eq!(s.stats.accepted, 1);
    }

    #[test]
    fn stale_decisions_are_dropped() {
        let mut s = Sequencer::new();
        assert!(s.submit(dec(100, 1, 0), 116).is_none());
        assert!(s.submit(dec(100, 1, 0), 500).is_none());
        assert_eq!(s.stats.dropped, 2);
    }

    #[test]
    fn rotation_is_fair_across_ticks() {
        // source 0 rotates to a different slot every tick
        let slots: Vec<usize> = (0..4).map(|t| rotation_slot(0, 4, t)).collect();
        assert_eq!(slots, vec![0, 1, 2, 3]);
        // and the rotation is a permutation inside a tick
        let mut all: Vec<usize> = (0..4).map(|s| rotation_slot(s, 4, 1)).collect();
        all.sort();
        assert_eq!(all, vec![0, 1, 2, 3]);
    }

    #[test]
    fn canonical_order_is_tick_then_rotated_source_then_seq() {
        let mut items = vec![
            Stamped { apply_tick: 2, source: 0, seq: 7, kind: Kind::Marker(0), arrival_tick: 0, late: false, lateness: 0 },
            Stamped { apply_tick: 1, source: 3, seq: 0, kind: Kind::Marker(0), arrival_tick: 0, late: false, lateness: 0 },
            Stamped { apply_tick: 1, source: 1, seq: 9, kind: Kind::Marker(0), arrival_tick: 0, late: false, lateness: 0 },
            Stamped { apply_tick: 1, source: 1, seq: 2, kind: Kind::Marker(0), arrival_tick: 0, late: false, lateness: 0 },
        ];
        canonical_sort(&mut items, 4);
        let keys: Vec<(u64, usize, u32)> = items
            .iter()
            .map(|s| (s.apply_tick, rotation_slot(s.source, 4, s.apply_tick), s.seq))
            .collect();
        // tick 1: rotated slots -> source 1 -> 2, source 3 -> 0
        assert_eq!(
            keys,
            vec![
                (1, rotation_slot(3, 4, 1), 0),
                (1, rotation_slot(1, 4, 1), 2),
                (1, rotation_slot(1, 4, 1), 9),
                (2, rotation_slot(0, 4, 2), 7),
            ]
        );
    }
}
