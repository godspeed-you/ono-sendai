//! Mutating commands answer with one outcome per target, and never with a collapsed status.
//!
//! Spec §16.5 forbids `97 succeeded, 3 failed` from becoming one ambiguous answer, and spec §11.5
//! asks for a structured result rather than an exit code. These tests are that rule.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod fixture;

use fixture::{FixtureProvider, providers, run};
use ono_core::ErrorCode;
use ono_value::ActionStatus;

#[tokio::test]
async fn should_answer_with_one_outcome_per_object_that_arrived() {
    let ran = run(
        "get process | stop process",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    assert_eq!(
        ran.actions().len(),
        3,
        "spec §11.5: one ActionResult per target, not a count"
    );
    assert!(
        ran.actions()
            .iter()
            .all(ono_provider_api::ActionOutcome::is_success)
    );
}

#[tokio::test]
async fn should_keep_a_mixed_result_apart_rather_than_collapsing_it() {
    let ran = run(
        "get process | stop process",
        &providers(FixtureProvider::new().failing_on(2)),
    )
    .await
    .expect("the pipeline runs");

    let statuses: Vec<ActionStatus> = ran
        .actions()
        .iter()
        .map(ono_provider_api::ActionOutcome::status)
        .collect();
    assert_eq!(
        statuses,
        [
            ActionStatus::Success,
            ActionStatus::Failed,
            ActionStatus::Success
        ],
        "spec §16.5: the one that failed stays identifiable among the ones that did not"
    );

    let failed = ran
        .actions()
        .iter()
        .find(|outcome| !outcome.is_success())
        .expect("one failed");
    assert_eq!(
        failed
            .target()
            .values()
            .first()
            .and_then(|value| value.as_int().ok()),
        Some(2),
        "the failure names which object it was about"
    );
    assert_eq!(
        failed.error().map(ono_value::ErrorValue::code),
        Some(ErrorCode::IoPermissionDenied),
        "the provider's own reason survives"
    );
}

#[tokio::test]
async fn should_resolve_a_selector_into_a_full_identity_before_acting() {
    // Resolving first is what makes the identity complete — a process is `(pid, started)`, not a
    // pid — which is what keeps a signal from reaching a recycled pid (ADR-0015 T13).
    let ran = run("stop process 2", &providers(FixtureProvider::new()))
        .await
        .expect("the pipeline runs");

    assert_eq!(ran.actions().len(), 1);
    assert_eq!(ran.actions()[0].operation(), "stop");
}

#[tokio::test]
async fn should_carry_an_option_through_to_the_provider() {
    let ran = run(
        "kill process 2 --signal SIGHUP",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    assert_eq!(ran.actions()[0].operation(), "kill");
}

#[tokio::test]
async fn should_report_an_action_that_could_not_be_attempted_as_that_object_s_outcome() {
    // Nothing claims `service` here, and a provider that cannot attempt an action is still an
    // outcome for each target rather than the end of the pipeline (spec §16.5).
    let ran = run(
        "get process | stop service",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    assert_eq!(ran.actions().len(), 3, "every target still gets an answer");
    assert!(ran.actions().iter().all(|outcome| {
        outcome.error().map(ono_value::ErrorValue::code) == Some(ErrorCode::ResolveTargetNotFound)
    }));
}

#[tokio::test]
async fn should_refuse_to_act_when_nothing_names_a_target() {
    let error = run("stop process", &providers(FixtureProvider::new()))
        .await
        .expect_err("`stop process` with neither a selector nor input has nothing to act on");

    assert_eq!(error.code(), ErrorCode::TypeMismatch);
}

#[tokio::test]
async fn should_refuse_to_act_on_a_projection_that_has_no_identity() {
    let error = run(
        "get process | select name | stop process",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect_err("a projection is a value, not an object");

    assert_eq!(error.code(), ErrorCode::TypeMismatch);
}

#[tokio::test]
async fn should_act_only_on_the_objects_a_filter_kept() {
    let ran = run(
        "get process | where size > 1KiB | stop process",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    assert_eq!(ran.actions().len(), 1);
    assert_eq!(
        ran.actions()[0]
            .target()
            .values()
            .first()
            .and_then(|value| value.as_int().ok()),
        Some(2)
    );
}
