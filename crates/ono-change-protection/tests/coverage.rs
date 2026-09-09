//! The protection coverage algorithm (v0.6 Appendix A, §10, §17.2).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::path::Path;

use ono_change_core::{
    ActionId, ConsistencyClass, EffectConfidence, EffectDomain, EffectKind, PersistenceDomain,
    PlanId, ProposedEffect, ProtectionLevel, ProtectionMode, RecoveryCandidate, RecoveryExclusion,
    RecoveryObjective, RestoreMethod,
};
use ono_change_protection::coverage::{
    CoverageRequest, MutationDomain, RejectionReason, analyse, objective_for, ranked,
};
use ono_change_protection::policy::{CostLimits, ProtectionPolicy};
use ono_change_protection::{MountTable, ProviderRegistry};
use ono_value::ByteSize;

mod support;

use support::{
    TestProvider, ZFS_ROOT, archive_cost, candidate, config_mutation, file_archive, snapshot_cost,
};

fn nginx_conf() -> PersistenceDomain {
    MountTable::from_text(ZFS_ROOT).resolve(Path::new("/etc/nginx/nginx.conf"))
}

fn dataset_snapshot() -> RecoveryCandidate {
    candidate(
        "ono.recovery.zfs",
        "zfs-dataset",
        "rpool/ROOT/debian",
        &["/etc/nginx/nginx.conf", "/etc/hosts", "/usr/lib/systemd"],
        EffectDomain::FilesystemPersistent,
        RecoveryObjective::PreserveExact,
    )
    .at_consistency(ConsistencyClass::FilesystemConsistent)
    .restored_by(RestoreMethod::DatasetRollback)
    .costing(snapshot_cost())
}

fn registry_with(
    providers: Vec<std::sync::Arc<dyn ono_change_core::RecoveryProvider>>,
) -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();
    for provider in providers {
        registry
            .register(provider)
            .expect("the fixtures declare every §12.2 capability");
    }
    registry
}

#[test]
fn should_reproduce_the_appendix_a_6_example_when_a_config_change_restarts_a_service() {
    let registry = registry_with(vec![
        TestProvider::new("ono.recovery.zfs")
            .offering(dataset_snapshot().restored_by(RestoreMethod::SelectiveFileRestore))
            .shared(),
    ]);
    let policy = ProtectionPolicy::default();
    let analysis = analyse(
        &CoverageRequest::new(&registry, &policy)
            .over(nginx_conf())
            .mutating(config_mutation())
            .mutating(
                MutationDomain::new(
                    EffectDomain::ProcessRuntime,
                    EffectKind::Replace,
                    "nginx.service",
                    "the service is restarted",
                )
                .compensated_by("restart service nginx"),
            )
            .mutating(MutationDomain::new(
                EffectDomain::NetworkRuntime,
                EffectKind::Interrupt,
                "nginx connections",
                "in-flight connections are cut",
            )),
    );

    assert_eq!(
        analysis.level(),
        ProtectionLevel::Protected,
        "Appendix A.6: the persistent portion is protected while the runtime portion is not"
    );
    assert!(
        analysis
            .summary()
            .exclusions()
            .iter()
            .any(|exclusion| exclusion.domain() == EffectDomain::NetworkRuntime),
        "Appendix A.6: `PROTECTED` is displayed only with the exclusions shown"
    );
    assert!(
        !analysis.summary().exclusions().is_empty(),
        "Appendix A.6 forbids a global green safe indicator, so the exclusions travel with the word"
    );
}

#[test]
fn should_prefer_a_configuration_backup_over_a_root_dataset_rollback_for_one_file() {
    let registry = registry_with(vec![
        TestProvider::new("ono.recovery.zfs")
            .offering(dataset_snapshot())
            .shared(),
        TestProvider::new("ono.recovery.files")
            .offering(file_archive("ono.recovery.files", "/etc/nginx/nginx.conf"))
            .shared(),
    ]);
    let policy = ProtectionPolicy::default();
    let analysis = analyse(
        &CoverageRequest::new(&registry, &policy)
            .over(nginx_conf())
            .mutating(config_mutation()),
    );

    assert_eq!(analysis.actions().len(), 1);
    assert_eq!(
        analysis.actions()[0].provider(),
        "ono.recovery.files",
        "Appendix A.4: a small configuration-file backup dominates a root-dataset rollback for one \
         file, because its recovery blast radius is smaller"
    );
    let rejected = analysis
        .rejected()
        .iter()
        .find(|rejected| rejected.candidate().provider() == "ono.recovery.zfs")
        .expect("the rollback was considered and lost");
    assert_eq!(rejected.reason(), RejectionReason::Dominated);
    assert!(
        rejected.detail().contains("Appendix A.4"),
        "the losing candidate says which rule it lost to: {}",
        rejected.detail()
    );
}

#[test]
fn should_prefer_the_least_destructive_restore_when_two_candidates_cover_the_same_objects() {
    let selective = candidate(
        "ono.recovery.zfs",
        "zfs-dataset",
        "rpool/ROOT/debian",
        &["/etc/nginx/nginx.conf"],
        EffectDomain::FilesystemPersistent,
        RecoveryObjective::PreserveExact,
    )
    .at_consistency(ConsistencyClass::FilesystemConsistent)
    .restored_by(RestoreMethod::SelectiveFileRestore)
    .costing(snapshot_cost());
    let rollback = candidate(
        "ono.recovery.rollback",
        "zfs-dataset",
        "rpool/ROOT/debian",
        &["/etc/nginx/nginx.conf"],
        EffectDomain::FilesystemPersistent,
        RecoveryObjective::PreserveExact,
    )
    .at_consistency(ConsistencyClass::FilesystemConsistent)
    .restored_by(RestoreMethod::DatasetRollback)
    .costing(snapshot_cost());

    let ordered = ranked(&[rollback, selective], RecoveryObjective::PreserveExact);
    assert_eq!(
        ordered[0].restore_method(),
        RestoreMethod::SelectiveFileRestore,
        "Appendix A.4 key 4 and Appendix C.1: the least destructive recovery wins a tie on scope"
    );
}

#[test]
fn should_never_prefer_a_candidate_whose_consistency_the_provider_could_not_establish() {
    let unknown = candidate(
        "ono.recovery.mystery",
        "file",
        "/etc/nginx/nginx.conf",
        &["/etc/nginx/nginx.conf"],
        EffectDomain::FilesystemPersistent,
        RecoveryObjective::PreserveExact,
    )
    .restored_by(RestoreMethod::SelectiveFileRestore);
    let known = dataset_snapshot();

    let ordered = ranked(&[unknown, known], RecoveryObjective::PreserveExact);
    assert_eq!(
        ordered[0].provider(),
        "ono.recovery.zfs",
        "§2.4 and §39.1: an unestablished consistency satisfies nothing, so it loses A.4's first key"
    );
}

#[test]
fn should_leave_a_domain_unprotected_and_in_the_shortfall_when_nothing_offers_a_candidate() {
    let registry = registry_with(vec![TestProvider::new("ono.recovery.zfs").shared()]);
    let policy = ProtectionPolicy::default();
    let analysis = analyse(
        &CoverageRequest::new(&registry, &policy)
            .over(nginx_conf())
            .mutating(config_mutation()),
    );

    assert_eq!(analysis.level(), ProtectionLevel::Unprotected);
    assert_eq!(
        analysis.shortfall().len(),
        1,
        "§10.3: the domain that needs coverage and has none is named in the matrix"
    );
    assert_eq!(
        analysis.shortfall()[0].domain(),
        EffectDomain::FilesystemPersistent
    );
    assert!(analysis.actions().is_empty());
}

#[test]
fn should_leave_a_tmpfs_target_unprotected_with_the_reason_in_the_row() {
    let volatile = MountTable::from_text(ZFS_ROOT).resolve(Path::new("/run/app/state"));
    let registry = registry_with(vec![
        TestProvider::new("ono.recovery.zfs")
            .offering(dataset_snapshot())
            .shared(),
    ]);
    let policy = ProtectionPolicy::default();
    let analysis = analyse(
        &CoverageRequest::new(&registry, &policy)
            .over(volatile)
            .mutating(MutationDomain::new(
                EffectDomain::FilesystemPersistent,
                EffectKind::Modify,
                "/run/app/state",
                "the runtime state file is rewritten",
            )),
    );

    assert_eq!(analysis.level(), ProtectionLevel::Unprotected);
    assert!(
        analysis.summary().rows()[0].note().contains("tmpfs"),
        "§32.4: the row says the path is on a volatile filesystem: {}",
        analysis.summary().rows()[0].note()
    );
}

#[test]
fn should_cap_a_plan_at_partially_protected_when_an_unknown_domain_is_present() {
    let registry = registry_with(vec![
        TestProvider::new("ono.recovery.zfs")
            .offering(dataset_snapshot().restored_by(RestoreMethod::SelectiveFileRestore))
            .shared(),
    ]);
    let policy = ProtectionPolicy::default();
    let analysis = analyse(
        &CoverageRequest::new(&registry, &policy)
            .over(nginx_conf())
            .mutating(config_mutation())
            .mutating(MutationDomain::new(
                EffectDomain::Unknown,
                EffectKind::Unknown,
                "vendor-tool --apply",
                "an opaque action whose effects Ono cannot classify",
            )),
    );

    assert_eq!(
        analysis.level(),
        ProtectionLevel::PartiallyProtected,
        "Appendix A.7 and §6.3: an opaque action MUST NOT inherit safety from an unrelated snapshot"
    );
    assert!(
        analysis
            .summary()
            .exclusions()
            .iter()
            .any(|exclusion| exclusion.domain() == EffectDomain::Unknown),
        "§6.3: the unknown domain stays visible beside the protection that does exist"
    );
}

#[test]
fn should_lift_the_unknown_cap_only_when_policy_declares_the_domain_irrelevant() {
    let registry = registry_with(vec![
        TestProvider::new("ono.recovery.zfs")
            .offering(dataset_snapshot().restored_by(RestoreMethod::SelectiveFileRestore))
            .shared(),
    ]);
    let policy = ProtectionPolicy::default().declaring_irrelevant(EffectDomain::Unknown);
    let analysis = analyse(
        &CoverageRequest::new(&registry, &policy)
            .over(nginx_conf())
            .mutating(config_mutation())
            .mutating(MutationDomain::new(
                EffectDomain::Unknown,
                EffectKind::Unknown,
                "vendor-tool --report",
                "an opaque action the operator declared irrelevant to recovery",
            )),
    );

    assert_eq!(
        analysis.level(),
        ProtectionLevel::Protected,
        "Appendix A.7: only an explicit policy declaration stops an unknown domain capping the plan"
    );
}

#[test]
fn should_require_exact_preservation_for_a_persistent_file_modification() {
    assert_eq!(
        objective_for(EffectDomain::FilesystemPersistent, EffectKind::Modify),
        RecoveryObjective::PreserveExact,
        "Appendix A.2: file and configuration state is where the operator expects the prior bytes"
    );
}

#[test]
fn should_require_only_semantic_restoration_for_a_service_restart() {
    assert_eq!(
        objective_for(EffectDomain::ProcessRuntime, EffectKind::Replace),
        RecoveryObjective::RestoreSemantic,
        "Appendix A.2: a restarted service's worker PIDs may legitimately change"
    );
}

#[test]
fn should_require_no_recovery_for_a_network_session_and_keep_it_as_an_exclusion() {
    let mutation = MutationDomain::new(
        EffectDomain::NetworkRuntime,
        EffectKind::Interrupt,
        "established sessions",
        "sessions are cut",
    );
    assert_eq!(mutation.objective(), RecoveryObjective::NoRecoveryRequired);
    let exclusion = ono_change_protection::coverage::exclusion_for(&mutation)
        .expect("Appendix A.5 keeps runtime effects visible as exclusions");
    assert!(
        !exclusion.is_irreversible(),
        "§34: a cut session is uncovered rather than irreversible"
    );
}

#[test]
fn should_mark_an_emitted_external_effect_irreversible_rather_than_merely_uncovered() {
    let mutation = MutationDomain::new(
        EffectDomain::ExternalSideEffect,
        EffectKind::Emit,
        "POST https://api.example.com/deploy",
        "the webhook is called",
    );
    assert_eq!(mutation.objective(), RecoveryObjective::NoRecoveryRequired);
    let exclusion = ono_change_protection::coverage::exclusion_for(&mutation)
        .expect("§35.1 keeps an external side effect visible");
    assert!(
        exclusion.is_irreversible(),
        "§2.13 and §35.1: the call has left the system and no recovery asset touches it"
    );
}

#[test]
fn should_keep_an_unknown_recovery_semantic_unknown_rather_than_calling_it_protection() {
    let mystery = candidate(
        "ono.recovery.mystery",
        "file",
        "rpool/ROOT/debian",
        &["/etc/nginx/nginx.conf"],
        EffectDomain::FilesystemPersistent,
        RecoveryObjective::PreserveExact,
    )
    .restored_by(RestoreMethod::SelectiveFileRestore);
    let registry = registry_with(vec![
        TestProvider::new("ono.recovery.mystery")
            .offering(mystery)
            .shared(),
    ]);
    let policy = ProtectionPolicy::default();
    let analysis = analyse(
        &CoverageRequest::new(&registry, &policy)
            .over(nginx_conf())
            .mutating(config_mutation()),
    );

    assert_eq!(
        analysis.level(),
        ProtectionLevel::Unknown,
        "§55.6 case 29: unknown provider recovery semantics remain UNKNOWN"
    );
    assert_eq!(analysis.shortfall().len(), 1);
}

#[test]
fn should_report_available_protection_while_creating_none_when_the_mode_is_off() {
    let registry = registry_with(vec![
        TestProvider::new("ono.recovery.zfs")
            .offering(dataset_snapshot())
            .shared(),
    ]);
    let policy = ProtectionPolicy::of(ProtectionMode::Off);
    let analysis = analyse(
        &CoverageRequest::new(&registry, &policy)
            .over(nginx_conf())
            .mutating(config_mutation()),
    );

    assert!(
        analysis.actions().is_empty(),
        "§17.2: `off` creates no automatic recovery assets"
    );
    assert_eq!(
        analysis.available().len(),
        1,
        "§17.2: `off` still shows available protection opportunities"
    );
    assert_eq!(
        analysis.rejected()[0].reason(),
        RejectionReason::ProtectionOff
    );
    assert_eq!(analysis.level(), ProtectionLevel::Unprotected);
}

#[test]
fn should_add_a_second_non_conflicting_mechanism_when_the_mode_is_maximize() {
    let registry = registry_with(vec![
        TestProvider::new("ono.recovery.zfs")
            .offering(dataset_snapshot())
            .shared(),
        TestProvider::new("ono.recovery.files")
            .offering(file_archive("ono.recovery.files", "/etc/nginx/nginx.conf"))
            .shared(),
    ]);
    let policy = ProtectionPolicy::of(ProtectionMode::Maximize);
    let analysis = analyse(
        &CoverageRequest::new(&registry, &policy)
            .over(nginx_conf())
            .mutating(config_mutation()),
    );

    assert_eq!(
        analysis.actions().len(),
        2,
        "§17.2: `maximize` attempts every non-conflicting mechanism that improves coverage"
    );
    assert!(
        analysis.actions()[0].is_required(),
        "the dominant candidate is still the one §2.3 aborts for"
    );
    assert!(
        !analysis.actions()[1].is_required(),
        "§17.2: an extra mechanism degrades the matrix rather than the plan when it fails"
    );
}

#[test]
fn should_not_propose_protecting_an_unrelated_filesystem_when_the_mode_is_maximize() {
    let elsewhere = candidate(
        "ono.recovery.zfs",
        "zfs-dataset",
        "rpool/home",
        &["/home/erin"],
        EffectDomain::FilesystemPersistent,
        RecoveryObjective::PreserveExact,
    )
    .at_consistency(ConsistencyClass::FilesystemConsistent)
    .restored_by(RestoreMethod::SelectiveFileRestore)
    .costing(snapshot_cost());
    let registry = registry_with(vec![
        TestProvider::new("ono.recovery.zfs")
            .offering(dataset_snapshot())
            .offering(elsewhere)
            .shared(),
    ]);
    let policy = ProtectionPolicy::of(ProtectionMode::Maximize);
    let analysis = analyse(
        &CoverageRequest::new(&registry, &policy)
            .over(nginx_conf())
            .mutating(config_mutation()),
    );

    assert_eq!(analysis.actions().len(), 1);
    assert!(
        analysis
            .actions()
            .iter()
            .all(|action| action.proposed_asset().scope().domain() == "rpool/ROOT/debian"),
        "§17.2 and §62.7: `maximize` follows the planned mutation scope and never snapshots \
         everything on the host"
    );
}

#[test]
fn should_refuse_a_candidate_that_exceeds_the_configured_size_limit() {
    let large = file_archive("ono.recovery.files", "/etc/nginx/nginx.conf")
        .costing(archive_cost(64 * 1024 * 1024));
    let registry = registry_with(vec![
        TestProvider::new("ono.recovery.files")
            .offering(large)
            .shared(),
    ]);
    let policy = ProtectionPolicy::default()
        .limited_by(CostLimits::default().sized(ByteSize::from_bytes(1024 * 1024)));
    let analysis = analyse(
        &CoverageRequest::new(&registry, &policy)
            .over(nginx_conf())
            .mutating(config_mutation()),
    );

    assert!(analysis.actions().is_empty());
    assert_eq!(analysis.rejected()[0].reason(), RejectionReason::CostLimit);
    assert!(
        analysis.rejected()[0].detail().contains("§38.3"),
        "§38.3: the bound that stopped it is named: {}",
        analysis.rejected()[0].detail()
    );
    assert_eq!(analysis.level(), ProtectionLevel::Unprotected);
}

#[test]
fn should_refuse_a_candidate_whose_recovery_scope_is_wider_than_policy_allows() {
    let registry = registry_with(vec![
        TestProvider::new("ono.recovery.zfs")
            .offering(dataset_snapshot())
            .shared(),
    ]);
    let policy = ProtectionPolicy::default().limited_by(CostLimits::default().scoped(2));
    let analysis = analyse(
        &CoverageRequest::new(&registry, &policy)
            .over(nginx_conf())
            .mutating(config_mutation()),
    );

    assert_eq!(analysis.rejected()[0].reason(), RejectionReason::CostLimit);
    assert!(
        analysis.rejected()[0].detail().contains("3 objects"),
        "§38.3: the target scope bound says how far the candidate reached: {}",
        analysis.rejected()[0].detail()
    );
}

#[test]
fn should_stop_proposing_snapshots_at_the_configured_count() {
    let etc = MountTable::from_text(ZFS_ROOT).resolve(Path::new("/etc/nginx/nginx.conf"));
    let home = MountTable::from_text(ZFS_ROOT).resolve(Path::new("/home/erin/notes.md"));
    let home_candidate = candidate(
        "ono.recovery.zfs",
        "zfs-dataset",
        "rpool/home",
        &["/home/erin/notes.md"],
        EffectDomain::FilesystemPersistent,
        RecoveryObjective::PreserveExact,
    )
    .at_consistency(ConsistencyClass::FilesystemConsistent)
    .restored_by(RestoreMethod::SelectiveFileRestore)
    .costing(snapshot_cost());
    let registry = registry_with(vec![
        TestProvider::new("ono.recovery.zfs")
            .offering(dataset_snapshot().restored_by(RestoreMethod::SelectiveFileRestore))
            .offering(home_candidate)
            .shared(),
    ]);
    let policy = ProtectionPolicy::default().limited_by(CostLimits::default().counted(1));
    let analysis = analyse(
        &CoverageRequest::new(&registry, &policy)
            .over(etc)
            .over(home)
            .mutating(config_mutation())
            .mutating(MutationDomain::new(
                EffectDomain::FilesystemPersistent,
                EffectKind::Modify,
                "/home/erin/notes.md",
                "the note is rewritten",
            )),
    );

    assert_eq!(
        analysis.actions().len(),
        1,
        "§38.3 and §53: `recovery.max_auto_snapshot_count` bounds automatic protection"
    );
    assert!(
        analysis
            .rejected()
            .iter()
            .any(|rejected| rejected.reason() == RejectionReason::CostLimit),
        "the candidate that did not fit says so"
    );
    assert_eq!(analysis.level(), ProtectionLevel::PartiallyProtected);
}

#[test]
fn should_carry_a_skipped_provider_into_the_analysis() {
    let registry = registry_with(vec![
        TestProvider::new("ono.recovery.zfs")
            .unavailable("the `zfs` command is not installed")
            .shared(),
    ]);
    let policy = ProtectionPolicy::default();
    let analysis = analyse(
        &CoverageRequest::new(&registry, &policy)
            .over(nginx_conf())
            .mutating(config_mutation()),
    );

    assert_eq!(analysis.provider_refusals().len(), 1);
    assert!(
        analysis.summary().rows()[0]
            .note()
            .contains("establishes nothing"),
        "§55.6 case 29: the absence of a candidate from an unasked provider establishes nothing: {}",
        analysis.summary().rows()[0].note()
    );
}

#[test]
fn should_call_a_single_provider_transaction_transactional() {
    let transactional = candidate(
        "ono.recovery.pkg",
        "package-transaction",
        "rpool/ROOT/debian",
        &["/etc/nginx/nginx.conf"],
        EffectDomain::FilesystemPersistent,
        RecoveryObjective::PreserveExact,
    )
    .at_consistency(ConsistencyClass::TransactionConsistent)
    .restored_by(RestoreMethod::ProviderNativeRestore)
    .costing(snapshot_cost());
    let registry = registry_with(vec![
        TestProvider::new("ono.recovery.pkg")
            .offering(transactional)
            .shared(),
    ]);
    let policy = ProtectionPolicy::default();
    let analysis = analyse(
        &CoverageRequest::new(&registry, &policy)
            .over(nginx_conf())
            .mutating(config_mutation()),
    );

    assert_eq!(
        analysis.level(),
        ProtectionLevel::Transactional,
        "§27.1: a provider that guarantees atomic commit and rollback inside its own boundary"
    );
    assert_eq!(
        analysis.summary().rows()[0].transaction_scope(),
        Some("ono.recovery.pkg"),
        "§27.2: the boundary the word applies to is named"
    );
}

#[test]
fn should_carry_a_candidate_exclusion_into_the_coverage_matrix() {
    let with_exclusion = dataset_snapshot()
        .restored_by(RestoreMethod::SelectiveFileRestore)
        .excluding(RecoveryExclusion::new(
            "/var/lib/nfs-data",
            "the NFS mount below the dataset is not in the snapshot (Appendix B.6)",
        ));
    let registry = registry_with(vec![
        TestProvider::new("ono.recovery.zfs")
            .offering(with_exclusion)
            .shared(),
    ]);
    let policy = ProtectionPolicy::default();
    let analysis = analyse(
        &CoverageRequest::new(&registry, &policy)
            .over(nginx_conf())
            .mutating(config_mutation()),
    );

    assert!(
        analysis
            .summary()
            .exclusions()
            .iter()
            .any(|exclusion| exclusion.subject() == "/var/lib/nfs-data"),
        "§10.3 and §62.6: what an asset does not cover is stated where the coverage is stated"
    );
}

#[test]
fn should_read_a_mutation_domain_out_of_a_proposed_effect() {
    let plan = PlanId::of("session-1", "2026-09-09T10:00:00Z", "replace nginx.conf");
    let action = ActionId::of(&plan, 1, "restart nginx");
    let effect = ProposedEffect::new(
        action,
        EffectDomain::ProcessRuntime,
        EffectKind::Replace,
        EffectConfidence::Expected,
        "the running worker set is replaced",
    )
    .on("nginx.service")
    .compensated_by("restart service nginx");

    let mutation = MutationDomain::from_effect(&effect);
    assert_eq!(mutation.domain(), EffectDomain::ProcessRuntime);
    assert_eq!(mutation.subject(), "nginx.service");
    assert_eq!(mutation.compensation(), Some("restart service nginx"));
    assert_eq!(
        mutation.objective(),
        RecoveryObjective::RestoreSemantic,
        "Appendix A.1 and A.2: the domains come from the actions' proposed effects"
    );
}

#[test]
fn should_call_a_compensated_runtime_domain_compensatable_and_never_rollback() {
    let registry = ProviderRegistry::new();
    let policy = ProtectionPolicy::default();
    let analysis = analyse(
        &CoverageRequest::new(&registry, &policy).mutating(
            MutationDomain::new(
                EffectDomain::ProcessRuntime,
                EffectKind::Replace,
                "nginx.service",
                "the service is restarted",
            )
            .compensated_by("restart service nginx"),
        ),
    );

    assert_eq!(analysis.level(), ProtectionLevel::Compensatable);
    assert!(
        analysis.summary().rows()[0]
            .note()
            .contains("not a rollback"),
        "§27.4: a compensating action is never described as rollback: {}",
        analysis.summary().rows()[0].note()
    );
}

#[test]
fn should_resolve_an_action_to_the_persistence_domain_of_its_containing_directory() {
    let registry = ProviderRegistry::new();
    let policy = ProtectionPolicy::default();
    let request = CoverageRequest::new(&registry, &policy)
        .over(MountTable::from_text(ZFS_ROOT).resolve(Path::new("/etc/nginx")));
    let found = request
        .persistence_for("/etc/nginx/conf.d/site.conf")
        .expect("§11.2: the containing persistence domain is what protection is claimed over");
    assert_eq!(found.object(), Some("rpool/ROOT/debian"));
    assert!(request.persistence_for("/srv/other").is_none());
}

#[test]
fn should_compose_the_cost_of_everything_it_proposes() {
    let registry = registry_with(vec![
        TestProvider::new("ono.recovery.zfs")
            .offering(dataset_snapshot())
            .shared(),
        TestProvider::new("ono.recovery.files")
            .offering(file_archive("ono.recovery.files", "/etc/nginx/nginx.conf"))
            .shared(),
    ]);
    let policy = ProtectionPolicy::of(ProtectionMode::Maximize);
    let analysis = analyse(
        &CoverageRequest::new(&registry, &policy)
            .over(nginx_conf())
            .mutating(config_mutation()),
    );

    let cost = analysis.estimated_cost();
    assert_eq!(
        cost.initial_bytes(),
        Some(ByteSize::from_bytes(2 * 1024 + 4 * 1024)),
        "§38.1: two assets occupy two lots of space"
    );
    assert!(
        cost.is_estimated(),
        "§37.5: a figure that came from an estimate stays labelled as one"
    );
}
