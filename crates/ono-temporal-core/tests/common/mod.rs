//! Fixtures shared by the crate's outcome tests: a fixed clock domain, a fixed scope and the
//! two builders every test needs, so a test states the one fact it is about.

#![allow(
    dead_code,
    reason = "each test binary uses the part of the fixture its own subject needs"
)]

use std::sync::Arc;

use jiff::Timestamp;
use ono_spatial_core::{BootIdentity, SpatialId, SpatialIdentity, SpatialScope, SpatialType};
use ono_temporal_core::{
    ClockDomain, EventKind, EventSeed, EventTimes, EvidenceSource, TemporalEvent,
};
use ono_value::{Provenance, SchemaId};

pub fn boot() -> BootIdentity {
    BootIdentity::new("testbox", "4d0a1f2b-0000-4000-8000-000000000001")
}

pub fn scope() -> SpatialScope {
    SpatialScope::host("testbox", boot())
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

pub fn source() -> EvidenceSource {
    EvidenceSource::recorder()
}

pub fn evidence() -> ono_temporal_core::Evidence {
    let source = EvidenceSource::recorder();
    let observed_at = instant("2026-08-31T14:03:13Z");
    let claim = ono_temporal_core::EvidenceClaim::FieldValue {
        field: "active_state".into(),
        value: ono_value::Value::string("active"),
        at: observed_at,
    };
    ono_temporal_core::Evidence {
        evidence_id: ono_temporal_core::EvidenceId::of(
            &source,
            observed_at,
            &scope(),
            Some(&subject(2741)),
            &claim,
        ),
        source,
        observed_at,
        source_time: None,
        scope: scope(),
        subject: Some(subject(2741)),
        claim,
        strength: ono_temporal_core::EvidenceStrength::Authoritative,
        raw_ref: None,
        derived_from: Vec::new(),
        provenance: provenance("ono.recorder"),
    }
}

pub fn action(external: Option<&str>) -> ono_temporal_core::ActionEvent {
    let requested_at = instant("2026-08-31T14:03:11Z");
    ono_temporal_core::ActionEvent {
        action_id: ono_temporal_core::ActionId::of(
            "session-1",
            requested_at,
            "restart",
            Some(&subject(1827)),
        ),
        command: ono_temporal_core::RedactedCommandSummary::of(
            "restart",
            Some("service"),
            &[
                ono_temporal_core::Redactable::plain("nginx"),
                ono_temporal_core::Redactable::secret("--password", "hunter2"),
            ],
        ),
        actor: "case".into(),
        session_id: "session-1".into(),
        requested_at,
        target: Some(subject(1827)),
        operation: "restart".into(),
        authorization: ono_temporal_core::AuthorizationSummary {
            decision: ono_temporal_core::AuthorizationDecision::Allowed,
            risk: "destructive".into(),
            capability: Some("system.service.manage".into()),
            reason: None,
        },
        result: external.map(|_| ono_temporal_core::ActionResultSummary {
            outcome: ono_temporal_core::ActionOutcome::Succeeded,
            completed_at: instant("2026-08-31T14:03:13Z"),
            detail: None,
            error_code: None,
        }),
        external_transaction: external.map(Arc::from),
        provenance: provenance("ono.session"),
    }
}

/// The hour every coverage suite composes over.
pub fn window() -> ono_temporal_core::TimeRange {
    ono_temporal_core::TimeRange::between(
        instant("2026-08-31T12:00:00Z"),
        instant("2026-08-31T13:00:00Z"),
    )
}

/// The authoritative source of unit state (v0.5 §7.2's own example).
pub fn systemd() -> EvidenceSource {
    EvidenceSource::parse("linux.systemd-dbus").expect("a §7.1 source class")
}

/// The sampling source of process existence.
pub fn procfs() -> EvidenceSource {
    EvidenceSource::parse("linux.procfs").expect("a §7.1 source class")
}
