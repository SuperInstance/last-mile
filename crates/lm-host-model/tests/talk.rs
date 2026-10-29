//! The done-when of phase 4: squads coordinate by talking, and the person
//! hears them.
//!
//! Five proofs, and they are the plan's own:
//!
//! * [`stub_round_end_to_end`] — a round with the stub voice backend and
//!   scripted lines: the lines are logged with their provenance, the round
//!   replays bit-exact, and the talk counters are not zero;
//! * [`an_order_is_heard_and_taken_up`] — the coordination proof: an agent
//!   says "beacon 2" on the radio, a squad-mate hears it, understands the
//!   point and walks to the beacon inside the uptake window;
//! * [`a_talk_round_replays_without_any_voice_service`] — the round plays
//!   the same way with the voice service on and with it off;
//! * [`the_map_shows_a_speech_tick_per_word`] — the map's draw model carries
//!   one speech tick per word of the line that is being spoken;
//! * [`live_round_with_riva`] — the opt-in live test: the real voice
//!   service, with at least one clip synthesized.
//!
//! The lines are **scripted**, not produced by the agents' threads: a round
//! whose lines arrive at whatever tick the model answered is not a round
//! that can be replayed, and the replay is the point.

use std::sync::Arc;

use lm_core::decision::{Action, Decision, Kind, Provenance, Source};
use lm_core::header::{Header, Role, RosterEntry};
use lm_core::log::RoundLog;
use lm_core::talk::talk_metrics;
use lm_core::utterance::{
    Addressee, Affect, Channel, Intent, Prosody, SpeechAct, Subject, Utterance, Word,
};
use lm_core::world::CellXY;
use lm_host_model::host_loop::HostLoop;
use lm_host_model::input::Viewer;
use lm_host_model::map::MapModel;
use lm_host_model::talk::{Listener, TalkConfig, TalkCounters};
use lm_tools::runner;
use lm_voice::{RivaTts, StubTts};

// ------------------------------------------------------------------ helpers

/// A round of `agents` in one squad, so the radio reaches all of them.
fn one_squad(agents: u8) -> Header {
    let mut header = Header::default_round(1, runner::DEFAULT_REFLEX);
    header.params.squads = 1;
    header.params.squad_size = agents;
    header.roster = (0..agents)
        .map(|i| RosterEntry {
            agent: u16::from(i),
            squad: 0,
            role: if i == 0 { Role::Leader } else { Role::Member },
            voice: "default".to_owned(),
            harness_commit: "none".to_owned(),
            reflex: "default".to_owned(),
        })
        .collect();
    header
}

/// The line an agent speaks: an order about a beacon, on the radio.
///
/// The radio reaches the speaker's squad whatever the distance and whatever
/// the walls, so a squad-mate always hears it — and the radio's clarity is
/// the whole, so the point of the line survives it.
fn order_about(beacon: u8) -> Utterance {
    Utterance {
        speaker: 0,
        channel: Channel::Radio,
        addressee: Addressee::Squad,
        act: SpeechAct::Command,
        intent: Intent::Order,
        affect: Affect::Urgent,
        subject: Subject::Beacon(beacon),
        prosody: Prosody::Synthesized,
        words: vec![
            Word::new("beacon", 0, 10),
            Word::new("two", 10, 20).with_pause(4),
        ],
    }
}

/// A line that is not an order: a callout, so the round has a line that
/// cannot be obeyed by walking.
fn callout() -> Utterance {
    Utterance {
        speaker: 0,
        channel: Channel::Radio,
        addressee: Addressee::Squad,
        act: SpeechAct::Callout,
        intent: Intent::Report,
        affect: Affect::Calm,
        subject: Subject::None,
        prosody: Prosody::Synthesized,
        words: vec![Word::new("clear", 0, 10)],
    }
}

/// The scripted lines of the round: what is said, and on which tick.
fn the_script() -> Vec<(u64, Utterance)> {
    vec![
        (10, order_about(2)),
        (40, order_about(1)),
        (70, callout()),
        (100, order_about(3)),
    ]
}

/// Submit a line as the agent `speaker` said it.
///
/// The decision carries the provenance of the thought that chose it, so the
/// log says where the line came from.
fn submit_line(host: &mut HostLoop, tick: u64, seq: u32, line: Utterance) {
    let decision = Decision {
        apply_tick: tick,
        source: Source::Agent(line.speaker),
        seq,
        kind: Kind::Utterance(line),
        provenance: Provenance::System1 {
            model: "stub".to_owned(),
            tokens: 64,
            latency_ms: 1,
            situation_key: 0x5eed,
        },
    };
    host.submit_agent(decision, tick)
        .expect("the line is accepted");
}

/// Play the scripted round, with or without a voice service.
///
/// The lines land on their own ticks, so the round is the same round every
/// time — which is what makes the two runs comparable.
fn play_scripted(ticks: u64, with_voice: bool) -> (RoundLog, TalkCounters) {
    let mut host = HostLoop::new(one_squad(2)).expect("the round starts");
    if with_voice {
        host.attach_talk(
            TalkConfig::default()
                .with_tts(Arc::new(StubTts::default()))
                .with_listener(Listener::As(1)),
        );
    }

    let script = the_script();
    for tick in 0..ticks {
        // The lines due on this tick are handed over before it is played,
        // so they land on it.
        for (at, line) in &script {
            if *at == tick {
                submit_line(
                    &mut host,
                    tick,
                    u32::try_from(tick).unwrap_or(u32::MAX) + 1,
                    line.clone(),
                );
            }
        }
        host.tick();
    }
    let counters = host.talk_counters();
    (host.finish(), counters)
}

// ------------------------------------------------------------- the five tests

#[test]
fn stub_round_end_to_end() {
    let (log, counters) = play_scripted(900, true);

    // The lines are in the log, with the provenance of the thought that
    // chose them.
    let spoken: Vec<_> = log
        .segments
        .iter()
        .flat_map(|segment| &segment.decisions)
        .filter(|stamped| matches!(stamped.d.kind, Kind::Utterance(_)))
        .collect();
    assert!(!spoken.is_empty(), "the round spoke");
    for stamped in &spoken {
        match &stamped.d.provenance {
            Provenance::System1 {
                model,
                tokens,
                latency_ms,
                situation_key,
            } => {
                assert_eq!(model, "stub");
                assert_eq!(*tokens, 64);
                assert_eq!(*latency_ms, 1);
                assert_ne!(*situation_key, 0);
            }
            other => panic!("the line's provenance is {other:?}"),
        }
    }

    // The round replays bit-exact: the log's hashes are the replay's hashes.
    runner::replay(&log).expect("the round replays bit-exact");

    // The talk counters are not zero: the round talked, and the host asked
    // for a clip of every line the human's ear could hear.
    assert!(counters.utterances > 0, "the round spoke");
    assert!(counters.orders > 0, "the round gave orders");
    assert!(counters.airtime_ticks > 0, "the round spent airtime");
    assert!(counters.clips > 0, "the host asked for clips");
    assert!(
        counters.bark_misses > 0,
        "the bark cache synthesized the first line"
    );
}

#[test]
fn an_order_is_heard_and_taken_up() {
    let mut host = HostLoop::new(one_squad(2)).expect("the round starts");
    host.attach_talk(
        TalkConfig::default()
            .with_tts(Arc::new(StubTts::default()))
            .with_listener(Listener::As(1)),
    );

    // The beacon the order names.
    let beacon = host.sim().world().beacons[2];

    // Agent 0 says "beacon two" on the radio, on tick 10.
    submit_line(&mut host, 10, 1, order_about(2));
    for _ in 0..11 {
        host.tick();
    }

    // The squad-mate heard it, and understood the point of it.
    let heard = host.sim().view(1).heard;
    let line = heard
        .iter()
        .find(|line| line.speaker == 0)
        .expect("agent 1 heard the order");
    assert!(line.intent_known(), "the intent of the order survived");
    assert_eq!(line.intent, Some(Intent::Order));
    assert_eq!(line.channel, Channel::Radio);

    // Agent 1 walks to the beacon, inside the uptake window: the window
    // opens when the line ends.
    let ended = line.ended_tick;
    let walk_at = ended + 10;
    let decision = Decision {
        apply_tick: walk_at,
        source: Source::Agent(1),
        seq: 1,
        kind: Kind::ActionOn(Action::GoTo(beacon)),
        provenance: Provenance::Test,
    };
    host.submit_agent(decision, walk_at)
        .expect("the walk is accepted");

    let before = host.sim().agent(1).expect("agent 1").pos;
    // The ticks from 11 to the walk, so the walk is played.
    for _ in 0..(walk_at - 10) {
        host.tick();
    }
    let after = host.sim().agent(1).expect("agent 1").pos;

    // It walked: it is closer to the beacon than it was.
    let distance = |pos: lm_core::fixed::Pos| {
        let (x, y) = pos.to_units();
        let (bx, by) = beacon_centre(host.sim().world(), beacon);
        (x - bx).abs() + (y - by).abs()
    };
    assert!(
        distance(after) < distance(before),
        "agent 1 did not walk to the beacon: {before:?} -> {after:?}"
    );

    // And the round's metrics count one taken-up order.
    let metrics = talk_metrics(host.sim().talk_records());
    assert_eq!(metrics.orders, 1, "the round gave one order");
    assert_eq!(
        metrics.orders_taken_up, 1,
        "the order was taken up: {metrics:?}"
    );
}

/// The centre of a cell, in world units.
fn beacon_centre(world: &lm_core::world::World, cell: CellXY) -> (i64, i64) {
    let centre = world.grid.center(cell, world.cell_units);
    let (x, y) = centre.to_units();
    (x, y)
}

#[test]
fn a_talk_round_replays_without_any_voice_service() {
    // The same round, once with the voice service and once without it.
    let (with_voice, _) = play_scripted(300, true);
    let (without_voice, _) = play_scripted(300, false);

    // The voice service changed nothing: the two rounds are the same round.
    assert!(
        runner::diff(&with_voice, &without_voice).is_none(),
        "the voice service changed the round"
    );

    // And each one replays on its own.
    runner::replay(&with_voice).expect("the round replays bit-exact");
    runner::replay(&without_voice).expect("the round replays bit-exact");
}

#[test]
fn the_map_shows_a_speech_tick_per_word() {
    let mut host = HostLoop::new(one_squad(2)).expect("the round starts");

    // Agent 0 says a two-word line on the radio.
    submit_line(&mut host, 10, 1, order_about(2));
    for _ in 0..11 {
        host.tick();
    }

    // The line is still being spoken, so the map draws its ticks.
    let model = MapModel::build(
        host.sim(),
        host.prev_positions(),
        &Viewer::Spectator { follow: None },
        0.0,
    );
    let ticks: Vec<_> = model
        .speech
        .iter()
        .filter(|tick| tick.speaker == 0)
        .collect();
    assert_eq!(
        ticks.len(),
        2,
        "the map shows one tick per word: {:?}",
        model.speech
    );
    // The ticks are the words of the line, in order.
    assert!(ticks[0].at < ticks[1].at, "the ticks are in order");
    assert_eq!(ticks[0].channel, Channel::Radio);

    // The ticks sit on the round's timeline: the first word has been said
    // by now and the second has not. A tick that is in the far future is a
    // line's own offset that was never moved onto the round, and the map
    // would draw every word as unspoken.
    assert!(
        ticks[0].at <= model.now_tick,
        "the first word has been said: {:?} at {}",
        ticks[0],
        model.now_tick
    );
    assert!(
        ticks[1].at > model.now_tick,
        "the second word has not been said yet: {:?} at {}",
        ticks[1],
        model.now_tick
    );
}

#[test]
#[ignore = "the live voice service is needed: LM_RIVA_BASE and LM_RIVA_TTS_PORT"]
fn live_round_with_riva() {
    let Some(tts) = RivaTts::from_config(
        &lm_voice::riva_config_from_env()
            .expect("the environment names the voice service: LM_RIVA_BASE and LM_RIVA_TTS_PORT"),
    ) else {
        panic!("the voice service is not configured");
    };

    let mut host = HostLoop::new(one_squad(2)).expect("the round starts");
    host.attach_talk(
        TalkConfig::default()
            .with_tts(Arc::new(tts))
            .with_listener(Listener::As(1)),
    );

    submit_line(&mut host, 10, 1, order_about(2));
    for _ in 0..300 {
        host.tick();
    }

    let counters = host.talk_counters();
    assert!(
        counters.clips > 0,
        "the voice service synthesized no clip: {counters:?}"
    );
}
