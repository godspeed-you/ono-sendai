//! Constructing a RecoveryPlan (v0.6 §24.1, §24.3, §24.4, §2.12, §35.2, §55.8, Appendix F.2).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::sync::Arc;

use ono_change_core::{
    ActionId, ActionRole, ActionStatus, ChangePlan, DirectoryRestorePolicy, EffectConfidence,
    EffectDomain, EffectKind, EquivalenceDomain, Execution, Intent, PlanAction, PlanKind,
    PlanState, ProposedEffect, RecoveryAsset, RecoveryGoal, RecoveryPlan, RestoreMethod, RiskClass,
    VerificationClass, VerificationContract, VerificationSet,
};
use ono_change_protection::ProviderRegistry;
use ono_change_recovery::builder::{RecoveryRequest, plan_recovery};
use ono_change_recovery::conflict::ObservedState;
use ono_value::ErrorValue;
use support::{
    EPOCH, TestProvider, at, content_mode_owner, nginx_plan, protected_filesystem, ready_asset,
    registry,
};

const PROVIDER: &str = "ono.recovery.zfs";
const SNAPSHOT: &str = "rpool/ROOT/debian@ono-a82f";
const NGINX_CONF: &str = "/etc/nginx/nginx.conf";

fn assets() -> Vec<RecoveryAsset> {
    vec![ready_asset(
        PROVIDER,
        SNAPSHOT,
        "rpool/ROOT/debian",
        &[NGINX_CONF, "/etc/hosts", "/etc/ssh/sshd_config"],
        at(14, 2),
    )]
}

/// The world of §24.4: the plan wrote the config at 14:03, and two unrelated files changed later.
fn world(object: &str) -> ObservedState {
    match object {
        NGINX_CONF => ObservedState::changed(at(14, 3), "sha256:written-by-the-plan"),
        "/etc/hosts" => ObservedState::changed(at(15, 0), "sha256:hosts"),
        _ => ObservedState::changed(at(15, 30), "sha256:sshd"),
    }
}

fn zfs() -> ProviderRegistry {
    registry(vec![
        TestProvider::new(PROVIDER)
            .verifying(
                "nginx.service",
                "state == running",
                VerificationClass::Required,
                EquivalenceDomain::RuntimeState,
            )
            .shared(),
    ])
}

fn nginx_recovery(registry: &ProviderRegistry, assets: &[RecoveryAsset]) -> RecoveryPlan {
    let source = nginx_plan();
    let observe = world;
    plan_recovery(
        &RecoveryRequest::new(
            registry,
            assets,
            &observe,
            RecoveryGoal::RestoreChangedObjects,
            "session-recovery",
            at(16, 0),
        )
        .recovering(&source)
        .applied_at(at(14, 3))
        .restoring(NGINX_CONF)
        .captured(NGINX_CONF, "sha256:before-the-plan"),
    )
    .expect("§24.1: a recovery over a validated asset produces a plan")
}

// -- §24.1: recover produces a plan and changes nothing ---------------------------------------

#[test]
fn should_produce_a_sealed_plan_that_has_not_been_applied() {
    let registry = zfs();
    let recovery = nginx_recovery(&registry, &assets());
    assert_eq!(
        recovery.plan().state(),
        PlanState::Sealed,
        "§24.1: `recover @plan` produces a RecoveryPlan and does not modify state"
    );
}

#[test]
fn should_not_touch_the_target_when_planning_recovery() {
    let provider = TestProvider::new(PROVIDER);
    let calls = provider.calls();
    let registry = registry(vec![provider.shared()]);
    let _ = nginx_recovery(&registry, &assets());
    assert_eq!(
        calls.mutating(),
        0,
        "§55.8 case 35: `recover @plan` does not immediately mutate state"
    );
}

#[test]
fn should_mark_the_embedded_plan_as_a_recovery() {
    let registry = zfs();
    let recovery = nginx_recovery(&registry, &assets());
    assert_eq!(
        recovery.plan().kind(),
        PlanKind::Recovery,
        "§3.8: a plan produced to recover from a prior plan is a recovery plan"
    );
}

#[test]
fn should_carry_only_recover_role_actions() {
    let registry = zfs();
    let recovery = nginx_recovery(&registry, &assets());
    assert!(
        !recovery.plan().actions().is_empty()
            && recovery
                .plan()
                .actions()
                .iter()
                .all(|action| action.role() == ActionRole::Recover),
        "§3.3: recovery actions restore or compensate after failure"
    );
}

#[test]
fn should_carry_verification_contracts_on_the_recovery_plan() {
    let registry = zfs();
    let recovery = nginx_recovery(&registry, &assets());
    assert!(
        recovery.plan().verification().has_required(),
        "§23.1: a plan containing a mutating action carries at least one contract"
    );
}

#[test]
fn should_state_which_equivalence_domain_each_recovery_check_reports_on() {
    let registry = zfs();
    let recovery = nginx_recovery(&registry, &assets());
    assert!(
        recovery
            .plan()
            .verification()
            .contracts()
            .iter()
            .all(|contract| contract.equivalence().is_some()),
        "§25.1: recovery verification distinguishes persistent, runtime and external equivalence"
    );
}

#[test]
fn should_seal_the_recovery_plan_with_a_digest_that_holds() {
    let registry = zfs();
    let recovery = nginx_recovery(&registry, &assets());
    assert!(
        recovery.plan().digest_holds(),
        "§2.12 and §4.4: a recovery goes through the same lifecycle, seal included"
    );
}

#[test]
fn should_produce_the_same_plan_twice_from_the_same_world() {
    let registry = zfs();
    let assets = assets();
    assert_eq!(
        nginx_recovery(&registry, &assets),
        nginx_recovery(&registry, &assets),
        "§4.4: the seal is over the plan, so two derivations of one plan compare equal"
    );
}

#[test]
fn should_bind_the_provider_at_the_version_the_plan_was_resolved_against() {
    let registry = zfs();
    let recovery = nginx_recovery(&registry, &assets());
    assert_eq!(
        recovery.plan().providers().len(),
        1,
        "§4.4: a plan resolved against one provider version is not the same plan against another"
    );
    assert_eq!(recovery.plan().providers()[0].id(), PROVIDER);
}

// -- §24.3, §24.4: what the plan shows ---------------------------------------------------------

#[test]
fn should_name_the_state_the_recovery_would_restore() {
    let registry = zfs();
    let recovery = nginx_recovery(&registry, &assets());
    assert_eq!(
        recovery.target_state(),
        SNAPSHOT,
        "§24.4 names the recovery asset the plan restores from"
    );
}

#[test]
fn should_name_the_object_the_recovery_would_restore() {
    let registry = zfs();
    let recovery = nginx_recovery(&registry, &assets());
    assert_eq!(recovery.restores(), &[Arc::from(NGINX_CONF)]);
}

#[test]
fn should_name_the_plan_being_recovered_from() {
    let registry = zfs();
    let recovery = nginx_recovery(&registry, &assets());
    assert_eq!(recovery.source_plan(), Some(nginx_plan().id()));
}

#[test]
fn should_name_the_assets_the_recovery_consumes() {
    let registry = zfs();
    let assets = assets();
    let recovery = nginx_recovery(&registry, &assets);
    assert_eq!(recovery.source_assets(), &[assets[0].id().clone()]);
}

#[test]
fn should_choose_a_selective_restore_for_a_single_configuration_object() {
    let registry = zfs();
    let recovery = nginx_recovery(&registry, &assets());
    assert_eq!(
        recovery.method(),
        RestoreMethod::SelectiveFileRestore,
        "§24.4: the method is a selective restore from the snapshot"
    );
}

#[test]
fn should_preserve_the_unrelated_newer_files_of_the_worked_example() {
    let registry = zfs();
    let recovery = nginx_recovery(&registry, &assets());
    let preserved: Vec<&str> = recovery
        .newer_state()
        .preserved()
        .iter()
        .map(|item| item.object())
        .collect();
    assert!(
        preserved.contains(&"/etc/hosts") && preserved.contains(&"/etc/ssh/sshd_config"),
        "§24.4's `newer state preserved`, and it preserved {preserved:?}"
    );
}

#[test]
fn should_classify_the_recovery_as_moderate_risk_when_nothing_unrelated_is_lost() {
    let registry = zfs();
    let recovery = nginx_recovery(&registry, &assets());
    assert_eq!(
        recovery.plan().risk().classify(),
        RiskClass::Moderate,
        "§24.4: the worked example's risk is MODERATE"
    );
}

#[test]
fn should_need_no_destructive_acceptance_for_the_worked_example() {
    let registry = zfs();
    let recovery = nginx_recovery(&registry, &assets());
    assert!(
        !recovery.needs_destructive_acceptance(),
        "§40.1 applied to recovery: the ordinary selective restore stays usable"
    );
}

#[test]
fn should_report_the_recovery_as_complete_when_nothing_is_beyond_its_reach() {
    let registry = zfs();
    let recovery = nginx_recovery(&registry, &assets());
    assert!(recovery.is_complete());
}

#[test]
fn should_run_the_drift_analysis_rather_than_leaving_it_undone() {
    let registry = zfs();
    let recovery = nginx_recovery(&registry, &assets());
    assert!(
        recovery.newer_state().is_complete(),
        "§62.8: recovering hours later without considering newer state is unacceptable"
    );
}

#[test]
fn should_keep_newer_extra_files_when_restoring_a_directory_by_default() {
    let registry = zfs();
    let recovery = nginx_recovery(&registry, &assets());
    assert_eq!(
        recovery.directory_restore_policy(),
        DirectoryRestorePolicy::KeepExtraFiles,
        "Appendix C.6: the default MUST NOT delete newer extra files"
    );
}

#[test]
fn should_show_the_metadata_the_chosen_restore_does_not_return() {
    let registry = zfs();
    let recovery = nginx_recovery(&registry, &assets());
    assert!(
        recovery.metadata().gaps().contains(&"SELinux labels"),
        "Appendix C.7: missing metadata support MUST be visible in the plan"
    );
}

#[test]
fn should_carry_the_metadata_coverage_the_chosen_provider_declared() {
    let registry = registry(vec![
        TestProvider::new(PROVIDER)
            .restoring_metadata(content_mode_owner())
            .shared(),
    ]);
    let recovery = nginx_recovery(&registry, &assets());
    assert!(
        recovery.metadata().owner,
        "Appendix C.7: owner/group is restored"
    );
    assert!(!recovery.metadata().acl);
}

#[test]
fn should_record_that_recovery_needs_a_reboot_when_the_provider_says_so() {
    let registry = registry(vec![TestProvider::new(PROVIDER).needing_reboot().shared()]);
    let recovery = nginx_recovery(&registry, &assets());
    assert!(
        recovery.requires_reboot(),
        "§13.7: Ono does not promise online rollback merely because a snapshot exists"
    );
}

#[test]
fn should_record_that_recovery_needs_the_filesystem_offline_when_the_provider_says_so() {
    let registry = registry(vec![TestProvider::new(PROVIDER).needing_offline().shared()]);
    let recovery = nginx_recovery(&registry, &assets());
    assert!(recovery.requires_offline(), "§14.6");
}

#[test]
fn should_raise_the_risk_when_the_recovery_needs_the_filesystem_offline() {
    let registry = registry(vec![TestProvider::new(PROVIDER).needing_offline().shared()]);
    let recovery = nginx_recovery(&registry, &assets());
    assert_eq!(
        recovery.plan().risk().classify(),
        RiskClass::High,
        "§19.1's downtime dimension"
    );
}

#[test]
fn should_raise_the_risk_when_the_method_rolls_a_whole_dataset_back() {
    let registry = registry(vec![
        TestProvider::new(PROVIDER)
            .restoring_by(RestoreMethod::DatasetRollback)
            .shared(),
    ]);
    let recovery = nginx_recovery(&registry, &assets());
    assert_eq!(
        recovery.plan().risk().classify(),
        RiskClass::High,
        "§19.1: a rollback that discards everything since is not a moderate change"
    );
}

// -- §11.4: an asset that cannot be restored from --------------------------------------------

fn refusal_for_asset(asset: RecoveryAsset) -> ErrorValue {
    let registry = zfs();
    let assets = vec![asset];
    let source = nginx_plan();
    let observe = world;
    plan_recovery(
        &RecoveryRequest::new(
            &registry,
            &assets,
            &observe,
            RecoveryGoal::RestoreChangedObjects,
            "session-recovery",
            at(16, 0),
        )
        .recovering(&source)
        .restoring(NGINX_CONF),
    )
    .expect_err("§11.4: only a validated asset can be recovered from")
}

#[test]
fn should_refuse_to_plan_recovery_from_an_expired_asset() {
    let error = refusal_for_asset(
        ready_asset(
            PROVIDER,
            SNAPSHOT,
            "rpool/ROOT/debian",
            &[NGINX_CONF],
            at(14, 2),
        )
        .expiring_at(at(15, 0))
        .expired(),
    );
    assert_eq!(
        error.code().name(),
        "recovery.asset_expired",
        "§37.1: assets are retained for a bounded window"
    );
}

#[test]
fn should_refuse_to_plan_recovery_from_an_invalid_asset() {
    let error = refusal_for_asset(
        ready_asset(
            PROVIDER,
            SNAPSHOT,
            "rpool/ROOT/debian",
            &[NGINX_CONF],
            at(14, 2),
        )
        .invalidated(),
    );
    assert_eq!(
        error.code().name(),
        "recovery.asset_invalid",
        "Appendix D.10: an asset that can no longer satisfy protection is not a way back"
    );
}

#[test]
fn should_refuse_to_plan_recovery_from_a_removed_asset() {
    let error = refusal_for_asset(
        ready_asset(
            PROVIDER,
            SNAPSHOT,
            "rpool/ROOT/debian",
            &[NGINX_CONF],
            at(14, 2),
        )
        .removed(),
    );
    assert_eq!(error.code().name(), "recovery.asset_not_found", "§37");
}

#[test]
fn should_refuse_to_plan_recovery_from_an_asset_whose_creation_failed() {
    let error = refusal_for_asset(
        ready_asset(
            PROVIDER,
            SNAPSHOT,
            "rpool/ROOT/debian",
            &[NGINX_CONF],
            at(14, 2),
        )
        .failed(),
    );
    assert_eq!(error.code().name(), "recovery.asset_invalid", "§11.1");
}

#[test]
fn should_refuse_to_plan_recovery_from_an_asset_that_was_only_ever_proposed() {
    let error = refusal_for_asset(RecoveryAsset::proposed(
        PROVIDER,
        ono_change_core::RecoveryAssetType::ZfsSnapshot,
        SNAPSHOT,
        ono_change_core::RecoveryScope::new("zfs-dataset", "rpool/ROOT/debian", "localhost")
            .covering(NGINX_CONF),
        at(14, 2),
    ));
    assert_eq!(
        error.code().name(),
        "recovery.asset_invalid",
        "§2.1: a plan that mentions an asset has not created one"
    );
}

#[test]
fn should_refuse_to_plan_recovery_with_no_asset_at_all() {
    let registry = zfs();
    let assets: Vec<RecoveryAsset> = Vec::new();
    let observe = world;
    let error = plan_recovery(&RecoveryRequest::new(
        &registry,
        &assets,
        &observe,
        RecoveryGoal::RestoreChangedObjects,
        "session-recovery",
        at(16, 0),
    ))
    .expect_err("§56.3: nothing to restore from is not a recovery plan");
    assert_eq!(error.code().name(), "recovery.plan_incomplete");
}

#[test]
fn should_refuse_when_the_chosen_provider_contributes_no_action_to_run() {
    let registry = registry(vec![
        TestProvider::new(PROVIDER).acting_on_nothing().shared(),
    ]);
    let assets = assets();
    let source = nginx_plan();
    let observe = world;
    let error = plan_recovery(
        &RecoveryRequest::new(
            &registry,
            &assets,
            &observe,
            RecoveryGoal::RestoreChangedObjects,
            "session-recovery",
            at(16, 0),
        )
        .recovering(&source)
        .restoring(NGINX_CONF),
    )
    .expect_err("§56.3: a recovery plan with nothing to apply is not a plan");
    assert_eq!(error.code().name(), "recovery.plan_incomplete");
}

#[test]
fn should_refuse_a_recovery_method_the_engine_cannot_choose() {
    let registry = zfs();
    let assets = assets();
    let source = nginx_plan();
    let observe = world;
    let error = plan_recovery(
        &RecoveryRequest::new(
            &registry,
            &assets,
            &observe,
            RecoveryGoal::RestoreChangedObjects,
            "session-recovery",
            at(16, 0),
        )
        .recovering(&source)
        .forcing_method("configuration-merge"),
    )
    .expect_err("Appendix C.5: automatic semantic merging is an explicit non-goal");
    assert_eq!(error.code().name(), "change.action_not_plannable");
}

// -- §55.8, §35.2, §35.3, Appendix F.2: what recovery cannot reach -----------------------------

/// A plan that writes a config on the protected dataset and a database on another one.
fn spanning_plan() -> ChangePlan {
    let plan = ChangePlan::draft(
        Intent::new("write config and database", "plan { ... }"),
        "session-span",
        EPOCH,
    );
    let config = mutating(&plan, 1, "replace nginx.conf", |effect| {
        effect.on(NGINX_CONF)
    });
    let database = PlanAction::new(
        plan.id(),
        2,
        ActionRole::Mutate,
        "write /data/customer.db",
        program("ono.file.write"),
    )
    .effecting(
        ProposedEffect::new(
            ActionId::of(plan.id(), 2, "write /data/customer.db"),
            EffectDomain::FilesystemPersistent,
            EffectKind::Modify,
            EffectConfidence::Guaranteed,
            "the database file is written",
        )
        .on("/data/customer.db"),
    );
    seal(plan, vec![config, database])
}

fn program(operation: &str) -> Execution {
    Execution::ProviderAction {
        provider: Arc::from("linux.files"),
        operation: Arc::from(operation),
        arguments: Vec::new(),
    }
}

fn mutating(
    plan: &ChangePlan,
    ordinal: usize,
    summary: &str,
    shape: impl FnOnce(ProposedEffect) -> ProposedEffect,
) -> PlanAction {
    let effect = ProposedEffect::new(
        ActionId::of(plan.id(), ordinal, summary),
        EffectDomain::FilesystemPersistent,
        EffectKind::Modify,
        EffectConfidence::Guaranteed,
        "the file is replaced",
    );
    PlanAction::new(
        plan.id(),
        ordinal,
        ActionRole::Mutate,
        summary,
        program("ono.file.write"),
    )
    .effecting(shape(effect))
}

fn seal(plan: ChangePlan, actions: Vec<PlanAction>) -> ChangePlan {
    let verification = VerificationSet::empty().with(VerificationContract::new(
        plan.id(),
        VerificationClass::Required,
        "the change",
        "applied == true",
    ));
    let mut sealed = plan;
    for action in actions {
        sealed = sealed
            .with_action(action)
            .expect("a draft accepts an action");
    }
    sealed
        .with_verification(verification)
        .with_protection(protected_filesystem())
        .seal(EPOCH)
        .expect("a plan with a required contract seals")
}

fn recovery_over(source: &ChangePlan) -> RecoveryPlan {
    let registry = zfs();
    let assets = assets();
    let observe = world;
    plan_recovery(
        &RecoveryRequest::new(
            &registry,
            &assets,
            &observe,
            RecoveryGoal::RestoreChangedObjects,
            "session-recovery",
            at(16, 0),
        )
        .recovering(source)
        .applied_at(at(14, 3)),
    )
    .expect("the asset is validated and the method reaches the goal")
}

#[test]
fn should_say_that_recovery_reaches_only_what_the_assets_cover() {
    let recovery = recovery_over(&spanning_plan());
    let beyond: Vec<&str> = recovery
        .unrecoverable()
        .iter()
        .map(ono_change_core::UnrecoverableEffect::subject)
        .collect();
    assert!(
        beyond.contains(&"/data/customer.db"),
        "§55.8: the source assets cover only part of what the plan changed, and it named {beyond:?}"
    );
}

#[test]
fn should_not_report_the_covered_object_as_beyond_reach() {
    let recovery = recovery_over(&spanning_plan());
    assert!(
        !recovery
            .unrecoverable()
            .iter()
            .any(|effect| effect.subject() == NGINX_CONF),
        "§13.4: the object the asset actually covers is restored"
    );
}

#[test]
fn should_report_a_partial_recovery_as_incomplete() {
    let recovery = recovery_over(&spanning_plan());
    assert!(
        !recovery.is_complete(),
        "§2.4: a recovery that reaches part of the change does not claim to reach all of it"
    );
}

/// A plan that posts a deployment webhook, for which the provider declares a compensation.
fn emitting_plan() -> ChangePlan {
    let plan = ChangePlan::draft(
        Intent::new(
            "write config and notify the deployment hook",
            "plan { ... }",
        ),
        "session-emit",
        EPOCH,
    );
    let config = mutating(&plan, 1, "replace nginx.conf", |effect| {
        effect.on(NGINX_CONF)
    });
    let webhook = PlanAction::new(
        plan.id(),
        2,
        ActionRole::Mutate,
        "post the deployment webhook",
        program("ono.http.post"),
    )
    .effecting(
        ProposedEffect::new(
            ActionId::of(plan.id(), 2, "post the deployment webhook"),
            EffectDomain::ExternalSideEffect,
            EffectKind::Emit,
            EffectConfidence::Guaranteed,
            "a deployment webhook request is sent",
        )
        .on("deployment webhook")
        .compensated_by("post a rollback notification"),
    );
    seal(plan, vec![config, webhook])
}

#[test]
fn should_keep_an_emitted_effect_unrecoverable_even_when_a_provider_offers_a_compensation() {
    let recovery = recovery_over(&emitting_plan());
    let webhook = recovery
        .unrecoverable()
        .iter()
        .find(|effect| effect.subject() == "deployment webhook")
        .expect("§35.2: such effects MUST remain separately visible");
    assert_eq!(
        webhook.compensation(),
        Some("post a rollback notification"),
        "§35.3: a provider MAY define a compensating action"
    );
}

#[test]
fn should_not_call_a_compensated_external_effect_recovered() {
    let recovery = recovery_over(&emitting_plan());
    assert!(
        !recovery.is_complete(),
        "§35.3 and §27.4: compensation is COMPENSATABLE, and never labelled rollback"
    );
}

#[test]
fn should_report_a_remote_effect_as_beyond_a_local_snapshots_reach() {
    let plan = ChangePlan::draft(
        Intent::new("write config on another host", "plan { ... }"),
        "session-remote",
        EPOCH,
    );
    let remote = PlanAction::new(
        plan.id(),
        1,
        ActionRole::Mutate,
        "write /etc/app.conf on web-02",
        program("ono.file.write"),
    )
    .effecting(
        ProposedEffect::new(
            ActionId::of(plan.id(), 1, "write /etc/app.conf on web-02"),
            EffectDomain::RemoteSystem,
            EffectKind::Modify,
            EffectConfidence::Expected,
            "the remote file is replaced",
        )
        .on("web-02:/etc/app.conf"),
    );
    let recovery = recovery_over(&seal(plan, vec![remote]));
    assert!(
        recovery
            .unrecoverable()
            .iter()
            .any(|effect| effect.subject() == "web-02:/etc/app.conf"),
        "§35.2: local snapshots do not reach it"
    );
}

#[test]
fn should_report_an_interrupted_connection_as_beyond_reach() {
    let plan = ChangePlan::draft(
        Intent::new("replace the default route", "plan { ... }"),
        "session-net",
        EPOCH,
    );
    let route = PlanAction::new(
        plan.id(),
        1,
        ActionRole::Mutate,
        "replace the default route",
        program("ono.route.replace"),
    )
    .effecting(
        ProposedEffect::new(
            ActionId::of(plan.id(), 1, "replace the default route"),
            EffectDomain::NetworkRuntime,
            EffectKind::Interrupt,
            EffectConfidence::Possible,
            "established sessions may be cut",
        )
        .on("established TCP sessions"),
    );
    let recovery = recovery_over(&seal(plan, vec![route]));
    assert!(
        recovery
            .unrecoverable()
            .iter()
            .any(|effect| effect.subject() == "established TCP sessions"),
        "§34: live sessions are not restored by any recovery asset"
    );
}

#[test]
fn should_not_report_a_restarted_services_workers_as_beyond_reach() {
    let registry = zfs();
    let recovery = nginx_recovery(&registry, &assets());
    assert!(
        !recovery
            .unrecoverable()
            .iter()
            .any(|effect| effect.subject() == "nginx.service workers"),
        "§25.2: new worker PIDs are an expected difference rather than an unrecoverable effect"
    );
}

/// A plan whose second action's outcome could not be established (Appendix F.2).
fn uncertain_plan() -> ChangePlan {
    let plan = ChangePlan::draft(
        Intent::new("write config and restart the remote agent", "plan { ... }"),
        "session-unknown",
        EPOCH,
    );
    let config = mutating(&plan, 1, "replace nginx.conf", |effect| {
        effect.on(NGINX_CONF)
    })
    .with_status(ActionStatus::Succeeded);
    let uncertain = PlanAction::new(
        plan.id(),
        2,
        ActionRole::Mutate,
        "restart the agent on web-02",
        program("ono.service.restart"),
    )
    .with_status(ActionStatus::Unknown);
    seal(plan, vec![config, uncertain])
}

#[test]
fn should_treat_an_unestablished_action_outcome_as_an_uncertainty_boundary() {
    let recovery = recovery_over(&uncertain_plan());
    assert!(
        recovery
            .unrecoverable()
            .iter()
            .any(|effect| effect.subject() == "restart the agent on web-02"),
        "Appendix F.2: recovery planning must treat an unknown outcome as an uncertainty boundary"
    );
}

#[test]
fn should_not_claim_a_complete_recovery_when_an_action_outcome_is_unknown() {
    let recovery = recovery_over(&uncertain_plan());
    assert!(
        !recovery.is_complete(),
        "Appendix F.2: the outcome could not be established, so recovery does not assume either"
    );
}

#[test]
fn should_say_that_the_unknown_actions_outcome_could_not_be_established() {
    let recovery = recovery_over(&uncertain_plan());
    let boundary = recovery
        .unrecoverable()
        .iter()
        .find(|effect| effect.subject() == "restart the agent on web-02")
        .expect("the uncertainty boundary is in the plan");
    assert!(
        boundary.reason().contains("could not be established"),
        "§29.3: Ono MUST NOT mark an unknown action failed or successful without evidence"
    );
}

// -- §5.8: recovering from an asset with no plan in hand ---------------------------------------

#[test]
fn should_plan_recovery_from_an_asset_with_no_source_plan() {
    let registry = zfs();
    let assets = assets();
    let observe = world;
    let recovery = plan_recovery(
        &RecoveryRequest::new(
            &registry,
            &assets,
            &observe,
            RecoveryGoal::RestoreChangedObjects,
            "session-recovery",
            at(16, 0),
        )
        .restoring(NGINX_CONF),
    )
    .expect("§5.8: `recover` accepts an asset directly");
    assert_eq!(recovery.source_plan(), None);
    assert_eq!(recovery.restores(), &[Arc::from(NGINX_CONF)]);
}

#[test]
fn should_restore_what_the_plan_changed_when_no_object_was_named() {
    let recovery = recovery_over(&nginx_plan());
    assert_eq!(
        recovery.restores(),
        &[Arc::from(NGINX_CONF)],
        "Appendix C.2: the goal is the objects the plan changed, not every file in /etc"
    );
}

#[test]
fn should_restore_what_the_asset_covers_when_there_is_neither_a_plan_nor_a_named_object() {
    let registry = zfs();
    let assets = assets();
    let observe = world;
    let recovery = plan_recovery(&RecoveryRequest::new(
        &registry,
        &assets,
        &observe,
        RecoveryGoal::RestoreChangedObjects,
        "session-recovery",
        at(16, 0),
    ))
    .expect("§5.8");
    assert_eq!(recovery.restores().len(), 3);
}

// -- §24.2: the analysis the gate rests on -----------------------------------------------------

#[test]
fn should_gate_a_recovery_whose_target_was_edited_again_after_the_plan() {
    let registry = zfs();
    let assets = assets();
    let source = nginx_plan();
    let observe = |object: &str| {
        if object == NGINX_CONF {
            ObservedState::changed(at(15, 12), "sha256:user-edit")
        } else {
            ObservedState::changed(at(15, 0), "sha256:other")
        }
    };
    let recovery = plan_recovery(
        &RecoveryRequest::new(
            &registry,
            &assets,
            &observe,
            RecoveryGoal::RestoreChangedObjects,
            "session-recovery",
            at(16, 0),
        )
        .recovering(&source)
        .applied_at(at(14, 3))
        .restoring(NGINX_CONF),
    )
    .expect("the plan is produced, and the gate decides whether it may run");
    assert!(
        recovery.needs_destructive_acceptance(),
        "Appendix C.4: recovery would discard the 15:12 edit"
    );
}

#[test]
fn should_gate_a_recovery_whose_scope_could_not_be_observed() {
    let registry = zfs();
    let assets = assets();
    let source = nginx_plan();
    let observe = |_object: &str| ObservedState::unestablished("the dataset did not answer");
    let recovery = plan_recovery(
        &RecoveryRequest::new(
            &registry,
            &assets,
            &observe,
            RecoveryGoal::RestoreChangedObjects,
            "session-recovery",
            at(16, 0),
        )
        .recovering(&source)
        .restoring(NGINX_CONF),
    )
    .expect("the plan is produced and says what it could not establish");
    assert!(
        recovery.needs_destructive_acceptance(),
        "§56.3: an unestablished recovery fact is a reason to block"
    );
}

#[test]
fn should_carry_the_provider_native_objects_a_rollback_would_destroy() {
    let registry = registry(vec![
        TestProvider::new(PROVIDER)
            .restoring_by(RestoreMethod::DatasetRollback)
            .shared(),
    ]);
    let assets = assets();
    let observe = world;
    let recovery = plan_recovery(
        &RecoveryRequest::new(
            &registry,
            &assets,
            &observe,
            RecoveryGoal::RestoreChangedObjects,
            "session-recovery",
            at(16, 0),
        )
        .destroying("tank/data@later-1")
        .destroying("tank/data@later-2")
        .restoring(NGINX_CONF),
    )
    .expect("the plan is produced");
    assert_eq!(recovery.newer_state().destroyed_assets().len(), 2, "§24.5");
    assert!(recovery.needs_destructive_acceptance(), "§13.6");
}
