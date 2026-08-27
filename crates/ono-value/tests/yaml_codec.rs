//! The YAML codec of spec §7.1 (`to`/`from`) and §13.6.
//!
//! YAML is the tagged codec of ADR-0016 item 6 written in a different syntax: it carries exactly
//! what JSON carries, so every test here asserts that a value comes back as the value it was.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failed precondition in a test should abort the test loudly"
)]

use std::sync::Arc;

use bytes::Bytes;
use ono_core::ErrorCode;
use ono_value::{
    ByteSize, Duration, ErrorValue, FieldDef, FieldType, MapValue, Provenance, RecordValue, Schema,
    SchemaId, SchemaRegistry, Value, from_yaml, to_yaml,
};

fn registry() -> SchemaRegistry {
    let mut schemas = SchemaRegistry::new();
    schemas
        .register(
            Schema::builder(SchemaId::new("ono.demo", 1), "Demo")
                .field(FieldDef::new("name", FieldType::String).required())
                .field(FieldDef::new("memory", FieldType::ByteSize).nullable())
                .identity(["name"])
                .build()
                .unwrap(),
        )
        .unwrap();
    schemas
}

fn round_trip(value: &Value) -> Value {
    let text = to_yaml(value).expect("the value should serialize");
    from_yaml(&text, &registry()).expect("the document should parse")
}

#[test]
fn should_keep_a_byte_size_a_byte_size_when_yaml_round_trips_it() {
    let value = Value::ByteSize(ByteSize::from_bytes(1_288_490_188));
    assert_eq!(round_trip(&value), value);
}

#[test]
fn should_keep_a_duration_a_duration_when_yaml_round_trips_it() {
    let value = Value::Duration(Duration::parse("4d 3h").unwrap());
    assert_eq!(round_trip(&value), value);
}

#[test]
fn should_carry_bytes_that_are_not_valid_utf8_through_yaml() {
    let value = Value::Bytes(Bytes::from_static(&[0xff, 0x00, 0xfe, 0x80]));
    assert_eq!(round_trip(&value), value);
}

#[test]
fn should_show_null_as_null_when_yaml_is_written() {
    let text = to_yaml(&Value::Null).unwrap();
    assert_eq!(
        text.trim(),
        "null",
        "spec §10.5: an unknown is never an empty string"
    );
    assert_eq!(round_trip(&Value::Null), Value::Null);
}

#[test]
fn should_keep_an_empty_string_apart_from_null_when_yaml_round_trips_them() {
    let value = Value::list([Value::string(""), Value::Null]);
    assert_eq!(round_trip(&value), value);
}

#[test]
fn should_keep_a_string_that_looks_like_a_number_a_string() {
    let value = Value::list([
        Value::string("42"),
        Value::string("true"),
        Value::string("null"),
        Value::string("~"),
        Value::string("2026-08-26"),
    ]);
    assert_eq!(round_trip(&value), value);
}

#[test]
fn should_keep_an_integer_apart_from_a_float_when_yaml_round_trips_them() {
    let value = Value::list([Value::Int(4), Value::Float(4.0)]);
    assert_eq!(round_trip(&value), value);
}

#[test]
fn should_round_trip_a_record_when_the_schema_is_registered() {
    let schemas = registry();
    let schema = schemas.get(&SchemaId::new("ono.demo", 1)).unwrap();
    let record = RecordValue::builder(
        schema,
        Provenance::local("demo", SchemaId::new("ono.demo", 1)),
    )
    .set("name", Value::string("postgres"))
    .unwrap()
    .set(
        "memory",
        Value::ByteSize(ByteSize::from_bytes(1_288_490_188)),
    )
    .unwrap()
    .build()
    .into_value();

    let text = to_yaml(&record).unwrap();
    assert_eq!(from_yaml(&text, &schemas).unwrap(), record);
}

#[test]
fn should_report_an_unregistered_schema_when_yaml_names_one() {
    let schemas = registry();
    let schema = schemas.get(&SchemaId::new("ono.demo", 1)).unwrap();
    let record = RecordValue::builder(
        schema,
        Provenance::local("demo", SchemaId::new("ono.demo", 1)),
    )
    .set("name", Value::string("postgres"))
    .unwrap()
    .build()
    .into_value();
    let text = to_yaml(&record).unwrap();

    let error = from_yaml(&text, &SchemaRegistry::new()).expect_err("no schema is registered");
    assert_eq!(error.code(), ErrorCode::ResolveTargetNotFound);
}

#[test]
fn should_read_plain_yaml_written_by_another_tool() {
    let value = from_yaml(
        "name: nginx\nport: 80\nopen: true\ntags:\n  - web\n  - edge\n",
        &registry(),
    )
    .unwrap();
    let map = value.as_map().unwrap();
    assert_eq!(map.get("name"), Some(&Value::string("nginx")));
    assert_eq!(map.get("port"), Some(&Value::Int(80)));
    assert_eq!(map.get("open"), Some(&Value::Bool(true)));
    assert_eq!(
        map.get("tags"),
        Some(&Value::list([Value::string("web"), Value::string("edge")]))
    );
}

#[test]
fn should_report_a_syntax_error_when_the_text_is_not_yaml() {
    let error = from_yaml("a: [1, 2\nb: {", &registry()).expect_err("this is not YAML");
    assert_eq!(error.code(), ErrorCode::ParseSyntax);
}

#[test]
fn should_round_trip_a_map_whose_keys_need_quoting() {
    let mut map = MapValue::new();
    map.insert("true".into(), Value::Int(1));
    map.insert("2026-08-26".into(), Value::Int(2));
    map.insert("".into(), Value::Int(3));
    let value = Value::Map(Arc::new(map));
    assert_eq!(round_trip(&value), value);
}

#[test]
fn should_round_trip_a_structured_error_with_its_causal_chain() {
    let value = ErrorValue::new(ErrorCode::IoPermissionDenied, "access denied")
        .with_help("requires root or read capability")
        .with_source(ErrorValue::new(
            ErrorCode::ProviderUnavailable,
            "procfs is not mounted",
        ))
        .into_value();
    assert_eq!(round_trip(&value), value);
}
