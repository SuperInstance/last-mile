//! Decisions: what an agent, a human or the system asks the simulation to do.
//!
//! A decision is the only thing that crosses the wire and the only thing that
//! is logged, so it has to be small, canonical and hashable. The reflex that
//! runs inside `lm-core` on every node is *not* a decision: it never travels
//! and never appears in the log (D8, D10).
//!
//! Every type here is plain data. `serde` gives it a canonical byte encoding
//! for the wire and the log, and [`StateHash`] gives the state hash that
//! proves two machines saw the same decision stream.

use crate::fixed::Angle;
use crate::hash::{Fnv64, StateHash};
use crate::world::CellXY;
use serde::{Deserialize, Serialize};

/// A simulation tick, counted from the round's `start_tick`.
pub type Tick = u64;

/// Who sent a decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Source {
    /// An agent driven by its own cognition loop (on the Thor).
    Agent(u16),
    /// A human playing as the agent with this id.
    Human(u16),
    /// The simulation itself (rules, world events).
    System,
}

impl Source {
    /// The agent this decision speaks for, if any.
    ///
    /// A human decision speaks for the agent it possesses; a system decision
    /// speaks for nobody.
    pub fn agent(&self) -> Option<u16> {
        match self {
            Source::Agent(i) | Source::Human(i) => Some(*i),
            Source::System => None,
        }
    }

    /// Canonical discriminant used in hashes (declaration order).
    fn tag(self) -> u8 {
        match self {
            Source::Agent(_) => 0,
            Source::Human(_) => 1,
            Source::System => 2,
        }
    }
}

/// A motor-level action.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    /// Walk to a cell of the navigation grid.
    GoTo(CellXY),
    /// Stand still.
    Hold,
    /// Turn to face an angle.
    Face(Angle),
}

/// A plan with a lifetime and a safe way out (D26, D45).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Intent {
    /// How long the intent stays alive, in ticks.
    pub ttl_ticks: u16,
    /// The optimistic plan: it may cross cells nobody has seen.
    pub plan: Vec<CellXY>,
    /// The safe branch: known-free cells only, ending in cover or a hold.
    pub safe: Vec<CellXY>,
}

/// A seeded choice over a menu (D23).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Roll {
    /// The menu the probabilities belong to.
    pub menu_id: u64,
    /// One probability per option, in Q16.
    pub p: Vec<u16>,
}

/// One presence-mode sample of a gate's probabilities (D38).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lane {
    /// The gate the sample belongs to.
    pub gate: u8,
    /// The menu the gate's options come from.
    pub menu_id: u64,
    /// One value per option.
    pub values: Vec<u8>,
}

/// What a decision asks for.
///
/// Utterance, Control and Program arrive in later phases, with a
/// `PROTO_VERSION` bump.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    /// Turn an action on.
    ActionOn(Action),
    /// Turn the current action off.
    ActionOff,
    /// Replace the agent's current intent.
    Intent(Intent),
    /// Roll a menu.
    Roll(Roll),
    /// Feed one presence-mode sample to the lane conditioner.
    Lane(Lane),
    /// A marker in the decision stream; it changes nothing in the world.
    Marker(u32),
}

/// Where a decision came from (D25, D37).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provenance {
    /// The reflex that runs inside the sim; it never travels.
    Reflex,
    /// A System-1 decision model.
    System1 {
        /// The model that produced the decision.
        model: String,
        /// The tokens the call cost.
        tokens: u32,
        /// How long the call took, in milliseconds.
        latency_ms: u32,
        /// A hash of the summary features the policy used (D25).
        situation_key: u64,
    },
    /// A System-2 (cloud) decision.
    System2 {
        /// The provider that answered.
        provider: String,
        /// The tokens the call cost.
        tokens: u32,
    },
    /// A human decision.
    Human,
    /// A decision injected by a test.
    Test,
}

/// A decision as it was submitted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    /// The tick the decision asks to be applied on.
    pub apply_tick: Tick,
    /// Who sent it.
    pub source: Source,
    /// A per-source counter that must strictly increase.
    pub seq: u32,
    /// What it asks for.
    pub kind: Kind,
    /// Where it came from.
    pub provenance: Provenance,
}

/// A decision as the sequencer stamped it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamped {
    /// The decision itself.
    pub d: Decision,
    /// The tick it arrived on: the first not-yet-simulated tick.
    pub arrived: Tick,
    /// The tick it will be applied on.
    pub tick: Tick,
    /// How late it was, in ticks; 0 when it was on time.
    pub late_by: u16,
}

impl StateHash for Source {
    fn hash_into(&self, h: &mut Fnv64) {
        h.write(&[self.tag()]);
        match self {
            Source::Agent(i) | Source::Human(i) => h.u16(*i),
            Source::System => {}
        }
    }
}

impl StateHash for Action {
    fn hash_into(&self, h: &mut Fnv64) {
        match self {
            Action::GoTo(c) => {
                h.write(&[0]);
                h.u16(c.x);
                h.u16(c.y);
            }
            Action::Hold => h.write(&[1]),
            Action::Face(a) => {
                h.write(&[2]);
                h.u16(a.0);
            }
        }
    }
}

impl StateHash for Intent {
    fn hash_into(&self, h: &mut Fnv64) {
        h.u16(self.ttl_ticks);
        h.u32(self.plan.len() as u32);
        for c in &self.plan {
            h.u16(c.x);
            h.u16(c.y);
        }
        h.u32(self.safe.len() as u32);
        for c in &self.safe {
            h.u16(c.x);
            h.u16(c.y);
        }
    }
}

impl StateHash for Roll {
    fn hash_into(&self, h: &mut Fnv64) {
        h.u64(self.menu_id);
        h.u32(self.p.len() as u32);
        for p in &self.p {
            h.u16(*p);
        }
    }
}

impl StateHash for Lane {
    fn hash_into(&self, h: &mut Fnv64) {
        h.write(&[self.gate]);
        h.u64(self.menu_id);
        h.u32(self.values.len() as u32);
        for v in &self.values {
            h.write(&[*v]);
        }
    }
}

impl StateHash for Kind {
    fn hash_into(&self, h: &mut Fnv64) {
        match self {
            Kind::ActionOn(a) => {
                h.write(&[0]);
                a.hash_into(h);
            }
            Kind::ActionOff => h.write(&[1]),
            Kind::Intent(i) => {
                h.write(&[2]);
                i.hash_into(h);
            }
            Kind::Roll(r) => {
                h.write(&[3]);
                r.hash_into(h);
            }
            Kind::Lane(l) => {
                h.write(&[4]);
                l.hash_into(h);
            }
            Kind::Marker(m) => {
                h.write(&[5]);
                h.u32(*m);
            }
        }
    }
}

impl StateHash for Provenance {
    fn hash_into(&self, h: &mut Fnv64) {
        match self {
            Provenance::Reflex => h.write(&[0]),
            Provenance::System1 {
                model,
                tokens,
                latency_ms,
                situation_key,
            } => {
                h.write(&[1]);
                write_str(h, model);
                h.u32(*tokens);
                h.u32(*latency_ms);
                h.u64(*situation_key);
            }
            Provenance::System2 { provider, tokens } => {
                h.write(&[2]);
                write_str(h, provider);
                h.u32(*tokens);
            }
            Provenance::Human => h.write(&[3]),
            Provenance::Test => h.write(&[4]),
        }
    }
}

impl StateHash for Decision {
    fn hash_into(&self, h: &mut Fnv64) {
        h.u64(self.apply_tick);
        self.source.hash_into(h);
        h.u32(self.seq);
        self.kind.hash_into(h);
        self.provenance.hash_into(h);
    }
}

impl StateHash for Stamped {
    fn hash_into(&self, h: &mut Fnv64) {
        self.d.hash_into(h);
        h.u64(self.arrived);
        h.u64(self.tick);
        h.u16(self.late_by);
    }
}

/// Absorb a string as a u32-LE length prefix followed by its bytes.
fn write_str(h: &mut Fnv64, s: &str) {
    h.u32(s.len() as u32);
    h.write(s.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_reports_its_agent() {
        assert_eq!(Source::Agent(3).agent(), Some(3));
        assert_eq!(Source::Human(4).agent(), Some(4));
        assert_eq!(Source::System.agent(), None);
    }

    #[test]
    fn source_orders_by_declaration() {
        assert!(Source::Agent(0) < Source::Human(0));
        assert!(Source::Human(0) < Source::System);
    }

    #[test]
    fn decision_hash_is_stable_and_sensitive() {
        let d = Decision {
            apply_tick: 10,
            source: Source::Agent(1),
            seq: 3,
            kind: Kind::ActionOn(Action::Hold),
            provenance: Provenance::Reflex,
        };
        assert_eq!(d.state_hash(), d.clone().state_hash());

        let mut other = d.clone();
        other.seq = 4;
        assert_ne!(d.state_hash(), other.state_hash());

        let mut other = d.clone();
        other.kind = Kind::ActionOn(Action::Face(Angle(1_024)));
        assert_ne!(d.state_hash(), other.state_hash());

        let mut other = d.clone();
        other.provenance = Provenance::Test;
        assert_ne!(d.state_hash(), other.state_hash());
    }

    #[test]
    fn every_kind_has_its_own_hash() {
        let kinds = [
            Kind::ActionOn(Action::Hold),
            Kind::ActionOff,
            Kind::Intent(Intent {
                ttl_ticks: 10,
                plan: vec![CellXY { x: 1, y: 2 }],
                safe: vec![],
            }),
            Kind::Roll(Roll {
                menu_id: 1,
                p: vec![1, 2],
            }),
            Kind::Lane(Lane {
                gate: 1,
                menu_id: 2,
                values: vec![3, 4],
            }),
            Kind::Marker(7),
        ];
        for (i, a) in kinds.iter().enumerate() {
            for (j, b) in kinds.iter().enumerate() {
                if i != j {
                    assert_ne!(a.state_hash(), b.state_hash(), "kinds {i} and {j}");
                }
            }
        }
    }

    #[test]
    fn stamped_hash_covers_the_stamp() {
        let d = Decision {
            apply_tick: 10,
            source: Source::Agent(1),
            seq: 3,
            kind: Kind::Marker(1),
            provenance: Provenance::Test,
        };
        let a = Stamped {
            d: d.clone(),
            arrived: 4,
            tick: 10,
            late_by: 0,
        };
        let b = Stamped {
            d,
            arrived: 4,
            tick: 10,
            late_by: 1,
        };
        assert_ne!(a.state_hash(), b.state_hash());
    }
}
