//! Records built by hand, in the shape `ono_temporal_core::value` produces them (v0.5 §35).
//!
//! The crate under test depends on `ono-value` alone, so its tests need no ledger, no recorder
//! and no provider: a timeline is a record, and a record is a literal. That is the whole point of
//! §39.3's dependency rule, and building the input here is what proves it.
//!
//! The nesting convention follows the production bridge exactly: a sub-record the schema declares
//! as `record` is a `Value::Map`, and a list of objects that have a schema of their own is a list
//! of `Value::Record`. A renderer that only read one of the two would break on real input.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    dead_code,
    reason = "a test states its preconditions directly, and not every helper is used by every \
              test binary (AGENTS.md section 16)"
)]

use std::sync::Arc;

use ono_value::{MapValue, Provenance, RecordValue, SchemaId, Value, builtin_schemas};

/// The day every fixture instant falls on, so `at("12:17:51.203")` reads like the spec's examples.
pub const DAY: &str = "2026-08-31";

/// An instant from a wall clock, in UTC.
pub fn at(clock: &str) -> Value {
    Value::parse_timestamp(&format!("{DAY}T{clock}Z")).expect("a well-formed instant")
}

/// A record of a builtin schema, with the fields a test cares about set.
pub fn record(id: &str, fields: &[(&str, Value)]) -> RecordValue {
    let schema_id = SchemaId::new(id, 1);
    let schema = builtin_schemas()
        .get(&schema_id)
        .unwrap_or_else(|| panic!("`{id}/1` is embedded in this build"));
    let mut builder = RecordValue::builder(schema, Provenance::local("test", schema_id));
    for (name, value) in fields {
        // A name the schema declares is a field; anything else is a namespaced extension of
        // v0.2 §10.4, which is how a producer can hand a renderer something the contract has not
        // yet grown a field for.
        builder = match builder.clone().set(name, value.clone()) {
            Ok(builder) => builder,
            Err(_) => builder.set_extra(name, value.clone()),
        };
    }
    builder.build()
}

/// The same, as a value ready to drop into a list field.
pub fn record_value(id: &str, fields: &[(&str, Value)]) -> Value {
    Value::Record(Arc::new(record(id, fields)))
}

/// A sub-record, which the production bridge writes as a map.
pub fn map(fields: &[(&str, Value)]) -> Value {
    let mut built = MapValue::new();
    for (name, value) in fields {
        built.insert(Arc::from(*name), value.clone());
    }
    Value::Map(Arc::new(built))
}

/// The provenance sub-record every schema of §35 carries.
pub fn provenance(provider: &str) -> Value {
    map(&[
        ("provider", Value::string(provider)),
        ("observed", Value::Null),
        ("source", Value::Null),
        ("link", Value::string("local")),
        ("schema", Value::string("ono.temporal-event/1")),
    ])
}

/// A resolved event subject (§5.5).
pub fn subject(label: &str, object_type: &str) -> Value {
    map(&[
        ("resolved", Value::Bool(true)),
        ("label", Value::string(label)),
        ("spatial_id", Value::string(&format!("id:{label}"))),
        ("object_type", Value::string(object_type)),
        ("source", Value::Null),
        ("described", Value::Null),
    ])
}

/// One §6.2 field change.
pub fn field_change(field: &str, before: Value, after: Value, certainty: &str) -> Value {
    map(&[
        ("field", Value::string(field)),
        ("before", before),
        ("after", after),
        ("certainty", Value::string(certainty)),
    ])
}

/// An `ono.temporal-event/1`, with whatever a test adds on top of the required fields.
pub fn event(id: &str, kind: &str, clock: &str, label: &str, extra: &[(&str, Value)]) -> Value {
    let mut fields: Vec<(&str, Value)> = vec![
        ("event_id", Value::string(id)),
        ("reference", Value::Null),
        ("source", Value::Null),
        ("kind", Value::string(kind)),
        ("scope", Value::string("host:web01")),
        ("subject", subject(label, "service")),
        ("related", Value::list(Vec::new())),
        ("source_time", Value::Null),
        ("observed_at", at(clock)),
        ("ingested_at", at(clock)),
        ("source_sequence", Value::Null),
        ("monotonic_nanos", Value::Null),
        ("boot_id", Value::Null),
        ("host", Value::string("web01")),
        ("clock_uncertainty", Value::Null),
        ("before", Value::Null),
        ("after", Value::Null),
        ("changed_fields", Value::list(Vec::new())),
        ("evidence", Value::list(Vec::new())),
        ("causal_parents", Value::list(Vec::new())),
        ("payload", Value::Null),
        ("provenance", provenance("ono.session")),
    ];
    for (name, value) in extra {
        if let Some(slot) = fields.iter_mut().find(|(existing, _)| existing == name) {
            slot.1 = value.clone();
        } else {
            fields.push((name, value.clone()));
        }
    }
    record_value("ono.temporal-event", &fields)
}

/// An `ono.temporal-gap/1`, as a value ready for a list field.
pub fn gap(from: &str, until: &str, reason: &str, detail: Option<&str>) -> Value {
    Value::Record(Arc::new(gap_record(from, until, reason, detail)))
}

/// The same gap as the record a frame is drawn from.
pub fn gap_record(from: &str, until: &str, reason: &str, detail: Option<&str>) -> RecordValue {
    record(
        "ono.temporal-gap",
        &[
            ("scope", Value::string("host:web01")),
            ("capability", Value::string("process.existence")),
            ("from", at(from)),
            ("until", at(until)),
            ("reason", Value::string(reason)),
            ("source", Value::string("ono.recorder")),
            ("detail", detail.map_or(Value::Null, Value::string)),
        ],
    )
}

/// A composed coverage summary, in the shape `ono_temporal_core::value::coverage_summary` writes.
pub fn coverage(headline: &str, capabilities: &[(&str, &str)], gaps: Vec<Value>) -> Value {
    let states: Vec<(&str, Value)> = capabilities
        .iter()
        .map(|(name, state)| (*name, Value::string(state)))
        .collect();
    map(&[
        ("headline", Value::string(headline)),
        ("capabilities", map(&states)),
        ("gaps", Value::list(gaps)),
        (
            "sources",
            Value::list(vec![
                Value::string("linux.procfs"),
                Value::string("linux.systemd-dbus"),
            ]),
        ),
        ("from", at("12:00:00")),
        ("until", at("13:00:00")),
    ])
}

/// An `ono.temporal-timeline/1` over `events`, with `gaps` inside the window.
pub fn timeline_record(events: Vec<Value>, gaps: Vec<Value>) -> RecordValue {
    timeline_of(events, gaps, &[])
}

/// The same, with fields a test overrides.
pub fn timeline_of(events: Vec<Value>, gaps: Vec<Value>, extra: &[(&str, Value)]) -> RecordValue {
    let mut fields: Vec<(&str, Value)> = vec![
        ("scope", Value::string("host:web01")),
        ("place", Value::string("id:nginx.service")),
        ("place_label", Value::string("local/service/nginx")),
        ("from", at("12:17:00")),
        ("until", at("12:25:00")),
        ("centre", Value::Null),
        ("events", Value::list(events)),
        ("groups", Value::Null),
        ("gaps", Value::list(gaps.clone())),
        ("coverage", coverage("complete", &[], gaps)),
        ("truncated", Value::Bool(false)),
        ("provenance", provenance("ono.temporal")),
    ];
    for (name, value) in extra {
        if let Some(slot) = fields.iter_mut().find(|(existing, _)| existing == name) {
            slot.1 = value.clone();
        }
    }
    record("ono.temporal-timeline", &fields)
}

/// One §19.4 density row, in the shape `ono_temporal_query::timeline` writes it.
pub fn group_row(
    event_id: &str,
    members: &[&str],
    hidden: i128,
    from: &str,
    until: &str,
    reason: Option<&str>,
) -> Value {
    map(&[
        ("event_id", Value::string(event_id)),
        (
            "members",
            Value::list(
                members
                    .iter()
                    .map(|id| Value::string(id))
                    .collect::<Vec<_>>(),
            ),
        ),
        ("hidden", Value::Int(hidden)),
        ("from", at(from)),
        ("until", at(until)),
        ("reason", reason.map_or(Value::Null, Value::string)),
    ])
}

/// A causal link as an entry of `causal_chain` or `correlations`, carrying the endpoint event the
/// renderer needs and cannot look up (§39.3).
pub fn link(relation: &str, rule: &str, causal: bool, endpoint: Value) -> Value {
    let inverse = match relation {
        "caused_by" => "caused",
        "triggered_by" => "triggered",
        "resulted_in" => "result",
        "preceded_by" => "followed_by",
        _ => "correlated_with",
    };
    map(&[
        ("link_id", Value::string(&format!("l-{relation}"))),
        ("relation", Value::string(relation)),
        ("inverse", Value::string(inverse)),
        ("is_causal", Value::Bool(causal)),
        ("cause", Value::string("e0000000000000000000000a")),
        ("effect", Value::string("e0000000000000000000000b")),
        ("rule", Value::string(rule)),
        ("evidence", Value::list(vec![Value::string("v01")])),
        (
            "strength",
            Value::string(if causal {
                "authoritative"
            } else {
                "correlated"
            }),
        ),
        (
            "source",
            Value::string(if causal {
                "linux.systemd-dbus"
            } else {
                "ono.recorder"
            }),
        ),
        ("event", endpoint),
    ])
}

/// An `ono.causal-explanation/1`.
pub fn explanation(fields: &[(&str, Value)]) -> RecordValue {
    let schema_id = SchemaId::new("ono.causal-explanation", 1);
    let schema = builtin_schemas()
        .get(&schema_id)
        .expect("`ono.causal-explanation/1` is embedded in this build");
    let mut builder = RecordValue::builder(schema, Provenance::local("test", schema_id));
    let defaults: Vec<(&str, Value)> = vec![
        ("subject", Value::string("id:nginx.service")),
        ("explained_event", Value::string("e0000000000000000000000b")),
        ("state_or_change", Value::string("nginx.service failed")),
        ("cause", Value::Null),
        ("causal_chain", Value::list(Vec::new())),
        ("correlations", Value::list(Vec::new())),
        ("preceding", Value::list(Vec::new())),
        ("gaps", Value::list(Vec::new())),
        ("coverage", coverage("partial", &[], Vec::new())),
        ("provenance", provenance("ono.temporal")),
    ];
    for (name, value) in defaults {
        let chosen = fields
            .iter()
            .find(|(given, _)| *given == name)
            .map_or(value, |(_, given)| given.clone());
        builder = builder.set(name, chosen).expect("a declared field");
    }
    // §16.5 renders "failed at 14:03:17.004" and §16.6 renders "11s before failure", and neither
    // is derivable from the ids the schema carries, so the explained instant is a declared field
    // the producer fills.
    let stated = fields
        .iter()
        .find(|(name, _)| *name == "at")
        .map_or(Value::Null, |(_, value)| value.clone());
    builder = builder.set("at", stated).expect("a declared field");
    builder.build()
}

/// An `ono.temporal-context/1`.
pub fn context(
    mode: &str,
    clock: Option<&str>,
    marker: Option<&str>,
    coverage: Value,
) -> RecordValue {
    record(
        "ono.temporal-context",
        &[
            ("mode", Value::string(mode)),
            ("requested", Value::string("-10m")),
            ("resolved_at", clock.map_or(Value::Null, at)),
            ("coverage", coverage),
            ("anchor_event", Value::Null),
            ("marker", marker.map_or(Value::Null, Value::string)),
        ],
    )
}

/// An `ono.temporal-change/1`.
pub fn change(
    kind: &str,
    label: &str,
    object_type: &str,
    field_changes: Vec<Value>,
) -> RecordValue {
    record(
        "ono.temporal-change",
        &[
            ("change_id", Value::string(&format!("c-{label}-{kind}"))),
            ("kind", Value::string(kind)),
            ("subject", subject(label, object_type)),
            ("from_time", at("12:17:00")),
            ("to_time", at("12:25:00")),
            ("field_changes", Value::list(field_changes)),
            ("relation", Value::Null),
            ("coverage", coverage("complete", &[], Vec::new())),
            ("provenance", provenance("ono.temporal")),
        ],
    )
}

/// An `ono.temporal-change/1` about an edge rather than an object (§6.4, §13.2).
pub fn relation_change(kind: &str, from: &str, to: &str, relation: &str) -> RecordValue {
    record(
        "ono.temporal-change",
        &[
            ("change_id", Value::string(&format!("c-{relation}-{kind}"))),
            ("kind", Value::string(kind)),
            ("subject", subject(from, "process")),
            ("from_time", at("12:17:00")),
            ("to_time", at("12:25:00")),
            ("field_changes", Value::list(Vec::new())),
            (
                "relation",
                map(&[
                    ("from", subject(from, "process")),
                    ("to", subject(to, "socket")),
                    ("relation", Value::string(relation)),
                    ("confidence", Value::string("observed")),
                ]),
            ),
            ("coverage", coverage("complete", &[], Vec::new())),
            ("provenance", provenance("ono.temporal")),
        ],
    )
}

/// An `ono.recorder-status/1`.
pub fn recorder(running: bool, store: Option<&str>, extra: &[(&str, Value)]) -> RecordValue {
    let mut fields: Vec<(&str, Value)> = vec![
        ("running", Value::Bool(running)),
        ("enabled", Value::Bool(running)),
        ("since", if running { at("12:00:00") } else { Value::Null }),
        (
            "store",
            store.map_or(Value::Null, |path| {
                Value::Path(std::path::PathBuf::from(path).into())
            }),
        ),
        (
            "max_age",
            Value::Duration(ono_value::Duration::parse("24h").expect("a duration")),
        ),
        (
            "max_size",
            Value::ByteSize(ono_value::ByteSize::from_bytes(512 * 1024 * 1024)),
        ),
        (
            "checkpoint_interval",
            Value::Duration(ono_value::Duration::parse("5m").expect("a duration")),
        ),
        (
            "flush_interval",
            Value::Duration(ono_value::Duration::parse("2s").expect("a duration")),
        ),
        ("session_max_events", Value::Int(100_000)),
        ("events", Value::Int(4_812)),
        (
            "size",
            Value::ByteSize(ono_value::ByteSize::from_bytes(1_048_576)),
        ),
        ("earliest", at("12:00:00")),
        ("latest", at("12:25:00")),
        (
            "sources",
            Value::list(vec![
                Value::string("linux.procfs"),
                Value::string("linux.systemd-dbus"),
            ]),
        ),
        ("dropped", Value::Int(0)),
        ("health", Value::string("healthy")),
    ];
    for (name, value) in extra {
        if let Some(slot) = fields.iter_mut().find(|(existing, _)| existing == name) {
            slot.1 = value.clone();
        }
    }
    record("ono.recorder-status", &fields)
}
