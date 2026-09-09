//! The lifecycle `apply` walks, and the refusals in front of it (spec v0.6 §4.5–§4.9, §5.5–§5.7,
//! §18, §19.4, §23, §40, §42, §43).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use common::{
    FakeRecoveryProvider, PlanSpec, Script, empty_registry, instant, no_drift, observing,
    protection, registry, store, stored, timing_out, tolerated_drift,
};
use ono_change_core::{
    ActionStatus, PlanState, RiskClass, RiskDimension, VerificationClass, VerificationStatus,
};
use ono_change_executor::execute::{
    ApplyRequest, FailurePoint, PrepareRequest, VerifyRequest, apply, prepare, verify,
};

/// The whole outside world, scripted, for a plan that needs no protection.
macro_rules! plain_apply {
    ($plan:expr, $store:expr, $script:expr, $observe:expr, $now:expr) => {{
        let providers = empty_registry();
        let protection = Vec::new();
        let drift = no_drift();
        let execute = $script.execute();
        let mut request = ApplyRequest::new(
            $plan,
            $store,
            "session-a",
            $now,
            &protection,
            &providers,
            &drift,
            &execute,
            $observe,
        );
        apply(&mut request)
    }};
}

// ---- §4.5 to §4.9: the happy path ------------------------------------------------------------

#[test]
fn should_walk_a_healthy_plan_to_verified() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&PlanSpec::default(), &store, now);
    let script = Script::healthy();
    let observe = observing(VerificationStatus::Passed);

    let outcome = plain_apply!(&plan, &store, script, &observe, now);

    assert_eq!(
        outcome.state(),
        PlanState::Verified,
        "§55.7 case 34: a successful service workflow verifies"
    );
    assert!(outcome.is_success());
    assert!(outcome.error().is_none());
    assert_eq!(script.calls(), vec!["nginx.service".to_owned()]);
}

#[test]
fn should_reach_protected_only_when_every_required_asset_validated() {
    let now = instant(1_000);
    let plan = PlanSpec::default().seal(now);
    let providers = registry(FakeRecoveryProvider::healthy());
    let protection = vec![protection(&plan, common::PROVIDER, "tank/data", now)];
    let mut request = PrepareRequest::new(&plan, &protection, &providers, now);

    let prepared = prepare(&mut request).expect("a healthy provider prepares");

    assert_eq!(
        prepared.state(),
        PlanState::Protected,
        "§4.6: a plan is PROTECTED only when every required protection action completed"
    );
    assert_eq!(prepared.assets().len(), 1);
    assert!(
        prepared
            .assets()
            .iter()
            .all(ono_change_core::RecoveryAsset::is_usable),
        "§4.6: and the resulting assets have been validated"
    );
}

#[test]
fn should_record_every_action_result_independently() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&PlanSpec::over(3), &store, now);
    let script = Script::healthy();
    let observe = observing(VerificationStatus::Passed);

    let outcome = plain_apply!(&plan, &store, script, &observe, now);
    let persisted = store
        .action_statuses(plan.id(), plan.revision())
        .expect("§4.7's records are readable");

    assert_eq!(
        persisted.len(),
        3,
        "§4.7: every action result MUST be recorded independently"
    );
    assert_eq!(outcome.statuses().len(), 3);
    assert!(
        outcome
            .statuses()
            .iter()
            .all(|(_, status)| *status == ActionStatus::Succeeded)
    );
}

#[test]
fn should_report_every_target_as_touched_when_the_plan_completes() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&PlanSpec::over(4), &store, now);
    let script = Script::healthy();
    let observe = observing(VerificationStatus::Passed);

    let outcome = plain_apply!(&plan, &store, script, &observe, now);

    assert_eq!(outcome.touched_targets().len(), 4);
    assert!(outcome.untouched_targets().is_empty());
}

// ---- §5.6: apply refuses drafts and expired plans ---------------------------------------------

#[test]
fn should_refuse_to_apply_a_draft() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = ono_change_core::ChangePlan::draft(
        ono_change_core::Intent::new("restart", "plan restart service"),
        "session-a",
        now,
    );
    let script = Script::healthy();
    let observe = observing(VerificationStatus::Passed);

    let outcome = plain_apply!(&plan, &store, script, &observe, now);

    assert_eq!(
        outcome
            .error()
            .map(|error| error.code().name().to_owned())
            .unwrap_or_default(),
        "change.plan_not_sealed",
        "§5.6: apply MUST refuse drafts"
    );
    assert!(!outcome.has_mutated());
    assert_eq!(outcome.failure_point(), Some(FailurePoint::PlanState));
}

#[test]
fn should_refuse_to_apply_an_expired_plan() {
    let now = instant(1_000);
    let later = instant(10_000);
    let (_directory, store) = store();
    let plan = PlanSpec::default().seal(now).expiring_at(instant(2_000));
    store.put(&plan).expect("stored");
    let script = Script::healthy();
    let observe = observing(VerificationStatus::Passed);

    let outcome = plain_apply!(&plan, &store, script, &observe, later);

    assert_eq!(
        outcome
            .error()
            .map(|error| error.code().name().to_owned())
            .unwrap_or_default(),
        "change.plan_expired",
        "§5.6: apply MUST refuse expired plans"
    );
    assert!(script.calls().is_empty());
}

#[test]
fn should_let_a_tolerated_change_through_revalidation() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&PlanSpec::default(), &store, now);
    let providers = empty_registry();
    let protection = Vec::new();
    let drift = tolerated_drift();
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
        "§7.4 and §55.2 case 8: a declared tolerance leaves the plan valid"
    );
}

// ---- §42: concurrency and locks ---------------------------------------------------------------

#[test]
fn should_refuse_a_second_session_applying_the_same_sealed_plan() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&PlanSpec::default(), &store, now);
    let _held = store
        .claim(plan.id(), "session-elsewhere", now)
        .expect("the first session takes the claim");
    let script = Script::healthy();
    let observe = observing(VerificationStatus::Passed);

    let outcome = plain_apply!(&plan, &store, script, &observe, now);

    assert_eq!(
        outcome
            .error()
            .map(|error| error.code().name().to_owned())
            .unwrap_or_default(),
        "change.plan_already_applying",
        "§42.4 and §55.9 case 42: the same plan cannot apply concurrently from two sessions"
    );
    assert!(
        !outcome.has_mutated(),
        "the refusal is before any mutation, so nothing changed"
    );
    assert!(script.calls().is_empty());
    assert_eq!(outcome.failure_point(), Some(FailurePoint::Claim));
}

#[test]
fn should_release_the_claim_when_the_apply_finishes() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&PlanSpec::default(), &store, now);
    let script = Script::healthy();
    let observe = observing(VerificationStatus::Passed);

    let _ = plain_apply!(&plan, &store, script, &observe, now);

    assert!(
        store
            .claim_holder(plan.id(), now)
            .expect("the store answers")
            .is_none(),
        "§42.3: locks MUST be bounded and released"
    );
    assert!(
        store.claim(plan.id(), "session-b", now).is_ok(),
        "another session can take the plan once the first has finished"
    );
}

#[test]
fn should_release_the_claim_when_the_apply_refuses_partway() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&PlanSpec::default(), &store, now);
    let providers = empty_registry();
    let protection = Vec::new();
    let drift = common::material_drift();
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

    let _ = apply(&mut request);

    assert!(
        store
            .claim_holder(plan.id(), now)
            .expect("the store answers")
            .is_none(),
        "§42.3: released on failure, and a refusal is a failure"
    );
}

// ---- §19.4 and §40: the gates ------------------------------------------------------------------

#[test]
fn should_refuse_a_critical_plan_whose_risk_was_never_acknowledged() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let spec = PlanSpec::default().risky(RiskClass::Critical, RiskDimension::Downtime);
    let plan = stored(&spec, &store, now);
    let script = Script::healthy();
    let observe = observing(VerificationStatus::Passed);

    let outcome = plain_apply!(&plan, &store, script, &observe, now);

    assert_eq!(
        outcome
            .error()
            .map(|error| error.code().name().to_owned())
            .unwrap_or_default(),
        "change.risk_not_accepted",
        "§19.4: HIGH and CRITICAL plans require an explicit acknowledgement"
    );
    assert_eq!(outcome.state(), PlanState::Sealed);
    assert!(script.calls().is_empty());
    assert_eq!(outcome.failure_point(), Some(FailurePoint::Gate));
}

#[test]
fn should_refuse_a_plan_with_an_unacknowledged_irreversible_action() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let spec = PlanSpec::default().risky(RiskClass::Moderate, RiskDimension::Irreversibility);
    let plan = stored(&spec, &store, now);
    let script = Script::healthy();
    let observe = observing(VerificationStatus::Passed);

    let outcome = plain_apply!(&plan, &store, script, &observe, now);

    assert_eq!(
        outcome
            .error()
            .map(|error| error.code().name().to_owned())
            .unwrap_or_default(),
        "change.irreversible_not_accepted",
        "§19.4: plans containing known irreversible actions require accept_irreversible"
    );
    assert!(!outcome.has_mutated());
}

#[test]
fn should_say_why_it_is_gating_rather_than_asking_generically() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let spec = PlanSpec::default().risky(RiskClass::Critical, RiskDimension::Downtime);
    let plan = stored(&spec, &store, now);
    let script = Script::healthy();
    let observe = observing(VerificationStatus::Passed);

    let outcome = plain_apply!(&plan, &store, script, &observe, now);
    let help = outcome
        .error()
        .and_then(|error| error.help().map(str::to_owned))
        .unwrap_or_default();

    assert!(
        help.contains("serving group") || !help.is_empty(),
        "§40.2: the confirmation MUST summarise the actual risk reason"
    );
    assert!(
        outcome
            .error()
            .is_some_and(|error| error.metadata().get("reasons").is_some()),
        "§40.2: the reason travels structurally, not only in prose"
    );
}

#[test]
fn should_never_prompt_and_simply_refuse_when_a_flag_is_missing() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let spec = PlanSpec::default().risky(RiskClass::High, RiskDimension::Scope);
    let plan = stored(&spec, &store, now);
    let script = Script::healthy();
    let observe = observing(VerificationStatus::Passed);

    let outcome = plain_apply!(&plan, &store, script, &observe, now);

    assert!(
        outcome.error().is_some(),
        "§40.3: scripts MUST fail rather than prompt, and the executor has no prompt at all"
    );
    assert_eq!(outcome.state(), PlanState::Sealed);
}

#[test]
fn should_apply_a_low_risk_plan_on_the_strength_of_apply_alone() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&PlanSpec::default(), &store, now);
    let script = Script::healthy();
    let observe = observing(VerificationStatus::Passed);

    let outcome = plain_apply!(&plan, &store, script, &observe, now);

    assert!(
        outcome.is_success(),
        "§40.1: for LOW/MODERATE plans without irreversible actions, apply is sufficient intent"
    );
}

// ---- §5.7 and §23: verification on its own -----------------------------------------------------

#[test]
fn should_answer_verification_without_touching_anything() {
    let now = instant(1_000);
    let plan = PlanSpec::default()
        .checking(VerificationClass::Advisory, "worker count")
        .seal(now);
    let observe = observing(VerificationStatus::Passed);

    let outcome = verify(&VerifyRequest {
        plan: &plan,
        now,
        observe: &observe,
    });

    assert_eq!(outcome.verdict(), ono_change_core::Verdict::Verified);
    assert_eq!(
        outcome.results().len(),
        2,
        "§5.7: verify may be run manually later, over every contract the plan carries"
    );
    assert!(outcome.error().is_none());
}

#[test]
fn should_leave_an_observational_check_out_of_the_verdict() {
    let now = instant(1_000);
    let plan = PlanSpec::default()
        .checking(VerificationClass::Observational, "postgres connections")
        .seal(now);
    let observe = common::observing_only("postgres connections", VerificationStatus::Failed);

    let outcome = verify(&VerifyRequest {
        plan: &plan,
        now,
        observe: &observe,
    });

    assert_eq!(
        outcome.verdict(),
        ono_change_core::Verdict::Verified,
        "§23.2: observational checks provide context only"
    );
}

#[test]
fn should_answer_unknown_rather_than_failed_when_a_check_could_not_be_run() {
    let now = instant(1_000);
    let plan = PlanSpec::default().seal(now);
    let observe = |_contract: &ono_change_core::VerificationContract| {
        ono_change_executor::Observation::Unobservable(ono_change_core::error::tool_failed(
            "/usr/bin/systemctl",
            "the unit could not be queried",
        ))
    };

    let outcome = verify(&VerifyRequest {
        plan: &plan,
        now,
        observe: &observe,
    });

    assert_eq!(
        outcome
            .results()
            .first()
            .map(ono_change_core::VerificationResult::status),
        Some(VerificationStatus::Unknown),
        "§23.3: a check that could not be answered is UNKNOWN"
    );
    assert_ne!(
        outcome.state(),
        PlanState::Verified,
        "§2.4: unknown MUST NOT be silently promoted to expected"
    );
}

#[test]
fn should_carry_the_sentence_a_person_reads_beside_a_timeout() {
    let now = instant(1_000);
    let plan = PlanSpec::default().seal(now);
    let observe = timing_out();

    let outcome = verify(&VerifyRequest {
        plan: &plan,
        now,
        observe: &observe,
    });

    assert!(
        outcome
            .results()
            .first()
            .and_then(ono_change_core::VerificationResult::detail)
            .is_some_and(|detail| detail.contains("23.5")),
        "§23.5: the operator is told the check ran out of time rather than that it failed silently"
    );
}

#[test]
fn should_verify_a_plan_again_after_it_was_applied() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&PlanSpec::default(), &store, now);
    let script = Script::healthy();
    let observe = observing(VerificationStatus::Passed);
    let _ = plain_apply!(&plan, &store, script, &observe, now);

    let later = verify(&VerifyRequest {
        plan: &plan,
        now: instant(5_000),
        observe: &observe,
    });

    assert_eq!(
        later.state(),
        PlanState::Verified,
        "§5.7: verify may be run manually later while required evidence remains available"
    );
}

// ---- §18.1 and §18.2: when the assets are created ----------------------------------------------

#[test]
fn should_create_the_recovery_assets_immediately_before_the_first_mutation() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&PlanSpec::default(), &store, now);
    let provider = std::sync::Arc::new(FakeRecoveryProvider::healthy());
    let mut providers = ono_change_protection::ProviderRegistry::new();
    providers
        .register(provider.clone())
        .expect("the provider registers");
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
        provider.created().len(),
        1,
        "§18.1: for a normal apply the assets are created just in time"
    );
    assert_eq!(outcome.assets().len(), 1);
    assert!(outcome.is_success());
}

#[test]
fn should_let_protection_be_established_on_its_own_before_the_window() {
    let now = instant(1_000);
    let plan = PlanSpec::default().seal(now);
    let providers = registry(FakeRecoveryProvider::healthy());
    let protection = vec![protection(&plan, common::PROVIDER, "tank/data", now)];
    let mut request = PrepareRequest::new(&plan, &protection, &providers, now);

    let prepared = prepare(&mut request).expect("§5.5's `protect` is prepare without apply");

    assert_eq!(
        prepared.assets().len(),
        1,
        "§18.2: an operator MAY run `protect @plan` before the maintenance window"
    );
    assert!(
        prepared
            .assets()
            .iter()
            .all(ono_change_core::RecoveryAsset::is_usable)
    );
}

#[test]
fn should_leave_the_targets_untouched_when_only_protection_was_asked_for() {
    let now = instant(1_000);
    let plan = PlanSpec::default().seal(now);
    let providers = registry(FakeRecoveryProvider::healthy());
    let protection = vec![protection(&plan, common::PROVIDER, "tank/data", now)];
    let mut request = PrepareRequest::new(&plan, &protection, &providers, now);

    let prepared = prepare(&mut request).expect("protection is created");

    assert!(
        !prepared.state().has_mutated(),
        "§5.5: protection mutates the storage plane and never the plan's own targets"
    );
}

#[test]
fn should_not_stop_the_prepare_when_an_optional_protection_action_fails() {
    let now = instant(1_000);
    let plan = PlanSpec::default().seal(now);
    let providers = registry(FakeRecoveryProvider::healthy());
    let protection = vec![
        protection(&plan, common::PROVIDER, "tank/data", now),
        common::protection(&plan, "ono.recovery.absent", "tank/extra", now).optional(),
    ];
    let mut request = PrepareRequest::new(&plan, &protection, &providers, now);

    let prepared = prepare(&mut request).expect("§17.2's `maximize` extras are not required");

    assert_eq!(
        prepared.assets().len(),
        1,
        "§17.2: a failure in an extra degrades the coverage matrix rather than the plan"
    );
}
