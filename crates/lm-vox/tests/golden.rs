//! The golden determinism test (design §8, D69): the L0/L1/L2 chain hashes
//! to the same bytes on every machine.
//!
//! The input is generated here from an integer LCG and the committed
//! trig/log tables — byte-identical on x86-64 and aarch64 by construction.
//! The analyzer itself uses only IEEE-754 `+ - * / sqrt` (every one of them
//! correctly rounded on both architectures), no libm transcendental, no
//! fused multiply-add, and every L0 field is quantized to an integer before
//! L1/L2 see it. So the chain's output, hashed with FNV-1a, is a machine
//! independent constant — and this test fails loudly if any of those
//! promises is ever broken by a refactor.
//!
//! The pinned hash below was produced on the aarch64 box (Jetson Thor);
//! an x86-64 run of this same test reads the same constant (D69's pattern:
//! only decisions travel between machines, and this is what makes the
//! analyzer's output a decision).

use lm_vox::encode::encode_stream;
use lm_vox::fnv64;
use lm_vox::math::{cos_turn, exp2};
use lm_vox::profile::SpeakerProfile;
use lm_vox::wav::Pcm;

/// The pinned hash of the whole chain over the golden utterance. If this
/// fails after an intentional analyzer change, re-pin it by running
/// `GOLDEN_PRINT=1 cargo test -p lm-vox --test golden -- --nocapture` and
/// pasting the printed hash — and say so in the commit.
///
/// Re-pinned 2026-10-29 for the α-β-γ smoothing pass (fleet 14z N5): the
/// smoothing changes L0's dials, so the chain hash moved; the input hash
/// below did not (the utterance is the same bytes). Re-pinned again the
/// same day when the level and tilt tracks lost their velocity state
/// (the E4 check caught their ramp overshoot inflating peaks) — the
/// smoothing defaults are parameters, and the hash follows them.
const PINNED: u64 = 0x71bd_c128_7ba9_ff0b;

/// The golden utterance, built from whole numbers: a pulse train with a
/// falling pitch contour, noise bursts, pauses, and a formant sweep —
/// enough performance to exercise every channel of L0, L1 and L2.
fn golden_pcm() -> Pcm {
    let rate = 22_050u32;
    let n = rate as usize * 3;
    let mut x = vec![0.0f64; n];

    // The deterministic noise source.
    let mut state = 0x0d_b1_a1_e5_c0_11_66_u64;
    let mut noise = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        ((state >> 33) as f64 / 4_294_967_296.0) - 0.5
    };

    // Three voiced islands with a falling F0 (1100 cents down), a formant
    // glide from /a/ toward /i/, separated by pauses; a noise burst opens
    // the last island.
    let islands = [(300_usize, 700), (1000, 400), (1600, 900)];
    for (idx, &(start_ms, len_ms)) in islands.iter().enumerate() {
        let s = start_ms * rate as usize / 1000;
        let l = len_ms * rate as usize / 1000;
        let mut phase = 0.0f64;
        for j in 0..l {
            let t = j as f64 / l as f64;
            // F0 from 145 Hz down to 102 Hz across the utterance.
            let cents = 1650.0 - 550.0 * (start_ms as f64 / 1000.0) - t * 150.0;
            let f0 = 55.0 * exp2(cents / 1200.0);
            phase += f0 / f64::from(rate);
            if phase >= 1.0 {
                phase -= 1.0;
            }
            // A Rosenberg pulse.
            let pulse = {
                let oq = 0.6;
                let tp = oq * 0.4;
                let tn = oq - tp;
                if phase < tp {
                    0.5 - 0.5 * cos_turn(phase / (2.0 * tp))
                } else if phase < oq {
                    cos_turn((phase - tp) / (4.0 * tn))
                } else {
                    0.0
                }
            };
            // The vocal tract sweeps F2 from 1100 to 2100 Hz.
            let f2 = 1100.0 + 1000.0 * t;
            let src = if idx == 2 && j < 30 * rate as usize / 1000 {
                pulse * 0.4 + noise() * 0.8
            } else {
                pulse + noise() * 0.02
            };
            let through = |y: &mut f64, y1: &mut f64, f: f64, bw: f64| {
                let r = exp2(-lm_vox::math::LOG2E * 0.5 * bw / f64::from(rate));
                let c = -r * r;
                let b = 2.0 * r * cos_turn(f / f64::from(rate));
                let a = 1.0 - b - c;
                let v = a * src + b * *y1 + c * *y;
                *y = *y1;
                *y1 = v;
                v
            };
            let (mut y1, mut y2) = (0.0f64, 0.0f64);
            let (mut z1, mut z2) = (0.0f64, 0.0f64);
            let v1 = through(&mut y1, &mut y2, 700.0, 80.0);
            let v2 = through(&mut z1, &mut z2, f2, 90.0);
            x[s + j] = v1 * 0.6 + v2 * 0.4;
        }
    }

    // Scale into 16-bit range, deterministically.
    let peak = x.iter().fold(0.0f64, |p, &v| p.max(v.abs())).max(1e-9);
    let scale = 0.7 * 32_767.0 / peak;
    Pcm {
        samples: x.iter().map(|&v| (v * scale) as i16).collect(),
        rate,
    }
}

#[test]
fn the_chain_hashes_to_the_pinned_constant() {
    let pcm = golden_pcm();

    // The input itself is pinned: same bytes everywhere.
    let input_hash = fnv64(&pcm.to_wav_bytes());

    let analysis = lm_vox::analyze(&pcm);
    let gestures = lm_vox::segment_gestures(&analysis.frames, &SpeakerProfile::default());
    let phrase = lm_vox::phrase_summary(&analysis.frames, &gestures, &SpeakerProfile::default());
    let line = lm_vox::l2_line(&phrase);
    let encoded = encode_stream(&analysis.frames);

    // Hash the whole chain: frames, gestures, the phrase's debug form, the
    // L2 line, and the encoded stream.
    let mut bytes = Vec::new();
    for f in &analysis.frames {
        bytes.extend_from_slice(&lm_vox::encode::frame_bytes(f));
    }
    for g in &gestures {
        bytes.extend_from_slice(format!("{g:?}").as_bytes());
    }
    bytes.extend_from_slice(format!("{phrase:?}").as_bytes());
    bytes.extend_from_slice(line.as_bytes());
    bytes.extend_from_slice(&encoded);
    let chain_hash = fnv64(&bytes);

    if std::env::var("GOLDEN_PRINT").is_ok() {
        println!("input_hash = {input_hash:#018x}");
        println!("chain_hash = {chain_hash:#018x}");
        println!("line = {line}");
    }

    assert_eq!(input_hash, PINNED_INPUT, "the golden utterance changed");
    assert_eq!(
        chain_hash, PINNED,
        "the analyzer chain drifted; if the change was intentional, \
         re-pin GOLDEN (and the input) with GOLDEN_PRINT=1 and say so in \
         the commit"
    );
}

/// The pinned hash of the golden utterance's WAV bytes.
const PINNED_INPUT: u64 = 0x9c04_7965_58fd_d7d2;

/// The chain is not empty: the golden utterance must produce real frames,
/// gestures and a line, or the test would pin nothing.
#[test]
fn the_golden_utterance_is_a_performance() {
    let pcm = golden_pcm();
    let analysis = lm_vox::analyze(&pcm);
    assert!(
        analysis.frames.len() > 250,
        "{} frames",
        analysis.frames.len()
    );
    let voiced = analysis
        .frames
        .iter()
        .filter(|f| f.flags.has(lm_vox::FrameFlags::VOICED))
        .count();
    assert!(voiced > 150, "{voiced} voiced frames");
    let gestures = lm_vox::segment_gestures(&analysis.frames, &SpeakerProfile::default());
    assert!(gestures.len() >= 3, "{} gestures", gestures.len());
    let phrase = lm_vox::phrase_summary(&analysis.frames, &gestures, &SpeakerProfile::default());
    let line = lm_vox::l2_line(&phrase);
    assert!(line.starts_with("vox: "), "{line}");
}
