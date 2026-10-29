//! The Rhai-scripted ruleset: a round whose rules are a script.
//!
//! D7: the rules of a round stay fixed while it runs, but between rounds they
//! can be rewritten in a sandboxed script. This ruleset is that seam. The
//! header names `"rhai:<script>"`, the script lives in the header's scripts,
//! and the script says what the rules of the tick are.
//!
//! ## What the script sees
//!
//! The script is called once per tick, after the agents have moved, with a
//! map of the world as the rules need to see it. The keys are `agents`,
//! `beacons`, `order`, `params`, `scores` and `tick`; a Rhai map is a
//! `BTreeMap`, so the keys are always in the same order.
//!
//! * `agents` — every agent of the round, in agent order: its `id`, its
//!   `squad`, the `cell` it stands in, its exact `pos` in 1/1024 m units, the
//!   `cell` and `pos` of its `spawn`, whether it is `stunned`, and the tick
//!   its `stunned_until` stun ends on (`-1` when there is none).
//! * `beacons` — every beacon, in beacon order: its `index`, its `cell`, its
//!   exact `pos`, and the `holder`: the squad holding it, or `-1` for nobody.
//! * `order` — the agents in the order they act this tick: the rotation.
//! * `params` — the parameters the rules read.
//! * `scores` — the score of every squad, in squad order.
//! * `tick` — the tick.
//!
//! The script may also call `los_clear(ax, ay, bx, by)`: whether the straight
//! line between two cells is unobstructed. It is a fact about the map, not a
//! rule of the round, so the host answers it and the script decides what to
//! do with it.
//!
//! ## What the script says
//!
//! The script returns an array of effects — maps with a `kind` key:
//!
//! | kind          | fields              | what it does                        |
//! |---------------|---------------------|-------------------------------------|
//! | `tag`         | `by`, `target`      | the agent `by` tags `target`        |
//! | `stun_until`  | `agent`, `until`    | the agent is stunned until `until`  |
//! | `respawn`     | `agent`             | the agent goes back to its spawn     |
//! | `hold_beacon` | `beacon`, `squad`   | the beacon is held by `squad` (`-1`: nobody) |
//! | `score`       | `squad`, `points`   | the squad scores `points`           |
//! | `round_over`  | —                   | the round ends                      |
//!
//! Rust applies the effects and checks every one of them. An effect that
//! names something the round does not have, or asks for something the round
//! does not allow, is refused and never applied (D26): the refusal is
//! recorded with its reason, and the rest of the tick still plays.
//!
//! The effects of a tick are applied in the order the script asks for them.
//! A `tag` therefore goes before the `stun_until` that goes with it: the tag
//! names the two agents, the stun says how long the target is out.
//!
//! The script is fuel-limited like every other script of the round: a script
//! that never returns runs out of fuel, and a tick whose script ran out of
//! fuel has no effects at all.
//!
//! ## The oracle
//!
//! The native [`Beacons`](super::Beacons) ruleset is the oracle of this one.
//! The same rules, written as a script, must produce the same events, the
//! same agent states and the same canonical hashes, tick for tick: the test
//! `rhai_beacons_matches_native` plays 50 seeds under both and compares the
//! hash sequences. Any difference is a bug in the script or in the layer that
//! applies its effects — never in the oracle.

use super::{
    add_score, hash_rule_state, is_stunned, winner, Event, Outcome, Refusal, RefusalKind, Ruleset,
};
use crate::agent::Agent;
use crate::decision::Tick;
use crate::hash::{Fnv64, StateHash};
use crate::params::Params;
use crate::script::{sandboxed_engine, ScriptError, RULES_ENTRY_POINT};
use crate::world::{los, CellXY, Grid, World};
use rhai::{Dynamic, Engine, EvalAltResult, Map, Scope, AST};

/// How many refusals the ruleset keeps.
///
/// The count is honest for every round; the list is a window on the first
/// refusals, which is where the reason for the rest is.
const MAX_REFUSALS: usize = 256;

/// One effect a rules script asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// An agent tagged an enemy. The target is not stunned by the effect
    /// itself: a `stun_until` effect does that.
    Tag {
        /// The agent that made the tag.
        by: u16,
        /// The agent that was tagged.
        target: u16,
    },
    /// An agent is stunned until this tick.
    StunUntil {
        /// The agent that is stunned.
        agent: u16,
        /// The tick the stun ends on.
        until: u64,
    },
    /// A stun ended: the agent goes back to its squad's spawn.
    Respawn {
        /// The agent that respawns.
        agent: u16,
    },
    /// A beacon is held by a squad; `None` means by nobody.
    HoldBeacon {
        /// The beacon, in beacon order.
        beacon: u8,
        /// The squad holding it.
        squad: Option<u8>,
    },
    /// A squad scores points.
    Score {
        /// The squad that scores.
        squad: u8,
        /// The points it scores.
        points: i64,
    },
    /// The round is over.
    RoundOver,
}

/// The ruleset that runs a Rhai script.
///
/// It is pure state plus the tick: the scores of the squads, who holds every
/// beacon and — once the round is over — how it ended. The state is the same
/// shape as the native ruleset's, so the two hash the same bytes.
pub struct RhaiRuleset {
    /// The engine the script runs on.
    engine: Engine,
    /// The compiled script.
    ast: AST,
    /// The name of the script, for the messages.
    name: String,
    /// The score of every squad, in squad order.
    scores: Vec<i64>,
    /// Who holds every beacon, in beacon order.
    holders: Vec<Option<u8>>,
    /// The outcome of the round, once it is over.
    outcome: Option<Outcome>,
    /// The refusals, oldest first.
    refusals: Vec<Refusal>,
    /// How many refusals there were, including the ones not kept.
    refused: u64,
}

impl RhaiRuleset {
    /// A ruleset that plays the script `src` on `world`.
    ///
    /// The script is compiled here, so a script that does not parse — or that
    /// defines no `fn rules(v)` — stops the round before it starts.
    pub fn new(
        name: &str,
        src: &str,
        world: &World,
        p: &Params,
    ) -> Result<RhaiRuleset, ScriptError> {
        let mut engine = sandboxed_engine();
        // The map answers one question the rules ask: can these two cells see
        // each other? It is a fact about the world, not a rule of the round,
        // so the host answers it and the script decides what to do with it.
        engine.register_fn("los_clear", los_fn(world.grid.clone()));
        let ast = engine
            .compile(src)
            .map_err(|err| ScriptError::Parse(err.to_string()))?;
        let has_entry = ast
            .iter_functions()
            .any(|f| f.name == RULES_ENTRY_POINT && f.params.len() == 1);
        if !has_entry {
            return Err(ScriptError::NoRulesEntryPoint(name.to_owned()));
        }
        Ok(RhaiRuleset {
            engine,
            ast,
            name: name.to_owned(),
            scores: vec![0; usize::from(p.squads)],
            holders: vec![None; world.beacons.len()],
            outcome: None,
            refusals: Vec::new(),
            refused: 0,
        })
    }

    /// The map the script sees: the world as the rules need to see it.
    ///
    /// The keys are `agents`, `beacons`, `order`, `params`, `scores` and
    /// `tick`. A Rhai map is a `BTreeMap`, so the keys are always in the same
    /// order and the script sees the same world every tick.
    fn view(
        &self,
        world: &World,
        agents: &[Agent],
        p: &Params,
        now: Tick,
        order: &[usize],
    ) -> Dynamic {
        let mut map = Map::new();
        map.insert(
            "agents".into(),
            Dynamic::from(agents_map(world, agents, now)),
        );
        map.insert(
            "beacons".into(),
            Dynamic::from(beacons_map(world, &self.holders)),
        );
        map.insert("order".into(), Dynamic::from(order_map(order)));
        map.insert("params".into(), params_map(p));
        map.insert("scores".into(), Dynamic::from(score_map(&self.scores)));
        map.insert("tick".into(), Dynamic::from(tick_int(now)));
        Dynamic::from_map(map)
    }

    /// Apply one effect, or refuse it with the reason why.
    ///
    /// The check is on the effect itself: it must name something the round
    /// has, and it must not ask for something the round does not allow. What
    /// the rules *are* is the script's business, not the host's.
    fn apply(
        &mut self,
        effect: Effect,
        world: &World,
        agents: &mut [Agent],
        now: Tick,
        p: &Params,
    ) -> Result<Vec<Event>, String> {
        match effect {
            Effect::Tag { by, target } => {
                let tagger = index_of(agents, by)
                    .ok_or_else(|| format!("there is no agent {by} to tag with"))?;
                let victim = index_of(agents, target)
                    .ok_or_else(|| format!("there is no agent {target} to tag"))?;
                if tagger == victim {
                    return Err(format!("agent {by} cannot tag itself"));
                }
                if agents[tagger].squad == agents[victim].squad {
                    return Err(format!("agent {by} cannot tag an agent of its own squad"));
                }
                if is_stunned(&agents[tagger], now) {
                    return Err(format!("agent {by} is stunned and cannot tag"));
                }
                if is_stunned(&agents[victim], now) {
                    return Err(format!("agent {target} is already stunned"));
                }
                Ok(vec![Event::Tagged { by, target }])
            }
            Effect::StunUntil { agent, until } => {
                let index = index_of(agents, agent)
                    .ok_or_else(|| format!("there is no agent {agent} to stun"))?;
                if until < now {
                    return Err(format!(
                        "the stun of agent {agent} cannot end before the tick it starts on"
                    ));
                }
                agents[index].stunned_until = Some(until);
                Ok(Vec::new())
            }
            Effect::Respawn { agent } => {
                let index = index_of(agents, agent)
                    .ok_or_else(|| format!("there is no agent {agent} to respawn"))?;
                agents[index].stunned_until = None;
                if let Some(&spawn) = world.spawns.get(usize::from(agents[index].squad)) {
                    agents[index].pos = world.grid.center(spawn, world.cell_units);
                }
                Ok(vec![Event::Respawned(agent)])
            }
            Effect::HoldBeacon { beacon, squad } => {
                let index = usize::from(beacon);
                if index >= world.beacons.len() {
                    return Err(format!("there is no beacon {beacon}"));
                }
                if let Some(squad) = squad {
                    if squad >= p.squads {
                        return Err(format!("there is no squad {squad}"));
                    }
                }
                let held = self.holders.get(index).copied().flatten();
                let mut events = Vec::new();
                if held != squad {
                    if held.is_some() {
                        events.push(Event::BeaconLost { beacon });
                    }
                    if let Some(squad) = squad {
                        events.push(Event::BeaconHeld { beacon, squad });
                    }
                    if let Some(slot) = self.holders.get_mut(index) {
                        *slot = squad;
                    }
                }
                Ok(events)
            }
            Effect::Score { squad, points } => {
                if squad >= p.squads {
                    return Err(format!("there is no squad {squad}"));
                }
                add_score(&mut self.scores, squad, points);
                Ok(Vec::new())
            }
            Effect::RoundOver => {
                let outcome = Outcome {
                    winner: winner(&self.scores),
                    scores: self.scores.clone(),
                    tick: now,
                };
                self.outcome = Some(outcome.clone());
                Ok(vec![Event::RoundOver(outcome)])
            }
        }
    }

    /// Record a refusal: what the rules did not do, and why.
    fn refuse(&mut self, refusal: Refusal) {
        self.refused += 1;
        if self.refusals.len() < MAX_REFUSALS {
            self.refusals.push(refusal);
        }
    }
}

impl Ruleset for RhaiRuleset {
    fn step(
        &mut self,
        world: &World,
        agents: &mut [Agent],
        now: Tick,
        order: &[usize],
        p: &Params,
    ) -> Vec<Event> {
        // The round is over: nothing else happens until the next one starts.
        if self.outcome.is_some() {
            return Vec::new();
        }

        let view = self.view(world, agents, p, now, order);
        // The budget belongs to this call alone: Rhai counts the operations
        // of one evaluation and starts fresh on the next. A fuel of zero is
        // read as "no limit" by Rhai, so zero buys the script nothing.
        self.engine.set_max_operations(p.script_fuel.max(1));
        let mut scope = Scope::new();
        let answer =
            self.engine
                .call_fn::<Dynamic>(&mut scope, &self.ast, RULES_ENTRY_POINT, (view,));
        let answer = match answer {
            Ok(answer) => answer,
            Err(err) => {
                let refusal = match *err {
                    EvalAltResult::ErrorTooManyOperations(_) => Refusal {
                        tick: now,
                        kind: RefusalKind::OutOfFuel,
                        reason: format!(
                            "the rules script {} ran out of fuel ({} operations)",
                            self.name, p.script_fuel
                        ),
                    },
                    other => Refusal {
                        tick: now,
                        kind: RefusalKind::BadAnswer,
                        reason: format!("the rules script {} failed: {other}", self.name),
                    },
                };
                self.refuse(refusal);
                return Vec::new();
            }
        };

        let effects = match parse_effects(answer) {
            Ok(effects) => effects,
            Err(reason) => {
                self.refuse(Refusal {
                    tick: now,
                    kind: RefusalKind::BadAnswer,
                    reason,
                });
                return Vec::new();
            }
        };

        let mut events = Vec::new();
        for effect in effects {
            match effect {
                Parsed::Effect(effect) => match self.apply(effect, world, agents, now, p) {
                    Ok(mut applied) => events.append(&mut applied),
                    Err(reason) => self.refuse(Refusal {
                        tick: now,
                        kind: RefusalKind::IllegalEffect,
                        reason,
                    }),
                },
                Parsed::Refused(reason) => self.refuse(Refusal {
                    tick: now,
                    kind: RefusalKind::IllegalEffect,
                    reason,
                }),
            }
        }
        events
    }

    fn scores(&self) -> &[i64] {
        &self.scores
    }

    fn beacon_holders(&self) -> &[Option<u8>] {
        &self.holders
    }

    fn outcome(&self) -> Option<&Outcome> {
        self.outcome.as_ref()
    }

    fn refusals(&self) -> &[Refusal] {
        &self.refusals
    }

    fn refused_count(&self) -> u64 {
        self.refused
    }
}

impl StateHash for RhaiRuleset {
    fn hash_into(&self, h: &mut Fnv64) {
        hash_rule_state(h, &self.scores, &self.holders, &self.outcome);
    }
}

/// One parsed effect: the effect itself, or the reason it was refused.
enum Parsed {
    /// The effect, ready to apply.
    Effect(Effect),
    /// The effect was refused, with the reason.
    Refused(String),
}

/// Parse the answer of a rules script into effects.
///
/// The answer is an array of maps. An element that is not a map, or a map the
/// contract does not name, is refused on its own: the other effects of the
/// tick still play (D26).
fn parse_effects(answer: Dynamic) -> Result<Vec<Parsed>, String> {
    if answer.is_unit() {
        // A script that says nothing has no effects: the tick is quiet.
        return Ok(Vec::new());
    }
    let array = match answer.as_array_ref() {
        Ok(array) => array,
        Err(_) => {
            return Err(format!(
                "the rules script returned a {}; expected an array of effects",
                answer.type_name()
            ))
        }
    };
    Ok(array.iter().map(parse_effect).collect())
}

/// Parse one effect of a rules script.
///
/// The map carries exactly the fields its kind names: a second key, or a key
/// the kind does not name, is refused (D26).
fn parse_effect(value: &Dynamic) -> Parsed {
    let Some(map) = value.clone().try_cast::<Map>() else {
        return Parsed::Refused(format!(
            "a rules script effect is a {}, not a map",
            value.type_name()
        ));
    };
    let Some(kind) = map.get("kind").and_then(|k| k.clone().into_string().ok()) else {
        return Parsed::Refused("a rules script effect has no kind".to_owned());
    };
    let kind = kind.to_string();
    let fields: &[&str] = match kind.as_str() {
        "tag" => &["by", "target"],
        "stun_until" => &["agent", "until"],
        "respawn" => &["agent"],
        "hold_beacon" => &["beacon", "squad"],
        "score" => &["squad", "points"],
        "round_over" => &[],
        other => {
            return Parsed::Refused(format!(
                "a rules script effect has an unknown kind {other:?}"
            ))
        }
    };
    if map.len() != fields.len() + 1 {
        return Parsed::Refused(format!(
            "a {kind} effect carries {} keys; expected {}",
            map.len(),
            fields.len() + 1
        ));
    }
    for field in fields {
        if !map.contains_key(*field) {
            return Parsed::Refused(format!("a {kind} effect has no {field:?}"));
        }
    }

    match kind.as_str() {
        "tag" => {
            let (Some(by), Some(target)) = (int_field(&map, "by"), int_field(&map, "target"))
            else {
                return Parsed::Refused(
                    "a tag effect needs an integer `by` and an integer `target`".to_owned(),
                );
            };
            let (Some(by), Some(target)) = (as_u16(by), as_u16(target)) else {
                return Parsed::Refused("a tag effect names an agent that no id can be".to_owned());
            };
            Parsed::Effect(Effect::Tag { by, target })
        }
        "stun_until" => {
            let (Some(agent), Some(until)) = (int_field(&map, "agent"), int_field(&map, "until"))
            else {
                return Parsed::Refused(
                    "a stun_until effect needs an integer `agent` and an integer `until`"
                        .to_owned(),
                );
            };
            let (Some(agent), Some(until)) = (as_u16(agent), as_u64(until)) else {
                return Parsed::Refused(
                    "a stun_until effect names an agent or a tick that does not fit".to_owned(),
                );
            };
            Parsed::Effect(Effect::StunUntil { agent, until })
        }
        "respawn" => {
            let Some(agent) = int_field(&map, "agent") else {
                return Parsed::Refused("a respawn effect needs an integer `agent`".to_owned());
            };
            let Some(agent) = as_u16(agent) else {
                return Parsed::Refused(
                    "a respawn effect names an agent that no id can be".to_owned(),
                );
            };
            Parsed::Effect(Effect::Respawn { agent })
        }
        "hold_beacon" => {
            let (Some(beacon), Some(squad)) = (int_field(&map, "beacon"), int_field(&map, "squad"))
            else {
                return Parsed::Refused(
                    "a hold_beacon effect needs an integer `beacon` and an integer `squad`"
                        .to_owned(),
                );
            };
            let Some(beacon) = as_u8(beacon) else {
                return Parsed::Refused(
                    "a hold_beacon effect names a beacon that no id can be".to_owned(),
                );
            };
            // A squad of -1 means: the beacon is held by nobody. Any other
            // squad has to be one the round can have.
            let squad = match squad {
                -1 => None,
                squad => match as_u8(squad) {
                    Some(squad) => Some(squad),
                    None => {
                        return Parsed::Refused(
                            "a hold_beacon effect names a squad that no id can be".to_owned(),
                        )
                    }
                },
            };
            Parsed::Effect(Effect::HoldBeacon { beacon, squad })
        }
        "score" => {
            let (Some(squad), Some(points)) = (int_field(&map, "squad"), int_field(&map, "points"))
            else {
                return Parsed::Refused(
                    "a score effect needs an integer `squad` and an integer `points`".to_owned(),
                );
            };
            let Some(squad) = as_u8(squad) else {
                return Parsed::Refused(
                    "a score effect names a squad that no id can be".to_owned(),
                );
            };
            Parsed::Effect(Effect::Score { squad, points })
        }
        _ => Parsed::Effect(Effect::RoundOver),
    }
}

/// The integer a field of an effect carries.
fn int_field(map: &Map, name: &str) -> Option<i64> {
    map.get(name).and_then(|value| value.as_int().ok())
}

/// An id of an agent.
fn as_u16(value: i64) -> Option<u16> {
    u16::try_from(value).ok()
}

/// An id of a squad or a beacon.
fn as_u8(value: i64) -> Option<u8> {
    u8::try_from(value).ok()
}

/// A tick of the round.
fn as_u64(value: i64) -> Option<u64> {
    u64::try_from(value).ok()
}

/// The index of the agent with that id, if the round has one.
fn index_of(agents: &[Agent], id: u16) -> Option<usize> {
    agents.iter().position(|agent| agent.id == id)
}

/// The `los_clear` function the rules script may call.
///
/// It answers a question about the map — can these two cells see each other —
/// so it is the same answer for every ruleset and every script.
fn los_fn(grid: Grid) -> impl Fn(i64, i64, i64, i64) -> bool + Send + Sync + 'static {
    move |ax: i64, ay: i64, bx: i64, by: i64| -> bool {
        let cell = |x: i64, y: i64| CellXY {
            x: u16::try_from(x).unwrap_or(u16::MAX),
            y: u16::try_from(y).unwrap_or(u16::MAX),
        };
        los::clear(&grid, cell(ax, ay), cell(bx, by))
    }
}

/// Every agent of the round, as the script sees them.
fn agents_map(world: &World, agents: &[Agent], now: Tick) -> Vec<Dynamic> {
    agents
        .iter()
        .map(|agent| agent_map(world, agent, now))
        .collect()
}

/// One agent, as the script sees it.
fn agent_map(world: &World, agent: &Agent, now: Tick) -> Dynamic {
    let cell = world.grid.cell_of(agent.pos, world.cell_units);
    // An agent whose squad has no spawn on this world respawns where it
    // stands: the round has no other cell to put it on.
    let spawn = world
        .spawns
        .get(usize::from(agent.squad))
        .copied()
        .unwrap_or(cell);
    let mut map = Map::new();
    map.insert("cell".into(), cell_array(cell));
    map.insert("id".into(), Dynamic::from(i64::from(agent.id)));
    map.insert("pos".into(), pos_array(agent.pos));
    map.insert("spawn_cell".into(), cell_array(spawn));
    map.insert(
        "spawn_pos".into(),
        pos_array(world.grid.center(spawn, world.cell_units)),
    );
    map.insert("squad".into(), Dynamic::from(i64::from(agent.squad)));
    map.insert("stunned".into(), Dynamic::from(is_stunned(agent, now)));
    map.insert(
        "stunned_until".into(),
        Dynamic::from(agent.stunned_until.map_or(-1, tick_int)),
    );
    Dynamic::from_map(map)
}

/// Every beacon of the round, as the script sees them.
fn beacons_map(world: &World, holders: &[Option<u8>]) -> Vec<Dynamic> {
    world
        .beacons
        .iter()
        .enumerate()
        .map(|(index, cell)| {
            let holder = holders.get(index).copied().flatten().map_or(-1, i64::from);
            let mut map = Map::new();
            map.insert("cell".into(), cell_array(*cell));
            map.insert("holder".into(), Dynamic::from(holder));
            map.insert("index".into(), Dynamic::from(index as i64));
            map.insert(
                "pos".into(),
                pos_array(world.grid.center(*cell, world.cell_units)),
            );
            Dynamic::from_map(map)
        })
        .collect()
}

/// The order the agents act in this tick.
fn order_map(order: &[usize]) -> Vec<Dynamic> {
    order
        .iter()
        .map(|index| Dynamic::from(*index as i64))
        .collect()
}

/// The parameters the rules read.
fn params_map(p: &Params) -> Dynamic {
    let mut map = Map::new();
    map.insert(
        "beacon_radius_units".into(),
        Dynamic::from(i64::from(p.beacon_radius_units)),
    );
    map.insert("squads".into(), Dynamic::from(i64::from(p.squads)));
    map.insert("stun_ticks".into(), Dynamic::from(i64::from(p.stun_ticks)));
    map.insert(
        "tag_range_units".into(),
        Dynamic::from(i64::from(p.tag_range_units)),
    );
    map.insert(
        "time_limit_ticks".into(),
        Dynamic::from(i64::from(p.time_limit_ticks)),
    );
    map.insert("win_score".into(), Dynamic::from(i64::from(p.win_score)));
    Dynamic::from_map(map)
}

/// The score of every squad, as the script sees it.
fn score_map(scores: &[i64]) -> Vec<Dynamic> {
    scores.iter().map(|score| Dynamic::from(*score)).collect()
}

/// A cell, as the script sees it: `[x, y]`.
fn cell_array(cell: CellXY) -> Dynamic {
    Dynamic::from(vec![
        Dynamic::from(i64::from(cell.x)),
        Dynamic::from(i64::from(cell.y)),
    ])
}

/// A position, as the script sees it: `[x, y]` in 1/1024 m units.
fn pos_array(pos: crate::fixed::Pos) -> Dynamic {
    let (x, y) = pos.to_units();
    Dynamic::from(vec![Dynamic::from(x), Dynamic::from(y)])
}

/// A tick, as the script sees it.
fn tick_int(tick: Tick) -> i64 {
    i64::try_from(tick).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header::Header;
    use crate::sim::Sim;

    /// The reflex every agent of a standard round runs.
    const REFLEX: &str = include_str!("../../../../assets/reflexes/default.rhai");

    /// The Beacons rules, as a script.
    const BEACONS_RHAI: &str = include_str!("../../../../assets/rules/beacons.rhai");

    /// A header for `seed` that plays the round with the native ruleset.
    fn native_header(seed: u64) -> Header {
        Header::default_round(seed, REFLEX)
    }

    /// A header for `seed` that plays the round with the scripted ruleset.
    fn rhai_header(seed: u64) -> Header {
        let mut h = native_header(seed);
        h.ruleset = "rhai:beacons".to_owned();
        h.scripts
            .insert("beacons".to_owned(), BEACONS_RHAI.to_owned());
        h
    }

    /// What a round of `ticks` ticks produced: the hashes and the events.
    #[derive(Debug, PartialEq, Eq)]
    struct Played {
        hashes: Vec<u64>,
        events: Vec<Vec<Event>>,
        refusals: Vec<String>,
    }

    /// Play a round of `ticks` ticks and record what it produced.
    fn play(h: &Header, ticks: u64) -> Played {
        let mut sim = Sim::new(h).expect("the round starts");
        let mut played = Played {
            hashes: Vec::new(),
            events: Vec::new(),
            refusals: Vec::new(),
        };
        for _ in 0..ticks {
            let out = sim.step(&[]);
            if let Some(hash) = out.hash {
                played.hashes.push(hash);
            }
            played.events.push(out.events);
        }
        played.refusals = sim
            .ruleset_refusals()
            .iter()
            .map(|r| format!("{:?}: {}", r.kind, r.reason))
            .collect();
        played
    }

    #[test]
    fn the_shipped_script_loads_and_answers() {
        let world = crate::world::generate(1, &Params::default());
        let p = Params::default();
        let rules = RhaiRuleset::new("beacons", BEACONS_RHAI, &world, &p)
            .expect("the shipped rules script loads");
        assert_eq!(rules.scores().len(), usize::from(p.squads));
        assert!(rules.outcome().is_none());
    }

    #[test]
    fn a_script_without_an_entry_point_is_refused() {
        let world = crate::world::generate(1, &Params::default());
        let p = Params::default();
        let Err(err) = RhaiRuleset::new("bad", "fn other(v){ v }", &world, &p) else {
            panic!("a script without fn rules(v) must be refused");
        };
        assert!(matches!(err, ScriptError::NoRulesEntryPoint(_)));
    }

    #[test]
    fn a_script_that_does_not_parse_is_refused() {
        let world = crate::world::generate(1, &Params::default());
        let p = Params::default();
        let Err(err) = RhaiRuleset::new("bad", "fn (", &world, &p) else {
            panic!("a script that does not parse must be refused");
        };
        assert!(matches!(err, ScriptError::Parse(_)));
    }

    #[test]
    fn a_float_in_a_rules_script_is_refused() {
        let world = crate::world::generate(1, &Params::default());
        let p = Params::default();
        let Err(err) = RhaiRuleset::new("bad", "fn rules(v){ 1.5 }", &world, &p) else {
            panic!("a float literal must be refused");
        };
        assert!(matches!(err, ScriptError::Parse(_)));
    }

    /// The oracle: the scripted ruleset plays the round exactly as the native
    /// one does.
    ///
    /// Every seed is played twice — once under the native ruleset, once under
    /// the scripted one — and the two hash sequences and event streams have to
    /// be identical, tick for tick. A difference is a bug in the script or in
    /// the layer that applies its effects, never in the oracle.
    #[test]
    fn rhai_beacons_matches_native() {
        const SEEDS: u64 = 50;
        const TICKS: u64 = 3_000;

        // The seeds are independent, so they are played side by side: the
        // oracle is a long test and there is no reason to wait for it. The
        // threads are capped: past a handful of them the rounds spend more
        // time fighting over the allocator than they save.
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .clamp(1, 8)
            .min(SEEDS as usize);
        let mut failures = Vec::new();
        let seeds: Vec<u64> = (1..=SEEDS).collect();
        std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for chunk in seeds.chunks(threads) {
                handles.push(scope.spawn(move || {
                    let mut failures = Vec::new();
                    for &seed in chunk {
                        let native = play(&native_header(seed), TICKS);
                        let scripted = play(&rhai_header(seed), TICKS);
                        if native.hashes != scripted.hashes {
                            let tick = native
                                .hashes
                                .iter()
                                .zip(&scripted.hashes)
                                .position(|(a, b)| a != b)
                                .unwrap_or(native.hashes.len().min(scripted.hashes.len()));
                            failures.push(format!(
                                "seed {seed}: the hashes differ at hash {tick} ({} vs {})",
                                native.hashes.get(tick).copied().unwrap_or(0),
                                scripted.hashes.get(tick).copied().unwrap_or(0)
                            ));
                            continue;
                        }
                        if native.events != scripted.events {
                            let tick = native
                                .events
                                .iter()
                                .zip(&scripted.events)
                                .position(|(a, b)| a != b)
                                .unwrap_or(native.events.len().min(scripted.events.len()));
                            failures.push(format!(
                                "seed {seed}: the events differ at tick {tick}:\n  native:    {:?}\n  scripted: {:?}",
                                native.events.get(tick),
                                scripted.events.get(tick)
                            ));
                        }
                    }
                    failures
                }));
            }
            for handle in handles {
                failures.extend(handle.join().expect("the oracle thread finishes"));
            }
        });

        assert!(
            failures.is_empty(),
            "the scripted ruleset is not the native one:\n{}",
            failures.join("\n")
        );
    }

    /// A script that never returns: the tick has no effects, the refusal is
    /// recorded, and the round goes on.
    ///
    /// The round must not stop: a broken rules script is a broken tick, not a
    /// broken round. The agents keep moving and the hashes keep coming.
    #[test]
    fn a_script_that_never_returns_runs_out_of_fuel_and_the_round_goes_on() {
        let mut h = native_header(1);
        h.ruleset = "rhai:loop".to_owned();
        h.scripts
            .insert("loop".to_owned(), "fn rules(v) { loop { } }".to_owned());
        let mut sim = Sim::new(&h).expect("the round starts");

        let mut hashes = 0;
        for _ in 0..120 {
            let out = sim.step(&[]);
            if out.hash.is_some() {
                hashes += 1;
            }
            assert!(
                out.events.is_empty(),
                "a tick whose script ran out of fuel has no effects: {:?}",
                out.events
            );
        }
        assert!(hashes > 0, "the round must keep going");

        let refusals = sim.ruleset_refusals();
        assert!(!refusals.is_empty(), "the refusal must be recorded");
        assert!(
            refusals.iter().all(|r| r.kind == RefusalKind::OutOfFuel),
            "the refusal must name the fuel: {:?}",
            refusals
        );
        assert_eq!(sim.stats().ruleset_refused, refusals.len() as u64);
    }

    /// An effect that names an agent the round does not have is refused: it
    /// is never applied, and the refusal is recorded with its reason.
    #[test]
    fn an_illegal_effect_is_refused_and_logged() {
        let mut h = native_header(1);
        h.ruleset = "rhai:bad".to_owned();
        h.scripts.insert(
            "bad".to_owned(),
            r#"fn rules(v) { [ #{ kind: "tag", by: 0, "target": 999 } ] }"#.to_owned(),
        );
        let mut sim = Sim::new(&h).expect("the round starts");

        let out = sim.step(&[]);
        assert!(
            out.events.is_empty(),
            "an illegal effect must never be applied: {:?}",
            out.events
        );
        let refusals = sim.ruleset_refusals();
        assert_eq!(refusals.len(), 1, "{refusals:?}");
        assert_eq!(refusals[0].kind, RefusalKind::IllegalEffect);
        assert!(refusals[0].reason.contains("999"), "{}", refusals[0].reason);
        assert_eq!(sim.stats().ruleset_refused, 1);
    }

    /// A script that answers something the contract does not name is refused
    /// whole: the tick has no effects at all.
    #[test]
    fn an_answer_that_is_not_a_list_of_effects_is_refused() {
        let mut h = native_header(1);
        h.ruleset = "rhai:bad".to_owned();
        h.scripts
            .insert("bad".to_owned(), "fn rules(v) { 42 }".to_owned());
        let mut sim = Sim::new(&h).expect("the round starts");

        let out = sim.step(&[]);
        assert!(out.events.is_empty(), "{:?}", out.events);
        let refusals = sim.ruleset_refusals();
        assert_eq!(refusals.len(), 1, "{refusals:?}");
        assert_eq!(refusals[0].kind, RefusalKind::BadAnswer);
    }

    /// An effect with a field the contract does not name is refused on its
    /// own: the other effects of the tick still play.
    #[test]
    fn an_effect_with_a_stray_field_is_refused_and_the_rest_still_play() {
        let mut h = native_header(1);
        h.ruleset = "rhai:bad".to_owned();
        h.scripts.insert(
            "bad".to_owned(),
            r#"fn rules(v) { [ #{ kind: "respawn", agent: 0, extra: 1 } ] }"#.to_owned(),
        );
        let mut sim = Sim::new(&h).expect("the round starts");

        let out = sim.step(&[]);
        assert!(
            out.events.is_empty(),
            "the effect with the stray field must not be applied: {:?}",
            out.events
        );
        let refusals = sim.ruleset_refusals();
        assert_eq!(refusals.len(), 1, "{refusals:?}");
        assert_eq!(refusals[0].kind, RefusalKind::IllegalEffect);
    }

    /// A script that says nothing asks for nothing: the tick is quiet and
    /// nothing is refused.
    #[test]
    fn a_silent_script_is_not_a_refusal() {
        let mut h = native_header(1);
        h.ruleset = "rhai:quiet".to_owned();
        h.scripts
            .insert("quiet".to_owned(), "fn rules(v) { [] }".to_owned());
        let mut sim = Sim::new(&h).expect("the round starts");

        let out = sim.step(&[]);
        assert!(out.events.is_empty(), "{:?}", out.events);
        assert!(sim.ruleset_refusals().is_empty());
        assert_eq!(sim.stats().ruleset_refused, 0);
    }
}
