//! Error rendering: terse by default, rich on demand (spec §16.1, §16.2).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed precondition in a test should abort the test loudly"
)]

use std::path::Path;

use ono_core::{ErrorCode, ErrorKind};
use ono_value::{ErrorValue, Value, ValueRef};

#[test]
fn should_render_the_terse_form_the_spec_shows() {
    let error = ErrorValue::new(ErrorCode::IoPermissionDenied, "access denied")
        .with_target(ValueRef::path(Path::new("/etc/shadow")))
        .with_help("requires root or read capability");

    assert_eq!(
        error.to_string(),
        "access denied: /etc/shadow\nrequires root or read capability"
    );
}

#[test]
fn should_omit_the_target_and_help_when_there_are_none() {
    let error = ErrorValue::new(ErrorCode::ProviderUnavailable, "procfs is not mounted");

    assert_eq!(error.to_string(), "procfs is not mounted");
}

#[test]
fn should_take_its_kind_from_its_code() {
    let error = ErrorValue::new(ErrorCode::IoPermissionDenied, "access denied");

    assert_eq!(error.kind(), ErrorKind::Permission);
}

#[test]
fn should_show_the_whole_causal_chain_in_the_full_rendering() {
    let cause = ErrorValue::new(ErrorCode::ProviderUnavailable, "procfs is not mounted");
    let error = ErrorValue::new(ErrorCode::IoPermissionDenied, "access denied")
        .with_target(ValueRef::path(Path::new("/etc/shadow")))
        .with_help("requires root or read capability")
        .with_retryable(false)
        .with_metadata("errno", Value::Int(13))
        .with_source(cause);

    let full = error.render_full();

    assert!(
        full.contains("Ono-Sendai-E0302"),
        "the full rendering must carry the stable code, got:\n{full}"
    );
    assert!(
        full.contains("io.permission_denied"),
        "the full rendering must carry the selector, got:\n{full}"
    );
    assert!(
        full.contains("permission"),
        "the full rendering must carry the kind, got:\n{full}"
    );
    assert!(
        full.contains("/etc/shadow"),
        "the full rendering must carry the target, got:\n{full}"
    );
    assert!(
        full.contains("errno"),
        "the full rendering must carry the metadata, got:\n{full}"
    );
    assert!(
        full.contains("caused by"),
        "the full rendering must announce the cause, got:\n{full}"
    );
    assert!(
        full.contains("Ono-Sendai-E0401"),
        "the full rendering must carry the cause's code, got:\n{full}"
    );
    assert!(
        full.contains("procfs is not mounted"),
        "the full rendering must carry the cause's message, got:\n{full}"
    );
}

#[test]
fn should_walk_the_chain_of_causes_in_order() {
    let root = ErrorValue::new(ErrorCode::ProviderUnavailable, "procfs is not mounted");
    let middle = ErrorValue::new(ErrorCode::IoNotFound, "no such file").with_source(root);
    let outer = ErrorValue::new(ErrorCode::IoPermissionDenied, "access denied").with_source(middle);

    let codes: Vec<ErrorCode> = outer.chain().map(ErrorValue::code).collect();

    assert_eq!(
        codes,
        vec![
            ErrorCode::IoPermissionDenied,
            ErrorCode::IoNotFound,
            ErrorCode::ProviderUnavailable
        ]
    );
}

#[test]
fn should_report_whether_the_operation_may_be_retried_only_when_it_is_known() {
    let unknown = ErrorValue::new(ErrorCode::RemoteUnreachable, "link lost");
    assert_eq!(
        unknown.retryable(),
        None,
        "an unstated retryability is unknown, not false"
    );

    let known = unknown.with_retryable(true);
    assert_eq!(known.retryable(), Some(true));
}

#[test]
fn should_render_an_object_reference_by_its_identity() {
    let reference = ValueRef::name("nginx.service");

    assert_eq!(reference.to_string(), "nginx.service");
}

#[test]
fn should_carry_an_error_as_an_ordinary_value() {
    let value = ErrorValue::new(ErrorCode::IoNotFound, "no such file").into_value();

    assert_eq!(value.type_name(), "error");
    assert_eq!(value.as_error().unwrap().code(), ErrorCode::IoNotFound);
}

#[test]
fn should_reject_reading_an_error_as_another_type() {
    let value = ErrorValue::new(ErrorCode::IoNotFound, "no such file").into_value();

    let error = value
        .as_int()
        .expect_err("an error value is not an integer");

    assert_eq!(error.code(), ErrorCode::TypeMismatch);
}
