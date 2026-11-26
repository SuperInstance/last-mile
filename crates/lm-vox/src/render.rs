//! The LPC/formant renderer (design §6): an L0 performance played back as
//! sound.
//!
//! This is the second rung of the render ladder — the robot voice that says
//! the right words in the right rhythm when the neural voice is late. It is
//! a classic source-filter synthesizer, the same physics the analysis
//! inverted:
//!
//! * the **source** is a Rosenberg glottal pulse train at the frames' F0
//!   for voicing, mixed with shaped noise by the frames' noise share;
//! * the **filter** is a cascade of three Klatt-style two-pole resonators
//!   at the frames' F1/F2/F3 — the vocal tract the analyzer measured;
//! * the **envelope** is the frames' own level, attack for attack.
//!
//! With word timing ([`crate::schedule`]) the vocal tract follows the
//! words' phone sketch instead — vowels at their formant targets, stop
//! closures silent, fricatives hissing — while pitch and level still come
//! from the performance. That is what E3 measures: the words survive the
//! round trip through the dials.
//!
//! A single global gain is fitted after synthesis so the render's level
//! matches the frames' own RMS scale — the relative dynamics are the
//! synth's, only the master fader is calibrated.

use crate::frame::{Frame, FrameFlags};
use crate::math::{cos_turn, exp2};
use crate::profile::SpeakerProfile;
use crate::schedule::{phone_sketch, PhoneKind, PhoneSeg, WordTiming};
use crate::wav::Pcm;

/// The resonator bandwidths the renderer gives the three formants, in Hz.
const F1_BW: f64 = 80.0;
const F2_BW: f64 = 90.0;
const F3_BW: f64 = 130.0;

/// The formants a frame falls back to when it measured none.
const DEFAULT_F: [f64; 3] = [500.0, 1500.0, 2500.0];

/// The renderer's knobs.
#[derive(Clone, Copy, Debug)]
pub struct RenderConfig {
    /// The output sample rate.
    pub sample_rate: u32,
    /// The open quotient of the glottal pulse: how much of each period the
    /// folds are open. 0.6 is a ordinary modal voice.
    pub open_quotient: f64,
}

impl RenderConfig {
    /// The stock config for an output rate.
    pub fn new(sample_rate: u32) -> RenderConfig {
        RenderConfig {
            sample_rate,
            open_quotient: 0.6,
        }
    }
}

/// Render an L0 performance as PCM, formants straight from the frames.
pub fn render_l0(frames: &[Frame], profile: &SpeakerProfile, cfg: &RenderConfig) -> Pcm {
    render(frames, profile, &[], cfg)
}

/// Render an L0 performance with word timing: the vocal tract follows the
/// words' phone sketch, the pitch and level follow the performance.
pub fn render_with_words(
    frames: &[Frame],
    profile: &SpeakerProfile,
    words: &[WordTiming],
    cfg: &RenderConfig,
) -> Pcm {
    render(frames, profile, words, cfg)
}

/// The synthesis parameters, interpolated per frame.
#[derive(Clone, Copy)]
struct Voice {
    /// F0 in Hz; 0.0 means unvoiced.
    f0: f64,
    /// The voicing gain, 0-1.
    voiced: f64,
    /// The noise gain, 0-1.
    noise: f64,
    /// Whether the noise is high-frequency weighted.
    noise_bright: bool,
    /// The amplitude, 0-1 (the frames' RMS scale).
    amp: f64,
    /// The formants, Hz.
    f: [f64; 3],
    /// The tilt shelf's target, dB per octave.
    tilt: f64,
}

impl Voice {
    fn from_frame(f: &Frame, profile: &SpeakerProfile) -> Voice {
        let voiced = f.flags.has(FrameFlags::VOICED);
        let f0 = f.f0_hz(profile.f0_ref()).unwrap_or(0.0);
        let formants = [
            Frame::formant_hz(f.f1).unwrap_or(DEFAULT_F[0]),
            Frame::formant_hz(f.f2).unwrap_or(DEFAULT_F[1]),
            Frame::formant_hz(f.f3).unwrap_or(DEFAULT_F[2]),
        ];
        Voice {
            f0,
            voiced: if voiced { 1.0 } else { 0.0 },
            noise: f.noise as f64 / 255.0,
            noise_bright: false,
            amp: exp2(f.rms_db() / 6.0206),
            f: formants,
            tilt: f64::from(f.tilt),
        }
    }

    fn from_frame_with_seg(f: &Frame, profile: &SpeakerProfile, seg: Option<&PhoneSeg>) -> Voice {
        let mut v = Voice::from_frame(f, profile);
        match seg.map(|s| s.kind) {
            Some(PhoneKind::Voiced { f1, f2, f3, amp }) => {
                v.f = [f64::from(f1), f64::from(f2), f64::from(f3)];
                v.voiced = 1.0;
                v.noise = 0.0;
                v.amp = v.amp * f64::from(amp) / 255.0 * 2.0;
            }
            Some(PhoneKind::Noise { level, bright }) => {
                v.voiced = 0.0;
                v.noise = f64::from(level) / 255.0 * 2.0;
                v.noise_bright = bright;
                v.f = DEFAULT_F;
            }
            Some(PhoneKind::Silent) => {
                v.voiced = 0.0;
                v.noise = 0.0;
                v.amp = 0.0;
            }
            None => {}
        }
        v
    }

    /// Linear blend between two voices (per-sample interpolation between
    /// frames).
    fn mix(&self, other: &Voice, t: f64) -> Voice {
        Voice {
            f0: self.f0 + (other.f0 - self.f0) * t,
            voiced: self.voiced + (other.voiced - self.voiced) * t,
            noise: self.noise + (other.noise - self.noise) * t,
            noise_bright: self.noise_bright || (t > 0.5 && other.noise_bright),
            amp: self.amp + (other.amp - self.amp) * t,
            f: [
                self.f[0] + (other.f[0] - self.f[0]) * t,
                self.f[1] + (other.f[1] - self.f[1]) * t,
                self.f[2] + (other.f[2] - self.f[2]) * t,
            ],
            tilt: self.tilt + (other.tilt - self.tilt) * t,
        }
    }
}

/// The renderer proper.
fn render(
    frames: &[Frame],
    profile: &SpeakerProfile,
    words: &[WordTiming],
    cfg: &RenderConfig,
) -> Pcm {
    let fs = f64::from(cfg.sample_rate);
    let n_samples = frames.len() * (cfg.sample_rate as usize) / 100;
    let segs = if words.is_empty() {
        Vec::new()
    } else {
        phone_sketch(words)
    };

    // The voices at each frame boundary, for interpolation.
    let voices: Vec<Voice> = frames
        .iter()
        .enumerate()
        .map(|(i, f)| {
            if words.is_empty() {
                Voice::from_frame(f, profile)
            } else {
                Voice::from_frame_with_seg(f, profile, seg_at(&segs, i))
            }
        })
        .collect();

    let mut samples: Vec<f64> = Vec::with_capacity(n_samples);
    let mut phase = 0.0f64;
    let mut noise_state = 0x2545_F491_4F6C_DD1Du64;
    let mut prev_shelf = 0.0f64;
    let mut prev_noise = 0.0f64;
    let mut r1 = [0.0f64; 2];
    let mut r2 = [0.0f64; 2];
    let mut r3 = [0.0f64; 2];
    let mut prev_radiation = 0.0f64;
    let mut dc_prev_in = 0.0f64;
    let mut dc_prev_out = 0.0f64;

    for n in 0..n_samples {
        let t_frames = n as f64 / fs * 100.0;
        let i = t_frames as usize;
        let alpha = t_frames - i as f64;
        let a = voices[i.min(voices.len() - 1)];
        let b = voices[(i + 1).min(voices.len() - 1)];
        let v = a.mix(&b, alpha);

        // The glottal source: a Rosenberg pulse train.
        let pulse = if v.f0 > 0.0 && v.voiced > 0.0 {
            phase += v.f0 / fs;
            if phase >= 1.0 {
                phase -= 1.0;
            }
            rosenberg(phase, cfg.open_quotient)
        } else {
            0.0
        };

        // The noise source, spectrally weighted when the sketch asks.
        noise_state = noise_state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let white = ((noise_state >> 33) as f64 / 2_147_483_648.0) - 1.0;
        let noise = if v.noise_bright {
            white - prev_noise
        } else {
            white
        };
        prev_noise = white;

        // Mix the source, then amplitude-envelope it.
        let excitation = (pulse * v.voiced + noise * v.noise) * v.amp;

        // The vocal tract: three resonators in cascade, retuned every
        // sample by the interpolated formants.
        let x1 = resonate(excitation, &mut r1, v.f[0], F1_BW, fs);
        let x2 = resonate(x1, &mut r2, v.f[1], F2_BW, fs);
        let x3 = resonate(x2, &mut r3, v.f[2], F3_BW, fs);

        // The spectral-tilt shelf: a first-order mix that tilts DC-to-
        // Nyquist by the frames' dB per octave, approximately.
        let k = exp2(v.tilt / 6.0206 / 2.0).clamp(0.125, 8.0);
        let p = (1.0 - k) / (1.0 + k);
        let shelf = x3 + p * prev_shelf;
        prev_shelf = x3;

        // Lip radiation (a differentiator) and a DC block.
        let radiated = shelf - prev_radiation;
        prev_radiation = shelf;
        let blocked = radiated - dc_prev_in + 0.995 * dc_prev_out;
        dc_prev_in = radiated;
        dc_prev_out = blocked;

        samples.push(blocked);
    }

    // One master fader, fitted so the render's level is the performance's:
    // the median dB gap between the frames' RMS and the render's, over the
    // sounding frames.
    let gain = fit_gain(&samples, frames, cfg);
    let out: Vec<i16> = samples
        .iter()
        .map(|&s| {
            let v = s * gain;
            let v = if v > 32_000.0 { 32_000.0 } else { v };
            let v = if v < -32_000.0 { -32_000.0 } else { v };
            v as i16
        })
        .collect();

    Pcm {
        samples: out,
        rate: cfg.sample_rate,
    }
}

/// The Rosenberg glottal pulse (Rosenberg 1971): a smooth rise and a
/// steeper fall over the open part of the period, silence over the closed
/// part. `t` is the phase in [0, 1).
fn rosenberg(t: f64, open_quotient: f64) -> f64 {
    let open = open_quotient;
    let tp = open * 0.4;
    let tn = open - tp;
    if t < tp {
        0.5 - 0.5 * cos_turn(t / (2.0 * tp))
    } else if t < open {
        cos_turn((t - tp) / (4.0 * tn))
    } else {
        0.0
    }
}

/// One Klatt resonator step: a two-pole IIR at `f` Hz with bandwidth `bw`,
/// unity gain at DC.
fn resonate(x: f64, state: &mut [f64; 2], f: f64, bw: f64, fs: f64) -> f64 {
    // The angles live in turns (cos_turn's units): one turn is 2 pi.
    let r = exp2(-crate::math::LOG2E * 0.5 * bw / fs);
    let c = -r * r;
    let b = 2.0 * r * cos_turn(f / fs);
    let a = 1.0 - b - c;
    let y = a * x + b * state[0] + c * state[1];
    state[1] = state[0];
    state[0] = y;
    y
}

/// The schedule segment active at a frame's center.
fn seg_at(segs: &[PhoneSeg], frame_index: usize) -> Option<&PhoneSeg> {
    let ms = frame_index as u32 * 10 + 5;
    segs.iter().find(|s| ms >= s.start_ms && ms < s.end_ms)
}

/// Fit the master gain: the median dB gap between the frames' own level
/// and the render's windowed RMS, over sounding frames.
fn fit_gain(samples: &[f64], frames: &[Frame], cfg: &RenderConfig) -> f64 {
    let win = cfg.sample_rate as usize / 100;
    let mut gaps: Vec<f64> = Vec::new();
    for (i, f) in frames.iter().enumerate() {
        if f.flags.has(FrameFlags::REST) {
            continue;
        }
        let start = i * win;
        let end = (start + win).min(samples.len());
        if end <= start {
            continue;
        }
        let energy: f64 = samples[start..end].iter().map(|&s| s * s).sum();
        if energy <= 0.0 {
            continue;
        }
        let rms = (energy / (end - start) as f64).sqrt();
        let rendered_db = 20.0 * crate::math::log10(rms / 32_768.0);
        gaps.push(f.rms_db() - rendered_db);
    }
    if gaps.is_empty() {
        return 1.0;
    }
    gaps.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = gaps[gaps.len() / 2];
    exp2(median / 6.0206)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rendered performance is finite PCM at the asked rate.
    #[test]
    fn render_makes_sound() {
        let frames: Vec<Frame> = (0..100)
            .map(|i| Frame {
                f0_cents: 1800 - i as i16 * 4,
                rms: 150,
                f1: Frame::formant_from_hz(730.0),
                f2: Frame::formant_from_hz(1090.0),
                f3: Frame::formant_from_hz(2440.0),
                tilt: 0,
                noise: 10,
                flags: FrameFlags::VOICED,
            })
            .collect();
        let pcm = render_l0(
            &frames,
            &SpeakerProfile::default(),
            &RenderConfig::new(22_050),
        );
        assert_eq!(pcm.rate, 22_050);
        assert_eq!(pcm.samples.len(), 22_050);
        assert!(pcm.samples.iter().any(|&s| s != 0));
        // And no NaN made it through.
        assert!(pcm.samples.iter().all(|&s| (s as i32).abs() <= 32_768));
    }

    /// Silence renders to silence.
    #[test]
    fn silence_renders_silent() {
        let pcm = render_l0(
            &[Frame::default(); 50],
            &SpeakerProfile::default(),
            &RenderConfig::new(16_000),
        );
        assert!(pcm.samples.iter().all(|&s| s == 0));
    }

    /// A rising pitch renders a rising pitch: analyze the render and check
    /// F0 follows the performance's slope.
    #[test]
    fn round_trip_keeps_the_slope() {
        let frames: Vec<Frame> = (0..120)
            .map(|i| Frame {
                f0_cents: 1200 + i as i16 * 10,
                rms: 150,
                f1: Frame::formant_from_hz(570.0),
                f2: Frame::formant_from_hz(840.0),
                f3: Frame::formant_from_hz(2410.0),
                tilt: 0,
                noise: 10,
                flags: FrameFlags::VOICED,
            })
            .collect();
        let pcm = render_l0(
            &frames,
            &SpeakerProfile::default(),
            &RenderConfig::new(16_000),
        );
        let back = crate::analyzer::analyze(&pcm);
        let voiced: Vec<&crate::analyzer::RawMeasures> = back
            .raw
            .iter()
            .filter(|m| m.f0_hz > 0.0 && m.periodicity > 0.6)
            .collect();
        assert!(voiced.len() > 40, "{} voiced frames", voiced.len());
        let first = voiced.iter().map(|m| m.f0_hz).fold(f64::MAX, f64::min);
        let last = voiced.iter().map(|m| m.f0_hz).fold(0.0, f64::max);
        assert!(last > first, "no rise: {first} to {last}");
        // The rise spans about an octave.
        assert!(last / first > 1.5, "{first} to {last}");
    }
}
