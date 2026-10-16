//! Signals, as a person at a terminal generates them.
//!
//! Spec §18.1: "Ono-Sendai MUST support foreground processes, background jobs, signals, terminal
//! process groups and PTYs well enough to run normal interactive Unix software." Ctrl-C is the
//! one every user presses, and it has to do two things at once: reach the running command, and
//! leave the shell standing.
//!
//! These drive the real binary through a real pseudo-terminal, because the behaviour only exists
//! on a terminal and because the timing matters — a Ctrl-C fed from a file arrives before the
//! shell has started, which tests nothing.

#![allow(
    clippy::panic,
    clippy::expect_used,
    reason = "AGENTS.md §16: a helper shared by tests states its preconditions the same way a test does"
)]

use std::time::{Duration, Instant};

use ono_process::{Command, Executor, Signal, WindowSize};

mod support;
use support::read_until;

/// Starts `ono` interactively on a pseudo-terminal of a known size.
fn interactive_shell() -> ono_testkit::Guarded<ono_process::PtySession> {
    let mut executor = Executor::detached();
    let command = Command::new(ono_testkit::ono_binary())
        .env("TERM", "xterm")
        .env("NO_COLOR", "1")
        .env("HOME", std::env::temp_dir().display().to_string());
    executor
        .run_pty(&command, WindowSize::new(24, 80))
        .map(|session| ono_testkit::Guarded::new(session, ono_process::PtySession::pid))
        .expect("a pseudo-terminal must be available")
}

/// Reads from the terminal until `needle` appears or `budget` runs out.
///
/// A terminal echoes what is typed, and the editor repaints the line on every keystroke, so any
/// needle taken from the command text appears in the stream long before the command has run.
/// Every needle here is therefore something only the *output* can contain — `alive-$?` is typed
/// and `alive-130` comes back.
#[test]
fn should_interrupt_the_running_command_and_leave_the_prompt_standing() {
    let mut shell = interactive_shell();
    read_until(&mut shell, ">", Duration::from_secs(10));

    shell
        .write_all(b"sleep 30\n")
        .expect("the terminal accepts input");
    // Long enough that the shell has certainly given `sleep` the terminal, short enough that the
    // test is not slow. A Ctrl-C that arrives before the child exists proves nothing.
    std::thread::sleep(Duration::from_millis(600));

    // Exactly what the terminal driver does when a person presses Ctrl-C: the byte, on the line
    // discipline, which turns it into SIGINT for the foreground process group.
    shell
        .write_all(&[0x03])
        .expect("the terminal accepts Ctrl-C");

    // `alive-$?` is typed; `alive-130` can only come back from a shell that survived the
    // interrupt, ran the command, and knows what the interrupted command's status was.
    shell
        .write_all(b"echo alive-$?\n")
        .expect("the terminal accepts input");
    let seen = read_until(&mut shell, "alive-130", Duration::from_secs(20));

    assert!(
        seen.contains("alive-130"),
        "the shell must survive Ctrl-C and keep taking commands; saw:\n{seen}"
    );

    shell
        .write_all(b"exit 0\n")
        .expect("the terminal accepts input");
    let status = shell
        .wait_timeout(Duration::from_secs(20))
        .expect("waiting works")
        .expect("the shell must exit when asked");
    assert_eq!(
        status.code(),
        0,
        "the shell exits with the status it was given"
    );
}

#[test]
fn should_not_wait_thirty_seconds_for_a_command_that_was_interrupted() {
    // The interruption has to actually reach `sleep`, not merely be swallowed by the shell.
    let mut shell = interactive_shell();
    read_until(&mut shell, ">", Duration::from_secs(10));

    let started = Instant::now();
    shell.write_all(b"sleep 30\n").expect("input");
    std::thread::sleep(Duration::from_millis(600));
    shell.write_all(&[0x03]).expect("input");
    shell.write_all(b"echo alive-$?\n").expect("input");
    let seen = read_until(&mut shell, "alive-130", Duration::from_secs(20));
    assert!(seen.contains("alive-130"), "saw:\n{seen}");

    assert!(
        started.elapsed() < Duration::from_secs(25),
        "`sleep 30` was not interrupted; the whole exchange took {:?}",
        started.elapsed()
    );

    shell.write_all(b"exit 0\n").expect("input");
    let _ = shell.wait_timeout(Duration::from_secs(20));
}

#[test]
fn should_report_a_command_the_terminal_interrupted_as_128_plus_sigint() {
    // ADR-0008's status contract, observed the way a script observes it.
    let mut shell = interactive_shell();
    read_until(&mut shell, ">", Duration::from_secs(10));

    shell.write_all(b"sleep 30\n").expect("input");
    std::thread::sleep(Duration::from_millis(600));
    shell.write_all(&[0x03]).expect("input");
    shell.write_all(b"echo status-was-$?\n").expect("input");
    let seen = read_until(&mut shell, "status-was-130", Duration::from_secs(20));

    assert!(
        seen.contains("status-was-130"),
        "an interrupted command reports 128 + SIGINT; saw:\n{seen}"
    );

    shell.write_all(b"exit 0\n").expect("input");
    let _ = shell.wait_timeout(Duration::from_secs(20));
}

#[test]
fn should_kill_the_shell_when_a_non_interactive_run_is_interrupted() {
    // A script is not an interactive session and must not ignore the signal: a `ono -c` inside a
    // pipeline that a user interrupts has to die like every other program, or Ctrl-C stops
    // working for whatever ran it.
    let mut executor = Executor::detached();
    let command = Command::new(ono_testkit::ono_binary())
        .arg("-c")
        .arg("sleep 30")
        .env("HOME", std::env::temp_dir().display().to_string());
    let mut session = executor
        .run_pty(&command, WindowSize::new(24, 80))
        .map(|session| ono_testkit::Guarded::new(session, ono_process::PtySession::pid))
        .expect("a pseudo-terminal");

    std::thread::sleep(Duration::from_millis(600));
    session
        .signal(Signal::INT)
        .expect("the signal is delivered");

    let status = session
        .wait_timeout(Duration::from_secs(20))
        .expect("waiting works")
        .expect("an interrupted non-interactive run must end");
    assert!(
        !status.is_success(),
        "an interrupted script must not report success, got {status}"
    );
}

#[test]
fn should_interrupt_a_native_pipeline_and_leave_the_prompt_standing() {
    // Spec §18.5: cancellation must propagate through *native* pipelines too. A walk over the
    // whole filesystem takes long enough that the Ctrl-C below arrives mid-stream, and without
    // cancellation the prompt would only return when the walk finished on its own.
    let mut shell = interactive_shell();
    read_until(&mut shell, ">", Duration::from_secs(10));

    shell
        .write_all(b"find file / | count\n")
        .expect("the terminal accepts a command");
    std::thread::sleep(Duration::from_millis(600));
    shell
        .write_all(&[0x03])
        .expect("the terminal accepts Ctrl-C");

    shell
        .write_all(b"echo alive-$?\n")
        .expect("the terminal accepts the follow-up");
    let seen = read_until(&mut shell, "alive-130", Duration::from_secs(8));
    assert!(
        seen.contains("alive-130"),
        "the native pipeline was cancelled with 128+SIGINT and the shell kept going; saw:\n{seen}"
    );

    shell.write_all(b"exit\n").expect("input");
    let _ = shell.wait();
}

#[test]
fn should_make_an_elevated_prompt_impossible_to_miss() {
    // Spec §17.2. Mode bits and prompts cannot be faked from outside: the marker is derived
    // from the kernel's effective uid, so the assertion has two sides — an unprivileged run
    // must NOT show the elevated marker, and a privileged run must.
    let mut shell = interactive_shell();
    let seen = read_until(&mut shell, ">", Duration::from_secs(10))
        + &read_until(&mut shell, "\u{200b}", Duration::from_millis(300));

    if ono_process::effective_uid() == 0 {
        assert!(
            seen.contains("root") && seen.contains('#'),
            "a root shell announces itself in the prompt (spec §17.2); saw:\n{seen}"
        );
    } else {
        assert!(
            !seen.contains(" root://"),
            "an unprivileged shell must not claim elevation; saw:\n{seen}"
        );
    }

    shell.write_all(b"exit\n").expect("input");
    let _ = shell.wait();
}

/// A `uname` that notes its process id and then blocks, on a `PATH` of its own: what
/// `link host … --transport local --agentless` asks first, run as a recorded child of the shell.
#[cfg(feature = "remote")]
fn blocking_uname(home: &ono_testkit::Scratch) -> (std::path::PathBuf, String) {
    let bin = home.path().join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    let noted = home.path().join("uname.pids");
    support::executable(
        &bin.join("uname"),
        &format!(
            "#!/bin/sh\necho $$ >> '{}'\nexec sleep 600\n",
            noted.display()
        ),
    );
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    (noted, path)
}

/// `ono` on a pseudo-terminal, in `home`, with `path`.
#[cfg(feature = "remote")]
fn interactive_shell_in(
    home: &ono_testkit::Scratch,
    path: &str,
) -> ono_testkit::Guarded<ono_process::PtySession> {
    let mut executor = Executor::detached();
    let at = |name: &str| home.path().join(name).display().to_string();
    let command = Command::new(ono_testkit::ono_binary())
        .env("TERM", "xterm")
        .env("NO_COLOR", "1")
        .env("PATH", path)
        .env("HOME", home.path().display().to_string())
        .env("XDG_CONFIG_HOME", at("xdg"))
        .env("XDG_STATE_HOME", at("state"))
        .env("XDG_DATA_HOME", at("data"))
        .env("XDG_CACHE_HOME", at("cache"))
        .env("ONO_CONFIG_DIR", at("ono"));
    executor
        .run_pty(&command, WindowSize::new(24, 80))
        .map(|session| ono_testkit::Guarded::new(session, ono_process::PtySession::pid))
        .expect("a pseudo-terminal must be available")
}

/// The process ids `noted` holds once it holds `count` of them, within the budget.
#[cfg(feature = "remote")]
fn noted_pids(noted: &std::path::Path, count: usize) -> Vec<i32> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let pids: Vec<i32> = std::fs::read_to_string(noted)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.trim().parse().ok())
            .collect();
        if pids.len() >= count || Instant::now() >= deadline {
            return pids;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(feature = "remote")]
fn runs(pid: i32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|stat| {
            stat.rsplit_once(')')
                .and_then(|(_, rest)| rest.split_whitespace().next().map(str::to_owned))
        })
        .is_some_and(|state| state != "Z" && state != "X")
}

/// Kills what the stubs left, whatever the test concluded.
#[cfg(feature = "remote")]
struct Reaper(Vec<i32>);

#[cfg(feature = "remote")]
impl Drop for Reaper {
    fn drop(&mut self) {
        for pid in &self.0 {
            let _ = std::process::Command::new("kill")
                .args(["-KILL", &pid.to_string()])
                .status();
        }
    }
}

#[cfg(feature = "remote")]
#[test]
fn should_interrupt_a_program_a_foreground_command_started_itself() {
    // Review W1 of issue #303: a command's own child — here the far side of an agentless link,
    // which a host that drops packets keeps waiting — runs in a process group of its own so a
    // shell ending on a signal can end what it started, and so the terminal's Ctrl-C no longer
    // reached it. The shell hands the interrupt on to a foreground command's recorded children.
    let home = ono_testkit::scratch();
    let (noted, path) = blocking_uname(&home);
    let mut shell = interactive_shell_in(&home, &path);
    read_until(&mut shell, ">", Duration::from_secs(10));

    shell
        .write_all(b"link host testbox --transport local --agentless\n")
        .expect("input");
    let pids = noted_pids(&noted, 1);
    let _reaper = Reaper(pids.clone());
    assert_eq!(pids.len(), 1, "the far side ran");
    shell.write_all(&[0x03]).expect("Ctrl-C");
    let deadline = Instant::now() + Duration::from_secs(5);
    while runs(pids[0]) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let interrupted = !runs(pids[0]);
    shell.write_all(b"echo alive-$?\n").expect("input");
    let seen = read_until(&mut shell, "alive-", Duration::from_secs(20));
    shell.write_all(b"exit 0\n").expect("input");
    let _ = shell.wait_timeout(Duration::from_secs(20));

    assert!(interrupted, "Ctrl-C reached the far side; saw:\n{seen}");
    assert!(
        seen.contains("alive-"),
        "the prompt came back; saw:\n{seen}"
    );
}

#[cfg(feature = "remote")]
#[test]
fn should_leave_a_background_jobs_own_program_running_on_a_foreground_ctrl_c() {
    // The other half of review W1: a job's recorded child is not the foreground's, and a Ctrl-C
    // meant for the foreground command leaves it running. The foreground command is one whose
    // own child the shell waits for, so the shell keeps the terminal and receives the Ctrl-C.
    let home = ono_testkit::scratch();
    let (noted, path) = blocking_uname(&home);
    let mut shell = interactive_shell_in(&home, &path);
    read_until(&mut shell, ">", Duration::from_secs(10));

    shell
        .write_all(b"fn far() { link host testbox --transport local --agentless }\nfar &\n")
        .expect("input");
    let background = noted_pids(&noted, 1);
    let _background = Reaper(background.clone());
    assert_eq!(background.len(), 1, "the job's far side ran");
    shell
        .write_all(b"link host nearbox --transport local --agentless\n")
        .expect("input");
    let both = noted_pids(&noted, 2);
    let _foreground = Reaper(both.clone());
    assert_eq!(both.len(), 2, "the foreground far side ran");
    shell.write_all(&[0x03]).expect("Ctrl-C");
    let deadline = Instant::now() + Duration::from_secs(5);
    while runs(both[1]) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let foreground_interrupted = !runs(both[1]);
    // Past the moment the interrupt reached the foreground: the job's far side would have died
    // of the same signal by now.
    std::thread::sleep(Duration::from_millis(300));
    let left_running = runs(background[0]);
    shell.write_all(b"exit 0\n").expect("input");
    let _ = shell.wait_timeout(Duration::from_secs(20));

    assert!(
        foreground_interrupted,
        "Ctrl-C reached the foreground far side"
    );
    assert!(
        left_running,
        "the background job's far side was not interrupted"
    );
}
