//! Completion at the real prompt (spec §15.1, §34; ADR-0252).
//!
//! `ono_command::complete` answers from the contracts; a *value* — the users on this machine, the
//! services of this host — can only come from a provider, and `ValueCompleter` is the seam the
//! contracts left for it. These tests drive a real pseudo-terminal, because completion only
//! happens at one, and assert what appears on the screen.

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
fn should_offer_this_machines_users_when_completing_a_user_selector() {
    // Spec §15.1: completion is provider-aware. `ValueCompleter` has been the seam for it since
    // phase D — "the users on this machine, the services of this host" — and the shell installed
    // nothing in it, so `get user <TAB>` could only ever answer from registry metadata, which
    // knows no users. `root` exists on every Linux host, so the fixture is the machine (ADR-0252).
    let directory = scratch();
    let mut shell = interactive_shell_in(&directory);
    let _ = read_until(&mut shell, "> ", Duration::from_secs(10));

    shell.write_all(b"get user ro\t").expect("input");
    let seen = read_until(&mut shell, "get user root", Duration::from_secs(10));
    assert!(
        seen.contains("get user root"),
        "spec §15.1: `get user ro<TAB>` completes from the accounts this machine has; saw:\n{seen}"
    );

    shell.write_all(b"\x03").expect("abandon the line");
    shell.write_all(b"exit\n").expect("input");
    let _ = shell.wait();
}

#[test]
fn should_answer_a_completion_that_no_provider_can_serve_without_waiting_for_one() {
    // The budget of spec §34 is a promise about the *keystroke*, not about the provider: a
    // target nothing can answer must come back as fast as one that is answered instantly, or a
    // container runtime that is not running would be felt on every Tab. Ten completions of a
    // target with no provider on this host, well inside ten times the 50 ms budget.
    let directory = scratch();
    let mut shell = interactive_shell_in(&directory);
    let _ = read_until(&mut shell, "> ", Duration::from_secs(10));

    let started = Instant::now();
    for _ in 0..10 {
        shell.write_all(b"get container zz\t").expect("input");
        let _ = read_until(&mut shell, "\u{200b}", Duration::from_millis(120));
        shell.write_all(b"\x03").expect("abandon the line");
    }
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(5),
        "ten completions took {elapsed:?}; a provider that cannot answer must not be waited for \
         (spec §34, ADR-0252)"
    );

    shell.write_all(b"exit\n").expect("input");
    let _ = shell.wait();
}
