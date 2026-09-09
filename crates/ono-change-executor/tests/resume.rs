//! Reading an interrupted apply back and deciding what may continue (spec v0.6 §41.2, §41.3,
//! §55.9 cases 40 and 41).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use common::{PlanSpec, instant, store, stored};
use ono_change_core::{
    ActionStatus, ChangePlan, DriftFinding, DriftVerdict, Idempotency, PlanState, Precondition,
    PreconditionKind,
};
use ono_change_executor::resume::{ResumeDecision, resume, resume_with};
use ono_change_plan::PlanStore;
use ono_value::Value;

/// Writes the state a crash left behind: the first `settled` actions at `status`.
fn crashed_after(
    plan: &ChangePlan,
    store: &PlanStore,
    settled: &[(usize, ActionStatus)],
    now: jiff::Timestamp,
) {
    for (index, status) in settled {
        let action = &plan.actions()[*index];
        store
            .record_action_status(
                plan.id(),
                plan.revision(),
                action.id(),
                action.ordinal(),
                *status,
                now,
                Some("recorded before the shell stopped"),
            )
            .expect("§41.2's records are written as each action settles");
    }
}

// ---- §55.9 case 40: a crash after an idempotent action resumes safely -------------------------

#[test]
fn should_resume_safely_after_a_crash_following_an_idempotent_action() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(
        &PlanSpec::over(3).declaring(Idempotency::Idempotent),
        &store,
        now,
    );
    crashed_after(&plan, &store, &[(0, ActionStatus::Succeeded)], now);

    let outcome = resume(&plan, &store, now);

    assert_eq!(
        outcome.decision(),
        ResumeDecision::Continue,
        "§55.9 case 40: a crash after an idempotent action can resume safely"
    );
    assert!(outcome.may_continue());
    assert_eq!(outcome.completed().len(), 1);
    assert_eq!(
        outcome.resumable().len(),
        2,
        "§41.3: only the actions whose status and idempotency permit it"
    );
    assert!(outcome.refusal().is_none());
}

#[test]
fn should_rerun_an_idempotent_action_whose_outcome_was_never_established() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(
        &PlanSpec::over(2).declaring(Idempotency::Idempotent),
        &store,
        now,
    );
    crashed_after(&plan, &store, &[(0, ActionStatus::Unknown)], now);

    let outcome = resume(&plan, &store, now);

    assert_eq!(
        outcome.decision(),
        ResumeDecision::Continue,
        "§41.1: running an idempotent action twice is running it once"
    );
    assert!(
        outcome.resumable().contains(plan.actions()[0].id()),
        "the unresolved action is rerun rather than left in the air"
    );
    assert_eq!(
        outcome.uncertainty_boundary().len(),
        1,
        "Appendix F.2: it is still recorded as an uncertainty boundary"
    );
}

#[test]
fn should_never_rerun_an_action_that_already_succeeded() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&PlanSpec::over(2), &store, now);
    crashed_after(
        &plan,
        &store,
        &[(0, ActionStatus::Succeeded), (1, ActionStatus::Succeeded)],
        now,
    );

    let outcome = resume(&plan, &store, now);

    assert_eq!(
        outcome.decision(),
        ResumeDecision::AlreadyComplete,
        "there is nothing left to continue"
    );
    assert!(outcome.resumable().is_empty());
    assert_eq!(outcome.completed().len(), 2);
}

// ---- §55.9 case 41: a crash after an unknown or non-idempotent action -------------------------

#[test]
fn should_refuse_to_blindly_retry_after_a_crash_following_an_unknown_action() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(
        &PlanSpec::over(3).declaring(Idempotency::Unknown),
        &store,
        now,
    );
    crashed_after(&plan, &store, &[(0, ActionStatus::Unknown)], now);

    let outcome = resume(&plan, &store, now);

    assert_eq!(
        outcome.decision(),
        ResumeDecision::RequiresRecoveryOrRebase,
        "§55.9 case 41: a crash after an unknown non-idempotent action does not blindly retry"
    );
    assert!(!outcome.may_continue());
    assert_eq!(outcome.blocked().len(), 1);
    assert!(
        outcome.blocked()[0]
            .reason()
            .contains("outcome was never established"),
        "the outcome says why, which is what the operator has to act on"
    );
    assert_eq!(outcome.blocked()[0].status(), ActionStatus::Unknown);
    assert_eq!(outcome.blocked()[0].idempotency(), Idempotency::Unknown);
}

#[test]
fn should_refuse_to_retry_a_non_idempotent_action_that_was_in_flight() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(
        &PlanSpec::over(2).declaring(Idempotency::NonIdempotent),
        &store,
        now,
    );
    crashed_after(&plan, &store, &[(0, ActionStatus::Running)], now);

    let outcome = resume(&plan, &store, now);

    assert_eq!(outcome.decision(), ResumeDecision::RequiresRecoveryOrRebase);
    assert!(
        outcome.blocked()[0].reason().contains("in flight"),
        "§41.2: an action that was running when the shell stopped is not rerun blindly"
    );
}

#[test]
fn should_say_that_a_new_recovery_or_rebase_decision_is_required() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(
        &PlanSpec::over(2).declaring(Idempotency::NonIdempotent),
        &store,
        now,
    );
    crashed_after(&plan, &store, &[(0, ActionStatus::Unknown)], now);

    let outcome = resume(&plan, &store, now);
    let refusal = outcome.refusal().expect("§41.3 refuses");

    assert_eq!(refusal.code().name(), "change.plan_state_invalid");
    assert!(
        refusal
            .help()
            .is_some_and(|help| help.contains("recovery or rebase")),
        "§41.3: otherwise a new recovery or rebase decision is required"
    );
    assert!(refusal.metadata().get("blocked_actions").is_some());
    assert!(refusal.metadata().get("reasons").is_some());
}

#[test]
fn should_retry_a_failed_action_whose_contract_accepts_a_request_token() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(
        &PlanSpec::over(2).declaring(Idempotency::RetrySafeWithToken),
        &store,
        now,
    );
    crashed_after(&plan, &store, &[(0, ActionStatus::Failed)], now);

    let outcome = resume(&plan, &store, now);

    assert_eq!(
        outcome.decision(),
        ResumeDecision::Continue,
        "§41.1: RETRY_SAFE_WITH_TOKEN is safe to retry when the same token is presented"
    );
}

#[test]
fn should_refuse_to_retry_a_failed_non_idempotent_action() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(
        &PlanSpec::over(2).declaring(Idempotency::NonIdempotent),
        &store,
        now,
    );
    crashed_after(&plan, &store, &[(0, ActionStatus::Failed)], now);

    let outcome = resume(&plan, &store, now);

    assert_eq!(outcome.decision(), ResumeDecision::RequiresRecoveryOrRebase);
    assert!(outcome.blocked()[0].reason().contains("§41.1"));
}

// ---- §41.2: the state the records reconstruct to ----------------------------------------------

#[test]
fn should_reconstruct_applying_from_the_persisted_records_after_a_crash() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&PlanSpec::over(3), &store, now);
    crashed_after(&plan, &store, &[(0, ActionStatus::Succeeded)], now);

    let outcome = resume(&plan, &store, now);

    assert_eq!(
        outcome.state(),
        PlanState::Applying,
        "§41.2: plan state MUST be reconstructable from persisted action records"
    );
    assert!(outcome.state().has_mutated());
}

#[test]
fn should_reconstruct_apply_failed_when_a_record_says_an_action_failed() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&PlanSpec::over(3), &store, now);
    crashed_after(
        &plan,
        &store,
        &[(0, ActionStatus::Succeeded), (1, ActionStatus::Failed)],
        now,
    );

    let outcome = resume(&plan, &store, now);

    assert_eq!(outcome.state(), PlanState::ApplyFailed);
}

#[test]
fn should_never_promote_an_unknown_record_to_a_failure() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&PlanSpec::over(2), &store, now);
    crashed_after(&plan, &store, &[(0, ActionStatus::Unknown)], now);

    let outcome = resume(&plan, &store, now);

    assert_eq!(
        outcome.state(),
        PlanState::Applying,
        "Appendix F.2: an unestablished outcome MUST NOT resolve to a failure"
    );
    assert_eq!(outcome.uncertainty_boundary().len(), 1);
}

#[test]
fn should_leave_the_plan_where_it_was_when_nothing_was_ever_recorded() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&PlanSpec::over(2), &store, now);

    let outcome = resume(&plan, &store, now);

    assert_eq!(
        outcome.state(),
        PlanState::Sealed,
        "§41.2: with no records there is nothing to reconstruct, and the seal is what remains"
    );
    assert_eq!(outcome.decision(), ResumeDecision::Continue);
}

#[test]
fn should_reconstruct_verifying_when_every_mutating_action_succeeded() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&PlanSpec::over(2), &store, now);
    crashed_after(
        &plan,
        &store,
        &[(0, ActionStatus::Succeeded), (1, ActionStatus::Succeeded)],
        now,
    );

    let outcome = resume(&plan, &store, now);

    assert_eq!(
        outcome.state(),
        PlanState::Verifying,
        "§4.8: verification starts after mutations complete, and that is where the records leave it"
    );
}

// ---- §41.3 and §7.3: a world that moved while the plan was interrupted ------------------------

#[test]
fn should_refuse_to_resume_a_plan_whose_world_has_drifted() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&PlanSpec::over(3), &store, now);
    crashed_after(&plan, &store, &[(0, ActionStatus::Succeeded)], now);
    let precondition = Precondition::new(
        PreconditionKind::ContentDigest,
        "/etc/nginx/nginx.conf",
        "sha256",
        Value::string("abc123"),
    );
    let drift = vec![DriftFinding::new(
        &precondition,
        DriftVerdict::Material,
        Some(Value::string("def456")),
    )];

    let outcome = resume_with(&plan, &store, now, &drift);

    assert_eq!(
        outcome.decision(),
        ResumeDecision::Drifted,
        "§7.3 and §41.3: the world moved while nobody was looking, so resume refuses"
    );
    assert!(!outcome.may_continue());
    assert_eq!(
        outcome
            .refusal()
            .map(|error| error.code().name().to_owned())
            .unwrap_or_default(),
        "change.plan_drift_detected"
    );
}

#[test]
fn should_resume_when_the_only_drift_was_declared_tolerable() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&PlanSpec::over(3), &store, now);
    crashed_after(&plan, &store, &[(0, ActionStatus::Succeeded)], now);
    let precondition = Precondition::new(
        PreconditionKind::Field,
        "nginx.service",
        "cpu",
        Value::Float(2.0),
    )
    .tolerant();
    let drift = vec![DriftFinding::new(
        &precondition,
        DriftVerdict::Tolerated,
        Some(Value::Float(41.0)),
    )];

    let outcome = resume_with(&plan, &store, now, &drift);

    assert_eq!(
        outcome.decision(),
        ResumeDecision::Continue,
        "§7.4: a change the contract declared harmless does not invalidate the plan"
    );
}

#[test]
fn should_report_the_reconstructed_state_even_when_it_refuses_to_continue() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&PlanSpec::over(3), &store, now);
    crashed_after(&plan, &store, &[(0, ActionStatus::Succeeded)], now);
    let precondition = Precondition::new(
        PreconditionKind::Existence,
        "svc-2",
        "exists",
        Value::Bool(true),
    );
    let drift = vec![DriftFinding::new(
        &precondition,
        DriftVerdict::Unknown,
        None,
    )];

    let outcome = resume_with(&plan, &store, now, &drift);

    assert_eq!(
        outcome.state(),
        PlanState::Applying,
        "the refusal does not erase what the records already established"
    );
    assert_eq!(outcome.completed().len(), 1);
}
