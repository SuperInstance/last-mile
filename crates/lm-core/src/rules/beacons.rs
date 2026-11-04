//! The "Beacons" ruleset: tagging, holding, scoring and the round outcome.
//!
//! Beacons is the default mode of M1 (spec §4): squads fight over beacons on
//! a procedural map, contact is a tag rather than a kill, and a squad scores
//! for every tick it holds a beacon.
//!
//! The tick of the ruleset runs in four fixed phases, in this order:
//!
//! 1. **Respawn.** Every stun that has ended puts its agent back on its
//!    squad's spawn and reports [`Event::Respawned`].
//! 2. **Tagging.** The agents act in the tick's rotating order. An agent
//!    that is not stunned tags the first enemy that is not stunned, is within
//!    `tag_range_units` and is in line of sight. The target is stunned for
//!    `stun_ticks` ticks. A tag never comes back in the same tick, so two
//!    agents that can see each other cannot trade tags.
//! 3. **Holding.** A beacon is held by the squad that has a living agent
//!    inside `beacon_radius_units` of it while no enemy is inside that
//!    radius. A beacon two squads stand in is held by nobody. Every held
//!    beacon is worth one point per tick.
//! 4. **Ending.** The round ends when a squad reaches `win_score` or the
//!    tick limit runs out. The higher score wins; a tie is a tie.

use super::{add_score, beacon_id, hash_rule_state, is_stunned, winner, Event, Outcome, Ruleset};
use crate::agent::Agent;
use crate::decision::Tick;
use crate::fixed::dist2;
use crate::hash::{Fnv64, StateHash};
use crate::params::Params;
use crate::world::{los, World};
use std::collections::BTreeSet;

/// The Beacons ruleset.
///
/// It is pure state plus the tick: the scores of the squads, who holds every
/// beacon and — once the round is over — how it ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Beacons {
    /// The points of every squad, in squad order.
    scores: Vec<i64>,
    /// Who holds every beacon, in beacon order.
    holders: Vec<Option<u8>>,
    /// The outcome of the round, once it is over.
    outcome: Option<Outcome>,
}

impl Beacons {
    /// A fresh ruleset for a round on `world`.
    ///
    /// The number of squads and the number of beacons come from the
    /// parameters and the world, so the ruleset never guesses a size.
    pub fn new(world: &World, p: &Params) -> Beacons {
        Beacons {
            scores: vec![0; usize::from(p.squads)],
            holders: vec![None; world.beacons.len()],
            outcome: None,
        }
    }

    /// Add points to a squad, if the round has a score for it.
    fn add_score(&mut self, squad: u8, points: i64) {
        add_score(&mut self.scores, squad, points);
    }
}

impl Ruleset for Beacons {
    fn step(
        &mut self,
        world: &World,
        agents: &mut [Agent],
        now: Tick,
        order: &[usize],
        p: &Params,
    ) -> Vec<Event> {
        let mut events = Vec::new();
        // The round is over: nothing else happens until the next one starts.
        if self.outcome.is_some() {
            return events;
        }

        // 1. A stun that has ended puts its agent back on its spawn.
        for agent in agents.iter_mut() {
            let Some(end) = agent.stunned_until else {
                continue;
            };
            if now < end {
                continue;
            }
            agent.stunned_until = None;
            if let Some(&spawn) = world.spawns.get(usize::from(agent.squad)) {
                agent.pos = world.grid.center(spawn, world.cell_units);
            }
            events.push(Event::Respawned(agent.id));
        }

        // 2. Tagging, in the tick's rotating order.
        //
        // `tagged` holds the agents that were tagged during this tick: a tag
        // never comes back in the same tick, so two agents that can see each
        // other cannot trade tags.
        let tag_range2 = squared(p.tag_range_units);
        let mut tagged: BTreeSet<usize> = BTreeSet::new();
        for &index in order {
            if index >= agents.len() {
                continue;
            }
            // A stunned agent does not act, and neither does an agent that
            // was tagged earlier in this tick.
            if is_stunned(&agents[index], now) || tagged.contains(&index) {
                continue;
            }
            let tagger_id = agents[index].id;
            let tagger_squad = agents[index].squad;
            let tagger_pos = agents[index].pos;
            let tagger_cell = world.grid.cell_of(tagger_pos, world.cell_units);

            // The first enemy that is not stunned, is in range and is in
            // sight.
            let mut target: Option<usize> = None;
            for (j, other) in agents.iter().enumerate() {
                if j == index || other.squad == tagger_squad {
                    continue;
                }
                if is_stunned(other, now) || tagged.contains(&j) {
                    continue;
                }
                if dist2(tagger_pos, other.pos) > tag_range2 {
                    continue;
                }
                let other_cell = world.grid.cell_of(other.pos, world.cell_units);
                if !los::clear(&world.grid, tagger_cell, other_cell) {
                    continue;
                }
                target = Some(j);
                break;
            }

            if let Some(j) = target {
                let target_id = agents[j].id;
                agents[j].stunned_until = Some(now.saturating_add(u64::from(p.stun_ticks)));
                tagged.insert(j);
                events.push(Event::Tagged {
                    by: tagger_id,
                    target: target_id,
                });
            }
        }

        // 3. Holding: a beacon belongs to the squad that has a living agent
        //    inside the radius while no enemy is inside it.
        let radius2 = squared(p.beacon_radius_units);
        for (index, &beacon) in world.beacons.iter().enumerate() {
            let beacon_pos = world.grid.center(beacon, world.cell_units);
            let mut holder: Option<u8> = None;
            for squad in 0..p.squads {
                let ours = agents.iter().any(|a| {
                    a.squad == squad && !is_stunned(a, now) && dist2(a.pos, beacon_pos) <= radius2
                });
                if !ours {
                    continue;
                }
                let enemy_near = agents.iter().any(|a| {
                    a.squad != squad && !is_stunned(a, now) && dist2(a.pos, beacon_pos) <= radius2
                });
                if !enemy_near {
                    // At most one squad can get here: two squads inside the
                    // radius would each see the other one as an enemy.
                    holder = Some(squad);
                }
            }

            let held = self.holders.get(index).copied().flatten();
            if held != holder {
                if held.is_some() {
                    events.push(Event::BeaconLost {
                        beacon: beacon_id(index),
                    });
                }
                if let Some(squad) = holder {
                    events.push(Event::BeaconHeld {
                        beacon: beacon_id(index),
                        squad,
                    });
                }
                if let Some(slot) = self.holders.get_mut(index) {
                    *slot = holder;
                }
            }
            if let Some(squad) = holder {
                self.add_score(squad, 1);
            }
        }

        // 4. The round ends on the score or on the clock.
        let reached_score = self
            .scores
            .iter()
            .any(|&score| score >= i64::from(p.win_score));
        let out_of_time = now >= u64::from(p.time_limit_ticks);
        if reached_score || out_of_time {
            let outcome = Outcome {
                winner: winner(&self.scores),
                scores: self.scores.clone(),
                tick: now,
            };
            events.push(Event::RoundOver(outcome.clone()));
            self.outcome = Some(outcome);
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
}

impl StateHash for Beacons {
    fn hash_into(&self, h: &mut Fnv64) {
        hash_rule_state(h, &self.scores, &self.holders, &self.outcome);
    }
}

/// A radius in units as the squared distance to compare with.
fn squared(units: i32) -> i64 {
    let units = i64::from(units);
    units * units
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header::Role;
    use crate::world::{CellKind, CellXY, Grid, World};

    /// A flat 32x32 world of 1 m cells with the given spawns and beacons.
    fn flat_world(spawns: [CellXY; 2], beacons: Vec<CellXY>) -> World {
        World {
            grid: Grid::new(32, 32),
            beacons,
            spawns: spawns.to_vec(),
            cell_units: 1024,
        }
    }

    /// An agent standing at the centre of `cell`.
    fn agent_at(world: &World, id: u16, squad: u8, cell: CellXY) -> Agent {
        let role = if id.is_multiple_of(4) {
            Role::Leader
        } else {
            Role::Member
        };
        Agent::new(id, squad, role, world.grid.center(cell, world.cell_units))
    }

    /// The rotating order of a tick: agent `(k + tick % n) % n` acts k-th.
    fn rotated(n: usize, tick: Tick) -> Vec<usize> {
        let n64 = n as u64;
        (0..n)
            .map(|k| ((k as u64 + tick % n64) % n64) as usize)
            .collect()
    }

    /// The ids that were tagged by the events.
    fn taggers(events: &[Event]) -> Vec<u16> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::Tagged { by, .. } => Some(*by),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn tag_stuns_and_respawns() {
        let p = Params::default();
        let world = flat_world(
            [CellXY { x: 4, y: 4 }, CellXY { x: 20, y: 20 }],
            vec![CellXY { x: 28, y: 28 }],
        );
        let mut agents = vec![
            agent_at(&world, 0, 0, CellXY { x: 4, y: 4 }),
            agent_at(&world, 4, 1, CellXY { x: 5, y: 4 }),
        ];
        let mut rules = Beacons::new(&world, &p);
        let order = rotated(agents.len(), 0);

        let events = rules.step(&world, &mut agents, 0, &order, &p);
        assert!(
            events.contains(&Event::Tagged { by: 0, target: 4 }),
            "{events:?}"
        );
        assert_eq!(agents[1].stunned_until, Some(90));

        // Ninety ticks later the stun is over: the agent is back on its
        // squad's spawn and the rules say so.
        let events = rules.step(&world, &mut agents, 90, &order, &p);
        assert!(events.contains(&Event::Respawned(4)), "{events:?}");
        assert_eq!(
            agents[1].pos,
            world.grid.center(world.spawns[1], world.cell_units)
        );
        assert_eq!(agents[1].stunned_until, None);
    }

    #[test]
    fn no_tag_through_wall() {
        let p = Params::default();
        let mut world = flat_world(
            [CellXY { x: 4, y: 4 }, CellXY { x: 6, y: 4 }],
            vec![CellXY { x: 28, y: 28 }],
        );
        // A wall between the two agents: they are in range but out of sight.
        world.grid.set(CellXY { x: 5, y: 4 }, CellKind::Wall);
        let mut agents = vec![
            agent_at(&world, 0, 0, CellXY { x: 4, y: 4 }),
            agent_at(&world, 4, 1, CellXY { x: 6, y: 4 }),
        ];
        let mut rules = Beacons::new(&world, &p);
        let order = rotated(agents.len(), 0);

        let events = rules.step(&world, &mut agents, 0, &order, &p);
        assert!(taggers(&events).is_empty(), "{events:?}");
        assert_eq!(agents[1].stunned_until, None);
    }

    #[test]
    fn rotation_decides_mutual_tags_fairly() {
        let p = Params {
            stun_ticks: 1,
            ..Params::default()
        };
        let world = flat_world(
            [CellXY { x: 4, y: 4 }, CellXY { x: 5, y: 4 }],
            vec![CellXY { x: 28, y: 28 }],
        );
        let mut agents = vec![
            agent_at(&world, 0, 0, CellXY { x: 4, y: 4 }),
            agent_at(&world, 4, 1, CellXY { x: 5, y: 4 }),
        ];
        let mut rules = Beacons::new(&world, &p);

        for tick in 0..4u64 {
            let order = rotated(agents.len(), tick);
            let events = rules.step(&world, &mut agents, tick, &order, &p);
            let expected = if tick % 2 == 0 { 0 } else { 4 };
            assert_eq!(taggers(&events), vec![expected], "tick {tick}: {events:?}");
        }
    }

    #[test]
    fn contested_beacon_scores_nobody() {
        let p = Params::default();
        let world = flat_world(
            [CellXY { x: 4, y: 4 }, CellXY { x: 20, y: 20 }],
            vec![CellXY { x: 10, y: 4 }],
        );
        // Both squads stand inside the beacon radius, but too far apart to
        // tag each other: the beacon is contested and scores nobody.
        let mut agents = vec![
            agent_at(&world, 0, 0, CellXY { x: 8, y: 4 }),
            agent_at(&world, 4, 1, CellXY { x: 12, y: 4 }),
        ];
        let mut rules = Beacons::new(&world, &p);
        let order = rotated(agents.len(), 0);

        let events = rules.step(&world, &mut agents, 0, &order, &p);
        assert!(
            !events.iter().any(|e| matches!(e, Event::BeaconHeld { .. })),
            "{events:?}"
        );
        assert_eq!(rules.scores(), &[0, 0]);
    }

    #[test]
    fn holding_scores_per_tick_and_wins() {
        let p = Params {
            win_score: 10,
            ..Params::default()
        };
        let world = flat_world(
            [CellXY { x: 4, y: 4 }, CellXY { x: 20, y: 20 }],
            vec![CellXY { x: 10, y: 4 }],
        );
        let mut agents = vec![agent_at(&world, 0, 0, CellXY { x: 9, y: 4 })];
        let mut rules = Beacons::new(&world, &p);
        let order = rotated(agents.len(), 0);

        for tick in 0..10u64 {
            let events = rules.step(&world, &mut agents, tick, &order, &p);
            if tick == 0 {
                assert!(
                    events.contains(&Event::BeaconHeld {
                        beacon: 0,
                        squad: 0
                    }),
                    "{events:?}"
                );
            }
        }
        assert_eq!(rules.scores(), &[10, 0]);
        let outcome = rules.outcome().expect("the round is over");
        assert_eq!(outcome.winner, Some(0));
        assert_eq!(outcome.scores, vec![10, 0]);
        assert_eq!(outcome.tick, 9);
    }

    #[test]
    fn time_limit_tie() {
        let p = Params {
            time_limit_ticks: 5,
            ..Params::default()
        };
        let world = flat_world(
            [CellXY { x: 4, y: 4 }, CellXY { x: 20, y: 20 }],
            vec![CellXY { x: 28, y: 28 }],
        );
        let mut agents = vec![agent_at(&world, 0, 0, CellXY { x: 4, y: 4 })];
        let mut rules = Beacons::new(&world, &p);
        let order = rotated(agents.len(), 0);

        for tick in 0..5u64 {
            let events = rules.step(&world, &mut agents, tick, &order, &p);
            assert!(
                !events.iter().any(|e| matches!(e, Event::RoundOver(_))),
                "tick {tick}: {events:?}"
            );
        }
        let events = rules.step(&world, &mut agents, 5, &order, &p);
        let outcome = events
            .iter()
            .find_map(|e| match e {
                Event::RoundOver(outcome) => Some(outcome.clone()),
                _ => None,
            })
            .expect("the round ends at the tick limit");
        assert_eq!(outcome.winner, None);
        assert_eq!(outcome.scores, vec![0, 0]);
        assert_eq!(outcome.tick, 5);
        assert_eq!(rules.outcome().map(|o| o.winner), Some(None));
    }

    #[test]
    fn no_counter_tag_in_the_same_tick() {
        // A stun of zero ticks frees the target immediately, but the tag that
        // stunned it still costs it this tick: the two agents cannot trade
        // tags inside one tick.
        let p = Params {
            stun_ticks: 0,
            ..Params::default()
        };
        let world = flat_world(
            [CellXY { x: 4, y: 4 }, CellXY { x: 5, y: 4 }],
            vec![CellXY { x: 28, y: 28 }],
        );
        let mut agents = vec![
            agent_at(&world, 0, 0, CellXY { x: 4, y: 4 }),
            agent_at(&world, 4, 1, CellXY { x: 5, y: 4 }),
        ];
        let mut rules = Beacons::new(&world, &p);
        let order = rotated(agents.len(), 0);

        let events = rules.step(&world, &mut agents, 0, &order, &p);
        assert_eq!(taggers(&events), vec![0], "{events:?}");
    }

    #[test]
    fn beacon_is_lost_when_the_holder_leaves() {
        let p = Params::default();
        let world = flat_world(
            [CellXY { x: 4, y: 4 }, CellXY { x: 20, y: 20 }],
            vec![CellXY { x: 10, y: 4 }],
        );
        let mut agents = vec![
            agent_at(&world, 0, 0, CellXY { x: 9, y: 4 }),
            agent_at(&world, 4, 1, CellXY { x: 20, y: 20 }),
        ];
        let mut rules = Beacons::new(&world, &p);
        let order = rotated(agents.len(), 0);

        let events = rules.step(&world, &mut agents, 0, &order, &p);
        assert!(
            events.contains(&Event::BeaconHeld {
                beacon: 0,
                squad: 0
            }),
            "{events:?}"
        );
        assert_eq!(rules.scores(), &[1, 0]);

        // The holder walks away: the beacon is lost and stops scoring.
        agents[0].pos = world.grid.center(CellXY { x: 4, y: 4 }, world.cell_units);
        let events = rules.step(&world, &mut agents, 1, &order, &p);
        assert!(
            events.contains(&Event::BeaconLost { beacon: 0 }),
            "{events:?}"
        );
        assert_eq!(rules.scores(), &[1, 0]);
    }

    #[test]
    fn the_higher_score_wins_at_the_time_limit() {
        let p = Params {
            time_limit_ticks: 3,
            ..Params::default()
        };
        let world = flat_world(
            [CellXY { x: 4, y: 4 }, CellXY { x: 20, y: 20 }],
            vec![CellXY { x: 10, y: 4 }],
        );
        let mut agents = vec![
            agent_at(&world, 0, 0, CellXY { x: 9, y: 4 }),
            agent_at(&world, 4, 1, CellXY { x: 20, y: 20 }),
        ];
        let mut rules = Beacons::new(&world, &p);
        let order = rotated(agents.len(), 0);

        for tick in 0..=3u64 {
            rules.step(&world, &mut agents, tick, &order, &p);
        }
        // The beacon scored on the tick the round ended as well.
        assert_eq!(rules.scores(), &[4, 0]);
        let outcome = rules.outcome().expect("the round is over");
        assert_eq!(outcome.winner, Some(0));
        assert_eq!(outcome.tick, 3);
    }

    #[test]
    fn the_round_stays_over() {
        let p = Params {
            time_limit_ticks: 2,
            ..Params::default()
        };
        let world = flat_world(
            [CellXY { x: 4, y: 4 }, CellXY { x: 20, y: 20 }],
            vec![CellXY { x: 28, y: 28 }],
        );
        let mut agents = vec![agent_at(&world, 0, 0, CellXY { x: 4, y: 4 })];
        let mut rules = Beacons::new(&world, &p);
        let order = rotated(agents.len(), 0);

        rules.step(&world, &mut agents, 2, &order, &p);
        assert!(rules.outcome().is_some());
        // After the round is over the rules are inert.
        let events = rules.step(&world, &mut agents, 3, &order, &p);
        assert!(events.is_empty(), "{events:?}");
    }

    #[test]
    fn rules_hash_is_stable_and_sensitive() {
        let p = Params::default();
        let world = flat_world(
            [CellXY { x: 4, y: 4 }, CellXY { x: 20, y: 20 }],
            vec![CellXY { x: 10, y: 4 }],
        );
        let mut agents = vec![agent_at(&world, 0, 0, CellXY { x: 9, y: 4 })];
        let mut rules = Beacons::new(&world, &p);
        let before = rules.state_hash();
        assert_eq!(before, Beacons::new(&world, &p).state_hash());

        let order = rotated(agents.len(), 0);
        rules.step(&world, &mut agents, 0, &order, &p);
        assert_ne!(before, rules.state_hash());
    }
}
