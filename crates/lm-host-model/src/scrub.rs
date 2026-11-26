//! The replay scrubber: a round log you can open and seek in.
//!
//! A round that was recorded can be played again: the scrubber opens the log,
//! replays it the way the headless tools do, and hands back the world of any
//! tick the viewer asks for. What it shows is the sim's own state, not a
//! picture of it -- the same [`lm_core::sim::Sim`] the round was played on,
//! rebuilt from the log's header and the log's decisions.
//!
//! ## Two promises
//!
//! * **What it shows is what was recorded.** The log is replayed before it is
//!   handed over, and a log whose replay does not match its recorded hashes
//!   is refused: it does not describe the round it claims to.
//! * **A bad file is named, never crashed on.** A truncated file, a file
//!   that is not a round log, a file from another version and a header this
//!   build cannot play all come back as a [`lm_core::log::LogError`] the
//!   viewer can show.
//!
//! ## Seeking
//!
//! [`Scrubber::seek`] replays the round from its first tick to the tick
//! asked for and stops there. The tick asked for is clamped to the last tick
//! of the round, so seeking past the end shows the end of the round instead
//! of running off it. Seeking backwards starts the round again: the sim is
//! rebuilt from the header and replayed to the tick asked for. The state it
//! arrives at is the same either way, because the round is deterministic.
//!
//! The scrubber holds the events of the round as well, so the piano roll can
//! be drawn from a round that is being replayed exactly as it is drawn from
//! a round that is being played.

use std::fmt;
use std::path::Path;

use lm_core::decision::Tick;
use lm_core::log::{LogError, RoundLog};
use lm_core::rules::Event;
use lm_core::sim::Sim;
use lm_tools::runner::{self, ReplayError};

use crate::score::{note_attention, AttentionTrace};

/// A round log you can scrub through.
pub struct Scrubber {
    /// The round, as it was recorded.
    log: RoundLog,
    /// The world at the tick being shown.
    sim: Sim,
    /// The tick being shown: the last tick that was simulated.
    shown: Option<Tick>,
    /// The state hash of the tick being shown, if that tick took one.
    hash: Option<u64>,
    /// The events of the round, with their ticks.
    events: Vec<(Tick, Event)>,
    /// The attention of every agent over the round, sampled by the opening
    /// replay: what the conductor's score reads its gaze and tier staves
    /// from, exactly as a played round samples them as it goes.
    attention: AttentionTrace,
}

impl Scrubber {
    /// Open a round log.
    ///
    /// The file is read, its hash chain checked, its header played and its
    /// round replayed. Anything wrong with it comes back as a
    /// [`LogError`], named:
    ///
    /// * a file that is not a round log -- or that cannot be read at all --
    ///   is [`LogError::BadMagic`];
    /// * a file from another version is [`LogError::Version`];
    /// * a file that ends in the middle of a blob is
    ///   [`LogError::Truncated`];
    /// * a header this build cannot play is [`LogError::Decode`], named at
    ///   the header blob;
    /// * a log whose replay does not match its recorded hashes is
    ///   [`LogError::ChainBroken`], named at the blob it broke in.
    pub fn open(path: &Path) -> Result<Scrubber, LogError> {
        // A file that cannot be read is not a round log: it is not a log at
        // all, and that is what the viewer has to say.
        let mut file = std::fs::File::open(path).map_err(|_| LogError::BadMagic)?;
        let log = runner::read_log(&mut file).map_err(read_error)?;

        // The header has to be playable, or there is nothing to show.
        let sim = Sim::new(&log.header).map_err(|_| LogError::Decode { segment: 0 })?;

        // The log has to describe the round it claims to: the same replay,
        // with the same verdict, that the headless tools give it.
        runner::replay(&log).map_err(|err| log_error(&log, err))?;

        // The events of the round, for the piano roll — and the attention
        // of every agent, for the score, sampled by the same replay.
        let (events, attention) = scan_of(&log);

        Ok(Scrubber {
            log,
            sim,
            shown: None,
            hash: None,
            events,
            attention,
        })
    }

    /// Show the world of a tick.
    ///
    /// The tick is clamped to the last tick of the round, so a viewer that
    /// asks for a tick past the end is shown the end of the round instead of
    /// running off it. The world of that tick is handed back.
    pub fn seek(&mut self, tick: Tick) -> &Sim {
        // A round with no ticks at all has nothing to show.
        if self.log.segments.is_empty() {
            return &self.sim;
        }

        let target = tick.min(self.last_tick());

        // Backwards: the round starts again from its header.
        if self.shown.is_none_or(|shown| shown > target) {
            self.sim = self.fresh();
            self.shown = None;
            self.hash = None;
        }

        // Forwards: the round is replayed to the tick asked for.
        let mut next = match self.shown {
            Some(shown) => shown + 1,
            None => self.log.header.start_tick,
        };
        while next <= target {
            let out = self.sim.step(self.log.decisions_for(next));
            self.hash = out.hash;
            self.shown = Some(next);
            next += 1;
        }
        &self.sim
    }

    /// The number of ticks in the round.
    ///
    /// The last tick of the round is `len() - 1`; a round with no ticks at
    /// all is empty.
    pub fn len(&self) -> Tick {
        match self.log.segments.last() {
            Some(segment) => segment
                .last_tick
                .saturating_sub(self.log.header.start_tick)
                .saturating_add(1),
            None => 0,
        }
    }

    /// Whether the round has no ticks at all.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The last tick of the round.
    pub fn last_tick(&self) -> Tick {
        self.log
            .segments
            .last()
            .map_or(self.log.header.start_tick, |segment| segment.last_tick)
    }

    /// The tick being shown: the last tick that was simulated.
    ///
    /// `None` before the first seek: the world is the one the round started
    /// with.
    pub fn tick(&self) -> Option<Tick> {
        self.shown
    }

    /// The state hash of the tick being shown, if that tick took one.
    ///
    /// A hash is taken every `hash_interval_ticks` ticks, so most ticks have
    /// none.
    pub fn hash(&self) -> Option<u64> {
        self.hash
    }

    /// The world at the tick being shown.
    pub fn sim(&self) -> &Sim {
        &self.sim
    }

    /// The round, as it was recorded.
    pub fn log(&self) -> &RoundLog {
        &self.log
    }

    /// The events of the round, with their ticks.
    ///
    /// The piano roll is drawn from these, exactly as it is drawn from the
    /// events of a round that is being played.
    pub fn events(&self) -> &[(Tick, Event)] {
        &self.events
    }

    /// The attention of every agent over the round, with the tick each run
    /// began on.
    ///
    /// The conductor's score is drawn from this, exactly as it is drawn
    /// from the samples a played round takes as it goes.
    pub fn attention(&self) -> &AttentionTrace {
        &self.attention
    }

    /// A world at the start of the round.
    ///
    /// The header was played when the log was opened, so this cannot fail.
    fn fresh(&self) -> Sim {
        Sim::new(&self.log.header).expect("the header was played when the log was opened")
    }
}

impl fmt::Debug for Scrubber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Scrubber")
            .field("tick", &self.shown)
            .field("hash", &self.hash)
            .field("ticks", &self.len())
            .field("events", &self.events.len())
            .finish()
    }
}

/// The events of a round with their ticks, and the attention of every
/// agent, found by one replay of the log.
///
/// The round is replayed to find them, because the log holds the decisions
/// and the hashes, but neither the events the rules reported nor the
/// agents' own state as it stood. The replay is the same one the headless
/// tools judge a log by, so what it samples is what the round played.
fn scan_of(log: &RoundLog) -> (Vec<(Tick, Event)>, AttentionTrace) {
    let mut events = Vec::new();
    let mut attention = AttentionTrace::new();
    let Ok(mut sim) = Sim::new(&log.header) else {
        return (events, attention);
    };
    for segment in &log.segments {
        for tick in segment.first_tick..=segment.last_tick {
            let out = sim.step(log.decisions_for(tick));
            note_attention(&sim, &mut attention, tick);
            for event in out.events {
                events.push((tick, event));
            }
        }
    }
    (events, attention)
}

/// The log error a read error stands for.
///
/// Reading a file can only fail on the file itself, so the reader's own
/// error is the one that is shown.
fn read_error(err: ReplayError) -> LogError {
    match err {
        ReplayError::Log(err) => err,
        // The reader does not replay, so it cannot report anything else.
        _ => LogError::Decode { segment: 0 },
    }
}

/// The log error a replay error stands for.
///
/// The log errors name what is wrong with a file; the replay errors name
/// what is wrong with a round. The second kind is folded into the first, so
/// the viewer only ever has one error to show.
fn log_error(log: &RoundLog, err: ReplayError) -> LogError {
    match err {
        ReplayError::Log(err) => err,
        // The log decodes, but the round it describes cannot be played: the
        // header names a ruleset this build does not have, breaks a
        // parameter or carries a script that does not compile. The header is
        // the blob that cannot be used.
        ReplayError::Setup(_) => LogError::Decode { segment: 0 },
        // The replay saw a different world than the log recorded, so the log
        // does not describe the round it claims to. That is a broken chain
        // of hashes, named at the blob it broke in.
        ReplayError::Diverged { tick, .. } => {
            let segment = log
                .segments
                .iter()
                .position(|segment| tick >= segment.first_tick && tick <= segment.last_tick)
                .map_or(1, |index| index + 1);
            LogError::ChainBroken { segment }
        }
    }
}
