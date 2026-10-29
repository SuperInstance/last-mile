//! The human's voice: the microphone, the words and the lanes.
//!
//! The microphone path is **push-to-talk → capture → ASR → prosody → a
//! score → a decision** (phase 4, task 6). Three rules shape it:
//!
//! - **the score is the truth.** The words come from the ASR with the
//!   timings it heard, and the lanes come from the measured tracks when
//!   there are any. The score is written into the log and replays exactly;
//!   nothing is ever inferred from the audio afterwards;
//! - **no prosody is absent, not zero.** A capture the prosody service
//!   could not measure still has its words and their timings: the score is
//!   marked [`Prosody::Absent`], which is *not* the same as a line with no
//!   lanes (the communicator's rule 4);
//! - **a dead microphone is refused.** A capture that is silent all the way
//!   through would otherwise become a silent, misread command, so it is
//!   refused by name and nothing is submitted.
//!
//! The conversions are fixed point: the score is hashed and replays, so the
//! same capture always gives the same score, on every machine.

use lm_core::talk::TalkParams;
use lm_core::utterance::{
    Addressee, Affect, Channel, Intent, Prosody, SpeechAct, Subject, Utterance, Word,
    MAX_PITCH_STEPS, MIN_PITCH_STEPS, Q16_ONE,
};

use crate::{Asr, MeasuredProsody, ProsodyPort, Transcript, TranscriptWord, VoiceError};

/// The rate the round ticks at, when the caller does not say.
///
/// The round's own rate is `Params::tick_hz`, and this is its default. A
/// caller that knows the round's rate uses [`build_human_utterance_at`].
pub const DEFAULT_TICK_HZ: u32 = 30;

/// The loudest share of silent samples a live microphone may have.
///
/// 0.99 in Q16: a capture that is 99 % zeros is already a dead microphone
/// wearing a hat, and one that is 100 % zeros is the perfect detector for
/// a dead input (fleet/08).
pub const DEFAULT_ZERO_FRACTION_MAX_Q16: u32 = 64_846;

/// The rate the microphone samples at, in hertz.
pub const DEFAULT_ASR_SAMPLE_RATE: u32 = 16_000;

/// How long one frame of a measured track is, in milliseconds.
///
/// The measured tracks are frames -- the pitch track says which frame a
/// pitch belongs to, and the energy track has one entry per frame -- and a
/// frame is ten milliseconds, which is the hop a pitch tracker works at.
pub const PROSODY_FRAME_MS: u32 = 10;

/// How healthy a capture is.
///
/// The health of a capture is one number: the share of the samples that are
/// exactly zero. A microphone that is unplugged, muted or pointed at
/// nothing gives nothing but zeros, and that is a perfect detector for a
/// dead input (fleet/08).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MicHealth {
    /// How many samples the capture holds.
    pub samples: usize,
    /// How many of them are exactly zero.
    pub zeros: usize,
    /// The share of zeros, in Q16.
    pub zero_fraction_q16: u32,
    /// Whether the capture is silent all the way through.
    ///
    /// This is the perfect detector: every sample is exactly zero. An empty
    /// capture is dead too -- there was nothing there to hear.
    pub dead: bool,
}

impl MicHealth {
    /// Whether the capture is healthy enough to be read.
    ///
    /// The microphone is alive when it is not silent all the way through
    /// and its share of zeros is under the limit.
    pub fn ok(&self, max_zero_fraction_q16: u32) -> bool {
        !self.dead && self.zero_fraction_q16 < max_zero_fraction_q16
    }
}

/// The health of a capture: the share of the samples that are exactly zero.
///
/// An empty capture is silent all the way through, and says so.
pub fn mic_health(pcm: &[i16]) -> MicHealth {
    let zeros = pcm.iter().filter(|sample| **sample == 0).count();
    let samples = pcm.len();
    let zero_fraction_q16 = if samples == 0 {
        Q16_ONE
    } else {
        u32::try_from(
            u64::from(u32::try_from(zeros).unwrap_or(u32::MAX)) * u64::from(Q16_ONE)
                / samples as u64,
        )
        .unwrap_or(Q16_ONE)
    };
    MicHealth {
        samples,
        zeros,
        zero_fraction_q16,
        dead: zero_fraction_q16 >= Q16_ONE,
    }
}

/// The health of a capture, and a refusal when the microphone is dead.
///
/// The capture is refused when its share of zeros reaches the limit: a dead
/// microphone would otherwise become a silent, misread command, and the
/// round must not act on a word nobody said.
pub fn check_mic(pcm: &[i16], max_zero_fraction_q16: u32) -> Result<MicHealth, VoiceError> {
    let health = mic_health(pcm);
    if health.zero_fraction_q16 >= max_zero_fraction_q16 {
        return Err(VoiceError::MicDead);
    }
    Ok(health)
}

/// The score of the line the human spoke.
///
/// The words come from the ASR with the timings it heard, scaled from
/// milliseconds to ticks; the lanes come from the measured tracks when
/// there are any. With no prosody the score is marked [`Prosody::Absent`]
/// -- **not** zero lanes -- and the words are still timed.
///
/// The tags are the ones a spoken command has: a command, to the squad,
/// about nothing in particular. The *choice* of what to say belongs to the
/// decision model (D37); the human's words are the wording.
pub fn build_human_utterance(
    transcript: &Transcript,
    prosody: Option<&MeasuredProsody>,
    speaker: u16,
    p: &TalkParams,
) -> Utterance {
    build_human_utterance_at(transcript, prosody, speaker, p, DEFAULT_TICK_HZ)
}

/// The score of the line the human spoke, at the round's own tick rate.
///
/// The same as [`build_human_utterance`], with the tick rate the round runs
/// at: the timings are in milliseconds and the score is in ticks, so the
/// rate is what turns one into the other.
pub fn build_human_utterance_at(
    transcript: &Transcript,
    prosody: Option<&MeasuredProsody>,
    speaker: u16,
    p: &TalkParams,
    tick_hz: u32,
) -> Utterance {
    let words = words_from(transcript, prosody, p, tick_hz);
    Utterance {
        speaker,
        channel: Channel::Voice,
        addressee: Addressee::Squad,
        act: prosody.and_then(|m| m.act).unwrap_or(SpeechAct::Command),
        intent: Intent::Order,
        affect: prosody.map_or(Affect::Calm, |m| m.affect),
        subject: Subject::None,
        prosody: if prosody.is_some() {
            Prosody::Measured
        } else {
            Prosody::Absent
        },
        words,
    }
}

/// The words of the line, with their timings and their lanes.
///
/// The timings are relative to the first word: the score starts where the
/// line starts, and the sim places the line on the tick it was spoken on.
fn words_from(
    transcript: &Transcript,
    prosody: Option<&MeasuredProsody>,
    p: &TalkParams,
    tick_hz: u32,
) -> Vec<Word> {
    let Some(first) = transcript.words.first() else {
        return Vec::new();
    };
    let origin_ms = first.t_on_ms;
    let baseline_q16 = prosody.map(pitch_baseline).unwrap_or(0);
    let min_word_ticks = u64::from(p.min_word_ticks);

    let mut words = Vec::with_capacity(transcript.words.len());
    for (index, said) in transcript.words.iter().enumerate() {
        let t_on = ms_to_ticks(said.t_on_ms.saturating_sub(origin_ms), tick_hz);
        let t_off = ms_to_ticks(said.t_off_ms.saturating_sub(origin_ms), tick_hz)
            .max(t_on.saturating_add(min_word_ticks));
        let mut word = Word::new(said.text.clone(), t_on, t_off);
        if let Some(measured) = prosody {
            let next = transcript.words.get(index + 1);
            word.pitch_steps = pitch_steps_at(measured, said, baseline_q16);
            word.energy_q16 = energy_at(measured, said);
            word.rate_q16 = rate_of(measured, p);
            word.pause_after_ticks = pause_after(measured, said, next, tick_hz);
        }
        words.push(word);
    }
    words
}

/// The tick a number of milliseconds lands on.
///
/// The scale is the round's tick rate, and the rounding is to the nearest
/// tick: the same capture always gives the same score.
fn ms_to_ticks(ms: u32, tick_hz: u32) -> u64 {
    if tick_hz == 0 {
        return 0;
    }
    (u64::from(ms) * u64::from(tick_hz) + 500) / 1000
}

/// The speaker's baseline pitch, in Q16 hertz.
///
/// The baseline is the mean of the measured pitch track: the pitch the
/// speaker's own voice sits at, which is what the score's pitch lane is
/// measured from.
fn pitch_baseline(measured: &MeasuredProsody) -> u32 {
    if measured.pitch_hz.is_empty() {
        return 0;
    }
    let sum: u64 = measured.pitch_hz.iter().map(|(_, hz)| u64::from(*hz)).sum();
    u32::try_from(sum / measured.pitch_hz.len() as u64).unwrap_or(0)
}

/// The pitch of a word, in semitones from the speaker's baseline.
///
/// The frame nearest the word's onset carries the pitch; the lane is the
/// nearest semitone to the ratio of that pitch and the baseline.
fn pitch_steps_at(measured: &MeasuredProsody, said: &TranscriptWord, baseline_q16: u32) -> i8 {
    if baseline_q16 == 0 || measured.pitch_hz.is_empty() {
        return 0;
    }
    let onset_frame = said.t_on_ms / PROSODY_FRAME_MS;
    let mut best: Option<(u32, u32)> = None;
    for (frame, hz) in &measured.pitch_hz {
        let distance = frame.abs_diff(onset_frame);
        if best.is_none_or(|(nearest, _)| distance < nearest) {
            best = Some((distance, *hz));
        }
    }
    let Some((_, hz)) = best else {
        return 0;
    };
    semitones(hz, baseline_q16)
}

/// The semitones of a pitch above the baseline, to the nearest one.
///
/// The ratio of the pitch and the baseline is a Q16 number, and the
/// logarithm of a Q16 number carries the Q16 offset with it: it is taken
/// off before the ratio becomes a number of semitones.
fn semitones(pitch_q16: u32, baseline_q16: u32) -> i8 {
    if pitch_q16 == 0 || baseline_q16 == 0 {
        return 0;
    }
    let ratio = (u64::from(pitch_q16) << 16) / u64::from(baseline_q16);
    let log_q16 = i64::from(log2_q16(ratio)) - 16 * 65_536;
    let semitones_q16 = 12 * log_q16;
    // To the nearest semitone, either way round.
    let steps = if semitones_q16 >= 0 {
        ((semitones_q16 + 32_768) >> 16) as i32
    } else {
        -(((-semitones_q16) + 32_768) >> 16) as i32
    };
    steps.clamp(i32::from(MIN_PITCH_STEPS), i32::from(MAX_PITCH_STEPS)) as i8
}

/// The base-2 logarithm of a Q16 number, in Q16.
///
/// The logarithm is built from the exponent of the number and a table of
/// the mantissa: no float, no library, and the same answer on every
/// machine -- which matters, because the lane it feeds is hashed.
fn log2_q16(x: u64) -> i32 {
    if x == 0 {
        return i32::MIN;
    }
    let exponent = 63 - x.leading_zeros() as i32;
    // The mantissa in Q16, in [1, 2).
    let mantissa = if exponent >= 16 {
        x >> (exponent - 16)
    } else {
        x << (16 - exponent)
    };
    let fraction = mantissa - Q16_ONE as u64;
    let index = usize::try_from(fraction >> 12).unwrap_or(0);
    let step = fraction & 0xFFF;
    let base = LOG2_MANTISSA_Q16[index];
    let next = LOG2_MANTISSA_Q16[index + 1];
    let interpolated = (step * u64::from((next - base) as u32)) >> 12;
    exponent * 65_536 + base + i32::try_from(interpolated).unwrap_or(0)
}

/// The base-2 logarithm of the mantissa steps, in Q16.
///
/// The mantissa of a Q16 number is in [1, 2), split into sixteen steps; the
/// table holds the logarithm of every step, and the value between two steps
/// is interpolated.
const LOG2_MANTISSA_Q16: [i32; 17] = [
    0, 5_732, 11_137, 16_251, 21_098, 25_708, 30_105, 34_308, 38_336, 42_186, 45_904, 49_465,
    52_916, 56_224, 59_426, 62_531, 65_536,
];

/// The loudness of a word, in Q16.
///
/// The loudness is the mean of the frames the word covers. A word the
/// tracks say nothing about keeps the neutral loudness.
fn energy_at(measured: &MeasuredProsody, said: &TranscriptWord) -> u32 {
    if measured.energy.is_empty() {
        return Q16_ONE;
    }
    let start_frame = said.t_on_ms / PROSODY_FRAME_MS;
    let end_frame = said.t_off_ms.saturating_add(PROSODY_FRAME_MS - 1) / PROSODY_FRAME_MS;
    let end_frame = end_frame.max(start_frame + 1);
    let mut sum = 0u64;
    let mut count = 0u64;
    for (index, energy) in measured.energy.iter().enumerate() {
        let frame = u32::try_from(index).unwrap_or(u32::MAX);
        if frame >= start_frame && frame < end_frame {
            sum += u64::from(*energy);
            count += 1;
        }
    }
    if count == 0 {
        return Q16_ONE;
    }
    u32::try_from(sum / count).unwrap_or(Q16_ONE).min(Q16_ONE)
}

/// The pace of the line, in Q16.
///
/// The tempo is a property of the line, not of one word: it is the
/// measured tempo against the round's base pace.
fn rate_of(measured: &MeasuredProsody, p: &TalkParams) -> u32 {
    if measured.tempo_bpm == 0 || p.base_wpm == 0 {
        return Q16_ONE;
    }
    let rate = u64::from(measured.tempo_bpm) * u64::from(Q16_ONE) / u64::from(p.base_wpm);
    rate.min(u64::from(Q16_ONE) * 2) as u32
}

/// The rest after a word, in ticks.
///
/// The rest is one of the measured pauses: the one that starts when the
/// word ends and is over before the next word begins. The rest after the
/// *last* word is silence, and silence is not part of the line.
fn pause_after(
    measured: &MeasuredProsody,
    said: &TranscriptWord,
    next: Option<&TranscriptWord>,
    tick_hz: u32,
) -> u16 {
    let Some(next) = next else {
        return 0;
    };
    for (start, end) in &measured.pauses {
        if *start >= said.t_off_ms && *end <= next.t_on_ms {
            let ticks = ms_to_ticks(end.saturating_sub(*start), tick_hz);
            return u16::try_from(ticks).unwrap_or(u16::MAX);
        }
    }
    0
}

/// The capture path: the samples in, the words and the measurement out.
///
/// The capture holds the ASR that writes the words down and, optionally,
/// the prosody service that measures the voice. The service is **optional**
/// on purpose: when it is absent, or when it answers with an error, the
/// capture still gives its words, and the score is built without prosody.
/// The round never waits for it.
pub struct Capture {
    /// The backend that writes the words down.
    asr: Box<dyn Asr>,
    /// The service that measures the voice, when there is one.
    prosody: Option<Box<dyn ProsodyPort>>,
    /// The rate the samples were taken at.
    sample_rate: u32,
}

impl Capture {
    /// A capture over an ASR, with no prosody service.
    pub fn new(asr: Box<dyn Asr>) -> Capture {
        Capture {
            asr,
            prosody: None,
            sample_rate: DEFAULT_ASR_SAMPLE_RATE,
        }
    }

    /// The capture with a prosody service behind it.
    pub fn with_prosody(mut self, port: Box<dyn ProsodyPort>) -> Capture {
        self.prosody = Some(port);
        self
    }

    /// The rate the samples were taken at, in hertz.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// The capture at another sample rate.
    pub fn with_sample_rate(mut self, sample_rate: u32) -> Capture {
        self.sample_rate = sample_rate;
        self
    }

    /// Read the capture: the words, and the measurement if the service
    /// answered.
    ///
    /// A prosody service that is absent or that fails costs nothing: the
    /// words are still there, and the score is built without prosody. Only
    /// the ASR can fail the capture.
    pub fn read(&self, pcm: &[i16]) -> Result<(Transcript, Option<MeasuredProsody>), VoiceError> {
        let transcript = self.asr.transcribe(pcm, self.sample_rate)?;
        let measured = match &self.prosody {
            Some(port) => port.measure(pcm, self.sample_rate).ok(),
            None => None,
        };
        Ok((transcript, measured))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A transcript of the words, one every 300 ms.
    fn transcript(text: &str) -> Transcript {
        let mut words = Vec::new();
        let mut at = 0u32;
        for word in text.split_whitespace() {
            words.push(TranscriptWord {
                text: word.to_owned(),
                t_on_ms: at,
                t_off_ms: at + 300,
            });
            at += 300;
        }
        let text = words
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<&str>>()
            .join(" ");
        Transcript { words, text }
    }

    /// A transcript of the words, with the timings it was given.
    fn transcript_at(words: &[(&str, u32, u32)]) -> Transcript {
        let words: Vec<TranscriptWord> = words
            .iter()
            .map(|(text, on, off)| TranscriptWord {
                text: (*text).to_owned(),
                t_on_ms: *on,
                t_off_ms: *off,
            })
            .collect();
        let text = words
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<&str>>()
            .join(" ");
        Transcript { words, text }
    }

    /// A measurement of a voice that sat on its own baseline.
    fn measured() -> MeasuredProsody {
        MeasuredProsody {
            // 220 Hz in Q16, the whole way through.
            pitch_hz: vec![(0, 14_417_920), (10, 14_417_920)],
            energy: vec![Q16_ONE; 20],
            tempo_bpm: 150,
            pauses: vec![(300, 450)],
            act: Some(SpeechAct::Command),
            affect: Affect::Urgent,
        }
    }

    #[test]
    fn no_prosody_is_absent_not_zero() {
        let p = TalkParams::default();
        let said = transcript("hold the line");

        let u = build_human_utterance(&said, None, 3, &p);

        // The score says the prosody was not measured: absent is not zero.
        assert_eq!(u.prosody, Prosody::Absent);
        assert_ne!(u.prosody, Prosody::Measured);

        // The words are still timed: the line has a length.
        assert_eq!(u.words.len(), 3);
        assert_eq!(u.words[0].t_on, 0);
        assert!(u.words[0].t_off > u.words[0].t_on, "the word is timed");
        assert!(
            u.words[2].t_off > u.words[1].t_off,
            "the words follow each other"
        );

        // And the lanes are the neutral ones, not zeros: absent prosody is
        // not a silent, flat line.
        assert_eq!(u.words[0].energy_q16, Q16_ONE);
        assert_eq!(u.words[0].rate_q16, Q16_ONE);
        assert_eq!(u.words[0].pitch_steps, 0);

        // The tags are the ones a spoken command has.
        assert_eq!(u.speaker, 3);
        assert_eq!(u.channel, Channel::Voice);
        assert_eq!(u.addressee, Addressee::Squad);
        assert_eq!(u.act, SpeechAct::Command);
        assert_eq!(u.intent, Intent::Order);
        assert_eq!(u.affect, Affect::Calm);
    }

    #[test]
    fn a_measured_prosody_fills_the_lanes() {
        let p = TalkParams::default();
        let said = transcript_at(&[("hold", 0, 300), ("the", 450, 750), ("line", 900, 1200)]);

        let u = build_human_utterance(&said, Some(&measured()), 0, &p);

        assert_eq!(u.prosody, Prosody::Measured);
        // The tone the service heard is the tone of the score.
        assert_eq!(u.affect, Affect::Urgent);

        // The pitch lane is the measured pitch against the speaker's own
        // baseline: the measurement sits on its baseline, so no semitones.
        assert_eq!(u.words[0].pitch_steps, 0);

        // The tempo is the pace, against the round's base pace: 150 wpm
        // against 200 is three quarters.
        assert_eq!(u.words[0].rate_q16, 49_152);

        // The loudness is the mean of the frames the word covers.
        assert_eq!(u.words[0].energy_q16, Q16_ONE);

        // The pause after the first word is the measured one: 150 ms is
        // four and a half ticks, rounded to five.
        assert_eq!(u.words[0].pause_after_ticks, 5);
        // And the rest after the last word is silence, not part of the line.
        assert_eq!(u.words[2].pause_after_ticks, 0);
    }

    #[test]
    fn a_pitch_above_the_baseline_is_semitones_above_it() {
        let p = TalkParams::default();
        // Two frames: 330 Hz and 110 Hz, so the speaker's baseline -- the
        // mean of the track -- is 220 Hz.
        let mut m = measured();
        m.pitch_hz = vec![(0, 21_626_880), (10, 7_208_960)];

        // A word that starts on the high frame is a fifth above it.
        let high = transcript_at(&[("hold", 0, 300)]);
        let u = build_human_utterance(&high, Some(&m), 0, &p);
        assert_eq!(u.words[0].pitch_steps, 7, "a fifth is seven semitones");

        // And a word that starts on the low frame is an octave below it.
        let low = transcript_at(&[("hold", 100, 400)]);
        let u = build_human_utterance(&low, Some(&m), 0, &p);
        assert_eq!(u.words[0].pitch_steps, -12, "an octave is twelve semitones");
    }

    #[test]
    fn the_words_are_timed_in_the_rounds_ticks() {
        let p = TalkParams::default();
        let said = transcript("hold the line");

        // At 30 Hz a 300 ms word is nine ticks.
        let u = build_human_utterance_at(&said, None, 0, &p, 30);
        assert_eq!(u.words[0].t_on, 0);
        assert_eq!(u.words[0].t_off, 9);
        assert_eq!(u.words[1].t_on, 9);
        assert_eq!(u.words[1].t_off, 18);

        // At 60 Hz it is eighteen: the same capture, the round's own rate.
        let u = build_human_utterance_at(&said, None, 0, &p, 60);
        assert_eq!(u.words[0].t_off, 18);
    }

    #[test]
    fn the_zero_fraction_is_the_share_of_silent_samples() {
        let health = mic_health(&[0, 0, 1, 2, 0]);
        assert_eq!(health.samples, 5);
        assert_eq!(health.zeros, 3);
        assert_eq!(health.zero_fraction_q16, 39_321);
        assert!(!health.dead, "a capture with a voice in it is alive");

        // Every sample zero is the perfect detector for a dead input.
        let dead = mic_health(&[0, 0, 0]);
        assert!(dead.dead);
        assert_eq!(dead.zero_fraction_q16, Q16_ONE);

        // And an empty capture is dead: there was nothing there.
        assert!(mic_health(&[]).dead);
    }

    #[test]
    fn the_mic_check_refuses_a_capture_of_zeros() {
        assert_eq!(
            check_mic(&[0; 16], DEFAULT_ZERO_FRACTION_MAX_Q16),
            Err(VoiceError::MicDead)
        );
        // A capture with a voice in it passes.
        let live = [1i16, 2, 3, 4];
        assert!(check_mic(&live, DEFAULT_ZERO_FRACTION_MAX_Q16).is_ok());
    }

    #[test]
    fn a_capture_without_a_prosody_service_still_gives_its_words() {
        let capture = Capture::new(Box::new(crate::StubAsr::scripted("hold")));

        let (transcript, measured) = capture.read(&[1, 2, 3]).expect("the capture reads");

        assert_eq!(transcript.text, "hold");
        assert!(measured.is_none(), "there was no service to measure with");
    }
}
