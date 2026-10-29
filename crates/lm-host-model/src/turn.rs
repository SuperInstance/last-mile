//! The human's turn: when the voice stops, and when the turn ends.
//!
//! Two different things are called "the end of the talking", and mixing
//! them up is how a third of Casey's commands get truncated:
//!
//! * the **voice stop** is the pause *inside* a turn ([`VOICE_STOP_MS`],
//!   150 ms): it divides the tempo, it ends nothing;
//! * the **turn end** is the end of the turn ([`TURN_END_MS`], three
//!   seconds of sustained silence -- or the key coming up, or a command
//!   verb): after it the line is finished and the host may answer.
//!
//! Casey's pauses have a median of 0.72 s and 33.8 % of them are a second
//! or more, so a one-second cut-off would eat a third of what he says
//! (fleet/08). Three seconds is the safe end: none of the recorded corpora
//! produce a boundary there, and the longest gap between two utterances
//! anywhere in them is 2.1 s.
//!
//! Push-to-talk is the default ([`TurnRules::ptt`]): the human holds the
//! key and the turn ends when the key comes up, not when he pauses. The
//! key is cut at [`MAX_TALK_MS`] (6 s), so a stuck key cannot talk
//! forever.
//!
//! The rules live in [`TurnRules`] so there is one copy of them: the
//! tracker's and the voice's are the same rules.

/// A pause inside the turn: the voice has stopped for this long.
pub const VOICE_STOP_MS: u32 = 150;

/// The silence that ends the turn.
pub const TURN_END_MS: u32 = 3_000;

/// The longest one turn may be: the key is cut here.
pub const MAX_TALK_MS: u32 = 6_000;

/// The rules of the turn: the client's, and there is one copy of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnRules {
    /// The pause that stops the voice, in milliseconds.
    pub voice_stop_ms: u32,
    /// The silence that ends the turn, in milliseconds.
    pub turn_end_ms: u32,
    /// The cut: the longest turn, in milliseconds.
    pub max_talk_ms: u32,
    /// Whether the turn ends when the key comes up (push-to-talk).
    pub ptt: bool,
}

impl Default for TurnRules {
    /// The shipped rules: the ones tuned to Casey's pauses.
    fn default() -> Self {
        TurnRules {
            voice_stop_ms: VOICE_STOP_MS,
            turn_end_ms: TURN_END_MS,
            max_talk_ms: MAX_TALK_MS,
            ptt: true,
        }
    }
}

/// The tracker of the human's turn.
///
/// It watches the clock and the voice and answers two different questions:
/// has the voice stopped ([`TurnDetector::voice_stopped`]) and is the turn
/// over ([`TurnDetector::turn_over`]). The first is a pause; the second is
/// the end of the line.
pub struct TurnDetector {
    rules: TurnRules,
    /// When the turn began: the press, or the first voice.
    started_ms: Option<u64>,
    /// When the voice was last heard.
    last_voice_ms: Option<u64>,
    /// When the turn was ended on the spot: the key, or a command verb.
    ended_ms: Option<u64>,
}

impl TurnDetector {
    /// A tracker with the end-silence and the cut spelled out.
    ///
    /// `end_silence_ms` is how long the silence must be before the turn
    /// ends, and `max_talk_ms` is where the turn is cut.
    pub fn new(end_silence_ms: u32, max_talk_ms: u32) -> Self {
        TurnDetector::with_rules(TurnRules {
            voice_stop_ms: VOICE_STOP_MS,
            turn_end_ms: end_silence_ms,
            max_talk_ms,
            ptt: true,
        })
    }

    /// A tracker with Casey's rules: the shipped ones.
    pub fn default_for_casey() -> Self {
        TurnDetector::with_rules(TurnRules::default())
    }

    /// A tracker with the rules given.
    pub fn with_rules(rules: TurnRules) -> Self {
        TurnDetector {
            rules,
            started_ms: None,
            last_voice_ms: None,
            ended_ms: None,
        }
    }

    /// The rules this tracker runs on.
    pub fn rules(&self) -> TurnRules {
        self.rules
    }

    /// The key goes down: the turn starts.
    pub fn on_press(&mut self, now_ms: u64) {
        self.started_ms = Some(now_ms);
        self.last_voice_ms = None;
        self.ended_ms = None;
    }

    /// The voice is heard.
    ///
    /// The voice does not end anything: it keeps the turn open and it
    /// resets the silence. A voice that comes after a long silence is the
    /// *next* turn, though: the old one was already closed.
    pub fn on_voice(&mut self, now_ms: u64) {
        // A turn ended on the key or on a command is finished: the host has
        // already taken the line, and the voice does not reopen it.
        if self.ended_ms.is_some() {
            return;
        }
        match self.last_activity_ms() {
            None => {
                // The first voice of the turn: it opens it.
                self.started_ms = Some(now_ms);
                self.last_voice_ms = Some(now_ms);
            }
            Some(last) => {
                // A long silence closed the turn: this voice is the next one.
                if now_ms.saturating_sub(last) >= self.rules.turn_end_ms as u64 {
                    self.started_ms = Some(now_ms);
                }
                self.last_voice_ms = Some(now_ms);
            }
        }
    }

    /// The key comes up: the turn ends at once.
    ///
    /// This is push-to-talk: the release is the end of the turn, not a
    /// pause. Without push-to-talk the release means nothing and the turn
    /// ends on the silence or on the cut.
    pub fn on_release(&mut self, now_ms: u64) {
        if !self.rules.ptt || self.started_ms.is_none() {
            return;
        }
        self.ended_ms = Some(now_ms);
    }

    /// A command verb ends the turn at once.
    ///
    /// "A system that can only see silence will misread every human
    /// decision as a long pause" (communicator's `turn.py`): the verb is
    /// the boundary, so the host need not wait for the silence.
    pub fn on_command(&mut self, now_ms: u64) {
        if self.started_ms.is_none() {
            return;
        }
        self.ended_ms = Some(now_ms);
    }

    /// Has the voice stopped? A pause, not the end of the turn.
    ///
    /// The pause divides the tempo: it is where a breath goes. It never
    /// ends the turn.
    pub fn voice_stopped(&self, now_ms: u64) -> bool {
        // The turn is over: there is no tempo left to divide.
        if self.ended_ms.is_some() {
            return false;
        }
        // Nothing was said yet, so nothing has stopped.
        let Some(last_voice) = self.last_voice_ms else {
            return false;
        };
        now_ms.saturating_sub(last_voice) >= self.rules.voice_stop_ms as u64
    }

    /// Is the turn over?
    ///
    /// It is over when the key came up, when a command verb ended it, when
    /// the silence has lasted [`TurnRules::turn_end_ms`], or when the turn
    /// has run [`TurnRules::max_talk_ms`].
    pub fn turn_over(&self, now_ms: u64) -> bool {
        // The key or a command ended it: over, whatever the clock says.
        if self.ended_ms.is_some() {
            return true;
        }
        let Some(started) = self.started_ms else {
            return false;
        };
        // The cut: the key is cut at max_talk_ms.
        if now_ms.saturating_sub(started) >= self.rules.max_talk_ms as u64 {
            return true;
        }
        // The silence: three seconds of nothing ends the turn.
        let Some(last) = self.last_activity_ms() else {
            return false;
        };
        now_ms.saturating_sub(last) >= self.rules.turn_end_ms as u64
    }

    /// Forget the turn: the next voice opens a new one.
    pub fn reset(&mut self) {
        self.started_ms = None;
        self.last_voice_ms = None;
        self.ended_ms = None;
    }

    /// When something was last heard: the voice, or the press that opened
    /// the turn.
    fn last_activity_ms(&self) -> Option<u64> {
        match (self.started_ms, self.last_voice_ms) {
            (None, None) => None,
            (Some(started), None) => Some(started),
            (None, Some(voice)) => Some(voice),
            (Some(started), Some(voice)) => Some(started.max(voice)),
        }
    }
}

/// The share of the samples that are exactly zero.
pub fn zero_fraction(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 1.0;
    }
    let zeros = samples.iter().filter(|&&x| x == 0.0).count();
    zeros as f32 / samples.len() as f32
}

/// Whether the microphone is deaf: every sample is zero.
pub fn mic_is_deaf(samples: &[f32]) -> bool {
    zero_fraction(samples) == 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_second_pause_does_not_end_the_turn() {
        let mut detector = TurnDetector::default_for_casey();
        detector.on_voice(0);
        // Pause until 1000ms - this should not end the turn
        assert!(!detector.turn_over(1000));
    }

    #[test]
    fn two_point_four_seconds_does_not_end_it_but_two_point_five_does() {
        // The end-silence is a rule, so it is given here: this tracker ends
        // the turn after 2.5 s of silence.
        let mut detector = TurnDetector::new(2500, 6000);
        detector.on_voice(0);
        // 2.4 seconds - should not end
        assert!(!detector.turn_over(2400));
        // 2.5 seconds - should end
        assert!(detector.turn_over(2500));
    }

    #[test]
    fn max_turn_cuts_a_long_talker() {
        let mut detector = TurnDetector::default_for_casey();
        // Simulate voice every 100ms up to 7000ms
        for i in 0..70 {
            detector.on_voice(i * 100);
        }
        // At 6000ms, the turn should be over due to max_turn_ms
        assert!(detector.turn_over(6000));
    }

    #[test]
    fn no_turn_before_voice() {
        let detector = TurnDetector::default_for_casey();
        // A fresh detector should never be over
        assert!(!detector.turn_over(0));
        assert!(!detector.turn_over(1000));
    }

    #[test]
    fn deaf_mic_detected() {
        // All zeros -> deaf
        assert!(mic_is_deaf(&[0.0, 0.0, 0.0]));
        // One non-zero sample -> not deaf
        assert!(!mic_is_deaf(&[0.0, 0.0, 1.0]));
        // Empty -> deaf
        assert!(mic_is_deaf(&[]));
    }

    #[test]
    fn three_seconds_of_silence_ends_the_turn() {
        let mut detector = TurnDetector::default_for_casey();
        detector.on_voice(0);
        // 2.5 seconds of silence is still a pause
        assert!(!detector.turn_over(2500));
        // 3 seconds of silence ends the turn
        assert!(detector.turn_over(3000));
    }

    #[test]
    fn push_to_talk_ends_the_turn_on_release() {
        let mut detector = TurnDetector::default_for_casey();
        detector.on_press(0);
        detector.on_voice(100);
        // The pause does not end it: the turn is still open
        assert!(!detector.turn_over(1100));
        // The key comes up: the turn is over
        detector.on_release(1200);
        assert!(detector.turn_over(1200));
    }

    #[test]
    fn a_command_verb_ends_the_turn_immediately() {
        let mut detector = TurnDetector::default_for_casey();
        detector.on_press(0);
        detector.on_voice(100);
        assert!(!detector.turn_over(150));
        // "hold" is a boundary of its own
        detector.on_command(200);
        assert!(detector.turn_over(200));
    }

    #[test]
    fn a_pause_stops_the_voice_but_not_the_turn() {
        let mut detector = TurnDetector::default_for_casey();
        detector.on_voice(0);
        // Still talking at 100ms
        assert!(!detector.voice_stopped(100));
        // The voice has stopped at 150ms, the turn has not
        assert!(detector.voice_stopped(200));
        assert!(!detector.turn_over(200));
    }

    #[test]
    fn without_push_to_talk_the_key_up_does_not_end_the_turn() {
        let rules = TurnRules {
            ptt: false,
            ..TurnRules::default()
        };
        let mut detector = TurnDetector::with_rules(rules);
        detector.on_voice(0);
        detector.on_release(200);
        // The key up means nothing: the silence ends the turn, not the key
        assert!(!detector.turn_over(200));
        assert!(detector.turn_over(3000));
    }
}
