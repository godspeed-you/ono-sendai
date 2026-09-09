//! Failure injection at every lifecycle boundary (spec v0.6 §54.5).
//!
//! §54.5 names ten failures and requires a test for each. The harness below injects one at a
//! chosen boundary and runs a real `apply`, so every case answers the same three questions
//! Appendix F's columns ask: what state the plan reached, whether the target was mutated, and what
//! the structured error says. The third is the one a person acts on, so it is asserted every time.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use std::sync::Arc;

use common::{
    FakeRecoveryProvider, PlanSpec, ProviderScript, Script, empty_registry, instant, no_drift,
    observing, protection, registry, store, stored, timing_out,
};
use ono_change_core::{ActionStatus, ChangePlan, PlanState, VerificationStatus};
use ono_change_executor::execute::{
    ApplyOutcome, ApplyRequest, CleanupDecision, CloseRequest, FailurePoint, apply, close,
};

/// The lifecycle boundary a case injects its failure at (§54.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Boundary {
    /// The provider cannot create the asset (§4.5).
    SnapshotCreation,
    /// The provider creates an asset whose scope is not the one the plan named (§11.4).
    SnapshotScope,
    /// The storage the asset would live on is full (Appendix D.3).
    StorageFull,
    /// A mutating action fails after protection completed (§4.7).
    MutationAfterProtection,
    /// A mutating action's outcome cannot be established, as after a shell death (§41.2).
    ShellCrash,
    /// The provider that would carry the action out is not reachable (§12.2).
    ProviderDisconnect,
    /// Every verification check runs out of time (§23.5).
    VerificationTimeout,
    /// A remote host stops answering mid-plan (§29.3).
    RemoteHostDisappears,
    /// Nothing fails at all, for the control case.
    None,
}

/// What one injected failure produced, in the three facts Appendix F fixes.
#[derive(Debug)]
struct Injected {
    state: PlanState,
    mutated: bool,
    code: String,
    failure_point: Option<FailurePoint>,
    touched: Vec<Arc<str>>,
    statuses: Vec<(ono_change_core::ActionId, ActionStatus)>,
    assets: usize,
    retained: usize,
}

impl Injected {
    fn from(outcome: &ApplyOutcome) -> Self {
        Self {
            state: outcome.state(),
            mutated: outcome.has_mutated(),
            code: outcome
                .error()
                .map(|error| error.code().name().to_owned())
                .unwrap_or_default(),
            failure_point: outcome.failure_point(),
            touched: outcome.touched_targets().to_vec(),
            statuses: outcome.statuses().to_vec(),
            assets: outcome.assets().len(),
            retained: outcome.retained_assets().len(),
        }
    }
}

/// Runs a real `apply` over a two-target protected plan with `boundary` failing.
fn inject(boundary: Boundary, spec: &PlanSpec) -> (ChangePlan, Injected) {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(spec, &store, now);
    let script_kind = match boundary {
        Boundary::SnapshotCreation => ProviderScript::CreateFails,
        Boundary::SnapshotScope => ProviderScript::ValidatesWrongScope,
        Boundary::StorageFull => ProviderScript::StorageFills,
        _ => ProviderScript::Healthy,
    };
    let providers = if boundary == Boundary::ProviderDisconnect {
        empty_registry()
    } else {
        registry(FakeRecoveryProvider::with_script(script_kind))
    };
    let protection = vec![protection(&plan, common::PROVIDER, "tank/data", now)];
    let drift = no_drift();
    let execution = match boundary {
        Boundary::MutationAfterProtection => Script::healthy().failing("svc-1"),
        Boundary::ShellCrash | Boundary::RemoteHostDisappears => Script::healthy().unknown("svc-1"),
        _ => Script::healthy(),
    };
    let execute = execution.execute();
    let passing = observing(VerificationStatus::Passed);
    let timeout = timing_out();
    let observe: &dyn Fn(
        &ono_change_core::VerificationContract,
    ) -> ono_change_executor::Observation = if boundary == Boundary::VerificationTimeout {
        &timeout
    } else {
        &passing
    };
    let mut request = ApplyRequest::new(
        &plan,
        &store,
        "session-under-test",
        now,
        &protection,
        &providers,
        &drift,
        &execute,
        observe,
    );
    let outcome = apply(&mut request);
    (plan, Injected::from(&outcome))
}

fn two_targets() -> PlanSpec {
    PlanSpec::over(2).chained().only_required("svc-1")
}

// ---- §54.5, case 1: snapshot creation fails ---------------------------------------------------

#[test]
fn should_leave_the_targets_untouched_when_snapshot_creation_fails() {
    let (_plan, result) = inject(Boundary::SnapshotCreation, &two_targets());

    assert_eq!(result.state, PlanState::PrepareFailed);
    assert!(
        !result.mutated,
        "§2.3: mutation MUST NOT begin without the protection the plan required"
    );
    assert_eq!(result.code, "change.prepare_failed");
    assert_eq!(
        result.failure_point,
        Some(FailurePoint::RecoveryAssetCreation)
    );
    assert!(result.touched.is_empty());
}

// ---- §54.5, case 2: snapshot validates wrong scope --------------------------------------------

#[test]
fn should_leave_the_targets_untouched_when_a_snapshot_validates_the_wrong_scope() {
    let (_plan, result) = inject(Boundary::SnapshotScope, &two_targets());

    assert_eq!(result.state, PlanState::PrepareFailed);
    assert!(!result.mutated);
    assert_eq!(result.code, "change.prepare_failed");
    assert_eq!(
        result.failure_point,
        Some(FailurePoint::RecoveryValidation),
        "§11.4: a scope that does not match the expected target is not protection"
    );
    assert_eq!(
        result.assets, 1,
        "the asset exists and is marked invalid rather than forgotten"
    );
}

// ---- §54.5, case 3: mutation fails after protection --------------------------------------------

#[test]
fn should_reach_apply_failed_with_its_protection_retained_when_mutation_fails() {
    let (plan, result) = inject(Boundary::MutationAfterProtection, &two_targets());

    assert_eq!(result.state, PlanState::ApplyFailed);
    assert!(
        result.mutated,
        "Appendix F: from APPLYING onwards, something may have happened"
    );
    assert_eq!(result.code, "change.apply_failed");
    assert_eq!(result.failure_point, Some(FailurePoint::FirstMutateAction));
    assert_eq!(
        result.retained, 1,
        "§37.2: the recovery asset of a failed plan is retained"
    );
    assert_eq!(
        result
            .statuses
            .iter()
            .find(|(id, _)| id == plan.actions()[1].id())
            .map(|(_, status)| *status),
        Some(ActionStatus::Skipped),
        "Appendix F: the dependency chain stopped"
    );
}

// ---- §54.5, case 4: the shell crashes mid-apply -------------------------------------------------

#[test]
fn should_leave_an_uncertainty_boundary_when_the_shell_dies_mid_apply() {
    let (plan, result) = inject(Boundary::ShellCrash, &two_targets());

    assert_eq!(
        result.state,
        PlanState::Applying,
        "Appendix F.2: the outcome is UNKNOWN, and no state word resolves it"
    );
    assert!(result.mutated);
    assert_eq!(result.code, "change.remote_state_unknown");
    assert_eq!(
        result
            .statuses
            .iter()
            .find(|(id, _)| id == plan.actions()[0].id())
            .map(|(_, status)| *status),
        Some(ActionStatus::Unknown)
    );
}

#[test]
fn should_let_a_later_session_read_back_what_the_crash_left() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&two_targets(), &store, now);
    let providers = empty_registry();
    let protection = Vec::new();
    let drift = no_drift();
    let script = Script::healthy().unknown("svc-1");
    let execute = script.execute();
    let observe = observing(VerificationStatus::Passed);
    let mut request = ApplyRequest::new(
        &plan,
        &store,
        "session-that-died",
        now,
        &protection,
        &providers,
        &drift,
        &execute,
        &observe,
    );
    let _ = apply(&mut request);

    let resumed = ono_change_executor::resume::resume(&plan, &store, instant(2_000));

    assert_eq!(
        resumed.state(),
        PlanState::Applying,
        "§41.2: plan state MUST be reconstructable from persisted action records"
    );
    assert_eq!(resumed.uncertainty_boundary().len(), 1);
}

// ---- §54.5, case 5: a provider disconnects ------------------------------------------------------

#[test]
fn should_refuse_before_mutation_when_the_recovery_provider_is_not_there() {
    let (_plan, result) = inject(Boundary::ProviderDisconnect, &two_targets());

    assert_eq!(result.state, PlanState::PrepareFailed);
    assert!(
        !result.mutated,
        "§12.2: a provider that cannot run says so, and §2.3 stops before mutation"
    );
    assert_eq!(result.code, "change.prepare_failed");
    assert!(result.touched.is_empty());
}

// ---- §54.5, case 6: verification times out ------------------------------------------------------

#[test]
fn should_never_call_a_timed_out_verification_a_success() {
    let (_plan, result) = inject(Boundary::VerificationTimeout, &two_targets());

    assert_ne!(
        result.state,
        PlanState::Verified,
        "§23.5 and Appendix F: a timeout MUST NOT be treated as success"
    );
    assert_eq!(result.state, PlanState::Failed);
    assert!(result.mutated);
    assert_eq!(result.code, "change.verification_failed");
    assert_eq!(
        result.failure_point,
        Some(FailurePoint::VerificationTimeout)
    );
    assert_eq!(
        result.touched.len(),
        2,
        "the mutations ran; it is the evidence about them that did not arrive"
    );
}

// ---- §54.5, case 7: recovery fails halfway ------------------------------------------------------

#[test]
fn should_preserve_the_partial_state_when_recovery_fails_halfway() {
    let spec = PlanSpec::over(3)
        .recovering()
        .chained()
        .only_required("svc-1");
    let now = instant(1_000);
    let (_directory, store) = store();
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
        "session-under-test",
        now,
        &protection,
        &providers,
        &drift,
        &execute,
        &observe,
    );

    let outcome = apply(&mut request);

    assert_eq!(outcome.state(), PlanState::RecoveryFailed);
    assert!(outcome.has_mutated());
    assert_eq!(
        outcome
            .error()
            .map(|error| error.code().name().to_owned())
            .unwrap_or_default(),
        "recovery.apply_failed"
    );
    assert_eq!(
        outcome.status_of(plan.actions()[0].id()),
        Some(ActionStatus::Succeeded),
        "Appendix F: preserve the remaining assets and the exact partial state"
    );
    assert_eq!(
        outcome.status_of(plan.actions()[2].id()),
        Some(ActionStatus::Skipped)
    );
}

// ---- §54.5, case 8: cleanup fails ----------------------------------------------------------------

#[test]
fn should_surface_the_asset_that_cleanup_could_not_remove() {
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
    let assets = vec![
        protection(&plan, common::PROVIDER, "tank/data", now)
            .proposed_asset()
            .clone(),
    ];
    let mut request = CloseRequest::new(&plan, &providers, &assets).removing_assets();

    let outcome = close(&mut request);

    assert_eq!(
        outcome.state(),
        PlanState::Closed,
        "Appendix F: a cleanup failure is no new target mutation"
    );
    assert!(outcome.is_closed_with_assets());
    assert_eq!(
        outcome
            .failures()
            .first()
            .map(|error| error.code().name().to_owned())
            .unwrap_or_default(),
        "recovery.cleanup_blocked",
        "§37.4: the operator is told which asset is still there"
    );
}

#[test]
fn should_not_let_a_failed_cleanup_hide_the_failure_that_caused_it() {
    let now = instant(1_000);
    let plan = PlanSpec::default().seal(now);
    let providers = registry(FakeRecoveryProvider::with_script(
        ProviderScript::CleanupFails,
    ));
    let mut protection = common::five_snapshots(&plan, common::PROVIDER, now);
    protection[4] = common::protection(&plan, "ono.recovery.absent", "tank/data5", now);
    let mut request =
        ono_change_executor::execute::PrepareRequest::new(&plan, &protection, &providers, now)
            .cleaning_up(CleanupDecision::RemoveWherePermitted);

    let refusal =
        ono_change_executor::execute::prepare(&mut request).expect_err("the fifth asset fails");

    assert_eq!(
        refusal.code().name(),
        "change.prepare_failed",
        "Appendix F.1: cleanup failure MUST NOT obscure the original prepare failure"
    );
}

// ---- §54.5, case 9: storage fills ----------------------------------------------------------------

#[test]
fn should_fail_closed_when_the_storage_a_snapshot_needs_is_full() {
    let (_plan, result) = inject(Boundary::StorageFull, &two_targets());

    assert_eq!(result.state, PlanState::PrepareFailed);
    assert!(
        !result.mutated,
        "Appendix D.3: automatic protection fails closed rather than worsening exhaustion"
    );
    assert_eq!(result.code, "change.prepare_failed");
    assert_eq!(
        result.failure_point,
        Some(FailurePoint::RecoveryAssetCreation)
    );
}

#[test]
fn should_say_which_storage_ran_out_when_a_snapshot_cannot_be_taken() {
    let now = instant(1_000);
    let plan = PlanSpec::default().seal(now);
    let providers = registry(FakeRecoveryProvider::with_script(
        ProviderScript::StorageFills,
    ));
    let protection = vec![protection(&plan, common::PROVIDER, "tank/data", now)];
    let mut request =
        ono_change_executor::execute::PrepareRequest::new(&plan, &protection, &providers, now);

    let refusal =
        ono_change_executor::execute::prepare(&mut request).expect_err("the pool is full");

    assert!(
        refusal
            .help()
            .is_some_and(|help| help.contains("10 GiB") || help.contains("free")),
        "Appendix D.3: the operator is told the floor that was not cleared"
    );
    assert!(
        refusal
            .metadata()
            .get("cause")
            .is_some_and(|value| *value == ono_value::Value::string("recovery.storage_pressure")),
        "the original cause travels with the prepare refusal rather than being flattened"
    );
}

// ---- §54.5, case 10: a remote host disappears ------------------------------------------------------

#[test]
fn should_keep_a_vanished_remote_host_unknown_rather_than_failed() {
    let spec = PlanSpec::over(2)
        .on_hosts(&["api-04", "api-05"])
        .chained()
        .only_required("svc-1");
    let (plan, result) = inject(Boundary::RemoteHostDisappears, &spec);

    assert_eq!(
        result.state,
        PlanState::Applying,
        "Appendix F: a remote disconnect is APPLYING/UNKNOWN, never FAILED"
    );
    assert_eq!(result.code, "change.remote_state_unknown");
    assert_eq!(result.failure_point, Some(FailurePoint::RemoteDisconnect));
    assert_eq!(
        result
            .statuses
            .iter()
            .find(|(id, _)| id == plan.actions()[0].id())
            .map(|(_, status)| *status),
        Some(ActionStatus::Unknown),
        "§29.3: MUST NOT mark unknown remote actions as failed or successful without evidence"
    );
}

// ---- the control case ------------------------------------------------------------------------------

#[test]
fn should_verify_the_same_plan_when_nothing_is_injected() {
    let (_plan, result) = inject(Boundary::None, &two_targets());

    assert_eq!(
        result.state,
        PlanState::Verified,
        "the harness's injections are the only difference between these cases"
    );
    assert!(result.mutated);
    assert!(result.code.is_empty());
    assert_eq!(result.assets, 1);
    assert_eq!(result.touched.len(), 2);
}

#[test]
fn should_leave_every_boundary_before_mutation_with_the_targets_untouched() {
    for boundary in [
        Boundary::SnapshotCreation,
        Boundary::SnapshotScope,
        Boundary::StorageFull,
        Boundary::ProviderDisconnect,
    ] {
        let (_plan, result) = inject(boundary, &two_targets());
        assert!(
            !result.mutated,
            "Appendix F: {boundary:?} is a prepare-time boundary, and §2.3 forbids mutating past it"
        );
        assert!(result.touched.is_empty());
    }
}

#[test]
fn should_leave_every_boundary_after_mutation_saying_something_may_have_happened() {
    for boundary in [
        Boundary::MutationAfterProtection,
        Boundary::ShellCrash,
        Boundary::VerificationTimeout,
    ] {
        let (_plan, result) = inject(boundary, &two_targets());
        assert!(
            result.mutated,
            "Appendix F: {boundary:?} is at or after the first mutation"
        );
        assert!(
            !result.code.is_empty(),
            "and the operator is told, in a structured refusal"
        );
    }
}
