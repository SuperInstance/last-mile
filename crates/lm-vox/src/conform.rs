//! The conformer (E10): pull a rendered line's measured performance toward
//! the performance it was meant to be, without resynthesizing a word of it.
//!
//! E3 settled the division of labor: the LPC/formant renderer cannot keep
//! the words (~0% ASR accuracy), so the words must stay in the *rendered*
//! audio and the performance must be conformed on top of it (the render
//! ladder of design §6, the polisher's half). The pipeline is the one the
//! E10 brief draws:
//!
//! ```text
//! intended marks -> target dials -> analyze the render -> per-channel diff
//!                -> tier rule (beyond threshold = re-ask) -> DSP correction
//! ```
//!
//! Every correction is a published method on the real waveform:
//!
//! | Channel | Method | Source |
//! |---|---|---|
//! | F0 and contour | TD-PSOLA: epoch-locked grains, 2-period Hann, synthesis marks at the analysis period over the pitch ratio; window-sum normalized OLA | Moulines & Charpentier 1990; Valbret, Moulines & Tubach 1992 |
//! | Tempo | WSOLA: fixed 30 ms Hann frames, similarity search against the already-written overlap | Verhelst & Roelands 1993 |
//! | Pauses | rest gaps rescaled and silence inserted at the deepest measured valley (editing silence cannot touch a phoneme) | — |
//! | Per-syllable gain/attack/release | a 100 Hz gain track per gesture: valley-to-peak ramps remapped onto the target attack, tails onto the target release, then a smooth per-sample multiplier | — |
//! | Tilt | first-order shelf: one-pole split at 1.2 kHz, low and high legs scaled ±D/2 dB | — |
//! | Noise | white noise from a committed-seed xorshift PRNG, mixed level-relative on sounding frames | — |
//! | Formants | resample by 2^(cents/1200) (everything shifts), then one PSOLA pass that restores pitch and duration (only the tract shape stays shifted) | the source-filter factoring PSOLA is built on |
//!
//! # The tier rule
//!
//! DSP nudges; it does not transform. Each channel's diff is checked
//! against a **measured artifact threshold** ([`Thresholds`]); if any
//! channel wants more than its threshold allows, [`conform`] returns
//! [`ConformOutcome::ReAsk`] naming the channel, and the caller re-asks
//! the generator for a render closer to the intent instead of correcting
//! beyond what the words survive. The thresholds were measured by the E10
//! sweeps (CONFORM.md): each channel swept over magnitudes with ASR
//! listening, the threshold set just inside where word accuracy drops.
//!
//! # Streaming
//!
//! Every stage is the streaming form of its algorithm: PSOLA and WSOLA
//! walk the signal with a bounded lookahead (two pitch periods plus one
//! epoch, and one frame plus the search window — 40-60 ms), the envelope
//! and noise tracks read the 100 Hz dial frames behind them, and the tilt
//! shelf is a one-pole causal filter. [`apply`] runs the stages as one
//! forward pass; the measured per-line wall time is in CONFORM.md.
//!
//! Determinism follows the crate's rule: IEEE-754 `+ - * / sqrt` and the
//! committed tables only (no libm transcendental, no fused multiply-add),
//! and the noise PRNG is integer arithmetic from a committed seed, so the
//! same input and marks give bit-identical output on x86-64 and aarch64.

use crate::analyzer::{analyze_with_profile, Analysis};
use crate::fft::hann;
use crate::frame::{Frame, FrameFlags, Gesture, Phrase};
use crate::math::{exp2, log2, LOG2E, LOG2_10};
use crate::profile::SpeakerProfile;
use crate::wav::Pcm;

/// Frame index to sample position, rounded (frame `i` is centered at
/// `i * 10 ms`).
fn frame_sample(i: usize, rate: u32) -> usize {
    (i * rate as usize + 50) / 100
}

/// The per-channel artifact thresholds of the tier rule, measured by the
/// E10 sweeps (CONFORM.md, "the tier rule's measured thresholds"). A diff
/// beyond any of these returns re-ask rather than correcting.
#[derive(Clone, Copy, Debug)]
pub struct Thresholds {
    /// The largest uniform pitch shift the words survive, in cents. The
    /// E10 sweep measured the words clean at ±100 cents and damaged from
    /// −200 down (the PSOLA's epoch chain skips epochs on a downward
    /// shift). Past +300 the shift is not even measurable: the
    /// analyzer's pitch tracker is anchored to the population reference,
    /// so at +400 the dial reads back −56 cents.
    pub pitch_cents: u32,
    /// The largest one-sided contour excursion (the imposed travel is
    /// centered, so each end moves by half the travel change), in cents.
    pub glide_cents: u32,
    /// The tempo ratio window (target tempo over measured). The E10
    /// sweep measured the words clean from alpha 0.69 (a ratio of 1.45)
    /// to alpha 1.45 (a ratio of 0.69) and damaged outside it, so the
    /// window's edges sit just inside both measurements.
    pub tempo_min: f64,
    /// See [`Thresholds::tempo_min`].
    pub tempo_max: f64,
    /// The pause-length ratio window.
    pub pause_min: f64,
    /// See [`Thresholds::pause_min`].
    pub pause_max: f64,
    /// The longest silence that may be inserted at a valley, in ms.
    pub pause_insert_ms: u32,
    /// The most breaths a line may be given. Two measured word-clean in
    /// the E10 insert sweep (silence edits no phoneme); more would be a
    /// rewrite of the line's rhythm.
    pub breaths: u32,
    /// The largest whole-line gain, in dB. The words do not care (ASR
    /// front ends are level-normalized, and the E10 tilt sweep -- a
    /// comparable full-spectrum reshaping -- cost zero accuracy at +-16
    /// dB); the limit guards the headroom stage from eating the
    /// correction, not the words.
    pub gain_db: u32,
    /// The attack-remap window (target attack over measured).
    pub attack_min: f64,
    /// See [`Thresholds::attack_min`].
    pub attack_max: f64,
    /// The release-scale window.
    pub release_min: f64,
    /// See [`Thresholds::release_min`].
    pub release_max: f64,
    /// The largest spectral-tilt correction, in dB per octave. The sweep
    /// measured the shelf word-clean at every magnitude up to ±16
    /// dB/oct (zero accuracy change), so this limit guards taste, not
    /// the words — and the analyzer's tilt dial moves so little that
    /// more would be pointless.
    pub tilt_db_oct: u32,
    /// The most noise that may be mixed in, in L0 noise-dial units. The
    /// sweep measured the dial moving +5 at 32 units and +13 at 64,
    /// while the probe set's word accuracy dropped 12-25 points across
    /// 16-64 (one fragile word dominating); 32 is the dial's floor, and
    /// everything above it costs words.
    pub noise_add: u8,
    /// The largest formant shift, in cents. The sweep measured the
    /// stage word-clean across ±200 cents with F2 tracking the request
    /// (-11% at -200, +7% at +200); the unit test pins F1-F3 movement
    /// at ±8% with the pitch restored.
    pub formant_cents: u32,
}

impl Default for Thresholds {
    fn default() -> Thresholds {
        Thresholds {
            pitch_cents: 150,
            glide_cents: 500,
            tempo_min: 0.68,
            tempo_max: 1.45,
            pause_min: 0.40,
            pause_max: 2.50,
            pause_insert_ms: 400,
            breaths: 2,
            gain_db: 15,
            attack_min: 0.25,
            attack_max: 3.0,
            release_min: 0.35,
            release_max: 2.5,
            tilt_db_oct: 8,
            noise_add: 32,
            formant_cents: 200,
        }
    }
}

/// A delivery mark: one word of performance intent. The vocabulary is the
/// L2 dial set the analyzer can measure — a mark that names a dial the
/// analyzer cannot read back would be unfalsifiable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// High arousal: louder, faster, sharper, brighter, noisier (class
    /// target: the urgent delivery class).
    Urgent,
    /// Low arousal: slower, longer, softer, with a breath between the
    /// clauses (class target: the calm delivery class).
    Calm,
    /// Staccato: fast attacks and short gaps. No tail sculpting: the E10
    /// ablation measured the release channel flipping a cliff-edge word
    /// ("Regroup" hearing as "grew up") at every scale down to 0.9 —
    /// a final consonant sitting on a gesture tail cannot be cut even
    /// mildly without tipping it, so the channel stays off the mark
    /// vocabulary (a caller may still write `release_scale` into a
    /// [`Target`]; the tier rule checks it).
    Clipped,
    /// The line falls head to tail (a declarative).
    Falling,
    /// The line rises head to tail (a question check).
    Rising,
    /// The line stays level.
    Flat,
    /// Brighter by 5 dB/oct.
    Bright,
    /// Darker by 5 dB/oct.
    Dark,
    /// 6 dB louder than measured.
    Loud,
    /// 6 dB softer than measured.
    Soft,
    /// 1.3x the syllable tempo.
    Fast,
    /// 0.8x the syllable tempo.
    Slow,
    /// Formants up 120 cents (a smaller, tenser tract).
    Tense,
    /// Formants down 120 cents (a laxer, darker tract).
    Lax,
    /// More aperiodic excitation: +32 noise-dial units.
    Rough,
}

impl Mark {
    /// The vocabulary, in the order the help prints it.
    pub const NAMES: [&'static str; 15] = [
        "urgent", "calm", "clipped", "falling", "rising", "flat", "bright", "dark", "loud", "soft",
        "fast", "slow", "tense", "lax", "rough",
    ];
}

/// Parse a comma-separated mark list (`"urgent,clipped,falling"`). An
/// unknown word is an error that names the whole vocabulary.
pub fn parse_marks(spec: &str) -> Result<Vec<Mark>, String> {
    let mut marks = Vec::new();
    for word in spec.split(',') {
        let word = word.trim();
        if word.is_empty() {
            continue;
        }
        let Some(idx) = Mark::NAMES.iter().position(|n| *n == word) else {
            return Err(format!(
                "unknown mark {word:?}; the vocabulary is: {}",
                Mark::NAMES.join(", ")
            ));
        };
        marks.push(match idx {
            0 => Mark::Urgent,
            1 => Mark::Calm,
            2 => Mark::Clipped,
            3 => Mark::Falling,
            4 => Mark::Rising,
            5 => Mark::Flat,
            6 => Mark::Bright,
            7 => Mark::Dark,
            8 => Mark::Loud,
            9 => Mark::Soft,
            10 => Mark::Fast,
            11 => Mark::Slow,
            12 => Mark::Tense,
            13 => Mark::Lax,
            _ => Mark::Rough,
        });
    }
    if marks.is_empty() {
        return Err(format!(
            "no marks in {spec:?}; try e.g. \"urgent,clipped,falling\""
        ));
    }
    Ok(marks)
}

/// What the analysis of the render said, in the dials the marks speak.
/// Every relative dial is profile-relative (the E4 lesson: absolute
/// classes do not discriminate).
#[derive(Clone, Debug)]
pub struct Measured {
    /// The L2 patch summary.
    pub phrase: Phrase,
    /// The L1 gestures.
    pub gestures: Vec<crate::frame::Gesture>,
    /// The peak level against the profile median, in dB.
    pub vel_db: i32,
    /// The syllable tempo, in tenths per second.
    pub tempo_tenths: u32,
    /// The median spectral tilt over sounding frames, dB/oct.
    pub tilt_db_oct: i32,
    /// The median sounding-gesture attack, in ms.
    pub attack_ms: u32,
    /// The head-to-tail pitch travel of the voiced span, in cents.
    pub contour_cents: i32,
}

/// Read the measured dials off an analysis.
pub fn measure(analysis: &Analysis, profile: &SpeakerProfile) -> Measured {
    let gestures = crate::segment::segment_gestures(&analysis.frames, profile);
    let phrase = crate::phrase::phrase_summary(&analysis.frames, &gestures, profile);

    let mut attacks: Vec<u32> = gestures
        .iter()
        .filter(|g| g.velocity_db != i8::MIN)
        .map(|g| u32::from(g.attack_ms))
        .collect();
    attacks.sort_unstable();
    let attack_ms = attacks
        .get(attacks.len() / 2)
        .copied()
        .unwrap_or(u32::from(profile.typical_attack_ms));

    // The contour travel: the mean of the last voiced eighth against the
    // first (the phrase contour class reads the same two ends).
    let voiced: Vec<i32> = analysis
        .frames
        .iter()
        .filter(|f| f.flags.has(FrameFlags::VOICED))
        .map(|f| i32::from(f.f0_cents))
        .collect();
    let contour_cents = if voiced.len() >= 8 {
        let n = voiced.len().min(8);
        let head = voiced[..n].iter().sum::<i32>() / n as i32;
        let tail = voiced[voiced.len() - n..].iter().sum::<i32>() / n as i32;
        tail - head
    } else {
        0
    };

    Measured {
        vel_db: i32::from(phrase.velocity_db),
        tempo_tenths: u32::from(phrase.tempo_tenths),
        tilt_db_oct: i32::from(phrase.bright_tilt_db_oct),
        attack_ms,
        contour_cents,
        phrase,
        gestures,
    }
}

/// The intended performance as dials: `None` means "no intent on this
/// channel, leave it measured". Built from marks against the measured
/// baseline by [`resolve_marks`], or written directly by a caller that
/// already knows the intent.
#[derive(Clone, Copy, Debug, Default)]
pub struct Target {
    /// A uniform pitch shift, in cents. No mark sets it; callers may.
    pub pitch_cents: i32,
    /// The intended head-to-tail contour travel, in cents.
    pub glide_cents: Option<i32>,
    /// The intended tempo ratio (target over measured).
    pub tempo_ratio: Option<f64>,
    /// The intended pause-length ratio.
    pub pause_ratio: Option<f64>,
    /// Silence to insert at each guarded valley, in ms.
    pub pause_insert_ms: Option<u32>,
    /// How many breaths (guarded valleys) to insert at.
    pub breaths: Option<u32>,
    /// The intended peak level against the profile median, in dB.
    pub vel_target_db: Option<i32>,
    /// The intended median gesture attack, in ms.
    pub attack_target_ms: Option<u32>,
    /// The intended release scale (tails multiplied by this).
    pub release_scale: Option<f64>,
    /// The intended tilt change from measured, in dB/oct.
    pub tilt_delta_db_oct: Option<i32>,
    /// Noise-dial units to mix in.
    pub noise_add: Option<u8>,
    /// A formant shift, in cents.
    pub formant_cents: Option<i32>,
}

/// Fold marks into a target against the measured baseline.
///
/// Class marks (`urgent`, `calm`) set absolute profile-relative dial
/// values; scalar marks (`loud`, `fast`, ...) are *deltas on top of*
/// whatever the class marks set (or of the measured dial when no class
/// mark spoke). Stacking same-channel marks is allowed — and the tier
/// rule is what stops a stack from mangling the line.
pub fn resolve_marks(marks: &[Mark], m: &Measured) -> Target {
    let has = |mark: Mark| marks.contains(&mark);
    // Repeating a scalar mark repeats its effect: "loud,loud" is +12 dB,
    // and the tier rule is what stops a stack from mangling the line.
    let count = |mark: Mark| marks.iter().filter(|&&k| k == mark).count();

    // Velocity: the class marks name absolute profile-relative targets
    // (set from the E10 calibration centroids: the urgent class sits at
    // +18 dB on the velocity dial and the calm class at +11, against a
    // neutral baseline of +8), so a line from anywhere in the neutral
    // spread (+2..+14 dB) is pulled onto the class. The scalar marks
    // are deltas on top.
    //
    // The dial itself moved +30 dB when the trunk fixed the profile's
    // fitted median level: the baseline used to be clamped as a bin
    // index and re-scaled, so every fitted profile read 248 and every
    // line read about -22 dB for a peak that sits *above* its own
    // speaker's median. The targets are re-derived from the same
    // calibration the same way — the class's centroid across the three
    // voices, less the 2 dB the calm stages read back hot.
    let mut vel: Option<i32> = None;
    if has(Mark::Urgent) {
        // +18, the urgent centroid's mean (+17, +19, +19).
        vel = Some(18);
    }
    if has(Mark::Calm) {
        // +9, not the centroid's +11: the stretch and shelf stages
        // read the peak back ~2 dB hot, so the ask is one step under
        // the class to land on it.
        vel = Some(9);
    }
    let vel = vel
        .or(Some(m.vel_db))
        .map(|v| v + count(Mark::Loud) as i32 * 6 - count(Mark::Soft) as i32 * 6);

    // Tempo and pauses as multiplying ratios.
    let mut tempo = 1.0;
    for _ in 0..count(Mark::Fast) {
        tempo *= 1.30;
    }
    for _ in 0..count(Mark::Slow) {
        tempo *= 0.80;
    }
    if has(Mark::Calm) {
        // The calm class is carried by tempo (centroid ratio 0.59-0.79)
        // and length; urgent is not given a tempo change at all — the
        // calibration's urgent class sits at 0.95-1.12, arousal on this
        // corpus is loud, not fast (E4 found the same). The 0.69
        // stretch (x1.45 duration) is the class's duration engine —
        // the E10 tempo sweep measured the slow side word-clean through
        // exactly x1.45 — and the guarded breath adds more length
        // where a wide-enough valley exists (on fluent renders it often
        // does not, and the stretch carries the line alone).
        tempo *= 0.69;
    }
    let mut pause = 1.0;
    if has(Mark::Calm) {
        pause *= 1.5;
    }
    if has(Mark::Clipped) {
        pause *= 0.6;
    }

    // Urgent carries no tilt, and the E10 calibration says why: the
    // corpus's tilt dial reads 0 for all three classes (the shelf's
    // step is inside the analyzer's quantization), so a tilt cannot
    // move the line from ordinary to urgent — it can only cost words.
    // The E10 tilt sweep measured the cost: +4 dB/oct flips "Regroup"
    // into "We grew up" in the probe set, a 25-point word-accuracy drop
    // on that one line. The channel stays in the vocabulary (`bright`
    // and `dark` are its marks); the class marks do not use it.
    let mut tilt = 0;
    if has(Mark::Calm) {
        tilt -= 3;
    }
    if has(Mark::Bright) {
        tilt += 5;
    }
    if has(Mark::Dark) {
        tilt -= 5;
    }

    // Urgent does not stack noise: the E10 sweep found that any noise
    // level strong enough to move the dial also flips the analyzer's
    // voicing at the utterance's edges, which reads as a +300-cent
    // contour jump (CONFORM.md, "what did not work"). The channel is
    // the `rough` mark's to exercise.
    let mut noise = 0u8;
    if has(Mark::Rough) {
        noise += 32;
    }

    let mut formant = 0i32;
    if has(Mark::Tense) {
        formant += 120;
    }
    if has(Mark::Lax) {
        formant -= 120;
    }

    // Attack: urgent and clipped want a sharper onset than measured —
    // half the measured attack, floored at 15 ms, so the intent is a
    // relative sharpening that always sits inside the tier window (an
    // absolute 30 ms target against a long merged-span attack would be a
    // ten-times squeeze, and the tier rule would re-ask every line). Calm
    // gets no attack intent: remapping onto a soft 90 ms onset is beyond
    // the artifact threshold for the same reason.
    let attack = if has(Mark::Clipped) || has(Mark::Urgent) {
        Some((m.attack_ms / 2).max(15))
    } else {
        None
    };
    // Clipped: no release intent — see [`Mark::Clipped`]; the ablation
    // measured tail sculpts flipping final consonants at any scale.
    let release = None;

    // The contour intents are partial-trust: the reshape moves the
    // measured travel at most 400 cents toward the mark's word, because
    // the E10 ablation measured full-travel reshapes destroying onset
    // words ("Hold" -> "Olden") — the linear lift put up to ~475 cents
    // on the line's head, where the consonants live. The 400-cent cap
    // with the terminal-weighted shape in [`apply`] keeps the surgery
    // off the onsets; a line already carrying the intent (travel near
    // the word) is left almost alone.
    let glide = |want: i32| Some((m.contour_cents - 400).max(want).min(m.contour_cents + 400));

    Target {
        pitch_cents: 0,
        glide_cents: if has(Mark::Falling) {
            glide(-300)
        } else if has(Mark::Rising) {
            glide(300)
        } else if has(Mark::Flat) {
            glide(0)
        } else {
            None
        },
        tempo_ratio: Some(tempo),
        pause_ratio: Some(pause),
        pause_insert_ms: has(Mark::Calm).then_some(400),
        breaths: has(Mark::Calm).then_some(2),
        vel_target_db: vel,
        attack_target_ms: attack,
        release_scale: release,
        tilt_delta_db_oct: Some(tilt),
        noise_add: (noise > 0).then_some(noise),
        formant_cents: (formant != 0).then_some(formant),
    }
}

/// One channel's diff: what was wanted, what was measured, what the DSP
/// would have to apply, and whether that is inside the artifact threshold.
#[derive(Clone, Debug)]
pub struct ChannelDiff {
    /// The channel's name, as the re-ask line prints it.
    pub name: &'static str,
    /// The intended dial value, in the channel's own unit.
    pub wanted: f64,
    /// The measured dial value.
    pub measured: f64,
    /// The correction the DSP would apply.
    pub correction: f64,
    /// The threshold the correction must fit inside.
    pub limit: f64,
    /// Whether it does not.
    pub beyond: bool,
}

/// The whole diff, channel by channel.
#[derive(Clone, Debug, Default)]
pub struct Diff {
    /// One entry per channel with an intent on it.
    pub channels: Vec<ChannelDiff>,
}

impl Diff {
    /// Whether any channel is beyond its threshold — the re-ask condition.
    pub fn any_beyond(&self) -> bool {
        self.channels.iter().any(|c| c.beyond)
    }

    /// The re-ask line: the channels that went beyond, with numbers.
    pub fn reask_line(&self) -> String {
        let over: Vec<String> = self
            .channels
            .iter()
            .filter(|c| c.beyond)
            .map(|c| {
                format!(
                    "{} wants {:+.2} (limit {:+.2})",
                    c.name, c.correction, c.limit
                )
            })
            .collect();
        format!("re-ask: {}", over.join("; "))
    }
}

/// Diff the intended target against the measured dials, channel by
/// channel, against the thresholds. Channels without intent are skipped.
pub fn diff(m: &Measured, t: &Target, th: &Thresholds) -> Diff {
    let mut channels = Vec::new();

    if let Some(want) = t.vel_target_db {
        channels.push(ChannelDiff {
            name: "vel",
            wanted: f64::from(want),
            measured: f64::from(m.vel_db),
            correction: f64::from(want - m.vel_db),
            limit: f64::from(th.gain_db),
            beyond: (want - m.vel_db).abs() > th.gain_db as i32,
        });
    }
    // The ratio channels have a window, not a magnitude.
    let ratio_channel = |name: &'static str, ratio: f64, lo: f64, hi: f64| -> ChannelDiff {
        ChannelDiff {
            name,
            wanted: ratio,
            measured: 1.0,
            correction: ratio,
            limit: if ratio > 1.0 { hi } else { lo },
            beyond: ratio > hi || ratio < lo,
        }
    };
    if let Some(ratio) = t.tempo_ratio {
        channels.push(ratio_channel("tempo", ratio, th.tempo_min, th.tempo_max));
    }
    if let Some(ratio) = t.pause_ratio {
        channels.push(ratio_channel("pause", ratio, th.pause_min, th.pause_max));
    }
    if let Some(ms) = t.pause_insert_ms {
        let breaths = t.breaths.unwrap_or(1);
        channels.push(ChannelDiff {
            name: "pause-insert",
            wanted: f64::from(ms),
            measured: 0.0,
            correction: f64::from(ms),
            limit: f64::from(th.pause_insert_ms),
            beyond: ms > th.pause_insert_ms || breaths > th.breaths,
        });
    }
    if let Some(want) = t.attack_target_ms {
        let scale = f64::from(want) / f64::from(m.attack_ms.max(1));
        channels.push(ratio_channel("attack", scale, th.attack_min, th.attack_max));
    }
    if let Some(scale) = t.release_scale {
        channels.push(ratio_channel(
            "release",
            scale,
            th.release_min,
            th.release_max,
        ));
    }
    if let Some(delta) = t.tilt_delta_db_oct {
        channels.push(ChannelDiff {
            name: "tilt",
            wanted: f64::from(delta + m.tilt_db_oct),
            measured: f64::from(m.tilt_db_oct),
            correction: f64::from(delta),
            limit: f64::from(th.tilt_db_oct),
            beyond: delta.abs() > th.tilt_db_oct as i32,
        });
    }
    if let Some(add) = t.noise_add {
        channels.push(ChannelDiff {
            name: "noise",
            wanted: f64::from(add),
            measured: 0.0,
            correction: f64::from(add),
            limit: f64::from(th.noise_add),
            beyond: add > th.noise_add,
        });
    }
    if let Some(cents) = t.formant_cents {
        channels.push(ChannelDiff {
            name: "formant",
            wanted: f64::from(cents),
            measured: 0.0,
            correction: f64::from(cents),
            limit: f64::from(th.formant_cents),
            beyond: cents.abs() > th.formant_cents as i32,
        });
    }
    if t.pitch_cents != 0 {
        channels.push(ChannelDiff {
            name: "pitch",
            wanted: f64::from(t.pitch_cents),
            measured: 0.0,
            correction: f64::from(t.pitch_cents),
            limit: f64::from(th.pitch_cents),
            beyond: t.pitch_cents.abs() > th.pitch_cents as i32,
        });
    }
    if let Some(want) = t.glide_cents {
        // The imposed travel is centered on the measured contour, so each
        // end of the line moves by half the travel change.
        let shift = f64::from(want - m.contour_cents);
        channels.push(ChannelDiff {
            name: "glide",
            wanted: f64::from(want),
            measured: f64::from(m.contour_cents),
            correction: shift,
            limit: f64::from(2 * th.glide_cents),
            beyond: (shift / 2.0).abs() > f64::from(th.glide_cents),
        });
    }

    Diff { channels }
}

/// The per-syllable envelope sculpting one gesture is asking for, in the
/// input's own timeline.
#[derive(Clone, Copy, Debug)]
pub struct GestureEnv {
    /// The gesture's first frame, in ms.
    pub start_ms: u32,
    /// One past the gesture's last frame, in ms.
    pub end_ms: u32,
    /// The intended attack, ms (valley to peak).
    pub attack_ms: u32,
    /// The gesture's measured attack, ms.
    pub measured_attack_ms: u32,
    /// The intended release, ms (peak to the gesture's end).
    pub release_ms: u32,
    /// The gesture's measured release, ms.
    pub measured_release_ms: u32,
}

/// The correction plan: what each DSP stage will apply. The numbers are
/// already the diffed, threshold-checked corrections.
#[derive(Clone, Debug)]
pub struct Plan {
    /// Whole-line gain toward the velocity target, dB.
    pub gain_db: f64,
    /// Attack/release sculpting per sounding gesture.
    pub envelopes: Vec<GestureEnv>,
    /// Uniform pitch shift, cents.
    pub pitch_cents: f64,
    /// The imposed contour travel, cents (the measured travel is reshaped
    /// onto this, centered so the mean pitch holds).
    pub glide_target_cents: Option<f64>,
    /// The sounding-tempo ratio (syllable rate multiplier).
    pub tempo_ratio: f64,
    /// The pause-length ratio.
    pub pause_ratio: f64,
    /// Silence to insert at each guarded valley, ms.
    pub pause_insert_ms: u32,
    /// How many guarded valleys breathe.
    pub breaths: u32,
    /// Spectral tilt correction, dB/oct.
    pub tilt_db_oct: f64,
    /// Noise-dial units to mix in.
    pub noise_add: u8,
    /// Formant shift, cents.
    pub formant_cents: f64,
    /// The diff the plan came from, for reporting.
    pub diff: Diff,
}

fn plan_of(t: &Target, m: &Measured) -> Plan {
    // The sculpting zones are bounded: a gesture's measured release can
    // be the whole second half of the gesture, and remapping more than
    // 300 ms of tail is a re-write, not a conform.
    const ZONE_MS: u32 = 300;
    // Only gestures with an actual intent on them are sculpted. The E10
    // ablation caught the alternative: building a zone per gesture
    // unconditionally made the envelope stage force every attack and
    // release onto its synthetic ramps even for a null target — a
    // whole-line mangle no other stage asked for — and that, not the
    // pitch pass, was what had been costing the words.
    let release_scale = t.release_scale.unwrap_or(1.0);
    let envelopes = m
        .gestures
        .iter()
        .filter(|g| g.velocity_db != i8::MIN)
        .filter(|_| t.attack_target_ms.is_some() || (release_scale - 1.0).abs() >= 0.01)
        .map(|g| GestureEnv {
            start_ms: g.onset_ms,
            end_ms: g.onset_ms + g.duration_ms,
            attack_ms: t.attack_target_ms.unwrap_or(u32::from(g.attack_ms)),
            measured_attack_ms: u32::from(g.attack_ms).min(ZONE_MS),
            release_ms: ((f64::from(g.release_ms) * release_scale) as u32).clamp(10, ZONE_MS),
            measured_release_ms: u32::from(g.release_ms).min(ZONE_MS),
        })
        .collect();
    Plan {
        gain_db: t
            .vel_target_db
            .map(|v| f64::from(v - m.vel_db))
            .unwrap_or(0.0),
        envelopes,
        pitch_cents: f64::from(t.pitch_cents),
        glide_target_cents: t.glide_cents.map(f64::from),
        tempo_ratio: t.tempo_ratio.unwrap_or(1.0),
        pause_ratio: t.pause_ratio.unwrap_or(1.0),
        pause_insert_ms: t.pause_insert_ms.unwrap_or(0),
        breaths: t.breaths.unwrap_or(1),
        tilt_db_oct: f64::from(t.tilt_delta_db_oct.unwrap_or(0)),
        noise_add: t.noise_add.unwrap_or(0),
        formant_cents: f64::from(t.formant_cents.unwrap_or(0)),
        diff: Diff::default(),
    }
}

/// What conforming came to.
pub enum ConformOutcome {
    /// The line was corrected; the audio and the plan that shaped it.
    Conformed {
        /// The plan (with its diff) that was applied.
        plan: Plan,
        /// The conformed audio.
        audio: Pcm,
    },
    /// A diff went beyond the artifact thresholds: re-ask the generator
    /// instead of correcting. The diff names the channels.
    ReAsk(Diff),
}

/// Conform a render toward a mark list. See [`conform_with_target`].
pub fn conform(pcm: &Pcm, marks: &[Mark], profile: &SpeakerProfile) -> ConformOutcome {
    let analysis = analyze_with_profile(pcm, profile);
    let measured = measure(&analysis, profile);
    let target = resolve_marks(marks, &measured);
    conform_with_target(pcm, &target, profile)
}

/// Conform a render toward a mark string (`"urgent,clipped,falling"`).
pub fn conform_str(
    pcm: &Pcm,
    spec: &str,
    profile: &SpeakerProfile,
) -> Result<ConformOutcome, String> {
    let marks = parse_marks(spec)?;
    Ok(conform(pcm, &marks, profile))
}

/// Fit a profile on the clip's own analysis: the single-file caller's
/// baseline (the CLI has one WAV and no corpus to learn from). Every
/// relative dial then reads against the clip's own median, which is the
/// honest baseline for "make this line urgent".
pub fn self_profile(analysis: &Analysis) -> SpeakerProfile {
    let mut p = SpeakerProfile::default();
    let gestures = crate::segment::segment_gestures(&analysis.frames, &p);
    let phrase = crate::phrase::phrase_summary(&analysis.frames, &gestures, &p);
    p.update(&analysis.frames, &gestures, &phrase);
    p
}

/// The full pipeline: analyze, diff, tier-rule, apply. The analysis is
/// done once and shared by the diff and the DSP.
pub fn conform_with_target(pcm: &Pcm, target: &Target, profile: &SpeakerProfile) -> ConformOutcome {
    let analysis = analyze_with_profile(pcm, profile);
    let measured = measure(&analysis, profile);
    let d = diff(&measured, target, &Thresholds::default());
    if d.any_beyond() {
        return ConformOutcome::ReAsk(d);
    }
    let mut plan = plan_of(target, &measured);
    plan.diff = d;
    let audio = apply(pcm, &plan, &analysis, &measured);
    ConformOutcome::Conformed { plan, audio }
}

// ---------------------------------------------------------------------------
// The DSP chain
// ---------------------------------------------------------------------------

/// Run the plan's stages, in the order that keeps every stage's timeline
/// assumptions true: the envelope reads the original 100 Hz dial frames,
/// so it goes first; the pitch stages (formant restore, contour) run on
/// the original timeline next; tempo and pauses move events, so they come
/// after them but before the timbre stages, which are time-blind.
fn apply(pcm: &Pcm, plan: &Plan, analysis: &Analysis, measured: &Measured) -> Pcm {
    let n = pcm.samples.len();
    let rate = pcm.rate;
    let mut x: Vec<f64> = pcm
        .samples
        .iter()
        .map(|&s| f64::from(s) / 32_768.0)
        .collect();
    let f0 = F0Track::from_analysis(analysis);

    // 1. Per-syllable gain and attack/release sculpting.
    if plan.gain_db.abs() >= 0.25 || !plan.envelopes.is_empty() {
        x = apply_envelope(&x, rate, plan, analysis);
    }

    // 2. Formant shift: resample (everything moves), PSOLA the pitch and
    //    duration back (only the tract shape stays moved).
    if plan.formant_cents.abs() >= 1.0 {
        x = formant_shift(&x, rate, &f0, plan.formant_cents);
    }

    // 3. Pitch and contour: one PSOLA pass with a time-varying ratio.
    if plan.pitch_cents.abs() >= 1.0 || plan.glide_target_cents.is_some() {
        let measured_travel = f64::from(measured.contour_cents);
        let glide = plan.glide_target_cents;
        let pitch = plan.pitch_cents;
        let (t0, t1) = f0.voiced_span_s();
        let ratio = move |t: f64| -> f64 {
            let mut cents = pitch;
            if let Some(target) = glide {
                // Reshape the contour onto the target travel with a
                // terminal-weighted profile: the head of the line moves
                // at most 0.35 of the travel change (onset consonants
                // are where the words live — the E10 ablation measured
                // linear reshapes destroying them), and the fall or rise
                // concentrates on the tail, where a declarative or
                // interrogative contour naturally carries it. Outside
                // the voiced span the shape holds its end value.
                if t1 > t0 {
                    let u = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
                    cents += (target - measured_travel) * (u * u - 0.35);
                }
            }
            cents
        };
        x = psola(&x, rate, &f0, &ratio, &|_| 1.0, n);
    }

    // 4. Tempo and pauses: WSOLA on the sounding islands, rest gaps
    //    rescaled, the calm breath at the guarded valley.
    if (plan.tempo_ratio - 1.0).abs() >= 0.01
        || (plan.pause_ratio - 1.0).abs() >= 0.01
        || plan.pause_insert_ms > 0
    {
        x = tempo_pass(&x, rate, &analysis.frames, &measured.gestures, plan);
    }

    // 5. Spectral tilt.
    if plan.tilt_db_oct.abs() >= 0.5 {
        x = tilt_eq(&x, rate, plan.tilt_db_oct);
    }

    // 6. Noise mix.
    if plan.noise_add > 0 {
        x = mix_noise(&x, &analysis.frames, plan.noise_add);
    }

    // 7. Headroom: if the gain pushed the peak past full scale, the whole
    //    line comes down (relative dynamics are what the marks asked for).
    let peak = x
        .iter()
        .fold(0.0_f64, |a, v| if v.abs() > a { v.abs() } else { a });
    if peak > 0.97 {
        let scale = 0.97 / peak;
        for v in x.iter_mut() {
            *v *= scale;
        }
    }

    Pcm {
        // Round, never truncate: a 1-LSB downward bias on every positive
        // sample is a (small) systematic degradation the conformed line
        // should not start life with.
        samples: x
            .iter()
            .map(|&v| (v * 32_767.0).round().clamp(-32_768.0, 32_767.0) as i16)
            .collect(),
        rate,
    }
}

/// The F0 the analyzer measured, addressable at sample resolution.
pub struct F0Track {
    /// The voiced pitch in Hz per 10 ms frame; `None` is unvoiced.
    pub hz: Vec<Option<f64>>,
}

impl F0Track {
    /// Read the pitch track out of an analysis.
    pub fn from_analysis(a: &Analysis) -> F0Track {
        let ref_hz = a.profile.f0_ref();
        F0Track {
            hz: a
                .frames
                .iter()
                .map(|f| {
                    if f.flags.has(FrameFlags::VOICED) && f.f0_cents > 0 {
                        Some(Frame::cents_to_hz(f64::from(f.f0_cents), ref_hz))
                    } else {
                        None
                    }
                })
                .collect(),
        }
    }

    /// The pitch at a sample position: linear in cents across the
    /// bracketing frames when both are voiced, the nearer one otherwise.
    fn at(&self, sample: usize, rate: u32) -> Option<f64> {
        if self.hz.is_empty() {
            return None;
        }
        let pos = sample as f64 * 100.0 / f64::from(rate);
        let i = pos.floor() as usize;
        if i >= self.hz.len() {
            return self.hz[self.hz.len() - 1];
        }
        let frac = pos - i as f64;
        match (
            self.hz.get(i).copied().flatten(),
            self.hz.get(i + 1).copied().flatten(),
        ) {
            (Some(a), Some(b)) => Some(a * exp2(log2(b / a) * frac)),
            (Some(a), None) if frac < 0.5 => Some(a),
            (None, Some(b)) if frac >= 0.5 => Some(b),
            _ => None,
        }
    }

    /// The voiced span in seconds: first voiced onset to last voiced
    /// offset. The contour ramp is drawn over exactly this span.
    fn voiced_span_s(&self) -> (f64, f64) {
        match (
            self.hz.iter().position(Option::is_some),
            self.hz.iter().rposition(Option::is_some),
        ) {
            (Some(f), Some(l)) => (f as f64 / 100.0, (l + 1) as f64 / 100.0),
            _ => (0.0, 0.0),
        }
    }

    /// The track under a uniform resample by `r` (frequencies scale by r,
    /// the timeline compresses by 1/r): the formant stage's twin.
    fn scaled(&self, r: f64) -> F0Track {
        let new_len = ((self.hz.len() as f64) / r).ceil() as usize;
        let pick = |j: usize| self.hz.get(j.min(self.hz.len() - 1)).copied().flatten();
        let mut hz = Vec::with_capacity(new_len);
        for i in 0..new_len {
            let src = i as f64 * r;
            let i0 = src.floor() as usize;
            let frac = src - i0 as f64;
            hz.push(match (pick(i0), pick(i0 + 1)) {
                (Some(a), Some(b)) => Some(a * exp2(log2(b / a) * frac) * r),
                (Some(a), None) if frac < 0.5 => Some(a * r),
                (None, Some(b)) if frac >= 0.5 => Some(b * r),
                _ => None,
            });
        }
        F0Track { hz }
    }
}

/// A deterministic PRNG for the noise mix: xorshift64* from a committed
/// seed. Integer arithmetic only, so every machine draws the same stream.
struct Rng(u64);

impl Rng {
    fn next_f64(&mut self) -> f64 {
        let mut z = self.0;
        z ^= z >> 12;
        z ^= z << 25;
        z ^= z >> 27;
        self.0 = z;
        let bits = z.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11; // 53 uniform bits
        (bits as f64) / 9_007_199_254_740_992.0
    }
}

// --- PSOLA -----------------------------------------------------------------

/// Analysis epochs: (sample position, local period in samples), found by
/// walking each voiced island one period at a time and peak-picking — the
/// plain epoch detector simple TD-PSOLA implementations carry.
fn detect_epochs(x: &[f64], rate: u32, f0: &F0Track) -> Vec<(usize, f64)> {
    let n = x.len();
    let step = (rate / 100) as usize;
    let period = |p: usize| -> f64 {
        let hz = f0.at(p, rate).unwrap_or(150.0);
        (f64::from(rate) / hz).clamp(f64::from(rate) / 500.0, f64::from(rate) / 60.0)
    };
    let argmax = |from: usize, to: usize| -> usize {
        let mut best = from;
        for p in from..to {
            if x[p] > x[best] {
                best = p;
            }
        }
        best
    };

    let mut epochs = Vec::new();
    let mut i = 0usize;
    while i < n {
        if f0.at(i, rate).is_none() {
            i += step;
            continue;
        }
        // The voiced island [i, j). The walk steps in quarters of a frame
        // and can step past the end of the signal, so the island's end is
        // clamped to it: the peak-pick windows below read the signal, and
        // a window that ran past the end read out of bounds — the arms
        // hit it on a line whose last voiced frame is the clip's last
        // frame, where the first window of the island starts within one
        // period of the end.
        let mut j = i;
        while j < n && f0.at(j, rate).is_some() {
            j += step / 4;
        }
        let j = j.min(n);
        // First epoch: the largest excursion of the first period.
        let mut e = argmax(i, (i + period(i) as usize + 1).min(j));
        loop {
            let p = period(e);
            epochs.push((e, p));
            let w0 = e + (0.7 * p) as usize;
            let w1 = ((e as f64 + 1.3 * p) as usize).min(j);
            if w1 <= w0 + 2 || w1 >= n {
                break;
            }
            e = argmax(w0, w1);
        }
        i = j;
    }
    epochs
}

/// TD-PSOLA: pitch ratio `pitch_cents(t)` and time ratio `alpha(t)`, the
/// output as long as `out_len`.
///
/// Synthesis marks advance by the analysis period over the pitch ratio
/// (that spacing is the output's periodicity); the read pointer advances
/// by the same hop over the time ratio (that walk sets the duration).
/// Each mark copies the 2-period Hann grain centered on the nearest
/// analysis epoch, and the sum is normalized by the accumulated window,
/// so overlap ripple does not become amplitude ripple.
fn psola(
    x: &[f64],
    rate: u32,
    f0: &F0Track,
    pitch_cents: &dyn Fn(f64) -> f64,
    alpha: &dyn Fn(f64) -> f64,
    out_len: usize,
) -> Vec<f64> {
    let n = x.len();
    if n == 0 || out_len == 0 {
        return x[..out_len.min(n)].to_vec();
    }
    let hop = (rate / 100) as usize;
    let epochs = detect_epochs(x, rate, f0);
    let mut acc = vec![0.0_f64; out_len];
    let mut wacc = vec![0.0_f64; out_len];

    let mut s = 0.0_f64; // synthesis position
    let mut tau = 0.0_f64; // read position
                           // The last epoch-locked grain's (source center, write position): the
                           // voiced grains of an island form a phase-locked chain, each written
                           // one output period (the true epoch spacing over the pitch ratio)
                           // after the last. At ratio 1 the chain writes every grain back where
                           // it came from — an exact identity — and the E10 ablation measured
                           // the plain synthesis-grid placement (marks drifting against the
                           // true epoch phase) leaving 10-30% sample error that cost words.
    let mut lock: Option<(f64, f64)> = None;
    while (s as usize) < out_len {
        let tau_i = tau as usize;
        let voiced = tau_i < n && f0.at(tau_i, rate).is_some();
        let ta = if voiced {
            let hz = f0.at(tau_i, rate).unwrap_or(150.0);
            (f64::from(rate) / hz).clamp(f64::from(rate) / 500.0, f64::from(rate) / 60.0)
        } else {
            hop as f64
        };
        let r = if voiced {
            exp2(pitch_cents(tau / f64::from(rate)) / 1200.0)
        } else {
            1.0
        };
        let a = alpha(tau / f64::from(rate)).max(0.25);
        let hs = (ta / r).max(8.0);

        // The grain: centered on the nearest epoch (voiced) or on the
        // read pointer itself (unvoiced, where there is nothing to lock).
        // Unvoiced grains carry a rectangular window, not Hann: an
        // unvoiced span is consonants — stop bursts, fricatives, breath
        // — and Hann-OLA at half-overlap low-passes exactly that detail
        // (the E10 ablation measured a pure reconstruction pass turning
        // "Hold" into "Old"). Rectangular, the window-sum normalization
        // makes the steady state an exact copy of the source, so the
        // words pass through untouched wherever there is no pitch to
        // move.
        let glen = (2.0 * ta).round() as usize;
        let w: Vec<f64> = if voiced { hann(glen) } else { vec![1.0; glen] };
        // The grain's source center: the nearest epoch (voiced) or the
        // read pointer itself (unvoiced, where there is nothing to
        // lock). `None` means no epoch lock was found.
        let locked_epoch: Option<usize> = if voiced {
            let nearest = epochs
                .binary_search_by(|(pos, _)| pos.cmp(&tau_i))
                .unwrap_or_else(|k| k);
            let before = nearest.checked_sub(1).and_then(|k| epochs.get(k));
            let after = epochs.get(nearest);
            let best = match (before, after) {
                (Some(b0), Some(a0)) => {
                    if (b0.0 as i64 - tau_i as i64).abs() <= (a0.0 as i64 - tau_i as i64).abs() {
                        b0.0
                    } else {
                        a0.0
                    }
                }
                (Some(b0), None) => b0.0,
                (None, Some(a0)) => a0.0,
                (None, None) => tau_i,
            };
            // An epoch more than 1.5 periods away is no lock: fall back
            // to the plain position rather than repeating a far pulse.
            if (best as i64 - tau_i as i64).abs() > (1.5 * ta) as i64 {
                None
            } else {
                Some(best)
            }
        } else {
            None
        };
        let center = locked_epoch.unwrap_or(tau_i.min(n - 1));

        // The write position: a locked grain joins the phase-locked
        // chain (see `lock` above) — but only on a steady timeline
        // (alpha ~ 1); the formant stage's restore pass works on a
        // squeezed timeline through the classic grid, and the chain's
        // per-epoch advance has no alpha term. Anything else writes on
        // the plain synthesis grid.
        let steady_time = (a - 1.0).abs() < 0.02;
        let dst_center = match (locked_epoch, steady_time) {
            (Some(e), true) => {
                let e = e as f64;
                let next = match lock {
                    Some((c_prev, d_prev)) => d_prev + (e - c_prev) / r,
                    None => s + (e - tau),
                };
                lock = Some((e, next));
                next
            }
            _ => {
                lock = None;
                s
            }
        };

        let half = glen / 2;
        let src_base = center as i64 - half as i64;
        let dst_base = dst_center as i64 - half as i64;
        for (i, &wi) in w.iter().enumerate() {
            let src = src_base + i as i64;
            let dst = dst_base + i as i64;
            if src < 0 || src >= n as i64 || dst < 0 || dst >= out_len as i64 {
                continue;
            }
            acc[dst as usize] += x[src as usize] * wi;
            wacc[dst as usize] += wi;
        }

        s += hs;
        tau += hs / a;
        if tau >= n as f64 + 2.0 * ta {
            break;
        }
    }

    // Normalize by the window, and where the window barely reached (the
    // very edges), keep the input's own samples, mapped onto this
    // timeline.
    (0..out_len)
        .map(|i| {
            if wacc[i] >= 0.35 {
                acc[i] / wacc[i]
            } else {
                let src = (i as f64 / alpha(i as f64 / f64::from(rate)).max(0.25)).round() as usize;
                x[src.min(n - 1)]
            }
        })
        .collect()
}

/// A uniform pitch shift by whole cents, duration and formants held —
/// the E10 sweep's handle and the formant stage's second half.
pub fn psola_pitch_shift(x: &[f64], rate: u32, f0: &F0Track, cents: f64) -> Vec<f64> {
    psola(x, rate, f0, &|_| cents, &|_| 1.0, x.len())
}

// --- WSOLA and the tempo pass ----------------------------------------------

/// WSOLA time-scale modification: `alpha` is the output-over-input
/// duration ratio (1.3 stretches). 30 ms Hann frames, half-overlap
/// analysis hop, similarity search of ±quarter-frame against the
/// already-written overlap, window-sum normalized OLA.
///
/// The write hop is the frame's half, always: a Hann window at 50%
/// overlap sums to a constant, so the overlap-add leaves the envelope
/// alone. Scaling the *write* hop instead (the obvious way to change
/// the time scale) breaks that sum whenever the ratio is not 1.0 and
/// puts a ripple on the envelope at the frame rate — which the
/// analyzer's onset detector reads as extra syllables, and the tempo
/// dial with them. The time scale is therefore carried by the *read*
/// hop: the read advances `half / alpha` per frame, so the input is
/// consumed once and the output is `alpha` times longer.
pub fn wsola(x: &[f64], rate: u32, alpha: f64) -> Vec<f64> {
    let n = x.len();
    if n < 64 {
        return x.to_vec();
    }
    let frame = ((f64::from(rate) * 0.030) as usize).max(16) & !1;
    let hs = frame / 2;
    let read_hop = ((hs as f64 / alpha).round() as usize).max(1);
    let out_len = ((n as f64) * alpha).round() as usize;
    let search = frame / 4;
    let w = hann(frame);
    let mut acc = vec![0.0_f64; out_len];
    let mut wacc = vec![0.0_f64; out_len];

    // The read is padded with the input's own tail, so the last frames
    // have a full window to read without being clamped onto the same
    // samples twice: a clamp there repeats the tail (a stutter), and the
    // analyzer reads the stutter's break as a new syllable.
    let pad = frame.min(n);
    let mut src: Vec<f64> = Vec::with_capacity(n + pad);
    src.extend_from_slice(x);
    src.extend_from_slice(&x[n - pad..]);
    let src_limit = src.len() - frame;

    let mut k = 0usize;
    while k * hs < out_len {
        let p = k * hs;
        let q_nominal = k.saturating_mul(read_hop);
        let overlap = frame.saturating_sub(hs).min(out_len - p);

        // The similarity search: the candidate whose head best matches
        // what has already been written where this frame will land —
        // scored as a normalized projection against the written overlap.
        // The read hop is `hs / alpha` and the write hop is `hs`, so
        // consecutive frames land at different signal phase;
        // this search is what re-phases them (pinning the read to the
        // nominal grid instead smears every join — measured as a shred
        // on the periodic unit-test fixture).
        let mut best_d = 0i64;
        if k > 0 && overlap >= 16 {
            let mut best_score = f64::NEG_INFINITY;
            let mut d = -(search as i64);
            while d <= search as i64 {
                let q = q_nominal as i64 + d;
                if q < 0 || q + overlap as i64 > src_limit as i64 {
                    d += 1;
                    continue;
                }
                let (mut dot, mut energy) = (0.0, 1e-12);
                for i in 0..overlap {
                    let v = src[(q + i as i64) as usize];
                    dot += v * acc[p + i];
                    energy += v * v;
                }
                let score = dot / energy.sqrt();
                if score > best_score {
                    best_score = score;
                    best_d = d;
                }
                d += 1;
            }
        }
        let q = (q_nominal as i64 + best_d).clamp(0, src_limit as i64) as usize;
        for i in 0..frame {
            if p + i >= out_len {
                break;
            }
            acc[p + i] += src[q + i] * w[i];
            wacc[p + i] += w[i];
        }
        k += 1;
    }

    (0..out_len)
        .map(|i| {
            // The tail beyond the last full frame: the input's own
            // continuation, mapped onto this timeline. The OLA and the
            // mapped tail are cross-faded rather than switched, so the
            // join is not a step.
            let src_at = ((i as f64) / alpha).round() as usize;
            let tail = src[src_at.min(src.len() - 1)];
            if wacc[i] <= 1e-6 {
                tail
            } else {
                let t = (wacc[i] / 0.5).min(1.0);
                (acc[i] / wacc[i]) * t + tail * (1.0 - t)
            }
        })
        .collect()
}

/// The sounding islands of the frames, as sample ranges.
fn islands(frames: &[Frame], rate: u32) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, f) in frames.iter().enumerate() {
        if f.flags.has(FrameFlags::REST) {
            if let Some(s) = start.take() {
                out.push((s, frame_sample(i, rate)));
            }
        } else if start.is_none() {
            start = Some(frame_sample(i, rate));
        }
    }
    if let Some(s) = start {
        out.push((s, frame_sample(frames.len(), rate)));
    }
    out
}

/// The calm breath's insertion point: the deepest measured energy valley
/// that sits at a gesture boundary the segmenter already cut, with at
/// least 100 ms of sound on each side, inside the middle 60% of the line.
/// `None` when no boundary qualifies — a short or unbroken line keeps its
/// timeline rather than breathing somewhere risky.
///
/// The earlier fallback — any level trough — was removed after
/// "Re|group": a trough can be a stop closure inside a word, and inserted
/// silence there splits the word. Reading the segmenter's own cuts
/// instead means the point is a transition the ear already heard as a
/// syllable or word edge, which is the least risky place to breathe; the
/// E10 insert sweep (CONFORM.md) measured the word cost of breathing
/// there.
pub fn breath_point(frames: &[Frame], gestures: &[Gesture], rate: u32) -> Option<usize> {
    breath_points(frames, gestures, rate, 1).into_iter().next()
}

/// Up to `want` breath points ([`breath_point`]'s rule), the deepest
/// first, no two within 400 ms of each other — a calm line breathes
/// between clauses, not twice in one gap.
pub fn breath_points(frames: &[Frame], gestures: &[Gesture], rate: u32, want: usize) -> Vec<usize> {
    let n = frames.len();
    if n < 30 || gestures.len() < 2 || want == 0 {
        return Vec::new();
    }
    let lo = n / 5;
    let hi = n - n / 5;
    // The qualifying candidates: (depth, frame index), deepest first.
    let mut cands: Vec<(u8, usize)> = Vec::new();
    for pair in gestures.windows(2) {
        // The boundary between two sounding gestures, as a frame index.
        let (a, b) = (pair[0], pair[1]);
        if a.velocity_db == i8::MIN || b.velocity_db == i8::MIN {
            continue;
        }
        let mid = ((a.onset_ms + a.duration_ms + b.onset_ms) / 20) as usize; // frames
        if mid < lo || mid > hi {
            continue;
        }
        // 100 ms of sound on each side of the valley.
        if mid < 10 || mid + 10 > n {
            continue;
        }
        if frames[mid.saturating_sub(10)..mid]
            .iter()
            .any(|f| f.flags.has(FrameFlags::REST))
            || frames[mid..mid + 10]
                .iter()
                .any(|f| f.flags.has(FrameFlags::REST))
        {
            continue;
        }
        let depth = frames[mid.saturating_sub(3)..(mid + 4).min(n)]
            .iter()
            .map(|f| f.rms)
            .min()
            .unwrap_or(u8::MAX);
        // The width guard: the low run around the valley must span at
        // least 80 ms. A stop closure ("Re|group" — the /g/ is a
        // near-silent, boundary-looking dip *inside* a word) runs
        // 30-70 ms; a word-edge trough runs wider. The E10 ablation
        // caught the closure case with inserted silence splitting the
        // word, and this guard is what rejects it.
        let floor = depth.saturating_add(6);
        let mut lo_w = mid;
        while lo_w > 0 && frames[lo_w - 1].rms <= floor {
            lo_w -= 1;
        }
        let mut hi_w = mid;
        while hi_w + 1 < n && frames[hi_w + 1].rms <= floor {
            hi_w += 1;
        }
        if hi_w - lo_w + 1 < 8 {
            continue;
        }
        cands.push((depth, mid));
    }
    cands.sort_unstable();
    // Take the deepest, then the deepest again at least 400 ms away,
    // and so on. (Frames are 10 ms, so 400 ms is 40 of them.)
    let min_gap = 40;
    let mut chosen: Vec<usize> = Vec::new();
    for (_, mid) in cands {
        if chosen.len() >= want {
            break;
        }
        if chosen.iter().all(|&c| c.abs_diff(mid) >= min_gap) {
            chosen.push(mid);
        }
    }
    chosen
        .into_iter()
        .map(|mid| frame_sample(mid, rate))
        .collect()
}

/// Silence insertion at a sample position: the calm breath. Editing
/// silence cannot touch a phoneme — the words before and after are
/// byte-identical slices of the render.
pub fn insert_silence(x: &[f64], at: usize, samples: usize) -> Vec<f64> {
    let at = at.min(x.len());
    let mut out = Vec::with_capacity(x.len() + samples);
    out.extend_from_slice(&x[..at]);
    out.extend(std::iter::repeat_n(0.0, samples));
    out.extend_from_slice(&x[at..]);
    out
}

/// Tempo and pauses: WSOLA on the sounding islands, rest gaps rescaled,
/// and the calm breath inserted at the deepest measured valley.
fn tempo_pass(
    x: &[f64],
    rate: u32,
    frames: &[Frame],
    gestures: &[crate::frame::Gesture],
    plan: &Plan,
) -> Vec<f64> {
    let n = x.len();
    let isles = islands(frames, rate);
    if isles.is_empty() {
        return x.to_vec();
    }
    // WSOLA's time ratio: tempo_ratio > 1 means faster, so shorter.
    let alpha = 1.0 / plan.tempo_ratio;

    // Where the inserted pauses go: [`breath_points`]'s guarded
    // valleys — gesture boundaries with sound on both side, in the
    // middle of the line, the deepest first. Fewer (or none) means the
    // line had fewer qualifying valleys; it keeps its timeline rather
    // than breathing somewhere risky.
    let mut cuts = if plan.pause_insert_ms > 0 {
        breath_points(frames, gestures, rate, plan.breaths as usize)
    } else {
        Vec::new()
    };
    cuts.sort_unstable();

    let mut out: Vec<f64> = Vec::with_capacity(n + n / 2);

    // The head, before the first island.
    out.extend_from_slice(&x[..isles[0].0.min(n)]);

    for (ii, &(s, e)) in isles.iter().enumerate() {
        if ii > 0 {
            // The gap before this island: near-silence, rescaled.
            let prev_e = isles[ii - 1].1;
            let gap = &x[prev_e.min(n)..s.min(n)];
            let want = ((gap.len() as f64) * plan.pause_ratio).round() as usize;
            for i in 0..want {
                out.push(gap.get(i).copied().unwrap_or(0.0));
            }
        }

        // The island, split at each insertion point inside it, each
        // stretch separated by the breath.
        let mut seg_start = s;
        for cut in cuts.iter().copied().filter(|&c| c > s && c < e) {
            if cut > seg_start {
                out.extend(wsola(&x[seg_start..cut], rate, alpha));
            }
            let insert_samples = plan.pause_insert_ms as usize * rate as usize / 1000;
            out.extend(std::iter::repeat_n(0.0, insert_samples));
            seg_start = cut;
        }
        let end = e.min(n);
        if end > seg_start {
            out.extend(wsola(&x[seg_start..end], rate, alpha));
        }
    }

    // The tail, after the last island.
    let last = isles[isles.len() - 1].1;
    if last < n {
        out.extend_from_slice(&x[last..]);
    }
    out
}

// --- The per-syllable envelope ---------------------------------------------

/// Apply the plan's gain and attack/release sculpting as one smooth
/// 100 Hz multiplier track, interpolated per sample.
fn apply_envelope(x: &[f64], rate: u32, plan: &Plan, analysis: &Analysis) -> Vec<f64> {
    let frames = &analysis.frames;
    let n = frames.len();
    if n == 0 {
        return x.to_vec();
    }
    let mut gain = vec![plan.gain_db; n];

    // Sculpt each sounding gesture's ramp onto its target.
    for env in &plan.envelopes {
        let s = (env.start_ms / 10) as usize;
        let e = ((env.end_ms / 10) as usize).min(n);
        if e <= s {
            continue;
        }
        let span = &frames[s..e];
        let peak = span
            .iter()
            .map(|f| f.rms_db())
            .fold(f64::NEG_INFINITY, f64::max);
        let base = span[0].rms_db();
        let end_level = span[span.len() - 1].rms_db();

        // Attack: reach the peak by attack_ms instead of the measured
        // ramp's own time — and only when the intent actually differs
        // from what was measured (a matched pair sculpts nothing).
        if env.attack_ms != env.measured_attack_ms {
            let a_m = (env.measured_attack_ms / 10).max(1) as usize;
            let a_t = (f64::from(env.attack_ms) / 10.0).max(1.0);
            for u in 0..a_m.min(span.len()) {
                let m = span[u].rms_db();
                let d = base + (peak - base) * ((u as f64 + 1.0) / a_t).min(1.0);
                gain[s + u] += (d - m).clamp(-12.0, 12.0);
            }
        }

        // Release: fall to the end level by release_ms, under the same
        // condition.
        if env.release_ms != env.measured_release_ms {
            let r_m = (env.measured_release_ms / 10).max(1) as usize;
            let r_t = (f64::from(env.release_ms) / 10.0).max(1.0);
            for u in 0..r_m.min(span.len()) {
                let i = span.len() - 1 - u;
                let m = span[i].rms_db();
                let d = peak + (end_level - peak) * ((u as f64 + 1.0) / r_t).min(1.0);
                gain[s + i] += (d - m).clamp(-12.0, 12.0);
            }
        }
    }

    // Median-of-3 smooth: no step the ear could hear as a click.
    let smooth: Vec<f64> = (0..n)
        .map(|i| {
            let (a, b, c) = (gain[i.saturating_sub(1)], gain[i], gain[(i + 1).min(n - 1)]);
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            if c < lo {
                lo
            } else if c > hi {
                hi
            } else {
                c
            }
        })
        .collect();

    // Interpolate per sample, in dB, and convert to a multiplier.
    let per_frame = f64::from(rate) / 100.0;
    x.iter()
        .enumerate()
        .map(|(i, &v)| {
            let pos = i as f64 / per_frame;
            let i0 = pos.floor() as usize;
            let frac = pos - i0 as f64;
            let g0 = smooth[i0.min(n - 1)];
            let g1 = smooth[(i0 + 1).min(n - 1)];
            let db = g0 + frac * (g1 - g0);
            v * exp2(db * LOG2_10 / 20.0)
        })
        .collect()
}

// --- Tilt, noise, formants -------------------------------------------------

/// The tilt shelf's total HF-vs-LF step per requested dB/oct, and the
/// pole ladder it is spread over. The analyzer's tilt dial is a
/// least-squares slope over FFT *bins* (linear in frequency, dense at
/// the top), so a single broad shelf barely moves it; spreading the same
/// total step over four staggered poles moves it about twice as far per
/// dB and stays monotone. The mapping was measured by the E10 sweep
/// (CONFORM.md): a requested `+d` dB/oct steps the spectrum `4*d` dB and
/// moves the measured dial by roughly `0.09*d` — the dial is a
/// deliberately compressed slope, so this channel is the weakest of the
/// set and CONFORM.md says so plainly.
const TILT_SPAN_OCT: f64 = 2.5;

/// The first-order tilt shelf: four one-pole splits at 250, 500, 1000
/// and 2000 Hz, each stepping ±D/8 dB, for a total HF-vs-LF step of D =
/// `db_oct * TILT_SPAN_OCT` dB spread across the band. Causal, one pole
/// per stage, no ringing; it can neither smear a transient nor touch a
/// phoneme's identity.
pub fn tilt_eq(x: &[f64], rate: u32, db_oct: f64) -> Vec<f64> {
    const POLES: [f64; 4] = [250.0, 500.0, 1000.0, 2000.0];
    let d_each = db_oct * TILT_SPAN_OCT / POLES.len() as f64;
    let g_h = exp2(d_each * 0.5 * LOG2_10 / 20.0);
    let g_l = exp2(-d_each * 0.5 * LOG2_10 / 20.0);
    let mut out = x.to_vec();
    for fc in POLES {
        // One-pole coefficient a = 1 - exp(-2 pi fc / rate), from the table.
        let a = 1.0 - exp2(-std::f64::consts::TAU * fc / f64::from(rate) * LOG2E);
        let mut lp = 0.0;
        out = out
            .iter()
            .map(|&v| {
                lp += a * (v - lp);
                g_l * lp + g_h * (v - lp)
            })
            .collect();
    }
    out
}

/// Mix white noise at a level-relative amplitude on the sounding frames.
/// The dial units map to a relative RMS level of units/80 (-12 dB at 40
/// units): the mapping that moves the analyzer's noise dial by about the
/// mixed amount, measured by the E10 sweep.
pub fn mix_noise(x: &[f64], frames: &[Frame], add_units: u8) -> Vec<f64> {
    if frames.is_empty() {
        return x.to_vec();
    }
    let mut rng = Rng(0x5678_9abc_def0_1234);
    let level = f64::from(add_units) / 100.0;
    // Frame each sample belongs to, proportionally (frames cover a hair
    // less than the whole clip; the last frame owns the tail).
    x.iter()
        .enumerate()
        .map(|(i, &v)| {
            let f = (i * frames.len() / x.len()).min(frames.len() - 1);
            if frames[f].flags.has(FrameFlags::REST) {
                return v;
            }
            let amp = exp2(frames[f].rms_db() * LOG2_10 / 20.0) * level;
            v + (rng.next_f64() * 2.0 - 1.0) * amp
        })
        .collect()
}

/// Catmull-Rom resampling to `new_len` samples: four taps, rational
/// arithmetic, no ringing worth a name at the small factors the formant
/// stage uses.
fn resample(x: &[f64], new_len: usize) -> Vec<f64> {
    if x.is_empty() || new_len == 0 {
        return vec![0.0; new_len];
    }
    if new_len == 1 {
        return vec![x[0]];
    }
    let scale = (x.len() - 1) as f64 / (new_len - 1) as f64;
    let at = |i: i64| -> f64 {
        if i < 0 {
            x[0]
        } else if i as usize >= x.len() {
            x[x.len() - 1]
        } else {
            x[i as usize]
        }
    };
    (0..new_len)
        .map(|i| {
            let pos = i as f64 * scale;
            let j = pos.floor() as i64;
            let u = pos - j as f64;
            let (p0, p1, p2, p3) = (at(j - 1), at(j), at(j + 1), at(j + 2));
            p1 + 0.5
                * u
                * (p2 - p0
                    + u * (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3 + u * (3.0 * (p1 - p2) + p3 - p0)))
        })
        .collect()
}

/// Shift the formants by `cents` with the pitch and duration held: the
/// resample moves everything (source and tract) by 2^(cents/1200), and
/// the PSOLA pass that restores the pitch and duration moves only the
/// source back — the classic source-filter separation, done on real
/// audio instead of a model.
pub fn formant_shift(x: &[f64], rate: u32, f0: &F0Track, cents: f64) -> Vec<f64> {
    let r = exp2(cents / 1200.0);
    let squeezed_len = ((x.len() as f64) / r).round() as usize;
    let squeezed = resample(x, squeezed_len.max(2));
    let f0_squeezed = f0.scaled(r);
    psola(&squeezed, rate, &f0_squeezed, &|_| -cents, &|_| r, x.len())
}

// ---------------------------------------------------------------------------
// The delivery-class reader (the E10 gate's measuring stick)
// ---------------------------------------------------------------------------

/// The delivery classes the E4/E5 vocabulary settled: calm, ordinary,
/// urgent. The class check reads the *same* analyzer the conformer
/// corrected toward, so the round trip is honest by construction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryClass {
    /// Slow, long, soft.
    Calm,
    /// The speaker's norm.
    Ordinary,
    /// Loud, fast, bright.
    Urgent,
}

/// How many measured channels the class reader uses.
pub const CLASS_FEATURES: usize = 4;

/// Nearest-centroid classing over standardized profile-relative channels
/// (the E4 finding: the dials discriminate only profile-relative). Fit by
/// [`fit_centroids`]; the per-channel scales are the training set's own
/// standard deviations, so one channel's units cannot dominate.
#[derive(Clone, Debug)]
pub struct Centroids {
    /// The per-class means, in standardized units.
    pub means: [([f64; CLASS_FEATURES], DeliveryClass); 3],
    /// The per-channel mean the training set was centered on.
    pub mean: [f64; CLASS_FEATURES],
    /// The per-channel standard deviations the features were divided by.
    pub scale: [f64; CLASS_FEATURES],
}

/// The profile-relative channels of one analysis: velocity, duration
/// ratio, contour travel, tilt. `baseline_ms` is the speaker's median
/// phrase length (the one dial the profile itself does not keep).
///
/// The syllable rate is deliberately *not* a channel. It reads the
/// analyzer's gesture count, and that count is a segmentation, not a
/// measurement of the speech: the E10 calibration heard the same words
/// in the same voice say "Hold the North Ridge." in the neutral and the
/// calm style, and the two renders differ by 43% in syllable count. A
/// reader built on it would class the segmentation's luck, and it is
/// not stable under the conformer's own edits either — the tempo
/// channel time-scales the line, and the re-cut moves the count in the
/// opposite direction to the one asked for. The duration ratio is the
/// rate the delivery actually moved by: the conformer's stretch moves
/// it exactly, and the corpus separates the classes on it.
pub fn class_features(
    analysis: &Analysis,
    profile: &SpeakerProfile,
    baseline_ms: u32,
) -> [f64; CLASS_FEATURES] {
    let m = measure(analysis, profile);
    let dur = if baseline_ms > 0 {
        f64::from(m.phrase.duration_ms) / f64::from(baseline_ms)
    } else {
        1.0
    };
    [
        f64::from(m.vel_db),
        dur,
        f64::from(m.contour_cents),
        f64::from(m.tilt_db_oct),
    ]
}

/// Fit the class centroids: the per-class means of the standardized
/// features. Deterministic — the same samples give the same centroids.
pub fn fit_centroids(samples: &[([f64; CLASS_FEATURES], DeliveryClass)]) -> Centroids {
    let n = samples.len().max(1) as f64;
    let mut mean = [0.0_f64; CLASS_FEATURES];
    for (f, _) in samples {
        for (i, &v) in f.iter().enumerate() {
            mean[i] += v / n;
        }
    }
    let mut var = [0.0_f64; CLASS_FEATURES];
    for (f, _) in samples {
        for (i, &v) in f.iter().enumerate() {
            var[i] += (v - mean[i]) * (v - mean[i]) / n;
        }
    }
    // A channel constant across the training set keeps scale 1: it adds
    // nothing to any distance (every member sits at 0 after its mean).
    let scale: [f64; CLASS_FEATURES] =
        std::array::from_fn(|i| if var[i] > 1e-12 { var[i].sqrt() } else { 1.0 });

    let mut means = [([0.0_f64; CLASS_FEATURES], DeliveryClass::Ordinary); 3];
    for (class, slot) in [
        (DeliveryClass::Calm, 0),
        (DeliveryClass::Ordinary, 1),
        (DeliveryClass::Urgent, 2),
    ] {
        let members: Vec<[f64; CLASS_FEATURES]> = samples
            .iter()
            .filter(|(_, c)| *c == class)
            .map(|(f, _)| *f)
            .collect();
        let mut m = [0.0_f64; CLASS_FEATURES];
        if !members.is_empty() {
            for f in &members {
                for (i, &v) in f.iter().enumerate() {
                    m[i] += v / members.len() as f64;
                }
            }
        }
        // Stored standardized, exactly as [`classify`] reads them.
        for i in 0..CLASS_FEATURES {
            m[i] = (m[i] - mean[i]) / scale[i];
        }
        means[slot] = (m, class);
    }
    Centroids { means, mean, scale }
}

/// Class one feature vector: the nearest standardized centroid.
pub fn classify(features: &[f64; CLASS_FEATURES], c: &Centroids) -> DeliveryClass {
    let dist = |m: &[f64; CLASS_FEATURES]| -> f64 {
        features
            .iter()
            .zip(c.mean)
            .zip(c.scale)
            .zip(m)
            .map(|(((&v, mu), s), &mm)| {
                let d = (v - mu) / s - mm;
                d * d
            })
            .sum()
    };
    c.means
        .iter()
        .min_by(|a, b| dist(&a.0).partial_cmp(&dist(&b.0)).unwrap())
        .map(|&(_, class)| class)
        .unwrap_or(DeliveryClass::Ordinary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::{analyze, RawMeasures};
    use crate::frame::Frame;
    use crate::math::sin_turn;

    /// An E1-style synthetic vowel: a 120 Hz impulse train through three
    /// resonators (Peterson & Barney's /a/: 730, 1090, 2440 Hz).
    fn synthetic_vowel(rate: u32, ms: usize, hz: f64) -> Vec<f64> {
        let n = ms * rate as usize / 1000;
        let mut x = vec![0.0_f64; n];
        let mut phase = 0.0;
        let period = f64::from(rate) / hz;
        for v in x.iter_mut() {
            phase += 1.0 / period;
            if phase >= 1.0 {
                phase -= 1.0;
                *v = 1.0;
            }
        }
        let resonance = |input: &[f64], freq: f64, bw: f64| -> Vec<f64> {
            let r = exp2(-std::f64::consts::TAU * bw / 2.0 / f64::from(rate) * LOG2E);
            let theta = std::f64::consts::TAU * freq / f64::from(rate);
            let c1 = 2.0 * r * cos_theta(theta);
            let c2 = r * r;
            let (mut y1, mut y2) = (0.0, 0.0);
            input
                .iter()
                .map(|&v| {
                    let y = v + c1 * y1 - c2 * y2;
                    y2 = y1;
                    y1 = y;
                    y
                })
                .collect()
        };
        let a = resonance(&x, 730.0, 70.0);
        let b = resonance(&a, 1090.0, 90.0);
        resonance(&b, 2440.0, 120.0)
    }

    /// cos of a radian angle, through the committed table.
    fn cos_theta(theta: f64) -> f64 {
        crate::math::cos_turn(theta / std::f64::consts::TAU)
    }

    /// Peak-normalized to `peak`, so the fixture sits where a neutral
    /// render sits against the default profile (vel near 0, rms far from
    /// both the rest floor and the top of the dial).
    fn to_pcm(x: &[f64], rate: u32, peak: f64) -> Pcm {
        let max = x
            .iter()
            .fold(0.0_f64, |a, v| if v.abs() > a { v.abs() } else { a });
        Pcm {
            samples: x
                .iter()
                .map(|&v| (v / max * peak * 32_767.0) as i16)
                .collect(),
            rate,
        }
    }

    fn median_f0(a: &Analysis) -> f64 {
        let mut hz: Vec<f64> = a
            .frames
            .iter()
            .filter(|f| f.flags.has(FrameFlags::VOICED) && f.f0_cents > 0)
            .map(|f| Frame::cents_to_hz(f64::from(f.f0_cents), a.profile.f0_ref()))
            .collect();
        hz.sort_by(|p, q| p.partial_cmp(q).unwrap());
        assert!(!hz.is_empty(), "the fixture must be voiced");
        hz[hz.len() / 2]
    }

    /// The analyzer's own formant estimate, in Hz: the LPC picker's raw
    /// measurement for one formant, median over the clip.
    ///
    /// Not the L0 field. The L0 field is the *wire* form of the same
    /// number, and it is not an instrument for these tests: the smoothing
    /// pass's formant track cannot move. Its hold band is 6 units and its
    /// slew 4, and the band is measured on the emission — a track that can
    /// travel at most 4 units a frame can never cross a 6-unit band, so
    /// outside a reset (an onset, a voicing flip, a rest) the emission
    /// stays on the first frame it was given, whose analysis window is
    /// truncated by the clip's edge. The field reads the edge, not the
    /// tract.
    ///
    /// The raw pick is the analyzer's measurement of the tract, and it is
    /// what the conformer's stages move — the same instrument the tilt
    /// test below already uses, for the same reason.
    fn median_raw_formant(a: &Analysis, pick: fn(&RawMeasures) -> f64) -> f64 {
        let mut hz: Vec<f64> = a.raw.iter().map(pick).filter(|v| *v > 0.0).collect();
        hz.sort_by(|p, q| p.partial_cmp(q).unwrap());
        assert!(!hz.is_empty(), "the fixture must have a formant pick");
        hz[hz.len() / 2]
    }

    /// The noise dial the analyzer measures on a voiced frame: the
    /// aperiodicity of the frame's YIN period, in L0 noise-dial units —
    /// the same mapping the analyzer's own quantization uses.
    ///
    /// Read off the raw measures for the reason `median_raw_formant`
    /// gives: the L0 noise field rides the same frozen track (an 8-unit
    /// band against a 4-unit slew), so it reads the clip's first frame
    /// rather than the noise in the audio.
    fn median_raw_noise(a: &Analysis) -> f64 {
        let mut v: Vec<f64> = a
            .raw
            .iter()
            .zip(a.frames.iter())
            .filter(|(_, f)| f.flags.has(FrameFlags::VOICED))
            .map(|(r, _)| (1.0 - r.periodicity).max(0.0) * 127.5)
            .collect();
        v.sort_by(|p, q| p.partial_cmp(q).unwrap());
        assert!(!v.is_empty(), "the fixture must be voiced");
        v[v.len() / 2]
    }

    /// Marks parse, and an unknown word names the vocabulary.
    #[test]
    fn marks_parse() {
        let marks = parse_marks("urgent, clipped ,falling").unwrap();
        assert_eq!(marks, vec![Mark::Urgent, Mark::Clipped, Mark::Falling]);
        let err = parse_marks("urgent,hurried").unwrap_err();
        assert!(err.contains("unknown mark"), "{err}");
        assert!(err.contains("calm"), "{err}");
        assert!(parse_marks("").is_err());
    }

    /// PSOLA shifts the pitch and keeps the formants and the duration.
    #[test]
    fn psola_shifts_pitch_keeps_formants() {
        let rate = 16_000;
        let x = synthetic_vowel(rate, 1200, 120.0);
        let pcm = to_pcm(&x, rate, 0.02);
        let a = analyze(&pcm);
        let f0 = F0Track::from_analysis(&a);
        let shifted = psola_pitch_shift(&x, rate, &f0, 200.0);

        let back = analyze(&to_pcm(&shifted, rate, 1.0));
        let want_hz = 120.0 * exp2(200.0 / 1200.0);
        let got = median_f0(&back);
        assert!(
            (got / want_hz - 1.0).abs() < 0.02,
            "f0 {got} Hz, want {want_hz} Hz"
        );
        assert_eq!(shifted.len(), x.len(), "duration must hold");
        for (pick, name) in [
            ((|r: &RawMeasures| r.f1_hz) as fn(&RawMeasures) -> f64, "F1"),
            ((|r: &RawMeasures| r.f2_hz) as fn(&RawMeasures) -> f64, "F2"),
            ((|r: &RawMeasures| r.f3_hz) as fn(&RawMeasures) -> f64, "F3"),
        ] {
            let before = median_raw_formant(&a, pick);
            let after = median_raw_formant(&back, pick);
            assert!(
                (after / before - 1.0).abs() < 0.06,
                "{name} moved {before} -> {after}"
            );
        }
    }

    /// WSOLA changes the duration and keeps the pitch.
    #[test]
    fn wsola_stretches_keeps_pitch() {
        let rate = 16_000;
        let x = synthetic_vowel(rate, 1000, 130.0);
        let stretched = wsola(&x, rate, 1.3);
        assert!(
            ((stretched.len() as f64 / x.len() as f64) - 1.3).abs() < 0.03,
            "length ratio {}",
            stretched.len() as f64 / x.len() as f64
        );
        let a = analyze(&to_pcm(&x, rate, 0.02));
        let b = analyze(&to_pcm(&stretched, rate, 1.0));
        let (fa, fb) = (median_f0(&a), median_f0(&b));
        assert!(1200.0 * log2(fb / fa).abs() < 20.0, "f0 {fa} -> {fb} Hz");
    }

    /// The formant stage moves the tract and restores the source.
    #[test]
    fn formant_shift_moves_tract_not_source() {
        let rate = 16_000;
        let x = synthetic_vowel(rate, 1200, 120.0);
        let pcm = to_pcm(&x, rate, 0.02);
        let a = analyze(&pcm);
        let f0 = F0Track::from_analysis(&a);
        let shifted = formant_shift(&x, rate, &f0, 100.0);

        assert_eq!(shifted.len(), x.len(), "duration must hold");
        let back = analyze(&to_pcm(&shifted, rate, 1.0));
        let f_before = median_f0(&a);
        let f_after = median_f0(&back);
        assert!(
            1200.0 * log2(f_after / f_before).abs() < 25.0,
            "f0 {f_before} -> {f_after} Hz"
        );
        let want = exp2(100.0 / 1200.0);
        for (pick, name) in [
            ((|r: &RawMeasures| r.f1_hz) as fn(&RawMeasures) -> f64, "F1"),
            ((|r: &RawMeasures| r.f2_hz) as fn(&RawMeasures) -> f64, "F2"),
        ] {
            let before = median_raw_formant(&a, pick);
            let after = median_raw_formant(&back, pick);
            assert!(
                (after / before / want - 1.0).abs() < 0.08,
                "{name} {before} -> {after} (want x{want})"
            );
        }
    }

    /// The tilt shelf moves the measured tilt by the asked slope.
    #[test]
    fn tilt_eq_moves_tilt() {
        let rate = 16_000;
        // Noise that already falls off (a pink-ish slope), so the shelf
        // has something to tilt against.
        let mut rng = Rng(99);
        let mut lp = 0.0;
        let x: Vec<f64> = (0..32_000)
            .map(|_| {
                let white = rng.next_f64() * 2.0 - 1.0;
                lp += 0.15 * (white - lp);
                lp + 0.2 * white
            })
            .collect();
        let a = analyze(&to_pcm(&x, rate, 0.02));
        let bright = tilt_eq(&x, rate, 8.0);
        let b = analyze(&to_pcm(&bright, rate, 0.02));
        // The dial's step-2 quantization keeps small shifts at 0, so the
        // raw slope is the honest measurement here (see CONFORM.md).
        let raw_med = |an: &Analysis| {
            let mut t: Vec<f64> = an.raw.iter().map(|r| r.tilt_db_oct).collect();
            t.sort_by(|p, q| p.partial_cmp(q).unwrap());
            t[t.len() / 2]
        };
        let (ta, tb) = (raw_med(&a), raw_med(&b));
        assert!(tb - ta >= 0.3, "raw tilt {ta} -> {tb} dB/oct");
    }

    /// The noise mix raises the measured noise dial.
    #[test]
    fn noise_mix_raises_the_dial() {
        let rate = 16_000;
        // The mix is level-relative against the *analyzed* frames, so it
        // runs on the same signal the analysis read.
        let pcm = to_pcm(&synthetic_vowel(rate, 1000, 120.0), rate, 0.02);
        let a = analyze(&pcm);
        let x: Vec<f64> = pcm
            .samples
            .iter()
            .map(|&s| f64::from(s) / 32_768.0)
            .collect();
        let noisy = mix_noise(&x, &a.frames, 40);
        let scaled = to_pcm(&noisy, rate, 1.0);
        let b = analyze(&scaled);
        // The dial is read off the analyzer's raw measures, not off the
        // L0 field: the field's noise track is frozen (an 8-unit hold band
        // against a 4-unit slew — see `median_raw_formant`), so it reports
        // the clip's first frame, +4 units here, whatever the audio holds.
        // The raw dial is the analyzer's own measurement of the noise, and
        // it moves by the amount the mix put in: 40 dial units of noise on
        // a voiced vowel reads as +8.6.
        let (da, db) = (median_raw_noise(&a), median_raw_noise(&b));
        assert!(db - da >= 8.0, "noise dial {da} -> {db} (want +8 or more)");
    }

    /// The envelope's gain shows up in the measured velocity.
    #[test]
    fn envelope_gain_measured() {
        let rate = 16_000;
        let x = synthetic_vowel(rate, 1000, 120.0);
        let pcm = to_pcm(&x, rate, 0.02);
        let a = analyze(&pcm);
        let m = measure(&a, &a.profile);
        let target = Target {
            vel_target_db: Some(m.vel_db + 6),
            ..Target::default()
        };
        let outcome = conform_with_target(&pcm, &target, &a.profile);
        let ConformOutcome::Conformed { audio, .. } = outcome else {
            panic!("a 6 dB gain is well inside the thresholds");
        };
        let back = measure(&analyze(&audio), &a.profile);
        assert!(
            (back.vel_db - m.vel_db - 6).abs() <= 2,
            "vel {} -> {} (want +6)",
            m.vel_db,
            back.vel_db
        );
    }

    /// The contour pass reshapes the measured travel.
    #[test]
    fn glide_reshapes_contour() {
        let rate = 16_000;
        // A rising line: 110 -> 160 Hz over the clip, through one
        // resonator so it is not a bare click train.
        let n = 16_000;
        let mut x = vec![0.0_f64; n];
        let mut phase = 0.0;
        for (i, v) in x.iter_mut().enumerate() {
            let hz = 110.0 + 50.0 * i as f64 / n as f64;
            phase += hz / f64::from(rate);
            if phase >= 1.0 {
                phase -= 1.0;
                *v = 1.0;
            }
        }
        let r = exp2(-std::f64::consts::TAU * 70.0 / 2.0 / f64::from(rate) * LOG2E);
        let theta = std::f64::consts::TAU * 730.0 / f64::from(rate);
        let (c1, c2) = (2.0 * r * cos_theta(theta), r * r);
        let (mut y1, mut y2) = (0.0, 0.0);
        let x: Vec<f64> = x
            .into_iter()
            .map(|v| {
                let y = v + c1 * y1 - c2 * y2;
                y2 = y1;
                y1 = y;
                y
            })
            .collect();

        let pcm = to_pcm(&x, rate, 0.02);
        let a = analyze(&pcm);
        let m = measure(&a, &a.profile);
        assert!(
            m.contour_cents > 300,
            "the fixture must rise: {}",
            m.contour_cents
        );

        // Flatten it.
        let target = Target {
            glide_cents: Some(0),
            ..Target::default()
        };
        let ConformOutcome::Conformed { audio, .. } =
            conform_with_target(&pcm, &target, &a.profile)
        else {
            panic!("flattening a 500-cent rise is inside the thresholds");
        };
        let back = measure(&analyze(&audio), &a.profile);
        assert!(
            back.contour_cents.abs() <= 150,
            "travel {} after flattening",
            back.contour_cents
        );
    }

    /// The tier rule: an impossible target is refused, with the channels
    /// named; a stacked mark set is refused where it stacks past a limit.
    #[test]
    fn tier_rule_reasks() {
        let rate = 16_000;
        let x = synthetic_vowel(rate, 1000, 120.0);
        let pcm = to_pcm(&x, rate, 0.02);
        let profile = SpeakerProfile::default();

        let extreme = Target {
            vel_target_db: Some(30),
            tempo_ratio: Some(2.0),
            ..Target::default()
        };
        match conform_with_target(&pcm, &extreme, &profile) {
            ConformOutcome::ReAsk(diff) => {
                let names: Vec<&str> = diff.channels.iter().map(|c| c.name).collect();
                assert!(names.contains(&"vel"), "{names:?}");
                assert!(names.contains(&"tempo"), "{names:?}");
            }
            ConformOutcome::Conformed { .. } => panic!("+30 dB and 2x must re-ask"),
        }

        // Marks that stack gain past the limit re-ask too.
        match conform_str(&pcm, "loud,loud,loud", &profile).unwrap() {
            ConformOutcome::ReAsk(diff) => {
                assert!(diff.reask_line().contains("vel"), "{}", diff.reask_line());
            }
            ConformOutcome::Conformed { .. } => panic!("+18 dB must re-ask"),
        }
    }

    /// The same input and marks give bit-identical output.
    #[test]
    fn conform_is_deterministic() {
        let rate = 16_000;
        let x = synthetic_vowel(rate, 800, 125.0);
        let pcm = to_pcm(&x, rate, 0.02);
        let profile = SpeakerProfile::default();
        // `clipped,falling` and not the brief's `urgent,clipped,falling`:
        // the fixture is a flat vowel and reads 0 dB on the velocity dial,
        // so the `urgent` class asks for +18 dB — past the tier rule's
        // 15 dB limit, which the rule refuses on purpose. The refusal is
        // deterministic too, and it is checked below.
        let one = conform_str(&pcm, "clipped,falling", &profile).unwrap();
        let two = conform_str(&pcm, "clipped,falling", &profile).unwrap();
        let bytes = |o: &ConformOutcome| match o {
            ConformOutcome::Conformed { audio, .. } => audio.to_wav_bytes(),
            ConformOutcome::ReAsk(_) => Vec::new(),
        };
        match (&one, &two) {
            (ConformOutcome::Conformed { .. }, ConformOutcome::Conformed { .. }) => {
                assert_eq!(bytes(&one), bytes(&two));
            }
            _ => panic!("the fixture must conform"),
        }

        // The class ask is refused, the same way, twice.
        let a = conform_str(&pcm, "urgent,clipped,falling", &profile).unwrap();
        let b = conform_str(&pcm, "urgent,clipped,falling", &profile).unwrap();
        match (&a, &b) {
            (ConformOutcome::ReAsk(x), ConformOutcome::ReAsk(y)) => {
                assert_eq!(x.reask_line(), y.reask_line());
                assert!(x.reask_line().contains("vel"), "{}", x.reask_line());
            }
            _ => panic!("+18 dB on a flat voice is past the tier window"),
        }
    }

    /// The scalar tempo marks resolve to their documented ratios (1.3x,
    /// 0.8x) — not stacked past the tier window — and so conform rather
    /// than re-ask.
    #[test]
    fn scalar_tempo_marks_conform() {
        let rate = 16_000;
        let x = synthetic_vowel(rate, 1000, 120.0);
        let pcm = to_pcm(&x, rate, 0.02);
        let profile = SpeakerProfile::default();
        let a = crate::analyzer::analyze(&pcm);
        let m = measure(&a, &profile);

        let fast = resolve_marks(&[Mark::Fast], &m);
        assert!(
            (fast.tempo_ratio.unwrap() - 1.30).abs() < 1e-9,
            "{:?}",
            fast.tempo_ratio
        );
        let slow = resolve_marks(&[Mark::Slow], &m);
        assert!(
            (slow.tempo_ratio.unwrap() - 0.80).abs() < 1e-9,
            "{:?}",
            slow.tempo_ratio
        );

        for marks in ["fast", "slow"] {
            match conform_str(&pcm, marks, &profile).unwrap() {
                ConformOutcome::Conformed { .. } => {}
                ConformOutcome::ReAsk(d) => panic!("{marks} must conform: {}", d.reask_line()),
            }
        }
    }

    /// The class reader separates what it was fitted on.
    #[test]
    fn classifier_separates() {
        let samples: Vec<([f64; CLASS_FEATURES], DeliveryClass)> = [
            ([2.0, 0.9, -400.0, 4.0], DeliveryClass::Urgent),
            ([3.0, 0.8, -300.0, 5.0], DeliveryClass::Urgent),
            ([0.0, 1.0, -500.0, 0.0], DeliveryClass::Ordinary),
            ([-1.0, 1.1, -450.0, -1.0], DeliveryClass::Ordinary),
            ([-3.0, 1.7, -350.0, -3.0], DeliveryClass::Calm),
            ([-2.0, 1.6, -300.0, -2.0], DeliveryClass::Calm),
        ]
        .into();
        let c = fit_centroids(&samples);
        assert_eq!(
            classify(&[2.5, 0.85, -350.0, 4.5], &c),
            DeliveryClass::Urgent
        );
        assert_eq!(
            classify(&[-2.5, 1.65, -320.0, -2.5], &c),
            DeliveryClass::Calm
        );
        assert_eq!(
            classify(&[0.0, 1.05, -480.0, 0.0], &c),
            DeliveryClass::Ordinary
        );
    }

    /// The PSOLA epoch walk locks onto a signal whose epochs are exact
    /// (the impulse train), which is the worst case for wandering.
    #[test]
    fn epochs_lock_on_impulse_train() {
        let rate = 16_000;
        let x = synthetic_vowel(rate, 500, 100.0);
        let pcm = to_pcm(&x, rate, 0.02);
        let a = analyze(&pcm);
        let f0 = F0Track::from_analysis(&a);
        let epochs = detect_epochs(&x, rate, &f0);
        assert!(epochs.len() > 40, "{} epochs in 0.5 s", epochs.len());
        for w in epochs.windows(2) {
            let d = (w[1].0 as i64 - w[0].0 as i64).abs();
            assert!(d > 100 && d < 220, "epoch spacing {d}");
        }
    }

    /// The epoch detector's windows stay inside the signal. A voiced
    /// island that runs to the clip's end made the island walk step past
    /// it, and the island's first peak-pick window then reached beyond
    /// the samples — an index-out-of-bounds the E10 arms hit on a line
    /// whose last voiced frame is the clip's last frame.
    #[test]
    fn epochs_stay_inside_the_signal() {
        let rate = 16_000;
        let n = 26624;
        let x: Vec<f64> = (0..n)
            .map(|i| if i % 80 == 0 { 1.0 } else { 0.0 })
            .collect();
        // The pitch track is per 10 ms frame. Only the last frame is
        // voiced, so the voiced island starts on the last frame-grid step
        // before the end — within one period of the signal's end.
        let frames = n / (rate as usize / 100) + 1;
        let hz: Vec<Option<f64>> = (0..frames)
            .map(|i| if i + 1 == frames { Some(200.0) } else { None })
            .collect();
        let f0 = F0Track { hz };
        let epochs = detect_epochs(&x, rate, &f0);
        assert!(
            epochs.iter().all(|(pos, _)| *pos < n),
            "an epoch sits past the signal: {epochs:?}"
        );
    }

    /// A tone survives the whole chain: no NaN, no runaway amplitude,
    /// sane length. The marks carry no velocity class — an absolute
    /// velocity target would pull any fixture onto the class's own level
    /// (a −13 dB faithful gain on this sine), which is the tier rule
    /// working, not the chain failing; here the tone keeps its level and
    /// the envelope, glide, tempo and tilt stages all run.
    #[test]
    fn chain_survives_a_tone() {
        let rate = 22_050;
        let x: Vec<f64> = (0..rate as usize)
            .map(|i| sin_turn(f64::from(i as u32) * 150.0 / f64::from(rate)))
            .collect();
        let pcm = to_pcm(&x, rate, 0.012);
        let outcome =
            conform_str(&pcm, "fast,clipped,falling", &SpeakerProfile::default()).unwrap();
        let ConformOutcome::Conformed { audio, .. } = outcome else {
            panic!("a tone should conform");
        };
        assert!(audio.samples.len() > rate as usize / 2);
        let peak = audio
            .samples
            .iter()
            .map(|s| i32::from(*s).abs())
            .max()
            .unwrap();
        assert!(peak <= 32_767 && peak > 100, "peak {peak}");
    }
}
