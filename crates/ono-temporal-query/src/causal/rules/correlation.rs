//! The three correlation rules (spec v0.5 §15.5).
//!
//! These are the only rules in the engine that may use a time window, and the window is the
//! reason they emit `correlated_with` and nothing else. §15.5 requires correlation to be
//! "visually and structurally distinct from causation": structurally it is a different relation
//! class whose `is_causal()` is false, it lands in a separate array of the explanation (§35.5),
//! and its strength is capped at `correlated`, which is §7.2's word for evidence that
//! "establishes association, never causation".
//!
//! A window alone is never enough. Each rule also demands a structural association — a spatial
//! relation, a shared scope, a connection identity — because §15.5 asks for "meaningful temporal
//! and/or structural association", and two readings that share nothing but a minute are two
//! readings.

use jiff::Timestamp;
use ono_spatial_core::SpatialType;
use ono_temporal_core::{
    CausalRelation, CausalRuleId, EventKind, Evidence, EvidenceStrength, TemporalEvent,
};
use ono_value::Duration;

use crate::causal::facts;
use crate::causal::link::CausalFinding;
use crate::causal::rule::{
    CausalContext, CausalRule, EventSet, EvidenceRequirements, RuleDescription,
};
use crate::causal::rules::{accepts, constraints};

/// A minute in nanoseconds.
const MINUTE: i128 = 60_000_000_000;

/// The types whose changes read as resource pressure (§15.5, §22.6).
const RESOURCE_TYPES: &[SpatialType] = &[
    SpatialType::Filesystem,
    SpatialType::Mount,
    SpatialType::BlockDevice,
    SpatialType::Cgroup,
    SpatialType::Device,
];

/// The instant, in nanoseconds.
fn nanos(at: Timestamp) -> i128 {
    at.as_nanosecond()
}

/// The object type an event's subject resolved to, where it resolved to one (§5.5).
fn subject_type(event: &TemporalEvent) -> Option<SpatialType> {
    match event.subject.as_ref()? {
        ono_temporal_core::SpatialRef::Resolved { object_type, .. } => Some(*object_type),
        ono_temporal_core::SpatialRef::Unresolved { .. } => None,
    }
}

/// Emits one association, with the earlier event at the cause end.
///
/// `correlated_with` is symmetric — §15.1 makes its inverse label itself — so the ends carry no
/// claim of direction. They are ordered so that one pair of events produces one edge whichever
/// way the rule walked them.
fn associate(
    rule: &CausalRuleId,
    a: &TemporalEvent,
    b: &TemporalEvent,
    evidence: Vec<ono_temporal_core::EvidenceId>,
    strength: EvidenceStrength,
    source: ono_temporal_core::EvidenceSource,
) -> Option<CausalFinding> {
    let (first, second) =
        if (facts::instant(a), a.event_id.as_str()) <= (facts::instant(b), b.event_id.as_str()) {
            (a, b)
        } else {
            (b, a)
        };
    CausalFinding::emit(
        rule,
        CausalRelation::CorrelatedWith,
        &first.event_id,
        &second.event_id,
        evidence,
        strength.weakest_of(EvidenceStrength::Correlated),
        source,
    )
    .ok()
}

/// The evidence on `event` that meets the rule's minimum for its kind and comes from an accepted
/// source.
fn side<'a>(
    event: &'a TemporalEvent,
    context: &'a CausalContext,
    requirements: &EvidenceRequirements,
    constraints: &[crate::causal::rule::SourceConstraint],
) -> Option<&'a Evidence> {
    let minimum = requirements.minimum_for(event.kind)?;
    facts::qualifying_evidence(event, context, minimum, &|source| {
        accepts(constraints, source)
    })
}

/// `ono.change-before-failure`: a change to a related object shortly before a failure (§15.5).
///
/// §15.5's first example, and §16.6's: a config file changed eleven seconds before nginx failed.
/// It is offered as context beside an explanation and never as the explanation, which is what
/// §55.3 names as the failure this engine exists against.
#[derive(Debug, Clone, Copy, Default)]
pub struct ChangeBeforeFailure;

impl CausalRule for ChangeBeforeFailure {
    fn describe(&self) -> RuleDescription {
        RuleDescription {
            rule_id: CausalRuleId::new("ono.change-before-failure"),
            input_event_kinds: vec![EventKind::ObjectChanged, EventKind::ActionFailed],
            required_evidence: EvidenceRequirements::new(&[
                (EventKind::ObjectChanged, EvidenceStrength::Observational),
                (EventKind::ActionFailed, EvidenceStrength::Asserted),
            ]),
            identity_constraints: "the changed object and the failing object are joined by an existing spatial \
                 relation; an association with no structural link is not emitted",
            time_constraints: Some(
                "the change precedes the failure by at most 5m, in the same clock domain",
            ),
            output_relation: CausalRelation::CorrelatedWith,
            source_constraints: constraints(&[
                "linux.procfs",
                "linux.systemd-dbus",
                "linux.journald",
                "ono.recorder",
            ]),
            window: Some(Duration::from_nanoseconds(5 * MINUTE)),
        }
    }

    fn evaluate(&self, candidate: &EventSet<'_>, context: &CausalContext) -> Vec<CausalFinding> {
        let description = self.describe();
        let sources = &description.source_constraints;
        let requirements = &description.required_evidence;
        let mut findings = Vec::new();

        for change in candidate.of_kind(EventKind::ObjectChanged) {
            let Some(changed) = facts::subject_id(change) else {
                continue;
            };
            let Some(change_evidence) = side(change, context, requirements, sources) else {
                continue;
            };
            for failure in facts::failures(candidate) {
                let Some(failed) = facts::subject_id(failure) else {
                    continue;
                };
                if failed == changed || !facts::same_clock_domain(change, failure) {
                    continue;
                }
                let gap = nanos(facts::instant(failure)) - nanos(facts::instant(change));
                if !(0..=5 * MINUTE).contains(&gap) {
                    continue;
                }
                let Some(relation) = context.any_relation_touching(changed, failed) else {
                    continue;
                };
                let Some(failure_evidence) = side(failure, context, requirements, sources) else {
                    continue;
                };
                if let Some(finding) = associate(
                    &description.rule_id,
                    change,
                    failure,
                    vec![
                        relation.evidence_id.clone(),
                        change_evidence.evidence_id.clone(),
                        failure_evidence.evidence_id.clone(),
                    ],
                    change_evidence
                        .strength
                        .weakest_of(failure_evidence.strength),
                    change_evidence.source.clone(),
                ) {
                    findings.push(finding);
                }
            }
        }
        findings
    }
}

/// `ono.resource-pressure-overlap`: pressure and errors in one scope (§15.5, §22.6).
#[derive(Debug, Clone, Copy, Default)]
pub struct ResourcePressureOverlap;

impl CausalRule for ResourcePressureOverlap {
    fn describe(&self) -> RuleDescription {
        RuleDescription {
            rule_id: CausalRuleId::new("ono.resource-pressure-overlap"),
            input_event_kinds: vec![EventKind::ObjectChanged, EventKind::ActionFailed],
            required_evidence: EvidenceRequirements::new(&[
                (EventKind::ObjectChanged, EvidenceStrength::Observational),
                (EventKind::ActionFailed, EvidenceStrength::Asserted),
            ]),
            identity_constraints: "the pressure and the errors share a scope — the same filesystem, cgroup or \
                 host — and the changed object is a resource rather than an arbitrary object",
            time_constraints: Some("the two intervals overlap, or are separated by at most 1m"),
            output_relation: CausalRelation::CorrelatedWith,
            source_constraints: constraints(&[
                "linux.procfs",
                "ono.recorder",
                "kuang:<package-id>/<provider-id>",
            ]),
            window: Some(Duration::from_nanoseconds(MINUTE)),
        }
    }

    fn evaluate(&self, candidate: &EventSet<'_>, context: &CausalContext) -> Vec<CausalFinding> {
        let description = self.describe();
        let sources = &description.source_constraints;
        let requirements = &description.required_evidence;
        let mut findings = Vec::new();

        for pressure in candidate.of_kind(EventKind::ObjectChanged) {
            if !subject_type(pressure).is_some_and(|kind| RESOURCE_TYPES.contains(&kind)) {
                continue;
            }
            let Some(pressure_evidence) = side(pressure, context, requirements, sources) else {
                continue;
            };
            for failure in facts::failures(candidate) {
                let shared = pressure.scope.contains(&failure.scope)
                    || failure.scope.contains(&pressure.scope);
                if !shared || !facts::same_clock_domain(pressure, failure) {
                    continue;
                }
                let gap = nanos(facts::instant(failure)) - nanos(facts::instant(pressure));
                if gap.abs() > MINUTE {
                    continue;
                }
                let Some(failure_evidence) = side(failure, context, requirements, sources) else {
                    continue;
                };
                if let Some(finding) = associate(
                    &description.rule_id,
                    pressure,
                    failure,
                    vec![
                        pressure_evidence.evidence_id.clone(),
                        failure_evidence.evidence_id.clone(),
                    ],
                    pressure_evidence
                        .strength
                        .weakest_of(failure_evidence.strength),
                    pressure_evidence.source.clone(),
                ) {
                    findings.push(finding);
                }
            }
        }
        findings
    }
}

/// `ono.remote-endpoint-retry-spike`: a far end went away near a local change (§15.5, §26.3).
///
/// The clearest case for why correlation exists as a separate class: two hosts, no shared
/// ordering evidence, and a pattern an operator still wants to see. §26.1's happens-before
/// answers `concurrent` for the pair, and this rule claims no order.
#[derive(Debug, Clone, Copy, Default)]
pub struct RemoteEndpointRetrySpike;

impl CausalRule for RemoteEndpointRetrySpike {
    fn describe(&self) -> RuleDescription {
        RuleDescription {
            rule_id: CausalRuleId::new("ono.remote-endpoint-retry-spike"),
            input_event_kinds: vec![EventKind::ObjectDisappeared, EventKind::ObjectChanged],
            required_evidence: EvidenceRequirements::new(&[
                (EventKind::ObjectDisappeared, EvidenceStrength::Asserted),
                (EventKind::ObjectChanged, EvidenceStrength::Observational),
            ]),
            identity_constraints: "the endpoint that went away is one the local socket was connected to, joined \
                 through the connection's own identity",
            time_constraints: Some(
                "the two lie within 30s of each other, subject to the clock uncertainty of the \
                 remote domain",
            ),
            output_relation: CausalRelation::CorrelatedWith,
            source_constraints: constraints(&[
                "linux.netlink",
                "linux.procfs",
                "remote:<link-id>/<provider-id>",
            ]),
            window: Some(Duration::from_nanoseconds(MINUTE / 2)),
        }
    }

    fn evaluate(&self, candidate: &EventSet<'_>, context: &CausalContext) -> Vec<CausalFinding> {
        let description = self.describe();
        let sources = &description.source_constraints;
        let requirements = &description.required_evidence;
        let mut findings = Vec::new();

        for gone in candidate.of_kind(EventKind::ObjectDisappeared) {
            let Some(far_end) = facts::subject_id(gone) else {
                continue;
            };
            let Some(gone_evidence) = side(gone, context, requirements, sources) else {
                continue;
            };
            for local in candidate.of_kind(EventKind::ObjectChanged) {
                let Some(near_end) = facts::subject_id(local) else {
                    continue;
                };
                let Some(connection) =
                    context.relation_touching(facts::CONNECTED_TO, near_end, far_end)
                else {
                    continue;
                };
                let gap = nanos(facts::instant(local)) - nanos(facts::instant(gone));
                if gap.abs() > MINUTE / 2 {
                    continue;
                }
                let Some(local_evidence) = side(local, context, requirements, sources) else {
                    continue;
                };
                if let Some(finding) = associate(
                    &description.rule_id,
                    gone,
                    local,
                    vec![
                        connection.evidence_id.clone(),
                        gone_evidence.evidence_id.clone(),
                        local_evidence.evidence_id.clone(),
                    ],
                    gone_evidence.strength.weakest_of(local_evidence.strength),
                    gone_evidence.source.clone(),
                ) {
                    findings.push(finding);
                }
            }
        }
        findings
    }
}
