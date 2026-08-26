//! Multiplexed, cancellable, backpressured streams over one link (spec §21.2, §21.4, §11.2).

mod common;

use std::sync::atomic::Ordering;

use common::{connect, settle, within};
use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, PipelineConfig, StreamEvent};
use ono_protocol::{ActRequest, RemoteMessage, RemoteQuery};
use ono_provider_api::{EventKind, ObjectId};
use ono_value::{Link, SchemaId, Value};

/// Drains a remote stream into the values it carried.
async fn drain(stream: &mut ono_protocol::RemoteStream) -> Vec<Value> {
    let mut values = Vec::new();
    while let Some(message) = stream.recv().await {
        if let RemoteMessage::Value(value) = message {
            values.push(value);
        }
    }
    values
}

#[tokio::test]
async fn should_stream_the_results_of_a_remote_query_in_order() {
    let fixture = connect().await;
    let mut stream = fixture
        .link
        .query(&RemoteQuery::target("demo").limit(4))
        .expect("a query opens a stream");

    let values = within(drain(&mut stream)).await;

    assert_eq!(
        values,
        [Value::Int(0), Value::Int(1), Value::Int(2), Value::Int(3)],
        "a remote query yields the values the remote produced, in the order it produced them"
    );
}

#[tokio::test]
async fn should_carry_a_record_across_the_link_as_the_same_record() {
    let fixture = connect().await;
    let mut stream = fixture
        .link
        .query(&RemoteQuery::target("record"))
        .expect("a query opens a stream");

    let values = within(drain(&mut stream)).await;

    let [Value::Record(record)] = values.as_slice() else {
        panic!("the remote sent exactly one record, got {values:?}");
    };
    assert_eq!(record.schema_id(), &SchemaId::new("ono.test.remote", 1));
    assert_eq!(record.get("pid"), Some(&Value::Int(4419)));
    assert_eq!(record.get("name"), Some(&Value::String("nginx".into())));
    assert_eq!(
        record.provenance().link(),
        &Link::Remote("testhost".into()),
        "spec §21 exists so a remote object stays an object, provenance and all"
    );
}

#[tokio::test]
async fn should_deliver_a_per_item_failure_without_ending_the_stream() {
    let fixture = connect().await;
    let mut stream = fixture
        .link
        .query(&RemoteQuery::target("flaky"))
        .expect("a query opens a stream");

    let mut values = Vec::new();
    let mut failures = Vec::new();
    within(async {
        while let Some(message) = stream.recv().await {
            match message {
                RemoteMessage::Value(value) => values.push(value),
                RemoteMessage::Failure(error) => failures.push(error),
                RemoteMessage::Event(_) => panic!("a query yields values, not events"),
            }
        }
    })
    .await;

    assert_eq!(
        values,
        [Value::Int(1), Value::Int(2)],
        "spec §16.5: one object that could not be read does not lose the ones that could"
    );
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].code(), ErrorCode::IoPermissionDenied);
}

#[tokio::test]
async fn should_report_a_query_the_remote_cannot_answer_as_a_failure_on_its_own_stream() {
    let fixture = connect().await;
    let mut stream = fixture
        .link
        .query(&RemoteQuery::target("nonesuch"))
        .expect("a query opens a stream even when it will fail");

    let mut failures = Vec::new();
    within(async {
        while let Some(message) = stream.recv().await {
            if let RemoteMessage::Failure(error) = message {
                failures.push(error);
            }
        }
    })
    .await;

    assert_eq!(failures.len(), 1, "the refusal arrives exactly once");
    assert_eq!(failures[0].code(), ErrorCode::ResolveTargetNotFound);
}

#[tokio::test]
async fn should_keep_concurrent_streams_apart_when_both_are_open() {
    let fixture = connect().await;
    let mut first = fixture
        .link
        .query(
            &RemoteQuery::target("demo")
                .limit(3)
                .option("base", Value::Int(100)),
        )
        .expect("the first query opens a stream");
    let mut second = fixture
        .link
        .query(
            &RemoteQuery::target("demo")
                .limit(3)
                .option("base", Value::Int(200)),
        )
        .expect("the second query opens a stream");

    assert_ne!(first.id(), second.id(), "each stream has its own id");

    let (left, right) = within(async { tokio::join!(drain(&mut first), drain(&mut second)) }).await;

    assert_eq!(
        left,
        [Value::Int(100), Value::Int(101), Value::Int(102)],
        "one connection carries both queries without either seeing the other's values"
    );
    assert_eq!(right, [Value::Int(200), Value::Int(201), Value::Int(202)]);
}

#[tokio::test]
async fn should_leave_the_other_streams_running_when_one_is_cancelled() {
    let fixture = connect().await;
    let endless = fixture
        .link
        .query(&RemoteQuery::target("endless"))
        .expect("the endless query opens a stream");
    let mut finite = fixture
        .link
        .query(&RemoteQuery::target("demo").limit(3))
        .expect("the finite query opens a stream");

    endless.cancel();

    let values = within(drain(&mut finite)).await;
    assert_eq!(
        values,
        [Value::Int(0), Value::Int(1), Value::Int(2)],
        "cancelling one stream must not disturb another on the same connection"
    );

    settle().await;
    assert!(
        fixture.observed.cancelled(),
        "the remote producer must learn that nobody is reading it any more"
    );
}

#[tokio::test]
async fn should_stop_the_remote_producer_when_its_stream_is_dropped() {
    let fixture = connect().await;
    let endless = fixture
        .link
        .query(&RemoteQuery::target("endless"))
        .expect("the endless query opens a stream");
    drop(endless);

    settle().await;

    assert!(
        fixture.observed.cancelled(),
        "dropping a remote stream ends it the way `yes | head -1` ends a local one"
    );
}

#[tokio::test]
async fn should_bound_a_fast_remote_producer_when_the_local_consumer_is_slow() {
    let window = 8;
    let fixture = common::try_connect(
        common::client_config("testhost").with_credit_window(window),
        common::server_config(),
        None,
    )
    .await
    .expect("the handshake succeeds");

    let mut stream = fixture
        .link
        .query(&RemoteQuery::target("endless"))
        .expect("the endless query opens a stream");

    let mut consumed = 0usize;
    for _ in 0..4 {
        within(stream.recv()).await.expect("a value arrives");
        consumed += 1;
    }
    settle().await;

    let sent = fixture.observed.sent();
    assert!(
        sent >= window as usize,
        "the window must be usable: only {sent} values were sent"
    );
    assert!(
        sent <= consumed + window as usize,
        "spec §11.2: a slow consumer bounds an endless producer. It consumed {consumed} values \
         with a window of {window} and the producer had already sent {sent}"
    );
}

#[tokio::test]
async fn should_refuse_to_open_more_streams_than_the_limit_allows() {
    let fixture = common::try_connect(
        common::client_config("testhost")
            .with_limits(ono_protocol::Limits::default().with_max_streams(2)),
        common::server_config(),
        None,
    )
    .await
    .expect("the handshake succeeds");

    let _first = fixture
        .link
        .query(&RemoteQuery::target("endless"))
        .expect("the first stream is within the limit");
    let _second = fixture
        .link
        .query(&RemoteQuery::target("endless"))
        .expect("the second stream is within the limit");

    let refused = fixture
        .link
        .query(&RemoteQuery::target("endless"))
        .expect_err("a third stream is over the limit");
    assert_eq!(
        refused.code(),
        ErrorCode::RemoteProtocolMismatch,
        "an unbounded number of streams is an unbounded amount of memory (ADR-0015 T7)"
    );
}

#[tokio::test]
async fn should_stream_object_events_when_a_subscription_is_opened() {
    let fixture = connect().await;
    let mut stream = fixture
        .link
        .subscribe(&RemoteQuery::target("process"))
        .expect("a subscription opens a stream");

    let mut events = Vec::new();
    within(async {
        while let Some(message) = stream.recv().await {
            if let RemoteMessage::Event(event) = message {
                events.push(event);
            }
        }
    })
    .await;

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].kind(), EventKind::Snapshot);
    assert_eq!(events[1].kind(), EventKind::Removed);
    assert_eq!(events[0].sequence(), Some(1));
    assert_eq!(
        events[0].object_id(),
        &ObjectId::new(SchemaId::new("ono.test.remote", 1), [Value::Int(4419)]),
        "object identity survives the wire, which is what makes a live remote view possible"
    );
}

#[tokio::test]
async fn should_return_a_structured_outcome_when_a_remote_action_is_performed() {
    let fixture = connect().await;
    let object = ObjectId::new(SchemaId::new("ono.test.remote", 1), [Value::Int(4419)]);
    let request = ActRequest::new("process", "stop", object.clone())
        .with_argument("signal", Value::String("TERM".into()));

    let outcome = within(fixture.link.act(&request))
        .await
        .expect("the remote performed the action");

    assert!(outcome.is_success());
    assert!(outcome.changed());
    assert_eq!(outcome.operation(), "stop");
    assert_eq!(
        outcome.target(),
        &object,
        "spec §11.5 answers per target, so the target must come back identified"
    );
}

#[tokio::test]
async fn should_feed_a_remote_stream_into_a_pipeline_with_the_same_contract_as_a_local_one() {
    let fixture = connect().await;
    let stream = fixture
        .link
        .query(&RemoteQuery::target("flaky"))
        .expect("a query opens a stream");

    let collected = within(
        stream
            .into_value_stream(PipelineConfig::new(), Boundedness::Bounded)
            .collect(),
    )
    .await;

    assert_eq!(collected.values(), [Value::Int(1), Value::Int(2)]);
    assert_eq!(collected.errors().len(), 1);
    assert_eq!(collected.errors()[0].code(), ErrorCode::IoPermissionDenied);
}

#[tokio::test]
async fn should_stop_a_remote_producer_when_the_pipeline_it_feeds_is_cancelled() {
    let fixture = connect().await;
    let stream = fixture
        .link
        .query(&RemoteQuery::target("endless"))
        .expect("a query opens a stream");

    let mut values = stream.into_value_stream(PipelineConfig::new(), Boundedness::Unbounded);
    let first = within(values.recv()).await;
    assert!(matches!(first, Some(StreamEvent::Value(_))));

    values.cancel_token().cancel();
    settle().await;

    assert!(
        fixture.observed.cancelled(),
        "spec §18.5: one cancellation reaches every stage, including the one on another machine"
    );
}

#[tokio::test]
async fn should_report_the_link_as_lost_when_the_remote_end_goes_away() {
    let fixture = connect().await;
    fixture.server.abort();
    let _ = fixture.server.await;

    settle().await;

    let error = fixture
        .link
        .query(&RemoteQuery::target("demo"))
        .expect_err("a query cannot be opened on a link that is gone");
    assert_eq!(error.code(), ErrorCode::RemoteUnreachable);
}

#[tokio::test]
async fn should_cap_the_credit_window_at_the_limit_however_much_is_asked_for() {
    let limits = ono_protocol::Limits::default();
    assert!(
        limits.max_credit() > 0,
        "a window of zero would deadlock every stream"
    );
    // A peer asking for more credit than the limit gets the limit, never the ask: the window is
    // the memory bound, so it is ours to cap.
    let fixture = common::try_connect(
        common::client_config("testhost").with_credit_window(u32::MAX),
        common::server_config(),
        None,
    )
    .await
    .expect("the handshake succeeds");

    assert!(fixture.link.negotiated().credit_window() <= limits.max_credit());
    assert_eq!(
        fixture.observed.sent.load(Ordering::SeqCst),
        0,
        "nothing is produced until something is asked for"
    );
}
