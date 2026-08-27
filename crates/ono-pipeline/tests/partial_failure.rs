//! Spec §16.5: a bulk operation reports per-item failure and never collapses it into one result.

mod common;

use common::{demo, field_of, within};
use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, PipelineConfig, StreamEvent, ValueStream, Where};
use ono_value::{ErrorValue, Value};

/// A producer that succeeds for some objects and fails for others, the way a provider does when
/// one process disappears between enumeration and detail read (ADR-0013).
fn flaky() -> ValueStream {
    ValueStream::spawn(
        PipelineConfig::new(),
        Boundedness::Bounded,
        |sink| async move {
            for pid in 0..6_i128 {
                if pid % 2 == 0 {
                    if sink.send(demo(pid, "alive", Some(1.0))).await.is_err() {
                        return;
                    }
                } else {
                    let error = ErrorValue::new(
                        ErrorCode::ProviderUnavailable,
                        format!("process {pid} vanished"),
                    );
                    if sink.fail(error).await.is_err() {
                        return;
                    }
                }
            }
        },
    )
}

#[tokio::test]
async fn should_yield_both_the_successes_and_the_per_item_failures() {
    let collected = within(flaky().collect()).await;

    assert_eq!(
        field_of(collected.values(), "pid"),
        [Value::Int(0), Value::Int(2), Value::Int(4)],
        "the objects that were readable are still delivered"
    );
    assert_eq!(
        collected.errors().len(),
        3,
        "three failures are three errors, not one summary (spec §16.5)"
    );
    assert!(
        collected
            .errors()
            .iter()
            .all(|error| error.code() == ErrorCode::ProviderUnavailable)
    );
}

#[tokio::test]
async fn should_carry_partial_failures_through_every_later_stage() {
    let collected = within(
        flaky()
            .transform(Where::new(|_: &Value| Value::Bool(true)))
            .expect("`where` streams")
            .transform(Where::new(|_: &Value| Value::Bool(true)))
            .expect("`where` streams")
            .collect(),
    )
    .await;

    assert_eq!(collected.values().len(), 3);
    assert_eq!(
        collected.errors().len(),
        3,
        "a later stage must not swallow an earlier stage's failures"
    );
}

#[tokio::test]
async fn should_interleave_failures_with_values_rather_than_waiting_for_the_end() {
    within(async {
        let mut stream = flaky();
        let mut seen_value = false;
        let mut seen_failure = false;
        while let Some(event) = stream.recv().await {
            match event {
                StreamEvent::Value(_) => seen_value = true,
                StreamEvent::Failure(_) => seen_failure = true,
            }
            if seen_value && seen_failure {
                return;
            }
        }
        panic!("a consumer must see both values and failures while the stream runs");
    })
    .await;
}
