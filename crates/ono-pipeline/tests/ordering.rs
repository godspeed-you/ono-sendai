//! What a consumer may conclude from the order it observed things in (v0.4.1 §27).
//!
//! §27.4 asks two things of this file. The rule must be stated where the stream is documented —
//! it is, in `stream.rs` and in `docs/spec/hardening/streaming_classification.yaml`, from which
//! `docs/reference/streaming.md` is rendered — and *"concurrency tests MUST prove only the
//! guarantees actually promised"*. So the tests below assert per-channel order, which is total,
//! and deliberately assert nothing about the order in which a value and a partial failure are
//! observed relative to one another, which is not promised at all.

mod common;

use common::within;
use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, PipelineConfig, StreamEvent, StreamSink, ValueStream, Where};
use ono_value::{ErrorValue, Value};

/// How many of each kind the producers below emit. Large enough that a reordering would be seen
/// rather than got away with.
const RUN: i128 = 200;

/// A producer that alternates a value and a failure, numbering both so order is checkable.
fn alternating() -> ValueStream {
    ValueStream::spawn(
        PipelineConfig::new(),
        Boundedness::Bounded,
        |sink| async move {
            for n in 0..RUN {
                if sink.send(Value::Int(n)).await.is_err() {
                    return;
                }
                let failure =
                    ErrorValue::new(ErrorCode::ProviderUnavailable, format!("item {n} was gone"));
                if sink.fail(failure).await.is_err() {
                    return;
                }
            }
        },
    )
}

#[tokio::test]
async fn should_deliver_every_event_of_one_channel_in_the_order_it_was_produced() {
    // v0.4.1 §27.1: "the existing guarantee that values preserve their value-channel order and
    // errors preserve their error-channel order MUST remain." Two stages stand between the
    // producer and the consumer, because the guarantee is about the pipeline rather than about
    // one channel in isolation.
    let collected = within(
        alternating()
            .transform(Where::new(|_: &Value| Value::Bool(true)))
            .expect("`where` streams")
            .transform(Where::new(|_: &Value| Value::Bool(true)))
            .expect("`where` streams")
            .collect(),
    )
    .await;

    let values: Vec<Value> = (0..RUN).map(Value::Int).collect();
    assert_eq!(
        collected.values(),
        values.as_slice(),
        "values arrive in the order they were produced, through every stage"
    );
    let messages: Vec<String> = collected
        .errors()
        .iter()
        .map(|error| error.message().to_owned())
        .collect();
    let expected: Vec<String> = (0..RUN).map(|n| format!("item {n} was gone")).collect();
    assert_eq!(
        messages, expected,
        "and partial failures arrive in the order they were reported (spec §16.5)"
    );
}

#[tokio::test]
async fn should_hold_the_documented_guarantee_between_values_diagnostics_and_status() {
    // v0.4.1 §27.2, quoted from the registry this test and the reference page share: "`StreamEvent`
    // does not promise a total temporal ordering between independently produced value and
    // partial-error channels unless a producer explicitly serializes them through one event
    // source."
    //
    // What is asserted here is therefore everything that *is* promised and nothing more: both
    // kinds reach the consumer while the stream runs, neither channel loses an event to the
    // other, and each is in its own order. The relative position of a value and a failure is
    // read but never asserted — §27.4 forbids hard-coding a stronger cross-channel order, and an
    // assertion on the interleaving would be exactly that.
    within(async {
        let mut stream = alternating();
        let mut values = Vec::new();
        let mut failures = Vec::new();
        let mut seen_both_while_running = false;
        while let Some(event) = stream.recv().await {
            match event {
                StreamEvent::Value(value) => values.push(value),
                StreamEvent::Failure(error) => failures.push(error),
            }
            seen_both_while_running |= !values.is_empty() && !failures.is_empty();
        }

        assert!(
            seen_both_while_running,
            "a partial failure reaches the consumer while the stream is still running, rather \
             than being held to the end (spec §16.5)"
        );
        assert_eq!(
            values.len() as i128,
            RUN,
            "no value was lost to the error channel"
        );
        assert_eq!(
            failures.len() as i128,
            RUN,
            "and no failure was lost to the value channel"
        );
        assert!(
            values == (0..RUN).map(Value::Int).collect::<Vec<_>>(),
            "each channel is in its own order, which is the whole of what §27.1 promises"
        );
    })
    .await;
}

#[tokio::test]
async fn should_produce_a_total_order_when_the_caller_asks_for_one() {
    // v0.4.1 §27.3: "a provider or operation that needs to express 'error occurred between value
    // A and value B' as part of its semantic contract MUST emit an ordered event stream through
    // one sequence-bearing path rather than rely on Tokio scheduling between two channels."
    //
    // That path is one channel, one send at a time. A producer that takes it gets exactly the
    // order it chose — here A, the failure, then B — and gets it every time, which is what the
    // two-channel form cannot offer and does not claim to.
    within(async {
        let stream = ValueStream::spawn(
            PipelineConfig::new(),
            Boundedness::Bounded,
            |sink: StreamSink| async move {
                let between =
                    ErrorValue::new(ErrorCode::ProviderUnavailable, "between A and B").into_value();
                for event in [
                    StreamEvent::Value(Value::string("A")),
                    StreamEvent::Value(between),
                    StreamEvent::Value(Value::string("B")),
                ] {
                    if sink.send_in_sequence(event).await.is_err() {
                        return;
                    }
                }
            },
        );
        let collected = stream.collect().await;

        assert_eq!(collected.values().len(), 3, "every event kept its place");
        assert_eq!(collected.values()[0], Value::string("A"));
        assert_eq!(collected.values()[2], Value::string("B"));
        assert!(
            matches!(collected.values()[1], Value::Error(_)),
            "the failure is between them, as the producer placed it, got {:?}",
            collected.values()[1]
        );
        assert!(
            collected.errors().is_empty(),
            "a producer that serialises its events reports on one channel, so nothing arrives \
             out of band to be ordered against"
        );
    })
    .await;
}
