//! The acknowledgement gates, and the normal path they leave alone (v0.6 §19.4, §40).
//!
//! §40.1 keeps `apply` usable: a LOW or MODERATE plan with no irreversible action applies on
//! `apply` alone, because a gate on every plan is a gate nobody reads. §40.2 fixes what a gate
//! says when there is one — the rule's own reason — and §40.3 fixes who may be asked: a script
//! never waits, so every gate is a flag and every refusal is structured.

// The core build of #127 leaves this tier out (ADR-0910, ADR-0925).
#![cfg(feature = "change")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

#[path = "change_support/mod.rs"]
mod change_support;
#[path = "support/service_manager.rs"]
mod service_manager;

use change_support::{build_disk_home, home, one, ono_at, ono_with, text};
use ono_testkit::{Run, Scratch, SkipReason, skipped};
use service_manager::FixtureServiceManager;

/// What a host without `dbus-daemon` cannot present (v0.4.1 §38.1).
const NO_BUS: &str = "this needs `dbus-daemon`, so the fixture can serve a service manager holding \
                      the fifty units a bulk restart plans against";

/// How many units the fixture's service manager holds: exactly §53's
/// `change.bulk.high_risk_targets`, so a plan over all of them is HIGH.
const BULK: i64 = 50;

/// A plan §19.4 gates, made in a scratch home against a service manager the test owns.
///
/// Fifty targets reaches `change.bulk.high_risk_targets` and is HIGH (§53). The fifty are the
/// fixture's units rather than the host's: a host's unit list changes while the suite runs — a
/// container's transient `docker-<id>.scope` exists when `get service` lists it and is gone when
/// the plan freezes it, and §4.3 then rightly refuses (`change.target_unresolved`, issue #300).
/// The fixture holds exactly the units it was started with, so the plan's verdict is the risk
/// gate's, and `apply` queues its jobs on the fixture rather than on the host (ADR-1080).
struct Bulk {
    home: Scratch,
    manager: FixtureServiceManager,
    reference: String,
}

impl Bulk {
    /// The plan, or `None` where this host has no `dbus-daemon`.
    fn plan() -> Option<Self> {
        let manager = FixtureServiceManager::try_start(BULK)?;
        let home = home();
        let run = ono_with(
            home.path(),
            "DBUS_SYSTEM_BUS_ADDRESS",
            &manager.address(),
            "get service | plan restart service | to json",
        );
        run.assert_success();
        let reference = text(&one(&run), "id")[..4].to_owned();
        assert_eq!(
            manager.requests().jobs,
            0,
            "v0.6 §2.1: planning a restart is a read, and it queued no job"
        );
        Some(Self {
            home,
            manager,
            reference,
        })
    }

    /// `script` in the plan's home, against the fixture's service manager.
    fn ono(&self, script: &str) -> Run {
        ono_with(
            self.home.path(),
            "DBUS_SYSTEM_BUS_ADDRESS",
            &self.manager.address(),
            script,
        )
    }
}

/// A plan §40.1 leaves alone: one file, no irreversible action.
fn ordinary_plan(home: &std::path::Path) -> String {
    let source = home.join("source.txt");
    let destination = home.join("destination.txt");
    std::fs::write(&source, b"new\n").expect("the source is written");
    std::fs::write(&destination, b"old\n").expect("the destination is written");
    let run = ono_at(
        home,
        &format!(
            "plan copy file {} {} --overwrite | to json",
            source.display(),
            destination.display()
        ),
    );
    run.assert_success();
    let record = one(&run);
    text(&record, "id")[..4].to_owned()
}

#[test]
fn should_refuse_a_high_risk_plan_when_the_risk_was_not_acknowledged() {
    let Some(bulk) = Bulk::plan() else {
        skipped(SkipReason::ExternalToolUnavailable, NO_BUS);
        return;
    };
    let reference = &bulk.reference;
    let run = bulk.ono(&format!("apply {reference}"));
    assert!(
        run.output().contains("change.risk_not_accepted"),
        "v0.6 §19.4: a HIGH plan requires an explicit acknowledgement before it may be applied. \
         Got {:?}",
        run.output()
    );
}

#[test]
fn should_name_the_rule_that_fired_when_a_risk_gate_refuses() {
    let Some(bulk) = Bulk::plan() else {
        skipped(SkipReason::ExternalToolUnavailable, NO_BUS);
        return;
    };
    let reference = &bulk.reference;
    let run = bulk.ono(&format!("apply {reference}"));
    assert!(
        run.output().contains("risk.bulk.high-threshold"),
        "v0.6 §40.2: the refusal carries the rule's own sentence rather than a generic question. \
         Got {:?}",
        run.output()
    );
}

#[test]
fn should_change_nothing_when_a_risk_gate_refuses() {
    let Some(bulk) = Bulk::plan() else {
        skipped(SkipReason::ExternalToolUnavailable, NO_BUS);
        return;
    };
    let reference = &bulk.reference;
    let run = bulk.ono(&format!("apply {reference}"));
    assert!(
        run.output().contains("Nothing was changed"),
        "v0.6 §40.2: a refused gate has changed nothing, and the refusal says so rather than \
         leaving the operator to infer it. Got {:?}",
        run.output()
    );
    let state = bulk.ono(&format!("get plan {reference} | to json"));
    state.assert_success();
    assert_eq!(
        bulk.manager.requests().jobs,
        0,
        "v0.6 §40.2: a refused gate restarted no unit — the service manager was asked for no job"
    );
    assert_eq!(
        text(&one(&state), "state"),
        "sealed",
        "v0.6 §4.1: a plan refused at the gate is still the sealed plan it was; nothing moved it \
         along the lifecycle"
    );
}

#[test]
fn should_name_the_flag_that_answers_the_gate() {
    let Some(bulk) = Bulk::plan() else {
        skipped(SkipReason::ExternalToolUnavailable, NO_BUS);
        return;
    };
    let reference = &bulk.reference;
    let run = bulk.ono(&format!("apply {reference}"));
    assert!(
        run.output().contains("--accept-risk"),
        "v0.6 §40.3: every acknowledgement has a flag, and the refusal names it so a script can \
         supply it as policy rather than waiting for a prompt. Got {:?}",
        run.output()
    );
}

#[test]
fn should_get_past_the_gate_when_the_risk_is_acknowledged() {
    let Some(bulk) = Bulk::plan() else {
        skipped(SkipReason::ExternalToolUnavailable, NO_BUS);
        return;
    };
    let reference = &bulk.reference;
    let run = bulk.ono(&format!("apply {reference} --accept-risk --confirm"));
    assert!(
        !run.output().contains("change.risk_not_accepted"),
        "v0.6 §19.4: `--accept-risk` is the acknowledgement, and a plan carrying it is not \
         refused for the class it acknowledged. Got {:?}",
        run.output()
    );
    // Review findings COR-C6, TST-7: the absence of one refusal is not an outcome. Past the risk
    // gate the apply meets the next one — a restart needs privilege this unprivileged test does
    // not hold (§43.3, §43.4) — and stops there: it exits non-zero with that refusal, and the
    // fixture's service manager was asked for no job. A gate that stopped refusing for the risk
    // class would still be caught (the first assertion); one that let the plan run unprivileged
    // would be caught by the job count.
    assert!(
        !run.status().is_success() && run.output().contains("change.privilege_required"),
        "v0.6 §43.3: past the risk gate, the privilege the restarts need is what refuses. Got {:?}",
        run.output()
    );
    assert_eq!(
        bulk.manager.requests().jobs,
        0,
        "v0.6 §43.4: nothing was changed — no restart job reached the fixture. Got {:?}",
        run.output()
    );
}

#[test]
fn should_seal_the_acknowledgement_into_a_new_revision() {
    let Some(bulk) = Bulk::plan() else {
        skipped(SkipReason::ExternalToolUnavailable, NO_BUS);
        return;
    };
    let reference = &bulk.reference;
    bulk.ono(&format!("apply {reference} --accept-risk --confirm"));
    let run = bulk.ono(&format!("get plan {reference} | to json"));
    run.assert_success();
    let record = one(&run);
    assert!(
        record["revision"].as_u64().unwrap_or(1) > 1,
        "v0.6 §19.4: the acknowledgement is stored in the sealed revision, so acknowledging a \
         plan produces the next revision of it rather than editing the one that was refused. \
         Got {record:?}"
    );
}

#[test]
fn should_require_the_commitment_to_be_confirmed_outside_a_terminal() {
    let Some(bulk) = Bulk::plan() else {
        skipped(SkipReason::ExternalToolUnavailable, NO_BUS);
        return;
    };
    let reference = &bulk.reference;
    let run = bulk.ono(&format!("apply {reference} --accept-risk"));
    assert!(
        run.output().contains("safety.confirmation_required"),
        "v0.6 §40.3: a plan carrying any gate needs the commitment stated outside a terminal, and \
         a script never waits for a prompt. Got {:?}",
        run.output()
    );
}

#[test]
fn should_apply_an_ordinary_plan_without_any_acknowledgement() {
    let home = home();
    let reference = ordinary_plan(home.path());
    let run = ono_at(home.path(), &format!("apply {reference} | to json"));
    assert!(
        !run.output().contains("safety.confirmation_required")
            && !run.output().contains("change.risk_not_accepted"),
        "v0.6 §40.1: for a LOW or MODERATE plan with no irreversible action, invoking `apply` is \
         sufficient intent — the normal path stays usable. Got {:?}",
        run.output()
    );
    assert_eq!(
        std::fs::read_to_string(home.path().join("destination.txt"))
            .expect("the destination is readable"),
        "new\n",
        "v0.6 §5.6: an applied plan carried out its mutation"
    );
}

#[test]
fn should_refuse_to_apply_a_plan_nothing_names() {
    let home = home();
    let run = ono_at(home.path(), "apply ffffffff");
    assert!(
        run.output().contains("change.plan_not_found"),
        "v0.6 §36.4: a reference that matches no plan is refused by name rather than treated as \
         an empty selection. Got {:?}",
        run.output()
    );
}

#[test]
fn should_refuse_to_apply_an_expired_plan() {
    let home = home();
    let source = home.path().join("source.txt");
    let destination = home.path().join("destination.txt");
    std::fs::write(&source, b"new\n").expect("the source is written");
    std::fs::write(&destination, b"old\n").expect("the destination is written");
    let run = ono_at(
        home.path(),
        &format!(
            "plan --expires 1ns copy file {} {} --overwrite | to json",
            source.display(),
            destination.display()
        ),
    );
    run.assert_success();
    let reference = text(&one(&run), "id")[..4].to_owned();
    let applied = ono_at(home.path(), &format!("apply {reference} --confirm"));
    assert!(
        applied.output().contains("change.plan_expired"),
        "v0.6 §5.6 and §4.1: `apply` refuses an expired plan. Got {:?}",
        applied.output()
    );
    assert_eq!(
        std::fs::read_to_string(&destination).expect("the destination is readable"),
        "old\n",
        "v0.6 §5.6: a plan refused for expiry changed nothing"
    );
}

#[test]
fn should_stream_an_action_result_for_every_action_it_ran() {
    // On the build disk: §11.2 protects the overwritten destination only on a persistent
    // filesystem, so under a tmpfs temporary directory the executor settles the copy alone, and
    // on any other disk the copy and its protection.
    let home = build_disk_home("change-gates-results");
    let reference = ordinary_plan(home.path());
    let run = ono_at(home.path(), &format!("apply {reference} | to json"));
    let rows = change_support::rows(&run);
    assert!(
        rows.len() == 2
            && rows.iter().any(|row| {
                row["operation"]
                    .as_str()
                    .is_some_and(|operation| operation.starts_with("copy file "))
            }),
        "v0.6 §5.6: `apply` answers with a stream of `ono.action-result/1`, one per action the \
         executor settled — the copy, and the protection §11.2 takes of the destination it \
         overwrites. Got {rows:?}"
    );
    assert!(
        rows.iter()
            .all(|row| row["status"].as_str() == Some("success")),
        "v0.6 §4.7: every action result is recorded independently. Got {rows:?}"
    );
}

#[test]
fn should_keep_the_progress_display_out_of_the_value_stream() {
    let home = home();
    let reference = ordinary_plan(home.path());
    let run = ono_at(home.path(), &format!("apply {reference} | to json"));
    assert!(
        !run.stdout().contains("PREPARE"),
        "v0.2 §13.1 and Appendix E.4: the progress display is a presentation of the stream and \
         goes to the diagnostic stream, never into the values. Got stdout {:?}",
        run.stdout()
    );
    assert!(
        run.stderr().contains("PREPARE"),
        "Appendix E.4: PREPARE, APPLY and VERIFY stay separately visible, so no single bar hides \
         whether protection completed. Got stderr {:?}",
        run.stderr()
    );
}
