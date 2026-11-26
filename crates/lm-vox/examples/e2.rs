//! E2's before/after harness (design §9, and the smoothing brief): the
//! encoded cost of the 20 skald lines with the α-β-γ smoothing off and
//! with the default gains, on the same renders, plus the packet mix that
//! says where the bytes go.
//!
//! ```text
//! cargo run -p lm-vox --release --example e2 -- [--keep DIR]
//! ```
//!
//! Riva's renders are not deterministic (E4-E5 §1.2: the same line twice
//! is two deliveries), so both columns are measured on the same audio:
//! the first run renders the lines once and keeps them in `--keep DIR`;
//! later runs reuse them. skald must be up at :8000.
//!
//! The "before" configuration is [`lm_vox::SmoothConfig::off`] — the
//! pre-smoothing analyzer exactly, all its other passes (voicing median,
//! octave hysteresis, dial steps, noise deadband) still on.

use std::io::Read;

use lm_vox::analyzer::{analyze_with, AnalyzerConfig};
use lm_vox::encode::{decode_stream, encode_stream};
use lm_vox::frame::Frame;
use lm_vox::profile::SpeakerProfile;
use lm_vox::smooth::SmoothConfig;
use lm_vox::wav::Pcm;

/// The twenty lines and the three voices, exactly the experiments harness
/// uses (examples/experiments.rs): the E2 number this reports is the same
/// population VOX.md's 498 B/s came from.
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

const VOICES: &[&str] = &["mia", "jason", "aria"];

/// One wire-packet census: how many of each opcode class, and their bytes.
#[derive(Default, Clone, Copy)]
struct Mix {
    rest: usize,
    rest_bytes: usize,
    full: usize,
    full_bytes: usize,
    repeat: usize,
    repeat_bytes: usize,
    flags: usize,
    flags_bytes: usize,
    delta8: usize,
    delta8_bytes: usize,
    delta4: usize,
    delta4_bytes: usize,
}

/// Walk an encoded stream and census its packets, the way the decoder
/// reads them.
fn packet_mix(bytes: &[u8]) -> Mix {
    let mut m = Mix::default();
    let mut i = 0usize;
    while i < bytes.len() {
        let op = bytes[i];
        i += 1;
        match op {
            0x00..=0x0F => {
                m.rest += 1;
                m.rest_bytes += 1;
            }
            0x10 => {
                m.full += 1;
                m.full_bytes += 10;
                i += 9;
            }
            0x11..=0x1E => {
                m.repeat += 1;
                m.repeat_bytes += 1;
            }
            0x1F => {
                m.flags += 1;
                m.flags_bytes += 2;
                i += 1;
            }
            0x20 => {
                let n = 2 + bytes[i].count_ones() as usize;
                m.delta8 += 1;
                m.delta8_bytes += n;
                i += n - 1;
            }
            0x80..=0xFF => {
                let n = (op & 0x7f).count_ones() as usize;
                let n = 1 + n.div_ceil(2);
                m.delta4 += 1;
                m.delta4_bytes += n;
                i += n - 1;
            }
            _ => {}
        }
    }
    m
}

/// Measure one clip under one smoothing configuration.
struct Reading {
    frames: Vec<Frame>,
    bps: f64,
    mix: Mix,
}

fn measure(pcm: &Pcm, smooth: SmoothConfig) -> Reading {
    let mut cfg = AnalyzerConfig::new(pcm.rate);
    cfg.smooth = smooth;
    let analysis = analyze_with(pcm, &SpeakerProfile::default(), &cfg);
    let encoded = encode_stream(&analysis.frames);
    // The wire is exact: decode(encode(x)) == x, or the census is of a
    // stream that does not exist.
    assert_eq!(decode_stream(&encoded).unwrap(), analysis.frames);
    let bps = encoded.len() as f64 * 100.0 / analysis.frames.len() as f64;
    Reading {
        frames: analysis.frames,
        bps,
        mix: packet_mix(&encoded),
    }
}

fn speak(agent: &ureq::Agent, line: &str, voice: &str) -> Result<Vec<u8>, String> {
    let body = format!("{{\"input\":{line:?},\"voice\":\"{voice}\"}}");
    let response = agent
        .post("http://localhost:8000/v1/audio/speech")
        .header("Content-Type", "application/json")
        .send(&body)
        .map_err(|e| e.to_string())?;
    let mut wav = Vec::new();
    response
        .into_body()
        .into_reader()
        .read_to_end(&mut wav)
        .map_err(|e| e.to_string())?;
    Ok(wav)
}

fn main() {
    let keep = std::env::args()
        .position(|a| a == "--keep")
        .and_then(|p| std::env::args().nth(p + 1))
        .unwrap_or_else(|| "target/vox-e2".to_string());
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(180)))
            .build(),
    );

    // Render (or reuse) the twenty lines.
    let mut clips: Vec<(String, Pcm)> = Vec::new();
    for (li, line) in LINES.iter().enumerate() {
        let voice = VOICES[li % VOICES.len()];
        let path = std::path::Path::new(&keep).join(format!("{li:02}-{voice}.wav"));
        let pcm = if path.exists() {
            match Pcm::from_wav_file(&path) {
                Ok(p) => p,
                Err(err) => {
                    eprintln!("cached {li:02}: {err}");
                    std::process::exit(1);
                }
            }
        } else {
            let wav = match speak(&agent, line, voice) {
                Ok(w) => w,
                Err(err) => {
                    eprintln!("skald failed on {line:?}/{voice}: {err}");
                    std::process::exit(1);
                }
            };
            let pcm = match Pcm::from_wav_bytes(&wav) {
                Ok(p) => p,
                Err(err) => {
                    eprintln!("skald's WAV on {line:?}: {err}");
                    std::process::exit(1);
                }
            };
            let _ = std::fs::create_dir_all(&keep);
            let _ = pcm.to_wav_file(&path);
            pcm
        };
        clips.push((format!("{li:02}-{voice}"), pcm));
    }

    println!("clip\tframes\tBs_off\tBs_on\tfull_off\tfull_on\td8_off\td8_on\td4_off\td4_on\trep_off\trep_on\trest_off\trest_on");
    let mut mean_off = 0.0;
    let mut mean_on = 0.0;
    let mut tot: [usize; 12] = [0; 12];
    let mut joined_off: Vec<Frame> = Vec::new();
    let mut joined_on: Vec<Frame> = Vec::new();
    for (name, pcm) in &clips {
        let off = measure(pcm, SmoothConfig::off());
        let on = measure(pcm, SmoothConfig::default());
        mean_off += off.bps;
        mean_on += on.bps;
        let row = [
            off.mix.full,
            on.mix.full,
            off.mix.delta8,
            on.mix.delta8,
            off.mix.delta4,
            on.mix.delta4,
            off.mix.repeat,
            on.mix.repeat,
            off.mix.rest,
            on.mix.rest,
        ];
        for (t, r) in tot.iter_mut().zip(row) {
            *t += r;
        }
        println!(
            "{}\t{}\t{:.0}\t{:.0}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            name,
            off.frames.len(),
            off.bps,
            on.bps,
            row[0],
            row[1],
            row[2],
            row[3],
            row[4],
            row[5],
            row[6],
            row[7],
            row[8],
            row[9],
        );

        // The joined stream: the lines with 400 ms pauses between them,
        // the shape a match's sidecar would carry.
        joined_off.extend(&off.frames);
        joined_off.extend([Frame::default(); 40]);
        joined_on.extend(&on.frames);
        joined_on.extend([Frame::default(); 40]);
    }

    let n = clips.len() as f64;
    println!();
    println!(
        "mean per line (n={n}): off {:.0} B/s, on {:.0} B/s",
        mean_off / n,
        mean_on / n
    );
    let jbps = |f: &[Frame]| encode_stream(f).len() as f64 * 100.0 / f.len() as f64;
    println!(
        "joined (32.4 s): off {:.0} B/s, on {:.0} B/s",
        jbps(&joined_off),
        jbps(&joined_on)
    );
    println!(
        "packet totals: full {} -> {}, delta8 {} -> {}, delta4 {} -> {}, repeat {} -> {}, rest {} -> {}",
        tot[0], tot[1], tot[2], tot[3], tot[4], tot[5], tot[6], tot[7], tot[8], tot[9]
    );
}
