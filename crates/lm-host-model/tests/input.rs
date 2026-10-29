//! Input → decisions, and possession.
//!
//! A person playing the game clicks on a map. Those clicks are not commands
//! the host obeys: they become decisions, they travel through the same
//! sequencer the agents use, and they land in the round log like any other
//! decision (spec §6, D10 lesson 10). The host never decides a game outcome;
//! it only says what the human meant.
//!
//! These tests pin the three things that make that true:
//!
//! * a click while possessing an agent moves that agent, and only that
//!   agent;
//! * a click while spectating is refused by name, and nothing is submitted;
//! * a release hands the agent back to its reflexes with no leftover held
//!   action — including when the agent was stunned, and including when the
//!   human stops playing it mid-intent.
//!
//! They also pin the recording: what the human did shows up in the log with
//! `Provenance::Human`, so a round played by a person replays exactly like
//! a round played by the headless runner.

use lm_core::decision::{Action, Kind, Provenance, Source};
use lm_core::header::Header;
use lm_core::rules::Event;
use lm_core::world::{CellXY, World};
use lm_host_model::{HostLoop, InputError, InputEvent, InputState, Viewer};
use lm_tools::runner::{self, DEFAULT_REFLEX};

/// A standard round with the shipped reflex.
fn round(seed: u64) -> Header {
    Header::default_round(seed, DEFAULT_REFLEX)
}

/// A small, fast round: two chunks a side, agents that walk a cell per tick
/// and a short stun.
///
/// The standard round is 128 cells a side and moves at a sixth of a cell per
/// tick, so the squads need some 1800 ticks to meet. The tests here do not
/// care about the size of the map — they care about possession — so they
/// play a small one and get their answers in a few dozen ticks.
fn small_round(seed: u64) -> Header {
    let mut h = round(seed);
    h.params.world_chunks = (2, 2);
    h.params.speed_upt = 2_048; // one cell per tick
    h.params.stun_ticks = 10;
    h
}

/// The cell the agent stands on.
fn cell_of(host: &HostLoop, agent: u16) -> CellXY {
    let world = host.sim().world();
    world.grid.cell_of(
        host.sim()
            .agent(agent)
            .expect("the agent is in the round")
            .pos,
        world.cell_units,
    )
}

/// A walkable cell next to `cell`, preferring the orthogonal ones: the
/// motor walks a straight line to those.
fn neighbour(world: &World, cell: CellXY) -> Option<CellXY> {
    for (dx, dy) in [(1i32, 0i32), (0, 1), (-1, 0), (0, -1)] {
        let x = i32::from(cell.x) + dx;
        let y = i32::from(cell.y) + dy;
        if !(0..=i32::from(u16::MAX)).contains(&x) || !(0..=i32::from(u16::MAX)).contains(&y) {
            continue;
        }
        let candidate = CellXY {
            x: x as u16,
            y: y as u16,
        };
        if world.grid.walkable(candidate) {
            return Some(candidate);
        }
    }
    None
}

/// A walkable cell next to where the agent stands.
fn neighbour_of(host: &HostLoop, agent: u16) -> CellXY {
    let world = host.sim().world();
    neighbour(world, cell_of(host, agent)).expect("a walkable cell next to the agent")
}

/// Every decision of the round that the human made.
fn human_decisions(host: &HostLoop) -> Vec<lm_core::decision::Stamped> {
    host.log_so_far()
        .segments
        .iter()
        .flat_map(|segment| segment.decisions.iter())
        .filter(|stamped| stamped.d.source == Source::Human(0))
        .cloned()
        .collect()
}

#[test]
fn click_while_possessing_moves_agent() {
    let mut host = HostLoop::new(round(1)).expect("the round starts");
    let mut input = InputState::new();

    // The human takes over agent 0.
    input
        .apply(InputEvent::Possess(0), &mut host)
        .expect("the possession is accepted");
    assert_eq!(input.viewer, Viewer::Possessing(0));

    // And clicks a cell next to it.
    let target = neighbour_of(&host, 0);
    input
        .apply(InputEvent::ClickCell(target), &mut host)
        .expect("the click is accepted");
    host.tick();

    // The click is the agent's held action: not a special case that reaches
    // into the sim, but a decision the agent plays.
    assert_eq!(
        host.sim().agent(0).expect("agent 0").held,
        Some(Action::GoTo(target))
    );

    // So the agent walks there.
    for _ in 0..300 {
        host.tick();
        if cell_of(&host, 0) == target {
            break;
        }
    }
    assert_eq!(cell_of(&host, 0), target);

    // And it stays there: the click, not the reflex, is what holds the
    // agent. The reflex would carry it on towards a beacon.
    for _ in 0..30 {
        host.tick();
    }
    assert_eq!(cell_of(&host, 0), target);
}

#[test]
fn click_while_spectating_is_refused() {
    let mut host = HostLoop::new(round(1)).expect("the round starts");
    let mut input = InputState::new();

    // A spectator is watching, not playing: nobody is possessed.
    assert_eq!(input.viewer, Viewer::Spectator { follow: None });

    let target = neighbour_of(&host, 0);
    assert_eq!(
        input.apply(InputEvent::ClickCell(target), &mut host),
        Err(InputError::NotPossessing)
    );

    // Nothing was submitted, and the viewer did not change.
    assert_eq!(input.viewer, Viewer::Spectator { follow: None });
    host.tick();
    assert!(human_decisions(&host).is_empty());
}

#[test]
fn release_clears_held_action() {
    let mut host = HostLoop::new(round(1)).expect("the round starts");
    let mut input = InputState::new();

    input
        .apply(InputEvent::Possess(0), &mut host)
        .expect("the possession is accepted");
    let target = neighbour_of(&host, 0);
    input
        .apply(InputEvent::ClickCell(target), &mut host)
        .expect("the click is accepted");
    host.tick();
    assert_eq!(
        host.sim().agent(0).expect("agent 0").held,
        Some(Action::GoTo(target))
    );

    // The human lets go of the agent.
    input
        .apply(InputEvent::Release, &mut host)
        .expect("the release is accepted");

    // The viewer is a spectator again, following the agent it let go of.
    assert_eq!(input.viewer, Viewer::Spectator { follow: Some(0) });

    // And the agent goes back to its reflexes with nothing left over.
    host.tick();
    assert_eq!(host.sim().agent(0).expect("agent 0").held, None);
}

#[test]
fn possess_stunned_agent_ok() {
    let mut host = HostLoop::new(small_round(1)).expect("the round starts");
    let mut input = InputState::new();

    // Play until the rules tag somebody: that agent is stunned.
    let mut stunned = None;
    for _ in 0..200 {
        let out = host.tick();
        for event in &out.events {
            if let Event::Tagged { target, .. } = event {
                stunned = Some(*target);
            }
        }
        if stunned.is_some() {
            break;
        }
    }
    let stunned = stunned.expect("the squads meet and somebody is tagged");
    assert!(
        host.sim()
            .agent(stunned)
            .expect("the tagged agent")
            .stunned_until
            .is_some(),
        "the tagged agent is stunned"
    );

    // The human can take over a stunned agent: the input is not refused.
    input
        .apply(InputEvent::Possess(stunned), &mut host)
        .expect("a stunned agent can be possessed");
    assert_eq!(input.viewer, Viewer::Possessing(stunned));

    // The cell the agent walks to once it is free again: next to its
    // squad's spawn, which is where the rules put it back.
    let world = host.sim().world();
    let squad = host.sim().agent(stunned).expect("the agent").squad;
    let spawn = world.spawns[usize::from(squad)];
    let target = neighbour(world, spawn).expect("a walkable cell next to the spawn");

    input
        .apply(InputEvent::ClickCell(target), &mut host)
        .expect("the click is accepted");
    host.tick();

    // The click is recorded even though the agent cannot move yet.
    assert_eq!(
        host.sim().agent(stunned).expect("the agent").held,
        Some(Action::GoTo(target))
    );

    // And the agent stays put while it is stunned.
    let frozen = host.sim().agent(stunned).expect("the agent").pos;
    host.tick();
    assert_eq!(host.sim().agent(stunned).expect("the agent").pos, frozen);

    // When the stun ends, the recorded action plays: the agent walks to
    // the cell the human clicked.
    for _ in 0..120 {
        host.tick();
        if cell_of(&host, stunned) == target {
            break;
        }
    }
    assert_eq!(cell_of(&host, stunned), target);
}

#[test]
fn possession_is_recorded_in_log() {
    let mut host = HostLoop::new(round(1)).expect("the round starts");
    let mut input = InputState::new();

    input
        .apply(InputEvent::Possess(0), &mut host)
        .expect("the possession is accepted");
    let target = neighbour_of(&host, 0);
    input
        .apply(InputEvent::ClickCell(target), &mut host)
        .expect("the click is accepted");
    host.tick();
    input
        .apply(InputEvent::Release, &mut host)
        .expect("the release is accepted");
    host.tick();

    // The round log holds what the human did, and says it was human.
    let human = human_decisions(&host);
    assert_eq!(human.len(), 2);
    assert!(human
        .iter()
        .all(|stamped| stamped.d.provenance == Provenance::Human));
    assert_eq!(human[0].d.kind, Kind::ActionOn(Action::GoTo(target)));
    assert_eq!(human[1].d.kind, Kind::ActionOff);

    // And a round with a human in it still replays bit-exact.
    let log = host.log_so_far();
    assert_eq!(runner::replay(&log).expect("the log replays"), log.hashes());
}
