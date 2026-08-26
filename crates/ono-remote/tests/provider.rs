//! A remote machine mounted into the ordinary provider registry (spec §21.4, §37 Phase H).
//!
//! The promise under test is spec §21's: `get process` against a linked host behaves exactly as
//! it does locally — same records, same schema, same partial-failure semantics — except that
//! every record's provenance says which host it came from (spec §25.2).

mod common;

use std::sync::Arc;

use common::fixture::{fixture_records, fixture_schema_id};
use common::{connect, settle, within};
use ono_core::ErrorCode;
use ono_pipeline::StreamEvent;
use ono_provider_api::{EventKind, Provider, ProviderRegistry, Query, Risk, Selector};
use ono_value::{Link, Value};

/// The fixture's providers, mounted into an ordinary local registry.
fn mounted(connected: &common::Connected) -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();
    connected.link.register_into(&mut registry);
    registry
}

/// Drains a bounded value stream into its values and failures.
async fn drain(mut stream: ono_pipeline::ValueStream) -> (Vec<Value>, Vec<ono_value::ErrorValue>) {
    let mut values = Vec::new();
    let mut failures = Vec::new();
    while let Some(event) = stream.recv().await {
        match event {
            StreamEvent::Value(value) => values.push(value),
            StreamEvent::Failure(error) => failures.push(error),
        }
    }
    (values, failures)
}

#[tokio::test]
async fn should_answer_get_process_through_the_registry_with_remote_provenance() {
    let connected = connect().await;
    let registry = mounted(&connected);

    let stream = registry
        .snapshot(&Query::target("process"))
        .expect("the mounted provider answers `process`");
    let (values, failures) = within(drain(stream)).await;

    assert!(failures.is_empty(), "nothing failed: {failures:?}");
    let expected: Vec<(Option<Value>, Option<Value>)> = fixture_records()
        .iter()
        .map(|record| (record.get("pid").cloned(), record.get("name").cloned()))
        .collect();
    let arrived: Vec<(Option<Value>, Option<Value>)> = values
        .iter()
        .map(|value| {
            let Value::Record(record) = value else {
                panic!("a remote process arrives as a record, got {value:?}");
            };
            (record.get("pid").cloned(), record.get("name").cloned())
        })
        .collect();
    assert_eq!(
        arrived, expected,
        "the records a remote `get process` yields are the records the remote provider produced"
    );

    for value in &values {
        let Value::Record(record) = value else {
            unreachable!("asserted above");
        };
        assert_eq!(record.schema_id(), &fixture_schema_id());
        assert_eq!(
            record.provenance().link(),
            &Link::Remote("remhost".into()),
            "spec §25.2: a record observed across a link says which host it came from"
        );
        assert_eq!(
            record.provenance().provider(),
            "fixture.demo",
            "the producing provider survives the crossing, so `inspect` stays truthful"
        );
        assert!(
            record
                .provenance()
                .source()
                .is_some_and(|source| source.starts_with("fixture://")),
            "what the remote provider read from survives the crossing"
        );
    }
}

#[tokio::test]
async fn should_narrow_a_remote_query_by_its_selectors() {
    let connected = connect().await;
    let registry = mounted(&connected);

    let stream = registry
        .snapshot(&Query::target("process").with(Selector::field("pid", Value::Int(2))))
        .expect("the mounted provider answers `process`");
    let (values, _) = within(drain(stream)).await;

    let names: Vec<Option<Value>> = values
        .iter()
        .map(|value| match value {
            Value::Record(record) => record.get("name").cloned(),
            other => panic!("expected records, got {other:?}"),
        })
        .collect();
    assert_eq!(
        names,
        [Some(Value::String("portd".into()))],
        "a selector narrows the remote query exactly as it narrows a local one"
    );
}

#[tokio::test]
async fn should_report_the_mounted_providers_capabilities_and_availability() {
    let connected = connect().await;
    let registry = mounted(&connected);

    let provider = registry
        .provider_for("process")
        .expect("the working provider is mounted and available");
    assert_eq!(provider.id(), "fixture.demo");
    let capabilities = provider.capabilities();
    let signal = capabilities
        .iter()
        .find(|capability| capability.id() == "process.signal")
        .expect("the mutating capability is projected");
    assert_eq!(signal.risk(), Risk::Mutate);
    assert!(signal.needs_elevation());

    let error = registry
        .provider_for("service")
        .expect_err("the absent provider is mounted but visibly unavailable");
    assert_eq!(error.code(), ErrorCode::ProviderUnavailable);
    assert!(
        error
            .message()
            .contains("no service manager in this fixture"),
        "the remote's own reason reaches the local user: {}",
        error.message()
    );
}

#[tokio::test]
async fn should_deliver_a_remote_partial_failure_beside_the_values() {
    let connected = connect().await;
    let registry = mounted(&connected);

    let stream = registry
        .snapshot(&Query::target("flaky"))
        .expect("the mounted provider answers `flaky`");
    let (values, failures) = within(drain(stream)).await;

    assert_eq!(
        values.len(),
        2,
        "spec §16.5: one object that could not be read does not lose the ones that could"
    );
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].code(), ErrorCode::IoPermissionDenied);
}

#[tokio::test]
async fn should_keep_two_interleaved_remote_streams_independent() {
    let connected = connect().await;
    let registry = mounted(&connected);

    let mut first = registry
        .snapshot(&Query::target("process").limit(2))
        .expect("the first stream opens");
    let mut second = registry
        .snapshot(&Query::target("process"))
        .expect("the second stream opens while the first is running");

    let mut first_pids = Vec::new();
    let mut second_pids = Vec::new();
    within(async {
        loop {
            tokio::select! {
                event = first.recv() => match event {
                    Some(StreamEvent::Value(Value::Record(record))) => {
                        first_pids.push(record.get("pid").cloned());
                    }
                    Some(_) => {}
                    None => break,
                },
                event = second.recv() => match event {
                    Some(StreamEvent::Value(Value::Record(record))) => {
                        second_pids.push(record.get("pid").cloned());
                    }
                    Some(_) => {}
                    None => {}
                },
            }
        }
        while let Some(event) = second.recv().await {
            if let StreamEvent::Value(Value::Record(record)) = event {
                second_pids.push(record.get("pid").cloned());
            }
        }
    })
    .await;

    assert_eq!(
        first_pids,
        [Some(Value::Int(1)), Some(Value::Int(2))],
        "the limited stream got exactly its two objects"
    );
    assert_eq!(
        second_pids,
        [
            Some(Value::Int(1)),
            Some(Value::Int(2)),
            Some(Value::Int(3))
        ],
        "the concurrent stream got all three, unaffected by its neighbour's limit"
    );
}

#[tokio::test]
async fn should_leave_the_other_stream_running_when_one_is_cancelled() {
    let connected = connect().await;
    let registry = mounted(&connected);

    let mut endless = registry
        .snapshot(&Query::target("tick"))
        .expect("the endless stream opens");
    within(async {
        for _ in 0..3 {
            let event = endless.recv().await;
            assert!(
                matches!(event, Some(StreamEvent::Value(_))),
                "the endless target produces values: {event:?}"
            );
        }
    })
    .await;

    endless.cancel_token().cancel();
    drop(endless);

    within(async {
        while !connected.observed.tick_cancelled() {
            settle().await;
        }
    })
    .await;

    let survivor = registry
        .snapshot(&Query::target("process"))
        .expect("the link still answers after one stream was cancelled");
    let (values, failures) = within(drain(survivor)).await;
    assert_eq!(
        values.len(),
        3,
        "cancelling one stream must not take the connection down (spec §18.5): {failures:?}"
    );
}

#[tokio::test]
async fn should_subscribe_to_remote_changes_with_remote_provenance() {
    let connected = connect().await;
    let registry = mounted(&connected);

    let mut events = registry
        .subscribe(&Query::target("process"))
        .expect("the mounted provider can watch `process`");

    let mut kinds = Vec::new();
    within(async {
        while let Some(event) = events.recv().await {
            assert_eq!(
                event.provenance().link(),
                &Link::Remote("remhost".into()),
                "a change observed across a link says which host observed it"
            );
            kinds.push(event.kind());
        }
    })
    .await;

    assert_eq!(
        kinds,
        [EventKind::Snapshot, EventKind::Removed],
        "the events arrive in the envelope of spec §31.14, in order"
    );
}

#[tokio::test]
async fn should_resolve_a_remote_selector_to_object_references() {
    let connected = connect().await;
    let registry = mounted(&connected);

    let refs = within(registry.resolve("process", &Selector::field("pid", Value::Int(3))))
        .await
        .expect("the mounted provider resolves selectors");

    assert_eq!(refs.len(), 1, "exactly one object has pid 3");
    assert_eq!(
        refs[0].label(),
        "nginx",
        "the reference labels the object the way a person names it"
    );
    assert_eq!(
        refs[0].provenance().link(),
        &Link::Remote("remhost".into()),
        "a reference to a remote object says where the object lives"
    );
}

#[tokio::test]
async fn should_refuse_provider_level_actions_with_a_structured_error() {
    let connected = connect().await;
    let registry = mounted(&connected);

    let object = ono_provider_api::ObjectId::new(fixture_schema_id(), [Value::Int(2)]);
    let action = ono_provider_api::Action::new("process", "stop", object);
    let error = within(registry.act(&action))
        .await
        .expect_err("the mounted provider cannot forward an action it cannot see in full");

    assert_eq!(
        error.code(),
        ErrorCode::ProviderUnsupported,
        "an action that would silently lose its arguments is refused, never half-forwarded"
    );
}

#[tokio::test]
async fn should_perform_a_remote_action_through_the_link() {
    let connected = connect().await;

    let object = ono_provider_api::ObjectId::new(fixture_schema_id(), [Value::Int(2)]);
    let request = ono_protocol::ActRequest::new("process", "stop", object)
        .with_argument("signal", Value::String("TERM".into()));
    let outcome = within(connected.link.act(&request))
        .await
        .expect("the remote performs the action");

    assert!(
        outcome.is_success(),
        "the remote's outcome arrives structured, not as an exit code (spec §11.5)"
    );
    assert!(outcome.changed());
}

#[tokio::test]
async fn should_mount_one_provider_per_negotiated_target() {
    let connected = connect().await;

    let mut targets: Vec<&str> = connected
        .link
        .providers()
        .iter()
        .flat_map(|provider| Arc::as_ref(provider).targets().iter().copied())
        .collect();
    targets.sort_unstable();
    assert_eq!(
        targets,
        ["flaky", "process", "service", "tick"],
        "every negotiated target is mountable, the unavailable one included"
    );
}
