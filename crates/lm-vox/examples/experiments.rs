//! The experiment harness (design §9): E2 and E3 on twenty skald lines.
//!
//! Run it on the Thor, where skald (:8000) and lm-ears (:8102) are up:
//!
//! ```text
//! cargo run -p lm-vox --release --example experiments -- [--keep DIR]
//! ```
//!
//! It renders the twenty lines through each of the three voices, and for
//! every clip it measures:
//!
//! * **E2** — the L0 stream's raw (900 B/s) against encoded bytes/s;
//! * **E3** — the per-channel round-trip error (analyze the LPC render
//!   again and compare channels), and ASR word accuracy on the skald
//!   render against ASR on the LPC render.
//!
//! Everything prints as TSV so VOX.md's tables can quote it directly, and
//! the Python reference analyzer (vox-eval) can diff the same numbers.

use std::io::Read;

use lm_vox::analyzer::analyze;
use lm_vox::encode::{decode_stream, encode_stream};
use lm_vox::frame::Frame;
use lm_vox::profile::SpeakerProfile;
use lm_vox::render::RenderConfig;
use lm_vox::schedule::{phone_sketch, word_timings_from_gestures};
use lm_vox::segment::segment_gestures;
use lm_vox::wav::Pcm;

/// The twenty lines: game-squad speech, the register the protocol is for.
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

/// The three voices the design names.
const VOICES: &[&str] = &["mia", "jason", "aria"];

fn main() {
    let keep = std::env::args().any(|a| a == "--keep");
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(180)))
            .build(),
    );

    println!("line\tvoice\tdur_ms\tE2_raw_Bs\tE2_enc_Bs\tE3_f0_cents\tE3_f1_pct\tE3_f2_pct\tE3_f3_pct\tE3_rms_dB\tE3_tilt_dB\tE3_noise\tASR_skald\tASR_lpc");

    let mut totals = Totals::default();
    for (li, line) in LINES.iter().enumerate() {
        let voice = VOICES[li % VOICES.len()];
        let wav = match speak(&agent, line, voice) {
            Ok(w) => w,
            Err(err) => {
                eprintln!("skald failed on {line:?}/{voice}: {err}");
                continue;
            }
        };
        let pcm = match Pcm::from_wav_bytes(&wav) {
            Ok(p) => p,
            Err(err) => {
                eprintln!("skald's WAV on {line:?}: {err}");
                continue;
            }
        };

        // E2: the stream's cost.
        let analysis = analyze(&pcm);
        let encoded = encode_stream(&analysis.frames);
        assert_eq!(decode_stream(&encoded).unwrap(), analysis.frames);
        let enc_bps = encoded.len() as f64 * 100.0 / analysis.frames.len() as f64;

        // The words' timing, from the performance itself.
        let words = word_timings_from_gestures(line, &analysis.frames);

        // E3: render and analyze again.
        let rendered = lm_vox::render_with_words(
            &analysis.frames,
            &SpeakerProfile::default(),
            &words,
            &RenderConfig::new(pcm.rate),
        );
        let back = analyze(&rendered);
        let err = round_trip_error(&analysis, &back);

        // ASR on both.
        let asr_skald = transcribe(&agent, &pcm.to_wav_bytes());
        let asr_lpc = transcribe(&agent, &rendered.to_wav_bytes());
        let acc_skald = accuracy(line, &asr_skald);
        let acc_lpc = accuracy(line, &asr_lpc);

        println!(
            "{}\t{}\t{}\t900\t{:.0}\t{:.1}\t{:.1}\t{:.1}\t{:.1}\t{:.1}\t{:.1}\t{:.1}\t{:.0}\t{:.0}",
            li + 1,
            voice,
            analysis.frames.len() * 10,
            enc_bps,
            err.f0_cents,
            err.f1_pct,
            err.f2_pct,
            err.f3_pct,
            err.rms_db,
            err.tilt_db,
            err.noise,
            acc_skald * 100.0,
            acc_lpc * 100.0,
        );

        totals.add(enc_bps, &err, acc_skald, acc_lpc);

        if keep {
            let dir = std::path::Path::new("target/vox-keep");
            let _ = std::fs::create_dir_all(dir);
            let _ = pcm.to_wav_file(&dir.join(format!("{li:02}-{voice}-skald.wav")));
            let _ = rendered.to_wav_file(&dir.join(format!("{li:02}-{voice}-lpc.wav")));
            let sketch = phone_sketch(&words);
            let l2 = l2_of(&analysis);
            let _ = std::fs::write(
                dir.join(format!("{li:02}-{voice}-schedule.txt")),
                format!("{line}\n{words:#?}\n{sketch:#?}\n{l2}\n"),
            );
        }
    }

    println!();
    println!(
        "mean\t\t\t\t{:.0}\t{:.1}\t{:.1}\t{:.1}\t{:.1}\t{:.1}\t{:.1}\t{:.1}\t{:.0}\t{:.0}",
        totals.enc_bps / totals.n as f64,
        totals.f0_cents / totals.n as f64,
        totals.f1_pct / totals.n as f64,
        totals.f2_pct / totals.n as f64,
        totals.f3_pct / totals.n as f64,
        totals.rms_db / totals.n as f64,
        totals.tilt_db / totals.n as f64,
        totals.noise / totals.n as f64,
        totals.acc_skald / totals.n as f64 * 100.0,
        totals.acc_lpc / totals.n as f64 * 100.0,
    );
    println!(
        "lines: {} (gestures per line, gestures field above)",
        totals.n
    );
}

fn l2_of(analysis: &lm_vox::analyzer::Analysis) -> String {
    let gestures = segment_gestures(&analysis.frames, &SpeakerProfile::default());
    let phrase = lm_vox::phrase_summary(&analysis.frames, &gestures, &SpeakerProfile::default());
    lm_vox::l2_line(&phrase)
}

/// Ask skald to speak a line.
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

/// Ask lm-ears to transcribe a WAV.
fn transcribe(agent: &ureq::Agent, wav: &[u8]) -> String {
    let form = ureq::unversioned::multipart::Form::new()
        .part(
            "file",
            ureq::unversioned::multipart::Part::bytes(wav)
                .file_name("clip.wav")
                .mime_str("audio/wav")
                .expect("audio/wav is a valid mime type"),
        )
        .text("language", "en");
    let response = match agent
        .post("http://localhost:8102/v1/audio/transcriptions")
        .send(form)
    {
        Ok(r) => r,
        Err(err) => {
            eprintln!("asr failed: {err}");
            return String::new();
        }
    };
    let mut text = String::new();
    let _ = response.into_body().into_reader().read_to_string(&mut text);
    // {"text": "...", ...}
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(v) => v["text"].as_str().unwrap_or_default().to_string(),
        Err(_) => String::new(),
    }
}

/// Word accuracy against the line's words: 1 - (S+D+I)/N by a greedy
/// alignment. Skald's own transcription is the ceiling, so the same
/// measure runs on it.
fn accuracy(line: &str, heard: &str) -> f64 {
    let want: Vec<String> = line
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphabetic())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect();
    let got: Vec<String> = heard
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphabetic())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect();
    if want.is_empty() {
        return 0.0;
    }
    // Levenshtein on words, then accuracy = (N - distance) / N.
    let n = want.len();
    let m = got.len();
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = usize::from(want[i - 1] != got[j - 1]);
            d[i][j] = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
        }
    }
    1.0 - d[n][m] as f64 / n as f64
}

/// The per-channel round-trip error between an analysis and the analysis
/// of its render: medians over the frames where both sides measured
/// something.
fn round_trip_error(a: &lm_vox::analyzer::Analysis, b: &lm_vox::analyzer::Analysis) -> Errors {
    let n = a.frames.len().min(b.frames.len());

    let f0_cents = median_f(
        (0..n)
            .filter(|&i| a.frames[i].f0_cents > 0 && b.frames[i].f0_cents > 0)
            .map(|i| (f64::from(a.frames[i].f0_cents) - f64::from(b.frames[i].f0_cents)).abs()),
    );

    let formant_pct = |pick: fn(&Frame) -> u8| {
        median_f(
            (0..n)
                .filter(|&i| pick(&a.frames[i]) > 0 && pick(&b.frames[i]) > 0)
                .map(|i| {
                    let ha = Frame::formant_hz(pick(&a.frames[i])).unwrap_or(1.0);
                    let hb = Frame::formant_hz(pick(&b.frames[i])).unwrap_or(1.0);
                    (ha - hb).abs() / ha * 100.0
                }),
        )
    };

    Errors {
        f0_cents,
        f1_pct: formant_pct(|f| f.f1),
        f2_pct: formant_pct(|f| f.f2),
        f3_pct: formant_pct(|f| f.f3),
        rms_db: median_f((0..n).map(|i| (a.frames[i].rms_db() - b.frames[i].rms_db()).abs())),
        tilt_db: median_f(
            (0..n).map(|i| (f64::from(a.frames[i].tilt) - f64::from(b.frames[i].tilt)).abs()),
        ),
        noise: median_f(
            (0..n).map(|i| (a.frames[i].noise as i16 - b.frames[i].noise as i16).abs() as f64),
        ),
    }
}

fn median_f(values: impl Iterator<Item = f64>) -> f64 {
    let mut v: Vec<f64> = values.collect();
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

#[derive(Default)]
struct Errors {
    f0_cents: f64,
    f1_pct: f64,
    f2_pct: f64,
    f3_pct: f64,
    rms_db: f64,
    tilt_db: f64,
    noise: f64,
}

#[derive(Default)]
struct Totals {
    n: usize,
    enc_bps: f64,
    f0_cents: f64,
    f1_pct: f64,
    f2_pct: f64,
    f3_pct: f64,
    rms_db: f64,
    tilt_db: f64,
    noise: f64,
    acc_skald: f64,
    acc_lpc: f64,
}

impl Totals {
    fn add(&mut self, enc_bps: f64, e: &Errors, acc_skald: f64, acc_lpc: f64) {
        self.n += 1;
        self.enc_bps += enc_bps;
        self.f0_cents += e.f0_cents;
        self.f1_pct += e.f1_pct;
        self.f2_pct += e.f2_pct;
        self.f3_pct += e.f3_pct;
        self.rms_db += e.rms_db;
        self.tilt_db += e.tilt_db;
        self.noise += e.noise;
        self.acc_skald += acc_skald;
        self.acc_lpc += acc_lpc;
    }
}
