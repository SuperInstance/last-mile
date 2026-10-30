//! The rules of a round: what the world does when the agents act.
//!
//! A ruleset is the authority on the outcome of a round. It sees the world,
//! the agents and the tick, and it reports what happened as a list of
//! [`Event`]s. It holds no clock and no randomness: the same world, the same
//! agents and the same tick always produce the same events, so a replay is
//! bit-exact.
//!
//! Two rulesets ship: [`Beacons`], the native one, and [`RhaiRuleset`], which
//! runs the rules of the tick as a script. They are chosen by name
//! ([`by_name`]) behind the [`Ruleset`] trait rather than hard-wired into the
//! sim, so a round can name either one — and a scripted round is checked
//! against the native one, tick for tick.

pub mod beacons;
pub mod paint;
pub mod rhai;

use crate::agent::Agent;
use crate::decision::Tick;
use crate::hash::{Fnv64, StateHash};
use crate::params::{ParamError, Params};
use crate::script::ScriptError;
use crate::world::World;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub use beacons::Beacons;
pub use rhai::{Effect, RhaiRuleset};

/// The prefix a ruleset name carries when its rules are a script:
/// `"rhai:beacons"` runs the script `beacons` of the round's scripts.
pub const RHAI_PREFIX: &str = "rhai:";

/// What kind of refusal it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefusalKind {
    /// The script's answer was not a list of effects at all.
    BadAnswer,
    /// The effect named something the round does not have, or asked for
    /// something the rules of the round do not allow.
    IllegalEffect,
    /// The script ran out of fuel, so the tick had no effects.
    OutOfFuel,
}

/// One refusal: something the rules did not do, and why (D26).
///
/// A refusal is an outcome: it is recorded, it is never applied, and the rest
/// of the round goes on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    /// The tick the refusal happened on.
    pub tick: Tick,
    /// What kind of refusal it was.
    pub kind: RefusalKind,
    /// Why, in words.
    pub reason: String,
}

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
///
/// The ruleset is `Send + Sync` so that a round can be owned by a host that
/// runs its systems on more than one thread: the Bevy client (phase 2) keeps
/// the whole simulation in a resource, and a resource has to be `Send + Sync`
/// to be safe to share between the systems that read it. The only ruleset,
/// `Beacons`, is plain data and satisfies the bound without a change.
pub trait Ruleset: StateHash + Send + Sync {
    /// Play one tick of the rules.
    ///
    /// `order` holds the indices of `agents` in the order they act this tick
    /// (the rotation of the tick, which is what keeps the first move fair).
    ///
    /// The world is borrowed mutably because a rule may write to it: a tag
    /// leaves a splat of paint on the ground, and the paint is world state
    /// (it is hashed with the world, so a painted round replays bit-exact).
    fn step(
        &mut self,
        world: &mut World,
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

    /// The refusals the ruleset has recorded, oldest first.
    ///
    /// A ruleset that applies a script records every effect it refused to
    /// apply, with the reason (D26: a refusal is an outcome). A ruleset
    /// that never refuses records nothing.
    fn refusals(&self) -> &[Refusal] {
        &[]
    }

    /// How many effects the ruleset has refused, in total.
    ///
    /// The refusals themselves are capped, so this is the honest count.
    fn refused_count(&self) -> u64 {
        0
    }
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
/// `"beacons"` is the native ruleset. A name that starts with
/// [`RHAI_PREFIX`] names a script of the round's `scripts`: `"rhai:beacons"`
/// plays the round with the rules of the script `beacons`.
///
/// The scripts are needed because a scripted ruleset is a script: a round
/// that names one carries its source in the header, and a header that does
/// not carry it is refused before the round starts.
pub fn by_name(
    name: &str,
    world: &World,
    p: &Params,
    scripts: &BTreeMap<String, String>,
) -> Result<Box<dyn Ruleset>, SimError> {
    match name {
        "beacons" => Ok(Box::new(Beacons::new(world, p))),
        other => {
            let Some(script) = rhai_script_name(other) else {
                return Err(SimError::UnknownRuleset(other.to_owned()));
            };
            let Some(src) = scripts.get(script) else {
                return Err(SimError::Script(ScriptError::Missing(script.to_owned())));
            };
            Ok(Box::new(RhaiRuleset::new(script, src, world, p)?))
        }
    }
}

/// The script a ruleset runs, if the ruleset is a scripted one.
///
/// `"rhai:beacons"` runs the script `beacons`; `"beacons"` runs none.
pub fn rhai_script_name(ruleset: &str) -> Option<&str> {
    ruleset.strip_prefix(RHAI_PREFIX)
}

/// Hash the state a Beacons-shaped ruleset keeps: the scores of the squads,
/// who holds every beacon and how the round ended.
///
/// The native ruleset and the scripted one keep the same state in the same
/// shape, so they hash the same bytes — which is what lets a round played by
/// one be bit-identical to a round played by the other.
pub(crate) fn hash_rule_state(
    h: &mut Fnv64,
    scores: &[i64],
    holders: &[Option<u8>],
    outcome: &Option<Outcome>,
) {
    h.u32(scores.len() as u32);
    for s in scores {
        h.i64(*s);
    }
    h.u32(holders.len() as u32);
    for holder in holders {
        match holder {
            None => h.write(&[0]),
            Some(squad) => {
                h.write(&[1]);
                h.write(&[*squad]);
            }
        }
    }
    match outcome {
        None => h.write(&[0]),
        Some(outcome) => {
            h.write(&[1]);
            outcome.hash_into(h);
        }
    }
}

/// Add points to a squad, if the round has a score for it.
///
/// The score saturates instead of overflowing: a round is played by scripts
/// too, and a script must not be able to panic the sim with a big number.
pub(crate) fn add_score(scores: &mut [i64], squad: u8, points: i64) {
    if let Some(slot) = scores.get_mut(usize::from(squad)) {
        *slot = slot.saturating_add(points);
    }
}

/// Whether the agent is stunned at this tick.
///
/// A stun runs from the tick of the tag up to (but not including) the tick it
/// ends on: `stunned_until = now + stun_ticks` holds for `stun_ticks` ticks.
pub(crate) fn is_stunned(agent: &Agent, now: Tick) -> bool {
    agent.stunned_until.is_some_and(|end| now < end)
}

/// The id an event carries for a beacon.
pub(crate) fn beacon_id(index: usize) -> u8 {
    u8::try_from(index).unwrap_or(u8::MAX)
}

/// The squad with the strictly highest score; `None` when they tie.
pub(crate) fn winner(scores: &[i64]) -> Option<u8> {
    let mut best: Option<(usize, i64)> = None;
    let mut tied = false;
    for (index, &score) in scores.iter().enumerate() {
        match best {
            None => best = Some((index, score)),
            Some((_, high)) if score > high => {
                best = Some((index, score));
                tied = false;
            }
            Some((_, high)) if score == high => tied = true,
            Some(_) => {}
        }
    }
    match best {
        Some((index, _)) if !tied => u8::try_from(index).ok(),
        _ => None,
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
            paint: crate::world::Paint::new(),
        }
    }

    /// No scripts: the native rulesets do not need any.
    fn no_scripts() -> BTreeMap<String, String> {
        BTreeMap::new()
    }

    #[test]
    fn by_name_builds_beacons() {
        let world = flat_world();
        let p = Params::default();
        let rules =
            by_name("beacons", &world, &p, &no_scripts()).expect("beacons is a phase-1 ruleset");
        assert_eq!(rules.scores().len(), usize::from(p.squads));
        assert!(rules.outcome().is_none());
    }

    #[test]
    fn by_name_refuses_an_unknown_ruleset() {
        let world = flat_world();
        let p = Params::default();
        let Err(err) = by_name("capture-the-flag", &world, &p, &no_scripts()) else {
            panic!("an unknown ruleset must be refused");
        };
        assert_eq!(err, SimError::UnknownRuleset("capture-the-flag".to_owned()));
        assert_eq!(err.to_string(), "unknown ruleset: capture-the-flag");
    }

    #[test]
    fn by_name_refuses_a_script_the_header_does_not_carry() {
        let world = flat_world();
        let p = Params::default();
        let Err(err) = by_name("rhai:beacons", &world, &p, &no_scripts()) else {
            panic!("a ruleset whose script is missing must be refused");
        };
        assert_eq!(
            err,
            SimError::Script(ScriptError::Missing("beacons".to_owned()))
        );
        assert!(err.to_string().contains("beacons"), "{err}");
    }

    #[test]
    fn the_script_of_a_rhai_ruleset_is_named() {
        assert_eq!(rhai_script_name("rhai:beacons"), Some("beacons"));
        assert_eq!(rhai_script_name("beacons"), None);
        assert_eq!(rhai_script_name("rhai:"), Some(""));
    }
}
