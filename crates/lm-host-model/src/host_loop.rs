//! The host loop: the sequencer, the sim and the recorder, in that order.
//!
//! A host tick is four steps, and they are the same four steps the headless
//! runner takes, because they have to be:
//!
//! 1. the decisions that have arrived are handed to the sequencer, which
//!    stamps each one on the tick it belongs to -- or refuses it, by name;
//! 2. the sequencer is drained for this tick, which yields the decisions in
//!    the canonical order (system first, then the agents in a rotating
//!    order);
//! 3. the sim steps the tick with those decisions;
//! 4. the recorder writes the tick into the round log.
//!
//! The host adds exactly one thing to that: a person. A human action is not
//! a special case that reaches into the simulation; it becomes a
//! [`lm_core::decision::Decision`] with `source: Human(agent)` and
//! `provenance: Human`, and it travels through the very same sequencer the
//! agents use (spec §6, D10 lesson 10). The host never decides game
//! outcomes.
//!
//! ## Human decisions never wait
//!
//! A human decision is submitted with `apply_tick` and `arrived` both set to
//! the tick that is about to be simulated, so it is stamped on that tick and
//! applied there: the person sees the agent move on the next tick, not after
//! a scheduling window.
//!
//! ## Keeping the tick rate
//!
//! The sim runs at `Params::tick_hz`, decoupled from the frame rate. When a
//! frame takes longer than a tick, several ticks have to run to catch up;
//! when it takes less, the leftover has to be kept, or the sim would
//! slowly fall behind the clock. [`catch_up`] converts elapsed wall clock
//! into whole ticks, and [`TickBudget`] does the same while carrying the
//! remainder, so no tick is ever skipped or played twice.

use std::collections::BTreeMap;

use lm_agents::mind::ThoughtRecord;
use lm_agents::sink::{ChannelSink, DecisionInbox};
use lm_agents::{AgentStats, Agents};
use lm_core::decision::{Decision, Kind, Provenance, Source, Tick};
use lm_core::fixed::Pos;
use lm_core::header::Header;
use lm_core::log::{Recorder, RoundLog};
use lm_core::rules::{Event, SimError};
use lm_core::sched::{Reject, SchedStats, Sequencer};
use lm_core::sim::Sim;

/// What one tick of the host produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TickOut {
    /// The tick that was just simulated.
    pub tick: Tick,
    /// What happened on it: the markers of the decisions, then the events of
    /// the ruleset.
    pub events: Vec<Event>,
    /// The canonical state hash, on the ticks that take one.
    pub hash: Option<u64>,
}

/// The host loop: the authority that plays a round for a person.
///
/// It holds the sequencer (which decides what happens on which tick), the
/// sim (which plays the world) and the recorder (which writes the round
/// down). The host itself decides nothing: it only hands decisions to the
/// sequencer and asks the sim for the next tick.
///
/// The loop is deterministic. The same header and the same decisions produce
/// the same log, byte for byte, whether the round was played by a person or
/// by the headless runner.
pub struct HostLoop {
    /// The world being played.
    sim: Sim,
    /// The authority on which tick each decision lands on.
    sequencer: Sequencer,
    /// The round being written down.
    recorder: Recorder,
    /// The header of the round: its parameters, roster and rules.
    header: Header,
    /// The tick that is about to be simulated.
    next_tick: Tick,
    /// Where every agent stood before the last tick, for the renderer.
    prev_positions: Vec<(u16, Pos)>,
    /// The last `seq` the human used for each agent it played.
    human_seq: BTreeMap<u16, u32>,
    /// The round's agents, when the host has them.
    ///
    /// They think on their own thread and hand their decisions over through
    /// the inbox below; the host never calls a model itself.
    agents: Option<Agents>,
    /// Where the agents' decisions wait until the host picks them up.
    inbox: Option<DecisionInbox>,
}

impl HostLoop {
    /// Start a round from its header.
    ///
    /// The header pins the round: the parameters are validated, the world is
    /// generated from the seed, the agents are placed and the reflex scripts
    /// are loaded. A header that names an unknown ruleset, breaks a
    /// parameter or carries a script that does not compile is refused here,
    /// before any tick runs.
    pub fn new(h: Header) -> Result<HostLoop, SimError> {
        let sim = Sim::new(&h)?;

        // The roster is the truth about who plays: the sequencer refuses a
        // decision from a source that is not in it.
        let n_agents = u16::try_from(h.roster.len()).unwrap_or(u16::MAX);
        let sequencer = Sequencer::new(&h.params, n_agents, h.start_tick);
        let recorder = Recorder::new(h.clone(), h.params.segment_ticks);

        Ok(HostLoop {
            sim,
            sequencer,
            recorder,
            header: h.clone(),
            next_tick: h.start_tick,
            prev_positions: Vec::new(),
            human_seq: BTreeMap::new(),
            agents: None,
            inbox: None,
        })
    }

    /// Attach the round's agents.
    ///
    /// The agents think on their own thread and hand their decisions over
    /// through a channel; the host picks them up on the next tick and puts
    /// them in the sequencer, where they are logged like every other
    /// decision. A round without agents plays exactly as it did before.
    pub fn attach_agents(&mut self, agents: Agents) {
        let (sink, inbox) = ChannelSink::new();
        self.agents = Some(agents.with_sink(Box::new(sink)));
        self.inbox = Some(inbox);
    }

    /// Play one tick.
    ///
    /// The tick that is played is the one the host is up to; the decisions
    /// stamped for it are applied, and the tick is recorded. The host stops
    /// when the round is over: [`Sim::outcome`] says when that is.
    pub fn tick(&mut self) -> TickOut {
        let tick = self.next_tick;

        // The agents think first, and their answers that came home are put in
        // the sequencer now: a decision that is due lands on this tick, not
        // on the next one.
        self.run_agents(tick);

        // The decisions of this tick, in the canonical order.
        let applied = self.sequencer.drain(tick);

        // Where the agents stand before the tick: the renderer interpolates
        // between this and the position after it.
        self.prev_positions = self.positions();

        let out = self.sim.step(&applied);
        self.recorder.record(tick, &applied, out.hash);

        // The sim owns the tick counter; the host follows it.
        self.next_tick = self.sim.tick();

        TickOut {
            tick: out.tick,
            events: out.events,
            hash: out.hash,
        }
    }

    /// Submit a decision one of the agents made.
    ///
    /// `arrived` is the tick the answer came home on: the first tick the
    /// host had not simulated yet. The sequencer has the last word — a
    /// decision for an agent that is not in the round, or a `seq` it has
    /// seen before, is refused by name.
    pub fn submit_agent(&mut self, decision: Decision, arrived: Tick) -> Result<Tick, Reject> {
        self.sequencer.submit(decision, arrived)
    }

    /// What the round's agents did, in numbers.
    ///
    /// `None` when the round has no agents: the counters are then the
    /// reflex's alone.
    pub fn agent_stats(&self) -> Option<&AgentStats> {
        self.agents.as_ref().map(Agents::stats)
    }

    /// The round's agents, when the host has them.
    pub fn agents(&self) -> Option<&Agents> {
        self.agents.as_ref()
    }

    /// The thought records of one agent (D55): what was asked, when, and
    /// when the answer landed. The piano roll draws them as bars.
    pub fn agent_thoughts(&self, agent: u16) -> &[ThoughtRecord] {
        self.agents
            .as_ref()
            .map(|agents| agents.thoughts(agent))
            .unwrap_or_default()
    }

    /// The thought records of every agent.
    pub fn agent_thoughts_all(&self) -> BTreeMap<u16, Vec<ThoughtRecord>> {
        self.agents
            .as_ref()
            .map(Agents::all_thoughts)
            .unwrap_or_default()
    }

    /// How many questions are with the models right now.
    pub fn agents_pending(&self) -> usize {
        self.agents.as_ref().map_or(0, Agents::pending)
    }

    /// Let the agents think, and put the decisions they made in the sequencer.
    fn run_agents(&mut self, now: Tick) {
        let Some(agents) = self.agents.as_mut() else {
            return;
        };
        agents.on_tick(now, &self.sim);

        // The decisions that came home since the last tick. They are picked
        // up before the sequencer is drained, so one that is due now is
        // applied now.
        let arrived: Vec<(Decision, Tick)> = match self.inbox.as_mut() {
            Some(inbox) => inbox.drain(),
            None => Vec::new(),
        };
        for (decision, arrived) in arrived {
            // The sequencer refuses by name; a refused decision is lost, and
            // that is the sequencer's last word.
            let _ = self.submit_agent(decision, arrived);
        }
    }

    /// Submit an action of the human playing as `agent`.
    ///
    /// The decision is stamped with the human's own `seq` for that agent
    /// (which must strictly increase), with `apply_tick` and `arrived` both
    /// set to the tick that is about to be simulated, and with the
    /// provenance `Human`. It is therefore applied on the very next tick.
    ///
    /// The sequencer has the last word: a decision for an agent that is not
    /// in the round is refused, by name.
    pub fn submit_human(&mut self, agent: u16, kind: Kind) -> Result<Tick, Reject> {
        let now = self.next_tick;
        let seq = self.next_human_seq(agent);
        let decision = Decision {
            apply_tick: now,
            source: Source::Human(agent),
            seq,
            kind,
            provenance: Provenance::Human,
        };
        self.sequencer.submit(decision, now)
    }

    /// The world as it stands.
    pub fn sim(&self) -> &Sim {
        &self.sim
    }

    /// The header of the round: its parameters, its roster and its rules.
    ///
    /// The header is the round's identity (the sim's own copy of it is
    /// private), and a host needs it to say what the round is: how fast it
    /// ticks, how long it may run, who plays and how many squads there are.
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// How the sequencer is coping.
    ///
    /// The three counters are the ones a host shows a person: the decisions
    /// that landed on the tick they asked for, the ones that landed later,
    /// and the ones that were too late to be used at all.
    pub fn sched_stats(&self) -> SchedStats {
        self.sequencer.stats()
    }

    /// The round log so far: a copy of the recorder's state.
    ///
    /// The copy is sealed the way [`HostLoop::finish`] seals it, so it is a
    /// log in its own right: it can be written out and replayed.
    pub fn log_so_far(&self) -> RoundLog {
        self.recorder.clone().finish()
    }

    /// Finish the round and hand back its log.
    pub fn finish(self) -> RoundLog {
        self.recorder.finish()
    }

    /// Where every agent stood before the last tick, in agent-id order.
    ///
    /// The renderer draws the agents between these positions and the ones
    /// they are in now, so the picture moves smoothly even when the frame
    /// rate is below the tick rate.
    pub fn prev_positions(&self) -> &[(u16, Pos)] {
        &self.prev_positions
    }

    /// The next `seq` of the human playing as `agent`.
    ///
    /// The sequencer refuses a `seq` it has seen before, and the human's
    /// counter only ever grows, so the human's decisions are always
    /// distinguishable from the agent's -- and from each other's.
    fn next_human_seq(&mut self, agent: u16) -> u32 {
        let counter = self.human_seq.entry(agent).or_insert(0);
        *counter = counter.saturating_add(1);
        *counter
    }

    /// Where every agent stands, in agent-id order.
    fn positions(&self) -> Vec<(u16, Pos)> {
        self.sim
            .agent_ids()
            .into_iter()
            .map(|id| {
                let pos = self.sim.agent(id).map_or_else(Pos::zero, |agent| agent.pos);
                (id, pos)
            })
            .collect()
    }
}

/// How many ticks of simulation `elapsed_ms` of wall clock is worth.
///
/// The sim runs at `tick_hz`, decoupled from the frame rate: a frame that
/// took longer than a tick has to be answered by several ticks, and a frame
/// that took less leaves a remainder that has to be kept. This is the
/// conversion on its own; [`TickBudget`] is the version that carries the
/// remainder from one call to the next.
pub fn catch_up(elapsed_ms: u64, tick_hz: u16) -> u32 {
    u32::try_from(elapsed_ms.saturating_mul(u64::from(tick_hz)) / 1000).unwrap_or(u32::MAX)
}

/// The tick budget of a host that keeps its own rate.
///
/// It counts in thousandths of a tick, so the remainder of one call is
/// carried into the next and no fraction of a tick is ever lost. Over many
/// calls it hands out exactly as many ticks as the elapsed time is worth:
/// the sim keeps its rate whatever the frame rate does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TickBudget {
    /// The time owed to the sim, in thousandths of a tick.
    carry: u64,
}

impl TickBudget {
    /// A budget that owes the sim nothing.
    pub fn new() -> Self {
        Self { carry: 0 }
    }

    /// The ticks to run now, keeping the remainder for later.
    pub fn catch_up(&mut self, elapsed_ms: u64, tick_hz: u16) -> u32 {
        let total = self
            .carry
            .saturating_add(elapsed_ms.saturating_mul(u64::from(tick_hz)));
        self.carry = total % 1000;
        u32::try_from(total / 1000).unwrap_or(u32::MAX)
    }

    /// The time still owed to the sim, in thousandths of a tick.
    pub fn carry(&self) -> u64 {
        self.carry
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lm_core::decision::Action;

    /// A standard round with the shipped reflex.
    fn round(seed: u64) -> Header {
        Header::default_round(seed, lm_tools::runner::DEFAULT_REFLEX)
    }

    #[test]
    fn a_new_loop_starts_on_the_first_tick() {
        let host = HostLoop::new(round(1)).expect("the round starts");
        assert_eq!(host.sim().tick(), 0);
        assert!(host.prev_positions().is_empty());
    }

    #[test]
    fn a_new_loop_refuses_a_broken_header() {
        let mut h = round(1);
        h.ruleset = "capture-the-flag".to_owned();
        assert!(matches!(HostLoop::new(h), Err(SimError::UnknownRuleset(_))));
    }

    /// The host loop lives in a resource of the Bevy client, and a resource
    /// has to be `Send + Sync` to be shared safely between the systems that
    /// read it. The ruleset of the round is the one thing that could have
    /// stopped that, so the bound is asserted here rather than discovered on
    /// the Windows build.
    #[test]
    fn the_host_loop_can_be_shared_between_threads() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<HostLoop>();
    }

    #[test]
    fn a_tick_advances_the_sim_by_one() {
        let mut host = HostLoop::new(round(1)).expect("the round starts");
        assert_eq!(host.tick().tick, 0);
        assert_eq!(host.tick().tick, 1);
        assert_eq!(host.sim().tick(), 2);
    }

    #[test]
    fn the_human_seq_grows_for_every_agent() {
        let mut host = HostLoop::new(round(1)).expect("the round starts");
        host.submit_human(0, Kind::ActionOn(Action::Hold))
            .expect("the decision is accepted");
        host.submit_human(0, Kind::ActionOff)
            .expect("the decision is accepted");
        host.submit_human(1, Kind::ActionOn(Action::Hold))
            .expect("the decision is accepted");

        // They all land on the tick that is about to be simulated.
        host.tick();

        let log = host.log_so_far();
        let seqs: Vec<u32> = log
            .segments
            .iter()
            .flat_map(|segment| segment.decisions.iter())
            .map(|stamped| stamped.d.seq)
            .collect();
        // Two for agent 0 and one for agent 1, each source counting on its
        // own.
        assert_eq!(seqs, vec![1, 2, 1]);
    }

    #[test]
    fn a_decision_for_an_agent_that_is_not_in_the_round_is_refused() {
        let mut host = HostLoop::new(round(1)).expect("the round starts");
        assert_eq!(
            host.submit_human(99, Kind::ActionOn(Action::Hold)),
            Err(Reject::UnknownSource)
        );
    }

    #[test]
    fn the_tick_budget_never_loses_a_fraction_of_a_tick() {
        // 1000 ms at 30 Hz is 30 ticks, whatever the frame rate.
        let mut budget = TickBudget::new();
        let mut ticks = 0;
        for _ in 0..1_000 {
            ticks += budget.catch_up(1, 30);
        }
        assert_eq!(ticks, 30);
        assert_eq!(budget.carry(), 0);
    }

    #[test]
    fn the_tick_budget_survives_a_stalled_frame() {
        // A frame that took a whole second owes thirty ticks at once.
        let mut budget = TickBudget::new();
        assert_eq!(budget.catch_up(1_000, 30), 30);
        assert_eq!(budget.catch_up(0, 30), 0);
    }
}
