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

use ono_process::{Command, Executor, PtySession, Signal, WindowSize};

/// Starts `ono` interactively on a pseudo-terminal of a known size.
fn interactive_shell() -> PtySession {
    let mut executor = Executor::detached();
    let command = Command::new(ono_testkit::ono_binary())
        .env("TERM", "xterm")
        .env("NO_COLOR", "1")
        .env("HOME", std::env::temp_dir().display().to_string());
    executor
        .run_pty(&command, WindowSize::new(24, 80))
        .expect("a pseudo-terminal must be available")
}

/// Reads from the terminal until `needle` appears or `budget` runs out.
///
/// A terminal echoes what is typed, and the editor repaints the line on every keystroke, so any
/// needle taken from the command text appears in the stream long before the command has run.
/// Every needle here is therefore something only the *output* can contain — `alive-$?` is typed
/// and `alive-130` comes back.
fn read_until(session: &mut PtySession, needle: &str, budget: Duration) -> String {
    let deadline = Instant::now() + budget;
    let mut seen = String::new();
    let mut buffer = [0u8; 4096];
    while Instant::now() < deadline {
        match session.read_timeout(&mut buffer, Duration::from_millis(200)) {
            Ok(Some(0)) | Err(_) => break,
            Ok(Some(count)) => {
                seen.push_str(&String::from_utf8_lossy(&buffer[..count]));
                if seen.contains(needle) {
                    return seen;
                }
            }
            Ok(None) => {}
        }
    }
    seen
}

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
