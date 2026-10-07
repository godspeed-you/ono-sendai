//! Spec §11.1: a blocking transform over an unbounded stream must be rejected with a structured
//! error, or bounded by an explicit window.

mod common;

use common::{demo, within};
use ono_core::ErrorCode;
use ono_pipeline::{
    Boundedness, Count, Group, Join, Measure, PipelineConfig, Reduce, Sort, Take, ValueStream,
    Where, Window,
};
use ono_value::Value;

fn endless() -> ValueStream {
    ValueStream::spawn(
        PipelineConfig::new().with_capacity(4),
        Boundedness::Unbounded,
        |sink| async move {
            let mut next: i128 = 0;
            while sink.send(Value::Int(next % 7)).await.is_ok() {
                next += 1;
            }
        },
    )
}

#[tokio::test]
async fn should_reject_a_blocking_transform_when_the_stream_is_unbounded() {
    let error = endless()
        .transform(Sort::new(|value: &Value| Ok(value.clone())))
        .expect_err("`sort` cannot order a stream that never ends");
    assert_eq!(error.code(), ErrorCode::StreamUnboundedOperation);
    assert_eq!(error.code().code(), "Ono-Sendai-E0801");
    assert!(
        error.help().is_some(),
        "a rejection must tell the user how to bound the stream"
    );
}

#[tokio::test]
async fn should_reject_every_blocking_transform_when_the_stream_is_unbounded() {
    let rejected: Vec<ErrorCode> = vec![
        endless()
            .transform(Count::new())
            .err()
            .map(|error| error.code())
            .expect("`count` needs input that ends"),
        endless()
            .transform(Group::new(|value: &Value| Ok(value.clone())))
            .err()
            .map(|error| error.code())
            .expect("`group` needs input that ends"),
        endless()
            .transform(Measure::new(|value: &Value| Ok(value.clone())))
            .err()
            .map(|error| error.code())
            .expect("`measure` needs input that ends"),
        endless()
            .transform(Reduce::new(|acc: &Value, value: &Value| acc.add(value)))
            .err()
            .map(|error| error.code())
            .expect("`reduce` needs input that ends"),
        endless()
            .transform(Join::new([demo(1, "a", None)], |value: &Value| {
                Ok(value.clone())
            }))
            .err()
            .map(|error| error.code())
            .expect("`join` needs input that ends"),
    ];
    assert!(
        rejected
            .iter()
            .all(|code| *code == ErrorCode::StreamUnboundedOperation),
        "every blocking transform reports the same structured error: {rejected:?}"
    );
}

#[tokio::test]
async fn should_accept_a_blocking_transform_when_an_explicit_window_bounds_the_stream() {
    let collected = within(
        endless()
            .transform(Count::new().with_window(Window::count(12)))
            .expect("a window bounds the input")
            .collect(),
    )
    .await;

    assert_eq!(collected.values(), [Value::Int(12)]);
    assert!(collected.errors().is_empty());
}

#[tokio::test]
async fn should_accept_a_blocking_transform_after_take_has_bounded_the_stream() {
    let collected = within(
        endless()
            .transform(Take::new(5))
            .expect("`take` streams")
            .transform(Sort::new(|value: &Value| Ok(value.clone())))
            .expect("`take` made the stream finite")
            .collect(),
    )
    .await;

    assert_eq!(
        collected.values(),
        [
            Value::Int(0),
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
            Value::Int(4)
        ]
    );
}

#[tokio::test]
async fn should_keep_streaming_transforms_available_on_an_unbounded_stream() {
    let mut stream = endless()
        .transform(Where::new(|value: &Value| {
            Value::Bool(value == &Value::Int(3))
        }))
        .expect("`where` streams whatever it is given");
    assert_eq!(stream.boundedness(), Boundedness::Unbounded);
    let first = within(stream.recv()).await;
    assert!(first.is_some(), "a streaming filter yields before the end");
}

#[tokio::test]
async fn should_refuse_an_unbounded_upstream_before_waiting_when_the_operation_requires_finite_input()
 {
    // v0.4.1 §22.3: "It MUST NOT wait forever to discover that an unbounded stream never ends."
    // The proof that it did not wait is that the source is still running and has been asked for
    // nothing: `transform` answers from the declared boundedness alone, before a task is spawned.
    let source = endless();
    assert_eq!(source.boundedness(), Boundedness::Unbounded);

    let error = source
        .transform(Sort::new(|value: &Value| Ok(value.clone())))
        .expect_err("`sort` requires finite input");

    assert_eq!(error.code(), ErrorCode::StreamUnboundedOperation);
    let message = error.render_full();
    assert!(
        message.contains("sort") && message.contains("finite"),
        "§54.1: the refusal names the operation and the requirement it could not meet: {message}"
    );
    assert!(
        message.contains("unbounded"),
        "§54.1: the refusal says what the upstream declared itself to be: {message}"
    );
    assert!(
        error.help().is_some(),
        "a refusal tells the user how to satisfy the requirement (§54.1)"
    );
}

#[tokio::test]
async fn should_refuse_before_a_value_is_drawn_when_a_materializer_meets_an_unbounded_upstream() {
    // The same refusal through the §22 helper rather than through `transform`, so an evaluator
    // that materializes without a `Transform` cannot be the path that waits forever.
    within(async {
        let error =
            ono_pipeline::materialize(endless(), ono_pipeline::Budget::materialization("collect"))
                .await
                .expect_err("the materialization helper refuses an unbounded upstream");
        assert_eq!(error.code(), ErrorCode::StreamUnboundedOperation);
    })
    .await;
}

#[tokio::test]
async fn should_report_a_finite_source_as_bounded() {
    let stream = ValueStream::from_values([Value::Int(1)]);
    assert_eq!(stream.boundedness(), Boundedness::Bounded);
}

// --- constant-state `measure` (ADR-0953, issue #174) ---------------------------------------------

/// A source that produces `values` and then waits, open, until the test ends.
fn produces_then_waits(values: Vec<i128>) -> (ValueStream, tokio::sync::oneshot::Sender<()>) {
    let (release, released) = tokio::sync::oneshot::channel::<()>();
    let stream = ValueStream::spawn(
        PipelineConfig::new().with_capacity(4),
        Boundedness::Unbounded,
        move |sink| async move {
            for value in values {
                if sink.send(Value::Int(value)).await.is_err() {
                    return;
                }
            }
            let _ = released.await;
        },
    );
    (stream, release)
}

fn field(record: &Value, name: &str) -> Value {
    match record.follow(&[ono_value::FieldStep::required(name)]) {
        Ok(value) => value,
        Err(error) => unreachable!("`{name}` is a field of ono.measure/1: {error}"),
    }
}

#[tokio::test]
async fn should_answer_after_every_value_when_constant_state_measure_reads_an_unbounded_stream() {
    // Issue #174's exit test: "`measure count` over an unbounded source answers without
    // materializing". The source has produced three values and is still open; a stage that
    // waited for its end, or refused it, would have answered nothing.
    let (source, still_open) = produces_then_waits(vec![1, 2, 3]);
    let mut measured = source
        .transform(Measure::constant_state(|value: &Value| Ok(value.clone())))
        .expect("constant-state statistics need no end, so an unbounded stream is a legal input");
    assert_eq!(
        measured.boundedness(),
        Boundedness::Unbounded,
        "a running answer to a stream that does not end does not end either"
    );

    let mut answers = Vec::new();
    while answers.len() < 3 {
        match within(measured.recv()).await {
            Some(ono_pipeline::StreamEvent::Value(record)) => answers.push(record),
            other => unreachable!("expected a running record, got {other:?}"),
        }
    }

    let counts: Vec<Value> = answers
        .iter()
        .map(|record| field(record, "count"))
        .collect();
    let sums: Vec<Value> = answers.iter().map(|record| field(record, "sum")).collect();
    assert_eq!(counts, [Value::Int(1), Value::Int(2), Value::Int(3)]);
    assert_eq!(sums, [Value::Int(1), Value::Int(3), Value::Int(6)]);
    assert_eq!(field(&answers[2], "max"), Value::Int(3));
    assert_eq!(
        field(&answers[2], "median"),
        Value::Null,
        "a median is the distribution's, and nothing here holds it"
    );
    assert!(
        !still_open.is_closed(),
        "the three answers arrived while the source was still open"
    );
}
