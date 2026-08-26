//! Terminal ownership of spec §18.1, proved by running the shell side under a real pty.
//!
//! A `cargo test` process usually has no controlling terminal, and it must never take the
//! developer's terminal hostage if it has one. So the tests here re-run this very test binary
//! under a pty allocated by `ono-process` itself, and the re-run acts as the shell: it opens
//! its controlling terminal, runs a foreground command, and prints what it observed. The outer
//! test then asserts on those observations.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failed precondition in a test should abort the test loudly"
)]
mod support;

use std::env;
use std::time::{Duration, Instant};

use ono_process::{Command, Executor, ForegroundOutcome, JobState, PtySession, Signal, WindowSize};
use support::{DEADLINE, poll_until, within};

/// Environment variable naming the role the re-run should play.
const ROLE: &str = "ONO_PROCESS_TEST_ROLE";

/// Runs this test binary again, under a pty, playing `role`, and returns everything it printed.
fn under_pty(role: &str) -> String {
    let role = role.to_owned();
    within(DEADLINE, move || {
        let exe = env::current_exe().expect("the test binary must be locatable");
        let command = Command::new(exe)
            .arg("--exact")
            .arg("acts_as_the_shell_when_a_role_is_requested")
            .arg("--nocapture")
            .arg("--test-threads=1")
            .env(ROLE, &role);
        let mut executor = Executor::detached();
        let mut session = executor
            .run_pty(&command, WindowSize::new(24, 80))
            .expect("a pty must be allocatable");
        let seen = drain(&mut session, DEADLINE, &role);
        let status = session.wait().expect("the re-run must be waitable");
        assert_eq!(
            status.code(),
            0,
            "the {role} re-run must succeed; it printed {seen:?}"
        );
        seen
    })
}

fn drain(session: &mut PtySession, timeout: Duration, role: &str) -> String {
    let deadline = Instant::now() + timeout;
    let mut seen = String::new();
    let mut buf = [0u8; 4096];
    loop {
        assert!(
            Instant::now() < deadline,
            "the {role} re-run never finished; it printed {seen:?}"
        );
        match session
            .read_timeout(&mut buf, Duration::from_millis(100))
            .expect("reading the pty must not fail")
        {
            None => {
                if seen.contains("NEEDS-INPUT") && !seen.contains("SENT-INPUT") {
                    session.write_all(b"ok\n").expect("write to the pty");
                    seen.push_str("SENT-INPUT");
                }
            }
            Some(0) => return seen,
            Some(read) => seen.push_str(&String::from_utf8_lossy(&buf[..read])),
        }
    }
}

/// Reads the value the re-run printed for `key`.
///
/// The re-run's own test harness prints a line of its own before the first marker, so the key
/// is looked for anywhere in the output rather than only at the start of a line.
fn field<'a>(output: &'a str, key: &str) -> &'a str {
    let start = output
        .find(key)
        .unwrap_or_else(|| panic!("the re-run never printed {key}; it printed {output:?}"))
        + key.len();
    let rest = &output[start..];
    let end = rest.find(['\r', '\n']).unwrap_or(rest.len());
    &rest[..end]
}

#[test]
fn should_hand_the_terminal_to_the_foreground_group_and_take_it_back() {
    let output = under_pty("foreground-ownership");

    let shell = field(&output, "SHELL-PGID=");
    assert_eq!(
        field(&output, "FG-BEFORE="),
        shell,
        "the shell owns its terminal before it runs anything"
    );
    assert_eq!(
        field(&output, "FG-AFTER="),
        shell,
        "the shell takes the terminal back when the command finishes"
    );
    assert_ne!(
        field(&output, "CHILD-PGID="),
        shell,
        "a foreground command runs in a new process group"
    );
    assert!(
        output.contains("CHILD-READ:ok"),
        "the foreground child could read the terminal, so it really owned it: {output:?}"
    );
    assert_eq!(field(&output, "STATUS="), "0");
}

#[test]
fn should_restore_the_terminal_attributes_a_child_changed() {
    let output = under_pty("termios-restore");

    assert_eq!(
        field(&output, "ECHO-BEFORE="),
        "on",
        "the pty starts in cooked mode with echo on"
    );
    assert_eq!(
        field(&output, "ECHO-DURING="),
        "off",
        "the child really did change the terminal"
    );
    assert_eq!(
        field(&output, "ECHO-AFTER="),
        "on",
        "the shell restores the attributes it saved before handing the terminal over"
    );
}

#[test]
fn should_stop_a_background_job_that_reads_the_terminal() {
    let output = under_pty("background-ttin");

    assert_eq!(
        field(&output, "BG-STOPPED="),
        "SIGTTIN",
        "a background job that reads the terminal is stopped, not served"
    );
}

#[test]
fn should_work_the_same_way_when_there_is_no_terminal_at_all() {
    let executor = Executor::detached();
    assert!(
        !executor.terminal().is_interactive(),
        "a detached executor never touches a terminal"
    );

    let outcome = within(DEADLINE, || {
        let mut executor = Executor::detached();
        executor.run_foreground(&Command::new("/bin/sh").arg("-c").arg("exit 0").into())
    })
    .expect("the run must not fail on the non-terminal path");
    assert_eq!(outcome.status().code(), 0);
}

/// The re-run entry point. Without [`ROLE`] in the environment this test does nothing.
#[test]
fn acts_as_the_shell_when_a_role_is_requested() {
    let Ok(role) = env::var(ROLE) else {
        return;
    };
    let code = match role.as_str() {
        "foreground-ownership" => foreground_ownership(),
        "termios-restore" => termios_restore(),
        "background-ttin" => background_ttin(),
        other => {
            println!("UNKNOWN-ROLE={other}");
            1
        }
    };
    std::process::exit(code);
}

fn foreground_ownership() -> i32 {
    let mut executor = Executor::new().expect("the shell must start");
    let terminal = executor.terminal();
    assert!(
        terminal.is_interactive(),
        "the re-run has a pty as its controlling terminal"
    );
    println!("SHELL-PGID={}", terminal.shell_group());
    println!(
        "FG-BEFORE={}",
        terminal
            .foreground_group()
            .expect("the foreground group must be readable")
            .expect("there is a foreground group")
    );
    println!("NEEDS-INPUT");

    let outcome = executor
        .run_foreground(
            &Command::new("/bin/sh")
                .arg("-c")
                .arg("read line; echo CHILD-READ:$line")
                .into(),
        )
        .expect("the foreground run must not fail");

    let outcome = match outcome {
        ForegroundOutcome::Completed(outcome) => outcome,
        ForegroundOutcome::Stopped { signal, .. } => {
            println!("UNEXPECTED-STOP={signal}");
            return 1;
        }
    };
    println!("CHILD-PGID={}", outcome.process_group());
    println!("STATUS={}", outcome.status().code());
    let terminal = executor.terminal();
    println!(
        "FG-AFTER={}",
        terminal
            .foreground_group()
            .expect("the foreground group must be readable")
            .expect("there is a foreground group")
    );
    0
}

fn termios_restore() -> i32 {
    fn echo_state(executor: &Executor) -> &'static str {
        if executor
            .terminal()
            .echo_enabled()
            .expect("the attributes must be readable")
        {
            "on"
        } else {
            "off"
        }
    }

    let mut executor = Executor::new().expect("the shell must start");
    println!("ECHO-BEFORE={}", echo_state(&executor));

    let outcome = executor
        .run_foreground(
            &Command::new("/bin/sh")
                .arg("-c")
                .arg("stty -echo; stty -a | grep -q -- '-echo' && echo ECHO-DURING=off || echo ECHO-DURING=on")
                .into(),
        )
        .expect("the foreground run must not fail");
    if !matches!(outcome, ForegroundOutcome::Completed(_)) {
        println!("UNEXPECTED-STOP");
        return 1;
    }
    println!("ECHO-AFTER={}", echo_state(&executor));
    0
}

fn background_ttin() -> i32 {
    let mut executor = Executor::new().expect("the shell must start");
    let id = executor
        .run_background(&Command::new("/bin/sh").arg("-c").arg("read line").into())
        .expect("the job must start");

    let signal = poll_until(Duration::from_secs(20), || {
        executor.poll_jobs().expect("polling must not fail");
        match executor.job(id).map(|job| job.state) {
            Some(JobState::Stopped(signal)) => Some(signal),
            _ => None,
        }
    });
    println!("BG-STOPPED={}", signal.name().unwrap_or("unknown"));
    let _ = executor.signal_job(id, Signal::KILL);
    let _ = executor.signal_job(id, Signal::CONT);
    let _ = executor.wait_job(id, Some(Duration::from_secs(20)));
    0
}
