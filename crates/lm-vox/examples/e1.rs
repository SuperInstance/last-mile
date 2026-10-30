//! E1's report (design §9): the analyzer's measured errors on synthetic
//! signals with known truth, printed as TSV so VOX.md and the Python
//! reference analyzer (vox-eval) can quote and diff the same numbers.
//!
//! ```text
//! cargo run -p lm-vox --release --example e1
//! ```
//!
//! The tolerance columns are the design's E1 gates: F0 ±10 cents,
//! formants ±5%, onsets ±10 ms.

use lm_vox::analyzer::analyze;
use lm_vox::frame::{FrameFlags, F0_REF_HZ};
use lm_vox::math::log2;
use lm_vox::wav::Pcm;

/// F0's gate, in cents.
const F0_TOL: f64 = 10.0;
/// The formants' gate, relative.
const FORMANT_TOL: f64 = 0.05;
/// The onset detector's gate, in ms.
const ONSET_TOL: f64 = 10.0;

/// The glottal pulse train, idealized as impulses (see e1_synthetic.rs).
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

fn vowel(rate: u32, f: [f64; 3], bw: [f64; 3]) -> Pcm {
    let n = rate as usize * 2;
    let mut x = pulse_train(n, rate, 120.0, 12_000.0);
    for (i, &hz) in f.iter().enumerate() {
        x = resonate(&x, f64::from(rate), hz, bw[i]);
    }
    let peak = x.iter().fold(0.0f64, |p, &v| p.max(v.abs())).max(1e-9);
    let scale = 0.75 * 32_767.0 / peak;
    Pcm {
        samples: x.iter().map(|&v| (v * scale) as i16).collect(),
        rate,
    }
}

fn main() {
    println!("case\trate\ttruth\tmeasured\terror\ttolerance\tpass");

    // Steady tones across the voice range, at both rates.
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
            let mut errs: Vec<f64> = a
                .raw
                .iter()
                .skip(10)
                .take(a.raw.len().saturating_sub(20))
                .filter(|m| m.f0_hz > 0.0 && m.periodicity > 0.55)
                .map(|m| (1200.0 * log2(m.f0_hz / F0_REF_HZ) - true_cents).abs())
                .collect();
            errs.sort_by(|x, y| x.partial_cmp(y).unwrap());
            let worst = errs.last().copied().unwrap_or(f64::NAN);
            println!(
                "F0 steady {f0:.0} Hz\t{rate}\t{f0:.1} Hz\t{:.1} cents\t{worst:.2} cents\t{F0_TOL:.0} cents\t{}",
                worst,
                if worst <= F0_TOL { "PASS" } else { "FAIL" }
            );
        }
    }

    // A sweep: 110 to 220 Hz over four seconds.
    for &rate in &[16_000u32, 22_050] {
        let seconds = 4.0;
        let n = (f64::from(rate) * seconds) as usize;
        let octaves = log2(220.0 / 110.0);
        let pcm = Pcm {
            samples: (0..n)
                .map(|i| {
                    let t = i as f64 / f64::from(rate);
                    let phase = 110.0 * seconds / octaves / std::f64::consts::LN_2
                        * ((2.0_f64).powf(octaves * t / seconds) - 1.0);
                    ((phase * std::f64::consts::TAU).sin() * 9_000.0) as i16
                })
                .collect(),
            rate,
        };
        let a = analyze(&pcm);
        let mut worst = 0.0f64;
        for (i, m) in a.raw.iter().enumerate() {
            if i < 10 || i + 10 >= a.raw.len() {
                continue;
            }
            if m.f0_hz <= 0.0 || m.periodicity <= 0.55 {
                continue;
            }
            let t = (i as f64 + 0.5) / 100.0;
            let f_t = 110.0 * (2.0_f64).powf(octaves * t / seconds);
            worst = worst.max((1200.0 * log2(m.f0_hz / f_t)).abs());
        }
        println!(
            "F0 sweep 110-220 Hz\t{rate}\t110-220 Hz\t-\t{worst:.2} cents\t{F0_TOL:.0} cents\t{}",
            if worst <= F0_TOL { "PASS" } else { "FAIL" }
        );
    }

    // The vowels.
    const VOWELS: &[(&str, [f64; 3], [f64; 3])] = &[
        ("/a/", [730.0, 1090.0, 2440.0], [80.0, 90.0, 120.0]),
        ("/i/", [270.0, 2290.0, 3010.0], [70.0, 100.0, 140.0]),
        ("/u/", [300.0, 870.0, 2240.0], [80.0, 90.0, 120.0]),
    ];
    for &rate in &[16_000u32, 22_050] {
        for (name, f, bw) in VOWELS {
            let pcm = vowel(rate, *f, *bw);
            let a = analyze(&pcm);
            for (slot, formant) in [(0usize, ("F1", f[0])), (1, ("F2", f[1])), (2, ("F3", f[2]))] {
                let mut vals: Vec<f64> = a
                    .raw
                    .iter()
                    .skip(10)
                    .take(a.raw.len().saturating_sub(20))
                    .filter(|m| m.periodicity > 0.55)
                    .map(|m| match slot {
                        0 => m.f1_hz,
                        1 => m.f2_hz,
                        _ => m.f3_hz,
                    })
                    .filter(|&hz| hz > 0.0)
                    .collect();
                vals.sort_by(|x, y| x.partial_cmp(y).unwrap());
                if vals.is_empty() {
                    println!(
                        "{name} {}\t{rate}\t{:.0} Hz\tnot measured\t-\t5%\tFAIL",
                        formant.0, formant.1
                    );
                    continue;
                }
                let med = vals[vals.len() / 2];
                let err = (med - formant.1).abs() / formant.1;
                println!(
                    "{name} {}\t{rate}\t{:.0} Hz\t{med:.0} Hz\t{:.1}%\t5%\t{}",
                    formant.0,
                    formant.1,
                    err * 100.0,
                    if err <= FORMANT_TOL { "PASS" } else { "FAIL" }
                );
            }
        }
    }

    // Onsets: noise bursts at known times.
    for &rate in &[22_050u32] {
        let n = rate as usize * 3;
        let mut x = vec![0.0f64; n];
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
        for &truth_ms in &[500usize, 1900] {
            let nearest = onsets
                .iter()
                .copied()
                .min_by_key(|&ms| (ms as i64 - truth_ms as i64).abs())
                .unwrap_or(0);
            let err = (nearest as i64 - truth_ms as i64).abs();
            println!(
                "onset burst @{truth_ms} ms\t{rate}\t{truth_ms} ms\t{nearest} ms\t{err} ms\t{ONSET_TOL:.0} ms\t{}",
                if err as f64 <= ONSET_TOL { "PASS" } else { "FAIL" }
            );
        }
    }
}
