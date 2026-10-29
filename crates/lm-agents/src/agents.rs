//! Task 7: the round's agents — the loop that turns a sim into questions and
//! the models' answers into decisions.
//!
//! One tick of the loop, per agent:
//!
//! ```text
//! ScoreView --mind--> a ThinkReq (or nothing: the moment does not deserve a model)
//!          --queue--> least-use-first, under the round's budgets (D16)
//!          --thinker--> a worker thread asks the model; the sim never waits
//!          --mind--> the answer becomes decisions, or a logged refusal (D26)
//!          --sink--> the host puts them in the sequencer, where they are logged
//! ```
//!
//! Nothing here is needed to replay a round: the decisions reach the sim as
//! logged decisions, and the models are gone by then.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fmt;

use lm_core::decision::{Decision, Tick};
use lm_core::header::Header;
use lm_core::sim::Sim;
use lm_core::view::menu;

use crate::config::AgentsConfig;
use crate::harness::Harness;
use crate::mind::{Mind, Thinker, ThoughtRecord};
use crate::queue::{Budgets, LeastUseQueue};
use crate::sink::DecisionSink;
use crate::sysone::SysOne;

/// The round's agents: one mind per roster entry, one queue, one budget set
/// and one worker thread asking the models.
pub struct Agents {
    minds: BTreeMap<u16, Mind>,
    queue: LeastUseQueue,
    budgets: Budgets,
    thinker: Thinker,
    sink: Option<Box<dyn DecisionSink>>,
    stats: AgentStats,
}

impl Agents {
    /// The agents of a round.
    ///
    /// `harness` holds the gates and the policy every mind thinks with, and
    /// `backends` are the models, by the name a tier asks for. A mind whose
    /// tier names a backend that is not here will hear a refusal, never a
    /// guessed answer (D26).
    pub fn new(
        cfg: &AgentsConfig,
        harness: &Harness,
        header: &Header,
        backends: BTreeMap<String, Box<dyn SysOne + Send + Sync>>,
    ) -> Agents {
        let minds = header
            .roster
            .iter()
            .map(|entry| {
                let mind = Mind::new(entry.agent, entry.role, harness, cfg)
                    .with_params(&header.params)
                    .with_roster(&header.roster)
                    .with_talk(&header.talk);
                (entry.agent, mind)
            })
            .collect();
        let budgets = Budgets::uniform(
            header.roster.iter().map(|entry| entry.agent),
            cfg.budget_calls_per_round,
        );
        let queue = LeastUseQueue::new(squad_weight_permille(cfg.squad_weight));
        Agents {
            minds,
            queue,
            budgets,
            thinker: Thinker::new(backends),
            sink: None,
            stats: AgentStats::default(),
        }
    }

    /// Give the agents somewhere to send their decisions.
    pub fn with_sink(mut self, sink: Box<dyn DecisionSink>) -> Agents {
        self.sink = Some(sink);
        self
    }

    /// The round's agents, by id.
    pub fn minds(&self) -> &BTreeMap<u16, Mind> {
        &self.minds
    }

    /// The counters of the round (for the HUD).
    pub fn stats(&self) -> &AgentStats {
        &self.stats
    }

    /// One tick of the cognition loop.
    ///
    /// Every mind looks at the world and may ask a question; the queue picks
    /// the requests that go to the models now; the answers that have come
    /// home become decisions, which go to the sink stamped with `now`.
    ///
    /// The loop never blocks: a model that is slow costs the round nothing
    /// but a late answer.
    pub fn on_tick(&mut self, now: Tick, sim: &Sim) {
        // 1. Every mind looks. A mind that is busy with an answer, or that
        //    does not want to think this tick, asks for nothing. The move
        //    pack and the talk pack are asked apart (D70): the talk pack
        //    only when the agent has something to say.
        let mut requests = Vec::new();
        for mind in self.minds.values_mut() {
            let view = sim.view(mind.agent());
            let menu = menu(&view);
            if let Some(req) = mind.on_tick(now, &view, sim.world(), &menu) {
                requests.push(req);
            }
            if let Some(req) = mind.on_tick_talk(now, &view, sim.world(), &menu) {
                requests.push(req);
            }
        }
        for req in requests {
            self.queue.push(req);
        }

        // 2. The queue picks who goes to the models now, under the budgets.
        //    One request per agent and pack at most, so the batch is never
        //    bigger than twice the number of minds.
        let popped = self
            .queue
            .pop_batch(now, self.minds.len() * 2, &mut self.budgets);
        self.stats.refusals += popped.refused_budget.len() as u64;
        self.stats.expired += popped.expired.len() as u64;
        for req in popped.send {
            let Some(ask) = self
                .minds
                .get_mut(&req.agent)
                .and_then(|mind| mind.take_flight(&req))
            else {
                // The mind replaced the request with a newer one; this one is
                // simply never sent.
                continue;
            };
            self.thinker.send(ask);
            // The call is made: the model is used, whether it answers or not.
            self.queue.record(req.agent, 1);
            *self.stats.usage.entry(req.agent).or_insert(0) += 1;
            self.stats.thinks += 1;
        }

        // 3. The answers that came home become decisions.
        let thoughts = self.thinker.drain();
        self.stats.answered += thoughts.len() as u64;
        let mut decisions: Vec<Decision> = Vec::new();
        for thought in thoughts {
            if thought.result.is_err() {
                self.stats.refusals += 1;
            }
            // The escalations (D71): every gate that was asked of a second
            // model, counted next to the refusals.
            if let Ok(answers) = &thought.result {
                self.stats.escalations += answers.escalated.len() as u64;
            }
            if let Some(mind) = self.minds.get_mut(&thought.ask.req.agent) {
                decisions.extend(mind.on_answer(thought.ask, thought.result));
            }
        }
        // 4. And so do the decisions that needed no model (the verdict cache).
        for mind in self.minds.values_mut() {
            decisions.extend(mind.take_decisions());
        }

        // 5. The decisions go to the host, stamped with the tick they landed
        //    on. The host puts them in the sequencer.
        if let Some(sink) = self.sink.as_mut() {
            for decision in decisions {
                sink.submit(decision, now);
                self.stats.decisions += 1;
            }
        }
        self.stats.queue_len = self.queue.len();
    }

    /// How many questions are with the models right now.
    pub fn pending(&self) -> usize {
        self.thinker.pending()
    }

    /// The thought records of one agent (D55): what was asked, when, and
    /// when the answer landed.
    pub fn thoughts(&self, agent: u16) -> &[ThoughtRecord] {
        self.minds
            .get(&agent)
            .map(Mind::thoughts)
            .unwrap_or_default()
    }

    /// The thought records of every agent.
    pub fn all_thoughts(&self) -> BTreeMap<u16, Vec<ThoughtRecord>> {
        self.minds
            .iter()
            .map(|(agent, mind)| (*agent, mind.thoughts().to_vec()))
            .collect()
    }
}

impl fmt::Debug for Agents {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Agents")
            .field("minds", &self.minds.len())
            .field("stats", &self.stats)
            .finish()
    }
}

/// What the round's agents did, in numbers.
///
/// The HUD shows these; the log keeps the decisions themselves.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentStats {
    /// Questions sent to the models.
    pub thinks: u64,
    /// Answers that came home (accepted or refused).
    pub answered: u64,
    /// Refusals: a failed call, a low-confidence answer, a spent budget.
    pub refusals: u64,
    /// Gates that were asked of a second model (D71).
    ///
    /// An escalation is not a refusal: it is a second opinion that was paid
    /// for, and the answer it produced is the gate's answer.
    pub escalations: u64,
    /// Requests dropped because their deadline passed while queued.
    pub expired: u64,
    /// Decisions the agents' answers became.
    pub decisions: u64,
    /// Questions waiting for a model right now.
    pub queue_len: usize,
    /// Model calls per agent.
    pub usage: BTreeMap<u16, u64>,
}

/// The squad weight in per-mille, so the queue's ordering stays integer.
fn squad_weight_permille(weight: f32) -> u64 {
    if !weight.is_finite() || weight <= 0.0 {
        return 0;
    }
    let per_mille = (weight * 1000.0).round();
    if per_mille >= u64::MAX as f32 {
        u64::MAX
    } else {
        per_mille as u64
    }
}

// -------------------------------------------------------------------- the tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_squad_weight_is_kept_in_per_mille() {
        assert_eq!(squad_weight_permille(0.5), 500);
        assert_eq!(squad_weight_permille(1.0), 1000);
        assert_eq!(squad_weight_permille(0.0), 0);
        assert_eq!(squad_weight_permille(-1.0), 0);
        assert_eq!(squad_weight_permille(f32::NAN), 0);
    }
}
