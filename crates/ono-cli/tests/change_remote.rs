//! A plan is about the machine it freezes on (spec v0.6 §29.1, §7.1).
//!
//! §29.1 makes truth per host, and §7.1 has a target on another host record that host. This build
//! freezes targets only on the machine the shell runs on, so a plan made inside `enter link` would
//! seal this machine's objects as the linked host's. It refuses instead, and nothing is sealed.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

#[path = "change_support/mod.rs"]
mod change_support;

use change_support::{home, ono_at};

#[test]
fn should_refuse_to_plan_inside_a_link_rather_than_freeze_this_machines_objects() {
    let home = home();
    let source = home.path().join("a");
    let target = home.path().join("b");
    std::fs::write(&source, "x\n").expect("the source is written");
    std::fs::write(&target, "y\n").expect("the target is written");

    let run = ono_at(
        home.path(),
        &format!(
            "link host l1 --transport local; enter link l1; \
             plan copy file {} {} --overwrite",
            source.display(),
            target.display()
        ),
    );

    assert!(
        !run.status().is_success() && run.stderr().contains("change.action_not_plannable"),
        "§29.1: a plan inside a link is refused rather than frozen on this machine. Got {:?}",
        run.output()
    );
    assert!(
        run.stderr().contains("link"),
        "§29.1: the refusal says the link is why. Got {:?}",
        run.stderr()
    );
    let plans = ono_at(home.path(), "get plan | count | to json");
    plans.assert_success();
    assert_eq!(
        plans.stdout().split_whitespace().collect::<String>(),
        "[0]",
        "§2.1: nothing was sealed"
    );
}
