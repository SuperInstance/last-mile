//! The rules of a round: what the world does when the agents act.
//!
//! A ruleset is the authority on the outcome of a round. It sees the world,
//! the agents and the tick, and it reports what happened as a list of
//! [`Event`]s. It holds no clock and no randomness: the same world, the same
//! agents and the same tick always produce the same events, so a replay is
//! bit-exact.
//!
//! Phase 1 ships one ruleset, [`Beacons`]. A Rhai-scripted ruleset arrives in
//! phase 5, which is why the ruleset is chosen by name ([`by_name`]) behind
//! the [`Ruleset`] trait rather than hard-wired into the sim.

pub mod beacons;

use crate::agent::Agent;
use crate::decision::Tick;
use crate::hash::{Fnv64, StateHash};
use crate::params::{ParamError, Params};
use crate::script::ScriptError;
use crate::world::World;
use serde::{Deserialize, Serialize};

pub use beacons::Beacons;

/// Something the rules did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    /// An agent tagged an enemy: the target is stunned until its stun ends.
    Tagged {
        /// The agent that made the tag.
        by: u16,
        /// The agent that was tagged.
        target: u16,
    },
    /// A stun ended and the agent was put back on its spawn.
    Respawned(u16),
    /// A beacon is now held by a squad.
    BeaconHeld {
        /// The beacon, in beacon order.
        beacon: u8,
        /// The squad holding it.
        squad: u8,
    },
    /// A beacon is no longer held by anybody.
    BeaconLost {
        /// The beacon, in beacon order.
        beacon: u8,
    },
    /// The round is over.
    RoundOver(Outcome),
    /// A marker decision reached the world.
    Marker(u32),
}

/// How a round ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcome {
    /// The winning squad, or `None` when the round ended in a tie.
    pub winner: Option<u8>,
    /// The score of every squad, in squad order.
    pub scores: Vec<i64>,
    /// The tick the round ended on.
    pub tick: Tick,
}

impl StateHash for Outcome {
    fn hash_into(&self, h: &mut Fnv64) {
        match self.winner {
            None => h.write(&[0]),
            Some(s) => {
                h.write(&[1]);
                h.write(&[s]);
            }
        }
        h.u32(self.scores.len() as u32);
        for s in &self.scores {
            h.i64(*s);
        }
        h.u64(self.tick);
    }
}

impl StateHash for Event {
    fn hash_into(&self, h: &mut Fnv64) {
        match self {
            Event::Tagged { by, target } => {
                h.write(&[0]);
                h.u16(*by);
                h.u16(*target);
            }
            Event::Respawned(id) => {
                h.write(&[1]);
                h.u16(*id);
            }
            Event::BeaconHeld { beacon, squad } => {
                h.write(&[2]);
                h.write(&[*beacon]);
                h.write(&[*squad]);
            }
            Event::BeaconLost { beacon } => {
                h.write(&[3]);
                h.write(&[*beacon]);
            }
            Event::RoundOver(outcome) => {
                h.write(&[4]);
                outcome.hash_into(h);
            }
            Event::Marker(m) => {
                h.write(&[5]);
                h.u32(*m);
            }
        }
    }
}

/// The rules of a round.
///
/// The sim owns the clock and the agents; the ruleset owns the outcome. It is
/// asked once per tick, after the agents have moved, with the agents in the
/// tick's rotating order, and it answers with the events of that tick.
pub trait Ruleset: StateHash {
    /// Play one tick of the rules.
    ///
    /// `order` holds the indices of `agents` in the order they act this tick
    /// (the rotation of the tick, which is what keeps the first move fair).
    fn step(
        &mut self,
        world: &World,
        agents: &mut [Agent],
        now: Tick,
        order: &[usize],
        p: &Params,
    ) -> Vec<Event>;

    /// The score of every squad, in squad order.
    fn scores(&self) -> &[i64];

    /// Who holds every beacon, in beacon order.
    ///
    /// The score view shows the holder of a beacon, so the sim asks the
    /// ruleset for it. A ruleset with no beacons holds none and publishes
    /// nothing.
    fn beacon_holders(&self) -> &[Option<u8>] {
        &[]
    }

    /// How the round ended, or `None` while it is still running.
    fn outcome(&self) -> Option<&Outcome>;
}

/// Why a round could not be set up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SimError {
    /// The round asked for a ruleset this build does not have.
    UnknownRuleset(String),
    /// The parameters of the round are not usable.
    Params(ParamError),
    /// A reflex script of the round could not be loaded.
    Script(ScriptError),
}

impl std::fmt::Display for SimError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SimError::UnknownRuleset(name) => write!(f, "unknown ruleset: {name}"),
            SimError::Params(e) => write!(f, "invalid parameter {}: {}", e.field, e.reason),
            SimError::Script(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SimError {}

impl From<ScriptError> for SimError {
    fn from(e: ScriptError) -> Self {
        SimError::Script(e)
    }
}

/// The ruleset with this name.
///
/// Phase 1 has exactly one: `"beacons"`. The name is looked up rather than
/// hard-wired, because a Rhai-scripted ruleset arrives in phase 5 and the
/// round header carries the name it wants.
pub fn by_name(name: &str, world: &World, p: &Params) -> Result<Box<dyn Ruleset>, SimError> {
    match name {
        "beacons" => Ok(Box::new(Beacons::new(world, p))),
        other => Err(SimError::UnknownRuleset(other.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::Params;
    use crate::world::{CellXY, Grid, World};

    fn flat_world() -> World {
        World {
            grid: Grid::new(8, 8),
            beacons: vec![CellXY { x: 4, y: 4 }],
            spawns: vec![CellXY { x: 0, y: 0 }, CellXY { x: 7, y: 7 }],
            cell_units: 1024,
        }
    }

    #[test]
    fn by_name_builds_beacons() {
        let world = flat_world();
        let p = Params::default();
        let rules = by_name("beacons", &world, &p).expect("beacons is a phase-1 ruleset");
        assert_eq!(rules.scores().len(), usize::from(p.squads));
        assert!(rules.outcome().is_none());
    }

    #[test]
    fn by_name_refuses_an_unknown_ruleset() {
        let world = flat_world();
        let p = Params::default();
        let Err(err) = by_name("capture-the-flag", &world, &p) else {
            panic!("an unknown ruleset must be refused");
        };
        assert_eq!(err, SimError::UnknownRuleset("capture-the-flag".to_owned()));
        assert_eq!(err.to_string(), "unknown ruleset: capture-the-flag");
    }
}
