//! The OPAL-style utterance notation: text in, score out, and back again.
//!
//! PLATO's OPAL (Octave-Pitch-Accent-Length) was a music language with a
//! compiler: a compact, line-oriented score that a person, an LLM and a
//! decision model can all read and write, and that compiles deterministically
//! into the lanes of a sound (fleet/07). This module is last-mile's OPAL: the
//! wording of a line, written down, and the **score** it compiles to.
//!
//! ```text
//! @squad say to=squad intent=order affect=urgent
//!   hold p+2 a! l2
//!   the p0 a. l1
//!   north p+3 a!! l3
//!   beacon p+1 a. l2 ~rest1
//! ```
//!
//! A header line says *what kind of line this is* -- who it is for, what it
//! is for, how it sounds -- and one line per word says how that word is
//! sung: its pitch step from the speaker's baseline, its accent, its length
//! in beats and the rest that follows it.
//!
//! The rules of the compiler are the rules of the score, and they are all
//! integers: the lanes come out of the text alone, so the same words always
//! compile to the same score on every machine (D12, D26). A line that does
//! not fit the grammar is a **refusal with a name** -- never a guess, never
//! a silent default (D26) -- and the realizer that produced it falls back
//! to the phrasebook or to silence.
//!
//! The wording is separate from the choice (D37): the decision model picks
//! the act, the addressee, the intent, the affect and the subject, and this
//! notation is how the *wording* is written down. The score is the truth;
//! what a listener hears is the score as degraded by the hearing physics.

use crate::decision::Tick;
use crate::header::RosterEntry;
use crate::utterance::{
    Addressee, Affect, Channel, Intent, Prosody, SpeechAct, Subject, Utterance, Word,
    MAX_PITCH_STEPS, MIN_PITCH_STEPS, Q16_ONE,
};

/// One beat of the notation, in ticks.
///
/// A beat is the notation's unit of time: `l2` is a word of two beats, and
/// `~rest1` is a rest of one beat. It is the same as the round's default
/// `TalkParams::beat_ticks` -- a test below keeps the two honest.
pub const BEAT_TICKS: u16 = 5;

/// The shortest word the notation may spell, in ticks.
///
/// A word of zero beats is still a word: the notation never spells a word
/// shorter than this.
pub const MIN_WORD_TICKS: u16 = 2;

/// The squad colours, in squad-index order.
///
/// The colours are fixed by the squad's index, so a line can name the squad
/// it is for in words (`to=red`) instead of in numbers. Squad 0 is red,
/// squad 1 is blue, and so on.
pub const SQUAD_COLOURS: [&str; 6] = ["red", "blue", "green", "yellow", "purple", "orange"];

/// Who is speaking, and who exists.
///
/// The context is what the notation cannot say by itself: the **speaker**
/// (the score's `speaker` field) and the **roster** the line is read against
/// -- a squad colour is resolved against it, so a line may only speak for
/// the squad its speaker belongs to.
#[derive(Clone, Copy, Debug)]
pub struct TalkContext<'a> {
    /// The agent who speaks the line.
    pub speaker: u16,
    /// The round's roster: who exists, and which squad each belongs to.
    pub roster: &'a [RosterEntry],
}

/// A refusal: the notation did not compile, and here is why.
///
/// Every variant is a **named** refusal (D26). Nothing here is ever repaired
/// by guessing: a broken line is dropped, and the caller falls back to the
/// template or to silence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpalError {
    /// The line is empty: no words at all, or no header at all.
    Empty,
    /// The header is malformed: a missing `@`, an unknown act, a missing
    /// intent, a duplicated key, or a value that is not in the vocabulary.
    BadHeader,
    /// A key the notation does not have (`mood=happy`).
    UnknownKey,
    /// The accent is not `.`, `!` or `!!`.
    BadAccent,
    /// The pitch is not a step, or not one a voice can sing.
    BadPitch,
    /// The length or the rest is not a number of beats.
    BadLength,
    /// The line addresses a squad the speaker does not belong to.
    ForeignSquad,
    /// The subject could not be named.
    NoSubject,
}

impl std::fmt::Display for OpalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            OpalError::Empty => "the line is empty",
            OpalError::BadHeader => "the header is malformed",
            OpalError::UnknownKey => "the key is not one the notation has",
            OpalError::BadAccent => "the accent is not '.', '!' or '!!'",
            OpalError::BadPitch => "the pitch is not a step a voice can sing",
            OpalError::BadLength => "the length or the rest is not a number of beats",
            OpalError::ForeignSquad => "the line speaks for a squad the speaker is not in",
            OpalError::NoSubject => "the subject could not be named",
        };
        f.write_str(text)
    }
}

impl std::error::Error for OpalError {}

/// The accent of a word that carries no stress, in Q16.
const EMPHASIS_NONE: u32 = 0;
/// The accent of a stressed word, in Q16.
const EMPHASIS_STRESSED: u32 = 32_768;
/// The accent of an emphatic word, in Q16.
const EMPHASIS_EMPHATIC: u32 = 65_536;

/// The loudest a word may be, in Q16.
const MAX_ENERGY_Q16: u32 = 65_536;
/// The fastest a word may be, in Q16 (twice the baseline pace).
const MAX_RATE_Q16: u32 = 131_072;
/// The longest rest the notation may spell, in beats: the pause after a word
/// is a number of ticks, and a tick count must fit in a `u16`.
const MAX_REST_BEATS: u16 = u16::MAX / BEAT_TICKS;

/// The parts of a header line.
struct Header {
    addressee: Addressee,
    act: SpeechAct,
    intent: Intent,
    affect: Affect,
    subject: Subject,
    channel: Channel,
}

/// Compile the notation into a score: text in, the score of a line out.
///
/// The header says what kind of line this is and one line per word says how
/// that word is sung; the compiler turns both into the score's lanes, in
/// integers, with no clock and no model (D12). The same text always
/// compiles to the same score.
///
/// A line that does not fit the grammar is a **refusal** with a name
/// ([`OpalError`]), never a guess (D26): the caller falls back to the
/// template or to silence, and logs that it did.
pub fn parse(ctx: &TalkContext<'_>, text: &str) -> Result<Utterance, OpalError> {
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    let header_line = lines.next().ok_or(OpalError::Empty)?;
    let header = parse_header(ctx, header_line)?;

    let mut words: Vec<Word> = Vec::new();
    let mut clock: Tick = 0;
    for line in lines {
        words.push(parse_word(&header, line, &mut clock)?);
    }
    if words.is_empty() {
        // A header with no words is not a quiet line: it is an empty one,
        // and an empty line is a refusal, never a zero-length score.
        return Err(OpalError::Empty);
    }

    Ok(Utterance {
        speaker: ctx.speaker,
        channel: header.channel,
        addressee: header.addressee,
        act: header.act,
        intent: header.intent,
        affect: header.affect,
        subject: header.subject,
        prosody: Prosody::Synthesized,
        words,
    })
}

/// Write a score back out in the canonical notation.
///
/// The canonical form is one header line -- the scope, the act, and then
/// `to=`, `intent=`, `affect=`, and the optional `subject=` and `channel=`
/// -- and one line per word, indented by two spaces:
///
/// ```text
/// @squad say to=squad intent=order affect=urgent
///   hold p+2 a! l2
///   the p0 a. l1
/// ```
///
/// The lines are joined by newlines and the text carries no trailing
/// newline, so a score renders to the same bytes every time.
///
/// A word is written out as it is: the notation spells one word per line, so
/// a word that is not a single token cannot be written back out. The score's
/// own `prose()` is the text for the TTS; this is the text for a human, an
/// LLM and a decision model to read.
pub fn render(u: &Utterance) -> String {
    let mut out = String::new();
    out.push('@');
    out.push_str(&addressee_word(u.addressee));
    out.push(' ');
    out.push_str(act_word(u.act));
    out.push_str(" to=");
    out.push_str(&addressee_word(u.addressee));
    out.push_str(" intent=");
    out.push_str(intent_word(u.intent));
    out.push_str(" affect=");
    out.push_str(affect_word(u.affect));
    if u.subject != Subject::None {
        out.push_str(" subject=");
        out.push_str(&subject_word(u.subject));
    }
    if u.channel != Channel::Voice {
        out.push_str(" channel=");
        out.push_str(channel_word(u.channel));
    }
    for word in &u.words {
        out.push('\n');
        out.push_str("  ");
        out.push_str(&word.text);
        out.push(' ');
        out.push_str(&pitch_word(word.pitch_steps));
        out.push(' ');
        out.push('a');
        out.push_str(accent_word(word.emphasis_q16));
        out.push(' ');
        out.push_str(&length_word(word));
        if word.pause_after_ticks > 0 {
            out.push_str(" ~rest");
            out.push_str(&rest_word(word.pause_after_ticks));
        }
    }
    out
}

/// Read the header line: the scope, the act and the keys.
fn parse_header(ctx: &TalkContext<'_>, line: &str) -> Result<Header, OpalError> {
    let mut tokens = line.split_whitespace();
    // The scope is written against the marker: `@squad`, not `@ squad`.
    let marker = tokens.next().ok_or(OpalError::BadHeader)?;
    let scope = marker.strip_prefix('@').ok_or(OpalError::BadHeader)?;
    if scope.is_empty() {
        return Err(OpalError::BadHeader);
    }
    let act = tokens.next().ok_or(OpalError::BadHeader)?;

    // The scope is the addressee unless the line names one itself.
    let mut addressee = parse_scope(scope)?;
    let mut to_seen = false;
    let mut intent: Option<Intent> = None;
    let mut affect: Option<Affect> = None;
    let mut subject: Option<Subject> = None;
    let mut channel: Option<Channel> = None;

    for token in tokens {
        let (key, value) = token.split_once('=').ok_or(OpalError::BadHeader)?;
        match key {
            "to" => {
                if to_seen {
                    return Err(OpalError::BadHeader);
                }
                to_seen = true;
                addressee = parse_addressee(ctx, value)?;
            }
            "intent" => {
                if intent.is_some() {
                    return Err(OpalError::BadHeader);
                }
                intent = Some(parse_intent(value)?);
            }
            "affect" => {
                if affect.is_some() {
                    return Err(OpalError::BadHeader);
                }
                affect = Some(parse_affect(value)?);
            }
            "subject" => {
                if subject.is_some() {
                    return Err(OpalError::BadHeader);
                }
                subject = Some(parse_subject(value)?);
            }
            "channel" => {
                if channel.is_some() {
                    return Err(OpalError::BadHeader);
                }
                channel = Some(parse_channel(value)?);
            }
            _ => return Err(OpalError::UnknownKey),
        }
    }

    Ok(Header {
        addressee,
        act: parse_act(act)?,
        // A line that does not say what it is for, or how it sounds, is
        // missing half of itself: there is no default to invent (D26).
        intent: intent.ok_or(OpalError::BadHeader)?,
        affect: affect.ok_or(OpalError::BadHeader)?,
        subject: subject.unwrap_or(Subject::None),
        channel: channel.unwrap_or(Channel::Voice),
    })
}

/// Read one word line: the word, its pitch, its accent, its length and the
/// rest that follows it.
fn parse_word(header: &Header, line: &str, clock: &mut Tick) -> Result<Word, OpalError> {
    let mut tokens = line.split_whitespace();
    let text = tokens.next().ok_or(OpalError::BadHeader)?.to_owned();
    let pitch_steps = parse_pitch(tokens.next().ok_or(OpalError::BadPitch)?)?;
    let emphasis_q16 = parse_accent(tokens.next().ok_or(OpalError::BadAccent)?)?;
    let beats = parse_beats(tokens.next().ok_or(OpalError::BadLength)?)?;
    let rest = match tokens.next() {
        Some(token) => parse_rest(token)?,
        None => 0,
    };
    if tokens.next().is_some() {
        return Err(OpalError::BadHeader);
    }

    // A word lasts at least the shortest word the notation may spell.
    let word_ticks = (u64::from(BEAT_TICKS) * u64::from(beats)).max(u64::from(MIN_WORD_TICKS));
    let t_on = *clock;
    let t_off = t_on + word_ticks;
    let pause_after_ticks = u64::from(BEAT_TICKS) * u64::from(rest);
    // The rest is silence, and the next word waits it out.
    *clock = t_off + pause_after_ticks;

    Ok(Word {
        text,
        t_on,
        t_off,
        pitch_steps,
        energy_q16: energy_q16(header.affect, emphasis_q16),
        rate_q16: rate_q16(header.affect, beats),
        emphasis_q16,
        pause_after_ticks: u16::try_from(pause_after_ticks).unwrap_or(u16::MAX),
    })
}

/// The loudness of a word: the affect's loudness, lifted by half the accent
/// and capped at one whole.
fn energy_q16(affect: Affect, emphasis_q16: u32) -> u32 {
    (affect_energy_q16(affect) + emphasis_q16 / 2).min(MAX_ENERGY_Q16)
}

/// The pace of a word: the affect's pace, scaled by how long the word is.
fn rate_q16(affect: Affect, beats: u16) -> u32 {
    let beats = u64::from(beats.max(1));
    let raw =
        (u64::from(Q16_ONE) / beats) * u64::from(affect_rate_q16(affect)) / u64::from(Q16_ONE);
    u32::try_from(raw.min(u64::from(MAX_RATE_Q16))).unwrap_or(MAX_RATE_Q16)
}

/// The loudness each affect carries, in Q16.
fn affect_energy_q16(affect: Affect) -> u32 {
    match affect {
        Affect::Calm => 16_384,
        Affect::Uncertain => 24_576,
        Affect::Urgent => 49_152,
    }
}

/// The pace each affect carries, in Q16.
fn affect_rate_q16(affect: Affect) -> u32 {
    match affect {
        Affect::Calm => 58_704,
        Affect::Uncertain => 65_536,
        Affect::Urgent => 78_432,
    }
}

/// The scope of a line: who it is spoken to, unless it says otherwise.
fn parse_scope(scope: &str) -> Result<Addressee, OpalError> {
    match scope {
        "squad" => Ok(Addressee::Squad),
        "all" => Ok(Addressee::All),
        _ => parse_agent(scope).map(Addressee::Agent),
    }
}

/// The addressee a line names: the squad, everyone, one agent, or a squad
/// by its colour.
fn parse_addressee(ctx: &TalkContext<'_>, value: &str) -> Result<Addressee, OpalError> {
    match value {
        "squad" => return Ok(Addressee::Squad),
        "all" => return Ok(Addressee::All),
        _ => {}
    }
    if value.starts_with("agent") {
        return parse_agent(value).map(Addressee::Agent);
    }
    match squad_of_colour(value) {
        // The speaker may speak for their own squad and for nobody else's.
        Some(squad) if speaker_squad(ctx) == Some(squad) => Ok(Addressee::Squad),
        Some(_) => Err(OpalError::ForeignSquad),
        None => Err(OpalError::BadHeader),
    }
}

/// One agent, by id.
fn parse_agent(value: &str) -> Result<u16, OpalError> {
    let digits = value.strip_prefix("agent").ok_or(OpalError::BadHeader)?;
    let index = index_of(digits).ok_or(OpalError::BadHeader)?;
    u16::try_from(index).map_err(|_| OpalError::BadHeader)
}

/// The act a line makes.
fn parse_act(act: &str) -> Result<SpeechAct, OpalError> {
    match act {
        "say" => Ok(SpeechAct::Statement),
        "ask" => Ok(SpeechAct::Question),
        "order" => Ok(SpeechAct::Command),
        "call" => Ok(SpeechAct::Callout),
        _ => Err(OpalError::BadHeader),
    }
}

/// What a line is for.
fn parse_intent(value: &str) -> Result<Intent, OpalError> {
    match value {
        "order" => Ok(Intent::Order),
        "ack" => Ok(Intent::Ack),
        "warn" => Ok(Intent::Warn),
        "query" => Ok(Intent::Query),
        "report" => Ok(Intent::Report),
        _ => Err(OpalError::BadHeader),
    }
}

/// How a line sounds.
fn parse_affect(value: &str) -> Result<Affect, OpalError> {
    match value {
        "calm" => Ok(Affect::Calm),
        "urgent" => Ok(Affect::Urgent),
        "uncertain" => Ok(Affect::Uncertain),
        _ => Err(OpalError::BadHeader),
    }
}

/// What a line is about.
fn parse_subject(value: &str) -> Result<Subject, OpalError> {
    if value == "none" {
        return Ok(Subject::None);
    }
    if let Some(digits) = value.strip_prefix("beacon") {
        let index = index_of(digits).ok_or(OpalError::NoSubject)?;
        let beacon = u8::try_from(index).map_err(|_| OpalError::NoSubject)?;
        return Ok(Subject::Beacon(beacon));
    }
    if let Some(digits) = value.strip_prefix("enemy") {
        let index = index_of(digits).ok_or(OpalError::NoSubject)?;
        let enemy = u16::try_from(index).map_err(|_| OpalError::NoSubject)?;
        return Ok(Subject::Enemy(enemy));
    }
    if let Some(digits) = value.strip_prefix("dir") {
        let index = index_of(digits).ok_or(OpalError::NoSubject)?;
        let direction = u8::try_from(index).map_err(|_| OpalError::NoSubject)?;
        return Ok(Subject::Direction(direction));
    }
    // The subject could not be named, and a subject that cannot be named is
    // not a subject of none: `subject=none` says so in words.
    Err(OpalError::NoSubject)
}

/// The channel a line is spoken on.
fn parse_channel(value: &str) -> Result<Channel, OpalError> {
    match value {
        "shout" => Ok(Channel::Shout),
        "voice" => Ok(Channel::Voice),
        "whisper" => Ok(Channel::Whisper),
        "radio" => Ok(Channel::Radio),
        _ => Err(OpalError::BadHeader),
    }
}

/// The pitch of a word: a step from the speaker's baseline.
///
/// A pitch a voice cannot sing is a refusal, not something to be flattened
/// into range behind the speaker's back: a broken notation is a broken line.
fn parse_pitch(token: &str) -> Result<i8, OpalError> {
    let digits = token.strip_prefix('p').ok_or(OpalError::BadPitch)?;
    let (sign, digits) = match digits.strip_prefix('-') {
        Some(rest) => (-1i32, rest),
        None => (1i32, digits.strip_prefix('+').unwrap_or(digits)),
    };
    let index = index_of(digits).ok_or(OpalError::BadPitch)?;
    let steps = sign * i32::try_from(index).map_err(|_| OpalError::BadPitch)?;
    if steps < i32::from(MIN_PITCH_STEPS) || steps > i32::from(MAX_PITCH_STEPS) {
        return Err(OpalError::BadPitch);
    }
    i8::try_from(steps).map_err(|_| OpalError::BadPitch)
}

/// The accent of a word.
fn parse_accent(token: &str) -> Result<u32, OpalError> {
    match token.strip_prefix('a') {
        Some(".") => Ok(EMPHASIS_NONE),
        Some("!") => Ok(EMPHASIS_STRESSED),
        Some("!!") => Ok(EMPHASIS_EMPHATIC),
        _ => Err(OpalError::BadAccent),
    }
}

/// The length of a word, in beats.
fn parse_beats(token: &str) -> Result<u16, OpalError> {
    let digits = token.strip_prefix('l').ok_or(OpalError::BadLength)?;
    let index = index_of(digits).ok_or(OpalError::BadLength)?;
    u16::try_from(index).map_err(|_| OpalError::BadLength)
}

/// The rest after a word, in beats.
fn parse_rest(token: &str) -> Result<u16, OpalError> {
    let digits = token.strip_prefix("~rest").ok_or(OpalError::BadLength)?;
    let index = index_of(digits).ok_or(OpalError::BadLength)?;
    let beats = u16::try_from(index).map_err(|_| OpalError::BadLength)?;
    if beats > MAX_REST_BEATS {
        return Err(OpalError::BadLength);
    }
    Ok(beats)
}

/// A plain decimal number, or nothing.
fn index_of(digits: &str) -> Option<u32> {
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse::<u32>().ok()
}

/// The squad a colour names.
fn squad_of_colour(colour: &str) -> Option<u8> {
    SQUAD_COLOURS
        .iter()
        .position(|name| *name == colour)
        .map(|index| u8::try_from(index).unwrap_or(u8::MAX))
}

/// The squad the speaker belongs to.
fn speaker_squad(ctx: &TalkContext<'_>) -> Option<u8> {
    ctx.roster
        .iter()
        .find(|entry| entry.agent == ctx.speaker)
        .map(|entry| entry.squad)
}

/// The act, as the notation spells it.
fn act_word(act: SpeechAct) -> &'static str {
    match act {
        SpeechAct::Statement => "say",
        SpeechAct::Question => "ask",
        SpeechAct::Command => "order",
        SpeechAct::Callout => "call",
    }
}

/// The intent, as the notation spells it.
fn intent_word(intent: Intent) -> &'static str {
    match intent {
        Intent::Order => "order",
        Intent::Ack => "ack",
        Intent::Warn => "warn",
        Intent::Query => "query",
        Intent::Report => "report",
    }
}

/// The affect, as the notation spells it.
fn affect_word(affect: Affect) -> &'static str {
    match affect {
        Affect::Calm => "calm",
        Affect::Urgent => "urgent",
        Affect::Uncertain => "uncertain",
    }
}

/// The channel, as the notation spells it.
fn channel_word(channel: Channel) -> &'static str {
    match channel {
        Channel::Shout => "shout",
        Channel::Voice => "voice",
        Channel::Whisper => "whisper",
        Channel::Radio => "radio",
    }
}

/// The addressee, as the notation spells it.
fn addressee_word(addressee: Addressee) -> String {
    match addressee {
        Addressee::Squad => "squad".to_owned(),
        Addressee::All => "all".to_owned(),
        Addressee::Agent(agent) => format!("agent{agent}"),
    }
}

/// The subject, as the notation spells it.
fn subject_word(subject: Subject) -> String {
    match subject {
        Subject::Beacon(beacon) => format!("beacon{beacon}"),
        Subject::Enemy(enemy) => format!("enemy{enemy}"),
        Subject::Direction(direction) => format!("dir{direction}"),
        Subject::None => "none".to_owned(),
    }
}

/// The pitch, as the notation spells it: `p+2`, `p0`, `p-3`.
fn pitch_word(pitch_steps: i8) -> String {
    if pitch_steps > 0 {
        format!("p+{pitch_steps}")
    } else {
        format!("p{pitch_steps}")
    }
}

/// The accent, as the notation spells it.
fn accent_word(emphasis_q16: u32) -> &'static str {
    if emphasis_q16 == EMPHASIS_NONE {
        "."
    } else if emphasis_q16 >= EMPHASIS_EMPHATIC {
        "!!"
    } else {
        "!"
    }
}

/// The length of a word, in beats.
fn length_word(word: &Word) -> String {
    let ticks = word.t_off.saturating_sub(word.t_on);
    format!("l{}", ticks / u64::from(BEAT_TICKS))
}

/// The rest after a word, in beats.
fn rest_word(pause_after_ticks: u16) -> String {
    format!("{}", u64::from(pause_after_ticks) / u64::from(BEAT_TICKS))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header::Role;
    use crate::talk::TalkParams;

    /// The fleet/07 sketch, in the spacing the sketch was written in: the
    /// keys are in any order and the columns are aligned for a human.
    const SKETCH: &str = "@squad say intent=order affect=urgent to=red\n  hold      p+2 a! l2\n  the       p0  a.  l1\n  north     p+3 a!! l3\n  beacon    p+1 a.  l2   ~rest1";

    /// The canonical form of the sketch: what the renderer writes, byte for
    /// byte. The `to=red` is resolved against the roster at compile time and
    /// comes back as the neutral `to=squad`.
    const CANONICAL: &str = "@squad say to=squad intent=order affect=urgent\n  hold p+2 a! l2\n  the p0 a. l1\n  north p+3 a!! l3\n  beacon p+1 a. l2 ~rest1";

    /// A table of ten scores: every act, every affect, every channel, every
    /// subject kind, and every addressee.
    const TABLE: [&str; 10] = [
        "@squad say to=squad intent=report affect=calm subject=beacon2\n  hold p+2 a! l2\n  position p0 a. l1",
        "@squad ask to=squad intent=query affect=uncertain channel=whisper\n  where p-1 a. l1\n  beacon p+2 a! l2 ~rest1",
        "@squad order to=squad intent=order affect=urgent channel=radio subject=enemy7\n  take p+3 a!! l3\n  beacon p0 a. l2",
        "@all call to=all intent=warn affect=urgent channel=shout subject=dir1\n  contact p+5 a!! l1\n  north p+1 a! l2",
        "@squad say to=squad intent=ack affect=calm channel=radio\n  copy p0 a. l1",
        "@squad ask to=agent3 intent=query affect=urgent\n  ready p+2 a! l2\n  now p0 a. l1",
        "@squad order to=squad intent=order affect=uncertain channel=shout\n  wait p-2 a. l1\n  now p0 a! l2 ~rest1",
        "@squad call to=squad intent=warn affect=calm channel=whisper\n  psst p+1 a. l1",
        "@squad say to=squad intent=report affect=uncertain channel=shout\n  report p0 a. l1\n  now p+1 a! l2",
        "@squad order to=squad intent=order affect=calm\n  move p+1 a! l2\n  north p-1 a. l3",
    ];

    /// A round of two squads of two: agents 0 and 1 are red, agents 2 and 3
    /// are blue.
    fn roster() -> Vec<RosterEntry> {
        (0..4u16)
            .map(|agent| RosterEntry {
                agent,
                squad: u8::try_from(agent / 2).unwrap_or(u8::MAX),
                role: if agent % 2 == 0 {
                    Role::Leader
                } else {
                    Role::Member
                },
                voice: format!("voice{agent}"),
                harness_commit: "0".repeat(40),
                reflex: "default".to_owned(),
            })
            .collect()
    }

    /// The context of a speaker in a roster.
    fn context<'a>(speaker: u16, roster: &'a [RosterEntry]) -> TalkContext<'a> {
        TalkContext { speaker, roster }
    }

    #[test]
    fn opal_parses_the_fleet_sketch() {
        let roster = roster();
        let u = parse(&context(0, &roster), SKETCH).expect("the fleet sketch compiles");

        assert_eq!(u.speaker, 0);
        assert_eq!(u.act, SpeechAct::Statement);
        assert_eq!(u.intent, Intent::Order);
        assert_eq!(u.affect, Affect::Urgent);
        assert_eq!(u.addressee, Addressee::Squad);
        assert_eq!(u.channel, Channel::Voice);
        assert_eq!(u.subject, Subject::None);
        assert_eq!(u.prosody, Prosody::Synthesized);
        assert_eq!(u.words.len(), 4);

        let hold = &u.words[0];
        assert_eq!(hold.text, "hold");
        assert_eq!(hold.pitch_steps, 2);
        assert_eq!(hold.emphasis_q16, 32_768);
        assert_eq!(hold.t_on, 0);
        assert_eq!(hold.t_off, 10);
        assert_eq!(hold.pause_after_ticks, 0);
        // Urgent is 49_152, the accent adds half of 32_768, the sum is
        // capped at one whole.
        assert_eq!(hold.energy_q16, 65_536);
        // (65_536 / 2 beats) x 78_432 / 65_536.
        assert_eq!(hold.rate_q16, 39_216);

        let the = &u.words[1];
        assert_eq!(the.text, "the");
        assert_eq!(the.pitch_steps, 0);
        assert_eq!(the.emphasis_q16, 0);
        assert_eq!(the.t_on, 10);
        assert_eq!(the.t_off, 15);
        assert_eq!(the.energy_q16, 49_152);
        assert_eq!(the.rate_q16, 78_432);

        let north = &u.words[2];
        assert_eq!(north.text, "north");
        assert_eq!(north.pitch_steps, 3);
        assert_eq!(north.emphasis_q16, 65_536);
        assert_eq!(north.t_on, 15);
        assert_eq!(north.t_off, 30);
        assert_eq!(north.energy_q16, 65_536);
        // (65_536 / 3 beats, the integer division) x 78_432 / 65_536.
        assert_eq!(north.rate_q16, 26_143);

        let beacon = &u.words[3];
        assert_eq!(beacon.text, "beacon");
        assert_eq!(beacon.pitch_steps, 1);
        assert_eq!(beacon.emphasis_q16, 0);
        assert_eq!(beacon.t_on, 30);
        assert_eq!(beacon.t_off, 40);
        assert_eq!(beacon.pause_after_ticks, 5);
        assert_eq!(beacon.energy_q16, 49_152);
        assert_eq!(beacon.rate_q16, 39_216);

        // The last word ends on tick 40 and the trailing rest runs to tick
        // 45: the rest is silence, so the line itself lasts 40 ticks.
        let last = u.words.last().expect("a word");
        assert_eq!(last.t_off + u64::from(last.pause_after_ticks), 45);
        assert_eq!(u.duration_ticks(), 40);
        assert_eq!(u.prose(), "hold the north beacon");
    }

    #[test]
    fn opal_renders_canonically() {
        let roster = roster();
        let u = parse(&context(0, &roster), SKETCH).expect("the fleet sketch compiles");
        assert_eq!(render(&u), CANONICAL);
        // The canonical text is the same line: it compiles back to the same
        // score and renders back to itself.
        let again = parse(&context(0, &roster), CANONICAL).expect("the canonical text compiles");
        assert_eq!(again, u);
        assert_eq!(render(&again), CANONICAL);
    }

    #[test]
    fn opal_round_trips_to_the_same_score() {
        let roster = roster();
        let ctx = context(0, &roster);
        assert_eq!(TABLE.len(), 10);

        // The table covers every act, every affect and every channel.
        let mut acts: Vec<SpeechAct> = Vec::new();
        let mut affects: Vec<Affect> = Vec::new();
        let mut channels: Vec<Channel> = Vec::new();
        for text in TABLE {
            let u = parse(&ctx, text).expect("the table compiles");
            if !acts.contains(&u.act) {
                acts.push(u.act);
            }
            if !affects.contains(&u.affect) {
                affects.push(u.affect);
            }
            if !channels.contains(&u.channel) {
                channels.push(u.channel);
            }

            // The score survives the round trip through the text, byte for
            // byte, and so does the text.
            let rendered = render(&u);
            let again = parse(&ctx, &rendered).expect("the canonical text compiles");
            assert_eq!(again, u, "the round trip changed the score of:\n{text}");
            assert_eq!(render(&again), rendered);
        }
        assert_eq!(acts.len(), 4, "every act is in the table");
        assert_eq!(affects.len(), 3, "every affect is in the table");
        assert_eq!(channels.len(), 4, "every channel is in the table");
    }

    #[test]
    fn opal_is_deterministic() {
        let roster = roster();
        let ctx = context(0, &roster);
        let first = render(&parse(&ctx, SKETCH).expect("the fleet sketch compiles"));
        for _ in 0..1_000 {
            let u = parse(&ctx, SKETCH).expect("the fleet sketch compiles");
            assert_eq!(render(&u), first);
        }
    }

    #[test]
    fn opal_rejects_a_bad_header() {
        let roster = roster();
        let ctx = context(0, &roster);
        let head = "@squad say to=squad intent=order affect=calm";

        // An act the notation does not have.
        assert_eq!(
            parse(&ctx, "@squad maybe intent=order affect=calm"),
            Err(OpalError::BadHeader)
        );
        // A line without its header.
        assert_eq!(parse(&ctx, "hold p0 a. l1"), Err(OpalError::BadHeader));
        // A key the notation does not have.
        assert_eq!(
            parse(&ctx, "@squad say intent=order affect=calm mood=happy"),
            Err(OpalError::UnknownKey)
        );
        // An accent that is not an accent.
        assert_eq!(
            parse(&ctx, &format!("{head}\n  hold p+2 a? l2")),
            Err(OpalError::BadAccent)
        );
        // A pitch a voice cannot sing.
        assert_eq!(
            parse(&ctx, &format!("{head}\n  hold p99 a. l2")),
            Err(OpalError::BadPitch)
        );
        // A length that is not a number of beats.
        assert_eq!(
            parse(&ctx, &format!("{head}\n  hold p+2 a. lx")),
            Err(OpalError::BadLength)
        );
        // A rest that is not a number of beats.
        assert_eq!(
            parse(&ctx, &format!("{head}\n  hold p+2 a. l2 ~restx")),
            Err(OpalError::BadLength)
        );
        // A value outside the vocabulary.
        assert_eq!(
            parse(&ctx, "@squad say intent=hurry affect=calm"),
            Err(OpalError::BadHeader)
        );
        // A header that does not say what the line is for.
        assert_eq!(
            parse(&ctx, "@squad say to=squad affect=calm"),
            Err(OpalError::BadHeader)
        );
        // The same key twice: the line says two different things.
        assert_eq!(
            parse(
                &ctx,
                "@squad say to=squad intent=order intent=ack affect=calm"
            ),
            Err(OpalError::BadHeader)
        );
        // A subject that cannot be named.
        assert_eq!(
            parse(
                &ctx,
                "@squad say to=squad intent=order affect=calm subject=beacon"
            ),
            Err(OpalError::NoSubject)
        );
    }

    #[test]
    fn opal_refuses_a_foreign_squad() {
        let roster = roster();
        let head = "@squad say intent=order affect=calm";

        // Agent 0 is red: it may speak for the red squad...
        let red = context(0, &roster);
        let own = parse(&red, &format!("{head} to=red\n  hold p0 a. l1")).expect("compiles");
        assert_eq!(own.addressee, Addressee::Squad);
        // ...but not for the blue one.
        assert_eq!(
            parse(&red, &format!("{head} to=blue\n  hold p0 a. l1")),
            Err(OpalError::ForeignSquad)
        );

        // The blue squad may speak for the blue squad.
        let blue = context(2, &roster);
        let own = parse(&blue, &format!("{head} to=blue\n  hold p0 a. l1")).expect("compiles");
        assert_eq!(own.addressee, Addressee::Squad);
        assert_eq!(
            parse(&blue, &format!("{head} to=red\n  hold p0 a. l1")),
            Err(OpalError::ForeignSquad)
        );
    }

    #[test]
    fn opal_refuses_an_empty_line() {
        let roster = roster();
        let ctx = context(0, &roster);
        assert_eq!(parse(&ctx, ""), Err(OpalError::Empty));
        assert_eq!(parse(&ctx, "   \n\n\t"), Err(OpalError::Empty));
        // A header with no words is an empty utterance, not a quiet one: it
        // is a refusal, never a zero-length line.
        assert_eq!(
            parse(&ctx, "@squad say to=squad intent=order affect=calm"),
            Err(OpalError::Empty)
        );
    }

    #[test]
    fn opal_a_rest_delays_the_next_word() {
        let roster = roster();
        let u = parse(
            &context(0, &roster),
            "@squad say to=squad intent=order affect=calm\n  hold p0 a. l1 ~rest2\n  now p0 a. l1",
        )
        .expect("compiles");
        assert_eq!(u.words[0].t_on, 0);
        assert_eq!(u.words[0].t_off, 5);
        assert_eq!(u.words[0].pause_after_ticks, 10);
        // The rest is silence the next word waits out: "now" starts at 15.
        assert_eq!(u.words[1].t_on, 15);
        assert_eq!(u.words[1].t_off, 20);
    }

    #[test]
    fn opal_names_the_addressee_and_the_scope() {
        let roster = roster();
        let ctx = context(0, &roster);
        let all = parse(
            &ctx,
            "@all say to=all intent=report affect=calm\n  hold p0 a. l1",
        )
        .expect("compiles");
        assert_eq!(all.addressee, Addressee::All);
        assert!(render(&all).starts_with("@all say to=all "));

        let one = parse(
            &ctx,
            "@agent3 say to=agent3 intent=report affect=calm\n  hold p0 a. l1",
        )
        .expect("compiles");
        assert_eq!(one.addressee, Addressee::Agent(3));
        assert!(render(&one).starts_with("@agent3 say to=agent3 "));
    }

    #[test]
    fn the_notation_beat_matches_the_round_params() {
        // The notation's beat is the round's default beat: the two must not
        // drift apart, or a line would be written in one unit and timed in
        // another.
        assert_eq!(BEAT_TICKS, TalkParams::default().beat_ticks);
        assert_eq!(MIN_WORD_TICKS, TalkParams::default().min_word_ticks);
    }
}
