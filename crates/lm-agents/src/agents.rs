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
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use lm_core::decision::{Decision, Tick};
use lm_core::header::Header;
use lm_core::refusal::{RefusalCounts, RefusalKind, Refused};
use lm_core::sim::Sim;
use lm_core::view::menu;

use crate::config::AgentsConfig;
use crate::gates::Pack;
use crate::harness::Harness;
use crate::mind::{AskOutcome, LineWatch, Mind, Thinker, ThoughtRecord};
use crate::persona::{AppliedBias, Persona, SkippedBias};
use crate::queue::{Budgets, LeastUseQueue};
use crate::sink::DecisionSink;
use crate::surprise::RoundSurprise;
use crate::sysone::{Refusal, SysOne};
use crate::utter::Realizer;

/// The round's agents: one mind per roster entry, one queue, one budget set
/// and one worker thread asking the models.
pub struct Agents {
    minds: BTreeMap<u16, Mind>,
    queue: LeastUseQueue,
    budgets: Budgets,
    thinker: Thinker,
    sink: Option<Box<dyn DecisionSink>>,
    stats: AgentStats,
    /// How many of each mind's refusals the round has counted already.
    counted: BTreeMap<u16, usize>,
    /// The life of every ask of the round, on the monotonic clock (N1).
    timings: Vec<AskTiming>,
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
        let personas = load_personas(cfg, harness);
        let minds = header
            .roster
            .iter()
            .map(|entry| {
                let persona = match personas.get(&entry.agent) {
                    Some(Ok(persona)) => Some(persona.clone()),
                    Some(Err(why)) => {
                        // A persona that did not load is a refusal with a
                        // name: the mind plays without it, and the round
                        // says why (D26).
                        let why = why.clone();
                        let mut mind = Mind::new(entry.agent, entry.role, harness, cfg)
                            .with_params(&header.params)
                            .with_roster(&header.roster)
                            .with_talk(&header.talk);
                        mind.refuse(Refusal::Malformed(why));
                        return (entry.agent, mind);
                    }
                    None => None,
                };
                let mind = Mind::new_with_persona(entry.agent, entry.role, harness, cfg, persona)
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
            counted: BTreeMap::new(),
            timings: Vec::new(),
        }
    }

    /// Give the agents somewhere to send their decisions.
    pub fn with_sink(mut self, sink: Box<dyn DecisionSink>) -> Agents {
        self.sink = Some(sink);
        self
    }

    /// Give every mind a realizer of its own.
    ///
    /// The mind picks its realizer when it is built: the configuration's
    /// `utter.realizer` over the harness's phrasebook. This replaces it,
    /// so a caller that wants to watch the wording -- a measurement -- can
    /// put its own in and read the lines afterwards.
    pub fn with_realizer(&mut self, make: impl Fn(u16) -> Box<dyn Realizer>) {
        for (agent, mind) in self.minds.iter_mut() {
            let realizer = make(*agent);
            mind.set_realizer(realizer);
        }
    }

    /// Watch the lines the minds word.
    ///
    /// The watcher sees every line, with the agent that said it: it is how
    /// a measurement hears whether two squads sound alike. The lines are
    /// telemetry — the sim still decides which of them are spoken — and
    /// the watcher never blocks the round.
    pub fn watch_lines(&mut self, watch: LineWatch) {
        for mind in self.minds.values_mut() {
            mind.set_line_watch(Arc::clone(&watch));
        }
    }

    /// The round's agents, by id.
    pub fn minds(&self) -> &BTreeMap<u16, Mind> {
        &self.minds
    }

    /// The counters of the round (for the HUD).
    pub fn stats(&self) -> &AgentStats {
        &self.stats
    }

    /// The round's surprise table: every mind's answers, merged into one
    /// table of situation buckets (the signal the between-round reviewer is
    /// pointed at).
    pub fn surprise(&self) -> RoundSurprise {
        let mut table = RoundSurprise::default();
        for mind in self.minds.values() {
            table.merge(mind.surprise());
        }
        table
    }

    /// The personas of the round, by agent id.
    ///
    /// The id is the persona's own: the file stem it was read from. An
    /// agent without a persona is not in here.
    pub fn personas(&self) -> BTreeMap<u16, String> {
        self.minds
            .iter()
            .filter_map(|(agent, mind)| mind.persona_id().map(|id| (*agent, id.to_string())))
            .collect()
    }

    /// The biases the personas applied, by agent, and what they did.
    ///
    /// The record is outside the sim: the patch is applied before the
    /// choice, so it is not a decision. It is kept because a reader cannot
    /// tell a mind that thinks with a personality from one that does not by
    /// looking at the round.
    pub fn persona_log(&self) -> BTreeMap<u16, Vec<AppliedBias>> {
        self.minds
            .iter()
            .filter(|(_, mind)| !mind.persona_log().is_empty())
            .map(|(agent, mind)| (*agent, mind.persona_log().to_vec()))
            .collect()
    }

    /// The biases the personas could not apply, by agent, and why.
    pub fn persona_skipped(&self) -> BTreeMap<u16, Vec<SkippedBias>> {
        self.minds
            .iter()
            .filter(|(_, mind)| !mind.persona_skipped().is_empty())
            .map(|(agent, mind)| (*agent, mind.persona_skipped().to_vec()))
            .collect()
    }

    /// One tick of the cognition loop.
    ///
    /// Every mind looks at the world and may ask a question; the queue picks
    /// the requests that go to the models now; the answers that have come
    /// home become decisions, which go to the sink stamped with `now`.
    ///
    /// The loop never blocks: a model that is slow costs the round nothing
    /// but a late answer.
    ///
    /// The tick's refusals come back to the caller, named: they are what the
    /// round log records (N2) and what the score view draws (E7). A question
    /// that was asked and never became an answer ends here, with a name —
    /// nothing is allowed to vanish on the way. A caller that keeps no log
    /// says so with `let _`, so the drop is a decision and not an oversight.
    #[must_use = "the refusals are the round's record; dropping them silently loses the why of every silence"]
    pub fn on_tick(&mut self, now: Tick, sim: &Sim) -> Vec<Refused> {
        let mut refused: Vec<Refused> = Vec::new();
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
                self.stats.asked += 1;
            }
            if let Some(req) = mind.on_tick_talk(now, &view, sim.world(), &menu) {
                requests.push(req);
                self.stats.asked += 1;
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
        // A request the budget refused and a request that expired in the
        // queue are two different problems, and both are questions that were
        // asked and never became an answer.
        for req in &popped.refused_budget {
            self.stats.refused += 1;
            self.stats.refusal_counts.count(RefusalKind::Budget);
            refused.push(Refused::new(now, req.agent, RefusalKind::Budget));
        }
        for req in &popped.expired {
            self.stats.expired += 1;
            self.stats.refused += 1;
            self.stats.refusal_counts.count(RefusalKind::Expired);
            refused.push(Refused::new(now, req.agent, RefusalKind::Expired));
        }
        for req in popped.send {
            let ask = self
                .minds
                .get_mut(&req.agent)
                .and_then(|mind| mind.take_flight(&req));
            let Some(ask) = ask else {
                // The mind replaced the request with a newer one; this one is
                // never sent. It ends here, and it ends with a name.
                self.stats.refused += 1;
                self.stats.refusal_counts.count(RefusalKind::Superseded);
                refused.push(Refused::new(now, req.agent, RefusalKind::Superseded));
                continue;
            };
            if !self.thinker.send(ask) {
                // The worker is gone: the call never reached a model. The
                // question ends here, and it ends with a name.
                self.stats.refused += 1;
                self.stats.refusal_counts.count(RefusalKind::Unreachable);
                refused.push(Refused::new(now, req.agent, RefusalKind::Unreachable));
                continue;
            }
            // The call is made: the model is used, whether it answers or not.
            self.queue.record(req.agent, 1);
            *self.stats.usage.entry(req.agent).or_insert(0) += 1;
            self.stats.thinks += 1;
        }

        // 3. The answers that came home become decisions.
        let landed_at = Instant::now();
        let thoughts = self.thinker.drain();
        self.stats.answered += thoughts.len() as u64;
        let mut decisions: Vec<Decision> = Vec::new();
        for thought in thoughts {
            // The escalations (D71): every gate that was asked of a second
            // model, counted next to the refusals.
            let latency_ms = match &thought.result {
                Ok(answers) => {
                    self.stats.escalations += answers.escalated.len() as u64;
                    answers.latency_ms
                }
                Err(_) => 0,
            };
            // The life of the ask (N1): asked, sent, taken by the worker,
            // answered, and the tick the decision was applied on.
            let pack = thought.ask.pack;
            let agent = thought.ask.req.agent;
            let backend = thought.ask.backend.clone();
            let asked = thought.ask.asked_at;
            let sent = thought.ask.sent_at.unwrap_or(thought.ask.asked_at);
            let picked_up = thought.picked_up;
            let answered = thought.answered_at;
            // How the question ended: taken, late, or refused. Every
            // question ends in exactly one of the three, and the round's
            // ledger adds up (N4).
            let (made, outcome) = match self.minds.get_mut(&agent) {
                Some(mind) => mind.on_answer(thought.ask, thought.result),
                None => {
                    // The answer belongs to an agent that is not in the
                    // round: there is no mind to take it home. The question
                    // ends here, and it ends with a name.
                    self.stats.refusal_counts.count(RefusalKind::Malformed);
                    refused.push(Refused::new(now, agent, RefusalKind::Malformed));
                    (Vec::new(), AskOutcome::Refused)
                }
            };
            self.timings.push(AskTiming {
                pack,
                agent,
                backend,
                asked,
                sent,
                picked_up,
                answered,
                landed: now,
                landed_at,
                applied: made.iter().map(|decision| decision.apply_tick).min(),
                latency_ms,
            });
            decisions.extend(made);
            match outcome {
                AskOutcome::Accepted => self.stats.accepted += 1,
                AskOutcome::Late => self.stats.late += 1,
                AskOutcome::Refused => self.stats.refused += 1,
            }
        }
        // 4. And so do the decisions that needed no model (the verdict cache).
        for mind in self.minds.values_mut() {
            decisions.extend(mind.take_decisions());
        }

        // 5. The refusals the minds recorded, by name. Every refusal the
        //    round saw is counted where it happened, so the record and the
        //    counters cannot disagree.
        self.take_mind_refusals(&mut refused);
        self.stats.refusals = self.stats.refusal_counts.total();

        // 6. The wording is counted where it happened: every mind knows
        //    which realizer wrote its lines.
        self.stats.words = self
            .minds
            .values()
            .fold(WordCounts::default(), |sum, mind| sum.plus(mind.words()));

        // 7. The decisions go to the host, stamped with the tick they landed
        //    on. The host puts them in the sequencer.
        if let Some(sink) = self.sink.as_mut() {
            for decision in decisions {
                sink.submit(decision, now);
                self.stats.decisions += 1;
            }
        }
        self.stats.queue_len = self.queue.len();
        refused
    }

    /// The refusals the minds have recorded that the round has not counted
    /// yet.
    ///
    /// The minds keep their refusals in the order they happened; the round
    /// counts them once, here, and never counts the same one twice.
    fn take_mind_refusals(&mut self, refused: &mut Vec<Refused>) {
        let agents: Vec<u16> = self.minds.keys().copied().collect();
        for agent in agents {
            let Some(mind) = self.minds.get_mut(&agent) else {
                continue;
            };
            let all = mind.refusals();
            let taken = self.counted.entry(agent).or_insert(0);
            let fresh = all[*taken..].to_vec();
            *taken = all.len();
            for (tick, refusal) in fresh {
                let kind = RefusalKind::from(&refusal);
                self.stats.refusal_counts.count(kind);
                refused.push(Refused {
                    tick,
                    agent,
                    kind,
                    detail: refusal.detail(),
                });
            }
        }
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

    /// The life of every ask of the round, on the monotonic clock (N1).
    ///
    /// The stamps are outside the sim: they are telemetry, and they never
    /// enter the sim's state — the sim only ever sees the decisions. They
    /// add up to the split of a think: the wait in our own queue, the
    /// worker's own wait, the wire, the model, and the wait for the
    /// decision to be applied.
    pub fn timings(&self) -> &[AskTiming] {
        &self.timings
    }
}

/// The life of one ask, on the monotonic clock (N1).
///
/// The four stamps the split is made of: **asked** (the mind decided to
/// ask), **sent** (the ask left the least-use-first queue), **picked up**
/// (the worker took it off its channel) and **answered** (the answer came
/// home), plus the tick the decision was applied on.
#[derive(Clone, Debug)]
pub struct AskTiming {
    /// Which pack the ask carried (D70).
    pub pack: Pack,
    /// The agent that asked.
    pub agent: u16,
    /// The backend the ask went to (a tier's backend name).
    ///
    /// The name is what joins a timing to the log of the call that made it:
    /// a round may ask several backends, and only one of them may be the one
    /// the measurement watches.
    pub backend: String,
    /// When the mind asked.
    pub asked: Instant,
    /// When the ask left the least-use-first queue.
    pub sent: Instant,
    /// When the worker took the ask off its channel.
    pub picked_up: Instant,
    /// When the answer came home.
    pub answered: Instant,
    /// The tick the answer landed on.
    pub landed: Tick,
    /// The wall clock when the answer was picked up, at the tick it landed
    /// on.
    pub landed_at: Instant,
    /// The tick the decision was applied on; `None` when it made none.
    pub applied: Option<Tick>,
    /// The round trip the backend measured, in milliseconds.
    pub latency_ms: u32,
}

impl AskTiming {
    /// The wait in our own least-use-first queue, in milliseconds.
    pub fn queue_ms(&self) -> u64 {
        ms_between(self.asked, self.sent)
    }

    /// The wait in the worker's own channel, in milliseconds.
    ///
    /// The worker takes one ask at a time, so an ask waits for the call
    /// before it. This is our own queue too, and it is the one the model's
    /// round trip does not see.
    pub fn worker_ms(&self) -> u64 {
        ms_between(self.sent, self.picked_up)
    }

    /// The flight: the moment the worker had the ask to the moment the
    /// answer came home, in milliseconds.
    pub fn flight_ms(&self) -> u64 {
        ms_between(self.picked_up, self.answered)
    }

    /// The wait from the answer landing to the decision being applied, in
    /// milliseconds.
    ///
    /// The decision's lead is the scheduling slack; the rest is what was
    /// left of the tick the answer landed in.
    pub fn apply_ms(&self, tick_hz: u16) -> u64 {
        let Some(applied) = self.applied else {
            return 0;
        };
        let lead_ms = applied.saturating_sub(self.landed) * 1000 / u64::from(tick_hz);
        lead_ms.saturating_add(ms_between(self.answered, self.landed_at))
    }
}

/// The milliseconds between two instants on the monotonic clock.
fn ms_between(from: Instant, to: Instant) -> u64 {
    u64::try_from(to.saturating_duration_since(from).as_millis()).unwrap_or(u64::MAX)
}

impl fmt::Debug for Agents {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Agents")
            .field("minds", &self.minds.len())
            .field("stats", &self.stats)
            .finish()
    }
}

/// The personas of the round, by agent id.
///
/// The configuration says which agent thinks with which persona, and the
/// library says what a persona is. A persona that does not load, or that
/// names a voice the harness's `voice.toml` does not carry, is a **refusal
/// with a name**: the agent plays without it, and the round says why
/// (D26).
///
/// The library is a directory of `<id>.toml` files, and the personas are
/// shared: every harness may use every persona.
fn load_personas(cfg: &AgentsConfig, harness: &Harness) -> BTreeMap<u16, Result<Persona, String>> {
    let mut loaded: BTreeMap<u16, Result<Persona, String>> = BTreeMap::new();
    if cfg.personas.is_empty() {
        return loaded;
    }
    let dir = PathBuf::from(&cfg.personas_dir);
    let voice_toml = harness.voice_toml();
    for (agent, persona_id) in &cfg.personas {
        let Ok(agent) = agent.parse::<u16>() else {
            eprintln!(
                "lm-agents: the persona table names the agent {agent:?}, which is not an agent id"
            );
            continue;
        };
        let persona = Persona::load(&dir, persona_id)
            .map_err(|error| error.to_string())
            .and_then(|persona| {
                persona
                    .validate_voice(&voice_toml)
                    .map_err(|error| error.to_string())?;
                Ok(persona)
            });
        match &persona {
            Ok(persona) => eprintln!(
                "lm-agents: agent {agent} thinks with the persona {} ({})",
                persona.id, persona.name
            ),
            Err(why) => eprintln!("lm-agents: agent {agent} has no persona: {why}"),
        }
        loaded.insert(agent, persona);
    }
    loaded
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
    /// The questions the minds asked.
    ///
    /// A question is asked when the mind asks it, before the queue decides
    /// whether it is sent: a question that the budget refuses or that the
    /// queue lets expire was still asked.
    pub asked: u64,
    /// Answers that came home and were taken.
    pub accepted: u64,
    /// Answers that came home after the tick the tier said they would land
    /// on.
    pub late: u64,
    /// Questions that never became an answer.
    ///
    /// The budget refused it, the queue let it expire, a newer question
    /// replaced it, the worker was gone, or the model did not answer.
    pub refused: u64,
    /// Refusals: a failed call, a low-confidence answer, a spent budget.
    ///
    /// This is the number of refusals, of every kind together. The kinds
    /// themselves are in `refusal_counts`: one column that adds four
    /// different problems up cannot tell them apart.
    pub refusals: u64,
    /// The refusals, by name.
    ///
    /// Every refusal the round saw is counted here, where it happened, so
    /// the numbers and the log's record cannot disagree.
    pub refusal_counts: RefusalCounts,
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
    /// The wording: which realizer wrote the lines.
    pub words: WordCounts,
}

impl AgentStats {
    /// Whether every question that was asked ended in exactly one place.
    ///
    /// A question is taken, late or refused, and it is exactly one of the
    /// three: this is the assertion that keeps a lost question from
    /// disappearing without a word (N4).
    pub fn asks_add_up(&self) -> bool {
        self.asked == self.accepted + self.late + self.refused
    }
}

/// What the wording did, in numbers.
///
/// The HUD shows these: how many lines the model on the Thor worded, how
/// many the harness's phrasebook worded, and how many of those the model
/// missed its deadline on. A round that ran with the phrasebook says so
/// here, and so does a round where the model was late.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WordCounts {
    /// Lines the model on the Thor worded.
    pub llm: u64,
    /// Lines the harness's phrasebook worded.
    pub phrasebook: u64,
    /// Lines the model was asked for and did not word in time.
    pub late: u64,
}

impl WordCounts {
    /// The counts of two minds, added together.
    pub fn plus(self, other: WordCounts) -> WordCounts {
        WordCounts {
            llm: self.llm + other.llm,
            phrasebook: self.phrasebook + other.phrasebook,
            late: self.late + other.late,
        }
    }

    /// The lines that were worded at all.
    pub fn lines(&self) -> u64 {
        self.llm + self.phrasebook
    }
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
