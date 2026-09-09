//! Fixtures shared by this crate's outcome tests.
//!
//! Every test in the package answers a question about a ledger somebody populated by hand, so
//! the builders here exist to let one test state one fact. Nothing reads a clock: the instants
//! are literals and the ledger is `ono_temporal_core::SessionLedger`, which satisfies the same
//! `LedgerRead` contract as the persistent store (§10.7, §39).

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
    BootIdentity, PermissionState, SpatialId, SpatialIdentity, SpatialScope, SpatialType,
};
use ono_temporal_core::{
    ChangeCertainty, ClockDomain, EventKind, EventSeed, EventTimes, EvidenceSource, FieldChange,
    LedgerWrite, SessionLedger, SpatialRef, TemporalCompleteness, TemporalCoverage, TemporalEvent,
};
use ono_value::{MapValue, Provenance, SchemaId, Value};

/// The boot every fixture identity is keyed on.
pub fn boot() -> BootIdentity {
    BootIdentity::new("testbox", "4d0a1f2b-0000-4000-8000-000000000001")
}

/// The host scope every fixture event belongs to.
pub fn scope() -> SpatialScope {
    SpatialScope::host("testbox", boot())
}

/// A literal instant. Time is a parameter in this crate, so a test spells its own (§39.2).
pub fn instant(text: &str) -> Timestamp {
    text.parse().expect("the fixture instant parses")
}

/// A lifetime-bound identity for an object of `object_type` called `name`.
pub fn id(object_type: SpatialType, name: &str) -> SpatialId {
    SpatialIdentity::lifetime(object_type, [("name", name.to_owned())]).spatial_id()
}

/// A resolved event subject.
pub fn subject(object_type: SpatialType, name: &str) -> SpatialRef {
    SpatialRef::Resolved {
        id: id(object_type, name),
        object_type,
        label: Arc::from(name),
    }
}

/// A subject a source named and nothing could reconcile (§5.5).
pub fn unresolved(described: &str) -> SpatialRef {
    SpatialRef::Unresolved {
        source: EvidenceSource::recorder(),
        described: Arc::from(described),
    }
}

/// Provenance naming the source that reported the event.
pub fn provenance(provider: &str) -> Provenance {
    Provenance::local(provider, SchemaId::new("ono.temporal-event", 1))
}

/// The clock domain every fixture event is timed in.
pub fn domain() -> ClockDomain {
    ClockDomain {
        host: Arc::from("testbox"),
        boot_id: Some(Arc::from("4d0a1f2b")),
    }
}

/// Times whose source instant and observation instant are both `at`.
pub fn times(at: &str) -> EventTimes {
    let at = instant(at);
    EventTimes {
        source_time: Some(at),
        observed_at: at,
        ingested_at: at,
        source_sequence: None,
        monotonic_nanos: None,
        clock_uncertainty: None,
        domain: domain(),
    }
}

/// An event of `kind` about `subject` at `at`, reported by `provider`.
pub fn event(
    kind: EventKind,
    at: &str,
    provider: &str,
    subject: Option<SpatialRef>,
) -> TemporalEvent {
    EventSeed {
        kind,
        subtype: None,
        scope: scope(),
        subject,
        related: Vec::new(),
        times: times(at),
        before: None,
        after: None,
        changed_fields: Vec::new(),
        evidence: Vec::new(),
        causal_parents: Vec::new(),
        payload: None,
        provenance: provenance(provider),
    }
    .seal()
}

/// An `object.changed` event carrying one observed field change.
pub fn changed(
    at: &str,
    provider: &str,
    subject: SpatialRef,
    field: &str,
    before: Option<&str>,
    after: Option<&str>,
) -> TemporalEvent {
    let certainty = match (before, after) {
        (Some(_), Some(_)) => ChangeCertainty::Observed,
        _ => ChangeCertainty::Unknown,
    };
    EventSeed {
        kind: EventKind::ObjectChanged,
        subtype: None,
        scope: scope(),
        subject: Some(subject),
        related: Vec::new(),
        times: times(at),
        before: before.map(Value::string),
        after: after.map(Value::string),
        changed_fields: vec![FieldChange {
            field: Arc::from(field),
            before: before.map(Value::string),
            after: after.map(Value::string),
            certainty,
        }],
        evidence: Vec::new(),
        causal_parents: Vec::new(),
        payload: None,
        provenance: provenance(provider),
    }
    .seal()
}

/// A relation event between `from` and `to`, carrying §6.4's relation payload.
pub fn relation(
    kind: EventKind,
    at: &str,
    provider: &str,
    from: SpatialRef,
    to: SpatialRef,
    relation: &str,
) -> TemporalEvent {
    let mut payload = MapValue::new();
    payload.insert("relation".into(), Value::string(relation));
    payload.insert("confidence".into(), Value::string("observed"));
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
        payload: Some(Value::Map(Arc::new(payload))),
        provenance: provenance(provider),
    }
    .seal()
}

/// A ledger holding `events`.
pub fn ledger(events: &[TemporalEvent]) -> SessionLedger {
    let held = SessionLedger::new();
    held.append(events, &[])
        .expect("the session ledger appends");
    held
}

/// A coverage interval for `capability` over `[from, until]`.
pub fn coverage(
    capability: &str,
    from: &str,
    until: &str,
    completeness: TemporalCompleteness,
) -> TemporalCoverage {
    TemporalCoverage {
        scope: scope(),
        capability: Arc::from(capability),
        from: instant(from),
        until: instant(until),
        completeness,
        sampling_interval: None,
        source: EvidenceSource::recorder(),
        permission: PermissionState::Available,
    }
}
