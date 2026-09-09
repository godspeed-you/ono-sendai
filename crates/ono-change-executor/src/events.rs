//! What a plan contributes to the v0.5 evidence ledger (spec v0.6 §22.1, §22.2, §22.4).
//!
//! v0.5's ledger is the audit trail, so v0.6 builds no second history. §22.1 lists thirteen things
//! a plan should record, and every one of them is written as an existing v0.5 event kind carrying
//! a namespaced subtype. `EventKind` is a closed list of seventeen (v0.5 §6.1), gated in both
//! directions by `xtask/src/temporal.rs`, and §6.1 already names the escape: *"providers and
//! plugins MAY define namespaced subtypes, but the top-level semantics MUST map to one of these
//! classes"*.
//!
//! # The mapping
//!
//! | §22.1 | `EventKind` | `subtype` | why |
//! |---|---|---|---|
//! | `PlanCreated` | `action.requested` | `ono.plan.created` | §17.2's "an operator asked for a mutation through the shell". A plan *is* the request, and it has been neither authorised nor executed. |
//! | `PlanSealed` | `action.authorized` | `ono.plan.sealed` | §4.4's seal is where the change stops moving and §19.4 stores the acknowledgements in the sealed revision. That is an authorisation gate, and no other kind is one. |
//! | `PlanProtected` | `object.changed` | `ono.plan.protected` | §4.6 is a change to the plan object's own state, not a mutation of a target. §5.5 makes the assets a real mutation, and those are recorded separately as `RecoveryAssetCreated`. |
//! | `ActionStarted` | `action.executed` | `ono.plan.action.started` | §17.2's own kind for "the mutation was carried out". |
//! | `ActionCompleted` | `action.completed` | `ono.plan.action.completed` | §17.2's own kind. An action that failed becomes `action.failed` with `ono.plan.action.failed`: §2.14 forbids calling a failure a completion, and §22.1's single name cannot be allowed to erase the difference. |
//! | `VerificationObserved` | `object.observed` | `ono.plan.verification.observed` | §23 observes the world without claiming it changed, which is precisely `object.observed`'s definition. |
//! | `PlanVerified` | `action.completed` | `ono.plan.verified` | the plan as a whole finished. A `DEGRADED` plan is `ono.plan.degraded` on the same kind — §4.8 says the intended primary state exists — and a `FAILED` one is `action.failed` with `ono.plan.failed`. |
//! | `RecoveryPlanned` | `action.requested` | `ono.recovery.planned` | §5.8: `recover` plans recovery and does not perform it, so the request is all that has happened. |
//! | `RecoveryStarted` | `action.executed` | `ono.recovery.started` | as `ActionStarted`, on the recovery branch of §4.1. |
//! | `RecoveryCompleted` | `action.completed` | `ono.recovery.completed` | as `ActionCompleted`; a failed recovery is `action.failed` with `ono.recovery.failed`. |
//! | `RecoveryVerified` | `object.observed` | `ono.recovery.verified` | §25.1 verifies scoped equivalence by observation, and §25.3 forbids a global claim, so it is an observation and not a completion. |
//! | `RecoveryAssetCreated` | `object.appeared` | `ono.recovery.asset.created` | §11.1's asset became observable, which is §6.3's own wording. |
//! | `RecoveryAssetRemoved` | `object.disappeared` | `ono.recovery.asset.removed` | the asset ceased to exist (§37). |
//!
//! # The causal anchor
//!
//! §22.4 makes the plan id a causal anchor — `timeline --plan @plan` and `why` both join on it —
//! and v0.5 already has the mechanism: an [`EvidenceClaim::Transaction`] whose token is the
//! identity, at [`EvidenceStrength::Authoritative`], exactly as `ActionLifecycle` claims an
//! `ActionId`. Every event this module writes carries one whose token is the [`PlanId`].
//!
//! # Two rules taken from the existing implementation
//!
//! - **The source is `ono.session`.** `EvidenceSource::parse` refuses anything outside v0.5 §7.1's
//!   closed list, so a plan cannot invent a source class for itself.
//! - **Recording never fails a command.** `crates/ono-cli/src/temporal/mod.rs` states it: a ledger
//!   that refused an append would be a reason to lose history, and §16.5 does not make it a reason
//!   to lose the mutation's own result. [`PlanLifecycle::record`] returns the refusal and every
//!   call site in the executor's own tests discards it.

use std::sync::Arc;

use jiff::Timestamp;
use ono_change_core::{
    ActionStatus, ChangePlan, PlanAction, PlanId, PlanState, RecoveryAsset, Verdict,
    VerificationResult,
};
use ono_spatial_core::{SpatialId, SpatialScope};
use ono_temporal_core::{
    ClockDomain, Evidence, EvidenceClaim, EvidenceId, EvidenceSource, EvidenceStrength, EventKind,
    EventSeed, EventTimes, LedgerWrite, ObjectState, RelationState, SpatialRef, TemporalEvent,
};
use ono_temporal_ledger::Ledger;
use ono_temporal_reconstruct::{
    CheckpointPolicy, CheckpointProjection, CheckpointRequest, project_checkpoint,
};
use ono_value::{ErrorValue, MapValue, Provenance, SchemaId, Value};

/// The subtype §22.1's `PlanCreated` is written under.
pub const PLAN_CREATED: &str = "ono.plan.created";
/// The subtype §22.1's `PlanSealed` is written under.
pub const PLAN_SEALED: &str = "ono.plan.sealed";
/// The subtype §22.1's `PlanProtected` is written under.
pub const PLAN_PROTECTED: &str = "ono.plan.protected";
/// The subtype §22.1's `ActionStarted` is written under.
pub const ACTION_STARTED: &str = "ono.plan.action.started";
/// The subtype §22.1's `ActionCompleted` is written under.
pub const ACTION_COMPLETED: &str = "ono.plan.action.completed";
/// The subtype an action that did not complete is written under (§2.14).
pub const ACTION_FAILED: &str = "ono.plan.action.failed";
/// The subtype §22.1's `VerificationObserved` is written under.
pub const VERIFICATION_OBSERVED: &str = "ono.plan.verification.observed";
/// The subtype §22.1's `PlanVerified` is written under.
pub const PLAN_VERIFIED: &str = "ono.plan.verified";
/// The subtype a plan that reached §4.8's `DEGRADED` is written under.
pub const PLAN_DEGRADED: &str = "ono.plan.degraded";
/// The subtype a plan that reached §4.8's `FAILED` is written under.
pub const PLAN_FAILED: &str = "ono.plan.failed";
/// The subtype §22.1's `RecoveryPlanned` is written under.
pub const RECOVERY_PLANNED: &str = "ono.recovery.planned";
/// The subtype §22.1's `RecoveryStarted` is written under.
pub const RECOVERY_STARTED: &str = "ono.recovery.started";
/// The subtype §22.1's `RecoveryCompleted` is written under.
pub const RECOVERY_COMPLETED: &str = "ono.recovery.completed";
/// The subtype a recovery that did not complete is written under (§4.1, Appendix F).
pub const RECOVERY_FAILED: &str = "ono.recovery.failed";
/// The subtype §22.1's `RecoveryVerified` is written under.
pub const RECOVERY_VERIFIED: &str = "ono.recovery.verified";
/// The subtype §22.1's `RecoveryAssetCreated` is written under.
pub const ASSET_CREATED: &str = "ono.recovery.asset.created";
/// The subtype §22.1's `RecoveryAssetRemoved` is written under.
pub const ASSET_REMOVED: &str = "ono.recovery.asset.removed";

/// The §22.1 lifecycle of one plan, accumulated and sealed before it reaches a ledger.
///
/// It mirrors `ono-cli`'s `ActionLifecycle` deliberately: events and evidence are staged together,
/// each event is sealed so its identity is a function of its content (v0.5 §6.8), and one
/// [`PlanLifecycle::record`] writes both at the end.
#[derive(Debug, Clone)]
pub struct PlanLifecycle {
    plan: PlanId,
    revision: u32,
    intent: Arc<str>,
    session: Arc<str>,
    scope: SpatialScope,
    events: Vec<TemporalEvent>,
    evidence: Vec<Evidence>,
}

impl PlanLifecycle {
    /// Opens the lifecycle of `plan` in `scope`, recording §22.1's `PlanCreated`.
    #[must_use]
    pub fn created(plan: &ChangePlan, scope: SpatialScope, at: Timestamp) -> Self {
        let mut lifecycle = Self {
            plan: plan.id().clone(),
            revision: plan.revision(),
            intent: Arc::from(plan.intent().text()),
            session: Arc::from(plan.session()),
            scope,
            events: Vec::new(),
            evidence: Vec::new(),
        };
        lifecycle.stage(
            EventKind::ActionRequested,
            PLAN_CREATED,
            at,
            None,
            Vec::new(),
        );
        lifecycle
    }

    /// The plan the lifecycle is about, which §22.4 makes the causal anchor.
    #[must_use]
    pub const fn plan(&self) -> &PlanId {
        &self.plan
    }

    /// Records §22.1's `PlanSealed` (§4.4).
    pub fn sealed(&mut self, digest: Option<&str>, at: Timestamp) {
        self.stage(
            EventKind::ActionAuthorized,
            PLAN_SEALED,
            at,
            None,
            vec![("digest", digest.map_or(Value::Null, Value::string))],
        );
    }

    /// Records §22.1's `PlanProtected` (§4.6).
    pub fn protected(&mut self, assets: usize, at: Timestamp) {
        self.stage(
            EventKind::ObjectChanged,
            PLAN_PROTECTED,
            at,
            None,
            vec![(
                "assets",
                Value::Int(i128::try_from(assets).unwrap_or(i128::MAX)),
            )],
        );
    }

    /// Records §22.1's `ActionStarted` (§4.7).
    pub fn action_started(&mut self, action: &PlanAction, at: Timestamp) {
        self.stage(
            EventKind::ActionExecuted,
            ACTION_STARTED,
            at,
            Some(action.summary()),
            vec![
                ("action", Value::string(action.id().as_str())),
                ("role", Value::string(action.role().as_str())),
                (
                    "target",
                    action.target().map_or(Value::Null, Value::string),
                ),
            ],
        );
    }

    /// Records §22.1's `ActionCompleted`, or §2.14's honest counterpart where it did not.
    ///
    /// An action that failed, was skipped or whose outcome could not be established is not a
    /// completion. `action.failed` is the kind for the first, and the last two are recorded with
    /// their status in the payload so Appendix F.2's uncertainty survives the round trip.
    pub fn action_settled(&mut self, action: &PlanAction, status: ActionStatus, at: Timestamp) {
        let (kind, subtype) = match status {
            ActionStatus::Succeeded => (EventKind::ActionCompleted, ACTION_COMPLETED),
            _ => (EventKind::ActionFailed, ACTION_FAILED),
        };
        self.stage(
            kind,
            subtype,
            at,
            Some(action.summary()),
            vec![
                ("action", Value::string(action.id().as_str())),
                ("status", Value::string(status.as_str())),
            ],
        );
    }

    /// Records §22.1's `VerificationObserved`, one event per check (§23.3).
    pub fn verification_observed(&mut self, result: &VerificationResult, at: Timestamp) {
        self.stage(
            EventKind::ObjectObserved,
            VERIFICATION_OBSERVED,
            at,
            Some(result.subject()),
            vec![
                ("check", Value::string(result.check().as_str())),
                ("class", Value::string(result.class().as_str())),
                ("status", Value::string(result.status().as_str())),
                ("expression", Value::string(result.expression())),
            ],
        );
    }

    /// Records §22.1's `PlanVerified`, at whichever of §4.8's three outcomes was reached.
    pub fn verified(&mut self, verdict: Verdict, at: Timestamp) {
        let (kind, subtype) = match verdict {
            Verdict::Verified => (EventKind::ActionCompleted, PLAN_VERIFIED),
            Verdict::Degraded => (EventKind::ActionCompleted, PLAN_DEGRADED),
            Verdict::Failed => (EventKind::ActionFailed, PLAN_FAILED),
        };
        self.stage(kind, subtype, at, None, Vec::new());
    }

    /// Records §22.1's `RecoveryPlanned` (§24.1, §5.8).
    pub fn recovery_planned(&mut self, recovery: &PlanId, at: Timestamp) {
        self.stage(
            EventKind::ActionRequested,
            RECOVERY_PLANNED,
            at,
            None,
            vec![("recovery_plan", Value::string(recovery.as_str()))],
        );
    }

    /// Records §22.1's `RecoveryStarted` (§4.1's `RECOVERING`).
    pub fn recovery_started(&mut self, at: Timestamp) {
        self.stage(
            EventKind::ActionExecuted,
            RECOVERY_STARTED,
            at,
            None,
            Vec::new(),
        );
    }

    /// Records §22.1's `RecoveryCompleted`, or the failure Appendix F preserves instead.
    pub fn recovery_settled(&mut self, state: PlanState, at: Timestamp) {
        let (kind, subtype) = if state == PlanState::RecoveryFailed {
            (EventKind::ActionFailed, RECOVERY_FAILED)
        } else {
            (EventKind::ActionCompleted, RECOVERY_COMPLETED)
        };
        self.stage(
            kind,
            subtype,
            at,
            None,
            vec![("state", Value::string(state.as_str()))],
        );
    }

    /// Records §22.1's `RecoveryVerified`, naming the domains §25.1 scopes it to.
    pub fn recovery_verified(&mut self, domains: &[String], at: Timestamp) {
        self.stage(
            EventKind::ObjectObserved,
            RECOVERY_VERIFIED,
            at,
            None,
            vec![(
                "domains",
                Value::list(domains.iter().map(|domain| Value::string(domain))),
            )],
        );
    }

    /// Records §22.1's `RecoveryAssetCreated` (§11.1, §5.5).
    pub fn asset_created(&mut self, asset: &RecoveryAsset, at: Timestamp) {
        self.stage(
            EventKind::ObjectAppeared,
            ASSET_CREATED,
            at,
            Some(asset.reference()),
            vec![
                ("asset", Value::string(asset.id().as_str())),
                ("provider", Value::string(asset.provider())),
                ("asset_type", Value::string(asset.asset_type().as_str())),
                ("state", Value::string(asset.state().as_str())),
                ("scope", Value::string(asset.scope().domain())),
            ],
        );
    }

    /// Records §22.1's `RecoveryAssetRemoved` (§37).
    pub fn asset_removed(&mut self, asset: &ono_change_core::RecoveryAssetId, at: Timestamp) {
        self.stage(
            EventKind::ObjectDisappeared,
            ASSET_REMOVED,
            at,
            None,
            vec![("asset", Value::string(asset.as_str()))],
        );
    }

    /// The events staged so far, sealed (v0.5 §6.8).
    #[must_use]
    pub fn events(&self) -> &[TemporalEvent] {
        &self.events
    }

    /// The evidence staged so far (v0.5 §3.4).
    #[must_use]
    pub fn evidence(&self) -> &[Evidence] {
        &self.evidence
    }

    /// Writes the lifecycle to `ledger`, events and evidence together (v0.5 §3.4).
    ///
    /// # Errors
    ///
    /// Whatever refusal the ledger raises. Discarding it at the call site is the rule
    /// `crates/ono-cli/src/temporal/mod.rs` states: a ledger that refused an append is not a
    /// reason to lose the mutation's own result.
    pub fn record(&self, ledger: &dyn LedgerWrite) -> Result<(), ErrorValue> {
        ledger.append(&self.events, &self.evidence)?;
        Ok(())
    }

    /// Stages one event with the §22.4 anchor claim behind it.
    fn stage(
        &mut self,
        kind: EventKind,
        subtype: &str,
        at: Timestamp,
        subject: Option<&str>,
        payload: Vec<(&str, Value)>,
    ) {
        // §22.4: the plan id is the causal anchor `why` and `timeline --plan` join on, and v0.5's
        // existing mechanism for that is a transaction claim whose token is the identity.
        let claim = EvidenceClaim::Transaction {
            token: Arc::from(self.plan.as_str()),
            at,
        };
        let source = EvidenceSource::session();
        let record = Evidence {
            evidence_id: EvidenceId::of(&source, at, &self.scope, None, &claim),
            source: source.clone(),
            observed_at: at,
            source_time: Some(at),
            scope: self.scope.clone(),
            subject: None,
            claim,
            strength: EvidenceStrength::Authoritative,
            raw_ref: None,
            derived_from: Vec::new(),
            provenance: Provenance::local(
                source.as_str(),
                SchemaId::new("ono.temporal-evidence", 1),
            ),
        };
        let evidence_id = record.evidence_id.clone();
        self.evidence.push(record);
        let seed = EventSeed {
            kind,
            subtype: Some(Arc::from(subtype)),
            scope: self.scope.clone(),
            subject: subject.map(|label| SpatialRef::Unresolved {
                source: EvidenceSource::session(),
                described: Arc::from(label),
            }),
            related: Vec::new(),
            times: EventTimes {
                source_time: Some(at),
                observed_at: at,
                ingested_at: at,
                source_sequence: None,
                monotonic_nanos: None,
                clock_uncertainty: None,
                domain: ClockDomain {
                    host: Arc::from(self.scope.host_scope().id()),
                    boot_id: None,
                },
            },
            before: None,
            after: None,
            changed_fields: Vec::new(),
            evidence: vec![evidence_id],
            causal_parents: Vec::new(),
            payload: Some(self.payload(subtype, payload)),
            provenance: Provenance::local(
                EvidenceSource::session().as_str(),
                SchemaId::new("ono.temporal-event", 1),
            )
            .from_source(EvidenceSource::session().as_str()),
        };
        self.events.push(seed.seal());
    }

    /// The plan-shaped body every one of these events carries (§22.4).
    fn payload(&self, subtype: &str, extra: Vec<(&str, Value)>) -> Value {
        let mut map = MapValue::new();
        map.insert("plan".into(), Value::string(self.plan.as_str()));
        map.insert(
            "revision".into(),
            Value::Int(i128::from(self.revision)),
        );
        map.insert("intent".into(), Value::string(&self.intent));
        map.insert("session".into(), Value::string(&self.session));
        map.insert("event".into(), Value::string(subtype));
        for (key, value) in extra {
            map.insert(key.into(), value);
        }
        Value::Map(Arc::new(map))
    }
}

/// §22.2's pre-plan checkpoint, taken immediately before mutation.
///
/// The checkpoint is written only when the ledger is persistent, and the reason is the whole point
/// of taking it: §22.2 exists so an operator can compare pre-plan and post-plan state (§22.3) and
/// investigate a failure afterwards (§22.4). A session ledger dies with the session that could not
/// finish reading it, so a checkpoint there is a cost with no payer.
///
/// `objects` and `relations` are what the caller currently sees; the projection keeps the ones the
/// plan actually reaches — its frozen targets and its impact graph — and v0.5 §42.2's policy bounds
/// the rest, so "all files under `/`" is not checkpointable by forgetting to filter.
///
/// Returns `None` when the ledger is not persistent, and the projection otherwise. A ledger that
/// refused the write still returns the projection: §16.5 does not make a store failure a reason to
/// lose the mutation's own result.
#[must_use]
pub fn checkpoint_before_mutation(
    plan: &ChangePlan,
    scope: &SpatialScope,
    objects: &[ObjectState],
    relations: &[RelationState],
    ledger: &Ledger,
    at: Timestamp,
) -> Option<CheckpointProjection> {
    if !ledger.is_persistent() {
        return None;
    }
    let relevant = relevant_ids(plan);
    let kept: Vec<ObjectState> = objects
        .iter()
        .filter(|object| relevant.contains(&object.id))
        .cloned()
        .collect();
    let keep: Vec<SpatialId> = kept.iter().map(|object| object.id.clone()).collect();
    let edges: Vec<RelationState> = relations
        .iter()
        .filter(|edge| keep.contains(&edge.from) || keep.contains(&edge.to))
        .cloned()
        .collect();
    let request = CheckpointRequest::new(scope.clone(), at)
        .with_objects(kept)
        .with_relations(edges)
        .with_provenance(Provenance::local(
            EvidenceSource::session().as_str(),
            SchemaId::new("ono.temporal-checkpoint", 1),
        ));
    let projection = project_checkpoint(&request, &CheckpointPolicy::default_local());
    // §16.5's rule again: the checkpoint is worth having and never worth the plan's own result.
    let _ = ledger.write_checkpoint(projection.checkpoint());
    Some(projection)
}

/// The spatial identities §22.2 calls "plan targets and impact-relevant objects".
fn relevant_ids(plan: &ChangePlan) -> Vec<SpatialId> {
    let mut ids: Vec<SpatialId> = plan
        .targets()
        .iter()
        .filter_map(|target| target.spatial_id().and_then(SpatialId::parse))
        .collect();
    for node in plan.impact().nodes() {
        if let Some(id) = SpatialId::parse(node.id())
            && !ids.contains(&id)
        {
            ids.push(id);
        }
    }
    ids
}
