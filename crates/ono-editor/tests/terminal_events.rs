//! What a full-screen view reads from the terminal: every key, whatever else the terminal said
//! at the same moment.
//!
//! A `cargo test` process usually has no controlling terminal, and it must never read the
//! developer's terminal if it has one. So the test re-runs this very test binary under a pty of
//! its own (the precedent is `ono-process/tests/terminal_control.rs`); the re-run reads the
//! terminal the way the map and the timeline do and prints what it was told, and the outer test
//! asserts on that.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failed precondition in a test should abort the test loudly"
)]

use std::env;
use std::io::Write;
use std::os::fd::AsFd;
use std::time::{Duration, Instant};

use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use nix::sys::signal::{Signal, raise};
use ono_editor::{KeyCode, KeyPress, RawMode, TerminalEvent, read_event_timeout};
use ono_process::{Command, Executor, PtySession, WindowSize};
use ono_testkit::SkipReason;

/// Environment variable naming the role the re-run should play.
const ROLE: &str = "ONO_EDITOR_TEST_ROLE";

/// How long any one step of the exchange may take before the test says which step it was.
const BUDGET: Duration = Duration::from_secs(30);

/// How long the re-run keeps asking for the key once it knows the key is in the terminal.
///
/// The key is already waiting when the asking starts, so a reader that can see it answers on
/// the first read; five seconds is only how long a reader that cannot is given to prove it.
const ANSWER: Duration = Duration::from_secs(5);

/// Printed by the re-run when it is ready for the parent to type, which is the parent's cue.
const READY: &str = "READY-FOR-KEY";

/// Runs this test binary again under a pty, playing `role`, types `Esc` when it is ready, and
/// returns everything it printed.
fn under_pty(role: &str) -> String {
    let exe = env::current_exe().expect("the test binary must be locatable");
    let command = Command::new(exe)
        .arg("--exact")
        .arg("acts_as_the_terminal_reader_when_a_role_is_requested")
        .arg("--nocapture")
        .arg("--test-threads=1")
        .env(ROLE, role);
    let mut executor = Executor::detached();
    let mut session = executor
        .run_pty(&command, WindowSize::new(24, 80))
        .expect("a pty must be allocatable");

    let seen = read_until(&mut session, READY);
    assert!(
        seen.contains(READY),
        "the {role} re-run must get ready for the key; it printed {seen:?}"
    );
    session.write_all(b"\x1b").expect("write Esc to the pty");
    seen + &read_to_exit(&mut session)
}

#[test]
fn should_answer_a_key_that_reaches_the_terminal_together_with_a_resize() {
    // v0.4 §43.4: a resize preserves the view, and nothing about a resize may cost the user a
    // key. The map pauses on `Space` and closes on `Esc`; a resize that is still unread when
    // `Esc` is typed must not leave `Esc` unanswered, because nothing else the user does will
    // bring it back — they typed it, and the view never saw it.
    let seen = under_pty("resize-then-key");
    assert!(
        seen.contains("ESC-ANSWERED"),
        "a key typed while a resize was still unread must be answered, not stranded in the \
         terminal; the re-run printed {seen:?}"
    );
    assert!(
        seen.contains("RESIZED="),
        "the resize itself is reported too — the fix must not trade one event for the other; \
         the re-run printed {seen:?}"
    );
}

#[test]
fn should_hand_over_a_key_already_waiting_when_asked_without_patience() {
    // ADR-0424: while a slow provider works, the map asks the terminal every 16 ms for whatever
    // key is *already* there, with no patience at all, so that `Esc` still closes a busy view.
    // A zero wait is "do not wait", never "do not look".
    let seen = under_pty("key-without-patience");
    assert!(
        seen.contains("ESC-ANSWERED"),
        "a key already in the terminal must be handed over by a read that does not wait; the \
         re-run printed {seen:?}"
    );
}

/// The re-run entry point. Without [`ROLE`] in the environment this test does nothing.
#[test]
fn acts_as_the_terminal_reader_when_a_role_is_requested() {
    let Ok(role) = env::var(ROLE) else {
        ono_testkit::skipped(
            SkipReason::FixtureNotApplicable,
            "this is the re-exec entry point the tests above drive; without a role in the \
             environment there is nothing for it to act as",
        );
        return;
    };
    let code = match role.as_str() {
        "resize-then-key" => resize_then_key(),
        "key-without-patience" => key_without_patience(),
        other => {
            println!("UNKNOWN-ROLE={other}");
            1
        }
    };
    std::process::exit(code);
}

/// Leaves a resize unread, has the parent type `Esc`, and reports what the reader then returns.
///
/// The order is the whole point, and it is made exact rather than likely. `raise` delivers the
/// resize signal to this thread before it returns, so the terminal's resize notice is pending
/// before the parent is told to type; and the terminal is asked — without being read — whether
/// the key has arrived, so both are waiting, the resize first, when the reader is next asked.
/// That is the moment a loaded machine produces by chance: a resize the view has not collected
/// yet, and a key typed after it.
fn resize_then_key() -> i32 {
    let _raw = RawMode::enter().expect("the pty must accept raw mode");
    // The first read is what starts listening for resizes at all; nothing has been typed yet.
    let _ = read_event_timeout(Duration::from_millis(10));

    raise(Signal::SIGWINCH).expect("the resize signal must be deliverable");
    if !key_typed() {
        return 1;
    }

    // Both are read back, in whichever order the reader reports them: the key is what this is
    // about, and the resize is still owed to the view, which redraws on it (§43.4).
    let esc = KeyPress::key(KeyCode::Esc);
    let (mut answered, mut resized) = (false, false);
    let deadline = Instant::now() + ANSWER;
    while !(answered && resized) && Instant::now() < deadline {
        match read_event_timeout(Duration::from_millis(250)) {
            Ok(Some(TerminalEvent::Key(key))) if key == esc => {
                answered = true;
                say("ESC-ANSWERED");
            }
            Ok(Some(TerminalEvent::Resize(columns, rows))) => {
                resized = true;
                say(&format!("RESIZED={columns}x{rows}"));
            }
            Ok(Some(other)) => say(&format!("OTHER={other:?}")),
            Ok(None) => {}
            Err(error) => {
                say(&format!("READ-FAILED={error}"));
                return 1;
            }
        }
    }
    if !answered {
        say("ESC-STRANDED");
    }
    0
}

/// Has the parent type `Esc`, waits until it is in the terminal, and asks for it once, with no
/// patience at all — the way the map asks while it is busy (ADR-0424).
fn key_without_patience() -> i32 {
    let _raw = RawMode::enter().expect("the pty must accept raw mode");
    let _ = read_event_timeout(Duration::from_millis(10));
    if !key_typed() {
        return 1;
    }
    match read_event_timeout(Duration::ZERO) {
        Ok(Some(TerminalEvent::Key(key))) if key == KeyPress::key(KeyCode::Esc) => {
            say("ESC-ANSWERED");
        }
        other => say(&format!("ZERO-WAIT-SAW={other:?}")),
    }
    0
}

/// Cues the parent to type and waits until the key is in the terminal, without reading it.
fn key_typed() -> bool {
    say(READY);
    let stdin = std::io::stdin();
    let mut waiting = [PollFd::new(stdin.as_fd(), PollFlags::POLLIN)];
    let typed = poll(&mut waiting, PollTimeout::from(10_000_u16)).unwrap_or(0) > 0;
    if !typed {
        say("KEY-NEVER-ARRIVED");
    }
    typed
}

/// Prints one marker on a line of its own. The terminal is raw, so the line is ended by hand.
fn say(marker: &str) {
    let mut out = std::io::stdout();
    let _ = write!(out, "{marker}\r\n");
    let _ = out.flush();
}

/// Reads what the re-run prints until `needle` appears or [`BUDGET`] runs out.
fn read_until(session: &mut PtySession, needle: &str) -> String {
    let deadline = Instant::now() + BUDGET;
    let mut seen = String::new();
    let mut buf = [0u8; 4096];
    while !seen.contains(needle) && Instant::now() < deadline {
        match session.read_timeout(&mut buf, Duration::from_millis(100)) {
            Ok(Some(0)) | Err(_) => break,
            Ok(Some(read)) => seen.push_str(&String::from_utf8_lossy(&buf[..read])),
            Ok(None) => {}
        }
    }
    seen
}

/// Reads what the re-run prints until it has exited, or [`BUDGET`] runs out.
fn read_to_exit(session: &mut PtySession) -> String {
    let deadline = Instant::now() + BUDGET;
    let mut seen = String::new();
    let mut buf = [0u8; 4096];
    loop {
        assert!(
            Instant::now() < deadline,
            "the re-run never finished; it printed {seen:?}"
        );
        match session.read_timeout(&mut buf, Duration::from_millis(100)) {
            Ok(Some(read)) if read > 0 => seen.push_str(&String::from_utf8_lossy(&buf[..read])),
            // Silence, end of file or an error on the master is the end only once the re-run has
            // actually finished; a loaded machine can report it before the re-run was scheduled.
            _ => {
                if session.try_wait().is_ok_and(|status| status.is_some()) {
                    return seen;
                }
            }
        }
    }
}
