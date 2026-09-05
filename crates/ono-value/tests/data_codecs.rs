//! The interop serialization of spec §33.5 and §12.3: what an external tool reads.
//!
//! `to json` and `to yaml` exist so a stream can leave Ono and be understood by something that
//! has never heard of Ono (spec §12.3, `get process | to json | external-tool`). Spec §33.5 shows
//! the shape exactly: the data, no envelope, canonical values rather than display strings. These
//! tests assert that shape. The tagged, lossless codec of ADR-0016 item 6 is a different codec
//! for a different job, and `roundtrip.rs` and `yaml_codec.rs` still hold it to its own contract.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failed precondition in a test should abort the test loudly"
)]

use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;
use ono_core::ErrorCode;
use ono_value::{
    ByteSize, Decimal, Duration, ErrorValue, FieldDef, FieldType, IpNetwork, MapValue, Percent,
    Provenance, RecordValue, RegexValue, Schema, SchemaId, Uuid, Value, to_json_data, to_yaml_data,
};
use serde_json::json;

/// A schema whose fields are the ones spec §33.5 prints.
fn process_schema() -> Arc<Schema> {
    Arc::new(
        Schema::builder(SchemaId::new("ono.process", 1), "Process")
            .field(FieldDef::new("pid", FieldType::Int).required())
            .field(FieldDef::new("name", FieldType::String).required())
            .field(FieldDef::new("cpu", FieldType::Percent).nullable())
            .field(FieldDef::new("memory", FieldType::ByteSize).nullable())
            .field(FieldDef::new(
                "user",
                FieldType::Record(SchemaId::new("ono.user", 1)),
            ))
            .identity(["pid"])
            .build()
            .unwrap(),
    )
}

fn user_schema() -> Arc<Schema> {
    Arc::new(
        Schema::builder(SchemaId::new("ono.user", 1), "User")
            .field(FieldDef::new("uid", FieldType::Int).required())
            .field(FieldDef::new("name", FieldType::String).required())
            .identity(["uid"])
            .build()
            .unwrap(),
    )
}

fn user_record(uid: i128, name: &str) -> Value {
    let schema = user_schema();
    let provenance = Provenance::local("test.fixture", schema.id().clone());
    RecordValue::builder(schema, provenance)
        .set("uid", Value::Int(uid))
        .and_then(|builder| builder.set("name", Value::string(name)))
        .unwrap()
        .build()
        .into_value()
}

/// A one-field schema, for the cases that only need a record to hang a value on.
fn holder_schema(field: FieldDef) -> Arc<Schema> {
    Arc::new(
        Schema::builder(SchemaId::new("ono.holder", 1), "Holder")
            .field(field)
            .build()
            .unwrap(),
    )
}

fn holder(field: FieldDef, value: Value) -> RecordValue {
    let name = field.name().to_owned();
    let schema = holder_schema(field);
    let provenance = Provenance::local("test.fixture", schema.id().clone());
    RecordValue::builder(schema, provenance)
        .set(&name, value)
        .unwrap()
        .build()
}

#[test]
fn should_serialize_the_example_the_specification_prints() {
    let schema = process_schema();
    let provenance = Provenance::local("ono.process", schema.id().clone());
    let record = RecordValue::builder(schema, provenance)
        .set("pid", Value::Int(812))
        .and_then(|builder| builder.set("name", Value::string("postgres")))
        .and_then(|builder| builder.set("cpu", Value::Percent(Percent::new(18.1))))
        .and_then(|builder| {
            builder.set(
                "memory",
                Value::ByteSize(ByteSize::from_bytes(1_288_490_188)),
            )
        })
        .and_then(|builder| builder.set("user", user_record(113, "postgres")))
        .unwrap()
        .build();

    assert_eq!(
        to_json_data(&Value::list([record.into_value()])),
        json!([{
            "pid": 812,
            "name": "postgres",
            "cpu": 18.1,
            "memory": 1_288_490_188_u64,
            "user": {"uid": 113, "name": "postgres"}
        }]),
        "spec §33.5 prints this document for this pipeline, byte for byte"
    );
}

#[test]
fn should_write_a_record_as_a_plain_object_without_an_envelope() {
    let record = holder(FieldDef::new("name", FieldType::String), Value::string("a"));
    let json = to_json_data(&record.into_value());

    assert_eq!(json, json!({"name": "a"}));
    let object = json.as_object().expect("a record is an object");
    assert_eq!(
        object.keys().collect::<Vec<_>>(),
        ["name"],
        "no `$record`, no `schema`, no `provenance`: provenance is reachable through `inspect` \
         (spec §10.7), not through the wire format"
    );
}

#[test]
fn should_keep_a_field_whose_value_is_unknown_as_null() {
    let record = holder(
        FieldDef::new("size", FieldType::ByteSize).nullable(),
        Value::Null,
    );

    assert_eq!(
        to_json_data(&record.into_value()),
        json!({"size": serde_json::Value::Null}),
        "spec §10.5: an unknown stays unknown, and never becomes a fabricated zero"
    );
}

#[test]
fn should_carry_the_extension_fields_a_provider_attached() {
    let record = holder(FieldDef::new("name", FieldType::String), Value::string("a"));
    let schema = record.schema().clone();
    let provenance = record.provenance().clone();
    let record = RecordValue::builder(schema, provenance)
        .set("name", Value::string("a"))
        .unwrap()
        .set_extra("cgroup", Value::string("/user.slice"))
        .build();

    assert_eq!(
        to_json_data(&record.into_value()),
        json!({"name": "a", "cgroup": "/user.slice"}),
        "an extension field is data the provider carried; dropping it would lose it silently"
    );
}

#[test]
fn should_let_the_declared_field_win_when_an_extension_shares_its_name() {
    let record = holder(FieldDef::new("name", FieldType::String), Value::string("a"));
    let schema = record.schema().clone();
    let provenance = record.provenance().clone();
    let record = RecordValue::builder(schema, provenance)
        .set("name", Value::string("declared"))
        .unwrap()
        .set_extra("name", Value::string("extension"))
        .build();

    assert_eq!(
        to_json_data(&record.into_value()),
        json!({"name": "declared"}),
        "the schema is the contract, so the field it declares is the one that is read"
    );
}

#[test]
fn should_nest_a_record_held_in_a_field_as_a_nested_object() {
    let record = holder(
        FieldDef::new("user", FieldType::Record(SchemaId::new("ono.user", 1))),
        user_record(113, "postgres"),
    );

    assert_eq!(
        to_json_data(&record.into_value()),
        json!({"user": {"uid": 113, "name": "postgres"}})
    );
}

#[test]
fn should_write_every_semantic_scalar_in_its_natural_json_form() {
    let cases: Vec<(Value, serde_json::Value)> = vec![
        (
            Value::ByteSize(ByteSize::from_bytes(1_288_490_188)),
            json!(1_288_490_188_u64),
        ),
        (Value::Percent(Percent::new(18.1)), json!(18.1)),
        (Value::Port(8080), json!(8080)),
        (
            Value::Duration(Duration::parse("250ms").unwrap()),
            json!("250000000ns"),
        ),
        (
            Value::Timestamp("2026-08-26T06:13:04.182Z".parse().unwrap()),
            json!("2026-08-26T06:13:04.182Z"),
        ),
        (
            Value::Uuid(Uuid::parse("6ba7b810-9dad-11d1-80b4-00c04fd430c8").unwrap()),
            json!("6ba7b810-9dad-11d1-80b4-00c04fd430c8"),
        ),
        (Value::Ip("10.4.2.11".parse().unwrap()), json!("10.4.2.11")),
        (
            Value::IpNetwork(IpNetwork::parse("10.4.2.0/24").unwrap()),
            json!("10.4.2.0/24"),
        ),
        (
            Value::Regex(Arc::new(RegexValue::new("^post.*$").unwrap())),
            json!("^post.*$"),
        ),
        (
            Value::Path(Arc::from(Path::new("/etc/passwd"))),
            json!("/etc/passwd"),
        ),
        (Value::Int(42), json!(42)),
        (Value::Float(0.5), json!(0.5)),
        (Value::Bool(true), json!(true)),
        (Value::string("text"), json!("text")),
        (Value::Null, serde_json::Value::Null),
    ];

    for (value, expected) in cases {
        assert_eq!(
            to_json_data(&value),
            expected,
            "spec §33.5: a {} serializes as the canonical value, with no tag around it",
            value.type_name()
        );
    }
}

#[test]
fn should_write_a_decimal_as_a_number_when_json_can_hold_it_exactly() {
    assert_eq!(
        to_json_data(&Value::Decimal(Decimal::from_int(7))),
        json!(7)
    );
    assert_eq!(
        to_json_data(&Value::Decimal(Decimal::parse("1.25").unwrap())),
        json!(1.25)
    );
}

#[test]
fn should_write_a_decimal_as_its_canonical_string_when_json_would_round_it() {
    let exact = "0.12345678901234567890123456789";

    assert_eq!(
        to_json_data(&Value::Decimal(Decimal::parse(exact).unwrap())),
        json!(exact),
        "a JSON number here would be a rounded lie; the canonical text is the honest form"
    );
}

#[test]
fn should_write_an_integer_beyond_json_as_its_canonical_string() {
    let beyond = i128::from(i64::MAX) + 1;

    assert_eq!(
        to_json_data(&Value::Int(beyond)),
        json!(beyond.to_string()),
        "JSON's number grammar cannot hold it, and a rounded number would be wrong"
    );
    assert_eq!(
        to_json_data(&Value::Int(i128::from(i64::MAX))),
        json!(i64::MAX)
    );
}

#[test]
fn should_write_a_non_finite_float_as_its_canonical_name() {
    assert_eq!(to_json_data(&Value::Float(f64::INFINITY)), json!("inf"));
    assert_eq!(
        to_json_data(&Value::Float(f64::NEG_INFINITY)),
        json!("-inf")
    );
    assert_eq!(to_json_data(&Value::Float(f64::NAN)), json!("nan"));
}

#[test]
fn should_write_bytes_that_are_not_text_as_hex() {
    assert_eq!(
        to_json_data(&Value::Bytes(Bytes::from_static(&[0xff, 0xfe, 0x00, 0x80]))),
        json!("fffe0080"),
        "spec §12.2 requires undecodable bytes never to be lost, and JSON has no byte type"
    );
}

#[test]
fn should_write_a_path_that_is_not_text_as_hex() {
    let raw: &std::ffi::OsStr = std::os::unix::ffi::OsStrExt::from_bytes(&[0x2f, 0xff, 0xfe][..]);

    assert_eq!(
        to_json_data(&Value::Path(Arc::from(Path::new(raw)))),
        json!("2ffffe"),
        "a path is bytes on Unix, and spec §12.2 forbids losing the ones that are not text"
    );
}

#[test]
fn should_show_a_reader_that_a_value_failed() {
    let error = ErrorValue::new(ErrorCode::IoPermissionDenied, "cannot read /proc/1/io");
    let json = to_json_data(&error.into_value());

    // `docs/contracts/schemas/error.v1.yaml`: the stable code in `code`, the dotted selector in
    // `name`, the kind beside them — flat, so a reader that knows nothing about Ono still finds
    // them where the schema says (ADR-0068 §2).
    assert_eq!(json["code"], json!("Ono-Sendai-E0302"));
    assert_eq!(json["name"], json!("io.permission_denied"));
    assert_eq!(json["kind"], json!("permission"));
    assert_eq!(json["message"], json!("cannot read /proc/1/io"));
    assert_eq!(
        json["metadata"],
        json!({}),
        "a failed value must stay visibly failed to a tool that knows nothing about Ono"
    );
}

#[test]
fn should_write_a_list_and_a_map_unchanged() {
    let mut map = MapValue::new();
    map.insert("size".into(), Value::ByteSize(ByteSize::from_bytes(1024)));

    assert_eq!(
        to_json_data(&Value::list([
            Value::Int(1),
            Value::Map(Arc::new(map)),
            Value::Null
        ])),
        json!([1, {"size": 1024}, serde_json::Value::Null])
    );
}

#[test]
fn should_write_the_same_data_as_yaml() {
    let record = holder(
        FieldDef::new("memory", FieldType::ByteSize),
        Value::ByteSize(ByteSize::from_bytes(1_288_490_188)),
    );

    assert_eq!(
        to_yaml_data(&Value::list([record.into_value()])).unwrap(),
        "- memory: 1288490188\n",
        "YAML is the same interop job in a different syntax (spec §12.3)"
    );
}

#[test]
fn should_write_yaml_for_the_example_the_specification_prints() {
    let schema = process_schema();
    let provenance = Provenance::local("ono.process", schema.id().clone());
    let record = RecordValue::builder(schema, provenance)
        .set("pid", Value::Int(812))
        .and_then(|builder| builder.set("name", Value::string("postgres")))
        .and_then(|builder| builder.set("cpu", Value::Percent(Percent::new(18.1))))
        .and_then(|builder| {
            builder.set(
                "memory",
                Value::ByteSize(ByteSize::from_bytes(1_288_490_188)),
            )
        })
        .and_then(|builder| builder.set("user", user_record(113, "postgres")))
        .unwrap()
        .build();

    let yaml = to_yaml_data(&record.into_value()).unwrap();

    assert!(
        !yaml.contains('$'),
        "no Ono envelope reaches the wire: {yaml}"
    );
    assert!(yaml.contains("memory: 1288490188"), "{yaml}");
    assert!(yaml.contains("name: postgres"), "{yaml}");
}
