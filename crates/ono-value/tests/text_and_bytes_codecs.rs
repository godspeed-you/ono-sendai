//! The line-oriented text form of spec §29.1 and the raw byte form of spec §12.2.
//!
//! `to text --field path | xargs` is the bridge that makes an object pipeline a good Unix
//! citizen. A bridge that silently turns one value into two lines, or a byte sequence into a
//! replacement character, would be worse than no bridge at all — so these tests pin down the
//! refusals as carefully as the conversions.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failed precondition in a test should abort the test loudly"
)]

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;
use ono_core::ErrorCode;
use ono_value::{
    ByteSize, Duration, ErrorValue, FieldDef, FieldType, MapValue, Provenance, RecordValue, Schema,
    SchemaId, Value, canonical_text, from_bytes, to_bytes, to_text,
};

fn schema() -> Arc<Schema> {
    Arc::new(
        Schema::builder(SchemaId::new("ono.demo", 1), "Demo")
            .field(FieldDef::new("path", FieldType::Path).required())
            .field(FieldDef::new("owner", FieldType::Map).nullable())
            .field(FieldDef::new("size", FieldType::ByteSize).nullable())
            .identity(["path"])
            .build()
            .unwrap(),
    )
}

fn file(path: &str, size: Option<u128>) -> Value {
    let mut owner = MapValue::new();
    owner.insert("name".into(), Value::string("root"));
    RecordValue::builder(
        schema(),
        Provenance::local("demo", SchemaId::new("ono.demo", 1)),
    )
    .set("path", Value::Path(Arc::from(Path::new(path))))
    .unwrap()
    .set("owner", Value::Map(Arc::new(owner)))
    .unwrap()
    .set(
        "size",
        size.map_or(Value::Null, |bytes| {
            Value::ByteSize(ByteSize::from_bytes(bytes))
        }),
    )
    .unwrap()
    .build()
    .into_value()
}

#[test]
fn should_write_one_newline_terminated_line_per_value() {
    let text = to_text(&[Value::Int(1), Value::string("two")], None).unwrap();
    assert_eq!(text, "1\ntwo\n");
}

#[test]
fn should_write_the_named_field_of_each_record() {
    let text = to_text(
        &[file("/etc/passwd", None), file("/etc/hosts", None)],
        Some("path"),
    )
    .unwrap();
    assert_eq!(text, "/etc/passwd\n/etc/hosts\n");
}

#[test]
fn should_follow_a_dotted_field_path() {
    let text = to_text(&[file("/etc/passwd", None)], Some("owner.name")).unwrap();
    assert_eq!(text, "root\n");
}

#[test]
fn should_write_null_as_the_word_null() {
    let text = to_text(&[file("/etc/passwd", None)], Some("size")).unwrap();
    assert_eq!(
        text, "null\n",
        "spec §10.5: an unknown value must never become an empty line"
    );
}

#[test]
fn should_write_the_canonical_form_of_a_semantic_scalar() {
    let text = to_text(
        &[
            Value::ByteSize(ByteSize::from_bytes(1_288_490_188)),
            Value::Duration(Duration::parse("843ms").unwrap()),
        ],
        None,
    )
    .unwrap();
    assert_eq!(
        text, "1288490188B\n843000000ns\n",
        "spec §33.5: canonical values unless a human form is explicitly requested"
    );
}

#[test]
fn should_refuse_a_record_when_no_field_was_named() {
    let error = to_text(&[file("/etc/passwd", None)], None).expect_err("a record is not a line");
    assert_eq!(error.code(), ErrorCode::TypeMismatch);
    assert!(
        error.help().unwrap_or_default().contains("--field"),
        "spec §12.3: the error must say what to do instead, got {error}"
    );
}

#[test]
fn should_refuse_a_string_that_contains_a_newline() {
    let error = to_text(&[Value::string("first\nsecond")], None)
        .expect_err("one value must never become two lines");
    assert_eq!(error.code(), ErrorCode::TypeMismatch);
}

#[test]
fn should_refuse_bytes_that_are_not_valid_text() {
    let error = to_text(&[Value::Bytes(Bytes::from_static(&[0xff, 0xfe]))], None)
        .expect_err("undecodable bytes must never be lost (spec §12.2)");
    assert_eq!(error.code(), ErrorCode::TypeMismatch);
}

#[test]
fn should_decode_bytes_that_are_valid_text() {
    assert_eq!(
        to_text(&[Value::Bytes(Bytes::from_static(b"nginx"))], None).unwrap(),
        "nginx\n"
    );
}

#[test]
fn should_refuse_a_path_that_is_not_valid_text() {
    let path: Arc<Path> = Arc::from(Path::new(OsStr::from_bytes(&[0x2f, 0xff, 0xfe])));
    let error = to_text(&[Value::Path(path)], None).expect_err("a lossy path would lose bytes");
    assert_eq!(error.code(), ErrorCode::TypeMismatch);
}

#[test]
fn should_propagate_a_failed_field_access_rather_than_writing_a_blank() {
    let denied = ErrorValue::new(ErrorCode::IoPermissionDenied, "access denied").into_value();
    let record = RecordValue::builder(
        schema(),
        Provenance::local("demo", SchemaId::new("ono.demo", 1)),
    )
    .set("path", Value::Path(Arc::from(Path::new("/etc/shadow"))))
    .unwrap()
    .set("size", denied)
    .unwrap()
    .build()
    .into_value();

    let error = to_text(&[record], Some("size")).expect_err("a failed read is not a null");
    assert_eq!(error.code(), ErrorCode::IoPermissionDenied);
}

#[test]
fn should_report_an_unknown_field() {
    let error = to_text(&[file("/etc/passwd", None)], Some("nowhere")).expect_err("no such field");
    assert_eq!(error.code(), ErrorCode::TypeUnknownField);
}

#[test]
fn should_write_the_terse_form_of_an_error_value() {
    let error = ErrorValue::new(ErrorCode::IoPermissionDenied, "access denied").into_value();
    assert_eq!(
        to_text(&[error], None).unwrap(),
        "io.permission_denied: access denied\n"
    );
}

#[test]
fn should_write_nothing_for_an_empty_stream() {
    assert_eq!(to_text(&[], None).unwrap(), "");
}

#[test]
fn should_give_bytes_their_canonical_hex_text() {
    assert_eq!(
        canonical_text(&Value::Bytes(Bytes::from_static(&[0xff, 0x00]))).unwrap(),
        "ff00"
    );
}

#[test]
fn should_refuse_a_compound_value_a_canonical_text() {
    let error = canonical_text(&Value::list([Value::Int(1)])).expect_err("a list is not a scalar");
    assert_eq!(error.code(), ErrorCode::TypeMismatch);
}

#[test]
fn should_return_the_raw_bytes_of_a_byte_value_unchanged() {
    let raw = Bytes::from_static(&[0xff, 0x00, 0xfe, 0x80]);
    assert_eq!(to_bytes(&Value::Bytes(raw.clone())).unwrap(), raw);
}

#[test]
fn should_round_trip_bytes_that_are_not_valid_utf8() {
    let value = Value::Bytes(Bytes::from_static(&[0xff, 0x00, 0xfe, 0x80]));
    assert_eq!(from_bytes(to_bytes(&value).unwrap()), value);
}

#[test]
fn should_write_a_string_as_its_utf8_bytes() {
    assert_eq!(
        to_bytes(&Value::string("nginx")).unwrap(),
        Bytes::from_static(b"nginx")
    );
}

#[test]
fn should_keep_a_path_that_is_not_valid_text_as_its_operating_system_bytes() {
    let path: Arc<Path> = Arc::from(Path::new(OsStr::from_bytes(&[0x2f, 0xff, 0xfe])));
    assert_eq!(
        to_bytes(&Value::Path(path)).unwrap(),
        Bytes::from_static(&[0x2f, 0xff, 0xfe])
    );
}

#[test]
fn should_concatenate_a_list_of_byte_values() {
    let value = Value::list([
        Value::string("a"),
        Value::Bytes(Bytes::from_static(&[0xff])),
    ]);
    assert_eq!(to_bytes(&value).unwrap(), Bytes::from_static(&[b'a', 0xff]));
}

#[test]
fn should_refuse_a_value_with_no_raw_byte_form() {
    for value in [Value::Null, Value::Int(1), Value::Bool(true)] {
        let error = to_bytes(&value).expect_err("a number has no canonical byte form");
        assert_eq!(error.code(), ErrorCode::TypeMismatch, "for {value}");
        assert!(
            error.help().unwrap_or_default().contains("to json"),
            "spec §12.3: suggest a serialization that can carry it, got {error}"
        );
    }
}

#[test]
fn should_read_any_byte_sequence_as_bytes() {
    assert_eq!(
        from_bytes(vec![0xff, 0xfe]),
        Value::Bytes(Bytes::from_static(&[0xff, 0xfe]))
    );
}

#[test]
fn should_write_the_only_field_of_a_one_field_record() {
    // `select path` leaves a record of exactly one field, and one field is one line: the
    // projection `--field` would ask for is already unambiguous (spec §29.1).
    let projected = RecordValue::builder(
        Arc::new(
            Schema::builder(SchemaId::new("ono.demo-one", 1), "One")
                .field(FieldDef::new("path", FieldType::Path).required())
                .identity(["path"])
                .build()
                .unwrap(),
        ),
        Provenance::local("demo", SchemaId::new("ono.demo-one", 1)),
    )
    .set("path", Value::Path(Arc::from(Path::new("/etc/passwd"))))
    .unwrap()
    .build()
    .into_value();

    assert_eq!(to_text(&[projected], None).unwrap(), "/etc/passwd\n");
}
