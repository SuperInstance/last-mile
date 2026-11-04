//! Proposal parsing and validation for the learning system.
//!
//! The reviewer (System 2) answers in prose; this module turns prose into
//! exactly one [`Proposal`], and the prompt ([`build_review_prompt`]) shows
//! it what it is allowed to ask for.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use regex::Regex;
use std::path::{Path, PathBuf};

use crate::ledger::Direction;
use crate::metrics::{Moment, RoundMetrics};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind")]
pub enum Proposal {
    AddGate {
        agent: u16,
        toml: String,
    },
    EditReflex {
        agent: u16,
        name: String,
        source: String,
    },
    LexiconEntry {
        squad: u8,
        text: String,
    },
    PolicyTweak {
        agent: u16,
        key: String,
        value: i64,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Rejection {
    NotJson(String),
    UnknownKind(String),
    PathEscape(String),
    BadName(String),
    Empty(&'static str),
    TooLarge(usize),
    Io(String),
    /// The prediction named a metric the evaluation cannot read.
    UnknownMetric(String),
}

impl std::fmt::Display for Rejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Rejection::NotJson(why) => write!(f, "not a proposal: {why}"),
            Rejection::UnknownKind(kind) => write!(f, "unknown proposal kind: {kind}"),
            Rejection::PathEscape(why) => write!(f, "path escape: {why}"),
            Rejection::BadName(why) => write!(f, "bad name: {why}"),
            Rejection::Empty(what) => write!(f, "{what} is empty"),
            Rejection::TooLarge(n) => write!(f, "{n} bytes is too large"),
            Rejection::Io(why) => write!(f, "io: {why}"),
            Rejection::UnknownMetric(metric) => write!(f, "unknown metric: {metric}"),
        }
    }
}

/// The metrics a prediction may name.
///
/// The list is the evaluation's: a claim about a metric the evaluation
/// cannot read is not a claim, it is a hope.
pub const KNOWN_METRICS: [&str; 7] = [
    "score_squad0",
    "score_squad1",
    "hold_ticks_squad0",
    "hold_ticks_squad1",
    "tags_for_squad0",
    "tags_for_squad1",
    "margin_squad0",
];

/// The metric a proposal is judged on when the reviewer names none.
pub const DEFAULT_METRIC: &str = "hold_ticks_squad0";

/// What the reviewer says the change will do, sealed before it is tested
/// (D32).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Claim {
    /// The metric the change is expected to move.
    pub metric: String,
    /// Which way it is expected to move.
    pub direction: Direction,
    /// The smallest move that counts, in the metric's own unit.
    pub min_delta: i64,
}

impl Claim {
    /// The claim the flow seals when the reviewer states none.
    ///
    /// It is the flow's own claim, not the reviewer's, and the ledger says
    /// so: a proposal nobody predicted anything about is judged on the
    /// flow's own default, not on a number nobody wrote down.
    pub fn default_claim() -> Claim {
        Claim {
            metric: DEFAULT_METRIC.to_string(),
            direction: Direction::Up,
            min_delta: 1,
        }
    }
}

/// What one reviewer's answer holds.
#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    /// The change the reviewer proposes.
    pub proposal: Proposal,
    /// What it expects to change, when the reviewer said.
    pub claim: Option<Claim>,
}

/// The envelope the reviewer is asked to answer with.
#[derive(Debug, serde::Deserialize)]
struct Envelope {
    proposal: Option<serde_json::Value>,
    #[serde(default)]
    prediction: Option<Claim>,
}

/// One agent's harness, as the reviewer sees it.
///
/// A view, not the harness itself: the prompt needs the harness's words, not
/// its type.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HarnessView {
    /// The agent this harness belongs to.
    pub agent: u16,
    /// The harness's pinned identity (a commit id or a content id).
    pub commit: String,
    /// The gates that decide what the agent may do.
    pub gates: String,
    /// The agent's policy.
    pub policy: String,
    /// The phrases the agent may use.
    pub phrasebook: String,
    /// The reflex scripts, by file stem.
    pub reflexes: BTreeMap<String, String>,
}

/// The first top-level JSON object in `text`, by brace matching.
///
/// Strings are skipped, so a brace inside a string does not end the object.
/// The offsets are byte offsets, so text before the object may hold any
/// UTF-8, not just ASCII.
fn first_object(text: &str) -> Option<&str> {
    let mut brace_count = 0i64;
    let mut start: Option<usize> = None;
    let mut end: Option<usize> = None;

    let mut chars = text.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '{' => {
                if brace_count == 0 {
                    start = Some(i);
                }
                brace_count += 1;
            }
            '}' => {
                brace_count -= 1;
                if brace_count == 0 {
                    if start.is_some() {
                        end = Some(i);
                    }
                    break;
                }
            }
            '"' => {
                // Skip the string: a brace inside it is not a brace.
                let rest = chars.by_ref();
                while let Some((_, c)) = rest.next() {
                    if c == '"' {
                        break;
                    }
                    if c == '\\' {
                        rest.next();
                    }
                }
            }
            _ => {}
        }
    }

    match (start, end) {
        (Some(s), Some(e)) => Some(&text[s..=e]),
        _ => None,
    }
}

/// Extracts the first JSON object from text using brace matching and parses it.
pub fn parse_proposal(text: &str) -> Result<Proposal, Rejection> {
    let Some(json_str) = first_object(text) else {
        return Err(Rejection::NotJson("no JSON object in the text".to_string()));
    };
    match serde_json::from_str::<Proposal>(json_str) {
        Ok(proposal) => Ok(proposal),
        Err(e) => Err(Rejection::NotJson(e.to_string())),
    }
}

/// Parse what the reviewer answered: a proposal, and the prediction that goes
/// with it.
///
/// Two shapes are accepted:
/// * the envelope `{"proposal": {...}, "prediction": {...}}`, which is what
///   the prompt asks for;
/// * a bare proposal, for a reviewer that only says what to change. Such an
///   answer carries no prediction, and the flow seals its own default claim
///   instead (marked as the flow's, in the ledger).
pub fn parse_answer(text: &str) -> Result<Answer, Rejection> {
    if let Some(json) = first_object(text) {
        if let Ok(envelope) = serde_json::from_str::<Envelope>(json) {
            if let Some(inner) = envelope.proposal {
                let proposal = parse_proposal(&inner.to_string())?;
                validate(&proposal)?;
                if let Some(claim) = &envelope.prediction {
                    if !KNOWN_METRICS.contains(&claim.metric.as_str()) {
                        return Err(Rejection::UnknownMetric(claim.metric.clone()));
                    }
                    if claim.min_delta < 0 {
                        return Err(Rejection::BadName(
                            "min_delta must not be negative".to_string(),
                        ));
                    }
                }
                return Ok(Answer {
                    proposal,
                    claim: envelope.prediction,
                });
            }
        }
    }
    let proposal = parse_proposal(text)?;
    validate(&proposal)?;
    Ok(Answer {
        proposal,
        claim: None,
    })
}

/// Validates a proposal according to the rules.
pub fn validate(p: &Proposal) -> Result<(), Rejection> {
    match p {
        Proposal::AddGate { agent: _, toml } => {
            if toml.is_empty() {
                return Err(Rejection::Empty("toml"));
            }
            if toml.len() > 16384 {
                return Err(Rejection::TooLarge(toml.len()));
            }
        }
        Proposal::EditReflex {
            agent: _,
            name,
            source,
        } => {
            // Check reflex name pattern: ^[a-z][a-z0-9_]{0,31}$
            let re = Regex::new(r"^[a-z][a-z0-9_]{0,31}$").unwrap();
            if !re.is_match(name) {
                return Err(Rejection::BadName("Invalid reflex name".to_string()));
            }
            if source.is_empty() {
                return Err(Rejection::Empty("source"));
            }
            if source.len() > 16384 {
                return Err(Rejection::TooLarge(source.len()));
            }
        }
        Proposal::LexiconEntry { squad: _, text } => {
            if text.is_empty() {
                return Err(Rejection::Empty("text"));
            }
            if text.len() > 16384 {
                return Err(Rejection::TooLarge(text.len()));
            }
        }
        Proposal::PolicyTweak {
            agent: _,
            key,
            value: _,
        } => {
            // Check policy key pattern: ^[a-z_]{1,32}$
            let re = Regex::new(r"^[a-z_]{1,32}$").unwrap();
            if !re.is_match(key) {
                return Err(Rejection::BadName("Invalid policy key".to_string()));
            }
        }
    }
    Ok(())
}

/// Determines the target path for a proposal under the harness root.
pub fn target_path(p: &Proposal, harness_root: &Path) -> Result<PathBuf, Rejection> {
    let path = match p {
        Proposal::AddGate { agent, .. } => {
            harness_root.join(format!("a{agent}")).join("gates.toml")
        }
        Proposal::EditReflex { agent, name, .. } => harness_root
            .join(format!("a{agent}"))
            .join("reflexes")
            .join(format!("{name}.rhai")),
        Proposal::LexiconEntry { squad, .. } => harness_root
            .join(format!("squad{squad}"))
            .join("lexicon.md"),
        Proposal::PolicyTweak { agent, .. } => {
            harness_root.join(format!("a{agent}")).join("policy.toml")
        }
    };

    // Ensure the path is within the harness_root by checking if it starts with the root
    // We can't use canonicalize in tests as paths may not exist
    if path.starts_with(harness_root) {
        Ok(path)
    } else {
        Err(Rejection::PathEscape("Path escape detected".to_string()))
    }
}

/// Applies a proposal to the file system under the harness root.
pub fn apply(
    p: &Proposal,
    harness_root: &std::path::Path,
) -> Result<std::path::PathBuf, Rejection> {
    validate(p)?;
    let path = target_path(p, harness_root)?;

    // Create parent directories as needed
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Rejection::Io(e.to_string()))?;
    }

    match p {
        Proposal::AddGate { agent: _, toml } => {
            // Append toml to the file (create if missing)
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|e| Rejection::Io(e.to_string()))?;

            use std::io::Write;
            writeln!(file, "{}", toml).map_err(|e| Rejection::Io(e.to_string()))?;
        }
        Proposal::EditReflex {
            agent: _,
            name: _,
            source,
        } => {
            // Write (replace) the file with source
            std::fs::write(&path, format!("{}\n", source))
                .map_err(|e| Rejection::Io(e.to_string()))?;
        }
        Proposal::LexiconEntry { squad: _, text } => {
            // Append "- " + text + "\n"
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|e| Rejection::Io(e.to_string()))?;

            use std::io::Write;
            writeln!(file, "- {}", text).map_err(|e| Rejection::Io(e.to_string()))?;
        }
        Proposal::PolicyTweak {
            agent: _,
            key,
            value,
        } => {
            let mut lines = Vec::new();

            // If file exists, read all lines
            if path.exists() {
                let content =
                    std::fs::read_to_string(&path).map_err(|e| Rejection::Io(e.to_string()))?;
                lines = content.lines().map(|s| s.to_string()).collect();
            }

            // Check if we need to replace an existing key
            let key_with_equals = format!("{} =", key);
            let mut found = false;

            for line in &mut lines {
                if line.trim_start().starts_with(&key_with_equals) {
                    *line = format!("{} = {}", key, value);
                    found = true;
                    break;
                }
            }

            // If key not found, append it
            if !found {
                lines.push(format!("{} = {}", key, value));
            }

            // Write back all lines
            let output = lines.join("\n") + "\n";
            std::fs::write(&path, output).map_err(|e| Rejection::Io(e.to_string()))?;
        }
    }

    Ok(path)
}

/// The longest harness file the prompt carries, in characters.
const MAX_FILE_CHARS: usize = 4_000;

/// Build the prompt the between-round reviewer answers.
///
/// The prompt is deterministic: the same round and the same harness always
/// produce the same prompt, so a run can be replayed and compared. It says
/// what happened, what the squad may change, and exactly how to answer.
pub fn build_review_prompt(
    metrics: &RoundMetrics,
    moments: &[Moment],
    harness: &[HarnessView],
) -> String {
    let mut p = String::new();

    let _ = writeln!(
        p,
        "You are the between-round reviewer (System 2) of a last-mile squad."
    );
    let _ = writeln!(
        p,
        "The round you are reviewing is over. Read it, then propose ONE change."
    );
    let _ = writeln!(p);

    let _ = writeln!(p, "## What happened");
    let _ = writeln!(p, "- ticks played: {}", metrics.ticks);
    let _ = writeln!(p, "- score: {}", join_i64(&metrics.scores, "squad"));
    let _ = writeln!(
        p,
        "- tags landed: {}",
        join_i64(&as_i64(&metrics.tags_for), "squad")
    );
    let _ = writeln!(
        p,
        "- tags taken: {}",
        join_i64(&as_i64(&metrics.tags_against), "squad")
    );
    let _ = writeln!(
        p,
        "- ticks holding a beacon: {}",
        join_i64(&as_i64(&metrics.beacon_hold_ticks), "squad")
    );
    let _ = writeln!(
        p,
        "- decisions: human {}, system1 {}, system2 {}, test {}, reflex {}",
        metrics.decisions.human,
        metrics.decisions.system1,
        metrics.decisions.system2,
        metrics.decisions.test,
        metrics.decisions.reflex
    );
    let _ = writeln!(
        p,
        "- timeliness: on time {}, late {}, stale {}",
        metrics.timeliness.on_time, metrics.timeliness.late, metrics.timeliness.stale
    );
    let _ = writeln!(
        p,
        "- refusals: refused {}, out of fuel {}, errors {}",
        metrics.refusals.refused, metrics.refusals.out_of_fuel, metrics.refusals.error
    );
    for (agent, dial) in &metrics.dial {
        let _ = writeln!(
            p,
            "- dial: agent {agent} drove {} ticks, its reflex {} ticks (model share {} per mil)",
            dial.model_ticks, dial.reflex_ticks, dial.model_share_x1000
        );
    }
    let _ = writeln!(p);

    let _ = writeln!(p, "## Moments worth a look");
    if moments.is_empty() {
        let _ = writeln!(p, "_none_");
    } else {
        for moment in moments {
            let kind = match &moment.kind {
                crate::metrics::MomentKind::ScoreSwing { squad, delta } => {
                    format!("squad {squad} moved {delta} points")
                }
                crate::metrics::MomentKind::TagCluster { tags } => {
                    format!("{tags} tags in one window")
                }
            };
            let _ = writeln!(p, "- tick {}: {kind}", moment.tick);
        }
    }
    let _ = writeln!(p);

    let _ = writeln!(p, "## The harness (what the squad may change)");
    if harness.is_empty() {
        let _ = writeln!(p, "_no harness is pinned for this round_");
    } else {
        for h in harness {
            let _ = writeln!(p, "### agent {} (pinned {})", h.agent, h.commit);
            let _ = writeln!(p, "gates.toml:");
            let _ = writeln!(p, "```toml");
            let _ = writeln!(p, "{}", cut(&h.gates));
            let _ = writeln!(p, "```");
            let _ = writeln!(p, "policy.toml:");
            let _ = writeln!(p, "```toml");
            let _ = writeln!(p, "{}", cut(&h.policy));
            let _ = writeln!(p, "```");
            let _ = writeln!(p, "phrasebook.toml:");
            let _ = writeln!(p, "```toml");
            let _ = writeln!(p, "{}", cut(&h.phrasebook));
            let _ = writeln!(p, "```");
            for (name, src) in &h.reflexes {
                let _ = writeln!(p, "reflexes/{name}.rhai:");
                let _ = writeln!(p, "```rhai");
                let _ = writeln!(p, "{}", cut(src));
                let _ = writeln!(p, "```");
            }
        }
    }
    let _ = writeln!(p);

    let _ = writeln!(p, "## How to answer");
    let _ = writeln!(p, "Answer with ONE JSON object and nothing else:");
    let _ = writeln!(
        p,
        r#"{{"proposal": {{"kind": "EditReflex", "agent": 0, "name": "default", "source": "<the whole new Rhai file>"}}, "prediction": {{"metric": "hold_ticks_squad0", "direction": "Up", "min_delta": 1}}}}"#
    );
    let _ = writeln!(p);
    let _ = writeln!(p, "The proposal is one of:");
    let _ = writeln!(
        p,
        r#"  {{"kind": "EditReflex", "agent": <agent>, "name": "<file stem>", "source": "<Rhai>"}}"#
    );
    let _ = writeln!(
        p,
        r#"  {{"kind": "AddGate", "agent": <agent>, "toml": "<the gate to add>"}}"#
    );
    let _ = writeln!(
        p,
        r#"  {{"kind": "PolicyTweak", "agent": <agent>, "key": "<policy key>", "value": <integer>}}"#
    );
    let _ = writeln!(
        p,
        r#"  {{"kind": "LexiconEntry", "squad": <squad>, "text": "<the phrase to remember>"}}"#
    );
    let _ = writeln!(p);
    let _ = writeln!(
        p,
        "The prediction says what the change is expected to do, and it is sealed"
    );
    let _ = writeln!(
        p,
        "BEFORE the change is tested on seeds it was not learned from. Name one of:"
    );
    let _ = writeln!(p, "  {}", KNOWN_METRICS.join(", "));
    let _ = writeln!(
        p,
        "and a direction (\"Up\" or \"Down\") and the smallest change that counts."
    );
    let _ = writeln!(
        p,
        "If you cannot say what the change will do, do not propose it."
    );

    p
}

/// `squad 0 = 12, squad 1 = 9` — the numbers of a pair of squads.
fn join_i64(values: &[i64], what: &str) -> String {
    values
        .iter()
        .enumerate()
        .map(|(i, v)| format!("{what} {i} = {v}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The same, for the metrics that count in `u64`.
fn as_i64(values: &[u64]) -> Vec<i64> {
    values.iter().map(|v| *v as i64).collect()
}

/// Cut a file at the prompt's limit, saying so when it happens.
fn cut(text: &str) -> String {
    if text.chars().count() <= MAX_FILE_CHARS {
        return text.to_string();
    }
    let head: String = text.chars().take(MAX_FILE_CHARS).collect();
    format!("{head}\n... (cut)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::{Moment, MomentKind, RoundMetrics};
    use std::{env, fs};

    /// A round's metrics, with the shape the prompt reads.
    fn metrics() -> RoundMetrics {
        RoundMetrics {
            ticks: 900,
            scores: vec![120, 90],
            tags_for: vec![3, 5],
            tags_against: vec![5, 3],
            beacon_hold_ticks: vec![800, 640],
            decisions: Default::default(),
            timeliness: Default::default(),
            refusals: Default::default(),
            dial: Default::default(),
            moments: vec![Moment {
                tick: 300,
                kind: MomentKind::ScoreSwing { squad: 0, delta: 7 },
            }],
        }
    }

    /// One agent's harness, as the reviewer sees it.
    fn harness() -> Vec<HarnessView> {
        let mut reflexes = BTreeMap::new();
        reflexes.insert(
            "default".to_string(),
            "fn reflex(v) { #{ hold: true } }".to_string(),
        );
        vec![HarnessView {
            agent: 0,
            commit: "abc123".to_string(),
            gates: "# the gates\n".to_string(),
            policy: "# the policy\n".to_string(),
            phrasebook: "# the phrases\n".to_string(),
            reflexes,
        }]
    }

    #[test]
    fn the_prompt_shows_the_round_and_says_how_to_answer() {
        let prompt = build_review_prompt(&metrics(), &metrics().moments, &harness());
        // What happened.
        assert!(
            prompt.contains("score: squad 0 = 120, squad 1 = 90"),
            "{prompt}"
        );
        assert!(
            prompt.contains("ticks holding a beacon: squad 0 = 800"),
            "{prompt}"
        );
        assert!(
            prompt.contains("tick 300: squad 0 moved 7 points"),
            "{prompt}"
        );
        // What may change.
        assert!(prompt.contains("### agent 0 (pinned abc123)"), "{prompt}");
        assert!(
            prompt.contains("fn reflex(v) { #{ hold: true } }"),
            "{prompt}"
        );
        // How to answer.
        assert!(prompt.contains(r#""kind": "EditReflex""#), "{prompt}");
        assert!(prompt.contains("hold_ticks_squad0"), "{prompt}");
        assert!(prompt.contains("\"prediction\""), "{prompt}");
    }

    #[test]
    fn an_answer_may_carry_a_prediction() {
        let text = r#"Here is my idea:
            {"proposal": {"kind": "EditReflex", "agent": 3, "name": "flee", "source": "fn reflex(v){()}"},
             "prediction": {"metric": "score_squad0", "direction": "Up", "min_delta": 25}}
            hope that helps"#;
        let answer = parse_answer(text).expect("the answer reads");
        assert_eq!(
            answer.proposal,
            Proposal::EditReflex {
                agent: 3,
                name: "flee".to_string(),
                source: "fn reflex(v){()}".to_string(),
            }
        );
        let claim = answer.claim.expect("the claim is there");
        assert_eq!(claim.metric, "score_squad0");
        assert_eq!(claim.direction, Direction::Up);
        assert_eq!(claim.min_delta, 25);
    }

    #[test]
    fn a_bare_proposal_carries_no_claim() {
        let text =
            r#"{"kind": "EditReflex", "agent": 1, "name": "flee", "source": "fn reflex(v){()}"}"#;
        let answer = parse_answer(text).expect("the answer reads");
        assert!(answer.claim.is_none());
        // And the flow's own default claim stands in for it.
        let default = Claim::default_claim();
        assert_eq!(default.metric, DEFAULT_METRIC);
        assert_eq!(default.min_delta, 1);
    }

    #[test]
    fn an_answer_that_is_not_a_proposal_is_refused() {
        assert!(parse_answer("the round looked fine").is_err());
        assert!(parse_answer("").is_err());
        // A prediction about a metric the evaluation cannot read is refused.
        let text = r#"{"proposal": {"kind": "LexiconEntry", "squad": 0, "text": "hi"},
                        "prediction": {"metric": "vibes", "direction": "Up", "min_delta": 1}}"#;
        match parse_answer(text) {
            Err(Rejection::UnknownMetric(metric)) => assert_eq!(metric, "vibes"),
            other => panic!("expected an unknown metric, got {other:?}"),
        }
    }

    #[test]
    fn text_before_the_json_does_not_break_the_parse() {
        // The braces are matched over the whole text, and the offsets are
        // byte offsets: a word with a multi-byte character in it is fine.
        let text = "Der Vorschlag — hier ist er: {\"kind\": \"LexiconEntry\", \"squad\": 1, \"text\": \"hi\"}";
        let answer = parse_answer(text).expect("the answer reads");
        assert_eq!(
            answer.proposal,
            Proposal::LexiconEntry {
                squad: 1,
                text: "hi".to_string(),
            }
        );
    }

    #[test]
    fn parses_proposal_in_prose() {
        let text = "Here is my idea: {\"kind\":\"EditReflex\",\"agent\":3,\"name\":\"flee\",\"source\":\"fn reflex(v){()}\"} thanks";
        let result = parse_proposal(text);
        assert!(result.is_ok());
        match result.unwrap() {
            Proposal::EditReflex {
                agent,
                name,
                source,
            } => {
                assert_eq!(agent, 3);
                assert_eq!(name, "flee");
                assert_eq!(source, "fn reflex(v){()}");
            }
            _ => panic!("Unexpected proposal type"),
        }
    }

    #[test]
    fn braces_inside_strings() {
        let text = "{\"kind\":\"EditReflex\",\"agent\":1,\"name\":\"test\",\"source\":\"{\\\"key\\\":\\\"value\\\"}\"}";
        let result = parse_proposal(text);
        assert!(result.is_ok());
        match result.unwrap() {
            Proposal::EditReflex {
                agent,
                name,
                source,
            } => {
                assert_eq!(agent, 1);
                assert_eq!(name, "test");
                assert_eq!(source, "{\"key\":\"value\"}");
            }
            _ => panic!("Unexpected proposal type"),
        }
    }

    #[test]
    fn bad_proposal_rejected() {
        let result = parse_proposal("no json here");
        assert!(result.is_err());
        match result.unwrap_err() {
            Rejection::NotJson(_) => {}
            _ => panic!("Expected NotJson error"),
        }

        let result = parse_proposal("{\"kind\":\"Nope\"}");
        assert!(result.is_err());
        // This should fail with a serde error, which gets converted to NotJson
        match result.unwrap_err() {
            Rejection::NotJson(_) => {}
            _ => panic!("Expected NotJson error"),
        }
    }

    #[test]
    fn path_escape_rejected() {
        let proposal = Proposal::EditReflex {
            agent: 1,
            name: "../../etc/passwd".to_string(),
            source: "fn test() {}".to_string(),
        };
        let result = validate(&proposal);
        assert!(result.is_err());
        match result.unwrap_err() {
            Rejection::BadName(_) => {}
            _ => panic!("Expected BadName error"),
        }
    }

    #[test]
    fn target_paths() {
        let root = Path::new("/tmp");

        // Test AddGate
        let proposal = Proposal::AddGate {
            agent: 5,
            toml: "test".to_string(),
        };
        let path = target_path(&proposal, root).unwrap();
        assert_eq!(path, Path::new("/tmp/a5/gates.toml"));

        // Test EditReflex
        let proposal = Proposal::EditReflex {
            agent: 3,
            name: "flee".to_string(),
            source: "fn test() {}".to_string(),
        };
        let path = target_path(&proposal, root).unwrap();
        assert_eq!(path, Path::new("/tmp/a3/reflexes/flee.rhai"));

        // Test LexiconEntry
        let proposal = Proposal::LexiconEntry {
            squad: 7,
            text: "test".to_string(),
        };
        let path = target_path(&proposal, root).unwrap();
        assert_eq!(path, Path::new("/tmp/squad7/lexicon.md"));

        // Test PolicyTweak
        let proposal = Proposal::PolicyTweak {
            agent: 2,
            key: "test_key".to_string(),
            value: 42,
        };
        let path = target_path(&proposal, root).unwrap();
        assert_eq!(path, Path::new("/tmp/a2/policy.toml"));
    }

    #[test]
    fn too_large_rejected() {
        let long_string = "x".repeat(16385); // 16385 characters

        let proposal = Proposal::AddGate {
            agent: 1,
            toml: long_string,
        };
        let result = validate(&proposal);
        assert!(result.is_err());
        match result.unwrap_err() {
            Rejection::TooLarge(len) => assert_eq!(len, 16385),
            _ => panic!("Expected TooLarge error"),
        }
    }

    #[test]
    fn apply_edit_reflex_writes_file() {
        let temp_dir = env::temp_dir().join("apply_edit_reflex_writes_file");
        let _ = fs::remove_dir_all(&temp_dir);

        let proposal = Proposal::EditReflex {
            agent: 1,
            name: "test_reflex".to_string(),
            source: "fn test() { println!(\"hello\"); }".to_string(),
        };

        let result = apply(&proposal, &temp_dir);
        assert!(result.is_ok());

        let expected_path = temp_dir
            .join("a1")
            .join("reflexes")
            .join("test_reflex.rhai");
        assert_eq!(result.unwrap(), expected_path);

        let content = fs::read_to_string(&expected_path).unwrap();
        assert_eq!(content, "fn test() { println!(\"hello\"); }\n");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn apply_add_gate_appends() {
        let temp_dir = env::temp_dir().join("apply_add_gate_appends");
        let _ = fs::remove_dir_all(&temp_dir);

        let proposal = Proposal::AddGate {
            agent: 2,
            toml: "[gate]\nname = \"test\"".to_string(),
        };

        let result = apply(&proposal, &temp_dir);
        assert!(result.is_ok());

        let expected_path = temp_dir.join("a2").join("gates.toml");
        assert_eq!(result.unwrap(), expected_path);

        let content = fs::read_to_string(&expected_path).unwrap();
        assert_eq!(content, "[gate]\nname = \"test\"\n");

        // Append again
        let result2 = apply(&proposal, &temp_dir);
        assert!(result2.is_ok());

        let content = fs::read_to_string(&expected_path).unwrap();
        assert_eq!(
            content,
            "[gate]\nname = \"test\"\n[gate]\nname = \"test\"\n"
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn apply_policy_tweak_replaces_existing_key() {
        let temp_dir = env::temp_dir().join("apply_policy_tweak_replaces_existing_key");
        let _ = fs::remove_dir_all(&temp_dir);

        // Create a policy.toml file with an existing key
        let policy_path = temp_dir.join("a3").join("policy.toml");
        fs::create_dir_all(policy_path.parent().unwrap()).unwrap();
        fs::write(&policy_path, "existing_key = 100\nother_key = 200\n").unwrap();

        let proposal = Proposal::PolicyTweak {
            agent: 3,
            key: "existing_key".to_string(),
            value: 300,
        };

        let result = apply(&proposal, &temp_dir);
        assert!(result.is_ok());

        let content = fs::read_to_string(&policy_path).unwrap();
        assert_eq!(content, "existing_key = 300\nother_key = 200\n");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn apply_policy_tweak_appends_new_key() {
        let temp_dir = env::temp_dir().join("apply_policy_tweak_appends_new_key");
        let _ = fs::remove_dir_all(&temp_dir);

        let proposal = Proposal::PolicyTweak {
            agent: 4,
            key: "new_key".to_string(),
            value: 500,
        };

        let result = apply(&proposal, &temp_dir);
        assert!(result.is_ok());

        let expected_path = temp_dir.join("a4").join("policy.toml");
        let content = fs::read_to_string(&expected_path).unwrap();
        assert_eq!(content, "new_key = 500\n");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn apply_rejects_bad_name_before_touching_disk() {
        let temp_dir = env::temp_dir().join("apply_rejects_bad_name_before_touching_disk");
        let _ = fs::remove_dir_all(&temp_dir);

        let proposal = Proposal::EditReflex {
            agent: 1,
            name: "../x".to_string(),
            source: "fn test() {}".to_string(),
        };

        let result = apply(&proposal, &temp_dir);
        assert!(result.is_err());

        // Check that the directory was not created
        assert!(!temp_dir.exists());

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
