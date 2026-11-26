//! The ears, live: the Thor's speech-to-text, end to end.
//!
//! The test is behind `#[ignore]` because it needs the box on the LAN and
//! not a mock: `scripts/live.sh` runs it there. It proves the whole
//! chain -- a line rendered by the Thor's voice, heard by the Thor's ears,
//! scored, and submitted to the round as the human's decision.
//!
//! The two services are named by the environment, so a test run can point
//! at another box:
//!
//! - `LM_EARS_BASE`: the ears, as the voice map names them: the whole
//!   address, port included. The default is the Thor's;
//! - `LM_SKALD_BASE`: the voice that renders the line, as the voice map
//!   names it: the base without the port, with the port from the map
//!   itself. The default is the Thor's.
//!
//! The line is rendered at the voice's own rate and brought to the
//! capture path's rate, exactly as the host's `--mic-wav` hook does.

use std::sync::Arc;
use std::time::{Duration, Instant};

use lm_core::decision::{Kind, Source};
use lm_core::header::Header;
use lm_core::log::RoundLog;
use lm_host_model::host_loop::HostLoop;
use lm_host_model::voice::{CommandParser, HumanVoice, TutorLadder, TutorMode};
use lm_host_model::wav;
use lm_tools::runner;
use lm_voice::{skald_base, Capture, HttpAsr, StubAsr, VoiceMap, ENV_EARS_BASE};

/// The Thor's ears.
const EARS: &str = "http://192.168.0.78:8102";

/// The line the test says.
const LINE: &str = "hold the north beacon";

/// The verb the line holds: the one the ladder gives the human first.
const VERB: &str = "hold";

/// Render a line with the Thor's voice: the wave of it.
fn render(line: &str) -> Vec<u8> {
    let map = VoiceMap::shipped();
    // The base without the port, as the voice map names it, and the port
    // from the map itself.
    let base = skald_base(&map);
    let url = format!("{base}:{}", map.skald.port);
    let body = serde_json::json!({ "input": line, "voice": "mia" }).to_string();
    let response = ureq::post(&format!("{url}/v1/audio/speech"))
        .header("Content-Type", "application/json")
        .send(body.as_bytes())
        .expect("the voice renders the line");
    response
        .into_body()
        .read_to_vec()
        .expect("the wave comes back")
}

/// The decisions a log holds from the human.
fn human_decisions(log: &RoundLog) -> Vec<&lm_core::decision::Stamped> {
    log.segments
        .iter()
        .flat_map(|segment| segment.decisions.iter())
        .filter(|stamped| matches!(stamped.d.source, Source::Human(_)))
        .collect()
}

#[test]
#[ignore]
fn the_ears_hear_a_line_and_the_line_reaches_the_score() {
    let base = std::env::var(ENV_EARS_BASE).unwrap_or_else(|_| EARS.to_owned());
    let ears = HttpAsr::new(base, 10_000);
    assert!(ears.healthy(), "the ears are not up at {}", ears.base());

    // The line, rendered by the Thor's voice and brought to the capture
    // path's rate.
    let wave = wav::read(&render(LINE))
        .expect("the wave reads")
        .at_capture_rate();
    println!(
        "the line is {} samples at {} Hz",
        wave.samples.len(),
        wave.sample_rate
    );

    // The path: the ears, with the stub behind them. The turn's deadline
    // is generous here: the lane is about the ears, not about the
    // deadline (the deadline has its own tests, against a mock).
    let mut voice = HumanVoice::new(
        Capture::new(Arc::new(ears.clone())),
        CommandParser::new(TutorLadder::shipped(), TutorMode::Ladder),
    )
    .with_fallback(Arc::new(StubAsr::scripted("hold")))
    .with_deadline(Duration::from_secs(15));

    let mut host =
        HostLoop::new(Header::default_round(1, runner::DEFAULT_REFLEX)).expect("the round starts");
    let talk = host.header().talk.clone();

    // The turn begins, and the round does not wait for the ears.
    let started = Instant::now();
    voice
        .begin(&wave.samples, 0, &talk)
        .expect("the turn begins");
    let waited = started.elapsed();
    assert!(
        waited < Duration::from_millis(200),
        "the round waited {waited:?} for the ears"
    );

    // The score, when the ears have answered.
    let mut score = None;
    while score.is_none() {
        std::thread::sleep(Duration::from_millis(5));
        score = voice.poll();
    }
    let took = started.elapsed();
    let score = score.expect("the score is there");
    let report = voice.last_turn().expect("the turn is reported");
    println!(
        "the ears heard {:?} in {took:?} (the call cost {:?})",
        report.text,
        ears.last_call()
    );
    let utterance = score.expect("the line was a command");

    // The ears heard it: not late, not down.
    let stats = voice.ears();
    println!("the turn was: {report:?}");
    assert_eq!(stats.heard, 1, "the ears heard the line: {report:?}");
    assert_eq!(stats.late, 0, "the ears were not late: {report:?}");
    assert_eq!(stats.down, 0, "the ears were not down: {report:?}");
    assert!(
        report.text.to_lowercase().contains(VERB),
        "the ears heard the verb: {report:?}"
    );

    // And the order reaches the score: the decision is the human's, and it
    // is logged like any other.
    host.submit_human(0, Kind::Utterance(utterance))
        .expect("the decision is accepted");
    host.tick();
    let log = host.log_so_far();
    assert!(
        !human_decisions(&log).is_empty(),
        "the line is in the round's log"
    );
    println!("the ears heard the line, and the line is in the log");
}
