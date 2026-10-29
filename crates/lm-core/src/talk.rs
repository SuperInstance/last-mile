//! The round's talk rules: `TalkParams`.
//!
//! Everything a round needs to know about speech lives here and travels in
//! the round header: how far a shout carries, what a wall costs, how long
//! an order stays live, how much airtime an agent has. The rules are the
//! round's, not the client's: two nodes that agree on the header agree on
//! who hears what, so a round replays the same way on both (D19).
//!
//! The talk params are **not** branchable. A branch may retune how the round
//! plays, but the talk rules change *what the round is* -- who hears what --
//! so they join the header hash and stay out of `Params::BRANCH_FIELDS`.
//!
//! Every value is an integer. The ranges are in 1/1024 m units (the same
//! units the world uses), the clarity thresholds are Q16 fractions (65 536 is
//! 1.0), and the timings are in ticks.

use crate::hash::{Fnv64, StateHash};
use crate::params::ParamError;
use crate::utterance::{Affect, Channel, Intent, SpeechAct, Subject};
use serde::{Deserialize, Serialize};

/// Q16: one whole.
pub const Q16_ONE: u32 = 65_536;

/// What an agent does when it has spent its airtime budget: fall back to
/// its reflex and keep playing.
pub const AIRTIME_ON_EXHAUST_REFLEX: &str = "reflex";
/// What an agent does when it has spent its airtime budget: stop talking.
pub const AIRTIME_ON_EXHAUST_FREEZE: &str = "freeze";

/// The talk rules of a round (the defaults of spec §11, phase 4).
///
/// The fields are in declaration order, and the hash follows that order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TalkParams {
    // Hearing: how far a line carries.
    /// How far a shout carries, in 1/1024 m units (60 m).
    pub range_shout_units: u32,
    /// How far a voice carries, in 1/1024 m units (25 m).
    pub range_voice_units: u32,
    /// How far a whisper carries, in 1/1024 m units (5 m).
    pub range_whisper_units: u32,
    /// The radio reaches the whole squad, whatever the distance and
    /// whatever the walls.
    pub radio_is_squad: bool,
    /// What one wall costs, in Q16 (0.5: half the clarity).
    pub wall_loss_q16: u32,
    /// Below this clarity nothing is heard at all (≈ 0.023).
    pub min_clarity_q16: u32,
    // Hearing: what survives the distance.
    /// Above this clarity the pitch survives intact (0.75).
    pub clarity_pitch_q16: u32,
    /// Above this clarity the pace and the pauses survive (0.25).
    pub clarity_rate_q16: u32,
    /// Above this clarity the speech act is understood (0.5).
    pub act_clarity_q16: u32,
    /// Above this clarity the intent is understood (0.25).
    pub intent_clarity_q16: u32,
    /// A word whose energy times the clarity is below this is dropped (0.05).
    pub word_drop_q16: u32,
    // The radio's own price.
    /// The radio squeezes the pitch to this many steps either side of the
    /// baseline.
    pub radio_pitch_steps: i8,
    /// The radio's word-dropout rate, in Q16 (1/16).
    pub radio_drop_q16: u32,
    // Saying it: the timing of the words.
    /// The base pace, in words per minute (200).
    pub base_wpm: u16,
    /// One beat of the notation, in ticks (5: a sixth of a second at 30 Hz).
    pub beat_ticks: u16,
    /// The shortest a word may be, in ticks (2).
    pub min_word_ticks: u16,
    /// The longest a line may run, in ticks (300: ten seconds).
    pub max_utterance_ticks: u32,
    // The budget.
    /// The airtime budget of one agent, in seconds (20).
    pub airtime_s: u16,
    /// What an agent does when the budget is gone: [`AIRTIME_ON_EXHAUST_REFLEX`]
    /// or [`AIRTIME_ON_EXHAUST_FREEZE`].
    pub airtime_on_exhaust: String,
    // Following an order.
    /// How long an order stays live, in ticks (150: five seconds).
    pub order_uptake_window_ticks: u32,
    // The other side of the fight.
    /// The enemies hear a voice and a shout that are in range.
    pub enemy_overhears_voice: bool,
}

impl Default for TalkParams {
    fn default() -> Self {
        TalkParams {
            range_shout_units: 61_440,
            range_voice_units: 25_600,
            range_whisper_units: 5_120,
            radio_is_squad: true,
            wall_loss_q16: 32_768,
            min_clarity_q16: 1_536,
            clarity_pitch_q16: 49_152,
            clarity_rate_q16: 16_384,
            act_clarity_q16: 32_768,
            intent_clarity_q16: 16_384,
            word_drop_q16: 3_276,
            radio_pitch_steps: 1,
            radio_drop_q16: 4_096,
            base_wpm: 200,
            beat_ticks: 5,
            min_word_ticks: 2,
            max_utterance_ticks: 300,
            airtime_s: 20,
            airtime_on_exhaust: AIRTIME_ON_EXHAUST_REFLEX.to_owned(),
            order_uptake_window_ticks: 150,
            enemy_overhears_voice: true,
        }
    }
}

impl TalkParams {
    /// Refuse nonsense before a round starts, naming the first offending
    /// field in declaration order.
    ///
    /// A range of zero would deafen a channel for good, a beat of zero
    /// would divide by zero when the notation is compiled, and a policy
    /// nobody knows is not a policy at all.
    pub fn validate(&self) -> Result<(), ParamError> {
        for (field, range) in [
            ("range_shout_units", self.range_shout_units),
            ("range_voice_units", self.range_voice_units),
            ("range_whisper_units", self.range_whisper_units),
        ] {
            if range == 0 {
                return Err(ParamError::new(field, "a channel must reach somewhere"));
            }
        }
        for (field, value) in [
            ("wall_loss_q16", self.wall_loss_q16),
            ("min_clarity_q16", self.min_clarity_q16),
            ("clarity_pitch_q16", self.clarity_pitch_q16),
            ("clarity_rate_q16", self.clarity_rate_q16),
            ("act_clarity_q16", self.act_clarity_q16),
            ("intent_clarity_q16", self.intent_clarity_q16),
            ("word_drop_q16", self.word_drop_q16),
            ("radio_drop_q16", self.radio_drop_q16),
        ] {
            if value > Q16_ONE {
                return Err(ParamError::new(
                    field,
                    "the value is Q16; it must be at most 1.0",
                ));
            }
        }
        if self.radio_pitch_steps < 0 || self.radio_pitch_steps > 24 {
            return Err(ParamError::new(
                "radio_pitch_steps",
                "the radio's pitch squeeze must be between 0 and 24 semitones",
            ));
        }
        if self.base_wpm == 0 {
            return Err(ParamError::new(
                "base_wpm",
                "the base pace must be at least 1",
            ));
        }
        if self.beat_ticks == 0 {
            return Err(ParamError::new(
                "beat_ticks",
                "a beat must be at least 1 tick",
            ));
        }
        if self.min_word_ticks == 0 {
            return Err(ParamError::new(
                "min_word_ticks",
                "a word must be at least 1 tick long",
            ));
        }
        if self.max_utterance_ticks < u32::from(self.min_word_ticks) {
            return Err(ParamError::new(
                "max_utterance_ticks",
                "a line may not be shorter than one word",
            ));
        }
        if self.airtime_on_exhaust != AIRTIME_ON_EXHAUST_REFLEX
            && self.airtime_on_exhaust != AIRTIME_ON_EXHAUST_FREEZE
        {
            return Err(ParamError::new(
                "airtime_on_exhaust",
                "the policy must be \"reflex\" or \"freeze\"",
            ));
        }
        Ok(())
    }
}

/// What one listener got of one line.
///
/// The record is bookkeeping: it is *not* hashed, so collecting it does not
/// change the round's state hash. What the line changed in the world — the
/// knowledge it gave the squads — is hashed, because that is game state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListenerRecord {
    /// The listener, by agent id.
    pub listener: u16,
    /// How clear the line reached them, in Q16.
    pub clarity_q16: u32,
    /// How many of the spoken words arrived.
    pub words_heard: u16,
    /// Whether the listener understood the gesture.
    pub act_known: bool,
    /// Whether the listener understood the point of the line.
    pub intent_known: bool,
}

/// One line, as the round heard it.
///
/// The record is the bookkeeping of the talk: what was said, to whom, and
/// how much of it arrived. Phase 5's metrics read these records (D49: no
/// model, no guess), and the round's state hash does **not** cover them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TalkRecord {
    /// The canonical id of the line: the hash of the score it was degraded
    /// from.
    pub id: u64,
    /// The agent who spoke the line.
    pub speaker: u16,
    /// The tick the line started on.
    pub at_tick: u64,
    /// The tick the line ended on.
    pub ended_tick: u64,
    /// The channel the line was spoken on.
    pub channel: Channel,
    /// The gesture of the line.
    pub act: SpeechAct,
    /// The point of the line.
    pub intent: Intent,
    /// The affect bucket of the line.
    pub affect: Affect,
    /// What the line was about.
    pub subject: Subject,
    /// How many words were spoken.
    pub words: u16,
    /// How long the line took, in ticks (the trailing rests do not count).
    pub duration_ticks: u32,
    /// What each listener got of it.
    pub listeners: Vec<ListenerRecord>,
}

impl StateHash for TalkParams {
    fn hash_into(&self, h: &mut Fnv64) {
        h.u32(self.range_shout_units);
        h.u32(self.range_voice_units);
        h.u32(self.range_whisper_units);
        h.write(&[u8::from(self.radio_is_squad)]);
        h.u32(self.wall_loss_q16);
        h.u32(self.min_clarity_q16);
        h.u32(self.clarity_pitch_q16);
        h.u32(self.clarity_rate_q16);
        h.u32(self.act_clarity_q16);
        h.u32(self.intent_clarity_q16);
        h.u32(self.word_drop_q16);
        h.write(&[self.radio_pitch_steps as u8]);
        h.u32(self.radio_drop_q16);
        h.u16(self.base_wpm);
        h.u16(self.beat_ticks);
        h.u16(self.min_word_ticks);
        h.u32(self.max_utterance_ticks);
        h.u16(self.airtime_s);
        write_str(h, &self.airtime_on_exhaust);
        h.u32(self.order_uptake_window_ticks);
        h.write(&[u8::from(self.enemy_overhears_voice)]);
    }
}

/// Absorb a string as a u32-LE length prefix followed by its bytes.
fn write_str(h: &mut Fnv64, s: &str) {
    h.u32(s.len() as u32);
    h.write(s.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_plan_s_defaults() {
        let p = TalkParams::default();
        assert_eq!(
            (
                p.range_shout_units,
                p.range_voice_units,
                p.range_whisper_units,
                p.radio_is_squad
            ),
            (61_440, 25_600, 5_120, true)
        );
        assert_eq!(
            (
                p.wall_loss_q16,
                p.min_clarity_q16,
                p.clarity_pitch_q16,
                p.clarity_rate_q16
            ),
            (32_768, 1_536, 49_152, 16_384)
        );
        assert_eq!(
            (
                p.act_clarity_q16,
                p.intent_clarity_q16,
                p.word_drop_q16,
                p.radio_pitch_steps,
                p.radio_drop_q16
            ),
            (32_768, 16_384, 3_276, 1, 4_096)
        );
        assert_eq!(
            (
                p.base_wpm,
                p.beat_ticks,
                p.min_word_ticks,
                p.max_utterance_ticks
            ),
            (200, 5, 2, 300)
        );
        assert_eq!(
            (
                p.airtime_s,
                p.airtime_on_exhaust.as_str(),
                p.order_uptake_window_ticks,
                p.enemy_overhears_voice
            ),
            (20, "reflex", 150, true)
        );
    }

    #[test]
    fn the_defaults_validate() {
        assert!(TalkParams::default().validate().is_ok());
    }

    #[test]
    #[allow(clippy::type_complexity)] // the case table is copied from the plan
    fn validate_names_the_offending_field() {
        let cases: [(&str, fn(&mut TalkParams)); 6] = [
            ("range_voice_units", |p| p.range_voice_units = 0),
            ("wall_loss_q16", |p| p.wall_loss_q16 = Q16_ONE + 1),
            ("radio_pitch_steps", |p| p.radio_pitch_steps = 30),
            ("beat_ticks", |p| p.beat_ticks = 0),
            ("airtime_on_exhaust", |p| {
                p.airtime_on_exhaust = "shout".to_owned()
            }),
            ("max_utterance_ticks", |p| p.max_utterance_ticks = 1),
        ];
        for (field, f) in cases {
            let mut p = TalkParams::default();
            f(&mut p);
            assert_eq!(p.validate().unwrap_err().field, field);
        }
    }

    #[test]
    fn the_hash_is_stable_and_sensitive() {
        let p = TalkParams::default();
        assert_eq!(p.state_hash(), TalkParams::default().state_hash());
        let mut q = p.clone();
        q.range_voice_units = 51_200;
        assert_ne!(p.state_hash(), q.state_hash());
        let mut q = p.clone();
        q.enemy_overhears_voice = false;
        assert_ne!(p.state_hash(), q.state_hash());
    }
}
