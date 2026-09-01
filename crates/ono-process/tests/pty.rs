//! PTY execution of spec §29.3: a real controlling terminal, window size and two-way bytes.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failed precondition in a test should abort the test loudly"
)]
mod support;

use std::fs::{self, File, OpenOptions};
use std::time::{Duration, Instant};

use ono_process::{Executor, Output, PtySession, WindowSize};
use support::{DEADLINE, sh, text, within};

/// Reads from the pty until `needle` appears or the deadline passes.
fn read_until(session: &mut PtySession, needle: &str, timeout: Duration) -> String {
    let deadline = Instant::now() + timeout;
    let mut seen = String::new();
    let mut buf = [0u8; 4096];
    while !seen.contains(needle) {
        assert!(
            Instant::now() < deadline,
            "never saw {needle:?} on the pty; saw {seen:?}"
        );
        match session
            .read_timeout(&mut buf, Duration::from_millis(100))
            .expect("reading the pty must not fail")
        {
            None => continue,
            Some(0) => panic!("the pty closed before {needle:?} appeared; saw {seen:?}"),
            Some(read) => seen.push_str(&String::from_utf8_lossy(&buf[..read])),
        }
    }
    seen
}

/// Drains the pty until end of file, then returns everything the child wrote.
fn drain(session: &mut PtySession, timeout: Duration) -> String {
    let deadline = Instant::now() + timeout;
    let mut seen = String::new();
    let mut buf = [0u8; 4096];
    loop {
        assert!(
            Instant::now() < deadline,
            "the pty never closed; saw {seen:?}"
        );
        match session
            .read_timeout(&mut buf, Duration::from_millis(100))
            .expect("reading the pty must not fail")
        {
            None => continue,
            Some(0) => return seen,
            Some(read) => seen.push_str(&String::from_utf8_lossy(&buf[..read])),
        }
    }
}

#[test]
fn should_give_the_child_a_terminal_when_it_runs_under_a_pty() {
    let status = within(DEADLINE, || {
        let mut executor = Executor::detached();
        let mut session = executor
            .run_pty(
                &sh("test -t 0 && test -t 1 && test -t 2"),
                WindowSize::new(24, 80),
            )
            .expect("a pty must be allocatable");
        drain(&mut session, DEADLINE);
        session.wait().expect("the child must be waitable")
    });
    assert_eq!(
        status.code(),
        0,
        "the child saw a terminal on all three streams"
    );
}

#[test]
fn should_not_pretend_there_is_a_terminal_when_the_child_runs_on_pipes() {
    let outcome = within(DEADLINE, || {
        let mut executor = Executor::detached();
        executor.run_foreground(&sh("test -t 1").stdout(Output::Capture).into())
    })
    .expect("the command must run");
    assert_eq!(
        outcome.status().code(),
        1,
        "a captured stream is not a terminal"
    );
}

#[test]
fn should_report_the_requested_window_size_to_the_child() {
    let seen = within(DEADLINE, || {
        let mut executor = Executor::detached();
        let mut session = executor
            .run_pty(&sh("stty size"), WindowSize::new(40, 100))
            .expect("a pty must be allocatable");
        let seen = drain(&mut session, DEADLINE);
        session.wait().expect("the child must be waitable");
        seen
    });
    assert!(
        seen.contains("40 100"),
        "the child must see the window size we asked for, saw {seen:?}"
    );
}

#[test]
fn should_report_the_window_size_of_the_session_itself() {
    within(DEADLINE, || {
        let mut executor = Executor::detached();
        let mut session = executor
            .run_pty(&sh("read ignored"), WindowSize::new(33, 111))
            .expect("a pty must be allocatable");
        assert_eq!(
            session.window_size().expect("the size must be readable"),
            WindowSize::new(33, 111)
        );
        session.write_all(b"go\n").expect("write to the pty");
        session.wait().expect("the child must be waitable");
    });
}

#[test]
fn should_propagate_a_window_size_change_to_the_child() {
    let seen = within(DEADLINE, || {
        let mut executor = Executor::detached();
        let mut session = executor
            .run_pty(
                &sh("stty size; read ignored; stty size"),
                WindowSize::new(24, 80),
            )
            .expect("a pty must be allocatable");
        read_until(&mut session, "24 80", DEADLINE);
        session
            .resize(WindowSize::new(50, 132))
            .expect("resizing must succeed");
        assert_eq!(
            session.window_size().expect("the size must be readable"),
            WindowSize::new(50, 132)
        );
        session.write_all(b"go\n").expect("write to the pty");
        let seen = drain(&mut session, DEADLINE);
        session.wait().expect("the child must be waitable");
        seen
    });
    assert!(
        seen.contains("50 132"),
        "the child must observe the new window size, saw {seen:?}"
    );
}

#[test]
fn should_deliver_sigwinch_to_the_child_when_the_window_changes() {
    let seen = within(DEADLINE, || {
        let mut executor = Executor::detached();
        let mut session = executor
            .run_pty(
                &sh("trap 'echo WINCHED' 28; echo READY; sleep 1; echo DONE"),
                WindowSize::new(24, 80),
            )
            .expect("a pty must be allocatable");
        read_until(&mut session, "READY", DEADLINE);
        session
            .resize(WindowSize::new(30, 90))
            .expect("resizing must succeed");
        let seen = drain(&mut session, DEADLINE);
        session.wait().expect("the child must be waitable");
        seen
    });
    assert!(
        seen.contains("WINCHED"),
        "the child's SIGWINCH trap must fire, saw {seen:?}"
    );
}

#[test]
fn should_carry_bytes_in_both_directions_over_the_pty() {
    let seen = within(DEADLINE, || {
        let mut executor = Executor::detached();
        let mut session = executor
            .run_pty(&sh("read x; echo got:$x"), WindowSize::new(24, 80))
            .expect("a pty must be allocatable");
        session.write_all(b"hello\n").expect("write to the pty");
        let seen = read_until(&mut session, "got:hello", DEADLINE);
        session.wait().expect("the child must be waitable");
        seen
    });
    assert!(seen.contains("got:hello"), "saw {seen:?}");
}

#[test]
fn should_pass_the_child_status_through_from_a_pty_session() {
    let status = within(DEADLINE, || {
        let mut executor = Executor::detached();
        let mut session = executor
            .run_pty(&sh("exit 7"), WindowSize::new(24, 80))
            .expect("a pty must be allocatable");
        drain(&mut session, DEADLINE);
        session.wait().expect("the child must be waitable")
    });
    assert_eq!(status.code(), 7);
}

#[test]
fn should_report_a_signalled_pty_child_as_128_plus_the_signal() {
    let status = within(DEADLINE, || {
        let mut executor = Executor::detached();
        let mut session = executor
            .run_pty(&sh("kill -TERM $$"), WindowSize::new(24, 80))
            .expect("a pty must be allocatable");
        drain(&mut session, DEADLINE);
        session.wait().expect("the child must be waitable")
    });
    assert_eq!(status.code(), 143);
}

#[test]
fn should_relay_bytes_between_plain_files_and_the_pty_when_neither_end_is_a_terminal() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let input = dir.path().join("in");
    let output = dir.path().join("out");
    fs::write(&input, "relayed\n").expect("seed the input");

    let status = {
        let input = input.clone();
        let output = output.clone();
        within(DEADLINE, move || {
            let mut executor = Executor::detached();
            let mut session = executor
                .run_pty(&sh("read x; echo got:$x"), WindowSize::new(24, 80))
                .expect("a pty must be allocatable");
            let source = File::open(&input).expect("open the input");
            let sink = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&output)
                .expect("open the output");
            session
                .relay(&source, &sink)
                .expect("the relay must finish")
        })
    };

    assert_eq!(status.code(), 0);
    let written = fs::read_to_string(&output).expect("read back");
    assert!(
        written.contains("got:relayed"),
        "the relay must carry the child's output, saw {written:?}"
    );
}

#[test]
fn should_signal_a_pty_child_when_asked() {
    let status = within(DEADLINE, || {
        let mut executor = Executor::detached();
        let mut session = executor
            .run_pty(&sh("echo READY; sleep 30"), WindowSize::new(24, 80))
            .expect("a pty must be allocatable");
        read_until(&mut session, "READY", DEADLINE);
        session
            .signal(ono_process::Signal::TERM)
            .expect("signalling must succeed");
        session.wait().expect("the child must be waitable")
    });
    assert_eq!(
        status.code(),
        143,
        "the child was terminated by the signal we sent"
    );
}

#[test]
fn should_report_that_no_status_is_available_while_a_pty_child_still_runs() {
    within(DEADLINE, || {
        let mut executor = Executor::detached();
        let mut session = executor
            .run_pty(&sh("echo READY; read x"), WindowSize::new(24, 80))
            .expect("a pty must be allocatable");
        read_until(&mut session, "READY", DEADLINE);
        assert_eq!(
            session.try_wait().expect("try_wait must not fail"),
            None,
            "a running child has no status yet"
        );
        session.write_all(b"go\n").expect("write to the pty");
        assert_eq!(session.wait().expect("wait").code(), 0);
        assert_eq!(
            session
                .try_wait()
                .expect("try_wait must not fail")
                .map(|s| s.code()),
            Some(0),
            "the status stays available after the child is reaped"
        );
    });
}

#[test]
fn should_report_the_pid_of_the_pty_child() {
    within(DEADLINE, || {
        let mut executor = Executor::detached();
        let mut session = executor
            .run_pty(&sh("exit 0"), WindowSize::new(24, 80))
            .expect("a pty must be allocatable");
        assert!(session.pid() > 1);
        drain(&mut session, DEADLINE);
        session.wait().expect("the child must be waitable");
    });
}

#[test]
fn should_leave_no_stale_output_when_the_child_writes_a_lot_through_the_pty() {
    let seen = within(DEADLINE, || {
        let mut executor = Executor::detached();
        let mut session = executor
            .run_pty(
                &sh("i=0; while [ $i -lt 200 ]; do echo line$i; i=$((i+1)); done"),
                WindowSize::new(24, 80),
            )
            .expect("a pty must be allocatable");
        let seen = drain(&mut session, DEADLINE);
        session.wait().expect("the child must be waitable");
        seen
    });
    assert!(seen.contains("line0"), "saw {}", text(seen.as_bytes()));
    assert!(seen.contains("line199"), "the pty must not lose the tail");
}
