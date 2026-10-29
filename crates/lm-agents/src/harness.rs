//! The harness store: each agent's editable "self" on disk (D15, D27; phase-3 Task 6).
//!
//! An agent's harness is a directory, ideally a git repo, holding:
//!
//! ```text
//! <dir>/gates.toml          the typed questions it asks its decision model (D37)
//! <dir>/policy.toml         when to think: think rates, novelty and threat thresholds
//! <dir>/phrasebook.toml     the wording of the lines it speaks (Task 4)
//! <dir>/reflexes/*.rhai     its sandboxed reflex scripts (run inside lm-core)
//! <dir>/lexicon.md          what its squad learned about language (phase 5)
//! ```
//!
//! Between rounds an agent (or the human) edits its harness and commits.
//! The **round header pins each agent's harness commit** and embeds the
//! reflex sources, so a recorded round replays exactly the behaviour it
//! was played with, even after the harness has moved on (D27).
//!
//! When a harness directory is not a git repo (tests, a fresh copy), its
//! identity is a content hash over every file instead, so pinning still
//! means "these exact bytes".

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use lm_core::{Fnv64, Header};

/// One agent's harness, read from disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Harness {
    /// Where it was read from.
    pub dir: PathBuf,
    /// `gates.toml` as text (parsed by `gates`), or empty if absent.
    pub gates_toml: String,
    /// `policy.toml` as text (parsed by `mind`), or empty if absent.
    pub policy_toml: String,
    /// `phrasebook.toml` as text (parsed by `phrasebook`), or empty if absent.
    ///
    /// The wording of the lines is doctrine like the gates are, so it
    /// travels with the harness and is pinned by the harness commit.
    pub phrasebook_toml: String,
    /// Reflex scripts by file stem (`reflexes/flee.rhai` -> `"flee"`).
    pub reflexes: BTreeMap<String, String>,
    /// The pinned identity: a git commit id, or `content:<16 hex>`.
    pub commit: String,
}

impl Harness {
    /// Read a harness directory. Missing optional files are empty; an
    /// unreadable directory is an error.
    pub fn open(dir: &Path) -> io::Result<Harness> {
        let read_opt = |name: &str| -> io::Result<String> {
            match fs::read_to_string(dir.join(name)) {
                Ok(s) => Ok(s),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
                Err(e) => Err(e),
            }
        };
        let gates_toml = read_opt("gates.toml")?;
        let policy_toml = read_opt("policy.toml")?;
        let phrasebook_toml = read_opt("phrasebook.toml")?;
        let reflexes = read_reflexes(&dir.join("reflexes"))?;
        let commit = match git_head(dir) {
            Some(id) => id,
            None => content_id(&gates_toml, &policy_toml, &phrasebook_toml, &reflexes),
        };
        Ok(Harness {
            dir: dir.to_path_buf(),
            gates_toml,
            policy_toml,
            phrasebook_toml,
            reflexes,
            commit,
        })
    }
}

/// `reflexes/*.rhai`, keyed by file stem, in sorted order.
fn read_reflexes(dir: &Path) -> io::Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) == Some("rhai") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                out.insert(stem.to_string(), fs::read_to_string(&path)?);
            }
        }
    }
    Ok(out)
}

/// The HEAD commit if `dir` is the top of its own git repo. A harness that
/// merely sits *inside* some other repo (like this one's `assets/`) is
/// identified by content instead, because the enclosing repo's HEAD says
/// nothing about this harness's bytes.
fn git_head(dir: &Path) -> Option<String> {
    if !dir.join(".git").exists() {
        return None;
    }
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let id = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!id.is_empty()).then_some(id)
}

/// A stable identity from the harness's bytes: FNV-1a 64 over every file,
/// length-prefixed, in a fixed order.
fn content_id(
    gates: &str,
    policy: &str,
    phrasebook: &str,
    reflexes: &BTreeMap<String, String>,
) -> String {
    let mut h = Fnv64::new();
    let mut put = |s: &str| {
        h.u64(s.len() as u64);
        h.write(s.as_bytes());
    };
    put(gates);
    put(policy);
    put(phrasebook);
    for (name, src) in reflexes {
        put(name);
        put(src);
    }
    format!("content:{:016x}", h.finish())
}

/// Pin a set of harnesses into a round header (D27).
///
/// For every roster entry with a harness in `set`:
/// - `harness_commit` becomes the harness's identity;
/// - each of its reflexes is embedded in `header.scripts` as
///   `"a<agent>/<stem>"`, so the log carries the exact source;
/// - the entry's `reflex` is pointed at `"a<agent>/<chosen>"`, where
///   `<chosen>` is the entry's current reflex name if the harness has it,
///   else the harness's `default`, else its first reflex. If the harness
///   has no reflexes the entry is left alone.
pub fn pin_into(header: &mut Header, set: &BTreeMap<u16, Harness>) {
    for entry in &mut header.roster {
        let Some(h) = set.get(&entry.agent) else {
            continue;
        };
        entry.harness_commit = h.commit.clone();
        for (stem, src) in &h.reflexes {
            header
                .scripts
                .insert(format!("a{}/{}", entry.agent, stem), src.clone());
        }
        let chosen = if h.reflexes.contains_key(&entry.reflex) {
            Some(entry.reflex.clone())
        } else if h.reflexes.contains_key("default") {
            Some("default".to_string())
        } else {
            h.reflexes.keys().next().cloned()
        };
        if let Some(stem) = chosen {
            entry.reflex = format!("a{}/{}", entry.agent, stem);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh temporary harness directory (no external crates: std only).
    fn temp_dir(tag: &str) -> PathBuf {
        let mut d = std::env::temp_dir();
        d.push(format!("lm-harness-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("reflexes")).unwrap();
        d
    }

    fn write(dir: &Path, rel: &str, body: &str) {
        fs::write(dir.join(rel), body).unwrap();
    }

    #[test]
    fn default_harness_loads() {
        let d = temp_dir("load");
        write(&d, "gates.toml", "# gates\n");
        write(&d, "policy.toml", "think_hz = 4\n");
        write(&d, "reflexes/default.rhai", "fn reflex(v) { () }");
        write(&d, "reflexes/notes.txt", "ignored: not a .rhai file");
        let h = Harness::open(&d).unwrap();
        assert_eq!(h.gates_toml, "# gates\n");
        assert_eq!(h.reflexes.keys().collect::<Vec<_>>(), vec!["default"]);
        assert!(h.commit.starts_with("content:"));
    }

    #[test]
    fn content_hash_when_not_git() {
        let a = temp_dir("ha");
        let b = temp_dir("hb");
        for d in [&a, &b] {
            write(d, "reflexes/default.rhai", "fn reflex(v) { () }");
        }
        assert_eq!(
            Harness::open(&a).unwrap().commit,
            Harness::open(&b).unwrap().commit
        );
        write(
            &b,
            "reflexes/default.rhai",
            "fn reflex(v) { #{ hold: true } }",
        );
        assert_ne!(
            Harness::open(&a).unwrap().commit,
            Harness::open(&b).unwrap().commit
        );
    }

    #[test]
    fn missing_files_are_empty_not_errors() {
        let d = temp_dir("empty");
        let h = Harness::open(&d).unwrap();
        assert!(h.gates_toml.is_empty() && h.policy_toml.is_empty() && h.reflexes.is_empty());
    }

    #[test]
    fn commit_pinned_in_header() {
        let d = temp_dir("pin");
        write(
            &d,
            "reflexes/default.rhai",
            "fn reflex(v) { #{ hold: true } }",
        );
        let h = Harness::open(&d).unwrap();
        let mut header = Header::default_round(1, "fn reflex(v) { () }");
        let set: BTreeMap<u16, Harness> = [(3u16, h.clone())].into_iter().collect();
        pin_into(&mut header, &set);
        let e = header.roster.iter().find(|e| e.agent == 3).unwrap();
        assert_eq!(e.harness_commit, h.commit);
        assert_eq!(e.reflex, "a3/default");
        assert_eq!(
            header.scripts["a3/default"],
            "fn reflex(v) { #{ hold: true } }"
        );
        // Agents without a harness in the set are untouched.
        let other = header.roster.iter().find(|e| e.agent == 0).unwrap();
        assert_eq!(other.reflex, "default");
    }

    #[test]
    fn pinned_header_starts_a_round() {
        // The pinned header is a valid round: the sim loads the embedded scripts.
        let d = temp_dir("sim");
        write(
            &d,
            "reflexes/default.rhai",
            "fn reflex(v) { #{ hold: true } }",
        );
        let mut header = Header::default_round(1, "fn reflex(v) { () }");
        let set: BTreeMap<u16, Harness> = header
            .roster
            .iter()
            .map(|e| (e.agent, Harness::open(&d).unwrap()))
            .collect();
        pin_into(&mut header, &set);
        let mut sim = lm_core::Sim::new(&header).expect("pinned header starts");
        for _ in 0..30 {
            sim.step(&[]);
        }
    }
}
