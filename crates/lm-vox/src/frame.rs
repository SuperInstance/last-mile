//! The protocol types: the four layers and their units.
//!
//! Every field carries its unit and its range in the doc comment, because
//! the protocol is read by machines that never see this crate's code — the
//! unit *is* the wire contract (design §4). L0 quantizes everything to
//! integers on purpose: once a frame is built, layers 1 and 2 and the
//! encoder are exact integer arithmetic, identical on every machine.

use crate::math::{exp2, log2};

/// The reference pitch L0's cents are measured against when no profile has
/// been fitted yet: 55 Hz, A1 — a whole voice (85-255 Hz) sits at
/// 750-2600 cents above it, far inside i16.
pub const F0_REF_HZ: f64 = 55.0;

/// The lowest level the L0 log scale can name: -96 dBFS. Digital silence is
/// below it and clamps to 0.
pub const RMS_DB_MIN: f64 = -96.0;

/// The bottom of the formant log scale: 150 Hz (a dark /u/ has F1 near
/// 250 Hz, and headroom below it costs little).
pub const FORMANT_MIN_HZ: f64 = 150.0;

/// The top of the formant log scale: 5000 Hz (F3 of a bright vowel or
/// consonant locus stays under 4 kHz in speech).
pub const FORMANT_MAX_HZ: f64 = 5000.0;

/// The bit flags an L0 frame carries. The three low bits are defined; the
/// rest are reserved and must be sent as zero until they are given a
/// meaning.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameFlags(pub u8);

impl FrameFlags {
    /// The frame shows periodic excitation (a reliable YIN period).
    pub const VOICED: FrameFlags = FrameFlags(1);
    /// The frame carries a detected onset (an attack's first frame).
    pub const ONSET: FrameFlags = FrameFlags(2);
    /// The frame is silence: a rest, not a note.
    pub const REST: FrameFlags = FrameFlags(4);

    /// Whether every defined bit of `self` is also set in `other`.
    pub fn includes(self, other: FrameFlags) -> bool {
        self.0 & other.0 == other.0
    }

    /// Set or clear one flag group.
    pub fn set(&mut self, flag: FrameFlags, on: bool) {
        if on {
            self.0 |= flag.0;
        } else {
            self.0 &= !flag.0;
        }
    }

    /// Whether the given flag group is fully set.
    pub fn has(self, flag: FrameFlags) -> bool {
        self.includes(flag)
    }
}

/// L0 Frame: the performance one 10 ms slice at a time (design §4).
///
/// The in-memory frame holds exactly what the 9-byte wire frame of §5
/// holds, no more: the quantization is part of the protocol, so analysis,
/// storage, rendering and round trips all agree bit for bit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    /// The fundamental, in cents above the profile's reference pitch
    /// (55 Hz until a profile is fitted). **0 means unvoiced** — there is
    /// no pitch. Typical speech: 750-2600 (85-255 Hz); the i16 range covers
    /// 55 Hz to 28 kHz, which no voice needs.
    pub f0_cents: i16,
    /// The level, on a log scale: 0 is -96 dBFS (or digital silence), 255
    /// is 0 dBFS. One unit is 96/255 dB, about 0.376 dB.
    pub rms: u8,
    /// The first formant, on a log scale from 150 Hz (unit 0) to 5000 Hz
    /// (unit 255); one unit is about 1.4%. 0 means "not measured this
    /// frame" (unvoiced or no peak found).
    pub f1: u8,
    /// The second formant, same scale as `f1`.
    pub f2: u8,
    /// The third formant, same scale as `f1`.
    pub f3: u8,
    /// Spectral tilt, in dB per octave: positive is brighter than flat.
    /// Speech sits between about -12 (muffled) and +12 (brassy); the i8
    /// range is headroom, not a claim about nature.
    pub tilt: i8,
    /// The noise share of the excitation: 0 is a pure tone, 255 is white
    /// noise. A voiced vowel sits low, a fricative high.
    pub noise: u8,
    /// The flag bits (voiced / onset / rest).
    pub flags: FrameFlags,
}

impl Default for Frame {
    /// A rest: silence, no pitch, no formants, no tilt, no noise.
    fn default() -> Frame {
        Frame {
            f0_cents: 0,
            rms: 0,
            f1: 0,
            f2: 0,
            f3: 0,
            tilt: 0,
            noise: 0,
            flags: FrameFlags::REST,
        }
    }
}

impl Frame {
    /// The frame's level in dBFS, on the log scale's own terms.
    pub fn rms_db(self) -> f64 {
        f64::from(self.rms) * (96.0 / 255.0) + RMS_DB_MIN
    }

    /// Quantize a dBFS level to the log scale, clamping at both ends.
    pub fn rms_from_db(db: f64) -> u8 {
        let unit = (db - RMS_DB_MIN) / (96.0 / 255.0);
        let unit = if unit < 0.0 { 0.0 } else { unit };
        let unit = if unit > 255.0 { 255.0 } else { unit };
        // Round half up, without libm's `round`.
        (unit + 0.5) as u8
    }

    /// A formant field's frequency in Hz, or `None` when it is 0
    /// (not measured). Field 1 is the scale's bottom (150 Hz), so the
    /// whole range 150-5000 Hz maps onto 1-255.
    pub fn formant_hz(field: u8) -> Option<f64> {
        if field == 0 {
            return None;
        }
        let octaves = ((f64::from(field) - 1.0) / 254.0) * log2(FORMANT_MAX_HZ / FORMANT_MIN_HZ);
        Some(FORMANT_MIN_HZ * exp2(octaves))
    }

    /// Quantize a frequency in Hz onto the formant log scale, clamping at
    /// both ends. `hz <= 0` maps to 0 (the "not measured" marker).
    pub fn formant_from_hz(hz: f64) -> u8 {
        if hz <= 0.0 {
            return 0;
        }
        let unit = 1.0 + 254.0 * log2(hz / FORMANT_MIN_HZ) / log2(FORMANT_MAX_HZ / FORMANT_MIN_HZ);
        let unit = if unit < 1.0 { 1.0 } else { unit };
        let unit = if unit > 255.0 { 255.0 } else { unit };
        (unit + 0.5) as u8
    }

    /// Hz to cents above the reference pitch.
    pub fn hz_to_cents(hz: f64, reference_hz: f64) -> f64 {
        1200.0 * log2(hz / reference_hz)
    }

    /// Cents above the reference pitch back to Hz.
    pub fn cents_to_hz(cents: f64, reference_hz: f64) -> f64 {
        reference_hz * exp2(cents / 1200.0)
    }

    /// The frame's F0 in Hz, or `None` when unvoiced.
    pub fn f0_hz(self, reference_hz: f64) -> Option<f64> {
        if self.f0_cents == 0 {
            None
        } else {
            Some(Frame::cents_to_hz(f64::from(self.f0_cents), reference_hz))
        }
    }
}

/// L1 Gesture: one syllable's worth of performance (design §4) — the
/// note-on/note-off with per-note expression that a renderer or a gesture
/// matcher reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Gesture {
    /// When the gesture starts, in ms from the start of the utterance.
    /// Range: 0 to the utterance's length.
    pub onset_ms: u32,
    /// How long the gesture lasts, in MS; a syllable is 80-400, a pause can
    /// be longer.
    pub duration_ms: u32,
    /// The pitch the gesture settles toward: the median voiced F0 of its
    /// middle half, in cents above the profile reference. 0 when the
    /// gesture is unvoiced throughout (a fricative, a pause).
    pub pitch_target_cents: i16,
    /// The glide: the mean of the last quarter minus the mean of the first
    /// quarter, in cents. Positive rises, negative falls; a declarative
    /// syllable falls 100-400.
    pub glide_cents: i16,
    /// The velocity: the gesture's peak level against the speaker's median
    /// level, in dB. A shout is +12 to +24; a mumble -12 to -20. i8::MIN
    /// marks a silent gesture (a pause).
    pub velocity_db: i8,
    /// The articulation's attack: onset to peak level, in MS. A plosive is
    /// 5-20, a soft vowel onset 40-120.
    pub attack_ms: u16,
    /// The articulation's release: peak level to the gesture's end, in MS.
    pub release_ms: u16,
    /// Whether the gesture opens with a noise burst: the noise channel high
    /// within 30 ms of the onset (a fricative or plosive release).
    pub noise_burst: bool,
}

/// L2 Phrase: the patch summary of a whole utterance (design §4) — what a
/// decision model reads as one short text line, with the numbers kept for
/// the renderer and the learning flow.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Phrase {
    /// The utterance's length in MS, onset of the first gesture to the end
    /// of the last.
    pub duration_ms: u32,
    /// The loudness envelope, as a synth patch would state it.
    pub loud: Adsr,
    /// The brightness envelope (the same ADSR shape measured on the
    /// spectral-tilt channel instead of the level channel).
    pub bright: Adsr,
    /// The patch's brightness word behind it: the median spectral tilt
    /// over the sounding frames, in dB per octave (negative is darker).
    pub bright_tilt_db_oct: i8,
    /// The pitch contour class over the voiced span.
    pub contour: Contour,
    /// The pitch LFO (vibrato): depth in cents, rate in hundredths of a Hz.
    pub vib: Lfo,
    /// The level LFO (tremolo): depth in hundredths of a dB, rate in
    /// hundredths of a Hz.
    pub trem: Lfo,
    /// The tempo: syllables per second, in tenths (61 is 6.1/s).
    pub tempo_tenths: u16,
    /// How many pauses of at least 200 MS the utterance holds.
    pub pauses: u16,
    /// The share of the utterance spent in those pauses, in 255ths.
    pub pause_frac_255: u8,
    /// The peak level against the speaker's median, in dB.
    pub velocity_db: i8,
}

impl Phrase {
    /// The tempo as a plain syllables-per-second reading.
    pub fn tempo_per_s(self) -> f64 {
        f64::from(self.tempo_tenths) / 10.0
    }
}

/// An ADSR envelope as a synth patch states it (design §4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Adsr {
    /// Onset to peak, in MS. Speech: 10-120.
    pub attack_ms: u16,
    /// Peak to the sustain level, in MS. Speech: 20-200.
    pub decay_ms: u16,
    /// The sustain level against the phrase's own peak, in dB: 0 means the
    /// phrase never sustains below its peak; -6 to -12 is a held line.
    pub sustain_db: i8,
    /// The end of the last gesture to the floor, in MS. A hard stop is
    /// under 40; a trailing-off line 150-400.
    pub release_ms: u16,
}

/// The pitch contour class (design §4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Contour {
    /// The line rises overall: it ends at least 200 cents above where it
    /// began, and never falls further than that first.
    #[default]
    Rise,
    /// The line falls overall (a declarative): at least 200 cents down.
    Fall,
    /// Neither: the span stays within 200 cents.
    Flat,
    /// Up then down, each leg at least 200 cents (a yes-no question said
    /// with the peak in the middle).
    RiseFall,
}

/// A low-frequency oscillation found on a channel (vibrato on pitch,
/// tremolo on level).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Lfo {
    /// The modulation depth: vibrato in cents (speech vibrato 20-100,
    /// normal micro-variation under 10), tremolo in hundredths of a dB.
    pub depth: u16,
    /// The modulation rate in hundredths of a Hz (3.00-8.00 Hz read as
    /// 300-800).
    pub rate_centi_hz: u16,
}

/// L3 Intent: the gate answers an utterance resolves to (design §4), plus
/// the words. The gates themselves live with the decision models; this is
/// the container their answers travel in, quantized like a key press.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Intent {
    /// How urgent the delivery sounds.
    pub urgency: Urgency,
    /// Order or question — the shape the answer takes.
    pub kind: UtteranceKind,
    /// The speaker's own confidence, 0-255.
    pub confidence_q8: u8,
    /// Who the line is aimed at.
    pub addressee: Addressee,
    /// The words, from ASR.
    pub words: Vec<String>,
}

/// The urgency gate's answer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Urgency {
    /// Routine.
    #[default]
    Low,
    /// Pressed.
    Mid,
    /// Now.
    High,
}

/// The order/question gate's answer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UtteranceKind {
    /// A statement.
    #[default]
    Statement,
    /// An order: answer with a move.
    Order,
    /// A question: answer with words.
    Question,
}

/// The addressee gate's answer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Addressee {
    /// Nobody the squad can name.
    #[default]
    Unknown,
    /// The player.
    Player,
    /// Another squad member.
    Squad,
    /// Someone outside the squad.
    Other,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The RMS scale's two ends round-trip, and dB below the floor clamps.
    #[test]
    fn rms_scale() {
        assert_eq!(Frame::rms_from_db(0.0), 255);
        assert_eq!(Frame::rms_from_db(-96.0), 0);
        assert_eq!(Frame::rms_from_db(-200.0), 0);
        assert_eq!(Frame::rms_from_db(6.0), 255);
        let quant = f64::from(Frame::rms_from_db(-48.0)) * (96.0 / 255.0) - 96.0;
        assert!((quant - (-48.0)).abs() < 0.4);
        let db = Frame {
            rms: 128,
            ..Frame::default()
        }
        .rms_db();
        assert!((db - (-48.0)).abs() < 0.4);
    }

    /// The formant scale round-trips within one quantization step (1.4%),
    /// which is well inside the 5% the experiments allow.
    #[test]
    fn formant_scale() {
        for &hz in &[150.0, 250.0, 730.0, 1090.0, 2440.0, 3500.0, 5000.0] {
            let field = Frame::formant_from_hz(hz);
            let back = Frame::formant_hz(field).unwrap();
            assert!(
                (back - hz).abs() / hz < 0.015,
                "{hz} Hz came back as {back}"
            );
        }
        assert_eq!(Frame::formant_from_hz(0.0), 0);
        assert_eq!(Frame::formant_hz(0), None);
    }

    /// Cents and Hz agree on the octaves.
    #[test]
    fn cents_scale() {
        assert!((Frame::hz_to_cents(110.0, 55.0) - 1200.0).abs() < 0.01);
        assert!((Frame::cents_to_hz(1200.0, 55.0) - 110.0).abs() < 1e-9);
        let f = Frame {
            f0_cents: 2400,
            ..Frame::default()
        };
        assert!((f.f0_hz(55.0).unwrap() - 220.0).abs() < 1e-6);
        let unvoiced = Frame::default();
        assert_eq!(unvoiced.f0_hz(55.0), None);
    }

    /// Flags set, clear and test as groups.
    #[test]
    fn flags() {
        let mut f = FrameFlags::default();
        assert!(!f.has(FrameFlags::VOICED));
        f.set(FrameFlags::VOICED, true);
        f.set(FrameFlags::ONSET, true);
        assert!(f.has(FrameFlags::VOICED));
        assert!(f.has(FrameFlags::ONSET));
        assert!(!f.has(FrameFlags::REST));
        f.set(FrameFlags::ONSET, false);
        assert!(f.has(FrameFlags::VOICED));
        assert!(!f.has(FrameFlags::ONSET));
    }
}
