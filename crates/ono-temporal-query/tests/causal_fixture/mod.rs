//! Fixtures the causal outcome tests share: a fixed host, a fixed clock, and builders for the
//! two things every causal test needs — an event and the evidence a rule is allowed to join on.
//!
//! The fixture is deliberately unhelpful about causality. It builds events and evidence and
//! nothing else; every causal claim in these tests comes from the engine.

#![allow(
    dead_code,
    reason = "each test binary uses the part of the fixture its own subject needs"
)]
#![allow(
    clippy::expect_used,
    reason = "a fixture states its preconditions the way a #[test] body does (AGENTS.md section 16)"
)]

use std::sync::Arc;

use jiff::Timestamp;
use ono_spatial_core::{BootIdentity, SpatialId, SpatialIdentity, SpatialScope, SpatialType};
use ono_temporal_core::{
    ChangeCertainty, ClockDomain, EventKind, EventSeed, EventTimes, Evidence, EvidenceClaim,
    EvidenceId, EvidenceSource, EvidenceStrength, FieldChange, SpatialRef, TemporalEvent,
    TimeRange,
};
use ono_temporal_query::causal::CausalContext;
use ono_value::{MapValue, Provenance, SchemaId, Value};

/// The fixture host's boot identity.
#[must_use]
pub fn boot() -> BootIdentity {
    BootIdentity::new("testbox", "4d0a1f2b-0000-4000-8000-000000000001")
}

/// The fixture scope: one host, one boot.
#[must_use]
pub fn scope() -> SpatialScope {
    SpatialScope::host("testbox", boot())
}

/// A second host, for the cross-host tests of §26.4.
#[must_use]
pub fn remote_scope() -> SpatialScope {
    SpatialScope::host(
        "db01",
        BootIdentity::new("db01", "4d0a1f2b-0000-4000-8000-000000000002"),
    )
}

/// An instant from RFC 3339 text.
///
/// # Panics
///
/// Panics when the fixture text is not an instant, which is a mistake in the test.
#[must_use]
pub fn instant(text: &str) -> Timestamp {
    text.parse().expect("the fixture instant parses")
}

/// A process identity, lifetime-bound the way v0.4 keys one.
#[must_use]
pub fn process(pid: i64) -> SpatialId {
    SpatialIdentity::lifetime(SpatialType::Process, [("pid", pid.to_string())]).spatial_id()
}

/// A service identity.
#[must_use]
pub fn service(unit: &str) -> SpatialId {
    SpatialIdentity::stable(SpatialType::Service, [("unit", unit.to_owned())]).spatial_id()
}

/// A file identity, for the config-change correlation of §15.5.
#[must_use]
pub fn file(path: &str) -> SpatialId {
    SpatialIdentity::stable(SpatialType::File, [("path", path.to_owned())]).spatial_id()
}

/// A filesystem identity, for the resource-pressure correlation of §15.5.
#[must_use]
pub fn filesystem(mount: &str) -> SpatialId {
    SpatialIdentity::stable(SpatialType::Filesystem, [("mount", mount.to_owned())]).spatial_id()
}

/// A socket identity.
#[must_use]
pub fn socket(inode: i64) -> SpatialId {
    SpatialIdentity::lifetime(SpatialType::Socket, [("inode", inode.to_string())]).spatial_id()
}

/// A far-end identity, which may be off this host.
#[must_use]
pub fn endpoint(address: &str) -> SpatialId {
    SpatialIdentity::stable(SpatialType::Endpoint, [("address", address.to_owned())]).spatial_id()
}

/// The provenance a fixture record carries.
#[must_use]
pub fn provenance(provider: &str) -> Provenance {
    Provenance::local(provider, SchemaId::new("ono.temporal-event", 1))
}

/// The fixture clock domain.
#[must_use]
pub fn domain() -> ClockDomain {
    ClockDomain::new("testbox", Some("4d0a1f2b"))
}

/// A second clock domain, which shares no ordering evidence with the first (§25.5).
#[must_use]
pub fn remote_domain() -> ClockDomain {
    ClockDomain::new("db01", Some("9a71ce04"))
}

/// The evidence sources the fixture uses.
///
/// # Panics
///
/// Panics when a source name the fixture spells is not one §7.1 declares.
#[must_use]
pub fn source(name: &str) -> EvidenceSource {
    EvidenceSource::parse(name).expect("the fixture names a §7.1 source")
}

/// Builds one event, one fact at a time.
#[derive(Debug, Clone)]
pub struct EventBuilder {
    seed: EventSeed,
}

impl EventBuilder {
    /// An event of `kind` observed at `at`, in the fixture scope and clock domain.
    #[must_use]
    pub fn new(kind: EventKind, at: &str) -> Self {
        Self {
            seed: EventSeed {
                kind,
                subtype: None,
                scope: scope(),
                subject: None,
                related: Vec::new(),
                times: EventTimes {
                    source_time: Some(instant(at)),
                    observed_at: instant(at),
                    ingested_at: instant(at),
                    source_sequence: None,
                    monotonic_nanos: None,
                    clock_uncertainty: None,
                    domain: domain(),
                },
                before: None,
                after: None,
                changed_fields: Vec::new(),
                evidence: Vec::new(),
                causal_parents: Vec::new(),
                payload: None,
                provenance: provenance("ono.recorder"),
            },
        }
    }

    /// §6.1's namespaced refinement of the kind.
    #[must_use]
    pub fn subtype(mut self, subtype: &str) -> Self {
        self.seed.subtype = Some(Arc::from(subtype));
        self
    }

    /// The object the event is about.
    #[must_use]
    pub fn subject(mut self, id: &SpatialId, object_type: SpatialType, label: &str) -> Self {
        self.seed.subject = Some(SpatialRef::Resolved {
            id: id.clone(),
            object_type,
            label: Arc::from(label),
        });
        self
    }

    /// A further object the event touches.
    #[must_use]
    pub fn related(mut self, id: &SpatialId, object_type: SpatialType, label: &str) -> Self {
        self.seed.related.push(SpatialRef::Resolved {
            id: id.clone(),
            object_type,
            label: Arc::from(label),
        });
        self
    }

    /// The provider that reported it.
    #[must_use]
    pub fn provider(mut self, provider: &str) -> Self {
        self.seed.provenance = provenance(provider);
        self
    }

    /// The source's own sequence number, which is ordering evidence §26.1 accepts.
    #[must_use]
    pub fn sequence(mut self, sequence: u64) -> Self {
        self.seed.times.source_sequence = Some(sequence);
        self
    }

    /// A monotonic reading inside this clock domain.
    #[must_use]
    pub fn monotonic(mut self, nanos: u64) -> Self {
        self.seed.times.monotonic_nanos = Some(nanos);
        self
    }

    /// Puts the event in another clock domain and scope.
    #[must_use]
    pub fn on_host(mut self, domain: ClockDomain, scope: SpatialScope) -> Self {
        self.seed.times.domain = domain;
        self.seed.scope = scope;
        self
    }

    /// Attaches an evidence record's identity to the event.
    #[must_use]
    pub fn evidence(mut self, id: &EvidenceId) -> Self {
        self.seed.evidence.push(id.clone());
        self
    }

    /// A typed field change (§6.2).
    #[must_use]
    pub fn changed(mut self, field: &str, before: Value, after: Value) -> Self {
        self.seed.changed_fields.push(FieldChange {
            field: Arc::from(field),
            before: Some(before),
            after: Some(after),
            certainty: ChangeCertainty::Observed,
        });
        self
    }

    /// A typed field change whose certainty the test names.
    #[must_use]
    pub fn changed_with(
        mut self,
        field: &str,
        before: Option<Value>,
        after: Option<Value>,
        certainty: ChangeCertainty,
    ) -> Self {
        self.seed.changed_fields.push(FieldChange {
            field: Arc::from(field),
            before,
            after,
            certainty,
        });
        self
    }

    /// A payload entry, as a provider writes one.
    #[must_use]
    pub fn payload(mut self, key: &str, value: Value) -> Self {
        let mut map = match self.seed.payload.take() {
            Some(Value::Map(map)) => (*map).clone(),
            _ => MapValue::new(),
        };
        map.insert(Arc::from(key), value);
        self.seed.payload = Some(Value::Map(Arc::new(map)));
        self
    }

    /// Computes the identity and produces the event.
    #[must_use]
    pub fn seal(self) -> TemporalEvent {
        self.seed.seal()
    }
}

/// The evidence a test hands the engine, and the context built from it.
#[derive(Debug, Default)]
pub struct World {
    evidence: Vec<Evidence>,
}

impl World {
    /// A world with no evidence in it.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A transaction token a source published (§21.6).
    pub fn transaction(
        &mut self,
        source: &str,
        at: &str,
        subject: Option<&SpatialId>,
        token: &str,
        strength: EvidenceStrength,
    ) -> EvidenceId {
        self.claim(
            source,
            at,
            subject,
            EvidenceClaim::Transaction {
                token: Arc::from(token),
                at: instant(at),
            },
            strength,
        )
    }

    /// A relationship a source observed over an interval (§6.4).
    pub fn relation(
        &mut self,
        source: &str,
        at: &str,
        subject: &SpatialId,
        relation: &str,
        other: &SpatialId,
        over: TimeRange,
        strength: EvidenceStrength,
    ) -> EvidenceId {
        self.claim(
            source,
            at,
            Some(subject),
            EvidenceClaim::RelationHeld {
                relation: Arc::from(relation),
                other: other.clone(),
                over,
            },
            strength,
        )
    }

    /// A source saying an object existed at an instant.
    pub fn existed(
        &mut self,
        source: &str,
        at: &str,
        subject: &SpatialId,
        strength: EvidenceStrength,
    ) -> EvidenceId {
        self.claim(
            source,
            at,
            Some(subject),
            EvidenceClaim::ObjectExisted { at: instant(at) },
            strength,
        )
    }

    /// A source saying a field held a value.
    pub fn field(
        &mut self,
        source: &str,
        at: &str,
        subject: &SpatialId,
        field: &str,
        value: Value,
        strength: EvidenceStrength,
    ) -> EvidenceId {
        self.claim(
            source,
            at,
            Some(subject),
            EvidenceClaim::FieldValue {
                field: Arc::from(field),
                value,
                at: instant(at),
            },
            strength,
        )
    }

    /// A source saying a field went from one value to another.
    pub fn transition(
        &mut self,
        source: &str,
        at: &str,
        subject: &SpatialId,
        field: &str,
        from: Value,
        to: Value,
        strength: EvidenceStrength,
    ) -> EvidenceId {
        self.claim(
            source,
            at,
            Some(subject),
            EvidenceClaim::Transition {
                field: Arc::from(field),
                from,
                to,
                at: instant(at),
            },
            strength,
        )
    }

    /// The claim, recorded and identified.
    pub fn claim(
        &mut self,
        source: &str,
        at: &str,
        subject: Option<&SpatialId>,
        claim: EvidenceClaim,
        strength: EvidenceStrength,
    ) -> EvidenceId {
        let source = self::source(source);
        let observed_at = instant(at);
        let scope = scope();
        let evidence_id = EvidenceId::of(&source, observed_at, &scope, subject, &claim);
        self.evidence.push(Evidence {
            evidence_id: evidence_id.clone(),
            source,
            observed_at,
            source_time: Some(observed_at),
            scope,
            subject: subject.cloned(),
            claim,
            strength,
            raw_ref: None,
            derived_from: Vec::new(),
            provenance: provenance("ono.recorder"),
        });
        evidence_id
    }

    /// The context the engine evaluates against, with every source declaring causal tokens.
    #[must_use]
    pub fn context(&self) -> CausalContext {
        CausalContext::new(self.evidence.clone()).with_causal_token_sources(
            &[
                "linux.systemd-dbus",
                "ono.session",
                "adapter:podman",
                "remote:db01/linux.procfs",
                "kuang:dev.example.packet-eye/flows",
            ]
            .map(self::source),
        )
    }

    /// The context with no source advertising `causal_tokens` (§21.6).
    #[must_use]
    pub fn context_without_tokens(&self) -> CausalContext {
        CausalContext::new(self.evidence.clone())
    }

    /// The evidence records the world holds.
    #[must_use]
    pub fn evidence(&self) -> &[Evidence] {
        &self.evidence
    }
}
