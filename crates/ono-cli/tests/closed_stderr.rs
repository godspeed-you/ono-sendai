//! A refusal stays a refusal when nobody reads the diagnostics (issue #163).
//!
//! `ono … 2>&1 | head -c0`, a supervisor that closed the log, a console an operator shut: the
//! standard error `ono` writes its diagnostics to may be a pipe nobody reads any more. Writing to
//! it then fails (`EPIPE`), and a diagnostic written with `eprintln!` panics on that failure, so
//! the process exits 101 — a crash, to anything reading the status — where it meant to refuse
//! with its own status (spec §43, ADR-0549). A diagnostic is commentary: losing it costs the line
//! and nothing else.
//!
//! Each case runs the real binary twice: once with standard error read, which gives the status
//! the refusal has, and once with standard error a pipe whose read end is already closed. The
//! two statuses must be the same, and neither may be the panic status 101.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::io::Read as _;
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use ono_testkit::{Scratch, scratch};

/// The status Rust gives a process that panicked.
const PANICKED: i32 = 101;

/// One invocation of `ono`, in a scratch home so no user state is read or written.
struct Case {
    what: &'static str,
    args: Vec<String>,
    env: Vec<(&'static str, String)>,
}

impl Case {
    fn new(what: &'static str, args: &[&str]) -> Self {
        Self {
            what,
            args: args.iter().map(|argument| (*argument).to_owned()).collect(),
            env: Vec::new(),
        }
    }

    fn env(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.env.push((name, value.into()));
        self
    }

    fn command(&self, home: &Scratch) -> Command {
        let mut command = Command::new(ono_testkit::ono_binary());
        command
            .args(&self.args)
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path().join("config"))
            .env("XDG_DATA_HOME", home.path().join("data"))
            .env("XDG_CACHE_HOME", home.path().join("cache"))
            .env("XDG_STATE_HOME", home.path().join("state"))
            .env("ONO_PLUGIN_PATH", home.path().join("plugins"))
            .env("NO_COLOR", "1")
            .current_dir(home.path())
            .stdin(Stdio::null())
            .stdout(Stdio::piped());
        for (name, value) in &self.env {
            command.env(name, value);
        }
        command
    }
}

/// Waits for `child` within a budget, killing it on overrun so a hang fails instead of stalling.
fn finish(mut child: std::process::Child, what: &str) -> (ExitStatus, String) {
    let mut stdout = child.stdout.take().expect("standard output was piped");
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        text
    });
    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(status) = child.try_wait().expect("the child can be waited for") {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{what}: `ono` did not finish within its budget");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    (status, reader.join().unwrap_or_default())
}

/// The status of `case` with its diagnostics read, and its diagnostics.
fn with_stderr_read(case: &Case, home: &Scratch) -> (ExitStatus, String) {
    let mut child = case
        .command(home)
        .stderr(Stdio::piped())
        .spawn()
        .expect("ono starts");
    let mut stderr = child.stderr.take().expect("standard error was piped");
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });
    let (status, _) = finish(child, case.what);
    (status, reader.join().unwrap_or_default())
}

/// The status of `case` with standard error a pipe whose read end is already closed, and what it
/// wrote to standard output.
fn with_stderr_closed(case: &Case, home: &Scratch) -> (ExitStatus, String) {
    let (reader, writer) = std::io::pipe().expect("a pipe");
    drop(reader);
    let child = case
        .command(home)
        .stderr(Stdio::from(writer))
        .spawn()
        .expect("ono starts");
    finish(child, case.what)
}

#[test]
fn should_exit_with_the_refusals_status_when_nobody_reads_its_diagnostics() {
    let home = scratch();
    let script = home.path().join("failing.ono");
    std::fs::write(&script, "echo started\nno-such-command-for-this-test\n")
        .expect("the script is written");
    let cases = [
        Case::new("a usage error", &["--bogus"]),
        Case::new(
            "an unknown command",
            &["-c", "no-such-command-for-this-test"],
        ),
        Case::new(
            "a failing pipeline",
            &["-c", "let one = [1]; $one | each (5 % 0) | to json"],
        ),
        Case::new(
            "a type error before execution",
            &["-c", "get process | where cpy > 1"],
        ),
        // A configuration directory that cannot hold a peer identity: `/dev/null` is no directory.
        Case::new("a `--print-peer-key` that fails", &["--print-peer-key"])
            .env("XDG_CONFIG_HOME", "/dev/null/config"),
        Case::new(
            "a script that fails",
            &[script.to_str().expect("a UTF-8 path")],
        ),
    ];
    for case in &cases {
        let (refused, said) = with_stderr_read(case, &home);
        assert!(
            !refused.success() && refused.code() != Some(PANICKED) && !said.contains("panicked"),
            "{}: the case is a refusal with a status of its own and a diagnostic, got {refused:?} \
             and {said:?}",
            case.what
        );
        let (unread, stdout) = with_stderr_closed(case, &home);
        assert_eq!(
            unread.code(),
            refused.code(),
            "{}: with nobody reading standard error `ono` still refuses with the same status — \
             not 101, a crash (issue #163, ADR-0549); stdout {stdout:?}",
            case.what
        );
        assert!(
            !stdout.contains("panicked"),
            "{}: no panic, got {stdout:?}",
            case.what
        );
    }
}
