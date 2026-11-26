//! The human's voice: the commands, the tutor ladder and the score.
//!
//! Three rules shape what the human says (D29):
//!
//! - **negations are refused, not interpreted.** A sentence that holds a
//!   negation token is refused by name: "don't stop" is not "stop", it is
//!   a refusal. Nothing is submitted, and the round does not move;
//! - **the tutor ladder teaches the verbs.** The human's verbs unlock tier
//!   by tier, and a locked verb is **refused with the tier it needs and how
//!   to unlock it** -- never silently reinterpreted;
//! - **the score is the truth.** What the human said becomes a score, the
//!   score becomes a decision with `Provenance::Human`, and the decision is
//!   logged, hashed and replayed like any other.
//!
//! The parser is the client's, not the round's: it is the rules of one
//! person talking to one game, and the round never sees them.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lm_core::talk::TalkParams;
use lm_core::utterance::Utterance;
use lm_voice::{
    build_human_utterance, check_mic, Asr, Capture, MeasuredProsody, Transcript, VoiceError,
};

/// The tokens that make a sentence a negation.
///
/// "don't", "dont" and "n't" are all in it, and so is the bare "not": a
/// sentence that holds one of them is refused, because a command the
/// system has to *interpret* is a command it will get wrong.
pub const NEGATIONS: [&str; 10] = [
    "not", "n't", "never", "no", "dont", "don't", "without", "except", "avoid", "quit",
];

/// Whether the tutor ladder is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TutorMode {
    /// Every verb is allowed: the ladder is off (tournaments).
    Off,
    /// The verbs unlock tier by tier.
    Ladder,
}

/// The verbs of the tutor ladder, tier by tier.
///
/// Tier one is what the human may say at the start; each tier unlocks the
/// next after its verbs have been used often enough.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TutorLadder {
    /// The tiers, tier one first.
    pub tiers: Vec<Vec<String>>,
    /// How many uses of a verb unlock the next tier.
    pub uses_to_unlock: u32,
}

impl Default for TutorLadder {
    /// The shipped ladder.
    fn default() -> Self {
        TutorLadder {
            tiers: vec![
                vec!["hold".to_owned(), "wait".to_owned(), "stay".to_owned()],
                vec![
                    "north".to_owned(),
                    "south".to_owned(),
                    "east".to_owned(),
                    "west".to_owned(),
                ],
                vec!["follow".to_owned(), "leader".to_owned()],
                vec![
                    "retreat".to_owned(),
                    "fall back".to_owned(),
                    "beacon".to_owned(),
                ],
            ],
            uses_to_unlock: 3,
        }
    }
}

impl TutorLadder {
    /// The shipped ladder.
    pub fn shipped() -> TutorLadder {
        TutorLadder::default()
    }

    /// The tier a verb belongs to, counting from one.
    pub fn tier_of(&self, verb: &str) -> Option<usize> {
        self.tiers
            .iter()
            .position(|tier| tier.iter().any(|word| word == verb))
            .map(|index| index + 1)
    }

    /// The verbs of a tier, counting from one.
    pub fn verbs(&self, tier: usize) -> &[String] {
        self.tiers
            .get(tier.wrapping_sub(1))
            .map_or(&[], Vec::as_slice)
    }
}

/// Why a command was refused.
///
/// Every refusal is named (D26): the HUD says what was wrong, and the
/// round is not moved by a guess.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandRefusal {
    /// The sentence holds a negation.
    Negation {
        /// The token that was found.
        token: String,
    },
    /// The verb is locked behind a tier the human has not reached.
    Locked {
        /// The verb that was refused.
        verb: String,
        /// The tier the verb is in.
        tier: usize,
        /// How many uses of a verb unlock a tier.
        uses_to_unlock: u32,
    },
    /// The sentence holds no verb the ladder knows.
    NoVerb,
}

impl fmt::Display for CommandRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandRefusal::Negation { token } => {
                write!(f, "the sentence is a negation ('{token}')")
            }
            CommandRefusal::Locked {
                verb,
                tier,
                uses_to_unlock,
            } => write!(
                f,
                "'{verb}' is in tier {tier}: use a tier {tier} verb {uses_to_unlock} times to unlock it"
            ),
            CommandRefusal::NoVerb => f.write_str("the sentence holds no command"),
        }
    }
}

/// What a spoken sentence turned out to be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandOutcome {
    /// The sentence is a command, and it is allowed.
    Accepted {
        /// The verb that was said.
        verb: String,
        /// The tier the verb is in.
        tier: usize,
    },
    /// The sentence is refused, by name.
    Refused(CommandRefusal),
}

/// The human's command parser: the negation guard and the tutor ladder.
///
/// The parser holds the human's progress on the ladder: how many times
/// each verb has been used, and how far up the ladder they have got.
#[derive(Clone, Debug)]
pub struct CommandParser {
    ladder: TutorLadder,
    mode: TutorMode,
    uses: BTreeMap<String, u32>,
    unlocked: usize,
}

impl CommandParser {
    /// A parser with a ladder and a mode.
    pub fn new(ladder: TutorLadder, mode: TutorMode) -> CommandParser {
        CommandParser {
            ladder,
            mode,
            uses: BTreeMap::new(),
            unlocked: 1,
        }
    }

    /// The ladder the parser is following.
    pub fn ladder(&self) -> &TutorLadder {
        &self.ladder
    }

    /// The mode the parser is in.
    pub fn mode(&self) -> TutorMode {
        self.mode
    }

    /// The highest tier the human has reached.
    pub fn unlocked_tier(&self) -> usize {
        self.unlocked
    }

    /// How many times a verb has been used.
    pub fn uses(&self, verb: &str) -> u32 {
        self.uses.get(verb).copied().unwrap_or(0)
    }

    /// The verb a sentence holds, if it holds one.
    ///
    /// The verb is looked for in the words: the two-word verbs first ("fall
    /// back"), then the one-word ones. The verb is the *first* one the
    /// sentence holds, so "hold the north beacon" is a hold and not a
    /// north.
    pub fn verb_in(&self, text: &str) -> Option<(String, usize)> {
        let words = tokenize(text);
        self.find_verb(&words)
    }

    /// Read a sentence: a command, or a refusal by name.
    ///
    /// A negation is refused before anything else: the sentence is not
    /// read as a command at all.
    pub fn parse(&mut self, text: &str) -> CommandOutcome {
        let words = tokenize(text);
        for word in &words {
            if is_negation(word) {
                return CommandOutcome::Refused(CommandRefusal::Negation {
                    token: word.clone(),
                });
            }
        }
        let Some((verb, tier)) = self.find_verb(&words) else {
            return CommandOutcome::Refused(CommandRefusal::NoVerb);
        };
        if self.mode == TutorMode::Ladder && tier > self.unlocked {
            return CommandOutcome::Refused(CommandRefusal::Locked {
                verb,
                tier,
                uses_to_unlock: self.ladder.uses_to_unlock,
            });
        }

        // The use is counted, and a verb that has been used enough times
        // unlocks the tier above it.
        let uses = self.uses.entry(verb.clone()).or_insert(0);
        *uses = uses.saturating_add(1);
        if self.mode == TutorMode::Ladder
            && tier == self.unlocked
            && *uses >= self.ladder.uses_to_unlock
        {
            self.unlocked = (tier + 1).min(self.ladder.tiers.len().max(1));
        }

        CommandOutcome::Accepted { verb, tier }
    }

    /// The verb the words hold, and the tier it is in.
    fn find_verb(&self, words: &[String]) -> Option<(String, usize)> {
        for index in 0..words.len() {
            // The two-word verbs first: "fall back".
            if let Some(next) = words.get(index + 1) {
                let phrase = format!("{} {}", words[index], next);
                if let Some(tier) = self.ladder.tier_of(&phrase) {
                    return Some((phrase, tier));
                }
            }
            // Then the one-word ones, with the aliases resolved: "stop" is
            // "hold".
            let name = alias_of(&words[index]);
            if let Some(tier) = self.ladder.tier_of(&name) {
                return Some((name, tier));
            }
        }
        None
    }
}

/// The verbs that are another verb's name.
const VERB_ALIASES: [(&str, &str); 1] = [("stop", "hold")];

/// The name a word stands for.
fn alias_of(word: &str) -> String {
    VERB_ALIASES
        .iter()
        .find(|(from, _)| *from == word)
        .map(|(_, to)| (*to).to_owned())
        .unwrap_or_else(|| word.to_owned())
}

/// The words of a sentence, lowercased and without their punctuation.
fn tokenize(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|word| {
            word.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '\u{2019}')
                .to_ascii_lowercase()
        })
        .filter(|word| !word.is_empty())
        .collect()
}

/// Whether a word is a negation.
///
/// The apostrophes are taken out before the word is looked up, so "don't"
/// and "dont" are the same word; and a word that ends in "n't" is a
/// negation whatever else it is ("can't", "won't", "isn't").
fn is_negation(word: &str) -> bool {
    let stripped: String = word
        .chars()
        .filter(|c| *c != '\'' && *c != '\u{2019}')
        .collect();
    NEGATIONS.contains(&stripped.as_str()) || word.ends_with("n't")
}

/// Why the human's voice did not become a decision.
///
/// A refusal is an outcome, never a guess (D26).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VoiceRefusal {
    /// The capture was refused: the microphone is dead.
    Mic(VoiceError),
    /// The voice service failed.
    Voice(VoiceError),
    /// The words were refused.
    Command(CommandRefusal),
}

impl fmt::Display for VoiceRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VoiceRefusal::Mic(why) => write!(f, "the microphone: {why}"),
            VoiceRefusal::Voice(why) => write!(f, "the voice service: {why}"),
            VoiceRefusal::Command(why) => write!(f, "the command: {why}"),
        }
    }
}

/// What the ears did with a turn.
///
/// The three names are the three ways a line can be scored, and the HUD
/// counts them: the ears heard the line, the ears were too late for it,
/// or the ears were not there at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EarsOutcome {
    /// The ears answered in time: the line is theirs.
    Heard,
    /// The ears were too late: the stub stood in.
    Late,
    /// The ears were not there: the stub stood in.
    Down,
}

/// The counters the HUD shows: what the ears did, turn by turn.
///
/// Every turn ends in exactly one of the three, so the three counters add
/// up to the number of turns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EarsStats {
    /// How many turns the ears heard.
    pub heard: u64,
    /// How many turns the ears were too late for.
    pub late: u64,
    /// How many turns the ears were not there for.
    pub down: u64,
}

impl EarsStats {
    /// How many turns there were.
    pub fn total(&self) -> u64 {
        self.heard + self.late + self.down
    }
}

/// The last turn: the line, and what the ears did with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnReport {
    /// The line, as it was heard.
    pub text: String,
    /// What the ears did with the turn.
    pub outcome: EarsOutcome,
    /// What the line was read as: the verb and its tier, or the refusal.
    pub command: CommandOutcome,
}

/// How long a turn waits for the ears before the stub stands in.
///
/// The ears have a deadline of their own, and this one is the last word:
/// it is past theirs, so the thread's own answer is the one that lands
/// first. It is the backstop for a thread that does not come back at all.
pub const DEFAULT_TURN_DEADLINE: Duration = Duration::from_millis(2_500);

/// A turn in flight: the ears are being asked, off the thread.
struct Pending {
    /// The thread's answer.
    ///
    /// The receiver is behind a lock because it is not shareable on its
    /// own, and the host keeps the voice in a resource that every system
    /// can see.
    rx: Mutex<mpsc::Receiver<Answer>>,
    /// When the turn has to end, whatever the ears say.
    deadline: Instant,
    /// The samples, kept for the stub that stands in.
    pcm: Vec<i16>,
    /// The speaker of the line.
    speaker: u16,
    /// The talk's rules, for the score.
    talk: TalkParams,
}

/// What the thread came back with.
struct Answer {
    /// The words.
    transcript: Transcript,
    /// The measurement, when a service answered.
    measured: Option<MeasuredProsody>,
    /// What the ears did.
    outcome: EarsOutcome,
}

/// What a turn cost: the wall clock, and what it was made of.
///
/// The two numbers are the split the round's budget is read with: what the
/// network cost, and what the machine did. The service's own time is the
/// third: it is the part of the compute that happened on the other side of
/// the wire.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TurnCost {
    /// The whole turn: the key coming up to the line being applied.
    pub total_ms: u64,
    /// The round trip: the call to the ears, less the service's own time.
    pub network_ms: u64,
    /// The service's own time, as it reported it.
    pub server_ms: u64,
}

impl TurnCost {
    /// The compute: the service's own time and the host's own work.
    pub fn compute_ms(&self) -> u64 {
        self.total_ms.saturating_sub(self.network_ms)
    }
}

/// The percentile of a list of numbers.
///
/// The nearest rank: the p-th percentile is the value at the rank
/// `ceil(p/100 × n)`, counting from one. With twenty values, p50 is the
/// tenth and p95 the nineteenth.
pub fn percentile(values: &[u64], p: u8) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let count = sorted.len();
    let rank = (u64::from(p) * count as u64).div_ceil(100).max(1);
    let index = usize::try_from(rank - 1)
        .unwrap_or(count - 1)
        .min(count - 1);
    sorted.get(index).copied()
}

/// The human's voice: the microphone path, end to end.
///
/// The path is **capture → the words → the command → the measurement → the
/// score**. A dead microphone, a negation and a locked verb all refuse:
/// nothing is submitted, and the round does not move. The prosody service
/// is optional: when it is absent or when it fails, the score is built
/// without prosody and the round never waits for it.
///
/// **The round never waits for the ears.** A turn is begun with
/// [`HumanVoice::begin`], the words are asked for on a thread, and the
/// score comes back when it is ready with [`HumanVoice::poll`]. An ear
/// that is late or down does not hold the round up: the stub stands in,
/// and the HUD says which of the three it was.
pub struct HumanVoice {
    capture: Arc<Capture>,
    fallback: Option<Arc<dyn Asr>>,
    parser: CommandParser,
    zero_fraction_max_q16: u32,
    measured: bool,
    ears: EarsStats,
    pending: Option<Pending>,
    last: Option<TurnReport>,
    deadline: Duration,
}

impl HumanVoice {
    /// The path, over a capture and a parser.
    pub fn new(capture: Capture, parser: CommandParser) -> HumanVoice {
        HumanVoice {
            capture: Arc::new(capture),
            fallback: None,
            parser,
            zero_fraction_max_q16: lm_voice::DEFAULT_ZERO_FRACTION_MAX_Q16,
            measured: false,
            ears: EarsStats::default(),
            pending: None,
            last: None,
            deadline: DEFAULT_TURN_DEADLINE,
        }
    }

    /// The path with another limit on a dead microphone.
    pub fn with_zero_fraction_max(mut self, zero_fraction_max_q16: u32) -> HumanVoice {
        self.zero_fraction_max_q16 = zero_fraction_max_q16;
        self
    }

    /// The path with a stub behind it.
    ///
    /// The stub is the app's own line: the one the round had before the
    /// ears existed. When the ears are late or down, the stub stands in
    /// and the line falls back exactly as it did before.
    pub fn with_fallback(mut self, stub: Arc<dyn Asr>) -> HumanVoice {
        self.fallback = Some(stub);
        self
    }

    /// The path with another deadline for a turn.
    pub fn with_deadline(mut self, deadline: Duration) -> HumanVoice {
        self.deadline = deadline;
        self
    }

    /// The command parser, for the HUD's counters.
    pub fn parser(&self) -> &CommandParser {
        &self.parser
    }

    /// The command parser, for the HUD's counters.
    pub fn parser_mut(&mut self) -> &mut CommandParser {
        &mut self.parser
    }

    /// What the ears did, turn by turn.
    pub fn ears(&self) -> EarsStats {
        self.ears
    }

    /// The last turn: the line, and what the ears did with it.
    pub fn last_turn(&self) -> Option<&TurnReport> {
        self.last.as_ref()
    }

    /// Whether a turn is in flight: the ears have not answered yet.
    pub fn turn_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Whether the last capture was measured.
    ///
    /// The prosody service is optional, so the host says so when the line
    /// was scored without prosody: the words are there and the score is
    /// marked absent, and the person can see that it was not measured.
    pub fn prosody_measured(&self) -> bool {
        self.measured
    }

    /// Whether a prosody service is behind the capture at all.
    ///
    /// It is the difference between "the service did not answer" and "there
    /// is no service": the first is a fault, the second is the shape of the
    /// machine.
    pub fn prosody_configured(&self) -> bool {
        self.capture.has_prosody()
    }

    /// Begin a turn: the capture is read on a thread, and the round goes on.
    ///
    /// The words come back later, and [`HumanVoice::poll`] gives the score
    /// when they do. A dead microphone is refused here and now: nothing is
    /// submitted, and the round does not move.
    pub fn begin(
        &mut self,
        pcm: &[i16],
        speaker: u16,
        talk: &TalkParams,
    ) -> Result<(), VoiceRefusal> {
        check_mic(pcm, self.zero_fraction_max_q16).map_err(VoiceRefusal::Mic)?;
        // A turn that is still in flight is dropped: the human spoke
        // again, and the new line is the one that counts. The dropped turn
        // is late -- its answer never landed.
        if self.pending.take().is_some() {
            self.ears.late = self.ears.late.saturating_add(1);
        }
        let capture = Arc::clone(&self.capture);
        let fallback = self.fallback.clone();
        let samples = pcm.to_vec();
        let rate = capture.sample_rate();
        let (tx, rx) = mpsc::channel();
        // The asking happens on a thread: the round does not wait for the
        // ears, and the frame does not stop for them.
        std::thread::spawn(move || {
            let answer = match capture.read(&samples) {
                Ok((transcript, measured)) => Answer {
                    transcript,
                    measured,
                    outcome: EarsOutcome::Heard,
                },
                Err(VoiceError::Timeout) => Answer {
                    transcript: stub_says(&fallback, &samples, rate),
                    measured: None,
                    outcome: EarsOutcome::Late,
                },
                Err(_) => Answer {
                    transcript: stub_says(&fallback, &samples, rate),
                    measured: None,
                    outcome: EarsOutcome::Down,
                },
            };
            let _ = tx.send(answer);
        });
        self.pending = Some(Pending {
            rx: Mutex::new(rx),
            deadline: Instant::now() + self.deadline,
            pcm: pcm.to_vec(),
            speaker,
            talk: talk.clone(),
        });
        Ok(())
    }

    /// The score of the turn, when it is ready.
    ///
    /// `None` means the ears have not answered yet: the round goes on
    /// without them, and the host asks again next frame. A refusal means
    /// the line was not a command, and nothing is submitted.
    pub fn poll(&mut self) -> Option<Result<Utterance, VoiceRefusal>> {
        let answer = {
            let pending = self.pending.as_ref()?;
            let received = match pending.rx.lock() {
                Ok(rx) => rx.try_recv(),
                // A lock that is poisoned means the thread died holding it:
                // the ears are down.
                Err(_) => Err(mpsc::TryRecvError::Disconnected),
            };
            match received {
                Ok(answer) => answer,
                Err(mpsc::TryRecvError::Empty) => {
                    if Instant::now() < pending.deadline {
                        return None;
                    }
                    // The deadline passed: the stub stands in.
                    self.stub_answer(pending, EarsOutcome::Late)
                }
                // The thread is gone: the ears are down.
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.stub_answer(pending, EarsOutcome::Down)
                }
            }
        };
        let pending = self.pending.take()?;
        self.count(answer.outcome);
        self.measured = answer.measured.is_some();
        let command = self.parser.parse(&answer.transcript.text);
        self.last = Some(TurnReport {
            text: answer.transcript.text.clone(),
            outcome: answer.outcome,
            command: command.clone(),
        });
        match command {
            CommandOutcome::Refused(why) => Some(Err(VoiceRefusal::Command(why))),
            CommandOutcome::Accepted { .. } => Some(Ok(build_human_utterance(
                &answer.transcript,
                answer.measured.as_ref(),
                pending.speaker,
                &pending.talk,
            ))),
        }
    }

    /// Read a capture into the score of a line, waiting for the ears.
    ///
    /// This is the blocking form of the path, and it is here for the
    /// callers that have nowhere else to wait. The host does not use it:
    /// the round must not wait for the ears, so the host begins the turn
    /// and polls for the score instead.
    pub fn capture(
        &mut self,
        pcm: &[i16],
        speaker: u16,
        p: &TalkParams,
    ) -> Result<Utterance, VoiceRefusal> {
        self.begin(pcm, speaker, p)?;
        loop {
            if let Some(outcome) = self.poll() {
                return outcome;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// The answer the stub gives when the ears do not.
    fn stub_answer(&self, pending: &Pending, outcome: EarsOutcome) -> Answer {
        let rate = self.capture.sample_rate();
        Answer {
            transcript: stub_says(&self.fallback, &pending.pcm, rate),
            measured: None,
            outcome,
        }
    }

    /// Count the turn.
    fn count(&mut self, outcome: EarsOutcome) {
        match outcome {
            EarsOutcome::Heard => self.ears.heard = self.ears.heard.saturating_add(1),
            EarsOutcome::Late => self.ears.late = self.ears.late.saturating_add(1),
            EarsOutcome::Down => self.ears.down = self.ears.down.saturating_add(1),
        }
    }
}

/// The words the stub gives when the ears do not.
///
/// The stub is the app's own line: the one the round had before the ears
/// existed. With no stub behind it there are no words at all, and the
/// line is refused by name rather than guessed.
fn stub_says(fallback: &Option<Arc<dyn Asr>>, pcm: &[i16], rate: u32) -> Transcript {
    match fallback {
        Some(asr) => asr.transcribe(pcm, rate).unwrap_or(Transcript {
            words: Vec::new(),
            text: String::new(),
        }),
        None => Transcript {
            words: Vec::new(),
            text: String::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lm_core::decision::{Kind, Provenance, Source, Stamped};
    use lm_core::header::Header;
    use lm_core::log::RoundLog;
    use lm_core::utterance::Prosody;
    use lm_tools::runner::{self, DEFAULT_REFLEX};
    use lm_voice::{MeasuredProsody, ProsodyPort, StubAsr};

    use crate::host_loop::HostLoop;

    /// A standard round with the shipped reflex.
    fn round() -> Header {
        Header::default_round(1, DEFAULT_REFLEX)
    }

    /// The human's voice, over the stub ASR, with the shipped ladder.
    fn voice(said: &str) -> HumanVoice {
        let capture = Capture::new(Arc::new(StubAsr::scripted(said)));
        HumanVoice::new(
            capture,
            CommandParser::new(TutorLadder::shipped(), TutorMode::Ladder),
        )
    }

    /// A prosody service that always fails.
    struct FailingProsody;

    impl ProsodyPort for FailingProsody {
        fn measure(&self, _pcm: &[i16], _sample_rate: u32) -> Result<MeasuredProsody, VoiceError> {
            Err(VoiceError::Http(500))
        }
    }

    /// The decisions a log holds from the human.
    fn human_decisions(log: &RoundLog) -> Vec<&Stamped> {
        log.segments
            .iter()
            .flat_map(|segment| segment.decisions.iter())
            .filter(|stamped| matches!(stamped.d.source, Source::Human(_)))
            .collect()
    }

    #[test]
    fn a_negation_is_refused() {
        let mut host = HostLoop::new(round()).expect("the round starts");
        let talk = host.header().talk.clone();
        let mut voice = voice("don't stop");

        // The refusal names the token: the sentence is not read as a
        // command at all.
        let err = voice
            .capture(&[1, 2, 3], 0, &talk)
            .expect_err("the refusal");
        assert_eq!(
            err,
            VoiceRefusal::Command(CommandRefusal::Negation {
                token: "don't".to_owned()
            })
        );

        // Nothing was submitted: the round's hash does not move.
        host.tick();
        let log = host.log_so_far();
        assert!(
            human_decisions(&log).is_empty(),
            "a refused command submits nothing"
        );

        // And the round is the round it would have been without the
        // refusal: the same world, tick for tick.
        let mut quiet = HostLoop::new(round()).expect("the round starts");
        quiet.tick();
        assert_eq!(
            host.sim().state_hash(),
            quiet.sim().state_hash(),
            "the round's hash is unchanged by the refusal"
        );
    }

    #[test]
    fn a_stop_on_its_own_is_a_verb() {
        let mut parser = CommandParser::new(TutorLadder::shipped(), TutorMode::Ladder);

        // "stop" is "hold": a verb of the first tier.
        assert_eq!(
            parser.parse("stop"),
            CommandOutcome::Accepted {
                verb: "hold".to_owned(),
                tier: 1
            }
        );
        // And "don't stop" is a refusal, not a hold.
        assert_eq!(
            parser.parse("don't stop"),
            CommandOutcome::Refused(CommandRefusal::Negation {
                token: "don't".to_owned()
            })
        );
    }

    #[test]
    fn the_tutor_ladder_locks_and_unlocks() {
        let mut parser = CommandParser::new(TutorLadder::shipped(), TutorMode::Ladder);

        // "north" is a tier two verb, and the human is at tier one.
        assert_eq!(
            parser.parse("north"),
            CommandOutcome::Refused(CommandRefusal::Locked {
                verb: "north".to_owned(),
                tier: 2,
                uses_to_unlock: 3
            })
        );
        // The refusal names the tier it needs and how to unlock it.
        let text = CommandRefusal::Locked {
            verb: "north".to_owned(),
            tier: 2,
            uses_to_unlock: 3,
        }
        .to_string();
        assert!(text.contains("tier 2"), "{text}");
        assert!(text.contains("3 times"), "{text}");

        // Three holds and the tier above opens.
        for _ in 0..3 {
            assert_eq!(
                parser.parse("hold"),
                CommandOutcome::Accepted {
                    verb: "hold".to_owned(),
                    tier: 1
                }
            );
        }
        assert_eq!(parser.unlocked_tier(), 2, "the second tier is open");
        assert_eq!(
            parser.parse("north"),
            CommandOutcome::Accepted {
                verb: "north".to_owned(),
                tier: 2
            }
        );
    }

    #[test]
    fn the_ladder_can_be_off() {
        let mut parser = CommandParser::new(TutorLadder::shipped(), TutorMode::Off);

        // With the ladder off every verb is allowed, from the first word.
        assert_eq!(
            parser.parse("north"),
            CommandOutcome::Accepted {
                verb: "north".to_owned(),
                tier: 2
            }
        );
        // But a negation is still a negation.
        assert_eq!(
            parser.parse("don't north"),
            CommandOutcome::Refused(CommandRefusal::Negation {
                token: "don't".to_owned()
            })
        );
    }

    #[test]
    fn a_dead_mic_is_refused() {
        let mut host = HostLoop::new(round()).expect("the round starts");
        let talk = host.header().talk.clone();
        let mut voice = voice("hold");

        // Every sample is zero: the microphone is dead.
        let err = voice
            .capture(&[0; 64], 0, &talk)
            .expect_err("the capture is refused");
        assert_eq!(err, VoiceRefusal::Mic(VoiceError::MicDead));

        // Nothing is submitted: a dead mic would otherwise become a
        // silent, misread command.
        host.tick();
        let log = host.log_so_far();
        assert!(
            human_decisions(&log).is_empty(),
            "a dead microphone submits nothing"
        );
    }

    #[test]
    fn a_live_mic_is_accepted() {
        let mut host = HostLoop::new(round()).expect("the round starts");
        let talk = host.header().talk.clone();
        let mut voice = voice("hold");

        // A capture with no zeros in it: the microphone is alive.
        let utterance = voice
            .capture(&[1, -1, 2, -2], 0, &talk)
            .expect("the capture is read");
        assert_eq!(utterance.prosody, Prosody::Absent);
        assert_eq!(utterance.words.len(), 1);
        // And the host can say so: no service answered.
        assert!(!voice.prosody_measured());

        // And it becomes a decision of the human.
        host.submit_human(0, Kind::Utterance(utterance))
            .expect("the decision is accepted");
        host.tick();
        let log = host.log_so_far();
        assert_eq!(human_decisions(&log).len(), 1, "the line was submitted");
    }

    #[test]
    fn the_human_utterance_is_logged_and_replays() {
        let mut host = HostLoop::new(round()).expect("the round starts");
        let talk = host.header().talk.clone();
        let mut voice = voice("hold");

        let utterance = voice
            .capture(&[1, -1, 2, -2], 0, &talk)
            .expect("the capture is read");
        host.submit_human(0, Kind::Utterance(utterance))
            .expect("the decision is accepted");

        for _ in 0..60 {
            host.tick();
        }
        let log = host.finish();

        // The log holds the human's line, with its provenance.
        let decisions = human_decisions(&log);
        assert_eq!(decisions.len(), 1, "the line is in the log");
        assert_eq!(decisions[0].d.source, Source::Human(0));
        assert_eq!(decisions[0].d.provenance, Provenance::Human);
        assert!(
            matches!(decisions[0].d.kind, Kind::Utterance(_)),
            "the decision is the utterance"
        );

        // And the round replays to the same hashes: the line is a decision
        // like any other.
        let hashes = runner::replay(&log).expect("the round replays");
        assert!(!hashes.is_empty(), "the round took hashes");
        assert_eq!(hashes, log.hashes(), "the replay is the same round");
    }

    #[test]
    fn the_prosody_service_is_optional() {
        // The same line, once with no service and once with a service that
        // fails. The failing service costs the round nothing.
        let without = play_round(None);
        let with = play_round(Some(Arc::new(FailingProsody)));

        assert_eq!(without.hashes, with.hashes, "the same round");
        assert_eq!(without.airtime, with.airtime, "the same airtime");
        assert_eq!(without.ticks, with.ticks, "the same round time");
    }

    /// A round the human speaks in: the hashes, the airtime and the ticks.
    struct Spoken {
        hashes: Vec<(u64, u64)>,
        airtime: u64,
        ticks: u64,
    }

    /// Play a round in which the human says "hold", with a prosody service
    /// or without one.
    fn play_round(prosody: Option<Arc<dyn ProsodyPort>>) -> Spoken {
        let mut host = HostLoop::new(round()).expect("the round starts");
        let talk = host.header().talk.clone();
        let mut capture = Capture::new(Arc::new(StubAsr::scripted("hold")));
        if let Some(port) = prosody {
            capture = capture.with_prosody(port);
        }
        let mut voice = HumanVoice::new(
            capture,
            CommandParser::new(TutorLadder::shipped(), TutorMode::Ladder),
        );

        let utterance = voice
            .capture(&[1, -1, 2, -2], 0, &talk)
            .expect("the capture is read");
        host.submit_human(0, Kind::Utterance(utterance))
            .expect("the decision is accepted");

        let mut hashes = Vec::new();
        for _ in 0..60 {
            let out = host.tick();
            if let Some(hash) = out.hash {
                hashes.push((out.tick, hash));
            }
        }
        let airtime = u64::from(host.sim().agent(0).expect("agent 0").airtime_ticks);
        Spoken {
            hashes,
            airtime,
            ticks: 60,
        }
    }

    /// An ASR that answers after a while, with a line of its own.
    struct SlowAsr {
        said: String,
        delay: Duration,
    }

    impl Asr for SlowAsr {
        fn transcribe(&self, _pcm: &[i16], _rate: u32) -> Result<Transcript, VoiceError> {
            std::thread::sleep(self.delay);
            Ok(Transcript {
                words: Vec::new(),
                text: self.said.clone(),
            })
        }
    }

    /// An ASR that never answers: the ears are down.
    struct DeadAsr;

    impl Asr for DeadAsr {
        fn transcribe(&self, _pcm: &[i16], _rate: u32) -> Result<Transcript, VoiceError> {
            Err(VoiceError::Http(503))
        }
    }

    /// An ASR that is too late: the call itself times out.
    struct TimedOutAsr;

    impl Asr for TimedOutAsr {
        fn transcribe(&self, _pcm: &[i16], _rate: u32) -> Result<Transcript, VoiceError> {
            Err(VoiceError::Timeout)
        }
    }

    /// The voice, over a given ASR, with the stub behind it.
    fn voice_over(asr: Arc<dyn Asr>) -> HumanVoice {
        HumanVoice::new(
            Capture::new(asr),
            CommandParser::new(TutorLadder::shipped(), TutorMode::Ladder),
        )
        .with_fallback(Arc::new(StubAsr::scripted("hold")))
    }

    /// The voice, over the stub ASR, with the stub behind it.
    fn voice_with_stub(said: &str) -> HumanVoice {
        voice_over(Arc::new(StubAsr::scripted(said)))
    }

    #[test]
    fn the_ears_are_counted_when_they_answer() {
        let talk = TalkParams::default();
        let mut voice = voice_with_stub("hold the north beacon");

        voice
            .begin(&[1, -1, 2, -2], 0, &talk)
            .expect("the turn begins");
        let outcome = voice.poll();
        // The thread may not have answered on the first poll.
        let mut outcome = outcome;
        while outcome.is_none() {
            std::thread::sleep(Duration::from_millis(1));
            outcome = voice.poll();
        }

        assert!(
            outcome.expect("the score").is_ok(),
            "the line was a command"
        );
        let ears = voice.ears();
        assert_eq!(ears.heard, 1);
        assert_eq!(ears.late, 0);
        assert_eq!(ears.down, 0);
        assert_eq!(ears.total(), 1);
        // And the line is the one the ears heard.
        let report = voice.last_turn().expect("the turn is reported");
        assert_eq!(report.text, "hold the north beacon");
        assert_eq!(report.outcome, EarsOutcome::Heard);
    }

    #[test]
    fn a_late_ear_falls_back_to_the_stub() {
        let talk = TalkParams::default();
        let mut voice = voice_over(Arc::new(TimedOutAsr));

        voice
            .begin(&[1, -1, 2, -2], 0, &talk)
            .expect("the turn begins");
        let mut outcome = voice.poll();
        while outcome.is_none() {
            std::thread::sleep(Duration::from_millis(1));
            outcome = voice.poll();
        }

        // The line is the stub's: the round has the same line it always had.
        assert!(
            outcome.expect("the score").is_ok(),
            "the line was a command"
        );
        let report = voice.last_turn().expect("the turn is reported");
        assert_eq!(report.text, "hold");
        assert_eq!(report.outcome, EarsOutcome::Late);
        let ears = voice.ears();
        assert_eq!(ears.late, 1);
        assert_eq!(ears.heard, 0);
        assert_eq!(ears.down, 0);
    }

    #[test]
    fn a_dead_ear_falls_back_to_the_stub() {
        let talk = TalkParams::default();
        let mut voice = voice_over(Arc::new(DeadAsr));

        voice
            .begin(&[1, -1, 2, -2], 0, &talk)
            .expect("the turn begins");
        let mut outcome = voice.poll();
        while outcome.is_none() {
            std::thread::sleep(Duration::from_millis(1));
            outcome = voice.poll();
        }

        assert!(
            outcome.expect("the score").is_ok(),
            "the line was a command"
        );
        let report = voice.last_turn().expect("the turn is reported");
        assert_eq!(report.text, "hold");
        assert_eq!(report.outcome, EarsOutcome::Down);
        let ears = voice.ears();
        assert_eq!(ears.down, 1);
        assert_eq!(ears.heard, 0);
        assert_eq!(ears.late, 0);
    }

    #[test]
    fn the_round_does_not_wait_for_the_ears() {
        let talk = TalkParams::default();
        // An ear that takes a second to answer.
        let mut voice = voice_over(Arc::new(SlowAsr {
            said: "hold".to_owned(),
            delay: Duration::from_millis(1_000),
        }));

        let started = Instant::now();
        voice
            .begin(&[1, -1, 2, -2], 0, &talk)
            .expect("the turn begins");
        let waited = started.elapsed();

        // The turn begins at once: the round does not wait for the ears.
        assert!(
            waited < Duration::from_millis(200),
            "the turn waited {waited:?} for the ears"
        );
        // And the score is not there yet: the round goes on without it.
        assert!(voice.poll().is_none(), "the score is not ready yet");
    }

    #[test]
    fn a_dead_microphone_is_refused_before_the_ears_are_asked() {
        let talk = TalkParams::default();
        let mut voice = voice_with_stub("hold");

        let err = voice
            .begin(&[0; 16_000], 0, &talk)
            .expect_err("the microphone is dead");
        assert!(matches!(err, VoiceRefusal::Mic(_)), "got {err:?}");
        // Nothing was asked, and nothing was counted.
        assert_eq!(voice.ears().total(), 0);
    }

    #[test]
    fn a_turn_that_is_dropped_is_late() {
        let talk = TalkParams::default();
        let mut voice = voice_over(Arc::new(SlowAsr {
            said: "hold".to_owned(),
            delay: Duration::from_millis(500),
        }));

        voice
            .begin(&[1, -1, 2, -2], 0, &talk)
            .expect("the turn begins");
        // The human spoke again: the first turn is dropped.
        voice
            .begin(&[1, -1, 2, -2], 0, &talk)
            .expect("the turn begins");

        assert_eq!(voice.ears().late, 1);
        assert_eq!(voice.ears().heard, 0);
    }

    #[test]
    fn a_thread_that_never_answers_does_not_hold_the_round() {
        let talk = TalkParams::default();
        // An ear that never comes back at all: the thread is lost, not
        // slow. The turn's own deadline is the last word.
        let mut voice = voice_over(Arc::new(SlowAsr {
            said: "hold".to_owned(),
            delay: Duration::from_secs(30),
        }))
        .with_deadline(Duration::from_millis(100));

        voice
            .begin(&[1, -1, 2, -2], 0, &talk)
            .expect("the turn begins");

        let mut outcome = voice.poll();
        while outcome.is_none() {
            std::thread::sleep(Duration::from_millis(5));
            outcome = voice.poll();
        }

        // The line is the stub's, and the turn is counted as late.
        assert!(
            outcome.expect("the score").is_ok(),
            "the line was a command"
        );
        let report = voice.last_turn().expect("the turn is reported");
        assert_eq!(report.text, "hold");
        assert_eq!(report.outcome, EarsOutcome::Late);
        assert_eq!(voice.ears().late, 1);
    }
}
