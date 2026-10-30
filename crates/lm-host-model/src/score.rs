//! The conductor's score: one system of staves per agent, on one clock.
//!
//! The piano roll ([`crate::roll`]) is a debugger: one lane per agent, the
//! decisions on it and what the world did to it. The score is the next
//! instrument (Vox Score §12, E7): each agent becomes a **system** in an
//! orchestral score — a small stack of staves that all read the same time
//! axis — so a reviewer can see not only *what* an agent did but *how it
//! performed*: what it said and how the line was meant, where its mind
//! gestured and where it hesitated, where its attention stood, and which
//! tier was playing the body at the time.
//!
//! The staves, and where each one's data comes from:
//!
//! | Staff | Drawn from | Lives in |
//! |---|---|---|
//! | **Voice** | the utterance decisions: each line a note bar of its airtime, its words above it and its tone/intention/urgency marks below | the round log |
//! | **Intent** | the System One decisions applied (move and action) as gestures, and the refusals and unsure gates (D72) as hollow marks | the log, plus the live minds for the hollows |
//! | **Gaze** | the agent's attention target over time — the move its lanes hold, and who its line was for while it spoke | the agent's own state, sampled as the round plays or replays |
//! | **Tier** | the same attention runs read as tiers: the reflex (engine) where the lanes held nothing, the gamepad where System One held the move | the same samples |
//!
//! ## Nothing here decides anything
//!
//! Like the roll, the score is a picture of the round. It is built from the
//! log, from the events and from samples of the agents' own state; it never
//! touches the simulation, and the simulation never knows it is being read.
//!
//! ## What a replay has and what it has not
//!
//! Everything on the voice, intent, gaze and tier staves comes from the log
//! or from replaying it, so a replayed round scores exactly as a live one.
//! The hollow marks used to be the one exception: a refusal is an outcome the
//! *mind* records (D26), and the round log carried no refusals, so the
//! hollows were drawn only while the round was being played. The log keeps
//! the refusals now (N2), so a replay draws the same marks a live round did:
//! the record is the one place both read from.

use std::collections::BTreeMap;
use std::ops::Range;

use lm_agents::sysone::Refusal;
use lm_core::decision::{Kind, Tick};
use lm_core::header::Role;
use lm_core::log::RoundLog;
use lm_core::refusal::RefusalKind;
use lm_core::sim::Sim;
use lm_core::utterance::{
    Addressee, Affect, Channel, Intent as TalkIntent, SpeechAct, Subject, Word,
};
use lm_core::view::MenuOption;

use crate::roll::MarkKind;

// ------------------------------------------------------------- the attention

/// Where an agent's attention stood, as the agent's own state held it.
///
/// The state is the lanes' selection (`lane_cmd`): the move the agent's
/// System One committed its body to, or nothing — and nothing means the
/// reflex tier is playing the moment (D26: late AI falls back to reflexes).
/// It is read, never inferred: the same agent state drives the motor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attention {
    /// The lanes hold no move: the reflex (the engine tier) plays.
    Reflex,
    /// The lanes hold this move (the gamepad tier).
    Move(MenuOption),
}

impl Attention {
    /// Whether this is a move the gamepad holds.
    pub fn is_move(self) -> bool {
        matches!(self, Attention::Move(_))
    }
}

/// One agent's attention over the round, run-length encoded: the tick a
/// state began on, and the state itself. A run lasts until the next entry.
pub type AttentionRuns = Vec<(Tick, Attention)>;

/// The attention of every agent, by agent id.
pub type AttentionTrace = BTreeMap<u16, AttentionRuns>;

/// Note the attention of every agent as its own state holds it *after*
/// `tick`, appending a run only where the state changed.
///
/// This is the whole sampling cost of the gaze and tier staves: one walk of
/// the agents per tick, and a push only on a change. The live host calls it
/// after every tick it plays; the replay scrubber calls it once per tick of
/// its opening replay, so a replayed round carries the same trace a played
/// one did.
pub fn note_attention(sim: &Sim, trace: &mut AttentionTrace, tick: Tick) {
    for id in sim.agent_ids() {
        let what = sim
            .agent(id)
            .and_then(|agent| agent.lane_cmd)
            .map(Attention::Move)
            .unwrap_or(Attention::Reflex);
        let runs = trace.entry(id).or_default();
        if runs.last().map(|&(_, held)| held) != Some(what) {
            runs.push((tick, what));
        }
    }
}

/// One run of the attention timeline, with its end tick resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    /// The first tick of the run.
    pub from: Tick,
    /// The last tick of the run (the next run begins on `to + 1`).
    pub to: Tick,
    /// The attention held through the run.
    pub what: Attention,
}

// ---------------------------------------------------------------- the staves

/// One note of the voice staff: a line as it was spoken.
///
/// The bar is the line's airtime (D31); the words ride above it at the
/// ticks the score gave them, and the marks below it are the gates' own
/// answer — the tone, the intention, the urgency and the address, which are
/// the plan's fields (`crates/lm-agents/src/utter.rs`) as the utterance
/// carries them into the log. They are read from the plan, never measured
/// from audio: there is no prosody in them yet (Vox Score fills that later).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    /// The tick the line began on.
    pub from: Tick,
    /// The tick the line ended on (from + airtime).
    pub to: Tick,
    /// The channel the line was spoken on.
    pub channel: Channel,
    /// The tone of the line: the affect bucket.
    pub affect: Affect,
    /// What the line was for.
    pub intent: TalkIntent,
    /// The gesture the line made.
    pub act: SpeechAct,
    /// Who the line was for.
    pub addressee: Addressee,
    /// What the line was about.
    pub subject: Subject,
    /// The words of the line, at the ticks the score timed them to
    /// (relative to `from`), with their lanes: pitch, energy, rate,
    /// emphasis.
    pub words: Vec<Word>,
}

impl Note {
    /// The line's marks, as short mark words in the score's order: its
    /// tone, what the line was for, who it was for and what it was about —
    /// `[urgent, order, to-squad, b2]`, the tone first because it is the
    /// mark a reader scans for.
    ///
    /// The words are short by design: they sit under a note bar that may be
    /// a dozen pixels wide. The drawing may drop the tail of the list when
    /// the bar is tight; the model always names all of them.
    pub fn marks(&self) -> Vec<String> {
        let mut out = vec![affect_word(self.affect), intent_word(self.intent)];
        out.push(addressee_word(self.addressee));
        if let Some(subject) = subject_word(self.subject) {
            out.push(subject);
        }
        out
    }
}

/// What a gesture on the intent staff stands for: which kind of decision
/// the mind's hand made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GestureKind {
    /// A move decision: the lanes, a roll or an intent.
    Move,
    /// An action decision: a pinned action.
    Action,
    /// The release of the pinned action.
    Release,
}

/// One gesture of the intent staff: a decision that was applied, the hand
/// it came from and how late it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gesture {
    /// The tick the decision was applied on.
    pub tick: Tick,
    /// What kind of decision it was.
    pub what: GestureKind,
    /// Where the decision came from (whose hand made the gesture).
    pub who: MarkKind,
    /// How late it was, in ticks; 0 when it was on time.
    pub late_by: u16,
}

/// Why a hollow mark stands on the intent staff: the named shape of a mind
/// that hesitated (D26). `Unsure` is the D72 case — a gate the backend was
/// not sure about, which made no decision of its own.
///
/// The score's own vocabulary: it is the *mark* that is drawn, not the rule
/// that failed. Every rule that can fail has a mark, and the mapping from
/// the record's vocabulary ([`RefusalKind`]) to this one is total, so no
/// refusal is ever lost by the view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HollowKind {
    /// The model did not answer before the deadline.
    Timeout,
    /// The call never reached the model, or the endpoint refused it.
    Unreachable,
    /// The answer did not match the schema.
    Malformed,
    /// The pack does not fit the API's schema: a bug in the pack, not in the
    /// model.
    InvalidPack,
    /// The gate's answer was below the confidence threshold (D72's
    /// `Answers::unsure`): the gate made no decision of its own.
    Unsure,
    /// The menu changed under the question.
    MenuChanged,
    /// The agent's budget for the round was spent.
    Budget,
    /// The wording did not follow the plan (D37).
    OffPlan,
    /// The request expired while it was queued.
    Expired,
    /// A newer question replaced this one before it was sent.
    Superseded,
    /// The second opinion did not answer before the decision's tick.
    EscalationLate,
    /// The backend crashed answering the question.
    Panicked,
}

impl HollowKind {
    /// The mark's own word, for a drawing that names it.
    pub fn as_str(self) -> &'static str {
        match self {
            HollowKind::Timeout => "timeout",
            HollowKind::Unreachable => "down",
            HollowKind::Malformed => "malformed",
            HollowKind::InvalidPack => "pack",
            HollowKind::Unsure => "unsure",
            HollowKind::MenuChanged => "menu",
            HollowKind::Budget => "budget",
            HollowKind::OffPlan => "off-plan",
            HollowKind::Expired => "expired",
            HollowKind::Superseded => "superseded",
            HollowKind::EscalationLate => "escalation-late",
            HollowKind::Panicked => "panicked",
        }
    }
}

/// One hollow mark: a tick where a mind asked, and did not answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hollow {
    /// The tick the refusal landed on.
    pub tick: Tick,
    /// The named shape of the hesitation.
    pub why: HollowKind,
    /// The number the rule cares about: the HTTP status, the confidence in
    /// Q16, the ticks late.
    pub detail: u32,
}

impl From<&RefusalKind> for HollowKind {
    /// The mark for a rule that failed.
    ///
    /// The total mapping: every kind of refusal has a mark, so the view can
    /// never lose one. The two kinds that share a mark — a call that never
    /// reached an HTTP status and an endpoint that answered an error — are
    /// the same mark with different numbers, and the number is in the mark's
    /// own `detail`.
    fn from(kind: &RefusalKind) -> HollowKind {
        match kind {
            RefusalKind::Timeout => HollowKind::Timeout,
            RefusalKind::Unreachable | RefusalKind::Http => HollowKind::Unreachable,
            RefusalKind::Malformed => HollowKind::Malformed,
            RefusalKind::InvalidPack => HollowKind::InvalidPack,
            RefusalKind::LowConfidence => HollowKind::Unsure,
            RefusalKind::MenuChanged => HollowKind::MenuChanged,
            RefusalKind::Budget => HollowKind::Budget,
            RefusalKind::OffPlan => HollowKind::OffPlan,
            RefusalKind::Expired => HollowKind::Expired,
            RefusalKind::Superseded => HollowKind::Superseded,
            RefusalKind::EscalationLate => HollowKind::EscalationLate,
            RefusalKind::Panicked => HollowKind::Panicked,
        }
    }
}

impl From<&Refusal> for HollowKind {
    fn from(refusal: &Refusal) -> HollowKind {
        HollowKind::from(&RefusalKind::from(refusal))
    }
}

/// The hollow marks of one agent's refusals, in tick order.
pub fn hollows_of(refusals: &[(Tick, Refusal)]) -> Vec<Hollow> {
    refusals
        .iter()
        .map(|&(tick, ref refusal)| Hollow {
            tick,
            why: HollowKind::from(refusal),
            detail: refusal.detail(),
        })
        .collect()
}

/// One agent's system of staves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct System {
    /// The agent the system belongs to.
    pub agent: u16,
    /// The agent's squad.
    pub squad: u8,
    /// The agent's role.
    pub role: Role,
    /// The voice staff: one note per line.
    pub voice: Vec<Note>,
    /// The intent staff: the decisions applied, as gestures.
    pub intent: Vec<Gesture>,
    /// The intent staff's hollow marks: the refusals and unsure gates.
    pub hollow: Vec<Hollow>,
    /// The attention runs the gaze and tier staves read. A run that was
    /// still open where the window ends runs to the window's edge, so the
    /// staves reach the right side instead of stopping mid-air.
    pub runs: Vec<Run>,
}

impl Default for System {
    /// A system of nobody, with empty staves: what an unplayed round holds.
    fn default() -> System {
        System {
            agent: 0,
            squad: 0,
            role: Role::Member,
            voice: Vec::new(),
            intent: Vec::new(),
            hollow: Vec::new(),
            runs: Vec::new(),
        }
    }
}

/// The conductor's score: one system per agent, stacked, on one clock.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScoreModel {
    /// The systems, in agent-id order.
    pub systems: Vec<System>,
}

impl ScoreModel {
    /// Build the score of a round from its log, its agents' attention runs
    /// and its hollow marks, over `window`.
    ///
    /// `attention` carries the runs [`note_attention`] collected (live or by
    /// the scrubber's replay); `hollow` carries the refusals the minds
    /// recorded, which a replay does not have and passes as empty. Only
    /// what reaches into the window is kept, as in the roll.
    pub fn from_log(
        log: &RoundLog,
        attention: &AttentionTrace,
        hollow: &BTreeMap<u16, Vec<Hollow>>,
        window: Range<Tick>,
    ) -> ScoreModel {
        let mut systems = systems_of(log);
        let index: BTreeMap<u16, usize> = systems
            .iter()
            .enumerate()
            .map(|(i, system)| (system.agent, i))
            .collect();

        for stamped in decisions_of(log) {
            let Some(agent) = stamped.d.source.agent() else {
                continue;
            };
            let Some(&system) = index.get(&agent) else {
                continue;
            };
            match &stamped.d.kind {
                // A line is the voice staff's note: the bar of its airtime,
                // the words above it, the marks below.
                Kind::Utterance(line) => {
                    let from = stamped.tick;
                    let to = from.saturating_add(u64::from(line.duration_ticks()));
                    if from < window.end && to >= window.start {
                        systems[system].voice.push(Note {
                            from,
                            to,
                            channel: line.channel,
                            affect: line.affect,
                            intent: line.intent,
                            act: line.act,
                            addressee: line.addressee,
                            subject: line.subject,
                            words: line.words.clone(),
                        });
                    }
                }
                // The mind's hand on the body: moves and actions.
                Kind::Lane(_) | Kind::Roll(_) | Kind::Intent(_) => {
                    if window.contains(&stamped.tick) {
                        systems[system].intent.push(Gesture {
                            tick: stamped.tick,
                            what: GestureKind::Move,
                            who: MarkKind::of(&stamped.d),
                            late_by: stamped.late_by,
                        });
                    }
                }
                Kind::ActionOn(_) => {
                    if window.contains(&stamped.tick) {
                        systems[system].intent.push(Gesture {
                            tick: stamped.tick,
                            what: GestureKind::Action,
                            who: MarkKind::of(&stamped.d),
                            late_by: stamped.late_by,
                        });
                    }
                }
                Kind::ActionOff => {
                    if window.contains(&stamped.tick) {
                        systems[system].intent.push(Gesture {
                            tick: stamped.tick,
                            what: GestureKind::Release,
                            who: MarkKind::of(&stamped.d),
                            late_by: stamped.late_by,
                        });
                    }
                }
                // A marker changes nothing, and the roll already draws it.
                Kind::Marker(_) => {}
            }
        }

        // The hollow marks: the hesitations of the minds. The log keeps them
        // (N2), so a replay draws the same marks a live round did: the
        // record is the one place both read from. The live round's own marks
        // are added for the ones the log does not carry — a live round's log
        // is written as the round runs, so a mark the mind has just made can
        // be here before the log has it.
        let mut marks: BTreeMap<u16, Vec<Hollow>> = BTreeMap::new();
        for refused in log.refusals() {
            marks.entry(refused.agent).or_default().push(Hollow {
                tick: refused.tick,
                why: HollowKind::from(&refused.kind),
                detail: refused.detail,
            });
        }
        for (agent, live) in hollow {
            let known = marks.entry(*agent).or_default();
            for mark in live {
                let seen = known.iter().any(|old| {
                    old.tick == mark.tick && old.why == mark.why && old.detail == mark.detail
                });
                if !seen {
                    known.push(*mark);
                }
            }
        }
        for (agent, live) in marks {
            let Some(&system) = index.get(&agent) else {
                continue;
            };
            systems[system].hollow.extend(
                live.iter()
                    .copied()
                    .filter(|mark| window.contains(&mark.tick)),
            );
        }

        // The attention runs, clipped to the window: the gaze and the tier
        // read the same runs, each in its own way.
        let end = last_tick_of(log);
        for (agent, runs) in attention {
            let Some(&system) = index.get(agent) else {
                continue;
            };
            let mut clipped: Vec<Run> = Vec::new();
            for (position, &(from, what)) in runs.iter().enumerate() {
                let to = runs
                    .get(position + 1)
                    .map_or(end, |&(next, _)| next.saturating_sub(1));
                if from < window.end && to >= window.start {
                    clipped.push(Run {
                        from: from.max(window.start),
                        to: to.min(window.end.saturating_sub(1)),
                        what,
                    });
                }
            }
            systems[system].runs = clipped;
        }

        // The staves are read from the top down, so they are sorted.
        for system in &mut systems {
            system.voice.sort_by_key(|note| (note.from, note.to));
            system.intent.sort_by_key(|gesture| gesture.tick);
            system.hollow.sort_by_key(|hollow| hollow.tick);
        }

        ScoreModel { systems }
    }

    /// The system of an agent, if the round has one.
    pub fn system(&self, agent: u16) -> Option<&System> {
        self.systems.iter().find(|system| system.agent == agent)
    }
}

/// The systems of the round: one per agent of the roster, in agent-id order.
fn systems_of(log: &RoundLog) -> Vec<System> {
    let mut agents: Vec<u16> = log.header.roster.iter().map(|entry| entry.agent).collect();
    agents.sort_unstable();
    agents.dedup();
    agents
        .into_iter()
        .map(|agent| {
            let entry = log.header.roster.iter().find(|entry| entry.agent == agent);
            System {
                agent,
                squad: entry.map_or(0, |entry| entry.squad),
                role: entry.map_or(Role::Member, |entry| entry.role),
                voice: Vec::new(),
                intent: Vec::new(),
                hollow: Vec::new(),
                runs: Vec::new(),
            }
        })
        .collect()
}

/// Every decision of the log, in tick order.
fn decisions_of(log: &RoundLog) -> Vec<&lm_core::decision::Stamped> {
    let mut out = Vec::new();
    for segment in &log.segments {
        out.extend(segment.decisions.iter());
    }
    out
}

/// The last tick of the round.
fn last_tick_of(log: &RoundLog) -> Tick {
    log.segments
        .last()
        .map_or(log.header.start_tick, |segment| segment.last_tick)
}

// ------------------------------------------------------------------- the words

/// The mark word of an intention.
fn intent_word(intent: TalkIntent) -> String {
    match intent {
        TalkIntent::Order => "order",
        TalkIntent::Ack => "ack",
        TalkIntent::Warn => "warn",
        TalkIntent::Query => "query",
        TalkIntent::Report => "report",
    }
    .to_owned()
}

/// The mark word of a tone.
fn affect_word(affect: Affect) -> String {
    match affect {
        Affect::Calm => "calm",
        Affect::Urgent => "urgent",
        Affect::Uncertain => "uncertain",
    }
    .to_owned()
}

/// The mark word of an address.
fn addressee_word(addressee: Addressee) -> String {
    match addressee {
        Addressee::Squad => "to-squad".to_owned(),
        Addressee::All => "to-all".to_owned(),
        Addressee::Agent(agent) => format!("to-{agent}"),
    }
}

/// The mark word of a subject, when the line had one.
fn subject_word(subject: Subject) -> Option<String> {
    match subject {
        Subject::Beacon(beacon) => Some(format!("b{beacon}")),
        Subject::Enemy(enemy) => Some(format!("e{enemy}")),
        Subject::Direction(direction) => Some(format!("dir{direction}")),
        Subject::None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lm_core::refusal::RefusalKind;

    #[test]
    fn every_kind_of_refusal_has_a_mark() {
        // The mapping is total: no refusal is lost by the view.
        for kind in RefusalKind::ALL {
            let mark = HollowKind::from(&kind);
            assert!(!mark.as_str().is_empty(), "{kind:?} has no mark");
        }
    }

    #[test]
    fn the_marks_have_their_own_words() {
        // Two kinds may share a mark — a call that never reached an HTTP
        // status and an endpoint that answered an error are the same mark
        // with different numbers — but two *marks* never share a word.
        let mut marks: Vec<HollowKind> = RefusalKind::ALL.iter().map(HollowKind::from).collect();
        marks.sort_by_key(|mark| mark.as_str());
        marks.dedup();
        let mut words: Vec<&str> = marks.iter().map(|mark| mark.as_str()).collect();
        let count = words.len();
        words.sort_unstable();
        words.dedup();
        assert_eq!(words.len(), count, "two marks share a word");
    }

    #[test]
    fn a_low_confidence_refusal_carries_its_number() {
        // The record's number reaches the mark: the confidence in Q16.
        let refusal = lm_agents::sysone::Refusal::LowConfidence(0.5);
        let hollow = hollows_of(&[(7u64, refusal)]);
        assert_eq!(hollow[0].why, HollowKind::Unsure);
        assert_eq!(hollow[0].detail, 32_768, "half confidence is 0.5 in Q16");
    }
}
