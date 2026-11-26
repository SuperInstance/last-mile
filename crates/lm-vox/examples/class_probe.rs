//! The delivery-class landing of each arm, without the ASR: the class
//! reader is the analyzer's own, so this measures the landing without
//! waiting on the ears. It is the cheap half of the E10 measurement —
//! `examples/e10.rs` does the same landing with the ASR's word accuracy
//! and the latency beside it — and it is what to re-run when a
//! conformer's dial targets change and the landing has to be re-checked
//! against the cached calibration clips.

use lm_vox::analyzer::analyze;
use lm_vox::conform::{
    self, classify, fit_centroids, ConformOutcome, DeliveryClass, CLASS_FEATURES,
};
use lm_vox::profile::SpeakerProfile;
use lm_vox::wav::Pcm;

const LINES: &[&str] = &[
    "Hold the north ridge.",
    "Two contacts, east side.",
    "I need cover, now.",
    "Reloading, give me a second.",
    "They are behind the crates.",
    "Move up when I say go.",
    "Sniper on the roof.",
    "Do you copy that?",
    "Falling back to the river.",
    "Watch your left flank.",
    "Medic down, medic down.",
    "We got company.",
    "Smoke going out.",
    "Take the point, quiet.",
    "Where is the rest of the squad?",
    "Enemy armor on the road.",
    "Stay low and follow me.",
    "That was too close.",
    "Regroup at the truck.",
    "Good effect on target.",
];
const VOICES: [&str; 3] = ["mia", "jason", "aria"];
const STYLES: [(&str, DeliveryClass); 3] = [
    ("Neutral", DeliveryClass::Ordinary),
    ("Calm", DeliveryClass::Calm),
    ("Angry", DeliveryClass::Urgent),
];

fn main() {
    let dir = std::path::Path::new("target/vox-e10");
    let mut clips: Vec<(usize, usize, usize, Pcm)> = Vec::new();
    for (li, _) in LINES.iter().enumerate() {
        for (vi, v) in VOICES.iter().enumerate() {
            for (si, (style, _)) in STYLES.iter().enumerate() {
                if let Ok(pcm) =
                    Pcm::from_wav_file(&dir.join(format!("{:02}-{}-{}.wav", li + 1, v, style)))
                {
                    clips.push((li, vi, si, pcm));
                }
            }
        }
    }
    println!("loaded {} clips", clips.len());

    let mut profiles = Vec::new();
    let mut baselines = Vec::new();
    for vi in 0..VOICES.len() {
        let mut profile = SpeakerProfile::default();
        let mut durations = Vec::new();
        for (_, _, _, pcm) in clips.iter().filter(|c| c.1 == vi && c.2 == 0) {
            let a = analyze(pcm);
            let m = conform::measure(&a, &profile);
            profile.update(&a.frames, &m.gestures, &m.phrase);
            durations.push(m.phrase.duration_ms);
        }
        durations.sort_unstable();
        baselines.push(durations[durations.len() / 2]);
        profiles.push(profile);
    }

    let samples: Vec<([f64; CLASS_FEATURES], DeliveryClass)> = clips
        .iter()
        .map(|(_, vi, si, pcm)| {
            let a = analyze(pcm);
            (
                conform::class_features(&a, &profiles[*vi], baselines[*vi]),
                STYLES[*si].1,
            )
        })
        .collect();
    let centroids = fit_centroids(&samples);
    let mut loo = 0;
    for (i, _) in samples.iter().enumerate() {
        let rest: Vec<_> = samples
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, s)| *s)
            .collect();
        if classify(&samples[i].0, &fit_centroids(&rest)) == samples[i].1 {
            loo += 1;
        }
    }
    println!("leave-one-out {}/{}", loo, samples.len());
    for (m, class) in centroids.means {
        println!(
            "  centroid {:?}: vel {:+.2} dur {:+.2} contour {:+.2} tilt {:+.2}",
            class, m[0], m[1], m[2], m[3]
        );
    }

    for (arm, marks, wanted) in [
        ("urgent ", "urgent,clipped", DeliveryClass::Urgent),
        ("example", "urgent,clipped,falling", DeliveryClass::Urgent),
        ("calm   ", "calm", DeliveryClass::Calm),
    ] {
        let (mut hit, mut n, mut reask) = (0usize, 0usize, 0usize);
        let mut misses: Vec<String> = Vec::new();
        for (li, vi, _, pcm) in clips.iter().filter(|c| c.2 == 0) {
            match conform::conform_str(pcm, marks, &profiles[*vi]).unwrap() {
                ConformOutcome::Conformed { audio, .. } => {
                    let a = analyze(&audio);
                    let f = conform::class_features(&a, &profiles[*vi], baselines[*vi]);
                    n += 1;
                    if classify(&f, &centroids) == wanted {
                        hit += 1;
                    } else {
                        misses.push(format!(
                            "{:02}-{}:{:?}(v{:.0} d{:.2} c{:+.0} t{:.1})",
                            li + 1,
                            VOICES[*vi],
                            classify(&f, &centroids),
                            f[0],
                            f[1],
                            f[2],
                            f[3]
                        ));
                    }
                }
                ConformOutcome::ReAsk(_) => reask += 1,
            }
        }
        println!(
            "arm {arm}: {hit}/{n} = {:.0}%  reask {reask}",
            100.0 * hit as f64 / n.max(1) as f64
        );
        if !misses.is_empty() {
            println!("  misses: {}", misses.join(" "));
        }
    }
}
