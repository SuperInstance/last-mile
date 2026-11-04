//! The L0 analyzer: mono PCM in, 100 frames of performance per second out
//! (design §2, §4).
//!
//! Every estimator here is a published method, written from the papers, in
//! the plainest form that meets the E1 tolerances (design §9):
//!
//! * **Pitch** — YIN, de Cheveigné & Kawahara 2002: the difference function,
//!   the cumulative mean normalized difference, the absolute threshold, and
//!   parabolic interpolation of the chosen period.
//! * **Vocal-tract shape** — linear prediction by the autocorrelation
//!   method with the Levinson-Durbin recursion (Markel & Gray 1976, Makhoul
//!   1975), formants read off the LPC envelope by peak picking with
//!   parabolic interpolation.
//! * **Level** — window RMS on a dB scale.
//! * **Noise** — zero-crossing rate and spectral flatness (Wiener entropy:
//!   geometric mean over arithmetic mean of the power spectrum).
//! * **Brightness** — spectral tilt, the least-squares slope of log power
//!   against log frequency, in dB per octave.
//! * **Onsets** — spectral flux against a running median.
//!
//! The arithmetic is IEEE-754 `+ - * / sqrt` only, plus the committed
//! tables in [`crate::tables`] — no libm transcendental, no fused
//! multiply-add — so the same PCM gives the same frames on x86-64 and
//! aarch64 (design §8), and each frame is quantized to the protocol's
//! integer fields before anything downstream sees it.

use crate::fft::{fft_in_place, hamming, hann, power_spectrum};
use crate::frame::{Frame, FrameFlags, RMS_DB_MIN};
use crate::math::{exp2, log2};
use crate::profile::SpeakerProfile;
use crate::wav::Pcm;

/// The analysis knobs. The defaults are the ones the E1 numbers were
/// measured with; a different config is a different analyzer, and its
/// numbers must be re-measured before they are believed.
#[derive(Clone, Copy, Debug)]
pub struct AnalyzerConfig {
    /// The sample rate the PCM was taken at: 16 000 or 22 050.
    pub sample_rate: u32,
    /// The lowest pitch the analyzer will report, in Hz.
    pub f0_min_hz: f64,
    /// The highest pitch the analyzer will report, in Hz.
    pub f0_max_hz: f64,
    /// YIN's absolute threshold on the cumulative mean normalized
    /// difference: a period is accepted when the CMND drops below it.
    pub yin_threshold: f64,
    /// The LPC order. 24: enough poles to carry three formants and the
    /// pulse train's harmonic structure at once (the classic Atal-Hanauer
    /// sr/1000 + 4 lands at 20-26 for these rates), which is what the E1
    /// formant numbers were measured with.
    pub lpc_order: usize,
    /// The pre-emphasis factor applied before the LPC: 0.88, the middle
    /// ground the E1 sweep landed on — 0.97 biases a low F1 up by 10-20%
    /// (the classic /u/ failure) and below 0.8 the fit loses F3.
    pub pre_emphasis: f64,
}

impl AnalyzerConfig {
    /// The stock config for a sample rate.
    pub fn new(sample_rate: u32) -> AnalyzerConfig {
        AnalyzerConfig {
            sample_rate,
            f0_min_hz: 60.0,
            f0_max_hz: 500.0,
            yin_threshold: 0.15,
            lpc_order: 24,
            pre_emphasis: 0.88,
        }
    }

    /// The spectral window, in samples: 32 ms of signal.
    pub fn window(&self) -> usize {
        ((self.sample_rate / 1000) * 32) as usize
    }

    /// The FFT size the window is zero-padded into: the next power of two
    /// that leaves a bin under 16 Hz wide.
    pub fn fft_size(&self) -> usize {
        let mut n = 1024;
        while n < self.window() * 2 {
            n *= 2;
        }
        n
    }

    /// The YIN window, in samples: 40 ms, long enough to hold the longest
    /// period at `f0_min_hz` with room to compare against.
    pub fn yin_window(&self) -> usize {
        ((self.sample_rate / 1000) * 40) as usize
    }

    /// The formant window, in samples: 60 ms. Formants move more slowly
    /// than pitch or level, and the longer window is what holds three of
    /// them steady (the E1 numbers again).
    pub fn formant_window(&self) -> usize {
        ((self.sample_rate as f64 * 0.060) as usize).max(self.window())
    }
}

/// The floating-point measurements behind one frame, before quantization.
///
/// L0 keeps only what the protocol can carry; these keep the rest, for the
/// experiments (E1 asserts against them, E3 measures round-trip error on
/// them).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RawMeasures {
    /// The fundamental in Hz; 0.0 when no period passed the threshold.
    pub f0_hz: f64,
    /// YIN's periodicity, `1 - min(CMND)`: 1.0 is a pure period, 0.0 is
    /// anything but.
    pub periodicity: f64,
    /// The window's level in dBFS.
    pub rms_db: f64,
    /// Spectral flatness in [0, 1]: 1.0 is white noise, near 0 a pure tone.
    pub flatness: f64,
    /// Zero crossings per second.
    pub zcr_hz: f64,
    /// The first three formants in Hz; 0.0 where no peak was found.
    pub f1_hz: f64,
    pub f2_hz: f64,
    pub f3_hz: f64,
    /// Spectral tilt in dB per octave.
    pub tilt_db_oct: f64,
    /// Spectral flux: the half-wave-rectified frame-to-frame rise in
    /// magnitude, for onset detection.
    pub flux: f64,
}

/// The analysis result: the quantized frames and the raw measures behind
/// them, index for index.
#[derive(Clone, Debug, Default)]
pub struct Analysis {
    /// The L0 frames, at 100 Hz.
    pub frames: Vec<Frame>,
    /// The raw measurements behind each frame.
    pub raw: Vec<RawMeasures>,
    /// The profile the relative readings (cents) were taken against.
    pub profile: SpeakerProfile,
}

/// Analyze a clip with the stock profile.
pub fn analyze(pcm: &Pcm) -> Analysis {
    analyze_with(
        pcm,
        &SpeakerProfile::default(),
        &AnalyzerConfig::new(pcm.rate),
    )
}

/// Analyze a clip against a fitted profile.
pub fn analyze_with_profile(pcm: &Pcm, profile: &SpeakerProfile) -> Analysis {
    analyze_with(pcm, profile, &AnalyzerConfig::new(pcm.rate))
}

/// Analyze a clip with an explicit config.
pub fn analyze_with(pcm: &Pcm, profile: &SpeakerProfile, cfg: &AnalyzerConfig) -> Analysis {
    assert!(
        cfg.sample_rate == pcm.rate,
        "config rate {} does not match the clip's {}",
        cfg.sample_rate,
        pcm.rate
    );
    let n_frames = pcm.samples.len() * 100 / pcm.rate as usize;
    let samples: Vec<f64> = pcm.samples.iter().map(|&s| f64::from(s)).collect();

    let window = cfg.window();
    let half = window / 2;
    let win_hamming = hamming(window);
    let fft_size = cfg.fft_size();
    let tau_min = (f64::from(cfg.sample_rate) / cfg.f0_max_hz) as usize;
    let tau_max = (f64::from(cfg.sample_rate) / cfg.f0_min_hz) as usize + 1;

    let mut frames = Vec::with_capacity(n_frames);
    let mut raw = Vec::with_capacity(n_frames);
    let mut prev_mag: Option<Vec<f64>> = None;
    let mut prev_formants: (f64, f64, f64) = (0.0, 0.0, 0.0);

    for i in 0..n_frames {
        // The frame's center sample, at i * 10 ms, rounded.
        let center = (i * cfg.sample_rate as usize + 50) / 100;
        let start = center as i64 - half as i64;
        let frame: Vec<f64> = (0..window)
            .map(|j| {
                let idx = start + j as i64;
                if idx < 0 || idx as usize >= samples.len() {
                    0.0
                } else {
                    samples[idx as usize]
                }
            })
            .collect();

        let mut m = RawMeasures::default();

        // Level, on the whole rectangular window.
        let energy: f64 = frame.iter().map(|&x| x * x).sum();
        m.rms_db = if energy <= 0.0 {
            RMS_DB_MIN
        } else {
            let rms = (energy / window as f64).sqrt();
            20.0 * crate::math::log10(rms / 32_768.0)
        };

        if m.rms_db > RMS_DB_MIN + 21.0 {
            // Pitch: YIN over a longer buffer that starts at the frame.
            let yin_len = cfg.yin_window() + tau_max;
            let yin_buf: Vec<f64> = (0..yin_len)
                .map(|j| {
                    let idx = start + j as i64;
                    if idx < 0 || idx as usize >= samples.len() {
                        0.0
                    } else {
                        samples[idx as usize]
                    }
                })
                .collect();
            if let Some((period, periodicity)) = yin(&yin_buf, tau_min, tau_max, cfg.yin_threshold)
            {
                if period > 0.0 {
                    m.f0_hz = f64::from(cfg.sample_rate) / period;
                    m.periodicity = periodicity;
                }
            }

            // Spectrum: Hamming window, half-overlapping the same span.
            let windowed: Vec<f64> = frame
                .iter()
                .zip(&win_hamming)
                .map(|(&x, &w)| x * w)
                .collect();
            let power = power_spectrum(&windowed, fft_size);
            let mag: Vec<f64> = power.iter().map(|&p| p.sqrt()).collect();

            // Flatness: geometric mean over arithmetic mean, in log space.
            let band = &power[1..fft_size / 2];
            let log_sum: f64 = band.iter().map(|&p| log2(p + 1e-30)).sum();
            let arith: f64 = band.iter().sum::<f64>() / band.len() as f64;
            if arith > 0.0 {
                let geo = exp2(log_sum / band.len() as f64);
                m.flatness = geo / arith;
            }

            // Tilt: least squares of log2 power against log2 frequency,
            // 200 Hz to 5 kHz (or the Nyquist, whichever comes first).
            m.tilt_db_oct = tilt_db_per_octave(&power, f64::from(cfg.sample_rate), fft_size);

            // Zero-crossing rate.
            let mut crossings = 0usize;
            for j in 1..window {
                if (frame[j - 1] < 0.0) != (frame[j] < 0.0) {
                    crossings += 1;
                }
            }
            m.zcr_hz = crossings as f64 * (f64::from(cfg.sample_rate) / window as f64);

            // Flux: the half-wave-rectified rise against the last frame.
            if let Some(prev) = &prev_mag {
                m.flux = mag
                    .iter()
                    .zip(prev)
                    .map(|(&now, &before)| (now - before).max(0.0))
                    .sum();
            }
            prev_mag = Some(mag);

            // Formants: LPC of the longer, pre-emphasized, Hann-windowed
            // frame.
            let fw = cfg.formant_window();
            let fstart = center as i64 - (fw / 2) as i64;
            let fframe: Vec<f64> = (0..fw)
                .map(|j| {
                    let idx = fstart + j as i64;
                    if idx < 0 || idx as usize >= samples.len() {
                        0.0
                    } else {
                        samples[idx as usize]
                    }
                })
                .collect();
            let (f1, f2, f3) = formants(&fframe, cfg, prev_formants);
            // A missing formant carries the last one found: the tract
            // persists through a frame the picker could not read, and a
            // found-lost-found flicker is the picker's noise, not the
            // tract's movement.
            let carried = (
                if f1 > 0.0 { f1 } else { prev_formants.0 },
                if f2 > 0.0 { f2 } else { prev_formants.1 },
                if f3 > 0.0 { f3 } else { prev_formants.2 },
            );
            m.f1_hz = carried.0;
            m.f2_hz = carried.1;
            m.f3_hz = carried.2;
            prev_formants = carried;
        } else {
            // Silence: no pitch, no spectrum worth reading, and no vocal
            // tract to be continuous with.
            prev_mag = None;
            prev_formants = (0.0, 0.0, 0.0);
        }

        raw.push(m);
        frames.push(quantize(&m, profile));
    }

    // Onsets, as a pass over the flux channel: a frame is an onset when its
    // flux towers over the recent past and the level is rising.
    flag_onsets(&mut frames, &raw);

    // Smooth the voicing decision the way every pitch tracker does
    // (Talkin's RAPT: a median over the voicing bits before anything else
    // reads them). A neural render's periodicity straddles the threshold
    // and flips voicing for single frames in the middle of vowels, which
    // no voice does and no renderer can play.
    smooth_voicing(&mut frames);

    // Quantize the dials to their meaningful step. A neural render's noise
    // channel jitters +-20 units a frame and its formants +-8: motion no
    // ear hears and no renderer plays, but every unit of it forces the
    // running-status encoder out of its 4-bit deltas (E2's 300 B/s). The
    // steps (8 on noise, 2 on tilt, a 3-frame median on the formants) are
    // an order of magnitude inside E1's tolerances, and the pass is
    // integer-only, so the frames stay machine-exact.
    quantize_dials(&mut frames);

    Analysis {
        frames,
        raw,
        profile: profile.clone(),
    }
}

/// Median-of-5 on the voicing decision and on the pitch track — the
/// post-processing every pitch tracker applies to its raw estimates
/// (Talkin's RAPT again). The voicing bit stops flipping inside vowels,
/// and the cents stop jittering +-15 a frame, which is estimator noise a
/// renderer would play as a warble. The f0 field is carried across the
/// flips the median removes: an unvoiced frame holds no pitch, and a
/// voiced one inside the same gesture should not pay a whole-octave jump
/// for the estimator's indecision.
fn smooth_voicing(frames: &mut [Frame]) {
    if frames.len() < 5 {
        return;
    }
    let original: Vec<Frame> = frames.to_vec();
    let mut last_stable = 0i16;
    for i in 2..frames.len() - 2 {
        // The pitch median, over the neighborhood's voiced frames only.
        let mut cands: Vec<i16> = (0..5)
            .map(|k| original[i - 2 + k].f0_cents)
            .filter(|&c| c > 0)
            .collect();
        if !cands.is_empty() {
            cands.sort_unstable();
            let mut mid = cands[cands.len() / 2];
            // Octave errors: a period detector sometimes locks onto half
            // or twice the true period for a stretch. Pitch is continuous
            // at 10 ms; if the median sits an octave from the last stable
            // value, bring it back over -- the alternative is a 1200-cent
            // "glide" no voice made and a whole-frame packet on the wire
            // every time it flips.
            if last_stable > 0 {
                while mid - last_stable > 600 {
                    mid -= 1200;
                }
                while last_stable - mid > 600 {
                    mid += 1200;
                }
            }
            frames[i].f0_cents = mid;
            if mid > 0 {
                last_stable = mid;
            }
        }

        let voiced_votes = (0..5)
            .filter(|k| original[i - 2 + k].flags.has(FrameFlags::VOICED))
            .count();
        let voiced = voiced_votes >= 3;
        let was = frames[i].flags.has(FrameFlags::VOICED);
        if voiced != was {
            frames[i].flags.set(FrameFlags::VOICED, voiced);
            if !voiced {
                frames[i].f0_cents = 0;
            }
        }
    }
}

/// The dials' temporal quantization: medians over each dial's neighbors,
/// then a step grid on the dials whose frame-to-frame jitter carries no
/// meaning (noise to steps of 8, tilt to 2, formants to 2, which is 2.8% —
/// a third of E1's 5% tolerance). What is left moves at most a step or two
/// a frame, which is what the running-status encoder's 4-bit deltas were
/// sized for (§5).
fn quantize_dials(frames: &mut [Frame]) {
    if frames.len() < 3 {
        return;
    }
    let original: Vec<Frame> = frames.to_vec();
    for i in 1..frames.len() - 1 {
        for (field, key) in [
            (&mut frames[i].f1, 0usize),
            (&mut frames[i].f2, 1),
            (&mut frames[i].f3, 2),
            (&mut frames[i].rms, 3),
        ] {
            let get = |f: &Frame| match key {
                0 => f.f1,
                1 => f.f2,
                2 => f.f3,
                _ => f.rms,
            };
            let (a, b, c) = (
                get(&original[i - 1]),
                get(&original[i]),
                get(&original[i + 1]),
            );
            *field = max3(min3(a, b), min3(max3(a, b), c));
        }
        // The noise dial gets a 5-frame median: of all the channels it is
        // the one a neural render jitters hardest and the one no listener
        // follows at 10 ms resolution.
        if i >= 2 && i + 2 < frames.len() {
            let mut w = [
                original[i - 2].noise,
                original[i - 1].noise,
                original[i].noise,
                original[i + 1].noise,
                original[i + 2].noise,
            ];
            w.sort_unstable();
            frames[i].noise = w[2];
        }
    }
    for f in frames.iter_mut() {
        f.noise = ((f.noise + 4) / 8) * 8;
        let t = f.tilt;
        f.tilt = ((t as i32 + 1) / 2 * 2).clamp(-127, 127) as i8;
        f.f1 = f.f1.div_ceil(2) * 2;
        f.f2 = f.f2.div_ceil(2) * 2;
        f.f3 = f.f3.div_ceil(2) * 2;
    }
    // The noise dial's deadband: hold the last value until the quantized
    // one has moved a full 16 away. The noise channel is the jitteriest
    // and the least listened-to; a deadband is what lets its changes fit
    // the encoder's deltas at all.
    let mut held = frames.first().map(|f| f.noise).unwrap_or(0);
    for f in frames.iter_mut() {
        if (i16::from(f.noise) - i16::from(held)).abs() >= 16 {
            held = f.noise;
        } else {
            f.noise = held;
        }
    }
}

fn min3(a: u8, b: u8) -> u8 {
    if a < b {
        a
    } else {
        b
    }
}

fn max3(a: u8, b: u8) -> u8 {
    if a > b {
        a
    } else {
        b
    }
}

/// YIN (de Cheveigné & Kawahara 2002): the period of `x`, as
/// `(period in samples, periodicity)`, or `None` when nothing periodic
/// enough was found. The caller turns the period into Hz.
///
/// `x` holds `yin_window + tau_max` samples; the difference function
/// compares the first `x.len() - tau_max` samples against their shifts.
pub fn yin(x: &[f64], tau_min: usize, tau_max: usize, threshold: f64) -> Option<(f64, f64)> {
    let w = x.len() - tau_max;
    let mut d = vec![0.0; tau_max + 1];
    for tau in tau_min..=tau_max {
        let mut sum = 0.0;
        for j in 0..w {
            let diff = x[j] - x[j + tau];
            sum += diff * diff;
        }
        d[tau] = sum;
    }

    // Cumulative mean normalized difference.
    let mut cmnd = vec![1.0; tau_max + 1];
    let mut running = 0.0;
    for tau in 1..=tau_max {
        running += d[tau];
        cmnd[tau] = if running > 0.0 {
            d[tau] * tau as f64 / running
        } else {
            1.0
        };
    }

    // The absolute threshold: the first local minimum under it.
    let mut tau_star = 0usize;
    for (tau, &c) in cmnd.iter().enumerate().take(tau_max).skip(tau_min + 1) {
        if c < threshold && c <= cmnd[tau - 1] && c < cmnd[tau + 1] {
            tau_star = tau;
            break;
        }
    }
    if tau_star == 0 {
        // Nothing under the threshold: take the global minimum, and only
        // call it periodic if it is periodic enough to mean it.
        let (mut best_tau, mut best) = (tau_min, cmnd[tau_min]);
        for (tau, &c) in cmnd.iter().enumerate().skip(tau_min) {
            if c < best {
                best = c;
                best_tau = tau;
            }
        }
        if best > 0.3 {
            return None;
        }
        tau_star = best_tau;
    }

    // Parabolic interpolation around the chosen minimum, one lag inside
    // the search range on each side.
    tau_star = tau_star.clamp(tau_min + 1, tau_max - 1);
    let refined = parabolic_peak(cmnd[tau_star - 1], cmnd[tau_star], cmnd[tau_star + 1]);
    let tau = tau_star as f64 + refined;
    Some((tau, 1.0 - cmnd[tau_star]))
}

/// The interpolated offset of a peak's true position: three samples around
/// it, parabola through all three, answer in [-1, 1].
fn parabolic_peak(left: f64, mid: f64, right: f64) -> f64 {
    let denom = left - 2.0 * mid + right;
    if denom == 0.0 {
        0.0
    } else {
        0.5 * (left - right) / denom
    }
}

/// Linear prediction by the autocorrelation method with the
/// Levinson-Durbin recursion (Makhoul 1975, §IV).
///
/// Returns the predictor coefficients `a[1..=order]` such that the frame is
/// modeled by `x[n] ~= sum a[k] x[n-k]`, and the final prediction error, or
/// `None` when the frame carries no power to predict.
pub fn levinson_durbin(frame: &[f64], order: usize) -> Option<(Vec<f64>, f64)> {
    let n = frame.len();
    let mut r = vec![0.0; order + 1];
    for k in 0..=order {
        let mut sum = 0.0;
        for j in 0..n - k {
            sum += frame[j] * frame[j + k];
        }
        r[k] = sum;
    }
    if r[0] <= 0.0 {
        return None;
    }

    // The white-noise correction factor: a relative floor on r[0], the
    // standard LPC regularizer (Makhoul 1975 discusses the joint noise
    // model; every production LPC carries one). Without it, a window of a
    // perfectly periodic vowel is almost exactly predictable, the Toeplitz
    // system goes near-singular, and the solver's last rounding digits
    // decide where the poles land -- an envelope no two implementations
    // would agree on. The floor bounds the poles' radii; at -40 dB it
    // moves a real signal's formants far less than a quantization step.
    r[0] *= 1.0 + 1.0e-4;

    let mut a = vec![0.0; order + 1];
    let mut e = r[0];
    for i in 1..=order {
        // The reflection coefficient.
        let mut acc = r[i];
        for j in 1..i {
            acc -= a[j] * r[i - j];
        }
        let k = acc / e;
        let mut new_a = a.clone();
        new_a[i] = k;
        for j in 1..i {
            new_a[j] = a[j] - k * a[i - j];
        }
        a = new_a;
        e *= 1.0 - k * k;
        // A reflection over 1 means the recursion left the stable region
        // (or produced NaN): the frame is not predictable, not a vocal
        // tract.
        if !e.is_finite() || e <= 0.0 {
            return None;
        }
    }
    Some((a, e))
}

/// The first three formants of a frame, by LPC and peak picking.
///
/// The frame is pre-emphasized (0.97, the standard speech front end that
/// flattens the spectral slope so the formants stand out), Hann-windowed,
/// and modeled; the envelope `1/|A(w)|^2` is evaluated through an FFT and
/// its local maxima, refined by parabolic interpolation, are read as
/// formants.
///
/// Peak picking rather than polynomial roots is a choice, and it is made
/// on purpose: root finding must sort and pair complex roots, clamp
/// bandwidths and reject pairs by region — a garden of edge cases whose
/// outcome depends on tiny numerical differences — while peak picking is a
/// scan of a committed envelope whose behavior is transparent. On clean
/// vowels the two agree; where they differ, the peak reading is the
/// honest one about what the envelope says.
pub fn formants(frame: &[f64], cfg: &AnalyzerConfig, prev: (f64, f64, f64)) -> (f64, f64, f64) {
    let pre_emph = cfg.pre_emphasis;
    let emph: Vec<f64> = frame
        .iter()
        .zip(std::iter::once(0.0).chain(frame.iter().copied()))
        .map(|(&x, prev)| x - pre_emph * prev)
        .collect();

    // The LPC runs at the full rate over the 60 ms window: 24 poles carry
    // three formants plus the pulse train's harmonic structure, which is
    // what keeps the third formant from being swallowed by the first two
    // (the classic Atal-Hanauer order).
    let band_win = hann(emph.len());
    let windowed: Vec<f64> = emph.iter().zip(&band_win).map(|(&x, &w)| x * w).collect();

    let Some((a, _)) = levinson_durbin(&windowed, cfg.lpc_order) else {
        return (0.0, 0.0, 0.0);
    };

    // The envelope: FFT of [1, -a1, -a2, ...] — the denominator polynomial
    // A(z) = 1 - sum a[k] z^-k — read as -log2|A|^2, which is log2 of the
    // power envelope 1/|A|^2, so formants are its peaks. The transform can
    // stay at the full size: the envelope is smooth, zero padding only
    // refines the grid the parabola sits on.
    let fft_size = cfg.fft_size();
    let mut re = vec![0.0; fft_size];
    let mut im = vec![0.0; fft_size];
    re[0] = 1.0;
    for (k, &ak) in a.iter().enumerate().skip(1) {
        re[k] = -ak;
    }
    fft_in_place(&mut re, &mut im, false);

    let bins = fft_size / 2;
    let mut env = vec![0.0; bins + 1];
    for k in 0..=bins {
        let mag2 = re[k] * re[k] + im[k] * im[k];
        env[k] = if mag2 > 1e-30 { -log2(mag2) } else { 60.0 };
    }
    let env_max = env
        .iter()
        .cloned()
        .fold(0.0_f64, |a, b| if b > a { b } else { a });
    // Peaks more than 72 dB (12 log2 units) under the tallest carry no
    // formant a renderer wants. The floor is deep on purpose: an /a/'s F3
    // can sit half a hundred dB under F1 in the fitted envelope and still
    // be exactly where the vowel keeps it; the region and gap rules below
    // are what keep spurious dips out, not this threshold.
    let peak_floor = env_max - 12.0;

    let hz_per_bin = f64::from(cfg.sample_rate) / fft_size as f64;
    let mut peaks: Vec<f64> = Vec::new();
    for k in 1..bins {
        if env[k] > env[k - 1] && env[k] >= env[k + 1] && env[k] > peak_floor {
            let delta = parabolic_peak(env[k - 1], env[k], env[k + 1]);
            peaks.push((k as f64 + delta) * hz_per_bin);
        }
    }

    // Pick F1, F2, F3: inside each region, at least 200 Hz apart, and —
    // when the last frame found one — the candidate nearest it. Formants
    // move at tens of Hz per 10 ms; a picker that jumps between candidate
    // peaks frame by frame is not measuring the tract, it is measuring its
    // own indecision.
    let mut picked: [f64; 3] = [0.0; 3];
    let regions = [
        (180.0, 1200.0, 0.0),
        (600.0, 3200.0, 200.0),
        (1700.0, 4500.0, 250.0),
    ];
    let mut floor_hz = 0.0;
    for (slot, &(lo, hi, gap)) in regions.iter().enumerate() {
        let in_region: Vec<f64> = peaks
            .iter()
            .copied()
            .filter(|&p| p >= lo && p <= hi && p >= floor_hz + gap)
            .collect();
        if in_region.is_empty() {
            continue;
        }
        let prev_hz = match slot {
            0 => prev.0,
            1 => prev.1,
            _ => prev.2,
        };
        let choice = if prev_hz > 0.0 {
            in_region
                .iter()
                .copied()
                .min_by(|a, b| {
                    (a - prev_hz)
                        .abs()
                        .partial_cmp(&(b - prev_hz).abs())
                        .unwrap()
                })
                .unwrap()
        } else {
            in_region[0]
        };
        picked[slot] = choice;
        floor_hz = choice;
    }
    (picked[0], picked[1], picked[2])
}

/// Spectral tilt in dB per octave: the least-squares slope of log2 power
/// against log2 frequency over 200 Hz to 5 kHz.
fn tilt_db_per_octave(power: &[f64], sample_rate: f64, fft_size: usize) -> f64 {
    let hz_per_bin = sample_rate / fft_size as f64;
    let lo_hz = 200.0;
    let hi_hz = (5000.0f64).min(sample_rate / 2.0 - hz_per_bin);
    let lo_bin = (lo_hz / hz_per_bin).ceil() as usize;
    let hi_bin = (hi_hz / hz_per_bin).floor() as usize;
    if hi_bin <= lo_bin + 1 {
        return 0.0;
    }
    let log_lo = log2(lo_bin as f64 * hz_per_bin);
    let log_step = log2((lo_bin + 1) as f64 * hz_per_bin) - log_lo;

    let mut n = 0.0;
    let mut sx = 0.0;
    let mut sy = 0.0;
    let mut sxx = 0.0;
    let mut sxy = 0.0;
    for (i, &p) in power.iter().enumerate().take(hi_bin + 1).skip(lo_bin) {
        let x = (i - lo_bin) as f64 * log_step;
        let y = log2(p + 1e-30);
        n += 1.0;
        sx += x;
        sy += y;
        sxx += x * x;
        sxy += x * y;
    }
    let denom = n * sxx - sx * sx;
    if denom == 0.0 {
        return 0.0;
    }
    // slope in log2 units per octave; 6.0206 dB is a doubling of power.
    (n * sxy - sx * sy) / denom * 6.020_599_913_279_624
}

/// Quantize one frame's raw measures into the protocol's integers.
fn quantize(m: &RawMeasures, profile: &SpeakerProfile) -> Frame {
    let mut flags = FrameFlags::default();
    let voiced = m.f0_hz > 0.0 && m.periodicity > 0.55;
    let f0_cents = if voiced {
        let cents = Frame::hz_to_cents(m.f0_hz, profile.f0_ref());
        cents as i16
    } else {
        0
    };
    if voiced {
        flags.set(FrameFlags::VOICED, true);
    }
    if m.rms_db <= RMS_DB_MIN + 3.0 {
        flags.set(FrameFlags::REST, true);
    }

    let noise = if voiced {
        let aperiodic = 1.0 - m.periodicity;
        ((aperiodic.max(0.0) * 255.0) * 0.5 + 0.5) as u8
    } else if m.rms_db > RMS_DB_MIN + 3.0 {
        // Unvoiced but not silent: the flatter the spectrum, the noisier.
        (64.0 + m.flatness * 191.0 + 0.5) as u8
    } else {
        0
    };

    let tilt = {
        let t = m.tilt_db_oct;
        let t = if t > 127.0 { 127.0 } else { t };
        let t = if t < -127.0 { -127.0 } else { t };
        (t + 0.5) as i8
    };

    Frame {
        f0_cents,
        rms: Frame::rms_from_db(m.rms_db),
        f1: if m.f1_hz > 0.0 {
            Frame::formant_from_hz(m.f1_hz)
        } else {
            0
        },
        f2: if m.f2_hz > 0.0 {
            Frame::formant_from_hz(m.f2_hz)
        } else {
            0
        },
        f3: if m.f3_hz > 0.0 {
            Frame::formant_from_hz(m.f3_hz)
        } else {
            0
        },
        tilt,
        noise,
        flags,
    }
}

/// Flag onsets: a frame whose flux towers over the running median of the
/// previous two seconds and whose level is rising — and any frame where
/// sound starts out of silence, which is an onset by definition.
fn flag_onsets(frames: &mut [Frame], raw: &[RawMeasures]) {
    let mut last_onset: i64 = -10;
    for i in 0..frames.len() {
        let mut onset = false;

        // Sound out of silence is an onset even with no flux history to
        // compare against (the very first syllable of an utterance).
        if i > 0
            && frames[i - 1].flags.has(FrameFlags::REST)
            && !frames[i].flags.has(FrameFlags::REST)
        {
            onset = true;
        }

        if !onset && raw[i].flux > 0.0 {
            let past = i.saturating_sub(200)..i;
            let mut history: Vec<f64> = past.map(|j| raw[j].flux).collect();
            if history.len() >= 10 {
                history.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let median = history[history.len() / 2];
                let rising = raw[i].rms_db > raw[i - 1].rms_db;
                if raw[i].flux > 2.5 * median.max(1e-12) && rising {
                    onset = true;
                }
            }
        }

        if onset && (i as i64 - last_onset) >= 3 {
            frames[i].flags.set(FrameFlags::ONSET, true);
            last_onset = i as i64;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pure tone's RMS reads as expected.
    #[test]
    fn rms_of_a_tone() {
        let cfg = AnalyzerConfig::new(16_000);
        let pcm = Pcm {
            samples: (0..16_000)
                .map(|i| ((f64::from(i) * 0.05 * std::f64::consts::TAU).sin() * 10_000.0) as i16)
                .collect(),
            rate: 16_000,
        };
        let a = analyze_with(&pcm, &SpeakerProfile::default(), &cfg);
        let mid = &a.raw[a.raw.len() / 2];
        // A 10000-amplitude sine has RMS 10000/sqrt(2), about -13.3 dBFS.
        assert!((mid.rms_db + 13.3).abs() < 0.5, "{} dB", mid.rms_db);
    }

    /// Levinson-Durbin recovers the coefficients of a known resonator.
    #[test]
    fn levinson_finds_a_resonance() {
        // Drive a one-pole-ish process and check the predictor places its
        // weight on the recent past.
        let mut x = vec![0.0; 2000];
        let mut state = 0.0_f64;
        for (i, v) in x.iter_mut().enumerate() {
            let noise = ((i * 1103515245 + 12345) as i64 % 2001 - 1000) as f64 / 1000.0;
            state = 0.9 * state + noise;
            *v = state;
        }
        let (a, e) = levinson_durbin(&x, 4).unwrap();
        assert!(e > 0.0 && e < x.iter().map(|v| v * v).sum::<f64>());
        assert!(a[1].abs() > 0.5, "a1 should carry the resonance: {a:?}");
    }

    /// Silence produces rest frames and nothing else.
    #[test]
    fn silence_is_rest() {
        let pcm = Pcm {
            samples: vec![0; 16_000],
            rate: 16_000,
        };
        let a = analyze(&pcm);
        assert!(a
            .frames
            .iter()
            .all(|f| f.flags.has(FrameFlags::REST) && f.rms == 0));
    }
}
