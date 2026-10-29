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
//! ```text
//! ScoreView --facts--> plain-language facts
//!          --gates--> the choice: act, addressee, intent, affect, subject
//!          --realize--> the wording, in the notation (lm_core::opal)
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

#![forbid(unsafe_code)]

use std::collections::VecDeque;
use std::fmt;
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    /// (D26).
    fn realize(&self, plan: &UtterPlan, facts: &Facts) -> Result<String, Refusal>;
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
    fn realize(&self, plan: &UtterPlan, facts: &Facts) -> Result<String, Refusal> {
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
        Ok(entry.render(plan, facts.key))
    }
}

/// The realizer of a small language model on the Thor.
///
/// The model is asked for the notation and nothing else, and the line it
/// writes is **compiled before it is used**: a line that does not compile
/// is a refusal (D26), and the bad line never reaches the log. The tags
/// are the gates' answer, whatever the model wrote: the header is rebuilt
/// from the plan, so a model may never choose a target (D37).
pub struct LlmRealizer {
    llm: Box<dyn Llm>,
    timeout: Duration,
}

impl LlmRealizer {
    /// A realizer that asks `llm`.
    pub fn new(llm: Box<dyn Llm>, timeout: Duration) -> Self {
        LlmRealizer { llm, timeout }
    }
}

impl Realizer for LlmRealizer {
    fn realize(&self, plan: &UtterPlan, facts: &Facts) -> Result<String, Refusal> {
        let deadline = Instant::now() + self.timeout;
        let written = self.llm.write(&prompt_for(plan, facts), deadline)?;
        let line = rebuild_with_plan(plan, &written)
            .ok_or_else(|| Refusal::Malformed("the model wrote no words".to_string()))?;
        // The line must compile: a line that does not is a refusal, and it
        // never reaches the log.
        opal::parse(&empty_context(), &line).map_err(|e| {
            Refusal::Malformed(format!(
                "the model {} wrote a line that does not compile: {e}",
                self.llm.model()
            ))
        })?;
        Ok(line)
    }
}

/// The automatic realizer: the template first, the model only when the
/// phrasebook has no line for this kind of line.
///
/// If the model fails, the line is **not** spoken: silence is an outcome
/// (D26).
pub struct AutoRealizer {
    template: TemplateRealizer,
    llm: LlmRealizer,
}

impl AutoRealizer {
    /// The automatic realizer over a phrasebook and a model.
    pub fn new(template: TemplateRealizer, llm: LlmRealizer) -> Self {
        AutoRealizer { template, llm }
    }
}

impl Realizer for AutoRealizer {
    fn realize(&self, plan: &UtterPlan, facts: &Facts) -> Result<String, Refusal> {
        if self.template.phrasebook().has(plan.act, plan.intent) {
            return self.template.realize(plan, facts);
        }
        self.llm.realize(plan, facts)
    }
}

/// The realizer the configuration asks for.
///
/// `utter.realizer` is `template`, `llm` or `auto`; `auto` is the
/// default: the phrasebook words what it can and the model words the
/// rest.
pub fn pick_realizer(cfg: &AgentsConfig) -> Box<dyn Realizer> {
    pick_realizer_with(cfg, &Phrasebook::shipped())
}

/// The realizer the configuration asks for, over a phrasebook.
///
/// The harness's own phrasebook is the one a round plays with: the
/// shipped one is only the fallback for a caller that has no harness.
pub fn pick_realizer_with(cfg: &AgentsConfig, phrasebook: &Phrasebook) -> Box<dyn Realizer> {
    let timeout = Duration::from_millis(u64::from(cfg.request_timeout_ms));
    let template = TemplateRealizer::new(phrasebook.clone());
    match cfg.utter.realizer.as_str() {
        "template" => Box::new(template),
        "llm" => Box::new(thor_llm_realizer(cfg, timeout)),
        _ => Box::new(AutoRealizer::new(template, thor_llm_realizer(cfg, timeout))),
    }
}

/// The realizer of the model on the Thor.
fn thor_llm_realizer(cfg: &AgentsConfig, timeout: Duration) -> LlmRealizer {
    LlmRealizer::new(
        Box::new(ThorLlm::new(
            &cfg.thor_base,
            cfg.utter.llm_port,
            &cfg.utter.llm_model,
            timeout,
        )),
        timeout,
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

/// The header of a line, in the notation.
fn header_of(plan: &UtterPlan, channel: Channel) -> String {
    let skeleton = Utterance {
        speaker: 0,
        channel,
        addressee: plan.addressee,
        act: plan.act,
        intent: plan.intent,
        affect: plan.affect,
        subject: plan.subject,
        prosody: lm_core::utterance::Prosody::Synthesized,
        words: Vec::new(),
    };
    opal::render(&skeleton)
}

/// Rebuild a written line with the plan's header.
///
/// The model writes the **words**; the header is ours. That is what keeps
/// a model from choosing a target: the addressee, the intent, the affect
/// and the subject of the line are the gates' answer, whatever the model
/// wrote (D37).
fn rebuild_with_plan(plan: &UtterPlan, written: &str) -> Option<String> {
    let mut lines = written.lines().map(str::trim).filter(|l| !l.is_empty());
    let first = lines.next()?;
    // A line that starts with the marker has a header; the words are what
    // follows it. A line without one is all words.
    let (channel, words) = if first.starts_with('@') {
        (channel_of(first), lines.collect::<Vec<&str>>())
    } else {
        (
            Channel::Voice,
            std::iter::once(first).chain(lines).collect::<Vec<&str>>(),
        )
    };
    if words.is_empty() {
        return None;
    }
    let mut out = header_of(plan, channel);
    for word in words {
        out.push('\n');
        out.push_str(word);
    }
    Some(out)
}

/// The channel a written header names, if it names one.
fn channel_of(header: &str) -> Channel {
    for token in header.split_whitespace() {
        if let Some(value) = token.strip_prefix("channel=") {
            return match value {
                "shout" => Channel::Shout,
                "whisper" => Channel::Whisper,
                "radio" => Channel::Radio,
                _ => Channel::Voice,
            };
        }
    }
    Channel::Voice
}

/// The prompt the model is asked for the wording.
///
/// The model is asked for the notation and nothing else: a line, no
/// explanation, no quotes. The tags of the plan are in the prompt, so the
/// model knows what the line is for, but the header it writes is thrown
/// away and rebuilt from the plan.
fn prompt_for(plan: &UtterPlan, facts: &Facts) -> String {
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
    format!(
        "You are the voice of an agent of a squad in a field game. Write \
         ONE line in the OPAL notation and nothing else: no explanation, no \
         quotes, no code fence.\n\n\
         The line is a header and one line per word:\n\n  \
         @squad say to=squad intent=order affect=calm subject=none \
         channel=voice\n    hold p0 a. l1\n    the p0 a. l1\n\n\
         The gesture after the marker is one of say, ask, order, call.\n\
         The header's keys are:\n  \
         to       squad | all | agent<number>\n  \
         intent   order | ack | warn | query | report\n  \
         affect   calm | urgent | uncertain\n  \
         subject  none | beacon<number> | enemy<number> | dir<number>\n  \
         channel  voice | radio | shout | whisper\n\
         A word line is two spaces, the word, the pitch from the speaker's \
         baseline (p0, p+2, p-1), the accent (a. , a! , a!!) and the \
         length in beats (l1, l2).\n\n\
         The line must be {act} to the {addressee}, {intent}, {affect}, \
         about {subject}. Keep it under eight words: it is a call over \
         the field, not a speech.\n\n\
         The situation:\n{facts}",
        act = phrasebook::act_name(plan.act),
        addressee = addressee,
        intent = phrasebook::intent_name(plan.intent),
        affect = phrasebook::affect_name(plan.affect),
        subject = subject,
        facts = facts.text,
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
            "max_tokens": 96,
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
        let text = realizer
            .realize(&plan, &facts())
            .expect("the line is worded");
        let utterance = opal::parse(
            &TalkContext {
                speaker: 0,
                roster: &roster,
            },
            &text,
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
        let realizer = LlmRealizer::new(
            Box::new(StubLlm::new("stub", "hold on")),
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
        // The model writes a line for somebody else: the header is thrown
        // away and rebuilt from the plan.
        let realizer = LlmRealizer::new(
            Box::new(StubLlm::new(
                "stub",
                "@all call to=agent9 intent=warn affect=urgent subject=enemy9 channel=shout\n  \
                 run p0 a. l1",
            )),
            Duration::from_millis(100),
        );
        let text = realizer
            .realize(&plan, &facts())
            .expect("the line is worded");
        let utterance = opal::parse(
            &TalkContext {
                speaker: 0,
                roster: &roster,
            },
            &text,
        )
        .expect("the line compiles");
        assert_eq!(utterance.addressee, Addressee::Squad, "the gate decides");
        assert_eq!(utterance.intent, Intent::Order);
        assert_eq!(utterance.subject, Subject::Beacon(2));
        assert_eq!(utterance.words[0].text, "run", "the words are the model's");
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

        // `auto`: the phrasebook has the line, so the model is never asked.
        let auto = AutoRealizer::new(template.clone(), down());
        assert!(auto.realize(&plan, &facts()).is_ok());

        // `llm` alone: the model is down, so nothing is said.
        assert!(matches!(
            down().realize(&plan, &facts()),
            Err(Refusal::Timeout)
        ));
    }

    #[test]
    fn the_auto_realizer_asks_the_model_only_when_the_phrasebook_has_no_line() {
        let roster = roster();
        let plan = plan_from_gates(&answers(), &menu(), &ctx(&roster)).expect("the plan is made");
        let empty = TemplateRealizer::new(Phrasebook::default());
        let model = LlmRealizer::new(
            Box::new(StubLlm::new(
                "stub",
                "@squad order to=squad intent=order affect=urgent subject=beacon2 channel=voice\n  \
                 move p0 a. l1",
            )),
            Duration::from_millis(100),
        );
        let auto = AutoRealizer::new(empty, model);
        let text = auto
            .realize(&plan, &facts())
            .expect("the model words the line");
        assert!(text.contains("move"), "the model's words are used: {text}");
    }
}
