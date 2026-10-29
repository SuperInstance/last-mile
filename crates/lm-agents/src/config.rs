//! Every tunable of the agents' cognition, with the phase-3 defaults.
//!
//! D19: any value with more than one workable choice is a parameter. The
//! defaults are the phase-3 plan's Task 1 table; an optional `agents.toml`
//! overrides any subset of the fields — a missing file is simply the
//! defaults, and a present file replaces each field it names (a table
//! field, `ports` or `min_confidence`, is replaced whole).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

/// Why an `agents.toml` could not be loaded. A missing file is not an
/// error: the file is optional, and its absence means the defaults.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigError {
    /// The file exists but cannot be read.
    Read(String),
    /// The file reads but does not parse as an [`AgentsConfig`].
    Parse(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Read(why) => write!(f, "agents config unreadable: {why}"),
            ConfigError::Parse(why) => write!(f, "agents config unparseable: {why}"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// All agent-cognition tunables (phase 3, Task 1), in the plan's table
/// order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentsConfig {
    /// The node the cognition loop runs on (D51; the models are reached
    /// over HTTP either way).
    pub node: String,
    /// The System One backend member agents ask.
    pub backend_member: String,
    /// The System One backend squad leaders ask.
    pub backend_leader: String,
    /// The Thor's LAN base URL; its model ports hang off it.
    pub thor_base: String,
    /// Model port per Thor endpoint, by backend name.
    pub ports: BTreeMap<String, u16>,
    /// Confidence floor per backend: an answer below its backend's floor is
    /// a refusal, never a decision (D26/D49 — each model its own threshold,
    /// never averaged).
    pub min_confidence: BTreeMap<String, f32>,
    /// The confidence below which a gate is escalated to a second model
    /// (D71).
    ///
    /// The cheap model's own confidence decides when to pay for the second
    /// opinion: a gate it answers below this is not acted on alone.
    pub escalate_below: f32,
    /// The backend that answers an escalated gate (D71).
    ///
    /// It is asked that gate **alone**, one question in one call, so its
    /// pack always fits.
    pub escalate_backend: String,
    /// Member think rate in Hz. 1 by default (D68): the Thor serves ~9
    /// requests/s for the whole match, so 8 agents asking at 4 Hz only queue
    /// up and go stale; at 1 Hz they get ~93% of the decisions with headroom
    /// (crates/lm-agents/tests/think_budget.md).
    pub think_hz_member: u16,
    /// Leader think rate in Hz.
    pub think_hz_leader: u16,
    /// The cognition mode: `point`, `presence` or `auto` (D38).
    pub mode: String,
    /// Presence-mode batch rate in Hz (D38/D42).
    pub presence_hz: u16,
    /// Questions per ask (d1 cost is 10–50 ms per question).
    ///
    /// The phase-4 pack asks the move gate, the five talk gates and their
    /// five presence gates, so the whole pack is eleven questions: a
    /// smaller cap would leave the talk gates unanswered and the agents
    /// would never speak.
    pub max_questions: u16,
    /// HTTP deadline per ask, in milliseconds; later than that is a refusal.
    ///
    /// The deadline follows the pack: the eleven-question pack of phase 4
    /// is a longer prompt than the one gate of phase 3, and a deadline
    /// that fits the old pack would refuse every real answer.
    ///
    /// Measured on the Thor's d1-2B: the eleven-question pack costs 600 ms
    /// on an idle box and 1400 ms when the box is busy, so the deadline has
    /// to clear the slow case and not only the median
    /// (crates/lm-agents/tests/talk_on_thor.md).
    ///
    /// D70 split the pack, and D71 added a second opinion: the talk pack
    /// costs 1350 ms on the d1-2B and the escalated gate is another 550 ms
    /// of clef-flash on top of it, so the deadline has to clear both. At
    /// 2400 ms half of the escalations died on the way (measured: 43 taken,
    /// 40 timed out), and the escalated gate stayed unsure.
    pub request_timeout_ms: u32,
    /// How strongly a squad's shared usage counts in queue priority (D16).
    pub squad_weight: f32,
    /// Model calls each agent may spend per round (D24).
    pub budget_calls_per_round: u32,
    /// What an exhausted budget does: `reflex` keeps the reflexes playing,
    /// `freeze` freezes (D24).
    pub on_exhaust: String,
    /// An enemy sighting older than this many ticks is stale: [`crate::facts`]
    /// omits it (D49). Was a constant in phase 3 Task 1; a parameter since
    /// Task 5.
    pub stale_enemy_ticks: u32,
    /// Allies farther away than this many cells (Chebyshev) are not stated in
    /// the facts. Was a constant; a parameter since Task 5.
    pub ally_range_cells: i32,
    /// How the agents word their lines (D37).
    #[serde(default)]
    pub utter: UterConfig,
}

/// The rules of the wording: which realizer words the lines, and which
/// model writes them when the phrasebook cannot.
///
/// The choice itself is not here: the gates make it (D37). These are the
/// knobs of the *wording*.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UterConfig {
    /// Which realizer words the lines: `template` (the phrasebook),
    /// `llm` (the model on the Thor) or `auto` (the phrasebook first, the
    /// model only for a line the phrasebook does not have).
    pub realizer: String,
    /// The model that writes the lines, on the Thor.
    pub llm_model: String,
    /// The port that model serves on.
    pub llm_port: u16,
}

impl Default for UterConfig {
    fn default() -> Self {
        UterConfig {
            realizer: "auto".to_owned(),
            llm_model: "LFM2-1.2B".to_owned(),
            llm_port: 8092,
        }
    }
}

impl Default for AgentsConfig {
    fn default() -> Self {
        Self {
            node: "proart".to_owned(),
            backend_member: "thor-d1-2b".to_owned(),
            backend_leader: "thor-d1-2b".to_owned(),
            thor_base: "http://192.168.0.78".to_owned(),
            ports: BTreeMap::from([
                ("d1-omni-600m".to_owned(), 8095),
                ("d1-2b".to_owned(), 8094),
                ("d1-3b".to_owned(), 8093),
                ("clef-flash".to_owned(), 8096),
            ]),
            min_confidence: BTreeMap::from([
                // The d1-2B's own calibration, measured over a round: the
                // move gate answers 0.80-0.94 confident, but the attention
                // gate — ten options, so unsure by nature — answers
                // 0.29-0.64. The floor is per gate now, so it has to clear
                // the model's weakest gate and not only its best one
                // (crates/lm-agents/tests/talk_on_thor.md).
                ("thor-d1-2b".to_owned(), 0.30),
                ("thor-d1-3b".to_owned(), 0.55),
                // The second opinion (D71). Its confidence scale runs about
                // half of the d1-2B's on the same gates — the attention gate
                // reads 0.13 against the d1-2B's 0.33 — so the d1-2B's 0.30
                // floor has no counterpart on it: a second opinion that
                // answered is taken, and the gate stays unsure only when the
                // second model is down, slow or shrugs.
                ("thor-clef-flash".to_owned(), 0.05),
                ("jev".to_owned(), 0.5),
            ]),
            escalate_below: 0.5,
            escalate_backend: "thor-clef-flash".to_owned(),
            think_hz_member: 1,
            think_hz_leader: 1,
            mode: "auto".to_owned(),
            presence_hz: 10,
            max_questions: 11,
            request_timeout_ms: 4_000,
            squad_weight: 0.5,
            budget_calls_per_round: 400,
            on_exhaust: "reflex".to_owned(),
            stale_enemy_ticks: 6,
            ally_range_cells: 4,
            utter: UterConfig::default(),
        }
    }
}

impl AgentsConfig {
    /// Parse an `agents.toml`'s text; missing fields fall back to the
    /// defaults.
    pub fn from_toml(text: &str) -> Result<Self, ConfigError> {
        toml::from_str(text).map_err(|e| ConfigError::Parse(e.to_string()))
    }

    /// Load `path` if it exists; a missing file is the defaults (the file
    /// is optional), anything else is an error.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::from_toml(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(ConfigError::Read(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_plan_table() {
        let c = AgentsConfig::default();
        assert_eq!(c.node, "proart");
        assert_eq!(c.backend_member, "thor-d1-2b");
        assert_eq!(c.backend_leader, "thor-d1-2b");
        assert_eq!(c.thor_base, "http://192.168.0.78");
        assert_eq!(
            c.ports,
            BTreeMap::from([
                ("d1-omni-600m".to_owned(), 8095),
                ("d1-2b".to_owned(), 8094),
                ("d1-3b".to_owned(), 8093),
                ("clef-flash".to_owned(), 8096),
            ])
        );
        assert_eq!(
            c.min_confidence,
            BTreeMap::from([
                ("thor-d1-2b".to_owned(), 0.30),
                ("thor-d1-3b".to_owned(), 0.55),
                // The second opinion (D71): its confidence scale runs about
                // half of the d1-2B's, so its floor is lower.
                ("thor-clef-flash".to_owned(), 0.05),
                ("jev".to_owned(), 0.5),
            ])
        );
        // The escalation (D71): the cheap model's confidence decides when to
        // pay for the second opinion.
        assert_eq!(c.escalate_below, 0.5);
        assert_eq!(c.escalate_backend, "thor-clef-flash");
        assert_eq!(
            (
                c.think_hz_member,
                c.think_hz_leader,
                c.mode.as_str(),
                c.presence_hz
            ),
            (1, 1, "auto", 10)
        );
        assert_eq!(
            (
                c.max_questions,
                c.request_timeout_ms,
                c.squad_weight,
                c.budget_calls_per_round,
                c.on_exhaust.as_str(),
                c.stale_enemy_ticks,
                c.ally_range_cells
            ),
            (11, 4_000, 0.5, 400, "reflex", 6, 4)
        );
        assert_eq!(
            (
                c.utter.realizer.as_str(),
                c.utter.llm_model.as_str(),
                c.utter.llm_port
            ),
            ("auto", "LFM2-1.2B", 8092)
        );
    }

    #[test]
    fn partial_toml_fills_defaults() {
        let c = AgentsConfig::from_toml("think_hz_member = 8\n").expect("partial toml loads");
        assert_eq!(c.think_hz_member, 8);
        // Every unmentioned field keeps its default.
        assert_eq!(c.node, AgentsConfig::default().node);
        assert_eq!(
            c.request_timeout_ms,
            AgentsConfig::default().request_timeout_ms
        );
        // A table field is replaced whole: the named port overrides, the
        // unmentioned ones do not survive.
        let c = AgentsConfig::from_toml("[ports]\nd1-3b = 9000\n").expect("ports toml loads");
        assert_eq!(c.ports, BTreeMap::from([("d1-3b".to_owned(), 9000)]));
        // The wording's own table is replaced whole, and the fields the
        // file does not name keep their defaults.
        let c = AgentsConfig::from_toml("[utter]\nrealizer = \"llm\"\n")
            .expect("the utter table loads");
        assert_eq!(c.utter.realizer, "llm");
        assert_eq!(c.utter.llm_model, UterConfig::default().llm_model);
        assert_eq!(c.utter.llm_port, UterConfig::default().llm_port);
    }

    #[test]
    fn full_toml_overrides_every_field() {
        // Scalars first: in TOML, keys after a [table] header belong to
        // that table, so the tables close the file.
        let text = r#"
node = "thor"
backend_member = "jev"
backend_leader = "clef-flash"
thor_base = "http://10.0.0.1"
think_hz_member = 2
think_hz_leader = 1
mode = "presence"
presence_hz = 5
max_questions = 6
request_timeout_ms = 250
squad_weight = 0.25
budget_calls_per_round = 40
on_exhaust = "freeze"
stale_enemy_ticks = 9
ally_range_cells = 7

[ports]
d1-3b = 9000

[min_confidence]
jev = 0.7
"#;
        let c = AgentsConfig::from_toml(text).expect("full toml loads");
        assert_eq!(c.node, "thor");
        assert_eq!(c.backend_member, "jev");
        assert_eq!(c.backend_leader, "clef-flash");
        assert_eq!(c.thor_base, "http://10.0.0.1");
        assert_eq!(c.ports, BTreeMap::from([("d1-3b".to_owned(), 9000)]));
        assert_eq!(c.min_confidence, BTreeMap::from([("jev".to_owned(), 0.7)]));
        assert_eq!(
            (
                c.think_hz_member,
                c.think_hz_leader,
                c.mode.as_str(),
                c.presence_hz,
                c.max_questions,
                c.request_timeout_ms,
                c.squad_weight,
                c.budget_calls_per_round,
                c.on_exhaust.as_str(),
                c.stale_enemy_ticks,
                c.ally_range_cells
            ),
            (2, 1, "presence", 5, 6, 250, 0.25, 40, "freeze", 9, 7)
        );
    }

    #[test]
    fn missing_file_loads_defaults() {
        let c = AgentsConfig::load(Path::new("no-such-agents.toml")).expect("missing file is ok");
        assert_eq!(c, AgentsConfig::default());
    }

    #[test]
    fn bad_toml_is_a_parse_error() {
        // A type error (node must be a string) and a syntax error both
        // refuse the file rather than half-loading it.
        assert!(matches!(
            AgentsConfig::from_toml("node = 3\n"),
            Err(ConfigError::Parse(_))
        ));
        assert!(matches!(
            AgentsConfig::from_toml("[ports\n"),
            Err(ConfigError::Parse(_))
        ));
    }
}
