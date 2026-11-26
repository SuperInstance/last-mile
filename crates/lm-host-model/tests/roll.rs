//! The piano roll: a lane per agent, the decisions on it and what happened
//! to it.
//!
//! The roll is the M1 debugger (spec §6): it shows, per agent and per tick,
//! who decided what and what the world did to them. It reads two sources: the
//! round log, which holds the decisions that were applied, and the events of
//! the round, which hold what the rules did with them.
//!
//! The host keeps the events as it plays (`TickOut::events`); the replay
//! viewer keeps the events of the round it opened. Either way the roll is
//! built the same way, from the log and from the events.

use lm_core::decision::{Action, Kind, Tick};
use lm_core::header::Header;
use lm_core::rules::Event;
use lm_core::world::CellXY;
use lm_host_model::host_loop::HostLoop;
use lm_host_model::roll::{MarkKind, RollModel, Span, SpanKind};
use lm_tools::runner::DEFAULT_REFLEX;

/// A standard round with the shipped reflex.
fn round(seed: u64) -> Header {
    Header::default_round(seed, DEFAULT_REFLEX)
}

/// The events of a round, with the tick they happened on.
fn events_of(host: &mut HostLoop, ticks: u64) -> Vec<(Tick, Event)> {
    let mut events = Vec::new();
    for _ in 0..ticks {
        let out = host.tick();
        for event in out.events {
            events.push((out.tick, event));
        }
    }
    events
}

#[test]
fn roll_has_lane_per_agent() {
    let mut host = HostLoop::new(round(1)).expect("the round starts");
    let events = events_of(&mut host, 30);
    let log = host.log_so_far();

    let roll = RollModel::from_log(&log, &events, 0..30);

    // One lane per agent of the roster, in agent-id order.
    assert_eq!(roll.lanes.len(), 8);
    assert_eq!(
        roll.lanes.iter().map(|lane| lane.agent).collect::<Vec<_>>(),
        vec![0, 1, 2, 3, 4, 5, 6, 7]
    );
}

#[test]
fn human_marks_on_possessed_lane() {
    let mut host = HostLoop::new(round(1)).expect("the round starts");
    events_of(&mut host, 10);

    // The human possesses agent 3 and clicks a cell, then releases.
    host.submit_human(3, Kind::ActionOn(Action::GoTo(CellXY { x: 12, y: 9 })))
        .expect("the decision is accepted");
    host.submit_human(3, Kind::ActionOff)
        .expect("the decision is accepted");
    let out = host.tick();
    let events: Vec<(Tick, Event)> = out.events.iter().map(|e| (out.tick, e.clone())).collect();

    let log = host.log_so_far();
    let roll = RollModel::from_log(&log, &events, 0..20);

    // Both human decisions are on the lane of the agent they were made for.
    let lane = roll.lane(3).expect("agent 3 has a lane");
    assert_eq!(lane.marks.len(), 2);
    assert!(lane.marks.iter().all(|mark| mark.kind == MarkKind::Human));
    assert_eq!(
        lane.marks.iter().map(|mark| mark.tick).collect::<Vec<_>>(),
        vec![10, 10]
    );
    assert!(lane.marks.iter().all(|mark| mark.late_by == 0));

    // And on nobody else's lane: a human decision speaks for its agent only.
    for lane in &roll.lanes {
        if lane.agent != 3 {
            assert!(lane.marks.is_empty(), "agent {} has marks", lane.agent);
        }
    }
}

#[test]
fn stun_span_matches_events() {
    let mut host = HostLoop::new(round(1)).expect("the round starts");
    events_of(&mut host, 5);
    let log = host.log_so_far();

    // A tag at tick 100 and the respawn 90 ticks later: the agent is stunned
    // from the tag to the respawn.
    let events = vec![
        (100, Event::Tagged { by: 1, target: 4 }),
        (190, Event::Respawned(4)),
    ];
    let roll = RollModel::from_log(&log, &events, 0..300);

    let lane = roll.lane(4).expect("agent 4 has a lane");
    assert_eq!(
        lane.spans,
        vec![Span {
            from: 100,
            to: 190,
            what: SpanKind::Stunned
        }]
    );

    // The tagger is not the stunned one: the span is on the target's lane.
    assert!(roll.lane(1).expect("agent 1 has a lane").spans.is_empty());
}

#[test]
fn stun_spans_follow_a_real_round() {
    // The same rule on a round that was really played: every tag is answered
    // by a span on the target's lane, and every respawn by the end of one.
    //
    // The round has to run long enough for the squads to meet: the first tag
    // of seed 1 is at tick 1841, and the respawn that lifts it is 90 ticks
    // later.
    let mut host = HostLoop::new(round(1)).expect("the round starts");
    let events = events_of(&mut host, 2000);
    let log = host.log_so_far();

    let roll = RollModel::from_log(&log, &events, 0..2000);

    let mut tags = 0;
    for (tick, event) in &events {
        let Event::Tagged { by, target } = event else {
            continue;
        };
        tags += 1;

        // The target is stunned from the tick of the tag.
        let lane = roll.lane(*target).expect("the target has a lane");
        let span = lane
            .spans
            .iter()
            .find(|span| span.what == SpanKind::Stunned && span.from == *tick)
            .unwrap_or_else(|| panic!("no stun span for the tag at tick {tick}"));
        assert_eq!(span.from, *tick);

        // And it ends on the respawn, when the round has one.
        let respawn = events.iter().find_map(|(at, event)| match event {
            Event::Respawned(id) if id == target && *at >= *tick => Some(*at),
            _ => None,
        });
        if let Some(respawn) = respawn {
            assert_eq!(span.to, respawn, "the stun of agent {target}");
        }

        // The tagger is not stunned by its own tag.
        assert!(roll
            .lane(*by)
            .expect("the tagger has a lane")
            .spans
            .iter()
            .all(|span| span.what != SpanKind::Stunned));
    }
    assert!(tags > 0, "the round produced no tag to check");
}

#[test]
fn the_roll_keeps_only_the_window() {
    let mut host = HostLoop::new(round(1)).expect("the round starts");
    events_of(&mut host, 60);
    let log = host.log_so_far();

    let all = RollModel::from_log(&log, &[], 0..60);
    let window = RollModel::from_log(&log, &[], 10..20);

    // The window does not change the lanes, only what is on them.
    assert_eq!(all.lanes.len(), window.lanes.len());
    for lane in &window.lanes {
        assert!(lane.marks.iter().all(|mark| (10..20).contains(&mark.tick)));
        assert!(lane
            .spans
            .iter()
            .all(|span| span.from < 20 && span.to >= 10));
    }
}
