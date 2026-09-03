//! Schema validation as a provider conformance boundary (spec §25, §27.3, §35.3).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed precondition in a test should abort the test loudly"
)]

use std::sync::Arc;

use ono_core::ErrorCode;
use ono_value::{
    ErrorValue, FieldDef, FieldType, Provenance, RecordValue, Schema, SchemaId, SchemaRegistry,
    Unit, Value,
};

fn schema() -> Arc<Schema> {
    Arc::new(
        Schema::builder(SchemaId::new("ono.test.thing", 1), "Thing")
            .field(FieldDef::new("id", FieldType::Int).required())
            .field(FieldDef::new("name", FieldType::String).required())
            .field(
                FieldDef::new("load", FieldType::Float)
                    .nullable()
                    .with_unit(Unit::Percent),
            )
            .identity(["id"])
            .default_view(["id", "name"])
            .build()
            .unwrap(),
    )
}

fn builder() -> ono_value::RecordBuilder {
    let schema = schema();
    let provenance = Provenance::local("test", schema.id().clone());
    RecordValue::builder(schema, provenance)
}

#[test]
fn should_accept_a_record_that_fills_every_required_field() {
    let record = builder()
        .set("id", Value::Int(1))
        .unwrap()
        .set("name", Value::String("thing".into()))
        .unwrap()
        .build();

    record.validate().expect("a complete record must validate");
}

#[test]
fn should_reject_a_record_that_leaves_a_required_field_unknown() {
    let record = builder().set("id", Value::Int(1)).unwrap().build();

    let error = record
        .validate()
        .expect_err("a required field left null must be a schema violation");

    assert_eq!(error.code(), ErrorCode::ProviderSchemaViolation);
    assert!(
        error.to_string().contains("name"),
        "the violation must name the offending field, got {error}"
    );
}

#[test]
fn should_reject_a_record_whose_field_has_the_wrong_type() {
    let record = builder()
        .set("id", Value::String("one".into()))
        .unwrap()
        .set("name", Value::String("thing".into()))
        .unwrap()
        .build();

    let error = record
        .validate()
        .expect_err("a string in an int field must be a schema violation");

    assert_eq!(error.code(), ErrorCode::ProviderSchemaViolation);
    assert!(
        error.to_string().contains("id"),
        "the violation must name the offending field, got {error}"
    );
}

#[test]
fn should_accept_a_nullable_field_left_unknown() {
    let record = builder()
        .set("id", Value::Int(1))
        .unwrap()
        .set("name", Value::String("thing".into()))
        .unwrap()
        .set("load", Value::Null)
        .unwrap()
        .build();

    record
        .validate()
        .expect("unknown data is null, and null is legal for a nullable field");
}

#[test]
fn should_accept_a_field_that_carries_an_access_failure() {
    let record = builder()
        .set("id", Value::Int(1))
        .unwrap()
        .set("name", Value::String("thing".into()))
        .unwrap()
        .set(
            "load",
            Value::Error(Arc::new(ErrorValue::new(
                ErrorCode::IoPermissionDenied,
                "access denied",
            ))),
        )
        .unwrap()
        .build();

    record
        .validate()
        .expect("a represented permission failure is not a schema violation");
}

#[test]
fn should_accept_provider_specific_extras_that_do_not_collide_with_the_schema() {
    let record = builder()
        .set("id", Value::Int(1))
        .unwrap()
        .set("name", Value::String("thing".into()))
        .unwrap()
        .set_extra("dev.example.probe", Value::Int(3))
        .build();

    record.validate().expect("namespaced extras are allowed");
    assert_eq!(
        record.get("dev.example.probe"),
        Some(&Value::Int(3)),
        "an extra must be readable by name"
    );
}

#[test]
fn should_reject_an_extra_that_shadows_a_schema_field() {
    let record = builder()
        .set("id", Value::Int(1))
        .unwrap()
        .set("name", Value::String("thing".into()))
        .unwrap()
        .set_extra("name", Value::Int(3))
        .build();

    let error = record
        .validate()
        .expect_err("an extra may not shadow a declared field");

    assert_eq!(error.code(), ErrorCode::ProviderSchemaViolation);
}

#[test]
fn should_reject_setting_a_field_the_schema_does_not_declare() {
    let error = builder()
        .set("cpy", Value::Int(1))
        .expect_err("`cpy` is not a field of Thing");

    assert_eq!(error.code(), ErrorCode::TypeUnknownField);
}

#[test]
fn should_reject_a_record_whose_schema_is_not_registered() {
    let record = builder()
        .set("id", Value::Int(1))
        .unwrap()
        .set("name", Value::String("thing".into()))
        .unwrap()
        .build();

    let error = SchemaRegistry::new()
        .validate(&record)
        .expect_err("an empty registry knows no schema");

    assert_eq!(error.code(), ErrorCode::ResolveTargetNotFound);
}

#[test]
fn should_refuse_to_register_two_different_schemas_under_one_id() {
    let mut registry = SchemaRegistry::new();
    registry
        .register(
            Schema::builder(SchemaId::new("ono.test.thing", 1), "Thing")
                .field(FieldDef::new("id", FieldType::Int).required())
                .build()
                .unwrap(),
        )
        .unwrap();

    let error = registry
        .register(
            Schema::builder(SchemaId::new("ono.test.thing", 1), "Other")
                .field(FieldDef::new("id", FieldType::Int).required())
                .build()
                .unwrap(),
        )
        .expect_err("one schema id must not name two contracts");

    assert_eq!(error.code(), ErrorCode::ResolveAmbiguous);
}

#[test]
fn should_reject_a_schema_whose_identity_names_no_field() {
    let error = Schema::builder(SchemaId::new("ono.test.thing", 1), "Thing")
        .field(FieldDef::new("id", FieldType::Int).required())
        .identity(["nope"])
        .build()
        .expect_err("identity must be made of declared fields");

    assert_eq!(error.code(), ErrorCode::TypeUnknownField);
}

#[test]
fn should_report_the_identity_values_of_a_record() {
    let record = builder()
        .set("id", Value::Int(42))
        .unwrap()
        .set("name", Value::String("thing".into()))
        .unwrap()
        .build();

    let identity = record.identity();

    assert_eq!(identity.len(), 1);
    assert_eq!(identity.get("id"), Some(&Value::Int(42)));
}

/// A schema whose identity can be incomplete, and which names what tells those records apart
/// (ADR-0553).
fn schema_with_a_fallback() -> Arc<Schema> {
    Arc::new(
        Schema::builder(SchemaId::new("ono.test.thing", 1), "Thing")
            .field(FieldDef::new("id", FieldType::Int).nullable())
            .field(FieldDef::new("name", FieldType::String).required())
            .field(FieldDef::new("slot", FieldType::String).nullable())
            .identity(["id"])
            .identity_fallback(["slot"])
            .build()
            .unwrap(),
    )
}

fn record_with_a_fallback(id: Value, slot: &str) -> RecordValue {
    RecordValue::builder(
        schema_with_a_fallback(),
        Provenance::local("test", SchemaId::new("ono.test.thing", 1)),
    )
    .set("id", id)
    .unwrap()
    .set("name", Value::String("thing".into()))
    .unwrap()
    .set("slot", Value::String(slot.into()))
    .unwrap()
    .build()
}

#[test]
fn should_identify_a_record_by_its_declared_identity_alone_when_that_identity_is_complete() {
    let identity = record_with_a_fallback(Value::Int(42), "left").identity();

    assert_eq!(
        identity.len(),
        1,
        "the fallback is for records that need it"
    );
    assert_eq!(identity.get("id"), Some(&Value::Int(42)));
}

#[test]
fn should_tell_two_records_apart_by_the_fallback_when_their_declared_identity_is_null() {
    let left = record_with_a_fallback(Value::Null, "left").identity();
    let right = record_with_a_fallback(Value::Null, "right").identity();

    assert_eq!(left.get("slot"), Some(&Value::String("left".into())));
    assert_ne!(left, right, "two different things are not one object");
}

#[test]
fn should_reject_a_schema_whose_identity_fallback_names_no_field() {
    let error = Schema::builder(SchemaId::new("ono.test.thing", 1), "Thing")
        .field(FieldDef::new("id", FieldType::Int).required())
        .identity(["id"])
        .identity_fallback(["nope"])
        .build()
        .expect_err("an identity fallback must be made of declared fields");

    assert_eq!(error.code(), ErrorCode::TypeUnknownField);
}
