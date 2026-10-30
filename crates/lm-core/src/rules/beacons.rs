//! The "Beacons" ruleset: tagging, holding, scoring and the round outcome.
//!
//! Beacons is the default mode of M1 (spec §4): squads fight over beacons on
//! a procedural map, contact is a tag rather than a kill, and a squad scores
//! for every tick it holds a beacon.
//!
//! The tick of the ruleset runs in five fixed phases, in this order:
//!
//! 1. **The paint.** An agent standing in the other team's paint is slowed,
//!    and the paint pushes it back every N-th tick of contact. The rule is
//!    [`super::paint`], and it is off until a knob is turned: a round whose
//!    paint rules are off is the round it always was.
//! 2. **Respawn.** Every stun that has ended puts its agent back on its
//!    squad's spawn and reports [`Event::Respawned`].
//! 3. **Tagging.** The agents act in the tick's rotating order. An agent
//!    that is not stunned tags the first enemy that is not stunned, is within
//!    `tag_range_units` and is in line of sight. The target is stunned for
//!    `stun_ticks` ticks. A tag never comes back in the same tick, so two
//!    agents that can see each other cannot trade tags. The tag also leaves
//!    a splat of paint: the tagged agent's cell and the cells around it,
//!    within `splat_radius_cells`, in the tagger's colour.
//! 4. **Holding.** A beacon is held by the squad that has a living agent
//!    inside `beacon_radius_units` of it while no enemy is inside that
//!    radius. A beacon two squads stand in is held by nobody. Every held
//!    beacon is worth one point per tick.
//! 5. **Ending.** The round ends when a squad reaches `win_score` or the
//!    tick limit runs out. The higher score wins; a tie is a tie.

use super::{add_score, beacon_id, hash_rule_state, is_stunned, winner, Event, Outcome, Ruleset};
use crate::agent::Agent;
use crate::decision::Tick;
use crate::fixed::dist2;
use crate::hash::{Fnv64, StateHash};
use crate::params::Params;
use crate::world::{los, splat_cells, World};
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
        world: &mut World,
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

        // 1. The paint: an agent standing in the other team's paint is
        //    slowed, and the paint pushes it back every N-th tick of contact.
        //
        //    The paint is read at the start of the tick, before any splat of
        //    this tick is written: a splat is felt from the next tick on. The
        //    slow is written into the agent and the motor of the next tick
        //    obeys it; the push-back is applied here, so an agent that steps
        //    into the paint is pushed out at the end of this same tick.
        //
        //    With both knobs at zero the paint plays no rule at all, and the
        //    round is the round it always was.
        if super::paint::paint_rules_on(p) {
            let period = super::paint::bounce_period(p.bounce_q8);
            for agent in agents.iter_mut() {
                let cell = world.grid.cell_of(agent.pos, world.cell_units);
                let contact = super::paint::enemy_paint_under(&world.paint, cell, agent.squad);
                if contact.is_none() {
                    agent.paint_contact = 0;
                    agent.paint_slow_q8 = 0;
                    continue;
                }
                agent.paint_contact = agent.paint_contact.saturating_add(1);
                agent.paint_slow_q8 = p.slow_q8;
                // The push-back: on every N-th tick of contact the paint
                // sends the agent back to the cell it came from.
                if period > 0 && agent.paint_contact % u32::from(period) == 0 {
                    if let Some(from) = agent.paint_from {
                        agent.pos = world.grid.center(from, world.cell_units);
                    }
                }
            }
        }

        // 2. A stun that has ended puts its agent back on its spawn.
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

        // 3. Tagging, in the tick's rotating order.
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
                // The tag leaves a splat: the tagged agent's cell and the
                // cells around it, in the tagger's colour. The splat is the
                // readable consequence of the tag, and it is world state.
                let target_cell = world.grid.cell_of(agents[j].pos, world.cell_units);
                for cell in splat_cells(&world.grid, target_cell, p.splat_radius_cells) {
                    world.paint.splat(cell, tagger_squad);
                }
                events.push(Event::Tagged {
                    by: tagger_id,
                    target: target_id,
                });
            }
        }

        // 4. Holding: a beacon belongs to the squad that has a living agent
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

        // 5. The round ends on the score or on the clock.
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
            paint: crate::world::Paint::new(),
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
        let mut world = flat_world(
            [CellXY { x: 4, y: 4 }, CellXY { x: 20, y: 20 }],
            vec![CellXY { x: 28, y: 28 }],
        );
        let mut agents = vec![
            agent_at(&world, 0, 0, CellXY { x: 4, y: 4 }),
            agent_at(&world, 4, 1, CellXY { x: 5, y: 4 }),
        ];
        let mut rules = Beacons::new(&world, &p);
        let order = rotated(agents.len(), 0);

        let events = rules.step(&mut world, &mut agents, 0, &order, &p);
        assert!(
            events.contains(&Event::Tagged { by: 0, target: 4 }),
            "{events:?}"
        );
        assert_eq!(agents[1].stunned_until, Some(90));

        // Ninety ticks later the stun is over: the agent is back on its
        // squad's spawn and the rules say so.
        let events = rules.step(&mut world, &mut agents, 90, &order, &p);
        assert!(events.contains(&Event::Respawned(4)), "{events:?}");
        assert_eq!(
            agents[1].pos,
            world.grid.center(world.spawns[1], world.cell_units)
        );
        assert_eq!(agents[1].stunned_until, None);
    }

    #[test]
    fn a_tag_leaves_a_splat() {
        let p = Params::default();
        let mut world = flat_world(
            [CellXY { x: 4, y: 4 }, CellXY { x: 20, y: 20 }],
            vec![CellXY { x: 28, y: 28 }],
        );
        let mut agents = vec![
            agent_at(&world, 0, 0, CellXY { x: 4, y: 4 }),
            agent_at(&world, 4, 1, CellXY { x: 5, y: 4 }),
        ];
        let mut rules = Beacons::new(&world, &p);
        let order = rotated(agents.len(), 0);

        let events = rules.step(&mut world, &mut agents, 0, &order, &p);
        assert!(
            events.contains(&Event::Tagged { by: 0, target: 4 }),
            "{events:?}"
        );

        // The splat is the tagged cell and its neighbours, in the tagger's
        // colour: the tag reads as paint on the ground.
        assert_eq!(world.paint.len(), 9);
        for x in 4..=6u16 {
            for y in 3..=5u16 {
                let cell = CellXY { x, y };
                let paint = world.paint.get(cell).expect("the cell carries paint");
                assert_eq!(paint.owner, 0, "the tagger's colour at {cell:?}");
                assert_eq!(paint.wet_q8, crate::world::paint::WET_Q8_MAX);
                assert_eq!(paint.splats, 1);
            }
        }
        // And nowhere else.
        assert!(world.paint.get(CellXY { x: 7, y: 4 }).is_none());
        assert!(world.paint.get(CellXY { x: 5, y: 6 }).is_none());
    }

    #[test]
    fn a_splat_follows_the_radius() {
        let p = Params {
            splat_radius_cells: 0,
            ..Params::default()
        };
        let mut world = flat_world(
            [CellXY { x: 4, y: 4 }, CellXY { x: 20, y: 20 }],
            vec![CellXY { x: 28, y: 28 }],
        );
        let mut agents = vec![
            agent_at(&world, 0, 0, CellXY { x: 4, y: 4 }),
            agent_at(&world, 4, 1, CellXY { x: 5, y: 4 }),
        ];
        let mut rules = Beacons::new(&world, &p);
        let order = rotated(agents.len(), 0);

        rules.step(&mut world, &mut agents, 0, &order, &p);
        assert_eq!(world.paint.len(), 1);
        assert!(world.paint.get(CellXY { x: 5, y: 4 }).is_some());
    }

    #[test]
    fn a_splat_is_clipped_at_the_edge_of_the_map() {
        let p = Params::default();
        let mut world = flat_world(
            [CellXY { x: 0, y: 0 }, CellXY { x: 20, y: 20 }],
            vec![CellXY { x: 28, y: 28 }],
        );
        // The tagger stands next to the corner, and the tagged agent stands
        // in it: the splat is cut off at the edge of the map.
        let mut agents = vec![
            agent_at(&world, 0, 0, CellXY { x: 1, y: 0 }),
            agent_at(&world, 4, 1, CellXY { x: 0, y: 0 }),
        ];
        let mut rules = Beacons::new(&world, &p);
        let order = rotated(agents.len(), 0);

        rules.step(&mut world, &mut agents, 0, &order, &p);
        // The corner of the map: the splat is cut off at the edge.
        assert_eq!(world.paint.len(), 4);
        for cell in [
            CellXY { x: 0, y: 0 },
            CellXY { x: 1, y: 0 },
            CellXY { x: 0, y: 1 },
            CellXY { x: 1, y: 1 },
        ] {
            assert!(world.paint.get(cell).is_some(), "no paint at {cell:?}");
        }
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

        let events = rules.step(&mut world, &mut agents, 0, &order, &p);
        assert!(taggers(&events).is_empty(), "{events:?}");
        assert_eq!(agents[1].stunned_until, None);
    }

    #[test]
    fn rotation_decides_mutual_tags_fairly() {
        let p = Params {
            stun_ticks: 1,
            ..Params::default()
        };
        let mut world = flat_world(
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
            let events = rules.step(&mut world, &mut agents, tick, &order, &p);
            let expected = if tick % 2 == 0 { 0 } else { 4 };
            assert_eq!(taggers(&events), vec![expected], "tick {tick}: {events:?}");
        }
    }

    #[test]
    fn contested_beacon_scores_nobody() {
        let p = Params::default();
        let mut world = flat_world(
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

        let events = rules.step(&mut world, &mut agents, 0, &order, &p);
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
        let mut world = flat_world(
            [CellXY { x: 4, y: 4 }, CellXY { x: 20, y: 20 }],
            vec![CellXY { x: 10, y: 4 }],
        );
        let mut agents = vec![agent_at(&world, 0, 0, CellXY { x: 9, y: 4 })];
        let mut rules = Beacons::new(&world, &p);
        let order = rotated(agents.len(), 0);

        for tick in 0..10u64 {
            let events = rules.step(&mut world, &mut agents, tick, &order, &p);
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
        let mut world = flat_world(
            [CellXY { x: 4, y: 4 }, CellXY { x: 20, y: 20 }],
            vec![CellXY { x: 28, y: 28 }],
        );
        let mut agents = vec![agent_at(&world, 0, 0, CellXY { x: 4, y: 4 })];
        let mut rules = Beacons::new(&world, &p);
        let order = rotated(agents.len(), 0);

        for tick in 0..5u64 {
            let events = rules.step(&mut world, &mut agents, tick, &order, &p);
            assert!(
                !events.iter().any(|e| matches!(e, Event::RoundOver(_))),
                "tick {tick}: {events:?}"
            );
        }
        let events = rules.step(&mut world, &mut agents, 5, &order, &p);
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
        let mut world = flat_world(
            [CellXY { x: 4, y: 4 }, CellXY { x: 5, y: 4 }],
            vec![CellXY { x: 28, y: 28 }],
        );
        let mut agents = vec![
            agent_at(&world, 0, 0, CellXY { x: 4, y: 4 }),
            agent_at(&world, 4, 1, CellXY { x: 5, y: 4 }),
        ];
        let mut rules = Beacons::new(&world, &p);
        let order = rotated(agents.len(), 0);

        let events = rules.step(&mut world, &mut agents, 0, &order, &p);
        assert_eq!(taggers(&events), vec![0], "{events:?}");
    }

    #[test]
    fn beacon_is_lost_when_the_holder_leaves() {
        let p = Params::default();
        let mut world = flat_world(
            [CellXY { x: 4, y: 4 }, CellXY { x: 20, y: 20 }],
            vec![CellXY { x: 10, y: 4 }],
        );
        let mut agents = vec![
            agent_at(&world, 0, 0, CellXY { x: 9, y: 4 }),
            agent_at(&world, 4, 1, CellXY { x: 20, y: 20 }),
        ];
        let mut rules = Beacons::new(&world, &p);
        let order = rotated(agents.len(), 0);

        let events = rules.step(&mut world, &mut agents, 0, &order, &p);
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
        let events = rules.step(&mut world, &mut agents, 1, &order, &p);
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
        let mut world = flat_world(
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
            rules.step(&mut world, &mut agents, tick, &order, &p);
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
        let mut world = flat_world(
            [CellXY { x: 4, y: 4 }, CellXY { x: 20, y: 20 }],
            vec![CellXY { x: 28, y: 28 }],
        );
        let mut agents = vec![agent_at(&world, 0, 0, CellXY { x: 4, y: 4 })];
        let mut rules = Beacons::new(&world, &p);
        let order = rotated(agents.len(), 0);

        rules.step(&mut world, &mut agents, 2, &order, &p);
        assert!(rules.outcome().is_some());
        // After the round is over the rules are inert.
        let events = rules.step(&mut world, &mut agents, 3, &order, &p);
        assert!(events.is_empty(), "{events:?}");
    }

    #[test]
    fn rules_hash_is_stable_and_sensitive() {
        let p = Params::default();
        let mut world = flat_world(
            [CellXY { x: 4, y: 4 }, CellXY { x: 20, y: 20 }],
            vec![CellXY { x: 10, y: 4 }],
        );
        let mut agents = vec![agent_at(&world, 0, 0, CellXY { x: 9, y: 4 })];
        let mut rules = Beacons::new(&world, &p);
        let before = rules.state_hash();
        assert_eq!(before, Beacons::new(&world, &p).state_hash());

        let order = rotated(agents.len(), 0);
        rules.step(&mut world, &mut agents, 0, &order, &p);
        assert_ne!(before, rules.state_hash());
    }

    /// A world with one painted cell: squad 1's paint on `paint_cell`.
    fn painted_world(paint_cell: CellXY) -> World {
        let mut world = flat_world(
            [CellXY { x: 4, y: 4 }, CellXY { x: 20, y: 20 }],
            vec![CellXY { x: 28, y: 28 }],
        );
        world.paint.splat(paint_cell, 1);
        world
    }

    #[test]
    fn the_other_teams_paint_slows_and_own_paint_does_not() {
        let p = Params {
            slow_q8: 128,
            ..Params::default()
        };
        let mut world = painted_world(CellXY { x: 5, y: 4 });
        // Squad 1's own paint on the cell its agent stands in.
        world.paint.splat(CellXY { x: 20, y: 20 }, 1);
        let mut agents = vec![
            // In the other team's paint.
            agent_at(&world, 0, 0, CellXY { x: 5, y: 4 }),
            // In its own team's paint.
            agent_at(&world, 4, 1, CellXY { x: 20, y: 20 }),
            // On bare ground.
            agent_at(&world, 1, 0, CellXY { x: 2, y: 2 }),
        ];
        let mut rules = Beacons::new(&world, &p);
        let order = rotated(agents.len(), 0);

        rules.step(&mut world, &mut agents, 0, &order, &p);

        assert_eq!(agents[0].paint_slow_q8, 128, "the enemy's paint slows");
        assert_eq!(agents[1].paint_slow_q8, 0, "own paint does not");
        assert_eq!(agents[2].paint_slow_q8, 0, "bare ground does not");
    }

    #[test]
    fn the_contact_count_rises_while_the_paint_is_under_the_agent_and_falls_after() {
        let p = Params {
            slow_q8: 64,
            ..Params::default()
        };
        let mut world = painted_world(CellXY { x: 5, y: 4 });
        let mut agents = vec![agent_at(&world, 0, 0, CellXY { x: 5, y: 4 })];
        let mut rules = Beacons::new(&world, &p);
        let order = rotated(agents.len(), 0);

        rules.step(&mut world, &mut agents, 0, &order, &p);
        assert_eq!(agents[0].paint_contact, 1);
        rules.step(&mut world, &mut agents, 1, &order, &p);
        assert_eq!(agents[0].paint_contact, 2);

        // The agent steps out of the paint: the count is gone.
        agents[0].pos = world.grid.center(CellXY { x: 2, y: 2 }, world.cell_units);
        rules.step(&mut world, &mut agents, 2, &order, &p);
        assert_eq!(
            agents[0].paint_contact, 0,
            "the count restarts off the paint"
        );
        assert_eq!(agents[0].paint_slow_q8, 0, "and the slow is off again");
    }

    #[test]
    fn the_paint_pushes_the_agent_back_on_every_nth_tick_of_contact() {
        // `bounce_q8 = 64` is a push-back every fourth tick of contact.
        let p = Params {
            slow_q8: 64,
            bounce_q8: 64,
            ..Params::default()
        };
        let mut world = painted_world(CellXY { x: 5, y: 4 });
        let mut agents = vec![agent_at(&world, 0, 0, CellXY { x: 5, y: 4 })];
        // The agent came from the cell next door.
        agents[0].paint_from = Some(CellXY { x: 4, y: 4 });
        let mut rules = Beacons::new(&world, &p);
        let order = rotated(agents.len(), 0);

        for tick in 0..3u64 {
            rules.step(&mut world, &mut agents, tick, &order, &p);
            assert_eq!(
                agents[0].pos,
                world.grid.center(CellXY { x: 5, y: 4 }, world.cell_units),
                "tick {tick}: the paint waits for the fourth tick of contact"
            );
        }
        rules.step(&mut world, &mut agents, 3, &order, &p);
        assert_eq!(
            agents[0].pos,
            world.grid.center(CellXY { x: 4, y: 4 }, world.cell_units),
            "the fourth tick of contact pushes the agent back"
        );
    }

    #[test]
    fn a_paint_without_a_bounce_never_pushes_anybody_back() {
        let p = Params {
            slow_q8: 64,
            bounce_q8: 0,
            ..Params::default()
        };
        let mut world = painted_world(CellXY { x: 5, y: 4 });
        let mut agents = vec![agent_at(&world, 0, 0, CellXY { x: 5, y: 4 })];
        agents[0].paint_from = Some(CellXY { x: 4, y: 4 });
        let mut rules = Beacons::new(&world, &p);
        let order = rotated(agents.len(), 0);

        for tick in 0..8u64 {
            rules.step(&mut world, &mut agents, tick, &order, &p);
            assert_eq!(
                agents[0].pos,
                world.grid.center(CellXY { x: 5, y: 4 }, world.cell_units),
                "tick {tick}: the paint never pushes"
            );
        }
    }

    #[test]
    fn the_paint_rules_are_off_until_a_knob_is_turned() {
        // With both knobs at zero the rules are inert: no count, no slow, and
        // an agent that never moved is never pushed.
        let p = Params::default();
        let mut world = painted_world(CellXY { x: 5, y: 4 });
        let mut agents = vec![agent_at(&world, 0, 0, CellXY { x: 5, y: 4 })];
        let mut rules = Beacons::new(&world, &p);
        let order = rotated(agents.len(), 0);

        rules.step(&mut world, &mut agents, 0, &order, &p);
        assert_eq!(agents[0].paint_slow_q8, 0);
        assert_eq!(agents[0].paint_contact, 0);
    }
}
