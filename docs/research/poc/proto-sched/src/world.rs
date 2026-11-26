//! The tiny world: agents, intents with a mandatory safe branch, and movement.

use crate::hash::Fnv1a64;
use crate::lane::LaneConditioner;
use crate::types::{Kind, Stamped};

/// The world state is hashed every this many ticks.
pub const HASH_EVERY: u64 = 30;

/// What an agent is currently following.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// No intent: the agent follows its lane-chosen action.
    Idle = 0,
    /// Following the intent's plan.
    Plan = 1,
    /// Following the intent's safe branch (never a default).
    Safe = 2,
}

/// A live intent: the plan, the safe branch and when it expires.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntentState {
    pub plan: Vec<(i16, i16)>,
    pub safe: Vec<(i16, i16)>,
    /// TTL in ticks, as submitted.
    pub ttl: u16,
    /// Tick at which the intent expires (`apply_tick + ttl`).
    pub expiry_tick: u64,
}

/// One agent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Agent {
    /// Cell position.
    pub pos: (i16, i16),
    pub mode: Mode,
    pub intent: Option<IntentState>,
    /// Gate whose lane-chosen action drives this agent.
    pub active_gate: Option<u8>,
    /// Set when an `ActionOn` already moved the agent this tick.
    pub moved_this_tick: bool,
}

impl Default for Agent {
    fn default() -> Self {
        Agent {
            pos: (0, 0),
            mode: Mode::Idle,
            intent: None,
            active_gate: None,
            moved_this_tick: false,
        }
    }
}

/// Movement vector of an action / lane index.
/// 0 = north, 1 = east, 2 = south, 3 = west, anything else = stay.
pub fn step_dir(action: u8) -> (i16, i16) {
    match action {
        0 => (0, 1),
        1 => (1, 0),
        2 => (0, -1),
        3 => (-1, 0),
        _ => (0, 0),
    }
}

/// The world: agents plus their lane conditioners.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct World {
    /// Next tick to be processed.
    pub tick: u64,
    /// True once the current tick has begun but not yet finished.
    /// A replay cut in the middle of a tick leaves this set.
    pub tick_started: bool,
    pub n_agents: usize,
    pub agents: Vec<Agent>,
    pub lanes: LaneConditioner,
    /// Number of markers applied so far.
    pub markers: u64,
    /// Value of the last marker applied.
    pub last_marker: u32,
}

impl World {
    pub fn new(n_agents: usize, n_gates: usize) -> Self {
        World {
            tick: 0,
            tick_started: false,
            n_agents,
            agents: vec![Agent::default(); n_agents],
            lanes: LaneConditioner::new(n_agents, n_gates),
            markers: 0,
            last_marker: 0,
        }
    }

    /// Open a tick: clear the per-tick move flags. Idempotent per tick, so a
    /// replay that resumes in the middle of a tick does not reset the flags
    /// that were already set.
    pub fn begin_tick(&mut self) {
        if self.tick_started {
            return;
        }
        self.tick_started = true;
        for a in &mut self.agents {
            a.moved_this_tick = false;
        }
    }

    /// Close a tick and advance the clock.
    pub fn end_tick(&mut self) {
        self.tick_started = false;
        self.tick += 1;
    }

    /// Apply a stamped decision to the world.
    pub fn apply(&mut self, s: &Stamped) {
        let idx = s.source as usize;
        if idx >= self.n_agents {
            return;
        }
        match &s.kind {
            Kind::ActionOn { action } => {
                let a = &mut self.agents[idx];
                a.moved_this_tick = true;
                let (dx, dy) = step_dir(*action);
                a.pos.0 += dx;
                a.pos.1 += dy;
            }
            Kind::Lane { gate, p } => {
                self.lanes.update(s.source, *gate, p);
                self.agents[idx].active_gate = Some(*gate);
            }
            Kind::Intent { ttl, plan, safe } => {
                self.apply_intent(idx, *ttl, plan.clone(), safe.clone())
            }
            Kind::Marker(v) => {
                self.markers += 1;
                self.last_marker = *v;
            }
        }
    }

    /// Install / refuse an intent.
    ///
    /// * no live intent, or the live one expired -> the new intent is
    ///   installed and the agent follows its plan;
    /// * live intent, same target -> refreshed, the agent follows the plan;
    /// * live intent, different target -> the replacement is refused and the
    ///   agent drops to the safe branch of the live intent (never to a
    ///   default).
    fn apply_intent(
        &mut self,
        idx: usize,
        ttl: u16,
        plan: Vec<(i16, i16)>,
        safe: Vec<(i16, i16)>,
    ) {
        let now = self.tick;
        let a = &mut self.agents[idx];
        let (expired, conflict) = match a.intent.as_ref() {
            None => (true, false),
            Some(cur) => (
                now >= cur.expiry_tick,
                cur.plan.first().copied() != plan.first().copied(),
            ),
        };
        if expired || !conflict {
            a.intent = Some(IntentState { plan, safe, ttl, expiry_tick: now + ttl as u64 });
            a.mode = Mode::Plan;
        } else {
            a.mode = Mode::Safe;
        }
    }

    /// One movement phase of the current tick.
    ///
    /// Precedence: an `ActionOn` already applied this tick wins, then an
    /// intent (plan, or safe branch when the intent expired), then the
    /// lane-chosen action of the agent's active gate.
    pub fn move_agents(&mut self) {
        let now = self.tick;
        for i in 0..self.n_agents {
            if self.agents[i].moved_this_tick {
                continue;
            }
            let expired = self.agents[i]
                .intent
                .as_ref()
                .map(|s| now >= s.expiry_tick)
                .unwrap_or(false);
            if expired {
                self.agents[i].mode = Mode::Safe;
            }

            match self.agents[i].mode {
                Mode::Plan | Mode::Safe => {
                    let mode = self.agents[i].mode;
                    let path: Vec<(i16, i16)> = match self.agents[i].intent.as_ref() {
                        Some(s) => {
                            if mode == Mode::Plan {
                                s.plan.clone()
                            } else {
                                s.safe.clone()
                            }
                        }
                        None => Vec::new(),
                    };
                    if path.is_empty() {
                        continue;
                    }
                    let target = path[0];
                    let pos = self.agents[i].pos;
                    let dx = target.0 as i32 - pos.0 as i32;
                    let dy = target.1 as i32 - pos.1 as i32;
                    if dx == 0 && dy == 0 {
                        let rest = path[1..].to_vec();
                        if let Some(s) = self.agents[i].intent.as_mut() {
                            if mode == Mode::Plan {
                                s.plan = rest;
                            } else {
                                s.safe = rest;
                            }
                        }
                    } else if dx.abs() >= dy.abs() {
                        self.agents[i].pos.0 += if dx > 0 { 1 } else { -1 };
                    } else {
                        self.agents[i].pos.1 += if dy > 0 { 1 } else { -1 };
                    }
                }
                Mode::Idle => {
                    if let Some(gate) = self.agents[i].active_gate {
                        let action = self.lanes.current(i as u16, gate);
                        let (dx, dy) = step_dir(action);
                        self.agents[i].pos.0 += dx;
                        self.agents[i].pos.1 += dy;
                    }
                }
            }
        }
    }

    /// FNV-1a 64 over the whole mutable state of the world.
    pub fn hash_state(&self) -> u64 {
        let mut h = Fnv1a64::new();
        h.write_u64(self.tick);
        h.write_u8(self.tick_started as u8);
        h.write_u64(self.markers);
        h.write_u32(self.last_marker);
        for (i, a) in self.agents.iter().enumerate() {
            h.write_i16(a.pos.0);
            h.write_i16(a.pos.1);
            h.write_u8(a.mode as u8);
            h.write_u8(a.moved_this_tick as u8);
            h.write_u8(a.active_gate.unwrap_or(0xFF));
            match &a.intent {
                None => {
                    h.write_u8(0);
                }
                Some(i2) => {
                    h.write_u8(1);
                    h.write_u64(i2.expiry_tick);
                    h.write_u16(i2.ttl);
                    h.write_u16(i2.plan.len() as u16);
                    for p in &i2.plan {
                        h.write_i16(p.0);
                        h.write_i16(p.1);
                    }
                    h.write_u16(i2.safe.len() as u16);
                    for p in &i2.safe {
                        h.write_i16(p.0);
                        h.write_i16(p.1);
                    }
                }
            }
            for g in 0..self.lanes.n_gates() {
                for v in self.lanes.lanes(i as u16, g as u8) {
                    h.write_i32(v);
                }
                h.write_u8(self.lanes.current(i as u16, g as u8));
            }
        }
        h.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamped(source: u16, kind: Kind) -> Stamped {
        Stamped {
            apply_tick: 0,
            source,
            seq: 0,
            kind,
            arrival_tick: 0,
            late: false,
            lateness: 0,
        }
    }

    #[test]
    fn action_on_moves_one_cell() {
        let mut w = World::new(1, 1);
        w.begin_tick();
        w.apply(&stamped(0, Kind::ActionOn { action: 1 }));
        assert_eq!(w.agents[0].pos, (1, 0));
        w.move_agents();
        // the ActionOn already used up this tick's movement
        assert_eq!(w.agents[0].pos, (1, 0));
    }

    #[test]
    fn lane_chosen_action_applies_every_tick() {
        let mut w = World::new(1, 1);
        w.begin_tick();
        w.apply(&stamped(0, Kind::Lane { gate: 0, p: [0, 0, 100, 0] }));
        assert_eq!(w.lanes.current(0, 0), 2);
        w.move_agents();
        assert_eq!(w.agents[0].pos, (0, -1));
        w.end_tick();
        w.begin_tick();
        w.move_agents();
        assert_eq!(w.agents[0].pos, (0, -2));
    }

    #[test]
    fn intent_follows_the_plan() {
        let mut w = World::new(1, 1);
        w.begin_tick();
        w.apply(&stamped(
            0,
            Kind::Intent { ttl: 5, plan: vec![(2, 0)], safe: vec![(-5, 0)] },
        ));
        assert_eq!(w.agents[0].mode, Mode::Plan);
        w.move_agents();
        assert_eq!(w.agents[0].pos, (1, 0));
    }

    #[test]
    fn expired_intent_switches_to_the_safe_branch() {
        let mut w = World::new(1, 1);
        w.begin_tick();
        w.apply(&stamped(
            0,
            Kind::Intent { ttl: 2, plan: vec![(10, 0)], safe: vec![(-1, 0)] },
        ));
        w.move_agents();
        assert_eq!(w.agents[0].pos, (1, 0));
        w.end_tick();
        // tick 1: still inside the TTL
        w.begin_tick();
        w.move_agents();
        assert_eq!(w.agents[0].mode, Mode::Plan);
        assert_eq!(w.agents[0].pos, (2, 0));
        w.end_tick();
        // tick 2: the intent expired -> safe branch
        w.begin_tick();
        w.move_agents();
        assert_eq!(w.agents[0].mode, Mode::Safe);
        assert_eq!(w.agents[0].pos, (1, 0));
    }

    #[test]
    fn refused_replacement_switches_to_the_safe_branch() {
        let mut w = World::new(1, 1);
        w.begin_tick();
        w.apply(&stamped(
            0,
            Kind::Intent { ttl: 50, plan: vec![(10, 0)], safe: vec![(-1, 0)] },
        ));
        w.move_agents();
        assert_eq!(w.agents[0].pos, (1, 0));
        // a replacement with a different target is refused
        w.apply(&stamped(
            0,
            Kind::Intent { ttl: 50, plan: vec![(0, 10)], safe: vec![(0, -10)] },
        ));
        assert_eq!(w.agents[0].mode, Mode::Safe);
        w.move_agents();
        assert_eq!(w.agents[0].pos, (0, 0), "the agent steps towards the safe waypoint");
        // the refused intent did not replace the live one
        assert_eq!(w.agents[0].intent.as_ref().unwrap().plan, vec![(10, 0)]);
    }

    #[test]
    fn new_intent_after_expiry_switches_back_to_the_plan() {
        let mut w = World::new(1, 1);
        w.begin_tick();
        w.apply(&stamped(
            0,
            Kind::Intent { ttl: 1, plan: vec![(10, 0)], safe: vec![(-1, 0)] },
        ));
        w.move_agents();
        w.end_tick();
        w.begin_tick();
        w.move_agents();
        assert_eq!(w.agents[0].mode, Mode::Safe);
        // a fresh intent takes over
        w.apply(&stamped(
            0,
            Kind::Intent { ttl: 10, plan: vec![(0, 10)], safe: vec![(0, -10)] },
        ));
        assert_eq!(w.agents[0].mode, Mode::Plan);
        w.move_agents();
        assert_eq!(w.agents[0].pos, (0, 1));
    }

    #[test]
    fn hash_changes_with_the_state() {
        let mut w = World::new(1, 1);
        let h0 = w.hash_state();
        w.agents[0].pos = (1, 0);
        assert_ne!(h0, w.hash_state());
    }
}
