//! The conductor's score: one system of staves per agent, on one clock
//! (Vox Score §12, E7).
//!
//! The score is built from the same sources the piano roll is built from —
//! the round log and the events of the round — with one thing more: the
//! agents' attention, sampled from their own state as the round plays or
//! replays. These tests state the properties that matter to a reviewer:
//! every agent gets a system, the lines become notes with their words and
//! marks, the decisions become gestures, the attention becomes the gaze
//! and the tier, and the refusals become hollow marks that only a played
//! round can have.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use lm_agents::config::AgentsConfig;
use lm_agents::harness::Harness;
use lm_agents::mind::Policy;
use lm_agents::sysone::{Refusal, StubBackend, SysOne};
use lm_agents::Agents;
use lm_core::decision::{Action, Kind};
use lm_core::header::Header;
use lm_core::utterance::{
    Addressee, Affect, Channel, Intent as TalkIntent, Prosody, SpeechAct, Subject, Utterance, Word,
};
use lm_core::world::CellXY;
use lm_host_model::host_loop::HostLoop;
use lm_host_model::roll::MarkKind;
use lm_host_model::score::{
    hollows_of, note_attention, Attention, AttentionTrace, GestureKind, HollowKind, Run, ScoreModel,
};
use lm_tools::runner::DEFAULT_REFLEX;

/// The shipped harness: the gates and the policy the default round thinks
/// with. The agents of these tests are the ones the game would use; only
/// the model behind them is a stub.
const DEFAULT_GATES: &str = include_str!("../../../assets/harness/default/gates.toml");
const DEFAULT_POLICY: &str = include_str!("../../../assets/harness/default/policy.toml");
const DEFAULT_PHRASEBOOK: &str = include_str!("../../../assets/harness/default/phrasebook.toml");

/// A standard round with the shipped reflex.
fn round(seed: u64) -> Header {
    Header::default_round(seed, DEFAULT_REFLEX)
}

/// The shipped harness, read into memory.
fn default_harness() -> Harness {
    Harness {
        dir: PathBuf::from("assets/harness/default"),
        gates_toml: DEFAULT_GATES.to_owned(),
        policy_toml: DEFAULT_POLICY.to_owned(),
        phrasebook_toml: DEFAULT_PHRASEBOOK.to_owned(),
        facts_toml: String::new(),
        reflexes: BTreeMap::new(),
        commit: "content:test-harness".to_owned(),
    }
}

/// A stub behind every tier the policy names.
fn stub_backends(harness: &Harness) -> BTreeMap<String, Box<dyn SysOne + Send + Sync>> {
    let policy = Policy::for_harness(&harness.policy_toml);
    policy
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
        .collect()
}

/// Give the thinker thread a chance to answer before the next tick, as the
/// agents test does: the host never waits for a model, but a test that
/// plays ticks as fast as the CPU allows would outrun the worker.
fn settle(host: &mut HostLoop) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while host.agents_pending() > 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Play `ticks` of a round, sampling every agent's attention after each
/// one — what the host's score does once a tick.
fn play(host: &mut HostLoop, ticks: u64, trace: &mut AttentionTrace) {
    for _ in 0..ticks {
        let out = host.tick();
        settle(host);
        note_attention(host.sim(), trace, out.tick);
    }
}

/// A spoken line: four words, ten ticks apart, an urgent order to the
/// squad about beacon 1, its first word lifted and stressed.
fn line(speaker: u16) -> Utterance {
    let words = ["hold", "the", "north", "beacon"]
        .iter()
        .enumerate()
        .map(|(index, text)| {
            let at = 10 * index as u64;
            Word::new(*text, at, at + 8)
                .with_pitch(if index == 0 { 4 } else { 0 })
                .with_emphasis(if index == 0 { 65_536 } else { 0 })
        })
        .collect();
    Utterance {
        speaker,
        channel: Channel::Voice,
        addressee: Addressee::Squad,
        act: SpeechAct::Command,
        intent: TalkIntent::Order,
        affect: Affect::Urgent,
        subject: Subject::Beacon(1),
        prosody: Prosody::Synthesized,
        words,
    }
}

/// The empty map of hollow marks: what a replay passes.
fn no_hollows() -> BTreeMap<u16, Vec<lm_host_model::score::Hollow>> {
    BTreeMap::new()
}

#[test]
fn every_agent_gets_a_system() {
    let mut host = HostLoop::new(round(1)).expect("the round starts");
    let mut trace = AttentionTrace::new();
    play(&mut host, 30, &mut trace);
    let log = host.log_so_far();

    let score = ScoreModel::from_log(&log, &trace, &no_hollows(), 0..30);

    // One system per agent of the roster, in agent-id order, with the
    // roster's own squad and role.
    assert_eq!(score.systems.len(), 8);
    assert_eq!(
        score
            .systems
            .iter()
            .map(|system| system.agent)
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3, 4, 5, 6, 7]
    );
    let leader = score.system(0).expect("agent 0 has a system");
    assert_eq!(leader.squad, 0);
    assert_eq!(leader.role, lm_core::header::Role::Leader);
    let member = score.system(1).expect("agent 1 has a system");
    assert_eq!(member.squad, 0);
    assert_eq!(member.role, lm_core::header::Role::Member);
}

#[test]
fn an_utterance_is_a_note_with_its_words_and_marks() {
    let mut host = HostLoop::new(round(1)).expect("the round starts");
    let mut trace = AttentionTrace::new();
    play(&mut host, 10, &mut trace);

    // The agent says a line at tick 10; the bar is the line's airtime.
    host.submit_human(2, Kind::Utterance(line(2)))
        .expect("the decision is accepted");
    host.tick();
    let log = host.log_so_far();

    let score = ScoreModel::from_log(&log, &trace, &no_hollows(), 0..60);
    let system = score.system(2).expect("agent 2 has a system");

    let note = system.voice.first().expect("the line is a note");
    let airtime = line(2).duration_ticks();
    assert_eq!(note.from, 10);
    assert_eq!(note.to, 10 + u64::from(airtime), "from plus the airtime");
    assert_eq!(note.affect, Affect::Urgent);
    assert_eq!(note.words.len(), 4);
    assert_eq!(note.words[0].text, "hold");
    // The words keep the lanes the score gave them: the pitch and the
    // emphasis of the first word ride along to the drawing.
    assert_eq!(note.words[0].pitch_steps, 4);
    assert_eq!(note.words[0].emphasis_q16, 65_536);

    // The marks name the plan the gates chose: the tone, what the line was
    // for, who it was for and what it was about.
    assert_eq!(
        note.marks(),
        vec![
            "urgent".to_owned(),
            "order".to_owned(),
            "to-squad".to_owned(),
            "b1".to_owned()
        ]
    );

    // And the note is on nobody else's staff: a line speaks for its speaker.
    for system in &score.systems {
        if system.agent != 2 {
            assert!(system.voice.is_empty(), "agent {} has a note", system.agent);
        }
    }
}

#[test]
fn a_calm_line_and_an_urgent_line_read_apart() {
    // The E7 pass question is whether a reader can tell the urgent moment
    // from the score alone, so the model has to keep the two readings
    // apart before any drawing can.
    let mut host = HostLoop::new(round(1)).expect("the round starts");
    let mut trace = AttentionTrace::new();
    play(&mut host, 5, &mut trace);

    let mut calm = line(1);
    calm.affect = Affect::Calm;
    calm.intent = TalkIntent::Report;
    calm.addressee = Addressee::All;
    calm.subject = Subject::None;
    host.submit_human(1, Kind::Utterance(calm))
        .expect("the calm line is accepted");
    host.submit_human(3, Kind::Utterance(line(3)))
        .expect("the urgent line is accepted");
    host.tick();
    let log = host.log_so_far();

    let score = ScoreModel::from_log(&log, &trace, &no_hollows(), 0..60);
    let calm = score.system(1).expect("agent 1 has a system");
    let urgent = score.system(3).expect("agent 3 has a system");
    assert_eq!(
        calm.voice[0].marks(),
        vec!["calm".to_owned(), "report".to_owned(), "to-all".to_owned()]
    );
    assert_eq!(
        urgent.voice[0].marks(),
        vec![
            "urgent".to_owned(),
            "order".to_owned(),
            "to-squad".to_owned(),
            "b1".to_owned()
        ]
    );
}

#[test]
fn the_decisions_are_gestures_by_whose_hand() {
    let mut host = HostLoop::new(round(1)).expect("the round starts");
    let mut trace = AttentionTrace::new();
    play(&mut host, 10, &mut trace);

    // The human possesses agent 4: a click, then a release.
    host.submit_human(4, Kind::ActionOn(Action::GoTo(CellXY { x: 12, y: 9 })))
        .expect("the decision is accepted");
    host.submit_human(4, Kind::ActionOff)
        .expect("the release is accepted");
    host.tick();
    let log = host.log_so_far();

    let score = ScoreModel::from_log(&log, &trace, &no_hollows(), 0..20);
    let system = score.system(4).expect("agent 4 has a system");
    assert_eq!(system.intent.len(), 2);
    assert!(system
        .intent
        .iter()
        .all(|gesture| gesture.who == MarkKind::Human));
    assert_eq!(system.intent[0].what, GestureKind::Action);
    assert_eq!(system.intent[1].what, GestureKind::Release);
    assert!(system.intent.iter().all(|gesture| gesture.late_by == 0));
}

#[test]
fn a_reflex_round_is_all_engine() {
    // No model played, so the engine did: every run is the reflex, and no
    // gesture was made. That is the tier staff's honest reading of a round
    // without System One, and the gaze staff's of an agent that attended
    // to nothing.
    let mut host = HostLoop::new(round(1)).expect("the round starts");
    let mut trace = AttentionTrace::new();
    play(&mut host, 40, &mut trace);
    let log = host.log_so_far();

    let score = ScoreModel::from_log(&log, &trace, &no_hollows(), 0..40);
    for system in &score.systems {
        assert_eq!(
            system.runs,
            vec![Run {
                from: 0,
                to: 39,
                what: Attention::Reflex
            }],
            "the engine played agent {}'s whole window",
            system.agent
        );
        assert!(system.intent.is_empty());
    }

    // The same trace in a narrow window is clipped to it.
    let score = ScoreModel::from_log(&log, &trace, &no_hollows(), 10..20);
    let system = score.system(0).expect("agent 0 has a system");
    assert_eq!(
        system.runs,
        vec![Run {
            from: 10,
            to: 19,
            what: Attention::Reflex
        }]
    );
}

#[test]
fn the_gamepad_takes_over_where_the_lanes_chose() {
    // A round with thinking agents: the stub answers every gate, so the
    // move gate's answer becomes a lane decision, and the lanes choose a
    // move. From the tick they first chose, the gamepad holds the body and
    // the engine's run is over — the tier staff's whole story.
    let header = round(7);
    let harness = default_harness();
    let agents = Agents::new(
        &AgentsConfig::default(),
        &harness,
        &header,
        stub_backends(&harness),
    );
    let mut host = HostLoop::new(header).expect("the round starts");
    host.attach_agents(agents);

    let mut trace = AttentionTrace::new();
    play(&mut host, 300, &mut trace);
    let log = host.log_so_far();

    let score = ScoreModel::from_log(&log, &trace, &no_hollows(), 0..300);
    let mut gamepad = 0;
    for system in &score.systems {
        for run in &system.runs {
            if run.what.is_move() {
                gamepad += 1;
            }
        }
    }
    assert!(
        gamepad > 0,
        "the lanes chose moves for some agent in a thinking round"
    );

    // And where they did, the agent also made move gestures: lane
    // decisions with System One provenance.
    let moves: Vec<_> = score
        .systems
        .iter()
        .flat_map(|system| system.intent.iter())
        .filter(|gesture| gesture.what == GestureKind::Move)
        .collect();
    assert!(
        moves.iter().all(|gesture| gesture.who == MarkKind::System1),
        "the move gestures are System One's"
    );
}

#[test]
fn the_refusals_become_hollow_marks() {
    // A mind that hesitated: one timeout, one unsure gate, one malformed
    // answer. The score names each by its own kind (D26), and the unsure
    // gate is the D72 case the intent staff draws hollow.
    let refusals = vec![
        (10u64, Refusal::Timeout),
        (20u64, Refusal::LowConfidence(0.3)),
        (30u64, Refusal::Malformed("no".to_owned())),
    ];
    let hollows = hollows_of(&refusals);
    assert_eq!(
        hollows
            .iter()
            .map(|hollow| (hollow.tick, hollow.why))
            .collect::<Vec<_>>(),
        vec![
            (10, HollowKind::Timeout),
            (20, HollowKind::Unsure),
            (30, HollowKind::Malformed),
        ]
    );
    assert_eq!(HollowKind::Unsure.as_str(), "unsure");

    // And they land on the system of the mind that refused, inside the
    // window and nowhere else.
    let mut host = HostLoop::new(round(1)).expect("the round starts");
    let mut trace = AttentionTrace::new();
    play(&mut host, 5, &mut trace);
    let log = host.log_so_far();
    let mut hollow = BTreeMap::new();
    hollow.insert(5u16, hollows);
    let score = ScoreModel::from_log(&log, &trace, &hollow, 15..25);
    let system = score.system(5).expect("agent 5 has a system");
    assert_eq!(
        system
            .hollow
            .iter()
            .map(|hollow| (hollow.tick, hollow.why))
            .collect::<Vec<_>>(),
        vec![(20, HollowKind::Unsure)]
    );
    assert!(score
        .system(0)
        .expect("agent 0 has a system")
        .hollow
        .is_empty());
}

#[test]
fn the_score_keeps_only_the_window() {
    let mut host = HostLoop::new(round(1)).expect("the round starts");
    let mut trace = AttentionTrace::new();
    play(&mut host, 90, &mut trace);

    // A line at tick 90, thirty-eight ticks of airtime.
    host.submit_human(2, Kind::Utterance(line(2)))
        .expect("the decision is accepted");
    host.tick();
    let log = host.log_so_far();
    let from = 90;
    let to = 90 + u64::from(line(2).duration_ticks());

    let all = ScoreModel::from_log(&log, &trace, &no_hollows(), 0..200);
    assert_eq!(all.system(2).expect("agent 2 has a system").voice.len(), 1);

    // A window the note's airtime reaches into keeps it, and a window
    // after its last tick does not — the note is the airtime, not the tick
    // it began on.
    let touching = ScoreModel::from_log(&log, &trace, &no_hollows(), from + 10..200);
    assert_eq!(
        touching
            .system(2)
            .expect("agent 2 has a system")
            .voice
            .len(),
        1,
        "a line from {from} to {to} reaches a window that starts at {}",
        from + 10
    );
    let after = ScoreModel::from_log(&log, &trace, &no_hollows(), to + 1..to + 40);
    assert!(
        after
            .system(2)
            .expect("agent 2 has a system")
            .voice
            .is_empty(),
        "a line that ended at {to} does not reach a window that starts at {}",
        to + 1
    );
}

#[test]
fn a_replayed_round_scores_like_a_played_one() {
    // The scrubber samples the attention of every agent as its opening
    // replay plays the round, so a replayed round draws the same score a
    // played one does — the whole point of building the staves from the
    // log and the agents' own state.
    use lm_host_model::record;
    use lm_host_model::scrub::Scrubber;

    let mut host = HostLoop::new(round(3)).expect("the round starts");
    let mut trace = AttentionTrace::new();
    play(&mut host, 120, &mut trace);
    let log = host.finish();

    let dir = std::env::temp_dir().join("lm-score-test");
    let path = dir.join("replayed-round.lml");
    record::write(&log, &path).expect("the log is written");

    let scrubber = Scrubber::open(&path).expect("the log replays");
    let replayed =
        ScoreModel::from_log(scrubber.log(), scrubber.attention(), &no_hollows(), 0..120);
    let played = ScoreModel::from_log(&log, &trace, &no_hollows(), 0..120);
    assert_eq!(played, replayed);
}
