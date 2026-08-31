//! What a cell shows when the value in it has a shape or a declared unit.
//!
//! Spec §13.4 asks a semantic scalar to render naturally and §13.2 prints `24.8%` for a `cpu`
//! the schema declares as a plain `float` — the unit lives on the field, not on the value, so
//! the renderer has to read the declaration. §13.2 prints `postgres` for a `user` column whose
//! field is a reference, and §10.5 forbids a value that *is* there from rendering as nothing:
//! a record-valued cell must show the record, never just its schema id.
//!
//! Every assertion here is about looks. `ono_value::canonical_text` is the serialisation and is
//! asserted unchanged beside them, because spec §33.5 keeps the two forms apart.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "AGENTS.md §16: a helper shared by tests states its preconditions the way a test does"
)]

use std::sync::Arc;

use jiff::tz::TimeZone;
use ono_render::Renderer;
use ono_value::{
    FieldDef, FieldType, Provenance, RecordValue, Schema, SchemaId, Unit, Value, canonical_text,
};

fn renderer() -> Renderer {
    Renderer::in_zone(TimeZone::UTC)
}

/// `ono.endpoint/1` as `docs/spec/schemas/endpoint.v1.yaml` declares it: a structural sub-record
/// with no identity of its own, and a default view of the two fields a reader reads.
fn endpoint_schema() -> Arc<Schema> {
    Arc::new(
        Schema::builder(SchemaId::new("ono.endpoint", 1), "Endpoint")
            .field(FieldDef::new("address", FieldType::Ip).nullable())
            .field(FieldDef::new("port", FieldType::Port).nullable())
            .field(FieldDef::new("path", FieldType::Path).nullable())
            .field(FieldDef::new("host", FieldType::String).nullable())
            .default_view(["address", "port"])
            .build()
            .unwrap(),
    )
}

fn endpoint() -> Value {
    let schema = endpoint_schema();
    RecordValue::builder(
        Arc::clone(&schema),
        Provenance::local("demo", SchemaId::new("ono.endpoint", 1)),
    )
    .set("address", Value::Ip("127.0.0.1".parse().unwrap()))
    .unwrap()
    .set("port", Value::Port(631))
    .unwrap()
    .set("path", Value::Null)
    .unwrap()
    .set("host", Value::Null)
    .unwrap()
    .build()
    .into_value()
}

/// `ono.user/1` as `docs/spec/schemas/user.v1.yaml` declares it, cut to the fields these tests
/// read: the uid is the identity and the name is what a person calls the account.
fn user_schema() -> Arc<Schema> {
    Arc::new(
        Schema::builder(SchemaId::new("ono.user", 1), "User")
            .field(FieldDef::new("uid", FieldType::Int).required())
            .field(FieldDef::new("name", FieldType::String).nullable())
            .identity(["uid"])
            .default_view(["uid", "name"])
            .build()
            .unwrap(),
    )
}

fn user(name: Option<&str>) -> Value {
    let schema = user_schema();
    RecordValue::builder(
        Arc::clone(&schema),
        Provenance::local("demo", SchemaId::new("ono.user", 1)),
    )
    .set("uid", Value::Int(0))
    .unwrap()
    .set("name", name.map_or(Value::Null, Value::string))
    .unwrap()
    .build()
    .into_value()
}

/// A socket-shaped record: `local` is a nested `ono.endpoint/1`, exactly as
/// `docs/spec/schemas/socket.v1.yaml` declares it.
fn socket() -> Value {
    let schema = Arc::new(
        Schema::builder(SchemaId::new("ono.socket", 1), "Socket")
            .field(FieldDef::new("protocol", FieldType::String).required())
            .field(
                FieldDef::new("local", FieldType::Record(SchemaId::new("ono.endpoint", 1)))
                    .nullable(),
            )
            .field(FieldDef::new("state", FieldType::String).nullable())
            .default_view(["protocol", "local", "state"])
            .build()
            .unwrap(),
    );
    RecordValue::builder(
        Arc::clone(&schema),
        Provenance::local("demo", SchemaId::new("ono.socket", 1)),
    )
    .set("protocol", Value::string("tcp"))
    .unwrap()
    .set("local", endpoint())
    .unwrap()
    .set("state", Value::string("listen"))
    .unwrap()
    .build()
    .into_value()
}

/// A process-shaped record: `cpu` is a `float` carrying `unit: percent` and `user` is a
/// `ref<ono.user/1>`, both as `docs/spec/schemas/process.v1.yaml` declares them.
fn process(user_field: Value) -> Value {
    let schema = Arc::new(
        Schema::builder(SchemaId::new("ono.process", 1), "Process")
            .field(FieldDef::new("pid", FieldType::Int).required())
            .field(
                FieldDef::new("cpu", FieldType::Float)
                    .nullable()
                    .with_unit(Unit::Percent),
            )
            .field(FieldDef::new("user", FieldType::Ref(SchemaId::new("ono.user", 1))).nullable())
            .identity(["pid"])
            .default_view(["pid", "cpu", "user"])
            .build()
            .unwrap(),
    );
    RecordValue::builder(
        Arc::clone(&schema),
        Provenance::local("demo", SchemaId::new("ono.process", 1)),
    )
    .set("pid", Value::Int(1))
    .unwrap()
    .set("cpu", Value::Float(2.049_129_327_151_451_4))
    .unwrap()
    .set("user", user_field)
    .unwrap()
    .build()
    .into_value()
}

fn cell_text(row: &Value, column: usize) -> String {
    let table = renderer().table(std::slice::from_ref(row));
    table.rows()[0][column].text().to_owned()
}

#[test]
fn should_render_a_nested_record_cell_as_the_fields_of_its_default_view() {
    assert_eq!(cell_text(&socket(), 1), "{address: 127.0.0.1, port: 631}");
}

#[test]
fn should_render_a_nested_record_outside_a_table_as_the_fields_of_its_default_view() {
    let cell = renderer().cell(&endpoint());
    assert_eq!(cell.text(), "{address: 127.0.0.1, port: 631}");
}

#[test]
fn should_render_every_field_of_a_nested_record_that_declares_no_default_view() {
    let schema = Arc::new(
        Schema::builder(SchemaId::new("ono.pair", 1), "Pair")
            .field(FieldDef::new("left", FieldType::Int).required())
            .field(FieldDef::new("right", FieldType::Int).required())
            .build()
            .unwrap(),
    );
    let pair = RecordValue::builder(
        Arc::clone(&schema),
        Provenance::local("demo", SchemaId::new("ono.pair", 1)),
    )
    .set("left", Value::Int(1))
    .unwrap()
    .set("right", Value::Int(2))
    .unwrap()
    .build()
    .into_value();
    assert_eq!(renderer().cell(&pair).text(), "{left: 1, right: 2}");
}

#[test]
fn should_keep_a_null_field_of_a_nested_record_visible_as_null() {
    let schema = endpoint_schema();
    let unix = RecordValue::builder(
        Arc::clone(&schema),
        Provenance::local("demo", SchemaId::new("ono.endpoint", 1)),
    )
    .set("address", Value::Null)
    .unwrap()
    .set("port", Value::Null)
    .unwrap()
    .build()
    .into_value();
    // Spec §10.5: an unknown value renders as the word `null`, never as a blank.
    assert_eq!(renderer().cell(&unix).text(), "{address: null, port: null}");
}

#[test]
fn should_render_a_reference_cell_as_the_name_of_what_it_refers_to() {
    // Spec §13.2 prints `postgres` in the USER column, not the account's whole record.
    assert_eq!(cell_text(&process(user(Some("root"))), 2), "root");
}

#[test]
fn should_render_a_reference_cell_by_its_identity_when_no_name_resolved() {
    // Spec §23.6 keeps the numeric identity of an unresolved id; a blank cell would hide it.
    assert_eq!(cell_text(&process(user(None)), 2), "{uid: 0}");
}

#[test]
fn should_render_a_percent_typed_float_as_a_percentage() {
    // §13.2's own table prints `24.8%` for a `cpu` the schema declares as a plain float.
    assert_eq!(cell_text(&process(user(Some("root"))), 1), "2.0%");
}

#[test]
fn should_round_a_percentage_to_one_decimal() {
    assert_eq!(
        renderer()
            .cell(&Value::Percent(ono_value::Percent::new(2.049_129_3)))
            .text(),
        "2.0%"
    );
}

#[test]
fn should_leave_the_serialisation_of_a_percent_typed_float_untouched() {
    // Spec §33.5: the canonical form is what every serializer uses, and it keeps every digit.
    let record = process(user(Some("root")));
    let cpu = record.as_record().unwrap().get("cpu").unwrap().clone();
    assert_eq!(canonical_text(&cpu).unwrap(), "2.0491293271514515");
}

#[test]
fn should_leave_a_float_without_a_declared_unit_at_its_own_precision() {
    // Nothing says this number is a percentage, so nothing may round it.
    let schema = Arc::new(
        Schema::builder(SchemaId::new("ono.reading", 1), "Reading")
            .field(FieldDef::new("value", FieldType::Float).required())
            .build()
            .unwrap(),
    );
    let reading = RecordValue::builder(
        Arc::clone(&schema),
        Provenance::local("demo", SchemaId::new("ono.reading", 1)),
    )
    .set("value", Value::Float(2.049_129_327_151_451_4))
    .unwrap()
    .build()
    .into_value();
    assert_eq!(cell_text(&reading, 0), "2.0491293271514515");
}

#[test]
fn should_leave_a_byte_counted_field_as_the_number_a_reader_expects() {
    // `mtu` is an `int` carrying `unit: bytes` (docs/spec/schemas/interface.v1.yaml), and an MTU
    // is read as a number of bytes, not as `64.00 KiB`. Only `percent` has no other spelling.
    let schema = Arc::new(
        Schema::builder(SchemaId::new("ono.interface", 1), "Interface")
            .field(
                FieldDef::new("mtu", FieldType::Int)
                    .required()
                    .with_unit(Unit::Bytes),
            )
            .build()
            .unwrap(),
    );
    let interface = RecordValue::builder(
        Arc::clone(&schema),
        Provenance::local("demo", SchemaId::new("ono.interface", 1)),
    )
    .set("mtu", Value::Int(65536))
    .unwrap()
    .build()
    .into_value();
    assert_eq!(cell_text(&interface, 0), "65536");
}
