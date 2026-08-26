//! Spec §34: pipeline throughput must not collapse on large streams, and a producer's first rows
//! must arrive promptly.
//!
//! The bounds here are deliberately generous: they catch a catastrophic regression — a quadratic
//! transform, a lost `Arc`, a per-value allocation storm — without failing on a loaded machine.

mod common;

use std::time::{Duration, Instant};

use common::{ints, within};
use ono_pipeline::{Boundedness, PipelineConfig, StreamEvent, Take, ValueStream, Where};
use ono_value::Value;

const VALUES: i128 = 100_000;

#[tokio::test]
async fn should_move_a_hundred_thousand_values_through_three_stages_quickly() {
    within(async {
        let started = Instant::now();
        let collected = ValueStream::from_values(ints(VALUES))
            .transform(Where::new(|value: &Value| {
                Value::Bool(matches!(value, Value::Int(n) if n % 2 == 0))
            }))
            .expect("`where` streams")
            .transform(Take::new(usize::MAX))
            .expect("`take` streams")
            .transform(Where::new(|value: &Value| {
                Value::Bool(matches!(value, Value::Int(n) if n % 3 != 0))
            }))
            .expect("`where` streams")
            .collect()
            .await;
        let elapsed = started.elapsed();

        assert_eq!(
            collected.values().len(),
            (0..VALUES).filter(|n| n % 2 == 0 && n % 3 != 0).count(),
            "the pipeline must be fast and also right"
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "{VALUES} values through three stages took {elapsed:?}; the budget is 5s (spec §34)"
        );
    })
    .await;
}

#[tokio::test]
async fn should_deliver_the_first_value_without_draining_the_producer() {
    within(async {
        let mut stream = ValueStream::spawn(
            PipelineConfig::new(),
            Boundedness::Unbounded,
            |sink| async move {
                let mut next: i128 = 0;
                while sink.send(Value::Int(next)).await.is_ok() {
                    next += 1;
                }
            },
        );

        let started = Instant::now();
        let first = stream.recv().await;
        let elapsed = started.elapsed();

        assert_eq!(first, Some(StreamEvent::Value(Value::Int(0))));
        assert!(
            elapsed < Duration::from_millis(50),
            "the first row took {elapsed:?}; spec §34 budgets 50 ms"
        );
    })
    .await;
}
