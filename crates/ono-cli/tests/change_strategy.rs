//! A canary batch is held to its own verification (spec v0.6 §28.4, §28.6, §55.10).
//!
//! §28.6: a canary strategy runs a first small batch whose required verification must pass
//! before the rest continue. The batch's verification is the checks about the objects it changed;
//! a check about an object no batch has reached yet would fail for the one reason the batches
//! exist, and stop a canary that did everything right.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

#[path = "change_support/mod.rs"]
mod change_support;

use change_support::{build_disk_home, one, ono_at, text};

#[test]
fn should_run_every_wave_when_the_canary_batch_verifies() {
    let home = build_disk_home("change-strategy");
    let source = home.path().join("src");
    std::fs::write(&source, "new\n").expect("the source is written");
    let targets: Vec<_> = ["t1", "t2", "t3"]
        .iter()
        .map(|name| {
            let target = home.path().join(name);
            std::fs::write(&target, "old\n").expect("a target is written");
            target
        })
        .collect();
    let copies: Vec<String> = targets
        .iter()
        .map(|target| {
            format!(
                "copy file {} {} --overwrite",
                source.display(),
                target.display()
            )
        })
        .collect();
    let planned = ono_at(
        home.path(),
        &format!(
            "plan --strategy 'canary 1 1' {{ {}; verify file {} exists == true }} | to json",
            copies.join("; "),
            targets[0].display()
        ),
    );
    planned.assert_success();
    let plan = text(&one(&planned), "id");

    let applied = ono_at(home.path(), &format!("apply {}", &plan[..8]));

    applied.assert_success();
    for target in &targets {
        assert_eq!(
            std::fs::read_to_string(target).expect("a target is readable"),
            "new\n",
            "§28.6: the canary verified, so every wave ran and {} changed",
            target.display()
        );
    }
    assert_eq!(
        text(
            &one(&ono_at(
                home.path(),
                &format!("get plan {} | to json", &plan[..8])
            )),
            "state"
        ),
        "verified",
        "§4.8: after the last wave the whole plan verifies"
    );
}
