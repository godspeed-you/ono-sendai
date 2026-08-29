//! The prompt as the HUD of spec §4.2: what it says about where the next command will run.
//!
//! Every test here drives a real pseudo-terminal, because the prompt is only drawn at one — a
//! piped run prints none at all (spec §29.1) — and asserts on what appears on the screen, never
//! on how a segment is assembled (AGENTS.md §11).

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::time::{Duration, Instant};

use ono_process::{Command, Executor, PtySession, WindowSize};
use ono_testkit::{Scratch, scratch};

/// Starts `ono` interactively on a pseudo-terminal, in `directory`.
fn interactive_shell_in(directory: &Scratch) -> PtySession {
    let mut executor = Executor::detached();
    let command = Command::new(ono_testkit::ono_binary())
        .env("TERM", "xterm")
        .env("NO_COLOR", "1")
        .env("HOME", directory.path().display().to_string())
        .current_dir(directory.path());
    executor
        .run_pty(&command, WindowSize::new(24, 100))
        .expect("a pseudo-terminal must be available")
}

/// Reads from the terminal until `needle` appears or `budget` runs out.
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
fn should_name_the_branch_in_the_prompt_when_the_working_directory_is_a_checkout() {
    // Spec §4.2's optional `vcs` segment, `git:main`. The branch is read from the checkout's own
    // `HEAD`, so the fixture is a checkout as far as the prompt is concerned and no `git` binary
    // has to exist for the test to mean something.
    let directory = scratch();
    directory.write(".git/HEAD", "ref: refs/heads/topic-branch\n");

    let mut shell = interactive_shell_in(&directory);
    let seen = read_until(&mut shell, "git:", Duration::from_secs(10));
    assert!(
        seen.contains("git:topic-branch"),
        "spec §4.2: the prompt carries the source-control segment inside a checkout; saw:\n{seen}"
    );

    shell.write_all(b"exit\n").expect("input");
    let _ = shell.wait();
}

#[test]
fn should_leave_the_branch_out_of_the_prompt_when_there_is_no_checkout() {
    // "Information that is not actionable SHOULD not be shown permanently" (spec §4.2): outside
    // a checkout there is no branch, and a segment saying so would be noise on every line.
    let directory = scratch();

    let mut shell = interactive_shell_in(&directory);
    let seen = read_until(&mut shell, "> ", Duration::from_secs(10))
        + &read_until(&mut shell, "\u{200b}", Duration::from_millis(300));
    assert!(
        !seen.contains("git:"),
        "a directory that belongs to no checkout has no source-control state to show; saw:\n{seen}"
    );

    shell.write_all(b"exit\n").expect("input");
    let _ = shell.wait();
}

#[test]
fn should_name_the_entered_object_in_the_prompt_instead_of_the_directory() {
    // Spec §14.3: inside an object context the prompt names the object — `local://process/1` —
    // "because a frame that changes what commands act on must be impossible to miss" (ADR-0023).
    // Every case that read a prompt so far used `cd` or a link, so this was implemented and
    // asserted by nothing. PID 1 exists on every host, so the fixture is the machine itself.
    let directory = scratch();
    let mut shell = interactive_shell_in(&directory);
    let _ = read_until(&mut shell, "> ", Duration::from_secs(10));

    shell.write_all(b"enter process 1\n").expect("input");
    let seen = read_until(&mut shell, "process/1", Duration::from_secs(10));
    assert!(
        seen.contains("://process/1"),
        "spec §14.3: the prompt names the entered object as `<link>://<target>/<identity>`; \
         saw:\n{seen}"
    );

    shell.write_all(b"leave\n").expect("input");
    let after = read_until(&mut shell, "://~", Duration::from_secs(10));
    assert!(
        after.contains("://~"),
        "leaving the frame gives the working directory back to the prompt; saw:\n{after}"
    );

    shell.write_all(b"exit\n").expect("input");
    let _ = shell.wait();
}
