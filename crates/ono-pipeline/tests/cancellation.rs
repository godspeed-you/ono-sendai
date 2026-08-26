//! Spec §18.5: cancellation propagates through native pipelines. A cancelled pipeline stops at
//! its next await and its consumer observes `stream.cancelled` (Ono-Sendai-E0802).

mod common;

use common::within;
use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, CancelToken, PipelineConfig, StreamEvent, ValueStream, Where};
use ono_value::Value;

fn endless(cancel: CancelToken) -> ValueStream {
    ValueStream::spawn(
        PipelineConfig::new()
            .with_capacity(4)
            .with_cancel_token(cancel),
        Boundedness::Unbounded,
        |sink| async move {
            let mut next: i128 = 0;
            while sink.send(Value::Int(next)).await.is_ok() {
                next += 1;
            }
        },
    )
}

#[tokio::test]
async fn should_terminate_an_infinite_pipeline_when_it_is_cancelled() {
    within(async {
        let cancel = CancelToken::new();
        let mut stream = endless(cancel.clone())
            .transform(Where::new(|_: &Value| Value::Bool(true)))
            .expect("`where` streams");

        for _ in 0..3 {
            stream
                .recv()
                .await
                .expect("values flow before cancellation");
        }
        cancel.cancel();

        let mut cancelled = false;
        while let Some(event) = stream.recv().await {
            if let StreamEvent::Failure(error) = event {
                assert_eq!(error.code(), ErrorCode::StreamCancelled);
                assert_eq!(error.code().code(), "Ono-Sendai-E0802");
                cancelled = true;
            }
        }
        assert!(
            cancelled,
            "a cancelled pipeline must tell its consumer why it ended (spec §18.5)"
        );
    })
    .await;
}

#[tokio::test]
async fn should_report_cancellation_exactly_once_however_long_the_pipeline_is() {
    within(async {
        let cancel = CancelToken::new();
        let mut stream = endless(cancel.clone())
            .transform(Where::new(|_: &Value| Value::Bool(true)))
            .expect("`where` streams")
            .transform(Where::new(|_: &Value| Value::Bool(true)))
            .expect("`where` streams")
            .transform(Where::new(|_: &Value| Value::Bool(true)))
            .expect("`where` streams");

        stream
            .recv()
            .await
            .expect("values flow before cancellation");
        cancel.cancel();

        let mut cancellations = 0;
        while let Some(event) = stream.recv().await {
            if let StreamEvent::Failure(error) = event
                && error.code() == ErrorCode::StreamCancelled
            {
                cancellations += 1;
            }
        }
        assert_eq!(
            cancellations, 1,
            "one cancellation is one event, not one per stage"
        );
    })
    .await;
}

#[tokio::test]
async fn should_cancel_a_pipeline_that_has_not_started_producing() {
    within(async {
        let cancel = CancelToken::new();
        cancel.cancel();
        let mut stream = endless(cancel.clone());

        let mut values = 0;
        let mut cancelled = false;
        while let Some(event) = stream.recv().await {
            match event {
                StreamEvent::Value(_) => values += 1,
                StreamEvent::Failure(error) => {
                    cancelled |= error.code() == ErrorCode::StreamCancelled;
                }
            }
        }
        assert!(cancelled, "the consumer learns the pipeline was cancelled");
        assert_eq!(
            values, 0,
            "a pipeline cancelled before it ran must deliver nothing, not a value it raced for"
        );
    })
    .await;
}

#[tokio::test]
async fn should_leave_an_uncancelled_pipeline_free_of_cancellation_errors() {
    let collected =
        within(ValueStream::from_values([Value::Int(1), Value::Int(2)]).collect()).await;
    assert_eq!(collected.values(), [Value::Int(1), Value::Int(2)]);
    assert!(
        collected.errors().is_empty(),
        "a pipeline that ended on its own was not cancelled"
    );
}

#[tokio::test]
async fn should_expose_the_token_so_a_caller_can_cancel_a_stream_it_was_handed() {
    within(async {
        let mut stream = endless(CancelToken::new());
        stream.recv().await.expect("values flow");
        stream.cancel_token().cancel();

        let mut events = 0;
        while stream.recv().await.is_some() {
            events += 1;
            assert!(
                events < 10_000,
                "cancellation must actually stop the stream"
            );
        }
    })
    .await;
}
