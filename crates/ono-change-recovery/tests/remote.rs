//! Remote recovery, planned per host (v0.6 §29.3, §29.4, §55.10 case 45).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_change_core::RestoreMethod;
use ono_change_recovery::remote::{HostObservation, HostState, plan_hosts};

/// A three-host recovery in which `web-02` was unreachable when the plan was made.
fn fleet() -> Vec<HostObservation> {
    vec![
        HostObservation::established(
            "web-01",
            RestoreMethod::SelectiveFileRestore,
            "rpool/ROOT/debian@ono-a82f",
        ),
        HostObservation::disconnected(
            "web-02",
            "restore /etc/nginx/nginx.conf",
            "the ssh transport closed mid-plan",
        ),
        HostObservation::established(
            "web-03",
            RestoreMethod::SelectiveFileRestore,
            "rpool/ROOT/debian@ono-a82f",
        ),
    ]
}

#[test]
fn should_plan_recovery_once_per_host() {
    let recovery = plan_hosts(&fleet());
    assert_eq!(
        recovery.hosts().len(),
        3,
        "§29.4: remote recovery is planned per host"
    );
}

#[test]
fn should_let_recovery_proceed_on_the_hosts_that_answered() {
    let recovery = plan_hosts(&fleet());
    let proceedable: Vec<&str> = recovery
        .proceedable()
        .iter()
        .map(|host| host.host())
        .collect();
    assert_eq!(proceedable, vec!["web-01", "web-03"]);
}

#[test]
fn should_say_that_recovery_cannot_proceed_on_a_disconnected_host() {
    let recovery = plan_hosts(&fleet());
    let blocked = recovery.blocked();
    assert_eq!(
        blocked.len(),
        1,
        "§29.4: Ono MUST show whether recovery can proceed"
    );
    assert_eq!(blocked[0].host(), "web-02");
    assert!(!blocked[0].can_proceed());
}

#[test]
fn should_carry_the_unknown_remote_state_refusal_for_a_disconnected_host() {
    let recovery = plan_hosts(&fleet());
    let error = recovery.blocked()[0]
        .refusal()
        .expect("§29.4: the fragment says why recovery cannot proceed there");
    assert_eq!(
        error.code().name(),
        "change.remote_state_unknown",
        "§29.3: Ono MUST NOT mark an unknown remote action failed or successful"
    );
}

#[test]
fn should_offer_to_ask_the_disconnected_host_again_when_the_link_returns() {
    let recovery = plan_hosts(&fleet());
    let error = recovery.blocked()[0]
        .refusal()
        .expect("the refusal is there");
    assert_eq!(
        error.retryable(),
        Some(true),
        "§29.3: querying again when the link returns is the way to resolve it"
    );
}

#[test]
fn should_name_the_action_whose_outcome_could_not_be_established() {
    let recovery = plan_hosts(&fleet());
    let error = recovery.blocked()[0]
        .refusal()
        .expect("the refusal is there");
    assert_eq!(
        error.metadata().get("action"),
        Some(&ono_value::Value::string("restore /etc/nginx/nginx.conf")),
        "§29.3: exact per-host action state is preserved"
    );
}

#[test]
fn should_assume_no_method_for_a_host_whose_state_could_not_be_established() {
    let recovery = plan_hosts(&fleet());
    assert_eq!(
        recovery.blocked()[0].method(),
        None,
        "§55.10 case 45: a disconnected host is UNKNOWN, not automatically failed"
    );
}

#[test]
fn should_name_the_method_and_asset_for_a_host_that_answered() {
    let recovery = plan_hosts(&fleet());
    let web01 = recovery.proceedable()[0];
    assert_eq!(web01.method(), Some(RestoreMethod::SelectiveFileRestore));
    assert_eq!(web01.asset(), Some("rpool/ROOT/debian@ono-a82f"));
}

#[test]
fn should_report_the_fleet_recovery_as_incomplete_while_a_host_is_unreachable() {
    let recovery = plan_hosts(&fleet());
    assert!(
        !recovery.is_complete(),
        "§29.4: a per-host plan that omitted the unreachable host would read as a fleet plan"
    );
}

#[test]
fn should_report_the_fleet_recovery_as_complete_when_every_host_answered() {
    let hosts = vec![
        HostObservation::established("web-01", RestoreMethod::SelectiveFileRestore, "snap"),
        HostObservation::established("web-03", RestoreMethod::SelectiveFileRestore, "snap"),
    ];
    assert!(plan_hosts(&hosts).is_complete());
}

#[test]
fn should_say_that_recovery_cannot_proceed_on_a_host_that_holds_no_asset() {
    let hosts = vec![HostObservation::new(
        "web-04",
        HostState::Unprotected {
            reason: std::sync::Arc::from("no validated recovery asset exists on this host"),
        },
    )];
    let recovery = plan_hosts(&hosts);
    let error = recovery.blocked()[0]
        .refusal()
        .expect("§11.4: an asset that does not exist is not a way back");
    assert_eq!(error.code().name(), "recovery.plan_incomplete");
}

#[test]
fn should_explain_per_host_why_recovery_cannot_proceed() {
    let recovery = plan_hosts(&fleet());
    assert!(
        recovery.blocked()[0]
            .reason()
            .contains("the ssh transport closed mid-plan"),
        "§45: the sentence a person reads carries the reason"
    );
}

#[test]
fn should_explain_per_host_how_recovery_would_proceed() {
    let recovery = plan_hosts(&fleet());
    assert!(
        recovery.proceedable()[0]
            .reason()
            .contains("selective-file-restore"),
        "§29.4: the per-host fragment says what it would do there"
    );
}

#[test]
fn should_keep_the_hosts_in_the_order_they_were_observed() {
    let recovery = plan_hosts(&fleet());
    let hosts: Vec<&str> = recovery.hosts().iter().map(|host| host.host()).collect();
    assert_eq!(
        hosts,
        vec!["web-01", "web-02", "web-03"],
        "a per-host plan is read beside the per-host matrix it came from (§29.2)"
    );
}
