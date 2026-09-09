//! Fixtures shared by the crate's outcome tests.
//!
//! Every instant is fixed text, every scope is one host on one boot, and nothing here reads a
//! clock: v0.5 §39.2 keeps the clock out of this crate, and a fixture that read one would hide
//! the fact that the engine does not.

#![allow(
    dead_code,
    reason = "each test binary uses the part of the fixture its own subject needs"
)]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::sync::Arc;

use jiff::Timestamp;
use ono_spatial_core::{
    BootIdentity, Confidence, PermissionState, Projection, SpatialId, SpatialScope, SpatialType,
};
use ono_temporal_core::{
    Checkpoint, CheckpointId, ClockDomain, EventKind, EventSeed, EventTimes, EvidenceSource,
    FieldChange, ObjectState, RelationState, SpatialRef, TemporalCompleteness, TemporalCoverage,
    TemporalEvent,
};
use ono_value::{Provenance, RecordValue, SchemaId, Value, builtin_schemas};

/// The boot every fixture object belongs to.
#[must_use]
pub fn boot() -> BootIdentity {
    BootIdentity::new("testbox", "4d0a1f2b-0000-4000-8000-000000000001")
}

/// The one host scope the fixtures live in.
#[must_use]
pub fn scope() -> SpatialScope {
    SpatialScope::host("testbox", boot())
}

/// Another host, for the partitioning rule of §42.1.
#[must_use]
pub fn other_scope() -> SpatialScope {
    SpatialScope::host(
        "web01",
        BootIdentity::new("web01", "9f0a1f2b-0000-4000-8000-000000000002"),
    )
}

#[must_use]
pub fn instant(text: &str) -> Timestamp {
    text.parse().expect("the fixture instant parses")
}

#[must_use]
pub fn provenance(provider: &str) -> Provenance {
    Provenance::local(provider, SchemaId::new("ono.temporal-event", 1))
}

#[must_use]
pub fn recorder() -> EvidenceSource {
    EvidenceSource::recorder()
}

#[must_use]
pub fn procfs() -> EvidenceSource {
    EvidenceSource::parse("linux.procfs").expect("a §7.1 source class")
}

#[must_use]
pub fn systemd() -> EvidenceSource {
    EvidenceSource::parse("linux.systemd-dbus").expect("a §7.1 source class")
}

/// A record of `schema`, with the named fields set.
#[must_use]
pub fn record(schema_name: &str, fields: &[(&str, Value)]) -> RecordValue {
    let id = SchemaId::new(schema_name, 1);
    let schema = builtin_schemas()
        .get(&id)
        .expect("the contract is embedded");
    let mut builder = RecordValue::builder(schema, Provenance::local("fixture", id));
    for (name, value) in fields {
        builder = builder.set(name, value.clone()).expect("a declared field");
    }
    builder.build()
}

/// An `ono.process/1` record. `started` is what separates one lifetime from a later pid reuse.
#[must_use]
pub fn process_record(pid: i64, name: &str, started: &str, state: &str) -> RecordValue {
    record(
        "ono.process",
        &[
            ("pid", Value::Int(i128::from(pid))),
            ("name", Value::string(name)),
            ("started", Value::Timestamp(instant(started))),
            ("state", Value::string(state)),
        ],
    )
}

/// An `ono.service/1` record — one conceptual identity across many process lifetimes (§5.3).
#[must_use]
pub fn service_record(name: &str, state: &str, pid: Option<i64>) -> RecordValue {
    record(
        "ono.service",
        &[
            ("name", Value::string(name)),
            ("state", Value::string(state)),
            ("provider", Value::string("systemd")),
            (
                "pid",
                pid.map_or(Value::Null, |pid| Value::Int(i128::from(pid))),
            ),
        ],
    )
}

/// An `ono.file/1` record, for §14.5's refusal.
#[must_use]
pub fn file_record(path: &str) -> RecordValue {
    record(
        "ono.file",
        &[
            ("path", Value::Path(std::path::Path::new(path).into())),
            (
                "name",
                Value::string(path.rsplit('/').next().unwrap_or(path)),
            ),
            ("kind", Value::string("dir")),
        ],
    )
}

/// The `SpatialId` a record projects to in the fixture scope — the identity it had when it was
/// live, which is the identity a reconstruction has to give it back (§5.1).
#[must_use]
pub fn identity_of(record: &RecordValue, object_type: SpatialType, at: &str) -> SpatialId {
    // `ono.file/1` declares no identity — a path is a reference, not an identity, because hard
    // links share one inode — so a filesystem place is a derived place the way v0.4 §42.3 makes
    // one, and everything else projects from its own record.
    if matches!(object_type, SpatialType::File | SpatialType::Directory) {
        let path = record
            .get("path")
            .and_then(|value| ono_value::canonical_text(value).ok())
            .unwrap_or_default();
        return Projection::new(scope(), instant(at))
            .derive(
                object_type,
                SchemaId::new("ono.file", 1),
                "path",
                &path,
                provenance("fixture"),
            )
            .spatial_id()
            .clone();
    }
    Projection::new(scope(), instant(at))
        .project_as(record, object_type)
        .expect("the fixture record carries an identity")
        .spatial_id()
        .clone()
}

/// An object as one source observed it, ready for a checkpoint.
#[must_use]
pub fn object_state(
    record: &RecordValue,
    object_type: SpatialType,
    label: &str,
    observed_at: &str,
    source: EvidenceSource,
) -> ObjectState {
    ObjectState {
        id: identity_of(record, object_type, observed_at),
        object_type,
        label: Arc::from(label),
        record: record.clone(),
        observed_at: instant(observed_at),
        source,
    }
}

#[must_use]
pub fn relation_state(
    from: &SpatialId,
    to: &SpatialId,
    relation: &str,
    observed_at: &str,
    source: EvidenceSource,
) -> RelationState {
    RelationState {
        from: from.clone(),
        to: to.clone(),
        relation: Arc::from(relation),
        confidence: Confidence::Exact,
        observed_at: instant(observed_at),
        source,
    }
}

/// One source's coverage claim over one interval.
#[must_use]
pub fn coverage(
    capability: &str,
    from: &str,
    until: &str,
    completeness: TemporalCompleteness,
    source: EvidenceSource,
) -> TemporalCoverage {
    TemporalCoverage {
        scope: scope(),
        capability: Arc::from(capability),
        from: instant(from),
        until: instant(until),
        completeness,
        sampling_interval: None,
        source,
        permission: PermissionState::Available,
    }
}

/// A snapshot at one instant and nothing more (§8.4).
#[must_use]
pub fn point_sample(capability: &str, at: &str, source: EvidenceSource) -> TemporalCoverage {
    coverage(
        capability,
        at,
        at,
        TemporalCompleteness::PointSample,
        source,
    )
}

#[must_use]
pub fn checkpoint(
    scope: SpatialScope,
    captured_at: &str,
    objects: Vec<ObjectState>,
    relations: Vec<RelationState>,
    coverage: Vec<TemporalCoverage>,
) -> Checkpoint {
    Checkpoint {
        checkpoint_id: CheckpointId::of(&scope, instant(captured_at)),
        scope,
        captured_at: instant(captured_at),
        coverage,
        objects,
        relations,
        provenance: provenance("ono.recorder"),
    }
}

#[must_use]
pub fn domain() -> ClockDomain {
    ClockDomain::new("testbox", Some("4d0a1f2b-0000-4000-8000-000000000001"))
}

#[must_use]
pub fn times(observed: &str) -> EventTimes {
    EventTimes {
        source_time: Some(instant(observed)),
        observed_at: instant(observed),
        ingested_at: instant(observed),
        source_sequence: None,
        monotonic_nanos: None,
        clock_uncertainty: None,
        domain: domain(),
    }
}

/// The same instant, with the source's own sequence number — the evidence §26.1 orders by.
#[must_use]
pub fn sequenced(observed: &str, sequence: u64) -> EventTimes {
    EventTimes {
        source_sequence: Some(sequence),
        ..times(observed)
    }
}

#[must_use]
pub fn resolved(id: &SpatialId, object_type: SpatialType, label: &str) -> SpatialRef {
    SpatialRef::Resolved {
        id: id.clone(),
        object_type,
        label: Arc::from(label),
    }
}

/// An event about one subject.
#[must_use]
pub fn event(kind: EventKind, times: EventTimes, subject: SpatialRef) -> TemporalEvent {
    EventSeed {
        kind,
        subtype: None,
        scope: scope(),
        subject: Some(subject),
        related: Vec::new(),
        times,
        before: None,
        after: None,
        changed_fields: Vec::new(),
        evidence: Vec::new(),
        causal_parents: Vec::new(),
        payload: None,
        provenance: provenance("ono.recorder"),
    }
    .seal()
}

/// A field change event: the canonical `object.changed` of §6.2.
#[must_use]
pub fn changed(
    at: &str,
    subject: SpatialRef,
    field: &str,
    before: Value,
    after: Value,
) -> TemporalEvent {
    let mut built = event(EventKind::ObjectChanged, times(at), subject);
    built.changed_fields = vec![FieldChange {
        field: Arc::from(field),
        before: Some(before),
        after: Some(after),
        certainty: ono_temporal_core::ChangeCertainty::Observed,
    }];
    EventSeed {
        kind: built.kind,
        subtype: built.subtype,
        scope: built.scope,
        subject: built.subject,
        related: built.related,
        times: built.times,
        before: built.before,
        after: built.after,
        changed_fields: built.changed_fields,
        evidence: built.evidence,
        causal_parents: built.causal_parents,
        payload: built.payload,
        provenance: built.provenance,
    }
    .seal()
}

/// A relation event, whose two ends are the subject and the one related reference (§6.4).
#[must_use]
pub fn relation_event(
    kind: EventKind,
    at: &str,
    from: SpatialRef,
    to: SpatialRef,
    relation: &str,
) -> TemporalEvent {
    EventSeed {
        kind,
        subtype: None,
        scope: scope(),
        subject: Some(from),
        related: vec![to],
        times: times(at),
        before: None,
        after: None,
        changed_fields: Vec::new(),
        evidence: Vec::new(),
        causal_parents: Vec::new(),
        payload: Some(ono_temporal_core::relation_payload(
            relation,
            Confidence::Exact,
        )),
        provenance: provenance("ono.recorder"),
    }
    .seal()
}
