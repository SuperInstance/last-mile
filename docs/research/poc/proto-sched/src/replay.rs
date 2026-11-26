//! Replay: decode the log, re-run the world, compare hash sequences.

use crate::log::{decode_records, LogError, MAGIC};
use crate::seq::canonical_sort;
use crate::types::Stamped;
use crate::world::{World, HASH_EVERY};

/// Outcome of a replay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayResult {
    /// World hash every `HASH_EVERY` ticks.
    pub hashes: Vec<u64>,
    /// Last hash of the sequence (0 when the run produced no hash).
    pub final_hash: u64,
    /// Ticks simulated.
    pub ticks: u64,
    /// Decisions applied.
    pub decisions: usize,
}

/// Replays a log, and can be resumed after a truncation.
#[derive(Clone, Debug)]
pub struct Replayer {
    pub world: World,
    pub hashes: Vec<u64>,
    records: Vec<Stamped>,
    idx: usize,
}

impl Replayer {
    pub fn new(n_agents: usize, n_gates: usize) -> Self {
        Replayer {
            world: World::new(n_agents, n_gates),
            hashes: Vec::new(),
            records: Vec::new(),
            idx: 0,
        }
    }

    /// Decode a log chunk and append it to the record list.
    /// The chunk may start with the magic header (a whole log) or be a bare
    /// sequence of records (the tail of a truncated log).
    /// Returns the number of records that were appended.
    pub fn push_bytes(&mut self, bytes: &[u8]) -> Result<usize, LogError> {
        let body = if bytes.len() >= MAGIC.len() && &bytes[..MAGIC.len()] == &MAGIC {
            &bytes[MAGIC.len()..]
        } else {
            bytes
        };
        let recs = decode_records(body)?;
        let n = recs.len();
        self.records.extend(recs);
        canonical_sort(&mut self.records, self.world.n_agents);
        Ok(n)
    }

    /// Apply the first `n` records and stop right there — possibly in the
    /// middle of a tick, which is exactly what a truncated log leaves behind.
    pub fn run_until_index(&mut self, n: usize) {
        self.run(Some(n));
    }

    /// Apply every remaining record and finish the run.
    pub fn run_all(&mut self) {
        self.run(None);
    }

    fn run(&mut self, stop_at: Option<usize>) {
        let last_tick = self.records.last().map(|r| r.apply_tick).unwrap_or(0);
        loop {
            if !self.world.tick_started {
                self.world.begin_tick();
            }
            let limit = stop_at.unwrap_or(self.records.len()).min(self.records.len());
            while self.idx < limit && self.records[self.idx].apply_tick == self.world.tick {
                let r = self.records[self.idx].clone();
                self.world.apply(&r);
                self.idx += 1;
            }
            match stop_at {
                // A truncation boundary: stop before the tick is finished, the
                // world is left exactly where the cut happened.
                Some(limit) => {
                    if self.idx >= limit {
                        return;
                    }
                }
                None => {
                    if self.idx >= self.records.len() && self.world.tick > last_tick {
                        return;
                    }
                }
            }
            self.world.move_agents();
            self.world.end_tick();
            if self.world.tick % HASH_EVERY == 0 {
                self.hashes.push(self.world.hash_state());
            }
        }
    }

    /// Result of the run so far.
    pub fn result(&self) -> ReplayResult {
        ReplayResult {
            hashes: self.hashes.clone(),
            final_hash: self.hashes.last().copied().unwrap_or(0),
            ticks: self.world.tick,
            decisions: self.idx,
        }
    }
}

/// Replay a whole log.
pub fn replay_bytes(bytes: &[u8], n_agents: usize, n_gates: usize) -> Result<ReplayResult, LogError> {
    let mut r = Replayer::new(n_agents, n_gates);
    r.push_bytes(bytes)?;
    r.run_all();
    Ok(r.result())
}

/// Replay an in-memory list of stamped decisions (no log involved).
pub fn replay_records(records: &[Stamped], n_agents: usize, n_gates: usize) -> ReplayResult {
    let mut r = Replayer::new(n_agents, n_gates);
    let mut recs = records.to_vec();
    canonical_sort(&mut recs, n_agents);
    r.records = recs;
    r.run_all();
    r.result()
}
