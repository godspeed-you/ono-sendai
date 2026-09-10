//! Auto-recovery policy (v0.6 §26.1, §26.2, §26.3).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::sync::Arc;

use ono_change_core::{
    ActionId, ActionRole, ChangePlan, EffectConfidence, EffectDomain, EffectKind,
    EquivalenceDomain, Execution, Intent, NewerStateClass, NewerStateImpact, NewerStateItem,
    PlanAction, ProposedEffect, ProtectionSummary, RecoveryGoal, RecoveryPlan, RestoreMethod,
    VerificationClass, VerificationContract, VerificationSet,
};
use ono_change_recovery::auto::admits_auto_recovery;
use ono_value::ErrorValue;
use support::{EPOCH, protected_filesystem, unprotected_filesystem};

/// The plan that would declare auto-recovery: it replaces one configuration file, and its
/// persistent domain is PROTECTED.
fn source(protection: ProtectionSummary, effect: ProposedEffect) -> ChangePlan {
    let plan = ChangePlan::draft(
        Intent::new("replace nginx configuration", "plan { ... }"),
        "session-auto",
        EPOCH,
    );
    let action = PlanAction::new(
        plan.id(),
        1,
        ActionRole::Mutate,
        "replace /etc/nginx/nginx.conf",
        Execution::ProviderAction {
            provider: Arc::from("linux.files"),
            operation: Arc::from("ono.file.write"),
            arguments: Vec::new(),
        },
    )
    .effecting(effect);
    let verification = VerificationSet::empty().with(VerificationContract::new(
        plan.id(),
        VerificationClass::Required,
        "nginx.service",
        "state == running",
    ));
    plan.with_action(action)
        .expect("a draft accepts an action")
        .with_verification(verification)
        .with_protection(protection)
        .seal(EPOCH)
        .expect("a plan with a required contract seals")
}

fn file_effect(plan: &ChangePlan) -> ProposedEffect {
    ProposedEffect::new(
        ActionId::of(plan.id(), 1, "replace /etc/nginx/nginx.conf"),
        EffectDomain::FilesystemPersistent,
        EffectKind::Modify,
        EffectConfidence::Guaranteed,
        "the file is replaced",
    )
    .on("/etc/nginx/nginx.conf")
}

fn emitting_effect(plan: &ChangePlan) -> ProposedEffect {
    ProposedEffect::new(
        ActionId::of(plan.id(), 1, "replace /etc/nginx/nginx.conf"),
        EffectDomain::ExternalSideEffect,
        EffectKind::Emit,
        EffectConfidence::Guaranteed,
        "a deployment webhook request is sent",
    )
    .on("deployment webhook")
}

fn compliant_plan() -> ChangePlan {
    let shape = ChangePlan::draft(
        Intent::new("replace nginx configuration", "plan { ... }"),
        "session-auto",
        EPOCH,
    );
    source(protected_filesystem(), file_effect(&shape))
}

/// A recovery plan meeting every part of §26.3 that is about the recovery itself.
fn compliant_recovery(items: Vec<NewerStateItem>, declare_domain: bool) -> RecoveryPlan {
    recovery(items, declare_domain, true)
}

fn recovery(items: Vec<NewerStateItem>, declare_domain: bool, sealed: bool) -> RecoveryPlan {
    let draft = ChangePlan::draft(
        Intent::new("restore nginx configuration", "recover plan/a82f"),
        "session-auto-recovery",
        EPOCH,
    );
    let action = PlanAction::new(
        draft.id(),
        1,
        ActionRole::Recover,
        "restore /etc/nginx/nginx.conf",
        Execution::RecoveryOperation {
            provider: Arc::from("ono.recovery.zfs"),
            capability: Arc::from("recovery.restore"),
            arguments: Vec::new(),
        },
    );
    let mut contract = VerificationContract::new(
        draft.id(),
        VerificationClass::Required,
        "/etc/nginx/nginx.conf",
        "digest == recovery-point",
    );
    if declare_domain {
        contract = contract.about(EquivalenceDomain::PersistentState);
    }
    let plan = draft
        .with_action(action)
        .expect("a draft accepts an action")
        .with_verification(VerificationSet::empty().with(contract));
    let plan = if sealed {
        plan.seal(EPOCH)
            .expect("a recovery plan with a required contract seals")
    } else {
        plan
    };
    RecoveryPlan::new(
        plan,
        RecoveryGoal::RestoreChangedObjects,
        RestoreMethod::SelectiveFileRestore,
        "rpool/ROOT/debian@ono-a82f",
    )
    .with_newer_state(NewerStateImpact::analysed(items))
}

fn unmet(error: &ErrorValue) -> Vec<String> {
    error
        .metadata()
        .get("unmet")
        .expect("§45: the refusal names what does not hold")
        .as_list()
        .expect("a list")
        .iter()
        .map(|value| value.as_str().expect("a string").to_owned())
        .collect()
}

#[test]
fn should_permit_auto_recovery_when_all_six_conditions_hold() {
    let plan = compliant_plan();
    let recovery = compliant_recovery(Vec::new(), true);
    assert!(
        admits_auto_recovery(&plan, Some(&recovery), true).is_ok(),
        "§26.3: a plan MAY declare auto-recovery when all of the conditions are true"
    );
}

#[test]
fn should_reject_auto_recovery_when_the_recovery_plan_is_not_constructed_yet() {
    let plan = compliant_plan();
    let recovery = recovery(Vec::new(), true, false);
    let error = admits_auto_recovery(&plan, Some(&recovery), true)
        .expect_err("§26.3: the recovery plan must be fully constructible before mutation");
    assert_eq!(unmet(&error).len(), 1, "only this condition fails");
    assert!(unmet(&error)[0].contains("fully constructed"));
}

#[test]
fn should_reject_auto_recovery_when_the_drift_analysis_did_not_run() {
    let plan = compliant_plan();
    let recovery =
        compliant_recovery(Vec::new(), true).with_newer_state(NewerStateImpact::unanalysed());
    let error = admits_auto_recovery(&plan, Some(&recovery), true)
        .expect_err("§62.8: the analysis is part of constructing the plan");
    assert_eq!(unmet(&error).len(), 1);
    assert!(
        unmet(&error)[0].contains("newer-state analysis did not run"),
        "§62.8: the refusal names the missing analysis, got {:?}",
        unmet(&error)
    );
}

#[test]
fn should_reject_auto_recovery_when_an_irreversible_external_side_effect_exists() {
    let shape = ChangePlan::draft(
        Intent::new("replace nginx configuration", "plan { ... }"),
        "session-auto",
        EPOCH,
    );
    let plan = source(protected_filesystem(), emitting_effect(&shape));
    let recovery = compliant_recovery(Vec::new(), true);
    let error = admits_auto_recovery(&plan, Some(&recovery), true)
        .expect_err("§26.3: no known irreversible external side effects may exist");
    assert_eq!(unmet(&error).len(), 1, "only this condition fails");
    assert!(unmet(&error)[0].contains("deployment webhook"));
}

#[test]
fn should_reject_auto_recovery_when_recovery_would_destroy_unrelated_newer_state() {
    let plan = compliant_plan();
    let recovery = compliant_recovery(
        vec![NewerStateItem::new(
            "/var/lib/app/db",
            NewerStateClass::DiscardedByMethod,
            "18 GiB written since the snapshot",
        )],
        true,
    );
    let error = admits_auto_recovery(&plan, Some(&recovery), true)
        .expect_err("§26.3: recovery must not destroy unrelated newer state");
    assert_eq!(unmet(&error).len(), 1, "only this condition fails");
    assert!(unmet(&error)[0].contains("/var/lib/app/db"));
}

#[test]
fn should_reject_auto_recovery_when_recovery_would_destroy_provider_native_history() {
    let plan = compliant_plan();
    let recovery = compliant_recovery(Vec::new(), true)
        .with_newer_state(NewerStateImpact::analysed(Vec::new()).destroying("tank/data@later-1"));
    let error = admits_auto_recovery(&plan, Some(&recovery), true)
        .expect_err("§13.6: destroying newer history is never automatic");
    assert_eq!(unmet(&error).len(), 1);
    assert!(
        unmet(&error)[0].contains("destroy unrelated newer state")
            && unmet(&error)[0].contains("tank/data@later-1"),
        "§13.6: the refusal names the history it would destroy, got {:?}",
        unmet(&error)
    );
}

#[test]
fn should_permit_auto_recovery_when_the_only_newer_state_is_the_recovery_target_itself() {
    let plan = compliant_plan();
    let recovery = compliant_recovery(
        vec![NewerStateItem::new(
            "/etc/nginx/nginx.conf",
            NewerStateClass::Conflicting,
            "the recovery target itself",
        )],
        true,
    );
    assert!(
        admits_auto_recovery(&plan, Some(&recovery), true).is_ok(),
        "§26.3 speaks of unrelated newer state, and Appendix C.4's conflict is the target itself"
    );
}

#[test]
fn should_reject_auto_recovery_when_a_required_domain_is_not_protected() {
    let shape = ChangePlan::draft(
        Intent::new("replace nginx configuration", "plan { ... }"),
        "session-auto",
        EPOCH,
    );
    let plan = source(unprotected_filesystem(), file_effect(&shape));
    let recovery = compliant_recovery(Vec::new(), true);
    let error = admits_auto_recovery(&plan, Some(&recovery), true)
        .expect_err("§26.3: protection must be PROTECTED or TRANSACTIONAL");
    assert_eq!(unmet(&error).len(), 1, "only this condition fails");
    assert!(unmet(&error)[0].contains("filesystem-persistent"));
}

#[test]
fn should_reject_auto_recovery_when_no_domain_carries_a_coverage_row_at_all() {
    let shape = ChangePlan::draft(
        Intent::new("replace nginx configuration", "plan { ... }"),
        "session-auto",
        EPOCH,
    );
    let plan = source(ProtectionSummary::empty(), file_effect(&shape));
    let recovery = compliant_recovery(Vec::new(), true);
    let error = admits_auto_recovery(&plan, Some(&recovery), true)
        .expect_err("§56.3: a condition nobody established has not been shown to hold");
    assert_eq!(unmet(&error).len(), 1);
    assert!(
        unmet(&error)[0].contains("no mutation domain carries a coverage row"),
        "§56.3: the refusal says no row exists rather than inventing a domain, got {:?}",
        unmet(&error)
    );
}

#[test]
fn should_reject_auto_recovery_when_the_recovery_carries_no_scoped_verification() {
    let plan = compliant_plan();
    let recovery = compliant_recovery(Vec::new(), false);
    let error = admits_auto_recovery(&plan, Some(&recovery), true)
        .expect_err("§26.3: recovery verification must exist");
    assert_eq!(unmet(&error).len(), 1, "only this condition fails");
    assert!(unmet(&error)[0].contains("equivalence domain"));
}

#[test]
fn should_reject_auto_recovery_when_user_policy_does_not_enable_it() {
    let plan = compliant_plan();
    let recovery = compliant_recovery(Vec::new(), true);
    let error = admits_auto_recovery(&plan, Some(&recovery), false)
        .expect_err("§26.1: automatic recovery is OFF by default");
    assert_eq!(unmet(&error).len(), 1, "only this condition fails");
    assert!(unmet(&error)[0].contains("off by default"));
}

#[test]
fn should_reject_auto_recovery_with_the_code_the_error_taxonomy_reserves() {
    let plan = compliant_plan();
    let recovery = compliant_recovery(Vec::new(), true);
    let error = admits_auto_recovery(&plan, Some(&recovery), false).expect_err("§26.1");
    assert_eq!(error.code().name(), "change.auto_recovery_rejected");
}

#[test]
fn should_name_every_condition_that_does_not_hold_at_once() {
    let shape = ChangePlan::draft(
        Intent::new("replace nginx configuration", "plan { ... }"),
        "session-auto",
        EPOCH,
    );
    let plan = source(unprotected_filesystem(), emitting_effect(&shape));
    let recovery = recovery(
        vec![NewerStateItem::new(
            "/var/lib/app/db",
            NewerStateClass::DiscardedByMethod,
            "written since the snapshot",
        )],
        false,
        false,
    );
    let error = admits_auto_recovery(&plan, Some(&recovery), false).expect_err("§26.3");
    let reported = unmet(&error);
    assert_eq!(
        reported.len(),
        6,
        "§45: an operator sees the whole list at once rather than one condition per attempt"
    );
    for condition in [
        "fully constructed",
        "deployment webhook",
        "/var/lib/app/db",
        "filesystem-persistent",
        "equivalence domain",
        "off by default",
    ] {
        assert!(
            reported.iter().any(|line| line.contains(condition)),
            "§26.3: each condition that does not hold is named, and `{condition}` is not in \
             {reported:?}"
        );
    }
}

#[test]
fn should_name_the_plan_whose_declaration_was_rejected() {
    let plan = compliant_plan();
    let recovery = compliant_recovery(Vec::new(), true);
    let error = admits_auto_recovery(&plan, Some(&recovery), false).expect_err("§26.1");
    assert_eq!(
        error.metadata().get("plan"),
        Some(&ono_value::Value::string(plan.id().as_str())),
        "§26.3: the declaration is rejected at seal time, so the plan is named"
    );
}

/// §26.3 decides at seal, and at seal a plan has no recovery asset yet: §4.5 creates them
/// immediately before mutation. With nothing to restore from, the first condition cannot hold, and
/// the rejection says so rather than evaluating a recovery that does not exist.
#[test]
fn should_reject_a_declaration_when_no_recovery_plan_can_exist_before_mutation() {
    let plan = compliant_plan();
    let error = admits_auto_recovery(&plan, None, true)
        .expect_err("§26.3: without a recovery plan the declaration is rejected");
    assert_eq!(error.code().name(), "change.auto_recovery_rejected");
    let unmet = unmet(&error);
    assert!(
        unmet
            .iter()
            .any(|condition| condition.contains("fully constructed")),
        "the first condition is named, got {unmet:?}"
    );
    assert!(
        unmet
            .iter()
            .any(|condition| condition.contains("verification")),
        "and nothing establishes a recovery verification either, got {unmet:?}"
    );
}
