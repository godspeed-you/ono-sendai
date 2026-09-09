//! The canonical event bridge: what the shell already knows, turned into temporal events
//! (spec v0.5 §6, §17, §39.1).
//!
//! Three streams reach the ledger through this module, and §39.1 fixes the shape of all three —
//! "a small adapter seam from `ono-spatial-events`' canonical change output into
//! `ono-temporal-core` events rather than moving provider/clock/store logic into it":
//!
//! - **spatial change output**, which since v0.5 carries its own observation time or interval, so
//!   an event built from it never borrows the ingest clock (§3.3, §9.2);
//! - **provider events** from the watch path, which already carry `at`, `sequence` and
//!   `changed_fields`;
//! - **the Ono action lifecycle** of §17.2, which is the one causal advantage a shell has.
//!
//! # What an action must carry, and why
//!
//! §17.1: the shell "knows exactly which actions the operator requested". Two claims turn that
//! knowledge into evidence a rule can join on, and [`ActionLifecycle`] writes both:
//!
//! - every `action.executed` carries an `ono.session` [`EvidenceClaim::Transaction`] whose token
//!   is the [`ActionId`] itself, which is what `ono.action-to-transaction` joins a provider's own
//!   job or request identifier against (§17.3, §15.3);
//! - where the action forked a process, it also carries an authoritative `ono.session`
//!   [`EvidenceClaim::ObjectExisted`] whose **subject** is that process's `SpatialId`, which is
//!   what `ono.action-launched-process` matches the appearing process against (§17.6).
//!
//! Without those two the strongest causal rules never fire, and §17 is the reason a shell can
//! explain anything at all.
//!
//! # What is never written
//!
//! §17.5 and §30.3: the command reaches the ledger as a [`RedactedCommandSummary`], which is
//! built before an [`ActionEvent`] exists. The raw text never enters the type, so there is no
//! path by which an unredacted argument could be persisted. §17.5 also keeps arbitrary external
//! command text out of the ledger as an action body unless it is explicitly configured, and
//! `temporal.record.process_argv` is that switch (§33).

use std::sync::Arc;

use jiff::Timestamp;
use ono_spatial_core::{SpatialId, SpatialScope, SpatialType};
use ono_spatial_events::{ChangeKind, ChangeSet, ObservedAt};
use ono_temporal_core::{
    ActionEvent, ActionId, ActionOutcome, ActionResultSummary, AuthorizationDecision,
    AuthorizationSummary, ClockDomain, EventKind, EventSeed, EventTimes, Evidence, EvidenceClaim,
    EvidenceId, EvidenceSource, EvidenceStrength, LedgerWrite, Redactable, RedactedCommandSummary,
    SpatialRef, TemporalEvent, TimeRange,
};
use ono_value::{Provenance, SchemaId, Value};

/// The schema an ingested event signs its provenance with, for the shell's own observations.
fn provenance() -> Provenance {
    provenance_of(&EvidenceSource::session())
}

/// The provenance of an event attributed to `source`.
///
/// `ono.temporal-event/1`'s `source` field is read back out of the provenance provider through
/// `EvidenceSource::parse`, so the provider name has to be one of §7.1's classes — a name that is
/// not renders with no source tag at all, and §11.5's tag is how a reader tells an observation
/// from a composition.
fn provenance_of(source: &EvidenceSource) -> Provenance {
    Provenance::local(source.as_str(), SchemaId::new("ono.temporal-event", 1))
        .from_source(source.as_str())
}

/// The §7.1 evidence source class a provider name belongs to (§7.1, §39.1).
///
/// Two names already in the tree are not §7.1 classes and reach values through
/// `ono-spatial-events`: `linux.sock-diag`, which is a socket observation and therefore
/// `linux.netlink`, and `ono.runtime`, which is the shell's own watch runtime and therefore
/// `ono.session`. Mapping them is the bridge's job — §7.1's list is closed, and a source outside
/// it is not a source the evidence model can weigh.
#[must_use]
pub fn source_class(provider: &str) -> EvidenceSource {
    match provider {
        "linux.sock-diag" => {
            EvidenceSource::builtin("linux.netlink").unwrap_or_else(EvidenceSource::session)
        }
        "ono.runtime" | "ono.spatial" | "ono.session" => EvidenceSource::session(),
        other => EvidenceSource::parse(other).unwrap_or_else(EvidenceSource::session),
    }
}

/// The clock domain of this host and boot (§25.5, §26.4).
fn domain() -> ClockDomain {
    ClockDomain {
        host: Arc::from(host_name().as_str()),
        boot_id: boot_id().map(|id| Arc::from(id.as_str())),
    }
}

/// This host's name, as the spatial scope already spells it (v0.4 §3.2).
fn host_name() -> String {
    crate::spatial::local_scope().host_scope().id().to_owned()
}

/// The kernel's boot identity, which is what makes two observations comparable (§25.5).
fn boot_id() -> Option<String> {
    std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .ok()
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}

/// The three timestamps §3.3 refuses to collapse, for an observation seen at `observed_at`.
fn times(source_time: Option<Timestamp>, observed_at: Timestamp) -> EventTimes {
    EventTimes {
        source_time,
        observed_at,
        ingested_at: observed_at,
        source_sequence: None,
        monotonic_nanos: None,
        clock_uncertainty: None,
        domain: domain(),
    }
}

/// An observation of `claim` by `source`, at the strength it can honestly claim.
fn evidence_from(
    source: EvidenceSource,
    scope: &SpatialScope,
    subject: Option<&SpatialId>,
    observed_at: Timestamp,
    claim: EvidenceClaim,
    strength: EvidenceStrength,
) -> Evidence {
    let provenance = Provenance::local(source.as_str(), SchemaId::new("ono.temporal-evidence", 1));
    Evidence {
        evidence_id: EvidenceId::of(&source, observed_at, scope, subject, &claim),
        source,
        observed_at,
        source_time: Some(observed_at),
        scope: scope.clone(),
        subject: subject.cloned(),
        claim,
        strength,
        raw_ref: None,
        derived_from: Vec::new(),
        provenance,
    }
}

/// The shell's own observation of `claim` (§7.1's `ono.session`).
fn evidence(
    scope: &SpatialScope,
    subject: Option<&SpatialId>,
    observed_at: Timestamp,
    claim: EvidenceClaim,
    strength: EvidenceStrength,
) -> Evidence {
    evidence_from(
        EvidenceSource::session(),
        scope,
        subject,
        observed_at,
        claim,
        strength,
    )
}

/// The §17.2 lifecycle of one Ono mutation, from the request to the outcome.
///
/// The [`ActionId`] is minted by [`ActionLifecycle::requested`] — before execution, as §17.3
/// requires — so it can be propagated into the provider call that follows and recorded against
/// whatever transaction identifier the authority returns.
#[derive(Debug, Clone)]
pub struct ActionLifecycle {
    action: ActionEvent,
    scope: SpatialScope,
    /// The process this action forked, where it forked one (§17.6).
    launched: Option<SpatialId>,
    events: Vec<TemporalEvent>,
    evidence: Vec<Evidence>,
}

impl ActionLifecycle {
    /// Mints the identity and records `action.requested` (§17.2, §17.3).
    ///
    /// `command` is already redacted: §17.5 keeps the raw text out of the type, so redaction
    /// happens at the call site that still has the typed arguments and knows which were secret.
    #[must_use]
    pub fn requested(
        scope: SpatialScope,
        session_id: &str,
        actor: &str,
        operation: &str,
        target: Option<SpatialId>,
        command: RedactedCommandSummary,
        requested_at: Timestamp,
    ) -> Self {
        let action_id = ActionId::of(session_id, requested_at, operation, target.as_ref());
        let action = ActionEvent {
            action_id,
            command,
            actor: Arc::from(actor),
            session_id: Arc::from(session_id),
            requested_at,
            target,
            operation: Arc::from(operation),
            authorization: AuthorizationSummary {
                decision: AuthorizationDecision::NotRequired,
                risk: Arc::from("mutate"),
                capability: None,
                reason: None,
            },
            result: None,
            external_transaction: None,
            provenance: Provenance::local("ono.session", SchemaId::new("ono.action-event", 1)),
        };
        let mut lifecycle = Self {
            action,
            scope,
            launched: None,
            events: Vec::new(),
            evidence: Vec::new(),
        };
        lifecycle.stage(EventKind::ActionRequested, requested_at, false);
        lifecycle
    }

    /// The identity §17.3 requires to exist before execution, for propagation into a provider.
    #[must_use]
    pub fn action_id(&self) -> &ActionId {
        &self.action.action_id
    }

    /// Records the authorization decision (§17.2).
    pub fn authorized(&mut self, authorization: AuthorizationSummary, at: Timestamp) {
        self.action.authorization = authorization;
        self.stage(EventKind::ActionAuthorized, at, false);
    }

    /// Names the process the shell forked for this action (§17.6).
    ///
    /// The claim it produces is the whole of `ono.action-launched-process`: the shell owns process
    /// creation, so this is a fact rather than an inference, and it is authoritative for exactly
    /// that reason.
    pub fn launched(&mut self, process: SpatialId) {
        self.launched = Some(process);
    }

    /// Records the external authority's own transaction identifier (§17.3).
    pub fn external_transaction(&mut self, token: &str) {
        self.action.external_transaction = Some(Arc::from(token));
    }

    /// Records `action.executed`, with the two claims §17.3 and §17.6 rest on.
    pub fn executed(&mut self, at: Timestamp) {
        self.stage(EventKind::ActionExecuted, at, true);
    }

    /// Records `action.completed` (§17.2).
    pub fn completed(&mut self, at: Timestamp, detail: Option<&str>) {
        self.finish(ActionOutcome::Succeeded, at, detail, None);
    }

    /// Records `action.failed`, with the structured code it failed with (§17.2).
    pub fn failed(&mut self, at: Timestamp, detail: Option<&str>, code: Option<&str>) {
        self.finish(ActionOutcome::Failed, at, detail, code);
    }

    fn finish(
        &mut self,
        outcome: ActionOutcome,
        at: Timestamp,
        detail: Option<&str>,
        code: Option<&str>,
    ) {
        self.action.result = Some(ActionResultSummary {
            outcome,
            completed_at: at,
            detail: detail.map(Arc::from),
            error_code: code.map(Arc::from),
        });
        let kind = match outcome {
            ActionOutcome::Succeeded => EventKind::ActionCompleted,
            ActionOutcome::Failed | ActionOutcome::Cancelled => EventKind::ActionFailed,
        };
        self.stage(kind, at, false);
    }

    /// Builds one lifecycle event, with the evidence its kind requires.
    fn stage(&mut self, kind: EventKind, at: Timestamp, causal_anchor: bool) {
        let mut ids = Vec::new();
        if causal_anchor {
            // §17.3: the shell's own record that this action reached the system. The token is the
            // ActionId, which is what a provider's transaction identifier is joined against.
            let claim = EvidenceClaim::Transaction {
                token: Arc::from(self.action.action_id.as_str()),
                at,
            };
            let record = evidence(
                &self.scope,
                self.action.target.as_ref(),
                at,
                claim,
                EvidenceStrength::Authoritative,
            );
            ids.push(record.evidence_id.clone());
            self.evidence.push(record);
            // §17.6: the process the shell forked. The subject is the process identity, because
            // that is what `ono.action-launched-process` matches the appearing object against.
            if let Some(process) = self.launched.clone() {
                let claim = EvidenceClaim::ObjectExisted { at };
                let record = evidence(
                    &self.scope,
                    Some(&process),
                    at,
                    claim,
                    EvidenceStrength::Authoritative,
                );
                ids.push(record.evidence_id.clone());
                self.evidence.push(record);
            }
        }
        let seed = EventSeed {
            kind,
            subtype: None,
            scope: self.scope.clone(),
            subject: self.action.target.as_ref().map(|id| SpatialRef::Resolved {
                id: id.clone(),
                object_type: SpatialType::Process,
                label: Arc::from(self.action.operation.as_ref()),
            }),
            related: Vec::new(),
            times: times(Some(at), at),
            before: None,
            after: None,
            changed_fields: Vec::new(),
            evidence: ids,
            causal_parents: Vec::new(),
            payload: Some(self.payload()),
            provenance: provenance(),
        };
        self.events.push(seed.seal());
    }

    /// The action body §35.1 has no column for, which INTERFACES §2.2 puts in `payload`.
    fn payload(&self) -> Value {
        let mut map = ono_value::MapValue::new();
        map.insert(
            "action_id".into(),
            Value::string(self.action.action_id.as_str()),
        );
        map.insert("actor".into(), Value::string(&self.action.actor));
        map.insert("session_id".into(), Value::string(&self.action.session_id));
        map.insert(
            "command".into(),
            Value::string(self.action.command.as_str()),
        );
        map.insert("operation".into(), Value::string(&self.action.operation));
        map.insert(
            "authorization".into(),
            Value::string(self.action.authorization.decision.as_str()),
        );
        map.insert(
            "result".into(),
            self.action
                .result
                .as_ref()
                .map_or(Value::Null, |result| Value::string(result.outcome.as_str())),
        );
        map.insert(
            "external_transaction".into(),
            self.action
                .external_transaction
                .as_deref()
                .map_or(Value::Null, Value::string),
        );
        Value::Map(Arc::new(map))
    }

    /// Writes the lifecycle to `ledger`, events and evidence together (§3.4).
    ///
    /// # Errors
    ///
    /// Returns whatever §34 refusal the ledger raises.
    pub fn record(&self, ledger: &dyn LedgerWrite) -> Result<(), ono_value::ErrorValue> {
        ledger.append(&self.events, &self.evidence)?;
        ledger.record_action(&self.action)?;
        Ok(())
    }

    /// The events this lifecycle produced, for a caller that writes them itself.
    #[must_use]
    pub fn events(&self) -> &[TemporalEvent] {
        &self.events
    }
}

/// Turns `ono-spatial-events`' change output into canonical temporal events (§39.1, §6.1).
///
/// The observation time travels with the change: a change observed at an instant becomes an event
/// at that instant, and a change found by comparing two projections becomes an event whose
/// `source_time` is unknown and whose evidence claims the interval rather than a point. §9.2
/// forbids naming a moment inside an interval nothing was observed in, and this is where that
/// rule is kept rather than lost in translation.
#[must_use]
pub fn ingest_changes(
    scope: &SpatialScope,
    changes: &ChangeSet,
    ingested_at: Timestamp,
) -> (Vec<TemporalEvent>, Vec<Evidence>) {
    let mut events = Vec::new();
    let mut records = Vec::new();
    for change in changes.changes() {
        let Some(kind) = event_kind_of(change.kind()) else {
            continue;
        };
        let observed = change.observed();
        let observed_at = observed.latest().unwrap_or(ingested_at);
        let source_time = observed.instant();
        let subject = change.places().next().cloned();
        // A snapshot comparison is `derived`: §7.2 puts a value obtained by a documented rule
        // below one a source stated. A provider event is `asserted` — the source said so.
        let strength = match changes.source() {
            ono_spatial_events::ChangeSource::ProviderEvents => EvidenceStrength::Asserted,
            ono_spatial_events::ChangeSource::SnapshotComparison => EvidenceStrength::Derived,
        };
        let claim = claim_of(change.kind(), observed, observed_at);
        let record = evidence(scope, subject.as_ref(), observed_at, claim, strength);
        let seed = EventSeed {
            kind,
            subtype: Some(Arc::from(change.kind().as_str())),
            scope: scope.clone(),
            subject: subject.map(|id| SpatialRef::Resolved {
                id,
                object_type: SpatialType::System,
                label: Arc::from(change.label()),
            }),
            related: change
                .places()
                .skip(1)
                .map(|id| SpatialRef::Resolved {
                    id: id.clone(),
                    object_type: SpatialType::System,
                    label: Arc::from(change.label()),
                })
                .collect(),
            times: times(source_time, observed_at),
            before: None,
            after: None,
            changed_fields: Vec::new(),
            evidence: vec![record.evidence_id.clone()],
            causal_parents: Vec::new(),
            payload: None,
            provenance: provenance(),
        };
        events.push(seed.seal());
        records.push(record);
    }
    (events, records)
}

/// The §6.1 kind a v0.4 change kind is (§39.1).
///
/// A landmark appearing or disappearing is a judgement about relevance rather than an observation
/// of the system, so it becomes no event: §27.2 keeps landmarks out of the alerting business, and
/// an event nothing observed would be exactly the fabrication §55.1 warns about.
fn event_kind_of(kind: ChangeKind) -> Option<EventKind> {
    match kind {
        ChangeKind::NodeAppeared => Some(EventKind::ObjectAppeared),
        ChangeKind::NodeRemoved => Some(EventKind::ObjectDisappeared),
        ChangeKind::NodeChanged => Some(EventKind::ObjectChanged),
        ChangeKind::EdgeAppeared => Some(EventKind::RelationAdded),
        ChangeKind::EdgeRemoved => Some(EventKind::RelationRemoved),
        ChangeKind::LandmarkAppeared | ChangeKind::LandmarkRemoved => None,
    }
}

/// What the shell can claim about a change, at the precision the observation supports (§9.2).
fn claim_of(kind: ChangeKind, observed: ObservedAt, at: Timestamp) -> EvidenceClaim {
    match kind {
        ChangeKind::NodeAppeared | ChangeKind::NodeChanged => EvidenceClaim::ObjectExisted { at },
        ChangeKind::NodeRemoved => EvidenceClaim::ObjectAbsent {
            over: TimeRange {
                from: observed.earliest(),
                until: observed.latest(),
            },
        },
        _ => EvidenceClaim::ObjectExisted { at },
    }
}

/// Turns provider live events into canonical temporal events (§6.5, §21.3).
///
/// A provider event already carries `at`, its sequence and its changed fields, so nothing is
/// invented here: the sequence travels into `source_sequence`, which is what `happens_before`
/// answers from inside one clock domain (§26.1).
#[must_use]
pub fn ingest_provider_events(
    scope: &SpatialScope,
    events: &[ono_provider_api::ObjectEvent],
) -> (Vec<TemporalEvent>, Vec<Evidence>) {
    let mut built = Vec::new();
    let mut records = Vec::new();
    for event in events {
        let observed_at = event.at();
        let claim = EvidenceClaim::ObjectExisted { at: observed_at };
        let source = source_class(event.provenance().provider());
        let record = evidence_from(
            source.clone(),
            scope,
            None,
            observed_at,
            claim,
            EvidenceStrength::Asserted,
        );
        let mut event_times = times(Some(event.at()), observed_at);
        event_times.source_sequence = event.sequence();
        let seed = EventSeed {
            kind: EventKind::ProviderEvent,
            subtype: Some(Arc::from(event.kind().as_str())),
            scope: scope.clone(),
            subject: None,
            related: Vec::new(),
            times: event_times,
            before: None,
            after: None,
            changed_fields: Vec::new(),
            evidence: vec![record.evidence_id.clone()],
            causal_parents: Vec::new(),
            payload: event
                .value()
                .map(|record| Value::Record(Arc::clone(record))),
            provenance: provenance_of(&source),
        };
        built.push(seed.seal());
        records.push(record);
    }
    (built, records)
}

/// The redacted summary §17.5 requires of a command before it can reach the ledger.
///
/// Every word is [`Redactable::plain`] unless the caller knows it was a secret, and
/// `RedactedCommandSummary::of` applies §30.3's capture-group rule to the rest: `--password=` is
/// kept and its value is not.
#[must_use]
pub fn redact(verb: &str, target: Option<&str>, arguments: &[String]) -> RedactedCommandSummary {
    let words: Vec<Redactable> = arguments
        .iter()
        .map(|word| Redactable::plain(word))
        .collect();
    RedactedCommandSummary::of(verb, target, &words)
}
