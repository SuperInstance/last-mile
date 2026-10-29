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
use serde::Deserialize;

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

/// Where the intelligibility gate's ears are.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct AsrConfig {
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
    /// The intelligibility gate's ears.
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
}
