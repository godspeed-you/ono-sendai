//! One plan, one applier — and a crashed applier holds nothing (spec v0.6 §41.2, §41.3, §42.3,
//! §42.4, §55.9 cases 40 and 42).
//!
//! §42.4: the plan store prevents two sessions applying one sealed plan at once, and the refusal
//! names the holder. §42.3: a lock is bounded and released on failure, so a session killed in the
//! middle of an action leaves a claim that `resume` can take over at once. Each test holds an
//! apply inside its copy by giving it a pipe to read, the way the acceptance cases do.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

#[path = "change_support/mod.rs"]
mod change_support;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use change_support::{build_disk_home, one, ono_at, text};

/// `ono -c script` in `home`, started and left running.
fn spawn_ono(home: &Path, script: &str) -> Child {
    let root = home.to_string_lossy().into_owned();
    Command::new(env!("CARGO_BIN_EXE_ono"))
        .env("NO_COLOR", "1")
        .env("HOME", &root)
        .env("XDG_CONFIG_HOME", format!("{root}/config"))
        .env("XDG_DATA_HOME", format!("{root}/data"))
        .env("XDG_STATE_HOME", format!("{root}/state"))
        .args(["-c", script])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("ono starts")
}

/// A plan copying `source` over `target`, with `source` then replaced by a pipe nobody writes.
fn plan_blocked_on_a_pipe(home: &Path) -> (String, PathBuf, PathBuf) {
    let source = home.join("source");
    let target = home.join("target");
    std::fs::write(&source, "source\n").expect("the source is written");
    std::fs::write(&target, "b\n").expect("the target is written");
    let planned = ono_at(
        home,
        &format!(
            "plan copy file {} {} --overwrite | to json",
            source.display(),
            target.display()
        ),
    );
    planned.assert_success();
    let plan = text(&one(&planned), "id");
    std::fs::remove_file(&source).expect("the source is removed");
    let made = Command::new("mkfifo")
        .arg(&source)
        .status()
        .expect("mkfifo runs");
    assert!(made.success(), "a pipe stands where the source was");
    (plan, source, target)
}

/// Waits until the plan's durable state is `applying`, which is where the copy blocks.
fn await_applying(home: &Path, plan: &str) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let state = ono_at(home, &format!("get plan {} | to json", &plan[..8]));
        if state.status().is_success() && text(&one(&state), "state") == "applying" {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "the first apply never reached its mutation"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn should_refuse_a_second_apply_naming_the_session_that_holds_the_plan() {
    let home = build_disk_home("change-claims");
    let (plan, source, target) = plan_blocked_on_a_pipe(home.path());
    let mut first = spawn_ono(home.path(), &format!("apply {}", &plan[..8]));
    await_applying(home.path(), &plan);

    let second = ono_at(home.path(), &format!("apply {}", &plan[..8]));
    let while_held = std::fs::read_to_string(&target).expect("the target is readable");

    // Writing nothing and closing the pipe lets the first apply's copy return.
    std::fs::write(&source, b"").expect("the pipe is closed");
    let _ = first.wait();
    assert!(
        !second.status().is_success() && second.stderr().contains("change.plan_already_applying"),
        "§42.4: the second apply is refused while another session holds the plan. Got {:?}",
        second.output()
    );
    assert!(
        second.stderr().contains("session s"),
        "§42.4: the refusal names the session that holds it. Got {:?}",
        second.stderr()
    );
    assert_eq!(
        while_held, "b\n",
        "§42.4: the refused apply changed nothing while the first was still running"
    );
}

#[test]
fn should_resume_at_once_a_plan_whose_applier_was_killed_mid_action() {
    let home = build_disk_home("change-claims");
    let (plan, source, target) = plan_blocked_on_a_pipe(home.path());
    let mut first = spawn_ono(home.path(), &format!("apply {}", &plan[..8]));
    await_applying(home.path(), &plan);
    first.kill().expect("the applier is killed");
    let _ = first.wait();
    std::fs::remove_file(&source).expect("the pipe is removed");
    std::fs::write(&source, "source\n").expect("the source is a file again");

    let resumed = ono_at(
        home.path(),
        &format!("resume plan {} --confirm", &plan[..8]),
    );

    resumed.assert_success();
    assert_eq!(
        std::fs::read_to_string(&target).expect("the target is readable"),
        "source\n",
        "§41.3: resume continues the idempotent copy to the end"
    );
    assert_eq!(
        text(
            &one(&ono_at(
                home.path(),
                &format!("get plan {} | to json", &plan[..8])
            )),
            "state"
        ),
        "verified",
        "§42.3 and §41.2: the dead session's claim did not hold the plan, and it verified"
    );
}
