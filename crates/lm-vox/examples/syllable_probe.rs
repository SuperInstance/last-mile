//! The measurement that ruled the syllable rate out of the class reader.
//!
//! The class reader is a nearest-centroid classifier over four
//! profile-relative channels — velocity, duration ratio, contour travel,
//! tilt — and it deliberately does *not* read the syllable rate. This is
//! why: the same words, in the same voice, rendered in two styles, do
//! not carry the same syllable count, because the count is the analyzer's
//! gesture count and a gesture is a segmentation rather than a
//! measurement of the speech.
//!
//! This probe measures that on the cached calibration clips: for each of
//! the 20 lines x 3 voices it takes the Neutral and the Calm render of
//! the *same* line, computes the syllable count the analyzer reads for
//! each (rate x length, from the phrase summary), and reports the
//! paired ratio. The words are the same in both renders, so any spread
//! is the segmentation's.
//!
//!     cargo run -p lm-vox --release --example syllable_probe
//!
//! It needs no ASR and no skald: the calibration clips are cached in
//! `target/vox-e10/`.

use lm_vox::analyzer::analyze;
use lm_vox::conform::{self, Measured};
use lm_vox::profile::SpeakerProfile;
use lm_vox::wav::Pcm;

const VOICES: [&str; 3] = ["mia", "jason", "aria"];
const LINES: usize = 20;

/// The syllables the analyzer reads for one clip: its rate (the phrase's
/// tempo, in syllables per second) times its length.
fn syllables(m: &Measured) -> f64 {
    f64::from(m.phrase.tempo_tenths) / 10.0 * (f64::from(m.phrase.duration_ms) / 1000.0)
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

fn main() {
    let dir = std::path::Path::new("target/vox-e10");

    // Each voice's profile, fitted on its Neutral renders only.
    let mut profiles = Vec::new();
    for voice in VOICES {
        let mut profile = SpeakerProfile::default();
        for li in 1..=LINES {
            let Ok(pcm) = Pcm::from_wav_file(&dir.join(format!("{li:02}-{voice}-Neutral.wav")))
            else {
                continue;
            };
            let a = analyze(&pcm);
            let m = conform::measure(&a, &profile);
            profile.update(&a.frames, &m.gestures, &m.phrase);
        }
        profiles.push(profile);
    }

    println!("voice\tcalm/neutral syllable ratio\tmedian\tmean");
    let mut all = Vec::new();
    for (vi, voice) in VOICES.iter().enumerate() {
        let mut ratios = Vec::new();
        for li in 1..=LINES {
            let (Ok(n), Ok(c)) = (
                Pcm::from_wav_file(&dir.join(format!("{li:02}-{voice}-Neutral.wav"))),
                Pcm::from_wav_file(&dir.join(format!("{li:02}-{voice}-Calm.wav"))),
            ) else {
                continue;
            };
            let mn = conform::measure(&analyze(&n), &profiles[vi]);
            let mc = conform::measure(&analyze(&c), &profiles[vi]);
            let sn = syllables(&mn);
            let sc = syllables(&mc);
            ratios.push(sc / sn);
        }
        let mut sorted = ratios.clone();
        let med = median(&mut sorted);
        let mean = ratios.iter().sum::<f64>() / ratios.len().max(1) as f64;
        println!(
            "{}\t{:+.1} %\t{:.3}\t{:.3}",
            voice,
            100.0 * (med - 1.0),
            med,
            mean
        );
        all.extend(ratios);
    }
    let mut sorted = all.clone();
    let med = median(&mut sorted);
    let mean = all.iter().sum::<f64>() / all.len().max(1) as f64;
    println!(
        "all\t{:+.1} %\t{:.3}\t{:.3}\t({} paired clips)",
        100.0 * (med - 1.0),
        med,
        mean,
        all.len()
    );
}
