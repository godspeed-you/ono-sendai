//! The bridge between the temporal vocabulary of §3 and the rows the store holds.
//!
//! The split follows §31.4: anything an index looks at is a column, everything else is one CBOR
//! payload per row. So the instant a timeline filters on, the scope a place query ranges over and
//! the kind a filter names are relational, while a field change, a claim and a provider body are
//! encoded once and read back whole.

use std::sync::Arc;

use ciborium::value::{Integer, Value as Cbor};
use jiff::Timestamp;
use ono_spatial_core::{Confidence, PermissionState, SpatialType};
use ono_temporal_core::{
    ActionEvent, ActionId, ActionOutcome, ActionResultSummary, AuthorizationDecision,
    AuthorizationSummary, CausalLink, CausalLinkId, CausalRelation, CausalRuleId, ChangeCertainty,
    Checkpoint, CheckpointId, ClockDomain, EventId, EventKind, EventTimes, Evidence, EvidenceClaim,
    EvidenceId, EvidenceSource, EvidenceStrength, FieldChange, ObjectState, OpaqueReference,
    RedactedCommandSummary, RelationState, SpatialRef, TemporalCompleteness, TemporalCoverage,
    TemporalEvent, TimeRange,
};
use ono_value::{RecordValue, SchemaRegistry, Value};

use crate::codec::{
    Decoded, array, instant, optional_instant, optional_string, optional_text, provenance,
    read_instant, read_optional_instant, read_provenance, read_scope_path, read_spatial_id,
    read_value, scope_path, seal, text, unseal, value,
};

// ---- events -----------------------------------------------------------------------------------

/// The columns of one event row: the scalars a query filters or orders on (§32.3).
pub(crate) struct EventColumns {
    pub(crate) event_id: String,
    pub(crate) kind: String,
    pub(crate) scope: String,
    pub(crate) subject: Option<String>,
    pub(crate) presentation_nanos: i64,
    pub(crate) source_nanos: Option<i64>,
    pub(crate) observed_nanos: i64,
    pub(crate) ingested_nanos: i64,
    pub(crate) source_sequence: Option<i64>,
    pub(crate) monotonic_nanos: Option<i64>,
    pub(crate) uncertainty_nanos: Option<i64>,
    pub(crate) domain_host: String,
    pub(crate) domain_boot: Option<String>,
    pub(crate) source: String,
    pub(crate) body: Vec<u8>,
}

/// Nanoseconds as SQLite holds them.
///
/// A 64-bit nanosecond count reaches from 1677 to 2262, which covers every instant `jiff` reports
/// for a running system; anything outside it is a corrupt reading rather than a history.
pub(crate) fn nanos(at: Timestamp) -> Decoded<i64> {
    i64::try_from(at.as_nanosecond())
        .map_err(|_| format!("{at} is outside the range the ledger stores"))
}

pub(crate) fn from_nanos(nanos: i64) -> Decoded<Timestamp> {
    Timestamp::from_nanosecond(i128::from(nanos))
        .map_err(|error| format!("a stored instant is not a timestamp: {error}"))
}

pub(crate) fn event_columns(event: &TemporalEvent) -> Decoded<EventColumns> {
    let body = seal(Cbor::Array(vec![
        optional_string(event.subtype.as_deref()),
        event.subject.as_ref().map_or(Cbor::Null, spatial_ref),
        Cbor::Array(event.related.iter().map(spatial_ref).collect()),
        event.before.as_ref().map_or(Cbor::Null, value),
        event.after.as_ref().map_or(Cbor::Null, value),
        Cbor::Array(event.changed_fields.iter().map(field_change).collect()),
        Cbor::Array(
            event
                .evidence
                .iter()
                .map(|id| Cbor::Text(id.as_str().to_owned()))
                .collect(),
        ),
        Cbor::Array(
            event
                .causal_parents
                .iter()
                .map(|id| Cbor::Text(id.as_str().to_owned()))
                .collect(),
        ),
        event.payload.as_ref().map_or(Cbor::Null, value),
        provenance(&event.provenance),
    ]))?;
    Ok(EventColumns {
        event_id: event.event_id.as_str().to_owned(),
        kind: event.kind.as_str().to_owned(),
        scope: scope_path(&event.scope),
        subject: event
            .subject
            .as_ref()
            .and_then(SpatialRef::spatial_id)
            .map(|id| id.as_str().to_owned()),
        presentation_nanos: nanos(event.times.presentation_instant())?,
        source_nanos: event.times.source_time.map(nanos).transpose()?,
        observed_nanos: nanos(event.times.observed_at)?,
        ingested_nanos: nanos(event.times.ingested_at)?,
        source_sequence: event
            .times
            .source_sequence
            .map(|sequence| i64::try_from(sequence).unwrap_or(i64::MAX)),
        monotonic_nanos: event
            .times
            .monotonic_nanos
            .map(|reading| i64::try_from(reading).unwrap_or(i64::MAX)),
        uncertainty_nanos: event
            .times
            .clock_uncertainty
            .map(|span| i64::try_from(span.nanoseconds()).unwrap_or(i64::MAX)),
        domain_host: event.times.domain.host.to_string(),
        domain_boot: event.times.domain.boot_id.as_ref().map(ToString::to_string),
        source: event.provenance.provider().to_owned(),
        body,
    })
}

/// Rebuilds an event from its columns and its payload.
#[allow(
    clippy::too_many_arguments,
    reason = "the arguments are one row's columns"
)]
pub(crate) fn read_event(
    columns: &EventColumns,
    schemas: &SchemaRegistry,
) -> Decoded<TemporalEvent> {
    let body = unseal(&columns.body)?;
    let parts = array(&body)?;
    let [
        subtype,
        subject,
        related,
        before,
        after,
        changed,
        evidence,
        parents,
        payload,
        prov,
    ] = parts
    else {
        return Err(format!(
            "an event body carries {} fields rather than ten",
            parts.len()
        ));
    };
    let event_id = EventId::parse(&columns.event_id)
        .ok_or_else(|| format!("`{}` is not an event identity", columns.event_id))?;
    let kind = EventKind::from_name(&columns.kind)
        .ok_or_else(|| format!("`{}` is not an event kind", columns.kind))?;
    let mut related_refs = Vec::new();
    for item in array(related)? {
        related_refs.push(read_spatial_ref(item)?);
    }
    let mut changes = Vec::new();
    for item in array(changed)? {
        changes.push(read_field_change(item, schemas)?);
    }
    let mut evidence_ids = Vec::new();
    for item in array(evidence)? {
        let raw = text(item)?;
        evidence_ids
            .push(EvidenceId::parse(raw).ok_or_else(|| format!("`{raw}` is not an evidence id"))?);
    }
    let mut parent_ids = Vec::new();
    for item in array(parents)? {
        let raw = text(item)?;
        parent_ids.push(
            CausalLinkId::parse(raw).ok_or_else(|| format!("`{raw}` is not a causal link id"))?,
        );
    }
    Ok(TemporalEvent {
        event_id,
        kind,
        subtype: optional_text(subtype)?.map(Arc::from),
        scope: read_scope_path(&columns.scope)?,
        subject: match subject {
            Cbor::Null => None,
            other => Some(read_spatial_ref(other)?),
        },
        related: related_refs,
        times: EventTimes {
            source_time: columns.source_nanos.map(from_nanos).transpose()?,
            observed_at: from_nanos(columns.observed_nanos)?,
            ingested_at: from_nanos(columns.ingested_nanos)?,
            source_sequence: columns
                .source_sequence
                .map(|sequence| u64::try_from(sequence).unwrap_or_default()),
            monotonic_nanos: columns
                .monotonic_nanos
                .map(|reading| u64::try_from(reading).unwrap_or_default()),
            clock_uncertainty: columns
                .uncertainty_nanos
                .map(|span| ono_value::Duration::from_nanoseconds(i128::from(span))),
            domain: ClockDomain {
                host: Arc::from(columns.domain_host.as_str()),
                boot_id: columns.domain_boot.as_deref().map(Arc::from),
            },
        },
        before: read_optional_value(before, schemas)?,
        after: read_optional_value(after, schemas)?,
        changed_fields: changes,
        evidence: evidence_ids,
        causal_parents: parent_ids,
        payload: read_optional_value(payload, schemas)?,
        provenance: read_provenance(prov)?,
    })
}

fn read_optional_value(item: &Cbor, schemas: &SchemaRegistry) -> Decoded<Option<Value>> {
    match item {
        Cbor::Null => Ok(None),
        other => read_value(other, schemas).map(Some),
    }
}

fn spatial_ref(reference: &SpatialRef) -> Cbor {
    match reference {
        SpatialRef::Resolved {
            id,
            object_type,
            label,
        } => Cbor::Array(vec![
            Cbor::Text("resolved".to_owned()),
            Cbor::Text(id.as_str().to_owned()),
            Cbor::Text(object_type.as_str().to_owned()),
            Cbor::Text(label.to_string()),
        ]),
        SpatialRef::Unresolved { source, described } => Cbor::Array(vec![
            Cbor::Text("unresolved".to_owned()),
            Cbor::Text(source.as_str().to_owned()),
            Cbor::Text(described.to_string()),
        ]),
    }
}

fn read_spatial_ref(item: &Cbor) -> Decoded<SpatialRef> {
    let parts = array(item)?;
    match parts.split_first() {
        Some((kind, [id, object_type, label])) if text(kind)? == "resolved" => {
            let object_type = text(object_type)?;
            Ok(SpatialRef::Resolved {
                id: read_spatial_id(text(id)?)?,
                object_type: SpatialType::from_name(object_type)
                    .ok_or_else(|| format!("`{object_type}` is not a spatial type"))?,
                label: Arc::from(text(label)?),
            })
        }
        Some((kind, [source, described])) if text(kind)? == "unresolved" => {
            Ok(SpatialRef::Unresolved {
                source: read_source(text(source)?)?,
                described: Arc::from(text(described)?),
            })
        }
        _ => Err("a subject reference names no known kind".to_owned()),
    }
}

pub(crate) fn read_source(name: &str) -> Decoded<EvidenceSource> {
    EvidenceSource::parse(name).ok_or_else(|| format!("`{name}` is not an evidence source class"))
}

fn field_change(change: &FieldChange) -> Cbor {
    Cbor::Array(vec![
        Cbor::Text(change.field.to_string()),
        change.before.as_ref().map_or(Cbor::Null, value),
        change.after.as_ref().map_or(Cbor::Null, value),
        Cbor::Text(change.certainty.as_str().to_owned()),
    ])
}

fn read_field_change(item: &Cbor, schemas: &SchemaRegistry) -> Decoded<FieldChange> {
    let parts = array(item)?;
    let [field, before, after, certainty] = parts else {
        return Err("a field change carries four fields".to_owned());
    };
    let certainty = text(certainty)?;
    Ok(FieldChange {
        field: Arc::from(text(field)?),
        before: read_optional_value(before, schemas)?,
        after: read_optional_value(after, schemas)?,
        certainty: ChangeCertainty::from_name(certainty)
            .ok_or_else(|| format!("`{certainty}` is not a change certainty"))?,
    })
}

// ---- evidence ---------------------------------------------------------------------------------

/// The columns of one evidence row.
pub(crate) struct EvidenceColumns {
    pub(crate) evidence_id: String,
    pub(crate) source: String,
    pub(crate) observed_nanos: i64,
    pub(crate) scope: String,
    pub(crate) subject: Option<String>,
    pub(crate) strength: String,
    pub(crate) body: Vec<u8>,
}

pub(crate) fn evidence_columns(evidence: &Evidence) -> Decoded<EvidenceColumns> {
    let body = seal(Cbor::Array(vec![
        optional_instant(evidence.source_time),
        claim(&evidence.claim),
        evidence.raw_ref.as_ref().map_or(Cbor::Null, |raw| {
            Cbor::Array(vec![
                Cbor::Text(raw.source().as_str().to_owned()),
                Cbor::Text(raw.handle().to_owned()),
            ])
        }),
        Cbor::Array(
            evidence
                .derived_from
                .iter()
                .map(|id| Cbor::Text(id.as_str().to_owned()))
                .collect(),
        ),
        provenance(&evidence.provenance),
    ]))?;
    Ok(EvidenceColumns {
        evidence_id: evidence.evidence_id.as_str().to_owned(),
        source: evidence.source.as_str().to_owned(),
        observed_nanos: nanos(evidence.observed_at)?,
        scope: scope_path(&evidence.scope),
        subject: evidence.subject.as_ref().map(|id| id.as_str().to_owned()),
        strength: evidence.strength.as_str().to_owned(),
        body,
    })
}

pub(crate) fn read_evidence(
    columns: &EvidenceColumns,
    schemas: &SchemaRegistry,
) -> Decoded<Evidence> {
    let body = unseal(&columns.body)?;
    let parts = array(&body)?;
    let [source_time, claim, raw_ref, derived, prov] = parts else {
        return Err(format!(
            "an evidence body carries {} fields rather than five",
            parts.len()
        ));
    };
    let mut chain = Vec::new();
    for item in array(derived)? {
        let raw = text(item)?;
        chain.push(EvidenceId::parse(raw).ok_or_else(|| format!("`{raw}` is not an evidence id"))?);
    }
    let source = read_source(&columns.source)?;
    Ok(Evidence {
        evidence_id: EvidenceId::parse(&columns.evidence_id)
            .ok_or_else(|| format!("`{}` is not an evidence id", columns.evidence_id))?,
        source,
        observed_at: from_nanos(columns.observed_nanos)?,
        source_time: read_optional_instant(source_time)?,
        scope: read_scope_path(&columns.scope)?,
        subject: columns
            .subject
            .as_deref()
            .map(read_spatial_id)
            .transpose()?,
        claim: read_claim(claim, schemas)?,
        strength: EvidenceStrength::from_name(&columns.strength)
            .ok_or_else(|| format!("`{}` is not an evidence strength", columns.strength))?,
        raw_ref: match raw_ref {
            Cbor::Null => None,
            other => {
                let parts = array(other)?;
                let [source, handle] = parts else {
                    return Err("a raw reference carries a source and a handle".to_owned());
                };
                Some(OpaqueReference::new(
                    read_source(text(source)?)?,
                    text(handle)?,
                ))
            }
        },
        derived_from: chain,
        provenance: read_provenance(prov)?,
    })
}

fn claim(claim: &EvidenceClaim) -> Cbor {
    let mut parts = vec![Cbor::Text(claim.kind().to_owned())];
    match claim {
        EvidenceClaim::ObjectExisted { at } => parts.push(instant(*at)),
        EvidenceClaim::ObjectAbsent { over } => parts.push(range(*over)),
        EvidenceClaim::FieldValue {
            field,
            value: held,
            at,
        } => {
            parts.push(Cbor::Text(field.to_string()));
            parts.push(value(held));
            parts.push(instant(*at));
        }
        EvidenceClaim::RelationHeld {
            relation,
            other,
            over,
        } => {
            parts.push(Cbor::Text(relation.to_string()));
            parts.push(Cbor::Text(other.as_str().to_owned()));
            parts.push(range(*over));
        }
        EvidenceClaim::Transition {
            field,
            from,
            to,
            at,
        } => {
            parts.push(Cbor::Text(field.to_string()));
            parts.push(value(from));
            parts.push(value(to));
            parts.push(instant(*at));
        }
        EvidenceClaim::Transaction { token, at } => {
            parts.push(Cbor::Text(token.to_string()));
            parts.push(instant(*at));
        }
        EvidenceClaim::SourceStatement { text } => parts.push(Cbor::Text(text.to_string())),
    }
    Cbor::Array(parts)
}

fn read_claim(item: &Cbor, schemas: &SchemaRegistry) -> Decoded<EvidenceClaim> {
    let parts = array(item)?;
    let (kind, rest) = parts
        .split_first()
        .ok_or_else(|| "an evidence claim names no kind".to_owned())?;
    match (text(kind)?, rest) {
        ("object_existed", [at]) => Ok(EvidenceClaim::ObjectExisted {
            at: read_instant(at)?,
        }),
        ("object_absent", [over]) => Ok(EvidenceClaim::ObjectAbsent {
            over: read_range(over)?,
        }),
        ("field_value", [field, held, at]) => Ok(EvidenceClaim::FieldValue {
            field: Arc::from(text(field)?),
            value: read_value(held, schemas)?,
            at: read_instant(at)?,
        }),
        ("relation_held", [relation, other, over]) => Ok(EvidenceClaim::RelationHeld {
            relation: Arc::from(text(relation)?),
            other: read_spatial_id(text(other)?)?,
            over: read_range(over)?,
        }),
        ("transition", [field, from, to, at]) => Ok(EvidenceClaim::Transition {
            field: Arc::from(text(field)?),
            from: read_value(from, schemas)?,
            to: read_value(to, schemas)?,
            at: read_instant(at)?,
        }),
        ("transaction", [token, at]) => Ok(EvidenceClaim::Transaction {
            token: Arc::from(text(token)?),
            at: read_instant(at)?,
        }),
        ("source_statement", [statement]) => Ok(EvidenceClaim::SourceStatement {
            text: Arc::from(text(statement)?),
        }),
        (kind, _) => Err(format!("`{kind}` is not an evidence claim")),
    }
}

fn range(range: TimeRange) -> Cbor {
    Cbor::Array(vec![
        optional_instant(range.from),
        optional_instant(range.until),
    ])
}

fn read_range(item: &Cbor) -> Decoded<TimeRange> {
    let parts = array(item)?;
    let [from, until] = parts else {
        return Err("a time range carries two ends".to_owned());
    };
    Ok(TimeRange {
        from: read_optional_instant(from)?,
        until: read_optional_instant(until)?,
    })
}

// ---- causal links -----------------------------------------------------------------------------

/// The columns of one causal link row.
pub(crate) struct LinkColumns {
    pub(crate) link_id: String,
    pub(crate) relation: String,
    pub(crate) cause: String,
    pub(crate) effect: String,
    pub(crate) rule: String,
    pub(crate) strength: String,
    pub(crate) source: String,
    pub(crate) evidence: Vec<String>,
}

pub(crate) fn link_columns(link: &CausalLink) -> LinkColumns {
    LinkColumns {
        link_id: link.link_id.as_str().to_owned(),
        relation: link.relation.as_str().to_owned(),
        cause: link.cause.as_str().to_owned(),
        effect: link.effect.as_str().to_owned(),
        rule: link.rule.as_str().to_owned(),
        strength: link.strength.as_str().to_owned(),
        source: link.source.as_str().to_owned(),
        evidence: link
            .evidence
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect(),
    }
}

pub(crate) fn read_link(columns: &LinkColumns) -> Decoded<CausalLink> {
    let mut evidence = Vec::new();
    for id in &columns.evidence {
        evidence
            .push(EvidenceId::parse(id).ok_or_else(|| format!("`{id}` is not an evidence id"))?);
    }
    Ok(CausalLink {
        link_id: CausalLinkId::parse(&columns.link_id)
            .ok_or_else(|| format!("`{}` is not a causal link id", columns.link_id))?,
        relation: CausalRelation::from_name(&columns.relation)
            .ok_or_else(|| format!("`{}` is not a causal relation", columns.relation))?,
        cause: EventId::parse(&columns.cause)
            .ok_or_else(|| format!("`{}` is not an event id", columns.cause))?,
        effect: EventId::parse(&columns.effect)
            .ok_or_else(|| format!("`{}` is not an event id", columns.effect))?,
        rule: CausalRuleId::new(&columns.rule),
        evidence,
        strength: EvidenceStrength::from_name(&columns.strength)
            .ok_or_else(|| format!("`{}` is not an evidence strength", columns.strength))?,
        source: read_source(&columns.source)?,
    })
}

// ---- coverage ---------------------------------------------------------------------------------

/// The columns of one coverage interval.
pub(crate) struct CoverageColumns {
    pub(crate) scope: String,
    pub(crate) capability: String,
    pub(crate) from_nanos: i64,
    pub(crate) until_nanos: i64,
    pub(crate) completeness: String,
    pub(crate) sampling_nanos: Option<i64>,
    pub(crate) source: String,
    pub(crate) permission: String,
}

pub(crate) fn coverage_columns(interval: &TemporalCoverage) -> Decoded<CoverageColumns> {
    Ok(CoverageColumns {
        scope: scope_path(&interval.scope),
        capability: interval.capability.to_string(),
        from_nanos: nanos(interval.from)?,
        until_nanos: nanos(interval.until)?,
        completeness: interval.completeness.as_str().to_owned(),
        sampling_nanos: interval
            .sampling_interval
            .map(|span| i64::try_from(span.nanoseconds()).unwrap_or(i64::MAX)),
        source: interval.source.as_str().to_owned(),
        permission: interval.permission.as_str().to_owned(),
    })
}

pub(crate) fn read_coverage(columns: &CoverageColumns) -> Decoded<TemporalCoverage> {
    Ok(TemporalCoverage {
        scope: read_scope_path(&columns.scope)?,
        capability: Arc::from(columns.capability.as_str()),
        from: from_nanos(columns.from_nanos)?,
        until: from_nanos(columns.until_nanos)?,
        completeness: TemporalCompleteness::from_name(&columns.completeness)
            .ok_or_else(|| format!("`{}` is not a completeness", columns.completeness))?,
        sampling_interval: columns
            .sampling_nanos
            .map(|span| ono_value::Duration::from_nanoseconds(i128::from(span))),
        source: read_source(&columns.source)?,
        permission: PermissionState::from_name(&columns.permission)
            .ok_or_else(|| format!("`{}` is not a permission state", columns.permission))?,
    })
}

// ---- actions ----------------------------------------------------------------------------------

/// The columns of one action row.
pub(crate) struct ActionColumns {
    pub(crate) action_id: String,
    pub(crate) requested_nanos: i64,
    pub(crate) actor: String,
    pub(crate) session_id: String,
    pub(crate) operation: String,
    pub(crate) target: Option<String>,
    pub(crate) external_transaction: Option<String>,
    pub(crate) body: Vec<u8>,
}

pub(crate) fn action_columns(action: &ActionEvent) -> Decoded<ActionColumns> {
    let body = seal(Cbor::Array(vec![
        Cbor::Text(action.command.as_str().to_owned()),
        Cbor::Array(vec![
            Cbor::Text(action.authorization.decision.as_str().to_owned()),
            Cbor::Text(action.authorization.risk.to_string()),
            optional_string(action.authorization.capability.as_deref()),
            optional_string(action.authorization.reason.as_deref()),
        ]),
        action.result.as_ref().map_or(Cbor::Null, |result| {
            Cbor::Array(vec![
                Cbor::Text(result.outcome.as_str().to_owned()),
                instant(result.completed_at),
                optional_string(result.detail.as_deref()),
                optional_string(result.error_code.as_deref()),
            ])
        }),
        provenance(&action.provenance),
    ]))?;
    Ok(ActionColumns {
        action_id: action.action_id.as_str().to_owned(),
        requested_nanos: nanos(action.requested_at)?,
        actor: action.actor.to_string(),
        session_id: action.session_id.to_string(),
        operation: action.operation.to_string(),
        target: action.target.as_ref().map(|id| id.as_str().to_owned()),
        external_transaction: action
            .external_transaction
            .as_ref()
            .map(ToString::to_string),
        body,
    })
}

pub(crate) fn read_action(columns: &ActionColumns) -> Decoded<ActionEvent> {
    let body = unseal(&columns.body)?;
    let parts = array(&body)?;
    let [command, authorization, result, prov] = parts else {
        return Err(format!(
            "an action body carries {} fields rather than four",
            parts.len()
        ));
    };
    let authorization = array(authorization)?;
    let [decision, risk, capability, reason] = authorization else {
        return Err("an authorisation carries four fields".to_owned());
    };
    let decision_name = text(decision)?;
    Ok(ActionEvent {
        action_id: ActionId::parse(&columns.action_id)
            .ok_or_else(|| format!("`{}` is not an action id", columns.action_id))?,
        command: RedactedCommandSummary::of(text(command)?, None, &[]),
        actor: Arc::from(columns.actor.as_str()),
        session_id: Arc::from(columns.session_id.as_str()),
        requested_at: from_nanos(columns.requested_nanos)?,
        target: columns.target.as_deref().map(read_spatial_id).transpose()?,
        operation: Arc::from(columns.operation.as_str()),
        authorization: AuthorizationSummary {
            decision: AuthorizationDecision::from_name(decision_name)
                .ok_or_else(|| format!("`{decision_name}` is not an authorisation decision"))?,
            risk: Arc::from(text(risk)?),
            capability: optional_text(capability)?.map(Arc::from),
            reason: optional_text(reason)?.map(Arc::from),
        },
        result: match result {
            Cbor::Null => None,
            other => {
                let parts = array(other)?;
                let [outcome, completed, detail, code] = parts else {
                    return Err("an action result carries four fields".to_owned());
                };
                let outcome_name = text(outcome)?;
                Some(ActionResultSummary {
                    outcome: ActionOutcome::from_name(outcome_name)
                        .ok_or_else(|| format!("`{outcome_name}` is not an action outcome"))?,
                    completed_at: read_instant(completed)?,
                    detail: optional_text(detail)?.map(Arc::from),
                    error_code: optional_text(code)?.map(Arc::from),
                })
            }
        },
        external_transaction: columns.external_transaction.as_deref().map(Arc::from),
        provenance: read_provenance(prov)?,
    })
}

// ---- checkpoints ------------------------------------------------------------------------------

/// The columns of one checkpoint header row; its objects and relations are their own sets (§31.3).
pub(crate) struct CheckpointColumns {
    pub(crate) checkpoint_id: String,
    pub(crate) scope: String,
    pub(crate) captured_nanos: i64,
    pub(crate) body: Vec<u8>,
}

/// One object inside a checkpoint, as its own row (§42.2, §42.3).
pub(crate) struct ObjectColumns {
    pub(crate) spatial_id: String,
    pub(crate) object_type: String,
    pub(crate) label: String,
    pub(crate) observed_nanos: i64,
    pub(crate) source: String,
    pub(crate) body: Vec<u8>,
}

/// One relation inside a checkpoint, as its own row (§42.2).
pub(crate) struct RelationColumns {
    pub(crate) from_id: String,
    pub(crate) to_id: String,
    pub(crate) relation: String,
    pub(crate) confidence: String,
    pub(crate) observed_nanos: i64,
    pub(crate) source: String,
}

pub(crate) fn checkpoint_columns(checkpoint: &Checkpoint) -> Decoded<CheckpointColumns> {
    let mut coverage = Vec::new();
    for interval in &checkpoint.coverage {
        let columns = coverage_columns(interval)?;
        coverage.push(Cbor::Array(vec![
            Cbor::Text(columns.scope),
            Cbor::Text(columns.capability),
            Cbor::Integer(Integer::from(columns.from_nanos)),
            Cbor::Integer(Integer::from(columns.until_nanos)),
            Cbor::Text(columns.completeness),
            columns
                .sampling_nanos
                .map_or(Cbor::Null, |span| Cbor::Integer(Integer::from(span))),
            Cbor::Text(columns.source),
            Cbor::Text(columns.permission),
        ]));
    }
    let body = seal(Cbor::Array(vec![
        Cbor::Array(coverage),
        provenance(&checkpoint.provenance),
    ]))?;
    Ok(CheckpointColumns {
        checkpoint_id: checkpoint.checkpoint_id.as_str().to_owned(),
        scope: scope_path(&checkpoint.scope),
        captured_nanos: nanos(checkpoint.captured_at)?,
        body,
    })
}

pub(crate) fn object_columns(state: &ObjectState) -> Decoded<ObjectColumns> {
    Ok(ObjectColumns {
        spatial_id: state.id.as_str().to_owned(),
        object_type: state.object_type.as_str().to_owned(),
        label: state.label.to_string(),
        observed_nanos: nanos(state.observed_at)?,
        source: state.source.as_str().to_owned(),
        body: seal(value(&Value::Record(Arc::new(state.record.clone()))))?,
    })
}

pub(crate) fn read_object(
    columns: &ObjectColumns,
    schemas: &SchemaRegistry,
) -> Decoded<ObjectState> {
    let record = match read_value(&unseal(&columns.body)?, schemas)? {
        Value::Record(record) => RecordValue::clone(&record),
        other => {
            return Err(format!(
                "a checkpoint object holds {} rather than a record",
                other.type_name()
            ));
        }
    };
    let object_type = columns.object_type.as_str();
    Ok(ObjectState {
        id: read_spatial_id(&columns.spatial_id)?,
        object_type: SpatialType::from_name(object_type)
            .ok_or_else(|| format!("`{object_type}` is not a spatial type"))?,
        label: Arc::from(columns.label.as_str()),
        record,
        observed_at: from_nanos(columns.observed_nanos)?,
        source: read_source(&columns.source)?,
    })
}

pub(crate) fn relation_columns(state: &RelationState) -> Decoded<RelationColumns> {
    Ok(RelationColumns {
        from_id: state.from.as_str().to_owned(),
        to_id: state.to.as_str().to_owned(),
        relation: state.relation.to_string(),
        confidence: state.confidence.as_str().to_owned(),
        observed_nanos: nanos(state.observed_at)?,
        source: state.source.as_str().to_owned(),
    })
}

pub(crate) fn read_relation(columns: &RelationColumns) -> Decoded<RelationState> {
    let confidence = columns.confidence.as_str();
    Ok(RelationState {
        from: read_spatial_id(&columns.from_id)?,
        to: read_spatial_id(&columns.to_id)?,
        relation: Arc::from(columns.relation.as_str()),
        confidence: Confidence::from_name(confidence)
            .ok_or_else(|| format!("`{confidence}` is not a confidence"))?,
        observed_at: from_nanos(columns.observed_nanos)?,
        source: read_source(&columns.source)?,
    })
}

pub(crate) fn read_checkpoint(
    columns: &CheckpointColumns,
    objects: Vec<ObjectState>,
    relations: Vec<RelationState>,
) -> Decoded<Checkpoint> {
    let body = unseal(&columns.body)?;
    let parts = array(&body)?;
    let [coverage, prov] = parts else {
        return Err("a checkpoint body carries coverage and provenance".to_owned());
    };
    let mut intervals = Vec::new();
    for item in array(coverage)? {
        let fields = array(item)?;
        let [
            scope,
            capability,
            from,
            until,
            completeness,
            sampling,
            source,
            permission,
        ] = fields
        else {
            return Err("a checkpoint coverage interval carries eight fields".to_owned());
        };
        intervals.push(read_coverage(&CoverageColumns {
            scope: text(scope)?.to_owned(),
            capability: text(capability)?.to_owned(),
            from_nanos: signed(from)?,
            until_nanos: signed(until)?,
            completeness: text(completeness)?.to_owned(),
            sampling_nanos: match sampling {
                Cbor::Null => None,
                other => Some(signed(other)?),
            },
            source: text(source)?.to_owned(),
            permission: text(permission)?.to_owned(),
        })?);
    }
    Ok(Checkpoint {
        checkpoint_id: CheckpointId::parse(&columns.checkpoint_id)
            .ok_or_else(|| format!("`{}` is not a checkpoint id", columns.checkpoint_id))?,
        scope: read_scope_path(&columns.scope)?,
        captured_at: from_nanos(columns.captured_nanos)?,
        coverage: intervals,
        objects,
        relations,
        provenance: read_provenance(prov)?,
    })
}

fn signed(item: &Cbor) -> Decoded<i64> {
    match item {
        Cbor::Integer(number) => {
            i64::try_from(*number).map_err(|_| "an integer does not fit a 64-bit field".to_owned())
        }
        other => Err(format!("expected an integer, found {other:?}")),
    }
}
