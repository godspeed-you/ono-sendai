//! Spec §11.2: a slow consumer must stop an infinite producer from exhausting memory.
//!
//! This is the test the crate exists for. It asserts the observable consequence — how much the
//! producer was allowed to produce — rather than anything about channels or tasks.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::within;
use ono_pipeline::{Boundedness, PipelineConfig, StreamEvent, ValueStream, Where};
use ono_value::Value;

/// Counts how far an endless producer got, so the test can assert on it.
fn endless(config: PipelineConfig, produced: Arc<AtomicUsize>) -> ValueStream {
    ValueStream::spawn(config, Boundedness::Unbounded, move |sink| async move {
        let mut next: i128 = 0;
        loop {
            produced.fetch_add(1, Ordering::SeqCst);
            if sink.send(Value::Int(next)).await.is_err() {
                return;
            }
            next += 1;
        }
    })
}

#[tokio::test]
async fn should_bound_an_infinite_producer_when_the_consumer_reads_slowly() {
    within(async {
        let produced = Arc::new(AtomicUsize::new(0));
        let capacity = 4;
        let config = PipelineConfig::new().with_capacity(capacity);
        let mut stream = endless(config, Arc::clone(&produced));

        let reads = 10;
        for expected in 0..reads {
            // Give the producer every chance to run ahead before each read.
            for _ in 0..64 {
                tokio::task::yield_now().await;
            }
            let event = stream.recv().await.expect("the producer never ends");
            assert_eq!(
                event,
                StreamEvent::Value(Value::Int(expected as i128)),
                "values arrive in order"
            );
        }

        let count = produced.load(Ordering::SeqCst);
        // A bounded channel of `capacity` lets the producer sit at most one send ahead of the
        // buffer it has filled. Anything more means the engine is buffering without a limit.
        assert!(
            count <= reads + capacity + 2,
            "after {reads} reads with capacity {capacity} the producer had made {count} values; \
             an unbounded producer must be held back by the consumer (spec §11.2)"
        );
    })
    .await;
}

#[tokio::test]
async fn should_bound_an_infinite_producer_through_a_chain_of_transforms() {
    within(async {
        let produced = Arc::new(AtomicUsize::new(0));
        let capacity = 2;
        let config = PipelineConfig::new().with_capacity(capacity);
        let stages = 3;
        let mut stream = endless(config, Arc::clone(&produced))
            .transform(Where::new(|_: &Value| Value::Bool(true)))
            .expect("`where` streams")
            .transform(Where::new(|_: &Value| Value::Bool(true)))
            .expect("`where` streams")
            .transform(Where::new(|_: &Value| Value::Bool(true)))
            .expect("`where` streams");

        let reads = 5;
        for _ in 0..reads {
            for _ in 0..128 {
                tokio::task::yield_now().await;
            }
            stream.recv().await.expect("the producer never ends");
        }

        let count = produced.load(Ordering::SeqCst);
        // Each stage owns one buffer, so the ceiling grows with the pipeline, not with time.
        let ceiling = reads + (stages + 1) * (capacity + 2);
        assert!(
            count <= ceiling,
            "a {stages}-stage pipeline produced {count} values for {reads} reads; the ceiling is \
             {ceiling}. Backpressure must survive composition."
        );
    })
    .await;
}

#[tokio::test]
async fn should_stop_the_producer_when_the_consumer_goes_away() {
    within(async {
        let produced = Arc::new(AtomicUsize::new(0));
        let config = PipelineConfig::new().with_capacity(2);
        let stream = endless(config, Arc::clone(&produced));
        drop(stream);

        for _ in 0..256 {
            tokio::task::yield_now().await;
        }
        let settled = produced.load(Ordering::SeqCst);
        for _ in 0..256 {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            produced.load(Ordering::SeqCst),
            settled,
            "a producer whose consumer is gone must stop, the way `yes | head -1` stops"
        );
    })
    .await;
}
