//! What a rule may read off an event, and the vocabulary the built-in rules join on.
//!
//! Every function here answers from evidence or from canonical identity. None of them answers
//! from a timestamp: §15.2's "temporal proximity is insufficient" is a property of what the rules
//! are able to look at, and this module is the list of what that is.

use jiff::Timestamp;
use ono_spatial_core::SpatialId;
use ono_temporal_core::{
    ActionId, EventKind, Evidence, EvidenceClaim, EvidenceSource, EvidenceStrength, TemporalEvent,
    TimeRange,
};
use ono_value::Value;

use crate::causal::rule::{CausalContext, EventSet};

/// The `subtype` a systemd `JobRemoved` signal carries (§6.1, §22.2).
///
/// §6.1 gives a provider a namespaced refinement of the top-level kind, and the two systemd job
/// signals are the case the causal rules need to tell apart: `ono.systemd-job-result` explains
/// the transition the *result* names, and `ono.systemd-job-to-unit-state` explains the
/// transitions the job produced while it was running.
pub const SYSTEMD_JOB_REMOVED: &str = "linux.systemd-dbus.job-removed";

/// The `subtype` a systemd `JobNew` signal carries (§6.1, §22.2).
pub const SYSTEMD_JOB_NEW: &str = "linux.systemd-dbus.job-new";

/// The payload field a systemd job event carries its type in — `start`, `stop`, `restart`,
/// `reload` (§22.2).
pub const JOB_TYPE_FIELD: &str = "job_type";

/// The field a service-manager unit's state lives in.
pub const UNIT_STATE_FIELD: &str = "active_state";

/// The alternative spelling of [`UNIT_STATE_FIELD`] a source may use.
pub const UNIT_STATE_FIELD_SHORT: &str = "state";

/// The v0.4 relation a kernel-reported parent/child creation is joined on (§15.2).
pub const PARENT_OF: &str = "process.parent_of";

/// The v0.4 relation a service's cgroup membership is joined on (§15.2).
pub const CONTROLS_PROCESS: &str = "service.controls_process";

/// The v0.4 relation a connection's two ends are joined on (§15.5).
pub const CONNECTED_TO: &str = "socket.connected_to";

/// The instant a human navigates the event by (§25.1).
#[must_use]
pub fn instant(event: &TemporalEvent) -> Timestamp {
    event.times.presentation_instant()
}

/// The canonical identity the event is about, where it reached one (§5.5).
#[must_use]
pub fn subject_id(event: &TemporalEvent) -> Option<&SpatialId> {
    event.subject.as_ref().and_then(|ref_| ref_.spatial_id())
}

/// Whether the event names `id` as its subject or among the objects it touches.
#[must_use]
pub fn mentions(event: &TemporalEvent, id: &SpatialId) -> bool {
    event
        .subject
        .iter()
        .chain(event.related.iter())
        .filter_map(|ref_| ref_.spatial_id())
        .any(|named| named == id)
}

/// Whether two events were read from clocks a wall-clock comparison is meaningful within (§25.5).
#[must_use]
pub fn same_clock_domain(a: &TemporalEvent, b: &TemporalEvent) -> bool {
    a.times.domain == b.times.domain
}

/// The transaction tokens a source published on the event (§21.6, §7.1).
///
/// Returned in the order the event lists its evidence, so a rule that iterates them is
/// deterministic.
pub fn transaction_tokens<'a>(
    event: &'a TemporalEvent,
    context: &'a CausalContext,
) -> impl Iterator<Item = (&'a Evidence, &'a str)> {
    context
        .evidence_of(event)
        .filter_map(|record| match &record.claim {
            EvidenceClaim::Transaction { token, .. } => Some((record, token.as_ref())),
            _ => None,
        })
}

/// Whether `token` carries `identifier` as one of its segments.
///
/// §17.3 propagates an `ActionId` into a provider's own transaction identifier, so the token an
/// effect event carries is `ono:a91f…`, `…/job/4821?ono=a91f…` or the identifier alone. Segments
/// are the runs of alphanumeric characters, which makes the match an equality on a whole segment
/// rather than a substring test that a longer identifier would satisfy by accident.
#[must_use]
pub fn token_carries(token: &str, identifier: &str) -> bool {
    token
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|segment| segment == identifier)
}

/// The Ono action the event belongs to, and the evidence that says so (§17.3).
///
/// The shell records the identity it minted as a transaction claim from `ono.session`; §17.3
/// requires the id to exist before execution, so the claim is the shell's own and authoritative.
/// A payload field is read as a fallback for a source that writes the id there instead.
#[must_use]
pub fn action_of<'a>(
    event: &'a TemporalEvent,
    context: &'a CausalContext,
) -> Option<(ActionId, &'a Evidence)> {
    transaction_tokens(event, context).find_map(|(record, token)| {
        let action = ActionId::parse(token)?;
        (record.source == EvidenceSource::session()).then_some((action, record))
    })
}

/// A payload entry, as text.
#[must_use]
pub fn payload_text<'a>(event: &'a TemporalEvent, field: &str) -> Option<&'a str> {
    let value = match event.payload.as_ref()? {
        Value::Map(map) => map.get(field)?,
        Value::Record(record) => record.get(field)?,
        _ => return None,
    };
    match value {
        Value::String(text) => Some(text.as_ref()),
        _ => None,
    }
}

/// The interval a relation claim says the edge held over (§6.4).
#[must_use]
pub fn relation_interval(record: &Evidence) -> Option<TimeRange> {
    match &record.claim {
        EvidenceClaim::RelationHeld { over, .. } => Some(*over),
        _ => None,
    }
}

/// The evidence on the event that states something about `field`, at the strength a rule needs.
///
/// A transition claim and a field-value claim both say what the field held; §6.2 keeps the two
/// apart because one of them also says what it held before.
pub fn field_evidence<'a>(
    event: &'a TemporalEvent,
    context: &'a CausalContext,
    field: &'a str,
) -> impl Iterator<Item = &'a Evidence> {
    context
        .evidence_of(event)
        .filter(move |record| match &record.claim {
            EvidenceClaim::FieldValue { field: named, .. }
            | EvidenceClaim::Transition { field: named, .. } => named.as_ref() == field,
            _ => false,
        })
}

/// The strongest evidence on the event that meets `minimum` and comes from an accepted source.
///
/// "Strongest" is a choice among records that all already qualify; nothing here raises a
/// strength, which §7.2 forbids outright.
#[must_use]
pub fn qualifying_evidence<'a>(
    event: &'a TemporalEvent,
    context: &'a CausalContext,
    minimum: EvidenceStrength,
    accepted: &dyn Fn(&EvidenceSource) -> bool,
) -> Option<&'a Evidence> {
    context
        .evidence_of(event)
        .filter(|record| record.strength >= minimum && accepted(&record.source))
        .max_by(|left, right| {
            left.strength
                .cmp(&right.strength)
                .then_with(|| right.evidence_id.cmp(&left.evidence_id))
        })
}

/// The interval spanned by every event in `events` that carries `token` (§22.2).
///
/// A systemd job's lifetime is bounded by the instants systemd itself reported for it, and this
/// is those instants: the earliest and the latest event the source stamped with the job's own
/// path. A transition outside that span was not produced by that job.
#[must_use]
pub fn token_span(
    events: &[TemporalEvent],
    context: &CausalContext,
    token: &str,
) -> Option<(Timestamp, Timestamp)> {
    let mut span: Option<(Timestamp, Timestamp)> = None;
    for event in events {
        if transaction_tokens(event, context).any(|(_, carried)| carried == token) {
            let at = instant(event);
            span = Some(match span {
                None => (at, at),
                Some((from, until)) => (from.min(at), until.max(at)),
            });
        }
    }
    span
}

/// Whether an event is a failure a correlation rule may sit beside (§15.5, §16.6).
///
/// Two shapes reach the ledger. An operator's mutation that failed is an `action.failed`. A
/// service that gave up on its own is an `object.changed` whose state field moved to one of
/// [`crate::landmark::FAILED_STATES`] — which is exactly §15.5's worked example, "config file
/// changed 9 seconds before a service failure", and exactly what §16.6 renders as
/// `nginx.service failed at 14:03:17.004`. A rule that admitted only the first would be silent
/// for every failure nobody caused, which is most of them.
#[must_use]
pub fn is_failure(event: &TemporalEvent) -> bool {
    match event.kind {
        EventKind::ActionFailed => true,
        EventKind::ObjectChanged => {
            crate::landmark::state_moved_to(event, crate::landmark::FAILED_STATES)
        }
        _ => false,
    }
}

/// Every failure in the candidate set, in the order the set holds them.
pub fn failures<'a>(candidate: &EventSet<'a>) -> impl Iterator<Item = &'a TemporalEvent> {
    candidate.events().iter().filter(|event| is_failure(event))
}
