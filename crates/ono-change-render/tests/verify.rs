//! Verification views: §23.4 for a plan, §25.2 for a recovery, and §25.3's prohibition.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_change_render::{NO_FULL_EQUIVALENCE, recovery_verification, verification_view};

mod support;
use support::{
    contains, equivalence, index_of, list, map, nginx_results, record, recovery_results, result,
    rewritten, s, sealed_nginx_plan, verified_recovery,
};

#[test]
fn should_separate_required_advisory_and_observed_checks() {
    let lines = verification_view(&sealed_nginx_plan(), &nginx_results(), 80);
    assert!(
        contains(&lines, "required") && contains(&lines, "advisory"),
        "§23.2: a failing required postcondition and a failing advisory one mean different things"
    );
}

#[test]
fn should_print_the_status_of_every_check_it_was_given() {
    let lines = verification_view(&sealed_nginx_plan(), &nginx_results(), 80);
    assert_eq!(
        lines.iter().filter(|line| line.contains("PASS")).count(),
        3,
        "§23.4's example prints one line per contract, and none of them is folded away"
    );
}

#[test]
fn should_name_the_check_the_way_the_contract_states_it() {
    let lines = verification_view(&sealed_nginx_plan(), &nginx_results(), 80);
    assert!(
        contains(&lines, "nginx.service == running"),
        "§23.4's example reads as the condition, which is the subject and the expression together"
    );
}

#[test]
fn should_close_with_the_verdict_the_results_compose_to() {
    let lines = verification_view(&sealed_nginx_plan(), &nginx_results(), 80);
    let status = index_of(&lines, "status").expect("§23.4 closes with a status");
    assert_eq!(
        lines[status + 1].trim(),
        "VERIFIED",
        "§4.8: every required postcondition held, and nothing else changes that word"
    );
}

#[test]
fn should_fail_the_plan_when_a_required_postcondition_failed() {
    let results = vec![result("required", "nginx.service", "== running", "failed")];
    let lines = verification_view(&sealed_nginx_plan(), &results, 80);
    assert!(
        contains(&lines, "FAILED") && contains(&lines, "FAIL"),
        "§23.2: a failing required postcondition makes the plan FAILED"
    );
}

#[test]
fn should_degrade_the_plan_when_an_advisory_expectation_failed() {
    let results = vec![
        result("required", "nginx.service", "== running", "passed"),
        result("advisory", "worker count", "== 4", "failed"),
    ];
    let lines = verification_view(&sealed_nginx_plan(), &results, 80);
    assert!(
        contains(&lines, "DEGRADED"),
        "§23.2: a failing advisory expectation may make the plan DEGRADED, never FAILED"
    );
}

#[test]
fn should_keep_an_unanswerable_check_out_of_the_passing_column() {
    let results = vec![result("required", "nginx.service", "== running", "unknown")];
    let lines = verification_view(&sealed_nginx_plan(), &results, 80);
    assert!(
        contains(&lines, "UNKNOWN"),
        "§23.5: a check that could not be answered has its own word"
    );
    assert!(
        contains(&lines, "DEGRADED"),
        "§23.5 forbids treating an unanswerable required check as success"
    );
}

#[test]
fn should_never_read_an_observation_as_a_reason_to_degrade() {
    let results = vec![
        result("required", "nginx.service", "== running", "passed"),
        result("observational", "postgres connection", "changed", "unknown"),
    ];
    let lines = verification_view(&sealed_nginx_plan(), &results, 80);
    assert!(
        contains(&lines, "VERIFIED") && contains(&lines, "observed"),
        "§23.2: an observational result provides context and never changes the plan's state"
    );
}

#[test]
fn should_say_no_contract_was_answered_rather_than_printing_an_empty_view() {
    let lines = verification_view(&sealed_nginx_plan(), &[], 80);
    assert!(
        contains(&lines, "no contract was answered"),
        "§10.5: an empty verification view and a verified plan must not look the same"
    );
}

#[test]
fn should_report_the_three_equivalence_domains_separately() {
    let lines = recovery_verification(&verified_recovery(), &recovery_results(), 80);
    for domain in ["persistent state", "runtime", "external side effects"] {
        assert!(
            contains(&lines, domain),
            "§25.1: recovery verification MUST distinguish `{domain}` from the others"
        );
    }
}

#[test]
fn should_call_a_new_worker_identity_a_difference_that_was_expected() {
    let lines = recovery_verification(&verified_recovery(), &recovery_results(), 80);
    let line = lines
        .iter()
        .find(|line| line.contains("worker PIDs"))
        .expect("the subject is rendered");
    assert!(
        line.contains("DIFFERENT / EXPECTED"),
        "§25.2: a service restart may restore configuration while naturally creating new PIDs"
    );
}

#[test]
fn should_call_a_live_session_not_recoverable() {
    let lines = recovery_verification(&verified_recovery(), &recovery_results(), 80);
    let line = lines
        .iter()
        .find(|line| line.contains("TCP connections"))
        .expect("the subject is rendered");
    assert!(
        line.contains("NOT RECOVERABLE"),
        "§25.2 and §35.2: no local asset brings a live session back, and the view says which word applies"
    );
}

#[test]
fn should_claim_only_the_persistent_scope_it_verified() {
    let lines = recovery_verification(&verified_recovery(), &recovery_results(), 80);
    assert!(
        contains(&lines, "PERSISTENT STATE VERIFIED"),
        "§25.2's result block names the scope the verification actually covers"
    );
    assert!(
        contains(&lines, NO_FULL_EQUIVALENCE),
        "§25.3: user-visible language MUST describe the verified scope and claim nothing beyond it"
    );
}

#[test]
fn should_deny_the_persistent_claim_when_something_did_not_come_back() {
    let results = vec![equivalence(
        "persistent-state",
        "nginx.conf",
        "not-restored",
    )];
    let lines = recovery_verification(&verified_recovery(), &results, 80);
    assert!(
        contains(&lines, "PERSISTENT STATE NOT VERIFIED"),
        "§10.5: an absent claim and a denied claim are different facts"
    );
    assert!(
        contains(&lines, NO_FULL_EQUIVALENCE),
        "§25.3 holds whether or not the persistent domain came back"
    );
}

#[test]
fn should_say_when_something_is_outside_any_recovery() {
    let lines = recovery_verification(&verified_recovery(), &recovery_results(), 80);
    assert!(
        contains(&lines, "OUTSIDE ANY RECOVERY"),
        "§35.2 and §62.2: a webhook already delivered is the reason there is no universal undo"
    );
}

#[test]
fn should_say_a_domain_was_not_observed_rather_than_leaving_it_out() {
    let results = vec![equivalence("persistent-state", "nginx.conf", "restored")];
    let lines = recovery_verification(&verified_recovery(), &results, 80);
    assert!(
        contains(&lines, "nothing was observed in this domain"),
        "§25.1: a domain nobody looked at is a fact, and an omitted heading reads as a pass"
    );
}

#[test]
fn should_never_invent_an_expected_difference_for_a_result_nobody_classified() {
    let mut results = Vec::new();
    results.push(result("required", "nginx.conf", "== restored", "passed"));
    let unclassified = support::record(
        "ono.change-verification",
        &[
            ("class", support::s("required")),
            ("subject", support::s("worker PIDs")),
            ("expression", support::s("")),
            ("status", support::s("passed")),
            ("equivalence_domain", support::s("runtime-state")),
        ],
    );
    results.push(unclassified);
    let lines = recovery_verification(&verified_recovery(), &results, 80);
    assert!(
        !contains(&lines, "DIFFERENT / EXPECTED"),
        "§50.1: whether a difference was anticipated is a fact, and a renderer may not decide one"
    );
}

#[test]
fn should_render_the_same_bytes_for_the_same_results() {
    assert_eq!(
        recovery_verification(&verified_recovery(), &recovery_results(), 80),
        recovery_verification(&verified_recovery(), &recovery_results(), 80),
        "§50: rendering is deterministic"
    );
}

#[test]
fn should_name_the_recovery_it_verified_in_the_title() {
    let lines = recovery_verification(&verified_recovery(), &recovery_results(), 80);
    assert_eq!(
        lines.first().map(String::as_str),
        Some("RECOVERY VERIFICATION / r91c"),
        "§25.3: the verified scope belongs to one recovery, and the title says which"
    );
}

#[test]
fn should_not_claim_the_persistent_scope_when_a_restored_object_was_never_checked() {
    let plan = rewritten(
        &selective_recovery_restoring_three(),
        "id",
        s("r91c4d2e8b6a3f150"),
    );
    let lines = recovery_verification(&plan, &recovery_results(), 80);
    assert!(
        lines
            .iter()
            .any(|line| line.contains("/etc/hosts") && line.contains("NOT VERIFIED")),
        "§25.1: an object the recovery restored and nothing checked is not verified. Got {lines:?}"
    );
    assert!(
        contains(&lines, "PERSISTENT STATE NOT VERIFIED"),
        "§25.3: the verified scope is every restored object, so one unchecked object denies the \
         claim. Got {lines:?}"
    );
}

#[test]
fn should_read_the_restored_objects_off_the_change_plan_a_recovery_apply_holds() {
    // After `apply`, the shell holds the recovery's own `ono.change-plan/1`, whose targets are
    // the objects it restores.
    let plan = record(
        "ono.change-plan",
        &[
            ("id", s("r91c4d2e8b6a3f150")),
            ("kind", s("recovery")),
            ("state", s("verifying")),
            (
                "targets",
                ono_value::Value::list([
                    map(&[("label", s("nginx.conf")), ("identity", s("nginx.conf"))]),
                    map(&[("label", s("/etc/hosts")), ("identity", s("/etc/hosts"))]),
                ]),
            ),
        ],
    );
    let lines = recovery_verification(&plan, &recovery_results(), 80);
    assert_eq!(
        lines.first().map(String::as_str),
        Some("RECOVERY VERIFICATION / r91c")
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("/etc/hosts") && line.contains("NOT VERIFIED")),
        "§25.1: the change plan's targets are what the recovery restored. Got {lines:?}"
    );
    assert!(contains(&lines, "PERSISTENT STATE NOT VERIFIED"));
}

/// The selective recovery, restoring one object §25.2's results do not speak about.
fn selective_recovery_restoring_three() -> ono_value::RecordValue {
    rewritten(
        &verified_recovery(),
        "restores",
        list(&["nginx.conf", "package version", "/etc/hosts"]),
    )
}

/// §25.3: a restored object is verified by the check about it, whatever way the check names it.
/// A contract written as `file /etc/app.conf` is about the restored `/etc/app.conf`; reading it as
/// some other object printed a passed restore as NOT VERIFIED.
#[test]
fn should_count_a_check_on_file_path_as_verifying_the_restored_path() {
    let recovery = support::rewritten(
        &verified_recovery(),
        "restores",
        ono_value::Value::list([ono_value::Value::string("/etc/app.conf")]),
    );
    let results = vec![support::equivalence(
        "persistent-state",
        "file /etc/app.conf",
        "restored",
    )];
    let lines = recovery_verification(&recovery, &results, 80);
    assert!(
        contains(&lines, "PERSISTENT STATE VERIFIED"),
        "§25.2: the one restored object was checked and came back, got {lines:#?}"
    );
    assert!(
        !contains(&lines, "NOT VERIFIED"),
        "and it is not listed a second time as unchecked, got {lines:#?}"
    );
}
