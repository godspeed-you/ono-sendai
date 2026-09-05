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

// --- v0.4.1 §21.4 and §53.1: the three resource refusals (issue #73) --------------------------

#[test]
fn should_declare_the_three_resource_refusal_codes_with_their_details() {
    // §53.1 names the three families and §21.4 fixes what they carry: "the configured limit and
    // observed/estimated consumption without dumping the retained values themselves". A resource
    // error that printed what it was holding would be a second resource problem.
    let registry = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/contracts/errors.yaml"),
    )
    .expect("docs/contracts/errors.yaml is the single taxonomy (ADR-0125)");

    for (code, name) in [
        (ErrorCode::ResourceItemLimit, "resource.item_limit"),
        (ErrorCode::ResourceByteLimit, "resource.byte_limit"),
        (
            ErrorCode::ResourceMaterializationLimit,
            "resource.materialization_limit",
        ),
    ] {
        assert_eq!(
            code.name(),
            name,
            "§53.1 fixes the selector automation matches on"
        );
        assert_eq!(
            code.kind(),
            ErrorKind::Resource,
            "a budget being reached is its own class, not a timeout and not a safety policy"
        );
        assert!(
            registry.contains(&format!("name: {name}")),
            "ADR-0125: `{name}` is implemented and `docs/contracts/errors.yaml` does not declare it"
        );
        assert!(
            registry.contains(code.code()),
            "ADR-0125: `{}` is implemented and the registry does not declare it",
            code.code()
        );
    }

    // §21.4's detail fields, and the payload that must not be among them.
    let mut budget = ono_value::Budget::of("sort", 2, 1 << 30);
    let secret = Value::string("a-token-nobody-should-see-in-a-diagnostic");
    budget.charge(&secret).expect("the first value fits");
    budget.charge(&secret).expect("the second value fits");
    let refusal = budget
        .charge(&secret)
        .expect_err("the third crosses the ceiling")
        .into_error();

    assert_eq!(refusal.code(), ErrorCode::ResourceItemLimit);
    let detail = refusal.metadata();
    assert_eq!(
        detail.get("limit").and_then(|limit| limit.as_int().ok()),
        Some(2),
        "§21.4: the refusal carries the configured limit"
    );
    assert_eq!(
        detail
            .get("consumed")
            .and_then(|consumed| consumed.as_int().ok()),
        Some(3),
        "§21.4: and the consumption that crossed it"
    );
    assert_eq!(
        detail
            .get("stage")
            .and_then(|stage| stage.as_str().ok().map(str::to_owned)),
        Some("sort".to_owned()),
        "§54.1: a refusal names the boundary that made the decision"
    );
    assert_eq!(
        detail
            .get("setting")
            .and_then(|key| key.as_str().ok().map(str::to_owned)),
        Some("limits.materialize_items".to_owned()),
        "§55.1: and the key a user would raise to permit more"
    );
    assert!(
        refusal.help().is_some_and(|help| help.contains("limits.")),
        "§54.1: and the configuration key that would permit more: {:?}",
        refusal.help()
    );

    let rendered = refusal.render_full();
    assert!(
        !rendered.contains("a-token-nobody-should-see-in-a-diagnostic"),
        "§21.4, §53.3: the refusal dumped the value it was holding: {rendered}"
    );
}

#[test]
fn should_answer_the_same_resource_refusal_for_the_same_ceiling_every_time() {
    // §53.2: automation matches codes, so a code has to be a function of the condition alone.
    let refuse = |max_items: u64, max_bytes: u64| {
        let mut budget = ono_value::Budget::of("collect", max_items, max_bytes);
        loop {
            if let Err(exceeded) = budget.charge(&Value::string(&"x".repeat(4096))) {
                return exceeded.into_error().code();
            }
        }
    };
    for _ in 0..8 {
        assert_eq!(refuse(2, 1 << 30), ErrorCode::ResourceItemLimit);
        assert_eq!(refuse(1_000_000, 4096), ErrorCode::ResourceByteLimit);
    }
}
