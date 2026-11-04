//! The human's voice: the microphone, the turn, and the line it becomes.
//!
//! The plugin is **thin on purpose**. The path lives where it can be
//! tested: the ASR and the prosody service in `lm-voice`, the turn, the
//! commands and the tutor ladder in `lm-host-model`. This file only wires
//! them to the window and to the round.
//!
//! The path is **push-to-talk → capture → ASR → prosody → a score → a
//! decision**:
//!
//! 1. the key goes down: the turn starts and the capture begins;
//! 2. the key comes up: the turn ends -- the release ends it, not a pause
//!    (fleet/08) -- and the capture is read;
//! 3. the capture is checked (a dead microphone is refused), written down
//!    by the ASR, and read as a command (a negation is refused, a locked
//!    verb is refused);
//! 4. what came out is a score, and the score is submitted as the human's
//!    decision: `source: Human(agent)`, `provenance: Human`. It is logged,
//!    hashed and replayed like any other decision.
//!
//! The audio is for the human and the sim never depends on it: with no
//! microphone, no ASR and no prosody service, the round plays exactly as
//! it did before.

use bevy::prelude::*;
use lm_core::decision::Kind;
use lm_host_model::turn::{TurnDetector, TurnRules};
use lm_host_model::voice::{CommandParser, HumanVoice, TutorLadder, TutorMode};
use lm_voice::{Capture, StubAsr};

use crate::input_plugin::Input;
use crate::sim_plugin::Round;

/// The key that talks: hold it and the microphone is open.
///
/// It is a key nobody else uses, so the push-to-talk never fights with the
/// map or the piano roll.
pub const PUSH_TO_TALK: KeyCode = KeyCode::KeyV;

/// The microphone: the samples of the capture, and whether it is running.
///
/// The device layer fills the samples; the host only reads them. With no
/// device there are no samples, and the capture is refused as a dead
/// microphone -- which is the honest answer, and it is why nothing is
/// submitted.
///
/// In **stub** mode the microphone makes up its own capture: a short ramp
/// that is not silent, so the whole path -- the health check, the ASR,
/// the command, the score -- runs with no hardware behind it. That is how
/// the app runs before there is a device to talk to.
#[derive(Resource, Default)]
pub struct Microphone {
    /// The samples of the capture, mono, 16 kHz.
    pub samples: Vec<i16>,
    /// Whether the capture is running.
    pub live: bool,
    /// Whether the microphone makes up its own capture.
    pub stub: bool,
}

impl Microphone {
    /// Take the samples of the capture, and start a new one.
    pub fn take(&mut self) -> Vec<i16> {
        self.live = false;
        let samples = std::mem::take(&mut self.samples);
        if samples.is_empty() && self.stub {
            return stub_capture();
        }
        samples
    }
}

/// A capture that is not silent: the microphone's stub.
///
/// It is a tenth of a second of a ramp, which is loud enough for the
/// health check to call the microphone alive.
fn stub_capture() -> Vec<i16> {
    (0..1_600i32).map(|i| (i % 64) as i16 - 32).collect()
}

/// The human's voice: the path, the turn, and what it last said.
///
/// The name is not `Voice`: Bevy's prelude has a `Voice` of its own, and
/// two types with one name is a fight nobody wins.
#[derive(Resource)]
pub struct VoiceState {
    /// The path: the capture, the ASR, the commands and the score.
    pub path: HumanVoice,
    /// The turn tracker.
    pub turn: TurnDetector,
    /// What the host said about the last thing the human said.
    pub status: Option<String>,
}

impl Default for VoiceState {
    /// The voice of the host: the stub ASR, the shipped ladder.
    ///
    /// The stub is what the app runs with when there is no voice service:
    /// it is deterministic, it needs no network, and the round plays the
    /// same way with it every time.
    fn default() -> Self {
        // The stub ASR hears one word: "hold", the first verb of the ladder.
        // With the stub microphone under it, the whole path runs in the app
        // with no hardware and no service behind it.
        let capture = Capture::new(Box::new(StubAsr::scripted("hold")));
        let parser = CommandParser::new(TutorLadder::shipped(), TutorMode::Ladder);
        // The turn's rules are the client's, and there is one copy of them:
        // the tracker's and the voice's are the same rules.
        let rules = TurnRules::default();
        VoiceState {
            path: HumanVoice::new(capture, parser),
            turn: TurnDetector::with_rules(rules),
            status: None,
        }
    }
}

/// The systems of the voice, as one set.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum VoiceSet {
    /// The microphone is read and the line is submitted.
    Read,
}

/// The voice plugin: the microphone, the turn and the decision.
pub struct VoicePlugin;

impl Plugin for VoicePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VoiceState>()
            .insert_resource(Microphone {
                samples: Vec::new(),
                live: false,
                stub: true,
            })
            .add_systems(Update, read_the_voice.in_set(VoiceSet::Read));
    }
}

/// Read the microphone and submit what the human said.
fn read_the_voice(
    mut voice: ResMut<VoiceState>,
    mut mic: ResMut<Microphone>,
    mut round: ResMut<Round>,
    input: Res<Input>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
) {
    let now_ms = (time.elapsed_secs() * 1000.0) as u64;

    // The key goes down: the turn starts and the capture begins.
    if keys.just_pressed(PUSH_TO_TALK) {
        voice.turn.on_press(now_ms);
        mic.samples.clear();
        mic.live = true;
        voice.status = None;
    }

    // The key comes up: the turn ends, and the capture is read.
    if !keys.just_released(PUSH_TO_TALK) {
        return;
    }
    voice.turn.on_release(now_ms);
    let samples = mic.take();

    // A round being replayed is watched, not played: there is no
    // sequencer to take the decision.
    let Some(host) = round.live.as_mut() else {
        voice.status = Some(String::from(
            "a replay is watched, not played: scrub it instead",
        ));
        return;
    };

    // The line is spoken for the agent the human is playing.
    let Some(agent) = input.0.possessing() else {
        voice.status = Some(String::from("play an agent before you talk to it"));
        return;
    };

    let talk = host.header().talk.clone();
    match voice.path.capture(&samples, agent, &talk) {
        Ok(utterance) => {
            // The score is the human's decision: it is logged, hashed and
            // replayed like any other.
            match host.submit_human(agent, Kind::Utterance(utterance)) {
                Ok(_) => {
                    // The prosody service is optional: say so when the line
                    // was scored without it.
                    voice.status = if voice.path.prosody_measured() {
                        None
                    } else {
                        Some(String::from(
                            "the line was scored without prosody: the service did not answer",
                        ))
                    };
                }
                Err(err) => voice.status = Some(format!("the line was refused: {err:?}")),
            }
        }
        Err(why) => voice.status = Some(why.to_string()),
    }
}
