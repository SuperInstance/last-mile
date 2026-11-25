//! The identity of a question: its id, its version, its normalized form, and
//! the digest of the context it was asked over.
//!
//! A question's **wording is behaviour** (docs/design/questions-as-experience.md):
//! the same gate, reworded, is a different question, and the round's ledger
//! has to be able to say so. So every gate carries three names:
//!
//! - the **id**: *which* question this is. It is the gate's slot and its
//!   kind, and nothing else, so it survives a rewording: the ledger keeps
//!   one row per question and the version says which revision it is.
//! - the **version**: *which revision* of that question. It is the digest of
//!   the normalized form, so any change to the wording or to the options is
//!   a new version — and a reworded gate is a new version, exactly.
//! - the **normalized form**: the canonical text of the question, so that
//!   two spellings of the same question are one question.
//!
//! Everything here is derived from **harness bytes** — the gate's own
//! definition — or from the deterministic sim, and never from a clock, a
//! random number or the order a map happened to iterate in. Two machines
//! that play the same round compute the same ids (D69).
//!
//! The **context digest** is the other half: the facts the gate saw,
//! normalized, and the options it was actually asked about. It is what
//! makes a cached answer safe to reuse — the same question over a different
//! situation is a different answer (D26: no answer is ever invented).
//!
//! The key format is the one scrap-spark already runs in production
//! (docs/research/lucineer-ecosystem.md §13): `SHA-256(version | question |
//! context)`, with a hit ledger in front of it.

#![forbid(unsafe_code)]

use serde_json::Value;
use sha2::{Digest, Sha256};

/// The domain separator every digest of a question starts with.
///
/// Without it a question's digest could be mistaken for another kind of
/// digest (a harness commit, a log hash), and the two would collide.
const TAG: &str = "lmq1";

/// The prefix of a question id.
pub const ID_PREFIX: &str = "q_";

/// The prefix of a question version.
pub const VERSION_PREFIX: &str = "v_";

/// How many hex digits of the digest an id or a version carries.
///
/// Sixteen hex digits are 64 bits: a gate set holds at most 256 gates
/// (D37's `u8`), so the ids of one harness cannot collide with each other,
/// and the ledger stays readable.
const HEX: usize = 16;

/// One of a question's options: its key and what it means.
///
/// A choice gate's options are its criteria (the menu's or the file's); a
/// score gate's are its levels, keyed by their score.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuestionOption {
    /// The option's key: `opt0`, `beacon`, or the level's score.
    pub key: String,
    /// What the option means, in the words the model reads.
    pub text: String,
}

impl QuestionOption {
    /// An option with a key and its text.
    pub fn new(key: impl Into<String>, text: impl Into<String>) -> QuestionOption {
        QuestionOption {
            key: key.into(),
            text: text.into(),
        }
    }
}

/// The identity of a question: its id, its version and its normalized form.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct QuestionIdentity {
    /// The question's id: `q_<16 hex>`. Stable across a rewording.
    pub id: String,
    /// The question's version: `v_<16 hex>`. A rewording is a new one.
    pub version: String,
    /// The question's normalized form: the canonical text.
    pub normalized: String,
}

impl QuestionIdentity {
    /// The identity of a question.
    ///
    /// `slot` is the gate's key (the name its answer comes back under),
    /// `kind` is the question's type, `instructions` is what the model is
    /// asked, and `options` are the options it is asked about.
    ///
    /// The id is the digest of the slot and the kind alone: a gate that is
    /// reworded keeps its id and gets a new version, so the ledger can hold
    /// one row per question and say which revision it measured.
    pub fn of(
        slot: &str,
        kind: &str,
        instructions: &str,
        options: &[QuestionOption],
    ) -> QuestionIdentity {
        let normalized = normalized_form(kind, instructions, options);
        let id = format!(
            "{ID_PREFIX}{}",
            &sha256_hex(&[TAG, "id", slot, kind])[..HEX]
        );
        let version = format!(
            "{VERSION_PREFIX}{}",
            &sha256_hex(&[TAG, "version", kind, &normalized])[..HEX]
        );
        QuestionIdentity {
            id,
            version,
            normalized,
        }
    }
}

/// The canonical text of a question.
///
/// The form is `kind | instructions | key=text;key=text`: the kind first, so
/// two questions of different kinds never read alike, then the wording, then
/// the options in the order they were given (the caller sorts them: a
/// choice gate's criteria by key, a score gate's levels by score).
///
/// Every part is normalized, so the form does not change when the wording
/// changes only in its whitespace or its case.
pub fn normalized_form(kind: &str, instructions: &str, options: &[QuestionOption]) -> String {
    let mut out = String::with_capacity(instructions.len() + 32);
    out.push_str(&normalize(kind));
    out.push_str(" | ");
    out.push_str(&normalize(instructions));
    out.push_str(" | ");
    for (index, option) in options.iter().enumerate() {
        if index > 0 {
            out.push(';');
        }
        out.push_str(&normalize(&option.key));
        out.push('=');
        out.push_str(&normalize(&option.text));
    }
    out
}

/// The normalized form of a text: trimmed, with every run of whitespace
/// collapsed to one space, and lowercased.
///
/// Case is folded on purpose: a gate that only changes the case of its
/// wording is the same question, and a new version for it would be noise
/// in the ledger.
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = true;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !space {
                out.push(' ');
                space = true;
            }
        } else {
            space = false;
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
        }
    }
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

/// The digest of the context a question was asked over: the facts the gate
/// saw, normalized, and the question as asked, normalized.
///
/// The facts are the gate's context; the options are part of it, because a
/// menu gate's options change with the world and an answer given about one
/// menu is not an answer about another.
///
/// The digest is what makes a cached answer safe: the same question over a
/// different context is a different key, so it is asked again.
pub fn context_digest(state: &str, question: &Value) -> String {
    let mut hasher = Sha256::new();
    put(&mut hasher, TAG);
    put(&mut hasher, "context");
    put(&mut hasher, &normalize(state));
    put(&mut hasher, &canonical_json(question));
    hex(&hasher.finalize())
}

/// The key of a cached answer: the question's id, its version and the
/// context digest.
///
/// This is scrap-spark's key format (SHA-256 of the version, the question
/// and the context), with the question's id in front of the version: the id
/// says which question it is, the version which revision, and the context
/// what it was asked about.
pub fn cache_key(id: &str, version: &str, context: &str) -> String {
    let mut hasher = Sha256::new();
    put(&mut hasher, TAG);
    put(&mut hasher, "cache");
    put(&mut hasher, id);
    put(&mut hasher, version);
    put(&mut hasher, context);
    hex(&hasher.finalize())
}

/// The digest of some strings, as hex.
///
/// The parts are length-prefixed, so `("ab", "c")` and `("a", "bc")` are
/// different digests.
pub fn sha256_hex(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        put(&mut hasher, part);
    }
    hex(&hasher.finalize())
}

/// The canonical JSON of a value: its strings normalized, its objects keyed
/// in order.
///
/// `serde_json`'s own map is ordered, so the only thing to add is the
/// normalization of the strings: two questions that differ only in their
/// whitespace are one question.
pub fn canonical_json(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => format!("{:?}", normalize(s)),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(canonical_json).collect();
            format!("[{}]", parts.join(","))
        }
        Value::Object(map) => {
            let mut parts: Vec<String> = map
                .iter()
                .map(|(key, value)| format!("{:?}:{}", normalize(key), canonical_json(value)))
                .collect();
            parts.sort();
            format!("{{{}}}", parts.join(","))
        }
    }
}

/// The identity of a question as it travels in a pack.
///
/// The pack carries the id and the version, so that anything holding the
/// pack — the answer cache, the ledger, the model server's own log — can
/// name the question without knowing the harness it came from.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct QuestionStamp {
    /// The question's id.
    pub id: String,
    /// The question's version.
    pub version: String,
    /// The question's normalized form.
    #[serde(default)]
    pub normalized: String,
    /// Whether this answer came from the cache.
    #[serde(default)]
    pub cached: bool,
    /// How many asks old the cached answer was, when it was cached.
    #[serde(default)]
    pub age_asks: u64,
}

/// The id and the version of a question, as the pack carries them.
///
/// A pack that was built before this existed — or one a test wrote by hand
/// — has neither, and the identity is computed from the question itself:
/// the same slot and the same kind give the same id, and the question as
/// asked gives the version. A pack that names its questions is always
/// preferred, because the harness's version survives a change in the menu.
pub fn stamp_of(slot: &str, question: &Value) -> QuestionStamp {
    let kind = question
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let id = question
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| {
            format!(
                "{ID_PREFIX}{}",
                &sha256_hex(&[TAG, "id", slot, kind])[..HEX]
            )
        });
    let version = question
        .get("version")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| {
            let instructions = question
                .get("instructions")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let options = options_of(question);
            let normalized = normalized_form(kind, instructions, &options);
            format!(
                "{VERSION_PREFIX}{}",
                &sha256_hex(&[TAG, "version", kind, &normalized])[..HEX]
            )
        });
    let normalized = question
        .get("normalized")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| {
            let instructions = question
                .get("instructions")
                .and_then(Value::as_str)
                .unwrap_or_default();
            normalized_form(kind, instructions, &options_of(question))
        });
    QuestionStamp {
        id,
        version,
        normalized,
        cached: false,
        age_asks: 0,
    }
}

/// The options of a question as it travels in a pack: its criteria (a map)
/// or its levels (an array).
///
/// The pack's own shape decides: a choice gate's criteria are a map keyed
/// by the option's key, and a score gate's levels are an array, lowest
/// score first.
pub fn options_of(question: &Value) -> Vec<QuestionOption> {
    let Some(criteria) = question.get("criteria") else {
        return Vec::new();
    };
    match criteria {
        Value::Object(map) => map
            .iter()
            .map(|(key, value)| QuestionOption::new(key, value.as_str().unwrap_or_default()))
            .collect(),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(index, value)| {
                QuestionOption::new(index.to_string(), value.as_str().unwrap_or_default())
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Feed a part to the hasher, length-prefixed.
fn put(hasher: &mut Sha256, part: &str) {
    hasher.update((part.len() as u64).to_be_bytes());
    hasher.update(part.as_bytes());
}

/// The digest as lower-case hex.
fn hex(digest: &[u8]) -> String {
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push(char::from_digit((byte >> 4) as u32, 16).unwrap_or('0'));
        out.push(char::from_digit((byte & 0x0f) as u32, 16).unwrap_or('0'));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalization_collapses_whitespace_and_case() {
        assert_eq!(normalize("  Hold   Position \n"), "hold position");
        assert_eq!(normalize("HOLD"), "hold");
        assert_eq!(normalize("a\t\tb"), "a b");
        assert_eq!(normalize(""), "");
        assert_eq!(normalize("   "), "");
    }

    #[test]
    fn the_normalized_form_is_canonical() {
        let options = vec![
            QuestionOption::new("opt0", "Go to beacon 0"),
            QuestionOption::new("opt1", "  Hold   position "),
        ];
        let a = normalized_form("choice", "What do you do?", &options);
        let b = normalized_form("choice", "what do you do?", &options);
        assert_eq!(a, b);
        assert!(a.starts_with("choice | what do you do? | "));
        assert!(a.contains("opt1=hold position"));
    }

    #[test]
    fn an_id_survives_a_rewording_and_a_version_does_not() {
        let options = vec![QuestionOption::new("opt0", "go")];
        let first = QuestionIdentity::of("move", "choice", "What do you do?", &options);
        let reworded = QuestionIdentity::of("move", "choice", "What should you do now?", &options);
        assert_eq!(first.id, reworded.id, "the id is the question's identity");
        assert_ne!(
            first.version, reworded.version,
            "a rewording is a new version"
        );
        assert_ne!(first.normalized, reworded.normalized);
    }

    #[test]
    fn a_different_kind_is_a_different_question() {
        let options = vec![QuestionOption::new("opt0", "go")];
        let choice = QuestionIdentity::of("move", "choice", "What do you do?", &options);
        let noul = QuestionIdentity::of("move", "noul", "What do you do?", &options);
        assert_ne!(choice.id, noul.id);
    }

    #[test]
    fn a_different_option_is_a_new_version() {
        let one = QuestionIdentity::of(
            "move",
            "choice",
            "What do you do?",
            &[QuestionOption::new("opt0", "go")],
        );
        let two = QuestionIdentity::of(
            "move",
            "choice",
            "What do you do?",
            &[
                QuestionOption::new("opt0", "go"),
                QuestionOption::new("opt1", "hold"),
            ],
        );
        assert_eq!(one.id, two.id);
        assert_ne!(one.version, two.version);
    }

    #[test]
    fn the_identity_is_the_same_on_every_call() {
        let options = vec![QuestionOption::new("opt0", "go")];
        let a = QuestionIdentity::of("move", "choice", "What do you do?", &options);
        let b = QuestionIdentity::of("move", "choice", "What do you do?", &options);
        assert_eq!(a, b);
        assert!(a.id.starts_with(ID_PREFIX));
        assert!(a.version.starts_with(VERSION_PREFIX));
        assert_eq!(a.id.len(), ID_PREFIX.len() + HEX);
        assert_eq!(a.version.len(), VERSION_PREFIX.len() + HEX);
    }

    #[test]
    fn the_context_digest_moves_with_the_facts() {
        let question = json!({"type": "choice", "instructions": "What do you do?", "criteria": {"opt0": "go"}});
        let a = context_digest("You are agent 0.", &question);
        let b = context_digest("You are agent 1.", &question);
        assert_ne!(a, b, "different facts, different context");
        let c = context_digest("You are agent 0.", &question);
        assert_eq!(a, c, "the same facts, the same context");
    }

    #[test]
    fn the_context_digest_moves_with_the_options() {
        let one = json!({"type": "choice", "instructions": "What do you do?", "criteria": {"opt0": "go"}});
        let two = json!({"type": "choice", "instructions": "What do you do?", "criteria": {"opt0": "go", "opt1": "hold"}});
        assert_ne!(
            context_digest("same facts", &one),
            context_digest("same facts", &two)
        );
    }

    #[test]
    fn the_context_digest_ignores_whitespace_and_case() {
        let one = json!({"type": "choice", "instructions": "What do you do?", "criteria": {"opt0": "go"}});
        let two = json!({"type": "choice", "instructions": "what   do you do?", "criteria": {"opt0": "  go "}});
        assert_eq!(
            context_digest("You are agent 0.", &one),
            context_digest("you are AGENT 0.", &two)
        );
    }

    #[test]
    fn the_cache_key_moves_with_the_version_and_the_context() {
        let a = cache_key("q_1", "v_1", "ctx");
        assert_eq!(a, cache_key("q_1", "v_1", "ctx"));
        assert_ne!(a, cache_key("q_1", "v_2", "ctx"));
        assert_ne!(a, cache_key("q_2", "v_1", "ctx"));
        assert_ne!(a, cache_key("q_1", "v_1", "other"));
    }

    #[test]
    fn a_pack_without_an_identity_gets_one() {
        let question = json!({"type": "choice", "instructions": "What do you do?", "criteria": {"opt0": "go"}});
        let stamp = stamp_of("move", &question);
        let identity = QuestionIdentity::of(
            "move",
            "choice",
            "What do you do?",
            &[QuestionOption::new("opt0", "go")],
        );
        assert_eq!(stamp.id, identity.id);
        assert_eq!(stamp.version, identity.version);
    }

    #[test]
    fn a_pack_that_names_its_questions_is_believed() {
        let identity = QuestionIdentity::of(
            "move",
            "choice",
            "What do you do?",
            &[QuestionOption::new("opt0", "go")],
        );
        let question = json!({
            "type": "choice",
            "instructions": "What do you do?",
            "criteria": {"opt0": "go"},
            "id": identity.id,
            "version": identity.version,
        });
        let stamp = stamp_of("move", &question);
        assert_eq!(stamp.id, identity.id);
        assert_eq!(stamp.version, identity.version);
    }
}
