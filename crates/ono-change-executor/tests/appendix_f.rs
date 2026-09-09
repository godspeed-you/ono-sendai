//! Appendix F's apply-time failure matrix, one test per row, plus F.1 and F.2 (spec v0.6).
//!
//! Every test asserts the three facts the matrix's columns fix: the resulting `PlanState`, whether
//! mutation occurred, and what the operator is told. The third is not decoration — Appendix F is a
//! contract about what a person is able to conclude after a failure, and a correct state with a
//! misleading message fails the requirement.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use common::{
    FakeQuiesce, FakeRecoveryProvider, PlanSpec, ProviderScript, Script, empty_registry,
    five_snapshots, instant, material_drift, no_drift, observing, protection, registry,
    sealed_plan, store, stored, timing_out,
};
use ono_change_core::{
    ActionStatus, ChangeCapability, PlanState, RecoveryCapability, VerificationStatus,
};
use ono_change_executor::execute::{
    ApplyRequest, Authority, CleanupDecision, CloseRequest, FailurePoint, PrepareRequest,
    Quiescing, apply, close, prepare,
};

// ---- row 1: target revalidation | no | SEALED | fail, no prepare -----------------------------

#[test]
fn should_refuse_before_preparing_anything_when_a_frozen_fact_moved() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&PlanSpec::default(), &store, now);
    let providers = registry(FakeRecoveryProvider::healthy());
    let protection = vec![protection(&plan, common::PROVIDER, "tank/data", now)];
    let drift = material_drift();
    let script = Script::healthy();
    let execute = script.execute();
    let observe = observing(VerificationStatus::Passed);
    let mut request = ApplyRequest::new(
        &plan,
        &store,
        "session-a",
        now,
        &protection,
        &providers,
        &drift,
        &execute,
        &observe,
    );

    let outcome = apply(&mut request);

    assert_eq!(
        outcome.state(),
        PlanState::Sealed,
        "Appendix F: a target revalidation failure leaves the plan SEALED"
    );
    assert!(
        !outcome.has_mutated(),
        "Appendix F: no mutation occurred, so nothing may say one did"
    );
    assert_eq!(
        outcome.failure_point(),
        Some(FailurePoint::TargetRevalidation)
    );
    assert!(
        outcome.assets().is_empty(),
        "§7.3: material drift stops execution before anything is prepared"
    );
    assert_eq!(
        outcome
            .error()
            .map(|error| error.code().name().to_owned())
            .unwrap_or_default(),
        "change.plan_drift_detected",
        "the operator is told the plan was resolved against state that has since changed"
    );
    assert!(
        script.calls().is_empty(),
        "§7.3: nothing is executed once revalidation refuses"
    );
}

#[test]
fn should_refuse_when_a_precondition_could_not_be_observed_at_all() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&PlanSpec::default(), &store, now);
    let providers = empty_registry();
    let protection = Vec::new();
    let drift = |_action: &ono_change_core::PlanAction| {
        Err(ono_change_core::error::tool_failed(
            "/usr/bin/systemctl",
            "the unit could not be queried",
        ))
    };
    let script = Script::healthy();
    let execute = script.execute();
    let observe = observing(VerificationStatus::Passed);
    let mut request = ApplyRequest::new(
        &plan,
        &store,
        "session-a",
        now,
        &protection,
        &providers,
        &drift,
        &execute,
        &observe,
    );

    let outcome = apply(&mut request);

    assert_eq!(
        outcome.state(),
        PlanState::Sealed,
        "§2.4: a check nobody could make has not held, and unknown is not promoted to expected"
    );
    assert_eq!(
        outcome.failure_point(),
        Some(FailurePoint::TargetRevalidation)
    );
}

// ---- row 2: privilege check | no | SEALED | fail, no prepare ---------------------------------

#[test]
fn should_refuse_before_preparing_anything_when_the_session_cannot_execute_actions() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&PlanSpec::default(), &store, now);
    let providers = registry(FakeRecoveryProvider::healthy());
    let protection = vec![protection(&plan, common::PROVIDER, "tank/data", now)];
    let drift = no_drift();
    let script = Script::healthy();
    let execute = script.execute();
    let observe = observing(VerificationStatus::Passed);
    let mut request = ApplyRequest::new(
        &plan,
        &store,
        "session-a",
        now,
        &protection,
        &providers,
        &drift,
        &execute,
        &observe,
    )
    .with_authority(Authority::full().without_change(ChangeCapability::ActionExecute));

    let outcome = apply(&mut request);

    assert_eq!(
        outcome.state(),
        PlanState::Sealed,
        "Appendix F: a privilege check failure leaves the plan SEALED"
    );
    assert!(!outcome.has_mutated());
    assert_eq!(outcome.failure_point(), Some(FailurePoint::PrivilegeCheck));
    assert!(
        outcome.assets().is_empty(),
        "Appendix F: fail, no prepare — the check comes before asset creation"
    );
    assert_eq!(
        outcome
            .error()
            .map(|error| error.code().name().to_owned())
            .unwrap_or_default(),
        "change.capability_missing",
        "§43.2: a session without `change.action.execute` was never granted it, and elevation \
         cannot supply a capability"
    );
}

#[test]
fn should_refuse_when_an_action_needs_elevation_the_session_does_not_hold() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&PlanSpec::default().privileged(), &store, now);
    let providers = empty_registry();
    let protection = Vec::new();
    let drift = no_drift();
    let script = Script::healthy();
    let execute = script.execute();
    let observe = observing(VerificationStatus::Passed);
    let mut request = ApplyRequest::new(
        &plan,
        &store,
        "session-a",
        now,
        &protection,
        &providers,
        &drift,
        &execute,
        &observe,
    )
    .with_authority(Authority::full().unprivileged());

    let outcome = apply(&mut request);

    assert_eq!(outcome.state(), PlanState::Sealed);
    assert_eq!(outcome.failure_point(), Some(FailurePoint::PrivilegeCheck));
    assert!(
        script.calls().is_empty(),
        "§43.3: the plan shows which actions require privilege, and none of them ran"
    );
}

#[test]
fn should_refuse_when_the_session_cannot_create_the_protection_the_plan_requires() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&PlanSpec::default(), &store, now);
    let providers = registry(FakeRecoveryProvider::healthy());
    let protection = vec![protection(&plan, common::PROVIDER, "tank/data", now)];
    let drift = no_drift();
    let script = Script::healthy();
    let execute = script.execute();
    let observe = observing(VerificationStatus::Passed);
    let mut request = ApplyRequest::new(
        &plan,
        &store,
        "session-a",
        now,
        &protection,
        &providers,
        &drift,
        &execute,
        &observe,
    )
    .with_authority(Authority::full().without_recovery(RecoveryCapability::Prepare));

    let outcome = apply(&mut request);

    assert_eq!(
        outcome.state(),
        PlanState::Sealed,
        "§43.2: applying requires target action capabilities plus protection provider capabilities"
    );
    assert!(outcome.assets().is_empty());
}

// ---- row 3: recovery discovery | no | SEALED | fail if policy requires -----------------------

#[test]
fn should_refuse_a_require_policy_that_found_no_protection_at_all() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let spec = PlanSpec::default().under(ono_change_core::ProtectionMode::Require);
    let plan = stored(&spec, &store, now);
    let providers = empty_registry();
    let protection = Vec::new();
    let drift = no_drift();
    let script = Script::healthy();
    let execute = script.execute();
    let observe = observing(VerificationStatus::Passed);
    let mut request = ApplyRequest::new(
        &plan,
        &store,
        "session-a",
        now,
        &protection,
        &providers,
        &drift,
        &execute,
        &observe,
    );

    let outcome = apply(&mut request);

    assert_eq!(
        outcome.state(),
        PlanState::Sealed,
        "Appendix F: recovery discovery fails with the plan SEALED and nothing prepared"
    );
    assert!(!outcome.has_mutated());
    assert_eq!(
        outcome.failure_point(),
        Some(FailurePoint::RecoveryDiscovery)
    );
    assert_eq!(
        outcome
            .error()
            .map(|error| error.code().name().to_owned())
            .unwrap_or_default(),
        "recovery.coverage_insufficient",
        "§17.2: `require` refuses to apply when a mutation domain cannot reach the class"
    );
}

#[test]
fn should_apply_a_prefer_policy_that_found_no_protection() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&PlanSpec::default(), &store, now);
    let providers = empty_registry();
    let protection = Vec::new();
    let drift = no_drift();
    let script = Script::healthy();
    let execute = script.execute();
    let observe = observing(VerificationStatus::Passed);
    let mut request = ApplyRequest::new(
        &plan,
        &store,
        "session-a",
        now,
        &protection,
        &providers,
        &drift,
        &execute,
        &observe,
    );

    let outcome = apply(&mut request);

    assert_eq!(
        outcome.state(),
        PlanState::Verified,
        "§17.2: only `require` refuses a shortfall, and Appendix F's row says so"
    );
}

// ---- row 4: recovery asset creation | no | PREPARE_FAILED | retain siblings ------------------

#[test]
fn should_reach_prepare_failed_without_mutating_when_an_asset_cannot_be_created() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&PlanSpec::default(), &store, now);
    let providers = registry(FakeRecoveryProvider::with_script(
        ProviderScript::CreateFails,
    ));
    let protection = vec![protection(&plan, common::PROVIDER, "tank/data", now)];
    let drift = no_drift();
    let script = Script::healthy();
    let execute = script.execute();
    let observe = observing(VerificationStatus::Passed);
    let mut request = ApplyRequest::new(
        &plan,
        &store,
        "session-a",
        now,
        &protection,
        &providers,
        &drift,
        &execute,
        &observe,
    );

    let outcome = apply(&mut request);

    assert_eq!(
        outcome.state(),
        PlanState::PrepareFailed,
        "Appendix F: an asset that could not be created reaches PREPARE_FAILED"
    );
    assert!(
        !outcome.has_mutated(),
        "§2.3: if a required recovery asset cannot be created, mutation MUST NOT begin"
    );
    assert_eq!(
        outcome.failure_point(),
        Some(FailurePoint::RecoveryAssetCreation)
    );
    assert!(
        script.calls().is_empty(),
        "§55.7 case 31: a required PREPARE failure means zero mutate actions execute"
    );
    assert_eq!(
        outcome
            .error()
            .map(|error| error.code().name().to_owned())
            .unwrap_or_default(),
        "change.prepare_failed"
    );
}

#[test]
fn should_retain_the_sibling_assets_a_failed_prepare_had_already_created() {
    let now = instant(1_000);
    let plan = sealed_plan(now);
    let provider = FakeRecoveryProvider::healthy();
    let providers = registry(provider);
    let mut protection = five_snapshots(&plan, common::PROVIDER, now);
    // The fifth is owned by a provider nobody registered, so its creation is the one that fails.
    protection[4] = common::protection(&plan, "ono.recovery.absent", "tank/data5", now);
    let mut request = PrepareRequest::new(&plan, &protection, &providers, now);

    let refusal = prepare(&mut request).expect_err("the fifth asset cannot be created");

    assert_eq!(
        request.created().len(),
        4,
        "Appendix F.1: four of five snapshots were created"
    );
    assert_eq!(
        request.retained().len(),
        4,
        "Appendix F: already-created sibling assets are retained until a cleanup decision"
    );
    assert_eq!(refusal.code().name(), "change.prepare_failed");
    assert_eq!(
        request.failure_point(),
        Some(FailurePoint::RecoveryAssetCreation)
    );
}

// ---- row 5: recovery validation | no | PREPARE_FAILED | mark invalid, no mutation ------------

#[test]
fn should_mark_the_asset_invalid_and_mutate_nothing_when_validation_fails() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&PlanSpec::default(), &store, now);
    let providers = registry(FakeRecoveryProvider::with_script(
        ProviderScript::ValidatesWrongScope,
    ));
    let protection = vec![protection(&plan, common::PROVIDER, "tank/data", now)];
    let drift = no_drift();
    let script = Script::healthy();
    let execute = script.execute();
    let observe = observing(VerificationStatus::Passed);
    let mut request = ApplyRequest::new(
        &plan,
        &store,
        "session-a",
        now,
        &protection,
        &providers,
        &drift,
        &execute,
        &observe,
    );

    let outcome = apply(&mut request);

    assert_eq!(
        outcome.state(),
        PlanState::PrepareFailed,
        "Appendix F: a failed validation reaches PREPARE_FAILED"
    );
    assert!(!outcome.has_mutated());
    assert_eq!(
        outcome.failure_point(),
        Some(FailurePoint::RecoveryValidation)
    );
    assert_eq!(
        outcome
            .assets()
            .first()
            .map(ono_change_core::RecoveryAsset::state),
        Some(ono_change_core::AssetState::Invalid),
        "Appendix F: the asset is marked invalid rather than quietly dropped"
    );
    assert!(
        script.calls().is_empty(),
        "Appendix F: no mutation, because §11.4's scope check did not pass"
    );
}

#[test]
fn should_mark_the_asset_invalid_when_the_validation_check_could_not_be_made() {
    let now = instant(1_000);
    let plan = sealed_plan(now);
    let providers = registry(FakeRecoveryProvider::with_script(
        ProviderScript::ValidationUnavailable,
    ));
    let protection = vec![protection(&plan, common::PROVIDER, "tank/data", now)];
    let mut request = PrepareRequest::new(&plan, &protection, &providers, now);

    let refusal = prepare(&mut request).expect_err("an unvalidatable asset is not protection");

    assert_eq!(
        request
            .created()
            .first()
            .map(ono_change_core::RecoveryAsset::state),
        Some(ono_change_core::AssetState::Invalid),
        "§11.4: an asset is usable only once every check has actually been made"
    );
    assert_eq!(refusal.code().name(), "change.prepare_failed");
}

// ---- row 6: application quiesce | maybe runtime-only | PREPARE_FAILED ------------------------

#[test]
fn should_reach_prepare_failed_and_resume_the_application_when_quiesce_fails() {
    let now = instant(1_000);
    let plan = sealed_plan(now);
    let providers = registry(FakeRecoveryProvider::healthy());
    let protection = vec![protection(&plan, common::PROVIDER, "tank/data", now)];
    let hook = FakeQuiesce::failing_pause();
    let mut request = PrepareRequest::new(&plan, &protection, &providers, now).quiescing(
        Quiescing::new("postgresql", std::time::Duration::from_secs(5), &hook),
    );

    let refusal = prepare(&mut request).expect_err("an application that will not pause");

    assert_eq!(refusal.code().name(), "recovery.quiesce_failed");
    assert_eq!(
        request.failure_point(),
        Some(FailurePoint::ApplicationQuiesce)
    );
    assert!(
        request.created().is_empty(),
        "§18.4: nothing was mutated, and no asset was taken over a running application"
    );
}

#[test]
fn should_record_the_critical_result_when_a_quiesced_application_will_not_resume() {
    let now = instant(1_000);
    let plan = sealed_plan(now);
    let providers = registry(FakeRecoveryProvider::healthy());
    let protection = vec![protection(&plan, common::PROVIDER, "tank/data", now)];
    let hook = FakeQuiesce::failing_resume();
    let mut request = PrepareRequest::new(&plan, &protection, &providers, now).quiescing(
        Quiescing::new("postgresql", std::time::Duration::from_secs(5), &hook),
    );

    let refusal = prepare(&mut request).expect_err("§18.4 makes a failed resume critical");

    assert_eq!(
        refusal.code().name(),
        "recovery.resume_failed",
        "§18.4: failure to resume is a critical error surfaced separately"
    );
    let report = request.quiesce_report().expect("the window was opened");
    assert!(report.was_paused());
    assert!(!report.was_resumed());
    assert!(
        report.critical().is_some(),
        "§18.4: the critical result is recorded, not folded into another message"
    );
    assert_eq!(
        hook.log(),
        vec!["pause", "resume"],
        "§18.4 bounds the window"
    );
}

#[test]
fn should_close_the_quiesce_window_when_preparation_succeeded() {
    let now = instant(1_000);
    let plan = sealed_plan(now);
    let providers = registry(FakeRecoveryProvider::healthy());
    let protection = vec![protection(&plan, common::PROVIDER, "tank/data", now)];
    let hook = FakeQuiesce::healthy();
    let mut request = PrepareRequest::new(&plan, &protection, &providers, now).quiescing(
        Quiescing::new("postgresql", std::time::Duration::from_secs(5), &hook),
    );

    let prepared = prepare(&mut request).expect("a healthy application quiesces and resumes");

    assert_eq!(prepared.state(), PlanState::Protected);
    assert!(
        prepared
            .quiesce()
            .is_some_and(ono_change_executor::execute::QuiesceReport::was_resumed),
        "§18.4: the window is bounded and the application is put back"
    );
}

// ---- row 7: first mutate action | yes/unknown | APPLY_FAILED ---------------------------------

#[test]
fn should_stop_the_dependency_chain_when_the_first_mutating_action_fails() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let spec = PlanSpec::over(3).chained();
    let plan = stored(&spec, &store, now);
    let providers = empty_registry();
    let protection = Vec::new();
    let drift = no_drift();
    let script = Script::healthy().failing("svc-1");
    let execute = script.execute();
    let observe = observing(VerificationStatus::Passed);
    let mut request = ApplyRequest::new(
        &plan,
        &store,
        "session-a",
        now,
        &protection,
        &providers,
        &drift,
        &execute,
        &observe,
    );

    let outcome = apply(&mut request);

    assert_eq!(
        outcome.state(),
        PlanState::ApplyFailed,
        "Appendix F: a failing first mutate action reaches APPLY_FAILED"
    );
    assert!(
        outcome.has_mutated(),
        "Appendix F: from APPLYING onwards the operator is told something may have happened"
    );
    assert_eq!(
        outcome.failure_point(),
        Some(FailurePoint::FirstMutateAction)
    );
    assert_eq!(
        script.calls(),
        vec!["svc-1".to_owned()],
        "Appendix F: stop the dependency chain — svc-2 and svc-3 never ran"
    );
    let skipped = outcome
        .statuses()
        .iter()
        .filter(|(_, status)| *status == ActionStatus::Skipped)
        .count();
    assert_eq!(
        skipped, 2,
        "Appendix F: what was blocked is SKIPPED, which is not FAILED"
    );
    assert_eq!(
        outcome
            .error()
            .map(|error| error.code().name().to_owned())
            .unwrap_or_default(),
        "change.apply_failed"
    );
}

#[test]
fn should_preserve_the_evidence_of_what_ran_when_a_mutating_action_fails() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let spec = PlanSpec::over(3).chained();
    let plan = stored(&spec, &store, now);
    let providers = empty_registry();
    let protection = Vec::new();
    let drift = no_drift();
    let script = Script::healthy().failing("svc-1");
    let execute = script.execute();
    let observe = observing(VerificationStatus::Passed);
    let mut request = ApplyRequest::new(
        &plan,
        &store,
        "session-a",
        now,
        &protection,
        &providers,
        &drift,
        &execute,
        &observe,
    );

    let _ = apply(&mut request);
    let persisted = store
        .action_statuses(plan.id(), plan.revision())
        .expect("§41.2's records were written as the action settled");

    assert_eq!(
        persisted.values().copied().collect::<Vec<_>>(),
        vec![ActionStatus::Failed],
        "§4.7: every action result MUST be recorded independently"
    );
}

// ---- row 8: middle mutate action | yes | APPLY_FAILED | offer a RecoveryPlan -----------------

#[test]
fn should_offer_recovery_rather_than_reversing_when_a_later_action_fails() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let spec = PlanSpec::over(3).chained();
    let plan = stored(&spec, &store, now);
    let providers = empty_registry();
    let protection = Vec::new();
    let drift = no_drift();
    let script = Script::healthy().failing("svc-2");
    let execute = script.execute();
    let observe = observing(VerificationStatus::Passed);
    let mut request = ApplyRequest::new(
        &plan,
        &store,
        "session-a",
        now,
        &protection,
        &providers,
        &drift,
        &execute,
        &observe,
    );

    let outcome = apply(&mut request);

    assert_eq!(outcome.state(), PlanState::ApplyFailed);
    assert!(outcome.has_mutated());
    assert_eq!(
        outcome.failure_point(),
        Some(FailurePoint::MiddleMutateAction),
        "Appendix F distinguishes the middle action, because earlier ones already changed things"
    );
    assert!(
        outcome.offers_recovery(),
        "Appendix F: offer a RecoveryPlan"
    );
    assert_eq!(
        script.calls(),
        vec!["svc-1".to_owned(), "svc-2".to_owned()],
        "Appendix F: do not blindly reverse — nothing ran backwards over svc-1"
    );
    assert_eq!(
        outcome.status_of(plan.actions()[0].id()),
        Some(ActionStatus::Succeeded),
        "the exact partial state is preserved: svc-1 succeeded and stays succeeded"
    );
}

// ---- row 9: remote disconnect | unknown | APPLYING/UNKNOWN -----------------------------------

#[test]
fn should_leave_the_plan_applying_and_the_action_unknown_when_a_remote_link_drops() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let spec = PlanSpec::over(2).on_hosts(&["api-04", "api-05"]).chained();
    let plan = stored(&spec, &store, now);
    let providers = empty_registry();
    let protection = Vec::new();
    let drift = no_drift();
    let script = Script::healthy().unknown("svc-1");
    let execute = script.execute();
    let observe = observing(VerificationStatus::Passed);
    let mut request = ApplyRequest::new(
        &plan,
        &store,
        "session-a",
        now,
        &protection,
        &providers,
        &drift,
        &execute,
        &observe,
    );

    let outcome = apply(&mut request);

    assert_eq!(
        outcome.state(),
        PlanState::Applying,
        "Appendix F: a remote disconnect leaves the plan APPLYING with an UNKNOWN inside it"
    );
    assert_eq!(
        outcome.failure_point(),
        Some(FailurePoint::RemoteDisconnect)
    );
    assert_eq!(
        outcome.status_of(plan.actions()[0].id()),
        Some(ActionStatus::Unknown),
        "§29.3: an unknown remote action is not marked failed or successful without evidence"
    );
    assert_eq!(
        outcome.uncertainty_boundary().len(),
        1,
        "Appendix F.2: the unresolved action is an uncertainty boundary"
    );
    assert_eq!(
        outcome
            .error()
            .map(|error| error.code().name().to_owned())
            .unwrap_or_default(),
        "change.remote_state_unknown",
        "the refusal says the link can be queried again when it returns"
    );
    assert_eq!(
        outcome.error().and_then(ono_value::ErrorValue::retryable),
        Some(true),
        "§29.3: querying again when the link returns is the way to resolve it"
    );
}

// ---- row 10: verification required failed | yes | FAILED -------------------------------------

#[test]
fn should_fail_the_plan_and_keep_its_protection_when_a_required_check_fails() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&PlanSpec::default(), &store, now);
    let providers = registry(FakeRecoveryProvider::healthy());
    let protection = vec![protection(&plan, common::PROVIDER, "tank/data", now)];
    let drift = no_drift();
    let script = Script::healthy();
    let execute = script.execute();
    let observe = observing(VerificationStatus::Failed);
    let mut request = ApplyRequest::new(
        &plan,
        &store,
        "session-a",
        now,
        &protection,
        &providers,
        &drift,
        &execute,
        &observe,
    );

    let outcome = apply(&mut request);

    assert_eq!(
        outcome.state(),
        PlanState::Failed,
        "§55.7 case 32: mutation succeeded and required verification failed, so the plan FAILED"
    );
    assert!(outcome.has_mutated());
    assert_eq!(
        outcome.failure_point(),
        Some(FailurePoint::RequiredVerification)
    );
    assert_eq!(
        outcome.retained_assets().len(),
        1,
        "Appendix F: preserve protection — §37.2 keeps a failed plan's assets"
    );
    assert!(
        outcome.offers_recovery(),
        "Appendix F: offer investigation or recovery"
    );
    assert_eq!(
        outcome
            .error()
            .map(|error| error.code().name().to_owned())
            .unwrap_or_default(),
        "change.verification_failed"
    );
}

#[test]
fn should_degrade_the_plan_when_only_an_advisory_check_fails() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let spec =
        PlanSpec::default().checking(ono_change_core::VerificationClass::Advisory, "worker count");
    let plan = stored(&spec, &store, now);
    let providers = empty_registry();
    let protection = Vec::new();
    let drift = no_drift();
    let script = Script::healthy();
    let execute = script.execute();
    let observe = common::observing_only("worker count", VerificationStatus::Failed);
    let mut request = ApplyRequest::new(
        &plan,
        &store,
        "session-a",
        now,
        &protection,
        &providers,
        &drift,
        &execute,
        &observe,
    );

    let outcome = apply(&mut request);

    assert_eq!(
        outcome.state(),
        PlanState::Degraded,
        "§55.7 case 33: an advisory failure makes the plan DEGRADED"
    );
}

// ---- row 11: verification timeout | yes | DEGRADED or FAILED per contract --------------------

#[test]
fn should_never_read_a_verification_timeout_as_success() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&PlanSpec::default(), &store, now);
    let providers = empty_registry();
    let protection = Vec::new();
    let drift = no_drift();
    let script = Script::healthy();
    let execute = script.execute();
    let observe = timing_out();
    let mut request = ApplyRequest::new(
        &plan,
        &store,
        "session-a",
        now,
        &protection,
        &providers,
        &drift,
        &execute,
        &observe,
    );

    let outcome = apply(&mut request);

    assert_ne!(
        outcome.state(),
        PlanState::Verified,
        "§23.5 and Appendix F: a timeout MUST NOT be treated as success"
    );
    assert_eq!(
        outcome.state(),
        PlanState::Failed,
        "a required contract whose default says a timeout is a failure produces FAILED"
    );
    assert_eq!(
        outcome.failure_point(),
        Some(FailurePoint::VerificationTimeout)
    );
    assert!(outcome.has_mutated());
}

#[test]
fn should_degrade_rather_than_fail_when_the_contract_says_a_timeout_is_unknown() {
    let now = instant(1_000);
    let plan = PlanSpec::default().seal(now);
    let contracts: Vec<ono_change_core::VerificationContract> = plan
        .verification()
        .contracts()
        .iter()
        .map(|contract| contract.clone().timeout_is_unknown())
        .collect();
    let plan = plan
        .revise()
        .with_verification(ono_change_core::VerificationSet::of(contracts))
        .seal(now)
        .expect("re-seals");
    let observe = timing_out();
    let outcome =
        ono_change_executor::execute::verify(&ono_change_executor::execute::VerifyRequest {
            plan: &plan,
            now,
            observe: &observe,
        });

    assert_eq!(
        outcome.state(),
        PlanState::Degraded,
        "Appendix F: DEGRADED or FAILED per contract, and the contract chose UNKNOWN"
    );
    assert_eq!(
        outcome.timed_out().len(),
        1,
        "§23.5: the checks that ran out of time are nameable"
    );
}

// ---- rows 12 and 13: recovery action and recovery verification -------------------------------

#[test]
fn should_reach_recovery_failed_and_preserve_the_partial_state_when_a_recovery_action_fails() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let spec = PlanSpec::over(3).recovering().chained();
    let plan = stored(&spec, &store, now);
    let providers = empty_registry();
    let protection = Vec::new();
    let drift = no_drift();
    let script = Script::healthy().failing("svc-2");
    let execute = script.execute();
    let observe = observing(VerificationStatus::Passed);
    let mut request = ApplyRequest::new(
        &plan,
        &store,
        "session-a",
        now,
        &protection,
        &providers,
        &drift,
        &execute,
        &observe,
    );

    let outcome = apply(&mut request);

    assert_eq!(
        outcome.state(),
        PlanState::RecoveryFailed,
        "Appendix F: a failing recovery action reaches RECOVERY_FAILED"
    );
    assert_eq!(
        outcome.status_of(plan.actions()[0].id()),
        Some(ActionStatus::Succeeded),
        "Appendix F and §55.8 case 39: the exact partial state is preserved"
    );
    assert_eq!(
        outcome.status_of(plan.actions()[2].id()),
        Some(ActionStatus::Skipped),
        "what never ran is SKIPPED, not failed"
    );
    assert_eq!(
        outcome
            .error()
            .map(|error| error.code().name().to_owned())
            .unwrap_or_default(),
        "recovery.apply_failed"
    );
}

#[test]
fn should_refuse_to_claim_recovery_when_its_verification_did_not_hold() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let spec = PlanSpec::default().recovering();
    let plan = stored(&spec, &store, now);
    let providers = empty_registry();
    let protection = Vec::new();
    let drift = no_drift();
    let script = Script::healthy();
    let execute = script.execute();
    let observe = observing(VerificationStatus::Failed);
    let mut request = ApplyRequest::new(
        &plan,
        &store,
        "session-a",
        now,
        &protection,
        &providers,
        &drift,
        &execute,
        &observe,
    );

    let outcome = apply(&mut request);

    assert_eq!(
        outcome.state(),
        PlanState::RecoveryFailed,
        "Appendix F: recovery verification failed, so nothing may claim the state was recovered"
    );
    assert!(
        !outcome.is_success(),
        "§25.3: 'rollback successful' is exactly the sentence this forbids"
    );
    assert_eq!(
        outcome
            .error()
            .map(|error| error.code().name().to_owned())
            .unwrap_or_default(),
        "recovery.verification_failed"
    );
}

#[test]
fn should_reach_recovery_verified_when_a_recovery_plan_verifies() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let spec = PlanSpec::default().recovering();
    let plan = stored(&spec, &store, now);
    let providers = empty_registry();
    let protection = Vec::new();
    let drift = no_drift();
    let script = Script::healthy();
    let execute = script.execute();
    let observe = observing(VerificationStatus::Passed);
    let mut request = ApplyRequest::new(
        &plan,
        &store,
        "session-a",
        now,
        &protection,
        &providers,
        &drift,
        &execute,
        &observe,
    );

    let outcome = apply(&mut request);

    assert_eq!(outcome.state(), PlanState::RecoveryVerified);
    assert!(outcome.is_success());
}

// ---- row 14: cleanup fails | no new target mutation | CLOSED_WITH_ASSETS ---------------------

#[test]
fn should_close_with_the_asset_still_there_when_cleanup_fails() {
    let now = instant(1_000);
    let plan = PlanSpec::default()
        .seal(now)
        .advance(ono_change_core::LifecycleEvent::BeginPrepare)
        .and_then(|plan| plan.advance(ono_change_core::LifecycleEvent::Protected))
        .and_then(|plan| plan.advance(ono_change_core::LifecycleEvent::BeginApply))
        .and_then(|plan| plan.advance(ono_change_core::LifecycleEvent::BeginVerify))
        .and_then(|plan| plan.advance(ono_change_core::LifecycleEvent::Verified))
        .expect("§4.1 walks to VERIFIED");
    let providers = registry(FakeRecoveryProvider::with_script(
        ProviderScript::CleanupFails,
    ));
    let asset = common::protection(&plan, common::PROVIDER, "tank/data", now)
        .proposed_asset()
        .clone();
    let assets = vec![asset.clone()];
    let mut request = CloseRequest::new(&plan, &providers, &assets).removing_assets();

    let outcome = close(&mut request);

    assert_eq!(
        outcome.state(),
        PlanState::Closed,
        "Appendix F: a cleanup failure still closes the plan — no new target mutation happened"
    );
    assert!(
        outcome.is_closed_with_assets(),
        "Appendix F: CLOSED_WITH_ASSETS, and the retained asset is surfaced"
    );
    assert_eq!(outcome.retained(), &[asset.id().clone()]);
    assert_eq!(
        outcome
            .failures()
            .first()
            .map(|error| error.code().name().to_owned())
            .unwrap_or_default(),
        "recovery.cleanup_blocked",
        "§37.4: the operator is told which asset is still occupying storage"
    );
    assert_eq!(outcome.failure_point(), Some(FailurePoint::Cleanup));
}

#[test]
fn should_close_a_plan_without_removing_its_assets_by_default() {
    let now = instant(1_000);
    let plan = PlanSpec::default()
        .seal(now)
        .advance(ono_change_core::LifecycleEvent::BeginPrepare)
        .and_then(|plan| plan.advance(ono_change_core::LifecycleEvent::Protected))
        .and_then(|plan| plan.advance(ono_change_core::LifecycleEvent::BeginApply))
        .and_then(|plan| plan.advance(ono_change_core::LifecycleEvent::BeginVerify))
        .and_then(|plan| plan.advance(ono_change_core::LifecycleEvent::Verified))
        .expect("§4.1 walks to VERIFIED");
    let providers = registry(FakeRecoveryProvider::healthy());
    let assets = vec![
        common::protection(&plan, common::PROVIDER, "tank/data", now)
            .proposed_asset()
            .clone(),
    ];
    let mut request = CloseRequest::new(&plan, &providers, &assets);

    let outcome = close(&mut request);

    assert_eq!(outcome.state(), PlanState::Closed);
    assert!(
        outcome.removed().is_empty(),
        "§4.9: closing a plan does not necessarily remove recovery assets"
    );
}

// ---- Appendix F.1 --------------------------------------------------------------------------

#[test]
fn should_not_mutate_the_plan_targets_when_the_fifth_of_five_snapshots_fails() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&PlanSpec::default(), &store, now);
    let providers = registry(FakeRecoveryProvider::healthy());
    let mut protection = five_snapshots(&plan, common::PROVIDER, now);
    protection[4] = common::protection(&plan, "ono.recovery.absent", "tank/data5", now);
    let drift = no_drift();
    let script = Script::healthy();
    let execute = script.execute();
    let observe = observing(VerificationStatus::Passed);
    let mut request = ApplyRequest::new(
        &plan,
        &store,
        "session-a",
        now,
        &protection,
        &providers,
        &drift,
        &execute,
        &observe,
    );

    let outcome = apply(&mut request);

    assert_eq!(outcome.state(), PlanState::PrepareFailed);
    assert!(
        script.calls().is_empty(),
        "Appendix F.1: if 4 of 5 snapshots are created and the fifth fails, Ono MUST NOT mutate \
         the plan targets"
    );
    assert_eq!(outcome.assets().len(), 4);
    assert_eq!(
        outcome.retained_assets().len(),
        4,
        "the default decision retains the four until somebody decides otherwise"
    );
}

#[test]
fn should_remove_the_four_created_assets_when_every_condition_of_appendix_f_one_holds() {
    let now = instant(1_000);
    let plan = sealed_plan(now);
    let providers = registry(FakeRecoveryProvider::healthy());
    let mut protection = five_snapshots(&plan, common::PROVIDER, now);
    protection[4] = common::protection(&plan, "ono.recovery.absent", "tank/data5", now);
    let mut request = PrepareRequest::new(&plan, &protection, &providers, now)
        .cleaning_up(CleanupDecision::RemoveWherePermitted);

    let _ = prepare(&mut request).expect_err("the fifth asset cannot be created");
    let report = request.cleanup_report().expect("a decision was taken");

    assert_eq!(
        report.removed().len(),
        4,
        "Appendix F.1: they were created solely for this failed prepare, cleanup is \
         provider-declared, and nothing depends on them"
    );
    assert!(report.retained().is_empty());
    assert!(report.failures().is_empty());
}

#[test]
fn should_keep_the_created_assets_when_the_session_may_not_remove_them() {
    let now = instant(1_000);
    let plan = sealed_plan(now);
    let providers = registry(FakeRecoveryProvider::healthy());
    let mut protection = five_snapshots(&plan, common::PROVIDER, now);
    protection[4] = common::protection(&plan, "ono.recovery.absent", "tank/data5", now);
    let mut request = PrepareRequest::new(&plan, &protection, &providers, now)
        .cleaning_up(CleanupDecision::RemoveWherePermitted)
        .with_authority(Authority::full().without_recovery(RecoveryCapability::Cleanup));

    let _ = prepare(&mut request).expect_err("the fifth asset cannot be created");
    let report = request.cleanup_report().expect("a decision was taken");

    assert!(
        report.removed().is_empty(),
        "Appendix F.1: cleanup must itself be safe and permitted, and §43.2 decides that"
    );
    assert_eq!(report.retained().len(), 4);
}

#[test]
fn should_keep_an_asset_an_earlier_protect_created_for_another_plan() {
    let now = instant(1_000);
    let plan = sealed_plan(now);
    let elsewhere = ono_change_core::PlanId::derive(&["an-earlier-plan"]);
    let providers = registry(FakeRecoveryProvider::healthy());
    let mut protection = five_snapshots(&plan, common::PROVIDER, now);
    protection[0] = common::protection_owned_by(&elsewhere, common::PROVIDER, "tank/shared", now);
    protection[4] = common::protection(&plan, "ono.recovery.absent", "tank/data5", now);
    let mut request = PrepareRequest::new(&plan, &protection, &providers, now)
        .cleaning_up(CleanupDecision::RemoveWherePermitted);

    let _ = prepare(&mut request).expect_err("the fifth asset cannot be created");
    let report = request.cleanup_report().expect("a decision was taken");

    assert_eq!(
        report.retained().len(),
        1,
        "Appendix F.1: only assets created solely for this failed prepare may be cleaned up, and \
         §18.2's early protect creates one that was not"
    );
    assert_eq!(
        report.removed().len(),
        3,
        "the three this prepare did create are removable"
    );
}

#[test]
fn should_keep_the_created_assets_when_the_quiesce_window_did_not_close() {
    let now = instant(1_000);
    let plan = sealed_plan(now);
    let providers = registry(FakeRecoveryProvider::healthy());
    let protection = five_snapshots(&plan, common::PROVIDER, now);
    let hook = FakeQuiesce::failing_resume();
    let mut request = PrepareRequest::new(&plan, &protection, &providers, now)
        .cleaning_up(CleanupDecision::RemoveWherePermitted)
        .quiescing(Quiescing::new(
            "postgresql",
            std::time::Duration::from_secs(5),
            &hook,
        ));

    let _ = prepare(&mut request).expect_err("the application is still quiesced");
    let report = request.cleanup_report().expect("a decision was taken");

    assert!(
        report.removed().is_empty(),
        "Appendix F.1: no quiesce or recovery dependency may require retention"
    );
    assert_eq!(report.retained().len(), 5);
}

#[test]
fn should_not_let_a_cleanup_failure_obscure_the_original_prepare_failure() {
    let now = instant(1_000);
    let plan = sealed_plan(now);
    let providers = registry(FakeRecoveryProvider::with_script(
        ProviderScript::CleanupFails,
    ));
    let mut protection = five_snapshots(&plan, common::PROVIDER, now);
    protection[4] = common::protection(&plan, "ono.recovery.absent", "tank/data5", now);
    let mut request = PrepareRequest::new(&plan, &protection, &providers, now)
        .cleaning_up(CleanupDecision::RemoveWherePermitted);

    let refusal = prepare(&mut request).expect_err("the fifth asset cannot be created");
    let report = request.cleanup_report().expect("a decision was taken");

    assert_eq!(
        refusal.code().name(),
        "change.prepare_failed",
        "Appendix F.1: cleanup failure MUST NOT obscure the original prepare failure"
    );
    assert_ne!(refusal.code().name(), "recovery.cleanup_blocked");
    assert_eq!(
        report.failures().len(),
        4,
        "the cleanup's own refusals are reported beside the prepare failure, never in place of it"
    );
    assert_eq!(
        report.retained().len(),
        4,
        "§37.4: an asset that could not be removed is surfaced rather than forgotten"
    );
}

// ---- Appendix F.2 ---------------------------------------------------------------------------

#[test]
fn should_keep_a_non_idempotent_actions_outcome_unknown_rather_than_guessing_it() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let spec = PlanSpec::over(2)
        .chained()
        .declaring(ono_change_core::Idempotency::NonIdempotent);
    let plan = stored(&spec, &store, now);
    let providers = empty_registry();
    let protection = Vec::new();
    let drift = no_drift();
    let script = Script::healthy().unknown("svc-1");
    let execute = script.execute();
    let observe = observing(VerificationStatus::Passed);
    let mut request = ApplyRequest::new(
        &plan,
        &store,
        "session-a",
        now,
        &protection,
        &providers,
        &drift,
        &execute,
        &observe,
    );

    let outcome = apply(&mut request);

    assert_eq!(
        outcome.status_of(plan.actions()[0].id()),
        Some(ActionStatus::Unknown),
        "Appendix F.2: the state MUST be UNKNOWN, not guessed"
    );
    assert!(outcome.is_outcome_unknown());
    assert_eq!(
        outcome.uncertainty_boundary(),
        &[plan.actions()[0].id().clone()],
        "Appendix F.2: recovery planning must treat unknown outcome as an uncertainty boundary"
    );
    assert_eq!(
        outcome.failure_point(),
        Some(FailurePoint::UnknownOutcome),
        "a local action whose outcome could not be established is not a remote disconnect"
    );
}

#[test]
fn should_persist_an_unknown_outcome_so_a_later_session_still_sees_the_uncertainty() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let spec = PlanSpec::over(2).chained();
    let plan = stored(&spec, &store, now);
    let providers = empty_registry();
    let protection = Vec::new();
    let drift = no_drift();
    let script = Script::healthy().unknown("svc-1");
    let execute = script.execute();
    let observe = observing(VerificationStatus::Passed);
    let mut request = ApplyRequest::new(
        &plan,
        &store,
        "session-a",
        now,
        &protection,
        &providers,
        &drift,
        &execute,
        &observe,
    );

    let _ = apply(&mut request);
    let persisted = store
        .action_statuses(plan.id(), plan.revision())
        .expect("the records survive");

    assert_eq!(
        persisted.get(plan.actions()[0].id().as_str()).copied(),
        Some(ActionStatus::Unknown),
        "Appendix F.2 and §41.2: the uncertainty is what the next session reads back"
    );
}
