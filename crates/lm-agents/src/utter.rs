//! Task 4: the utterances -- the **gates** choose, the **realizer** words.
//!
//! An agent's line is two things kept apart on purpose (D37):
//!
//! - the **choice**: the gesture, the addressee, the intent, the affect
//!   and the subject, all made by the decision model out of typed gates;
//! - the **wording**: the words, which come from the phrasebook (the
//!   default) or from a small language model on the Thor.
//!
//! The split is why a model can never choose a target. The gates pick the
//! addressee out of a closed set that is checked against the round's
//! roster, and the subject out of the options of the round's own menu --
//! and the realizer only ever writes *how* the line says it. The tags of
//! the score are the gates' answer, whatever the wording says.
//!
//! The **words** and the **notation** are apart too. A realizer writes
//! words; the notation is written here, from the plan's tags and the
//! words, so that a line the model words and a line the phrasebook words
//! are the same shape. A small model on the Thor writes words well and
//! notation badly, and the split is what lets it be used at all.
//!
//! ```text
//! ScoreView --facts--> plain-language facts
//!          --gates--> the choice: act, addressee, intent, affect, subject
//!          --realize--> the words, and the notation around them
//!          --parse--> the score: an utterance, with its words timed
//!          --decide--> a logged decision the sim applies (D12)
//! ```
//!
//! Everything that can go wrong here is a **refusal** with a name (D26):
//! a malformed answer, a line that does not compile, a model that is down.
//! A refusal never becomes a guess -- the agent falls back to the other
//! realizer or to silence, and says nothing rather than something made up.
//! Silence is an outcome (D53): a presence gate that says "this is not even
//! here" produces no line at all, and costs no airtime.
//!
//! The **automatic** realizer is honest about what it does: the model is
//! asked for every line, with the line's deadline, and a line the model
//! did not answer in time is worded by the phrasebook instead -- for that
//! line, and only for that line. The round's counters say how often that
//! happened ([`WordCounts`]).

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use lm_core::header::RosterEntry;
use lm_core::opal::{self, TalkContext};
use lm_core::utterance::{Addressee, Affect, Channel, Intent, SpeechAct, Subject, Utterance};
use lm_core::view::{Menu, MenuOption};
use serde_json::{json, Value};

use crate::config::AgentsConfig;
use crate::facts::Facts;
use crate::phrasebook::{self, Phrasebook};
use crate::sysone::{post_pack, Answer, Answers, Refusal};

// ------------------------------------------------------------------ the gates

/// The gate that picks the gesture of the line.
pub const GATE_GESTURE: &str = "gesture";
/// The gate that picks who the line is for.
pub const GATE_ATTENTION: &str = "attention";
/// The gate that scores how urgent the moment is.
pub const GATE_INTENTION: &str = "intention";
/// The gate that picks what the line is for.
pub const GATE_INTENT: &str = "intent";
/// The gate that picks what the line is about.
pub const GATE_SUBJECT: &str = "subject";

/// The suffix of a gate's **presence** gate (D53).
///
/// The presence gate is the noul "is this even here?". It is asked in the
/// same call as the gate it belongs to, and a noul of about zero means the
/// line is not spoken at all.
pub const PRESENCE_SUFFIX: &str = "_present";

/// The gates the utterance is chosen by, in the order they are read.
pub const UTTERANCE_GATES: [&str; 5] = [
    GATE_GESTURE,
    GATE_ATTENTION,
    GATE_INTENTION,
    GATE_INTENT,
    GATE_SUBJECT,
];

/// A presence noul below this is "not here at all" (5 %).
///
/// The noul is a belief between 0 and 1: a noul of about zero says the
/// thing is not present, and the agent says nothing.
pub const PRESENCE_FLOOR: f32 = 0.05;

/// The intention score above which the line is urgent, in Q16 (1.0).
///
/// The intention scale runs from idle (0) through the middle (1) to
/// urgent (2), so a score above one is a pressed moment.
pub const AFFECT_URGENT_Q16: u32 = 65_536;

/// The intention score at or above which the line is uncertain, in Q16
/// (0.5).
pub const AFFECT_UNCERTAIN_Q16: u32 = 32_768;

/// The choice the decision model made: what the line is, before it is
/// worded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UtterPlan {
    /// The gesture of the line.
    pub act: SpeechAct,
    /// Who the line is for.
    pub addressee: Addressee,
    /// What the line is for.
    pub intent: Intent,
    /// The affect bucket of the line.
    pub affect: Affect,
    /// What the line is about.
    pub subject: Subject,
    /// Whether the line is spoken at all.
    ///
    /// A presence gate that says "this is not even here" makes this
    /// false, and a false plan produces no decision: silence is an
    /// outcome (D53).
    pub should_speak: bool,
}

/// What the plan needs that the answers do not carry.
#[derive(Clone, Copy, Debug)]
pub struct GateCtx<'a> {
    /// The agent that is speaking.
    pub agent: u16,
    /// The round's roster: who exists, so an addressee can be checked
    /// against it (a line may not name an agent that is not in the round).
    pub roster: &'a [RosterEntry],
}

impl GateCtx<'_> {
    /// Whether an agent is in the round.
    pub fn has_agent(&self, agent: u16) -> bool {
        self.roster.iter().any(|entry| entry.agent == agent)
    }
}

/// The choice the gates make, from the answers of one System One call.
///
/// The gates are the same call as the action gates (D37): the model is
/// asked once, and its answers carry both the move and the line.
///
/// - The **gesture** gate picks the act, the **attention** gate the
///   addressee, the **intention** gate the affect, the **intent** gate the
///   intent and the **subject** gate what the line is about.
/// - The subject comes from the round's own **menu**: the model picks an
///   option that exists, and an answer that names an option the menu does
///   not have is refused ([`Refusal::MenuChanged`]), never mapped onto a
///   neighbour (D26).
/// - Every one of the five gates carries a **presence** gate (D53): a noul
///   of about zero means the line is not spoken at all.
/// - An answer the pack does not carry is a refusal: nothing is invented
///   to fill the gap (D26).
pub fn plan_from_gates(
    answers: &Answers,
    menu: &Menu,
    ctx: &GateCtx<'_>,
) -> Result<UtterPlan, Refusal> {
    let should_speak = presence(answers)?;
    let act = gesture(answers)?;
    let addressee = attention(answers, ctx)?;
    let affect = intention(answers)?;
    let intent = intent(answers)?;
    let subject = subject(answers, menu)?;
    Ok(UtterPlan {
        act,
        addressee,
        intent,
        affect,
        subject,
        should_speak,
    })
}

/// Whether the line is spoken at all: every presence gate must say it is
/// here.
fn presence(answers: &Answers) -> Result<bool, Refusal> {
    for gate in UTTERANCE_GATES {
        let id = presence_gate(gate);
        match answered(answers, &id)? {
            Answer::Noul(value) => {
                if *value < PRESENCE_FLOOR {
                    return Ok(false);
                }
            }
            other => {
                return Err(Refusal::Malformed(format!(
                    "the gate {id} answered a {}, not a noul",
                    kind_of(other)
                )))
            }
        }
    }
    Ok(true)
}

/// The id of a gate's presence gate.
pub fn presence_gate(gate: &str) -> String {
    format!("{gate}{PRESENCE_SUFFIX}")
}

/// The gesture of the line.
fn gesture(answers: &Answers) -> Result<SpeechAct, Refusal> {
    let key = choice(answers, GATE_GESTURE)?;
    phrasebook::act_from_key(&key)
        .ok_or_else(|| Refusal::Malformed(format!("the gesture {key} is not one this build knows")))
}

/// Who the line is for.
fn attention(answers: &Answers, ctx: &GateCtx<'_>) -> Result<Addressee, Refusal> {
    let key = choice(answers, GATE_ATTENTION)?;
    match key.as_str() {
        "squad" => Ok(Addressee::Squad),
        "all" => Ok(Addressee::All),
        other => {
            let Some(agent) = other
                .strip_prefix("agent")
                .and_then(|digits| digits.parse::<u16>().ok())
            else {
                return Err(Refusal::Malformed(format!(
                    "the addressee {other} is not one this build knows"
                )));
            };
            // A line may not name an agent that is not in the round: the
            // target is never a free invention (D37).
            if !ctx.has_agent(agent) {
                return Err(Refusal::Malformed(format!(
                    "the addressee agent {agent} is not in this round"
                )));
            }
            Ok(Addressee::Agent(agent))
        }
    }
}

/// The affect of the line, bucketed by code and never by the model.
///
/// The `intention` gate is a `score` question, and the API answers it with
/// the **index** of the level it picked: the gate's levels are sent as an
/// ordered array, lowest score first, so the index *is* the level's score
/// (idle 0, ordinary 1, urgent 2). The gate set is what says which level
/// that is — see `GateSet::level_for` — and the score is what is bucketed
/// here.
fn intention(answers: &Answers) -> Result<Affect, Refusal> {
    match answered(answers, GATE_INTENTION)? {
        Answer::Score { score, .. } => Ok(bucket_affect(*score)),
        other => Err(Refusal::Malformed(format!(
            "the gate {GATE_INTENTION} answered a {}, not a score",
            kind_of(other)
        ))),
    }
}

/// The affect bucket of an intention score.
///
/// The bucketing is **code**, not the model: the model says how urgent
/// the moment is on a scale from idle (0) through the middle (1) to urgent
/// (2), and the code says which bucket that is.
pub fn bucket_affect(score: f32) -> Affect {
    // The score is on the scale from idle (0) through the middle (1) to
    // urgent (2), and Q16 is one whole of the scale's half: 65 536 is
    // the middle of it.
    let q16 = score * (AFFECT_URGENT_Q16 as f32);
    let q16 = if q16.is_finite() { q16 } else { 0.0 };
    let q16 = q16.clamp(0.0, u32::MAX as f32) as u32;
    if q16 > AFFECT_URGENT_Q16 {
        Affect::Urgent
    } else if q16 >= AFFECT_UNCERTAIN_Q16 {
        Affect::Uncertain
    } else {
        Affect::Calm
    }
}

/// What the line is for.
fn intent(answers: &Answers) -> Result<Intent, Refusal> {
    let key = choice(answers, GATE_INTENT)?;
    phrasebook::intent_from_key(&key)
        .ok_or_else(|| Refusal::Malformed(format!("the intent {key} is not one this build knows")))
}

/// What the line is about, from the round's own menu.
///
/// The subject gate is asked over the menu, so its options are the
/// beacons the agent may talk about and the moves it may make. The answer
/// must name exactly the menu's options: an answer that names another
/// option is a menu that changed under the question, and it is refused
/// rather than mapped onto a neighbour (D26).
fn subject(answers: &Answers, menu: &Menu) -> Result<Subject, Refusal> {
    let Answer::Choice { probs, .. } = answered(answers, GATE_SUBJECT)? else {
        return Err(Refusal::Malformed(format!(
            "the gate {GATE_SUBJECT} answered a {}, not a choice",
            kind_of(answered(answers, GATE_SUBJECT)?)
        )));
    };
    if probs.len() != menu.options.len() {
        return Err(Refusal::MenuChanged);
    }
    for index in 0..menu.options.len() {
        if !probs.contains_key(&option_key(index)) {
            return Err(Refusal::MenuChanged);
        }
    }
    let key = choice(answers, GATE_SUBJECT)?;
    let index = key
        .strip_prefix("opt")
        .and_then(|digits| digits.parse::<usize>().ok())
        .ok_or(Refusal::MenuChanged)?;
    let option = menu.options.get(index).ok_or(Refusal::MenuChanged)?;
    Ok(match option {
        MenuOption::GoToBeacon(beacon) => Subject::Beacon(*beacon),
        MenuOption::Hold | MenuOption::FollowLeader | MenuOption::Retreat => Subject::None,
    })
}

/// The key of the option at `index` in a menu.
fn option_key(index: usize) -> String {
    format!("opt{index}")
}

/// The answer of a gate, or a refusal: the pack did not answer it.
fn answered<'a>(answers: &'a Answers, gate: &str) -> Result<&'a Answer, Refusal> {
    answers
        .per_gate
        .get(gate)
        .ok_or_else(|| Refusal::Malformed(format!("the pack did not answer the gate {gate}")))
}

/// The option a choice gate's answer picks.
///
/// The largest probability wins, and a tie goes to the first option in
/// key order, so the same answer always picks the same option.
fn choice(answers: &Answers, gate: &str) -> Result<String, Refusal> {
    let Answer::Choice { probs, .. } = answered(answers, gate)? else {
        return Err(Refusal::Malformed(format!(
            "the gate {gate} answered a {}, not a choice",
            kind_of(answered(answers, gate)?)
        )));
    };
    let mut best: Option<(&String, f32)> = None;
    for (key, probability) in probs {
        if best.is_none_or(|(_, best)| *probability > best) {
            best = Some((key, *probability));
        }
    }
    best.map(|(key, _)| key.clone())
        .ok_or_else(|| Refusal::Malformed(format!("the gate {gate} answered no option")))
}

/// The name of an answer's kind, for a refusal that names it.
fn kind_of(answer: &Answer) -> &'static str {
    match answer {
        Answer::Choice { .. } => "choice",
        Answer::Score { .. } => "score",
        Answer::Noul(_) => "noul",
    }
}

// ---------------------------------------------------------------- the wording

/// Where a line's words came from.
///
/// The realizer says which: a line is worded by the model on the Thor or by
/// the harness's phrasebook, and the round's counters ([`crate::agents::
/// WordCounts`]) say how many of each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordSource {
    /// The small language model on the Thor.
    Llm,
    /// The harness's phrasebook.
    Phrasebook,
}

/// A worded line: the words, and where they came from.
///
/// The `late` flag says the model was asked for this line's words and did
/// not bring them in time: the phrasebook worded the line instead, because
/// the line's deadline passed first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Worded {
    /// The line, in the notation.
    pub text: String,
    /// Where the words came from.
    pub source: WordSource,
    /// Whether the model's wording missed the line's deadline.
    pub late: bool,
}

impl Worded {
    /// A line the phrasebook worded.
    pub fn phrasebook(text: String) -> Worded {
        Worded {
            text,
            source: WordSource::Phrasebook,
            late: false,
        }
    }

    /// A line the model worded.
    pub fn llm(text: String) -> Worded {
        Worded {
            text,
            source: WordSource::Llm,
            late: false,
        }
    }
}

/// The wording of a line.
///
/// The realizer writes the words; the gates wrote the tags. The two are
/// apart on purpose: a realizer may not choose a target, and a gate may
/// not choose a wording (D37).
pub trait Realizer: Send + Sync {
    /// The line, in the notation, for this plan in this situation.
    ///
    /// A realizer that cannot word the line **refuses**: it never invents
    /// a fallback phrase and never writes a line that does not compile
    /// (D26). The automatic realizer is the exception that is not an
    /// exception: it falls back to the phrasebook for that one line, and
    /// says so in the [`Worded`] it returns.
    fn realize(&self, plan: &UtterPlan, facts: &Facts) -> Result<Worded, Refusal>;
}

/// The realizer of the phrasebook: the default.
///
/// The phrasebook is the harness's own file, so the wording is doctrine
/// and a reviewer may edit it between rounds (D25/D47). A line the
/// phrasebook does not have is a **refusal**, not a fallback phrase: the
/// caller falls back to the other realizer or to silence.
#[derive(Clone, Debug)]
pub struct TemplateRealizer {
    phrasebook: Phrasebook,
}

impl TemplateRealizer {
    /// A realizer over `phrasebook`.
    pub fn new(phrasebook: Phrasebook) -> Self {
        TemplateRealizer { phrasebook }
    }

    /// A realizer over the phrasebook the harness ships.
    pub fn shipped() -> Self {
        TemplateRealizer::new(Phrasebook::shipped())
    }

    /// The phrasebook this realizer words with.
    pub fn phrasebook(&self) -> &Phrasebook {
        &self.phrasebook
    }
}

impl Realizer for TemplateRealizer {
    fn realize(&self, plan: &UtterPlan, facts: &Facts) -> Result<Worded, Refusal> {
        let entry = self
            .phrasebook
            .entry(plan.act, plan.intent, plan.subject)
            .ok_or_else(|| {
                Refusal::Malformed(format!(
                    "the phrasebook has no line for {}.{}",
                    phrasebook::act_name(plan.act),
                    phrasebook::intent_name(plan.intent)
                ))
            })?;
        Ok(Worded::phrasebook(entry.render(plan, facts.key)))
    }
}

/// The realizer of a small language model on the Thor.
///
/// The model is asked for the **words** and nothing else, and the line it
/// words is **compiled before it is used**: a line that does not compile
/// is a refusal (D26), and the bad line never reaches the log. The tags
/// are the gates' answer, whatever the model wrote: the notation is
/// written here, from the plan, so a model may never choose a target
/// (D37).
///
/// The model's own words are checked against the plan as well: a wording
/// that names a beacon or an enemy the gates did not choose is a refusal,
/// for the same reason the header is ours (D37).
pub struct LlmRealizer {
    llm: Box<dyn Llm>,
    deadline: Duration,
}

impl LlmRealizer {
    /// A realizer that asks `llm`, with `deadline` for a line's words.
    pub fn new(llm: Box<dyn Llm>, deadline: Duration) -> Self {
        LlmRealizer { llm, deadline }
    }

    /// The deadline one line's words get.
    pub fn deadline(&self) -> Duration {
        self.deadline
    }

    /// The model's name, for the log.
    pub fn model(&self) -> &str {
        self.llm.model()
    }

    /// The model itself, for a caller that asks it on its own thread.
    pub fn into_llm(self) -> Box<dyn Llm> {
        self.llm
    }
}

impl Realizer for LlmRealizer {
    fn realize(&self, plan: &UtterPlan, facts: &Facts) -> Result<Worded, Refusal> {
        let words = words_for(&*self.llm, plan, facts, self.deadline)?;
        let line = line_for(plan, Channel::Voice, &words, self.llm.model())?;
        Ok(Worded::llm(line))
    }
}

/// The model's words for a line, checked against the plan.
///
/// The words are the model's own: only the shape of the reply is ours
/// (the grammar the Thor's server is asked for), and only the *target* is
/// checked -- a wording that names a beacon or an enemy the gates did not
/// choose is a refusal (D37).
fn words_for(
    llm: &dyn Llm,
    plan: &UtterPlan,
    facts: &Facts,
    deadline: Duration,
) -> Result<String, Refusal> {
    let words = written_words(llm, plan, facts, deadline)?;
    if let Some(other) = other_target(&words, plan.subject) {
        return Err(Refusal::Malformed(format!(
            "the model named {other}, which the gates did not choose"
        )));
    }
    Ok(words)
}

/// The words a model wrote for a line, before they are checked.
///
/// A model that does not deliver is a refusal here: the call missed the
/// deadline, or the model was not there, or its answer was not a line.
fn written_words(
    llm: &dyn Llm,
    plan: &UtterPlan,
    _facts: &Facts,
    deadline: Duration,
) -> Result<String, Refusal> {
    let written = llm.write(&prompt_for(plan), Instant::now() + deadline)?;
    let words = words_of(&written);
    if words.is_empty() {
        return Err(Refusal::Malformed("the model wrote no words".to_string()));
    }
    Ok(words)
}

/// A line in the notation, from the model's words, checked that it compiles.
///
/// The line must compile: a line that does not is a refusal, and it never
/// reaches the log.
fn line_for(
    plan: &UtterPlan,
    channel: Channel,
    words: &str,
    model: &str,
) -> Result<String, Refusal> {
    let line = phrasebook::line_of(plan, channel, words);
    opal::parse(&empty_context(), &line).map_err(|e| {
        Refusal::Malformed(format!(
            "the model {model} wrote a line that does not compile: {e}"
        ))
    })?;
    Ok(line)
}

/// The words of a model's reply.
///
/// The reply is trimmed and its surrounding quotes are dropped: the model
/// writes a line, and a line that is wrapped in quotes is the same line.
/// Nothing else is touched -- a word is a word, and a word the model wrote
/// is the model's (D26).
fn words_of(written: &str) -> String {
    let trimmed = written.trim();
    let trimmed = trimmed.strip_prefix('"').unwrap_or(trimmed);
    let trimmed = trimmed.strip_suffix('"').unwrap_or(trimmed);
    trimmed.split_whitespace().collect::<Vec<&str>>().join(" ")
}

/// The target a wording names, when it names one the gates did not choose.
///
/// The gates choose the subject; the words may not name another one (D37).
/// A wording that names the subject the gates chose is fine, and a wording
/// that names no target at all is fine.
pub fn other_target(words: &str, subject: Subject) -> Option<String> {
    let wanted = match subject {
        Subject::Beacon(beacon) => Some(("beacon", beacon.to_string())),
        Subject::Enemy(enemy) => Some(("enemy", enemy.to_string())),
        _ => None,
    };
    let mut named: Option<(String, String)> = None;
    let tokens: Vec<&str> = words.split_whitespace().collect();
    for pair in tokens.windows(2) {
        let (kind, number) = (pair[0].to_ascii_lowercase(), pair[1]);
        if !matches!(kind.as_str(), "beacon" | "enemy") {
            continue;
        }
        let number = number.trim_matches(|c: char| !c.is_ascii_digit());
        if number.is_empty() {
            continue;
        }
        named = Some((kind, number.to_owned()));
        break;
    }
    let (kind, number) = named?;
    match &wanted {
        Some((wanted_kind, wanted_number)) if *wanted_kind == kind && *wanted_number == number => {
            None
        }
        _ => Some(format!("{kind} {number}")),
    }
}

/// The automatic realizer: the model words the line, and the phrasebook
/// words it when the model is late.
///
/// This is the honest `auto`. The model is asked for **every** line, and
/// the phrasebook is not consulted first: a line the model has not
/// answered yet is worded by the phrasebook instead, for that line and
/// only for that line, and the round's counters say how often that
/// happened (`late`).
///
/// **The game never waits on the wording.** The model is asked on the
/// realizer's own thread ([`WordBank`]), and the words are read when they
/// are in hand. A line that is due before its words are in hand is worded
/// by the phrasebook, and the model's words are kept for the next line of
/// the same kind.
pub struct AutoRealizer {
    template: TemplateRealizer,
    bank: WordBank,
    model: String,
}

impl AutoRealizer {
    /// The automatic realizer over a phrasebook and a model.
    pub fn new(template: TemplateRealizer, llm: LlmRealizer) -> Self {
        let model = llm.model().to_owned();
        let deadline = llm.deadline();
        AutoRealizer {
            template,
            bank: WordBank::new(llm.into_llm(), deadline),
            model,
        }
    }

    /// How long the model took for the wordings in the bank, in microseconds.
    ///
    /// This is the wording's own latency: the game waits for none of it.
    pub fn latencies(&self) -> Vec<u64> {
        self.bank.latencies()
    }
}

impl Realizer for AutoRealizer {
    fn realize(&self, plan: &UtterPlan, facts: &Facts) -> Result<Worded, Refusal> {
        // Ask for the wording if it has not been asked for: the bank is
        // asked once per kind of line, and the answer is kept.
        self.bank.ask(plan, facts);
        match self.bank.take(plan, facts) {
            Some(banked) => {
                if let Some(other) = other_target(&banked.words, plan.subject) {
                    // The model answered and named a target the gates did
                    // not choose: the phrasebook words this line, and the
                    // line is not late -- it was wrong (D37).
                    say(&format!(
                        "the model {} named {other}, which the gates did not choose; the \
                         phrasebook words the line",
                        self.model
                    ));
                    let worded = self.template.realize(plan, facts)?;
                    return Ok(Worded {
                        late: false,
                        ..worded
                    });
                }
                // The channel is the harness's business, so a line the model
                // words is spoken where the phrasebook would have spoken it.
                let channel = self
                    .template
                    .phrasebook()
                    .channel_of(plan.act, plan.intent, plan.subject)
                    .unwrap_or(Channel::Voice);
                let line = line_for(plan, channel, &banked.words, &self.model)?;
                Ok(Worded::llm(line))
            }
            None => {
                // The model's words are not in hand: the line is late, and
                // the phrasebook words it.
                let worded = self.template.realize(plan, facts)?;
                Ok(Worded {
                    late: true,
                    ..worded
                })
            }
        }
    }
}

/// The wording's identity: the plan.
///
/// The wording is a function of the line's **kind**, and that is what the
/// harness's own phrasebook does: its templates are keyed by the gesture,
/// the intent and the kind of subject, not by the situation. The facts
/// change every tick -- the agents move -- so a key that carried them
/// would never come round again and the bank would never fill.
///
/// The plan carries the gesture, the recipient, the intent, the urgency
/// and the target, so a wording kept for a plan still says what that line
/// is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct WordKey {
    /// The plan the line is made from.
    plan: UtterPlan,
}

/// The model's words for a line, once they are in.
#[derive(Clone, Debug)]
pub struct Banked {
    /// The words the model wrote.
    pub words: String,
    /// How long the model took, in microseconds.
    pub micros: u64,
}

/// A job for the wording thread: the line to word, and the prompt.
struct Job {
    /// The line the job is for.
    key: WordKey,
    /// The prompt the model is asked.
    prompt: String,
}

/// How many wordings the bank keeps.
///
/// The bank is a cache, not a log: a round has a few hundred kinds of
/// line, and the bank drops the oldest when it grows past this.
const BANK_CAPACITY: usize = 512;

/// The bank of prepared wordings, and the thread that fills it.
///
/// The game never waits on wording. The model is asked here, on the
/// realizer's own thread, and the words are read when they are in hand: a
/// line that is due before its words are in hand is worded by the
/// phrasebook instead, and the model is asked again for the next line of
/// the same kind.
pub struct WordBank {
    inner: Arc<Mutex<BankInner>>,
    wake: Arc<Condvar>,
    worker: Option<thread::JoinHandle<()>>,
}

/// What the bank holds.
#[derive(Default)]
struct BankInner {
    /// The wordings that are in, by key, in the order they came in.
    ready: BTreeMap<WordKey, Banked>,
    /// The order the wordings came in, so the oldest can go.
    order: VecDeque<WordKey>,
    /// The keys that have been asked for.
    asked: BTreeSet<WordKey>,
    /// The jobs the thread has to do.
    queue: VecDeque<Job>,
    /// Whether the thread is to stop.
    stop: bool,
}

impl WordBank {
    /// A bank that asks `llm` for the wording of a line.
    ///
    /// The thread is the realizer's own: it asks the model while the game
    /// plays on.
    pub fn new(llm: Box<dyn Llm>, deadline: Duration) -> WordBank {
        let inner = Arc::new(Mutex::new(BankInner::default()));
        let wake = Arc::new(Condvar::new());
        let worker_inner = Arc::clone(&inner);
        let worker_wake = Arc::clone(&wake);
        let worker = thread::Builder::new()
            .name("lm-words".to_owned())
            .spawn(move || {
                loop {
                    let job = {
                        let mut bank = worker_inner.lock().expect("the word bank");
                        loop {
                            if bank.stop {
                                return;
                            }
                            if let Some(job) = bank.queue.pop_front() {
                                break job;
                            }
                            bank = worker_wake.wait(bank).expect("the word bank");
                        }
                    };
                    let started = Instant::now();
                    let written = llm.write(&job.prompt, started + deadline);
                    let mut bank = worker_inner.lock().expect("the word bank");
                    if let Ok(reply) = written {
                        let words = words_of(&reply);
                        if !words.is_empty() {
                            let banked = Banked {
                                words,
                                micros: started.elapsed().as_micros() as u64,
                            };
                            if bank.ready.insert(job.key, banked).is_none() {
                                bank.order.push_back(job.key);
                            }
                            // The bank is a cache: the oldest wording goes
                            // when it grows past its capacity.
                            while bank.order.len() > BANK_CAPACITY {
                                if let Some(oldest) = bank.order.pop_front() {
                                    bank.ready.remove(&oldest);
                                }
                            }
                        }
                    }
                }
            })
            .expect("the wording thread starts");
        WordBank {
            inner,
            wake,
            worker: Some(worker),
        }
    }

    /// Ask for the wording of a line, if it has not been asked for.
    ///
    /// This never waits: the model is asked on the thread, and the game
    /// plays on.
    pub fn ask(&self, plan: &UtterPlan, _facts: &Facts) {
        let key = WordKey { plan: *plan };
        let mut bank = self.inner.lock().expect("the word bank");
        if !bank.asked.insert(key) {
            return;
        }
        bank.queue.push_back(Job {
            key,
            prompt: prompt_for(plan),
        });
        drop(bank);
        self.wake.notify_one();
    }

    /// The wording of a line, if it is in the bank.
    pub fn take(&self, plan: &UtterPlan, _facts: &Facts) -> Option<Banked> {
        let key = WordKey { plan: *plan };
        self.inner
            .lock()
            .expect("the word bank")
            .ready
            .get(&key)
            .cloned()
    }

    /// How long the model took for the wordings in the bank, in microseconds.
    pub fn latencies(&self) -> Vec<u64> {
        self.inner
            .lock()
            .expect("the word bank")
            .ready
            .values()
            .map(|banked| banked.micros)
            .collect()
    }
}

impl Drop for WordBank {
    fn drop(&mut self) {
        // The thread is told to stop before it is joined: it waits for
        // jobs, and a thread that waits forever is a thread that never
        // lets go of the round.
        {
            let mut bank = self.inner.lock().expect("the word bank");
            bank.stop = true;
        }
        self.wake.notify_all();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// The realizer the configuration asks for.
///
/// `utter.realizer` is `template`, `llm` or `auto`; `auto` is the
/// default.
pub fn pick_realizer(cfg: &AgentsConfig) -> Box<dyn Realizer> {
    pick_realizer_with(cfg, &Phrasebook::shipped())
}

/// The realizer the configuration asks for, over a phrasebook.
///
/// The harness's own phrasebook is the one a round plays with: the
/// shipped one is only the fallback for a caller that has no harness.
///
/// `auto` **probes** the model before the round starts, with a short
/// deadline, and says which realizer it chose and why. A model that is
/// not serving is named in the log, and the phrasebook words the lines.
pub fn pick_realizer_with(cfg: &AgentsConfig, phrasebook: &Phrasebook) -> Box<dyn Realizer> {
    let template = TemplateRealizer::new(phrasebook.clone());
    let deadline = Duration::from_millis(u64::from(cfg.utter.deadline_ms));
    match cfg.utter.realizer.as_str() {
        "template" => {
            say("the phrasebook words the lines (utter.realizer = template)");
            Box::new(template)
        }
        "llm" => {
            say(&format!(
                "the model {} words the lines (utter.realizer = llm)",
                cfg.utter.llm_model
            ));
            Box::new(thor_llm_realizer(cfg, deadline))
        }
        _ => match probe_llm(cfg) {
            Ok(()) => {
                say(&format!(
                    "the model {} on {}:{} is serving, so it words the lines and \
                     the phrasebook words the ones it misses (utter.realizer = auto)",
                    cfg.utter.llm_model, cfg.thor_base, cfg.utter.llm_port
                ));
                Box::new(AutoRealizer::new(
                    template,
                    thor_llm_realizer(cfg, deadline),
                ))
            }
            Err(why) => {
                say(&format!(
                    "the model {} on {}:{} did not answer the probe ({}), so the \
                     phrasebook words the lines (utter.realizer = auto)",
                    cfg.utter.llm_model, cfg.thor_base, cfg.utter.llm_port, why
                ));
                Box::new(template)
            }
        },
    }
}

/// Say what the realizer chose, and why.
///
/// The log is where the honesty lives: a round that runs with the
/// phrasebook says so, and a round that runs with the model says so too.
fn say(message: &str) {
    eprintln!("lm-agents: utter: {message}");
}

/// The probes already made, by the model they were made of.
///
/// A round has one mind per agent, and every mind picks its realizer when
/// it is built: without this the model would be asked for a line eight
/// times before the round's first tick, and eight times as long when the
/// model is not serving.
static PROBES: OnceLock<Mutex<BTreeMap<String, Result<(), String>>>> = OnceLock::new();

/// The probe of the model on the Thor: does it word a line?
///
/// The probe asks for a line, not for a health check: a server that
/// answers `/health` may still not be able to word anything, and `auto`
/// needs to know the second thing.
fn probe_llm(cfg: &AgentsConfig) -> Result<(), String> {
    let key = format!(
        "{}:{}:{}",
        cfg.thor_base, cfg.utter.llm_port, cfg.utter.llm_model
    );
    let cache = PROBES.get_or_init(|| Mutex::new(BTreeMap::new()));
    if let Some(outcome) = cache.lock().expect("the probe cache").get(&key).cloned() {
        return outcome;
    }
    let outcome = probe_llm_now(cfg);
    cache
        .lock()
        .expect("the probe cache")
        .insert(key, outcome.clone());
    outcome
}

/// The probe itself, without the cache.
fn probe_llm_now(cfg: &AgentsConfig) -> Result<(), String> {
    let timeout = Duration::from_millis(u64::from(cfg.utter.probe_ms));
    let llm = ThorLlm::new(
        &cfg.thor_base,
        cfg.utter.llm_port,
        &cfg.utter.llm_model,
        timeout,
    );
    let reply = llm
        .write(PROBE_PROMPT, Instant::now() + timeout)
        .map_err(|refusal| refusal.to_string())?;
    if words_of(&reply).is_empty() {
        return Err("the model wrote no words".to_string());
    }
    Ok(())
}

/// The prompt of the probe: the smallest line there is.
const PROBE_PROMPT: &str = "\
You are the voice of an agent of a squad in a field game. Write ONE line in \
plain words: what the agent says on the radio. Write the words of the line \
only: no quotes, no explanation, no trailing punctuation. Keep it under four \
words.";

/// The realizer of the model on the Thor.
fn thor_llm_realizer(cfg: &AgentsConfig, deadline: Duration) -> LlmRealizer {
    LlmRealizer::new(
        Box::new(ThorLlm::new(
            &cfg.thor_base,
            cfg.utter.llm_port,
            &cfg.utter.llm_model,
            deadline,
        )),
        deadline,
    )
}

/// The score of a realized line.
///
/// The words come from the realizer; the tags come from the gates. The
/// line is compiled by the notation, so a line that does not compile is a
/// refusal and never a decision (D26).
pub fn utterance_from_text(
    ctx: &TalkContext<'_>,
    plan: &UtterPlan,
    text: &str,
) -> Result<Utterance, Refusal> {
    let utterance = opal::parse(ctx, text)
        .map_err(|e| Refusal::Malformed(format!("the line does not compile: {e}")))?;
    Ok(Utterance {
        act: plan.act,
        addressee: plan.addressee,
        intent: plan.intent,
        affect: plan.affect,
        subject: plan.subject,
        ..utterance
    })
}

/// The context a line is read against when it is only checked for its
/// grammar.
///
/// The speaker and the roster are not needed to check that a line
/// compiles: the only form of the notation that needs the roster is a
/// squad colour, and the plan never writes one.
fn empty_context() -> TalkContext<'static> {
    TalkContext {
        speaker: 0,
        roster: &[],
    }
}

/// The grammar the Thor's server is asked for.
///
/// The **shape** of the model's reply is ours: a handful of words and
/// nothing else. The words themselves are the model's. Without the
/// grammar a small model answers a request like this with an explanation
/// of what it was about to say, and an explanation is not a line.
pub const WORD_GRAMMAR: &str = r#"
root ::= line
line ::= word ( " " word ){0,7}
word ::= [a-zA-Z0-9']+
"#;

/// The prompt the model is asked for the wording.
///
/// The model is asked for the **words**, not for the notation: the
/// notation is written here, from the plan, so that a model may never
/// choose a target, and so that a line the model words and a line the
/// phrasebook words are the same shape.
///
/// The prompt asks for one short line and shows what one looks like. The
/// tags of the plan are in the prompt, so the model knows what the line is
/// for, but what the line *says* about them is its own.
///
/// **The prompt carries the plan and not the facts.** The bank keeps a
/// wording for the kind of line, so the wording has to be a function of
/// the kind: the facts change every tick -- the agents move -- and a
/// wording written for one situation would be the wrong words for the
/// next. The harness's own phrasebook is keyed the same way: its
/// templates are keyed by the gesture, the intent and the kind of
/// subject. Measured, dropping the facts from the prompt takes the model
/// naming the right target from seven lines in ten to nine (see
/// `tests/realizer_on_thor.md`).
fn prompt_for(plan: &UtterPlan) -> String {
    let addressee = match plan.addressee {
        Addressee::Squad => "your squad".to_owned(),
        Addressee::All => "everybody".to_owned(),
        Addressee::Agent(agent) => format!("agent {agent}"),
    };
    let subject = match plan.subject {
        Subject::Beacon(beacon) => format!("beacon {beacon}"),
        Subject::Enemy(enemy) => format!("enemy {enemy}"),
        Subject::Direction(direction) => phrasebook::COMPASS_WORDS
            .get(usize::from(direction))
            .copied()
            .unwrap_or("here")
            .to_owned(),
        Subject::None => "nothing in particular".to_owned(),
    };
    // A line about a target says which one; a line about nothing says
    // nothing about any.
    let about = match plan.subject {
        Subject::None => "The line is about nothing in particular: do not name a beacon \
             or an enemy."
            .to_owned(),
        _ => {
            format!("The line must talk about {subject}. Do not mention any other beacon or enemy.")
        }
    };
    format!(
        "You are the voice of an agent of a squad in a field game. Write ONE \
         line in plain words: what the agent says on the radio.\n\n\
         Write a {act} for {addressee}. It is {intent} and {affect}. It is \
         about {subject}.\n\
         {about}\n\
         Write the words of the line only: no quotes, no explanation, no \
         trailing punctuation.\n\
         Keep it under eight words: it is a call over the field, not a speech.\n\n\
         Examples:\n\
         - move to beacon 2 now\n\
         - take beacon 3 at once\n\
         - hold at beacon 0\n\
         - enemy near beacon 3\n\
         - beacon 1 is ours\n\
         - where is beacon 4",
        act = phrasebook::act_name(plan.act),
        addressee = addressee,
        intent = phrasebook::intent_name(plan.intent),
        affect = phrasebook::affect_name(plan.affect),
        subject = subject,
        about = about,
    )
}

// ------------------------------------------------------------------ the model

/// A model that writes text.
///
/// The System One protocol asks typed questions and gets typed answers;
/// the wording is free text, so it has its own small door.
pub trait Llm: Send + Sync {
    /// The text the model wrote for `prompt`, before the deadline.
    fn write(&self, prompt: &str, deadline: Instant) -> Result<String, Refusal>;

    /// The model's name, for the log.
    fn model(&self) -> &str;
}

/// A model that writes scripted lines.
///
/// The stub is what the tests and the headless runner use: no network, no
/// clock, and the same line every time.
pub struct StubLlm {
    model: String,
    replies: VecDeque<Result<String, Refusal>>,
}

impl StubLlm {
    /// A stub that answers with `reply`.
    pub fn new(model: &str, reply: &str) -> Self {
        let mut replies = VecDeque::new();
        replies.push_back(Ok(reply.to_owned()));
        StubLlm {
            model: model.to_owned(),
            replies,
        }
    }

    /// A stub that refuses.
    pub fn refusing(model: &str, refusal: Refusal) -> Self {
        let mut replies = VecDeque::new();
        replies.push_back(Err(refusal));
        StubLlm {
            model: model.to_owned(),
            replies,
        }
    }
}

impl Llm for StubLlm {
    fn write(&self, _prompt: &str, _deadline: Instant) -> Result<String, Refusal> {
        self.replies
            .front()
            .cloned()
            .unwrap_or_else(|| Err(Refusal::Malformed("the stub has no reply left".to_string())))
    }

    fn model(&self) -> &str {
        &self.model
    }
}

/// The path of the Thor's chat endpoint.
pub const CHAT_PATH: &str = "/v1/chat/completions";

/// The language model on the Thor that writes the wording.
///
/// The Thor serves one model per port; this one is LFM2-1.2B, the small
/// model of D35. The endpoint is the usual chat completion shape, and the
/// port is configuration, never a guess.
pub struct ThorLlm {
    agent: ureq::Agent,
    base: String,
    port: u16,
    model: String,
}

impl ThorLlm {
    /// A model on `base:port`.
    pub fn new(base: &str, port: u16, model: &str, timeout: Duration) -> Self {
        ThorLlm {
            agent: build_llm_agent(timeout),
            base: base.trim_end_matches('/').to_owned(),
            port,
            model: model.to_owned(),
        }
    }

    /// The URL this model is asked at.
    pub fn url(&self) -> String {
        format!("{}:{}{}", self.base, self.port, CHAT_PATH)
    }

    /// The model this client asks for.
    pub fn model(&self) -> &str {
        &self.model
    }
}

impl fmt::Debug for ThorLlm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ThorLlm")
            .field("url", &self.url())
            .field("model", &self.model)
            .finish()
    }
}

impl Llm for ThorLlm {
    fn write(&self, prompt: &str, deadline: Instant) -> Result<String, Refusal> {
        let body = json!({
            "model": self.model,
            "messages": [{ "role": "user", "content": prompt }],
            "temperature": 0,
            "max_tokens": 64,
            // The shape of the reply is ours; the words are the model's.
            "grammar": WORD_GRAMMAR,
        })
        .to_string();
        let (text, _latency_ms) = post_pack(&self.agent, &self.url(), None, &body, deadline)?;
        let value: Value = serde_json::from_str(&text)
            .map_err(|e| Refusal::Malformed(format!("the reply is not JSON: {e}")))?;
        let content = value
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str)
            .ok_or_else(|| Refusal::Malformed("the reply carries no content".to_string()))?;
        Ok(content.to_owned())
    }

    fn model(&self) -> &str {
        &self.model
    }
}

/// The HTTP client of the wording model: no proxy, a short timeout.
///
/// The Thor is on the LAN, so a proxy in the environment would only break
/// the route to it.
fn build_llm_agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .timeout_per_call(Some(timeout))
        .max_idle_connections_per_host(4)
        .proxy(None)
        .build()
        .into()
}

// -------------------------------------------------------------------- the tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phrasebook::Phrasebook;
    use lm_core::utterance::{Addressee, Affect, Intent, SpeechAct, Subject};
    use std::collections::BTreeMap;

    /// A menu of one beacon and the three other moves.
    fn menu() -> Menu {
        Menu {
            id: 1,
            options: vec![
                MenuOption::GoToBeacon(0),
                MenuOption::GoToBeacon(1),
                MenuOption::GoToBeacon(2),
                MenuOption::Hold,
                MenuOption::FollowLeader,
                MenuOption::Retreat,
            ],
        }
    }

    /// The roster of the default round.
    fn roster() -> Vec<RosterEntry> {
        vec![
            RosterEntry {
                agent: 0,
                squad: 0,
                role: lm_core::header::Role::Leader,
                voice: "default".to_owned(),
                harness_commit: "none".to_owned(),
                reflex: "default".to_owned(),
            },
            RosterEntry {
                agent: 1,
                squad: 0,
                role: lm_core::header::Role::Member,
                voice: "default".to_owned(),
                harness_commit: "none".to_owned(),
                reflex: "default".to_owned(),
            },
        ]
    }

    /// The context of the plan: agent 0 of the default round.
    fn ctx(roster: &[RosterEntry]) -> GateCtx<'_> {
        GateCtx { agent: 0, roster }
    }

    /// Wait for the bank to have the wording of a line.
    ///
    /// The wording is asked on the realizer's own thread, so a line that
    /// is wanted before the answer is in hand is late. The tests wait for
    /// the answer rather than guess when it lands.
    fn wait_for_the_bank(auto: &AutoRealizer, plan: &UtterPlan, facts: &Facts) {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(2) {
            if auto.bank.take(plan, facts).is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the bank never had the wording");
    }

    /// The answers of a full pack: a command to the squad, urgent, about
    /// beacon 2.
    fn answers() -> Answers {
        let mut per_gate: BTreeMap<String, Answer> = BTreeMap::new();
        per_gate.insert(
            GATE_GESTURE.to_owned(),
            Answer::Choice {
                probs: BTreeMap::from([
                    ("command".to_owned(), 0.8),
                    ("question".to_owned(), 0.1),
                    ("statement".to_owned(), 0.05),
                    ("callout".to_owned(), 0.05),
                ]),
                confidence: 0.9,
            },
        );
        per_gate.insert(
            GATE_ATTENTION.to_owned(),
            Answer::Choice {
                probs: BTreeMap::from([
                    ("squad".to_owned(), 0.7),
                    ("all".to_owned(), 0.2),
                    ("agent1".to_owned(), 0.1),
                ]),
                confidence: 0.9,
            },
        );
        per_gate.insert(
            GATE_INTENTION.to_owned(),
            Answer::Score {
                probs: BTreeMap::from([("urgent".to_owned(), 0.8)]),
                score: 1.5,
                confidence: 0.9,
            },
        );
        per_gate.insert(
            GATE_INTENT.to_owned(),
            Answer::Choice {
                probs: BTreeMap::from([
                    ("order".to_owned(), 0.8),
                    ("ack".to_owned(), 0.1),
                    ("warn".to_owned(), 0.05),
                    ("query".to_owned(), 0.03),
                    ("report".to_owned(), 0.02),
                ]),
                confidence: 0.9,
            },
        );
        per_gate.insert(
            GATE_SUBJECT.to_owned(),
            Answer::Choice {
                probs: BTreeMap::from([
                    ("opt0".to_owned(), 0.1),
                    ("opt1".to_owned(), 0.1),
                    ("opt2".to_owned(), 0.7),
                    ("opt3".to_owned(), 0.03),
                    ("opt4".to_owned(), 0.03),
                    ("opt5".to_owned(), 0.04),
                ]),
                confidence: 0.9,
            },
        );
        for gate in UTTERANCE_GATES {
            per_gate.insert(presence_gate(gate), Answer::Noul(0.9));
        }
        Answers {
            per_gate,
            unsure: BTreeMap::new(),
            escalated: BTreeMap::new(),
            model: "stub".to_owned(),
            input_tokens: 64,
            latency_ms: 12,
        }
    }

    /// The facts of a moment, for the realizers.
    fn facts() -> Facts {
        Facts {
            text: "You are agent 0, the leader, of squad 0 at column 2, row 2.".to_owned(),
            key: 0x1234_5678_9abc_def0,
            fields: BTreeMap::new(),
        }
    }

    #[test]
    fn the_gates_choose_the_line() {
        let roster = roster();
        let plan = plan_from_gates(&answers(), &menu(), &ctx(&roster)).expect("the plan is made");
        assert_eq!(plan.act, SpeechAct::Command);
        assert_eq!(plan.addressee, Addressee::Squad);
        assert_eq!(plan.affect, Affect::Urgent);
        assert_eq!(plan.intent, Intent::Order);
        assert_eq!(plan.subject, Subject::Beacon(2));
        assert!(plan.should_speak);
    }

    #[test]
    fn the_presence_gate_silences_the_agent() {
        let roster = roster();
        let mut answers = answers();
        answers
            .per_gate
            .insert(presence_gate(GATE_GESTURE), Answer::Noul(0.0));
        let plan = plan_from_gates(&answers, &menu(), &ctx(&roster)).expect("the plan is made");
        assert!(!plan.should_speak, "a noul of zero means silence");
    }

    #[test]
    fn a_missing_gate_is_a_refusal() {
        let roster = roster();
        let mut answers = answers();
        answers.per_gate.remove(GATE_INTENT);
        assert!(matches!(
            plan_from_gates(&answers, &menu(), &ctx(&roster)),
            Err(Refusal::Malformed(_))
        ));
    }

    #[test]
    fn the_subject_comes_from_the_menu() {
        let roster = roster();
        let mut answers = answers();
        // An answer that names an option the menu does not have.
        let Answer::Choice { probs, .. } = answers
            .per_gate
            .get_mut(GATE_SUBJECT)
            .expect("the subject gate is answered")
        else {
            panic!("the subject gate is a choice");
        };
        probs.clear();
        probs.insert("opt9".to_owned(), 0.9);
        assert!(matches!(
            plan_from_gates(&answers, &menu(), &ctx(&roster)),
            Err(Refusal::MenuChanged)
        ));
    }

    #[test]
    fn the_affect_is_bucketed_by_code() {
        assert_eq!(bucket_affect(0.0), Affect::Calm);
        assert_eq!(bucket_affect(0.4), Affect::Calm);
        assert_eq!(bucket_affect(0.5), Affect::Uncertain);
        assert_eq!(bucket_affect(1.0), Affect::Uncertain);
        assert_eq!(bucket_affect(1.01), Affect::Urgent);
        assert_eq!(bucket_affect(2.0), Affect::Urgent);
    }

    #[test]
    fn an_addressee_outside_the_round_is_refused() {
        let roster = roster();
        let mut answers = answers();
        let Answer::Choice { probs, .. } = answers
            .per_gate
            .get_mut(GATE_ATTENTION)
            .expect("the attention gate is answered")
        else {
            panic!("the attention gate is a choice");
        };
        probs.clear();
        probs.insert("agent9".to_owned(), 0.9);
        assert!(matches!(
            plan_from_gates(&answers, &menu(), &ctx(&roster)),
            Err(Refusal::Malformed(_))
        ));
    }

    #[test]
    fn the_phrasebook_renders_the_line() {
        let roster = roster();
        let plan = plan_from_gates(&answers(), &menu(), &ctx(&roster)).expect("the plan is made");
        let realizer = TemplateRealizer::shipped();
        let worded = realizer
            .realize(&plan, &facts())
            .expect("the line is worded");
        assert_eq!(worded.source, WordSource::Phrasebook);
        assert!(!worded.late, "the phrasebook is never late");
        let utterance = opal::parse(
            &TalkContext {
                speaker: 0,
                roster: &roster,
            },
            &worded.text,
        )
        .expect("the line compiles");
        assert_eq!(
            utterance.subject,
            Subject::Beacon(2),
            "the subject survives"
        );
        assert_eq!(utterance.act, SpeechAct::Command);
        assert_eq!(utterance.intent, Intent::Order);
        assert_eq!(utterance.affect, Affect::Urgent);
        assert_eq!(utterance.addressee, Addressee::Squad);
        assert!(!utterance.words.is_empty());
    }

    #[test]
    fn a_missing_phrasebook_entry_is_a_refusal() {
        let roster = roster();
        let plan = plan_from_gates(&answers(), &menu(), &ctx(&roster)).expect("the plan is made");
        let realizer = TemplateRealizer::new(Phrasebook::default());
        assert!(matches!(
            realizer.realize(&plan, &facts()),
            Err(Refusal::Malformed(_))
        ));
    }

    #[test]
    fn bad_realizer_output_is_a_refusal() {
        let roster = roster();
        let plan = plan_from_gates(&answers(), &menu(), &ctx(&roster)).expect("the plan is made");
        // The model writes no words at all: there is no line to say.
        let realizer = LlmRealizer::new(
            Box::new(StubLlm::new("stub", "   ")),
            Duration::from_millis(100),
        );
        assert!(matches!(
            realizer.realize(&plan, &facts()),
            Err(Refusal::Malformed(_))
        ));
    }

    #[test]
    fn the_model_may_not_choose_a_target() {
        let roster = roster();
        let plan = plan_from_gates(&answers(), &menu(), &ctx(&roster)).expect("the plan is made");
        // The model writes a line about another beacon: the gates chose the
        // subject, so the wording is refused rather than believed (D37).
        let realizer = LlmRealizer::new(
            Box::new(StubLlm::new("stub", "move to beacon 9 now")),
            Duration::from_millis(100),
        );
        assert!(
            matches!(
                realizer.realize(&plan, &facts()),
                Err(Refusal::Malformed(_))
            ),
            "a line about another target is refused"
        );

        // The model writes a line that names no target: the tags of the
        // score are the gates' answer, whatever the model wrote.
        let realizer = LlmRealizer::new(
            Box::new(StubLlm::new("stub", "hold on")),
            Duration::from_millis(100),
        );
        let worded = realizer
            .realize(&plan, &facts())
            .expect("the line is worded");
        let utterance = opal::parse(
            &TalkContext {
                speaker: 0,
                roster: &roster,
            },
            &worded.text,
        )
        .expect("the line compiles");
        assert_eq!(utterance.addressee, Addressee::Squad, "the gate decides");
        assert_eq!(utterance.intent, Intent::Order);
        assert_eq!(utterance.subject, Subject::Beacon(2));
        assert_eq!(utterance.words[0].text, "hold", "the words are the model's");
    }

    #[test]
    fn llm_down_falls_back_to_the_template() {
        let roster = roster();
        let plan = plan_from_gates(&answers(), &menu(), &ctx(&roster)).expect("the plan is made");
        let template = TemplateRealizer::shipped();
        let down = || {
            LlmRealizer::new(
                Box::new(StubLlm::refusing("stub", Refusal::Timeout)),
                Duration::from_millis(100),
            )
        };

        // `auto`: the model missed the line's deadline, so the phrasebook
        // words the line, and the line is counted late.
        let auto = AutoRealizer::new(template.clone(), down());
        let worded = auto.realize(&plan, &facts()).expect("the line is worded");
        assert_eq!(worded.source, WordSource::Phrasebook);
        assert!(worded.late, "the model missed the deadline");

        // `llm` alone: the model is down, so nothing is said.
        assert!(matches!(
            down().realize(&plan, &facts()),
            Err(Refusal::Timeout)
        ));
    }

    #[test]
    fn the_auto_realizer_asks_the_model_first() {
        let roster = roster();
        let plan = plan_from_gates(&answers(), &menu(), &ctx(&roster)).expect("the plan is made");
        // The phrasebook has the line, and the model is asked anyway: `auto`
        // is the model first, and the phrasebook is the fallback for the
        // lines the model has not answered yet.
        let model = LlmRealizer::new(
            Box::new(StubLlm::new("stub", "move to beacon 2 now")),
            Duration::from_millis(100),
        );
        let auto = AutoRealizer::new(TemplateRealizer::shipped(), model);

        // The game does not wait on the wording: the first line of this
        // kind is worded by the phrasebook, and it is late.
        let first = auto.realize(&plan, &facts()).expect("the line is worded");
        assert_eq!(first.source, WordSource::Phrasebook);
        assert!(first.late, "the model's words were not in hand");

        // The model's words land, and the next line of this kind is
        // worded by the model.
        wait_for_the_bank(&auto, &plan, &facts());
        let worded = auto
            .realize(&plan, &facts())
            .expect("the model words the line");
        assert_eq!(worded.source, WordSource::Llm);
        assert!(!worded.late);
        assert!(
            worded.text.contains("move"),
            "the model's words are used: {}",
            worded.text
        );
    }

    #[test]
    fn a_wording_the_model_got_wrong_is_not_late() {
        let roster = roster();
        let plan = plan_from_gates(&answers(), &menu(), &ctx(&roster)).expect("the plan is made");
        // The model answered, but it named another beacon: the phrasebook
        // words the line, and the line is not late -- it was wrong.
        let model = LlmRealizer::new(
            Box::new(StubLlm::new("stub", "move to beacon 9 now")),
            Duration::from_millis(100),
        );
        let auto = AutoRealizer::new(TemplateRealizer::shipped(), model);
        // The first line asks the model; the second one has its answer.
        let _ = auto.realize(&plan, &facts()).expect("the line is worded");
        wait_for_the_bank(&auto, &plan, &facts());
        let worded = auto.realize(&plan, &facts()).expect("the line is worded");
        assert_eq!(worded.source, WordSource::Phrasebook);
        assert!(!worded.late, "the model was in time, and wrong");
    }

    #[test]
    fn the_probe_fails_when_the_model_is_not_serving() {
        // Nothing listens on port 9.
        let cfg = AgentsConfig {
            thor_base: "http://127.0.0.1:9".to_owned(),
            utter: AgentsConfig::default().utter.with_probe_ms(50),
            ..AgentsConfig::default()
        };
        assert!(probe_llm(&cfg).is_err(), "the model is not there");
    }

    #[test]
    fn auto_falls_back_to_the_phrasebook_when_the_probe_fails() {
        let cfg = AgentsConfig {
            thor_base: "http://127.0.0.1:9".to_owned(),
            utter: AgentsConfig::default().utter.with_probe_ms(50),
            ..AgentsConfig::default()
        };
        let roster = roster();
        let plan = plan_from_gates(&answers(), &menu(), &ctx(&roster)).expect("the plan is made");
        let realizer = pick_realizer_with(&cfg, &Phrasebook::shipped());
        let worded = realizer
            .realize(&plan, &facts())
            .expect("the line is worded");
        assert_eq!(
            worded.source,
            WordSource::Phrasebook,
            "a model that is not serving does not word the lines"
        );
        assert!(
            !worded.late,
            "the model was never asked, so it was never late"
        );
    }

    #[test]
    fn the_model_reply_is_trimmed_of_its_quotes() {
        assert_eq!(
            words_of("  \"move to beacon 2 now\"  "),
            "move to beacon 2 now"
        );
        assert_eq!(words_of(""), "");
    }

    #[test]
    fn another_target_is_caught() {
        assert!(other_target("move to beacon 9 now", Subject::Beacon(2)).is_some());
        assert!(other_target("move to beacon 2 now", Subject::Beacon(2)).is_none());
        assert!(other_target("hold on", Subject::Beacon(2)).is_none());
        assert!(
            other_target("move to beacon 2 now", Subject::None).is_some(),
            "a line about a target the gates did not choose is refused"
        );
        assert!(other_target("enemy 4 is near", Subject::Enemy(4)).is_none());
        assert!(other_target("enemy 5 is near", Subject::Enemy(4)).is_some());
    }
}
