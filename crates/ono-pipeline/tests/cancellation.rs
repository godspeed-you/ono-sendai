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

#[tokio::test]
async fn should_wake_a_waiting_producer_when_the_consumer_drops_the_stream() {
    // A producer blocked on the outside world has no next `send` to learn from; the sink tells
    // it directly that nothing will read another value, so `tail journal | take 1` stops
    // reading the journal the moment `take` is satisfied.
    within(async {
        let (woke, awoken) = tokio::sync::oneshot::channel::<()>();
        let stream = ValueStream::spawn(
            PipelineConfig::new(),
            Boundedness::Unbounded,
            |sink| async move {
                sink.closed().await;
                let _ = woke.send(());
            },
        );
        drop(stream);
        awoken
            .await
            .expect("the producer observed the consumer going away");
    })
    .await;
}

// --- v0.4.1 §23.3: cancellation stops a capture growing (issue #71) ---------------------------

/// A source that counts what it has sent, so a test can watch retention stop rather than time it.
fn counted(cancel: CancelToken, sent: std::sync::Arc<std::sync::atomic::AtomicU64>) -> ValueStream {
    ValueStream::spawn(
        PipelineConfig::new()
            .with_capacity(4)
            .with_cancel_token(cancel),
        // Declared bounded, because a materializer refuses an unbounded upstream outright (§22.3)
        // and the question here is what happens to one that is allowed to start.
        Boundedness::Bounded,
        move |sink| async move {
            let payload = Value::string(&"x".repeat(1024));
            while sink.send(payload.clone()).await.is_ok() {
                sent.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        },
    )
}

#[tokio::test]
async fn should_stop_a_capture_growing_when_the_scope_is_cancelled() {
    // §23.3: "Cancellation while capturing MUST stop upstream consumption promptly and release
    // retained values as soon as the owning operation unwinds."
    //
    // Nothing here reads a clock. What is asserted is the outcome cancellation is *for*: the
    // materializing operation unwinds instead of running to the end of its budget, and once it
    // has unwound the source has stopped producing — read twice, and equal. A latency figure over
    // the same event is issue #71's other half and is measured by the benchmark harness of phase
    // H7 (#83, #84, ADR-0459), because a millisecond threshold on shared hardware is this
    // repository's most reliable source of flakes (issue #21, ADR-0252).
    within(async {
        let cancel = CancelToken::new();
        let sent = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let stream = counted(cancel.clone(), std::sync::Arc::clone(&sent));

        // A budget large enough that reaching it, rather than the cancellation, would take a very
        // long time: whatever ends this operation, it is not the ceiling.
        let budget = ono_pipeline::Budget::of("capture", 10_000_000, 1 << 40);
        let collecting = tokio::spawn(ono_pipeline::materialize(stream, budget));

        while sent.load(std::sync::atomic::Ordering::SeqCst) < 32 {
            tokio::task::yield_now().await;
        }
        cancel.cancel();

        let collected = collecting
            .await
            .expect("the collecting task must not panic")
            .expect("a cancelled materialization ends without a resource refusal");

        let at_unwind = sent.load(std::sync::atomic::Ordering::SeqCst);
        assert!(
            at_unwind < 10_000_000,
            "the operation ran to its ceiling rather than stopping at the signal: {at_unwind}"
        );

        // §23.3's "stop upstream consumption": once the owning operation has unwound, the source
        // is not producing into anything and cannot be.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(
            sent.load(std::sync::atomic::Ordering::SeqCst),
            at_unwind,
            "the capture kept growing after the operation that owned it had unwound"
        );

        assert!(
            collected
                .errors()
                .iter()
                .any(|error| error.code() == ErrorCode::StreamCancelled),
            "the consumer is told why it ended (spec §18.5): {:?}",
            collected.errors()
        );
    })
    .await;
}

// --- v0.4.1 §28.3: cancellation wins the race with capacity (issue #81) ------------------------

#[tokio::test]
async fn should_stop_an_in_flight_block_when_the_pipeline_is_cancelled() {
    // v0.4.1 §28.3: "when cancellation and capacity availability race, cancellation SHOULD win
    // such that a cancelled producer does not continue to enqueue a large tail of values."
    //
    // The race is arranged rather than waited for. Capacity is one and nothing reads, so by the
    // time the counts stop moving every part of the pipeline is parked on a `send` that only
    // needs one reader to complete — which is exactly the moment §28.3 is about. Cancelling then
    // must stop them where they stand, and the proof is a pair of readings that are equal: what
    // the stage had mapped before the cancellation is what it had mapped after it (ADR-0459 —
    // a stop is proven by what stops, never by a stopwatch).
    within(async {
        let cancel = CancelToken::new();
        let mapped = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = std::sync::Arc::clone(&mapped);

        let mut stream = ValueStream::spawn(
            PipelineConfig::new()
                .with_capacity(1)
                .with_cancel_token(cancel.clone()),
            Boundedness::Unbounded,
            |sink| async move {
                let mut next: i128 = 0;
                while sink.send(Value::Int(next)).await.is_ok() {
                    next += 1;
                }
            },
        )
        .transform(ono_pipeline::Each::new(move |value: &Value| {
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(vec![value.clone()])
        }))
        .expect("`each` never requires finite input");

        // Run everything to a standstill: every channel full, every task parked on a send.
        let parked = loop {
            let before = mapped.load(std::sync::atomic::Ordering::SeqCst);
            for _ in 0..64 {
                tokio::task::yield_now().await;
            }
            if mapped.load(std::sync::atomic::Ordering::SeqCst) == before && before > 0 {
                break before;
            }
        };

        cancel.cancel();
        for _ in 0..256 {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            mapped.load(std::sync::atomic::Ordering::SeqCst),
            parked,
            "a cancelled stage enqueued a tail after it was cancelled"
        );

        // And the consumer is told, once, why the values stopped.
        let mut cancelled = false;
        while let Some(event) = stream.recv().await {
            if let StreamEvent::Failure(error) = event {
                assert_eq!(error.code(), ErrorCode::StreamCancelled);
                cancelled = true;
            }
        }
        assert!(
            cancelled,
            "a cancelled pipeline tells its consumer why it ended (spec §18.5)"
        );
    })
    .await;
}
