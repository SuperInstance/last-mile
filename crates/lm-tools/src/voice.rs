//! `lm voice`: the voice lane's own report — a seeded round, every line
//! it spoke, pre-rendered through the bank, and the numbers that say
//! whether the audio can keep the round's pace.
//!
//! The round plays with the **stub minds of the shipped harness** (the
//! same gates, the same policy, the same phrasebook; only the model
//! behind them is a stub), so the report runs anywhere and the lines are
//! the lines the game would speak. Every utterance the round makes is
//! collected with the tick it arrived on (when the host learned of it)
//! and the tick it applied on (when it is due in the human's ear), and
//! each one is mapped through `voice.toml` — the speaker per agent, the
//! style per tone — into a content-addressed ask of the skald daemon.
//!
//! The report's five questions:
//!
//! 1. **how many unique lines** — the distinct asks (text, voice, style,
//!    engine); the bank makes the rest of the round's lines free;
//! 2. **the cache hit rate on a second run** — the same round's lines
//!    looked up again, after the first pass rendered them;
//! 3. **render latency p50/p95** — measured on the daemon this run;
//! 4. **the real-time factor** — the wall time of rendering over the
//!    wall time of the audio it produced;
//! 5. **how many lines would have been late at the round's real pace** —
//!    the prefetch schedule replayed against the round's own clock: one
//!    worker, submits at the lines' arrival ticks, each render taking
//!    its measured latency, and a line whose clip is not ready by its
//!    due tick plays as the tone (the fallback contract — the round
//!    never blocks).
//!
//! And the sixth, skald's rule: the **intelligibility gate** — audio
//! "works" only when a speech recognizer agrees with what was asked for.
//! When no ASR is on the box, the report says so plainly and claims
//! nothing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use lm_agents::config::AgentsConfig;
use lm_agents::harness::Harness;
use lm_agents::mind::Policy;
use lm_agents::sink::ChannelSink;
use lm_agents::sysone::{StubBackend, SysOne};
use lm_agents::Agents;
use lm_core::decision::{Kind, Tick};
use lm_core::header::Header;
use lm_core::sched::Sequencer;
use lm_core::sim::Sim;
use lm_core::utterance::Utterance;
use lm_voice::asr::{Gate, GateOutcome};
use lm_voice::bank::VoiceBank;
use lm_voice::prefetch::{Lookup, PrefetchQueue};
use lm_voice::skald::{SpeechLine, VoiceClient};
use lm_voice::voicemap::VoiceMap;

use crate::runner::DEFAULT_REFLEX;

/// The shipped harness, read into memory: the gates, the policy and the
/// phrasebook the default round thinks with.
const DEFAULT_GATES: &str = include_str!("../../../assets/harness/default/gates.toml");
const DEFAULT_POLICY: &str = include_str!("../../../assets/harness/default/policy.toml");
const DEFAULT_PHRASEBOOK: &str = include_str!("../../../assets/harness/default/phrasebook.toml");

/// How many ticks the tail drain may run, waiting for the stub minds'
/// last answers to land.
const TAIL_TICKS: u64 = 600;

/// How long the report waits for the prefetch worker to drain.
const DRAIN_WAIT: Duration = Duration::from_secs(600);

/// The floor of the intelligibility gate, in per-mille word accuracy.
///
/// skald's rule calls **tiny.en-level** agreement a pass: homophone
/// swaps and dropped words on these two-word barks ("still here" heard
/// back as "Still.") score 500, while babble — the failure the gate
/// exists to catch, fluent wrong tokens — measured at 333 or less in the
/// word-accuracy tests. 500 per-mille separates the two classes; the
/// mean is reported beside it for taste.
const GATE_FLOOR_PERMILLE: u32 = 500;

/// One collected line: what was said, when the host learned of it, and
/// when the human's ear is due to hear it.
struct Collected {
    /// The tick the decision arrived: the host's earliest prefetch.
    arrived: Tick,
    /// The tick the line is applied on: the clip's due time.
    applied: Tick,
    /// The line itself.
    utterance: Utterance,
}

/// The settings of `lm voice`.
pub struct VoiceArgs {
    /// The seed of the round.
    pub seed: u64,
    /// How many ticks the round may run.
    pub ticks: u64,
    /// Where the bank lives.
    pub bank: PathBuf,
    /// The voice map, when a file overrides the shipped one.
    pub voice_toml: Option<PathBuf>,
    /// How many lines the intelligibility gate samples.
    pub sample: usize,
}

/// Run the voice lane's report.
pub fn cmd_voice(args: &VoiceArgs) -> ExitCode {
    let map = match load_map(args.voice_toml.as_deref()) {
        Ok(map) => map,
        Err(err) => {
            eprintln!("lm: {err}");
            return ExitCode::FAILURE;
        }
    };

    // 1. The round, with the stub minds of the shipped harness.
    let header = Header::default_round(args.seed, DEFAULT_REFLEX);
    let lines = play_round(&header, args.ticks);
    if lines.is_empty() {
        eprintln!(
            "lm voice: seed {} spoke nothing in {} ticks; nothing to report",
            args.seed, args.ticks
        );
        return ExitCode::FAILURE;
    }

    // 2. Every utterance mapped through the voice map into an ask.
    let asks: Vec<(SpeechLine, Tick, Tick)> = lines
        .iter()
        .map(|line| {
            let voice = map.voice_of(line.utterance.speaker).to_owned();
            let style = map.style_of(line.utterance.affect, line.utterance.intent);
            (
                SpeechLine::new(line.utterance.prose(), voice, style),
                line.arrived,
                line.applied,
            )
        })
        .collect();
    let unique = unique_asks(&asks);

    // 3. The prefetch pass: render every unique line through the bank.
    let bank = match VoiceBank::open(&args.bank) {
        Ok(bank) => bank,
        Err(err) => {
            eprintln!("lm: {err}");
            return ExitCode::FAILURE;
        }
    };
    let bank_before = bank.len();
    let client = VoiceClient::new(&map.skald.base, map.skald.port, map.skald.sample_rate_hz);
    let mut queue = PrefetchQueue::new(client, bank, map.skald.deadline_ms);
    let started = Instant::now();
    let mut keys = Vec::with_capacity(unique.len());
    for (ask, _, _) in &unique {
        keys.push(queue.submit(ask, &map.skald.engine_id));
    }
    let drained = queue.wait(DRAIN_WAIT);
    let render_wall = started.elapsed();
    let stats = queue.stats();

    // 4. The warm pass: every line of the round looked up again.
    let mut warm_hits = 0usize;
    let mut keys_of_ask: Vec<u64> = Vec::with_capacity(asks.len());
    for (ask, _, _) in &asks {
        let key = queue.bank().key(ask, &map.skald.engine_id);
        keys_of_ask.push(key);
        if matches!(queue.lookup(key), Lookup::Hit(_)) {
            warm_hits += 1;
        }
    }
    let bank = queue.bank().clone();
    queue.shutdown();

    // 5. The pace replay and the audio arithmetic.
    let occurrences: Vec<(u64, Tick, Tick)> = keys_of_ask
        .iter()
        .zip(
            asks.iter()
                .map(|(_, arrived, applied)| (*arrived, *applied)),
        )
        .map(|(key, (arrived, applied))| (*key, arrived, applied))
        .collect();
    // The replay models what this run would have played: a line the
    // worker rendered takes its measured latency; a line the bank
    // already held was ready before the round began; a line that failed
    // is never ready.
    let latency_of: BTreeMap<u64, u64> = keys
        .iter()
        .map(|key| {
            let latency = match stats.latency_by_key.get(key) {
                Some(latency) => *latency,
                // Not rendered this run: a bank hit (ready at once)
                // unless the render also failed, which is never ready.
                None if bank.lookup(*key).is_some() => 0,
                None => u64::MAX,
            };
            (*key, latency)
        })
        .collect();
    let tick_ms = 1000 / u64::from(header.params.tick_hz.max(1));
    let late = late_lines(&occurrences, &latency_of, tick_ms);
    let audio_ms: u64 = keys
        .iter()
        .filter_map(|key| bank.lookup(*key).map(|m| m.duration_ms))
        .sum();
    let render_ms: u64 = stats.latencies_ms.iter().sum();

    // 6. The report.
    println!(
        "lm voice: seed {}, {} ticks, the stub minds of the shipped harness",
        args.seed, args.ticks
    );
    println!(
        "round: {} utterances, {} unique lines",
        asks.len(),
        unique.len()
    );
    println!(
        "bank: {} ({} entries before this run, {} now)",
        args.bank.display(),
        bank_before,
        bank.len()
    );
    println!(
        "daemon: {}:{} (engine {})",
        map.skald.base, map.skald.port, map.skald.engine_id
    );
    println!(
        "render pass: {} rendered, {} failed, {} already in the bank, {} s wall{}",
        stats.rendered,
        stats.failed,
        stats.bank_hits,
        decimal_permille(render_wall.as_millis() as u64),
        if drained {
            String::new()
        } else {
            " — DRAIN TIMEOUT".to_owned()
        }
    );
    if let Some(err) = &stats.last_error {
        println!("render pass: the last failure said: {err}");
    }
    if !stats.latencies_ms.is_empty() {
        println!(
            "render latency: p50 {} ms, p95 {} ms (n={})",
            percentile(&stats.latencies_ms, 50),
            percentile(&stats.latencies_ms, 95),
            stats.latencies_ms.len()
        );
    }
    if audio_ms > 0 {
        println!(
            "real-time factor: {} ({} s of audio in {} s of rendering)",
            decimal_permille(render_ms * 1000 / audio_ms.max(1)),
            decimal_permille(audio_ms),
            decimal_permille(render_ms)
        );
    }
    println!(
        "warm pass: {}/{} lines hit the bank ({} per-mille)",
        warm_hits,
        asks.len(),
        warm_hits * 1000 / asks.len()
    );
    println!(
        "pace: at {} Hz ({} ms a tick), {} of {} lines would have been late and played as the tone",
        header.params.tick_hz,
        tick_ms,
        late,
        asks.len()
    );

    // 7. The intelligibility gate, on a spread sample of the lines.
    let sample = spread(&unique, args.sample);
    if sample.is_empty() {
        println!("intelligibility: no lines to sample");
        return ExitCode::SUCCESS;
    }
    let mut gates: Vec<(PathBuf, String)> = Vec::new();
    for (ask, _, _) in &sample {
        let key = bank.key(ask, &map.skald.engine_id);
        let wav = bank.wav_path(key);
        if wav.is_file() {
            gates.push((wav, ask.text.clone()));
        } else {
            println!(
                "  not in the bank (the render failed): {:?} as {} ({})",
                ask.text, ask.voice, ask.style
            );
        }
    }
    match Gate::new(map.asr.clone()).hear(&gates) {
        GateOutcome::Heard(ear, heard) => {
            let mean =
                heard.iter().map(|h| h.accuracy_permille).sum::<u32>() / heard.len().max(1) as u32;
            let worst = heard.iter().map(|h| h.accuracy_permille).min().unwrap_or(0);
            for h in &heard {
                println!(
                    "  {:>4} per-mille  {:?} -> {:?}",
                    h.accuracy_permille, h.expected, h.heard
                );
            }
            let verdict = if worst >= GATE_FLOOR_PERMILLE {
                "PASS"
            } else {
                "FAIL"
            };
            println!(
                "intelligibility: {ear} heard {} sampled lines at {} per-mille mean, {} at worst — {verdict} (floor {GATE_FLOOR_PERMILLE})",
                heard.len(), mean, worst
            );
            if worst < GATE_FLOOR_PERMILLE {
                return ExitCode::FAILURE;
            }
        }
        GateOutcome::Unavailable(reasons) => {
            println!("intelligibility: NOT VERIFIED — no ASR on this box answered:");
            for reason in &reasons {
                println!("  {reason}");
            }
            println!("  The audio is not claimed to work.");
        }
    }

    ExitCode::SUCCESS
}

/// The lines a seeded round speaks with the stub minds of the shipped
/// harness: what the host's pre-warm asks the bank for before the round's
/// own lines arrive.
///
/// The stub round is the honest enumerator of the phrasebook: the
/// templates are realized by the same gates and the same world the real
/// round uses, so the lines that come out are the lines the round will
/// say — all six of them, in VOICE.md's measurement — rather than a cross
/// product of every template with every subject the phrasebook can name.
pub fn stub_round_lines(header: &Header, ticks: u64) -> Vec<Utterance> {
    play_round(header, ticks)
        .into_iter()
        .map(|line| line.utterance)
        .collect()
}

/// The voice map: the shipped one, or the file that overrides it.
fn load_map(path: Option<&Path>) -> Result<VoiceMap, String> {
    match path {
        Some(path) => VoiceMap::load(path).map_err(|err| err.to_string()),
        None => Ok(VoiceMap::shipped()),
    }
}

/// Play one round with the stub minds of the shipped harness and collect
/// every utterance, with the tick it arrived on and the tick it applied
/// on.
///
/// This is the host loop's own order of steps — the agents think, their
/// answers are sequenced, the sequencer is drained, the sim steps —
/// because the lines have to be the lines a real round speaks. The
/// tail drain waits for the stub minds' last answers, so a line that was
/// decided near the end of the round is collected too.
fn play_round(header: &Header, ticks: u64) -> Vec<Collected> {
    let harness = Harness {
        dir: PathBuf::from("assets/harness/default"),
        gates_toml: DEFAULT_GATES.to_owned(),
        policy_toml: DEFAULT_POLICY.to_owned(),
        phrasebook_toml: DEFAULT_PHRASEBOOK.to_owned(),
        reflexes: BTreeMap::new(),
        commit: "content:lm-voice".to_owned(),
    };
    let policy = Policy::for_harness(&harness.policy_toml);
    let backends: BTreeMap<String, Box<dyn SysOne + Send + Sync>> = policy
        .tiers
        .tiers()
        .iter()
        .filter(|tier| !tier.is_reflex())
        .map(|tier| {
            (
                tier.backend().to_owned(),
                Box::new(StubBackend::menu()) as Box<dyn SysOne + Send + Sync>,
            )
        })
        .collect();
    let (sink, mut inbox) = ChannelSink::new();
    let mut agents =
        Agents::new(&AgentsConfig::default(), &harness, header, backends).with_sink(Box::new(sink));

    let mut sim = Sim::new(header).expect("the round starts");
    let n_agents = u16::try_from(header.roster.len()).unwrap_or(u16::MAX);
    let mut sequencer = Sequencer::new(&header.params, n_agents, header.start_tick);

    let mut collected = Vec::new();
    let end = header
        .start_tick
        .saturating_add(ticks)
        .saturating_add(TAIL_TICKS);
    let mut quiet_ticks = 0u64;
    let mut tick = header.start_tick;
    while tick < end {
        // The round proper runs `ticks` ticks; the tail runs only while
        // answers are still in flight.
        if tick >= header.start_tick + ticks {
            if agents.pending() == 0 && inbox.is_empty() && quiet_ticks > 30 {
                break;
            }
            if agents.pending() == 0 && inbox.is_empty() {
                quiet_ticks += 1;
            }
        }
        // This harness keeps no round log, so the tick's refusals are
        // counted and let go of on purpose: there is no record to put them
        // in, and the voice only wants the lines.
        let _ = agents.on_tick(tick, &sim);
        for (decision, arrived) in inbox.drain() {
            let _ = sequencer.submit(decision, arrived);
        }
        let applied = sequencer.drain(tick);
        for stamped in &applied {
            if let Kind::Utterance(line) = &stamped.d.kind {
                collected.push(Collected {
                    arrived: stamped.arrived,
                    applied: stamped.tick,
                    utterance: line.clone(),
                });
            }
        }
        let _ = sim.step(&applied);
        tick += 1;
        if tick > header.start_tick + ticks && sim.outcome().is_some() {
            break;
        }
    }
    collected
}

/// The unique asks of a round, in first-appearance order, each with the
/// arrival and due ticks of that first appearance.
fn unique_asks(asks: &[(SpeechLine, Tick, Tick)]) -> Vec<(SpeechLine, Tick, Tick)> {
    let mut seen = std::collections::BTreeSet::new();
    let mut unique = Vec::new();
    for (ask, arrived, applied) in asks {
        // The ask's identity is the bank's key fields; style, voice and
        // text are all of them.
        let id = format!("{}\u{1}\u{2}{}\u{1}\u{2}{}", ask.text, ask.voice, ask.style);
        if seen.insert(id) {
            unique.push((ask.clone(), *arrived, *applied));
        }
    }
    unique
}

/// A spread sample of `n` of the asks: evenly spaced from the first to
/// the last, both ends included.
fn spread<T>(items: &[T], n: usize) -> Vec<&T> {
    if items.is_empty() || n == 0 {
        return Vec::new();
    }
    if items.len() <= n {
        return items.iter().collect();
    }
    let last = items.len() - 1;
    (0..n)
        .map(|i| i.saturating_mul(last) / (n - 1))
        .map(|i| &items[i])
        .collect()
}

/// The p-th percentile of a list of milliseconds, nearest-rank.
pub fn percentile(ms: &[u64], p: u64) -> u64 {
    if ms.is_empty() {
        return 0;
    }
    let mut sorted = ms.to_vec();
    sorted.sort_unstable();
    let rank = ((p as usize * sorted.len()).saturating_sub(1) / 100).min(sorted.len() - 1);
    sorted[rank]
}

/// A per-mille integer as a decimal string: `712` -> `"0.712"`.
pub fn decimal_permille(permille: u64) -> String {
    format!("{}.{:03}", permille / 1000, permille % 1000)
}

/// How many lines would have been late at the round's real pace.
///
/// The prefetch schedule replayed against the round's own clock: one
/// worker (the queue's), the submits at the lines' arrival ticks, each
/// unique line rendered once at its measured latency, repeats served by
/// the first render. A line is **late** when its clip is not ready by
/// the tick it applies on — and a late line plays as the tone (the
/// fallback contract), which is a miss to count, never a wait.
pub fn late_lines(
    occurrences: &[(u64, Tick, Tick)],
    latency_of: &BTreeMap<u64, u64>,
    tick_ms: u64,
) -> usize {
    let mut completion: BTreeMap<u64, u64> = BTreeMap::new();
    let mut busy_until = 0u64;
    let mut late = 0usize;
    for (key, arrived, applied) in occurrences {
        let submit = arrived.saturating_mul(tick_ms);
        let due = applied.saturating_mul(tick_ms);
        let done = match completion.get(key) {
            Some(done) => *done,
            None => {
                // The worker renders one line at a time: this render
                // starts when both the submit and the worker are ready.
                let start = busy_until.max(submit);
                let latency = latency_of.get(key).copied().unwrap_or(0);
                let done = start.saturating_add(latency);
                busy_until = done;
                completion.insert(*key, done);
                done
            }
        };
        if done > due {
            late += 1;
        }
    }
    late
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_with_lead_is_on_time() {
        // Submitted at tick 10, due at tick 40 (1000 ms of lead), and
        // rendered in 700 ms from the submit: 330 ms early.
        let occ = vec![(1u64, 10u64, 40u64)];
        let latency = BTreeMap::from([(1u64, 700u64)]);
        assert_eq!(late_lines(&occ, &latency, 33), 0);
    }

    #[test]
    fn a_line_due_as_it_arrives_is_late() {
        // No lead at all: any render is later than the due tick.
        let occ = vec![(1u64, 10u64, 10u64)];
        let latency = BTreeMap::from([(1u64, 700u64)]);
        assert_eq!(late_lines(&occ, &latency, 33), 1);
    }

    #[test]
    fn the_worker_is_one_and_a_queue_behind_is_late() {
        // Two lines submitted on the same tick, each 700 ms: the second
        // waits for the first, and a 750 ms lead covers only one.
        let occ = vec![(1u64, 10u64, 40u64), (2u64, 10u64, 33u64)];
        let latency = BTreeMap::from([(1u64, 700u64), (2u64, 700u64)]);
        // submit 330 ms; first done at 1030 ms; second done at 1730 ms;
        // due at 33 ticks = 1089 ms: the second is late.
        assert_eq!(late_lines(&occ, &latency, 33), 1);
    }

    #[test]
    fn a_repeat_is_served_by_the_first_render() {
        // The same key asked twice: one render serves both, and the
        // repeat is as on time as the first.
        let occ = vec![(1u64, 10u64, 40u64), (1u64, 100u64, 130u64)];
        let latency = BTreeMap::from([(1u64, 700u64)]);
        assert_eq!(late_lines(&occ, &latency, 33), 0);
    }

    #[test]
    fn a_bank_hit_renders_in_no_time() {
        // A key the bank already holds (latency 0) is ready the moment
        // it is submitted — even no-lead lines are on time.
        let occ = vec![(1u64, 10u64, 10u64)];
        let latency = BTreeMap::from([(1u64, 0u64)]);
        assert_eq!(late_lines(&occ, &latency, 33), 0);
    }

    #[test]
    fn the_percentiles_are_nearest_rank() {
        let ms = vec![100, 200, 300, 400, 500, 600, 700, 800, 900, 1000];
        assert_eq!(percentile(&ms, 50), 500);
        assert_eq!(percentile(&ms, 95), 1000);
        assert_eq!(percentile(&[], 50), 0);
    }

    #[test]
    fn permille_prints_as_a_decimal() {
        assert_eq!(decimal_permille(712), "0.712");
        assert_eq!(decimal_permille(1000), "1.000");
        assert_eq!(decimal_permille(23400), "23.400");
        assert_eq!(decimal_permille(0), "0.000");
    }

    #[test]
    fn the_spread_sample_covers_the_ends() {
        let items: Vec<u64> = (0..100).collect();
        let sample = spread(&items, 5);
        assert_eq!(sample.len(), 5);
        assert_eq!(*sample[0], 0, "the first is sampled");
        assert_eq!(*sample[4], 99, "the last is sampled");
        // A short list is the whole list.
        let tiny = vec![1u64, 2];
        assert_eq!(spread(&tiny, 5).len(), 2);
        assert!(spread::<u64>(&[], 5).is_empty());
    }

    #[test]
    fn the_unique_asks_keep_the_first_appearance() {
        let a = SpeechLine::new("hold", "mia", "Neutral");
        let b = SpeechLine::new("hold", "mia", "Angry");
        let asks = vec![(a.clone(), 5, 9), (b.clone(), 6, 9), (a.clone(), 100, 140)];
        let unique = unique_asks(&asks);
        assert_eq!(unique.len(), 2, "the repeat is one ask");
        assert_eq!(unique[0].0, a);
        assert_eq!(unique[0].1, 5, "the first appearance's ticks are kept");
    }
}
