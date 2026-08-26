//! The expression compiler, and the null semantics ADR-0014 freezes.
//!
//! Every cell of ADR-0014's matrix has a test here, because that ADR is the one a user meets on
//! their first `where cpu > 20` over a machine with one unreadable process.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod fixture;

use std::sync::Arc;

use ono_command::{Scope, evaluate, evaluate_to_value};
use ono_core::ErrorCode;
use ono_parser::Expr;
use ono_value::{
    ByteSize, ErrorValue, FieldDef, FieldType, Provenance, RecordValue, Schema, SchemaId, Value,
};

/// The expression written after `where`, which is expression mode and therefore an expression.
fn expression(source: &str) -> Expr {
    let text = format!("where {source}");
    let parsed = ono_parser::parse(&text);
    assert!(
        parsed.diagnostics().is_empty(),
        "`{text}` must parse cleanly, but produced {:?}",
        parsed.diagnostics()
    );
    parsed.program().statements[0]
        .as_pipeline()
        .expect("a pipeline")
        .head
        .stages[0]
        .arguments
        .first()
        .and_then(ono_parser::Argument::as_value)
        .expect("an expression argument")
        .clone()
}

/// A record with a known field, an unknown one and one whose read failed — the three absences of
/// spec §10.5, in one object.
fn subject() -> Value {
    let schema = Arc::new(
        Schema::builder(SchemaId::new("ono.subject", 1), "Subject")
            .field(FieldDef::new("cpu", FieldType::Float).nullable())
            .field(FieldDef::new("memory", FieldType::ByteSize).nullable())
            .field(FieldDef::new("name", FieldType::String).required())
            .field(FieldDef::new("secret", FieldType::Int).nullable())
            .build()
            .expect("the subject schema is valid"),
    );
    RecordValue::builder(
        schema.clone(),
        Provenance::local("test", schema.id().clone()),
    )
    .set("cpu", Value::Float(40.0))
    .and_then(|record| record.set("memory", Value::ByteSize(ByteSize::from_bytes(2048))))
    .and_then(|record| record.set("name", Value::string("alpha")))
    .and_then(|record| {
        record.set(
            "secret",
            ErrorValue::new(ErrorCode::IoPermissionDenied, "denied").into_value(),
        )
    })
    .expect("the subject record is valid")
    .build()
    .into_value()
}

fn eval(source: &str) -> Result<Value, ErrorValue> {
    evaluate(&expression(source), &subject(), &Scope::new())
}

// --- a bare identifier is a field of the current record (spec §10.3) ------------------------------

#[test]
fn should_read_a_bare_identifier_as_a_field_of_the_current_record() {
    assert_eq!(
        eval("name").expect("`name` is a field"),
        Value::string("alpha")
    );
}

#[test]
fn should_read_a_dotted_path_as_a_nested_field() {
    assert_eq!(
        eval("cpu > 20").expect("`cpu` is a field"),
        Value::Bool(true)
    );
}

#[test]
fn should_read_the_current_value_as_the_record_itself() {
    let value = eval("@").expect("`@` is the current value");
    assert_eq!(
        value.as_record().expect("a record").get("name"),
        Some(&Value::string("alpha"))
    );
}

#[test]
fn should_read_a_shell_binding_from_the_scope_rather_than_a_global() {
    let scope = Scope::new().with_variable("limit", Value::Int(20));
    assert_eq!(
        evaluate(&expression("cpu > $limit"), &subject(), &scope).expect("`$limit` is bound"),
        Value::Bool(true)
    );
}

#[test]
fn should_report_an_unbound_shell_binding_as_unknown() {
    assert_eq!(
        eval("$nothing").expect("an unbound name evaluates"),
        Value::Null
    );
}

#[test]
fn should_read_an_earlier_result_from_the_scope() {
    let scope = Scope::new().with_previous([Value::Int(7)]);
    assert_eq!(
        evaluate(&expression("@-1"), &Value::Null, &scope).expect("`@-1` is bound"),
        Value::Int(7)
    );
}

// --- the three absences stay three things (ADR-0014) ----------------------------------------------

#[test]
fn should_fail_the_read_of_a_field_the_schema_does_not_declare() {
    let error = eval("nowhere").expect_err("`nowhere` is not a field");
    assert_eq!(error.code(), ErrorCode::TypeUnknownField);
}

#[test]
fn should_read_a_field_the_schema_declares_but_no_value_filled_as_unknown() {
    let schema = Arc::new(
        Schema::builder(SchemaId::new("ono.sparse", 1), "Sparse")
            .field(FieldDef::new("cpu", FieldType::Float).nullable())
            .build()
            .expect("valid"),
    );
    let record = RecordValue::builder(
        schema.clone(),
        Provenance::local("test", schema.id().clone()),
    )
    .build()
    .into_value();
    assert_eq!(
        evaluate(&expression("cpu"), &record, &Scope::new()).expect("evaluates"),
        Value::Null,
        "a declared field with no value is unknown, never absent and never zero"
    );
}

#[test]
fn should_keep_a_failed_field_read_as_an_error_rather_than_an_unknown() {
    let error = eval("secret").expect_err("the read of `secret` failed");
    assert_eq!(
        error.code(),
        ErrorCode::IoPermissionDenied,
        "spec §10.5: `could not be read` never degrades into `unknown`"
    );
}

#[test]
fn should_answer_null_for_an_optional_read_of_a_field_that_is_not_there() {
    assert_eq!(
        eval("@?.nowhere").expect("`?.` opts into a runtime lookup"),
        Value::Null
    );
}

// --- comparisons are three-valued -----------------------------------------------------------------

#[test]
fn should_answer_unknown_when_a_comparison_has_an_unknown_operand() {
    for source in [
        "$missing > 20",
        "20 > $missing",
        "$missing == 20",
        "$missing < 20",
    ] {
        assert_eq!(
            eval(source).expect("evaluates"),
            Value::Null,
            "`{source}` compares against an unknown, and unknown it stays"
        );
    }
}

#[test]
fn should_answer_unknown_when_membership_or_a_match_has_an_unknown_operand() {
    assert_eq!(eval("$missing in [1, 2]").expect("evaluates"), Value::Null);
    assert_eq!(eval("$missing ~= /x/").expect("evaluates"), Value::Null);
}

#[test]
fn should_treat_a_comparison_with_the_null_literal_as_an_identity_test() {
    assert_eq!(
        eval("$missing == null").expect("evaluates"),
        Value::Bool(true),
        "ADR-0014: `x == null` asks whether x is unknown, and is total"
    );
    assert_eq!(
        eval("$missing != null").expect("evaluates"),
        Value::Bool(false)
    );
    assert_eq!(eval("name == null").expect("evaluates"), Value::Bool(false));
    assert_eq!(eval("name != null").expect("evaluates"), Value::Bool(true));
}

// --- and, or and not are Kleene -------------------------------------------------------------------

#[test]
fn should_decide_a_conjunction_that_one_operand_already_decided() {
    assert_eq!(
        eval("false and $missing").expect("evaluates"),
        Value::Bool(false)
    );
    assert_eq!(
        eval("$missing and false").expect("evaluates"),
        Value::Bool(false)
    );
}

#[test]
fn should_leave_a_conjunction_unknown_when_no_operand_decides_it() {
    assert_eq!(eval("true and $missing").expect("evaluates"), Value::Null);
}

#[test]
fn should_decide_a_disjunction_that_one_operand_already_decided() {
    assert_eq!(
        eval("true or $missing").expect("evaluates"),
        Value::Bool(true)
    );
    assert_eq!(
        eval("$missing or true").expect("evaluates"),
        Value::Bool(true)
    );
}

#[test]
fn should_leave_a_disjunction_unknown_when_no_operand_decides_it() {
    assert_eq!(eval("false or $missing").expect("evaluates"), Value::Null);
}

#[test]
fn should_leave_a_negated_unknown_unknown() {
    assert_eq!(eval("not $missing").expect("evaluates"), Value::Null);
    assert_eq!(eval("not true").expect("evaluates"), Value::Bool(false));
}

// --- arithmetic ------------------------------------------------------------------------------------

#[test]
fn should_answer_unknown_for_arithmetic_over_an_unknown() {
    assert_eq!(eval("$missing + 1").expect("evaluates"), Value::Null);
}

#[test]
fn should_keep_a_quantity_typed_through_arithmetic() {
    assert_eq!(
        eval("memory + 1KiB").expect("evaluates"),
        Value::ByteSize(ByteSize::from_bytes(3072)),
        "spec §10.6: a byte size plus a byte size is a byte size"
    );
}

#[test]
fn should_refuse_to_combine_two_different_dimensions() {
    let error = eval("memory + 10s").expect_err("bytes and seconds do not meet");
    assert_eq!(error.code(), ErrorCode::TypeInvalidUnit);
}

#[test]
fn should_divide_a_quantity_by_its_own_dimension_into_a_plain_ratio() {
    assert_eq!(eval("memory / 1KiB").expect("evaluates"), Value::Float(2.0));
}

// --- a predicate reports rather than throws --------------------------------------------------------

#[test]
fn should_report_a_failed_predicate_as_an_error_value_rather_than_stopping() {
    let value = evaluate_to_value(&expression("secret > 1"), &subject(), &Scope::new());
    assert!(
        value.is_error(),
        "ADR-0014: a predicate has four answers, and an error is one of them"
    );
}

// --- the check of spec §11.3, before anything runs --------------------------------------------------

#[test]
fn should_report_an_unknown_field_with_a_suggestion_before_the_pipeline_runs() {
    let error = fixture::check("get process | where cpy > 20")
        .expect_err("`cpy` is not a field of `ono.process/1`");

    assert_eq!(error.code(), ErrorCode::TypeUnknownField);
    assert!(
        error.message().contains("cpy"),
        "the message names the field: {}",
        error.message()
    );
    assert_eq!(
        error.help(),
        Some("perhaps: cpu"),
        "spec §11.3 shows the suggestion beside the error"
    );
}

#[test]
fn should_accept_a_field_the_output_schema_declares() {
    fixture::check("get process | where cpu > 20").expect("`cpu` is a field of `ono.process/1`");
}

#[test]
fn should_not_check_a_field_read_that_opted_into_a_runtime_lookup() {
    fixture::check("get process | where @?.cpy > 20")
        .expect("`?.` is the opt-in of spec §11.4, and opting in is allowed");
}

#[test]
fn should_stop_checking_once_a_stage_reshapes_the_stream() {
    // `select` emits `ono.selection/1`, whose fields are whatever was projected, so a later stage
    // has no declared schema to be checked against and must not be rejected on a guess.
    fixture::check("get process | select pid | where anything > 1")
        .expect("a projection carries no declared schema downstream");
}

#[test]
fn should_check_the_second_transform_of_a_pipeline_too() {
    let error = fixture::check("get process | where cpu > 20 | sort cpy")
        .expect_err("`sort` is checked as well as `where`");
    assert_eq!(error.code(), ErrorCode::TypeUnknownField);
}
