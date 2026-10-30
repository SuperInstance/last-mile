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
//!
//! ## The other half: what the agents say
//!
//! The round's talk is **half-duplex** (`lm-host-model::audio`): while the
//! human holds the key the agents go quiet, and they stay quiet for the hang
//! time after the key comes up. A clip that is dropped is dropped for good —
//! it is not queued, and it does not come back. The clips themselves are
//! asked for by the host model (`--talk off|stub|riva`), and this file only
//! plays them.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use bevy::prelude::*;
use lm_core::decision::Kind;
use lm_host_model::audio::{Duplex, DuplexGate, GateAction};
use lm_host_model::talk::Listener;
use lm_host_model::turn::{TurnDetector, TurnRules};
use lm_host_model::voice::{
    percentile, CommandParser, EarsOutcome, HumanVoice, TurnCost, TutorLadder, TutorMode,
};
use lm_voice::{Asr, AsrBackend, AsrConfig, Capture, HttpAsr, MicHealth, StubAsr};

use crate::input_plugin::Input;
use crate::sim_plugin::Round;

/// The key that talks: hold it and the microphone is open.
///
/// The key is `V`, and it is the talk's alone — every key the host binds
/// lives in one table ([`crate::keys`]), and a test there fails the day two
/// bindings share a key. That is what keeps the microphone from flipping
/// the 2D↔3D view under the person's voice.
pub use crate::keys::PUSH_TO_TALK;

/// How long the agents stay quiet after the human lets go of the key.
///
/// The hang is what stops the two from talking over each other: the human's
/// line is still in the room when the key comes up.
const HANG_MS: u32 = 400;

/// How long the human may talk in one go.
///
/// The turn ends on its own after this, so a stuck key cannot hold the
/// round hostage.
const MAX_TALK_MS: u32 = 6_000;

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
    /// A capture that came from a file: the test hook.
    ///
    /// It is taken by the same `take` as the device's samples, so the path
    /// after it is the same path.
    pub wav: Option<Vec<i16>>,
}

impl Microphone {
    /// Take the samples of the capture, and start a new one.
    pub fn take(&mut self) -> Vec<i16> {
        self.live = false;
        // A wave from the test hook is the capture: it is taken first, so
        // the hook wins over a device that is not there.
        if let Some(wav) = self.wav.take() {
            return wav;
        }
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
    /// The health of the last capture: how much of it was silence.
    ///
    /// It is the honest answer to "is the microphone there?", and the HUD
    /// shows it so a dead microphone is visible before a line is refused.
    pub mic: MicHealth,
    /// Which backend hears the human.
    pub backend: AsrBackend,
    /// The ears themselves, when the backend is one of the HTTP ones.
    ///
    /// The handle is a second copy of the same client, so the cost of the
    /// last call can be read without reaching into the capture.
    pub ears: Option<HttpAsr>,
    /// When the key came up on the turn in flight.
    pub key_up_ms: Option<u64>,
    /// The agent the turn in flight is for.
    pub speaker: Option<u16>,
    /// What every turn cost: the wall clock, and what it was made of.
    pub costs: Vec<TurnCost>,
}

impl Default for VoiceState {
    /// The voice of the host: the stub ASR, the shipped ladder.
    ///
    /// The stub is what the app runs with when there is no voice service:
    /// it is deterministic, it needs no network, and the round plays the
    /// same way with it every time.
    fn default() -> Self {
        VoiceState::with_asr(&AsrConfig::default())
    }
}

impl VoiceState {
    /// The voice of the host, over an ASR configuration.
    ///
    /// The three backends are the three shapes of the path:
    ///
    /// - `stub`: the deterministic one. The app runs exactly as it did
    ///   before the ears existed;
    /// - `http`: the ears on the Thor. The line is scored when the words
    ///   come back, and the stub stands in when they are late or down;
    /// - `auto`: the ears if the health probe says they are up at start-up,
    ///   the stub if they are not.
    ///
    /// The stub is always behind the ears: it is what a late or missing
    /// answer falls back to, so the line is never lost.
    pub fn with_asr(cfg: &AsrConfig) -> VoiceState {
        let parser = CommandParser::new(TutorLadder::shipped(), TutorMode::Ladder);
        let rules = TurnRules::default();
        // The stub ASR hears one word: "hold", the first verb of the
        // ladder. With the stub microphone under it, the whole path runs in
        // the app with no hardware and no service behind it.
        let stub: Arc<dyn Asr> = Arc::new(StubAsr::scripted("hold"));
        let http = HttpAsr::from_config(cfg);
        let (backend, capture, ears) = match cfg.backend {
            AsrBackend::Stub => (AsrBackend::Stub, Capture::new(Arc::clone(&stub)), None),
            AsrBackend::Http => (
                AsrBackend::Http,
                Capture::new(Arc::new(http.clone())),
                Some(http),
            ),
            AsrBackend::Auto => {
                // The probe is asked once, at start-up: the ears if they
                // answer, the stub if they do not.
                if http.healthy() {
                    (
                        AsrBackend::Http,
                        Capture::new(Arc::new(http.clone())),
                        Some(http),
                    )
                } else {
                    println!(
                        "lm-host: the ears are not up at {}: the stub hears instead",
                        http.base()
                    );
                    (AsrBackend::Stub, Capture::new(Arc::clone(&stub)), None)
                }
            }
        };
        // The turn's deadline is the ears' deadline plus a slack: the
        // thread's own answer is the one that lands first, and this is the
        // backstop for a thread that does not come back at all.
        let deadline = Duration::from_millis(u64::from(cfg.deadline_ms) + TURN_SLACK_MS);
        VoiceState {
            path: HumanVoice::new(capture, parser)
                .with_fallback(Arc::clone(&stub))
                .with_deadline(deadline),
            turn: TurnDetector::with_rules(rules),
            status: None,
            // No capture yet: the microphone is not known to be alive, and
            // the HUD says so.
            mic: lm_voice::mic_health(&[]),
            backend,
            ears,
            key_up_ms: None,
            speaker: None,
            costs: Vec::new(),
        }
    }

    /// What the last turn cost: the wall clock, and what it was made of.
    fn cost(&self, applied_ms: u64) -> TurnCost {
        let total_ms = self
            .key_up_ms
            .map(|up| applied_ms.saturating_sub(up))
            .unwrap_or(0);
        let call = self.ears.as_ref().and_then(|ears| ears.last_call());
        TurnCost {
            total_ms,
            network_ms: call.map(|call| call.network_ms()).unwrap_or(0),
            server_ms: call.map(|call| call.server_ms).unwrap_or(0),
        }
    }
}

/// The slack past the ears' own deadline, in milliseconds.
///
/// The thread's call has a deadline of its own, and this is the last word:
/// the answer lands before the deadline unless the thread is lost.
const TURN_SLACK_MS: u64 = 500;

/// The waves to play into the capture path: the test hook.
///
/// The hook feeds the capture path a wave instead of a device, so the
/// whole path -- the health check, the ears, the command, the score --
/// runs with no microphone and no human. The wave is played as a
/// push-to-talk turn, so the path after it is the real one.
#[derive(Resource, Default)]
pub struct MicWavs {
    /// The waves, in order.
    queue: VecDeque<Vec<i16>>,
    /// How many frames the hook has waited.
    frames: u32,
    /// Whether the waves are done: the hook has nothing left to play.
    pub finished: bool,
}

impl MicWavs {
    /// The hook with the waves to play.
    pub fn new(waves: Vec<Vec<i16>>) -> MicWavs {
        MicWavs {
            finished: waves.is_empty(),
            queue: waves.into(),
            frames: 0,
        }
    }
}

/// What the host hears, and what it says back.
///
/// The gate is the half-duplex rule: while the human holds the key the
/// agents go quiet, and they stay quiet for the hang time after the key
/// comes up. The counters are the HUD's: how much was played, and how much
/// the gate took away.
#[derive(Resource)]
pub struct Playback {
    /// The half-duplex gate.
    pub gate: DuplexGate,
    /// How many clips were played.
    pub played: u64,
    /// How many clips the gate dropped.
    pub dropped: u64,
}

impl Default for Playback {
    fn default() -> Self {
        Playback {
            gate: DuplexGate::new(Duplex::Half, HANG_MS, MAX_TALK_MS),
            played: 0,
            dropped: 0,
        }
    }
}

/// The played-clip log: the outside proof of playback.
///
/// Nobody in the harness can hear the ProArt's speakers, so when the
/// question is "did the voices actually play?" the answer is written down:
/// one line a clip the player was handed, with the samples it was handed,
/// the tick it aired, whose voice and style it was, whether it was the
/// bank's clip or the tone, and what the render cost. A clip the gate cut
/// is logged as cut, not played — the log is honest about what reached
/// the ear and what the half-duplex gate took away.
#[derive(Resource)]
pub struct VoiceLog {
    file: std::fs::File,
}

impl VoiceLog {
    /// The log over an already-created file (a header line is written, so
    /// an empty log names its own columns).
    pub fn open(mut file: std::fs::File) -> VoiceLog {
        use std::io::Write;
        let _ = writeln!(
            file,
            "# tick speaker outcome origin voice style samples rate dur_ms render_ms key text"
        );
        VoiceLog { file }
    }

    /// One line of the log: what the player was handed, or what the gate
    /// cut. A write that fails is named on the console and then given up
    /// on — the log is a diagnostic, never a dependency.
    fn line(
        &mut self,
        tick: lm_core::decision::Tick,
        speaker: u16,
        outcome: &str,
        clip: &lm_voice::TtsClip,
        spoken: &Option<lm_host_model::voices::Spoken>,
    ) {
        use std::io::Write;
        let (origin, voice, style, render_ms, key) = match spoken {
            Some(spoken) => (
                spoken.origin.as_str(),
                spoken.voice.as_str(),
                spoken.style.as_str(),
                spoken
                    .latency_ms
                    .map_or_else(|| "-".to_owned(), |ms| ms.to_string()),
                format!("{:016x}", spoken.key),
            ),
            None => ("stub", "-", "-", "-".to_owned(), "-".to_owned()),
        };
        let text = spoken
            .as_ref()
            .map_or_else(|| "-".to_owned(), |spoken| format!("{:?}", spoken.text));
        let written = writeln!(
            self.file,
            "{tick} a{speaker} {outcome} {origin} {voice} {style} {} {} {} {render_ms} {key} {text}",
            clip.pcm.len(),
            clip.sample_rate,
            clip.duration_ms,
        );
        if let Err(err) = written {
            eprintln!("lm-host: the voice log could not be written: {err}");
        }
    }
}

/// The systems of the voice, as one set.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum VoiceSet {
    /// The microphone is read and the line is submitted.
    Read,
    /// The clips are cut and played.
    Play,
}

/// The voice plugin: the microphone, the turn and the decision.
pub struct VoicePlugin {
    /// The ASR's rules: which backend hears, and where.
    pub asr: AsrConfig,
}

impl Plugin for VoicePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(VoiceState::with_asr(&self.asr))
            .init_resource::<Playback>()
            .insert_resource(Microphone {
                samples: Vec::new(),
                live: false,
                stub: true,
                wav: None,
            })
            .init_resource::<MicWavs>()
            // The hook feeds the capture before the voice reads it, so the
            // wave is the capture of the turn.
            .add_systems(
                Update,
                (feed_the_microphone, read_the_voice, apply_the_turn)
                    .chain()
                    .in_set(VoiceSet::Read),
            )
            .add_systems(
                Update,
                (cut_the_audio, play_the_clips)
                    .chain()
                    .in_set(VoiceSet::Play),
            )
            // The ear moves before the round ticks: the clips are asked for
            // by the round, and the round asks for the ones the ear can
            // hear.
            .add_systems(
                FixedUpdate,
                sync_the_listener.before(crate::sim_plugin::SimSet::Step),
            )
            .configure_sets(Update, VoiceSet::Read.before(VoiceSet::Play));
    }
}

/// Feed the capture path with a wave, when there is one waiting.
///
/// The wave is played as a push-to-talk turn: the key goes down and comes
/// up on the same frame, so the path is the real one -- the health check,
/// the ears, the command, the score. This is a test hook: it is how the
/// path is exercised with no microphone and no human.
fn feed_the_microphone(
    mut wavs: ResMut<MicWavs>,
    mut mic: ResMut<Microphone>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    voice: Res<VoiceState>,
) {
    if wavs.finished {
        return;
    }
    wavs.frames = wavs.frames.saturating_add(1);
    // The round gets a moment to settle before the first wave.
    if wavs.frames < WARMUP_FRAMES {
        return;
    }
    // One wave at a time: the hook waits for the turn in flight.
    if voice.path.turn_pending() {
        return;
    }
    let Some(wave) = wavs.queue.pop_front() else {
        wavs.finished = true;
        report_the_costs(&voice);
        return;
    };
    mic.wav = Some(wave);
    keys.press(PUSH_TO_TALK.key);
    keys.release(PUSH_TO_TALK.key);
}

/// How many frames the hook waits before the first wave.
const WARMUP_FRAMES: u32 = 10;

/// Read the microphone and begin the turn.
///
/// The turn is **begun** here, not finished: the ears are asked on a
/// thread, and the round goes on without them. The score is submitted when
/// the words come back.
fn read_the_voice(
    mut voice: ResMut<VoiceState>,
    mut mic: ResMut<Microphone>,
    mut playback: ResMut<Playback>,
    mut round: ResMut<Round>,
    input: Res<Input>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
) {
    let now_ms = (time.elapsed_secs() * 1000.0) as u64;

    // The key goes down: the turn starts and the capture begins.
    if keys.just_pressed(PUSH_TO_TALK.key) {
        voice.turn.on_press(now_ms);
        // The gate hears the key too: from now on the agents are quiet.
        playback.gate.on_human_press(now_ms);
        mic.samples.clear();
        mic.live = true;
        voice.status = None;
    }

    // The key comes up: the turn ends, and the capture is read.
    if keys.just_released(PUSH_TO_TALK.key) {
        voice.turn.on_release(now_ms);
        playback.gate.on_human_release(now_ms);
        let samples = mic.take();
        voice.mic = lm_voice::mic_health(&samples);
        voice.key_up_ms = Some(now_ms);
        begin_the_turn(&mut *voice, &mut *round, &input, &samples);
    }
}

/// Begin the turn: the ears are asked, and the round goes on.
///
/// Nothing is scored here: the words are not here yet. The line is
/// submitted when they come back.
fn begin_the_turn(voice: &mut VoiceState, round: &mut Round, input: &Input, samples: &[i16]) {
    // A round being replayed is watched, not played: there is no
    // sequencer to take the decision.
    let Some(host) = round.live.as_ref() else {
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
    voice.speaker = Some(agent);
    if let Err(why) = voice.path.begin(samples, agent, &talk) {
        voice.status = Some(why.to_string());
    }
}

/// Submit the line, when the ears have answered.
///
/// The round never waited: the score lands on the frame the words came
/// back on, and a line the ears could not hear in time falls back to the
/// stub, exactly as it did before they existed.
fn apply_the_turn(mut voice: ResMut<VoiceState>, mut round: ResMut<Round>, time: Res<Time>) {
    let now_ms = (time.elapsed_secs() * 1000.0) as u64;
    let Some(outcome) = voice.path.poll() else {
        return;
    };
    let cost = voice.cost(now_ms);
    voice.costs.push(cost);
    let report = voice.path.last_turn().cloned();
    if let Some(report) = &report {
        println!(
            "lm-host: the ears {} \"{}\" -- {} ms ({} ms network, {} ms compute; the ears took {} ms)",
            ears_word(report.outcome),
            report.text,
            cost.total_ms,
            cost.network_ms,
            cost.compute_ms(),
            cost.server_ms,
        );
    }
    let spoken = match outcome {
        Ok(utterance) => {
            let Some(agent) = voice.speaker else {
                return;
            };
            let Some(host) = round.live.as_mut() else {
                voice.status = Some(String::from(
                    "a replay is watched, not played: scrub it instead",
                ));
                return;
            };
            // The score is the human's decision: it is logged, hashed and
            // replayed like any other.
            match host.submit_human(agent, Kind::Utterance(utterance)) {
                Ok(_) => true,
                Err(err) => {
                    voice.status = Some(format!("the line was refused: {err:?}"));
                    return;
                }
            }
        }
        Err(why) => {
            voice.status = Some(why.to_string());
            return;
        }
    };
    if !spoken {
        return;
    }
    // The status says what the ears did, and whether the line was scored
    // with prosody: the prosody service is optional, and the host says so
    // when the line was scored without it.
    let ears = report.map(|report| report.outcome);
    voice.status = match ears {
        Some(EarsOutcome::Late) => Some(String::from(
            "the ears were late: the line fell back to the stub",
        )),
        Some(EarsOutcome::Down) => Some(String::from(
            "the ears were not there: the line fell back to the stub",
        )),
        _ if !voice.path.prosody_measured() => Some(String::from(
            "the line was scored without prosody: the service did not answer",
        )),
        _ => None,
    };
}

/// What the ears did, in a word.
fn ears_word(outcome: EarsOutcome) -> &'static str {
    match outcome {
        EarsOutcome::Heard => "heard",
        EarsOutcome::Late => "were late for",
        EarsOutcome::Down => "were not there for",
    }
}

/// Print what the turns cost.
///
/// The wall clock and the split: what the network cost, and what the
/// machine did. The percentiles are the nearest rank, so with twenty
/// turns p50 is the tenth and p95 the nineteenth.
fn report_the_costs(voice: &VoiceState) {
    if voice.costs.is_empty() {
        return;
    }
    let totals: Vec<u64> = voice.costs.iter().map(|cost| cost.total_ms).collect();
    let networks: Vec<u64> = voice.costs.iter().map(|cost| cost.network_ms).collect();
    let computes: Vec<u64> = voice.costs.iter().map(|cost| cost.compute_ms()).collect();
    let ears = voice.path.ears();
    println!(
        "lm-host: {} turns -- the ears heard {}, {} were late, {} were down",
        totals.len(),
        ears.heard,
        ears.late,
        ears.down
    );
    println!(
        "lm-host: the turn cost -- p50 {} ms, p95 {} ms (network p50 {} ms, p95 {} ms; compute p50 {} ms, p95 {} ms)",
        percentile(&totals, 50).unwrap_or(0),
        percentile(&totals, 95).unwrap_or(0),
        percentile(&networks, 50).unwrap_or(0),
        percentile(&networks, 95).unwrap_or(0),
        percentile(&computes, 50).unwrap_or(0),
        percentile(&computes, 95).unwrap_or(0),
    );
}

/// Move the human's ear.
///
/// The person plays an agent, and hears as that agent hears: where it
/// stands, and the squad it belongs to. Watching the round, the person
/// listens to the first squad's radio instead. Either way the ear moves,
/// and the clips follow it.
fn sync_the_listener(mut round: ResMut<Round>, input: Res<Input>) {
    let Some(host) = round.live.as_mut() else {
        return;
    };
    let listener = match input.0.possessing() {
        Some(agent) => Listener::As(agent),
        None => Listener::Squad(0),
    };
    host.set_listener(listener);
}

/// Cut the audio when the human takes the key.
///
/// The clip that was playing stops where it is, and it does not come back:
/// the gate drops the rest of the line, and the dropped clip is not queued.
fn cut_the_audio(sinks: Query<&AudioSink>, keys: Res<ButtonInput<KeyCode>>) {
    if !keys.just_pressed(PUSH_TO_TALK.key) {
        return;
    }
    for sink in &sinks {
        sink.stop();
    }
}

/// Play the clips the round asked for.
///
/// The gate decides: while the human holds the key the agents are quiet, and
/// for the hang time after the key comes up. A clip the gate drops is
/// dropped for good — it never comes back, and it is never queued.
///
/// Every clip the player is handed is written to the played-clip log when
/// one is open, with its samples: the outside proof of playback (nobody in
/// the harness can hear this machine's speakers).
fn play_the_clips(
    mut round: ResMut<Round>,
    mut playback: ResMut<Playback>,
    mut sources: ResMut<Assets<AudioSource>>,
    mut commands: Commands,
    mut log: Option<ResMut<VoiceLog>>,
    time: Res<Time>,
) {
    let Some(host) = round.live.as_mut() else {
        return;
    };
    let clips = host.take_clips();
    if clips.is_empty() {
        return;
    }
    let now_ms = (time.elapsed_secs() * 1000.0) as u64;
    for clip in clips {
        if playback.gate.on_agent_clip(now_ms) == GateAction::Drop {
            playback.dropped += 1;
            if let Some(log) = log.as_mut() {
                log.line(clip.tick, clip.speaker, "cut", &clip.clip, &clip.spoken);
            }
            continue;
        }
        // A clip with no samples is not a sound, and a sound with no rate is
        // not a wave: the player would choke on it, so it is not played.
        if clip.clip.pcm.is_empty() || clip.clip.sample_rate == 0 {
            continue;
        }
        if let Some(log) = log.as_mut() {
            log.line(clip.tick, clip.speaker, "played", &clip.clip, &clip.spoken);
        }
        // The clip is raw samples; the player wants a wave.
        let wave = lm_voice::prosody::wav_bytes(&clip.clip.pcm, clip.clip.sample_rate);
        let handle = sources.add(AudioSource { bytes: wave.into() });
        commands.spawn((
            AudioPlayer::new(handle),
            PlaybackSettings {
                mode: bevy::audio::PlaybackMode::Despawn,
                ..default()
            },
        ));
        playback.played += 1;
    }
}
