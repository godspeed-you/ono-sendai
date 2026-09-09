//! Fixtures shared by the ledger's outcome tests: a fixed clock domain, a fixed scope and the
//! builders every test needs, so each test states the one fact it is about.

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

use std::path::{Path, PathBuf};
use std::sync::Arc;

use jiff::Timestamp;
use ono_spatial_core::{
    BootIdentity, Confidence, PermissionState, SpatialId, SpatialIdentity, SpatialScope,
    SpatialType,
};
use ono_temporal_core::{
    ActionEvent, ActionId, AuthorizationDecision, AuthorizationSummary, CausalLink, CausalLinkId,
    CausalRelation, CausalRuleId, Checkpoint, CheckpointId, ClockDomain, EventKind, EventSeed,
    EventTimes, Evidence, EvidenceClaim, EvidenceId, EvidenceSource, EvidenceStrength, ObjectState,
    RedactedCommandSummary, RelationState, TemporalCompleteness, TemporalCoverage, TemporalEvent,
};
use ono_temporal_ledger::{LedgerStore, StoreOptions};
use ono_value::{Provenance, RecordValue, SchemaId, Value, builtin_schemas};

pub fn boot() -> BootIdentity {
    BootIdentity::new("testbox", "4d0a1f2b-0000-4000-8000-000000000001")
}

pub fn scope() -> SpatialScope {
    SpatialScope::host("testbox", boot())
}

pub fn nested_scope() -> SpatialScope {
    scope().nest(ono_spatial_core::ScopeKind::Container, "payments-api")
}

pub fn instant(text: &str) -> Timestamp {
    text.parse().expect("the fixture instant parses")
}

pub fn subject(pid: i64) -> SpatialId {
    SpatialIdentity::lifetime(SpatialType::Process, [("pid", pid.to_string())]).spatial_id()
}

pub fn provenance(provider: &str) -> Provenance {
    Provenance::local(provider, SchemaId::new("ono.temporal-event", 1))
}

pub fn domain(host: &str, boot_id: Option<&str>) -> ClockDomain {
    ClockDomain {
        host: Arc::from(host),
        boot_id: boot_id.map(Arc::from),
    }
}

pub fn times(observed: &str) -> EventTimes {
    EventTimes {
        source_time: None,
        observed_at: instant(observed),
        ingested_at: instant(observed),
        source_sequence: None,
        monotonic_nanos: None,
        clock_uncertainty: None,
        domain: domain("testbox", Some("4d0a1f2b")),
    }
}

pub fn sequenced(observed: &str, sequence: u64) -> EventTimes {
    EventTimes {
        source_sequence: Some(sequence),
        ..times(observed)
    }
}

pub fn seed(kind: EventKind, times: EventTimes, provider: &str) -> EventSeed {
    EventSeed {
        kind,
        subtype: None,
        scope: scope(),
        subject: None,
        related: Vec::new(),
        times,
        before: None,
        after: None,
        changed_fields: Vec::new(),
        evidence: Vec::new(),
        causal_parents: Vec::new(),
        payload: None,
        provenance: provenance(provider),
    }
}

pub fn event(kind: EventKind, observed: &str, provider: &str) -> TemporalEvent {
    seed(kind, times(observed), provider).seal()
}

/// An event about `pid`, so subject filtering has something to filter on.
pub fn event_about(kind: EventKind, observed: &str, pid: i64) -> TemporalEvent {
    let mut seed = seed(kind, times(observed), "linux.procfs");
    seed.subject = Some(ono_temporal_core::SpatialRef::Resolved {
        id: subject(pid),
        object_type: SpatialType::Process,
        label: Arc::from(format!("pid {pid}")),
    });
    seed.seal()
}

pub fn evidence_for(observed: &str, pid: i64, value: &str) -> Evidence {
    let source = EvidenceSource::recorder();
    let observed_at = instant(observed);
    let claim = EvidenceClaim::FieldValue {
        field: Arc::from("active_state"),
        value: Value::string(value),
        at: observed_at,
    };
    Evidence {
        evidence_id: EvidenceId::of(&source, observed_at, &scope(), Some(&subject(pid)), &claim),
        source,
        observed_at,
        source_time: None,
        scope: scope(),
        subject: Some(subject(pid)),
        claim,
        strength: EvidenceStrength::Authoritative,
        raw_ref: None,
        derived_from: Vec::new(),
        provenance: provenance("ono.recorder"),
    }
}

pub fn link(cause: &TemporalEvent, effect: &TemporalEvent, evidence: &Evidence) -> CausalLink {
    CausalLink {
        link_id: CausalLinkId::of(
            &CausalRuleId::new("ono.action-to-job"),
            CausalRelation::CausedBy,
            &cause.event_id,
            &effect.event_id,
        ),
        relation: CausalRelation::CausedBy,
        cause: cause.event_id.clone(),
        effect: effect.event_id.clone(),
        rule: CausalRuleId::new("ono.action-to-job"),
        evidence: vec![evidence.evidence_id.clone()],
        strength: EvidenceStrength::Authoritative,
        source: EvidenceSource::recorder(),
    }
}

pub fn coverage(from: &str, until: &str) -> TemporalCoverage {
    TemporalCoverage {
        scope: scope(),
        capability: Arc::from("process.existence"),
        from: instant(from),
        until: instant(until),
        completeness: TemporalCompleteness::Complete,
        sampling_interval: Some(ono_value::Duration::from_nanoseconds(1_000_000_000)),
        source: EvidenceSource::recorder(),
        permission: PermissionState::Available,
    }
}

pub fn action(at: &str, command: RedactedCommandSummary) -> ActionEvent {
    let requested_at = instant(at);
    ActionEvent {
        action_id: ActionId::of("session-1", requested_at, "restart", Some(&subject(1827))),
        command,
        actor: Arc::from("case"),
        session_id: Arc::from("session-1"),
        requested_at,
        target: Some(subject(1827)),
        operation: Arc::from("restart"),
        authorization: AuthorizationSummary {
            decision: AuthorizationDecision::Confirmed,
            risk: Arc::from("service_affecting"),
            capability: Some(Arc::from("service.manage")),
            reason: Some(Arc::from("a service restart interrupts its clients")),
        },
        result: None,
        external_transaction: Some(Arc::from("systemd:/org/freedesktop/systemd1/job/4821")),
        provenance: provenance("ono.session"),
    }
}

pub fn object_record(pid: i64) -> RecordValue {
    let schema = builtin_schemas()
        .get(&SchemaId::new("ono.process", 1))
        .expect("the process contract ships with the shell");
    RecordValue::builder(
        Arc::clone(&schema),
        Provenance::local("linux.procfs", schema.id().clone()),
    )
    .set("pid", Value::Int(i128::from(pid)))
    .expect("pid is a declared field")
    .build()
}

pub fn checkpoint(at: &str) -> Checkpoint {
    let captured_at = instant(at);
    Checkpoint {
        checkpoint_id: CheckpointId::of(&scope(), captured_at),
        scope: scope(),
        captured_at,
        coverage: vec![coverage(at, at)],
        objects: vec![ObjectState {
            id: subject(1827),
            object_type: SpatialType::Process,
            label: Arc::from("nginx"),
            record: object_record(1827),
            observed_at: captured_at,
            source: EvidenceSource::builtin("linux.procfs").expect("a built-in source class"),
        }],
        relations: vec![RelationState {
            from: subject(1827),
            to: subject(1828),
            relation: Arc::from("process.owns_socket"),
            confidence: Confidence::Exact,
            observed_at: captured_at,
            source: EvidenceSource::builtin("linux.procfs").expect("a built-in source class"),
        }],
        provenance: provenance("ono.recorder"),
    }
}

/// A store in a fresh private directory under `home`, opened the way the recorder opens it.
pub fn store_in(home: &Path) -> LedgerStore {
    let path = ono_temporal_ledger::ledger_path(|_| None, Some(home)).expect("a home has a store");
    LedgerStore::open(&path).expect("a fresh store opens")
}

/// The canonical path under `home`, without opening anything.
pub fn path_in(home: &Path) -> PathBuf {
    ono_temporal_ledger::ledger_path(|_| None, Some(home)).expect("a home has a store")
}

pub fn options(path: &Path) -> StoreOptions {
    StoreOptions::at(path)
}
