//! Round metrics: what actually happened, measured by replaying the log
//! (phase-5 Task 1).
//!
//! [`RoundMetrics::from_log`] re-runs the round from the header in the log,
//! feeding the sim the decisions the log recorded — the same walk
//! `lm-tools`' `replay` does — and counts what it sees: the scores, the tags,
//! the beacon holds, the provenance of every decision, the lateness the
//! stamps admit, and the model-vs-code dial per agent (D25/D40). It also
//! picks out **moments**: the ticks where the score swung or tags clustered,
//! which is what the between-round reviewer is shown.
//!
//! Everything is integer maths: every rate is a share x1000 (per mil), and a
//! metric over a round with no events is a defined value, never a division by
//! zero (Review Focus 5: `metrics_on_empty_round`).
//!
//! The log is taken as it was read: [`RoundLog::read`](lm_core::log::RoundLog)
//! verifies the hash chain before a log is handed back, so metrics re-measure
//! a round that already proved itself byte-exact.

use std::collections::{BTreeMap, VecDeque};

use lm_core::decision::{Provenance, Tick};
use lm_core::header::Header;
use lm_core::log::RoundLog;
use lm_core::rules::Event;
use lm_core::sim::Sim;
use serde::{Deserialize, Serialize};

/// What one round did, per squad, per agent, per provenance.
///
/// Built by [`RoundMetrics::from_log`]; every count covers exactly the ticks
/// the log recorded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoundMetrics {
    /// The ticks the replay ran: one per recorded tick of the log.
    pub ticks: u64,
    /// The final score of every squad, in squad order.
    pub scores: Vec<i64>,
    /// Tags landed by each squad, in squad order.
    pub tags_for: Vec<u64>,
    /// Tags taken by each squad, in squad order.
    pub tags_against: Vec<u64>,
    /// The ticks each squad held at least one beacon, in squad order.
    pub beacon_hold_ticks: Vec<u64>,
    /// Decision counts by provenance, the reflex implied (D8/D10).
    pub decisions: ProvenanceCounts,
    /// On-time, late and stale decisions, from `Stamped::late_by`.
    pub timeliness: Timeliness,
    /// Refusals seen in the replay.
    pub refusals: Refusals,
    /// The model-vs-code dial per agent, in agent-id order (D25/D40).
    pub dial: BTreeMap<u16, Dial>,
    /// The ticks worth a reviewer's attention, in tick order.
    pub moments: Vec<Moment>,
}

/// Logged decisions by where they came from (D25, D37).
///
/// `reflex` counts the reflex's decisions, which the log never carries — a
/// reflex runs inside the sim, never travels, never logs (D8/D10) — so they
/// are implied instead: one per agent-tick the reflex drove, plus any
/// `Reflex`-provenance decisions a crafted log carries (a real one carries
/// none).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProvenanceCounts {
    /// Decisions a human made.
    pub human: u64,
    /// Decisions from a System-1 model (local, fast).
    pub system1: u64,
    /// Decisions from a System-2 model (cloud).
    pub system2: u64,
    /// Decisions injected by a test.
    pub test: u64,
    /// Reflex decisions: implied, one per reflex-driven agent-tick.
    pub reflex: u64,
}

impl ProvenanceCounts {
    /// Every decision, logged or implied.
    pub fn total(&self) -> u64 {
        self.human + self.system1 + self.system2 + self.test + self.reflex
    }

    /// Count one decision by its provenance.
    fn count(&mut self, p: &Provenance) {
        match p {
            Provenance::Human => self.human += 1,
            Provenance::System1 { .. } => self.system1 += 1,
            Provenance::System2 { .. } => self.system2 += 1,
            Provenance::Test => self.test += 1,
            Provenance::Reflex => self.reflex += 1,
        }
    }
}

/// What happened to the decisions the log recorded, by their own stamps.
///
/// `late_by` is the sequencer's own accounting, so these are the stamps the
/// round itself left: on time (`late_by == 0`), late but inside the stale
/// window, or later than the window (which a real sequencer drops; a crafted
/// log can still say it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timeliness {
    /// Decisions stamped on the tick they asked for.
    pub on_time: u64,
    /// Decisions stamped on their arrival tick instead.
    pub late: u64,
    /// Decisions later than `stale_window_ticks`.
    pub stale: u64,
}

/// Refusals the round produced, recovered from the replay.
///
/// M1's log carries no refusal records of its own; the replayed sim counts
/// what its reflexes refused, which is what the log implies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refusals {
    /// Reflex calls that refused to answer (D26: a refusal is an outcome).
    pub refused: u64,
    /// Reflex calls that ran out of script fuel.
    pub out_of_fuel: u64,
    /// Reflex calls that failed.
    pub error: u64,
}

/// The model-vs-code dial of one agent over the round (D25, D40).
///
/// An agent's tick is **model-driven** when a logged decision's effect was in
/// force on it — a pinned action, a live intent, or a lane/roll command — and
/// **reflex-driven** otherwise, which is when the sim asked the agent's
/// reflex. The dial is the share of model-driven ticks, reported, not
/// maximised (D40); the distance to a role's ⚙ `dial.target` is the caller's
/// to compute against the target it configures.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dial {
    /// Ticks a logged decision drove the agent.
    pub model_ticks: u64,
    /// Ticks the agent's reflex answered for.
    pub reflex_ticks: u64,
    /// The model share x1000; 0 for an agent that never played.
    pub model_share_x1000: u64,
}

impl Dial {
    /// The reflex share x1000: the other end of the dial (D25).
    pub fn reflex_share_x1000(&self) -> u64 {
        1000 - self.model_share_x1000
    }
}

/// A tick a reviewer should look at, and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Moment {
    /// The tick it happened on.
    pub tick: Tick,
    /// What made it a moment.
    pub kind: MomentKind,
}

/// The shape of a moment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MomentKind {
    /// A squad's score moved by more than the threshold over the trailing
    /// window. The delta is signed and absolute: a collapse counts the same
    /// as a run.
    ScoreSwing {
        /// The squad whose score swung.
        squad: u8,
        /// The score change over the window, signed.
        delta: i64,
    },
    /// Tags landed in one window: `tags` of them within `tag_window_ticks`.
    TagCluster {
        /// How many tags the window held when it became a cluster.
        tags: u64,
    },
}

/// What makes a tick a moment; thresholds, with the defaults the round review
/// starts from (D19: a value with more than one workable choice is a
/// parameter).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MomentThresholds {
    /// The trailing ticks a score swing is measured over.
    pub swing_window_ticks: u64,
    /// The smallest score change over the window that counts as a swing.
    pub swing_min_delta: i64,
    /// The trailing ticks tags are counted in.
    pub tag_window_ticks: u64,
    /// The fewest tags in the window that count as a cluster.
    pub tag_cluster_min: u64,
}

impl Default for MomentThresholds {
    fn default() -> Self {
        MomentThresholds {
            // Five seconds of play (150 ticks at 30 Hz).
            swing_window_ticks: 150,
            // Two seconds of one beacon (60 points at one per tick).
            swing_min_delta: 60,
            // One stun's length (the default 90 ticks).
            tag_window_ticks: 90,
            // Three trades inside one stun window.
            tag_cluster_min: 3,
        }
    }
}

impl RoundMetrics {
    /// Measure a round by replaying its log, with the default thresholds.
    pub fn from_log(log: &RoundLog) -> RoundMetrics {
        Self::from_log_with(log, &MomentThresholds::default())
    }

    /// Measure a round by replaying its log, with the given thresholds.
    ///
    /// The round is re-run from the log's header and its own stamped
    /// decisions — the same tick-by-tick walk `lm-tools`' `replay` does —
    /// and everything is counted from what the replay shows. A header that
    /// cannot be played at all yields the defined empty metrics for its
    /// shape: no panic, no division by zero.
    pub fn from_log_with(log: &RoundLog, thresholds: &MomentThresholds) -> RoundMetrics {
        let header = &log.header;
        let squads = squad_count(header);
        let mut metrics = RoundMetrics {
            ticks: 0,
            scores: vec![0; squads],
            tags_for: vec![0; squads],
            tags_against: vec![0; squads],
            beacon_hold_ticks: vec![0; squads],
            decisions: ProvenanceCounts::default(),
            timeliness: Timeliness::default(),
            refusals: Refusals::default(),
            dial: dial_of(header),
            moments: Vec::new(),
        };
        let Ok(mut sim) = Sim::new(header) else {
            return metrics;
        };

        // Which squad every agent plays for, and how late a stamp may be
        // before it counts as stale.
        let agent_squad: BTreeMap<u16, u8> = header
            .roster
            .iter()
            .map(|entry| (entry.agent, entry.squad))
            .collect();
        let stale_window = u64::from(header.params.stale_window_ticks);

        // The trailing state the moments are read against.
        let initial: Vec<i64> = sim.scores().to_vec();
        let mut history: Vec<Vec<i64>> = Vec::new();
        let mut prev_swing: Vec<i64> = vec![0; squads];
        let mut tag_window: VecDeque<Tick> = VecDeque::new();
        let mut prev_tags: u64 = 0;
        let mut holders: BTreeMap<u8, u8> = BTreeMap::new();

        for segment in &log.segments {
            for tick in segment.first_tick..=segment.last_tick {
                let stamped = log.decisions_for(tick);

                // The decisions of this tick, by where they came from and by
                // what their own stamps say about lateness.
                for s in stamped {
                    metrics.decisions.count(&s.d.provenance);
                    let late_by = u64::from(s.late_by);
                    if late_by == 0 {
                        metrics.timeliness.on_time += 1;
                    } else if late_by <= stale_window {
                        metrics.timeliness.late += 1;
                    } else {
                        metrics.timeliness.stale += 1;
                    }
                }

                let out = sim.step(stamped);
                metrics.ticks += 1;

                for event in &out.events {
                    match event {
                        Event::Tagged { by, target } => {
                            if let Some(squad) = agent_squad.get(by) {
                                metrics.tags_for[usize::from(*squad)] += 1;
                            }
                            if let Some(squad) = agent_squad.get(target) {
                                metrics.tags_against[usize::from(*squad)] += 1;
                            }
                            tag_window.push_back(tick);
                        }
                        Event::BeaconHeld { beacon, squad } => {
                            holders.insert(*beacon, *squad);
                        }
                        Event::BeaconLost { beacon } => {
                            holders.remove(beacon);
                        }
                        _ => {}
                    }
                }
                // Every beacon still held after the tick scored a point for
                // its holder on it.
                for squad in holders.values() {
                    metrics.beacon_hold_ticks[usize::from(*squad)] += 1;
                }

                // The dial: a decision in force drove the agent this tick,
                // or its reflex answered for it (D25/D40).
                for (agent, dial) in &mut metrics.dial {
                    let driven = sim.agent(*agent).is_some_and(|a| {
                        a.held.is_some() || a.intent.is_some() || a.lane_cmd.is_some()
                    });
                    if driven {
                        dial.model_ticks += 1;
                    } else {
                        dial.reflex_ticks += 1;
                    }
                }

                // The scores, and the swing of each squad over the window.
                let now = sim.scores().to_vec();
                Self::find_swing(
                    &mut metrics,
                    tick,
                    &now,
                    &history,
                    &initial,
                    &mut prev_swing,
                    thresholds,
                );
                history.push(now);

                // Tags slide out of the window; what is left may have just
                // become a cluster.
                while tag_window
                    .front()
                    .is_some_and(|&at| at.saturating_add(thresholds.tag_window_ticks) <= tick)
                {
                    tag_window.pop_front();
                }
                let tags = tag_window.len() as u64;
                if tags >= thresholds.tag_cluster_min && prev_tags < thresholds.tag_cluster_min {
                    metrics.moments.push(Moment {
                        tick,
                        kind: MomentKind::TagCluster { tags },
                    });
                }
                prev_tags = tags;
            }
        }

        metrics.scores[..sim.scores().len()].copy_from_slice(sim.scores());
        for dial in metrics.dial.values_mut() {
            let played = dial.model_ticks + dial.reflex_ticks;
            // An agent that never played sits at 0, not at a division by zero.
            dial.model_share_x1000 = (dial.model_ticks * 1000).checked_div(played).unwrap_or(0);
        }
        let stats = sim.stats();
        metrics.refusals = Refusals {
            refused: stats.reflex_refused,
            out_of_fuel: stats.reflex_out_of_fuel,
            error: stats.reflex_error,
        };
        // The reflex never logs decisions; its count is implied by the ticks
        // it drove (D8/D10).
        metrics.decisions.reflex += metrics
            .dial
            .values()
            .map(|dial| dial.reflex_ticks)
            .sum::<u64>();
        metrics
    }

    /// Mark a score-swing moment on its rising edge: the tick a squad's
    /// window delta first exceeds the threshold.
    fn find_swing(
        metrics: &mut RoundMetrics,
        tick: Tick,
        now: &[i64],
        history: &[Vec<i64>],
        initial: &[i64],
        prev_swing: &mut [i64],
        thresholds: &MomentThresholds,
    ) {
        let window = thresholds.swing_window_ticks as usize;
        if window == 0 {
            return;
        }
        // The scores `window` ticks ago, or the round's start when fewer
        // than a window has been played.
        let base: &[i64] = match history.len().checked_sub(window) {
            Some(at) => &history[at],
            None => initial,
        };
        for (squad, &score) in now.iter().enumerate() {
            let delta = score - base.get(squad).copied().unwrap_or(0);
            let swing = delta.abs();
            let prev = prev_swing.get(squad).copied().unwrap_or(0);
            if swing > thresholds.swing_min_delta && prev <= thresholds.swing_min_delta {
                metrics.moments.push(Moment {
                    tick,
                    kind: MomentKind::ScoreSwing {
                        squad: u8::try_from(squad).unwrap_or(u8::MAX),
                        delta,
                    },
                });
            }
            if let Some(slot) = prev_swing.get_mut(squad) {
                *slot = swing;
            }
        }
    }
}

/// How many squads the metrics carry a column for: the parameters' count and
/// the roster's, whichever is more (the same shape the sim sizes its
/// knowledge by).
fn squad_count(h: &Header) -> usize {
    let roster = h
        .roster
        .iter()
        .map(|entry| usize::from(entry.squad) + 1)
        .max()
        .unwrap_or(0);
    usize::from(h.params.squads).max(roster)
}

/// One zeroed dial per agent of the roster, in agent-id order.
fn dial_of(h: &Header) -> BTreeMap<u16, Dial> {
    let mut dial = BTreeMap::new();
    for entry in &h.roster {
        dial.insert(entry.agent, Dial::default());
    }
    dial
}

#[cfg(test)]
mod tests {
    use super::*;
    use lm_core::decision::{Action, Decision, Kind, Lane, Source, Stamped};
    use lm_core::header::Header;
    use lm_core::log::Recorder;
    use lm_core::view::menu;

    /// The reflex the tests play: the shipped one, as `lm run` does.
    const REFLEX: &str = include_str!("../../../assets/reflexes/default.rhai");

    /// A decision of `agent` for `tick`, stamped as the sequencer would.
    fn stamped(agent: u16, tick: Tick, seq: u32, kind: Kind, p: Provenance) -> Stamped {
        Stamped {
            d: Decision {
                apply_tick: tick,
                source: Source::Agent(agent),
                seq,
                kind,
                provenance: p,
            },
            arrived: tick,
            tick,
            late_by: 0,
        }
    }

    /// A log of `ticks` ticks, one segment per 300, with `applied` deciding
    /// the decisions of each tick.
    fn recorded(h: Header, ticks: u64, applied: impl Fn(Tick) -> Vec<Stamped>) -> RoundLog {
        let mut rec = Recorder::new(h, 300);
        for t in 0..ticks {
            let decisions = applied(t);
            rec.record(t, &decisions, None);
        }
        rec.finish()
    }

    /// A brawl: both squads pinned to the one beacon of a one-chunk world,
    /// every tick, until they meet and trade tags.
    fn melee_log() -> RoundLog {
        let mut h = Header::default_round(1, REFLEX);
        h.params.world_chunks = (1, 1);
        h.params.beacons = 1;
        // Short stuns, so the tagged walk back and the tags keep landing.
        h.params.stun_ticks = 2;
        let beacon = Sim::new(&h).expect("the round starts").world().beacons[0];
        recorded(h, 800, |t| {
            (0..8)
                .map(|agent| {
                    stamped(
                        agent,
                        t,
                        u32::try_from(t).unwrap() + 1,
                        Kind::ActionOn(Action::GoTo(beacon)),
                        Provenance::Test,
                    )
                })
                .collect()
        })
    }

    #[test]
    fn metrics_on_empty_round() {
        // A log with no segments: the round was never played.
        let empty = Recorder::new(Header::default_round(7, REFLEX), 300).finish();
        let m = RoundMetrics::from_log(&empty);
        assert_eq!(m.ticks, 0);
        assert_eq!(m.scores, vec![0, 0]);
        assert_eq!(m.tags_for, vec![0, 0]);
        assert_eq!(m.tags_against, vec![0, 0]);
        assert_eq!(m.beacon_hold_ticks, vec![0, 0]);
        assert_eq!(m.decisions, ProvenanceCounts::default());
        assert_eq!(m.timeliness, Timeliness::default());
        assert_eq!(m.refusals, Refusals::default());
        assert!(m.moments.is_empty());
        assert_eq!(m.dial.len(), 8);
        for dial in m.dial.values() {
            // Defined values, not a division by zero: an agent that never
            // played sits at 0 on the dial.
            assert_eq!((dial.model_ticks, dial.reflex_ticks), (0, 0));
            assert_eq!(dial.model_share_x1000, 0);
        }

        // A round whose header cannot be played at all is just as empty.
        let mut h = Header::default_round(7, REFLEX);
        h.ruleset = "capture-the-flag".to_owned();
        let unplayable = recorded(h, 100, |_| Vec::new());
        let m = RoundMetrics::from_log(&unplayable);
        assert_eq!(m.ticks, 0);
        assert_eq!(m.scores, vec![0, 0]);
        assert!(m.moments.is_empty());
    }

    #[test]
    fn dial_counts_provenance() {
        let h = Header::default_round(1, REFLEX);
        // A lane decision from a System-1 model, arriving on time for tick 5.
        let probe = Sim::new(&h).expect("the round starts");
        let menu = menu(&probe.view(0));
        let lane = Kind::Lane(Lane {
            gate: 0,
            menu_id: menu.id,
            values: vec![255; menu.options.len()],
        });
        let s1 = Provenance::System1 {
            model: "d1".to_owned(),
            tokens: 64,
            latency_ms: 30,
            situation_key: 9,
        };
        let log = recorded(h, 120, |t| {
            if t == 5 {
                vec![stamped(0, 5, 1, lane.clone(), s1.clone())]
            } else {
                Vec::new()
            }
        });

        let m = RoundMetrics::from_log(&log);
        assert_eq!(m.decisions.system1, 1);
        assert_eq!(m.decisions.test, 0);
        assert_eq!(m.decisions.human, 0);
        assert_eq!(m.decisions.system2, 0);
        assert_eq!(m.timeliness.on_time, 1);

        // The lane command persists, so agent 0 is model-driven from tick 5.
        let d0 = &m.dial[&0];
        assert_eq!(d0.model_ticks, 115);
        assert_eq!(d0.reflex_ticks, 5);
        assert_eq!(d0.model_share_x1000, 115 * 1000 / 120);
        assert_eq!(d0.reflex_share_x1000(), 1000 - 958);

        // Everybody else ran their reflex for the whole round.
        for (agent, dial) in &m.dial {
            if *agent != 0 {
                assert_eq!((dial.model_ticks, dial.reflex_ticks), (0, 120), "{agent}");
                assert_eq!(dial.model_share_x1000, 0);
            }
        }
        // The reflex's decisions are implied: one per reflex-driven tick.
        assert_eq!(m.decisions.reflex, 5 + 7 * 120);
    }

    #[test]
    fn moments_found_at_tag_clusters() {
        let log = melee_log();
        let m = RoundMetrics::from_log(&log);

        let tags: u64 = m.tags_for.iter().sum();
        assert!(tags >= 3, "the setup made no play: {tags} tags");
        let clusters = m
            .moments
            .iter()
            .filter(|x| matches!(x.kind, MomentKind::TagCluster { .. }))
            .count();
        assert!(
            clusters >= 1,
            "no tag-cluster moment in a round of {tags} tags"
        );
        let min = MomentThresholds::default().tag_cluster_min;
        for moment in &m.moments {
            if let MomentKind::TagCluster { tags } = moment.kind {
                assert!(tags >= min, "a cluster below the threshold: {moment:?}");
                assert!(moment.tick < 800);
            }
        }

        // The beacons ruleset scores exactly one point per beacon held per
        // tick, so the hold ticks and the scores are the same total.
        assert_eq!(
            m.beacon_hold_ticks.iter().sum::<u64>(),
            m.scores.iter().sum::<i64>() as u64
        );
        // Every tag landed by one squad was taken by the other.
        assert_eq!(m.tags_for[0], m.tags_against[1]);
        assert_eq!(m.tags_for[1], m.tags_against[0]);
    }

    #[test]
    fn metrics_deterministic() {
        let log = melee_log();
        assert_eq!(RoundMetrics::from_log(&log), RoundMetrics::from_log(&log));

        // Tuned thresholds measure just as deterministically.
        let th = MomentThresholds {
            tag_cluster_min: 2,
            ..MomentThresholds::default()
        };
        assert_eq!(
            RoundMetrics::from_log_with(&log, &th),
            RoundMetrics::from_log_with(&log, &th)
        );
    }

    #[test]
    fn late_and_stale_read_from_late_by() {
        let h = Header::default_round(3, REFLEX);
        let hold = || Kind::ActionOn(Action::Hold);
        let on_time = stamped(0, 10, 1, hold(), Provenance::Test);
        // Late by two ticks: stamped on its arrival tick, still applied.
        let mut late = stamped(1, 10, 1, hold(), Provenance::Test);
        late.arrived = 12;
        late.tick = 12;
        late.late_by = 2;
        // Later than the stale window (15): a sequencer drops this; a
        // crafted log can still say it, and the stamp is what counts.
        let mut stale = stamped(2, 10, 1, hold(), Provenance::Test);
        stale.arrived = 40;
        stale.tick = 40;
        stale.late_by = 30;

        let log = recorded(h, 60, |t| match t {
            10 => vec![on_time.clone()],
            12 => vec![late.clone()],
            40 => vec![stale.clone()],
            _ => Vec::new(),
        });
        let m = RoundMetrics::from_log(&log);
        assert_eq!(
            m.timeliness,
            Timeliness {
                on_time: 1,
                late: 1,
                stale: 1
            }
        );
        assert_eq!(m.decisions.test, 3);
        // Each pin drove its agent from its own tick to the last one.
        assert_eq!(m.dial[&0].model_ticks, 50);
        assert_eq!(m.dial[&1].model_ticks, 48);
        assert_eq!(m.dial[&2].model_ticks, 20);
    }
}
