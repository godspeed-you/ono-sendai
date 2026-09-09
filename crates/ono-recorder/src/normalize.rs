//! Turning what a provider reports into what the ledger holds (v0.5 §6, §22.1, §7.1).
//!
//! A provider speaks `ObjectEvent`: an object id, a schema, an instant, a sequence, a list of
//! changed field names. The ledger speaks [`TemporalEvent`]: a canonical kind, a spatial subject,
//! three separate timestamps, typed field changes and a provenance. This module is the one place
//! the first becomes the second, and three rules of the specification are enforced by its shape.
//!
//! - **The provenance names a §7.1 evidence source class.** `ono_temporal_reconstruct` reads
//!   `provenance.provider()` to attribute a reconstructed field to a source, and an unrecognised
//!   name falls back to `ono.session`, which would credit the shell with what procfs saw. So the
//!   provider field is the source class — `linux.procfs`, `linux.systemd-dbus` — and the
//!   provider's own id travels in the provenance source beside it.
//! - **A diffed event says `snapshot_diff`.** §22.1: the recorder "MAY create process
//!   appearance/disappearance events by comparing complete-enough snapshots, but provenance MUST
//!   say `snapshot_diff`". [`SNAPSHOT_DIFF`] is that word and [`Normalizer::from_snapshot_diff`]
//!   is the only path that sets it.
//! - **A relation event's body is `relation_payload`.** §6.4 makes `relation` and `confidence`
//!   required, `docs/contracts/temporal/events.yaml` states it, and `ono_temporal_core` owns both
//!   the writing and the reading, so an ingest path and a reconstruction cannot disagree.
//!
//! Redaction happens here rather than later. §30.3 requires secrets to be redacted "before
//! persistence", and the only way to be sure of that is for the unredacted record to have no path
//! into a [`TemporalEvent`] at all.

use std::sync::Arc;

use jiff::Timestamp;
use ono_provider_api::ObjectEvent;
use ono_spatial_core::{Confidence, SpatialId, SpatialScope, SpatialType};
use ono_temporal_core::{
    ChangeCertainty, ClockDomain, EventKind, EventSeed, EventTimes, FieldChange, GapReason,
    SpatialRef, TemporalCompleteness, TemporalCoverage, TemporalEvent, TemporalGap,
    relation_payload,
};
use ono_value::{MapValue, Provenance, RecordValue, SchemaId, Value};

use crate::redact::Redaction;
use crate::source::SourceProfile;

/// What §22.1 requires a provenance to say when the recorder derived the event itself.
pub const SNAPSHOT_DIFF: &str = "snapshot_diff";

/// The schema every temporal event's provenance is bound to.
fn event_schema() -> SchemaId {
    SchemaId::new("ono.temporal-event", 1)
}

/// A source's own name for an object, reconciled to the v0.4 identity (§5.1, §5.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSubject {
    /// The canonical identity the rest of the shell navigates by.
    pub id: SpatialId,
    /// What kind of object it is.
    pub object_type: SpatialType,
    /// What a person calls it.
    pub label: Arc<str>,
}

impl ResolvedSubject {
    /// A subject with these three parts.
    #[must_use]
    pub fn new(id: SpatialId, object_type: SpatialType, label: &str) -> Self {
        Self {
            id,
            object_type,
            label: Arc::from(label),
        }
    }

    /// The event subject this is.
    #[must_use]
    pub fn as_ref(&self) -> SpatialRef {
        SpatialRef::Resolved {
            id: self.id.clone(),
            object_type: self.object_type,
            label: Arc::clone(&self.label),
        }
    }
}

/// Reconciles a provider record to a v0.4 spatial identity (§5.1).
///
/// The recorder does not know how a process record becomes a `SpatialId` — that is v0.4's
/// identity model, and the shell already owns it. What the recorder owns is the consequence of
/// failing: §5.5 requires an event Ono could not reconcile to be "rendered as such rather than
/// attached to a guessed object", so a resolver that answers `None` produces an
/// [`SpatialRef::Unresolved`] subject and never a plausible one.
pub trait SubjectResolver: Send + Sync + std::fmt::Debug {
    /// The canonical identity of the object this record describes, or `None`.
    fn resolve(&self, record: &RecordValue) -> Option<ResolvedSubject>;
}

/// A resolver that reconciles nothing, which is what §5.5 says to do when nothing can be.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnresolvedSubjects;

impl SubjectResolver for UnresolvedSubjects {
    fn resolve(&self, _record: &RecordValue) -> Option<ResolvedSubject> {
        None
    }
}

/// Turns one source's reports into canonical temporal events (§6).
#[derive(Debug, Clone)]
pub struct Normalizer {
    profile: SourceProfile,
    scope: SpatialScope,
    domain: ClockDomain,
    resolver: Arc<dyn SubjectResolver>,
    redaction: Redaction,
}

impl Normalizer {
    /// A normalizer for `profile`, recording about `scope` in `domain`.
    #[must_use]
    pub fn new(profile: SourceProfile, scope: SpatialScope, domain: ClockDomain) -> Self {
        Self {
            profile,
            scope,
            domain,
            resolver: Arc::new(UnresolvedSubjects),
            redaction: Redaction::default(),
        }
    }

    /// The same normalizer reconciling subjects through `resolver` (§5.1).
    #[must_use]
    pub fn with_resolver(mut self, resolver: Arc<dyn SubjectResolver>) -> Self {
        self.resolver = resolver;
        self
    }

    /// The same normalizer applying `redaction` before anything is persisted (§30.3).
    #[must_use]
    pub fn with_redaction(mut self, redaction: Redaction) -> Self {
        self.redaction = redaction;
        self
    }

    /// The source this normalizer speaks for.
    #[must_use]
    pub const fn profile(&self) -> &SourceProfile {
        &self.profile
    }

    /// The v0.4 boundary its events belong to.
    #[must_use]
    pub const fn scope(&self) -> &SpatialScope {
        &self.scope
    }

    /// The clock domain its events are numbered in (§25.5).
    #[must_use]
    pub const fn domain(&self) -> &ClockDomain {
        &self.domain
    }

    /// One event as a provider reported it (§6.1, §21.3).
    #[must_use]
    pub fn from_provider(&self, observed: &ObjectEvent, ingested_at: Timestamp) -> TemporalEvent {
        let record = observed.value().map(|value| self.redaction.record(value));
        let kind = kind_of(observed.kind());
        let changed = observed.changed_fields().map_or_else(Vec::new, |fields| {
            fields
                .iter()
                .map(|field| self.field_change(field, record.as_ref()))
                .collect()
        });
        self.seal(
            kind,
            self.subject_of(record.as_ref(), observed),
            Vec::new(),
            self.times(observed.at(), ingested_at, observed.sequence()),
            record.map(|record| Value::Record(Arc::new(record))),
            changed,
            None,
            self.provenance(observed.at(), observed.provenance().source()),
        )
    }

    /// One event the recorder derived by comparing two snapshots (§22.1).
    ///
    /// The provenance says `snapshot_diff`, which is what §22.1 requires and what stops a derived
    /// appearance being read as a source's own report of one.
    #[must_use]
    pub fn from_snapshot_diff(
        &self,
        kind: EventKind,
        record: &RecordValue,
        at: Timestamp,
        ingested_at: Timestamp,
    ) -> TemporalEvent {
        let record = self.redaction.record(record);
        let subject = self.subject_of_record(&record);
        self.seal(
            kind,
            Some(subject),
            Vec::new(),
            self.times(at, ingested_at, None),
            Some(Value::Record(Arc::new(record))),
            Vec::new(),
            None,
            self.provenance(at, Some(SNAPSHOT_DIFF)),
        )
    }

    /// A relation event, with the body §6.4 requires (§6.4, §9.5).
    #[must_use]
    pub fn relation(
        &self,
        added: bool,
        from: &ResolvedSubject,
        to: &ResolvedSubject,
        relation: &str,
        confidence: Confidence,
        at: Timestamp,
        ingested_at: Timestamp,
    ) -> TemporalEvent {
        let kind = if added {
            EventKind::RelationAdded
        } else {
            EventKind::RelationRemoved
        };
        self.seal(
            kind,
            Some(from.as_ref()),
            vec![to.as_ref()],
            self.times(at, ingested_at, None),
            None,
            Vec::new(),
            Some(relation_payload(relation, confidence)),
            self.provenance(at, None),
        )
    }

    /// The `coverage.started` marker that makes a stored interval distinguishable from an
    /// unwatched one (§8.1, §10.6).
    #[must_use]
    pub fn coverage_started(
        &self,
        coverage: &TemporalCoverage,
        ingested_at: Timestamp,
    ) -> TemporalEvent {
        self.seal(
            EventKind::CoverageStarted,
            None,
            Vec::new(),
            self.times(coverage.from, ingested_at, None),
            None,
            Vec::new(),
            Some(coverage_payload(
                &coverage.capability,
                coverage.completeness,
                None,
            )),
            self.provenance(coverage.from, None),
        )
    }

    /// The `coverage.ended` marker a gap begins at (§7.5, §43.2).
    ///
    /// The whole gap rides in the payload, because `ono.temporal-event/1` has no column for a
    /// capability, a reason or an interval, and a marker that carried only its instant would make
    /// the ledger unable to answer what §11.7 has to draw.
    #[must_use]
    pub fn coverage_ended(&self, gap: &TemporalGap, ingested_at: Timestamp) -> TemporalEvent {
        self.seal(
            EventKind::CoverageEnded,
            None,
            Vec::new(),
            self.times(gap.from, ingested_at, None),
            None,
            Vec::new(),
            Some(gap_payload(gap)),
            self.provenance(gap.from, None),
        )
    }

    /// The three instants of §3.3, and the ordering evidence §25.4 keeps beside them.
    fn times(
        &self,
        source_time: Timestamp,
        ingested_at: Timestamp,
        sequence: Option<u64>,
    ) -> EventTimes {
        EventTimes {
            source_time: Some(source_time),
            observed_at: source_time,
            ingested_at,
            source_sequence: sequence,
            monotonic_nanos: None,
            clock_uncertainty: None,
            domain: self.domain.clone(),
        }
    }

    /// The provenance an event carries (§7.1).
    ///
    /// `provider` is the §7.1 source class, because field-level source attribution reads it.
    fn provenance(&self, at: Timestamp, source: Option<&str>) -> Provenance {
        let provenance =
            Provenance::local(self.profile.source.as_str(), event_schema()).observed_at(at);
        match source {
            Some(source) => provenance.from_source(source),
            None => provenance.from_source(&self.profile.provider),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn seal(
        &self,
        kind: EventKind,
        subject: Option<SpatialRef>,
        related: Vec<SpatialRef>,
        times: EventTimes,
        after: Option<Value>,
        changed_fields: Vec<FieldChange>,
        payload: Option<Value>,
        provenance: Provenance,
    ) -> TemporalEvent {
        EventSeed {
            kind,
            subtype: None,
            scope: self.scope.clone(),
            subject,
            related,
            times,
            before: None,
            after,
            changed_fields,
            evidence: Vec::new(),
            causal_parents: Vec::new(),
            payload,
            provenance,
        }
        .seal()
    }

    fn field_change(&self, field: &str, record: Option<&RecordValue>) -> FieldChange {
        FieldChange {
            field: Arc::from(field),
            before: None,
            after: record.and_then(|record| record.get(field).cloned()),
            certainty: ChangeCertainty::Unknown,
        }
    }

    fn subject_of(
        &self,
        record: Option<&RecordValue>,
        observed: &ObjectEvent,
    ) -> Option<SpatialRef> {
        match record {
            Some(record) => Some(self.subject_of_record(record)),
            None => Some(SpatialRef::Unresolved {
                source: self.profile.source.clone(),
                described: Arc::from(describe(observed)),
            }),
        }
    }

    fn subject_of_record(&self, record: &RecordValue) -> SpatialRef {
        self.resolver.resolve(record).map_or_else(
            || SpatialRef::Unresolved {
                source: self.profile.source.clone(),
                described: Arc::from(describe_record(record)),
            },
            |subject| subject.as_ref(),
        )
    }
}

/// The canonical kind a provider's event kind maps to (§6.1).
#[must_use]
pub const fn kind_of(kind: ono_provider_api::EventKind) -> EventKind {
    match kind {
        ono_provider_api::EventKind::Snapshot => EventKind::ObjectObserved,
        ono_provider_api::EventKind::Added => EventKind::ObjectAppeared,
        ono_provider_api::EventKind::Changed => EventKind::ObjectChanged,
        ono_provider_api::EventKind::Removed => EventKind::ObjectDisappeared,
    }
}

/// The body a coverage marker carries, since `ono.temporal-event/1` has no column for it.
fn coverage_payload(
    capability: &str,
    completeness: TemporalCompleteness,
    reason: Option<&str>,
) -> Value {
    let mut map = MapValue::new();
    map.insert("capability".into(), Value::string(capability));
    map.insert("completeness".into(), Value::string(completeness.as_str()));
    map.insert("reason".into(), reason.map_or(Value::Null, Value::string));
    Value::Map(Arc::new(map))
}

/// The body a `coverage.ended` marker carries, so the ledger holds the whole gap (§7.5, §43.2).
///
/// [`gap_of`] reads it back. The two are here together for the same reason `relation_payload` and
/// `relation_of` are together in `ono-temporal-core`: one spelling, and no way for a writer and a
/// reader to drift apart.
#[must_use]
pub fn gap_payload(gap: &TemporalGap) -> Value {
    let mut map = MapValue::new();
    map.insert("capability".into(), Value::string(&gap.capability));
    map.insert(
        "completeness".into(),
        Value::string(TemporalCompleteness::Unavailable.as_str()),
    );
    map.insert("reason".into(), Value::string(gap.reason.as_str()));
    map.insert(
        "detail".into(),
        gap.detail.as_deref().map_or(Value::Null, Value::string),
    );
    map.insert("from".into(), Value::Timestamp(gap.from));
    map.insert("until".into(), Value::Timestamp(gap.until));
    map.insert("source".into(), Value::string(gap.source.as_str()));
    Value::Map(Arc::new(map))
}

/// The gap a `coverage.ended` marker names, or `None` where it names none.
#[must_use]
pub fn gap_of(event: &TemporalEvent) -> Option<TemporalGap> {
    if event.kind != EventKind::CoverageEnded {
        return None;
    }
    let payload = event.payload.as_ref()?.as_map().ok()?;
    let Value::String(capability) = payload.get("capability")? else {
        return None;
    };
    let reason = match payload.get("reason") {
        Some(Value::String(name)) => {
            ono_temporal_core::GapReason::from_name(name).unwrap_or(GapReason::NotRecorded)
        }
        _ => GapReason::NotRecorded,
    };
    let source = match payload.get("source") {
        Some(Value::String(name)) => ono_temporal_core::EvidenceSource::parse(name)
            .unwrap_or_else(ono_temporal_core::EvidenceSource::recorder),
        _ => ono_temporal_core::EvidenceSource::recorder(),
    };
    let from = match payload.get("from") {
        Some(Value::Timestamp(at)) => *at,
        _ => event.times.presentation_instant(),
    };
    let until = match payload.get("until") {
        Some(Value::Timestamp(at)) => *at,
        _ => from,
    };
    let detail = match payload.get("detail") {
        Some(Value::String(text)) => Some(Arc::clone(text)),
        _ => None,
    };
    Some(TemporalGap {
        scope: event.scope.clone(),
        from,
        until,
        capability: Arc::clone(capability),
        reason,
        source,
        detail,
    })
}

/// What a source called an object it could not be reconciled for (§5.5).
fn describe(observed: &ObjectEvent) -> String {
    let values: Vec<String> = observed
        .object_id()
        .values()
        .iter()
        .map(ToString::to_string)
        .collect();
    format!("{} {}", observed.schema().name(), values.join("/"))
}

fn describe_record(record: &RecordValue) -> String {
    let identity = record.identity();
    let values: Vec<String> = identity
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    format!("{} {}", record.schema().name(), values.join(" "))
}
