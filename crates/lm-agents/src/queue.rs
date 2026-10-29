//! The least-use-first think queue and per-agent budgets (D16, phase-3 Task 4).
//!
//! The local decision model on the Thor is shared by every agent in the
//! round. Who gets it next is part of the game: the agent that has used it
//! **least this round goes first**, so saving thinking for the late round is
//! a real strategy, and a chatty agent can never starve its squad.
//!
//! Three rules keep the queue honest under load:
//!
//! 1. **One pending request per agent.** A newer request from the same agent
//!    replaces the older one: the newest facts are the only ones worth
//!    asking about, and an outage can never build a backlog (Review Focus 1).
//! 2. **Expired requests are dropped, never sent.** A request whose deadline
//!    has passed would only produce a stale decision the sequencer drops
//!    anyway, so the queue drops it first and spends no model time on it.
//! 3. **Budgets are checked before a call, not after.** An agent with no
//!    calls left is refused at pop time and makes **no HTTP call at all**
//!    (Review Focus 4). The caller decides whether that means "keep playing
//!    on reflexes" or "freeze" (`on_exhaust`, D24).
//!
//! Time is measured in sim ticks, not wall clock, so ordering decisions are
//! reproducible in tests. The queue is not part of the deterministic sim
//! (model answers enter the sim only as logged decisions), but keeping it
//! tick-based makes its behaviour easy to reason about and to test.

use std::collections::BTreeMap;

use lm_core::Tick;

use crate::mind::Pack;

/// One agent asking to think about its current situation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThinkReq {
    /// The agent asking.
    pub agent: u16,
    /// Its squad (for squad-weighted priority).
    pub squad: u8,
    /// Which pack this request carries (D70).
    ///
    /// The move pack and the talk pack are separate requests, so a mind may
    /// have one of each waiting.
    pub pack: Pack,
    /// The tick it asked on.
    pub asked: Tick,
    /// The last tick on which an answer is still useful. A request still
    /// queued after this tick is dropped without being sent.
    pub deadline: Tick,
    /// The situation key of the facts it will send (D25), for logging and
    /// later reflex promotion.
    pub situation_key: u64,
}

/// What one `pop_batch` call decided.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Popped {
    /// Requests to send to the model now, in priority order.
    pub send: Vec<ThinkReq>,
    /// Requests refused because the agent's budget is spent. No call is made.
    pub refused_budget: Vec<ThinkReq>,
    /// Requests dropped because their deadline passed while queued.
    pub expired: Vec<ThinkReq>,
}

/// Per-agent call budgets for one round.
///
/// Real provider limits live in `lm-router` (spec §5); these are the game's
/// allotments, the numbers agents can see and plan around.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Budgets {
    calls_left: BTreeMap<u16, u32>,
}

impl Budgets {
    /// Every listed agent starts the round with `calls` model calls.
    pub fn uniform(agents: impl IntoIterator<Item = u16>, calls: u32) -> Self {
        Self {
            calls_left: agents.into_iter().map(|a| (a, calls)).collect(),
        }
    }

    /// Calls left for `agent` (an unknown agent has none).
    pub fn left(&self, agent: u16) -> u32 {
        self.calls_left.get(&agent).copied().unwrap_or(0)
    }

    /// Spend one call. `false` (and nothing spent) when the budget is empty.
    pub fn try_spend(&mut self, agent: u16) -> bool {
        match self.calls_left.get_mut(&agent) {
            Some(left) if *left > 0 => {
                *left -= 1;
                true
            }
            _ => false,
        }
    }
}

/// The least-use-first queue.
#[derive(Clone, Debug)]
pub struct LeastUseQueue {
    /// Pending requests, at most one per agent and pack (rule 1).
    pending: BTreeMap<(u16, Pack), ThinkReq>,
    /// Model usage this round per agent, in the configured unit (calls by
    /// default; tokens or GPU-ms once the router reports them).
    usage: BTreeMap<u16, u64>,
    /// Squad weight in per-mille: how much an agent's priority also counts
    /// its squad's total usage. 500 = 0.5 (the spec default).
    squad_weight_permille: u64,
}

impl LeastUseQueue {
    /// A new queue. `squad_weight` is the spec's `queue.squad_weight`
    /// (default 0.5), passed in per-mille so the ordering stays integer.
    pub fn new(squad_weight_permille: u64) -> Self {
        Self {
            pending: BTreeMap::new(),
            usage: BTreeMap::new(),
            squad_weight_permille,
        }
    }

    /// Queue a request. It replaces any request the same agent still has
    /// pending for the same pack (rule 1).
    ///
    /// The two packs are queued apart (D70): a mind may have a move pack
    /// and a talk pack waiting at once.
    pub fn push(&mut self, req: ThinkReq) {
        self.pending.insert((req.agent, req.pack), req);
    }

    /// Requests waiting.
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// True when nothing is waiting.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Record model usage after an answer (or a failed call that still cost
    /// time). `units` is in the queue's usage unit.
    pub fn record(&mut self, agent: u16, units: u64) {
        let u = self.usage.entry(agent).or_insert(0);
        *u = u.saturating_add(units);
    }

    /// This round's usage for `agent`.
    pub fn usage(&self, agent: u16) -> u64 {
        self.usage.get(&agent).copied().unwrap_or(0)
    }

    /// Forget all usage, at the start of a round.
    pub fn new_round(&mut self) {
        self.usage.clear();
        self.pending.clear();
    }

    /// Take up to `max` requests to send at tick `now`.
    ///
    /// - Expired requests (`deadline < now`) are dropped (rule 2).
    /// - The rest are ordered by priority, ascending:
    ///   `own_usage * 1000 + squad_weight_permille * squad_usage`,
    ///   ties broken by agent id and then by pack: the move pack goes
    ///   before the talk pack of the same agent, because the move is the
    ///   decision that moves the game (D70). The least-used agent goes
    ///   first.
    /// - Each chosen request spends one budget call. An empty budget refuses
    ///   the request instead (rule 3), and refusals don't use up a slot.
    pub fn pop_batch(&mut self, now: Tick, max: usize, budgets: &mut Budgets) -> Popped {
        let mut out = Popped::default();

        let (live, expired): (Vec<ThinkReq>, Vec<ThinkReq>) = std::mem::take(&mut self.pending)
            .into_values()
            .partition(|r| r.deadline >= now);
        out.expired = expired;

        // Squad totals, for the squad-weighted term.
        let mut squad_usage: BTreeMap<u8, u64> = BTreeMap::new();
        for r in &live {
            squad_usage.entry(r.squad).or_insert(0);
        }
        for (agent, used) in &self.usage {
            if let Some(r) = live.iter().find(|r| r.agent == *agent) {
                *squad_usage.entry(r.squad).or_insert(0) += used;
            }
        }

        let mut ranked: Vec<(u64, u16, Pack, ThinkReq)> = live
            .into_iter()
            .map(|r| {
                let own = self.usage(r.agent).saturating_mul(1000);
                let squad = squad_usage
                    .get(&r.squad)
                    .copied()
                    .unwrap_or(0)
                    .saturating_mul(self.squad_weight_permille);
                (own.saturating_add(squad), r.agent, r.pack, r)
            })
            .collect();
        // (priority, agent id, pack) is unique per request: a total order.
        ranked.sort_by_key(|(p, a, pack, _)| (*p, *a, *pack));

        for (_, _, _, r) in ranked {
            if out.send.len() >= max {
                // Not this time: keep it pending for the next batch.
                self.pending.insert((r.agent, r.pack), r);
            } else if budgets.try_spend(r.agent) {
                out.send.push(r);
            } else {
                out.refused_budget.push(r);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(agent: u16, squad: u8, asked: Tick, deadline: Tick) -> ThinkReq {
        ThinkReq {
            agent,
            squad,
            pack: Pack::Move,
            asked,
            deadline,
            situation_key: u64::from(agent),
        }
    }

    fn rich(agents: u16) -> Budgets {
        Budgets::uniform(0..agents, 1_000)
    }

    #[test]
    fn least_used_goes_first() {
        let mut q = LeastUseQueue::new(0);
        q.record(0, 5);
        q.record(1, 1);
        for a in 0..3 {
            q.push(req(a, 0, 10, 20));
        }
        let got: Vec<u16> = q
            .pop_batch(10, 3, &mut rich(3))
            .send
            .iter()
            .map(|r| r.agent)
            .collect();
        assert_eq!(got, vec![2, 1, 0]);
    }

    #[test]
    fn newer_request_replaces_older() {
        let mut q = LeastUseQueue::new(0);
        q.push(req(0, 0, 10, 20));
        let mut newer = req(0, 0, 12, 22);
        newer.situation_key = 99;
        q.push(newer.clone());
        assert_eq!(q.len(), 1);
        assert_eq!(q.pop_batch(12, 8, &mut rich(1)).send, vec![newer]);
    }

    #[test]
    fn hog_does_not_starve_others() {
        // Agent 0 has hammered the model all round and keeps asking; agents
        // 1-3 ask once each. With one model slot per batch, 1-3 are all
        // served in the first three pops, before the hog. (Their order among
        // themselves is the squad weight's business: agent 1 shares the hog's
        // squad, so it goes after 2 and 3.)
        let mut q = LeastUseQueue::new(500);
        q.record(0, 50);
        let mut budgets = rich(4);
        let mut served = Vec::new();
        for tick in 0..4u64 {
            q.push(req(0, 0, tick, tick + 10)); // the hog asks every tick
            if tick == 0 {
                for a in 1..4 {
                    q.push(req(a, u8::from(a >= 2), 0, 10));
                }
            }
            for r in q.pop_batch(tick, 1, &mut budgets).send {
                served.push(r.agent);
                q.record(r.agent, 1);
            }
        }
        let mut first_three = served[..3].to_vec();
        first_three.sort_unstable();
        assert_eq!(first_three, vec![1, 2, 3], "served order {served:?}");
        assert_eq!(served[3], 0, "the hog only gets a turn after the others");
    }

    #[test]
    fn expired_requests_dropped() {
        let mut q = LeastUseQueue::new(0);
        q.push(req(0, 0, 0, 5));
        q.push(req(1, 0, 0, 50));
        let p = q.pop_batch(10, 8, &mut rich(2));
        assert_eq!(
            p.expired.iter().map(|r| r.agent).collect::<Vec<_>>(),
            vec![0]
        );
        assert_eq!(p.send.iter().map(|r| r.agent).collect::<Vec<_>>(), vec![1]);
    }

    #[test]
    fn exhausted_budget_makes_no_calls() {
        let mut q = LeastUseQueue::new(0);
        let mut budgets = Budgets::uniform([0u16], 2);
        let mut calls = 0;
        for tick in 0..5u64 {
            q.push(req(0, 0, tick, tick + 5));
            let p = q.pop_batch(tick, 4, &mut budgets);
            calls += p.send.len();
            if tick >= 2 {
                assert_eq!(p.refused_budget.len(), 1, "tick {tick}: refused, not sent");
            }
        }
        assert_eq!(calls, 2, "only the budgeted calls are made");
        assert_eq!(budgets.left(0), 0);
    }

    #[test]
    fn batch_cap_keeps_the_rest_pending() {
        let mut q = LeastUseQueue::new(0);
        for a in 0..5 {
            q.push(req(a, 0, 0, 100));
        }
        let p = q.pop_batch(0, 2, &mut rich(5));
        assert_eq!(p.send.len(), 2);
        assert_eq!(q.len(), 3, "the rest wait for the next batch");
    }

    #[test]
    fn no_backlog_after_outage() {
        // During an outage nothing is popped, but agents keep asking. The
        // queue never holds more than one request per agent.
        let mut q = LeastUseQueue::new(0);
        for tick in 0..300u64 {
            for a in 0..8 {
                q.push(req(a, 0, tick, tick + 15));
            }
        }
        assert_eq!(q.len(), 8);
    }

    #[test]
    fn squad_weight_breaks_ties_toward_the_quieter_squad() {
        // Agents 0 (squad 0) and 1 (squad 1) are equally unused, but squad 0's
        // other member (agent 2) has used the model a lot. With weight 0.5,
        // agent 1 goes first.
        let mut q = LeastUseQueue::new(500);
        q.record(2, 40);
        q.push(req(0, 0, 0, 10));
        q.push(req(1, 1, 0, 10));
        q.push(req(2, 0, 0, 10));
        let got: Vec<u16> = q
            .pop_batch(0, 3, &mut rich(3))
            .send
            .iter()
            .map(|r| r.agent)
            .collect();
        assert_eq!(got[0], 1);
    }
}
