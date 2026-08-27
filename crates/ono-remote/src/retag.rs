//! Stamping arriving values with the host they came from (spec §25.2).
//!
//! A record leaves the remote provider with `link: Local` in its provenance, because over there
//! it *was* local. On arrival that claim is no longer true, and spec §25.2 requires that
//! provenance say where an observation was really made — it is what makes `inspect` on a remote
//! object trustworthy, and what the remote-context prompt of spec §14.4 will lean on. So every
//! value crossing into this shell is re-tagged: `Local` becomes `Remote(host)`, named as the
//! user named the host (`ono_value::Link` documents that convention).
//!
//! A record that already carries a `Remote` link is left alone. That happens when the remote
//! shell was itself linked onward: the truthful origin is the machine that observed the record,
//! not the last hop it travelled through.
//!
//! Everything else in the provenance — provider, observation time, source, confidence — is
//! preserved, because none of it stopped being true in transit.

use std::sync::Arc;

use ono_value::{Provenance, RecordValue, Value};

/// Re-tags every record in `value`, recursing through lists, maps and nested records.
/// Marks every record inside `value` as observed across the link to `host` (spec §21.2).
pub fn retag_value(value: Value, host: &Arc<str>) -> Value {
    match value {
        Value::Record(record) => Value::Record(Arc::new(retag_record(&record, host))),
        Value::List(items) => Value::List(
            items
                .iter()
                .map(|item| retag_value(item.clone(), host))
                .collect(),
        ),
        Value::Map(map) => Value::Map(Arc::new(
            map.iter()
                .map(|(key, item)| (Arc::from(key), retag_value(item.clone(), host)))
                .collect(),
        )),
        other => other,
    }
}

/// The record with its provenance saying it was observed across the link to `host`.
pub(crate) fn retag_record(record: &RecordValue, host: &Arc<str>) -> RecordValue {
    let provenance = retag_provenance(record.provenance(), host);
    let schema = Arc::clone(record.schema());
    let mut builder = RecordValue::builder(Arc::clone(&schema), provenance);
    for field in schema.fields() {
        if let Some(value) = record.get(field.name()) {
            match builder.set(field.name(), retag_value(value.clone(), host)) {
                Ok(next) => builder = next,
                // The name came from the record's own schema, so it cannot be unknown to it;
                // if it somehow were, the untouched original is more truthful than a panic.
                Err(_) => return record.clone(),
            }
        }
    }
    for (key, value) in record.extra() {
        builder = builder.set_extra(key, retag_value(value.clone(), host));
    }
    builder.build()
}

/// The event, with the object it carries re-tagged to `host`.
///
/// An event without a value cannot be rebuilt around a re-tagged record, so it is passed
/// through unchanged; every constructor of [`ObjectEvent`](ono_provider_api::ObjectEvent)
/// carries one, so the case does not arise from this crate's own peers.
pub(crate) fn retag_event(
    event: ono_provider_api::ObjectEvent,
    host: &Arc<str>,
) -> ono_provider_api::ObjectEvent {
    use ono_provider_api::{EventKind, ObjectEvent};
    let Some(record) = event.value() else {
        return event;
    };
    let record = retag_record(record, host);
    let retagged = match event.kind() {
        EventKind::Snapshot => ObjectEvent::snapshot(&record),
        EventKind::Added => ObjectEvent::added(&record),
        EventKind::Changed => ObjectEvent::changed(
            &record,
            event.changed_fields().unwrap_or_default().iter().cloned(),
        ),
        EventKind::Removed => ObjectEvent::removed(&record),
    };
    match event.sequence() {
        Some(sequence) => retagged.with_sequence(sequence),
        None => retagged,
    }
}

fn retag_provenance(provenance: &Provenance, host: &Arc<str>) -> Provenance {
    if let ono_value::Link::Remote(_) = provenance.link() {
        // Observed on a machine beyond this link: the original origin stays the origin.
        return provenance.clone();
    }
    let mut retagged = Provenance::remote(provenance.provider(), host, provenance.schema().clone());
    if let Some(observed) = provenance.observed() {
        retagged = retagged.observed_at(observed);
    }
    if let Some(source) = provenance.source() {
        retagged = retagged.from_source(source);
    }
    if let Some(confidence) = provenance.confidence() {
        retagged = retagged.with_confidence(confidence);
    }
    retagged
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "a test states its preconditions; a failed precondition should abort loudly"
    )]

    use super::*;
    use ono_value::{FieldDef, FieldType, Schema, SchemaId};

    fn record() -> RecordValue {
        let schema = Arc::new(
            Schema::builder(SchemaId::new("ono.test.retag", 1), "Retag")
                .field(FieldDef::new("pid", FieldType::Int).required())
                .identity(["pid"])
                .build()
                .expect("the test schema is well formed"),
        );
        let provenance = Provenance::local("test.provider", schema.id().clone())
            .observed_at("2026-08-26T10:00:00Z".parse().expect("a fixed timestamp"))
            .from_source("/proc/7/status")
            .with_confidence(0.5);
        RecordValue::builder(schema, provenance)
            .set("pid", Value::Int(7))
            .expect("pid is a field")
            .build()
    }

    #[test]
    fn should_preserve_everything_but_the_link_when_retagging_a_local_record() {
        let host: Arc<str> = Arc::from("db-1");
        let retagged = retag_record(&record(), &host);

        assert_eq!(
            retagged.provenance().link(),
            &ono_value::Link::Remote("db-1".into())
        );
        assert_eq!(retagged.provenance().provider(), "test.provider");
        assert_eq!(retagged.provenance().source(), Some("/proc/7/status"));
        assert_eq!(retagged.provenance().confidence(), Some(0.5));
        assert_eq!(
            retagged.provenance().observed(),
            record().provenance().observed()
        );
        assert_eq!(retagged.get("pid"), Some(&Value::Int(7)));
    }

    #[test]
    fn should_leave_a_record_from_a_further_hop_attributed_to_its_origin() {
        let base = record();
        let schema = Arc::clone(base.schema());
        let origin = Provenance::remote("test.provider", "far-host", schema.id().clone());
        let hopped = RecordValue::builder(schema, origin)
            .set("pid", Value::Int(7))
            .expect("pid is a field")
            .build();

        let host: Arc<str> = Arc::from("near-host");
        let retagged = retag_record(&hopped, &host);
        assert_eq!(
            retagged.provenance().link(),
            &ono_value::Link::Remote("far-host".into()),
            "the origin is the machine that observed the record, not the last hop"
        );
    }
}
