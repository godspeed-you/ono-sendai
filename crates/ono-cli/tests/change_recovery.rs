//! A recovery plan is a plan: it is inspected, its impact is shown, it is applied and it is
//! verified (spec v0.6 §2.12, §24.1, §55.8, §63's twelfth release criterion).
//!
//! §2.12 is the invariant: recovery is itself a change. `recover` produces a distinct plan and
//! touches nothing (§24.1); the plan answers `inspect plan` and `impact` like any other before it
//! runs; `apply` carries it out and `verify` holds it to its contract. Each test drives the real
//! binary against a change it applied first, so the recovery has a real asset to come from.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

#[path = "change_support/mod.rs"]
mod change_support;

use std::path::{Path, PathBuf};

use change_support::{build_disk_home, one, ono_at, rows, text};

/// A configuration file an applied, protected plan overwrote: `(the file, the plan's id)`.
fn overwritten_by_a_plan(home: &Path) -> (PathBuf, String) {
    let config = home.join("service.conf");
    let replacement = home.join("service.conf.next");
    std::fs::write(&config, "before the change\n").expect("the file is written");
    std::fs::write(&replacement, "after the change\n").expect("the replacement is written");
    let planned = ono_at(
        home,
        &format!(
            "plan copy file {} {} --overwrite | to json",
            replacement.display(),
            config.display()
        ),
    );
    planned.assert_success();
    let plan = text(&one(&planned), "id");
    ono_at(home, &format!("apply {}", &plan[..8])).assert_success();
    (config, plan)
}

/// `recover <plan> | to json`: the recovery plan's record.
fn recovery_plan(home: &Path, plan: &str) -> serde_yaml_ng::Value {
    let run = ono_at(home, &format!("recover {} | to json", &plan[..8]));
    run.assert_success();
    one(&run)
}

#[test]
fn should_apply_a_recovery_in_the_session_that_applied_the_change() {
    // §2.12 and §55.11 case 50: recovery is a plan like any other, protected before it runs, and
    // the session that applied a change can recover it without starting again.
    let home = build_disk_home("change-recovery-same-session");
    let config = home.path().join("service.conf");
    let replacement = home.path().join("service.conf.next");
    std::fs::write(&config, "before the change\n").expect("the file is written");
    std::fs::write(&replacement, "after the change\n").expect("the replacement is written");

    let run = ono_at(
        home.path(),
        &format!(
            "plan copy file {} {} --overwrite | apply\n\
             recover \"@\" | apply --accept-newer-state-loss --confirm",
            replacement.display(),
            config.display()
        ),
    );

    run.assert_success();
    assert_eq!(
        std::fs::read_to_string(&config).expect("the file is readable"),
        "before the change\n",
        "§24: the recovery put back the state the change replaced"
    );
}

#[test]
fn should_produce_a_distinct_recovery_plan_and_touch_nothing_when_recovery_is_asked_for() {
    let home = build_disk_home("change-recovery");
    let (config, plan) = overwritten_by_a_plan(home.path());

    let recovery = recovery_plan(home.path(), &plan);

    let id = text(&recovery, "id");
    assert_ne!(
        id, plan,
        "§24.1: `recover` produces a plan of its own, distinct from the change it recovers"
    );
    assert_eq!(
        text(&recovery, "source_plan"),
        plan,
        "§24.3: the recovery plan names the change it recovers. Got {recovery:?}"
    );
    let stored = ono_at(home.path(), &format!("get plan {} | to json", &id[..8]));
    stored.assert_success();
    assert_eq!(
        text(&one(&stored), "kind"),
        "recovery",
        "§2.12: the store holds the recovery as a plan of kind recovery"
    );
    assert_eq!(
        std::fs::read_to_string(&config).expect("readable"),
        "after the change\n",
        "§24.1: planning the recovery changes nothing on the system"
    );
}

#[test]
fn should_answer_inspect_and_impact_for_a_recovery_plan_before_it_runs() {
    let home = build_disk_home("change-recovery");
    let (config, plan) = overwritten_by_a_plan(home.path());
    let recovery = text(&recovery_plan(home.path(), &plan), "id");

    let inspected = ono_at(
        home.path(),
        &format!("inspect plan {} | to json", &recovery[..8]),
    );
    let impact = ono_at(home.path(), &format!("impact {} | to json", &recovery[..8]));

    inspected.assert_success();
    assert_eq!(
        text(&one(&inspected), "id"),
        recovery,
        "§2.12: `inspect plan` opens a recovery plan like any other"
    );
    impact.assert_success();
    assert!(
        impact.stdout().contains(&config.display().to_string()),
        "§2.12 and §24.3: the recovery's impact names the file it would put back. Got {:?}",
        impact.stdout()
    );
    assert_eq!(
        std::fs::read_to_string(&config).expect("readable"),
        "after the change\n",
        "§2.1: inspecting a plan and showing its impact change nothing"
    );
}

#[test]
fn should_apply_and_verify_a_recovery_plan_through_the_same_commands_as_any_plan() {
    let home = build_disk_home("change-recovery");
    let (config, plan) = overwritten_by_a_plan(home.path());
    let recovery = text(&recovery_plan(home.path(), &plan), "id");

    let applied = ono_at(home.path(), &format!("apply {}", &recovery[..8]));
    let verified = ono_at(home.path(), &format!("verify {}", &recovery[..8]));
    let state = ono_at(
        home.path(),
        &format!("get plan {} | to json", &recovery[..8]),
    );

    applied.assert_success();
    assert_eq!(
        std::fs::read_to_string(&config).expect("readable"),
        "before the change\n",
        "§24: applying the recovery plan puts the file back as it was before the change"
    );
    verified.assert_success();
    state.assert_success();
    let record = rows(&state)
        .into_iter()
        .next()
        .expect("`get plan` answers with the recovery plan");
    assert!(
        text(&record, "state").contains("verified"),
        "§2.12 and §25: the recovery plan reaches a verified state through `verify`. Got {record:?}"
    );
}

#[test]
fn should_carry_what_the_recovery_will_do_when_the_plan_is_read_back() {
    let home = build_disk_home("change-recovery");
    let (_, plan) = overwritten_by_a_plan(home.path());
    let recovery = text(&recovery_plan(home.path(), &plan), "id");

    let stored = one(&ono_at(
        home.path(),
        &format!("get plan {} | to json", &recovery[..8]),
    ));

    let analysis = stored.get("ono.change/recovery").unwrap_or_else(|| {
        panic!("§24.1 and §36.1: a recovery plan read back carries its analysis. Got {stored:?}")
    });
    assert_eq!(
        text(analysis, "method"),
        "selective-file-restore",
        "§24.3: the plan a later process reads states the method it will run"
    );
}
