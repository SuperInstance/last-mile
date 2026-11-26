////! Deposit records human change requests between rounds.

use std::fmt::Write;

/// A deposit records one human change request between rounds
#[derive(Debug)]
pub struct Deposit {
    pub round_id: String,               // e.g. "r7"
    pub said: String,                   // the human's words, verbatim
    pub inferred: String,               // what the system understood them to want
    pub diff: String, // the exact change (a unified diff or a params patch), may be empty
    pub questions: Vec<String>, // open questions back to the human
    pub harness_commit: Option<String>, // the commit the change landed in, if applied
}

/// Converts a deposit to markdown format
pub fn to_markdown(d: &Deposit) -> String {
    let mut result = String::new();
    writeln!(result, "# Deposit {}", d.round_id).unwrap();
    writeln!(result, "## Said").unwrap();
    writeln!(result, "{}", d.said).unwrap();
    writeln!(result, "## Inferred").unwrap();
    writeln!(result, "{}", d.inferred).unwrap();

    if !d.diff.is_empty() {
        // Check if the diff contains backticks to determine fence length
        let fence = if d.diff.contains("```") {
            "````"
        } else {
            "```"
        };
        writeln!(result, "## Diff").unwrap();
        writeln!(result, "{}diff", fence).unwrap();
        writeln!(result, "{}", d.diff).unwrap();
        writeln!(result, "{}", fence).unwrap();
    }

    writeln!(result, "## Open questions").unwrap();
    if d.questions.is_empty() {
        writeln!(result, "_none_").unwrap();
    } else {
        for question in &d.questions {
            writeln!(result, "- {}", question).unwrap();
        }
    }

    writeln!(result, "## Applied").unwrap();
    match &d.harness_commit {
        Some(commit) => writeln!(result, "{}", commit).unwrap(),
        None => writeln!(result, "_not applied_").unwrap(),
    }

    result
}

/// Parses markdown format back into a deposit
pub fn from_markdown(s: &str) -> Result<Deposit, String> {
    let mut lines = s.lines().peekable();
    let mut deposit = Deposit {
        round_id: String::new(),
        said: String::new(),
        inferred: String::new(),
        diff: String::new(),
        questions: Vec::new(),
        harness_commit: None,
    };

    // Parse the deposit header
    if let Some(line) = lines.next() {
        if !line.starts_with("# Deposit ") {
            return Err("Missing deposit header".to_string());
        }
        deposit.round_id = line[9..].trim().to_string();
    } else {
        return Err("Missing deposit header".to_string());
    }

    let mut current_section = String::new();
    let mut current_content = String::new();

    for line in lines {
        if let Some(heading) = line.strip_prefix("## ") {
            // Save the previous section
            if !current_section.is_empty() {
                match current_section.as_str() {
                    "Said" => {
                        deposit.said = current_content.trim().to_string();
                    }
                    "Inferred" => {
                        deposit.inferred = current_content.trim().to_string();
                    }
                    "Diff" => {
                        // Remove the fence and any leading/trailing whitespace
                        let diff_lines: Vec<&str> = current_content.lines().collect();
                        if diff_lines.len() >= 2 {
                            // Skip the first line (fence) and last line (fence)
                            deposit.diff = diff_lines[1..diff_lines.len() - 1].join("\n");
                        }
                    }
                    "Open questions" => {
                        if current_content.trim() == "_none_" {
                            deposit.questions = Vec::new();
                        } else {
                            deposit.questions = current_content
                                .lines()
                                .filter_map(|l| l.strip_prefix("- ").map(str::to_string))
                                .collect();
                        }
                    }
                    "Applied" => {
                        let content = current_content.trim();
                        if content != "_not applied_" {
                            deposit.harness_commit = Some(content.to_string());
                        }
                    }
                    _ => {}
                }
            }

            // Start new section
            current_section = heading.to_string();
            current_content = String::new();
        } else {
            if !current_content.is_empty() {
                current_content.push('\n');
            }
            current_content.push_str(line);
        }
    }

    // Save the last section
    if !current_section.is_empty() {
        match current_section.as_str() {
            "Said" => {
                deposit.said = current_content.trim().to_string();
            }
            "Inferred" => {
                deposit.inferred = current_content.trim().to_string();
            }
            "Diff" => {
                // Remove the fence and any leading/trailing whitespace
                let diff_lines: Vec<&str> = current_content.lines().collect();
                if diff_lines.len() >= 2 {
                    // Skip the first line (fence) and last line (fence)
                    deposit.diff = diff_lines[1..diff_lines.len() - 1].join("\n");
                }
            }
            "Open questions" => {
                if current_content.trim() == "_none_" {
                    deposit.questions = Vec::new();
                } else {
                    deposit.questions = current_content
                        .lines()
                        .filter_map(|l| l.strip_prefix("- ").map(str::to_string))
                        .collect();
                }
            }
            "Applied" => {
                let content = current_content.trim();
                if content != "_not applied_" {
                    deposit.harness_commit = Some(content.to_string());
                }
            }
            _ => {}
        }
    }

    // Validate all required sections are present
    if deposit.said.is_empty() {
        return Err("Missing Said section".to_string());
    }
    if deposit.inferred.is_empty() {
        return Err("Missing Inferred section".to_string());
    }

    Ok(deposit)
}

/// Generates a filename for a deposit
pub fn file_name(d: &Deposit, seq: u32) -> String {
    format!("deposit-{}-{:03}.md", d.round_id, seq)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deposit_roundtrip() {
        let deposit = Deposit {
            round_id: "r7".to_string(),
            said: "Add a new feature".to_string(),
            inferred: "Implement a new feature".to_string(),
            diff: "diff --git a/src/main.rs b/src/main.rs\nindex 1234567..8901234 100644\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,3 +1,5 @@\n fn main() {\n     println!(\"Hello, world!\");\n+    // New feature\n+    println!(\"New feature implemented\");\n }".to_string(),
            questions: vec![],
            harness_commit: None,
        };

        let markdown = to_markdown(&deposit);
        let parsed = from_markdown(&markdown).unwrap();

        assert_eq!(deposit.round_id, parsed.round_id);
        assert_eq!(deposit.said, parsed.said);
        assert_eq!(deposit.inferred, parsed.inferred);
        assert_eq!(deposit.diff, parsed.diff);
        assert_eq!(deposit.questions, parsed.questions);
        assert_eq!(deposit.harness_commit, parsed.harness_commit);
    }

    #[test]
    fn missing_section_named() {
        let markdown = "# Deposit r7\n## Said\nSome text\n## Diff\n```diff\n\n```\n## Open questions\n_none_\n## Applied\n_not applied_";

        let result = from_markdown(markdown);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Inferred"));
    }

    #[test]
    fn file_name_format() {
        let deposit = Deposit {
            round_id: "r7".to_string(),
            said: "test".to_string(),
            inferred: "test".to_string(),
            diff: "".to_string(),
            questions: vec![],
            harness_commit: None,
        };

        assert_eq!(file_name(&deposit, 3), "deposit-r7-003.md");
    }

    #[test]
    fn diff_with_backticks_survives() {
        let deposit = Deposit {
            round_id: "r7".to_string(),
            said: "test".to_string(),
            inferred: "test".to_string(),
            diff: "```\nThis contains backticks\n```".to_string(),
            questions: vec![],
            harness_commit: None,
        };

        let markdown = to_markdown(&deposit);
        let parsed = from_markdown(&markdown).unwrap();

        assert_eq!(deposit.diff, parsed.diff);
    }
}
