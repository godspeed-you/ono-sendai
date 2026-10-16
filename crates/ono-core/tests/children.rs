//! A shell that has begun to end on a signal ends a child recorded after that at once (review Z7
//! of issue #303, ADR-0959). The rule is process-wide and cannot be undone, so it lives in a test
//! binary of its own.

#![allow(
    clippy::expect_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::os::unix::process::CommandExt as _;
use std::time::{Duration, Instant};

use ono_core::children::{Recorded, close};

/// What the ending shell does to a late child: kill it, by its group where it leads one.
fn kill(target: i32) {
    let _ = std::process::Command::new("kill")
        .args(["-KILL", "--", &target.to_string()])
        .status();
}

/// Whether `child` ends within the deadline.
fn ends(child: &mut std::process::Child) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if child
            .try_wait()
            .expect("the child can be waited for")
            .is_some()
        {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

fn sleeper() -> std::process::Child {
    std::process::Command::new("sleep")
        .arg("600")
        .process_group(0)
        .spawn()
        .expect("sleep starts")
}

#[test]
fn should_end_a_child_recorded_once_the_shell_has_begun_to_end_and_only_then() {
    let mut before = sleeper();
    let early = Recorded::group(before.id());
    std::thread::sleep(Duration::from_millis(100));
    let ran = before
        .try_wait()
        .expect("the child can be waited for")
        .is_none();
    let _ = before.kill();
    let _ = before.wait();
    drop(early);

    close(kill);
    let mut late = sleeper();
    let recorded = Recorded::group(late.id());
    let ended = ends(&mut late);
    drop(recorded);
    if !ended {
        let _ = late.kill();
        let _ = late.wait();
    }

    assert!(
        ran,
        "a child recorded before the shell began to end runs on"
    );
    assert!(
        ended,
        "a child recorded once the shell is ending is ended at once"
    );
}
