//! The piano roll: one lane per agent, its decisions and what happened to it.
//!
//! The piano roll is the M1 debugger (spec §6): a timeline lane per agent
//! showing what it did and what the world did to it. It is built from two
//! sources, and it needs both:
//!
//! * the **round log** holds the decisions that were applied, with the tick
//!   they were applied on and how late they were. Those are the *marks* on a
//!   lane: one per decision, on the lane of the agent the decision speaks
//!   for.
//! * the **events** of the round hold what the rules did: a tag, a respawn, a
//!   beacon taken or lost. Those are the *spans* on a lane: the stun an agent
//!   was in, the beacon its squad held, the intent it was walking.
//!
//! The host keeps the events as it plays ([`lm_host_model::host_loop::TickOut`]
//! carries them); the replay viewer keeps the events of the round it opened.
//! Either way the roll is built the same way.
//!
//! ## Nothing here decides anything
//!
//! The roll is a picture of the round, drawn from the log and the events. It
//! never touches the simulation and never reaches back into it: the lanes are
//! what happened, not an opinion about what should happen.
//!
//! ## The window
//!
//! A piano roll is a window on a long round, so [`RollModel::from_log`]
//! takes the range of ticks on screen. Only the marks inside the window and
//! the spans that reach into it are kept. A span that is still open when the
//! round ended -- a stun that never was lifted, a beacon that was still held
//! -- runs to the last tick of the round, so the bar reaches the edge of the
//! window instead of stopping in mid-air.

use std::collections::BTreeMap;
use std::ops::Range;

use lm_core::decision::{Decision, Kind, Provenance, Stamped, Tick};
use lm_core::log::RoundLog;
use lm_core::rules::Event;

/// What a mark on a lane stands for: where the decision on it came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MarkKind {
    /// A decision a person made.
    Human,
    /// A decision a System-1 model made.
    System1,
    /// A decision a System-2 model made.
    System2,
    /// A decision a test injected.
    Test,
    /// A marker: a decision that changes nothing in the world.
    Marker,
}

impl MarkKind {
    /// The mark a decision leaves on the lane of the agent it speaks for.
    ///
    /// A marker decision is a marker whatever it came from -- it says
    /// nothing about the world -- and every other decision is marked by its
    /// provenance, so the roll shows at a glance who was playing.
    ///
    /// The reflex never travels: it runs inside the sim and never reaches a
    /// log (D8, D10). A decision that claims to be one is local all the same,
    /// so it is drawn like a test decision.
    pub fn of(d: &Decision) -> MarkKind {
        if matches!(d.kind, Kind::Marker(_)) {
            return MarkKind::Marker;
        }
        match &d.provenance {
            Provenance::Human => MarkKind::Human,
            Provenance::System1 { .. } => MarkKind::System1,
            Provenance::System2 { .. } => MarkKind::System2,
            Provenance::Test => MarkKind::Test,
            Provenance::Reflex => MarkKind::Test,
        }
    }
}

/// One decision, on the lane of the agent it speaks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mark {
    /// The tick the decision was applied on.
    pub tick: Tick,
    /// Where the decision came from.
    pub kind: MarkKind,
    /// How late it was, in ticks; 0 when it was on time.
    pub late_by: u16,
}

/// What a span on a lane stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanKind {
    /// The agent is stunned: it holds still until it respawns.
    Stunned,
    /// The agent's squad holds this beacon.
    HoldingBeacon(u8),
    /// The agent is walking the plan of an intent.
    OnIntent,
    /// The agent is walking the safe branch of an intent.
    OnSafeBranch,
}

/// A run of ticks on a lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    /// The first tick of the run.
    pub from: Tick,
    /// The last tick of the run.
    pub to: Tick,
    /// What the run is.
    pub what: SpanKind,
}

/// One agent's lane: the decisions it was given and the runs it went through.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lane {
    /// The agent the lane belongs to.
    pub agent: u16,
    /// The decisions applied for the agent, in tick order.
    pub marks: Vec<Mark>,
    /// The runs the agent went through, in tick order.
    pub spans: Vec<Span>,
}

/// The piano roll: one lane per agent of the round.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RollModel {
    /// The lanes, in agent-id order.
    pub lanes: Vec<Lane>,
}

impl RollModel {
    /// Build the roll of a round from its log and its events.
    ///
    /// `log` holds the decisions that were applied, `events` the events of
    /// the round with their ticks, and `window` the range of ticks on screen.
    /// The lanes are the agents of the log's roster, in agent-id order,
    /// whether or not anything happened on them.
    pub fn from_log(log: &RoundLog, events: &[(Tick, Event)], window: Range<Tick>) -> RollModel {
        let mut lanes = lanes_of(log);
        let index = index_of(&lanes);

        // The decisions: a mark per decision, on the lane of the agent it
        // speaks for. A decision of the system speaks for nobody, so it has
        // no lane of its own.
        for stamped in decisions_of(log) {
            let Some(agent) = stamped.d.source.agent() else {
                continue;
            };
            let Some(&lane) = index.get(&agent) else {
                continue;
            };
            lanes[lane].marks.push(Mark {
                tick: stamped.tick,
                kind: MarkKind::of(&stamped.d),
                late_by: stamped.late_by,
            });
        }

        // The events, in tick order: a tag and a respawn make a stun, a
        // beacon taken and lost make a hold.
        let mut events = events.to_vec();
        events.sort_by_key(|(tick, _)| *tick);

        let mut stuns: BTreeMap<u16, Tick> = BTreeMap::new();
        let mut holds: BTreeMap<u8, (Tick, u8)> = BTreeMap::new();
        for (tick, event) in &events {
            match event {
                Event::Tagged { target, .. } => {
                    // The first tag is the one that stuns: an agent that is
                    // already down is not tagged again.
                    stuns.entry(*target).or_insert(*tick);
                }
                Event::Respawned(agent) => {
                    if let Some(from) = stuns.remove(agent) {
                        push_span(
                            &mut lanes,
                            &index,
                            *agent,
                            Span {
                                from,
                                to: *tick,
                                what: SpanKind::Stunned,
                            },
                        );
                    }
                }
                Event::BeaconHeld { beacon, squad } => {
                    holds.insert(*beacon, (*tick, *squad));
                }
                Event::BeaconLost { beacon } => {
                    if let Some((from, squad)) = holds.remove(beacon) {
                        for agent in squad_agents(log, squad) {
                            push_span(
                                &mut lanes,
                                &index,
                                agent,
                                Span {
                                    from,
                                    to: *tick,
                                    what: SpanKind::HoldingBeacon(*beacon),
                                },
                            );
                        }
                    }
                }
                // A marker reaches the world, and the round ending is not
                // something an agent goes through.
                Event::Marker(_) | Event::RoundOver(_) => {}
            }
        }

        // Whatever was still open when the round ended runs to its last tick.
        let end = last_tick_of(log);
        for (agent, from) in stuns {
            push_span(
                &mut lanes,
                &index,
                agent,
                Span {
                    from,
                    to: end,
                    what: SpanKind::Stunned,
                },
            );
        }
        for (beacon, (from, squad)) in holds {
            for agent in squad_agents(log, squad) {
                push_span(
                    &mut lanes,
                    &index,
                    agent,
                    Span {
                        from,
                        to: end,
                        what: SpanKind::HoldingBeacon(beacon),
                    },
                );
            }
        }

        // The intents: an agent that is committed walks the plan of its
        // intent until the TTL runs out, and its safe branch after that.
        for (agent, intents) in intents_of(log) {
            let Some(&lane) = index.get(&agent) else {
                continue;
            };
            for (position, (tick, ttl)) in intents.iter().enumerate() {
                let plan_end = tick.saturating_add(u64::from(*ttl));
                let next = intents.get(position + 1).map(|(tick, _)| *tick);

                // The plan, until the TTL ends or a newer intent takes over.
                let on_intent_end = next.map_or(plan_end, |next| plan_end.min(next));
                if *tick <= on_intent_end {
                    lanes[lane].spans.push(Span {
                        from: *tick,
                        to: on_intent_end,
                        what: SpanKind::OnIntent,
                    });
                }

                // The safe branch, until a newer intent takes over.
                let safe_end = next.unwrap_or(end);
                if plan_end < safe_end {
                    lanes[lane].spans.push(Span {
                        from: plan_end,
                        to: safe_end,
                        what: SpanKind::OnSafeBranch,
                    });
                }
            }
        }

        // The lanes are read from the top down, so they are sorted.
        for lane in &mut lanes {
            lane.marks.sort_by_key(|mark| mark.tick);
            lane.spans.sort_by_key(|span| (span.from, span.to));
        }

        // Only what reaches into the window is kept.
        RollModel {
            lanes: lanes
                .into_iter()
                .map(|mut lane| {
                    lane.marks.retain(|mark| window.contains(&mark.tick));
                    lane.spans
                        .retain(|span| span.from < window.end && span.to >= window.start);
                    lane
                })
                .collect(),
        }
    }

    /// The lane of an agent, if the round has one.
    pub fn lane(&self, agent: u16) -> Option<&Lane> {
        self.lanes.iter().find(|lane| lane.agent == agent)
    }
}

/// Put a span on the lane of an agent.
fn push_span(lanes: &mut [Lane], index: &BTreeMap<u16, usize>, agent: u16, span: Span) {
    if let Some(&lane) = index.get(&agent) {
        lanes[lane].spans.push(span);
    }
}

/// The lanes of the round: one per agent of the roster, in agent-id order.
fn lanes_of(log: &RoundLog) -> Vec<Lane> {
    let mut agents: Vec<u16> = log.header.roster.iter().map(|entry| entry.agent).collect();
    agents.sort_unstable();
    agents.dedup();
    agents
        .into_iter()
        .map(|agent| Lane {
            agent,
            marks: Vec::new(),
            spans: Vec::new(),
        })
        .collect()
}

/// Where the lane of an agent is.
fn index_of(lanes: &[Lane]) -> BTreeMap<u16, usize> {
    lanes
        .iter()
        .enumerate()
        .map(|(index, lane)| (lane.agent, index))
        .collect()
}

/// Every decision of the log, in tick order.
fn decisions_of(log: &RoundLog) -> Vec<&Stamped> {
    let mut out: Vec<&Stamped> = Vec::new();
    for segment in &log.segments {
        out.extend(segment.decisions.iter());
    }
    out
}

/// The intents of every agent, in tick order: the tick and the TTL of each.
fn intents_of(log: &RoundLog) -> BTreeMap<u16, Vec<(Tick, u16)>> {
    let mut out: BTreeMap<u16, Vec<(Tick, u16)>> = BTreeMap::new();
    for stamped in decisions_of(log) {
        let Some(agent) = stamped.d.source.agent() else {
            continue;
        };
        let Kind::Intent(intent) = &stamped.d.kind else {
            continue;
        };
        out.entry(agent)
            .or_default()
            .push((stamped.tick, intent.ttl_ticks));
    }
    out
}

/// The agents of a squad, in agent-id order.
fn squad_agents(log: &RoundLog, squad: u8) -> Vec<u16> {
    let mut agents: Vec<u16> = log
        .header
        .roster
        .iter()
        .filter(|entry| entry.squad == squad)
        .map(|entry| entry.agent)
        .collect();
    agents.sort_unstable();
    agents
}

/// The last tick of the round.
fn last_tick_of(log: &RoundLog) -> Tick {
    log.segments
        .last()
        .map_or(log.header.start_tick, |segment| segment.last_tick)
}
