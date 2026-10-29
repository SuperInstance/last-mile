//! Proposal parsing and validation for the learning system.

use regex::Regex;
use std::path::{Path, PathBuf};

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
}

/// Extracts the first JSON object from text using brace matching and parses it.
pub fn parse_proposal(text: &str) -> Result<Proposal, Rejection> {
    // Find the first top-level { and match braces properly
    let mut brace_count = 0;
    let mut start = None;
    let mut end = None;

    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '{' => {
                if brace_count == 0 {
                    start = Some(i);
                }
                brace_count += 1;
            }
            '}' => {
                brace_count -= 1;
                if brace_count == 0 && start.is_some() {
                    end = Some(i);
                    break;
                }
            }
            '"' => {
                // Skip strings
                i += 1; // Skip opening quote
                while i < chars.len() {
                    if chars[i] == '"' {
                        break;
                    } else if chars[i] == '\\' {
                        i += 2; // Skip escaped character
                    } else {
                        i += 1;
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }

    if let (Some(s), Some(e)) = (start, end) {
        let json_str = &text[s..=e];
        match serde_json::from_str::<Proposal>(json_str) {
            Ok(proposal) => Ok(proposal),
            Err(e) => Err(Rejection::NotJson(e.to_string())),
        }
    } else {
        Err(Rejection::NotJson("No valid JSON object found".to_string()))
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, fs};

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
