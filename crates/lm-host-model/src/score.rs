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
//! The hollow marks are the one exception: a refusal is an outcome the
//! *mind* records (D26), and the round log carries no refusals, so the
//! hollows are drawn only while the round is being played. That gap is
//! named in `docs/notes/score-view.md`; filling it is a logging change, not
//! a view change.

use std::collections::BTreeMap;
use std::ops::Range;

use lm_agents::sysone::Refusal;
use lm_core::decision::{Kind, Tick};
use lm_core::header::Role;
use lm_core::log::RoundLog;
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HollowKind {
    /// The model did not answer before the deadline.
    Timeout,
    /// The call never reached the model, or the endpoint refused it.
    Unreachable,
    /// The answer did not match the schema.
    Malformed,
    /// The gate's answer was below the confidence threshold (D72's
    /// `Answers::unsure`): the gate made no decision of its own.
    Unsure,
    /// The menu changed under the question.
    MenuChanged,
    /// The agent's budget for the round was spent.
    Budget,
    /// The second opinion would have landed after the decision's apply tick
    /// (N3): the gate took the safe branch.
    LateEscalation,
}

impl HollowKind {
    /// The mark's own word, for a drawing that names it.
    pub fn as_str(self) -> &'static str {
        match self {
            HollowKind::Timeout => "timeout",
            HollowKind::Unreachable => "down",
            HollowKind::Malformed => "malformed",
            HollowKind::Unsure => "unsure",
            HollowKind::MenuChanged => "menu",
            HollowKind::Budget => "budget",
            HollowKind::LateEscalation => "escalation_late",
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
}

impl From<&Refusal> for HollowKind {
    fn from(refusal: &Refusal) -> HollowKind {
        match refusal {
            Refusal::Timeout => HollowKind::Timeout,
            Refusal::Http(_) => HollowKind::Unreachable,
            Refusal::Malformed(_) => HollowKind::Malformed,
            Refusal::InvalidPack(_) => HollowKind::Malformed,
            Refusal::LowConfidence(_) => HollowKind::Unsure,
            Refusal::MenuChanged => HollowKind::MenuChanged,
            Refusal::Budget => HollowKind::Budget,
            Refusal::EscalationLate => HollowKind::LateEscalation,
        }
    }
}

/// The hollow marks of one agent's refusals, in tick order.
pub fn hollows_of(refusals: &[(Tick, Refusal)]) -> Vec<Hollow> {
    refusals
        .iter()
        .map(|&(tick, ref refusal)| Hollow {
            tick,
            why: HollowKind::from(refusal),
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

        // The hollow marks: the hesitations of the minds that are playing
        // the round (a replay's log carries none).
        for (agent, marks) in hollow {
            let Some(&system) = index.get(agent) else {
                continue;
            };
            systems[system].hollow.extend(
                marks
                    .iter()
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
