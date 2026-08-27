//! The three-way distinction spec §10.5 requires, asserted from the outside.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failed precondition in a test should abort the test loudly"
)]

use std::sync::Arc;

use ono_core::ErrorCode;
use ono_value::{
    ErrorValue, FieldAccess, FieldDef, FieldStep, FieldType, Provenance, RecordValue, Schema,
    SchemaId, Value,
};

fn schema() -> Arc<Schema> {
    Arc::new(
        Schema::builder(SchemaId::new("ono.test.thing", 1), "Thing")
            .field(FieldDef::new("id", FieldType::Int).required())
            .field(FieldDef::new("memory", FieldType::ByteSize).nullable())
            .field(FieldDef::new("owner", FieldType::String).nullable())
            .identity(["id"])
            .default_view(["id", "memory"])
            .build()
            .unwrap(),
    )
}

fn record() -> RecordValue {
    let schema = schema();
    let provenance = Provenance::local("test", schema.id().clone());
    RecordValue::builder(schema, provenance)
        .set("id", Value::Int(7))
        .unwrap()
        .set("owner", Value::Null)
        .unwrap()
        .set(
            "memory",
            Value::Error(Arc::new(
                ErrorValue::new(ErrorCode::IoPermissionDenied, "access denied")
                    .with_help("requires root or read capability"),
            )),
        )
        .unwrap()
        .build()
}

#[test]
fn should_report_a_field_that_the_schema_does_not_define_as_absent() {
    assert_eq!(
        record().access("cpy"),
        FieldAccess::Absent,
        "a field the schema never declared must be reported as absent, not as null"
    );
}

#[test]
fn should_report_a_field_whose_value_is_unknown_as_unknown() {
    assert_eq!(
        record().access("owner"),
        FieldAccess::Unknown,
        "a declared field holding null must be reported as unknown, not as absent"
    );
}

#[test]
fn should_report_a_field_whose_access_failed_as_failed() {
    let access = record().access("memory");

    match access {
        FieldAccess::Failed(error) => {
            assert_eq!(error.code(), ErrorCode::IoPermissionDenied);
        }
        other => panic!("a field carrying an error must be reported as failed, got {other:?}"),
    }
}

#[test]
fn should_report_a_field_with_a_value_as_known() {
    assert_eq!(record().access("id"), FieldAccess::Known(Value::Int(7)));
}

#[test]
fn should_keep_the_three_outcomes_distinguishable_from_one_another() {
    let record = record();
    let absent = record.access("nothing_like_this");
    let unknown = record.access("owner");
    let failed = record.access("memory");

    assert_ne!(absent, unknown, "absent and unknown must not collapse");
    assert_ne!(unknown, failed, "unknown and failed must not collapse");
    assert_ne!(absent, failed, "absent and failed must not collapse");
}

#[test]
fn should_fail_with_an_unknown_field_error_when_a_required_path_names_no_field() {
    let value = Value::Record(Arc::new(record()));
    let error = value
        .follow(&[FieldStep::required("cpy")])
        .expect_err("`cpy` is not a field of Thing");

    assert_eq!(error.code(), ErrorCode::TypeUnknownField);
}

#[test]
fn should_yield_null_when_an_optional_path_names_no_field() {
    let value = Value::Record(Arc::new(record()));

    assert_eq!(
        value.follow(&[FieldStep::optional("cpy")]).unwrap(),
        Value::Null,
        "`?.` turns an absent field into null"
    );
}

#[test]
fn should_propagate_the_error_even_when_the_path_step_is_optional() {
    let value = Value::Record(Arc::new(record()));
    let error = value
        .follow(&[FieldStep::optional("memory")])
        .expect_err("`?.` guards absence, never a permission failure");

    assert_eq!(
        error.code(),
        ErrorCode::IoPermissionDenied,
        "an access failure must survive optional access, or the three cases collapse"
    );
}

#[test]
fn should_short_circuit_an_optional_path_over_a_null_receiver() {
    let value = Value::Record(Arc::new(record()));

    assert_eq!(
        value
            .follow(&[FieldStep::optional("owner"), FieldStep::optional("name")])
            .unwrap(),
        Value::Null
    );
}

#[test]
fn should_reject_a_required_path_over_a_null_receiver() {
    let value = Value::Record(Arc::new(record()));
    let error = value
        .follow(&[FieldStep::required("owner"), FieldStep::required("name")])
        .expect_err("a required field access on null is a type error");

    assert_eq!(error.code(), ErrorCode::TypeMismatch);
}
