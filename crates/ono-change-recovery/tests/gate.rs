//! §24.5's explicit acceptance (v0.6 §13.6, §24.5, §40.1, §56.3, §62.8, Appendix C.4).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use ono_change_core::{
    ChangePlan, Intent, NewerStateClass, NewerStateImpact, NewerStateItem, RecoveryGoal,
    RecoveryPlan, RestoreMethod,
};
use ono_change_recovery::gate::{check, conflicting_objects, destroyed_objects};
use ono_value::Value;
use support::{EPOCH, at};

fn recovery(impact: NewerStateImpact) -> RecoveryPlan {
    let plan = ChangePlan::draft(
        Intent::new("restore nginx configuration", "recover plan/a82f"),
        "session-gate",
        EPOCH,
    )
    .seal(EPOCH)
    .expect("a plan with no mutating action seals without a contract");
    RecoveryPlan::new(
        plan,
        RecoveryGoal::RestoreChangedObjects,
        RestoreMethod::SelectiveFileRestore,
        "rpool/ROOT/debian@ono-a82f",
    )
    .with_newer_state(impact)
}

fn preserved(object: &str) -> NewerStateItem {
    NewerStateItem::new(
        object,
        NewerStateClass::PreservedByMethod,
        "outside the restore set",
    )
}

fn conflicting(object: &str) -> NewerStateItem {
    NewerStateItem::new(
        object,
        NewerStateClass::Conflicting,
        "the user edited it again at 15:12",
    )
    .changed_at(at(15, 12))
}

fn discarded(object: &str) -> NewerStateItem {
    NewerStateItem::new(
        object,
        NewerStateClass::DiscardedByMethod,
        "the rollback rewinds the whole domain",
    )
}

fn unknown(object: &str) -> NewerStateItem {
    NewerStateItem::new(
        object,
        NewerStateClass::Unknown,
        "the dataset did not answer",
    )
}

#[test]
fn should_let_an_ordinary_selective_restore_through_without_a_gate() {
    let plan = recovery(NewerStateImpact::analysed(vec![
        preserved("/etc/hosts"),
        preserved("/etc/ssh/sshd_config"),
    ]));
    assert!(
        check(&plan).is_ok(),
        "§40.1 applied to recovery: a recovery that loses nothing needs no acknowledgement"
    );
}

#[test]
fn should_let_a_recovery_that_found_nothing_newer_through() {
    let plan = recovery(NewerStateImpact::analysed(Vec::new()));
    assert!(check(&plan).is_ok());
}

#[test]
fn should_refuse_a_recovery_whose_drift_analysis_never_ran() {
    let plan = recovery(NewerStateImpact::unanalysed());
    let error = check(&plan).expect_err("§62.8: recovery without drift analysis is a failure mode");
    assert_eq!(error.code().name(), "recovery.plan_incomplete");
}

#[test]
fn should_refuse_an_unanalysed_recovery_even_once_destruction_was_accepted() {
    let plan = recovery(NewerStateImpact::unanalysed()).destruction_accepted();
    let error =
        check(&plan).expect_err("§56.3: there is nothing to accept about a loss nobody measured");
    assert_eq!(error.code().name(), "recovery.plan_incomplete");
}

#[test]
fn should_refuse_a_recovery_whose_scope_holds_an_object_nobody_could_observe() {
    let plan = recovery(NewerStateImpact::analysed(vec![unknown("/mnt/remote/etc")]));
    let error = check(&plan).expect_err("§56.3: fail closed");
    assert_eq!(error.code().name(), "recovery.plan_incomplete");
    assert!(
        error
            .help()
            .is_some_and(|help| help.contains("/mnt/remote/etc")),
        "§45: the refusal names what could not be established"
    );
}

#[test]
fn should_refuse_an_unobservable_scope_even_once_destruction_was_accepted() {
    let plan = recovery(NewerStateImpact::analysed(vec![unknown("/mnt/remote/etc")]))
        .destruction_accepted();
    assert!(
        check(&plan).is_err(),
        "§56.3: acceptance under §24.5 is acceptance of a known loss"
    );
}

#[test]
fn should_refuse_a_recovery_that_would_destroy_newer_snapshots() {
    let plan = recovery(
        NewerStateImpact::analysed(Vec::new())
            .destroying("tank/data@later-1")
            .destroying("tank/data@later-2"),
    );
    let error = check(&plan).expect_err("§13.6: Ono never silently adds destructive flags");
    assert_eq!(
        error.code().name(),
        "recovery.destructive_history_not_accepted"
    );
}

#[test]
fn should_enumerate_every_provider_native_object_that_would_be_destroyed() {
    let plan = recovery(
        NewerStateImpact::analysed(Vec::new())
            .destroying("tank/data@later-1")
            .destroying("tank/data@later-2"),
    );
    let error = check(&plan).expect_err("§24.5");
    let destroyed = error
        .metadata()
        .get("destroyed")
        .expect("§13.6: every object that would be destroyed is in the metadata")
        .as_list()
        .expect("a list")
        .to_vec();
    assert_eq!(
        destroyed,
        vec![
            Value::string("tank/data@later-1"),
            Value::string("tank/data@later-2"),
        ],
        "§24.5's `will destroy` lists each one"
    );
}

#[test]
fn should_refuse_a_recovery_that_would_discard_a_newer_edit_to_its_own_target() {
    let plan = recovery(NewerStateImpact::analysed(vec![conflicting(
        "/etc/nginx/nginx.conf",
    )]));
    let error = check(&plan).expect_err("Appendix C.4: recovery would discard the 15:12 edit");
    assert_eq!(error.code().name(), "recovery.newer_state_conflict");
}

#[test]
fn should_name_every_conflicting_object_in_the_refusal() {
    let plan = recovery(NewerStateImpact::analysed(vec![
        conflicting("/etc/nginx/nginx.conf"),
        discarded("/var/lib/app/db"),
        preserved("/etc/hosts"),
    ]));
    let error = check(&plan).expect_err("Appendix C.4");
    let conflicts = error
        .metadata()
        .get("conflicts")
        .expect("§24.3: newer state discarded")
        .as_list()
        .expect("a list")
        .len();
    assert_eq!(
        conflicts, 2,
        "the preserved object is not a loss, and the other two are"
    );
}

#[test]
fn should_describe_why_each_conflicting_object_conflicts() {
    let plan = recovery(NewerStateImpact::analysed(vec![conflicting(
        "/etc/nginx/nginx.conf",
    )]));
    let objects = conflicting_objects(&plan);
    assert!(
        objects[0].contains("/etc/nginx/nginx.conf") && objects[0].contains("15:12"),
        "§40.2: the confirmation summarises the actual reason, and it said {objects:?}"
    );
}

#[test]
fn should_report_the_destroyed_objects_for_a_renderer_to_show() {
    let plan = recovery(
        NewerStateImpact::analysed(Vec::new())
            .destroying("tank/data@later-1")
            .destroying("tank/data@later-2"),
    );
    assert_eq!(
        destroyed_objects(&plan),
        vec![
            "tank/data@later-1".to_owned(),
            "tank/data@later-2".to_owned()
        ],
        "§24.5 prints `will destroy` before the gate, not after it"
    );
}

#[test]
fn should_let_an_accepted_destructive_recovery_through() {
    let plan = recovery(NewerStateImpact::analysed(vec![conflicting(
        "/etc/nginx/nginx.conf",
    )]))
    .destruction_accepted();
    assert!(
        check(&plan).is_ok(),
        "§24.5: the explicit gate is what recovery execution waits for"
    );
}

#[test]
fn should_let_an_accepted_history_destroying_recovery_through() {
    let plan = recovery(NewerStateImpact::analysed(Vec::new()).destroying("tank/data@later-1"))
        .destruction_accepted();
    assert!(check(&plan).is_ok());
}

#[test]
fn should_report_destroyed_history_before_a_newer_state_conflict() {
    let plan = recovery(
        NewerStateImpact::analysed(vec![conflicting("/etc/nginx/nginx.conf")])
            .destroying("tank/data@later-1"),
    );
    let error = check(&plan).expect_err("both apply");
    assert_eq!(
        error.code().name(),
        "recovery.destructive_history_not_accepted",
        "§13.6: the objects an operator cannot get back are named first"
    );
}
