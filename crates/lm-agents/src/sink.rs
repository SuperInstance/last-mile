//! Task 7: the seam between the agents and the host.
//!
//! The cognition loop never touches the sim. It hands its decisions to a
//! [`DecisionSink`], and the host picks them up and puts them in the
//! sequencer, where they become logged decisions like every other decision
//! (D26).
//!
//! M1 ships one sink: [`ChannelSink`], an in-process channel into
//! `HostLoop::submit_agent`. The later `lm-proto` socket sink implements the
//! same trait, so the loop does not change when the link does.

#![forbid(unsafe_code)]

use std::fmt;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Mutex;

use lm_core::decision::{Decision, Tick};

/// Where the agents' decisions go.
///
/// The trait is the whole contract between "how the agents think" and "what
/// runs the round": the loop never calls the sim, and the host never calls a
/// model.
pub trait DecisionSink: Send + Sync {
    /// Hand one decision over, stamped with the tick it arrived on.
    ///
    /// `arrived` is the first tick the host had not simulated yet. The
    /// sequencer uses it to tell a decision that is on time from one that is
    /// late (D22), so it has to be the tick the answer came home on, not the
    /// tick the question was asked on.
    fn submit(&mut self, decision: Decision, arrived: Tick);
}

/// An in-process sink: a channel into the host.
///
/// The channel is the reason the agents can be held by the host without
/// borrowing it: the loop writes decisions down, the host picks them up on
/// the next tick and puts them in the sequencer.
pub struct ChannelSink {
    tx: Sender<(Decision, Tick)>,
}

impl ChannelSink {
    /// A sink and the inbox the host drains.
    pub fn new() -> (ChannelSink, DecisionInbox) {
        let (tx, rx) = mpsc::channel();
        (ChannelSink { tx }, DecisionInbox { rx: Mutex::new(rx) })
    }
}

impl DecisionSink for ChannelSink {
    fn submit(&mut self, decision: Decision, arrived: Tick) {
        // The host dropped the inbox: the round is over and the decision has
        // nowhere to go. That is not worth breaking the round for.
        let _ = self.tx.send((decision, arrived));
    }
}

impl fmt::Debug for ChannelSink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChannelSink").finish_non_exhaustive()
    }
}

/// The host's end of the decision channel.
///
/// The inbox is behind a lock so the host can hold it in a resource that is
/// shared between threads (the Bevy round resource).
pub struct DecisionInbox {
    rx: Mutex<Receiver<(Decision, Tick)>>,
}

impl DecisionInbox {
    /// Every decision that has arrived since the last drain, oldest first.
    pub fn drain(&mut self) -> Vec<(Decision, Tick)> {
        let rx = self.rx.lock().expect("the decision inbox is not poisoned");
        let mut out = Vec::new();
        while let Ok(item) = rx.try_recv() {
            out.push(item);
        }
        out
    }

    /// How many decisions are waiting.
    pub fn len(&self) -> usize {
        self.rx
            .lock()
            .expect("the decision inbox is not poisoned")
            .try_iter()
            .count()
    }

    /// Whether nothing is waiting.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl fmt::Debug for DecisionInbox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DecisionInbox")
            .field("waiting", &self.len())
            .finish()
    }
}

// -------------------------------------------------------------------- the tests

#[cfg(test)]
mod tests {
    use super::*;
    use lm_core::decision::{Kind, Lane, Provenance, Source};

    fn a_decision(agent: u16) -> Decision {
        Decision {
            seq: 1,
            kind: Kind::Lane(Lane {
                gate: 0,
                menu_id: 3,
                values: vec![1, 0],
            }),
            apply_tick: 4,
            provenance: Provenance::System1 {
                model: "stub".to_owned(),
                tokens: 64,
                latency_ms: 3,
                situation_key: 7,
            },
            source: Source::Agent(agent),
        }
    }

    #[test]
    fn decisions_arrive_in_the_order_they_were_sent() {
        let (mut sink, mut inbox) = ChannelSink::new();
        sink.submit(a_decision(1), 10);
        sink.submit(a_decision(2), 10);

        let got = inbox.drain();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].0.source, Source::Agent(1));
        assert_eq!(got[0].1, 10);
        assert_eq!(got[1].0.source, Source::Agent(2));
    }

    #[test]
    fn a_drained_inbox_is_empty() {
        let (mut sink, mut inbox) = ChannelSink::new();
        sink.submit(a_decision(3), 1);
        assert_eq!(inbox.len(), 1);

        let _ = inbox.drain();
        assert!(inbox.is_empty());
        assert!(inbox.drain().is_empty());
    }

    #[test]
    fn a_dropped_inbox_does_not_break_the_loop() {
        let (mut sink, inbox) = ChannelSink::new();
        drop(inbox);
        // The round is over: the decision goes nowhere, and nothing panics.
        sink.submit(a_decision(4), 2);
    }
}
