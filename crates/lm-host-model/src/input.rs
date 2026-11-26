//! Input: what the person means, and what the host does with it.
//!
//! A person playing the game clicks on a map and presses keys. None of that
//! is a command the host obeys. Every human action becomes a
//! [`lm_core::decision::Decision`] with `source: Human(agent)` and
//! `provenance: Human`, and it travels through the very same
//! [`lm_core::sched::Sequencer`] the agents use (spec §6, D10 lesson 10).
//! The host never decides a game outcome; it only says what the human meant
//! and lets the rules have their say.
//!
//! That is why this module is so small, and why it is worth being exactly
//! this small: the input layer has no special path into the simulation.
//! A click becomes `ActionOn(GoTo(cell))`, a release becomes `ActionOff`,
//! and from there the decision is the sim's business — it is stamped on a
//! tick, it is applied on that tick, and it is written into the round log
//! like every other decision. A round played by a person replays exactly
//! like a round played by the headless runner.
//!
//! ## Who is playing
//!
//! [`Viewer`] is the one piece of state here: either the human plays as an
//! agent ([`Viewer::Possessing`]) or the human watches ([`Viewer::Spectator`],
//! with a free camera or one that follows an agent). The input layer needs
//! to know which, because most inputs only mean something while somebody is
//! being played: a click on a cell is an order to an agent, and there is no
//! agent to order when the human is watching. That case is refused by name
//! ([`InputError::NotPossessing`]) instead of quietly doing nothing.
//!
//! ## Leaving an agent behind
//!
//! An agent the human was playing goes back to its reflexes when the human
//! lets go of it — with no leftover held action, so the agent does not keep
//! walking to a cell the human clicked a moment ago. That happens when the
//! human presses release, and also when the human simply starts playing
//! somebody else: possession is a mode, and leaving the mode ends the
//! orders that came with it.
//!
//! ## A stunned agent
//!
//! Possessing an agent that is currently stunned is fine, and the inputs
//! that arrive while it is stunned are recorded rather than dropped: the
//! decision is stamped and logged like any other, the held action waits
//! behind the stun, and it plays when the stun ends (phase 1, `Agent::choose_cmd`).
//! The host does not need to know anything about stuns to do that — which is
//! the point of routing every human action through the sequencer.

use std::fmt;

use lm_core::decision::{Action, Kind};
use lm_core::sched::Reject;
use lm_core::world::CellXY;

use crate::host_loop::HostLoop;

/// What the person did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputEvent {
    /// Take over an agent and play as it.
    Possess(u16),
    /// Let go of the agent being played.
    Release,
    /// Click a cell of the map: the agent being played walks there.
    ClickCell(CellXY),
    /// Stand still: the agent being played holds its ground.
    Hold,
    /// Stop playing and watch, with a free camera.
    Spectate,
    /// Stop playing and watch one agent from the outside.
    FollowAgent(u16),
}

/// Who is looking at the round.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Viewer {
    /// Watching, not playing: the camera is free, or follows an agent.
    Spectator {
        /// The agent the camera follows, if any.
        follow: Option<u16>,
    },
    /// Playing as an agent.
    Possessing(u16),
}

impl Default for Viewer {
    /// A spectator with a free camera: nobody is playing until the human
    /// takes over an agent.
    fn default() -> Self {
        Viewer::Spectator { follow: None }
    }
}

impl Viewer {
    /// The agent being played, if any.
    pub fn possessing(&self) -> Option<u16> {
        match self {
            Viewer::Possessing(agent) => Some(*agent),
            Viewer::Spectator { .. } => None,
        }
    }

    /// The agent the camera follows, if any.
    pub fn following(&self) -> Option<u16> {
        match self {
            Viewer::Spectator { follow } => *follow,
            Viewer::Possessing(_) => None,
        }
    }
}

/// Why the host refused an input.
///
/// Every refusal is named, so the app can say what went wrong instead of
/// swallowing the click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputError {
    /// The input needs an agent to be played, and none is: the human is
    /// watching.
    NotPossessing,
    /// The agent is not in this round.
    UnknownAgent {
        /// The agent that was asked for.
        agent: u16,
    },
    /// The sequencer refused the decision the input became.
    Rejected(Reject),
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InputError::NotPossessing => write!(f, "no agent is possessed: the human is watching"),
            InputError::UnknownAgent { agent } => {
                write!(f, "agent {agent} is not in this round")
            }
            InputError::Rejected(reject) => {
                write!(f, "the sequencer refused the decision: {reject:?}")
            }
        }
    }
}

impl std::error::Error for InputError {}

/// The input state: who is playing, and who is watching.
///
/// It holds no clock and no decisions of its own: it turns the person's
/// events into decisions and hands them to the [`HostLoop`], which owns
/// the sequencer that stamps them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputState {
    /// Who is looking at the round.
    pub viewer: Viewer,
}

impl InputState {
    /// A fresh input state: a spectator with a free camera.
    pub fn new() -> Self {
        Self::default()
    }

    /// The agent being played, if any.
    pub fn possessing(&self) -> Option<u16> {
        self.viewer.possessing()
    }

    /// The agent the camera follows, if any.
    pub fn following(&self) -> Option<u16> {
        self.viewer.following()
    }

    /// Apply what the person did.
    ///
    /// Every event that reaches an agent becomes a decision submitted
    /// through the host's sequencer, so it lands on the next simulated tick
    /// and shows up in the round log. An event that means nothing right now
    /// is refused by name and changes nothing: a click while watching is
    /// [`InputError::NotPossessing`], and an agent that is not in the round
    /// is [`InputError::UnknownAgent`].
    pub fn apply(&mut self, ev: InputEvent, host: &mut HostLoop) -> Result<(), InputError> {
        match ev {
            InputEvent::Possess(agent) => self.possess(agent, host),
            InputEvent::Release => self.release(host),
            InputEvent::ClickCell(cell) => {
                let agent = self.possessing().ok_or(InputError::NotPossessing)?;
                host.submit_human(agent, Kind::ActionOn(Action::GoTo(cell)))
                    .map_err(InputError::Rejected)?;
                Ok(())
            }
            InputEvent::Hold => {
                let agent = self.possessing().ok_or(InputError::NotPossessing)?;
                host.submit_human(agent, Kind::ActionOn(Action::Hold))
                    .map_err(InputError::Rejected)?;
                Ok(())
            }
            InputEvent::Spectate => self.spectate(None, host),
            InputEvent::FollowAgent(agent) => {
                if host.sim().agent(agent).is_none() {
                    return Err(InputError::UnknownAgent { agent });
                }
                self.spectate(Some(agent), host)
            }
        }
    }

    /// Take over an agent, letting go of the one being played.
    ///
    /// The agent the human was playing goes back to its reflexes first: its
    /// orders were the human's, and the human is playing somebody else now.
    fn possess(&mut self, agent: u16, host: &mut HostLoop) -> Result<(), InputError> {
        if host.sim().agent(agent).is_none() {
            return Err(InputError::UnknownAgent { agent });
        }
        if let Some(previous) = self.possessing() {
            if previous != agent {
                host.submit_human(previous, Kind::ActionOff)
                    .map_err(InputError::Rejected)?;
            }
        }
        self.viewer = Viewer::Possessing(agent);
        Ok(())
    }

    /// Let go of the agent being played.
    ///
    /// The agent goes back to its reflexes with no leftover held action, and
    /// the camera keeps watching it: the human has just stopped playing it,
    /// not stopped looking at it.
    fn release(&mut self, host: &mut HostLoop) -> Result<(), InputError> {
        let agent = self.possessing().ok_or(InputError::NotPossessing)?;
        host.submit_human(agent, Kind::ActionOff)
            .map_err(InputError::Rejected)?;
        self.viewer = Viewer::Spectator {
            follow: Some(agent),
        };
        Ok(())
    }

    /// Stop playing and watch, with a free camera or one that follows an
    /// agent.
    ///
    /// Like a release, this hands the agent being played back to its
    /// reflexes: the human is not playing it any more.
    fn spectate(&mut self, follow: Option<u16>, host: &mut HostLoop) -> Result<(), InputError> {
        if let Some(previous) = self.possessing() {
            host.submit_human(previous, Kind::ActionOff)
                .map_err(InputError::Rejected)?;
        }
        self.viewer = Viewer::Spectator { follow };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lm_core::header::Header;
    use lm_tools::runner::DEFAULT_REFLEX;

    /// A standard round with the shipped reflex.
    fn round(seed: u64) -> Header {
        Header::default_round(seed, DEFAULT_REFLEX)
    }

    /// A host playing that round.
    fn host() -> HostLoop {
        HostLoop::new(round(1)).expect("the round starts")
    }

    #[test]
    fn a_new_state_is_a_spectator_with_a_free_camera() {
        let state = InputState::new();
        assert_eq!(state.viewer, Viewer::Spectator { follow: None });
        assert_eq!(state.possessing(), None);
        assert_eq!(state.following(), None);
    }

    #[test]
    fn possessing_an_agent_that_is_not_in_the_round_is_refused() {
        let mut state = InputState::new();
        let mut host = host();
        assert_eq!(
            state.apply(InputEvent::Possess(99), &mut host),
            Err(InputError::UnknownAgent { agent: 99 })
        );
        // The viewer did not change.
        assert_eq!(state.viewer, Viewer::Spectator { follow: None });
    }

    #[test]
    fn following_an_agent_that_is_not_in_the_round_is_refused() {
        let mut state = InputState::new();
        let mut host = host();
        assert_eq!(
            state.apply(InputEvent::FollowAgent(99), &mut host),
            Err(InputError::UnknownAgent { agent: 99 })
        );
        assert_eq!(state.viewer, Viewer::Spectator { follow: None });
    }

    #[test]
    fn holding_while_watching_is_refused() {
        let mut state = InputState::new();
        let mut host = host();
        assert_eq!(
            state.apply(InputEvent::Hold, &mut host),
            Err(InputError::NotPossessing)
        );
    }

    #[test]
    fn releasing_while_watching_is_refused() {
        let mut state = InputState::new();
        let mut host = host();
        assert_eq!(
            state.apply(InputEvent::Release, &mut host),
            Err(InputError::NotPossessing)
        );
    }

    #[test]
    fn playing_another_agent_releases_the_one_being_replaced() {
        let mut state = InputState::new();
        let mut host = host();
        state
            .apply(InputEvent::Possess(0), &mut host)
            .expect("the possession is accepted");
        state
            .apply(InputEvent::Hold, &mut host)
            .expect("the hold is accepted");
        host.tick();
        assert_eq!(
            host.sim().agent(0).expect("agent 0").held,
            Some(Action::Hold)
        );

        state
            .apply(InputEvent::Possess(1), &mut host)
            .expect("the possession is accepted");

        assert_eq!(state.viewer, Viewer::Possessing(1));
        // Agent 0's orders end with the human's attention: its held action
        // is released on the next tick.
        host.tick();
        assert_eq!(host.sim().agent(0).expect("agent 0").held, None);
    }

    #[test]
    fn taking_over_the_same_agent_again_changes_nothing() {
        let mut state = InputState::new();
        let mut host = host();
        state
            .apply(InputEvent::Possess(0), &mut host)
            .expect("the possession is accepted");
        state
            .apply(InputEvent::Hold, &mut host)
            .expect("the hold is accepted");
        host.tick();

        state
            .apply(InputEvent::Possess(0), &mut host)
            .expect("the possession is accepted");

        assert_eq!(state.viewer, Viewer::Possessing(0));
        // The agent was not let go of: its held action is still there.
        host.tick();
        assert_eq!(
            host.sim().agent(0).expect("agent 0").held,
            Some(Action::Hold)
        );
    }

    #[test]
    fn spectating_releases_the_agent_being_played() {
        let mut state = InputState::new();
        let mut host = host();
        state
            .apply(InputEvent::Possess(0), &mut host)
            .expect("the possession is accepted");
        state
            .apply(InputEvent::Hold, &mut host)
            .expect("the hold is accepted");
        host.tick();

        state
            .apply(InputEvent::Spectate, &mut host)
            .expect("the spectating is accepted");

        assert_eq!(state.viewer, Viewer::Spectator { follow: None });
        host.tick();
        assert_eq!(host.sim().agent(0).expect("agent 0").held, None);
    }

    #[test]
    fn following_an_agent_watches_it_from_the_outside() {
        let mut state = InputState::new();
        let mut host = host();
        state
            .apply(InputEvent::FollowAgent(3), &mut host)
            .expect("the following is accepted");
        assert_eq!(state.viewer, Viewer::Spectator { follow: Some(3) });
        assert_eq!(state.following(), Some(3));
    }
}
