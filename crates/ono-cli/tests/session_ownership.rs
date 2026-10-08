//! A session ends what it owns, and only that (review S4 of issue #303).
//!
//! Two sessions in one process — the shell's tests do it, an embedding host could — each own
//! the jobs they started. Dropping one ends its jobs and leaves the other's running.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::time::{Duration, Instant};

use ono_cli::report::Reporter;
use ono_cli::session::Session;
use ono_render::Presentation;

fn running(needle: &str) -> bool {
    !support::processes_naming(needle).is_empty()
}

#[test]
fn should_leave_another_sessions_job_running_when_one_session_ends() {
    let home = ono_testkit::scratch();
    let path = home.path().join("owned-by-a.sh");
    let started = home.path().join("owned-by-a.started");
    support::executable(
        &path,
        &format!(
            "#!/bin/sh\nexec </dev/null >/dev/null 2>&1\n: > '{}'\nwhile :; do sleep 1; done\n",
            started.display()
        ),
    );
    let needle = path.display().to_string();
    let reporter = Reporter::new(Presentation::Plain);

    let mut a = Session::new(false);
    let status = ono_cli::repl::run_source(&mut a, &format!("{needle} &"), &reporter);
    assert!(status.is_success(), "session A started its job");
    let deadline = Instant::now() + Duration::from_secs(30);
    while !started.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }

    let mut b = Session::new(false);
    let status = ono_cli::repl::run_source(&mut b, "true", &reporter);
    assert!(status.is_success(), "session B ran");
    drop(b);
    let still = running(&needle);

    drop(a);
    let deadline = Instant::now() + Duration::from_secs(10);
    while running(&needle) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let gone = !running(&needle);
    support::kill_processes_naming(&needle);

    assert!(still, "ending session B left session A's job running");
    assert!(gone, "ending session A ended its own job");
}
