//! An answer nobody reads is not a crash (review R11/C6, v0.6.3).
//!
//! `ono --complete … | head -c0`, `ono --version | true`: the standard output an invocation flag
//! answers on may be a pipe whose reader is gone. Writing then fails with `EPIPE`, and `println!`
//! panics on that failure, so the process exits 101 — a crash, to anything reading the status.
//! A pipeline whose consumer left stops and keeps its status (ADR-0220); an invocation flag's one
//! answer does the same: the status is the one the flag has when its output is read.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use ono_testkit::{Scratch, scratch};

/// The status Rust gives a process that panicked.
const PANICKED: i32 = 101;

fn command(home: &Scratch, args: &[&str]) -> Command {
    let mut command = Command::new(ono_testkit::ono_binary());
    command
        .args(args)
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("XDG_DATA_HOME", home.path().join("data"))
        .env("XDG_CACHE_HOME", home.path().join("cache"))
        .env("XDG_STATE_HOME", home.path().join("state"))
        .env("ONO_PLUGIN_PATH", home.path().join("plugins"))
        .env("NO_COLOR", "1")
        .current_dir(home.path())
        .stdin(Stdio::null())
        .stderr(Stdio::piped());
    command
}

fn finish(mut child: std::process::Child, what: &str) -> (ExitStatus, String) {
    let mut stderr = child.stderr.take().expect("standard error was piped");
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut stderr, &mut text);
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

#[test]
fn should_keep_the_flags_status_when_nobody_reads_its_answer() {
    let home = scratch();
    for args in [
        &["--no-config", "--complete", "get pro"][..],
        &["--version"][..],
        &["--help"][..],
    ] {
        let what = args.join(" ");
        let read = command(&home, args)
            .stdout(Stdio::null())
            .status()
            .expect("ono starts");
        let (reader, writer) = std::io::pipe().expect("a pipe");
        drop(reader);
        let child = command(&home, args)
            .stdout(Stdio::from(writer))
            .spawn()
            .expect("ono starts");
        let (unread, said) = finish(child, &what);
        assert_ne!(
            unread.code(),
            Some(PANICKED),
            "`ono {what}` with nobody reading its answer must not crash, said {said:?}"
        );
        assert_eq!(
            unread.code(),
            read.code(),
            "`ono {what}` keeps the status it has when its answer is read, said {said:?}"
        );
        assert!(
            !said.contains("panicked"),
            "`ono {what}`: no panic, got {said:?}"
        );
    }
}
