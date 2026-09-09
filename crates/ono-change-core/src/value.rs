//! The bridge from the v0.6 types to the public schemas of §46.
//!
//! This is the single place a change value becomes an Ono value, and the single place one is read
//! back. §36.4's drift check then has one producer to compare against `docs/contracts/schemas/`,
//! and no other crate spells a v0.6 field name by hand — which is what kept the file schema and
//! the interface contract from drifting apart in v0.2 (see `ono_value::builtin`) and what v0.5's
//! `ono_temporal_core::value` does for the temporal family.
//!
//! # What travels, and in what shape
//!
//! §46 names ten records, and the house convention v0.4 set with `spatial-place.v1.yaml` decides
//! how the pieces nest: a list of addressable objects is a list of records bound to their own
//! schema — a plan's actions are `ono.plan-action/1`, its protection matrix is
//! `ono.protection-coverage/1` — and a structure with no schema of its own is a `record` field
//! carrying an `ono_value::MapValue`. Every one of those map shapes is documented on the helper
//! that builds it, because a map with no contract file is still a wire shape somebody will parse.
//!
//! # What the wire may never disagree about
//!
//! Three fields are computed here rather than copied, because §62.1 is what happens when a
//! summary can contradict the detail beside it:
//!
//! - `protection_level` comes from [`crate::ProtectionSummary::level`], never from a caller;
//! - `coverage_exclusions` carries every exclusion the matrix holds (§10.3, §62.6), so protection
//!   is never rendered without what it leaves out;
//! - `impact_summary` is [`crate::ImpactGraph::blast_radius`], which counts the boundaries §9.6
//!   forbids a summary to hide.
//!
//! # Reading back
//!
//! [`plan_from_record`], [`recovery_plan_from_record`] and [`asset_from_record`] are what §36.1's
//! persistent store uses to survive a shell restart. §41.2 requires plan state to be
//! reconstructable from persisted records after a crash, so they restore the lifecycle state, the
//! seal digest, the action statuses and the acknowledgements exactly as they were written — a plan
//! that came back as a fresh draft would have lost the one fact the operator needs. A record whose
//! enum spelling is not in the vocabulary, or whose required field is missing, is refused by
//! [`crate::error::store_corrupt`] naming the field: §2.4 forbids reading an unknown word as a
//! default.

use std::sync::Arc;

use jiff::Timestamp;
use ono_core::ErrorCode;
use ono_value::{
    ByteSize, ErrorValue, MapValue, Percent, Provenance, RecordBuilder, RecordValue, Schema,
    SchemaId, Value, builtin_schemas,
};

use crate::action::{ActionRole, ActionStatus, Execution, Idempotency, PlanAction};
use crate::asset::{
    AssetState, RecoveryAsset, RecoveryAssetType, RecoveryCost, RecoveryExclusion, RecoveryScope,
    RecoveryValidation, RestoreMethod, RetentionPolicy,
};
use crate::domain::PersistenceDomain;
use crate::effect::{EffectConfidence, EffectDomain, EffectKind, ProposedEffect};
use crate::id::{ActionId, EffectId, PlanId, RecoveryAssetId};
use crate::impact::{ImpactClass, ImpactGraph, ImpactNode, UnknownBoundary};
use crate::plan::{ChangePlan, Intent, PlanKind, ProviderBinding};
use crate::protection::{
    ConsistencyClass, CoverageExclusion, DomainCoverage, DomainProtection, ProtectionMode,
    ProtectionSummary, RecoveryObjective,
};
use crate::provider::RecoveryCandidate;
use crate::recovery::{
    DirectoryRestorePolicy, MetadataCoverage, NewerStateClass, NewerStateImpact, NewerStateItem,
    RecoveryGoal, RecoveryPlan, UnrecoverableEffect,
};
use crate::risk::{RiskAssessment, RiskClass, RiskDimension, RiskFinding};
use crate::state::PlanState;
use crate::strategy::Strategy;
use crate::target::{FrozenTarget, Precondition, PreconditionKind};
use crate::verification::{
    EquivalenceDomain, VerificationClass, VerificationContract, VerificationResult,
    VerificationSet, VerificationStatus, duration_value,
};

/// The provenance provider every v0.6 record carries (§46).
///
/// One string for the whole family, so a consumer can tell a record this crate produced from a
/// provider's own reading of the same object without inspecting the schema id.
const PROVIDER: &str = "ono.change";

/// The acknowledgement flag §40.3 spells for an accepted risk class (§19.4).
const ACCEPT_RISK: &str = "--accept-risk";

/// The acknowledgement flag §40.3 spells for accepted irreversible effects (§19.4).
const ACCEPT_IRREVERSIBLE: &str = "--accept-irreversible";

/// The pieces of file metadata Appendix C.7 requires a restore to account for, in its order.
///
/// The spellings are [`MetadataCoverage::gaps`]'s, so the list a plan view prints as missing and
/// the list a record carries as restored are drawn from one vocabulary rather than two.
const METADATA_PIECES: [&str; 8] = [
    "content",
    "mode",
    "owner/group",
    "ACLs",
    "extended attributes",
    "file capabilities",
    "SELinux labels",
    "hard-link relationships",
];

// ---------------------------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------------------------

/// The `ono.change-plan/1` record of §46.1.
///
/// `protection_level`, `coverage_exclusions`, `impact_summary`, `effects`, `preconditions` and
/// `requires_privilege` are derived from the plan rather than taken from a caller, so the wire
/// cannot carry a summary that disagrees with the detail beside it (§10.2, §62.1).
///
/// # Errors
///
/// Returns `ono.provider_schema_violation` where a contract of §46 is not in this build, which the
/// `ono-value` contract test and `cargo run -p xtask -- spec-check` both prevent from shipping.
pub fn plan_record(plan: &ChangePlan) -> Result<RecordValue, ErrorValue> {
    let (schema, provenance) = target("ono.change-plan")?;
    let builder = RecordValue::builder(schema, provenance);

    let mut actions = Vec::with_capacity(plan.actions().len());
    let mut effects = Vec::new();
    let mut preconditions = Vec::new();
    for action in plan.actions() {
        actions.push(Value::Record(Arc::new(action_record(action)?)));
        for effect in action.effects() {
            effects.push(Value::Record(Arc::new(effect_record(effect)?)));
        }
        for precondition in action.preconditions() {
            preconditions.push(precondition_map(precondition));
        }
    }
    let mut protection = Vec::with_capacity(plan.protection().rows().len());
    for row in plan.protection().rows() {
        protection.push(Value::Record(Arc::new(coverage_record(row)?)));
    }

    let builder = put(builder, "id", Value::string(plan.id().as_str()));
    let builder = put(builder, "revision", Value::Int(i128::from(plan.revision())));
    let builder = put(builder, "kind", Value::string(plan.kind().as_str()));
    let builder = put(builder, "state", Value::string(plan.state().as_str()));
    let builder = put(builder, "intent", Value::string(plan.intent().text()));
    let builder = put(builder, "source", Value::string(plan.intent().source()));
    let builder = put(builder, "session", Value::string(plan.session()));
    let builder = put(builder, "created_at", Value::Timestamp(plan.created_at()));
    let builder = put(
        builder,
        "sealed_at",
        plan.sealed_at().map_or(Value::Null, Value::Timestamp),
    );
    let builder = put(
        builder,
        "expires_at",
        plan.expires_at().map_or(Value::Null, Value::Timestamp),
    );
    let builder = put(
        builder,
        "targets",
        Value::list(plan.targets().iter().map(frozen_target_map)),
    );
    let builder = put(builder, "actions", Value::list(actions));
    let builder = put(builder, "effects", Value::list(effects));
    let builder = put(builder, "impact_summary", impact_summary_map(plan.impact()));
    let builder = put(builder, "protection", Value::list(protection));
    let builder = put(
        builder,
        "protection_level",
        Value::string(plan.protection().level().as_str()),
    );
    let builder = put(
        builder,
        "protection_mode",
        Value::string(plan.protection_mode().as_str()),
    );
    let builder = put(
        builder,
        "coverage_exclusions",
        Value::list(
            plan.protection()
                .exclusions()
                .into_iter()
                .map(coverage_exclusion_map),
        ),
    );
    let builder = put(
        builder,
        "risk",
        Value::string(plan.risk().classify().as_str()),
    );
    let builder = put(
        builder,
        "risk_findings",
        Value::list(plan.risk().findings().iter().map(risk_finding_map)),
    );
    let builder = put(
        builder,
        "strategy",
        Value::string(&plan.strategy().to_string()),
    );
    let builder = put(
        builder,
        "verification_contracts",
        Value::list(
            plan.verification()
                .contracts()
                .iter()
                .map(verification_contract_map),
        ),
    );
    let builder = put(builder, "preconditions", Value::list(preconditions));
    let builder = put(
        builder,
        "provider_bindings",
        Value::list(plan.providers().iter().map(provider_binding_map)),
    );
    let builder = put(
        builder,
        "accepted_risk_overrides",
        Value::list(acknowledgement_flags(plan.risk())),
    );
    let builder = put(
        builder,
        "requires_privilege",
        Value::Bool(plan.needs_privilege()),
    );
    let builder = put(builder, "digest", optional_text(plan.digest()));
    Ok(builder.build())
}

/// The `ono.plan-action/1` record of §46.2.
///
/// `provider` is [`Execution::actor`] rather than a field of its own on [`PlanAction`]: §4.4 seals
/// the provider bindings, and an action that named a different provider from the one it runs
/// through would put two answers on the wire.
///
/// # Errors
///
/// Returns `ono.provider_schema_violation` where a contract of §46 is not in this build.
pub fn action_record(action: &PlanAction) -> Result<RecordValue, ErrorValue> {
    let (schema, provenance) = target("ono.plan-action")?;
    let builder = RecordValue::builder(schema, provenance);
    let mut effects = Vec::with_capacity(action.effects().len());
    for effect in action.effects() {
        effects.push(Value::Record(Arc::new(effect_record(effect)?)));
    }

    let builder = put(builder, "id", Value::string(action.id().as_str()));
    let builder = put(
        builder,
        "ordinal",
        Value::Int(i128::try_from(action.ordinal()).unwrap_or(i128::MAX)),
    );
    let builder = put(builder, "role", Value::string(action.role().as_str()));
    let builder = put(builder, "summary", Value::string(action.summary()));
    let builder = put(builder, "target", optional_text(action.target()));
    let builder = put(builder, "execution", execution_map(action.execution()));
    let builder = put(
        builder,
        "provider",
        Value::string(action.execution().actor()),
    );
    let builder = put(
        builder,
        "depends_on",
        Value::list(
            action
                .depends_on()
                .iter()
                .map(|id| Value::string(id.as_str())),
        ),
    );
    let builder = put(
        builder,
        "preconditions",
        Value::list(action.preconditions().iter().map(precondition_map)),
    );
    let builder = put(
        builder,
        "idempotency",
        Value::string(action.idempotency().as_str()),
    );
    let builder = put(builder, "proposed_effects", Value::list(effects));
    let builder = put(
        builder,
        "recovery_semantics",
        optional_text(action.declared_recovery()),
    );
    let builder = put(
        builder,
        "requires_privilege",
        Value::Bool(action.needs_privilege()),
    );
    // §23.1 binds checks to the plan rather than to one action, so an action states no check of
    // its own and the nullable field stays null instead of repeating the plan's set.
    let builder = put(builder, "verification", Value::Null);
    let builder = put(builder, "status", Value::string(action.status().as_str()));
    Ok(builder.build())
}

/// The `ono.proposed-effect/1` record of §8.2.
///
/// # Errors
///
/// Returns `ono.provider_schema_violation` where the contract is not in this build.
pub fn effect_record(effect: &ProposedEffect) -> Result<RecordValue, ErrorValue> {
    let (schema, provenance) = target("ono.proposed-effect")?;
    let builder = RecordValue::builder(schema, provenance);
    let builder = put(builder, "id", Value::string(effect.id().as_str()));
    let builder = put(
        builder,
        "action_id",
        Value::string(effect.action().as_str()),
    );
    let builder = put(builder, "object", optional_text(effect.object()));
    let builder = put(builder, "domain", Value::string(effect.domain().as_str()));
    let builder = put(builder, "kind", Value::string(effect.kind().as_str()));
    let builder = put(
        builder,
        "confidence",
        Value::string(effect.confidence().as_str()),
    );
    let builder = put(
        builder,
        "before",
        effect.before().cloned().unwrap_or(Value::Null),
    );
    let builder = put(
        builder,
        "proposed",
        effect.proposed().cloned().unwrap_or(Value::Null),
    );
    let builder = put(
        builder,
        "evidence",
        Value::list(effect.evidence().iter().map(|text| Value::string(text))),
    );
    let builder = put(builder, "explanation", Value::string(effect.explanation()));
    let builder = put(
        builder,
        "irreversible",
        Value::Bool(effect.is_irreversible()),
    );
    let builder = put(
        builder,
        "compensation",
        optional_text(effect.compensation()),
    );
    Ok(builder.build())
}

/// The `ono.recovery-asset/1` record of §11.1.
///
/// `shares_failure_domain` is read off the mechanism (§11.5): a copy-on-write snapshot lives on
/// the storage it protects, and that fact travels with the asset so a renderer never has to
/// remember which types are recovery points and which are backups.
///
/// # Errors
///
/// Returns `ono.provider_schema_violation` where the contract is not in this build.
pub fn asset_record(asset: &RecoveryAsset) -> Result<RecordValue, ErrorValue> {
    let (schema, provenance) = target("ono.recovery-asset")?;
    let builder = RecordValue::builder(schema, provenance);
    let cost = asset.cost();

    let builder = put(builder, "id", Value::string(asset.id().as_str()));
    let builder = put(builder, "provider", Value::string(asset.provider()));
    let builder = put(builder, "type", Value::string(asset.asset_type().as_str()));
    let builder = put(builder, "reference", Value::string(asset.reference()));
    let builder = put(builder, "host", Value::string(asset.scope().host()));
    let builder = put(builder, "scope", recovery_scope_map(asset.scope()));
    let builder = put(builder, "created_at", Value::Timestamp(asset.created_at()));
    let builder = put(
        builder,
        "source_plan",
        asset
            .source_plan()
            .map_or(Value::Null, |plan| Value::string(plan.as_str())),
    );
    let builder = put(builder, "state", Value::string(asset.state().as_str()));
    let builder = put(
        builder,
        "consistency",
        Value::string(asset.consistency().as_str()),
    );
    let builder = put(
        builder,
        "restore_method",
        Value::string(asset.restore_method().as_str()),
    );
    let builder = put(
        builder,
        "validation",
        asset.validation().map_or(Value::Null, validation_map),
    );
    let builder = put(
        builder,
        "retention",
        duration_value(asset.retention().window()),
    );
    let builder = put(
        builder,
        "expires_at",
        asset.expires_at().map_or(Value::Null, Value::Timestamp),
    );
    let builder = put(builder, "held", Value::Bool(asset.retention().is_held()));
    let builder = put(
        builder,
        "initial_size",
        cost.initial_bytes().map_or(Value::Null, Value::ByteSize),
    );
    let builder = put(
        builder,
        "retained_size",
        cost.retained_bytes().map_or(Value::Null, Value::ByteSize),
    );
    let builder = put(builder, "size_estimated", Value::Bool(cost.is_estimated()));
    let builder = put(
        builder,
        "creation_latency",
        cost.creation_latency()
            .map_or(Value::Null, crate::verification::duration_value),
    );
    let builder = put(
        builder,
        "io_overhead",
        cost.io_overhead().map_or(Value::Null, Value::Percent),
    );
    let builder = put(
        builder,
        "quiesce_duration",
        cost.quiesce()
            .map_or(Value::Null, crate::verification::duration_value),
    );
    let builder = put(
        builder,
        "requires_reboot",
        Value::Bool(cost.requires_reboot()),
    );
    let builder = put(
        builder,
        "requires_offline",
        Value::Bool(cost.requires_offline()),
    );
    let builder = put(
        builder,
        "cleanup_latency",
        cost.cleanup_latency()
            .map_or(Value::Null, crate::verification::duration_value),
    );
    let builder = put(
        builder,
        "shares_failure_domain",
        Value::Bool(asset.is_local_recovery_point()),
    );
    let builder = put(
        builder,
        "dependencies",
        Value::list(
            asset
                .dependencies()
                .iter()
                .map(|id| Value::string(id.as_str())),
        ),
    );
    let builder = put(
        builder,
        "exclusions",
        Value::list(asset.exclusions().iter().map(recovery_exclusion_map)),
    );
    let builder = put(
        builder,
        "captured_state",
        optional_text(asset.captured_state()),
    );
    Ok(builder.build())
}

/// The `ono.recovery-plan/1` record of §46.5.
///
/// `requires_acceptance` is [`RecoveryPlan::needs_destructive_acceptance`], which answers `true`
/// for an unanalysed recovery: §62.8 names recovery without drift analysis as a failure mode, and
/// §56.3 makes an unestablished fact a reason to block rather than to guess.
///
/// # Errors
///
/// Returns `ono.provider_schema_violation` where a contract of §46 is not in this build.
pub fn recovery_plan_record(plan: &RecoveryPlan) -> Result<RecordValue, ErrorValue> {
    let (schema, provenance) = target("ono.recovery-plan")?;
    let builder = RecordValue::builder(schema, provenance);
    let inner = plan.plan();
    let newer = plan.newer_state();

    let mut actions = Vec::with_capacity(inner.actions().len());
    for action in inner.actions() {
        actions.push(Value::Record(Arc::new(action_record(action)?)));
    }
    let (restored, gaps) = metadata_lists(plan.metadata());

    let builder = put(builder, "id", Value::string(inner.id().as_str()));
    let builder = put(
        builder,
        "source_plan",
        plan.source_plan()
            .map_or(Value::Null, |id| Value::string(id.as_str())),
    );
    let builder = put(
        builder,
        "source_assets",
        Value::list(
            plan.source_assets()
                .iter()
                .map(|id| Value::string(id.as_str())),
        ),
    );
    // §2.12 puts recovery through the same lifecycle as any other change, so the state travels:
    // `recovery-planned` means nothing has been restored, and `recovery-failed` means the
    // remaining assets and the exact partial state are preserved (Appendix F).
    let builder = put(
        builder,
        "state",
        Value::string(plan.plan().state().as_str()),
    );
    let builder = put(builder, "goal", Value::string(plan.goal().as_str()));
    let builder = put(builder, "method", Value::string(plan.method().as_str()));
    let builder = put(builder, "target_state", Value::string(plan.target_state()));
    let builder = put(builder, "restore_actions", Value::list(actions));
    let builder = put(
        builder,
        "restores",
        Value::list(plan.restores().iter().map(|text| Value::string(text))),
    );
    let builder = put(
        builder,
        "newer_state",
        Value::list(newer.items().iter().map(newer_state_map)),
    );
    let builder = put(
        builder,
        "newer_state_analysed",
        Value::Bool(newer.is_complete()),
    );
    let builder = put(
        builder,
        "destroyed_assets",
        Value::list(
            newer
                .destroyed_assets()
                .iter()
                .map(|text| Value::string(text)),
        ),
    );
    let builder = put(
        builder,
        "discarded_size",
        newer.discarded_bytes().map_or(Value::Null, Value::ByteSize),
    );
    let builder = put(
        builder,
        "unrecoverable_effects",
        Value::list(plan.unrecoverable().iter().map(unrecoverable_effect_map)),
    );
    let builder = put(
        builder,
        "metadata_restored",
        Value::list(restored.iter().map(|name| Value::string(name))),
    );
    let builder = put(
        builder,
        "metadata_gaps",
        Value::list(gaps.iter().map(|name| Value::string(name))),
    );
    let builder = put(
        builder,
        "directory_policy",
        Value::string(plan.directory_restore_policy().as_str()),
    );
    let builder = put(
        builder,
        "risk",
        Value::string(inner.risk().classify().as_str()),
    );
    let builder = put(
        builder,
        "requires_reboot",
        Value::Bool(plan.requires_reboot()),
    );
    let builder = put(
        builder,
        "requires_offline",
        Value::Bool(plan.requires_offline()),
    );
    let builder = put(
        builder,
        "requires_acceptance",
        Value::Bool(plan.needs_destructive_acceptance()),
    );
    let builder = put(
        builder,
        "verification_contracts",
        Value::list(
            inner
                .verification()
                .contracts()
                .iter()
                .map(verification_contract_map),
        ),
    );
    Ok(builder.build())
}

/// The `ono.change-verification/1` record of §23.3.
///
/// `expression` is a parameter because [`VerificationResult`] carries the check's identity and its
/// subject and not the sentence the contract was written as, which §23.3 requires on the record so
/// a result can be argued with rather than believed. The caller that ran the check is holding the
/// [`VerificationContract`] it came from, and taking the expression from there keeps the two from
/// disagreeing.
///
/// # Errors
///
/// Returns `ono.provider_schema_violation` where the contract is not in this build.
pub fn verification_record(
    result: &VerificationResult,
    expression: &str,
) -> Result<RecordValue, ErrorValue> {
    let (schema, provenance) = target("ono.change-verification")?;
    let builder = RecordValue::builder(schema, provenance);
    let builder = put(builder, "plan_id", Value::string(result.plan().as_str()));
    let builder = put(builder, "check_id", Value::string(result.check().as_str()));
    let builder = put(builder, "class", Value::string(result.class().as_str()));
    let builder = put(builder, "subject", Value::string(result.subject()));
    let builder = put(builder, "expression", Value::string(expression));
    let builder = put(builder, "status", Value::string(result.status().as_str()));
    let builder = put(
        builder,
        "observed",
        result.observed().cloned().unwrap_or(Value::Null),
    );
    let builder = put(
        builder,
        "expected",
        result.expected().cloned().unwrap_or(Value::Null),
    );
    let builder = put(
        builder,
        "evidence",
        Value::list(result.evidence().iter().map(|text| Value::string(text))),
    );
    let builder = put(
        builder,
        "equivalence_domain",
        result
            .equivalence()
            .map_or(Value::Null, |domain| Value::string(domain.as_str())),
    );
    let builder = put(
        builder,
        "equivalence_state",
        result
            .equivalence_state()
            .map_or(Value::Null, |state| Value::string(state.as_str())),
    );
    let builder = put(builder, "detail", optional_text(result.detail()));
    let builder = put(builder, "timestamp", Value::Timestamp(result.at()));
    Ok(builder.build())
}

/// The `ono.impact-graph/1` record of §46.7.
///
/// The counts are [`ImpactGraph::blast_radius`]'s, so `boundary_count` cannot disagree with
/// `boundaries`: §9.6 requires a graph that stopped early to say so, and a summary computed
/// somewhere else is the summary that eventually stops saying it.
///
/// # Errors
///
/// Returns `ono.provider_schema_violation` where the contract is not in this build.
pub fn impact_record(plan: &PlanId, graph: &ImpactGraph) -> Result<RecordValue, ErrorValue> {
    let (schema, provenance) = target("ono.impact-graph")?;
    let radius = graph.blast_radius();
    let builder = RecordValue::builder(schema, provenance);
    let builder = put(builder, "plan_id", Value::string(plan.as_str()));
    let builder = put(
        builder,
        "nodes",
        Value::list(graph.nodes().iter().map(impact_node_map)),
    );
    let builder = put(
        builder,
        "boundaries",
        Value::list(graph.boundaries().iter().map(boundary_map)),
    );
    let builder = put(
        builder,
        "direct_targets",
        count_value(radius.direct_targets),
    );
    let builder = put(
        builder,
        "direct_effects",
        count_value(radius.direct_effects),
    );
    let builder = put(builder, "dependents", count_value(radius.dependents));
    let builder = put(builder, "transitive", count_value(radius.transitive));
    let builder = put(builder, "external", count_value(radius.external));
    let builder = put(builder, "boundary_count", count_value(radius.boundaries));
    let builder = put(builder, "hosts", count_value(radius.hosts));
    let builder = put(builder, "complete", Value::Bool(graph.is_complete()));
    let builder = put(
        builder,
        "truncated_reason",
        optional_text(graph.truncation()),
    );
    Ok(builder.build())
}

/// The `ono.protection-coverage/1` record of §10.3 — one row of the coverage matrix.
///
/// `satisfied` is Appendix A.5's rule applied to the row rather than a claim the row carries: a
/// `preserve-exact` objective is met only by a captured state image, because an inverse action
/// cannot promise the same bytes back (§27.4).
///
/// # Errors
///
/// Returns `ono.provider_schema_violation` where the contract is not in this build.
pub fn coverage_record(row: &DomainCoverage) -> Result<RecordValue, ErrorValue> {
    let (schema, provenance) = target("ono.protection-coverage")?;
    let builder = RecordValue::builder(schema, provenance);
    let builder = put(builder, "domain", Value::string(row.domain().as_str()));
    let builder = put(
        builder,
        "objective",
        Value::string(row.objective().as_str()),
    );
    let builder = put(
        builder,
        "protection",
        Value::string(row.protection().as_str()),
    );
    let builder = put(builder, "satisfied", Value::Bool(row.is_satisfied()));
    let builder = put(builder, "required", Value::Bool(row.is_required()));
    // Appendix A.7's escape travels rather than being inferred from `required`. A
    // `no-recovery-required` row is not required either way, so inferring the declaration from the
    // pair would lose it for exactly the domains where it changes nothing — and a plan whose
    // digest depends on it would then re-seal differently after a store round trip.
    let builder = put(
        builder,
        "declared_irrelevant",
        Value::Bool(row.is_declared_irrelevant()),
    );
    let builder = put(
        builder,
        "assets",
        Value::list(row.assets().iter().map(|id| Value::string(id.as_str()))),
    );
    let builder = put(
        builder,
        "consistency",
        row.consistency()
            .map_or(Value::Null, |class| Value::string(class.as_str())),
    );
    let builder = put(
        builder,
        "transaction_scope",
        optional_text(row.transaction_scope()),
    );
    let builder = put(
        builder,
        "exclusions",
        Value::list(row.exclusions().iter().map(coverage_exclusion_map)),
    );
    let builder = put(builder, "note", Value::string(row.note()));
    Ok(builder.build())
}

/// The `ono.persistence-domain/1` record of Appendix B.10.
///
/// A path Appendix B.1's pipeline could not resolve produces a row with a `refusal` rather than no
/// row at all: §32.4 makes "there is no protectable domain here, and this is why" an answer, and
/// omitting it would read as an absence.
///
/// # Errors
///
/// Returns `ono.provider_schema_violation` where the contract is not in this build.
pub fn domain_record(domain: &PersistenceDomain) -> Result<RecordValue, ErrorValue> {
    let (schema, provenance) = target("ono.persistence-domain")?;
    let mount = domain.mount();
    let builder = RecordValue::builder(schema, provenance);
    let builder = put(builder, "path", Value::string(domain.path()));
    let builder = put(builder, "mount_point", Value::string(mount.mount_point()));
    let builder = put(builder, "mount_id", Value::string(mount.mount_id()));
    let builder = put(builder, "filesystem", Value::string(mount.filesystem()));
    let builder = put(
        builder,
        "filesystem_kind",
        Value::string(mount.kind().as_str()),
    );
    let builder = put(builder, "source", Value::string(mount.source()));
    let builder = put(builder, "filesystem_root", Value::string(mount.root()));
    let builder = put(builder, "object", optional_text(domain.object()));
    let builder = put(builder, "object_kind", Value::string(domain.object_kind()));
    let builder = put(builder, "boundary", optional_text(domain.boundary()));
    let builder = put(builder, "read_only", Value::Bool(mount.is_read_only()));
    let builder = put(builder, "namespace", optional_text(mount.namespace()));
    let builder = put(builder, "protectable", Value::Bool(domain.is_protectable()));
    let builder = put(
        builder,
        "refusal",
        domain
            .refusal()
            .map_or(Value::Null, |reason| Value::string(reason.as_str())),
    );
    let builder = put(builder, "detail", Value::string(domain.detail()));
    Ok(builder.build())
}

/// The `ono.recovery-candidate/1` record of Appendix A.3.
///
/// `scope_width` and `restore_destructiveness` are on the record because Appendix A.4's preference
/// order is not "strongest snapshot wins": a small configuration backup dominates a root-dataset
/// rollback for one file, and a comparison that cannot see the recovery blast radius gets that
/// backwards.
///
/// # Errors
///
/// Returns `ono.provider_schema_violation` where the contract is not in this build.
pub fn candidate_record(candidate: &RecoveryCandidate) -> Result<RecordValue, ErrorValue> {
    let (schema, provenance) = target("ono.recovery-candidate")?;
    let builder = RecordValue::builder(schema, provenance);
    let builder = put(builder, "provider", Value::string(candidate.provider()));
    let builder = put(
        builder,
        "domain",
        Value::string(candidate.domain().as_str()),
    );
    let builder = put(
        builder,
        "objective",
        Value::string(candidate.objective().as_str()),
    );
    let builder = put(
        builder,
        "scope_object",
        Value::string(candidate.scope().domain()),
    );
    let builder = put(
        builder,
        "scope_kind",
        Value::string(candidate.scope().domain_kind()),
    );
    let builder = put(builder, "scope_width", count_value(candidate.scope_width()));
    let builder = put(
        builder,
        "consistency",
        Value::string(candidate.consistency().as_str()),
    );
    let builder = put(
        builder,
        "restore_method",
        Value::string(candidate.restore_method().as_str()),
    );
    let builder = put(
        builder,
        "restore_destructiveness",
        Value::Int(i128::from(candidate.restore_method().destructiveness())),
    );
    let builder = put(
        builder,
        "estimated_size",
        candidate
            .cost()
            .initial_bytes()
            .map_or(Value::Null, Value::ByteSize),
    );
    let builder = put(
        builder,
        "creation_requirements",
        Value::list(
            candidate
                .creation_requirements()
                .iter()
                .map(|text| Value::string(text)),
        ),
    );
    let builder = put(
        builder,
        "restore_requirements",
        Value::list(
            candidate
                .restore_requirements()
                .iter()
                .map(|text| Value::string(text)),
        ),
    );
    let builder = put(
        builder,
        "exclusions",
        Value::list(candidate.exclusions().iter().map(recovery_exclusion_map)),
    );
    let builder = put(builder, "detail", Value::string(candidate.detail()));
    Ok(builder.build())
}

// ---------------------------------------------------------------------------------------------
// Sub-records: the map shapes §46 declares as `record` without a contract file of their own
// ---------------------------------------------------------------------------------------------

/// One frozen target as the `targets` list of `ono.change-plan/1` carries it (§7.1).
///
/// Keys: `schema`, `identity`, `label`, `spatial_id`, `host`, `selector`, `persistence_domain`.
/// The last four are null where the target has none. `selector` travels because §4.3 keeps the
/// selector only so `explain` can show where the frozen set came from; §2.6 forbids re-running it.
#[must_use]
pub fn frozen_target_map(target: &FrozenTarget) -> Value {
    let mut map = MapValue::new();
    map.insert("schema".into(), Value::string(target.schema()));
    map.insert("identity".into(), Value::string(target.identity()));
    map.insert("label".into(), Value::string(target.label()));
    map.insert("spatial_id".into(), optional_text(target.spatial_id()));
    map.insert("host".into(), optional_text(target.host()));
    map.insert("selector".into(), optional_text(target.selector()));
    map.insert(
        "persistence_domain".into(),
        optional_text(target.persistence_domain()),
    );
    Value::Map(Arc::new(map))
}

/// One precondition as `ono.plan-action/1` and `ono.change-plan/1` both carry it (§7.2).
///
/// Keys: `kind`, `subject`, `field`, `expected` (the frozen value itself, any type), `material`,
/// `detail`. `material` is a field rather than an inference: §7.4 makes tolerance a contract, and
/// the safe reading is the one a provider gets by saying nothing.
#[must_use]
pub fn precondition_map(precondition: &Precondition) -> Value {
    let mut map = MapValue::new();
    map.insert("kind".into(), Value::string(precondition.kind().as_str()));
    map.insert("subject".into(), Value::string(precondition.subject()));
    map.insert("field".into(), Value::string(precondition.field()));
    map.insert("expected".into(), precondition.expected().clone());
    map.insert("material".into(), Value::Bool(precondition.is_material()));
    map.insert("detail".into(), Value::string(precondition.detail()));
    Value::Map(Arc::new(map))
}

/// One execution as the `execution` field of `ono.plan-action/1` carries it (§2.17, §12.3).
///
/// Keys: `method` (`provider-action`, `program`, `recovery-operation` or `opaque`), `provider`,
/// `operation`, `program`, `description`, `argv` (a list of strings) and `arguments` (a list of
/// `{name, value}` maps, in order). There is deliberately no key that could hold a command line:
/// §2.17 forbids an interpolated shell string, and a shape with nowhere to put one enforces that
/// without review.
#[must_use]
pub fn execution_map(execution: &Execution) -> Value {
    let mut map = MapValue::new();
    let arguments = |arguments: &[(Arc<str>, Value)]| {
        Value::list(arguments.iter().map(|(name, value)| {
            let mut pair = MapValue::new();
            pair.insert("name".into(), Value::string(name));
            pair.insert("value".into(), value.clone());
            Value::Map(Arc::new(pair))
        }))
    };
    let words = |argv: &[Arc<str>]| Value::list(argv.iter().map(|word| Value::string(word)));
    match execution {
        Execution::ProviderAction {
            provider,
            operation,
            arguments: declared,
        } => {
            map.insert("method".into(), Value::string("provider-action"));
            map.insert("provider".into(), Value::string(provider));
            map.insert("operation".into(), Value::string(operation));
            map.insert("program".into(), Value::Null);
            map.insert("description".into(), Value::Null);
            map.insert("argv".into(), Value::list([]));
            map.insert("arguments".into(), arguments(declared));
        }
        Execution::RecoveryOperation {
            provider,
            capability,
            arguments: declared,
        } => {
            map.insert("method".into(), Value::string("recovery-operation"));
            map.insert("provider".into(), Value::string(provider));
            map.insert("operation".into(), Value::string(capability));
            map.insert("program".into(), Value::Null);
            map.insert("description".into(), Value::Null);
            map.insert("argv".into(), Value::list([]));
            map.insert("arguments".into(), arguments(declared));
        }
        Execution::Program { program, argv } => {
            map.insert("method".into(), Value::string("program"));
            map.insert("provider".into(), Value::Null);
            map.insert("operation".into(), Value::Null);
            map.insert("program".into(), Value::string(program));
            map.insert("description".into(), Value::Null);
            map.insert("argv".into(), words(argv));
            map.insert("arguments".into(), Value::list([]));
        }
        Execution::Opaque {
            description,
            program,
            argv,
        } => {
            map.insert("method".into(), Value::string("opaque"));
            map.insert("provider".into(), Value::Null);
            map.insert("operation".into(), Value::Null);
            map.insert("program".into(), optional_text(program.as_deref()));
            map.insert("description".into(), Value::string(description));
            map.insert("argv".into(), words(argv));
            map.insert("arguments".into(), Value::list([]));
        }
    }
    Value::Map(Arc::new(map))
}

/// One risk finding as the `risk_findings` list of `ono.change-plan/1` carries it (§19.2).
///
/// Keys: `dimension`, `class`, `rule`, `reason`. `reason` travels because §40.2 prints it instead
/// of "Are you sure?", and §62.11 forbids generating one that no rule produced.
#[must_use]
pub fn risk_finding_map(finding: &RiskFinding) -> Value {
    let mut map = MapValue::new();
    map.insert(
        "dimension".into(),
        Value::string(finding.dimension().as_str()),
    );
    map.insert("class".into(), Value::string(finding.class().as_str()));
    map.insert("rule".into(), Value::string(finding.rule()));
    map.insert("reason".into(), Value::string(finding.reason()));
    Value::Map(Arc::new(map))
}

/// One coverage exclusion, as both `ono.change-plan/1` and `ono.protection-coverage/1` carry it.
///
/// Keys: `domain`, `subject`, `reason`, `irreversible`. §2.13 makes the last one a separate
/// question from coverage: an uncovered domain may still be restorable later, and an irreversible
/// one never will be.
#[must_use]
pub fn coverage_exclusion_map(exclusion: &CoverageExclusion) -> Value {
    let mut map = MapValue::new();
    map.insert("domain".into(), Value::string(exclusion.domain().as_str()));
    map.insert("subject".into(), Value::string(exclusion.subject()));
    map.insert("reason".into(), Value::string(exclusion.reason()));
    map.insert(
        "irreversible".into(),
        Value::Bool(exclusion.is_irreversible()),
    );
    Value::Map(Arc::new(map))
}

/// One asset exclusion as `ono.recovery-asset/1` and `ono.recovery-candidate/1` carry it (§11.1).
///
/// Keys: `subject`, `reason`.
#[must_use]
pub fn recovery_exclusion_map(exclusion: &RecoveryExclusion) -> Value {
    let mut map = MapValue::new();
    map.insert("subject".into(), Value::string(exclusion.subject()));
    map.insert("reason".into(), Value::string(exclusion.reason()));
    Value::Map(Arc::new(map))
}

/// One provider binding as the `provider_bindings` list of `ono.change-plan/1` carries it (§4.4).
///
/// Keys: `id`, `version`. The version is in the seal because a plan resolved against one version
/// of a provider is not the same plan against another (Appendix G.4).
#[must_use]
pub fn provider_binding_map(binding: &ProviderBinding) -> Value {
    let mut map = MapValue::new();
    map.insert("id".into(), Value::string(binding.id()));
    map.insert("version".into(), Value::string(binding.version()));
    Value::Map(Arc::new(map))
}

/// One verification contract as `ono.change-plan/1` and `ono.recovery-plan/1` carry it (§23.1).
///
/// Keys: `id`, `class`, `subject`, `expression`, `expected`, `timeout`, `timeout_status`,
/// `equivalence_domain`. `timeout_status` says what a timeout produces, because §23.5 forbids
/// waiting forever and §2.4 forbids reading the resulting `unknown` as a pass.
#[must_use]
pub fn verification_contract_map(contract: &VerificationContract) -> Value {
    let mut map = MapValue::new();
    map.insert("id".into(), Value::string(contract.id().as_str()));
    map.insert("class".into(), Value::string(contract.class().as_str()));
    map.insert("subject".into(), Value::string(contract.subject()));
    map.insert("expression".into(), Value::string(contract.expression()));
    map.insert(
        "expected".into(),
        contract.expected().cloned().unwrap_or(Value::Null),
    );
    map.insert("timeout".into(), duration_value(contract.timeout()));
    map.insert(
        "timeout_status".into(),
        Value::string(contract.timeout_status().as_str()),
    );
    map.insert(
        "equivalence_domain".into(),
        contract
            .equivalence()
            .map_or(Value::Null, |domain| Value::string(domain.as_str())),
    );
    Value::Map(Arc::new(map))
}

/// The `scope` record of `ono.recovery-asset/1` (§11.2).
///
/// Keys: `domain` (the resolved persistence object), `domain_kind`, `covers` (a list of the
/// objects the scope actually holds) and `host`. Membership is exact: §13.4 makes a path prefix
/// no evidence of coverage at all.
#[must_use]
pub fn recovery_scope_map(scope: &RecoveryScope) -> Value {
    let mut map = MapValue::new();
    map.insert("domain".into(), Value::string(scope.domain()));
    map.insert("domain_kind".into(), Value::string(scope.domain_kind()));
    map.insert(
        "covers".into(),
        Value::list(scope.covers().iter().map(|object| Value::string(object))),
    );
    map.insert("host".into(), Value::string(scope.host()));
    Value::Map(Arc::new(map))
}

/// The `validation` record of `ono.recovery-asset/1` (§11.4).
///
/// Keys: `exists`, `identity_matches`, `scope_matches`, `restore_available`,
/// `permissions_present`, `at`, `detail`. Each check travels separately because §11.4's checks are
/// not advisory: an asset whose scope does not match what the plan expected is `invalid`, and the
/// coverage algorithm then finds no validated path for that domain.
#[must_use]
pub fn validation_map(validation: &RecoveryValidation) -> Value {
    let mut map = MapValue::new();
    let failures = validation.failures();
    let passed = |name: &str| !failures.contains(&name);
    map.insert(
        "exists".into(),
        Value::Bool(passed("the asset does not exist")),
    );
    map.insert(
        "identity_matches".into(),
        Value::Bool(passed(
            "the asset's identity does not match the planned source",
        )),
    );
    map.insert(
        "scope_matches".into(),
        Value::Bool(passed(
            "the asset's scope does not match the expected target",
        )),
    );
    map.insert(
        "restore_available".into(),
        Value::Bool(passed("no restore path is available")),
    );
    map.insert(
        "permissions_present".into(),
        Value::Bool(passed("the permissions recovery needs are not held")),
    );
    map.insert("at".into(), Value::Timestamp(validation.at()));
    map.insert("detail".into(), Value::string(validation.detail()));
    Value::Map(Arc::new(map))
}

/// One newer-state item as the `newer_state` list of `ono.recovery-plan/1` carries it (C.3).
///
/// Keys: `object`, `class`, `changed_at`, `detail`. `class: unknown` is a first-class answer:
/// §56.3 makes "whether the method touches this could not be established" a reason to block.
#[must_use]
pub fn newer_state_map(item: &NewerStateItem) -> Value {
    let mut map = MapValue::new();
    map.insert("object".into(), Value::string(item.object()));
    map.insert("class".into(), Value::string(item.class().as_str()));
    map.insert(
        "changed_at".into(),
        item.changed_instant().map_or(Value::Null, Value::Timestamp),
    );
    map.insert("detail".into(), Value::string(item.detail()));
    Value::Map(Arc::new(map))
}

/// One unrecoverable effect as `ono.recovery-plan/1` carries it (§24.3, §35.2).
///
/// Keys: `subject`, `domain`, `reason`, `compensation`. A named compensation does not remove the
/// effect from the list: §35.3 keeps compensation and rollback apart, and a restored filesystem
/// still has not un-sent the webhook.
#[must_use]
pub fn unrecoverable_effect_map(effect: &UnrecoverableEffect) -> Value {
    let mut map = MapValue::new();
    map.insert("subject".into(), Value::string(effect.subject()));
    map.insert("domain".into(), Value::string(effect.domain().as_str()));
    map.insert("reason".into(), Value::string(effect.reason()));
    map.insert("compensation".into(), optional_text(effect.compensation()));
    Value::Map(Arc::new(map))
}

/// One impact node as the `nodes` list of `ono.impact-graph/1` carries it (§9.2, §3.5).
///
/// Keys: `id`, `label`, `object_type`, `class`, `depth`, `relation`, `confidence`, `evidence`,
/// `host`. The relation and its confidence are the v0.4 edge's, unchanged: §3.5 requires impact to
/// retain provenance, and strengthening an `inferred` edge on the way through here is exactly the
/// invention §1.3 forbids.
#[must_use]
pub fn impact_node_map(node: &ImpactNode) -> Value {
    let mut map = MapValue::new();
    map.insert("id".into(), Value::string(node.id()));
    map.insert("label".into(), Value::string(node.label()));
    map.insert("object_type".into(), Value::string(node.object_type()));
    map.insert("class".into(), Value::string(node.class().as_str()));
    map.insert("depth".into(), count_value(node.depth()));
    map.insert("relation".into(), optional_text(node.relation()));
    map.insert("confidence".into(), Value::string(node.confidence()));
    map.insert(
        "evidence".into(),
        Value::list(node.evidence().iter().map(|text| Value::string(text))),
    );
    map.insert("host".into(), optional_text(node.host()));
    Value::Map(Arc::new(map))
}

/// One opaque boundary as the `boundaries` list of `ono.impact-graph/1` carries it (§9.6).
///
/// Keys: `at`, `beyond`, `reason`. A graph that simply stops has said nothing about whether it
/// stopped because there was nothing more or because Ono could not see further.
#[must_use]
pub fn boundary_map(boundary: &UnknownBoundary) -> Value {
    let mut map = MapValue::new();
    map.insert("at".into(), Value::string(boundary.at()));
    map.insert("beyond".into(), Value::string(boundary.beyond()));
    map.insert("reason".into(), Value::string(boundary.reason()));
    Value::Map(Arc::new(map))
}

/// The `impact_summary` record of `ono.change-plan/1` — §9.5's blast radius.
///
/// Keys: `direct_targets`, `direct_effects`, `dependents`, `transitive`, `external`,
/// `boundary_count`, `hosts`, `complete`, `truncated_reason`, and the four label lists
/// `direct_labels`, `dependent_labels`, `possible_labels` and `boundary_labels`.
///
/// The key is `boundary_count` and not `boundaries`, so it is spelled the same here and on
/// `ono.impact-graph/1`: a renderer that had to know two names for one number would eventually
/// read the wrong one, and §9.6's whole point is that the boundary count is never lost.
///
/// The label lists are what §20.2's plan view names. `impact` returns the full
/// `ono.impact-graph/1`; a plan carries the summary, and a summary that could only count would
/// force the default view to print `2 direct dependents` where §20.2 prints `4 worker processes,
/// :80, :443`. They are bounded — a plan over five thousand targets is a plan whose labels are a
/// count — and the bound is stated in `truncated_reason` when it bites.
#[must_use]
pub fn impact_summary_map(graph: &ImpactGraph) -> Value {
    /// How many labels one summary row carries before it becomes a count.
    const LABEL_BUDGET: usize = 12;

    let radius = graph.blast_radius();
    let labels = |class: crate::ImpactClass| {
        Value::list(
            graph
                .of_class(class)
                .into_iter()
                .take(LABEL_BUDGET)
                .map(|node| Value::string(node.label())),
        )
    };
    let mut map = MapValue::new();
    map.insert("direct_targets".into(), count_value(radius.direct_targets));
    map.insert("direct_effects".into(), count_value(radius.direct_effects));
    map.insert("dependents".into(), count_value(radius.dependents));
    map.insert("transitive".into(), count_value(radius.transitive));
    map.insert("external".into(), count_value(radius.external));
    map.insert("boundary_count".into(), count_value(radius.boundaries));
    map.insert("hosts".into(), count_value(radius.hosts));
    map.insert("complete".into(), Value::Bool(graph.is_complete()));
    map.insert("truncated_reason".into(), optional_text(graph.truncation()));
    map.insert(
        "direct_labels".into(),
        labels(crate::ImpactClass::DirectTarget),
    );
    map.insert(
        "dependent_labels".into(),
        labels(crate::ImpactClass::Dependent),
    );
    map.insert(
        "possible_labels".into(),
        labels(crate::ImpactClass::TransitiveRelated),
    );
    map.insert(
        "boundary_labels".into(),
        Value::list(
            graph
                .boundaries()
                .iter()
                .take(LABEL_BUDGET)
                .map(|boundary| Value::string(boundary.beyond())),
        ),
    );
    Value::Map(Arc::new(map))
}

// ---------------------------------------------------------------------------------------------
// Decoding
// ---------------------------------------------------------------------------------------------

/// Reads a plan back out of its `ono.change-plan/1` record (§36.1, §41.2).
///
/// Everything the seal covers comes back: the lifecycle state, the revision, the digest, the
/// frozen targets, the action graph with each action's status and preconditions, the protection
/// matrix, the risk findings and the acknowledgements, the verification contracts and the provider
/// bindings. A plan read back therefore equals the plan written, and [`ChangePlan::digest_holds`]
/// still answers `true` for a sealed one — which is what §63.2 asks to be verifiable.
///
/// Two fields are recomputed rather than read, because §46.1 does not carry them:
///
/// - the impact graph, which is `ono.impact-graph/1` and a record of its own. A plan comes back
///   with an empty graph; a caller that stored the graph reattaches it with
///   [`ChangePlan::with_impact`] and [`impact_from_record`].
/// - `supersedes`, which [`ChangePlan::revise`] always sets to the previous revision, so the
///   stored revision determines it.
///
/// # Errors
///
/// Returns `ono.plan_store_corrupt` naming the field where a required field is missing, an
/// identity is not an identity, or an enum spelling is not in the vocabulary — §2.4 forbids
/// reading an unknown word as a default.
pub fn plan_from_record(record: &RecordValue) -> Result<ChangePlan, ErrorValue> {
    let id = plan_id(record.get("id"), "id")?;
    let revision = counter(record.get("revision"), "revision")?;
    let kind = enumeration(record.get("kind"), "kind", PlanKind::from_name)?;
    let state = enumeration(record.get("state"), "state", PlanState::from_name)?;
    let intent = Intent::new(
        text(record.get("intent"), "intent")?,
        text(record.get("source"), "source")?,
    );
    let session = text(record.get("session"), "session")?;
    let created_at = instant(record.get("created_at"), "created_at")?;
    let sealed_at = optional_instant(record.get("sealed_at"), "sealed_at")?;
    let expires_at = optional_instant(record.get("expires_at"), "expires_at")?;

    let mut targets = Vec::new();
    for item in list_field(record.get("targets"), "targets")? {
        targets.push(frozen_target_from(item)?);
    }
    let mut actions = Vec::new();
    for item in list_field(record.get("actions"), "actions")? {
        actions.push(action_from_record(nested_record(item, "actions")?)?);
    }
    let mut rows = Vec::new();
    for item in list_field(record.get("protection"), "protection")? {
        rows.push(coverage_from_record(nested_record(item, "protection")?)?);
    }
    let covered: usize = rows.iter().map(|row| row.exclusions().len()).sum();
    let mut protection = ProtectionSummary::of(rows);
    // The rows' own exclusions lead the plan's list, in row order, so what follows them is what
    // belongs to the plan rather than to any single domain (§10.3).
    for item in list_field(record.get("coverage_exclusions"), "coverage_exclusions")?
        .iter()
        .skip(covered)
    {
        protection = protection.excluding(coverage_exclusion_from(item)?);
    }
    let protection_mode = enumeration(
        record.get("protection_mode"),
        "protection_mode",
        ProtectionMode::from_name,
    )?;

    let mut findings = Vec::new();
    for item in list_field(record.get("risk_findings"), "risk_findings")? {
        findings.push(risk_finding_from(item)?);
    }
    let mut risk = RiskAssessment::of(findings);
    for flag in text_list(
        record.get("accepted_risk_overrides"),
        "accepted_risk_overrides",
    )? {
        risk = match flag.as_ref() {
            ACCEPT_RISK => risk.risk_accepted(),
            ACCEPT_IRREVERSIBLE => risk.irreversible_accepted(),
            other => {
                return Err(malformed(
                    "accepted_risk_overrides",
                    &format!("carries `{other}`, which is not an acknowledgement §40.3 spells"),
                ));
            }
        };
    }

    let strategy = strategy_from_text(&text(record.get("strategy"), "strategy")?)?;
    let mut contracts = Vec::new();
    for item in list_field(
        record.get("verification_contracts"),
        "verification_contracts",
    )? {
        contracts.push(verification_contract_from(item, &id)?);
    }
    let mut providers = Vec::new();
    for item in list_field(record.get("provider_bindings"), "provider_bindings")? {
        let map = nested_map(item, "provider_bindings")?;
        providers.push(ProviderBinding::new(
            text(map.get("id"), "provider_bindings.id")?,
            text(map.get("version"), "provider_bindings.version")?,
        ));
    }

    Ok(ChangePlan::restore(
        id,
        revision,
        kind,
        state,
        intent,
        session,
        created_at,
        sealed_at,
        expires_at,
        targets,
        actions,
        ImpactGraph::empty(),
        protection,
        protection_mode,
        risk,
        strategy,
        VerificationSet::of(contracts),
        providers,
        optional_text_of(record.get("digest")),
        revision.checked_sub(1).filter(|earlier| *earlier > 0),
    ))
}

/// Reads a recovery plan back out of its `ono.recovery-plan/1` record and the plan it is (§3.8).
///
/// §46.5's record describes the recovery — the goal, the method, what would come back, what would
/// be destroyed and what cannot be reversed — and says nothing about the intent, the session or
/// the lifecycle of the [`ChangePlan`] it wraps, because those live in `ono.change-plan/1`. So the
/// plan is a parameter: §36.1's store writes both records and reads both back, and inventing an
/// intent for a recovery would be exactly the fabrication §1.3 forbids.
///
/// # Errors
///
/// Returns `ono.plan_store_corrupt` naming the field where a required field is missing or an enum
/// spelling is not in the vocabulary.
pub fn recovery_plan_from_record(
    record: &RecordValue,
    plan: ChangePlan,
) -> Result<RecoveryPlan, ErrorValue> {
    let source_plan = match optional_text_of(record.get("source_plan")) {
        None => None,
        Some(text) => Some(parse_plan_id(&text, "source_plan")?),
    };
    let mut source_assets = Vec::new();
    for id in text_list(record.get("source_assets"), "source_assets")? {
        source_assets.push(parse_asset_id(&id, "source_assets")?);
    }
    let goal = enumeration(record.get("goal"), "goal", RecoveryGoal::from_name)?;
    let method = enumeration(record.get("method"), "method", RestoreMethod::from_name)?;
    let target_state = text(record.get("target_state"), "target_state")?;
    let restores = text_list(record.get("restores"), "restores")?;

    let mut items = Vec::new();
    for item in list_field(record.get("newer_state"), "newer_state")? {
        items.push(newer_state_from(item)?);
    }
    let newer = NewerStateImpact::restore(
        items,
        text_list(record.get("destroyed_assets"), "destroyed_assets")?,
        byte_size(record.get("discarded_size"), "discarded_size")?,
        flag(record.get("newer_state_analysed"), "newer_state_analysed")?,
    );

    let mut unrecoverable = Vec::new();
    for item in list_field(record.get("unrecoverable_effects"), "unrecoverable_effects")? {
        unrecoverable.push(unrecoverable_effect_from(item)?);
    }
    let metadata = metadata_from_names(&text_list(
        record.get("metadata_restored"),
        "metadata_restored",
    )?)?;
    let directory_policy = enumeration(
        record.get("directory_policy"),
        "directory_policy",
        DirectoryRestorePolicy::from_name,
    )?;
    // §24.5's gate is outstanding exactly when the analysis demands acceptance and the operator
    // has not given it, so the flag the record carries determines the acceptance behind it.
    let outstanding = flag(record.get("requires_acceptance"), "requires_acceptance")?;
    let accepted = newer.requires_destructive_acceptance() && !outstanding;

    Ok(RecoveryPlan::restore(
        plan,
        source_plan,
        source_assets,
        goal,
        method,
        target_state,
        restores,
        newer,
        unrecoverable,
        metadata,
        directory_policy,
        flag(record.get("requires_reboot"), "requires_reboot")?,
        flag(record.get("requires_offline"), "requires_offline")?,
        accepted,
    ))
}

/// Reads a recovery asset back out of its `ono.recovery-asset/1` record (§11.1, §37.5).
///
/// The identity travels rather than being re-derived: an asset's id is what a plan references and
/// what `remove recovery` names, so re-deriving it on read would break every reference that had
/// already been printed. The state travels for the same reason §11.4 exists — `ready` is a claim
/// only a validation that passed may make, and recomputing it here would make it a guess.
///
/// # Errors
///
/// Returns `ono.plan_store_corrupt` naming the field where a required field is missing, an
/// identity is not an identity, or an enum spelling is not in the vocabulary.
pub fn asset_from_record(record: &RecordValue) -> Result<RecoveryAsset, ErrorValue> {
    let id = parse_asset_id(&text(record.get("id"), "id")?, "id")?;
    let source_plan = match optional_text_of(record.get("source_plan")) {
        None => None,
        Some(text) => Some(parse_plan_id(&text, "source_plan")?),
    };
    let scope = recovery_scope_from(record.get("scope"), "scope")?;
    let validation = match optional_map(record.get("validation"), "validation")? {
        None => None,
        Some(map) => Some(validation_from(map)?),
    };

    let mut retention = RetentionPolicy::of(span(record.get("retention"), "retention")?);
    if flag(record.get("held"), "held")? {
        retention = retention.holding();
    }
    let mut cost = RecoveryCost::unknown().with_space(
        byte_size(record.get("initial_size"), "initial_size")?,
        byte_size(record.get("retained_size"), "retained_size")?,
        flag(record.get("size_estimated"), "size_estimated")?,
    );
    if let Some(latency) = optional_span(record.get("creation_latency"), "creation_latency")? {
        cost = cost.with_latency(latency);
    }
    if let Some(overhead) = optional_percent(record.get("io_overhead"), "io_overhead")? {
        cost = cost.with_io_overhead(overhead);
    }
    if let Some(quiesce) = optional_span(record.get("quiesce_duration"), "quiesce_duration")? {
        cost = cost.with_quiesce(quiesce);
    }
    if let Some(latency) = optional_span(record.get("cleanup_latency"), "cleanup_latency")? {
        cost = cost.with_cleanup_latency(latency);
    }
    if flag(record.get("requires_reboot"), "requires_reboot")? {
        cost = cost.needing_reboot();
    }
    if flag(record.get("requires_offline"), "requires_offline")? {
        cost = cost.needing_offline();
    }

    let mut dependencies = Vec::new();
    for dependency in text_list(record.get("dependencies"), "dependencies")? {
        dependencies.push(parse_asset_id(&dependency, "dependencies")?);
    }
    let mut exclusions = Vec::new();
    for item in list_field(record.get("exclusions"), "exclusions")? {
        let map = nested_map(item, "exclusions")?;
        exclusions.push(RecoveryExclusion::new(
            text(map.get("subject"), "exclusions.subject")?,
            text(map.get("reason"), "exclusions.reason")?,
        ));
    }

    Ok(RecoveryAsset::restore(
        id,
        text(record.get("provider"), "provider")?,
        enumeration(record.get("type"), "type", RecoveryAssetType::from_name)?,
        text(record.get("reference"), "reference")?,
        scope,
        instant(record.get("created_at"), "created_at")?,
        source_plan,
        enumeration(record.get("state"), "state", AssetState::from_name)?,
        enumeration(
            record.get("consistency"),
            "consistency",
            ConsistencyClass::from_name,
        )?,
        enumeration(
            record.get("restore_method"),
            "restore_method",
            RestoreMethod::from_name,
        )?,
        validation,
        retention,
        cost,
        dependencies,
        exclusions,
        optional_text_of(record.get("captured_state")),
        optional_instant(record.get("expires_at"), "expires_at")?,
    ))
}

/// Reads one action back out of its `ono.plan-action/1` record (§46.2).
///
/// # Errors
///
/// Returns `ono.plan_store_corrupt` naming the field where a required field is missing or an enum
/// spelling is not in the vocabulary.
pub fn action_from_record(record: &RecordValue) -> Result<PlanAction, ErrorValue> {
    let id = ActionId::parse(&text(record.get("id"), "id")?)
        .ok_or_else(|| malformed("id", "is not an action identity"))?;
    let mut depends_on = Vec::new();
    for dependency in text_list(record.get("depends_on"), "depends_on")? {
        depends_on.push(ActionId::parse(&dependency).ok_or_else(|| {
            malformed(
                "depends_on",
                "holds something that is not an action identity",
            )
        })?);
    }
    let mut preconditions = Vec::new();
    for item in list_field(record.get("preconditions"), "preconditions")? {
        preconditions.push(precondition_from(item)?);
    }
    let mut effects = Vec::new();
    for item in list_field(record.get("proposed_effects"), "proposed_effects")? {
        effects.push(effect_from_record(nested_record(
            item,
            "proposed_effects",
        )?)?);
    }

    Ok(PlanAction::restore(
        id,
        size(record.get("ordinal"), "ordinal")?,
        enumeration(record.get("role"), "role", ActionRole::from_name)?,
        text(record.get("summary"), "summary")?,
        optional_text_of(record.get("target")),
        execution_from(record.get("execution"), "execution")?,
        depends_on,
        preconditions,
        enumeration(
            record.get("idempotency"),
            "idempotency",
            Idempotency::from_name,
        )?,
        effects,
        optional_text_of(record.get("recovery_semantics")),
        flag(record.get("requires_privilege"), "requires_privilege")?,
        enumeration(record.get("status"), "status", ActionStatus::from_name)?,
    ))
}

/// Reads one proposed effect back out of its `ono.proposed-effect/1` record (§8.2).
///
/// # Errors
///
/// Returns `ono.plan_store_corrupt` naming the field where a required field is missing or an enum
/// spelling is not in the vocabulary.
pub fn effect_from_record(record: &RecordValue) -> Result<ProposedEffect, ErrorValue> {
    let id = EffectId::parse(&text(record.get("id"), "id")?)
        .ok_or_else(|| malformed("id", "is not an effect identity"))?;
    let action = ActionId::parse(&text(record.get("action_id"), "action_id")?)
        .ok_or_else(|| malformed("action_id", "is not an action identity"))?;
    Ok(ProposedEffect::restore(
        id,
        action,
        optional_text_of(record.get("object")),
        enumeration(record.get("domain"), "domain", EffectDomain::from_name)?,
        enumeration(record.get("kind"), "kind", EffectKind::from_name)?,
        enumeration(
            record.get("confidence"),
            "confidence",
            EffectConfidence::from_name,
        )?,
        held_value(record.get("before")),
        held_value(record.get("proposed")),
        text_list(record.get("evidence"), "evidence")?,
        text(record.get("explanation"), "explanation")?,
        flag(record.get("irreversible"), "irreversible")?,
        optional_text_of(record.get("compensation")),
    ))
}

/// Reads one coverage row back out of its `ono.protection-coverage/1` record (§10.3).
///
/// `satisfied` is not read: Appendix A.5 computes it from the objective and the protection, and a
/// row that could carry a `satisfied` disagreeing with them is §62.1's snapshot theatre.
///
/// # Errors
///
/// Returns `ono.plan_store_corrupt` naming the field where a required field is missing or an enum
/// spelling is not in the vocabulary.
pub fn coverage_from_record(record: &RecordValue) -> Result<DomainCoverage, ErrorValue> {
    let domain = enumeration(record.get("domain"), "domain", EffectDomain::from_name)?;
    let objective = enumeration(
        record.get("objective"),
        "objective",
        RecoveryObjective::from_name,
    )?;
    let protection = enumeration(
        record.get("protection"),
        "protection",
        DomainProtection::from_name,
    )?;
    let mut row = DomainCoverage::new(
        domain,
        objective,
        protection,
        text(record.get("note"), "note")?,
    );
    for asset in text_list(record.get("assets"), "assets")? {
        row = row.by_asset(parse_asset_id(&asset, "assets")?);
    }
    if let Some(consistency) = optional_enumeration(
        record.get("consistency"),
        "consistency",
        ConsistencyClass::from_name,
    )? {
        row = row.at_consistency(consistency);
    }
    if let Some(scope) = optional_text_of(record.get("transaction_scope")) {
        row = row.within_transaction(scope);
    }
    for item in list_field(record.get("exclusions"), "exclusions")? {
        row = row.excluding(coverage_exclusion_from(item)?);
    }
    if flag(record.get("declared_irrelevant"), "declared_irrelevant")? {
        row = row.declared_irrelevant();
    }
    Ok(row)
}

/// Reads an impact graph back out of its `ono.impact-graph/1` record (§9).
///
/// The counts are not read: [`ImpactGraph::blast_radius`] derives them from the nodes, and §9.5's
/// summary is only honest while it cannot disagree with them.
///
/// # Errors
///
/// Returns `ono.plan_store_corrupt` naming the field where a required field is missing or an enum
/// spelling is not in the vocabulary.
pub fn impact_from_record(record: &RecordValue) -> Result<ImpactGraph, ErrorValue> {
    let mut graph = ImpactGraph::empty();
    for item in list_field(record.get("nodes"), "nodes")? {
        graph.add(impact_node_from(item)?);
    }
    for item in list_field(record.get("boundaries"), "boundaries")? {
        let map = nested_map(item, "boundaries")?;
        graph.add_boundary(UnknownBoundary::new(
            text(map.get("at"), "boundaries.at")?,
            text(map.get("beyond"), "boundaries.beyond")?,
            text(map.get("reason"), "boundaries.reason")?,
        ));
    }
    if let Some(reason) = optional_text_of(record.get("truncated_reason")) {
        graph = graph.truncated(reason);
    }
    Ok(graph)
}

// ---------------------------------------------------------------------------------------------
// Sub-record readers
// ---------------------------------------------------------------------------------------------

/// One frozen target, out of the map [`frozen_target_map`] wrote.
fn frozen_target_from(item: &Value) -> Result<FrozenTarget, ErrorValue> {
    let map = nested_map(item, "targets")?;
    let mut frozen = FrozenTarget::new(
        text(map.get("schema"), "targets.schema")?,
        text(map.get("identity"), "targets.identity")?,
        text(map.get("label"), "targets.label")?,
    );
    if let Some(spatial) = optional_text_of(map.get("spatial_id")) {
        frozen = frozen.at_place(spatial);
    }
    if let Some(host) = optional_text_of(map.get("host")) {
        frozen = frozen.on_host(host);
    }
    if let Some(selector) = optional_text_of(map.get("selector")) {
        frozen = frozen.resolved_from(selector);
    }
    if let Some(domain) = optional_text_of(map.get("persistence_domain")) {
        frozen = frozen.in_domain(domain);
    }
    Ok(frozen)
}

/// One precondition, out of the map [`precondition_map`] wrote.
fn precondition_from(item: &Value) -> Result<Precondition, ErrorValue> {
    let map = nested_map(item, "preconditions")?;
    let mut precondition = Precondition::new(
        enumeration(
            map.get("kind"),
            "preconditions.kind",
            PreconditionKind::from_name,
        )?,
        text(map.get("subject"), "preconditions.subject")?,
        text(map.get("field"), "preconditions.field")?,
        map.get("expected").cloned().unwrap_or(Value::Null),
    )
    .explained(text(map.get("detail"), "preconditions.detail")?);
    if !flag(map.get("material"), "preconditions.material")? {
        precondition = precondition.tolerant();
    }
    Ok(precondition)
}

/// One execution, out of the map [`execution_map`] wrote.
fn execution_from(source: Option<&Value>, field: &str) -> Result<Execution, ErrorValue> {
    let map = map_field(source, field)?;
    let method = text(map.get("method"), "execution.method")?;
    let execution = match method.as_ref() {
        "provider-action" => Execution::ProviderAction {
            provider: text(map.get("provider"), "execution.provider")?,
            operation: text(map.get("operation"), "execution.operation")?,
            arguments: arguments_from(map.get("arguments"))?,
        },
        "recovery-operation" => Execution::RecoveryOperation {
            provider: text(map.get("provider"), "execution.provider")?,
            capability: text(map.get("operation"), "execution.operation")?,
            arguments: arguments_from(map.get("arguments"))?,
        },
        "program" => Execution::Program {
            program: text(map.get("program"), "execution.program")?,
            argv: text_list(map.get("argv"), "execution.argv")?,
        },
        "opaque" => Execution::Opaque {
            description: text(map.get("description"), "execution.description")?,
            program: optional_text_of(map.get("program")),
            argv: text_list(map.get("argv"), "execution.argv")?,
        },
        other => {
            return Err(malformed(
                "execution.method",
                &format!("holds `{other}`, which is not one of §2.17's four execution methods"),
            ));
        }
    };
    Ok(execution)
}

/// The typed argument vector of a provider or recovery operation, in the order it was written.
fn arguments_from(source: Option<&Value>) -> Result<Vec<(Arc<str>, Value)>, ErrorValue> {
    let mut arguments = Vec::new();
    for item in list_field(source, "execution.arguments")? {
        let pair = nested_map(item, "execution.arguments")?;
        arguments.push((
            text(pair.get("name"), "execution.arguments.name")?,
            pair.get("value").cloned().unwrap_or(Value::Null),
        ));
    }
    Ok(arguments)
}

/// One risk finding, out of the map [`risk_finding_map`] wrote.
fn risk_finding_from(item: &Value) -> Result<RiskFinding, ErrorValue> {
    let map = nested_map(item, "risk_findings")?;
    Ok(RiskFinding::new(
        enumeration(
            map.get("dimension"),
            "risk_findings.dimension",
            RiskDimension::from_name,
        )?,
        enumeration(
            map.get("class"),
            "risk_findings.class",
            RiskClass::from_name,
        )?,
        text(map.get("rule"), "risk_findings.rule")?,
        text(map.get("reason"), "risk_findings.reason")?,
    ))
}

/// One coverage exclusion, out of the map [`coverage_exclusion_map`] wrote.
fn coverage_exclusion_from(item: &Value) -> Result<CoverageExclusion, ErrorValue> {
    let map = nested_map(item, "coverage_exclusions")?;
    let exclusion = CoverageExclusion::new(
        enumeration(
            map.get("domain"),
            "coverage_exclusions.domain",
            EffectDomain::from_name,
        )?,
        text(map.get("subject"), "coverage_exclusions.subject")?,
        text(map.get("reason"), "coverage_exclusions.reason")?,
    );
    if flag(map.get("irreversible"), "coverage_exclusions.irreversible")? {
        return Ok(exclusion.irreversible());
    }
    Ok(exclusion)
}

/// One newer-state item, out of the map [`newer_state_map`] wrote.
fn newer_state_from(item: &Value) -> Result<NewerStateItem, ErrorValue> {
    let map = nested_map(item, "newer_state")?;
    let entry = NewerStateItem::new(
        text(map.get("object"), "newer_state.object")?,
        enumeration(
            map.get("class"),
            "newer_state.class",
            NewerStateClass::from_name,
        )?,
        text(map.get("detail"), "newer_state.detail")?,
    );
    match optional_instant(map.get("changed_at"), "newer_state.changed_at")? {
        None => Ok(entry),
        Some(at) => Ok(entry.changed_at(at)),
    }
}

/// One unrecoverable effect, out of the map [`unrecoverable_effect_map`] wrote.
fn unrecoverable_effect_from(item: &Value) -> Result<UnrecoverableEffect, ErrorValue> {
    let map = nested_map(item, "unrecoverable_effects")?;
    let effect = UnrecoverableEffect::new(
        text(map.get("subject"), "unrecoverable_effects.subject")?,
        enumeration(
            map.get("domain"),
            "unrecoverable_effects.domain",
            EffectDomain::from_name,
        )?,
        text(map.get("reason"), "unrecoverable_effects.reason")?,
    );
    match optional_text_of(map.get("compensation")) {
        None => Ok(effect),
        Some(action) => Ok(effect.compensated_by(action)),
    }
}

/// One validation, out of the map [`validation_map`] wrote.
fn validation_from(map: &MapValue) -> Result<RecoveryValidation, ErrorValue> {
    Ok(RecoveryValidation::none(
        instant(map.get("at"), "validation.at")?,
        text(map.get("detail"), "validation.detail")?,
    )
    .existing(flag(map.get("exists"), "validation.exists")?)
    .identity(flag(
        map.get("identity_matches"),
        "validation.identity_matches",
    )?)
    .scope(flag(map.get("scope_matches"), "validation.scope_matches")?)
    .restore(flag(
        map.get("restore_available"),
        "validation.restore_available",
    )?)
    .permissions(flag(
        map.get("permissions_present"),
        "validation.permissions_present",
    )?))
}

/// One recovery scope, out of the map [`recovery_scope_map`] wrote.
fn recovery_scope_from(source: Option<&Value>, field: &str) -> Result<RecoveryScope, ErrorValue> {
    let map = map_field(source, field)?;
    let mut scope = RecoveryScope::new(
        text(map.get("domain_kind"), "scope.domain_kind")?,
        text(map.get("domain"), "scope.domain")?,
        text(map.get("host"), "scope.host")?,
    );
    for object in text_list(map.get("covers"), "scope.covers")? {
        scope = scope.covering(object);
    }
    Ok(scope)
}

/// One verification contract, out of the map [`verification_contract_map`] wrote.
///
/// The check's identity is derived from the plan, the subject and the expression exactly as
/// [`VerificationContract::new`] derives it, so the stored `id` is a reader's convenience rather
/// than a second source of truth.
fn verification_contract_from(
    item: &Value,
    plan: &PlanId,
) -> Result<VerificationContract, ErrorValue> {
    let map = nested_map(item, "verification_contracts")?;
    let class = enumeration(
        map.get("class"),
        "verification_contracts.class",
        VerificationClass::from_name,
    )?;
    let mut contract = VerificationContract::new(
        plan,
        class,
        text(map.get("subject"), "verification_contracts.subject")?,
        text(map.get("expression"), "verification_contracts.expression")?,
    )
    .within(span(map.get("timeout"), "verification_contracts.timeout")?);
    if let Some(expected) = held_value(map.get("expected")) {
        contract = contract.expecting(expected);
    }
    if let Some(domain) = optional_enumeration(
        map.get("equivalence_domain"),
        "verification_contracts.equivalence_domain",
        EquivalenceDomain::from_name,
    )? {
        contract = contract.about(domain);
    }
    let on_timeout = enumeration(
        map.get("timeout_status"),
        "verification_contracts.timeout_status",
        VerificationStatus::from_name,
    )?;
    if on_timeout != VerificationStatus::Failed {
        contract = contract.timeout_is_unknown();
    }
    Ok(contract)
}

/// One impact node, out of the map [`impact_node_map`] wrote.
fn impact_node_from(item: &Value) -> Result<ImpactNode, ErrorValue> {
    let map = nested_map(item, "nodes")?;
    let mut node = ImpactNode::new(
        text(map.get("id"), "nodes.id")?,
        text(map.get("label"), "nodes.label")?,
        text(map.get("object_type"), "nodes.object_type")?,
        enumeration(map.get("class"), "nodes.class", ImpactClass::from_name)?,
        size(map.get("depth"), "nodes.depth")?,
    )
    .with_confidence(text(map.get("confidence"), "nodes.confidence")?);
    if let Some(relation) = optional_text_of(map.get("relation")) {
        node = node.reached_by(relation);
    }
    for evidence in text_list(map.get("evidence"), "nodes.evidence")? {
        node = node.citing(evidence);
    }
    if let Some(host) = optional_text_of(map.get("host")) {
        node = node.on_host(host);
    }
    Ok(node)
}

// ---------------------------------------------------------------------------------------------
// Vocabularies that are not enums
// ---------------------------------------------------------------------------------------------

/// The acknowledgement flags §40.3 spells for what the operator has already accepted (§19.4).
fn acknowledgement_flags(risk: &RiskAssessment) -> Vec<Value> {
    let mut flags = Vec::new();
    if risk.is_risk_accepted() {
        flags.push(Value::string(ACCEPT_RISK));
    }
    if risk.is_irreversible_accepted() {
        flags.push(Value::string(ACCEPT_IRREVERSIBLE));
    }
    flags
}

/// The metadata a restore puts back, and the metadata it does not (Appendix C.7).
fn metadata_lists(metadata: MetadataCoverage) -> (Vec<&'static str>, Vec<&'static str>) {
    let present = [
        metadata.content,
        metadata.mode,
        metadata.owner,
        metadata.acl,
        metadata.xattrs,
        metadata.capabilities,
        metadata.selinux,
        metadata.hardlinks,
    ];
    let mut restored = Vec::new();
    let mut gaps = Vec::new();
    for (index, piece) in METADATA_PIECES.iter().enumerate() {
        if present.get(index).copied().unwrap_or(false) {
            restored.push(*piece);
        } else {
            gaps.push(*piece);
        }
    }
    (restored, gaps)
}

/// The coverage a `metadata_restored` list describes (Appendix C.7).
fn metadata_from_names(names: &[Arc<str>]) -> Result<MetadataCoverage, ErrorValue> {
    let mut metadata = MetadataCoverage::none();
    for name in names {
        match name.as_ref() {
            "content" => metadata.content = true,
            "mode" => metadata.mode = true,
            "owner/group" => metadata.owner = true,
            "ACLs" => metadata.acl = true,
            "extended attributes" => metadata.xattrs = true,
            "file capabilities" => metadata.capabilities = true,
            "SELinux labels" => metadata.selinux = true,
            "hard-link relationships" => metadata.hardlinks = true,
            other => {
                return Err(malformed(
                    "metadata_restored",
                    &format!(
                        "names `{other}`, which is not a piece of metadata Appendix C.7 lists"
                    ),
                ));
            }
        }
    }
    Ok(metadata)
}

/// The strategy a `strategy` field describes (§28.4).
///
/// §28.4 fixes four strategies and rules out unbounded parallelism, so a width of zero is refused
/// here exactly as [`Strategy::parallel`] refuses it rather than read as "as many as possible".
fn strategy_from_text(text: &str) -> Result<Strategy, ErrorValue> {
    let trimmed = text.trim();
    if trimmed == "sequential" {
        return Ok(Strategy::Sequential);
    }
    if let Some(rest) = trimmed.strip_prefix("batch ") {
        return Strategy::batch(width(rest)?).ok_or_else(|| unbounded(trimmed));
    }
    if let Some(rest) = trimmed.strip_prefix("parallel ") {
        return Strategy::parallel(width(rest)?).ok_or_else(|| unbounded(trimmed));
    }
    if let Some(rest) = trimmed.strip_prefix("canary ")
        && let Some((canary, batch)) = rest
            .split_once(", then batch ")
            .or_else(|| rest.split_once(" then batch "))
    {
        return Strategy::canary(width(canary)?, width(batch)?).ok_or_else(|| unbounded(trimmed));
    }
    Err(malformed(
        "strategy",
        &format!("holds `{text}`, which is not one of §28.4's four strategies"),
    ))
}

/// A strategy width, which §28.4 never permits to be zero or absent.
fn width(text: &str) -> Result<usize, ErrorValue> {
    text.trim().parse::<usize>().map_err(|_| {
        malformed(
            "strategy",
            &format!("holds the width `{}`, which is not a number", text.trim()),
        )
    })
}

/// The refusal §28.4's "unlimited parallel mutation is not a default strategy" earns.
fn unbounded(text: &str) -> ErrorValue {
    malformed(
        "strategy",
        &format!("holds `{text}`, and §28.4 permits no strategy of width zero"),
    )
}

// ---------------------------------------------------------------------------------------------
// Writing and reading helpers
// ---------------------------------------------------------------------------------------------

/// The schema and the provenance a record of `id` is built with (§46).
fn target(id: &str) -> Result<(Arc<Schema>, Provenance), ErrorValue> {
    let schema_id = SchemaId::new(id, 1);
    let schema = builtin_schemas().get(&schema_id).ok_or_else(|| {
        ErrorValue::new(
            ErrorCode::ProviderSchemaViolation,
            format!("the `{id}/1` contract is not in this build"),
        )
    })?;
    Ok((schema, Provenance::local(PROVIDER, schema_id)))
}

/// Sets a field the schema declares.
///
/// A name the schema does not declare is a bug in this crate rather than something a caller can
/// cause, so the field stays unknown and the record still reaches its validation, where the
/// missing field is reported by name.
fn put(builder: RecordBuilder, name: &str, value: Value) -> RecordBuilder {
    let fallback = builder.clone();
    builder.set(name, value).unwrap_or(fallback)
}

/// Text, or null where there is none — never an empty string, which would read as a value.
fn optional_text(text: Option<&str>) -> Value {
    text.map_or(Value::Null, Value::string)
}

/// A count as the value model spells it, saturating rather than wrapping.
fn count_value(count: usize) -> Value {
    Value::Int(i128::try_from(count).unwrap_or(i128::MAX))
}

/// The refusal a record that cannot be read back earns (§36.2).
///
/// It names the field and what is wrong with it, because §45 requires a refusal a script can match
/// on and a person can act on, and "the plan store is corrupt" on its own is neither.
fn malformed(field: &str, detail: &str) -> ErrorValue {
    crate::error::store_corrupt(&format!("the `{field}` field {detail}"))
        .with_metadata("field", Value::string(field))
}

/// The text a field holds, or a refusal naming it.
fn text(source: Option<&Value>, field: &str) -> Result<Arc<str>, ErrorValue> {
    match source {
        Some(Value::String(text)) => Ok(Arc::clone(text)),
        None | Some(Value::Null) => Err(malformed(field, "is missing")),
        Some(other) => Err(malformed(
            field,
            &format!("holds a {} where text belongs", other.type_name()),
        )),
    }
}

/// The text a nullable field holds, or `None` where it holds nothing.
fn optional_text_of(source: Option<&Value>) -> Option<Arc<str>> {
    match source {
        Some(Value::String(text)) => Some(Arc::clone(text)),
        _ => None,
    }
}

/// The vocabulary member a field spells, or a refusal naming the field and the word.
fn enumeration<T>(
    source: Option<&Value>,
    field: &str,
    from_name: fn(&str) -> Option<T>,
) -> Result<T, ErrorValue> {
    let word = text(source, field)?;
    from_name(&word).ok_or_else(|| {
        malformed(
            field,
            &format!("holds `{word}`, which is not a word that vocabulary declares"),
        )
    })
}

/// The vocabulary member a nullable field spells, or `None` where it spells nothing.
fn optional_enumeration<T>(
    source: Option<&Value>,
    field: &str,
    from_name: fn(&str) -> Option<T>,
) -> Result<Option<T>, ErrorValue> {
    match source {
        None | Some(Value::Null) => Ok(None),
        _ => enumeration(source, field, from_name).map(Some),
    }
}

/// The whole number a field holds.
fn integer(source: Option<&Value>, field: &str) -> Result<i128, ErrorValue> {
    match source {
        Some(Value::Int(number)) => Ok(*number),
        None | Some(Value::Null) => Err(malformed(field, "is missing")),
        Some(other) => Err(malformed(
            field,
            &format!("holds a {} where a number belongs", other.type_name()),
        )),
    }
}

/// A count a field holds, refused rather than wrapped when it does not fit.
fn size(source: Option<&Value>, field: &str) -> Result<usize, ErrorValue> {
    usize::try_from(integer(source, field)?)
        .map_err(|_| malformed(field, "holds a number that is not a count"))
}

/// A revision a field holds, refused rather than wrapped when it does not fit.
fn counter(source: Option<&Value>, field: &str) -> Result<u32, ErrorValue> {
    u32::try_from(integer(source, field)?)
        .map_err(|_| malformed(field, "holds a number that is not a revision"))
}

/// The boolean a field holds.
fn flag(source: Option<&Value>, field: &str) -> Result<bool, ErrorValue> {
    match source {
        Some(Value::Bool(flag)) => Ok(*flag),
        None | Some(Value::Null) => Err(malformed(field, "is missing")),
        Some(other) => Err(malformed(
            field,
            &format!("holds a {} where a boolean belongs", other.type_name()),
        )),
    }
}

/// The instant a field holds.
fn instant(source: Option<&Value>, field: &str) -> Result<Timestamp, ErrorValue> {
    match source {
        Some(Value::Timestamp(at)) => Ok(*at),
        None | Some(Value::Null) => Err(malformed(field, "is missing")),
        Some(other) => Err(malformed(
            field,
            &format!("holds a {} where an instant belongs", other.type_name()),
        )),
    }
}

/// The instant a nullable field holds, or `None` where it holds nothing.
fn optional_instant(source: Option<&Value>, field: &str) -> Result<Option<Timestamp>, ErrorValue> {
    match source {
        None | Some(Value::Null) => Ok(None),
        _ => instant(source, field).map(Some),
    }
}

/// The quantity a nullable `bytesize` field holds. Null is unknown, never zero (§38.2).
fn byte_size(source: Option<&Value>, field: &str) -> Result<Option<ByteSize>, ErrorValue> {
    match source {
        None | Some(Value::Null) => Ok(None),
        Some(Value::ByteSize(size)) => Ok(Some(*size)),
        Some(other) => Err(malformed(
            field,
            &format!("holds a {} where a size belongs", other.type_name()),
        )),
    }
}

/// The span a `duration` field holds, clamped at zero because retention never runs backwards.
fn span(source: Option<&Value>, field: &str) -> Result<std::time::Duration, ErrorValue> {
    match source {
        Some(Value::Duration(duration)) => {
            let nanoseconds = u64::try_from(duration.nanoseconds().max(0)).unwrap_or(u64::MAX);
            Ok(std::time::Duration::from_nanos(nanoseconds))
        }
        None | Some(Value::Null) => Err(malformed(field, "is missing")),
        Some(other) => Err(malformed(
            field,
            &format!("holds a {} where a span belongs", other.type_name()),
        )),
    }
}

/// A span a nullable field holds, or `None` where nobody measured it (spec v0.2 §35.3).
fn optional_span(
    source: Option<&Value>,
    field: &str,
) -> Result<Option<std::time::Duration>, ErrorValue> {
    match source {
        None | Some(Value::Null) => Ok(None),
        Some(_) => span(source, field).map(Some),
    }
}

/// A percent field that may be null.
fn optional_percent(source: Option<&Value>, field: &str) -> Result<Option<Percent>, ErrorValue> {
    match source {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Percent(share)) => Ok(Some(*share)),
        Some(_) => Err(malformed(field, "is not a percent")),
    }
}

/// The items a list field holds.
fn list_field<'a>(source: Option<&'a Value>, field: &str) -> Result<&'a [Value], ErrorValue> {
    match source {
        Some(Value::List(items)) => Ok(items.as_ref()),
        None | Some(Value::Null) => Err(malformed(field, "is missing")),
        Some(other) => Err(malformed(
            field,
            &format!("holds a {} where a list belongs", other.type_name()),
        )),
    }
}

/// The strings a `list<string>` field holds.
fn text_list(source: Option<&Value>, field: &str) -> Result<Vec<Arc<str>>, ErrorValue> {
    let mut items = Vec::new();
    for item in list_field(source, field)? {
        items.push(text(Some(item), field)?);
    }
    Ok(items)
}

/// The map a `record` field holds.
fn map_field<'a>(source: Option<&'a Value>, field: &str) -> Result<&'a MapValue, ErrorValue> {
    match source {
        Some(Value::Map(map)) => Ok(map.as_ref()),
        None | Some(Value::Null) => Err(malformed(field, "is missing")),
        Some(other) => Err(malformed(
            field,
            &format!("holds a {} where a sub-record belongs", other.type_name()),
        )),
    }
}

/// The map a nullable `record` field holds, or `None` where it holds nothing.
fn optional_map<'a>(
    source: Option<&'a Value>,
    field: &str,
) -> Result<Option<&'a MapValue>, ErrorValue> {
    match source {
        None | Some(Value::Null) => Ok(None),
        _ => map_field(source, field).map(Some),
    }
}

/// One map out of a list of them.
fn nested_map<'a>(item: &'a Value, field: &str) -> Result<&'a MapValue, ErrorValue> {
    map_field(Some(item), field)
}

/// One schema-bound record out of a list of them.
fn nested_record<'a>(item: &'a Value, field: &str) -> Result<&'a RecordValue, ErrorValue> {
    match item {
        Value::Record(record) => Ok(record.as_ref()),
        other => Err(malformed(
            field,
            &format!("holds a {} where a record belongs", other.type_name()),
        )),
    }
}

/// The value a `value`-typed field holds, or `None` where it holds nothing (§10.5).
fn held_value(source: Option<&Value>) -> Option<Value> {
    match source {
        None | Some(Value::Null) => None,
        Some(value) => Some(value.clone()),
    }
}

/// The plan identity a field holds.
fn plan_id(source: Option<&Value>, field: &str) -> Result<PlanId, ErrorValue> {
    parse_plan_id(&text(source, field)?, field)
}

/// A plan identity, refused rather than searched for when it is not one (§36.4).
fn parse_plan_id(text: &str, field: &str) -> Result<PlanId, ErrorValue> {
    PlanId::parse(text).ok_or_else(|| {
        malformed(
            field,
            &format!("holds `{text}`, which is not a plan identity"),
        )
    })
}

/// A recovery asset identity, refused rather than searched for when it is not one (§11.1).
fn parse_asset_id(text: &str, field: &str) -> Result<RecoveryAssetId, ErrorValue> {
    RecoveryAssetId::parse(text).ok_or_else(|| {
        malformed(
            field,
            &format!("holds `{text}`, which is not a recovery asset identity"),
        )
    })
}
