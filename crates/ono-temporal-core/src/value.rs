//! The bridge from temporal types to the public schemas of v0.5 §35.
//!
//! This is the single place a temporal value becomes an Ono value. §36.4's drift check then has
//! one producer to compare against the contracts, and no other crate spells a temporal field
//! name by hand — which is what stopped the file schema and the interface contract drifting
//! apart in v0.2 (see `ono_value::builtin`).
//!
//! Nested structures follow the house convention v0.4 set with `spatial-place.v1.yaml`: a
//! sub-record is a `record` field carrying a map, and a list of addressable objects is a list of
//! records bound to their own schema. §9.4's `_temporal` metadata is registered as a `temporal`
//! record field with no underscore, because the object tree has none.

use std::sync::Arc;

use jiff::Timestamp;
use ono_core::ErrorCode;
use ono_spatial_core::SpatialId;
use ono_value::{
    ErrorValue, MapValue, Provenance, RecordBuilder, RecordValue, Schema, SchemaId, Value,
    builtin_schemas,
};

use crate::action::ActionEvent;
use crate::causal::CausalLink;
use crate::context::TemporalContext;
use crate::coverage::{CoverageSummary, TemporalCoverage, TemporalGap};
use crate::event::{FieldChange, SpatialRef, TemporalEvent};
use crate::evidence::{Evidence, EvidenceClaim};
use crate::source::{EvidenceSource, TemporalSourceDescription};

/// The `ono.temporal-event/1` record of §35.1, with no session reference on it.
///
/// # Errors
///
/// Returns `ono.provider_schema_violation` where the contract is not in this build, which the
/// `ono-value` contract test and `cargo xtask spec-check` both prevent from shipping.
pub fn event_record(event: &TemporalEvent) -> Result<RecordValue, ErrorValue> {
    event_record_with_reference(event, None)
}

/// The same record, carrying the short form a session minted for the event (§11.6).
///
/// `reference` is the string the session's own resolver accepts — ADR-0660's shortest prefix of
/// the digest that names one event inside that session — written without the `@` a renderer adds.
/// `None` for a persisted event and for any record not produced for a session: a reference is one
/// session's word for an event, and a stored one goes stale the moment another session shortens
/// it differently.
///
/// # Errors
///
/// Returns `ono.provider_schema_violation` where the contract is not in this build.
pub fn event_record_with_reference(
    event: &TemporalEvent,
    reference: Option<&str>,
) -> Result<RecordValue, ErrorValue> {
    let (schema, provenance) = target("ono.temporal-event")?;
    let builder = RecordValue::builder(schema, provenance);
    let related: Vec<Value> = event.related.iter().map(spatial_ref).collect();
    let changed: Vec<Value> = event.changed_fields.iter().map(field_change).collect();
    let evidence: Vec<Value> = event
        .evidence
        .iter()
        .map(|id| Value::string(&id.to_string()))
        .collect();
    let parents: Vec<Value> = event
        .causal_parents
        .iter()
        .map(|id| Value::string(&id.to_string()))
        .collect();

    let builder = put(builder, "event_id", Value::string(event.event_id.as_str()));
    let builder = put(
        builder,
        "reference",
        optional_text(reference.map(str::trim).filter(|text| !text.is_empty())),
    );
    // §7.1's list is closed, so the class is read back through `EvidenceSource::parse` rather
    // than copied: a provider name outside the vocabulary leaves the field null, and §11.5's tag
    // is then absent instead of wrong (ADR-0709).
    let builder = put(
        builder,
        "source",
        EvidenceSource::parse(event.provenance.provider())
            .map_or(Value::Null, |source| Value::string(source.as_str())),
    );
    let builder = put(builder, "kind", Value::string(event.kind.as_str()));
    let builder = put(builder, "subtype", optional_text(event.subtype.as_deref()));
    let builder = put(builder, "scope", Value::string(&event.scope.to_string()));
    let builder = put(
        builder,
        "subject",
        event.subject.as_ref().map_or(Value::Null, spatial_ref),
    );
    let builder = put(builder, "related", Value::list(related));
    let builder = put(
        builder,
        "source_time",
        event
            .times
            .source_time
            .map_or(Value::Null, Value::Timestamp),
    );
    let builder = put(
        builder,
        "observed_at",
        Value::Timestamp(event.times.observed_at),
    );
    let builder = put(
        builder,
        "ingested_at",
        Value::Timestamp(event.times.ingested_at),
    );
    let builder = put(
        builder,
        "source_sequence",
        event
            .times
            .source_sequence
            .map_or(Value::Null, |sequence| Value::Int(i128::from(sequence))),
    );
    let builder = put(
        builder,
        "monotonic_nanos",
        event
            .times
            .monotonic_nanos
            .map_or(Value::Null, |nanos| Value::Int(i128::from(nanos))),
    );
    let builder = put(
        builder,
        "boot_id",
        optional_text(event.times.domain.boot_id.as_deref()),
    );
    let builder = put(builder, "host", Value::string(&event.times.domain.host));
    let builder = put(
        builder,
        "clock_uncertainty",
        event
            .times
            .clock_uncertainty
            .map_or(Value::Null, Value::Duration),
    );
    let builder = put(
        builder,
        "before",
        event.before.clone().unwrap_or(Value::Null),
    );
    let builder = put(builder, "after", event.after.clone().unwrap_or(Value::Null));
    let builder = put(builder, "changed_fields", Value::list(changed));
    let builder = put(builder, "evidence", Value::list(evidence));
    let builder = put(builder, "causal_parents", Value::list(parents));
    let builder = put(
        builder,
        "payload",
        event.payload.clone().unwrap_or(Value::Null),
    );
    let builder = put(builder, "provenance", provenance_value(&event.provenance));
    Ok(builder.build())
}

/// The `ono.temporal-context/1` record of §35.2.
///
/// # Errors
///
/// Returns `ono.provider_schema_violation` where the contract is not in this build.
pub fn context_record(context: &TemporalContext) -> Result<RecordValue, ErrorValue> {
    let (schema, provenance) = target("ono.temporal-context")?;
    let builder = RecordValue::builder(schema, provenance);
    let coverage = match context.coverage() {
        Some(summary) => coverage_summary(summary)?,
        None => Value::Null,
    };
    let anchor = match context {
        TemporalContext::Present => Value::Null,
        TemporalContext::Historical { anchor_event, .. } => anchor_event
            .as_ref()
            .map_or(Value::Null, |id| Value::string(&id.to_string())),
    };
    let builder = put(
        builder,
        "mode",
        Value::string(if context.is_historical() {
            "historical"
        } else {
            "present"
        }),
    );
    let builder = put(
        builder,
        "requested",
        optional_text(context.requested_text()),
    );
    let builder = put(
        builder,
        "resolved_at",
        context.instant().map_or(Value::Null, Value::Timestamp),
    );
    let builder = put(builder, "coverage", coverage);
    let builder = put(builder, "anchor_event", anchor);
    let builder = put(
        builder,
        "marker",
        context.prompt_marker().map_or(Value::Null, Value::string),
    );
    Ok(builder.build())
}

/// The `ono.temporal-coverage/1` record of §35.3 — one source's claim over one interval.
///
/// # Errors
///
/// Returns `ono.provider_schema_violation` where the contract is not in this build.
pub fn coverage_record(coverage: &TemporalCoverage) -> Result<RecordValue, ErrorValue> {
    let (schema, provenance) = target("ono.temporal-coverage")?;
    let builder = RecordValue::builder(schema, provenance);
    let builder = put(builder, "scope", Value::string(&coverage.scope.to_string()));
    let builder = put(builder, "capability", Value::string(&coverage.capability));
    let builder = put(builder, "from", Value::Timestamp(coverage.from));
    let builder = put(builder, "until", Value::Timestamp(coverage.until));
    let builder = put(
        builder,
        "completeness",
        Value::string(coverage.completeness.as_str()),
    );
    let builder = put(
        builder,
        "sampling_interval",
        coverage
            .sampling_interval
            .map_or(Value::Null, Value::Duration),
    );
    let builder = put(builder, "source", Value::string(coverage.source.as_str()));
    let builder = put(
        builder,
        "permission_state",
        Value::string(coverage.permission.as_str()),
    );
    Ok(builder.build())
}

/// The `ono.temporal-gap/1` record of §7.5.
///
/// # Errors
///
/// Returns `ono.provider_schema_violation` where the contract is not in this build.
pub fn gap_record(gap: &TemporalGap) -> Result<RecordValue, ErrorValue> {
    let (schema, provenance) = target("ono.temporal-gap")?;
    let builder = RecordValue::builder(schema, provenance);
    let builder = put(builder, "scope", Value::string(&gap.scope.to_string()));
    let builder = put(builder, "capability", Value::string(&gap.capability));
    let builder = put(builder, "from", Value::Timestamp(gap.from));
    let builder = put(builder, "until", Value::Timestamp(gap.until));
    let builder = put(builder, "reason", Value::string(gap.reason.as_str()));
    let builder = put(builder, "source", Value::string(gap.source.as_str()));
    let builder = put(builder, "detail", optional_text(gap.detail.as_deref()));
    Ok(builder.build())
}

/// The `ono.temporal-evidence/1` record of §3.4 (ADR-0611).
///
/// # Errors
///
/// Returns `ono.provider_schema_violation` where the contract is not in this build.
pub fn evidence_record(evidence: &Evidence) -> Result<RecordValue, ErrorValue> {
    let (schema, provenance) = target("ono.temporal-evidence")?;
    let builder = RecordValue::builder(schema, provenance);
    let derived: Vec<Value> = evidence
        .derived_from
        .iter()
        .map(|id| Value::string(&id.to_string()))
        .collect();
    let builder = put(
        builder,
        "evidence_id",
        Value::string(evidence.evidence_id.as_str()),
    );
    let builder = put(builder, "source", Value::string(evidence.source.as_str()));
    let builder = put(
        builder,
        "observed_at",
        Value::Timestamp(evidence.observed_at),
    );
    let builder = put(
        builder,
        "source_time",
        evidence.source_time.map_or(Value::Null, Value::Timestamp),
    );
    let builder = put(builder, "scope", Value::string(&evidence.scope.to_string()));
    let builder = put(
        builder,
        "subject",
        evidence
            .subject
            .as_ref()
            .map(SpatialId::as_str)
            .map_or(Value::Null, Value::string),
    );
    let builder = put(builder, "claim_kind", Value::string(evidence.claim.kind()));
    let builder = put(builder, "claim", claim_value(&evidence.claim));
    let builder = put(
        builder,
        "strength",
        Value::string(evidence.strength.as_str()),
    );
    let builder = put(
        builder,
        "raw_ref",
        evidence.raw_ref.as_ref().map_or(Value::Null, |reference| {
            let mut map = MapValue::new();
            map.insert("source".into(), Value::string(reference.source().as_str()));
            map.insert("handle".into(), Value::string(reference.handle()));
            Value::Map(Arc::new(map))
        }),
    );
    let builder = put(builder, "derived_from", Value::list(derived));
    let builder = put(
        builder,
        "provenance",
        provenance_value(&evidence.provenance),
    );
    Ok(builder.build())
}

/// The `ono.causal-link/1` record of §15.
///
/// # Errors
///
/// Returns `ono.provider_schema_violation` where the contract is not in this build.
pub fn causal_link_record(link: &CausalLink) -> Result<RecordValue, ErrorValue> {
    let (schema, provenance) = target("ono.causal-link")?;
    let builder = RecordValue::builder(schema, provenance);
    let evidence: Vec<Value> = link
        .evidence
        .iter()
        .map(|id| Value::string(&id.to_string()))
        .collect();
    let builder = put(builder, "link_id", Value::string(link.link_id.as_str()));
    let builder = put(builder, "relation", Value::string(link.relation.as_str()));
    let builder = put(
        builder,
        "inverse",
        Value::string(link.relation.inverse_label()),
    );
    let builder = put(builder, "is_causal", Value::Bool(link.is_causal()));
    let builder = put(builder, "cause", Value::string(&link.cause.to_string()));
    let builder = put(builder, "effect", Value::string(&link.effect.to_string()));
    let builder = put(builder, "rule", Value::string(link.rule.as_str()));
    let builder = put(builder, "evidence", Value::list(evidence));
    let builder = put(builder, "strength", Value::string(link.strength.as_str()));
    let builder = put(builder, "source", Value::string(link.source.as_str()));
    Ok(builder.build())
}

/// The `ono.action-event/1` record of §17.4.
///
/// # Errors
///
/// Returns `ono.provider_schema_violation` where the contract is not in this build.
pub fn action_record(action: &ActionEvent) -> Result<RecordValue, ErrorValue> {
    let (schema, provenance) = target("ono.action-event")?;
    let builder = RecordValue::builder(schema, provenance);
    let mut authorization = MapValue::new();
    authorization.insert(
        "decision".into(),
        Value::string(action.authorization.decision.as_str()),
    );
    authorization.insert("risk".into(), Value::string(&action.authorization.risk));
    authorization.insert(
        "capability".into(),
        optional_text(action.authorization.capability.as_deref()),
    );
    authorization.insert(
        "reason".into(),
        optional_text(action.authorization.reason.as_deref()),
    );

    let result = action.result.as_ref().map_or(Value::Null, |result| {
        let mut map = MapValue::new();
        map.insert("outcome".into(), Value::string(result.outcome.as_str()));
        map.insert("completed_at".into(), Value::Timestamp(result.completed_at));
        map.insert("detail".into(), optional_text(result.detail.as_deref()));
        map.insert(
            "error_code".into(),
            optional_text(result.error_code.as_deref()),
        );
        Value::Map(Arc::new(map))
    });

    let builder = put(
        builder,
        "action_id",
        Value::string(action.action_id.as_str()),
    );
    let builder = put(builder, "command", Value::string(action.command.as_str()));
    let builder = put(builder, "actor", Value::string(&action.actor));
    let builder = put(builder, "session_id", Value::string(&action.session_id));
    let builder = put(
        builder,
        "requested_at",
        Value::Timestamp(action.requested_at),
    );
    let builder = put(
        builder,
        "target",
        action
            .target
            .as_ref()
            .map(SpatialId::as_str)
            .map_or(Value::Null, Value::string),
    );
    let builder = put(builder, "operation", Value::string(&action.operation));
    let builder = put(
        builder,
        "authorization",
        Value::Map(Arc::new(authorization)),
    );
    let builder = put(builder, "result", result);
    let builder = put(
        builder,
        "external_transaction",
        optional_text(action.external_transaction.as_deref()),
    );
    let builder = put(builder, "provenance", provenance_value(&action.provenance));
    Ok(builder.build())
}

/// The `ono.temporal-source/1` record of §21.1.
///
/// # Errors
///
/// Returns `ono.provider_schema_violation` where the contract is not in this build.
pub fn source_record(source: &TemporalSourceDescription) -> Result<RecordValue, ErrorValue> {
    let (schema, provenance) = target("ono.temporal-source")?;
    let builder = RecordValue::builder(schema, provenance);
    let capabilities = &source.capabilities;
    let builder = put(builder, "source", Value::string(source.source.as_str()));
    let builder = put(builder, "provider", Value::string(&source.provider));
    let builder = put(
        builder,
        "current_snapshot",
        Value::Bool(capabilities.current_snapshot),
    );
    let builder = put(
        builder,
        "live_events",
        Value::Bool(capabilities.live_events),
    );
    let builder = put(
        builder,
        "historical_query",
        Value::Bool(capabilities.historical_query),
    );
    let builder = put(
        builder,
        "exhaustive_events",
        Value::Bool(capabilities.exhaustive_events),
    );
    let builder = put(
        builder,
        "causal_tokens",
        Value::Bool(capabilities.causal_tokens),
    );
    let builder = put(
        builder,
        "checkpointable",
        Value::Bool(capabilities.checkpointable),
    );
    let builder = put(
        builder,
        "retained_history",
        capabilities
            .retained_history
            .map_or(Value::Null, Value::Duration),
    );
    let builder = put(
        builder,
        "availability",
        Value::string(source.availability.as_str()),
    );
    let builder = put(builder, "detail", optional_text(source.detail.as_deref()));
    Ok(builder.build())
}

/// The `temporal` sub-record a reconstructed object carries (§9.4).
///
/// A nested `record` field on the object's own schema, so nothing collides with a provider field
/// and no underscore appears in a tree that has none.
///
/// # Errors
///
/// Returns `ono.provider_schema_violation` where a contract is not in this build.
pub fn temporal_metadata(
    as_of: Timestamp,
    coverage: &CoverageSummary,
    reconstructed: bool,
    sources: &[EvidenceSource],
    gaps: &[TemporalGap],
) -> Result<Value, ErrorValue> {
    let mut rendered_gaps = Vec::with_capacity(gaps.len());
    for gap in gaps {
        rendered_gaps.push(Value::Record(Arc::new(gap_record(gap)?)));
    }
    let mut map = MapValue::new();
    map.insert("as_of".into(), Value::Timestamp(as_of));
    map.insert("coverage".into(), coverage_summary(coverage)?);
    map.insert("reconstructed".into(), Value::Bool(reconstructed));
    map.insert(
        "sources".into(),
        Value::list(
            sources
                .iter()
                .map(|source| Value::string(source.as_str()))
                .collect::<Vec<_>>(),
        ),
    );
    map.insert("gaps".into(), Value::list(rendered_gaps));
    Ok(Value::Map(Arc::new(map)))
}

/// The composed `TemporalCoverageSummary` of §8.5, as the nested record §35.2 carries.
///
/// It is a sub-record rather than a schema of its own: `ono.temporal-coverage/1` is §35.3's
/// per-source interval, and a summary is what composing many of those produces.
///
/// # Errors
///
/// Returns `ono.provider_schema_violation` where the gap contract is not in this build.
pub fn coverage_summary(summary: &CoverageSummary) -> Result<Value, ErrorValue> {
    let mut capabilities = MapValue::new();
    for (capability, state) in summary.per_capability() {
        capabilities.insert(Arc::from(capability), Value::string(state.as_str()));
    }
    let mut gaps = Vec::with_capacity(summary.gaps().len());
    for gap in summary.gaps() {
        gaps.push(Value::Record(Arc::new(gap_record(gap)?)));
    }
    let mut map = MapValue::new();
    map.insert(
        "headline".into(),
        Value::string(summary.headline().as_str()),
    );
    map.insert("capabilities".into(), Value::Map(Arc::new(capabilities)));
    map.insert("gaps".into(), Value::list(gaps));
    map.insert(
        "sources".into(),
        Value::list(
            summary
                .sources()
                .map(|source| Value::string(source.as_str()))
                .collect::<Vec<_>>(),
        ),
    );
    map.insert(
        "from".into(),
        summary.window().from.map_or(Value::Null, Value::Timestamp),
    );
    map.insert(
        "until".into(),
        summary.window().until.map_or(Value::Null, Value::Timestamp),
    );
    Ok(Value::Map(Arc::new(map)))
}

/// One §6.2 field change as the sub-record `ono.temporal-event/1` carries.
///
/// A side with no evidence is `null` and the certainty says why: §6.2 forbids overloading a null
/// side to mean arbitrary unknown, and v0.2 §10.5 keeps unknown apart from absent at access.
#[must_use]
pub fn field_change(change: &FieldChange) -> Value {
    let mut map = MapValue::new();
    map.insert("field".into(), Value::string(&change.field));
    map.insert(
        "before".into(),
        change.before.clone().unwrap_or(Value::Null),
    );
    map.insert("after".into(), change.after.clone().unwrap_or(Value::Null));
    map.insert("certainty".into(), Value::string(change.certainty.as_str()));
    Value::Map(Arc::new(map))
}

/// One event subject as the sub-record `ono.temporal-event/1` carries (§5.5).
#[must_use]
pub fn spatial_ref(subject: &SpatialRef) -> Value {
    let mut map = MapValue::new();
    map.insert("resolved".into(), Value::Bool(subject.is_resolved()));
    map.insert("label".into(), Value::string(subject.label()));
    match subject {
        SpatialRef::Resolved {
            id, object_type, ..
        } => {
            map.insert("spatial_id".into(), Value::string(id.as_str()));
            map.insert("object_type".into(), Value::string(object_type.as_str()));
            map.insert("source".into(), Value::Null);
            map.insert("described".into(), Value::Null);
        }
        SpatialRef::Unresolved { source, described } => {
            map.insert("spatial_id".into(), Value::Null);
            map.insert("object_type".into(), Value::Null);
            map.insert("source".into(), Value::string(source.as_str()));
            map.insert("described".into(), Value::string(described));
        }
    }
    Value::Map(Arc::new(map))
}

/// The schema and the provenance a record of `id` is built with.
fn target(id: &str) -> Result<(Arc<Schema>, Provenance), ErrorValue> {
    let schema_id = SchemaId::new(id, 1);
    let schema = builtin_schemas().get(&schema_id).ok_or_else(|| {
        ErrorValue::new(
            ErrorCode::ProviderSchemaViolation,
            format!("the `{id}/1` contract is not in this build"),
        )
    })?;
    Ok((schema, Provenance::local("ono.temporal", schema_id)))
}

/// Sets a field the schema declares.
///
/// A name the schema does not declare is a bug in this crate rather than something a caller can
/// cause, so the field stays unknown and the record still reaches its validation, where the
/// missing field is reported by name.
fn put(builder: RecordBuilder, name: &str, value: Value) -> RecordBuilder {
    let fallback = builder.clone();
    builder.set(name, value).unwrap_or(fallback)
}

/// Text, or null where there is none — never an empty string, which would read as a value.
fn optional_text(text: Option<&str>) -> Value {
    text.map_or(Value::Null, Value::string)
}

/// The claim of an evidence record, in the shape its kind fixes.
fn claim_value(claim: &EvidenceClaim) -> Value {
    let mut map = MapValue::new();
    match claim {
        EvidenceClaim::ObjectExisted { at } => {
            map.insert("at".into(), Value::Timestamp(*at));
        }
        EvidenceClaim::ObjectAbsent { over } => {
            map.insert(
                "from".into(),
                over.from.map_or(Value::Null, Value::Timestamp),
            );
            map.insert(
                "until".into(),
                over.until.map_or(Value::Null, Value::Timestamp),
            );
        }
        EvidenceClaim::FieldValue { field, value, at } => {
            map.insert("field".into(), Value::string(field));
            map.insert("value".into(), value.clone());
            map.insert("at".into(), Value::Timestamp(*at));
        }
        EvidenceClaim::RelationHeld {
            relation,
            other,
            over,
        } => {
            map.insert("relation".into(), Value::string(relation));
            map.insert("other".into(), Value::string(other.as_str()));
            map.insert(
                "from".into(),
                over.from.map_or(Value::Null, Value::Timestamp),
            );
            map.insert(
                "until".into(),
                over.until.map_or(Value::Null, Value::Timestamp),
            );
        }
        EvidenceClaim::Transition {
            field,
            from,
            to,
            at,
        } => {
            map.insert("field".into(), Value::string(field));
            map.insert("from".into(), from.clone());
            map.insert("to".into(), to.clone());
            map.insert("at".into(), Value::Timestamp(*at));
        }
        EvidenceClaim::Transaction { token, at } => {
            map.insert("token".into(), Value::string(token));
            map.insert("at".into(), Value::Timestamp(*at));
        }
        EvidenceClaim::SourceStatement { text } => {
            map.insert("text".into(), Value::string(text));
        }
    }
    Value::Map(Arc::new(map))
}

/// Provenance as the nested record every schema of §35 declares (v0.2 §25.2).
fn provenance_value(provenance: &Provenance) -> Value {
    let mut map = MapValue::new();
    map.insert("provider".into(), Value::string(provenance.provider()));
    map.insert(
        "observed".into(),
        provenance.observed().map_or(Value::Null, Value::Timestamp),
    );
    map.insert("source".into(), optional_text(provenance.source()));
    map.insert("link".into(), Value::string(&provenance.link().to_string()));
    map.insert(
        "schema".into(),
        Value::string(&provenance.schema().to_string()),
    );
    Value::Map(Arc::new(map))
}
