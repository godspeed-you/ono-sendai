//! The plan of §20.2 and §64, built once so every view test reads the same worked example.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    dead_code,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::sync::Arc;

use jiff::Timestamp;
use ono_change_core::{
    ActionRole, ActionStatus, ChangePlan, ConsistencyClass, CoverageExclusion, DomainCoverage,
    DomainProtection, EffectConfidence, EffectDomain, EffectKind, Execution, FrozenTarget,
    ImpactClass, ImpactGraph, ImpactNode, Intent, LifecycleEvent, NewerStateClass,
    NewerStateImpact, NewerStateItem, PlanAction, ProposedEffect, ProtectionSummary, RecoveryAsset,
    RecoveryAssetType, RecoveryCost, RecoveryGoal, RecoveryObjective, RecoveryPlan, RecoveryScope,
    RecoveryValidation, RestoreMethod, RetentionPolicy, RiskAssessment, RiskClass, RiskDimension,
    RiskFinding, Strategy, UnknownBoundary, UnrecoverableEffect, VerificationClass,
    VerificationContract, VerificationResult, VerificationSet, VerificationStatus,
};

/// A fixed instant, because §50 makes rendering deterministic and a test may not read a clock.
#[must_use]
pub fn instant() -> Timestamp {
    Timestamp::UNIX_EPOCH
}

/// `now`, `seconds` after the fixed instant.
#[must_use]
pub fn later(seconds: i64) -> Timestamp {
    Timestamp::from_second(instant().as_second() + seconds).expect("a representable instant")
}

/// A provider action, which is the only shape §2.17 permits an execution to take.
#[must_use]
pub fn provider_action(operation: &str) -> Execution {
    Execution::ProviderAction {
        provider: Arc::from("ono.provider.linux"),
        operation: Arc::from(operation),
        arguments: Vec::new(),
    }
}

/// §64's nginx plan: two targets, five actions, a ZFS recovery point and real exclusions.
#[must_use]
pub fn nginx_plan() -> ChangePlan {
    let plan = ChangePlan::draft(
        Intent::new(
            "replace nginx configuration and restart service",
            "plan { replace file /etc/nginx/nginx.conf }",
        ),
        "session-1",
        instant(),
    )
    .resolve(vec![
        FrozenTarget::new(
            "ono.file/1",
            "/etc/nginx/nginx.conf",
            "/etc/nginx/nginx.conf",
        ),
        FrozenTarget::new("ono.service/1", "nginx.service", "nginx.service"),
    ])
    .expect("targets resolve on a draft plan");
    let id = plan.id().clone();

    let snapshot = PlanAction::new(
        &id,
        1,
        ActionRole::Prepare,
        "snapshot rpool/ROOT/debian",
        provider_action("ono.recovery.zfs.snapshot"),
    );
    let replace = PlanAction::new(
        &id,
        2,
        ActionRole::Mutate,
        "replace nginx.conf",
        provider_action("ono.file.replace"),
    )
    .on("/etc/nginx/nginx.conf")
    .effecting(
        ProposedEffect::new(
            ActionId_of(&id, 2, "replace nginx.conf"),
            EffectDomain::FilesystemPersistent,
            EffectKind::Modify,
            EffectConfidence::Guaranteed,
            "the file content is replaced",
        )
        .on("/etc/nginx/nginx.conf"),
    );
    let validate = PlanAction::new(
        &id,
        3,
        ActionRole::Mutate,
        "validate nginx configuration",
        provider_action("ono.service.validate"),
    );
    let restart = PlanAction::new(
        &id,
        4,
        ActionRole::Mutate,
        "restart nginx.service",
        provider_action("ono.service.restart"),
    )
    .on("nginx.service")
    .effecting(
        ProposedEffect::new(
            ActionId_of(&id, 4, "restart nginx.service"),
            EffectDomain::NetworkRuntime,
            EffectKind::Interrupt,
            EffectConfidence::Possible,
            "active client connections may be cut",
        )
        .on("active TCP sessions")
        .irreversible(),
    );
    let verify = PlanAction::new(
        &id,
        5,
        ActionRole::Verify,
        "verify service running",
        provider_action("ono.service.state"),
    );

    plan.with_action(snapshot)
        .expect("a prepare action is plannable")
        .with_action(replace)
        .expect("a mutate action is plannable")
        .with_action(validate)
        .expect("a mutate action is plannable")
        .with_action(restart)
        .expect("a mutate action is plannable")
        .with_action(verify)
        .expect("a verify action is plannable")
        .with_impact(nginx_impact())
        .with_protection(protected_summary())
        .with_risk(moderate_risk())
        .with_verification(nginx_verification(&id))
}

/// The action identity core derives, spelled the same way the fixture spells it.
#[expect(
    non_snake_case,
    reason = "it stands for the core constructor it mirrors"
)]
fn ActionId_of(
    plan: &ono_change_core::PlanId,
    ordinal: usize,
    summary: &str,
) -> ono_change_core::ActionId {
    ono_change_core::ActionId::of(plan, ordinal, summary)
}

/// §64's sealed plan, ready to apply.
#[must_use]
pub fn sealed_nginx_plan() -> ChangePlan {
    nginx_plan().seal(instant()).expect("the plan seals")
}

/// §9's impact graph for the nginx plan, with §9.6's boundary on the end of it.
#[must_use]
pub fn nginx_impact() -> ImpactGraph {
    let mut graph = ImpactGraph::empty();
    graph.add(ImpactNode::new(
        "/etc/nginx/nginx.conf",
        "nginx.conf",
        "ono.file/1",
        ImpactClass::DirectTarget,
        0,
    ));
    graph.add(ImpactNode::new(
        "nginx.service",
        "nginx.service",
        "ono.service/1",
        ImpactClass::DirectTarget,
        0,
    ));
    for worker in 1..=4 {
        graph.add(
            ImpactNode::new(
                format!("worker/{worker}"),
                format!("worker {worker}"),
                "ono.process/1",
                ImpactClass::Dependent,
                1,
            )
            .reached_by("service.controls_process")
            .with_confidence("exact"),
        );
    }
    graph.add(
        ImpactNode::new(
            ":443",
            ":443",
            "ono.socket/1",
            ImpactClass::TransitiveRelated,
            2,
        )
        .reached_by("process.listens_on")
        .with_confidence("exact"),
    );
    graph.add(
        ImpactNode::new(
            "sessions",
            "14 active client connections",
            "ono.socket/1",
            ImpactClass::ExternalSideEffect,
            3,
        )
        .with_confidence("possible"),
    );
    graph.add_boundary(UnknownBoundary::new(
        "nginx",
        "external API",
        "the outbound HTTPS request leaves this machine",
    ));
    graph
}

/// §10.3's matrix for a plan that is PROTECTED and still excludes real things (Appendix A.6).
#[must_use]
pub fn protected_summary() -> ProtectionSummary {
    ProtectionSummary::of(vec![
        DomainCoverage::new(
            EffectDomain::FilesystemPersistent,
            RecoveryObjective::PreserveExact,
            DomainProtection::Protected,
            "covered by the ZFS recovery point",
        )
        .at_consistency(ConsistencyClass::FilesystemConsistent)
        .excluding(CoverageExclusion::new(
            EffectDomain::FilesystemPersistent,
            "/home",
            "a separate dataset",
        )),
        DomainCoverage::new(
            EffectDomain::ProcessRuntime,
            RecoveryObjective::RestoreSemantic,
            DomainProtection::Compensatable,
            "the service can be restarted",
        )
        .excluding(CoverageExclusion::new(
            EffectDomain::ProcessRuntime,
            "process memory",
            "process identity cannot be captured",
        )),
    ])
    .excluding(
        CoverageExclusion::new(
            EffectDomain::NetworkRuntime,
            "active TCP sessions",
            "a live session cannot be re-established",
        )
        .irreversible(),
    )
    .excluding(
        CoverageExclusion::new(
            EffectDomain::ExternalSideEffect,
            "requests already served externally",
            "the response has left the machine",
        )
        .irreversible(),
    )
}

/// A matrix whose required persistent domain nothing covers (§10.2's UNPROTECTED).
#[must_use]
pub fn unprotected_summary() -> ProtectionSummary {
    ProtectionSummary::of(vec![DomainCoverage::new(
        EffectDomain::FilesystemPersistent,
        RecoveryObjective::PreserveExact,
        DomainProtection::Unprotected,
        "no provider offered a recovery asset",
    )])
}

/// A matrix nobody recorded an exclusion for, which Appendix E.8 still forbids rendering bare.
#[must_use]
pub fn summary_without_exclusions() -> ProtectionSummary {
    ProtectionSummary::of(vec![DomainCoverage::new(
        EffectDomain::FilesystemPersistent,
        RecoveryObjective::PreserveExact,
        DomainProtection::Protected,
        "covered by the ZFS recovery point",
    )])
}

/// §19.3's worked class: one config file and one service restart.
#[must_use]
pub fn moderate_risk() -> RiskAssessment {
    RiskAssessment::of(vec![RiskFinding::new(
        RiskDimension::Downtime,
        RiskClass::Moderate,
        "rule.service.restart",
        "restarting nginx interrupts the connections it is serving",
    )])
}

/// §23.1's contracts for the nginx plan.
#[must_use]
pub fn nginx_verification(plan: &ono_change_core::PlanId) -> VerificationSet {
    VerificationSet::of(vec![
        VerificationContract::new(
            plan,
            VerificationClass::Required,
            "nginx.service",
            "== running",
        ),
        VerificationContract::new(plan, VerificationClass::Required, "socket :443", "exists"),
        VerificationContract::new(plan, VerificationClass::Advisory, "worker count", "== 4"),
    ])
}

/// The results §23.4's example prints, with one observation that changes nothing.
#[must_use]
pub fn nginx_results(plan: &ChangePlan) -> Vec<VerificationResult> {
    plan.verification()
        .contracts()
        .iter()
        .map(|contract| {
            VerificationResult::new(
                plan.id().clone(),
                contract,
                VerificationStatus::Passed,
                instant(),
            )
        })
        .collect()
}

/// §13.8's ZFS recovery point, proposed rather than created (§2.1).
#[must_use]
pub fn zfs_asset() -> RecoveryAsset {
    RecoveryAsset::proposed(
        "ono.recovery.zfs",
        RecoveryAssetType::ZfsSnapshot,
        "rpool/ROOT/debian@ono-a82f",
        RecoveryScope::new("zfs-dataset", "rpool/ROOT/debian", "local")
            .covering("/etc/nginx/nginx.conf"),
        instant(),
    )
    .at_consistency(ConsistencyClass::FilesystemConsistent)
    .restored_by(RestoreMethod::SelectiveFileRestore)
    .retained_for(RetentionPolicy::default())
    .costing(RecoveryCost::unknown().with_space(
        Some(ono_value::ByteSize::from_bytes(327_155_712)),
        Some(ono_value::ByteSize::from_bytes(327_155_712)),
        true,
    ))
    .excluding(ono_change_core::RecoveryExclusion::new(
        "/home",
        "a separate dataset",
    ))
    .for_plan(sealed_nginx_plan().id().clone())
}

/// The same asset, created, validated and with an exact cost (§11.4, §37.5).
#[must_use]
pub fn ready_asset() -> RecoveryAsset {
    zfs_asset()
        .creating()
        .validated(RecoveryValidation::complete(
            instant(),
            "the snapshot exists and a restore path was confirmed",
        ))
        .costing(RecoveryCost::unknown().with_space(
            Some(ono_value::ByteSize::from_bytes(327_155_712)),
            Some(ono_value::ByteSize::from_bytes(327_155_712)),
            false,
        ))
        .expiring_at(later(24 * 3_600))
}

/// An asset whose size nobody measured — v0.2 §35.3's unknown, never a zero.
#[must_use]
pub fn unmeasured_asset() -> RecoveryAsset {
    RecoveryAsset::proposed(
        "ono.recovery.btrfs",
        RecoveryAssetType::BtrfsSnapshot,
        "@root.ono-91aa",
        RecoveryScope::new("btrfs-subvolume", "@root", "local"),
        instant(),
    )
    .costing(RecoveryCost::unknown())
}

/// A plan that failed at its fourth action, with two actions never executed (Appendix E.5).
#[must_use]
pub fn failed_plan() -> ChangePlan {
    with_statuses(
        sealed_nginx_plan(),
        &[
            ActionStatus::Succeeded,
            ActionStatus::Succeeded,
            ActionStatus::Succeeded,
            ActionStatus::Failed,
            ActionStatus::Pending,
        ],
    )
    .advance(LifecycleEvent::BeginPrepare)
    .expect("a sealed plan may prepare")
    .advance(LifecycleEvent::Protected)
    .expect("preparation completes")
    .advance(LifecycleEvent::BeginApply)
    .expect("a protected plan may apply")
    .advance(LifecycleEvent::ApplyFailed)
    .expect("an applying plan may fail")
}

/// The same plan rebuilt with the action statuses a run would have written.
///
/// A sealed plan is immutable, so the statuses are set on a fresh draft carrying the same actions
/// and the result is sealed again — which is what an executor's own record looks like when a
/// store reads it back.
#[must_use]
pub fn with_statuses(plan: ChangePlan, statuses: &[ActionStatus]) -> ChangePlan {
    let mut rebuilt = ChangePlan::draft(
        Intent::new(plan.intent().text(), plan.intent().source()),
        plan.session(),
        plan.created_at(),
    )
    .resolve(plan.targets().to_vec())
    .expect("targets resolve on a draft plan");
    for (index, action) in plan.actions().iter().enumerate() {
        let status = statuses
            .get(index)
            .copied()
            .unwrap_or(ActionStatus::Pending);
        rebuilt = rebuilt
            .with_action(action.clone().with_status(status))
            .expect("an action of a valid plan stays plannable");
    }
    rebuilt
        .with_impact(plan.impact().clone())
        .with_protection(plan.protection().clone())
        .with_risk(plan.risk().clone())
        .with_strategy(plan.strategy())
        .with_verification(plan.verification().clone())
        .seal(instant())
        .expect("the rebuilt plan seals")
}

/// §24.4's selective recovery: three newer files, two of them left alone.
#[must_use]
pub fn selective_recovery() -> RecoveryPlan {
    RecoveryPlan::new(
        recovery_base(),
        RecoveryGoal::RestoreChangedObjects,
        RestoreMethod::SelectiveFileRestore,
        "rpool/ROOT/debian@ono-a82f",
    )
    .recovering(sealed_nginx_plan().id().clone())
    .using(zfs_asset().id().clone())
    .restoring("/etc/nginx/nginx.conf")
    .with_newer_state(NewerStateImpact::analysed(vec![
        NewerStateItem::new(
            "/etc/ssh/sshd_config",
            NewerStateClass::PreservedByMethod,
            "changed after the plan, outside the restore set",
        ),
        NewerStateItem::new(
            "/etc/hosts",
            NewerStateClass::PreservedByMethod,
            "changed after the plan, outside the restore set",
        ),
    ]))
    .leaving(
        UnrecoverableEffect::new(
            "TCP sessions",
            EffectDomain::NetworkRuntime,
            "a live session cannot be re-established",
        )
        .compensated_by("clients reconnect"),
    )
    .restoring_metadata(ono_change_core::MetadataCoverage {
        content: true,
        mode: true,
        owner: true,
        ..ono_change_core::MetadataCoverage::none()
    })
}

/// §24.5's full rollback: newer snapshots destroyed and live data discarded.
#[must_use]
pub fn rollback_recovery() -> RecoveryPlan {
    RecoveryPlan::new(
        recovery_base(),
        RecoveryGoal::RestoreDomain,
        RestoreMethod::DatasetRollback,
        "tank/data@ono-91aa",
    )
    .restoring("tank/data")
    .with_newer_state(
        NewerStateImpact::analysed(vec![NewerStateItem::new(
            "tank/data",
            NewerStateClass::DiscardedByMethod,
            "everything written since the snapshot",
        )])
        .destroying("tank/data@later-1")
        .destroying("tank/data@later-2")
        .discarding(ono_value::ByteSize::from_bytes(19_327_352_832)),
    )
}

/// A recovery nobody analysed for drift, which §62.8 forbids rendering as safe.
#[must_use]
pub fn unanalysed_recovery() -> RecoveryPlan {
    RecoveryPlan::new(
        recovery_base(),
        RecoveryGoal::RestoreChangedObjects,
        RestoreMethod::SelectiveFileRestore,
        "rpool/ROOT/debian@ono-a82f",
    )
    .restoring("/etc/nginx/nginx.conf")
}

/// The plan every recovery plan is built around (§2.12).
fn recovery_base() -> ChangePlan {
    let plan = ChangePlan::draft(
        Intent::new("restore nginx configuration", "recover plan/a82f"),
        "session-1",
        instant(),
    );
    let id = plan.id().clone();
    plan.with_action(PlanAction::new(
        &id,
        1,
        ActionRole::Recover,
        "restore /etc/nginx/nginx.conf",
        provider_action("ono.recovery.zfs.restore"),
    ))
    .expect("a recover action is plannable")
    .with_risk(moderate_risk())
    .with_verification(VerificationSet::of(vec![VerificationContract::new(
        &id,
        VerificationClass::Required,
        "nginx.conf",
        "== restored",
    )]))
}

/// A canary strategy over many identical actions, which Appendix E.2 collapses.
#[must_use]
pub fn long_plan() -> ChangePlan {
    let plan = ChangePlan::draft(
        Intent::new("roll the config out to every api host", "plan { ... }"),
        "session-2",
        instant(),
    );
    let id = plan.id().clone();
    let mut built = plan;
    let mut ordinal = 0usize;
    for _ in 0..20 {
        ordinal += 1;
        built = built
            .with_action(PlanAction::new(
                &id,
                ordinal,
                ActionRole::Prepare,
                "recovery assets",
                provider_action("ono.recovery.zfs.snapshot"),
            ))
            .expect("a prepare action is plannable");
    }
    for _ in 0..20 {
        ordinal += 1;
        built = built
            .with_action(PlanAction::new(
                &id,
                ordinal,
                ActionRole::Mutate,
                "update config",
                provider_action("ono.file.replace"),
            ))
            .expect("a mutate action is plannable");
    }
    for _ in 0..20 {
        ordinal += 1;
        built = built
            .with_action(PlanAction::new(
                &id,
                ordinal,
                ActionRole::Mutate,
                "restart service",
                provider_action("ono.service.restart"),
            ))
            .expect("a mutate action is plannable");
    }
    for _ in 0..23 {
        ordinal += 1;
        built = built
            .with_action(PlanAction::new(
                &id,
                ordinal,
                ActionRole::Verify,
                "service/listener checks",
                provider_action("ono.service.state"),
            ))
            .expect("a verify action is plannable");
    }
    built
        .with_protection(protected_summary())
        .with_risk(RiskAssessment::of(vec![RiskFinding::new(
            RiskDimension::BulkCount,
            RiskClass::High,
            "rule.bulk",
            "the plan touches twenty hosts",
        )]))
        .with_strategy(Strategy::canary(1, 3).expect("a canary of one is permitted"))
        .with_verification(VerificationSet::of(vec![VerificationContract::new(
            &id,
            VerificationClass::Required,
            "nginx.service",
            "== running",
        )]))
}

/// The headings of a rendered view: the lines that start at column zero and carry text.
#[must_use]
pub fn headings(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter(|line| !line.is_empty() && !line.starts_with(' '))
        .cloned()
        .collect()
}

/// Where `needle` first appears in `lines`, as a whole line or inside one.
#[must_use]
pub fn index_of(lines: &[String], needle: &str) -> Option<usize> {
    lines.iter().position(|line| line.contains(needle))
}

/// Whether any line contains `needle`.
#[must_use]
pub fn contains(lines: &[String], needle: &str) -> bool {
    index_of(lines, needle).is_some()
}
