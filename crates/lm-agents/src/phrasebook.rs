//! The phrasebook: the **wording** of the lines the agents speak.
//!
//! The decision model chooses *what* is said — the gesture, the addressee,
//! the intent, the affect and the subject (D37) — and the phrasebook says
//! *how* it is said. The two are kept apart on purpose: a model may never
//! choose a target, and a phrase may never choose a target either (D37).
//!
//! The phrasebook is **doctrine**. It is a harness file, edited between
//! rounds by the reviewer, and the harness commit pins it into the round
//! log (D27). A phase-5 trainer edits it the way it edits gates (D47).
//!
//! ```toml
//! [phrasebook."Command.Order"]
//! channel = "radio"
//! templates = [
//!   "take {subject} now",
//!   "move to {subject}",
//! ]
//! ```
//!
//! An entry is keyed by `"{act}.{intent}"` — the tags the gates chose — and
//! may be refined by the kind of subject it is for
//! (`"Command.Order.Beacon"`), which wins over the general entry. The
//! templates are plain words with two placeholders: `{subject}` is what the
//! line is about, `{direction}` is the compass word of the direction it is
//! about. The realizer renders the chosen template into the notation
//! (`lm_core::opal`), which the simulation compiles into the score.
//!
//! A key that is not one the code knows is a refusal, never a guess: the
//! phrasebook that does not load is a phrasebook that is not used, and the
//! agent falls back to the other realizer or to silence (D26).

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fmt;

use lm_core::hash::Fnv64;
use lm_core::opal;
use lm_core::utterance::{Affect, Channel, Intent, SpeechAct, Subject};

use crate::utter::UtterPlan;

/// The phrasebook the harness ships, as text.
pub const DEFAULT_PHRASEBOOK_TOML: &str =
    include_str!("../../../assets/harness/default/phrasebook.toml");

/// The placeholder of the thing the line is about.
const SUBJECT_PLACEHOLDER: &str = "{subject}";
/// The placeholder of the compass word of the direction.
const DIRECTION_PLACEHOLDER: &str = "{direction}";

/// The compass words, in the order their indices run; row 0 is north, so
/// +y is south. "here" is a zero step.
///
/// These are the same words the facts builder states (see
/// [`crate::facts::COMPASS_WORDS`]): a line and the facts it is made of
/// must name a direction the same way.
pub const COMPASS_WORDS: [&str; 5] = ["north", "east", "south", "west", "here"];

/// Why a phrasebook did not load.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PhrasebookError {
    /// The file is not TOML.
    Toml(String),
    /// An entry's key is not one the code knows.
    BadKey(String),
    /// An entry lists no template.
    NoTemplates(String),
    /// An entry names a channel the notation does not have.
    BadChannel(String),
    /// A template carries a number of its own.
    ///
    /// The numbers of a line are the gates': the beacon and the direction
    /// the gates chose. A template that carries a number could name
    /// another target -- so it is refused, and a phrasebook that cannot
    /// be trusted is not used (D37).
    Numbered(String),
}

impl fmt::Display for PhrasebookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PhrasebookError::Toml(why) => write!(f, "the phrasebook is not valid TOML: {why}"),
            PhrasebookError::BadKey(key) => {
                write!(f, "the phrasebook key {key} is not one this build knows")
            }
            PhrasebookError::NoTemplates(key) => {
                write!(f, "the phrasebook entry {key} lists no template")
            }
            PhrasebookError::BadChannel(key) => {
                write!(
                    f,
                    "the phrasebook entry {key} names a channel that is not one"
                )
            }
            PhrasebookError::Numbered(key) => write!(
                f,
                "the phrasebook entry {key} has a template with a number in it: \
                 the numbers of a line are the gates'"
            ),
        }
    }
}

impl std::error::Error for PhrasebookError {}

/// One entry of the phrasebook: how a line of this kind is worded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The channel the line is spoken on.
    pub channel: Channel,
    /// The wordings, in the order the file lists them.
    pub templates: Vec<String>,
}

impl Entry {
    /// The wording for this moment, chosen by `seed`.
    ///
    /// The choice is a **hash**, never a random number: the same situation
    /// always gets the same wording, so a round replays with the same
    /// lines (and the bark cache still hits).
    pub fn pick(&self, seed: u64) -> &str {
        let mut h = Fnv64::new();
        h.u64(seed);
        let index = (h.finish() % self.templates.len() as u64) as usize;
        &self.templates[index]
    }

    /// The wording of a line, in the notation.
    ///
    /// The header carries the tags the gates chose; the words carry the
    /// wording the phrasebook gives them. The subject and the direction
    /// are filled in, so the line says what it is about.
    pub fn render(&self, plan: &UtterPlan, seed: u64) -> String {
        let template = self.pick(seed);
        let words = fill_placeholders(template, plan.subject);
        line_of(plan, self.channel, &words)
    }
}

/// A line in the notation: the header the gates chose, and the words.
///
/// The **wording** is the realizer's business -- the phrasebook's words or
/// the model's -- and the notation is the notation's: the header carries
/// the tags the gates chose (D37), and every word is a word of one beat,
/// at the speaker's baseline pitch, without stress. The affect carries the
/// loudness and the pace when the notation compiles it.
///
/// Both realizers write their words through here, so a line the model
/// words and a line the phrasebook words are the same shape and can be
/// read side by side.
pub fn line_of(plan: &UtterPlan, channel: Channel, words: &str) -> String {
    let mut out = header_line(plan, channel);
    for word in words.split_whitespace() {
        out.push('\n');
        out.push_str("  ");
        out.push_str(word);
        out.push_str(" p0 a. l1");
    }
    out
}

/// The phrasebook: the wording of every kind of line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Phrasebook {
    entries: BTreeMap<String, Entry>,
}

impl Phrasebook {
    /// The phrasebook the harness ships.
    pub fn shipped() -> Phrasebook {
        Phrasebook::parse(DEFAULT_PHRASEBOOK_TOML).unwrap_or_default()
    }

    /// Parse a phrasebook from its text.
    ///
    /// The file is a table of entries under the `phrasebook` key, each
    /// keyed by `"{act}.{intent}"` (and optionally by the kind of subject
    /// it is for). A file that does not parse is a refusal with a name:
    /// the caller keeps the phrasebook it had and says so.
    pub fn parse(text: &str) -> Result<Phrasebook, PhrasebookError> {
        let value: toml::Value =
            toml::from_str(text).map_err(|e| PhrasebookError::Toml(e.to_string()))?;
        let Some(table) = value.as_table().and_then(|t| t.get("phrasebook")) else {
            // A file with no entries is an empty phrasebook, not an error:
            // every line falls back to the other realizer or to silence.
            return Ok(Phrasebook::default());
        };
        let Some(table) = table.as_table() else {
            return Err(PhrasebookError::Toml(
                "the phrasebook is not a table of entries".to_string(),
            ));
        };

        let mut entries = BTreeMap::new();
        for (key, entry) in table {
            // A key the code does not know is a refusal: the phrasebook is
            // doctrine, and doctrine that cannot be read is not used.
            parse_key(key)?;
            let channel = match entry.get("channel") {
                Some(toml::Value::String(name)) => {
                    parse_channel(name).ok_or_else(|| PhrasebookError::BadChannel(key.clone()))?
                }
                Some(_) => return Err(PhrasebookError::BadChannel(key.clone())),
                None => Channel::Voice,
            };
            let templates = entry
                .get("templates")
                .and_then(toml::Value::as_array)
                .ok_or_else(|| PhrasebookError::NoTemplates(key.clone()))?
                .iter()
                .map(|t| {
                    t.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| PhrasebookError::NoTemplates(key.clone()))
                })
                .collect::<Result<Vec<String>, PhrasebookError>>()?;
            if templates.is_empty() {
                return Err(PhrasebookError::NoTemplates(key.clone()));
            }
            // A template may not carry a number: the only numbers in a
            // line are the ones the gates chose, and they come in through
            // the placeholders. A phrase cannot smuggle a target past the
            // gates (D37).
            for template in &templates {
                if template.chars().any(|c| c.is_ascii_digit()) {
                    return Err(PhrasebookError::Numbered(key.clone()));
                }
            }
            entries.insert(key.clone(), Entry { channel, templates });
        }
        Ok(Phrasebook { entries })
    }

    /// The entry for a line of this kind, if the phrasebook has one.
    ///
    /// The entry for the kind of subject wins over the general one, so a
    /// phrasebook may word a beacon differently from an enemy without
    /// giving up the general line.
    pub fn entry(&self, act: SpeechAct, intent: Intent, subject: Subject) -> Option<&Entry> {
        self.entries
            .get(&key_for(act, intent, Some(subject)))
            .or_else(|| self.entries.get(&key_for(act, intent, None)))
    }

    /// The phrasebook with a persona's **barks** seeded into it.
    ///
    /// A persona's barks are the short lines it says: they are added to
    /// every entry as templates of their own, so the agent's lines carry
    /// the persona's words and not only the harness's.
    ///
    /// The barks are the persona's own words, so they are checked the way
    /// the harness's templates are: a bark that carries a number is
    /// refused here, because the numbers of a line are the gates' (D37).
    ///
    /// The choice among the templates is a **hash**, so the same situation
    /// always gets the same wording: a persona's line replays with the
    /// round.
    pub fn with_barks(&self, barks: &[String]) -> Result<Phrasebook, PhrasebookError> {
        if barks.is_empty() {
            return Ok(self.clone());
        }
        for bark in barks {
            if bark.chars().any(|c| c.is_ascii_digit()) {
                return Err(PhrasebookError::Numbered(bark.clone()));
            }
        }
        let mut entries = self.entries.clone();
        for entry in entries.values_mut() {
            for bark in barks {
                if !entry.templates.iter().any(|template| template == bark) {
                    entry.templates.push(bark.clone());
                }
            }
        }
        Ok(Phrasebook { entries })
    }

    /// Whether the phrasebook has a line for this act and intent at all.
    ///
    /// This is what the automatic realizer asks: a line the phrasebook
    /// cannot word is a line the model may word.
    pub fn has(&self, act: SpeechAct, intent: Intent) -> bool {
        self.entries.contains_key(&key_for(act, intent, None))
            || self
                .entries
                .keys()
                .any(|key| key.starts_with(&format!("{}.{}.", act_name(act), intent_name(intent))))
    }

    /// How many entries the phrasebook holds.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the phrasebook holds nothing at all.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The channel a line of this kind is spoken on, if the phrasebook has
    /// a line for it at all.
    ///
    /// The channel is the harness's business, not the model's: a line the
    /// model words is spoken on the same channel the phrasebook would have
    /// spoken it on, so the two wordings of one plan differ only in their
    /// words.
    pub fn channel_of(&self, act: SpeechAct, intent: Intent, subject: Subject) -> Option<Channel> {
        self.entry(act, intent, subject).map(|entry| entry.channel)
    }
}

/// The key of an entry: the act, the intent, and (optionally) the kind of
/// subject the entry is for.
fn key_for(act: SpeechAct, intent: Intent, subject: Option<Subject>) -> String {
    match subject {
        Some(subject) => format!(
            "{}.{}.{}",
            act_name(act),
            intent_name(intent),
            subject_kind(subject)
        ),
        None => format!("{}.{}", act_name(act), intent_name(intent)),
    }
}

/// The name of an act, as the phrasebook keys it.
pub fn act_name(act: SpeechAct) -> &'static str {
    match act {
        SpeechAct::Command => "Command",
        SpeechAct::Question => "Question",
        SpeechAct::Statement => "Statement",
        SpeechAct::Callout => "Callout",
    }
}

/// The name of an intent, as the phrasebook keys it.
pub fn intent_name(intent: Intent) -> &'static str {
    match intent {
        Intent::Order => "Order",
        Intent::Ack => "Ack",
        Intent::Warn => "Warn",
        Intent::Query => "Query",
        Intent::Report => "Report",
    }
}

/// The kind of a subject, as the phrasebook keys it.
pub fn subject_kind(subject: Subject) -> &'static str {
    match subject {
        Subject::Beacon(_) => "Beacon",
        Subject::Enemy(_) => "Enemy",
        Subject::Direction(_) => "Direction",
        Subject::None => "None",
    }
}

/// The name of an act, as the gates answer it.
pub fn act_key(act: SpeechAct) -> &'static str {
    match act {
        SpeechAct::Command => "command",
        SpeechAct::Question => "question",
        SpeechAct::Statement => "statement",
        SpeechAct::Callout => "callout",
    }
}

/// The name of an intent, as the gates answer it.
pub fn intent_key(intent: Intent) -> &'static str {
    match intent {
        Intent::Order => "order",
        Intent::Ack => "ack",
        Intent::Warn => "warn",
        Intent::Query => "query",
        Intent::Report => "report",
    }
}

/// The act a gate's option names, if it names one.
pub fn act_from_key(key: &str) -> Option<SpeechAct> {
    match key {
        "command" => Some(SpeechAct::Command),
        "question" => Some(SpeechAct::Question),
        "statement" => Some(SpeechAct::Statement),
        "callout" => Some(SpeechAct::Callout),
        _ => None,
    }
}

/// The intent a gate's option names, if it names one.
pub fn intent_from_key(key: &str) -> Option<Intent> {
    match key {
        "order" => Some(Intent::Order),
        "ack" => Some(Intent::Ack),
        "warn" => Some(Intent::Warn),
        "query" => Some(Intent::Query),
        "report" => Some(Intent::Report),
        _ => None,
    }
}

/// Check that a phrasebook key is one this build knows.
fn parse_key(key: &str) -> Result<(), PhrasebookError> {
    let parts: Vec<&str> = key.split('.').collect();
    let (act, intent, kind) = match parts.as_slice() {
        [act, intent] => (*act, *intent, None),
        [act, intent, kind] => (*act, *intent, Some(*kind)),
        _ => return Err(PhrasebookError::BadKey(key.to_owned())),
    };
    if !matches!(act, "Command" | "Question" | "Statement" | "Callout") {
        return Err(PhrasebookError::BadKey(key.to_owned()));
    }
    if !matches!(intent, "Order" | "Ack" | "Warn" | "Query" | "Report") {
        return Err(PhrasebookError::BadKey(key.to_owned()));
    }
    if let Some(kind) = kind {
        if !matches!(kind, "Beacon" | "Enemy" | "Direction" | "None") {
            return Err(PhrasebookError::BadKey(key.to_owned()));
        }
    }
    Ok(())
}

/// The channel a name means, if it means one.
fn parse_channel(name: &str) -> Option<Channel> {
    match name {
        "shout" => Some(Channel::Shout),
        "voice" => Some(Channel::Voice),
        "whisper" => Some(Channel::Whisper),
        "radio" => Some(Channel::Radio),
        _ => None,
    }
}

/// Fill the placeholders of a template with the subject it is about.
fn fill_placeholders(template: &str, subject: Subject) -> String {
    template
        .replace(SUBJECT_PLACEHOLDER, &subject_words(subject))
        .replace(DIRECTION_PLACEHOLDER, &direction_words(subject))
}

/// The words that name a subject: "beacon 2", "enemy 4", "north", or
/// "nothing".
fn subject_words(subject: Subject) -> String {
    match subject {
        Subject::Beacon(beacon) => format!("beacon {beacon}"),
        Subject::Enemy(enemy) => format!("enemy {enemy}"),
        Subject::Direction(direction) => compass_word(direction).to_owned(),
        Subject::None => "nothing".to_owned(),
    }
}

/// The compass word of a direction, or "here" when the line is about no
/// direction at all.
fn direction_words(subject: Subject) -> String {
    match subject {
        Subject::Direction(direction) => compass_word(direction).to_owned(),
        _ => compass_word(4).to_owned(),
    }
}

/// The compass word of a direction index.
fn compass_word(direction: u8) -> &'static str {
    COMPASS_WORDS
        .get(usize::from(direction))
        .copied()
        .unwrap_or(COMPASS_WORDS[4])
}

/// The header of a line, in the notation.
///
/// The header is written by the notation itself, so the wording and the
/// tags always agree: the scope, the gesture, and then who the line is
/// for, what it is for, how it sounds and what it is about.
fn header_line(plan: &UtterPlan, channel: Channel) -> String {
    let skeleton = lm_core::utterance::Utterance {
        // The speaker is not in the notation: the score's speaker comes
        // from the round, when the line is compiled.
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

/// The affect of a plan, as the notation spells it.
///
/// This is here so the realizer does not have to reach into the notation's
/// own vocabulary: the tags and the words always come from one place.
pub fn affect_name(affect: Affect) -> &'static str {
    match affect {
        Affect::Calm => "calm",
        Affect::Urgent => "urgent",
        Affect::Uncertain => "uncertain",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lm_core::utterance::Addressee;

    /// A plan, for the rendering tests.
    fn plan() -> UtterPlan {
        UtterPlan {
            act: SpeechAct::Command,
            addressee: Addressee::Squad,
            intent: Intent::Order,
            affect: Affect::Urgent,
            subject: Subject::Beacon(2),
            should_speak: true,
        }
    }

    #[test]
    fn the_shipped_phrasebook_parses() {
        let book = Phrasebook::shipped();
        assert!(!book.is_empty(), "the shipped phrasebook has entries");
        assert!(book.has(SpeechAct::Command, Intent::Order));
        assert!(book.has(SpeechAct::Statement, Intent::Report));
        assert!(book.has(SpeechAct::Callout, Intent::Ack));
    }

    #[test]
    fn the_shipped_phrasebook_covers_every_act_and_intent() {
        let book = Phrasebook::shipped();
        for act in [
            SpeechAct::Command,
            SpeechAct::Question,
            SpeechAct::Statement,
            SpeechAct::Callout,
        ] {
            for intent in [
                Intent::Order,
                Intent::Ack,
                Intent::Warn,
                Intent::Query,
                Intent::Report,
            ] {
                assert!(
                    book.has(act, intent),
                    "the phrasebook has no line for {act:?}/{intent:?}"
                );
            }
        }
    }

    #[test]
    fn an_entry_is_chosen_by_act_intent_and_subject() {
        let text = r#"
            [phrasebook."Command.Order"]
            templates = ["general"]

            [phrasebook."Command.Order.Beacon"]
            templates = ["about a beacon"]
        "#;
        let book = Phrasebook::parse(text).expect("the phrasebook parses");
        let beacon = book
            .entry(SpeechAct::Command, Intent::Order, Subject::Beacon(1))
            .expect("the beacon entry wins");
        assert_eq!(beacon.templates, vec!["about a beacon".to_owned()]);
        let enemy = book
            .entry(SpeechAct::Command, Intent::Order, Subject::Enemy(3))
            .expect("the general entry is the fallback");
        assert_eq!(enemy.templates, vec!["general".to_owned()]);
    }

    #[test]
    fn a_missing_entry_is_not_an_error() {
        let book = Phrasebook::parse("[phrasebook]\n").expect("an empty phrasebook parses");
        assert!(book.is_empty());
        assert!(book
            .entry(SpeechAct::Command, Intent::Order, Subject::None)
            .is_none());
        assert!(!book.has(SpeechAct::Command, Intent::Order));
    }

    #[test]
    fn a_template_may_not_carry_a_number() {
        // The numbers of a line are the gates': a template that carries
        // one could name another target (D37).
        let text = "[phrasebook.\"Command.Order\"]\ntemplates = [\"take beacon 7\"]\n";
        assert!(matches!(
            Phrasebook::parse(text),
            Err(PhrasebookError::Numbered(_))
        ));
        // The placeholders are how a line gets its numbers.
        let text = "[phrasebook.\"Command.Order\"]\ntemplates = [\"take {subject}\"]\n";
        assert!(Phrasebook::parse(text).is_ok());
    }

    #[test]
    fn a_bad_key_is_refused() {
        let text = "[phrasebook.\"Command.Dance\"]\ntemplates = [\"x\"]\n";
        assert!(matches!(
            Phrasebook::parse(text),
            Err(PhrasebookError::BadKey(_))
        ));
        let text = "[phrasebook.\"Command.Order\"]\n";
        assert!(matches!(
            Phrasebook::parse(text),
            Err(PhrasebookError::NoTemplates(_))
        ));
        let text = "[phrasebook.\"Command.Order\"]\nchannel = \"telepathy\"\ntemplates = [\"x\"]\n";
        assert!(matches!(
            Phrasebook::parse(text),
            Err(PhrasebookError::BadChannel(_))
        ));
    }

    #[test]
    fn the_wording_is_chosen_by_a_hash() {
        let text = "[phrasebook.\"Command.Order\"]\ntemplates = [\"one\", \"two\", \"three\"]\n";
        let book = Phrasebook::parse(text).expect("the phrasebook parses");
        let entry = book
            .entry(SpeechAct::Command, Intent::Order, Subject::None)
            .expect("the entry");
        // The same seed always picks the same wording.
        assert_eq!(entry.pick(7), entry.pick(7));
        // And the wording is one of the file's.
        assert!(["one", "two", "three"].contains(&entry.pick(7)));
    }

    #[test]
    fn the_placeholders_are_filled() {
        let text = "[phrasebook.\"Command.Order\"]\ntemplates = [\"take {subject} {direction}\"]\n";
        let book = Phrasebook::parse(text).expect("the phrasebook parses");
        let entry = book
            .entry(SpeechAct::Command, Intent::Order, Subject::Beacon(2))
            .expect("the entry");
        let line = entry.render(&plan(), 1);
        assert!(
            line.contains("subject=beacon2"),
            "the subject is named: {line}"
        );
        assert!(line.contains("take"), "the wording is the file's: {line}");
        assert!(line.contains("here"), "the direction is filled: {line}");
    }

    #[test]
    fn the_direction_placeholder_names_the_compass() {
        let text = "[phrasebook.\"Command.Order\"]\ntemplates = [\"go {direction}\"]\n";
        let book = Phrasebook::parse(text).expect("the phrasebook parses");
        let entry = book
            .entry(SpeechAct::Command, Intent::Order, Subject::Direction(2))
            .expect("the entry");
        let line = entry.render(
            &UtterPlan {
                subject: Subject::Direction(2),
                ..plan()
            },
            1,
        );
        assert!(line.contains("south"), "the compass word is filled: {line}");
    }
}
