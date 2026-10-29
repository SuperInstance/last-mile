//! The utterance score: what is said, how it is said, and to whom.
//!
//! An utterance is a **score**, not a string (D12). The decision model
//! chooses the act, the addressee, the intent, the affect and the subject;
//! only the *wording* comes from the phrasebook or the realizer (D37), and a
//! realizer that returns nonsense is a refusal, never a guess.
//!
//! The score carries every word with its lanes -- pitch, energy, rate,
//! emphasis and the pause after it -- so `lm-core` can expand the score into
//! a timeline and degrade it per listener with integer math alone: no model,
//! no clock, no float, no networking.
//!
//! The score is the truth; the *heard* copy is the truth as degraded. The
//! log carries the score and replays exactly; what a listener gets is
//! recomputed from the world, so it is reproducible too. Nothing is ever
//! inferred from audio.
//!
//! Every type here hashes with an explicit `tag()` in declaration order, so
//! the state hash of a score is canonical and cannot drift with a later
//! reordering of the declarations.

use crate::decision::Tick;
use crate::hash::{Fnv64, StateHash};
use serde::{Deserialize, Serialize};

/// The lowest pitch a word may carry: 24 semitones below the speaker's
/// baseline.
pub const MIN_PITCH_STEPS: i8 = -24;
/// The highest pitch a word may carry: 24 semitones above the speaker's
/// baseline.
pub const MAX_PITCH_STEPS: i8 = 24;
/// Q16: one whole.
pub const Q16_ONE: u32 = 65_536;

/// The channel a line is spoken on.
///
/// The channel decides how far the line reaches and how it is heard: a shout
/// carries across a field, a voice across a room, a whisper only at arm's
/// length, and the radio reaches the speaker's squad and nobody else.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Channel {
    /// Carries across a field.
    Shout,
    /// Carries across a room.
    Voice,
    /// Carries across an arm's length.
    Whisper,
    /// Carries to the speaker's squad, through walls, at the cost of the
    /// pitch and of some of the words.
    Radio,
}

impl Channel {
    /// The canonical discriminant used in hashes (declaration order).
    fn tag(self) -> u8 {
        match self {
            Channel::Shout => 0,
            Channel::Voice => 1,
            Channel::Whisper => 2,
            Channel::Radio => 3,
        }
    }
}

/// The gesture the line makes (the communicator's GESTURE bank, four
/// classes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SpeechAct {
    /// The line tells the addressee to do something.
    Command,
    /// The line asks the addressee for something.
    Question,
    /// The line says something.
    Statement,
    /// The line calls out to nobody in particular.
    Callout,
}

impl SpeechAct {
    /// The canonical discriminant used in hashes (declaration order).
    fn tag(self) -> u8 {
        match self {
            SpeechAct::Command => 0,
            SpeechAct::Question => 1,
            SpeechAct::Statement => 2,
            SpeechAct::Callout => 3,
        }
    }
}

/// What the line is *for* (spec §3's intent tags).
///
/// `Order` is the one the talk metrics count: an order that is heard is an
/// order that can be taken up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Intent {
    /// A command to act.
    Order,
    /// An acknowledgement.
    Ack,
    /// A warning.
    Warn,
    /// A question.
    Query,
    /// A report of something that happened.
    Report,
}

impl Intent {
    /// The canonical discriminant used in hashes (declaration order).
    fn tag(self) -> u8 {
        match self {
            Intent::Order => 0,
            Intent::Ack => 1,
            Intent::Warn => 2,
            Intent::Query => 3,
            Intent::Report => 4,
        }
    }
}

/// The affect bucket of the line -- the bark cache's third key.
///
/// The affect is a bucket, not a measurement: it is what the voice sounds
/// like, not how loud it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Affect {
    /// Level.
    Calm,
    /// Pressed.
    Urgent,
    /// Unsteady.
    Uncertain,
}

impl Affect {
    /// The canonical discriminant used in hashes (declaration order).
    fn tag(self) -> u8 {
        match self {
            Affect::Calm => 0,
            Affect::Urgent => 1,
            Affect::Uncertain => 2,
        }
    }
}

/// Who the line is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Addressee {
    /// The speaker's own squad.
    Squad,
    /// Every squad that can hear the line.
    All,
    /// One agent, by id.
    Agent(u16),
}

impl Addressee {
    /// The canonical discriminant used in hashes (declaration order).
    fn tag(self) -> u8 {
        match self {
            Addressee::Squad => 0,
            Addressee::All => 1,
            Addressee::Agent(_) => 2,
        }
    }
}

/// What the line is about -- the game-relevant content of the score.
///
/// The subject is what the line is *for*; the words are only how it says
/// it. A line without a subject still says something, so `None` is a real
/// subject and not a missing one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Subject {
    /// A beacon, by its index.
    Beacon(u8),
    /// An enemy, by its id.
    Enemy(u16),
    /// A direction, by its compass index.
    Direction(u8),
    /// Nothing in particular.
    None,
}

impl Subject {
    /// The canonical discriminant used in hashes (declaration order).
    fn tag(self) -> u8 {
        match self {
            Subject::Beacon(_) => 0,
            Subject::Enemy(_) => 1,
            Subject::Direction(_) => 2,
            Subject::None => 3,
        }
    }
}

/// Where the lanes of the line came from.
///
/// `Absent` is *not* zero (the communicator's rule 4): a line with no
/// measured prosody still has its words and its timing, it simply has no
/// expressive lanes to report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Prosody {
    /// The score's own lanes: the phrasebook's or the realizer's.
    Synthesized,
    /// Lanes measured from a voice (the human's, or a captured sample).
    Measured,
    /// No expressive lanes at all.
    Absent,
}

impl Prosody {
    /// The canonical discriminant used in hashes (declaration order).
    fn tag(self) -> u8 {
        match self {
            Prosody::Synthesized => 0,
            Prosody::Measured => 1,
            Prosody::Absent => 2,
        }
    }
}

/// One word of the line: its text, its place on the timeline and its lanes.
///
/// The lanes are fixed point. `pitch_steps` is semitones from the speaker's
/// baseline and is clamped to [`MIN_PITCH_STEPS`]..=[`MAX_PITCH_STEPS`];
/// `energy_q16`, `rate_q16` and `emphasis_q16` are Q16 fractions
/// ([`Q16_ONE`] is 1.0, and the rate may reach 2.0).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Word {
    /// The word as it is written.
    pub text: String,
    /// The tick the word starts on.
    pub t_on: Tick,
    /// The tick the word ends on.
    pub t_off: Tick,
    /// The pitch, in semitones from the speaker's baseline.
    pub pitch_steps: i8,
    /// The loudness, in Q16.
    pub energy_q16: u32,
    /// The pace, in Q16; 65 536 is the baseline pace and 131 072 is twice it.
    pub rate_q16: u32,
    /// The stress on the word, in Q16.
    pub emphasis_q16: u32,
    /// The rest after the word, in ticks.
    pub pause_after_ticks: u16,
}

impl Word {
    /// A word on the timeline, with its lanes at their neutral values.
    pub fn new(text: impl Into<String>, t_on: Tick, t_off: Tick) -> Word {
        Word {
            text: text.into(),
            t_on,
            t_off,
            pitch_steps: 0,
            energy_q16: Q16_ONE,
            rate_q16: Q16_ONE,
            emphasis_q16: 0,
            pause_after_ticks: 0,
        }
    }

    /// The word with a pitch, clamped to the range a voice can reach.
    pub fn with_pitch(mut self, pitch_steps: i8) -> Word {
        self.pitch_steps = pitch_steps.clamp(MIN_PITCH_STEPS, MAX_PITCH_STEPS);
        self
    }

    /// The word with a loudness, in Q16.
    pub fn with_energy(mut self, energy_q16: u32) -> Word {
        self.energy_q16 = energy_q16;
        self
    }

    /// The word with a pace, in Q16.
    pub fn with_rate(mut self, rate_q16: u32) -> Word {
        self.rate_q16 = rate_q16;
        self
    }

    /// The word with a stress, in Q16.
    pub fn with_emphasis(mut self, emphasis_q16: u32) -> Word {
        self.emphasis_q16 = emphasis_q16;
        self
    }

    /// The word with a rest after it, in ticks.
    pub fn with_pause(mut self, pause_after_ticks: u16) -> Word {
        self.pause_after_ticks = pause_after_ticks;
        self
    }

    /// Clamp the word's pitch into the range a voice can reach.
    pub fn clamp_pitch(&mut self) {
        self.pitch_steps = self.pitch_steps.clamp(MIN_PITCH_STEPS, MAX_PITCH_STEPS);
    }
}

/// A line, as the score: who says it, how, to whom, about what, and with
/// which words.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Utterance {
    /// The agent who speaks the line.
    pub speaker: u16,
    /// The channel the line is spoken on.
    pub channel: Channel,
    /// Who the line is for.
    pub addressee: Addressee,
    /// The gesture the line makes.
    pub act: SpeechAct,
    /// What the line is for.
    pub intent: Intent,
    /// The affect bucket of the line.
    pub affect: Affect,
    /// What the line is about.
    pub subject: Subject,
    /// Where the lanes came from.
    pub prosody: Prosody,
    /// The words, in order.
    pub words: Vec<Word>,
}

impl Utterance {
    /// How long the line takes, in ticks.
    ///
    /// From the first word's onset to the last word's offset. The rest after
    /// the last word is silence, and silence is not part of the line.
    pub fn duration_ticks(&self) -> u32 {
        let (Some(first), Some(last)) = (self.words.first(), self.words.last()) else {
            return 0;
        };
        u32::try_from(last.t_off.saturating_sub(first.t_on)).unwrap_or(u32::MAX)
    }

    /// The line as text: the words joined by one space.
    ///
    /// This is the TTS text and the bark key's raw form; the score's meaning
    /// lives in the tags, not in the string.
    pub fn prose(&self) -> String {
        let mut out = String::new();
        for (index, word) in self.words.iter().enumerate() {
            if index > 0 {
                out.push(' ');
            }
            out.push_str(&word.text);
        }
        out
    }
}

/// A line, as one listener heard it: the truth, degraded.
///
/// The score is the truth; this is the truth as it reached one ear. The
/// words are the ones that survived the distance and the walls, with
/// their lanes flattened; the tags are the ones the listener understood
/// (`None` below the thresholds), because a line heard badly is still a
/// line — it just no longer says what it meant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heard {
    /// The canonical id of the line: the hash of the score it was degraded
    /// from, so the same line always carries the same id (and the radio's
    /// dropouts, which hash it, always drop the same words).
    pub id: u64,
    /// The agent who spoke the line.
    pub speaker: u16,
    /// The channel the line was spoken on.
    pub channel: Channel,
    /// Who the line was for.
    ///
    /// The addressee survives the degradation: it is *who was spoken to*,
    /// not something the listener has to understand. A listener who cannot
    /// make out the gesture or the point still knows the line was meant
    /// for them -- and a squad member who was named may answer it (D31).
    pub addressee: Addressee,
    /// The gesture, if the listener understood it.
    pub act: Option<SpeechAct>,
    /// The point of the line, if the listener understood it.
    pub intent: Option<Intent>,
    /// The affect bucket: the tone of a line survives farther than its
    /// gesture.
    pub affect: Affect,
    /// What the line is about.
    pub subject: Subject,
    /// How clear the line reached this listener, in Q16.
    pub clarity_q16: u32,
    /// The words that arrived, with their degraded lanes.
    pub words: Vec<Word>,
    /// How many words the speaker said, dropped ones included.
    pub spoken_words: u16,
    /// The tick the line started on.
    pub started_tick: Tick,
    /// The tick the line ended on.
    pub ended_tick: Tick,
    /// Whether the line is still being spoken.
    pub ongoing: bool,
}

impl Heard {
    /// Whether the listener understood the speech act.
    pub fn act_known(&self) -> bool {
        self.act.is_some()
    }

    /// Whether the listener understood the intent.
    pub fn intent_known(&self) -> bool {
        self.intent.is_some()
    }

    /// Whether the listener got anything at all: a word, a gesture or a
    /// point.
    pub fn heard_anything(&self) -> bool {
        !self.words.is_empty() || self.act.is_some() || self.intent.is_some()
    }
}

impl StateHash for Channel {
    fn hash_into(&self, h: &mut Fnv64) {
        h.write(&[self.tag()]);
    }
}

impl StateHash for SpeechAct {
    fn hash_into(&self, h: &mut Fnv64) {
        h.write(&[self.tag()]);
    }
}

impl StateHash for Intent {
    fn hash_into(&self, h: &mut Fnv64) {
        h.write(&[self.tag()]);
    }
}

impl StateHash for Affect {
    fn hash_into(&self, h: &mut Fnv64) {
        h.write(&[self.tag()]);
    }
}

impl StateHash for Addressee {
    fn hash_into(&self, h: &mut Fnv64) {
        h.write(&[self.tag()]);
        if let Addressee::Agent(agent) = self {
            h.u16(*agent);
        }
    }
}

impl StateHash for Subject {
    fn hash_into(&self, h: &mut Fnv64) {
        h.write(&[self.tag()]);
        match self {
            Subject::Beacon(beacon) => h.write(&[*beacon]),
            Subject::Enemy(enemy) => h.u16(*enemy),
            Subject::Direction(direction) => h.write(&[*direction]),
            Subject::None => {}
        }
    }
}

impl StateHash for Prosody {
    fn hash_into(&self, h: &mut Fnv64) {
        h.write(&[self.tag()]);
    }
}

impl StateHash for Word {
    fn hash_into(&self, h: &mut Fnv64) {
        h.u32(self.text.len() as u32);
        h.write(self.text.as_bytes());
        h.u64(self.t_on);
        h.u64(self.t_off);
        h.write(&[self.pitch_steps as u8]);
        h.u32(self.energy_q16);
        h.u32(self.rate_q16);
        h.u32(self.emphasis_q16);
        h.u16(self.pause_after_ticks);
    }
}

impl StateHash for Utterance {
    fn hash_into(&self, h: &mut Fnv64) {
        h.u16(self.speaker);
        self.channel.hash_into(h);
        self.addressee.hash_into(h);
        self.act.hash_into(h);
        self.intent.hash_into(h);
        self.affect.hash_into(h);
        self.subject.hash_into(h);
        self.prosody.hash_into(h);
        h.u32(self.words.len() as u32);
        for word in &self.words {
            word.hash_into(h);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A word on the timeline, ten ticks long, one beat apart.
    fn word(text: &str, at: Tick) -> Word {
        Word::new(text, at, at + 10)
    }

    /// The score of a four-word order.
    fn sample() -> Utterance {
        Utterance {
            speaker: 2,
            channel: Channel::Voice,
            addressee: Addressee::Squad,
            act: SpeechAct::Command,
            intent: Intent::Order,
            affect: Affect::Urgent,
            subject: Subject::Beacon(1),
            prosody: Prosody::Synthesized,
            words: vec![
                word("hold", 0),
                word("the", 10),
                word("north", 20),
                word("beacon", 30),
            ],
        }
    }

    #[test]
    fn utterance_hash_is_stable_and_sensitive() {
        let base = sample();
        assert_eq!(base.state_hash(), base.clone().state_hash());

        // Every tag moves the hash.
        let mut variants: Vec<Utterance> = Vec::new();
        let mut v = base.clone();
        v.speaker = 3;
        variants.push(v);
        let mut v = base.clone();
        v.channel = Channel::Radio;
        variants.push(v);
        let mut v = base.clone();
        v.addressee = Addressee::Agent(4);
        variants.push(v);
        let mut v = base.clone();
        v.act = SpeechAct::Question;
        variants.push(v);
        let mut v = base.clone();
        v.intent = Intent::Report;
        variants.push(v);
        let mut v = base.clone();
        v.affect = Affect::Calm;
        variants.push(v);
        let mut v = base.clone();
        v.subject = Subject::Enemy(9);
        variants.push(v);
        let mut v = base.clone();
        v.prosody = Prosody::Measured;
        variants.push(v);
        for variant in &variants {
            assert_ne!(base.state_hash(), variant.state_hash());
        }

        // Every lane moves the hash.
        let mut v = base.clone();
        v.words[0].pitch_steps = 3;
        assert_ne!(base.state_hash(), v.state_hash());
        let mut v = base.clone();
        v.words[0].energy_q16 = 32_768;
        assert_ne!(base.state_hash(), v.state_hash());
        let mut v = base.clone();
        v.words[0].rate_q16 = 32_768;
        assert_ne!(base.state_hash(), v.state_hash());
        let mut v = base.clone();
        v.words[0].emphasis_q16 = 32_768;
        assert_ne!(base.state_hash(), v.state_hash());
        let mut v = base.clone();
        v.words[0].pause_after_ticks = 5;
        assert_ne!(base.state_hash(), v.state_hash());

        // And so does every word: its text, its place and its length.
        let mut v = base.clone();
        v.words[1].text = "that".to_owned();
        assert_ne!(base.state_hash(), v.state_hash());
        let mut v = base.clone();
        v.words[1].t_on = 11;
        assert_ne!(base.state_hash(), v.state_hash());
        let mut v = base.clone();
        v.words[1].t_off = 21;
        assert_ne!(base.state_hash(), v.state_hash());
        let mut v = base.clone();
        v.words.pop();
        assert_ne!(base.state_hash(), v.state_hash());
    }

    #[test]
    fn the_tags_hash_apart() {
        // A subject that is none is not a subject that is missing, and an
        // addressee that is a squad is not an addressee that is everyone.
        assert_ne!(Subject::None.state_hash(), Subject::Beacon(0).state_hash());
        assert_ne!(
            Subject::Beacon(0).state_hash(),
            Subject::Beacon(1).state_hash()
        );
        assert_ne!(
            Subject::Enemy(1).state_hash(),
            Subject::Beacon(1).state_hash()
        );
        assert_ne!(Addressee::Squad.state_hash(), Addressee::All.state_hash());
        assert_ne!(
            Addressee::Agent(1).state_hash(),
            Addressee::Agent(2).state_hash()
        );
        assert_ne!(Channel::Voice.state_hash(), Channel::Radio.state_hash());
        assert_ne!(
            SpeechAct::Command.state_hash(),
            SpeechAct::Callout.state_hash()
        );
        assert_ne!(Intent::Order.state_hash(), Intent::Ack.state_hash());
        assert_ne!(Affect::Calm.state_hash(), Affect::Urgent.state_hash());
        assert_ne!(Prosody::Absent.state_hash(), Prosody::Measured.state_hash());
    }

    #[test]
    fn duration_is_the_voice_not_the_silence() {
        let mut u = sample();
        // The trailing rest is silence: it does not count.
        u.words.last_mut().expect("a word").pause_after_ticks = 30;
        assert_eq!(u.duration_ticks(), 40);
        // An empty line lasts no time at all.
        u.words.clear();
        assert_eq!(u.duration_ticks(), 0);
    }

    #[test]
    fn prose_joins_the_words_with_one_space() {
        assert_eq!(sample().prose(), "hold the north beacon");
        let mut u = sample();
        u.words.clear();
        assert_eq!(u.prose(), "");
    }

    #[test]
    fn the_pitch_is_clamped_to_the_range_a_voice_can_reach() {
        assert_eq!(Word::new("x", 0, 1).with_pitch(99).pitch_steps, 24);
        assert_eq!(Word::new("x", 0, 1).with_pitch(-99).pitch_steps, -24);
        assert_eq!(Word::new("x", 0, 1).with_pitch(4).pitch_steps, 4);

        let mut w = Word::new("x", 0, 1);
        w.pitch_steps = 40;
        w.clamp_pitch();
        assert_eq!(w.pitch_steps, 24);
    }
}
