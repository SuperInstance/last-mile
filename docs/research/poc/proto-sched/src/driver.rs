//! The live pipeline: submissions -> sequencer -> canonical tick groups ->
//! world + log.

use std::collections::BTreeMap;

use crate::log::{encode_record, MAGIC};
use crate::seq::{canonical_sort, Sequencer};
use crate::types::{Decision, Stamped};
use crate::world::{World, HASH_EVERY};

/// Drives the world and records every stamped decision in canonical order.
#[derive(Clone, Debug)]
pub struct Driver {
    pub seq: Sequencer,
    pub world: World,
    /// The decision log: magic + records, in canonical order.
    pub log: Vec<u8>,
    /// World hashes, one every `HASH_EVERY` ticks.
    pub hashes: Vec<u64>,
    pending: BTreeMap<u64, Vec<Stamped>>,
    n_agents: usize,
}

impl Driver {
    pub fn new(n_agents: usize, n_gates: usize) -> Self {
        Driver {
            seq: Sequencer::new(),
            world: World::new(n_agents, n_gates),
            log: MAGIC.to_vec(),
            hashes: Vec::new(),
            pending: BTreeMap::new(),
            n_agents,
        }
    }

    /// Submit a decision that arrived at `arrival_tick`.
    /// Returns the stamped decision, or `None` when it was dropped as stale.
    pub fn submit(&mut self, d: Decision, arrival_tick: u64) -> Option<Stamped> {
        let stamped = self.seq.submit(d, arrival_tick)?;
        let tick = stamped.apply_tick;
        self.pending.entry(tick).or_default().push(stamped.clone());
        Some(stamped)
    }

    /// Run the world until every pending group has been flushed.
    pub fn run_all(&mut self) {
        let last = match self.pending.keys().next_back() {
            Some(t) => *t,
            None => return,
        };
        while self.world.tick <= last {
            self.world.begin_tick();
            if let Some(group) = self.pending.remove(&self.world.tick) {
                let mut group = group;
                canonical_sort(&mut group, self.n_agents);
                for s in &group {
                    encode_record(&mut self.log, s);
                }
                for s in &group {
                    self.world.apply(s);
                }
            }
            self.world.move_agents();
            self.world.end_tick();
            if self.world.tick % HASH_EVERY == 0 {
                self.hashes.push(self.world.hash_state());
            }
        }
    }

    /// Number of decisions still waiting for their tick.
    pub fn pending_len(&self) -> usize {
        self.pending.values().map(|v| v.len()).sum()
    }
}
