//! E1 (design §9): does the analyzer measure what it claims?
//!
//! Synthetic signals with known truth go in, the measured dials come out,
//! and the numbers meet the spec's tolerances: **F0 within ±10 cents,
//! formants within ±5%, onsets within ±10 ms**. The signals are exactly the
//! ones the design names: sine sweeps, glottal pulse trains through known
//! formant filters (/a/, /i/, /u/), noise bursts, amplitude envelopes with
//! known attack times.
//!
//! The generators below may use std's floats freely: they only *make* the
//! test signals. What must be deterministic across machines is the
//! *analyzer*, which is what the golden test pins; here the tolerances are
//! in cents, percent and milliseconds, far above any last-bit difference.

use lm_vox::analyzer::{analyze, analyze_with, AnalyzerConfig};
use lm_vox::frame::{Frame, FrameFlags, F0_REF_HZ};
use lm_vox::math::log2;
use lm_vox::profile::SpeakerProfile;
use lm_vox::wav::Pcm;

/// F0's tolerance in cents (design §9, E1).
const F0_TOLERANCE_CENTS: f64 = 10.0;

/// How many frames at each edge are left out of the readings: their
/// analysis windows are truncated by the clip's ends, and the design's
/// tolerance is about the analyzer, not about zero padding.
const EDGE_FRAMES: usize = 10;
/// The formants' tolerance, relative (design §9, E1).
const FORMANT_TOLERANCE: f64 = 0.05;
/// The onset detector's tolerance, in ms (design §9, E1).
const ONSET_TOLERANCE_MS: f64 = 10.0;

/// A glottal pulse train, idealized as an impulse train at `f0` — the
/// standard evaluation source for formant analysis: spectrally flat, so
/// what the analyzer measures is the vocal tract, not the source's own
/// rolloff and nulls.
fn pulse_train(n: usize, rate: u32, f0: f64, amp: f64) -> Vec<f64> {
    let period = rate as f64 / f0;
    let mut out = vec![0.0; n];
    let mut phase = 0.0;
    for v in out.iter_mut() {
        phase += 1.0 / period;
        if phase >= 1.0 {
            phase -= 1.0;
        }
        if phase < 1.0 / period {
            *v = amp;
        }
    }
    out
}

/// A two-pole resonator at `f` Hz with bandwidth `bw`, as in the synthesis
/// experiments the design's E1 describes: a known vocal tract.
fn resonate(x: &[f64], rate: f64, f: f64, bw: f64) -> Vec<f64> {
    let r = (-std::f64::consts::PI * bw / rate).exp();
    let c = -r * r;
    let b = 2.0 * r * (2.0 * std::f64::consts::PI * f / rate).cos();
    let a = 1.0 - b - c;
    let (mut y1, mut y2) = (0.0, 0.0);
    x.iter()
        .map(|&x| {
            let y = a * x + b * y1 + c * y2;
            y2 = y1;
            y1 = y;
            y
        })
        .collect()
}

/// The three vowels of the design's E1, with Peterson & Barney's measured
/// formants and the bandwidths a resonable tract gives them.
const VOWELS: &[(&str, [f64; 3], [f64; 3])] = &[
    ("/a/", [730.0, 1090.0, 2440.0], [80.0, 90.0, 120.0]),
    ("/i/", [270.0, 2290.0, 3010.0], [70.0, 100.0, 140.0]),
    ("/u/", [300.0, 870.0, 2240.0], [80.0, 90.0, 120.0]),
];

/// A vowel: a 120 Hz pulse train through the vowel's three resonators.
fn vowel_pcm(rate: u32, seconds: f64, formants: [f64; 3], bws: [f64; 3]) -> Pcm {
    let n = (rate as f64 * seconds) as usize;
    let train = pulse_train(n, rate, 120.0, 12_000.0);
    let mut x = train;
    for (i, f) in formants.iter().enumerate() {
        x = resonate(&x, f64::from(rate), *f, bws[i]);
    }
    // Scale to the headroom of 16-bit PCM: a clipped pulse train is not a
    // glottal pulse train, and clipping would put source nulls back into
    // the spectrum the way a shaped pulse does.
    let peak = x.iter().fold(0.0f64, |p, &v| p.max(v.abs())).max(1e-9);
    let scale = 0.75 * 32_767.0 / peak;
    Pcm {
        samples: x.iter().map(|&v| (v * scale) as i16).collect(),
        rate,
    }
}

/// F0 on a steady tone: within 10 cents at several pitches, at both rates.
#[test]
fn e1_f0_steady_tones() {
    for &rate in &[16_000u32, 22_050] {
        for &f0 in &[82.0, 110.0, 146.0, 220.0, 330.0] {
            let n = rate as usize * 2;
            let pcm = Pcm {
                samples: (0..n)
                    .map(|i| {
                        ((f64::from(i as u32) * f0 / f64::from(rate) * std::f64::consts::TAU).sin()
                            * 9_000.0) as i16
                    })
                    .collect(),
                rate,
            };
            let a = analyze(&pcm);
            let true_cents = 1200.0 * log2(f0 / F0_REF_HZ);
            let errs: Vec<f64> = a
                .raw
                .iter()
                .skip(EDGE_FRAMES)
                .take(a.raw.len().saturating_sub(2 * EDGE_FRAMES))
                .filter(|m| m.f0_hz > 0.0 && m.periodicity > 0.55)
                .map(|m| 1200.0 * log2(m.f0_hz / F0_REF_HZ) - true_cents)
                .collect();
            assert!(
                errs.len() > 100,
                "{f0} Hz at {rate}: {} voiced frames",
                errs.len()
            );
            let worst = errs.iter().fold(0.0_f64, |w, e| w.max(e.abs()));
            assert!(
                worst <= F0_TOLERANCE_CENTS,
                "{f0} Hz at {rate}: worst {worst} cents"
            );
        }
    }
}

/// F0 on a sweep: the tone moves an octave over four seconds and the
/// analyzer must follow it, frame by frame, within 10 cents.
#[test]
fn e1_f0_sweep() {
    for &rate in &[16_000u32, 22_050] {
        let seconds = 4.0;
        let n = (f64::from(rate) * seconds) as usize;
        let f_start = 110.0;
        let f_end = 220.0;
        let octaves = log2(f_end / f_start);
        let pcm = Pcm {
            samples: (0..n)
                .map(|i| {
                    let t = i as f64 / f64::from(rate);
                    let f_t = f_start * (2.0_f64).powf(octaves * t / seconds);
                    // Integrate the instantaneous frequency into a phase.
                    // For an exponential sweep the closed form is exact.
                    let phase = f_start * seconds / octaves / std::f64::consts::LN_2
                        * ((2.0_f64).powf(octaves * t / seconds) - 1.0);
                    let _ = f_t;
                    ((phase * std::f64::consts::TAU).sin() * 9_000.0) as i16
                })
                .collect(),
            rate,
        };
        let a = analyze(&pcm);
        let mut worst = 0.0_f64;
        let mut checked = 0;
        for (i, m) in a.raw.iter().enumerate() {
            if i < EDGE_FRAMES || i + EDGE_FRAMES >= a.raw.len() {
                continue;
            }
            if m.f0_hz <= 0.0 || m.periodicity <= 0.55 {
                continue;
            }
            let t = (i as f64 + 0.5) / 100.0;
            // The analyzer's YIN window is 40 ms; its estimate is the
            // period around the frame, so compare at the frame center.
            let f_t = f_start * (2.0_f64).powf(octaves * t / seconds);
            let err = (1200.0 * log2(m.f0_hz / f_t)).abs();
            worst = worst.max(err);
            checked += 1;
        }
        assert!(checked > 200, "{rate}: only {checked} voiced frames");
        assert!(
            worst <= F0_TOLERANCE_CENTS,
            "sweep at {rate}: worst {worst} cents over {checked} frames"
        );
    }
}

/// The formants of a pulse train through known resonators: within 5% on
/// /a/, /i/ and /u/, at both rates.
#[test]
fn e1_formants_of_vowels() {
    for &rate in &[16_000u32, 22_050] {
        for (name, formants, bws) in VOWELS {
            let pcm = vowel_pcm(rate, 1.5, *formants, *bws);
            let a = analyze(&pcm);
            // The measured formant of each frame, where all three were
            // found; the utterance's reading is the median of those.
            let med = |pick: fn(&lm_vox::analyzer::RawMeasures) -> f64| -> Option<f64> {
                let mut vals: Vec<f64> = a
                    .raw
                    .iter()
                    .filter(|m| {
                        m.periodicity > 0.55 && m.f1_hz > 0.0 && m.f2_hz > 0.0 && m.f3_hz > 0.0
                    })
                    .map(pick)
                    .collect();
                if vals.len() < 50 {
                    return None;
                }
                vals.sort_by(|x, y| x.partial_cmp(y).unwrap());
                Some(vals[vals.len() / 2])
            };
            let f1 = med(|m| m.f1_hz).expect("F1 measured");
            let f2 = med(|m| m.f2_hz).expect("F2 measured");
            let f3 = med(|m| m.f3_hz).expect("F3 measured");
            for (slot, measured) in [f1, f2, f3].iter().enumerate() {
                let truth = formants[slot];
                let err = (measured - truth).abs() / truth;
                assert!(
                    err <= FORMANT_TOLERANCE,
                    "{name} F{} at {rate}: {measured:.0} Hz vs {truth:.0} ({:.1}%)",
                    slot + 1,
                    err * 100.0
                );
            }
        }
    }
}

/// Onsets: a noise burst at a known time is flagged within ±10 ms, and a
/// second burst later is found too.
#[test]
fn e1_onsets_of_noise_bursts() {
    let rate = 22_050u32;
    let n = rate as usize * 3;
    let mut x = vec![0.0f64; n];
    // Two bursts: one at 500 ms, one at 1900 ms, each with a 5 ms attack.
    for &(start_ms, len_ms) in &[(500u32, 300u32), (1900, 300)] {
        let s = start_ms as usize * rate as usize / 1000;
        let l = len_ms as usize * rate as usize / 1000;
        let attack = 5 * rate as usize / 1000;
        let mut state = 0x1234_5678_9abc_def0u64;
        for i in 0..l {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let white = ((state >> 40) as i32 - 128) as f64 / 128.0;
            let ramp = if i < attack {
                i as f64 / attack as f64
            } else {
                1.0
            };
            x[s + i] = white * ramp * 8_000.0;
        }
    }
    let pcm = Pcm {
        samples: x.iter().map(|&v| v as i16).collect(),
        rate,
    };
    let a = analyze(&pcm);
    let onsets: Vec<usize> = a
        .frames
        .iter()
        .enumerate()
        .filter(|(_, f)| f.flags.has(FrameFlags::ONSET))
        .map(|(i, _)| i * 10)
        .collect();
    assert!(onsets.len() >= 2, "onsets found at {onsets:?} ms");
    for &truth_ms in &[500usize, 1900] {
        let nearest = onsets
            .iter()
            .copied()
            .min_by_key(|&ms| (ms as i64 - truth_ms as i64).abs())
            .unwrap();
        assert!(
            (nearest as i64 - truth_ms as i64).abs() <= ONSET_TOLERANCE_MS as i64,
            "burst at {truth_ms} ms flagged at {nearest} ms (onsets {onsets:?})"
        );
    }
}

/// An amplitude envelope with a known attack: the gesture's attack reads
/// back within a working tolerance, and the onset flag is on time.
#[test]
fn e1_attack_envelope() {
    let rate = 22_050u32;
    let n = rate as usize * 2;
    let attack_ms = 40.0f64;
    let start = 300usize * rate as usize / 1000;
    let mut x = vec![0.0f64; n];
    let mut phase = 0.0f64;
    for (i, v) in x.iter_mut().enumerate().skip(start) {
        let t = (i - start) as f64 / f64::from(rate);
        let env = (t / (attack_ms / 1000.0)).min(1.0);
        phase += 130.0 / f64::from(rate);
        if phase >= 1.0 {
            phase -= 1.0;
        }
        // A soft pulse, so the spectrum is voiced and periodic.
        let pulse = if phase < 0.25 {
            (std::f64::consts::TAU * phase / 0.25).sin() * 0.5
        } else {
            0.0
        };
        *v = pulse * env * 12_000.0;
    }
    let pcm = Pcm {
        samples: x.iter().map(|&v| v as i16).collect(),
        rate,
    };
    let a = analyze(&pcm);
    let onsets: Vec<usize> = a
        .frames
        .iter()
        .enumerate()
        .filter(|(_, f)| f.flags.has(FrameFlags::ONSET))
        .map(|(i, _)| i * 10)
        .collect();
    assert!(
        onsets
            .iter()
            .any(|&ms| (ms as i64 - 300).abs() <= ONSET_TOLERANCE_MS as i64),
        "onsets {onsets:?}"
    );
    let gestures = lm_vox::segment_gestures(&a.frames, &SpeakerProfile::default());
    let attack = gestures
        .iter()
        .filter(|g| g.velocity_db != i8::MIN)
        .map(|g| g.attack_ms)
        .min()
        .unwrap_or(600);
    // The 40 ms attack reads back within a frame either side, plus the
    // envelope's own rise.
    assert!(
        (i32::from(attack) - 40).abs() <= 40,
        "attack read as {attack} ms"
    );
}

/// The whole chain is exercised on a vowel: L0 frames come out voiced, and
/// quantized F0 stays inside the tolerance (the cents field is the wire
/// form).
#[test]
fn e1_quantized_frames_agree() {
    let pcm = vowel_pcm(22_050, 1.0, [730.0, 1090.0, 2440.0], [80.0, 90.0, 120.0]);
    let a = analyze_with(
        &pcm,
        &SpeakerProfile::default(),
        &AnalyzerConfig::new(22_050),
    );
    let voiced = a
        .frames
        .iter()
        .filter(|f| f.flags.has(FrameFlags::VOICED))
        .count();
    assert!(voiced > 50, "{voiced} voiced frames");
    let true_cents = 1200.0 * log2(120.0 / F0_REF_HZ);
    let worst = a
        .frames
        .iter()
        .filter(|f| f.flags.has(FrameFlags::VOICED))
        .map(|f| (f64::from(f.f0_cents) - true_cents).abs())
        .fold(0.0, f64::max);
    assert!(worst <= F0_TOLERANCE_CENTS, "worst {worst} cents");
}

/// A rest frame is a rest: silence in, silence out, at both rates.
#[test]
fn e1_silence() {
    for &rate in &[16_000u32, 22_050] {
        let pcm = Pcm {
            samples: vec![0; rate as usize],
            rate,
        };
        let a = analyze(&pcm);
        assert!(a
            .frames
            .iter()
            .all(|f: &Frame| f.flags.has(FrameFlags::REST)));
    }
}
