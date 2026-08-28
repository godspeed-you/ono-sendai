//! An interactive shell must not outlive the terminal it was given (spec §18.1, §29.3, ADR-0160).

#![allow(
    clippy::expect_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::time::{Duration, Instant};

use ono_process::{Command, Executor, PtySession, WindowSize};

fn interactive_shell() -> (Executor, PtySession) {
    let mut executor = Executor::detached();
    let command = Command::new(ono_testkit::ono_binary())
        .env("TERM", "xterm")
        .env("NO_COLOR", "1")
        .env("HOME", std::env::temp_dir().display().to_string());
    let session = executor
        .run_pty(&command, WindowSize::new(30, 100))
        .expect("a pseudo-terminal");
    (executor, session)
}

/// Waits until the shell has printed its prompt, so the test drops a terminal that is in use.
fn wait_for_prompt(shell: &mut PtySession) -> bool {
    let mut buffer = [0u8; 8192];
    let mut seen = String::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Ok(Some(count)) = shell.read_timeout(&mut buffer, Duration::from_millis(150)) {
            seen.push_str(&String::from_utf8_lossy(&buffer[..count]));
        }
        if seen.contains('>') {
            return true;
        }
    }
    false
}

/// Whether the process is still running — a process that has exited but not been reaped is a
/// zombie, which no longer holds a terminal, a D-Bus connection or any other resource.
fn still_running(pid: u32) -> bool {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    // The state letter is the first field after the parenthesised command name, which may
    // itself contain spaces and parentheses.
    let Some(rest) = stat.rsplit_once(") ") else {
        return false;
    };
    !rest.1.starts_with('Z') && !rest.1.starts_with('X')
}

fn gone_within(pid: u32, budget: Duration) -> bool {
    let deadline = Instant::now() + budget;
    while Instant::now() < deadline {
        if !still_running(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    !still_running(pid)
}

/// Leaves nothing behind whatever the assertion finds; the pid cannot be reused while the
/// process is this one's unreaped child.
fn make_sure_it_is_gone(pid: u32) {
    let _ = std::process::Command::new("kill")
        .arg("-KILL")
        .arg(pid.to_string())
        .status();
}

#[test]
fn should_exit_when_the_terminal_it_was_given_goes_away() {
    let (_executor, mut shell) = interactive_shell();
    let pid = shell.pid();
    assert!(wait_for_prompt(&mut shell), "the shell reaches its prompt");

    // The far end of the pseudoterminal goes, exactly as it does when the process that started
    // the shell exits: the shell's input can never produce another byte.
    drop(shell);

    let exited = gone_within(pid, Duration::from_secs(10));
    make_sure_it_is_gone(pid);
    assert!(
        exited,
        "an interactive shell whose terminal has gone must exit rather than wait forever \
         (spec §18.1, §29.3, ADR-0160); pid {pid} was still running"
    );
}

#[test]
fn should_not_hold_the_terminal_that_drives_it() {
    let (_executor, mut shell) = interactive_shell();
    let pid = shell.pid();
    assert!(wait_for_prompt(&mut shell), "the shell reaches its prompt");

    // A shell that holds the master side of its own pseudoterminal keeps its own input alive
    // and can never see end of file. Nothing under a shell's control may point at /dev/ptmx.
    let mut held = Vec::new();
    if let Ok(entries) = std::fs::read_dir(format!("/proc/{pid}/fd")) {
        for entry in entries.flatten() {
            if let Ok(target) = std::fs::read_link(entry.path())
                && target.to_string_lossy().contains("ptmx")
            {
                held.push(format!(
                    "{} -> {}",
                    entry.file_name().display(),
                    target.display()
                ));
            }
        }
    }

    drop(shell);
    make_sure_it_is_gone(pid);
    assert!(
        held.is_empty(),
        "the shell inherited the master side of a pseudoterminal, so its own input can never \
         end (ADR-0160): {held:?}"
    );
}
