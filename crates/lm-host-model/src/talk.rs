//! The host's talk: the clips the human hears, and the counters the HUD
//! shows.
//!
//! The round's talk and the human's audio are two different things, and this
//! module keeps them apart:
//!
//! * the **score** is the truth. What was said, who heard it and what they
//!   understood is `lm-core`'s business, and the round replays the same way
//!   whether or not a voice service is running;
//! * the **audio** is for the person. A line is synthesized **only** when the
//!   human's ear can hear it, and only through the bark cache, so the same
//!   line in the same voice with the same tone never reaches the voice
//!   service twice.
//!
//! ## The ear
//!
//! The ear is the human's listener position: whose hearing decides what the
//! person is told. It is one of three things:
//!
//! * [`Listener::As`] — the person plays an agent and hears what that agent
//!   hears: where the agent stands, and the agent's squad;
//! * [`Listener::Squad`] — the person watches and listens to a squad's
//!   radio: the squad's own lines, and whatever reaches the ear of the
//!   squad's first agent;
//! * [`Listener::Deaf`] — the person hears nothing: the round still talks,
//!   and it is silent for them.
//!
//! ## Nothing here decides anything
//!
//! The clips are a consequence of the round, never an input to it. A voice
//! service that is down, slow or absent changes what the person hears and
//! nothing else: the sim never waits for it, and the log never mentions it.

use std::collections::BTreeMap;
use std::sync::Arc;

use lm_core::decision::Tick;
use lm_core::talk::TalkMetrics;
use lm_core::utterance::Utterance;
use lm_voice::{BarkCache, Tts, TtsClip, TtsRequest, VoiceError};

/// The voice the agents' lines are spoken in when nothing says otherwise.
pub const DEFAULT_VOICE: &str = "default";

/// The human's ear: whose hearing decides what the person is told.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Listener {
    /// The person hears as this agent hears: where the agent stands, and the
    /// squad the agent belongs to.
    As(u16),
    /// The person listens to a squad's radio: the squad's own lines, and
    /// whatever reaches the ear of the squad's first agent.
    Squad(u8),
    /// The person hears nothing: the round is silent for them.
    ///
    /// This is the default: an ear is only given when somebody asks for one.
    #[default]
    Deaf,
}

/// A clip the host asked for, and that is waiting to be played.
///
/// The clip carries the line it speaks, so the player can see who said what
/// while it is being spoken.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TalkClip {
    /// The canonical id of the line the clip speaks.
    pub line: u64,
    /// The agent who said the line.
    pub speaker: u16,
    /// The tick the line started on.
    pub tick: Tick,
    /// The clip itself.
    pub clip: TtsClip,
}

/// The talk counters the HUD shows.
///
/// Every number here is counted, not estimated: the utterances and the
/// airtime come from the round's own records, and the clips from the bark
/// cache's own counters.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TalkCounters {
    /// How many lines the round has spoken.
    pub utterances: u64,
    /// How many ticks of airtime the round has spent.
    pub airtime_ticks: u64,
    /// The airtime of every agent that spoke, in ticks.
    pub airtime_by_agent: BTreeMap<u16, u64>,
    /// How many orders were spoken.
    pub orders: u64,
    /// How many orders were taken up.
    pub orders_taken_up: u64,
    /// How many clips the host asked for.
    pub clips: u64,
    /// How many clips the bark cache answered from itself.
    pub bark_hits: u64,
    /// How many clips the bark cache had to synthesize.
    pub bark_misses: u64,
    /// What the voice backend said last, when it said something.
    pub voice: Option<String>,
}

impl TalkCounters {
    /// The counters of a round that has talked nothing yet.
    pub fn of(metrics: &TalkMetrics) -> TalkCounters {
        TalkCounters {
            utterances: metrics.utterances,
            airtime_ticks: metrics.airtime_ticks,
            airtime_by_agent: metrics.airtime_by_agent.clone(),
            orders: metrics.orders,
            orders_taken_up: metrics.orders_taken_up,
            ..TalkCounters::default()
        }
    }
}

/// The host's talk: the voice backend, the bark cache and the human's ear.
pub struct TalkConfig {
    /// The voice backend.
    ///
    /// `None` means no audio at all: the round still talks, and nobody hears
    /// it. This is the `off` of `--talk off`.
    pub tts: Option<Arc<dyn Tts>>,
    /// The bark cache: the same line, in the same voice, with the same tone
    /// never reaches the voice service twice.
    pub bark: BarkCache,
    /// The human's ear.
    pub listener: Listener,
    /// The voice the lines are spoken in.
    pub voice: String,
    /// What the voice backend said last, when it said something.
    pub status: Option<String>,
}

impl Default for TalkConfig {
    fn default() -> Self {
        TalkConfig {
            tts: None,
            bark: BarkCache::default(),
            listener: Listener::Deaf,
            voice: DEFAULT_VOICE.to_owned(),
            status: None,
        }
    }
}

impl TalkConfig {
    /// The talk of a round with a voice backend behind it.
    ///
    /// The backend is shared, so the host can keep it in a resource and hand
    /// the same handle to every round it plays.
    pub fn with_tts(mut self, tts: Arc<dyn Tts>) -> TalkConfig {
        self.tts = Some(tts);
        self
    }

    /// The talk with an ear: whose hearing decides what the person is told.
    pub fn with_listener(mut self, listener: Listener) -> TalkConfig {
        self.listener = listener;
        self
    }

    /// The talk in a named voice.
    pub fn with_voice(mut self, voice: impl Into<String>) -> TalkConfig {
        self.voice = voice.into();
        self
    }

    /// The clip of a line, from the bark cache.
    ///
    /// The cache is the whole point: the same line never reaches the voice
    /// service twice. A backend that refuses says so, and the refusal is
    /// kept as the talk's status — the round does not care either way.
    pub fn speak(&mut self, line: &Utterance) -> Result<TtsClip, VoiceError> {
        let Some(tts) = self.tts.clone() else {
            return Err(VoiceError::NotConfigured);
        };
        let request = TtsRequest::from_utterance(line, &self.voice);
        let clip = self.bark.synthesize(tts.as_ref(), &request)?;
        Ok(clip)
    }
}
