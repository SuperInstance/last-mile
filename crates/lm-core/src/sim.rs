//! The simulation: the tick loop, the canonical state hash and the replay
//! contract.
//!
//! A [`Sim`] owns everything a round needs to run: the generated world, the
//! agents, what each squad knows, the ruleset and the reflex runtime. The
//! host drives it one tick at a time with [`Sim::step`], handing in the
//! decisions that arrived for that tick.
//!
//! ## The tick
//!
//! Every tick runs the same five phases, in this order. The order is the
//! replay contract: a round replayed from the same header and the same
//! stamped decisions produces the same bytes.
//!
//! 1. **Decisions.** The stamped decisions for this tick are applied: an
//!    action is pinned or released, an intent is committed, a lane is
//!    conditioned and a roll is sampled. A decision for a stunned agent
//!    still applies; its effect starts when the stun ends.
//! 2. **Knowledge.** Every `view_update_ticks` ticks, each squad refreshes
//!    what it knows from its own eyes.
//! 3. **The agents.** The agents act in the tick's rotating order, so no
//!    agent always goes first. Each agent turns its lane or roll command
//!    into a motor command, or runs its reflex for `script_fuel` of script
//!    fuel, and then walks.
//! 4. **The rules.** The ruleset scores the tick and reports its events.
//! 5. **The hash.** Every `hash_interval_ticks` ticks the canonical state
//!    hash is taken, and the tick counter moves on.
//!
//! Nothing in the sim reads the clock, the environment or a random source
//! that is not derived from the round seed, so two machines running the same
//! round agree on every hash.

use crate::agent::{Agent, MotorCmd};
use crate::decision::{Kind, Lane, Roll, Stamped, Tick};
use crate::fixed::Pos;
use crate::hash::{Fnv64, StateHash};
use crate::header::{Header, Role, RosterEntry};
use crate::lane::LaneConditioner;
use crate::params::Params;
use crate::rng::{Domain, Rng};
use crate::rules::{Event, Outcome, Ruleset};
use crate::script::{ReflexOut, RhaiRuntime, ScriptRuntime};
use crate::view::{menu, option_cmd, AgentSnap, BeaconSnap, ScoreView, SquadKnowledge};
use crate::world::{CellXY, World};
use std::collections::BTreeMap;

pub use crate::rules::SimError;

/// What one tick of the sim produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepOut {
    /// The tick that was just simulated.
    pub tick: Tick,
    /// What happened on it: the markers of the decisions, then the events of
    /// the ruleset.
    pub events: Vec<Event>,
    /// The canonical state hash, on the ticks that take one.
    pub hash: Option<u64>,
}

/// How the reflexes of the round have been coping.
///
/// A reflex that runs out of fuel, fails or refuses leaves the agent without
/// a command, so the counts are how a host sees a broken script.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SimStats {
    /// Reflex calls that ran out of script fuel.
    pub reflex_out_of_fuel: u64,
    /// Reflex calls that failed.
    pub reflex_error: u64,
    /// Reflex calls that refused to answer.
    pub reflex_refused: u64,
}

/// The cell offsets, relative to the spawn, the agents of a squad start on.
///
/// The first agent of a squad starts on the spawn cell and the others two
/// cells out around it, so a squad does not start stacked on a single cell
/// and has room to spread out on the way in.
const SPAWN_OFFSETS: [(i32, i32); 4] = [(0, 0), (2, 0), (0, 2), (2, 2)];

/// A round in flight.
pub struct Sim {
    /// The seed every random stream of the round is derived from.
    seed: u64,
    /// Every tunable of the round.
    params: Params,
    /// The world the round is played on.
    world: World,
    /// The roster, in agent-id order.
    agents: Vec<Agent>,
    /// What each squad knows, in squad order.
    knowledge: Vec<SquadKnowledge>,
    /// The rules that score the round.
    ruleset: Box<dyn Ruleset>,
    /// The reflex scripts of the round, by name.
    runtime: RhaiRuntime,
    /// The reflex script of every agent, in agent-id order.
    reflex_names: Vec<String>,
    /// The tick the sim is about to simulate.
    tick: Tick,
    /// How the reflexes have been coping.
    stats: SimStats,
}

impl Sim {
    /// Start a round from its header.
    ///
    /// The header pins the round: the parameters are validated, the world is
    /// generated from the seed, the agents are placed on their squad's
    /// spawn, the reflex scripts are loaded and the ruleset is built. A
    /// header that names an unknown ruleset, breaks a parameter or carries
    /// a script that does not compile is refused here, before any tick
    /// runs.
    pub fn new(h: &Header) -> Result<Sim, SimError> {
        h.params.validate().map_err(SimError::Params)?;

        let world = crate::world::generate(h.seed, &h.params);

        let mut runtime = RhaiRuntime::new();
        for (name, src) in &h.scripts {
            runtime.load(name, src).map_err(SimError::Script)?;
        }

        let ruleset = crate::rules::by_name(&h.ruleset, &world, &h.params)?;

        // The roster is the truth about who plays; it is kept in agent-id
        // order, so every hash of the round walks the agents in one fixed
        // order.
        let mut roster: Vec<&RosterEntry> = h.roster.iter().collect();
        roster.sort_by_key(|entry| entry.agent);

        let mut agents = Vec::with_capacity(roster.len());
        let mut reflex_names = Vec::with_capacity(roster.len());
        let mut squad_index: BTreeMap<u8, usize> = BTreeMap::new();
        for entry in roster {
            let index = squad_index.entry(entry.squad).or_insert(0);
            let pos = spawn_pos(&world, entry.squad, *index);
            *index += 1;
            agents.push(Agent::new(entry.agent, entry.squad, entry.role, pos));
            reflex_names.push(entry.reflex.clone());
        }

        // One knowledge per squad: the squads of the roster, and the squads
        // the parameters ask for, whichever there are more of.
        let squads = usize::from(h.params.squads).max(
            agents
                .iter()
                .map(|a| usize::from(a.squad) + 1)
                .max()
                .unwrap_or(0),
        );
        let knowledge = (0..squads)
            .map(|_| SquadKnowledge::new(&world.grid))
            .collect();

        Ok(Sim {
            seed: h.seed,
            params: h.params,
            world,
            agents,
            knowledge,
            ruleset,
            runtime,
            reflex_names,
            tick: h.start_tick,
            stats: SimStats::default(),
        })
    }

    /// The tick the sim is about to simulate.
    pub fn tick(&self) -> Tick {
        self.tick
    }

    /// The world the round is played on.
    pub fn world(&self) -> &World {
        &self.world
    }

    /// The agent with that id, if the round has one.
    pub fn agent(&self, agent: u16) -> Option<&Agent> {
        self.agents.iter().find(|a| a.id == agent)
    }

    /// The ids of every agent of the round, in agent-id order.
    pub fn agent_ids(&self) -> Vec<u16> {
        self.agents.iter().map(|a| a.id).collect()
    }

    /// The score of every squad, in squad order.
    pub fn scores(&self) -> &[i64] {
        self.ruleset.scores()
    }

    /// How the reflexes of the round have been coping.
    pub fn stats(&self) -> SimStats {
        self.stats
    }

    /// How the round ended, or `None` while it is still running.
    pub fn outcome(&self) -> Option<&Outcome> {
        self.ruleset.outcome()
    }

    /// The world as the agent's squad knows it on the current tick.
    ///
    /// An agent that is not in the round gets an empty view of it: the
    /// beacons and the scores, but no squad and no enemies.
    pub fn view(&self, agent: u16) -> ScoreView {
        match self.index_of(agent) {
            Some(index) => self.view_of(index, self.tick),
            None => self.empty_view(agent),
        }
    }

    /// The canonical state hash of the round.
    ///
    /// It covers the tick, the world, every agent in agent-id order, what
    /// each squad knows and the ruleset. Two rounds that agree on it have
    /// played the same game.
    pub fn state_hash(&self) -> u64 {
        let mut h = Fnv64::new();
        h.u64(self.tick);
        self.world.hash_into(&mut h);
        h.u32(self.agents.len() as u32);
        for agent in &self.agents {
            agent.hash_into(&mut h);
        }
        h.u32(self.knowledge.len() as u32);
        for knowledge in &self.knowledge {
            knowledge.hash_into(&mut h);
        }
        self.ruleset.hash_into(&mut h);
        h.finish()
    }

    /// Simulate one tick.
    ///
    /// `decisions` are the decisions stamped for this tick. The tick runs
    /// the five phases described at the top of this module, and reports what
    /// it produced.
    pub fn step(&mut self, decisions: &[Stamped]) -> StepOut {
        let now = self.tick;
        let mut events = Vec::new();

        // 1. The decisions of this tick.
        self.apply_decisions(decisions, now, &mut events);

        // 2. What the squads know.
        if now.is_multiple_of(u64::from(self.params.view_update_ticks)) {
            self.update_knowledge(now);
        }

        // 3. The agents, in the tick's rotating order.
        let order = self.order(now);
        for index in &order {
            let view = self.view_of(*index, now);
            let cmd = match self.agents[*index].lane_cmd {
                Some(option) => Some(option_cmd(&option, &view, &self.world)),
                None => self.reflex_cmd(*index, &view),
            };
            let agent = &mut self.agents[*index];
            let cmd = agent.choose_cmd(now, cmd);
            agent.move_step(cmd, &self.world, &self.params, now);
        }

        // 4. The rules.
        let rule_events =
            self.ruleset
                .step(&self.world, &mut self.agents, now, &order, &self.params);
        events.extend(rule_events);

        // 5. The hash of the tick, and the next tick.
        let hash = if (now + 1).is_multiple_of(u64::from(self.params.hash_interval_ticks)) {
            Some(self.state_hash())
        } else {
            None
        };
        self.tick = now + 1;

        StepOut {
            tick: now,
            events,
            hash,
        }
    }

    /// The order the agents act in on this tick.
    ///
    /// The order rotates by one place every tick, so no agent always goes
    /// first and no agent is always last.
    fn order(&self, now: Tick) -> Vec<usize> {
        let n = self.agents.len();
        if n == 0 {
            return Vec::new();
        }
        let n = u64::try_from(n).unwrap_or(0);
        (0..n)
            .map(|slot| usize::try_from((slot + now % n) % n).unwrap_or(0))
            .collect()
    }

    /// Apply the decisions stamped for this tick.
    fn apply_decisions(&mut self, decisions: &[Stamped], now: Tick, events: &mut Vec<Event>) {
        for stamped in decisions {
            let Some(agent) = stamped.d.source.agent() else {
                // A decision of the system speaks for no agent; only a
                // marker means anything in the world.
                if let Kind::Marker(marker) = stamped.d.kind {
                    events.push(Event::Marker(marker));
                }
                continue;
            };
            let Some(index) = self.index_of(agent) else {
                continue;
            };
            match &stamped.d.kind {
                Kind::ActionOn(action) => {
                    self.agents[index].held = Some(action.clone());
                }
                Kind::ActionOff => {
                    self.agents[index].held = None;
                }
                Kind::Intent(intent) => {
                    self.agents[index].set_intent(intent, now);
                }
                Kind::Lane(lane) => self.apply_lane(index, lane, now),
                Kind::Roll(roll) => self.apply_roll(index, roll, stamped.d.seq, now),
                Kind::Marker(marker) => events.push(Event::Marker(*marker)),
            }
        }
    }

    /// Condition a lane and store the option it selects.
    ///
    /// The lane is conditioned for its gate and selects one option of the
    /// menu. The selection becomes the agent's lane command and releases
    /// any pinned action, so the lanes and the actions never fight.
    fn apply_lane(&mut self, index: usize, lane: &Lane, now: Tick) {
        let menu = menu(&self.view_of(index, now));
        let alpha_shift = self.params.lane_alpha_shift;
        let deadband_q16 = self.params.lane_deadband_q16;
        let selected = self.agents[index]
            .lanes
            .entry(lane.gate)
            .or_insert_with(|| LaneConditioner::new(alpha_shift, deadband_q16))
            .update(lane.menu_id, &lane.values);
        let Some(selected) = selected else {
            return;
        };
        let Some(option) = menu.options.get(usize::from(selected)).copied() else {
            return;
        };
        self.agents[index].held = None;
        self.agents[index].lane_cmd = Some(option);
    }

    /// Sample a roll and store the option it picks.
    ///
    /// The roll draws from the probabilities of the menu, in proportion,
    /// from the stream of the round seed and the agent and the sequence
    /// number of the decision, so the same roll always picks the same
    /// option.
    fn apply_roll(&mut self, index: usize, roll: &Roll, seq: u32, now: Tick) {
        let menu = menu(&self.view_of(index, now));
        // The probabilities name the options of the menu, one for one. A
        // roll that does not fit the menu it names is refused: the motor
        // never guesses (D26).
        if roll.p.len() != menu.options.len() {
            return;
        }
        let total: u64 = roll.p.iter().map(|&p| u64::from(p)).sum();
        if total == 0 {
            return;
        }
        let id = (u64::from(self.agents[index].id) << 32) | u64::from(seq);
        let mut rng = Rng::stream(self.seed, Domain::Roll, id);
        let draw = ((rng.next_u64() >> 32) * total) >> 32;
        let mut acc = 0u64;
        let mut chosen = None;
        for (option, &p) in roll.p.iter().enumerate() {
            acc += u64::from(p);
            if draw < acc {
                chosen = Some(option);
                break;
            }
        }
        let Some(chosen) = chosen else {
            return;
        };
        let Some(option) = menu.options.get(chosen).copied() else {
            return;
        };
        self.agents[index].held = None;
        self.agents[index].lane_cmd = Some(option);
    }

    /// Refresh what every squad knows.
    fn update_knowledge(&mut self, now: Tick) {
        for (index, knowledge) in self.knowledge.iter_mut().enumerate() {
            let squad = u8::try_from(index).unwrap_or(u8::MAX);
            knowledge.update(&self.world, &self.agents, squad, now, &self.params);
        }
    }

    /// Run the reflex of an agent and count what went wrong.
    fn reflex_cmd(&mut self, index: usize, view: &ScoreView) -> Option<MotorCmd> {
        let name = &self.reflex_names[index];
        let fuel = self.params.script_fuel;
        match self.runtime.reflex(name, view, fuel) {
            ReflexOut::Cmd(cmd) => Some(cmd),
            ReflexOut::Nothing => None,
            ReflexOut::OutOfFuel => {
                self.stats.reflex_out_of_fuel += 1;
                None
            }
            ReflexOut::Error(_) => {
                self.stats.reflex_error += 1;
                None
            }
            ReflexOut::Refused(_) => {
                self.stats.reflex_refused += 1;
                None
            }
        }
    }

    /// The view of the round as the agent's squad knows it.
    fn view_of(&self, index: usize, now: Tick) -> ScoreView {
        let agent = &self.agents[index];
        let knowledge = self
            .knowledge
            .get(usize::from(agent.squad))
            .unwrap_or(&self.knowledge[0]);
        ScoreView::for_agent(
            agent,
            &self.agents,
            &self.world,
            knowledge,
            self.ruleset.scores(),
            self.ruleset.beacon_holders(),
            now,
        )
    }

    /// The view of an agent that is not in the round.
    fn empty_view(&self, agent: u16) -> ScoreView {
        let holders = self.ruleset.beacon_holders();
        ScoreView {
            tick: self.tick,
            me: AgentSnap {
                id: agent,
                cell: CellXY { x: 0, y: 0 },
                role: Role::Member,
                stunned: false,
            },
            squad: Vec::new(),
            enemies: Vec::new(),
            beacons: self
                .world
                .beacons
                .iter()
                .enumerate()
                .map(|(i, &cell)| BeaconSnap {
                    cell,
                    holder: holders.get(i).copied().flatten(),
                })
                .collect(),
            score: self.ruleset.scores().to_vec(),
            squad_id: 0,
        }
    }

    /// The index of the agent with that id.
    fn index_of(&self, agent: u16) -> Option<usize> {
        self.agents.iter().position(|a| a.id == agent)
    }
}

/// Where the agent with index `index` in a squad starts the round.
///
/// The agents of a squad start on their spawn cell and on the cells around
/// it. The paired squad — the odd squad ids, which worldgen mirrors too —
/// starts mirrored as well, so the whole roster is point-symmetric like the
/// map it plays on. An offset cell that is not walkable falls back to the
/// spawn cell, which worldgen always leaves clear.
fn spawn_pos(world: &World, squad: u8, index: usize) -> Pos {
    let Some(&spawn) = world.spawns.get(usize::from(squad)) else {
        return Pos::zero();
    };
    let max_x = i32::from(world.grid.w).saturating_sub(1);
    let max_y = i32::from(world.grid.h).saturating_sub(1);
    if max_x < 0 || max_y < 0 {
        return Pos::zero();
    }
    let (dx, dy) = SPAWN_OFFSETS[index % SPAWN_OFFSETS.len()];
    let (dx, dy) = if squad.is_multiple_of(2) {
        (dx, dy)
    } else {
        (-dx, -dy)
    };
    let candidate = CellXY {
        x: u16::try_from((i32::from(spawn.x) + dx).clamp(0, max_x)).unwrap_or(spawn.x),
        y: u16::try_from((i32::from(spawn.y) + dy).clamp(0, max_y)).unwrap_or(spawn.y),
    };
    if world.grid.walkable(candidate) {
        world.grid.center(candidate, world.cell_units)
    } else {
        world.grid.center(spawn, world.cell_units)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision::{Action, Decision, Provenance, Source};
    use crate::header::Header;
    use crate::view::MenuOption;

    /// The default reflex, the one every agent of a standard round runs.
    const DEFAULT_REFLEX: &str = include_str!("../../../assets/reflexes/default.rhai");

    /// The hashes a round of `ticks` ticks produces.
    fn run(h: &Header, ticks: u64) -> Vec<u64> {
        let mut sim = Sim::new(h).expect("the round starts");
        let mut hashes = Vec::new();
        for _ in 0..ticks {
            let out = sim.step(&[]);
            if let Some(hash) = out.hash {
                hashes.push(hash);
            }
        }
        hashes
    }

    /// A decision of an agent, stamped for `tick`.
    fn stamped(agent: u16, tick: Tick, seq: u32, kind: Kind) -> Stamped {
        Stamped {
            d: Decision {
                apply_tick: tick,
                source: Source::Agent(agent),
                seq,
                kind,
                provenance: Provenance::Test,
            },
            arrived: tick,
            tick,
            late_by: 0,
        }
    }

    /// The distance between two cells, in cells.
    fn chebyshev(a: CellXY, b: CellXY) -> u32 {
        u32::from(a.x.abs_diff(b.x)).max(u32::from(a.y.abs_diff(b.y)))
    }

    #[test]
    fn rounds_are_deterministic() {
        let h = Header::default_round(7, DEFAULT_REFLEX);
        assert_eq!(run(&h, 600), run(&h, 600));
    }

    #[test]
    fn seeds_diverge() {
        let a = Header::default_round(1, DEFAULT_REFLEX);
        let b = Header::default_round(2, DEFAULT_REFLEX);
        assert_ne!(run(&a, 600), run(&b, 600));
    }

    /// A round of the shipped reflex has play: the squads reach the beacons,
    /// hold them and score.
    ///
    /// The shipped reflex breaks contact — it steps away from an enemy
    /// within 6 cells, and a tag needs contact within one cell (2 m) — so
    /// two squads running it never trade tags. They fight over the beacons
    /// instead, and that is what this checks. The tagging itself is covered
    /// by [`contact_is_tagged`].
    #[test]
    fn reflex_round_has_play() {
        let h = Header::default_round(1, DEFAULT_REFLEX);
        let mut sim = Sim::new(&h).expect("the round starts");
        let mut held = 0u64;
        for _ in 0..3000 {
            let out = sim.step(&[]);
            for event in &out.events {
                if matches!(event, Event::BeaconHeld { .. }) {
                    held += 1;
                }
            }
        }
        assert!(held >= 1, "no beacon held in 3000 ticks");
        let scores = sim.scores();
        assert!(scores.iter().any(|&s| s > 0), "nobody scored: {scores:?}");
    }

    /// Two agents of different squads that meet are tagged.
    ///
    /// The round is played on a single chunk, so the two agents meet
    /// quickly, and both are pinned to the same cell by a decision: the
    /// ruleset tags the one that gets there first.
    #[test]
    fn contact_is_tagged() {
        let mut h = Header::default_round(1, DEFAULT_REFLEX);
        h.params.world_chunks = (1, 1);
        h.params.beacons = 1;
        let mut sim = Sim::new(&h).expect("the round starts");
        let beacon = sim.world().beacons[0];
        let mut tagged = 0u64;
        for tick in 0..400 {
            let decisions = vec![
                stamped(0, tick, 1, Kind::ActionOn(Action::GoTo(beacon))),
                stamped(4, tick, 2, Kind::ActionOn(Action::GoTo(beacon))),
            ];
            for event in &sim.step(&decisions).events {
                if matches!(event, Event::Tagged { .. }) {
                    tagged += 1;
                }
            }
        }
        assert!(tagged >= 1, "the agents met and no tag came");
    }

    #[test]
    fn lane_decision_overrides_reflex() {
        let h = Header::default_round(9, DEFAULT_REFLEX);
        let probe = Sim::new(&h).expect("the round starts");
        let menu = menu(&probe.view(0));
        let retreat = menu
            .options
            .iter()
            .position(|o| *o == MenuOption::Retreat)
            .expect("the menu has a retreat option");
        let mut values = vec![0u8; menu.options.len()];
        values[retreat] = 255;
        let lane = Kind::Lane(Lane {
            gate: 0,
            menu_id: menu.id,
            values,
        });

        // The same round twice: once with the lane decision, once without.
        let mut with = Sim::new(&h).expect("the round starts");
        let mut without = Sim::new(&h).expect("the round starts");
        for tick in 0..60 {
            let decisions: Vec<Stamped> = if tick == 5 {
                vec![stamped(0, 5, 1, lane.clone())]
            } else {
                Vec::new()
            };
            with.step(&decisions);
            without.step(&[]);
        }

        let spawn = with.world().spawns[0];
        let retreated = chebyshev(with.view(0).me.cell, spawn);
        let hunted = chebyshev(without.view(0).me.cell, spawn);
        assert!(
            retreated < hunted,
            "the lane did not pull agent 0 back: {retreated} cells from the spawn, \
             against {hunted} without it"
        );
    }

    #[test]
    fn roll_is_replayable() {
        let h = Header::default_round(4, DEFAULT_REFLEX);
        let probe = Sim::new(&h).expect("the round starts");
        let menu = menu(&probe.view(0));
        let mut p = vec![0u16; menu.options.len()];
        p[0] = 60_000;
        p[1] = 5_535;
        let roll = Kind::Roll(Roll {
            menu_id: menu.id,
            p,
        });

        let mut a = Sim::new(&h).expect("the round starts");
        let mut b = Sim::new(&h).expect("the round starts");
        let mut hashes_a = Vec::new();
        let mut hashes_b = Vec::new();
        for tick in 0..120 {
            let decisions: Vec<Stamped> = if tick == 3 {
                vec![stamped(1, 3, 7, roll.clone())]
            } else {
                Vec::new()
            };
            if let Some(hash) = a.step(&decisions).hash {
                hashes_a.push(hash);
            }
            if let Some(hash) = b.step(&decisions).hash {
                hashes_b.push(hash);
            }
        }

        let chosen = a.agent(1).and_then(|agent| agent.lane_cmd);
        assert_eq!(chosen, b.agent(1).and_then(|agent| agent.lane_cmd));
        assert!(chosen.is_some(), "the roll picked nothing");
        assert!(
            menu.options
                .contains(&chosen.expect("the roll picked something")),
            "the roll picked an option that is not on the menu"
        );
        assert_eq!(hashes_a, hashes_b);
    }

    #[test]
    fn hash_every_interval() {
        let h = Header::default_round(2, DEFAULT_REFLEX);
        let mut sim = Sim::new(&h).expect("the round starts");
        let mut ticks = 0u64;
        let mut hashed = 0u64;
        let mut hashed_ticks = Vec::new();
        for _ in 0..300 {
            let out = sim.step(&[]);
            ticks += 1;
            if out.hash.is_some() {
                hashed += 1;
                hashed_ticks.push(out.tick);
            }
        }
        assert_eq!(hashed, ticks / u64::from(h.params.hash_interval_ticks));
        // The first hash is taken when the interval is up, not on the first
        // tick.
        assert_eq!(hashed_ticks.first(), Some(&29));
    }
}
