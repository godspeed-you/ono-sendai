//! `ActionResult` flows through a pipeline like any other record (spec §11.5).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed precondition in a test should abort the test loudly"
)]

use std::path::Path;

use ono_core::ErrorCode;
use ono_value::{ActionResult, ActionStatus, Duration, ErrorValue, Value, ValueRef};

#[test]
fn should_become_a_record_carrying_every_field_the_spec_lists() {
    let result = ActionResult::new(
        ValueRef::name("nginx.service"),
        "stop",
        ActionStatus::Success,
    )
    .changed(true)
    .with_message("stopped")
    .with_duration(Duration::parse("120ms").unwrap());

    let record = result.into_record();

    assert_eq!(record.schema_id().to_string(), "ono.action-result/1");
    assert_eq!(record.get("operation"), Some(&Value::String("stop".into())));
    assert_eq!(record.get("status"), Some(&Value::String("success".into())));
    assert_eq!(record.get("changed"), Some(&Value::Bool(true)));
    assert_eq!(
        record.get("duration"),
        Some(&Value::Duration(Duration::parse("120ms").unwrap()))
    );
    assert_eq!(record.get("error"), Some(&Value::Null));
    record
        .validate()
        .expect("an action result must satisfy its own schema");
}

#[test]
fn should_carry_the_structured_error_when_the_action_failed() {
    let result = ActionResult::new(
        ValueRef::path(Path::new("/tmp/x")),
        "remove",
        ActionStatus::Failed,
    )
    .with_error(ErrorValue::new(
        ErrorCode::IoPermissionDenied,
        "access denied",
    ));

    let record = result.into_record();

    assert_eq!(record.get("status"), Some(&Value::String("failed".into())));
    let error = record.get("error").unwrap().as_error().unwrap();
    assert_eq!(error.code(), ErrorCode::IoPermissionDenied);
    record
        .validate()
        .expect("a failed action result must still satisfy its schema");
}

#[test]
fn should_report_a_skipped_action_as_unchanged() {
    let result = ActionResult::new(ValueRef::name("nginx"), "start", ActionStatus::Skipped);

    assert!(!result.is_changed(), "a skipped action changed nothing");
    assert_eq!(result.status(), ActionStatus::Skipped);
    assert_eq!(result.duration(), Duration::ZERO);
}

#[test]
fn should_name_the_three_statuses_the_spec_defines() {
    let names: Vec<&str> = ActionStatus::ALL
        .iter()
        .map(|status| status.as_str())
        .collect();

    assert_eq!(names, vec!["success", "skipped", "failed"]);
}

#[test]
fn should_let_a_pipeline_filter_action_results_by_status() {
    let results = [
        ActionResult::new(ValueRef::name("a"), "stop", ActionStatus::Success),
        ActionResult::new(ValueRef::name("b"), "stop", ActionStatus::Failed),
    ];

    let failed: Vec<Value> = results
        .into_iter()
        .map(ActionResult::into_value)
        .filter(|value| {
            value
                .as_record()
                .ok()
                .and_then(|record| {
                    record
                        .get("status")
                        .map(|status| status == &Value::String("failed".into()))
                })
                .unwrap_or(false)
        })
        .collect();

    assert_eq!(failed.len(), 1, "`where status == failed` must find one");
}
