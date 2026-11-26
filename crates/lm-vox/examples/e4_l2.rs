//! The E4b L2-line check under the α-β-γ smoothing: build the 7-field
//! profile-relative L2 line (E4b's revised field set, docs/research/vox/
//! E4-E5.md §8.5) from lm-vox's own analysis of the 340 E4 clips, with
//! the smoothing off and with the default gains, so the d1-2B gate can be
//! asked the same urgency question on both and the answers compared with
//! the 90.6% the vox_ref line reached.
//!
//! ```text
//! cargo run -p lm-vox --release --example e4_l2 -- \
//!     --manifest evidence/vox-e4/manifest.csv \
//!     --clip-root ~/lm-evidence/vox-e4 \
//!     --off target/vox-e4/off --on target/vox-e4/on
//! ```
//!
//! The line's vocabulary and class thresholds mirror `tools/vox-ref`'s
//! `l2.py` (soft/normal/loud at ±3 dB, rise/level/fall at ±5% of the
//! speaker's pitch range, tone at ±10%) so a reader of the line — the
//! gate — reads the same words it was trained on. The quantities are
//! lm-vox's: velocity is the phrase's peak-against-median, brightness is
//! the spectral-tilt median against the speaker's own tilt median (vox_ref
//! uses the centroid; tilt is this analyzer's timbre axis).
//!
//! This is a measurement harness, not protocol code: floats are fine here
//! (the analysis underneath is deterministic and integer at the frame
//! layer).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lm_vox::analyzer::{analyze_with, Analysis, AnalyzerConfig};
use lm_vox::frame::{Frame, FrameFlags};
use lm_vox::profile::SpeakerProfile;
use lm_vox::smooth::SmoothConfig;
use lm_vox::wav::Pcm;

/// One manifest row.
struct Clip {
    file: String,
    speaker: String,
    pcm: Pcm,
}

fn main() {
    let mut manifest = PathBuf::from("evidence/vox-e4/manifest.csv");
    let mut clip_root: Option<PathBuf> = None;
    let mut off = PathBuf::from("target/vox-e4/off");
    let mut on = PathBuf::from("target/vox-e4/on");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--manifest" => manifest = PathBuf::from(args.next().expect("--manifest PATH")),
            "--clip-root" => clip_root = Some(PathBuf::from(args.next().expect("--clip-root DIR"))),
            "--off" => off = PathBuf::from(args.next().expect("--off DIR")),
            "--on" => on = PathBuf::from(args.next().expect("--on DIR")),
            _ => {
                eprintln!("unknown argument {arg}");
                std::process::exit(2);
            }
        }
    }

    let mut clips: Vec<Clip> = Vec::new();
    let text = std::fs::read_to_string(&manifest).expect("the manifest reads");
    for (li, line) in text.lines().enumerate() {
        if line.starts_with("file,") {
            continue;
        }
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() < 4 || cols[0].is_empty() {
            continue;
        }
        let (file, speaker) = (cols[0].to_string(), cols[1].to_string());
        if cols.len() >= 9 && !cols[8].is_empty() && cols[8] != "error" {
            eprintln!("skipping errored render {file}");
            continue;
        }
        if li % 40 == 0 {
            eprintln!("  {file}");
        }
        let path = match &clip_root {
            Some(root) => root.join(&file),
            None => PathBuf::from(&file),
        };
        let pcm = match Pcm::from_wav_file(&path) {
            Ok(p) => p,
            Err(err) => {
                eprintln!("cannot read {path:?}: {err}");
                std::process::exit(1);
            }
        };
        clips.push(Clip { file, speaker, pcm });
    }
    eprintln!("{} clips", clips.len());

    for (label, smooth, out_dir) in [
        ("off", SmoothConfig::off(), &off),
        ("on", SmoothConfig::default(), &on),
    ] {
        write_dataset(label, smooth, &clips, out_dir);
    }
}

/// Analyze every clip under `smooth`, fit one profile per speaker, rebuild
/// the lines against the fitted profiles, and write the dataset directory
/// the vox-e4 evaluation tooling reads (`l2.csv`).
fn write_dataset(label: &str, smooth: SmoothConfig, clips: &[Clip], out_dir: &Path) {
    let mut analyses: Vec<Analysis> = Vec::with_capacity(clips.len());
    for clip in clips {
        let mut cfg = AnalyzerConfig::new(clip.pcm.rate);
        cfg.smooth = smooth;
        analyses.push(analyze_with(&clip.pcm, &SpeakerProfile::default(), &cfg));
    }

    // Pass 1: fit each speaker's profile and brightness reference.
    let mut profiles: BTreeMap<String, SpeakerProfile> = BTreeMap::new();
    let mut tilts: BTreeMap<String, Vec<i32>> = BTreeMap::new();
    let mut peaks: BTreeMap<String, Vec<i32>> = BTreeMap::new();
    for (clip, a) in clips.iter().zip(&analyses) {
        let gestures = lm_vox::segment_gestures(&a.frames, &SpeakerProfile::default());
        let phrase = lm_vox::phrase_summary(&a.frames, &gestures, &SpeakerProfile::default());
        profiles
            .entry(clip.speaker.clone())
            .or_default()
            .update(&a.frames, &gestures, &phrase);
        tilts
            .entry(clip.speaker.clone())
            .or_default()
            .push(i32::from(phrase.bright_tilt_db_oct));
        peaks
            .entry(clip.speaker.clone())
            .or_default()
            .push(i32::from(a.frames.iter().map(|f| f.rms).max().unwrap_or(0)));
    }
    let median_of_i32 = |v: &Vec<i32>| -> f64 {
        let mut s = v.clone();
        s.sort_unstable();
        if s.is_empty() {
            0.0
        } else {
            f64::from(s[s.len() / 2])
        }
    };
    let tilt_median =
        |speaker: &str| -> f64 { median_of_i32(tilts.get(speaker).unwrap_or(&vec![])) };
    // The speaker's typical *peak*, the reference the velocity class is
    // read against (vox_ref's profile_level_peak_db): a delivery is loud
    // when its peak stands over this speaker's usual peak, not over the
    // median frame.
    let peak_median =
        |speaker: &str| -> f64 { median_of_i32(peaks.get(speaker).unwrap_or(&vec![])) };

    // Pass 2: the lines, against the fitted profiles.
    let mut rows = vec![
        "file,l2_abs,l2_rel,l2_v3,velocity_db,tilt_db,contour_norm,end_norm,dur_s,tempo"
            .to_string(),
    ];
    for (clip, a) in clips.iter().zip(&analyses) {
        let profile = profiles.get(&clip.speaker).cloned().unwrap_or_default();
        let gestures = lm_vox::segment_gestures(&a.frames, &profile);
        let phrase = lm_vox::phrase_summary(&a.frames, &gestures, &profile);
        let line = v3_line(
            a,
            &phrase,
            &profile,
            tilt_median(&clip.speaker),
            peak_median(&clip.speaker),
        );
        rows.push(format!(
            "{},{},{},{},{},{},{},{},{},{}",
            clip.file,
            "",
            "",
            line.line,
            line.velocity_db,
            line.tilt_db,
            line.contour_norm,
            line.end_norm,
            line.dur_s,
            line.tempo
        ));
    }

    std::fs::create_dir_all(out_dir).expect("the out dir creates");
    let out = out_dir.join("l2.csv");
    std::fs::write(&out, rows.join("\n") + "\n").expect("l2.csv writes");
    eprintln!("{label}: {} lines -> {}", rows.len() - 1, out.display());
}

/// The line's fields, plus the raw numbers behind them (for diffing the
/// two configurations clip by clip).
struct Line {
    line: String,
    velocity_db: f64,
    tilt_db: f64,
    contour_norm: f64,
    end_norm: f64,
    dur_s: f64,
    tempo: f64,
}

/// The 7-field profile-relative line, E4b's revised field set, from
/// lm-vox's measurements.
fn v3_line(
    a: &Analysis,
    phrase: &lm_vox::frame::Phrase,
    profile: &SpeakerProfile,
    tilt_ref: f64,
    peak_ref: f64,
) -> Line {
    let frames = &a.frames;

    // vel: this delivery's peak against the speaker's typical peak.
    let peak = f64::from(frames.iter().map(|f| f.rms).max().unwrap_or(0));
    let velocity_db = (peak - peak_ref) * (96.0 / 255.0);
    let vel_class = if velocity_db <= -3.0 {
        "soft"
    } else if velocity_db >= 3.0 {
        "loud"
    } else {
        "normal"
    };

    // bright: the tilt median against the speaker's own.
    let tilt_db = f64::from(phrase.bright_tilt_db_oct) - tilt_ref;
    let bright_class = if tilt_db <= -2.0 {
        "low"
    } else if tilt_db >= 2.0 {
        "high"
    } else {
        "mid"
    };

    // The voiced cents, and the speaker's range.
    let voiced: Vec<i32> = frames
        .iter()
        .filter(|f| f.flags.has(FrameFlags::VOICED) && f.f0_cents > 0)
        .map(|f| i32::from(f.f0_cents))
        .collect();
    let span = f64::from(profile.f0_span_cents).max(100.0);
    let median_of = |v: &[i32]| -> f64 {
        let mut s = v.to_vec();
        s.sort_unstable();
        if s.is_empty() {
            0.0
        } else {
            f64::from(s[s.len() / 2])
        }
    };

    // contour: the pitch movement over the delivery, in ranges.
    let third = (voiced.len() / 3).max(1);
    let contour_change = if voiced.len() >= 6 {
        median_of(&voiced[voiced.len() - third..]) - median_of(&voiced[..third])
    } else {
        0.0
    };
    let contour_norm = contour_change / span;

    // The final window: the last 250 ms of voiced speech. A clip with no
    // voiced frames reads as no movement and no end.
    let (rise, end_norm) = if voiced.is_empty() {
        (0.0, 0.0)
    } else {
        let window = 25.min(voiced.len());
        let final_window = &voiced[voiced.len() - window..];
        let w_third = (final_window.len() / 3).max(1).min(final_window.len());
        let rise = median_of(&final_window[final_window.len() - w_third..])
            - median_of(&final_window[..w_third]);
        let ended_at = median_of(&final_window[final_window.len() - w_third..]);
        let end_norm = (ended_at - f64::from(profile.median_f0_cents)) / span;
        (rise, end_norm)
    };

    let offset_class = |v: f64| -> &'static str {
        if v >= 0.05 {
            "rise"
        } else if v <= -0.05 {
            "fall"
        } else {
            "level"
        }
    };
    let tone = if rise / span >= 0.10 {
        "rise"
    } else if rise / span <= -0.10 {
        "fall"
    } else {
        "level"
    };

    let dur_s = f64::from(phrase.duration_ms) / 1000.0;
    let tempo = phrase.tempo_per_s();

    Line {
        line: format!(
            "vox: vel {vel_class} {velocity_db:+.0}dB | bright {bright_class} | tempo {tempo:.1}/s | dur {dur_s:.2}s | contour {} {contour_norm:+.2} | end {} {end_norm:+.2} | tone {tone}",
            offset_class(contour_norm),
            offset_class(end_norm),
        ),
        velocity_db,
        tilt_db,
        contour_norm,
        end_norm,
        dur_s,
        tempo,
    }
}

/// Silence an unused-import warning path for Frame (kept for the
/// doc-comment type references).
#[allow(dead_code)]
fn _frame_type(_: &Frame) {}
