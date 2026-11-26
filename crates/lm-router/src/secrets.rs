//! Reading the keys, and never showing them.
//!
//! The keys live in `~/.config/last-mile/secrets.env`, one `NAME=value` per
//! line. They are read at runtime, when a pool is about to be used: a
//! missing key means *that pool is unavailable*, never a failed start-up.
//!
//! Nothing in this module prints a key. [`Secret`] is the only thing that
//! holds one, its `Debug` and `Display` are `***`, and the only way to read
//! the value is [`Secret::expose`] — named so that every use of a key is a
//! deliberate, greppable act.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::Path;

use crate::error::RouterError;

/// One API key.
///
/// The value is only reachable through [`Secret::expose`]. `Debug` and
/// `Display` print `***`, so a key cannot leak into a log line, an error
/// message or a test failure by accident.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret {
    value: String,
}

impl Secret {
    /// Wrap a value as a secret.
    pub fn new(value: impl Into<String>) -> Secret {
        Secret {
            value: value.into(),
        }
    }

    /// The value itself.
    ///
    /// Every call site is a place a key can leak, so this is deliberately
    /// awkward to call: it takes work to say it out loud.
    pub fn expose(&self) -> &str {
        &self.value
    }

    /// Whether the secret is empty.
    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

/// The keys, by name.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Secrets {
    entries: BTreeMap<String, Secret>,
}

impl Secrets {
    /// Parse the `NAME=value` form.
    ///
    /// Blank lines and `#` comments are ignored, an `export ` prefix is
    /// dropped, and a value may be wrapped in single or double quotes.
    pub fn parse(text: &str) -> Secrets {
        let mut entries = BTreeMap::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
            let Some((name, value)) = line.split_once('=') else {
                continue;
            };
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            let mut value = value.trim();
            if value.len() >= 2 {
                let first = value.chars().next().unwrap();
                let last = value.chars().last().unwrap();
                if (first == '"' && last == '"') || (first == '\'' && last == '\'') {
                    value = &value[1..value.len() - 1];
                }
            }
            entries.insert(name.to_string(), Secret::new(value));
        }
        Secrets { entries }
    }

    /// Read the keys from a file.
    ///
    /// A missing file is not an error: it means no pool has a key, so no
    /// pool can be used. Anything else (a directory, a permission problem)
    /// is an error.
    pub fn from_file(path: &Path) -> Result<Secrets, RouterError> {
        match fs::read_to_string(path) {
            Ok(text) => Ok(Secrets::parse(&text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Secrets::default()),
            Err(e) => Err(RouterError::Secrets {
                why: format!("{}: {e}", path.display()),
            }),
        }
    }

    /// The key with this name, if there is one.
    pub fn get(&self, name: &str) -> Option<&Secret> {
        self.entries.get(name)
    }

    /// The names of the keys that are present, in order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    /// Whether there are no keys at all.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The number of keys.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Remove every key value from a piece of text.
    ///
    /// Used on provider answers before they are stored in an error: a
    /// provider that echoes the request back would otherwise put the key
    /// into a log line.
    pub fn scrub(&self, text: &str) -> String {
        let mut out = text.to_string();
        for secret in self.entries.values() {
            let value = secret.expose();
            if value.is_empty() {
                continue;
            }
            if out.contains(value) {
                out = out.replace(value, "***");
            }
        }
        out
    }
}

impl fmt::Debug for Secrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Secrets")
            .field("keys", &self.names().collect::<Vec<_>>())
            .finish()
    }
}

impl fmt::Display for Secrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Secrets({})",
            self.names().collect::<Vec<_>>().join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_names_and_values() {
        let s = Secrets::parse(
            "# the cloud purse\nZAI_API_KEY=abc123\nMINIMAX_API_KEY=\"def456\"\n\nexport KIMI_API_KEY=ghi789\n",
        );
        assert_eq!(
            s.names().collect::<Vec<_>>(),
            ["KIMI_API_KEY", "MINIMAX_API_KEY", "ZAI_API_KEY"]
        );
        assert_eq!(s.get("ZAI_API_KEY").unwrap().expose(), "abc123");
        assert_eq!(s.get("MINIMAX_API_KEY").unwrap().expose(), "def456");
        assert_eq!(s.get("KIMI_API_KEY").unwrap().expose(), "ghi789");
    }

    #[test]
    fn a_line_without_an_equals_is_not_a_key() {
        let s = Secrets::parse("JUST_A_WORD\nANTHROPIC_API_KEY=xyz\n");
        assert_eq!(s.len(), 1);
        assert!(s.get("JUST_A_WORD").is_none());
    }

    #[test]
    fn a_missing_file_is_no_keys() {
        let path = std::env::temp_dir().join("lm-router-no-such-secrets.env");
        let s = Secrets::from_file(&path).unwrap();
        assert!(s.is_empty());
    }

    #[test]
    fn the_value_never_prints() {
        let secret = Secret::new("sk-do-not-print-me");
        assert!(!format!("{secret:?}").contains("sk-do-not-print-me"));
        assert!(!format!("{secret}").contains("sk-do-not-print-me"));
        let secrets = Secrets::parse("ZAI_API_KEY=sk-do-not-print-me\n");
        assert!(!format!("{secrets:?}").contains("sk-do-not-print-me"));
        assert!(!format!("{secrets}").contains("sk-do-not-print-me"));
        // The name is not a secret: it is useful to see which pools are ready.
        assert!(format!("{secrets:?}").contains("ZAI_API_KEY"));
    }

    #[test]
    fn scrub_removes_the_value() {
        let secrets = Secrets::parse("ZAI_API_KEY=sk-do-not-print-me\n");
        let text = "the key is sk-do-not-print-me, sorry";
        let scrubbed = secrets.scrub(text);
        assert!(!scrubbed.contains("sk-do-not-print-me"));
        assert!(scrubbed.contains("***"));
    }
}
