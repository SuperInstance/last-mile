//! The voice map: `voice.toml`, the harness's own word on who says what
//! and how it sounds.
//!
//! The map is the whole of the game's opinion about audio. Which speaker
//! an agent's lines are rendered in is a mapping per agent; which emotion
//! style a line carries is a mapping per tone (the affect bucket the
//! intention gate's score becomes, bucketed by code) with an optional
//! per-intent override; where the skald daemon is and which engine it
//! runs are a mapping too, and the engine id joins every bank key, so a
//! swapped engine can never be served as the old one's clips.
//!
//! Every entry is a parameter with a default (D19): the shipped
//! `assets/harness/default/voice.toml` is doctrine a reviewer edits
//! between rounds, and the round pins it like every other harness file
//! (D27). A file that is missing or unparsable means the defaults, which
//! name the daemon on this Thor and the styles probed on 2026-10-29 —
//! the same ones the file ships with.

use lm_core::utterance::{Affect, Intent};
use serde::{Deserialize, Serialize};

use crate::VoiceConfigError;

/// The shipped voice map, as the harness's default directory holds it.
pub const SHIPPED_VOICE_TOML: &str = include_str!("../../../assets/harness/default/voice.toml");

/// Where the skald daemon is, and which engine it runs.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct SkaldConfig {
    /// The daemon's host, as a base URL.
    pub base: String,
    /// The daemon's port.
    pub port: u16,
    /// The engine's identity: hashed into every bank key.
    pub engine_id: String,
    /// The WAV rate the daemon is asked for.
    pub sample_rate_hz: u32,
    /// How long one render may take, in milliseconds.
    pub deadline_ms: u64,
}

impl Default for SkaldConfig {
    fn default() -> Self {
        SkaldConfig {
            base: "http://localhost".to_owned(),
            port: 8000,
            engine_id: "skald-0.1.0+riva-2.26.0-tegra-thor-magpie".to_owned(),
            sample_rate_hz: 22_050,
            deadline_ms: 8_000,
        }
    }
}

/// The speaker per agent: the table's own keys, which are the agent ids
/// as TOML spells them (`"0"`, `"4"`, ...) and the `default` every agent
/// the table does not name falls to.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct AgentsConfig {
    /// The speaker per key: an agent id, or `default`.
    pub entries: std::collections::BTreeMap<String, String>,
}

/// The emotion style per tone, with the optional per-intent override.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct StylesConfig {
    /// The style per affect bucket: `calm`, `urgent`, `uncertain`.
    pub tone: std::collections::BTreeMap<String, String>,
    /// The style per intent, which wins over the tone table when the
    /// line's intent is named here: `order`, `ack`, `warn`, `query`,
    /// `report`.
    pub intent: std::collections::BTreeMap<String, String>,
}

/// Which backend hears the human.
///
/// The names are the ones the configuration uses: `stub`, `http` and
/// `auto`. Anything else is refused by name -- a typo in the
/// configuration must not be a backend nobody asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AsrBackend {
    /// The deterministic stub: no network, no clock, the same words every
    /// time. This is the default, so a map that says nothing hears with
    /// the stub and the round plays exactly as it did before the ears
    /// existed.
    #[default]
    Stub,
    /// The ears on the Thor: the capture goes out and the words come
    /// back.
    Http,
    /// The ears when they are up at start-up, the stub when they are not.
    ///
    /// This is the realizer's `auto` in the same shape: the source that
    /// can do the work is used, and the deterministic one stands in when
    /// it cannot answer.
    Auto,
}

impl AsrBackend {
    /// The backend a name stands for.
    ///
    /// The name is refused by name: "htpp" is not "http", and a backend
    /// nobody asked for is not a backend that is quietly chosen.
    pub fn parse(name: &str) -> Result<AsrBackend, String> {
        match name.trim().to_ascii_lowercase().as_str() {
            "stub" => Ok(AsrBackend::Stub),
            "http" => Ok(AsrBackend::Http),
            "auto" => Ok(AsrBackend::Auto),
            other => Err(format!(
                "the ASR backend `{other}` is not one of stub, http or auto"
            )),
        }
    }

    /// The name the backend is written as in the configuration.
    pub fn as_str(&self) -> &'static str {
        match self {
            AsrBackend::Stub => "stub",
            AsrBackend::Http => "http",
            AsrBackend::Auto => "auto",
        }
    }
}

/// The ears, as the Thor's ears are by default.
pub const DEFAULT_EARS_URL: &str = "http://192.168.0.78:8102";

/// How long one call to the ears may take, in milliseconds, when the
/// configuration says nothing.
///
/// The deadline is generous, because the round does not wait for it: the
/// words are asked for on a thread and the line is scored when they come
/// back. The Thor's ears have answered in 0.3 s when the box was idle and
/// in 1.8-3.4 s when it was busy, so five seconds is above what they have
/// taken and below what a person waits for.
pub const DEFAULT_DEADLINE_MS: u32 = 5_000;

/// The language the ears are asked in, when the configuration names none.
pub const DEFAULT_LANGUAGE: &str = "en";

/// Where the ears are, and which of them hears.
///
/// One section, one set of answers: the game asks the ears on the Thor
/// and the intelligibility gate asks the same door whether the rendered
/// audio carries the words it was given. The gate's own fallbacks (Riva's
/// transcriptions path, the faster-whisper venv) are named here too, so
/// that one file says where every ear is.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct AsrConfig {
    /// Which backend hears the human.
    pub backend: AsrBackend,
    /// The base URL of the ears: the Thor's LFM3-Audio door.
    pub url: String,
    /// How long one call may take, in milliseconds.
    pub deadline_ms: u32,
    /// The language the ears are asked in.
    pub language: String,
    /// Riva's host: its HTTP surface has the transcriptions path, though
    /// this deployment carries TTS models only.
    pub riva_base: String,
    /// Riva's HTTP port.
    pub riva_port: u16,
    /// The faster-whisper venv's python, as skald's own smoke test uses
    /// it (`daemon/server/test_smoke.py`).
    pub whisper_python: String,
}

impl Default for AsrConfig {
    fn default() -> Self {
        AsrConfig {
            backend: AsrBackend::Stub,
            url: DEFAULT_EARS_URL.to_owned(),
            deadline_ms: DEFAULT_DEADLINE_MS,
            language: DEFAULT_LANGUAGE.to_owned(),
            riva_base: "http://localhost".to_owned(),
            riva_port: 50_000,
            whisper_python: "/tmp/whisperenv/bin/python3".to_owned(),
        }
    }
}

/// The voice map: the whole of `voice.toml`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct VoiceMap {
    /// The skald daemon and its engine.
    pub skald: SkaldConfig,
    /// The speaker per agent.
    pub agents: AgentsConfig,
    /// The style per tone and per intent.
    pub styles: StylesConfig,
    /// The ears: the one that hears the human, and the ones that verify
    /// the rendered audio.
    pub asr: AsrConfig,
}

impl VoiceMap {
    /// The map as the harness ships it.
    pub fn shipped() -> VoiceMap {
        VoiceMap::parse(SHIPPED_VOICE_TOML).expect("the shipped voice.toml parses")
    }

    /// The map from a TOML document.
    ///
    /// Every section is optional and every field has a default, so a map
    /// that says only what its author means to change is a whole map.
    pub fn parse(text: &str) -> Result<VoiceMap, VoiceConfigError> {
        toml::from_str(text).map_err(|err| VoiceConfigError::Parse(err.to_string()))
    }

    /// The map from a file.
    pub fn load(path: &std::path::Path) -> Result<VoiceMap, VoiceConfigError> {
        let text = std::fs::read_to_string(path)
            .map_err(|err| VoiceConfigError::Read(format!("{}: {err}", path.display())))?;
        VoiceMap::parse(&text)
    }

    /// The speaker an agent's lines are rendered in.
    ///
    /// The agent the table names is spoken as the table says; an agent it
    /// does not name falls to the table's `default`; and a table with
    /// neither falls to `mia`, the daemon's own default, so there is
    /// always a voice.
    pub fn voice_of(&self, agent: u16) -> &str {
        self.agents
            .entries
            .get(&agent.to_string())
            .or_else(|| self.agents.entries.get("default"))
            .map(String::as_str)
            .filter(|voice| !voice.is_empty())
            .unwrap_or("mia")
    }

    /// The emotion style a line is rendered in: the intent's override if
    /// the intent table names one, else the tone's style, else `Neutral`,
    /// the engine's own everyday voice.
    pub fn style_of(&self, affect: Affect, intent: Intent) -> String {
        if let Some(style) = self.styles.intent.get(intent_name(intent)) {
            return style.clone();
        }
        if let Some(style) = self.styles.tone.get(affect_name(affect)) {
            return style.clone();
        }
        "Neutral".to_owned()
    }
}

/// The TOML table's spelling of an affect bucket.
fn affect_name(affect: Affect) -> &'static str {
    match affect {
        Affect::Calm => "calm",
        Affect::Urgent => "urgent",
        Affect::Uncertain => "uncertain",
    }
}

/// The TOML table's spelling of an intent.
fn intent_name(intent: Intent) -> &'static str {
    match intent {
        Intent::Order => "order",
        Intent::Ack => "ack",
        Intent::Warn => "warn",
        Intent::Query => "query",
        Intent::Report => "report",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_map_names_every_agent_of_the_default_round() {
        let map = VoiceMap::shipped();
        // The default round is two squads of four: agents 0..8.
        for agent in 0..8u16 {
            assert!(!map.voice_of(agent).is_empty(), "agent {agent} has a voice");
        }
        // The leaders are spoken as the file says, the members as the
        // default.
        assert_eq!(map.voice_of(0), "mia");
        assert_eq!(map.voice_of(4), "aria");
        assert_eq!(map.voice_of(1), "jason");
        // An agent the roster grows by is not voiceless.
        assert_eq!(map.voice_of(99), "jason");
    }

    #[test]
    fn the_intent_override_wins_and_the_tone_answers_otherwise() {
        let map = VoiceMap::parse(
            "[styles.tone]\ncalm = \"Neutral\"\nurgent = \"Angry\"\nuncertain = \"Sad\"\n\
             [styles.intent]\nquery = \"Happy\"",
        )
        .expect("the map parses");
        assert_eq!(map.style_of(Affect::Urgent, Intent::Order), "Angry");
        // The intent's override beats the tone's style.
        assert_eq!(map.style_of(Affect::Calm, Intent::Query), "Happy");
        // And a table that says nothing means the engine's everyday voice.
        let bare = VoiceMap::default();
        assert_eq!(bare.style_of(Affect::Urgent, Intent::Warn), "Neutral");
    }

    #[test]
    fn the_engine_id_is_part_of_the_map() {
        let map = VoiceMap::shipped();
        assert!(map.skald.engine_id.contains("riva"), "the engine is named");
        assert_eq!(map.skald.port, 8000);
        assert!(!map.skald.base.is_empty());
    }

    #[test]
    fn a_broken_map_is_named_not_guessed() {
        assert!(matches!(
            VoiceMap::parse("not [ toml"),
            Err(VoiceConfigError::Parse(_))
        ));
    }

    #[test]
    fn the_ears_are_read_from_the_one_section() {
        let map = VoiceMap::parse(
            "[asr]\nbackend = \"http\"\nurl = \"http://192.168.0.78:8102\"\ndeadline_ms = 1500\n",
        )
        .expect("the map parses");

        assert_eq!(map.asr.backend, AsrBackend::Http);
        assert_eq!(map.asr.url, "http://192.168.0.78:8102");
        assert_eq!(map.asr.deadline_ms, 1_500);
        // And the language falls back to the default.
        assert_eq!(map.asr.language, DEFAULT_LANGUAGE);
        // The gate's own ears are still named in the same section.
        assert_eq!(map.asr.riva_port, 50_000);
    }

    #[test]
    fn a_map_that_says_nothing_hears_with_the_stub() {
        let map = VoiceMap::parse("[asr]").expect("an empty section parses");

        assert_eq!(map.asr.backend, AsrBackend::Stub);
        assert_eq!(map.asr.url, DEFAULT_EARS_URL);
        assert_eq!(map.asr.deadline_ms, DEFAULT_DEADLINE_MS);
        assert_eq!(map.asr.language, DEFAULT_LANGUAGE);
    }

    #[test]
    fn the_shipped_map_names_the_ears_of_the_thor() {
        let map = VoiceMap::shipped();

        // The shipped map is the one config: it says where the ears are,
        // and the game and the gate read the same section.
        assert_eq!(map.asr.url, DEFAULT_EARS_URL);
        assert_eq!(map.asr.backend, AsrBackend::Auto);
        assert!(map.asr.deadline_ms > 0, "the ears have a deadline");
    }

    #[test]
    fn a_backend_that_is_not_one_is_refused_by_name() {
        assert_eq!(AsrBackend::parse("stub"), Ok(AsrBackend::Stub));
        assert_eq!(AsrBackend::parse("HTTP"), Ok(AsrBackend::Http));
        assert_eq!(AsrBackend::parse(" auto "), Ok(AsrBackend::Auto));
        let err = AsrBackend::parse("htpp").expect_err("the typo is refused");
        assert!(err.contains("htpp"), "{err}");
    }

    #[test]
    fn a_backend_that_is_not_one_does_not_parse_into_the_map() {
        let err = VoiceMap::parse("[asr]\nbackend = \"htpp\"\n").expect_err("the typo is refused");
        assert!(matches!(err, VoiceConfigError::Parse(_)), "got {err:?}");
    }
}
