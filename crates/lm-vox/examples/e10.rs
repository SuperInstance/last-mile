//! The E10 experiment harness: does the conformer move a real render's
//! measured delivery without moving its words?
//!
//! Run it on the Thor, where skald (:8000) and lm-ears (:8102) are up:
//!
//! ```text
//! cargo run -p lm-vox --release --example e10 [-- --keep]
//! ```
//!
//! Three phases, everything TSV so CONFORM.md can quote it directly:
//!
//! 1. **Calibration** — 20 lines x 3 voices x {Neutral, Calm, Angry}
//!    rendered by skald. The Neutral clips fit each voice's profile and
//!    median phrase length; all three styles fit the delivery-class
//!    centroids, and leave-one-out accuracy of that reader is reported.
//! 2. **Sweeps** — each DSP channel applied alone at rising magnitudes
//!    on six neutral clips, with ASR listening: the artifact thresholds
//!    of the tier rule are read off where word accuracy drops.
//! 3. **E10 arms** — every neutral line conformed toward
//!    `urgent,clipped,falling` and toward `calm`, plus a re-ask arm.
//!    Measured: ASR word accuracy against the unconformed line, the
//!    conformed clip's delivery class, and the conform's wall time
//!    against skald's own re-render latency.
//!
//! skald's synthesis is not deterministic (E4-E5 1.2), so every number
//! here is a statement about a distribution of renders, and each line's
//! own render is its baseline.

use std::io::Read;
use std::time::Instant;

use lm_vox::analyzer::{analyze, analyze_with_profile};
use lm_vox::conform::{
    self, classify, fit_centroids, psola_pitch_shift, tilt_eq, wsola, Centroids, ConformOutcome,
    DeliveryClass, F0Track, CLASS_FEATURES,
};
use lm_vox::profile::SpeakerProfile;
use lm_vox::wav::Pcm;

/// The twenty lines of the E2/E3 harness, reused so the seats compare.
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

/// The three voices the design names, with their full Riva voice ids.
const VOICES: [(&str, &str); 3] = [
    ("mia", "Magpie-Multilingual.EN-US.Mia"),
    ("jason", "Magpie-Multilingual.EN-US.Jason"),
    ("aria", "Magpie-Multilingual.EN-US.Aria"),
];

/// The three styles that map onto the delivery classes.
const STYLES: [(&str, DeliveryClass); 3] = [
    ("Neutral", DeliveryClass::Ordinary),
    ("Calm", DeliveryClass::Calm),
    ("Angry", DeliveryClass::Urgent),
];

fn main() {
    let keep = std::env::args().any(|a| a == "--keep");
    wait_for_ears();
    let mut ears = Ears::new();
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(180)))
            .build(),
    );
    let mut asr = AsrCache::load();

    let calib = calibrate(&agent);
    if keep {
        write_keep(&calib);
    }
    // The re-render baseline, measured fresh: the calibration renders are
    // cached, and a cache hit's 0 ms would read as a 0 ms re-render.
    let skald = re_render_latency(&agent);
    let centroids = sweep(&mut ears, &calib, &mut asr);
    arms(&mut ears, &calib, &centroids, keep, &mut asr, &skald);
    asr.save();
    ears.verify(asr.hits);
    ears.report(asr.hits);
}

/// The ears: lm-ears' door on :8102, and the model server behind it.
///
/// The door is the ASR service, but it is shared with the rest of the
/// Thor's audio work: its event loop carries another seat's
/// audio-understanding calls, so a transcription can sit in its queue for
/// a minute or more while the model server itself would answer in a
/// second. The harness asks the door first and falls back to the model
/// server — with the door's own parameters, `response_format=json` and
/// `language=en` — when the door does not answer promptly, and counts
/// which one answered. At the end of the run it re-asks the door for a
/// sample of the clips and checks that the two agree.
///
/// The cross-check samples *every* clip the run saw, not only the ones
/// the model server answered fresh: on a warm cache nothing is heard
/// during the run, and the door's agreement is then the only thing
/// tying the cached transcripts to the service the product calls.
struct Ears {
    door: ureq::Agent,
    model: ureq::Agent,
    door_hits: usize,
    model_hits: usize,
    /// The clips to cross-check against the door, with what the run heard
    /// for each.
    sampled: Vec<(Vec<u8>, String)>,
}

impl Ears {
    fn new() -> Ears {
        Ears {
            // The door's queue can hold a minute of another seat's
            // audio-understanding work, so the quick probe waits eight
            // seconds and then the model server behind it answers.
            door: ureq::Agent::new_with_config(
                ureq::Agent::config_builder()
                    .timeout_global(Some(std::time::Duration::from_secs(8)))
                    .build(),
            ),
            model: ureq::Agent::new_with_config(
                ureq::Agent::config_builder()
                    .timeout_global(Some(std::time::Duration::from_secs(180)))
                    .build(),
            ),
            door_hits: 0,
            model_hits: 0,
            sampled: Vec::new(),
        }
    }

    /// Ask one endpoint to transcribe. The door's answer is the
    /// reference; the model server is asked with the door's parameters.
    fn ask(&self, agent: &ureq::Agent, url: &str, wav: &[u8], model_form: bool) -> Option<String> {
        let mut form = ureq::unversioned::multipart::Form::new().part(
            "file",
            ureq::unversioned::multipart::Part::bytes(wav)
                .file_name("clip.wav")
                .mime_str("audio/wav")
                .expect("audio/wav is a valid mime type"),
        );
        form = form.text("language", "en");
        if model_form {
            form = form.text("response_format", "json");
        }
        let response = agent.post(url).send(form).ok()?;
        let mut text = String::new();
        response
            .into_body()
            .into_reader()
            .read_to_string(&mut text)
            .ok()?;
        let v: serde_json::Value = serde_json::from_str(&text).ok()?;
        let said = v["text"].as_str().unwrap_or_default().to_string();
        if said.is_empty() {
            None
        } else {
            Some(said)
        }
    }

    /// The transcript of one clip: the door's answer when the door is
    /// free, the model server's when the door is busy.
    fn transcribe(&mut self, wav: &[u8]) -> Option<String> {
        if let Some(text) = self.ask(&self.door, DOOR_URL, wav, false) {
            self.door_hits += 1;
            return Some(text);
        }
        let text = self.ask(&self.model, MODEL_URL, wav, true);
        if text.is_some() {
            self.model_hits += 1;
        }
        text
    }

    /// Keep a clip for the door's cross-check, with what the run heard.
    /// The list is capped: a clip's WAV is tens of kilobytes and the run
    /// sees hundreds.
    fn sample(&mut self, wav: &[u8], said: &str) {
        const CAP: usize = 160;
        if self.sampled.len() < CAP {
            self.sampled.push((wav.to_vec(), said.to_string()));
        }
    }

    /// Re-ask the door for a sample of the run's clips, and report
    /// whether the two agree. The door is the service the product calls;
    /// the measurement is only worth anything if the door would have said
    /// the same thing.
    fn verify(&mut self, cache_hits: usize) {
        if self.sampled.is_empty() {
            if cache_hits > 0 {
                println!(
                    "every clip came from the cache: nothing was heard this run, so there is no door cross-check to report"
                );
            } else {
                println!("the door answered every clip: no cross-check needed");
            }
            return;
        }
        // The door answers in its own time — measured at 49 s behind the
        // model server on a busy box — so the check waits for it, and it
        // asks a bounded sample spread across the run rather than every
        // clip: the queue is the bottleneck, and a spread shows a change
        // in the ASR's reading as a spread instead of one clip's luck.
        let patient = ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .timeout_global(Some(std::time::Duration::from_secs(90)))
                .build(),
        );
        const WANT: usize = 8;
        let step = (self.sampled.len() / WANT).max(1);
        let picks: Vec<(Vec<u8>, String)> = self
            .sampled
            .iter()
            .step_by(step)
            .take(WANT)
            .cloned()
            .collect();
        let mut agree = 0;
        let mut asked = 0;
        for (wav, cached) in &picks {
            let Some(text) = self.ask(&patient, DOOR_URL, wav, false) else {
                continue;
            };
            asked += 1;
            if &text == cached {
                agree += 1;
            } else {
                eprintln!("the door disagrees: {cached:?} vs {text:?}");
            }
        }
        println!(
            "the door re-asked {asked} of the run's {} clips: {agree} agreed",
            self.sampled.len()
        );
    }

    fn report(&self, cache_hits: usize) {
        println!(
            "asr calls: {} through the lm-ears door, {} through the model server behind it, {} served from the cache",
            self.door_hits, self.model_hits, cache_hits
        );
    }
}

/// The lm-ears door and the model server behind it.
const DOOR_URL: &str = "http://localhost:8102/v1/audio/transcriptions";
const MODEL_URL: &str = "http://127.0.0.1:8103/v1/audio/transcriptions";

/// The ASR answer cache. The ears' door is shared with the rest of the
/// Thor's audio work, so one call can take tens of seconds, and the sweep
/// is hundreds of calls. Keyed by the clip's own bytes (FNV-1a), so a re-run
/// after a crash, a service restart or an interrupted measurement only pays
/// for what it has not heard yet.
struct AsrCache {
    seen: std::collections::HashMap<u64, String>,
    path: &'static str,
    /// How many clips the cache answered this run. A clip served from the
    /// cache was not heard this run at all, and the report has to say so
    /// rather than claim the ears answered it.
    hits: usize,
}

impl AsrCache {
    fn load() -> AsrCache {
        let path = "target/vox-e10/asr-cache.tsv";
        let mut seen = std::collections::HashMap::new();
        if let Ok(text) = std::fs::read_to_string(path) {
            for line in text.lines() {
                if let Some((k, v)) = line.split_once('\t') {
                    if let Ok(key) = u64::from_str_radix(k, 16) {
                        seen.insert(key, v.to_string());
                    }
                }
            }
        }
        AsrCache {
            seen,
            path,
            hits: 0,
        }
    }

    fn save(&self) {
        let mut keys: Vec<u64> = self.seen.keys().copied().collect();
        keys.sort_unstable();
        let mut out = String::new();
        for k in keys {
            out.push_str(&format!("{k:016x}\t{}\n", self.seen[&k]));
        }
        if let Some(dir) = std::path::Path::new(self.path).parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(self.path, out);
    }
}

/// The re-render baseline: skald's own latency on the corpus, measured
/// fresh. The calibration grid is cached, so the p50 has to come from
/// renders this run actually asked for — a cache hit's 0 ms would read as
/// a 0 ms re-render, which is the whole comparison the latency gate makes.
fn re_render_latency(agent: &ureq::Agent) -> Vec<u128> {
    let mut lat = Vec::new();
    for (li, line) in LINES.iter().enumerate() {
        // One voice, every line: twenty fresh renders is a p50 with room.
        let (short, id) = VOICES[0];
        match speak(
            agent,
            line,
            li + 1,
            &format!("{id}.Neutral"),
            short,
            "Latency",
        ) {
            Ok((_, ms)) if ms > 0 => lat.push(ms),
            Ok(_) => {}
            Err(err) => eprintln!("the latency probe failed on {line:?}: {err}"),
        }
    }
    let mut sorted = lat.clone();
    sorted.sort_unstable();
    if !sorted.is_empty() {
        println!(
            "skald re-render on {} fresh renders: p50 {} ms p95 {} ms",
            sorted.len(),
            sorted[sorted.len() / 2],
            sorted[95 * sorted.len() / 100],
        );
    } else {
        println!("skald re-render: no fresh renders this run");
    }
    lat
}

// ---------------------------------------------------------------------------
// Phase 1: calibration
// ---------------------------------------------------------------------------

struct Clip {
    line: usize,
    voice: usize,
    style: usize,
    pcm: Pcm,
}

struct Calibration {
    clips: Vec<Clip>,
    profiles: Vec<SpeakerProfile>,
    /// Each voice's median phrase length over its neutral renders, ms.
    baseline_ms: Vec<u32>,
    centroids: Centroids,
}

/// The render cache: skald's synthesis is not deterministic, so a cached
/// render is a *different sample* of the same delivery — fine for every
/// number this harness reports (each line is its own baseline), and it
/// keeps a tuning pass from re-rendering the grid.
const CACHE: &str = "target/vox-e10";

fn speak(
    agent: &ureq::Agent,
    line: &str,
    line_no: usize,
    voice_id: &str,
    short: &str,
    style: &str,
) -> Result<(Vec<u8>, u128), String> {
    let cached = std::path::Path::new(CACHE).join(format!("{line_no:02}-{short}-{style}.wav"));
    if let Ok(bytes) = std::fs::read(&cached) {
        return Ok((bytes, 0));
    }
    let body = format!("{{\"input\":{line:?},\"voice\":\"{voice_id}\"}}");
    let started = Instant::now();
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
    Ok((wav, started.elapsed().as_millis()))
}

fn calibrate(agent: &ureq::Agent) -> Calibration {
    println!(
        "== calibration: {} renders ==",
        LINES.len() * VOICES.len() * STYLES.len()
    );
    let mut clips = Vec::new();
    for (li, line) in LINES.iter().enumerate() {
        for (vi, (short, id)) in VOICES.iter().enumerate() {
            for (si, (style, _)) in STYLES.iter().enumerate() {
                let (wav, _ms) =
                    match speak(agent, line, li + 1, &format!("{id}.{style}"), short, style) {
                        Ok(x) => x,
                        Err(err) => {
                            eprintln!("skald failed on {line:?}/{short}/{style}: {err}");
                            continue;
                        }
                    };
                let pcm = match Pcm::from_wav_bytes(&wav) {
                    Ok(p) => p,
                    Err(err) => {
                        eprintln!("bad WAV on {line:?}/{short}/{style}: {err}");
                        continue;
                    }
                };
                clips.push(Clip {
                    line: li,
                    voice: vi,
                    style: si,
                    pcm,
                });
            }
        }
    }

    // Per-voice profiles and median phrase lengths, over the neutral
    // renders only: the speaker's own norm is the baseline every relative
    // dial reads against (the E4 lesson).
    let mut profiles = Vec::new();
    let mut baseline_ms = Vec::new();
    for vi in 0..VOICES.len() {
        let mut profile = SpeakerProfile::default();
        let mut durations: Vec<u32> = Vec::new();
        for clip in clips.iter().filter(|c| c.voice == vi && c.style == 0) {
            let a = analyze(&clip.pcm);
            let m = conform::measure(&a, &profile);
            profile.update(&a.frames, &m.gestures, &m.phrase);
            durations.push(m.phrase.duration_ms);
        }
        durations.sort_unstable();
        let baseline = durations.get(durations.len() / 2).copied().unwrap_or(1000);
        profiles.push(profile);
        baseline_ms.push(baseline);
    }

    // Class features for every calibration clip, and the centroids.
    let samples: Vec<([f64; CLASS_FEATURES], DeliveryClass)> = clips
        .iter()
        .map(|c| {
            let a = analyze(&c.pcm);
            (
                conform::class_features(&a, &profiles[c.voice], baseline_ms[c.voice]),
                STYLES[c.style].1,
            )
        })
        .collect();
    let centroids = fit_centroids(&samples);

    // The syllable rate, kept for the record: the class reader does not
    // use it (see `class_features`), but the calibration table says
    // where each class sits on it, and the E10 write-up quotes it.
    let tempos: Vec<f64> = clips
        .iter()
        .map(|c| {
            let a = analyze(&c.pcm);
            let m = conform::measure(&a, &profiles[c.voice]);
            if profiles[c.voice].tempo_tenths > 0 {
                f64::from(m.phrase.tempo_tenths) / f64::from(profiles[c.voice].tempo_tenths)
            } else {
                1.0
            }
        })
        .collect();

    // Leave-one-out: the reader's own reliability, the number that bounds
    // what any class landing can promise (E5's lesson).
    let mut loo_correct = 0;
    for (i, _) in samples.iter().enumerate() {
        let rest: Vec<([f64; CLASS_FEATURES], DeliveryClass)> = samples
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, s)| *s)
            .collect();
        let c = fit_centroids(&rest);
        if classify(&samples[i].0, &c) == samples[i].1 {
            loo_correct += 1;
        }
    }

    // The per-class dial medians, so CONFORM.md can quote where each
    // delivery actually sits.
    println!("voice\tclass\tvel\ttempo_r\tdur_r\tcontour\ttilt");
    for (vi, (short, _)) in VOICES.iter().enumerate() {
        for (si, (_, class)) in STYLES.iter().enumerate() {
            let feats: Vec<[f64; CLASS_FEATURES]> = clips
                .iter()
                .enumerate()
                .filter(|(_, c)| c.voice == vi && c.style == si)
                .map(|(i, _)| samples[i].0)
                .collect();
            let median = |k: usize| {
                let mut v: Vec<f64> = feats.iter().map(|f| f[k]).collect();
                if v.is_empty() {
                    return 0.0;
                }
                v.sort_by(|a, b| a.partial_cmp(b).unwrap());
                v[v.len() / 2]
            };
            let mut tempo_vals: Vec<f64> = tempos
                .iter()
                .enumerate()
                .filter(|(i, _)| clips[*i].voice == vi && clips[*i].style == si)
                .map(|(_, t)| *t)
                .collect();
            tempo_vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let tempo = tempo_vals.get(tempo_vals.len() / 2).copied().unwrap_or(0.0);
            println!(
                "{}\t{:?}\t{:+.1}\t{:.2}\t{:.2}\t{:+.0}\t{:+.1}",
                short,
                class,
                median(0),
                tempo,
                median(1),
                median(2),
                median(3),
            );
        }
    }
    println!(
        "calibration: {} clips, reader leave-one-out {}/{} = {:.0}%",
        clips.len(),
        loo_correct,
        samples.len(),
        100.0 * loo_correct as f64 / samples.len().max(1) as f64,
    );

    Calibration {
        clips,
        profiles,
        baseline_ms,
        centroids,
    }
}

fn write_keep(calib: &Calibration) {
    let dir = std::path::Path::new("target/vox-e10");
    let _ = std::fs::create_dir_all(dir);
    for clip in &calib.clips {
        let name = format!(
            "{:02}-{}-{}.wav",
            clip.line + 1,
            VOICES[clip.voice].0,
            STYLES[clip.style].0
        );
        let _ = clip.pcm.to_wav_file(&dir.join(name));
    }
}

// ---------------------------------------------------------------------------
// Phase 2: the artifact-threshold sweeps
// ---------------------------------------------------------------------------

fn sweep(ears: &mut Ears, calib: &Calibration, asr: &mut AsrCache) -> Centroids {
    println!("\n== sweeps: each channel alone, ASR listening ==");
    // Six neutral clips: lines 1, 4, 7, 11, 15, 19, from mia (clips are
    // collected line-then-voice, so the first render of each line is hers).
    let probe: Vec<&Clip> = [0usize, 3, 6, 10, 14, 18]
        .iter()
        .filter_map(|li| calib.clips.iter().find(|c| c.line == *li && c.style == 0))
        .collect();

    // The baseline: what the ears say about each probe clip before any
    // channel touches it. A clip the ears failed on is dropped from the
    // sweep — a silent clip would read as a 100-point drop.
    let mut baseline_acc: Vec<Option<f64>> = Vec::new();
    for clip in &probe {
        let heard = transcribe(ears, &clip.pcm.to_wav_bytes(), asr);
        baseline_acc.push(heard.map(|h| accuracy(LINES[clip.line], &h)));
    }
    println!("channel\tmagnitude\tasr_drop_pts\tdial_delta\tn_clips");
    let mut rows: Vec<String> = Vec::new();

    let mut run = |name: &str,
                   magnitude: f64,
                   make: &dyn Fn(&Pcm) -> Pcm,
                   dial: &dyn Fn(&Pcm, &Pcm) -> f64| {
        let mut drop_sum = 0.0;
        let mut dial_sum = 0.0;
        let mut n = 0;
        for (i, clip) in probe.iter().enumerate() {
            let Some(base) = baseline_acc[i] else {
                continue;
            };
            let out = make(&clip.pcm);
            let Some(heard) = transcribe(ears, &out.to_wav_bytes(), asr) else {
                continue;
            };
            let acc = accuracy(LINES[clip.line], &heard);
            drop_sum += (base - acc) * 100.0;
            dial_sum += dial(&clip.pcm, &out);
            n += 1;
        }
        let row = format!(
            "{name}\t{magnitude:+.2}\t{:+.1}\t{:+.1}\t{n}",
            drop_sum / n as f64,
            dial_sum / n as f64,
        );
        println!("{row}");
        rows.push(row);
    };

    // Pitch: PSOLA uniform shifts; dial = the voiced f0 medians' gap.
    let f0_cents_gap = |a: &Pcm, b: &Pcm| -> f64 {
        match (median_f0_hz(a), median_f0_hz(b)) {
            (Some(x), Some(y)) => 1200.0 * (y / x).log2(),
            _ => 0.0,
        }
    };
    for cents in [
        -600.0f64, -400.0, -300.0, -200.0, -100.0, 100.0, 200.0, 300.0, 400.0, 600.0,
    ] {
        run(
            "pitch_cents",
            cents,
            &|pcm: &Pcm| {
                let a = analyze(pcm);
                let f0 = F0Track::from_analysis(&a);
                let x: Vec<f64> = pcm
                    .samples
                    .iter()
                    .map(|&s| f64::from(s) / 32_768.0)
                    .collect();
                let out = psola_pitch_shift(&x, pcm.rate, &f0, cents);
                to_pcm(&out, pcm.rate)
            },
            &f0_cents_gap,
        );
    }

    // Tempo: WSOLA alone; dial = the tempo-dial ratio. Both sides are
    // swept — alpha < 1 is compression (faster), alpha > 1 is stretch
    // (slower) — because the tier rule's tempo window has to be measured
    // on each side separately.
    for alpha in [
        0.65f64,
        0.72,
        0.80,
        1.0 / 1.3,
        1.0 / 1.45,
        1.0 / 1.6,
        1.15,
        1.25,
        1.3333,
        1.45,
        1.5385,
    ] {
        run(
            "tempo_alpha",
            alpha,
            &|pcm: &Pcm| {
                let x: Vec<f64> = pcm
                    .samples
                    .iter()
                    .map(|&s| f64::from(s) / 32_768.0)
                    .collect();
                to_pcm(&wsola(&x, pcm.rate, alpha), pcm.rate)
            },
            &|a: &Pcm, b: &Pcm| {
                let ta = tempo_dial(a);
                let tb = tempo_dial(b);
                if ta > 0.0 {
                    tb / ta
                } else {
                    0.0
                }
            },
        );
    }

    // Tilt: the shelf alone; dial = the median raw tilt's gap.
    for db in [4.0f64, 8.0, 12.0, 16.0, -4.0, -8.0, -12.0] {
        run(
            "tilt_db_oct",
            db,
            &|pcm: &Pcm| {
                let x: Vec<f64> = pcm
                    .samples
                    .iter()
                    .map(|&s| f64::from(s) / 32_768.0)
                    .collect();
                to_pcm(&tilt_eq(&x, pcm.rate, db), pcm.rate)
            },
            &|a: &Pcm, b: &Pcm| median_raw_tilt(b) - median_raw_tilt(a),
        );
    }

    // Noise: the mix alone; dial = the noise dial's gap.
    for units in [16u8, 32, 64, 96, 128] {
        run(
            "noise_units",
            f64::from(units),
            &|pcm: &Pcm| {
                let a = analyze(pcm);
                let x: Vec<f64> = pcm
                    .samples
                    .iter()
                    .map(|&s| f64::from(s) / 32_768.0)
                    .collect();
                to_pcm(&conform::mix_noise(&x, &a.frames, units), pcm.rate)
            },
            &|a: &Pcm, b: &Pcm| (median_noise_dial(b) as f64) - (median_noise_dial(a) as f64),
        );
    }

    // Breath insertion: the guarded valley alone; dial = the live-span
    // growth in ms. This is the channel that measures whether breathing
    // at a gesture boundary is word-safe — the "Re|group" question.
    let dur_ms = |p: &Pcm| -> f64 {
        let a = analyze(p);
        let m = conform::measure(&a, &a.profile);
        f64::from(m.phrase.duration_ms)
    };
    for ms in [150u32, 250, 400] {
        run(
            "insert_ms",
            f64::from(ms),
            &|pcm: &Pcm| {
                let a = analyze(pcm);
                let gestures = lm_vox::segment_gestures(&a.frames, &a.profile);
                let x: Vec<f64> = pcm
                    .samples
                    .iter()
                    .map(|&s| f64::from(s) / 32_768.0)
                    .collect();
                match conform::breath_point(&a.frames, &gestures, pcm.rate) {
                    Some(at) => to_pcm(
                        &conform::insert_silence(&x, at, ms as usize * pcm.rate as usize / 1000),
                        pcm.rate,
                    ),
                    None => to_pcm(&x, pcm.rate),
                }
            },
            &|a: &Pcm, b: &Pcm| dur_ms(b) - dur_ms(a),
        );
    }

    // Formants: the resample+PSOLA stage alone; dial = F2's relative move.
    for cents in [-200.0f64, -120.0, -60.0, 60.0, 120.0, 200.0] {
        run(
            "formant_cents",
            cents,
            &|pcm: &Pcm| {
                let a = analyze(pcm);
                let f0 = F0Track::from_analysis(&a);
                let x: Vec<f64> = pcm
                    .samples
                    .iter()
                    .map(|&s| f64::from(s) / 32_768.0)
                    .collect();
                to_pcm(&conform::formant_shift(&x, pcm.rate, &f0, cents), pcm.rate)
            },
            &|a: &Pcm, b: &Pcm| {
                let fa = median_formant_hz(a, 2);
                let fb = median_formant_hz(b, 2);
                match (fa, fb) {
                    (Some(x), Some(y)) => 100.0 * (y / x - 1.0),
                    _ => 0.0,
                }
            },
        );
    }

    // The sweep table, on disk, so CONFORM.md's thresholds are a table
    // someone can re-read rather than a claim.
    let mut table = String::from("channel\tmagnitude\tasr_drop_pts\tdial_delta\tn_clips\n");
    for row in &rows {
        table.push_str(row);
        table.push('\n');
    }
    let _ = std::fs::create_dir_all("target/vox-e10");
    let _ = std::fs::write("target/vox-e10/sweeps.tsv", table);

    calib.centroids.clone()
}

fn to_pcm(x: &[f64], rate: u32) -> Pcm {
    Pcm {
        samples: x.iter().map(|&v| (v * 32_767.0) as i16).collect(),
        rate,
    }
}

fn median_f0_hz(pcm: &Pcm) -> Option<f64> {
    let a = analyze(pcm);
    let mut hz: Vec<f64> = a
        .frames
        .iter()
        .filter(|f| f.flags.has(lm_vox::frame::FrameFlags::VOICED) && f.f0_cents > 0)
        .map(|f| lm_vox::frame::Frame::cents_to_hz(f64::from(f.f0_cents), a.profile.f0_ref()))
        .collect();
    if hz.is_empty() {
        return None;
    }
    hz.sort_by(|p, q| p.partial_cmp(q).unwrap());
    hz.get(hz.len() / 2).copied()
}

fn median_raw_tilt(pcm: &Pcm) -> f64 {
    let a = analyze(pcm);
    let mut t: Vec<f64> = a.raw.iter().map(|r| r.tilt_db_oct).collect();
    t.sort_by(|p, q| p.partial_cmp(q).unwrap());
    if t.is_empty() {
        0.0
    } else {
        t[t.len() / 2]
    }
}

fn median_noise_dial(pcm: &Pcm) -> i32 {
    let a = analyze(pcm);
    let mut v: Vec<i32> = a
        .frames
        .iter()
        .filter(|f| f.flags.has(lm_vox::frame::FrameFlags::VOICED))
        .map(|f| i32::from(f.noise))
        .collect();
    v.sort_unstable();
    if v.is_empty() {
        0
    } else {
        v[v.len() / 2]
    }
}

fn median_formant_hz(pcm: &Pcm, slot: u8) -> Option<f64> {
    let a = analyze(pcm);
    let mut hz: Vec<f64> = a
        .frames
        .iter()
        .filter(|f| f.f1 > 0)
        .filter_map(|f| {
            lm_vox::frame::Frame::formant_hz(match slot {
                1 => f.f1,
                _ => f.f2,
            })
        })
        .collect();
    hz.sort_by(|p, q| p.partial_cmp(q).unwrap());
    hz.get(hz.len() / 2).copied()
}

fn tempo_dial(pcm: &Pcm) -> f64 {
    let a = analyze(pcm);
    let m = conform::measure(&a, &a.profile);
    m.tempo_tenths as f64 / 10.0
}

// ---------------------------------------------------------------------------
// Phase 3: the E10 arms
// ---------------------------------------------------------------------------

fn arms(
    ears: &mut Ears,
    calib: &Calibration,
    centroids: &Centroids,
    keep: bool,
    asr: &mut AsrCache,
    skald_latency: &[u128],
) {
    println!("\n== E10 arms: conform every neutral line ==");
    println!("line\tvoice\tarm\tasr_before\tasr_after\tdrop\tclass\twanted\tlatency_ms\treask");

    struct ArmTotals {
        n: usize,
        class_hit: usize,
        asr_before: f64,
        asr_after: f64,
        latency: Vec<u128>,
        reask: usize,
    }
    let mut totals: Vec<(String, ArmTotals)> = Vec::new();
    let mut rows: Vec<String> = Vec::new();

    for clip in calib.clips.iter().filter(|c| c.style == 0) {
        let line = LINES[clip.line];
        let profile = &calib.profiles[clip.voice];
        let heard = transcribe(ears, &clip.pcm.to_wav_bytes(), asr);
        let asr_before = accuracy(line, &heard.unwrap_or_default());

        for (name, marks, wanted) in [
            ("urgent", "urgent,clipped", DeliveryClass::Urgent),
            // The brief's own example mark set, measured as its own arm.
            ("example", "urgent,clipped,falling", DeliveryClass::Urgent),
            ("calm", "calm", DeliveryClass::Calm),
            // The re-ask arm: +18 dB of gain is 3 dB past the 15 dB
            // threshold on *every* baseline, so the tier rule must refuse
            // all sixty lines (a target-relative stack would only refuse
            // the lines whose measured baseline happens to sit low).
            ("reask", "loud,loud,loud", DeliveryClass::Ordinary),
        ] {
            let started = Instant::now();
            let outcome = conform::conform_str(&clip.pcm, marks, profile)
                .expect("the arm mark sets are valid");
            let latency = started.elapsed().as_millis();
            let slot = totals.iter_mut().find(|(n, _)| n == name).map(|(_, t)| t);
            let slot = match slot {
                Some(t) => t,
                None => {
                    totals.push((
                        name.to_string(),
                        ArmTotals {
                            n: 0,
                            class_hit: 0,
                            asr_before: 0.0,
                            asr_after: 0.0,
                            latency: Vec::new(),
                            reask: 0,
                        },
                    ));
                    &mut totals.last_mut().unwrap().1
                }
            };

            match outcome {
                ConformOutcome::Conformed { audio, .. } => {
                    let heard2 = transcribe(ears, &audio.to_wav_bytes(), asr);
                    let asr_after = accuracy(line, &heard2.unwrap_or_default());
                    let a = analyze_with_profile(&audio, profile);
                    let features =
                        conform::class_features(&a, profile, calib.baseline_ms[clip.voice]);
                    let class = classify(&features, centroids);
                    let hit = class == wanted;
                    slot.n += 1;
                    slot.class_hit += usize::from(hit);
                    slot.asr_before += asr_before;
                    slot.asr_after += asr_after;
                    slot.latency.push(latency);
                    if keep {
                        let dir = std::path::Path::new("target/vox-e10");
                        let _ = std::fs::create_dir_all(dir);
                        let _ = audio.to_wav_file(&dir.join(format!(
                            "{:02}-{}-{}.wav",
                            clip.line + 1,
                            VOICES[clip.voice].0,
                            name
                        )));
                    }
                    let row = format!(
                        "{}\t{}\t{}\t{:.2}\t{:.2}\t{:+.1}\t{:?}\t{:?}\t{}\tconformed\tvel {:+.0} dur {:.2} contour {:+.0} tilt {:+.0}",
                        clip.line + 1,
                        VOICES[clip.voice].0,
                        name,
                        asr_before,
                        asr_after,
                        (asr_before - asr_after) * 100.0,
                        class,
                        wanted,
                        latency,
                        features[0],
                        features[1],
                        features[2],
                        features[3],
                    );
                    println!("{row}");
                    rows.push(row);
                }
                ConformOutcome::ReAsk(diff) => {
                    slot.reask += 1;
                    let row = format!(
                        "{}\t{}\t{}\t{:.2}\t\t\t\t{:?}\t{}\t{}",
                        clip.line + 1,
                        VOICES[clip.voice].0,
                        name,
                        asr_before,
                        wanted,
                        latency,
                        diff.reask_line(),
                    );
                    println!("{row}");
                    rows.push(row);
                }
            }
        }
    }

    println!();
    let mut skald_lat = skald_latency.to_vec();
    skald_lat.sort_unstable();
    if !skald_lat.is_empty() {
        println!(
            "skald re-render p50 {} ms p95 {} ms ({} fresh renders this run)",
            skald_lat[skald_lat.len() / 2],
            skald_lat[95 * skald_lat.len() / 100],
            skald_lat.len(),
        );
    }
    let _ = std::fs::create_dir_all("target/vox-e10");
    let mut table = String::from(
        "line\tvoice\tarm\tasr_before\tasr_after\tdrop\tclass\twanted\tlatency_ms\toutcome\n",
    );
    for row in &rows {
        table.push_str(row);
        table.push('\n');
    }
    let _ = std::fs::write("target/vox-e10/arms.tsv", table);
    for (name, t) in &totals {
        let mut lat = t.latency.clone();
        lat.sort_unstable();
        let p50 = lat.get(lat.len() / 2).copied().unwrap_or(0);
        let p95 = if lat.is_empty() {
            0
        } else {
            lat.get(95 * lat.len() / 100).copied().unwrap_or(0)
        };
        // The minimum is the least-disturbed sample: the box this ran on
        // is shared, and the re-render baseline is measured on the same
        // box at the same time, so the p50-to-p50 comparison is the fair
        // one and the minimum is what the conform costs when it is quiet.
        let pmin = lat.first().copied().unwrap_or(0);
        println!(
            "arm {name}: n {} class {}/{} = {:.0}%  asr {:.0}% -> {:.0}% (drop {:+.1} pts)  conform p50 {p50} ms p95 {p95} ms min {pmin} ms  reask {}",
            t.n,
            t.class_hit,
            t.n,
            100.0 * t.class_hit as f64 / t.n.max(1) as f64,
            100.0 * t.asr_before / t.n.max(1) as f64,
            100.0 * t.asr_after / t.n.max(1) as f64,
            (t.asr_before - t.asr_after) * 100.0 / t.n.max(1) as f64,
            t.reask,
        );
    }
}

// ---------------------------------------------------------------------------
// ASR and word accuracy (the E2/E3 harness's own measure)
// ---------------------------------------------------------------------------

/// The transcript of one clip, from the ears.
///
/// lm-ears restarts occasionally (another seat supervises it); a
/// transcription waits out a restart rather than counting the clip as
/// zero-accuracy, and a service that stays down drops the clip instead
/// of quietly corrupting the measurement.
///
/// `None` means the clip was never heard: the caller drops it from the
/// measurement instead of scoring a silent clip as zero accuracy. The
/// answer is cached by the clip's own bytes, so a re-run only pays for
/// what the ears have not already said, and the cache is written as the
/// run goes so an interrupted run keeps its answers.
fn transcribe(ears: &mut Ears, wav: &[u8], cache: &mut AsrCache) -> Option<String> {
    let key = lm_vox::fnv64(wav);
    if let Some(text) = cache.seen.get(&key) {
        cache.hits += 1;
        // A cached clip was heard on an earlier run, by the same service.
        // It still belongs in the door's cross-check: that is what ties
        // the cached transcripts to the door.
        ears.sample(wav, text);
        return Some(text.clone());
    }
    for attempt in 0..6 {
        match ears.transcribe(wav) {
            Some(said) => {
                cache.seen.insert(key, said.clone());
                cache.save();
                ears.sample(wav, &said);
                return Some(said);
            }
            None => {
                eprintln!("asr attempt {attempt} heard nothing");
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        }
    }
    eprintln!("the ears stayed quiet through every retry");
    None
}

fn wait_for_ears() {
    for _ in 0..60 {
        let ok = std::net::TcpStream::connect("127.0.0.1:8102").is_ok();
        if ok {
            return;
        }
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
    eprintln!("ears never came up on :8102");
    std::process::exit(2);
}

/// Word accuracy against the line's words: 1 - levenshtein/N, the E2/E3
/// harness's own greedy alignment.
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
    let (n, m) = (want.len(), got.len());
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
