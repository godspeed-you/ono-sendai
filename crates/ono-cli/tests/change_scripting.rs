//! v0.6 in a script: nothing waits for a prompt, and every command answers with a value
//! (spec v0.6 §17.4, §40.3, §12.3, v0.2 §33.5).
//!
//! §40.3 forbids a script waiting for a question: every acknowledgement §19.4 can ask for has a
//! flag, and a gate nobody answered refuses with a structured error. §17.4 extends the rule to
//! policy: a protection requirement that cannot be met fails the same way. And a v0.6 command is
//! a pipeline stage like any other, so its output is a typed value `where` can filter and
//! `to json` can serialise.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

#[path = "change_support/mod.rs"]
mod change_support;

use std::path::Path;

use change_support::{Sleeper, build_disk_home, home, one, ono_at, rows, text};

/// Plans a copy over a fresh file in `home`, answering with the plan's id.
fn planned_copy(home: &Path, name: &str) -> String {
    let source = home.join(format!("{name}.next"));
    let destination = home.join(format!("{name}.conf"));
    std::fs::write(&source, "next\n").expect("the source is written");
    std::fs::write(&destination, "current\n").expect("the destination is written");
    let run = ono_at(
        home,
        &format!(
            "plan copy file {} {} --overwrite | to json",
            source.display(),
            destination.display()
        ),
    );
    run.assert_success();
    text(&one(&run), "id")
}

#[test]
fn should_name_a_flag_for_every_acknowledgement_apply_can_ask_for() {
    let home = home();

    let help = ono_at(home.path(), "help apply");

    help.assert_success();
    for flag in [
        "--accept-risk",
        "--accept-irreversible",
        "--accept-service-outage",
        "--accept-newer-state-loss",
        "--accept-stale-protection",
        "--confirm",
    ] {
        assert!(
            help.stdout().contains(flag),
            "§40.3: every acknowledgement a script may need has a flag, and `{flag}` is missing \
             from `help apply`. Got {:?}",
            help.stdout()
        );
    }
}

#[test]
fn should_refuse_a_gated_apply_in_a_script_rather_than_wait_for_an_answer() {
    let home = home();
    let mut sleeper = Sleeper::start();
    let plan = ono_at(
        home.path(),
        &format!(
            "plan kill process {} --signal SIGKILL | to json",
            sleeper.pid()
        ),
    );
    plan.assert_success();
    let id = text(&one(&plan), "id");

    let applied = ono_at(home.path(), &format!("apply {}", &id[..8]));

    assert!(
        !applied.status().is_success(),
        "§40.3: a gated apply nobody acknowledged refuses in a script. Got {:?}",
        applied.output()
    );
    assert!(
        applied.stderr().contains(" --accept-") || applied.stderr().contains("--confirm"),
        "§40.2: the refusal names the flag that answers it. Got {:?}",
        applied.stderr()
    );
    assert!(
        sleeper.is_alive(),
        "§40.3: the process the refused plan would have killed is untouched"
    );
}

#[test]
fn should_fail_with_a_structured_error_when_required_protection_cannot_be_met() {
    let home = home();
    let mut sleeper = Sleeper::start();

    // §33.1: process runtime is not recoverable by any snapshot, so `require` cannot be met.
    let planned = ono_at(
        home.path(),
        &format!(
            "plan --protection require kill process {} --signal SIGKILL | to json",
            sleeper.pid()
        ),
    );
    let refusal = if planned.status().is_success() {
        let id = text(&one(&planned), "id");
        ono_at(
            home.path(),
            &format!("apply {} --accept-irreversible --confirm", &id[..8]),
        )
    } else {
        planned
    };

    assert!(
        !refusal.status().is_success(),
        "§17.2 and §17.4: a requirement that cannot be met fails the script. Got {:?}",
        refusal.output()
    );
    assert!(
        refusal.stderr().contains("recovery.coverage_insufficient"),
        "§45: the failure is the structured `recovery.coverage_insufficient`. Got {:?}",
        refusal.stderr()
    );
    assert!(
        sleeper.is_alive(),
        "§2.3: nothing mutates when the required protection is missing"
    );
}

#[test]
fn should_serialise_every_change_command_as_a_list_a_script_can_read() {
    let home = build_disk_home("change-scripting");
    let sealed = planned_copy(home.path(), "sealed");
    let applied = planned_copy(home.path(), "applied");
    let apply = ono_at(home.path(), &format!("apply {} | to json", &applied[..8]));
    apply.assert_success();
    rows(&apply);
    let assets = ono_at(home.path(), "get recovery | to json");
    assets.assert_success();
    let asset = text(
        rows(&assets)
            .first()
            .expect("the applied plan's protection made an asset"),
        "id",
    );

    for script in [
        "get plan".to_owned(),
        format!("get plan {}", &sealed[..8]),
        format!("inspect plan {}", &sealed[..8]),
        format!("impact {}", &sealed[..8]),
        format!("rebase plan {}", &sealed[..8]),
        format!("verify {}", &applied[..8]),
        format!("recover {}", &applied[..8]),
        "get recovery".to_owned(),
        format!("inspect recovery {}", &asset[..8]),
        format!("remove recovery {} --dry-run", &asset[..8]),
    ] {
        let run = ono_at(home.path(), &format!("{script} | to json"));
        assert!(
            run.status().is_success(),
            "§12.3: `{script} | to json` answers in a script. Got {:?}",
            run.output()
        );
        rows(&run);
    }

    // §41.3: a settled plan has nothing to resume, and a script learns that from a structured
    // refusal rather than from a prompt or a success that did nothing.
    let resumed = ono_at(
        home.path(),
        &format!("resume plan {} --confirm | to json", &applied[..8]),
    );
    assert!(
        !resumed.status().is_success() && resumed.stderr().contains("change.resume_refused"),
        "§41.3 and §45: resuming a settled plan is the structured `change.resume_refused`. Got {:?}",
        resumed.output()
    );
}

#[test]
fn should_filter_plans_like_any_other_value() {
    let home = build_disk_home("change-scripting");
    planned_copy(home.path(), "first");
    planned_copy(home.path(), "second");

    let counted = ono_at(
        home.path(),
        "get plan | where state == \"sealed\" | count | to json",
    );

    counted.assert_success();
    assert_eq!(
        counted.stdout().split_whitespace().collect::<String>(),
        "[2]",
        "§12.3: a plan is a value a pipeline filters, and both sealed plans pass the filter. Got {:?}",
        counted.output()
    );
}

#[test]
fn should_fail_a_script_when_verify_finds_a_required_check_broken() {
    let home = build_disk_home("change-scripting");
    let plan = planned_copy(home.path(), "checked");
    ono_at(home.path(), &format!("apply {}", &plan[..8])).assert_success();
    // §2.14: the world moving after the fact makes the same check fail.
    std::fs::write(home.path().join("checked.conf"), "something else\n")
        .expect("the target is changed again");

    let verified = ono_at(home.path(), &format!("verify {}", &plan[..8]));
    let piped = ono_at(home.path(), &format!("verify {} | to json", &plan[..8]));

    assert!(
        !verified.status().is_success() && verified.stderr().contains("change.verification_failed"),
        "§23.3 and v0.2 §43: a required check that does not hold fails the script, by name. Got {:?}",
        verified.output()
    );
    assert!(
        piped.stdout().contains("\"failed\""),
        "§23.3: the per-check results still reach the pipe. Got {:?}",
        piped.output()
    );
}
