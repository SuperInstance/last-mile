//! The Orin build script: the steps that build the archive on the box and
//! run the golden check there.
//!
//! The script drives the Orin over SSH, so it cannot be run from this
//! machine — but it can be **read**, and that is what these tests do: they
//! check that the steps it would run are the right ones, and that running it
//! twice says the same thing twice.

use std::path::PathBuf;
use std::process::{Command, Output};

/// The script itself.
fn script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/orin-build.sh")
}

/// Run the script with the given arguments.
fn run(args: &[&str]) -> Output {
    Command::new("bash")
        .arg(script())
        .args(args)
        .env_remove("ORIN_HOST")
        .output()
        .expect("the script runs")
}

/// The text of the script's output, stdout and stderr together.
fn text_of(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn build_script_is_idempotent() {
    // Running the script twice is safe and says the same thing twice: no
    // timestamps, no random names, nothing that changes between the runs.
    let first = run(&["--dry-run"]);
    assert!(
        first.status.success(),
        "the dry run failed:\n{}",
        text_of(&first)
    );
    let second = run(&["--dry-run"]);
    assert!(
        second.status.success(),
        "the second dry run failed:\n{}",
        text_of(&second)
    );
    assert_eq!(
        text_of(&first),
        text_of(&second),
        "the script says something different the second time"
    );

    // And what it says is the right thing: the tree, the build, the golden
    // check, and the archive's own golden check.
    let text = text_of(&first);
    assert!(text.contains("git clone --branch"), "{text}");
    assert!(text.contains("git -C"), "the pull: {text}");
    assert!(
        text.contains("cargo build --release -p lm-archive -p lm-tools"),
        "{text}"
    );
    assert!(
        text.contains("cargo run --release --bin lm -- golden"),
        "the headline check: {text}"
    );
    assert!(
        text.contains("lm-archive\" golden"),
        "the recorded check: {text}"
    );
    // And the toolchain the box needs.
    assert!(text.contains("rustc --version"), "{text}");
    assert!(text.contains("nice"), "{text}");
    assert!(text.contains("ionice"), "{text}");
}

#[test]
fn the_build_script_wants_a_host() {
    // Without ORIN_HOST there is nothing to drive, and the script says so
    // rather than guessing.
    let output = run(&[]);
    assert_eq!(output.status.code(), Some(2), "{}", text_of(&output));
    let text = text_of(&output);
    assert!(text.contains("ORIN_HOST"), "{text}");
}

#[test]
fn the_build_script_uses_the_phase_branch() {
    // The phase lands on phase6-3d, and the script says so.
    let output = run(&["--dry-run"]);
    let text = text_of(&output);
    assert!(text.contains("phase6-3d"), "{text}");
}
