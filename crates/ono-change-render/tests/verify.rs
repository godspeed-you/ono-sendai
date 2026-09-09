//! Verification views: §23.4 for a plan, §25.2 for a recovery, and §25.3's prohibition.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_change_core::{EquivalenceDomain, EquivalenceState, RecoveryOutcome, VerificationStatus};
use ono_change_render::{NO_FULL_EQUIVALENCE, recovery_verification, verification_view};

mod support;
use support::{contains, index_of, instant, nginx_results, sealed_nginx_plan};

fn recovered() -> RecoveryOutcome {
    RecoveryOutcome::empty()
        .recording(
            EquivalenceDomain::PersistentState,
            "nginx.conf",
            EquivalenceState::Restored,
        )
        .recording(
            EquivalenceDomain::PersistentState,
            "package version",
            EquivalenceState::Restored,
        )
        .recording(
            EquivalenceDomain::RuntimeState,
            "service state",
            EquivalenceState::Restored,
        )
        .recording(
            EquivalenceDomain::RuntimeState,
            "worker PIDs",
            EquivalenceState::DifferentAsExpected,
        )
        .recording(
            EquivalenceDomain::RuntimeState,
            "TCP connections",
            EquivalenceState::NotRecoverable,
        )
        .recording(
            EquivalenceDomain::ExternalSideEffect,
            "1 webhook request",
            EquivalenceState::NotRecoverable,
        )
}

#[test]
fn should_separate_required_advisory_and_observed_checks() {
    let plan = sealed_nginx_plan();
    let lines = verification_view(plan.id(), &nginx_results(&plan), 80);
    assert!(
        contains(&lines, "required") && contains(&lines, "advisory"),
        "§23.2: a failing required postcondition and a failing advisory one mean different things"
    );
}

#[test]
fn should_print_the_status_of_every_check_it_was_given() {
    let plan = sealed_nginx_plan();
    let lines = verification_view(plan.id(), &nginx_results(&plan), 80);
    assert_eq!(
        lines.iter().filter(|line| line.contains("PASS")).count(),
        3,
        "§23.4's example prints one line per contract, and none of them is folded away"
    );
}

#[test]
fn should_close_with_the_verdict_the_contracts_composed_to() {
    let plan = sealed_nginx_plan();
    let lines = verification_view(plan.id(), &nginx_results(&plan), 80);
    let status = index_of(&lines, "status").expect("§23.4 closes with a status");
    assert_eq!(
        lines[status + 1].trim(),
        "VERIFIED",
        "§4.8 and §50.1: the verdict is composed by the verification set, never by the view"
    );
}

#[test]
fn should_keep_an_unanswerable_check_out_of_the_passing_column() {
    let plan = sealed_nginx_plan();
    let mut results = nginx_results(&plan);
    if let Some(first) = results.first_mut() {
        *first = ono_change_core::VerificationResult::new(
            plan.id().clone(),
            &plan.verification().contracts()[0],
            VerificationStatus::Unknown,
            instant(),
        );
    }
    let lines = verification_view(plan.id(), &results, 80);
    assert!(
        contains(&lines, "UNKNOWN"),
        "§23.5: a check that could not be answered has its own word"
    );
    assert!(
        contains(&lines, "FAILED") || contains(&lines, "DEGRADED"),
        "§23.5 forbids treating an unanswerable required check as success"
    );
}

#[test]
fn should_say_no_contract_was_answered_rather_than_printing_an_empty_view() {
    let plan = sealed_nginx_plan();
    let lines = verification_view(plan.id(), &[], 80);
    assert!(
        contains(&lines, "no contract was answered"),
        "§10.5: an empty verification view and a verified plan must not look the same"
    );
}

#[test]
fn should_report_the_three_equivalence_domains_separately() {
    let lines = recovery_verification(&recovered(), 80);
    for domain in ["persistent state", "runtime", "external side effects"] {
        assert!(
            contains(&lines, domain),
            "§25.1: recovery verification MUST distinguish `{domain}` from the others"
        );
    }
}

#[test]
fn should_call_a_new_worker_identity_a_difference_that_was_expected() {
    let lines = recovery_verification(&recovered(), 80);
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
    let lines = recovery_verification(&recovered(), 80);
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
    let lines = recovery_verification(&recovered(), 80);
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
    let outcome = RecoveryOutcome::empty().recording(
        EquivalenceDomain::PersistentState,
        "nginx.conf",
        EquivalenceState::NotRestored,
    );
    let lines = recovery_verification(&outcome, 80);
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
    let lines = recovery_verification(&recovered(), 80);
    assert!(
        contains(&lines, "OUTSIDE ANY RECOVERY"),
        "§35.2 and §62.2: a webhook already delivered is the reason there is no universal undo"
    );
}

#[test]
fn should_say_a_domain_was_not_observed_rather_than_leaving_it_out() {
    let outcome = RecoveryOutcome::empty().recording(
        EquivalenceDomain::PersistentState,
        "nginx.conf",
        EquivalenceState::Restored,
    );
    let lines = recovery_verification(&outcome, 80);
    assert!(
        contains(&lines, "nothing was observed in this domain"),
        "§25.1: a domain nobody looked at is a fact, and an omitted heading reads as a pass"
    );
}

#[test]
fn should_render_the_same_bytes_for_the_same_outcome() {
    assert_eq!(
        recovery_verification(&recovered(), 80),
        recovery_verification(&recovered(), 80),
        "§50: rendering is deterministic"
    );
}
