#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]
//! The v0.6 value bridge against the contracts of §46.
//!
//! Two questions run through the whole file. Does what this crate writes satisfy the contract a
//! consumer compiled against (§36.5)? And does what §36.1's store wrote come back as the same
//! plan after a shell restart (§41.2)? Everything else — the protection level agreeing with its
//! matrix (§62.1), the exclusions travelling with it (§62.6), an unknown enum spelling refusing
//! rather than defaulting (§2.4) — is a property of those two.

use std::sync::Arc;
use std::time::Duration;

use jiff::Timestamp;
use ono_change_core::value::{
    action_record, asset_from_record, asset_record, candidate_record, coverage_record,
    domain_record, effect_record, impact_from_record, impact_record, plan_from_record, plan_record,
    recovery_plan_from_record, recovery_plan_record, verification_record,
};
use ono_change_core::{
    ActionRole, ActionStatus, AssetState, ChangePlan, ConsistencyClass, CoverageExclusion,
    DirectoryRestorePolicy, DomainCoverage, DomainProtection, EffectConfidence, EffectDomain,
    EffectKind, Execution, FrozenTarget, Idempotency, ImpactClass, ImpactGraph, ImpactNode, Intent,
    LifecycleEvent, MetadataCoverage, NewerStateClass, NewerStateImpact, NewerStateItem,
    NonPersistentReason, PersistenceDomain, PlanAction, PlanState, Precondition, PreconditionKind,
    ProposedEffect, ProtectionLevel, ProtectionMode, ProtectionSummary, ProviderBinding,
    RecoveryAsset, RecoveryAssetType, RecoveryCandidate, RecoveryCost, RecoveryExclusion,
    RecoveryGoal, RecoveryObjective, RecoveryPlan, RecoveryScope, RecoveryValidation,
    ResolvedMount, RestoreMethod, RetentionPolicy, RiskAssessment, RiskClass, RiskDimension,
    RiskFinding, Strategy, UnknownBoundary, UnrecoverableEffect, VerificationClass,
    VerificationContract, VerificationResult, VerificationSet, VerificationStatus,
};
use ono_value::{ByteSize, RecordValue, Value};

// ---------------------------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------------------------

fn instant() -> Timestamp {
    Timestamp::UNIX_EPOCH
}

fn at(second: i64) -> Timestamp {
    Timestamp::from_second(second).expect("a valid instant")
}

fn draft() -> ChangePlan {
    ChangePlan::draft(
        Intent::new(
            "replace the nginx configuration and restart the service",
            "plan { replace file /etc/nginx/nginx.conf from ./nginx.conf; restart nginx }",
        ),
        "session-7",
        instant(),
    )
}

fn targets() -> Vec<FrozenTarget> {
    vec![
        FrozenTarget::new("ono.file/1", "/etc/nginx/nginx.conf", "nginx.conf")
            .on_host("host-1")
            .resolved_from("file /etc/nginx/nginx.conf")
            .in_domain("rpool/etc"),
        FrozenTarget::new("ono.service/1", "nginx.service", "nginx")
            .at_place("place/9f21")
            .on_host("host-1"),
    ]
}

fn prepare_action(plan: &ChangePlan) -> PlanAction {
    let action = PlanAction::new(
        plan.id(),
        1,
        ActionRole::Prepare,
        "snapshot rpool/etc before the change",
        Execution::RecoveryOperation {
            provider: Arc::from("ono.recovery.zfs"),
            capability: Arc::from("recovery.prepare"),
            arguments: vec![
                (Arc::from("dataset"), Value::string("rpool/etc")),
                (Arc::from("recursive"), Value::Bool(false)),
            ],
        },
    );
    let id = action.id().clone();
    action
        .on("rpool/etc")
        .with_idempotency(Idempotency::Idempotent)
        .with_status(ActionStatus::Succeeded)
        .requiring(
            Precondition::new(
                PreconditionKind::PersistenceDomain,
                "/etc/nginx/nginx.conf",
                "dataset",
                Value::string("rpool/etc"),
            )
            .explained("the path must still resolve to the dataset the plan froze"),
        )
        .effecting(
            ProposedEffect::new(
                id,
                EffectDomain::FilesystemPersistent,
                EffectKind::Create,
                EffectConfidence::Guaranteed,
                "a zfs snapshot of rpool/etc comes into being",
            )
            .on("rpool/etc@ono-a82f")
            .citing("ono.recovery.zfs/1"),
        )
}

fn mutate_action(plan: &ChangePlan, after: &PlanAction) -> PlanAction {
    let action = PlanAction::new(
        plan.id(),
        2,
        ActionRole::Mutate,
        "replace /etc/nginx/nginx.conf",
        Execution::ProviderAction {
            provider: Arc::from("linux.files"),
            operation: Arc::from("ono.file.write"),
            arguments: vec![(Arc::from("path"), Value::string("/etc/nginx/nginx.conf"))],
        },
    );
    let id = action.id().clone();
    action
        .on("/etc/nginx/nginx.conf")
        .after(after.id().clone())
        .privileged()
        .with_idempotency(Idempotency::NonIdempotent)
        .with_status(ActionStatus::Failed)
        .recovery_semantics("restoring the file from the snapshot restores the prior bytes")
        .requiring(
            Precondition::new(
                PreconditionKind::ContentDigest,
                "/etc/nginx/nginx.conf",
                "digest",
                Value::string("sha256:0011"),
            )
            .tolerant()
            .explained("a comment-only edit does not invalidate the plan"),
        )
        .effecting(
            ProposedEffect::new(
                id.clone(),
                EffectDomain::FilesystemPersistent,
                EffectKind::Replace,
                EffectConfidence::Guaranteed,
                "the configuration file is replaced",
            )
            .on("/etc/nginx/nginx.conf")
            .from_to(
                Some(Value::string("sha256:0011")),
                Some(Value::string("sha256:2200")),
            )
            .citing("linux.files/1"),
        )
        .effecting(
            ProposedEffect::new(
                id,
                EffectDomain::ExternalSideEffect,
                EffectKind::Emit,
                EffectConfidence::Unknown,
                "a reload webhook may leave the machine, and Ono has no model for it",
            )
            .on("https://example.invalid/reload")
            .compensated_by("post a cancellation to the same endpoint"),
        )
}

fn verify_action(plan: &ChangePlan) -> PlanAction {
    PlanAction::new(
        plan.id(),
        3,
        ActionRole::Verify,
        "observe that nginx is running",
        Execution::Program {
            program: Arc::from("/usr/bin/systemctl"),
            argv: vec![Arc::from("is-active"), Arc::from("nginx.service")],
        },
    )
    .with_status(ActionStatus::Unknown)
}

fn protection() -> ProtectionSummary {
    ProtectionSummary::of(vec![
        DomainCoverage::new(
            EffectDomain::FilesystemPersistent,
            RecoveryObjective::PreserveExact,
            DomainProtection::Protected,
            "the zfs snapshot rpool/etc@ono-a82f holds the prior bytes",
        )
        .by_asset(asset().id().clone())
        .at_consistency(ConsistencyClass::FilesystemConsistent)
        .excluding(CoverageExclusion::new(
            EffectDomain::FilesystemPersistent,
            "/etc/nginx/cache",
            "the cache directory is a separate dataset",
        )),
        DomainCoverage::new(
            EffectDomain::ProcessRuntime,
            RecoveryObjective::RestoreSemantic,
            DomainProtection::Compensatable,
            "the service is restarted, with new worker identities",
        )
        .within_transaction("ono.service.systemd"),
        DomainCoverage::new(
            EffectDomain::ExternalSideEffect,
            RecoveryObjective::NoRecoveryRequired,
            DomainProtection::Unprotected,
            "an emitted request is outside every recovery asset",
        ),
    ])
    .excluding(
        CoverageExclusion::new(
            EffectDomain::ExternalSideEffect,
            "the reload webhook",
            "an emitted request cannot be taken back",
        )
        .irreversible(),
    )
}

fn risk() -> RiskAssessment {
    RiskAssessment::of(vec![
        RiskFinding::new(
            RiskDimension::Downtime,
            RiskClass::Moderate,
            "rule.service-restart",
            "nginx stops serving for the length of the restart",
        ),
        RiskFinding::new(
            RiskDimension::Irreversibility,
            RiskClass::High,
            "rule.external-emit",
            "the reload webhook cannot be recalled once it has left",
        ),
    ])
    .risk_accepted()
    .irreversible_accepted()
}

fn verification(plan: &ChangePlan) -> VerificationSet {
    VerificationSet::empty()
        .with(
            VerificationContract::new(
                plan.id(),
                VerificationClass::Required,
                "nginx.service",
                "state == running",
            )
            .expecting(Value::string("running"))
            .within(Duration::from_secs(45)),
        )
        .with(
            VerificationContract::new(
                plan.id(),
                VerificationClass::Advisory,
                "listener :443",
                "socket is bound",
            )
            .about(ono_change_core::EquivalenceDomain::RuntimeState)
            .timeout_is_unknown(),
        )
}

fn impact() -> ImpactGraph {
    let mut graph = ImpactGraph::empty();
    graph.add(
        ImpactNode::new(
            "file:/etc/nginx/nginx.conf",
            "nginx.conf",
            "ono.file",
            ImpactClass::DirectTarget,
            0,
        )
        .on_host("host-1"),
    );
    graph.add(
        ImpactNode::new(
            "unit:nginx.service",
            "nginx",
            "ono.service",
            ImpactClass::DirectEffect,
            1,
        )
        .reached_by("configures")
        .with_confidence("observed")
        .citing("ono.service.systemd/1")
        .on_host("host-1"),
    );
    graph.add(ImpactNode::new(
        "unit:php-fpm.service",
        "php-fpm",
        "ono.service",
        ImpactClass::Dependent,
        2,
    ));
    graph.add_boundary(UnknownBoundary::new(
        "unit:nginx.service",
        "whatever the upstream servers serve",
        "Ono cannot see past a proxy configuration",
    ));
    graph.truncated("the traversal budget of §52.2 was reached")
}

fn rich_plan() -> ChangePlan {
    let plan = draft().resolve(targets()).expect("a draft resolves");
    let prepare = prepare_action(&plan);
    let mutate = mutate_action(&plan, &prepare);
    let verify = verify_action(&plan);
    let verification = verification(&plan);
    plan.with_action(prepare)
        .and_then(|plan| plan.with_action(mutate))
        .and_then(|plan| plan.with_action(verify))
        .expect("a resolved plan accepts actions")
        .with_impact(impact())
        .with_protection(protection())
        .with_protection_mode(ProtectionMode::Require)
        .with_risk(risk())
        .with_strategy(Strategy::canary(1, 4).expect("§28.4 permits a canary of one"))
        .with_verification(verification)
        .binding(ProviderBinding::new("linux.files", "1.4.0"))
        .binding(ProviderBinding::new("ono.recovery.zfs", "0.9.2"))
        .expiring_at(at(3600))
        .seal(at(60))
        .expect("a plan with a required contract seals")
}

/// The same plan, moved to where §4.7 says something may already have happened.
fn applying_plan() -> ChangePlan {
    rich_plan()
        .advance(LifecycleEvent::BeginPrepare)
        .and_then(|plan| plan.advance(LifecycleEvent::Protected))
        .and_then(|plan| plan.advance(LifecycleEvent::BeginApply))
        .expect("§4.1 draws sealed -> preparing -> protected -> applying")
}

fn scope() -> RecoveryScope {
    RecoveryScope::new("zfs-dataset", "rpool/etc", "host-1")
        .covering("/etc/nginx/nginx.conf")
        .covering("/etc/nginx/conf.d")
}

fn asset() -> RecoveryAsset {
    RecoveryAsset::proposed(
        "ono.recovery.zfs",
        RecoveryAssetType::ZfsSnapshot,
        "rpool/etc@ono-a82f",
        scope(),
        instant(),
    )
    .at_consistency(ConsistencyClass::FilesystemConsistent)
    .restored_by(RestoreMethod::SelectiveFileRestore)
    .retained_for(RetentionPolicy::of(Duration::from_secs(3600)).holding())
    .costing(
        RecoveryCost::unknown()
            .with_space(
                Some(ByteSize::from_bytes(1024)),
                Some(ByteSize::from_bytes(4096)),
                false,
            )
            .needing_reboot(),
    )
    .excluding(RecoveryExclusion::new(
        "/etc/nginx/cache",
        "the cache directory is a separate dataset",
    ))
    .capturing("sha256:00ff")
    .expiring_at(at(7200))
}

fn recovery_plan() -> RecoveryPlan {
    let source = rich_plan();
    let plan = ChangePlan::draft(
        Intent::new(
            "restore /etc/nginx/nginx.conf from rpool/etc@ono-a82f",
            "recover plan/a82f",
        ),
        "session-8",
        at(600),
    );
    let action = PlanAction::new(
        plan.id(),
        1,
        ActionRole::Recover,
        "restore nginx.conf out of the snapshot",
        Execution::RecoveryOperation {
            provider: Arc::from("ono.recovery.zfs"),
            capability: Arc::from("recovery.restore"),
            arguments: vec![(Arc::from("path"), Value::string("/etc/nginx/nginx.conf"))],
        },
    );
    let contract = VerificationContract::new(
        plan.id(),
        VerificationClass::Required,
        "/etc/nginx/nginx.conf",
        "digest == sha256:0011",
    )
    .about(ono_change_core::EquivalenceDomain::PersistentState);
    let plan = plan
        .with_action(action)
        .expect("a draft accepts an action")
        .with_verification(VerificationSet::of(vec![contract]))
        .with_risk(RiskAssessment::of(vec![RiskFinding::new(
            RiskDimension::RecoveryComplexity,
            RiskClass::Moderate,
            "rule.selective-restore",
            "restoring one file out of a dataset snapshot is bounded work",
        )]))
        .seal(at(660))
        .expect("a recovery plan seals like any other (§2.12)");

    RecoveryPlan::new(
        plan,
        RecoveryGoal::RestoreChangedObjects,
        RestoreMethod::SelectiveFileRestore,
        "rpool/etc@ono-a82f",
    )
    .recovering(source.id().clone())
    .using(asset().id().clone())
    .restoring("/etc/nginx/nginx.conf")
    .with_newer_state(
        NewerStateImpact::analysed(vec![
            NewerStateItem::new(
                "/etc/nginx/conf.d/new-site.conf",
                NewerStateClass::DiscardedByMethod,
                "the file did not exist when the snapshot was taken",
            )
            .changed_at(at(900)),
            NewerStateItem::new(
                "/etc/nginx/mime.types",
                NewerStateClass::PreservedByMethod,
                "a selective restore leaves it alone",
            ),
            NewerStateItem::new(
                "/etc/nginx/ssl",
                NewerStateClass::Unknown,
                "whether the restore touches this could not be established",
            ),
        ])
        .destroying("rpool/etc@nightly-2026-09-09")
        .discarding(ByteSize::from_bytes(8192)),
    )
    .leaving(
        UnrecoverableEffect::new(
            "the reload webhook",
            EffectDomain::ExternalSideEffect,
            "an emitted request cannot be un-sent",
        )
        .compensated_by("post a cancellation to the same endpoint"),
    )
    .leaving(UnrecoverableEffect::new(
        "the previous worker processes",
        EffectDomain::ProcessRuntime,
        "process identity does not come back (§33.1)",
    ))
    .restoring_metadata(MetadataCoverage {
        content: true,
        mode: true,
        owner: true,
        ..MetadataCoverage::none()
    })
    .directory_policy(DirectoryRestorePolicy::ExactTree)
    .needing_reboot()
    .needing_offline()
}

fn verification_result() -> (VerificationResult, VerificationContract) {
    let plan = rich_plan();
    let contract = VerificationContract::new(
        plan.id(),
        VerificationClass::Required,
        "nginx.service",
        "state == running",
    )
    .expecting(Value::string("running"))
    .about(ono_change_core::EquivalenceDomain::RuntimeState);
    let result = VerificationResult::new(
        plan.id().clone(),
        &contract,
        VerificationStatus::Passed,
        at(120),
    )
    .observing(Value::string("running"))
    .citing("systemd unit state")
    .explained("the unit reported active at the observation instant");
    (result, contract)
}

fn resolved_domain() -> PersistenceDomain {
    PersistenceDomain::resolved(
        "/etc/nginx/nginx.conf",
        ResolvedMount::new("42", "/etc", "zfs", "rpool/etc", "/"),
        "zfs-dataset",
        "rpool/etc",
        "the path's state lives in the dataset rpool/etc",
    )
    .with_boundary("rpool/etc")
}

fn refused_domain() -> PersistenceDomain {
    PersistenceDomain::refused(
        "/proc/1/status",
        ResolvedMount::new("13", "/proc", "proc", "proc", "/"),
        NonPersistentReason::Pseudo,
        "procfs is a runtime interface, whatever its mountpoint looks like",
    )
}

fn candidate() -> RecoveryCandidate {
    RecoveryCandidate::new(
        "ono.recovery.zfs",
        scope(),
        EffectDomain::FilesystemPersistent,
        RecoveryObjective::PreserveExact,
        "a zfs snapshot of rpool/etc, taken immediately before the change",
    )
    .at_consistency(ConsistencyClass::FilesystemConsistent)
    .restored_by(RestoreMethod::SelectiveFileRestore)
    .costing(RecoveryCost::unknown().with_space(Some(ByteSize::from_bytes(2048)), None, true))
    .excluding(RecoveryExclusion::new(
        "/etc/nginx/cache",
        "a separate dataset the snapshot does not hold",
    ))
    .needing_to_create("the zfs snapshot capability")
    .needing_to_restore("no unmount, because the restore is selective")
}

// ---------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------

/// Every record §46 declares, so a contract check can sweep all of them at once.
fn every_record() -> Vec<(&'static str, RecordValue)> {
    let plan = rich_plan();
    let (result, contract) = verification_result();
    let action = plan
        .actions()
        .first()
        .cloned()
        .expect("the fixture plan has actions");
    let effect = plan
        .effects()
        .first()
        .cloned()
        .cloned()
        .expect("the fixture plan has effects");
    let row = protection()
        .rows()
        .first()
        .cloned()
        .expect("the fixture matrix has rows");
    vec![
        ("ono.change-plan/1", plan_record(&plan).expect("contract")),
        (
            "ono.plan-action/1",
            action_record(&action).expect("contract"),
        ),
        (
            "ono.proposed-effect/1",
            effect_record(&effect).expect("contract"),
        ),
        (
            "ono.recovery-asset/1",
            asset_record(&asset()).expect("contract"),
        ),
        (
            "ono.recovery-plan/1",
            recovery_plan_record(&recovery_plan()).expect("contract"),
        ),
        (
            "ono.change-verification/1",
            verification_record(&result, contract.expression()).expect("contract"),
        ),
        (
            "ono.impact-graph/1",
            impact_record(plan.id(), &impact()).expect("contract"),
        ),
        (
            "ono.protection-coverage/1",
            coverage_record(&row).expect("contract"),
        ),
        (
            "ono.persistence-domain/1",
            domain_record(&resolved_domain()).expect("contract"),
        ),
        (
            "ono.recovery-candidate/1",
            candidate_record(&candidate()).expect("contract"),
        ),
    ]
}

/// The same record with one field rewritten, so a decoder can be shown a record it must refuse.
fn rewritten(record: &RecordValue, field: &str, value: Value) -> RecordValue {
    let mut builder =
        RecordValue::builder(Arc::clone(record.schema()), record.provenance().clone());
    for declared in record.schema().fields() {
        let name = declared.name();
        let held = if name == field {
            value.clone()
        } else {
            record.get(name).cloned().unwrap_or(Value::Null)
        };
        builder = builder
            .set(name, held)
            .expect("the schema declares its own fields");
    }
    builder.build()
}

// ---------------------------------------------------------------------------------------------
// Every encoder satisfies its contract
// ---------------------------------------------------------------------------------------------

#[test]
fn should_satisfy_the_change_plan_contract_when_a_plan_is_encoded() {
    let record = plan_record(&rich_plan()).expect("§46.1's contract is in this build");
    record
        .validate()
        .expect("§36.5: a record this crate writes must satisfy the contract it claims");
    assert_eq!(record.schema_id().to_string(), "ono.change-plan/1");
}

#[test]
fn should_satisfy_the_plan_action_contract_when_an_action_is_encoded() {
    for action in rich_plan().actions() {
        let record = action_record(action).expect("§46.2's contract is in this build");
        record
            .validate()
            .expect("§36.5: an action record must satisfy ono.plan-action/1");
    }
}

#[test]
fn should_satisfy_the_proposed_effect_contract_when_an_effect_is_encoded() {
    for effect in rich_plan().effects() {
        let record = effect_record(effect).expect("§8.2's contract is in this build");
        record
            .validate()
            .expect("§36.5: an effect record must satisfy ono.proposed-effect/1");
    }
}

#[test]
fn should_satisfy_the_recovery_asset_contract_when_an_asset_is_encoded() {
    let record = asset_record(&asset()).expect("§11.1's contract is in this build");
    record
        .validate()
        .expect("§36.5: an asset record must satisfy ono.recovery-asset/1");
}

#[test]
fn should_satisfy_the_recovery_plan_contract_when_a_recovery_plan_is_encoded() {
    let record = recovery_plan_record(&recovery_plan()).expect("§46.5's contract is in this build");
    record
        .validate()
        .expect("§36.5: a recovery plan record must satisfy ono.recovery-plan/1");
}

#[test]
fn should_satisfy_the_change_verification_contract_when_a_result_is_encoded() {
    let (result, contract) = verification_result();
    let record =
        verification_record(&result, contract.expression()).expect("§23.3's contract is in build");
    record
        .validate()
        .expect("§36.5: a verification record must satisfy ono.change-verification/1");
}

#[test]
fn should_satisfy_the_impact_graph_contract_when_a_graph_is_encoded() {
    let plan = rich_plan();
    let record = impact_record(plan.id(), &impact()).expect("§46.7's contract is in this build");
    record
        .validate()
        .expect("§36.5: an impact record must satisfy ono.impact-graph/1");
}

#[test]
fn should_satisfy_the_protection_coverage_contract_when_a_row_is_encoded() {
    for row in protection().rows() {
        let record = coverage_record(row).expect("§10.3's contract is in this build");
        record
            .validate()
            .expect("§36.5: a coverage row must satisfy ono.protection-coverage/1");
    }
}

#[test]
fn should_satisfy_the_persistence_domain_contract_when_a_path_resolves_or_refuses() {
    for domain in [resolved_domain(), refused_domain()] {
        let record = domain_record(&domain).expect("Appendix B.10's contract is in this build");
        record
            .validate()
            .expect("§32.4: a refusal is an answer and carries the same contract as a resolution");
    }
}

#[test]
fn should_satisfy_the_recovery_candidate_contract_when_a_candidate_is_encoded() {
    let record = candidate_record(&candidate()).expect("Appendix A.3's contract is in this build");
    record
        .validate()
        .expect("§36.5: a candidate record must satisfy ono.recovery-candidate/1");
}

#[test]
fn should_fill_every_required_field_of_every_record_section_forty_six_declares() {
    for (id, record) in every_record() {
        for field in record.schema().fields() {
            if !field.is_required() {
                continue;
            }
            let held = record.get(field.name());
            assert!(
                held.is_some_and(|value| !value.is_null()),
                "§36.5: `{}` of {id} is required and must not be null on the wire",
                field.name()
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The wire never contradicts the matrix beside it
// ---------------------------------------------------------------------------------------------

#[test]
fn should_carry_the_protection_level_the_matrix_composes_when_a_plan_is_encoded() {
    let plan = rich_plan();
    let record = plan_record(&plan).expect("contract");
    assert_eq!(
        record.get("protection_level"),
        Some(&Value::string(plan.protection().level().as_str())),
        "§10.2 and §62.1: the level on the wire is composed from the matrix beside it"
    );
}

#[test]
fn should_refuse_to_call_a_plan_protected_when_a_required_domain_is_uncovered() {
    let matrix = ProtectionSummary::of(vec![
        DomainCoverage::new(
            EffectDomain::FilesystemPersistent,
            RecoveryObjective::PreserveExact,
            DomainProtection::Protected,
            "covered",
        ),
        DomainCoverage::new(
            EffectDomain::ApplicationPersistent,
            RecoveryObjective::PreserveExact,
            DomainProtection::Unprotected,
            "nothing covers the database",
        ),
    ]);
    let plan = draft().with_protection(matrix);
    let record = plan_record(&plan).expect("contract");
    assert_eq!(
        record.get("protection_level"),
        Some(&Value::string(ProtectionLevel::PartiallyProtected.as_str())),
        "§4.6 forbids the word `protected` for partial coverage"
    );
}

#[test]
fn should_carry_every_exclusion_the_matrix_holds_when_a_plan_is_encoded() {
    let plan = rich_plan();
    let record = plan_record(&plan).expect("contract");
    let Some(Value::List(exclusions)) = record.get("coverage_exclusions") else {
        panic!("§10.3 makes coverage_exclusions a list");
    };
    assert!(
        !plan.protection().exclusions().is_empty(),
        "the fixture matrix carries exclusions, or this test proves nothing"
    );
    assert_eq!(
        exclusions.len(),
        plan.protection().exclusions().len(),
        "§62.6: the summary must never hide what the matrix does not cover"
    );
}

#[test]
fn should_carry_no_exclusions_when_the_matrix_holds_none() {
    let plan = draft().with_protection(ProtectionSummary::of(vec![DomainCoverage::new(
        EffectDomain::FilesystemPersistent,
        RecoveryObjective::PreserveExact,
        DomainProtection::Protected,
        "everything the plan touches is covered",
    )]));
    let record = plan_record(&plan).expect("contract");
    assert_eq!(
        record.get("coverage_exclusions"),
        Some(&Value::list([])),
        "§10.3: an empty exclusion list is a claim, so it must not be filled with anything else"
    );
}

#[test]
fn should_count_the_boundaries_beside_the_objects_when_impact_is_summarised() {
    let plan = rich_plan();
    let record = plan_record(&plan).expect("contract");
    let Some(Value::Map(summary)) = record.get("impact_summary") else {
        panic!("§9.5's blast radius is a sub-record");
    };
    assert_eq!(
        summary.get("boundaries"),
        Some(&Value::Int(1)),
        "§9.6: a summary that hides a boundary is the summary the spec forbids"
    );
    assert_eq!(
        summary.get("complete"),
        Some(&Value::Bool(false)),
        "§52.2: a graph cut short by a budget is not a graph that ended"
    );
}

#[test]
fn should_keep_an_unknown_effect_visible_when_a_plan_is_encoded() {
    let plan = rich_plan();
    let record = plan_record(&plan).expect("contract");
    let Some(Value::List(effects)) = record.get("effects") else {
        panic!("§8 makes effects a list");
    };
    let unknown = effects.iter().any(|value| match value {
        Value::Record(record) => record.get("confidence") == Some(&Value::string("unknown")),
        _ => false,
    });
    assert!(
        unknown,
        "§2.4 forbids promoting an unknown effect on the way to a renderer"
    );
}

// ---------------------------------------------------------------------------------------------
// The round trip §36.1's store depends on
// ---------------------------------------------------------------------------------------------

#[test]
fn should_return_the_same_plan_when_a_sealed_plan_is_read_back() {
    let plan = rich_plan();
    let record = plan_record(&plan).expect("contract");
    let read = plan_from_record(&record)
        .expect("§41.2: a plan must be reconstructable from its record")
        .with_impact(impact());
    assert_eq!(
        read, plan,
        "§36.1: a plan read back after a restart is the plan that was written"
    );
}

#[test]
fn should_produce_an_identical_record_when_a_plan_is_encoded_decoded_and_encoded_again() {
    // §36.1's store writes the plan and its impact graph as the two records §46 declares, and
    // reads both back; `impact_summary` on the plan is §9.5's derived summary of the second.
    let plan = rich_plan();
    let written = plan_record(&plan).expect("contract");
    let graph = impact_record(plan.id(), plan.impact()).expect("contract");
    let read = plan_from_record(&written)
        .expect("decodes")
        .with_impact(impact_from_record(&graph).expect("decodes"));
    assert_eq!(
        plan_record(&read).expect("contract"),
        written,
        "§36.5: the bridge is one shape in both directions, or the store rewrites what it read"
    );
    assert_eq!(
        impact_record(read.id(), read.impact()).expect("contract"),
        graph,
        "§9: what the graph record said the second time is what it said the first"
    );
}

#[test]
fn should_keep_the_seal_verifiable_when_a_sealed_plan_is_read_back() {
    let plan = rich_plan();
    let record = plan_record(&plan).expect("contract");
    let read = plan_from_record(&record).expect("decodes");
    assert_eq!(
        read.digest(),
        plan.digest(),
        "§4.4: the digest travels rather than being recomputed on read"
    );
    assert!(
        read.digest_holds(),
        "§63.2: a sealed plan must still be digest-verifiable after a restart"
    );
}

#[test]
fn should_keep_the_lifecycle_state_when_a_plan_that_had_begun_applying_is_read_back() {
    let plan = applying_plan();
    let record = plan_record(&plan).expect("contract");
    let read = plan_from_record(&record).expect("decodes");
    assert_eq!(
        read.state(),
        PlanState::Applying,
        "Appendix F: a plan that came back as a draft would have lost `something may have \
         happened`"
    );
    assert!(read.state().has_mutated());
}

#[test]
fn should_keep_every_action_status_when_a_plan_is_read_back() {
    let plan = rich_plan();
    let read = plan_from_record(&plan_record(&plan).expect("contract")).expect("decodes");
    let statuses: Vec<ActionStatus> = read.actions().iter().map(PlanAction::status).collect();
    assert_eq!(
        statuses,
        vec![
            ActionStatus::Succeeded,
            ActionStatus::Failed,
            ActionStatus::Unknown
        ],
        "Appendix F.2: an unknown outcome is not a failure and not a success, and must survive"
    );
}

#[test]
fn should_keep_the_acknowledgements_when_a_plan_is_read_back() {
    let plan = rich_plan();
    let read = plan_from_record(&plan_record(&plan).expect("contract")).expect("decodes");
    assert!(
        read.risk().is_risk_accepted() && read.risk().is_irreversible_accepted(),
        "§19.4 stores the acknowledgements in the sealed revision, so they travel with it"
    );
    assert!(
        read.outstanding_acknowledgements().is_empty(),
        "a plan whose acknowledgements were given must not ask for them again after a restart"
    );
}

#[test]
fn should_keep_the_protection_matrix_and_its_exclusions_when_a_plan_is_read_back() {
    let plan = rich_plan();
    let read = plan_from_record(&plan_record(&plan).expect("contract")).expect("decodes");
    assert_eq!(read.protection(), plan.protection());
    assert_eq!(
        read.protection().exclusions().len(),
        plan.protection().exclusions().len(),
        "§10.3: a row's exclusions and the plan's are different statements and stay apart"
    );
}

#[test]
fn should_keep_the_preconditions_and_their_tolerance_when_a_plan_is_read_back() {
    let plan = rich_plan();
    let read = plan_from_record(&plan_record(&plan).expect("contract")).expect("decodes");
    let material: Vec<bool> = read
        .actions()
        .iter()
        .flat_map(PlanAction::preconditions)
        .map(Precondition::is_material)
        .collect();
    assert_eq!(
        material,
        vec![true, false],
        "§7.4 makes tolerance a contract, so a tolerated precondition must not come back material"
    );
}

#[test]
fn should_keep_the_frozen_targets_when_a_plan_is_read_back() {
    let plan = rich_plan();
    let read = plan_from_record(&plan_record(&plan).expect("contract")).expect("decodes");
    assert_eq!(
        read.targets(),
        plan.targets(),
        "§2.6: the frozen set is the plan's, and reading it back must not resolve anything again"
    );
}

#[test]
fn should_keep_the_provider_bindings_and_their_versions_when_a_plan_is_read_back() {
    let plan = rich_plan();
    let read = plan_from_record(&plan_record(&plan).expect("contract")).expect("decodes");
    assert_eq!(
        read.providers(),
        plan.providers(),
        "Appendix G.4: a plan resolved against one provider version is not the same plan against \
         another"
    );
}

#[test]
fn should_keep_every_strategy_of_section_twenty_eight_when_a_plan_is_read_back() {
    for strategy in [
        Strategy::Sequential,
        Strategy::batch(4).expect("a batch of four"),
        Strategy::canary(1, 4).expect("a canary of one"),
        Strategy::parallel(3).expect("a bounded width"),
    ] {
        let plan = draft().with_strategy(strategy);
        let read = plan_from_record(&plan_record(&plan).expect("contract")).expect("decodes");
        assert_eq!(
            read.strategy(),
            strategy,
            "§28.5 puts the strategy in the seal, so it must survive the wire exactly"
        );
    }
}

#[test]
fn should_keep_every_execution_method_when_an_action_is_read_back() {
    let plan = draft();
    let executions = [
        Execution::ProviderAction {
            provider: Arc::from("linux.files"),
            operation: Arc::from("ono.file.write"),
            arguments: vec![
                (Arc::from("path"), Value::string("/etc/hosts")),
                (Arc::from("mode"), Value::Int(0o644)),
            ],
        },
        Execution::Program {
            program: Arc::from("/bin/systemctl"),
            argv: vec![Arc::from("restart"), Arc::from("nginx; rm -rf /")],
        },
        Execution::RecoveryOperation {
            provider: Arc::from("ono.recovery.btrfs"),
            capability: Arc::from("recovery.prepare"),
            arguments: vec![(Arc::from("subvolume"), Value::string("@etc"))],
        },
        Execution::Opaque {
            description: Arc::from("a script the operator declared opaque"),
            program: Some(Arc::from("/usr/local/bin/deploy")),
            argv: vec![Arc::from("--all")],
        },
    ];
    for (ordinal, execution) in executions.into_iter().enumerate() {
        let action = PlanAction::new(plan.id(), ordinal, ActionRole::Mutate, "act", execution);
        let record = action_record(&action).expect("contract");
        let read = plan_from_record(
            &plan_record(
                &draft()
                    .with_action(action.clone())
                    .expect("a draft accepts an action"),
            )
            .expect("contract"),
        )
        .expect("decodes");
        record.validate().expect("§46.2's contract");
        assert_eq!(
            read.actions().first(),
            Some(&action),
            "§2.17: an argument vector is data, and it must come back exactly as it went out"
        );
    }
}

#[test]
fn should_keep_a_shell_metacharacter_as_one_argument_when_an_action_is_read_back() {
    let plan = draft();
    let action = PlanAction::new(
        plan.id(),
        1,
        ActionRole::Mutate,
        "destroy a snapshot whose name looks like shell",
        Execution::Program {
            program: Arc::from("/sbin/zfs"),
            argv: vec![Arc::from("destroy"), Arc::from("rpool/etc@a; reboot")],
        },
    );
    let read = plan_from_record(
        &plan_record(&plan.with_action(action).expect("a draft accepts an action"))
            .expect("contract"),
    )
    .expect("decodes");
    let Some(Execution::Program { argv, .. }) = read.actions().first().map(PlanAction::execution)
    else {
        panic!("§12.3 keeps a program and its argument vector apart");
    };
    assert_eq!(
        argv.get(1).map(Arc::as_ref),
        Some("rpool/etc@a; reboot"),
        "§43.6: a snapshot name containing shell syntax is one argument, before and after the wire"
    );
}

#[test]
fn should_keep_a_draft_without_a_digest_when_it_is_read_back() {
    let plan = draft();
    let read = plan_from_record(&plan_record(&plan).expect("contract")).expect("decodes");
    assert_eq!(read, plan);
    assert!(
        read.digest().is_none(),
        "§4.4 puts the digest on the seal, and a draft has none to invent"
    );
}

#[test]
fn should_keep_the_revision_and_what_it_supersedes_when_a_revised_plan_is_read_back() {
    let plan = rich_plan().revise();
    let read = plan_from_record(&plan_record(&plan).expect("contract"))
        .expect("decodes")
        .with_impact(impact());
    assert_eq!(read.revision(), 2);
    assert_eq!(
        read.supersedes(),
        Some(1),
        "§7.5: a revision knows which revision it was derived from"
    );
    assert_eq!(read, plan);
}

#[test]
fn should_return_the_same_graph_when_an_impact_record_is_read_back() {
    let plan = rich_plan();
    let graph = impact();
    let record = impact_record(plan.id(), &graph).expect("contract");
    let read = impact_from_record(&record).expect("decodes");
    assert_eq!(
        read, graph,
        "§9: the graph a store wrote is the graph it reads"
    );
    assert_eq!(
        read.blast_radius(),
        graph.blast_radius(),
        "§9.5's counts are derived, so they cannot disagree with the nodes after a round trip"
    );
}

// ---------------------------------------------------------------------------------------------
// Recovery
// ---------------------------------------------------------------------------------------------

#[test]
fn should_return_the_same_recovery_plan_when_it_is_read_back() {
    let recovery = recovery_plan();
    let record = recovery_plan_record(&recovery).expect("contract");
    let plan = plan_from_record(&plan_record(recovery.plan()).expect("contract")).expect("decodes");
    let read = recovery_plan_from_record(&record, plan).expect("decodes");
    assert_eq!(
        read, recovery,
        "§36.1: a recovery plan read back after a restart is the one that was written"
    );
}

#[test]
fn should_keep_the_newer_state_analysis_when_a_recovery_plan_is_read_back() {
    let recovery = recovery_plan();
    let record = recovery_plan_record(&recovery).expect("contract");
    let plan = plan_from_record(&plan_record(recovery.plan()).expect("contract")).expect("decodes");
    let read = recovery_plan_from_record(&record, plan).expect("decodes");
    assert_eq!(read.newer_state().items().len(), 3);
    assert_eq!(
        read.newer_state().losses().len(),
        1,
        "Appendix C.3: what the method discards must survive the wire"
    );
    assert_eq!(
        read.newer_state().destroyed_assets(),
        recovery.newer_state().destroyed_assets(),
        "§13.6: the provider-native objects recovery would destroy are never dropped quietly"
    );
    assert_eq!(
        read.newer_state().discarded_bytes(),
        Some(ByteSize::from_bytes(8192))
    );
}

#[test]
fn should_gate_a_recovery_that_destroys_newer_state_after_it_is_read_back() {
    let recovery = recovery_plan();
    assert!(recovery.needs_destructive_acceptance());
    let record = recovery_plan_record(&recovery).expect("contract");
    let plan = plan_from_record(&plan_record(recovery.plan()).expect("contract")).expect("decodes");
    let read = recovery_plan_from_record(&record, plan).expect("decodes");
    assert!(
        read.needs_destructive_acceptance(),
        "§24.5: no recovery execution occurs without the explicit gate, restart or no restart"
    );
}

#[test]
fn should_keep_an_accepted_destruction_when_a_recovery_plan_is_read_back() {
    let recovery = recovery_plan().destruction_accepted();
    let record = recovery_plan_record(&recovery).expect("contract");
    let plan = plan_from_record(&plan_record(recovery.plan()).expect("contract")).expect("decodes");
    let read = recovery_plan_from_record(&record, plan).expect("decodes");
    assert!(
        !read.needs_destructive_acceptance(),
        "§24.5: an acceptance the operator gave must not be asked for twice"
    );
    assert_eq!(read, recovery);
}

#[test]
fn should_gate_an_unanalysed_recovery_when_it_is_read_back() {
    let recovery = RecoveryPlan::new(
        draft()
            .seal(instant())
            .expect("a plan that mutates nothing seals"),
        RecoveryGoal::RestoreDomain,
        RestoreMethod::DatasetRollback,
        "rpool/etc@ono-a82f",
    );
    let record = recovery_plan_record(&recovery).expect("contract");
    assert_eq!(
        record.get("newer_state_analysed"),
        Some(&Value::Bool(false)),
        "§62.8: `false` is not `nothing would be lost`"
    );
    let plan = plan_from_record(&plan_record(recovery.plan()).expect("contract")).expect("decodes");
    let read = recovery_plan_from_record(&record, plan).expect("decodes");
    assert!(
        read.needs_destructive_acceptance(),
        "§56.3: an unestablished fact is a reason to block rather than to guess"
    );
    assert_eq!(read, recovery);
}

#[test]
fn should_keep_an_unrecoverable_effect_listed_even_where_a_compensation_exists() {
    let recovery = recovery_plan();
    let record = recovery_plan_record(&recovery).expect("contract");
    let plan = plan_from_record(&plan_record(recovery.plan()).expect("contract")).expect("decodes");
    let read = recovery_plan_from_record(&record, plan).expect("decodes");
    assert_eq!(read.unrecoverable().len(), 2);
    assert!(
        read.unrecoverable()
            .iter()
            .any(|effect| effect.compensation().is_some()),
        "§35.3: a compensation does not remove an effect from the unrecoverable list"
    );
}

#[test]
fn should_keep_the_metadata_gaps_visible_when_a_recovery_plan_is_read_back() {
    let recovery = recovery_plan();
    let record = recovery_plan_record(&recovery).expect("contract");
    let Some(Value::List(gaps)) = record.get("metadata_gaps") else {
        panic!("Appendix C.7 makes the gaps a list");
    };
    assert_eq!(
        gaps.len(),
        5,
        "Appendix C.7: missing metadata support reduces recovery coverage and MUST be visible"
    );
    let plan = plan_from_record(&plan_record(recovery.plan()).expect("contract")).expect("decodes");
    let read = recovery_plan_from_record(&record, plan).expect("decodes");
    assert_eq!(read.metadata(), recovery.metadata());
}

// ---------------------------------------------------------------------------------------------
// Assets
// ---------------------------------------------------------------------------------------------

#[test]
fn should_return_the_same_asset_in_each_of_its_seven_states_when_it_is_read_back() {
    let complete = RecoveryValidation::complete(at(30), "every §11.4 check passed");
    let partial = RecoveryValidation::none(at(30), "the scope did not match").existing(true);
    let assets = [
        asset(),
        asset().creating(),
        asset().validated(complete),
        asset().validated(partial),
        asset().expired(),
        asset().removed(),
        asset().failed(),
    ];
    let states: Vec<AssetState> = assets.iter().map(RecoveryAsset::state).collect();
    assert_eq!(
        states,
        vec![
            AssetState::Proposed,
            AssetState::Creating,
            AssetState::Ready,
            AssetState::Invalid,
            AssetState::Expired,
            AssetState::Removed,
            AssetState::Failed,
        ],
        "§11.1's lifecycle has seven states, and the fixture must exercise all of them"
    );
    for asset in assets {
        let record = asset_record(&asset).expect("contract");
        record.validate().expect("§11.1's contract");
        assert_eq!(
            asset_from_record(&record).expect("decodes"),
            asset,
            "§11.4: `ready` is a claim only a validation may make, so the state travels"
        );
    }
}

#[test]
fn should_keep_the_asset_identity_rather_than_deriving_it_again_on_read() {
    let plan = rich_plan();
    let asset = asset().for_plan(plan.id().clone());
    let record = asset_record(&asset).expect("contract");
    let read = asset_from_record(&record).expect("decodes");
    assert_eq!(
        read.id(),
        asset.id(),
        "§11.1: the id is what a plan references and what `remove recovery` names"
    );
    assert_eq!(read.source_plan(), asset.source_plan());
}

#[test]
fn should_keep_an_unvalidated_asset_unvalidated_when_it_is_read_back() {
    let record = asset_record(&asset()).expect("contract");
    assert_eq!(
        record.get("validation"),
        Some(&Value::Null),
        "§11.4: an asset nothing has validated is not an asset that failed validation"
    );
    assert!(
        asset_from_record(&record)
            .expect("decodes")
            .validation()
            .is_none()
    );
}

#[test]
fn should_keep_the_hold_and_the_retention_window_when_an_asset_is_read_back() {
    let read = asset_from_record(&asset_record(&asset()).expect("contract")).expect("decodes");
    assert!(
        read.retention().is_held(),
        "§37.2: a hold is what keeps a failed plan's assets out of ordinary success retention"
    );
    assert_eq!(read.retention().window(), Duration::from_secs(3600));
}

#[test]
fn should_keep_an_unmeasurable_size_null_rather_than_zero_when_an_asset_is_read_back() {
    let asset = RecoveryAsset::proposed(
        "ono.recovery.file-copy",
        RecoveryAssetType::FileArchive,
        "/var/lib/ono/recovery/a82f",
        scope(),
        instant(),
    );
    let record = asset_record(&asset).expect("contract");
    assert_eq!(record.get("initial_size"), Some(&Value::Null));
    let read = asset_from_record(&record).expect("decodes");
    assert_eq!(
        read.cost().initial_bytes(),
        None,
        "§38.2: null is unknown, and Ono does not display `free` for a size it never measured"
    );
}

// ---------------------------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------------------------

#[test]
fn should_refuse_a_plan_record_whose_state_is_not_a_word_of_the_vocabulary() {
    let record = plan_record(&rich_plan()).expect("contract");
    let corrupt = rewritten(&record, "state", Value::string("almost-applied"));
    let error = plan_from_record(&corrupt).expect_err("§2.4 forbids reading an unknown as default");
    assert_eq!(error.code().name(), "change.plan_store_corrupt");
    assert!(
        error
            .help()
            .is_some_and(|help| help.contains("state") && help.contains("almost-applied")),
        "§45: a refusal names the field and the word it could not read, got {:?}",
        error.help()
    );
    assert_eq!(error.metadata().get("field"), Some(&Value::string("state")));
}

#[test]
fn should_refuse_an_action_record_whose_role_is_not_a_word_of_the_vocabulary() {
    let plan = rich_plan();
    let record = plan_record(&plan).expect("contract");
    let Some(Value::List(actions)) = record.get("actions") else {
        panic!("§46.1 makes actions a list");
    };
    let Some(Value::Record(first)) = actions.first() else {
        panic!("an action is a record");
    };
    let broken = rewritten(first, "role", Value::string("mutilate"));
    let corrupt = rewritten(
        &record,
        "actions",
        Value::list([Value::Record(Arc::new(broken))]),
    );
    let error = plan_from_record(&corrupt).expect_err("an unknown role is refused");
    assert_eq!(error.code().name(), "change.plan_store_corrupt");
    assert!(
        error.help().is_some_and(|help| help.contains("role")),
        "§45: the refusal names the field, got {:?}",
        error.help()
    );
}

#[test]
fn should_refuse_an_asset_record_whose_state_is_not_a_word_of_the_vocabulary() {
    let record = asset_record(&asset()).expect("contract");
    let corrupt = rewritten(&record, "state", Value::string("nearly-ready"));
    let error = asset_from_record(&corrupt).expect_err("§11.1's lifecycle is a closed list");
    assert_eq!(error.code().name(), "change.plan_store_corrupt");
    assert!(
        error
            .help()
            .is_some_and(|help| help.contains("nearly-ready"))
    );
}

#[test]
fn should_refuse_a_plan_record_that_is_missing_a_required_field() {
    for field in ["id", "intent", "session", "created_at", "strategy"] {
        let record = plan_record(&rich_plan()).expect("contract");
        let corrupt = rewritten(&record, field, Value::Null);
        let error =
            plan_from_record(&corrupt).expect_err("§36.2: a record with a hole is not a plan");
        assert_eq!(error.code().name(), "change.plan_store_corrupt");
        assert!(
            error.help().is_some_and(|help| help.contains(field)),
            "§45: the refusal names `{field}`, got {:?}",
            error.help()
        );
    }
}

#[test]
fn should_refuse_an_asset_record_that_is_missing_a_required_field() {
    for field in ["id", "provider", "reference", "scope", "retention"] {
        let record = asset_record(&asset()).expect("contract");
        let corrupt = rewritten(&record, field, Value::Null);
        let error = asset_from_record(&corrupt).expect_err("§36.2: a record with a hole");
        assert_eq!(error.code().name(), "change.plan_store_corrupt");
        assert!(
            error.help().is_some_and(|help| help.contains(field)),
            "§45: the refusal names `{field}`, got {:?}",
            error.help()
        );
    }
}

#[test]
fn should_refuse_a_plan_record_whose_identity_is_not_an_identity() {
    let record = plan_record(&rich_plan()).expect("contract");
    let corrupt = rewritten(&record, "id", Value::string("../etc/passwd"));
    let error = plan_from_record(&corrupt).expect_err("§36.4 resolves identities, not paths");
    assert_eq!(error.code().name(), "change.plan_store_corrupt");
}

#[test]
fn should_refuse_a_plan_record_whose_strategy_is_not_one_of_the_four() {
    let record = plan_record(&rich_plan()).expect("contract");
    for spelling in [
        "parallel",
        "parallel 0",
        "as fast as possible",
        "batch many",
    ] {
        let corrupt = rewritten(&record, "strategy", Value::string(spelling));
        let error = plan_from_record(&corrupt)
            .expect_err("§28.4: unlimited parallel mutation is not a strategy");
        assert_eq!(
            error.code().name(),
            "change.plan_store_corrupt",
            "`{spelling}` must be refused rather than read as sequential"
        );
    }
}

#[test]
fn should_refuse_a_plan_record_whose_acknowledgement_is_not_one_section_forty_spells() {
    let record = plan_record(&rich_plan()).expect("contract");
    let corrupt = rewritten(
        &record,
        "accepted_risk_overrides",
        Value::list([Value::string("--yes")]),
    );
    let error = plan_from_record(&corrupt)
        .expect_err("§19.4: an acknowledgement is a recorded decision, not any string");
    assert_eq!(error.code().name(), "change.plan_store_corrupt");
}

#[test]
fn should_refuse_a_recovery_plan_record_whose_metadata_piece_is_not_in_appendix_c_seven() {
    let recovery = recovery_plan();
    let record = recovery_plan_record(&recovery).expect("contract");
    let corrupt = rewritten(
        &record,
        "metadata_restored",
        Value::list([Value::string("everything")]),
    );
    let plan = plan_from_record(&plan_record(recovery.plan()).expect("contract")).expect("decodes");
    let error = recovery_plan_from_record(&corrupt, plan)
        .expect_err("Appendix C.7 lists the pieces a restore can claim");
    assert_eq!(error.code().name(), "change.plan_store_corrupt");
}

#[test]
fn should_refuse_a_plan_record_whose_execution_method_is_not_structured() {
    let plan = rich_plan();
    let record = plan_record(&plan).expect("contract");
    let Some(Value::List(actions)) = record.get("actions") else {
        panic!("§46.1 makes actions a list");
    };
    let Some(Value::Record(first)) = actions.first() else {
        panic!("an action is a record");
    };
    let mut execution = ono_value::MapValue::new();
    execution.insert("method".into(), Value::string("shell"));
    let broken = rewritten(first, "execution", Value::Map(Arc::new(execution)));
    let corrupt = rewritten(
        &record,
        "actions",
        Value::list([Value::Record(Arc::new(broken))]),
    );
    let error = plan_from_record(&corrupt)
        .expect_err("§2.17: there is no execution method that holds a command line");
    assert_eq!(error.code().name(), "change.plan_store_corrupt");
}

// ---------------------------------------------------------------------------------------------
// The three round-trip losses the first pass left, and the fields that closed them.
//
// Each of these was reachable: a plan came back with a different digest, or an asset came back
// claiming a cost nobody had measured. §36.1 persists sealed plans so they survive shell exit, and
// a plan that survives as something slightly different has not survived.
// ---------------------------------------------------------------------------------------------

#[test]
fn should_keep_a_policy_declaration_of_irrelevance_when_a_plan_is_read_back() {
    // Appendix A.7's only escape from the unknown cap is an operator's explicit declaration, and
    // §4.4 puts the protection policy in the seal. A row that came back without the declaration
    // would re-seal to a different digest, and the plan store would disagree with itself.
    let summary = ProtectionSummary::of(vec![
        DomainCoverage::new(
            EffectDomain::FilesystemPersistent,
            RecoveryObjective::PreserveExact,
            DomainProtection::Protected,
            "zfs snapshot of rpool/ROOT/debian",
        ),
        DomainCoverage::new(
            EffectDomain::Unknown,
            RecoveryObjective::RestoreSemantic,
            DomainProtection::Compensatable,
            "an opaque action the operator declared irrelevant",
        )
        .declared_irrelevant(),
    ]);
    let plan = rich_plan().with_protection(summary);
    let record = plan_record(&plan).expect("contract");
    let read = plan_from_record(&record).expect("round trip");
    let declared: Vec<bool> = read
        .protection()
        .rows()
        .iter()
        .map(DomainCoverage::is_declared_irrelevant)
        .collect();
    assert_eq!(
        declared,
        vec![false, true],
        "Appendix A.7's declaration must survive the store, or the cap it lifts comes back"
    );
    assert_eq!(
        read.protection().level(),
        ProtectionLevel::Protected,
        "and the level it composes to must be the same one the operator saw"
    );
}

#[test]
fn should_keep_a_no_recovery_row_that_policy_declared_irrelevant() {
    // The narrow case the first encoding lost: `required` is already false for a
    // `no-recovery-required` objective, so inferring the declaration from that pair discarded it
    // exactly where it changed nothing visible — and changed the digest.
    let summary = ProtectionSummary::of(vec![
        DomainCoverage::new(
            EffectDomain::NetworkRuntime,
            RecoveryObjective::NoRecoveryRequired,
            DomainProtection::Unprotected,
            "active sessions are not recovered",
        )
        .declared_irrelevant(),
    ]);
    let before = rich_plan().with_protection(summary);
    let record = plan_record(&before).expect("contract");
    let after = plan_from_record(&record).expect("round trip");
    assert_eq!(
        before.protection().digest_text(),
        after.protection().digest_text(),
        "§4.4: a plan read back from the store must seal to the digest it was written with"
    );
}

#[test]
fn should_keep_the_measured_cost_of_an_asset_when_it_is_read_back() {
    // §38.1 names initial latency and quiesce duration as cost dimensions, and §18.4 makes the
    // quiesce window something an operator agreed to. An asset that came back without them looked
    // cheaper than the one that was created.
    let costed = asset().costing(
        RecoveryCost::unknown()
            .with_space(Some(ByteSize::from_bytes(4096)), None, true)
            .with_latency(Duration::from_millis(140))
            .with_quiesce(Duration::from_secs(3)),
    );
    let record = asset_record(&costed).expect("contract");
    let read = asset_from_record(&record).expect("round trip");
    assert_eq!(
        read.cost().creation_latency(),
        Some(Duration::from_millis(140)),
        "§38.1: initial latency is a cost dimension, and a stored asset keeps it"
    );
    assert_eq!(
        read.cost().quiesce(),
        Some(Duration::from_secs(3)),
        "§18.4: the quiesce window an operator accepted is not re-derivable afterwards"
    );
    assert!(
        read.cost().is_estimated(),
        "§37.5: the estimated label travels with the figures it qualifies"
    );
}

#[test]
fn should_report_an_unmeasured_cost_as_unknown_rather_than_as_zero() {
    let record = asset_record(&asset()).expect("contract");
    let read = asset_from_record(&record).expect("round trip");
    assert_eq!(
        read.cost().creation_latency(),
        None,
        "spec v0.2 §35.3: unknown data is null, never fabricated or zero"
    );
    assert_eq!(read.cost().quiesce(), None);
}

#[test]
fn should_carry_the_condition_a_verification_result_answered() {
    // §23.3 lists `expression` beside `observed` and `expected` for the reason both of those are
    // there: a result that says only FAILED has told the operator nothing they can act on.
    let plan = rich_plan();
    let contract = VerificationContract::new(
        plan.id(),
        VerificationClass::Required,
        "nginx.service",
        "state == running",
    );
    let result = VerificationResult::new(
        plan.id().clone(),
        &contract,
        VerificationStatus::Failed,
        Timestamp::UNIX_EPOCH,
    );
    assert_eq!(
        result.expression(),
        "state == running",
        "a result carries what it checked, so it can be read without its contract"
    );
    let record = verification_record(&result, result.expression()).expect("contract");
    assert_eq!(
        record.get("expression"),
        Some(&Value::string("state == running"))
    );
}

#[test]
fn should_report_each_validation_check_without_reading_its_prose() {
    // §11.4's five checks are facts, and a caller that has to match on sentences to learn which
    // one failed is a caller that breaks when the sentence is reworded.
    let validation = RecoveryValidation::complete(Timestamp::UNIX_EPOCH, "checked").scope(false);
    assert!(validation.exists());
    assert!(validation.identity_matches());
    assert!(
        !validation.scope_matches(),
        "§11.4: a scope that does not match the expected target is not protection"
    );
    assert!(validation.restore_available());
    assert!(validation.permissions_present());
    assert!(!validation.is_complete());

    let checked = asset().validated(validation);
    let record = asset_record(&checked).expect("contract");
    let read = asset_from_record(&record).expect("round trip");
    assert_eq!(
        read.validation().map(RecoveryValidation::scope_matches),
        Some(false),
        "and the failing check survives the store, because §11.1's INVALID state rests on it"
    );
    assert_eq!(read.state(), AssetState::Invalid);
}
