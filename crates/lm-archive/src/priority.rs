//! Yielding to the desktop: the service's own CPU and I/O priority.
//!
//! There is **no std API** for `setpriority` or `ioprio_set`:
//! `std::os::unix::process::CommandExt` exposes `uid`, `gid`, `groups`,
//! `pre_exec`, `exec`, `arg0`, `process_group`, `chroot` and `setsid` — and
//! none of them is a scheduler call. `libc` is in the local registry, but the
//! archive is std-only by design, so the priority is applied the way the
//! other tools are: by **wrapping the command**.
//!
//! - `nice -n <n>` raises the niceness — lowers the CPU priority. The range
//!   is −20 (most favourable) to 19 (least favourable), and `nice -n n`
//!   **adds** `n` to the niceness the process already has. The default is
//!   **10**, which is `nice`'s own default and the conventional "background"
//!   value.
//! - `ionice -c <class>` sets the I/O scheduling class. **3 is idle**: the
//!   process does its I/O only when nothing else is pending. The default is
//!   **3**.
//!
//! Both binaries have to be on the box (`/usr/bin/nice`, `/usr/bin/ionice`);
//! the handoff says to confirm that on the Orin before deployment.
//!
//! ## The re-exec
//!
//! [`apply_to_self`] re-executes the process under `nice`/`ionice`. The
//! service comes back up as the **same process** — the same pid, the same
//! systemd unit, the same sockets — at the priority it asked for. An
//! environment variable marks the re-exec, so it happens **exactly once**:
//! the second call sees the mark and returns without exec'ing again.
//!
//! ## The check
//!
//! `nice` can only *raise* niceness, and the value is capped at 19, so "the
//! command ran" is not "the priority took". [`is_low_priority`] reads field
//! 19 of `/proc/self/stat` — the niceness the kernel actually gave this
//! process — and compares it with what was asked for. The I/O class is read
//! back the same way, with `ionice -p`.
//!
//! ## The premise
//!
//! The spec only says the Orin "archives and verifies"; nothing in the repo
//! says the Orin runs a desktop. Check the box first
//! (`ps -eo comm | grep -iE 'xorg|wayland|gnome|kde'`) before assuming it
//! has to yield. The yielding is on by default either way, because it is
//! harmless insurance.

use std::env;
use std::ffi::OsString;
use std::io;
use std::process::Command;

use crate::ArchiveConfig;

/// The environment variable that marks the priority as already applied.
///
/// The re-exec sets it, and the re-exec'd process sees it: that is what
/// keeps the re-exec from looping.
pub const APPLIED_ENV: &str = "LM_ARCHIVE_PRIORITY";

/// The environment variable that carries the niceness the service asked for.
///
/// The re-exec sets it, and [`is_low_priority`] compares the process's real
/// niceness with it — the request, not a guess.
pub const NICE_ENV: &str = "LM_ARCHIVE_NICE";

/// The default niceness: `nice`'s own default, and the conventional
/// "background" value.
pub const DEFAULT_NICE: i32 = 10;

/// The default I/O scheduling class: 3, idle.
pub const DEFAULT_IO_CLASS: u8 = 3;

/// The I/O scheduling class that means "idle": I/O only when nothing else is
/// pending.
pub const IO_CLASS_IDLE: u8 = 3;

/// Apply the configured priority to **this** process, by re-executing it
/// under `nice` and `ionice`.
///
/// The process comes back as itself — the same pid, so a systemd unit that
/// tracks the main pid is happy — with the priority it asked for. The
/// re-exec happens **once**: the environment variable [`APPLIED_ENV`] marks
/// it, and a process that already carries the mark returns without exec'ing.
///
/// If the re-exec cannot happen (no `nice`, no `ionice`, a niceness the
/// user may not set), the error names the reason and the caller decides what
/// to do. The archive keeps serving either way: an archive that refuses to
/// start would be a worse outcome than an archive that runs at the
/// foreground priority.
pub fn apply_to_self(cfg: &ArchiveConfig) -> io::Result<()> {
    if already_applied() {
        return Ok(());
    }
    let exe = env::current_exe()?;
    let mut cmd = Command::new(exe);
    cmd.args(env::args().skip(1));
    wrap(&mut cmd, cfg)?;
    exec(cmd)
}

/// Wrap a command so that it runs under `nice` and `ionice`.
///
/// The command is rewritten as
/// `ionice -c <class> nice -n <nice> <program> <args...>`, and the
/// environment of the wrapper carries the [`APPLIED_ENV`] and [`NICE_ENV`]
/// marks, so a process that was started this way knows it is already at the
/// priority and does not re-exec itself again.
///
/// `ionice` comes first so that the class is set on the process that
/// eventually execs the program: the I/O priority is inherited across the
/// fork and kept across the exec.
pub fn wrap(cmd: &mut Command, cfg: &ArchiveConfig) -> io::Result<()> {
    let program = cmd.get_program().to_os_string();
    let args: Vec<OsString> = cmd.get_args().map(OsString::from).collect();

    let mut wrapped = Command::new("ionice");
    wrapped.arg("-c").arg(cfg.ionice_class.to_string());
    wrapped.arg("nice");
    wrapped.arg("-n").arg(cfg.nice.to_string());
    wrapped.arg(&program);
    wrapped.args(&args);
    wrapped.env(APPLIED_ENV, "1");
    wrapped.env(NICE_ENV, cfg.nice.to_string());

    *cmd = wrapped;
    Ok(())
}

/// Whether the priority has already been applied to this process.
///
/// True when the process was started by a wrapped command — or by a
/// re-exec — and therefore must not re-exec itself again.
pub fn already_applied() -> bool {
    env::var(APPLIED_ENV).is_ok_and(|value| !value.is_empty())
}

/// The niceness the service asked for.
///
/// It is read from the environment the re-exec set, and falls back to
/// [`DEFAULT_NICE`]: a process that was never wrapped has asked for
/// nothing, and the conventional background value is the safe answer.
pub fn target_nice() -> i32 {
    env::var(NICE_ENV)
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(DEFAULT_NICE)
}

/// Whether this process is running at the priority it asked for.
///
/// The niceness comes from field 19 of `/proc/self/stat`, which is what the
/// kernel gave the process — not what was requested. A request that could
/// not be met (the value is capped at 19, and only a privileged process may
/// lower niceness) is therefore caught here rather than assumed.
pub fn is_low_priority() -> bool {
    is_low_priority_at(target_nice())
}

/// Whether this process is running at least as low a priority as `target`.
///
/// The comparison is "at least": `nice -n 10` **adds** to the niceness the
/// process already had, so a service that started at 5 ends at 15, and a
/// service that started at 19 stays at 19. Both are at least 10, and both
/// pass.
pub fn is_low_priority_at(target: i32) -> bool {
    match current_nice() {
        Ok(nice) => nice >= target,
        Err(_) => false,
    }
}

/// The niceness the kernel gave this process.
///
/// Field 19 of `/proc/self/stat` is the niceness. The second field is the
/// command name in brackets, and it may contain spaces, so the fields are
/// counted from the **last** `)`: everything after it starts at field 3.
pub fn current_nice() -> io::Result<i32> {
    let stat = std::fs::read_to_string("/proc/self/stat")?;
    let Some(at) = stat.rfind(')') else {
        return Err(io::Error::other("/proc/self/stat has no command name"));
    };
    // Fields 3 to 18 come first; field 19 is the niceness.
    let mut fields = stat[at + 1..].split_whitespace();
    for _ in 0..16 {
        if fields.next().is_none() {
            return Err(io::Error::other(
                "/proc/self/stat is too short to hold a niceness",
            ));
        }
    }
    fields
        .next()
        .and_then(|value| value.parse::<i32>().ok())
        .ok_or_else(|| io::Error::other("/proc/self/stat has no readable niceness"))
}

/// The I/O scheduling class this process is running at.
///
/// The class is read back with `ionice -p`, because the kernel does not
/// publish it in `/proc`. The answer is the class number: 0 is "none", 1 is
/// realtime, 2 is best-effort and 3 is idle.
pub fn current_io_class() -> io::Result<u8> {
    let pid = std::process::id();
    let output = Command::new("ionice")
        .arg("-p")
        .arg(pid.to_string())
        .output()
        .map_err(|err| io::Error::new(io::ErrorKind::NotFound, format!("ionice: {err}")))?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        return Err(io::Error::other(format!(
            "ionice -p {pid} failed: {}",
            message.trim()
        )));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let text = text.trim();
    // "idle", "none: prio 0", "best-effort: prio 4", "realtime: prio 0".
    if text.starts_with("idle") {
        return Ok(IO_CLASS_IDLE);
    }
    if text.starts_with("none") {
        return Ok(0);
    }
    if text.starts_with("best-effort") {
        return Ok(2);
    }
    if text.starts_with("realtime") {
        return Ok(1);
    }
    Err(io::Error::other(format!("ionice said {text:?}")))
}

/// Replace this process with `cmd`.
///
/// `exec` keeps the pid, so the service stays the process the systemd unit
/// is watching. It only ever comes back when the exec **failed**, and then
/// it comes back with the reason.
#[cfg(unix)]
fn exec(mut cmd: Command) -> io::Result<()> {
    use std::os::unix::process::CommandExt;
    Err(cmd.exec())
}

/// Replace this process with `cmd`.
///
/// A process that is not on Unix cannot be re-exec'd under `nice`/`ionice`.
#[cfg(not(unix))]
fn exec(_cmd: Command) -> io::Result<()> {
    Err(io::Error::other(
        "the archive's priority needs a Unix process",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::{Duration, Instant};

    /// The name of the test the child process runs.
    const CHILD_TEST: &str = "the_re_exec_child";

    /// The environment variable that carries the counter that stops a
    /// re-exec loop from running forever.
    const COUNTER_ENV: &str = "LM_ARCHIVE_REEXEC_COUNTER";

    /// A command that does nothing, for the wrap tests.
    fn sleep_command() -> Command {
        let mut cmd = Command::new("sleep");
        cmd.arg("30");
        cmd
    }

    /// The niceness of a process, read the way the kernel writes it.
    fn nice_of(pid: u32) -> i32 {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).expect("the stat file");
        let at = stat.rfind(')').expect("the command name");
        stat[at + 1..]
            .split_whitespace()
            .nth(16)
            .and_then(|value| value.parse::<i32>().ok())
            .expect("the niceness")
    }

    #[test]
    fn the_defaults_are_the_conventional_background_ones() {
        let cfg = ArchiveConfig::default();
        assert_eq!(cfg.nice, DEFAULT_NICE);
        assert_eq!(cfg.ionice_class, DEFAULT_IO_CLASS);
        assert_eq!(DEFAULT_NICE, 10);
        assert_eq!(DEFAULT_IO_CLASS, 3);
    }

    #[test]
    fn the_wrap_names_the_priority_it_applies() {
        let mut cmd = sleep_command();
        wrap(&mut cmd, &ArchiveConfig::default()).expect("the command is wrapped");

        assert_eq!(cmd.get_program(), "ionice");
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args[0], "-c");
        assert_eq!(args[1], "3");
        assert_eq!(args[2], "nice");
        assert_eq!(args[3], "-n");
        assert_eq!(args[4], "10");
        assert_eq!(args[5], "sleep");
        assert_eq!(args[6], "30");

        // The wrapper carries the marks, so the wrapped process knows it is
        // already at the priority.
        let envs: Vec<(String, Option<String>)> = cmd
            .get_envs()
            .map(|(key, value)| {
                (
                    key.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect();
        assert!(envs.contains(&(APPLIED_ENV.to_owned(), Some("1".to_owned()))));
        assert!(envs.contains(&(NICE_ENV.to_owned(), Some("10".to_owned()))));
    }

    #[test]
    fn a_wrapped_process_really_is_at_the_priority() {
        // The mechanism itself: a process started the way the service starts
        // itself is at the configured priority, in the kernel's own words.
        let cfg = ArchiveConfig {
            nice: 10,
            ionice_class: 3,
            ..ArchiveConfig::default()
        };
        let mut cmd = sleep_command();
        wrap(&mut cmd, &cfg).expect("the command is wrapped");
        let mut child = cmd.spawn().expect("the child starts");

        let pid = child.id();
        // Give the wrapper a moment to exec the program.
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut nice = nice_of(pid);
        while nice < cfg.nice && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
            nice = nice_of(pid);
        }
        assert!(
            nice >= cfg.nice,
            "the child's niceness is {nice}, not at least {}",
            cfg.nice
        );

        // And the I/O class is idle.
        let class = current_io_class_of(pid);
        assert_eq!(class, IO_CLASS_IDLE, "the child's I/O class is {class}");

        let _ = child.kill();
        let _ = child.wait();
    }

    /// The I/O class of another process, read back with `ionice`.
    fn current_io_class_of(pid: u32) -> u8 {
        let output = Command::new("ionice")
            .arg("-p")
            .arg(pid.to_string())
            .output()
            .expect("ionice runs");
        let text = String::from_utf8_lossy(&output.stdout);
        let text = text.trim();
        if text.starts_with("idle") {
            return IO_CLASS_IDLE;
        }
        if text.starts_with("none") {
            return 0;
        }
        if text.starts_with("best-effort") {
            return 2;
        }
        if text.starts_with("realtime") {
            return 1;
        }
        panic!("ionice said {text:?}");
    }

    #[test]
    fn the_niceness_is_read_from_the_kernel() {
        // The reading agrees with what the kernel says about this very
        // process: `ps -o ni=` is an independent oracle.
        let mine = current_nice().expect("the niceness reads");
        let output = Command::new("ps")
            .arg("-o")
            .arg("ni=")
            .arg("-p")
            .arg(std::process::id().to_string())
            .output()
            .expect("ps runs");
        let theirs = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<i32>()
            .expect("ps prints a number");
        assert_eq!(mine, theirs);
    }

    #[test]
    fn the_priority_check_follows_the_kernel() {
        let mine = current_nice().expect("the niceness reads");
        // At least as low as itself: true.
        assert!(is_low_priority_at(mine));
        // One step lower than itself: false, whatever the machine does.
        assert!(!is_low_priority_at(mine + 1));
    }

    #[test]
    fn the_guard_is_the_environment_variable() {
        // A process that was never wrapped has not applied the priority.
        assert_eq!(
            already_applied(),
            env::var(APPLIED_ENV).is_ok_and(|v| !v.is_empty())
        );
    }

    #[test]
    fn the_re_exec_child() {
        // This test is the child of `re_exec_is_idempotent`. It only does
        // something when the parent set the counter's path: the counter is
        // what stops a broken guard from looping forever.
        let Ok(path) = env::var(COUNTER_ENV) else {
            return;
        };
        let generation = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| text.trim().parse::<u32>().ok())
            .unwrap_or(0);
        if generation >= 2 {
            panic!("the re-exec looped: generation {generation}");
        }
        std::fs::write(&path, (generation + 1).to_string()).expect("the counter moves");
        eprintln!("child: before generation {generation}");
        apply_to_self(&ArchiveConfig::default()).expect("the re-exec happens");
        eprintln!("child: after generation {generation}");
    }

    #[test]
    fn re_exec_is_idempotent() {
        // The child runs the same test twice: once as the process that was
        // started, and once as the process that replaced it. The second run
        // must not exec again — that is the guard, and it is what keeps the
        // service from forking itself forever.
        let exe = env::current_exe().expect("the test binary");
        let counter = env::temp_dir().join(format!(
            "lm-archive-reexec-{}-{}",
            std::process::id(),
            CHILD_TEST
        ));
        let _ = std::fs::remove_file(&counter);

        let output = Command::new(&exe)
            .arg(CHILD_TEST)
            .arg("--nocapture")
            .env(COUNTER_ENV, &counter)
            .output()
            .expect("the child runs");
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        assert_eq!(
            text.matches("child: before").count(),
            2,
            "the child execs exactly once, and the process it became does not exec again:\n{text}"
        );
        assert_eq!(
            text.matches("child: after").count(),
            1,
            "the re-exec'd process returns without exec'ing again:\n{text}"
        );
        assert!(output.status.success(), "the child finished clean:\n{text}");
        let _ = std::fs::remove_file(&counter);
    }
}
