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
//! A branch round — one whose header carries a lineage — applies the
//! lineage's changes between two ticks, before phase 1: the tick a change
//! names is the first that runs with it. Everything else about a branch is
//! ordinary replay, which is why a branch log replays from its own header.
//!
//! Nothing in the sim reads the clock, the environment or a random source
//! that is not derived from the round seed, so two machines running the same
//! round agree on every hash.

use crate::agent::{Agent, MotorCmd};
use crate::decision::{Kind, Lane, Roll, Source, Stamped, Tick};
use crate::fixed::Pos;
use crate::hash::{Fnv64, StateHash};
use crate::header::{Header, Lineage, LineageChange, Role, RosterEntry};
use crate::hearing;
use crate::lane::LaneConditioner;
use crate::params::{ParamError, Params};
use crate::rng::{Domain, Rng};
use crate::rules::{Event, Outcome, Ruleset};
use crate::script::{ReflexOut, RhaiRuntime, ScriptRuntime};
use crate::talk::{ListenerRecord, TalkParams, TalkRecord};
use crate::utterance::{Addressee, Channel, Heard, Subject, Utterance};
use crate::view::{menu, option_cmd, AgentSnap, BeaconSnap, ScoreView, SquadKnowledge};
use crate::world::{CellXY, World};
use std::collections::{BTreeMap, BTreeSet};

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
    /// The round's talk rules: how far a voice carries, what a wall costs.
    talk: TalkParams,
    /// The world the round is played on.
    world: World,
    /// The roster, in agent-id order.
    agents: Vec<Agent>,
    /// What each squad knows, in squad order.
    knowledge: Vec<SquadKnowledge>,
    /// What each agent has heard since the last view update, in agent-id
    /// order: the degraded copies, one list per listener.
    heard: Vec<Vec<Heard>>,
    /// The bookkeeping of the talk: every line the round heard.
    talk_records: Vec<TalkRecord>,
    /// The rules that score the round.
    ruleset: Box<dyn Ruleset>,
    /// The reflex scripts of the round, by name.
    runtime: RhaiRuntime,
    /// The reflex script of every agent, in agent-id order.
    reflex_names: Vec<String>,
    /// The tick the sim is about to simulate.
    tick: Tick,
    /// The branch changes that have not fired yet, each with the tick it
    /// fires on, in the order history plays them; empty for a round that is
    /// nobody's branch.
    branch: Vec<(Tick, LineageChange)>,
    /// The sources whose decisions the branch dropped, from the tick the
    /// drop fired on.
    dropped: BTreeSet<Source>,
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

        // A branch round carries its changes in the header's lineage. Every
        // change is checked here the way a script is — a branch that cannot
        // run stops the round before it starts — and each fires when its
        // tick begins.
        let mut branch = Vec::new();
        if let Some(lineage) = &h.lineage {
            let mut params = h.params;
            for (at, change) in flatten_lineage(lineage) {
                match &change {
                    LineageChange::Reflex { name, src } => {
                        runtime.check(name, src).map_err(SimError::Script)?;
                    }
                    LineageChange::Param { field, value } => {
                        match params.set_branchable(field, *value) {
                            Ok(true) => {}
                            Ok(false) => {
                                return Err(SimError::Params(ParamError {
                                    field: "params",
                                    reason: format!(
                                        "the field {field:?} is not branchable mid-round"
                                    ),
                                }));
                            }
                            Err(err) => return Err(SimError::Params(err)),
                        }
                    }
                    LineageChange::DropDecisions { .. } => {}
                }
                branch.push((at, change));
            }
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

        // One heard-list per agent: the copies are per listener, because
        // every listener gets its own degraded copy of a line.
        let agent_count = agents.len();

        Ok(Sim {
            seed: h.seed,
            params: h.params,
            talk: h.talk.clone(),
            world,
            agents,
            knowledge,
            heard: (0..agent_count).map(|_| Vec::new()).collect(),
            talk_records: Vec::new(),
            ruleset,
            runtime,
            reflex_names,
            tick: h.start_tick,
            branch,
            dropped: BTreeSet::new(),
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

    /// The bookkeeping of the talk: every line the round heard, in the
    /// order the lines were spoken.
    ///
    /// The records are not hashed — they are bookkeeping, like the reflex
    /// counters — so collecting them does not change the round's state
    /// hash. What the lines changed in the *knowledge* is hashed, because
    /// that is game state.
    pub fn talk_records(&self) -> &[TalkRecord] {
        &self.talk_records
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

        // The lines the agents are holding are still going, or are over:
        // the flag says which, at this tick.
        self.refresh_heard(now);

        // 0. The branch fires. A change applies between the tick before it
        //    and its own tick, so the tick it names is the first that runs
        //    with it.
        self.fire_branch(now);

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

        // 6. The window of what the agents heard. At every view update the
        //    lines that are over leave the window; the ones still being
        //    spoken stay, because a line that has only started is still
        //    news (the pre-draft needs it).
        self.refresh_heard(self.tick);
        if now.is_multiple_of(u64::from(self.params.view_update_ticks)) {
            for buffer in &mut self.heard {
                buffer.retain(|line| line.ongoing);
            }
        }

        StepOut {
            tick: now,
            events,
            hash,
        }
    }

    /// Hot-swap the reflex script `name`, between ticks.
    ///
    /// This is how a branch changes one rule of a round: `src` is compiled
    /// and checked exactly like a round-setup script, and every roster entry
    /// that names `name` runs the new source from the next tick. A source
    /// that does not parse, or defines no entry point, is refused and the
    /// previous script keeps running. The sim writes no logs; a branch
    /// records the swap itself, as a marker decision on the branch tick.
    pub fn replace_script(&mut self, name: &str, src: &str) -> Result<(), SimError> {
        self.runtime.load(name, src).map_err(SimError::Script)
    }

    /// Apply every branch change whose tick has come.
    ///
    /// Each change was checked when the round was set up, so none of them
    /// can fail here; the results are still looked at, because a failure
    /// half through a fire is a bug and must not slip past silently.
    fn fire_branch(&mut self, now: Tick) {
        while self.branch.first().is_some_and(|(at, _)| *at <= now) {
            let (_, change) = self.branch.remove(0);
            match change {
                LineageChange::Reflex { name, src } => {
                    if let Err(err) = self.replace_script(&name, &src) {
                        debug_assert!(false, "a checked branch change failed: {err}");
                    }
                }
                LineageChange::Param { field, value } => {
                    if !self.params.set_branchable(&field, value).unwrap_or(false) {
                        debug_assert!(false, "a checked branch change failed: {field:?}");
                    }
                }
                LineageChange::DropDecisions { source } => {
                    self.dropped.insert(source);
                }
            }
        }
    }

    /// The order the agents act in on this tick.
    ///
    /// The order rotates by one place every tick, so no agent always goes
    /// first and no agent is always last.
    ///
    /// The rotation starts at the round's initiative, the point of the
    /// rotation the seed picks. Without it the rotation is in the same
    /// phase on every seed at the moment the two squads first touch — the
    /// walk to the middle takes nearly the same number of ticks wherever
    /// it goes — so the same squad landed the first tag on most seeds
    /// (measured: squad 1 in 50 of 63 tagged rounds, 23 of them in phase
    /// 3 and 18 in phase 5). The first tag decides the round, because the
    /// tagged agent is stunned, walks back from its spawn and loses the
    /// contested beacon while the other squad holds it. The initiative
    /// spreads the contact over the phases, so the first tag is an even
    /// coin flip.
    fn order(&self, now: Tick) -> Vec<usize> {
        let n = self.agents.len();
        if n == 0 {
            return Vec::new();
        }
        let n = u64::try_from(n).unwrap_or(0);
        let phase = self.initiative(n);
        (0..n)
            .map(|slot| usize::try_from((slot + now % n + phase) % n).unwrap_or(0))
            .collect()
    }

    /// The point of the acting order's rotation the round starts from.
    ///
    /// The seed is mixed through the canonical hash, so the initiative
    /// spreads evenly over the phases and leans on neither the world's
    /// layout nor the low bits of the seed.
    fn initiative(&self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        let mut h = Fnv64::new();
        h.u64(self.seed);
        (h.finish() >> 32) % n
    }

    /// Apply the decisions stamped for this tick.
    fn apply_decisions(&mut self, decisions: &[Stamped], now: Tick, events: &mut Vec<Event>) {
        for stamped in decisions {
            // A branch may drop a source's decisions from its tick on: the
            // parent's stream keeps flowing, the branch stops listening.
            if self.dropped.contains(&stamped.d.source) {
                continue;
            }
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
                // A line is a score for the ear: the sim applies it as one
                // — the listeners, their clarity and their degraded copies
                // — and the log keeps the score, so the round replays
                // exactly.
                Kind::Utterance(utterance) => self.apply_utterance(index, utterance, now),
            }
        }
    }

    /// Apply an utterance: who hears it, how clearly, and what it teaches
    /// the squads.
    ///
    /// The listener set is the addressee's set — the speaker's squad for
    /// `Squad`, one agent for `Agent`, everybody in earshot for `All` —
    /// plus the **enemies** that overhear a voice or a shout in range when
    /// the round says so. The radio is the squad's channel: it reaches the
    /// whole squad, through the walls, whatever the addressee says.
    ///
    /// Each listener gets its own copy, degraded by its own clarity: the
    /// words that arrive, the tags they understood. A listener who cannot
    /// hear the line at all hears nothing — silence is an outcome.
    fn apply_utterance(&mut self, index: usize, u: &Utterance, now: Tick) {
        let speaker_squad = self.agents[index].squad;
        let speaker_pos = self.agents[index].pos;

        let mut candidates: BTreeSet<usize> = BTreeSet::new();
        if u.channel == Channel::Radio && self.talk.radio_is_squad {
            candidates.extend(
                (0..self.agents.len())
                    .filter(|&i| self.agents[i].squad == speaker_squad && i != index),
            );
        } else {
            match u.addressee {
                Addressee::Squad => candidates.extend(
                    (0..self.agents.len())
                        .filter(|&i| self.agents[i].squad == speaker_squad && i != index),
                ),
                Addressee::Agent(id) => {
                    candidates.extend(
                        (0..self.agents.len()).filter(|&i| i != index && self.agents[i].id == id),
                    );
                }
                Addressee::All => {
                    candidates.extend((0..self.agents.len()).filter(|&i| i != index));
                }
            }
        }
        // The other side of the fight hears a voice or a shout that reach
        // them.
        if self.talk.enemy_overhears_voice && matches!(u.channel, Channel::Voice | Channel::Shout) {
            candidates.extend(
                (0..self.agents.len())
                    .filter(|&i| i != index && self.agents[i].squad != speaker_squad),
            );
        }

        let mut listeners: Vec<ListenerRecord> = Vec::new();
        let mut heard_squads: BTreeSet<u8> = BTreeSet::new();
        for listener_index in candidates {
            let listener = &self.agents[listener_index];
            let clarity = hearing::clarity_between(
                u.channel,
                speaker_pos,
                listener.pos,
                &self.world.grid,
                self.world.cell_units,
                &self.talk,
            );
            if clarity < self.talk.min_clarity_q16 {
                // Nothing arrives: the listener never learns the line was
                // spoken.
                continue;
            }
            let mut copy = hearing::degrade_at(u, now, clarity, listener.id, &self.talk);
            copy.ongoing = now < copy.ended_tick;
            self.heard[listener_index].push(copy.clone());
            heard_squads.insert(listener.squad);
            listeners.push(ListenerRecord {
                listener: listener.id,
                clarity_q16: clarity,
                words_heard: u16::try_from(copy.words.len()).unwrap_or(u16::MAX),
                act_known: copy.act.is_some(),
                intent_known: copy.intent.is_some(),
            });
        }

        // What the squads know: the speaker's own squad knows what its
        // speaker said (the squad shares the speaker's knowledge), and
        // every other squad knows it only if one of its members heard the
        // line.
        if let Subject::Enemy(enemy) = u.subject {
            if let Some(target) = self.agents.iter().find(|a| a.id == enemy) {
                let cell = self.world.grid.cell_of(target.pos, self.world.cell_units);
                let mut squads = heard_squads;
                squads.insert(speaker_squad);
                for squad in squads {
                    if let Some(knowledge) = self.knowledge.get_mut(usize::from(squad)) {
                        knowledge.tell(enemy, cell, now);
                    }
                }
            }
        }

        self.talk_records.push(TalkRecord {
            id: u.state_hash(),
            speaker: u.speaker,
            at_tick: now,
            ended_tick: now + u64::from(u.duration_ticks()),
            channel: u.channel,
            act: u.act,
            intent: u.intent,
            affect: u.affect,
            subject: u.subject,
            words: u16::try_from(u.words.len()).unwrap_or(u16::MAX),
            duration_ticks: u.duration_ticks(),
            listeners,
        });
    }

    /// Refresh the `ongoing` flag of every line the agents are holding.
    fn refresh_heard(&mut self, now: Tick) {
        for buffer in &mut self.heard {
            for line in buffer.iter_mut() {
                line.ongoing = now < line.ended_tick;
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
            self.heard.get(index).cloned().unwrap_or_default(),
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
            heard: Vec::new(),
        }
    }

    /// The index of the agent with that id.
    fn index_of(&self, agent: u16) -> Option<usize> {
        self.agents.iter().position(|a| a.id == agent)
    }
}

/// Flatten a lineage chain into the changes that have to fire, in the order
/// history plays them.
///
/// A branch of a branch carries its parent's lineage in `prior`, and the
/// whole chain fires — each change on its own tick, the older fork first
/// among changes that share a tick — so a fork made before an earlier
/// branch's change still plays that change afterwards, exactly as the round
/// it forked would have.
fn flatten_lineage(lineage: &Lineage) -> Vec<(Tick, LineageChange)> {
    // Walk the chain from the newest fork to the root, then put history
    // back in order; the stable sort keeps same-tick changes in the order
    // the walk left them, oldest fork first.
    let mut flat = Vec::new();
    let mut link = Some(lineage);
    while let Some(current) = link {
        for change in &current.changes {
            flat.push((current.at, change.clone()));
        }
        link = current.prior.as_deref();
    }
    flat.reverse();
    flat.sort_by_key(|(at, _)| *at);
    flat
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
    use crate::utterance::{Affect, Intent, Prosody, SpeechAct, Word};
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

    /// A four-word order on the voice channel.
    fn utterance(speaker: u16, subject: Subject) -> Utterance {
        Utterance {
            speaker,
            channel: Channel::Voice,
            addressee: Addressee::Squad,
            act: SpeechAct::Command,
            intent: Intent::Order,
            affect: Affect::Urgent,
            subject,
            prosody: Prosody::Synthesized,
            words: vec![
                Word::new("hold", 0, 10),
                Word::new("the", 10, 20),
                Word::new("north", 20, 30),
                Word::new("beacon", 30, 40),
            ],
        }
    }

    /// Put an agent where the test needs it.
    fn place_agent(sim: &mut Sim, agent: u16, pos: Pos) {
        let index = sim
            .agents
            .iter()
            .position(|a| a.id == agent)
            .expect("the agent is in the round");
        sim.agents[index].pos = pos;
    }

    /// Clear a stretch of cells, so a test measures the distance and not
    /// the map it happens to be standing on.
    fn clear_row(sim: &mut Sim, y: u16, from: u16, to: u16) {
        for x in from..=to {
            sim.world
                .grid
                .set(CellXY { x, y }, crate::world::grid::CellKind::Floor);
        }
    }

    /// The enemy of squad 0 hears a voice that reaches it, and its squad
    /// learns the fact it was told.
    ///
    /// The enemy is 10 m away — inside the 25 m voice range — and the
    /// words arrive, degraded: the copy is the truth as it reached that
    /// ear.
    #[test]
    fn enemies_overhear_voice() {
        let h = Header::default_round(1, DEFAULT_REFLEX);
        let mut sim = Sim::new(&h).expect("the round starts");

        // A clear corridor on row 0: the speaker at the origin, the enemy
        // 10 m east.
        let speaker_pos = Pos::from_units(0, 0);
        let enemy_pos = Pos::from_units(10 * i64::from(crate::fixed::UNITS_PER_M), 0);
        clear_row(&mut sim, 0, 0, 8);
        place_agent(&mut sim, 0, speaker_pos);
        place_agent(&mut sim, 4, enemy_pos);

        // Squad 0's agent says something about the enemy it sees.
        sim.step(&[stamped(
            0,
            0,
            0,
            Kind::Utterance(utterance(0, Subject::Enemy(4))),
        )]);

        // The enemy heard it: a degraded copy, with the words.
        let view = sim.view(4);
        assert_eq!(view.heard.len(), 1, "the enemy must hear the voice");
        let heard = &view.heard[0];
        assert_eq!(heard.speaker, 0);
        assert_eq!(heard.channel, Channel::Voice);
        assert!(heard.clarity_q16 > 0 && heard.clarity_q16 < 65_536);
        assert_eq!(heard.words.len(), 4, "the words arrive: {heard:?}");
        assert_eq!(heard.act, Some(SpeechAct::Command));
        assert_eq!(heard.intent, Some(Intent::Order));

        // And its squad knows the fact it was told.
        let knowledge = &sim.knowledge[1];
        let (cell, tick) = knowledge.told.get(&4).expect("the fact was told");
        assert_eq!(
            *cell,
            sim.world.grid.cell_of(enemy_pos, sim.world.cell_units)
        );
        assert_eq!(*tick, 0);

        // The squad that was addressed knows it too: it shares its
        // speaker's knowledge.
        assert!(sim.knowledge[0].told.contains_key(&4));

        // The bookkeeping of the talk: one record, with the listener.
        let records = sim.talk_records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].speaker, 0);
        assert_eq!(records[0].words, 4);
        assert!(records[0]
            .listeners
            .iter()
            .any(|l| l.listener == 4 && l.words_heard == 4 && l.act_known));
    }

    /// The hearing physics is recomputed from the world, so it is
    /// reproducible: the same line, in the same round, is heard the same
    /// way every time — word for word, drop for drop.
    #[test]
    fn the_same_line_is_heard_the_same_way_twice() {
        /// One round, one line, and what the squad-mate heard of it.
        fn heard_once() -> Vec<Heard> {
            let h = Header::default_round(1, DEFAULT_REFLEX);
            let mut sim = Sim::new(&h).expect("the round starts");
            clear_row(&mut sim, 0, 0, 8);
            place_agent(&mut sim, 0, Pos::from_units(0, 0));
            place_agent(
                &mut sim,
                1,
                Pos::from_units(5 * i64::from(crate::fixed::UNITS_PER_M), 0),
            );
            sim.step(&[stamped(
                0,
                0,
                0,
                Kind::Utterance(utterance(0, Subject::None)),
            )]);
            sim.view(1).heard
        }

        let first = heard_once();
        assert_eq!(first.len(), 1);
        assert_eq!(first, heard_once());
    }

    /// The radio is the squad's channel: it reaches a squad-mate through
    /// the walls and over any distance, and it reaches nobody else.
    ///
    /// This is the half of the hearing physics the squads coordinate on:
    /// without it, phase 4 would play exactly like phase 1.
    #[test]
    fn the_radio_reaches_the_squad_through_the_walls() {
        let h = Header::default_round(1, DEFAULT_REFLEX);
        let mut sim = Sim::new(&h).expect("the round starts");
        clear_row(&mut sim, 0, 0, 40);
        // A wall between the speaker and the squad-mate, and 60 m of
        // distance: the voice would never get there.
        sim.world
            .grid
            .set(CellXY { x: 3, y: 0 }, crate::world::grid::CellKind::Wall);
        place_agent(&mut sim, 0, Pos::from_units(0, 0));
        place_agent(
            &mut sim,
            1,
            Pos::from_units(60 * i64::from(crate::fixed::UNITS_PER_M), 0),
        );
        place_agent(
            &mut sim,
            4,
            Pos::from_units(61 * i64::from(crate::fixed::UNITS_PER_M), 0),
        );

        let mut radio = utterance(0, Subject::None);
        radio.channel = Channel::Radio;
        sim.step(&[stamped(0, 0, 0, Kind::Utterance(radio))]);

        // The squad-mate heard it, at full clarity, through the wall.
        let view = sim.view(1);
        assert_eq!(view.heard.len(), 1, "the radio must reach the squad");
        assert_eq!(view.heard[0].clarity_q16, 65_536);
        assert_eq!(view.heard[0].words.len(), 4);

        // The enemy heard nothing: the radio is the squad's channel.
        assert!(
            sim.view(4).heard.is_empty(),
            "the radio must not reach the other squad"
        );
    }

    /// The lines an agent heard are in its view, and they leave the view
    /// when the window closes.
    ///
    /// The view shows what the agent heard **since the last view update**:
    /// a line that ended inside the window is still news, and one that
    /// ended before it is gone.
    #[test]
    fn heard_appears_in_the_score_view() {
        let h = Header::default_round(1, DEFAULT_REFLEX);
        let mut sim = Sim::new(&h).expect("the round starts");
        clear_row(&mut sim, 0, 0, 8);
        place_agent(&mut sim, 0, Pos::from_units(0, 0));
        place_agent(
            &mut sim,
            1,
            Pos::from_units(2 * i64::from(crate::fixed::UNITS_PER_M), 0),
        );

        // Tick 0: nothing is said.
        sim.step(&[]);
        // Tick 1: the agent of squad 0 speaks for four ticks.
        sim.step(&[stamped(
            0,
            1,
            0,
            Kind::Utterance(utterance(0, Subject::None)),
        )]);

        // The squad-mate heard it: the line is in the view, and it is
        // still being spoken.
        let view = sim.view(1);
        assert_eq!(view.heard.len(), 1, "the line must be in the view");
        assert_eq!(view.heard[0].started_tick, 1);
        assert_eq!(view.heard[0].ended_tick, 41);
        assert!(view.heard[0].ongoing, "the line is still being spoken");

        // The line ends at tick 41; the view window closes at the next
        // view update (every `view_update_ticks` ticks), and the line
        // leaves the view with it.
        for _ in 0..40 {
            sim.step(&[]);
        }
        assert!(
            !sim.view(1).heard.is_empty(),
            "the line is still in the window"
        );
        for _ in 0..6 {
            sim.step(&[]);
        }
        assert!(
            sim.view(1).heard.is_empty(),
            "the line must leave the window: {:?}",
            sim.view(1).heard
        );
    }

    /// The roster's starting formation is point-symmetric: squad 1's
    /// agents stand exactly at the 180°-rotated cells of squad 0's (Task
    /// 11's mirrored spawn offsets), so the round starts as symmetric as
    /// the world it plays on.
    #[test]
    fn spawn_formation_is_point_symmetric() {
        for seed in [1u64, 5, 42] {
            let h = Header::default_round(seed, DEFAULT_REFLEX);
            let sim = Sim::new(&h).expect("the round starts");
            let world = sim.world();
            let cell_of = |id: u16| {
                let a = sim.agent(id).expect("roster id");
                world.grid.cell_of(a.pos, world.cell_units)
            };
            let squad0: Vec<CellXY> = (0..4u16).map(cell_of).collect();
            let squad1: Vec<CellXY> = (4..8u16).map(cell_of).collect();
            let mut rotated: Vec<CellXY> = squad0
                .iter()
                .copied()
                .map(|c| world.grid.mirror(c))
                .collect();
            rotated.sort();
            let mut own = squad1;
            own.sort();
            assert_eq!(
                own, rotated,
                "seed {seed}: squad 1's formation must mirror squad 0's"
            );
        }
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
        let mut tagged = 0u64;
        for _ in 0..3000 {
            let out = sim.step(&[]);
            for event in &out.events {
                match event {
                    Event::BeaconHeld { .. } => held += 1,
                    Event::Tagged { .. } => tagged += 1,
                    _ => {}
                }
            }
        }
        assert!(held >= 1, "no beacon held in 3000 ticks");
        // Restored from the plan: squads must make contact. The first reflex
        // fled every enemy, so this was 0; the retuned one charges with numbers.
        assert!(tagged >= 1, "no tag in 3000 ticks");
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
        h.params.beacons = 2;
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

    /// A lineage that swaps the default reflex for one that always holds.
    fn hold_lineage(at: Tick) -> Lineage {
        Lineage {
            parent_header: 0,
            parent_head: 0,
            at,
            changes: vec![LineageChange::Reflex {
                name: "default".to_owned(),
                src: "fn reflex(v){ #{ hold: true } }".to_owned(),
            }],
            prior: None,
        }
    }

    #[test]
    fn replace_script_swaps_the_reflex_mid_round() {
        let h = Header::default_round(1, DEFAULT_REFLEX);
        let mut swapped = Sim::new(&h).expect("the round starts");
        let mut plain = Sim::new(&h).expect("the round starts");
        for _ in 0..60 {
            swapped.step(&[]);
            plain.step(&[]);
        }
        swapped
            .replace_script("default", "fn reflex(v){ #{ hold: true } }")
            .expect("the swap loads");

        let (mut a, mut b) = (Vec::new(), Vec::new());
        for _ in 0..120 {
            if let Some(hash) = swapped.step(&[]).hash {
                a.push(hash);
            }
            if let Some(hash) = plain.step(&[]).hash {
                b.push(hash);
            }
        }
        assert_ne!(a, b, "the swap changed nothing");
    }

    #[test]
    fn replace_script_refuses_a_broken_script_and_keeps_the_old_one() {
        let h = Header::default_round(1, DEFAULT_REFLEX);
        let mut swapped = Sim::new(&h).expect("the round starts");
        let mut plain = Sim::new(&h).expect("the round starts");
        swapped.step(&[]);
        plain.step(&[]);
        assert!(swapped.replace_script("default", "fn (").is_err());

        let (mut a, mut b) = (Vec::new(), Vec::new());
        for _ in 0..90 {
            if let Some(hash) = swapped.step(&[]).hash {
                a.push(hash);
            }
            if let Some(hash) = plain.step(&[]).hash {
                b.push(hash);
            }
        }
        assert_eq!(a, b, "the refused swap changed the round");
    }

    #[test]
    fn lineage_fires_on_its_tick_and_not_before() {
        let mut h = Header::default_round(1, DEFAULT_REFLEX);
        h.lineage = Some(hold_lineage(60));
        let mut branched = Sim::new(&h).expect("the branch starts");
        let mut plain =
            Sim::new(&Header::default_round(1, DEFAULT_REFLEX)).expect("the round starts");

        let (mut a, mut b) = (Vec::new(), Vec::new());
        for tick in 0..150u64 {
            if let Some(hash) = branched.step(&[]).hash {
                a.push(hash);
            }
            if let Some(hash) = plain.step(&[]).hash {
                b.push(hash);
            }
            if tick < 60 {
                assert_eq!(a, b, "the branch changed something before its tick");
            }
        }
        assert_ne!(a, b, "the branch never fired");
    }

    #[test]
    fn dropped_sources_stop_applying_from_the_branch_tick() {
        let mut h = Header::default_round(1, DEFAULT_REFLEX);
        h.lineage = Some(Lineage {
            parent_header: 0,
            parent_head: 0,
            at: 60,
            changes: vec![LineageChange::DropDecisions {
                source: Source::Agent(0),
            }],
            prior: None,
        });
        let mut dropped = Sim::new(&h).expect("the branch starts");
        let mut plain =
            Sim::new(&Header::default_round(1, DEFAULT_REFLEX)).expect("the round starts");

        // Agent 0 is pinned to two far-apart cells, alternating: the pin it
        // holds when the drop fires is the pin it keeps.
        let near = CellXY { x: 10, y: 10 };
        let far = CellXY { x: 100, y: 100 };
        let (mut a, mut b) = (Vec::new(), Vec::new());
        for tick in 0..150u64 {
            let target = if tick % 20 < 10 { near } else { far };
            let decisions = vec![stamped(
                0,
                tick,
                u32::try_from(tick).unwrap() + 1,
                Kind::ActionOn(Action::GoTo(target)),
            )];
            if let Some(hash) = dropped.step(&decisions).hash {
                a.push(hash);
            }
            if let Some(hash) = plain.step(&decisions).hash {
                b.push(hash);
            }
            if tick < 60 {
                assert_eq!(a, b, "the drop changed something before its tick");
            }
        }
        assert_ne!(a, b, "the drop never fired, or the pins never mattered");
    }
}
