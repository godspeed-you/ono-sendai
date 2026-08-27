//! The CSV codec of spec §7.1 and §12.3.
//!
//! CSV has one type — text — and no nesting. These tests pin down exactly what that costs: what
//! is written, what is refused rather than flattened, and what survives a round trip.

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
    ByteSize, FieldDef, FieldType, MapValue, Provenance, RecordValue, Schema, SchemaId, Value,
    from_csv, to_csv,
};

fn schema() -> Arc<Schema> {
    Arc::new(
        Schema::builder(SchemaId::new("ono.demo", 1), "Demo")
            .field(FieldDef::new("name", FieldType::String).required())
            .field(FieldDef::new("pid", FieldType::Int).required())
            .field(FieldDef::new("memory", FieldType::ByteSize).nullable())
            .identity(["pid"])
            .default_view(["pid", "name"])
            .build()
            .unwrap(),
    )
}

fn record(name: &str, pid: i128, memory: Option<u128>) -> Value {
    RecordValue::builder(
        schema(),
        Provenance::local("demo", SchemaId::new("ono.demo", 1)),
    )
    .set("name", Value::string(name))
    .unwrap()
    .set("pid", Value::Int(pid))
    .unwrap()
    .set(
        "memory",
        memory.map_or(Value::Null, |bytes| {
            Value::ByteSize(ByteSize::from_bytes(bytes))
        }),
    )
    .unwrap()
    .build()
    .into_value()
}

fn map(pairs: &[(&str, Value)]) -> Value {
    let mut map = MapValue::new();
    for (key, value) in pairs {
        map.insert((*key).into(), value.clone());
    }
    Value::Map(Arc::new(map))
}

#[test]
fn should_write_a_header_row_and_one_row_per_record() {
    let text = to_csv(&[
        record("postgres", 812, Some(1024)),
        record("nginx", 4419, Some(2048)),
    ])
    .unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "name,pid,memory");
    assert_eq!(lines[1], "postgres,812,1024B");
    assert_eq!(lines[2], "nginx,4419,2048B");
    assert_eq!(lines.len(), 3);
}

#[test]
fn should_write_the_schema_field_order_as_the_column_order() {
    let text = to_csv(&[record("postgres", 812, None)]).unwrap();
    assert!(
        text.starts_with("name,pid,memory\n"),
        "the schema declares name, pid, memory in that order, got {text:?}"
    );
}

#[test]
fn should_write_null_as_the_word_null_rather_than_an_empty_field() {
    let text = to_csv(&[record("postgres", 812, None)]).unwrap();
    assert_eq!(
        text.lines().nth(1),
        Some("postgres,812,null"),
        "spec §10.5: an unknown value must never become an empty string"
    );
}

#[test]
fn should_keep_an_empty_string_apart_from_a_null() {
    let text = to_csv(&[map(&[("a", Value::string("")), ("b", Value::Null)])]).unwrap();
    assert_eq!(text.lines().nth(1), Some(",null"));
}

#[test]
fn should_refuse_a_heterogeneous_stream() {
    let other = map(&[("name", Value::string("nginx"))]);
    let error = to_csv(&[record("postgres", 812, None), other]).expect_err("columns differ");
    assert_eq!(error.code(), ErrorCode::TypeMismatch);
    assert!(
        error.help().unwrap_or_default().contains("json"),
        "spec §12.3: the error should name a serialization that can carry it, got {error}"
    );
}

#[test]
fn should_refuse_a_record_valued_field() {
    let nested = map(&[("inner", record("postgres", 812, None))]);
    let error = to_csv(&[nested]).expect_err("CSV cannot nest");
    assert_eq!(error.code(), ErrorCode::TypeMismatch);
    assert!(error.message().contains("inner"), "got {error}");
}

#[test]
fn should_refuse_a_list_valued_field() {
    let nested = map(&[("tags", Value::list([Value::string("web")]))]);
    let error = to_csv(&[nested]).expect_err("CSV cannot hold a list");
    assert_eq!(error.code(), ErrorCode::TypeMismatch);
    assert!(error.message().contains("tags"), "got {error}");
}

#[test]
fn should_refuse_a_map_valued_field() {
    let nested = map(&[("labels", map(&[("a", Value::Int(1))]))]);
    let error = to_csv(&[nested]).expect_err("CSV cannot nest");
    assert_eq!(error.code(), ErrorCode::TypeMismatch);
}

#[test]
fn should_refuse_a_value_that_is_not_a_record_or_a_map() {
    let error = to_csv(&[Value::Int(1)]).expect_err("a scalar has no columns");
    assert_eq!(error.code(), ErrorCode::TypeMismatch);
}

#[test]
fn should_write_an_empty_document_for_an_empty_stream() {
    assert_eq!(to_csv(&[]).unwrap(), "");
}

#[test]
fn should_carry_bytes_that_are_not_valid_utf8_as_hex() {
    let value = map(&[(
        "blob",
        Value::Bytes(Bytes::from_static(&[0xff, 0x00, 0xfe, 0x80])),
    )]);
    let text = to_csv(&[value]).unwrap();
    assert_eq!(text.lines().nth(1), Some("ff00fe80"));
}

#[test]
fn should_quote_a_cell_that_contains_the_delimiter() {
    let value = map(&[("command", Value::string("sh,-c"))]);
    let text = to_csv(&[value]).unwrap();
    assert_eq!(text.lines().nth(1), Some("\"sh,-c\""));
}

#[test]
fn should_read_the_header_row_as_field_names() {
    let value = from_csv("name,pid\npostgres,812\nnginx,4419\n").unwrap();
    let rows = value.as_list().unwrap();
    assert_eq!(rows.len(), 2);
    let first = rows[0].as_map().unwrap();
    assert_eq!(first.get("name"), Some(&Value::string("postgres")));
    assert_eq!(
        first.get("pid"),
        Some(&Value::string("812")),
        "CSV carries no types; inferring one would fabricate it (spec §35.3)"
    );
}

#[test]
fn should_read_null_as_null_and_an_empty_cell_as_an_empty_string() {
    let value = from_csv("a,b\nnull,\n").unwrap();
    let row = value.as_list().unwrap()[0].as_map().unwrap();
    assert_eq!(row.get("a"), Some(&Value::Null));
    assert_eq!(row.get("b"), Some(&Value::string("")));
}

#[test]
fn should_read_an_empty_document_as_an_empty_stream() {
    assert_eq!(from_csv("").unwrap(), Value::list([]));
}

#[test]
fn should_report_a_syntax_error_when_a_row_has_the_wrong_number_of_cells() {
    let error = from_csv("a,b\n1,2,3\n").expect_err("the row does not match the header");
    assert_eq!(error.code(), ErrorCode::ParseSyntax);
}

#[test]
fn should_write_the_same_document_on_a_second_pass_even_though_types_are_lost() {
    // CSV is a lossy export: `from_csv` can only return text, so the values do not come back.
    // What does come back is the document, exactly — which is the strongest property CSV allows.
    let first = to_csv(&[
        record("postgres", 812, Some(1024)),
        record("nginx", 4419, None),
    ])
    .unwrap();
    let reread = from_csv(&first).unwrap();
    let second = to_csv(reread.as_list().unwrap()).unwrap();
    assert_eq!(second, first);
}
