//! The outside-in acceptance suite: the `lm` binary, driven the way a person
//! drives it.
//!
//! Every test here runs the built binary with `std::process::Command` and
//! checks what a person would check: what it printed on each stream, and what
//! it returned. Nothing reaches inside the crates, so a test here fails when
//! the tool stops being usable — not only when the simulation drifts.
//!
//! The tool is the product. A round that cannot be replayed, a diff that
//! cannot say where two rounds parted company, an error that does not name
//! the file it is talking about: these are the failures a user hits first,
//! and they are invisible to a test that calls the runner directly.
//!
//! Each test writes into a scratch directory of its own — the system temp
//! directory plus a name nothing else uses — so the tests leave the
//! repository alone and can run side by side.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The ticks a test round plays: short enough to be quick, long enough to
/// leave state hashes behind.
const TICKS: u64 = 900;

/// What the binary said, and what it returned.
#[derive(Clone, Debug)]
struct Answer {
    /// The exit code of the process.
    code: i32,
    /// Everything it wrote to stdout.
    stdout: String,
    /// Everything it wrote to stderr.
    stderr: String,
}

impl Answer {
    /// Both streams, for the messages that say what went wrong.
    fn said(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }
}

/// Run the `lm` binary with `args`, from `dir`.
///
/// This is the whole harness of the suite: the binary cargo built, run as a
/// child process, with its output captured.
fn lm(dir: &Path, args: &[&str]) -> Answer {
    let output = Command::new(env!("CARGO_BIN_EXE_lm"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("the lm binary runs");
    Answer {
        code: output.status.code().expect("the binary exits with a code"),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// The path as a string, for handing to the binary.
fn as_str(path: &Path) -> &str {
    path.to_str().expect("the paths are UTF-8")
}

/// A scratch directory that cleans itself up.
struct Scratch(PathBuf);

impl Scratch {
    /// A directory of this test's own, in the system temp directory.
    ///
    /// The name carries the process id and a counter, so two tests — or two
    /// runs of the same test — never share a directory.
    fn new(name: &str) -> Scratch {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("lm-cli-{}-{n}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the scratch directory is created");
        Scratch(dir)
    }

    /// The directory itself.
    fn path(&self) -> &Path {
        &self.0
    }

    /// A path inside the directory.
    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The number of hashes in `OK <n> hashes`.
fn ok_hashes(stdout: &str) -> usize {
    let text = stdout.trim();
    let rest = text
        .strip_prefix("OK ")
        .expect("the replay says 'OK <n> hashes'");
    let digits = rest
        .strip_suffix(" hashes")
        .expect("... and ends in hashes");
    digits.parse().expect("... and the count is a number")
}

/// The number of hashes the run said it made: `... <n> hashes, ...`.
fn run_hashes(stdout: &str) -> usize {
    let words: Vec<&str> = stdout.split_whitespace().collect();
    for (index, word) in words.iter().enumerate() {
        if *word == "hashes," {
            return words[index - 1]
                .parse()
                .expect("the run counts the hashes it made");
        }
    }
    panic!("the run says how many hashes it made: {stdout:?}");
}

/// The tick and the two hashes in `DIFFER at tick <n>: <left> != <right>`.
fn differ(stdout: &str) -> (u64, String, String) {
    let text = stdout.trim();
    let rest = text
        .strip_prefix("DIFFER at tick ")
        .expect("the diff says 'DIFFER at tick <n>'");
    let (tick, rest) = rest.split_once(": ").expect("... and then the two hashes");
    let (left, right) = rest.split_once(" != ").expect("... one against the other");
    (
        tick.trim().parse().expect("... the tick is a number"),
        left.trim().to_string(),
        right.trim().to_string(),
    )
}

/// A round of `seed` and `ticks`, written to `out` in `scratch`.
fn run_round(scratch: &Scratch, seed: u64, ticks: u64, out: &Path) -> Answer {
    let ticks = ticks.to_string();
    let seed = seed.to_string();
    lm(
        scratch.path(),
        &[
            "run",
            "--seed",
            &seed,
            "--ticks",
            &ticks,
            "--out",
            as_str(out),
        ],
    )
}

#[test]
fn a_run_replays_the_hashes_it_recorded() {
    let scratch = Scratch::new("replay");
    let log = scratch.join("a.lml");

    let run = run_round(&scratch, 1, TICKS, &log);
    assert_eq!(run.code, 0, "the run failed: {}", run.said());
    assert!(run.stderr.is_empty(), "the run kept quiet: {}", run.said());
    assert!(
        run.stdout.contains(&format!("{TICKS} ticks")),
        "the run says how long it played: {}",
        run.said()
    );
    assert!(log.exists(), "the run wrote its log");

    let replay = lm(scratch.path(), &["replay", as_str(&log)]);
    assert_eq!(replay.code, 0, "the replay failed: {}", replay.said());
    assert!(
        replay.stderr.is_empty(),
        "the replay kept quiet: {}",
        replay.said()
    );

    // The replay says how many hashes it checked.
    let checked = ok_hashes(&replay.stdout);
    assert!(
        checked > 0,
        "the replay checked something: {}",
        replay.said()
    );
    // And it checked exactly what the run made: same round, same hashes.
    assert_eq!(
        checked,
        run_hashes(&run.stdout),
        "the replay checked the hashes the run made: {}",
        replay.said()
    );
}

#[test]
fn the_same_seed_twice_writes_the_same_bytes() {
    let scratch = Scratch::new("same-seed");
    let first = scratch.join("first.lml");
    let second = scratch.join("second.lml");

    for out in [&first, &second] {
        let run = run_round(&scratch, 42, 600, out);
        assert_eq!(run.code, 0, "the run failed: {}", run.said());
    }

    let one = std::fs::read(&first).expect("the first log reads");
    let two = std::fs::read(&second).expect("the second log reads");
    assert!(!one.is_empty(), "the log has content");
    assert_eq!(one, two, "the same seed writes the same bytes");
}

#[test]
fn diff_names_the_first_tick_that_moves() {
    let scratch = Scratch::new("diff");
    let one = scratch.join("one.lml");
    let two = scratch.join("two.lml");

    let run = run_round(&scratch, 1, TICKS, &one);
    assert_eq!(run.code, 0, "the run failed: {}", run.said());
    let run = run_round(&scratch, 2, TICKS, &two);
    assert_eq!(run.code, 0, "the run failed: {}", run.said());

    // Two different rounds: the diff names the tick they parted company on,
    // and says so by failing.
    let diff = lm(scratch.path(), &["diff", as_str(&one), as_str(&two)]);
    assert_eq!(
        diff.code,
        1,
        "the diff reports the difference: {}",
        diff.said()
    );
    assert!(
        diff.stderr.is_empty(),
        "the diff kept quiet: {}",
        diff.said()
    );
    let (tick, left, right) = differ(&diff.stdout);
    assert!(tick > 0, "the diff names a tick: {}", diff.said());
    assert_ne!(left, right, "the two hashes differ: {}", diff.said());

    // The same round twice: nothing moved, so the diff succeeds.
    let same = lm(scratch.path(), &["diff", as_str(&one), as_str(&one)]);
    assert_eq!(
        same.code,
        0,
        "the diff of a round with itself: {}",
        same.said()
    );
    assert!(
        same.stderr.is_empty(),
        "the diff kept quiet: {}",
        same.said()
    );
    assert_eq!(
        same.stdout.trim(),
        "IDENTICAL",
        "the diff of a round with itself says so: {}",
        same.said()
    );
}

#[test]
fn a_truncated_log_is_named() {
    let scratch = Scratch::new("truncated");
    let log = scratch.join("a.lml");
    let cut = scratch.join("cut.lml");

    let run = run_round(&scratch, 1, TICKS, &log);
    assert_eq!(run.code, 0, "the run failed: {}", run.said());

    // Cut the log inside its last blob: the file ends in the middle of a
    // round, and the tool has to say so rather than replay half of it.
    let bytes = std::fs::read(&log).expect("the log reads");
    assert!(bytes.len() > 32, "the log has content");
    std::fs::write(&cut, &bytes[..bytes.len() - 16]).expect("the cut log is written");

    let replay = lm(scratch.path(), &["replay", as_str(&cut)]);
    assert_eq!(replay.code, 1, "the replay refuses: {}", replay.said());
    assert!(
        replay.stdout.is_empty(),
        "the replay kept quiet: {}",
        replay.said()
    );
    assert!(
        replay.stderr.contains("truncated"),
        "the message names the damage: {}",
        replay.said()
    );
    assert!(
        replay.stderr.contains(as_str(&cut)),
        "the message names the file: {}",
        replay.said()
    );
}

#[test]
fn a_missing_file_is_named() {
    let scratch = Scratch::new("missing");
    let missing = scratch.join("not-here.lml");

    let replay = lm(scratch.path(), &["replay", as_str(&missing)]);
    assert_eq!(replay.code, 1, "the replay fails: {}", replay.said());
    assert!(
        replay.stdout.is_empty(),
        "the replay kept quiet: {}",
        replay.said()
    );
    assert!(
        replay.stderr.contains(as_str(&missing)),
        "the message names the path: {}",
        replay.said()
    );

    // The same goes for a diff: one of the two files is not there.
    let here = scratch.join("here.lml");
    let run = run_round(&scratch, 1, 600, &here);
    assert_eq!(run.code, 0, "the run failed: {}", run.said());
    let diff = lm(scratch.path(), &["diff", as_str(&here), as_str(&missing)]);
    assert_eq!(diff.code, 1, "the diff fails: {}", diff.said());
    assert!(
        diff.stderr.contains(as_str(&missing)),
        "the message names the path: {}",
        diff.said()
    );
}

#[test]
fn an_unknown_subcommand_is_a_usage_error() {
    let scratch = Scratch::new("unknown");

    let answer = lm(scratch.path(), &["frobnicate"]);
    assert_eq!(answer.code, 2, "a usage error is exit 2: {}", answer.said());
    assert!(
        answer.stdout.is_empty(),
        "the error went to stderr: {}",
        answer.said()
    );
    assert!(
        answer.stderr.contains("unrecognized subcommand"),
        "the error says what is wrong: {}",
        answer.said()
    );
    assert!(
        answer.stderr.contains("lm <COMMAND>"),
        "the usage names the command: {}",
        answer.said()
    );
}

#[test]
fn golden_matches_the_pinned_hashes() {
    let scratch = Scratch::new("golden");

    let answer = lm(scratch.path(), &["golden"]);
    assert_eq!(answer.code, 0, "the golden check failed: {}", answer.said());
    assert!(answer.stderr.is_empty(), "it kept quiet: {}", answer.said());
    assert_eq!(
        answer.stdout.trim(),
        "OK 3/3",
        "the three pinned seeds match: {}",
        answer.said()
    );
}

#[test]
fn a_round_of_zero_ticks_is_an_empty_but_valid_round() {
    let scratch = Scratch::new("zero-ticks");
    let log = scratch.join("empty.lml");

    // A round of no ticks is a round: the log has a header and no ticks, and
    // it is still a log the tool can read back.
    let run = run_round(&scratch, 1, 0, &log);
    assert_eq!(run.code, 0, "the run failed: {}", run.said());
    assert!(run.stderr.is_empty(), "the run kept quiet: {}", run.said());
    assert!(
        run.stdout.contains("empty round"),
        "the run says the round was empty: {}",
        run.said()
    );
    assert!(log.exists(), "the run wrote its log");
    assert!(
        std::fs::metadata(&log).expect("the log has a size").len() > 0,
        "the log has content"
    );

    // And it replays: an empty round is a valid round.
    let replay = lm(scratch.path(), &["replay", as_str(&log)]);
    assert_eq!(replay.code, 0, "the replay failed: {}", replay.said());
    assert!(
        replay.stderr.is_empty(),
        "the replay kept quiet: {}",
        replay.said()
    );
    assert_eq!(
        replay.stdout.trim(),
        "OK 0 hashes",
        "the empty round replays: {}",
        replay.said()
    );
}

#[test]
fn a_broken_injection_line_is_named() {
    let scratch = Scratch::new("inject");
    let inject = scratch.join("decisions.jsonl");
    let out = scratch.join("a.lml");

    // Line 1 is a comment, line 2 is a decision, line 3 is broken.
    let text = concat!(
        "# a comment\n",
        r#"{"arrived": 1, "decision": {"apply_tick": 1, "source": {"Agent": 0}, "seq": 1, "kind": {"ActionOn": "Hold"}, "provenance": "Test"}}"#,
        "\n",
        r#"{"arrived": 2}"#,
        "\n",
    );
    std::fs::write(&inject, text).expect("the injection file is written");

    let run = lm(
        scratch.path(),
        &[
            "run",
            "--seed",
            "1",
            "--ticks",
            "60",
            "--inject",
            as_str(&inject),
            "--out",
            as_str(&out),
        ],
    );
    assert_ne!(
        run.code,
        0,
        "the run refuses the broken file: {}",
        run.said()
    );
    assert!(run.stdout.is_empty(), "the run kept quiet: {}", run.said());
    assert!(
        run.stderr.contains("injection line 3"),
        "the message names the line: {}",
        run.said()
    );
    // A file that did not read leaves no log behind.
    assert!(!out.exists(), "the run wrote no log");
}

#[test]
fn a_run_cannot_write_into_a_directory_that_is_not_there() {
    let scratch = Scratch::new("no-directory");
    let out = scratch.join("not-a-directory").join("round.lml");

    let run = run_round(&scratch, 1, 60, &out);
    assert_ne!(run.code, 0, "the run fails: {}", run.said());
    assert!(run.stdout.is_empty(), "the run kept quiet: {}", run.said());
    assert!(
        run.stderr.contains("cannot write"),
        "the message says what went wrong: {}",
        run.said()
    );
    assert!(
        run.stderr.contains(as_str(&out)),
        "the message names the path: {}",
        run.said()
    );
}
