//! Provenance: v0.5 §9.4's temporal metadata, §8.5's per-capability composition carried onto
//! every reconstructed field, and §7.2's rule that nothing raises an evidence strength.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use std::sync::Arc;

use ono_spatial_core::SpatialType;
use ono_temporal_core::{
    EvidenceStrength, LedgerWrite, SessionLedger, TemporalCompleteness, value::coverage_summary,
};
use ono_temporal_reconstruct::{
    HistoricalAnswer, HistoricalAnswers, ReconstructionRequest, Reconstructor, capability,
};
use ono_value::{FieldAccess, RecordValue, Schema, SchemaId, Value};

use common::{
    changed, checkpoint, coverage, instant, object_state, point_sample, procfs, recorder, resolved,
    scope, service_record, systemd,
};

#[test]
fn should_name_the_source_of_every_field_when_an_object_is_reconstructed() {
    let record = service_record("nginx.service", "active", None);
    let id = common::identity_of(&record, SpatialType::Service, "2026-08-31T12:00:00Z");
    let ledger = SessionLedger::new();
    ledger
        .write_checkpoint(&checkpoint(
            scope(),
            "2026-08-31T12:00:00Z",
            vec![object_state(
                &record,
                SpatialType::Service,
                "nginx.service",
                "2026-08-31T12:00:00Z",
                systemd(),
            )],
            Vec::new(),
            vec![coverage(
                &capability::field(SpatialType::Service, "state"),
                "2026-08-31T12:00:00Z",
                "2026-08-31T13:00:00Z",
                TemporalCompleteness::Complete,
                systemd(),
            )],
        ))
        .expect("the checkpoint is written");
    ledger
        .append(
            &[changed(
                "2026-08-31T12:05:00Z",
                resolved(&id, SpatialType::Service, "nginx.service"),
                "state",
                Value::string("active"),
                Value::string("failed"),
            )],
            &[],
        )
        .expect("the event is appended");

    let world = Reconstructor::new(&ledger)
        .reconstruct(&ReconstructionRequest::new(
            scope(),
            instant("2026-08-31T12:10:00Z"),
        ))
        .expect("the reconstruction answers");
    let object = world.object(&id).expect("the service is reconstructed");
    let field = object.field("state").expect("the field is reconstructed");

    assert!(field.sources().any(|source| source == &recorder()));
    assert_eq!(
        field.completeness(),
        TemporalCompleteness::Complete,
        "§8.5: coverage travels with the field it describes",
    );
    assert!(object.sources().any(|source| source == &systemd()));
}

#[test]
fn should_keep_the_objects_own_schema_when_the_temporal_metadata_is_attached() {
    let record = service_record("nginx.service", "active", None);
    let id = common::identity_of(&record, SpatialType::Service, "2026-08-31T12:00:00Z");
    let ledger = SessionLedger::new();
    ledger
        .write_checkpoint(&checkpoint(
            scope(),
            "2026-08-31T12:00:00Z",
            vec![object_state(
                &record,
                SpatialType::Service,
                "nginx.service",
                "2026-08-31T12:00:00Z",
                systemd(),
            )],
            Vec::new(),
            vec![point_sample(
                &capability::existence(SpatialType::Service),
                "2026-08-31T12:00:00Z",
                systemd(),
            )],
        ))
        .expect("the checkpoint is written");

    let world = Reconstructor::new(&ledger)
        .reconstruct(&ReconstructionRequest::new(
            scope(),
            instant("2026-08-31T12:00:00Z"),
        ))
        .expect("the reconstruction answers");
    let attached = world
        .object(&id)
        .expect("the service is reconstructed")
        .to_record()
        .expect("the record carries its metadata");

    assert_eq!(attached.schema_id(), &SchemaId::new("ono.service", 1));
    assert_eq!(
        attached.access("name"),
        FieldAccess::Known(Value::string("nginx.service")),
        "§9.4: a reconstructed object keeps its canonical schema and its provider fields",
    );
    let temporal = ono_temporal_reconstruct::temporal_of(&attached)
        .expect("§9.4: the metadata travels with the object");
    let map = temporal.as_map().expect("the metadata is a record");
    assert_eq!(
        map.get("as_of"),
        Some(&Value::Timestamp(instant("2026-08-31T12:00:00Z"))),
    );
    assert_eq!(map.get("reconstructed"), Some(&Value::Bool(true)));
}

#[test]
fn should_use_the_declared_field_when_the_objects_schema_carries_one_named_temporal() {
    let id = SchemaId::new("dev.example.thing", 1);
    let schema = Schema::builder(id.clone(), "Thing")
        .field(ono_value::FieldDef::new("name", ono_value::FieldType::String).required())
        .field(ono_value::FieldDef::new("temporal", ono_value::FieldType::Map).nullable())
        .identity(["name"])
        .build()
        .expect("the schema builds");
    let record = RecordValue::builder(
        Arc::new(schema),
        ono_value::Provenance::local("fixture", id.clone()),
    )
    .set("name", Value::string("thing"))
    .expect("a declared field")
    .build();

    let attached = ono_temporal_reconstruct::attach_temporal(&record, Value::Bool(true))
        .expect("the metadata attaches");
    assert_eq!(
        attached.access("temporal"),
        FieldAccess::Known(Value::Bool(true))
    );
    assert_eq!(
        ono_temporal_reconstruct::temporal_of(&attached),
        Some(Value::Bool(true)),
    );
}

#[test]
fn should_keep_the_weaker_strength_when_a_provider_answer_merges_with_a_replayed_value() {
    let record = service_record("nginx.service", "active", None);
    let id = common::identity_of(&record, SpatialType::Service, "2026-08-31T12:00:00Z");
    let ledger = SessionLedger::new();
    ledger
        .write_checkpoint(&checkpoint(
            scope(),
            "2026-08-31T12:00:00Z",
            vec![object_state(
                &record,
                SpatialType::Service,
                "nginx.service",
                "2026-08-31T12:00:00Z",
                systemd(),
            )],
            Vec::new(),
            vec![point_sample(
                &capability::existence(SpatialType::Service),
                "2026-08-31T12:00:00Z",
                systemd(),
            )],
        ))
        .expect("the checkpoint is written");

    let answers = HistoricalAnswers::of(vec![
        HistoricalAnswer::new(
            id.clone(),
            SpatialType::Service,
            "nginx.service",
            service_record("nginx.service", "reloading", None),
            instant("2026-08-31T12:00:00Z"),
            procfs(),
        )
        .with_strength(EvidenceStrength::Observational)
        .valid_over(
            Some(instant("2026-08-31T11:55:00Z")),
            Some(instant("2026-08-31T12:05:00Z")),
        ),
    ]);

    let world = Reconstructor::new(&ledger)
        .with_answers(&answers)
        .reconstruct(&ReconstructionRequest::new(
            scope(),
            instant("2026-08-31T12:00:00Z"),
        ))
        .expect("the reconstruction answers");
    let field = world
        .object(&id)
        .expect("the service is reconstructed")
        .field("state")
        .expect("the field is reconstructed");

    assert_eq!(
        field.strength(),
        EvidenceStrength::Observational,
        "§7.2: no API raises an evidence strength",
    );
    assert_eq!(field.value(), Some(&Value::string("reloading")));
    assert_eq!(field.valid_from(), Some(instant("2026-08-31T11:55:00Z")));
    assert_eq!(field.valid_until(), Some(instant("2026-08-31T12:05:00Z")));
}

#[test]
fn should_carry_the_composed_coverage_when_the_world_reports_its_own_metadata() {
    let ledger = SessionLedger::new();
    ledger
        .record_coverage(&[point_sample(
            &capability::existence(SpatialType::Process),
            "2026-08-31T12:00:00Z",
            recorder(),
        )])
        .expect("the coverage is recorded");

    let world = Reconstructor::new(&ledger)
        .reconstruct(&ReconstructionRequest::new(
            scope(),
            instant("2026-08-31T12:00:00Z"),
        ))
        .expect("the reconstruction answers");

    let rendered = coverage_summary(world.coverage()).expect("the summary renders");
    let map = rendered.as_map().expect("the summary is a map");
    assert_eq!(map.get("headline"), Some(&Value::string("partial")));
}
