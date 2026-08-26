//! The message set of spec §21.2 and §21.4, and what survives a trip across the wire.

mod common;

use std::sync::Arc;

use common::{remote_record, schemas};
use ono_core::ErrorCode;
use ono_protocol::{
    ActRequest, Limits, Message, ProtocolError, RemoteQuery, decode_message, encode_message,
};
use ono_provider_api::{ActionOutcome, EventKind, ObjectEvent, ObjectId, Selector};
use ono_value::{ActionStatus, ByteSize, Duration, ErrorValue, SchemaId, Value};

/// Encodes a message and decodes it back, the way a link does.
#[allow(
    clippy::expect_used,
    reason = "a helper outside a #[test] body states its preconditions the same way a test does"
)]
fn round_trip(message: &Message) -> Message {
    let payload = encode_message(message, &Limits::default()).expect("the message encodes");
    decode_message(message.kind(), &payload, &schemas(), &Limits::default())
        .expect("the message decodes")
}

#[test]
fn should_round_trip_a_query_with_its_selectors_options_and_limit() {
    let query = RemoteQuery::target("process")
        .with(Selector::field("pid", Value::Int(4419)))
        .with(Selector::contains("name", "ngin"))
        .option("recursive", Value::Bool(true))
        .limit(25);

    let Message::StartQuery(decoded) = round_trip(&Message::StartQuery(query.clone())) else {
        panic!("a query decodes as a query");
    };

    assert_eq!(decoded.target_name(), "process");
    assert_eq!(decoded.selectors(), query.selectors());
    assert_eq!(decoded.option_value("recursive"), Some(&Value::Bool(true)));
    assert_eq!(decoded.max(), Some(25));
}

#[test]
fn should_round_trip_a_selector_naming_one_object() {
    let id = ObjectId::new(SchemaId::new("ono.test.remote", 1), [Value::Int(4419)]);
    let query = RemoteQuery::target("process").with(Selector::identity(id.clone()));

    let Message::StartQuery(decoded) = round_trip(&Message::StartQuery(query)) else {
        panic!("a query decodes as a query");
    };

    assert_eq!(decoded.selectors(), [Selector::identity(id)]);
}

#[test]
fn should_round_trip_a_semantic_scalar_without_losing_its_unit() {
    let value = Value::ByteSize(ByteSize::from_bytes(1_288_490_188));

    let Message::Value(decoded) = round_trip(&Message::Value(value.clone())) else {
        panic!("a value decodes as a value");
    };

    assert_eq!(
        decoded, value,
        "spec §21 exists so that a remote value keeps its type; a bare number would lose the unit"
    );
}

#[test]
fn should_round_trip_a_record_with_its_schema_and_provenance() {
    let record = remote_record(4419, "nginx");

    let Message::Value(Value::Record(decoded)) =
        round_trip(&Message::Value(record.clone().into_value()))
    else {
        panic!("a record decodes as a record");
    };

    assert_eq!(decoded.schema_id(), record.schema_id());
    assert_eq!(decoded.get("pid"), record.get("pid"));
    assert_eq!(decoded.provenance(), record.provenance());
}

#[test]
fn should_round_trip_an_object_event_with_its_identity_and_sequence() {
    let event = ObjectEvent::changed(&remote_record(4419, "nginx"), ["name"]).with_sequence(17);

    let Message::Event(decoded) = round_trip(&Message::Event(event.clone())) else {
        panic!("an event decodes as an event");
    };

    assert_eq!(decoded.kind(), EventKind::Changed);
    assert_eq!(decoded.object_id(), event.object_id());
    assert_eq!(decoded.sequence(), Some(17));
    assert_eq!(
        decoded.changed_fields(),
        Some(["name".to_owned()].as_slice())
    );
    assert_eq!(decoded.at(), event.at());
}

#[test]
fn should_round_trip_a_structured_error_with_its_code_and_help() {
    let error = ErrorValue::new(ErrorCode::IoPermissionDenied, "cannot read /proc/1/environ")
        .with_help("run with elevated privilege, or query a process you own")
        .with_retryable(false);

    let Message::Failure(decoded) = round_trip(&Message::Failure(error.clone())) else {
        panic!("a failure decodes as a failure");
    };

    assert_eq!(decoded.code(), ErrorCode::IoPermissionDenied);
    assert_eq!(decoded.message(), error.message());
    assert_eq!(decoded.help(), error.help());
}

#[test]
fn should_round_trip_an_action_request_with_its_arguments() {
    let object = ObjectId::new(SchemaId::new("ono.test.remote", 1), [Value::Int(4419)]);
    let request = ActRequest::new("process", "stop", object.clone())
        .with_argument("signal", Value::String("TERM".into()))
        .as_dry_run();

    let Message::Act(decoded) = round_trip(&Message::Act(request)) else {
        panic!("an action decodes as an action");
    };

    assert_eq!(decoded.target_name(), "process");
    assert_eq!(decoded.operation(), "stop");
    assert_eq!(decoded.object(), &object);
    assert_eq!(
        decoded.argument("signal"),
        Some(&Value::String("TERM".into()))
    );
    assert!(decoded.is_dry_run());
}

#[test]
fn should_round_trip_every_action_outcome_status() {
    let object = ObjectId::new(SchemaId::new("ono.test.remote", 1), [Value::Int(4419)]);
    let action = ActRequest::new("process", "stop", object).to_action();
    let outcomes = [
        ActionOutcome::succeeded(&action, true),
        ActionOutcome::skipped(&action, "the process had already exited"),
        ActionOutcome::failed(
            &action,
            ErrorValue::new(ErrorCode::IoPermissionDenied, "not yours to signal"),
        ),
    ];

    for outcome in outcomes {
        let Message::Outcome(decoded) = round_trip(&Message::Outcome(outcome.clone())) else {
            panic!("an outcome decodes as an outcome");
        };
        assert_eq!(decoded.status(), outcome.status());
        assert_eq!(decoded.changed(), outcome.changed());
        assert_eq!(decoded.target(), outcome.target());
        assert_eq!(
            decoded.error().map(ErrorValue::code),
            outcome.error().map(ErrorValue::code)
        );
    }
    assert_eq!(
        ActionOutcome::skipped(&action, "x").status(),
        ActionStatus::Skipped
    );
}

#[test]
fn should_carry_the_note_an_outcome_gives_for_skipping() {
    let object = ObjectId::new(SchemaId::new("ono.test.remote", 1), [Value::Int(4419)]);
    let action = ActRequest::new("process", "stop", object).to_action();
    let skipped = ActionOutcome::skipped(&action, "the process had already exited");

    let Message::Outcome(decoded) = round_trip(&Message::Outcome(skipped)) else {
        panic!("an outcome decodes as an outcome");
    };

    assert_eq!(
        decoded.into_record(Duration::from_nanoseconds(0)).message(),
        Some("the process had already exited"),
        "spec §11.5: why an action was skipped is part of the answer, not decoration"
    );
}

#[test]
fn should_round_trip_the_control_messages() {
    for message in [Message::Cancel, Message::End, Message::Credit(64)] {
        let decoded = round_trip(&message);
        assert_eq!(decoded.kind(), message.kind());
    }
}

#[test]
fn should_refuse_a_payload_that_is_not_the_document_its_kind_promises() {
    let outcome = decode_message(
        ono_protocol::FrameKind::Value,
        b"not json at all",
        &schemas(),
        &Limits::default(),
    );

    assert!(
        matches!(outcome, Err(ProtocolError::MalformedPayload { .. })),
        "a payload that is not a document is a protocol failure, not a panic"
    );
}

#[test]
fn should_refuse_a_value_nested_deeper_than_the_limit() {
    let limits = Limits::default().with_max_value_depth(8);
    let mut payload = String::new();
    for _ in 0..32 {
        payload.push('[');
    }
    for _ in 0..32 {
        payload.push(']');
    }

    let outcome = decode_message(
        ono_protocol::FrameKind::Value,
        payload.as_bytes(),
        &schemas(),
        &limits,
    );

    assert!(
        matches!(outcome, Err(ProtocolError::ValueTooDeep { limit: 8, .. })),
        "ADR-0015 T7: nesting is bounded before the value is built, got {outcome:?}"
    );
}

#[test]
fn should_refuse_a_record_naming_a_schema_this_end_does_not_hold() {
    let record = remote_record(4419, "nginx");
    let payload =
        encode_message(&Message::Value(record.into_value()), &Limits::default()).expect("encodes");

    let outcome = decode_message(
        ono_protocol::FrameKind::Value,
        &payload,
        &Arc::new(ono_value::SchemaRegistry::new()),
        &Limits::default(),
    );

    assert!(
        matches!(outcome, Err(ProtocolError::MalformedPayload { .. })),
        "an unknown schema is refused rather than guessed at, got {outcome:?}"
    );
}

#[test]
fn should_refuse_a_message_larger_than_the_frame_limit() {
    let limits = Limits::default().with_max_frame_payload(64);
    let long = "x".repeat(4096);

    let outcome = encode_message(&Message::Value(Value::String(long.into())), &limits);

    assert!(
        matches!(outcome, Err(ProtocolError::FrameTooLarge { limit: 64, .. })),
        "a message too large to frame is refused at the sender, got {outcome:?}"
    );
}
