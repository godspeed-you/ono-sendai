//! The streaming transforms of spec §11.1 and §53, and the `where` semantics ADR-0014 freezes.

mod common;

use common::{demo, demo_owned, demo_unreadable, field_of, ints, within};
use ono_core::ErrorCode;
use ono_pipeline::{
    Each, First, Last, PathSegment, Select, SelectField, Skip, Take, ValueStream, Where,
};
use ono_value::{ErrorValue, Value};

// --- `where`: ADR-0014's matrix ------------------------------------------------------------

#[tokio::test]
async fn should_keep_a_value_when_its_predicate_is_true() {
    let collected = within(
        ValueStream::from_values(ints(5))
            .transform(Where::new(|value: &Value| {
                Value::Bool(matches!(value, Value::Int(n) if n % 2 == 0))
            }))
            .expect("`where` streams")
            .collect(),
    )
    .await;
    assert_eq!(
        collected.values(),
        [Value::Int(0), Value::Int(2), Value::Int(4)]
    );
    assert!(collected.errors().is_empty());
}

#[tokio::test]
async fn should_exclude_a_value_when_its_predicate_is_false() {
    let collected = within(
        ValueStream::from_values(ints(3))
            .transform(Where::new(|_: &Value| Value::Bool(false)))
            .expect("`where` streams")
            .collect(),
    )
    .await;
    assert!(collected.values().is_empty());
    assert_eq!(
        collected.diagnostics().excluded_unknown(),
        0,
        "`false` is a decision, not an unknown"
    );
}

#[tokio::test]
async fn should_exclude_a_value_and_count_it_when_its_predicate_is_null() {
    let collected = within(
        ValueStream::from_values(ints(4))
            .transform(Where::new(|value: &Value| match value {
                Value::Int(n) if *n < 2 => Value::Bool(true),
                _ => Value::Null,
            }))
            .expect("`where` streams")
            .collect(),
    )
    .await;
    assert_eq!(collected.values(), [Value::Int(0), Value::Int(1)]);
    assert!(
        collected.errors().is_empty(),
        "an unknown predicate is not a failure"
    );
    assert_eq!(
        collected.diagnostics().excluded_unknown(),
        2,
        "a user surprised by a row count must be able to find the missing rows (ADR-0014)"
    );
}

#[tokio::test]
async fn should_exclude_a_value_and_report_it_when_its_predicate_fails() {
    let denied = ErrorValue::new(ErrorCode::IoPermissionDenied, "cannot read memory");
    let collected = within(
        ValueStream::from_values([demo(7, "guarded", None), demo(8, "plain", Some(1.0))])
            .transform(Where::new(move |value: &Value| match value {
                Value::Record(record) if record.get("pid") == Some(&Value::Int(7)) => {
                    denied.clone().into_value()
                }
                _ => Value::Bool(true),
            }))
            .expect("`where` streams")
            .collect(),
    )
    .await;

    assert_eq!(field_of(collected.values(), "pid"), [Value::Int(8)]);
    assert_eq!(
        collected.errors().len(),
        1,
        "the failure is reported, not swallowed"
    );
    let error = &collected.errors()[0];
    assert_eq!(error.code(), ErrorCode::IoPermissionDenied);
    assert!(
        error.target().is_some(),
        "a partial failure carries the identity of the object it is about (spec §16.5)"
    );
    assert_eq!(
        collected.diagnostics().excluded_unknown(),
        0,
        "a failure is not an unknown; conflating them is the ambiguity Ono removes"
    );
}

#[tokio::test]
async fn should_report_a_predicate_that_yields_neither_a_boolean_nor_null() {
    let collected = within(
        ValueStream::from_values(ints(2))
            .transform(Where::new(|_: &Value| Value::string("maybe")))
            .expect("`where` streams")
            .collect(),
    )
    .await;
    assert!(collected.values().is_empty());
    assert_eq!(collected.errors().len(), 2);
    assert!(
        collected
            .errors()
            .iter()
            .all(|error| error.code() == ErrorCode::TypeMismatch)
    );
}

#[tokio::test]
async fn should_yield_nothing_when_where_runs_over_an_empty_stream() {
    let collected = within(
        ValueStream::from_values([])
            .transform(Where::new(|_: &Value| Value::Bool(true)))
            .expect("`where` streams")
            .collect(),
    )
    .await;
    assert!(collected.values().is_empty());
    assert!(collected.errors().is_empty());
}

// --- `select` -------------------------------------------------------------------------------

#[tokio::test]
async fn should_project_the_named_fields_in_the_order_given() {
    let collected = within(
        ValueStream::from_values([demo(11, "alpha", Some(3.5))])
            .transform(
                Select::new([SelectField::field("name"), SelectField::field("pid")])
                    .expect("distinct projected names"),
            )
            .expect("`select` streams")
            .collect(),
    )
    .await;

    assert_eq!(
        field_of(collected.values(), "name"),
        [Value::string("alpha")]
    );
    assert_eq!(field_of(collected.values(), "pid"), [Value::Int(11)]);
    let record = match &collected.values()[0] {
        Value::Record(record) => record.clone(),
        other => panic!("select yields records, got {other}"),
    };
    assert_eq!(
        record
            .schema()
            .fields()
            .iter()
            .map(|field| field.name().to_owned())
            .collect::<Vec<_>>(),
        ["name", "pid"],
        "projection order is the order the user wrote"
    );
    assert!(
        record.get("cpu").is_none(),
        "a projection keeps only what was projected"
    );
}

#[tokio::test]
async fn should_project_a_field_under_a_new_name() {
    let collected = within(
        ValueStream::from_values([demo(12, "beta", None)])
            .transform(
                Select::new([SelectField::field("pid").named("id")]).expect("distinct names"),
            )
            .expect("`select` streams")
            .collect(),
    )
    .await;
    assert_eq!(field_of(collected.values(), "id"), [Value::Int(12)]);
}

#[tokio::test]
async fn should_project_a_nested_path() {
    let collected = within(
        ValueStream::from_values([demo_owned(13, "root")])
            .transform(
                Select::new([SelectField::path([
                    PathSegment::required("owner"),
                    PathSegment::required("name"),
                ])])
                .expect("distinct names"),
            )
            .expect("`select` streams")
            .collect(),
    )
    .await;
    assert_eq!(
        field_of(collected.values(), "name"),
        [Value::string("root")]
    );
}

#[tokio::test]
async fn should_yield_null_for_an_optional_step_over_a_missing_field() {
    let collected = within(
        ValueStream::from_values([demo(14, "gamma", None)])
            .transform(
                Select::new([SelectField::path([PathSegment::optional("nowhere")])])
                    .expect("distinct names"),
            )
            .expect("`select` streams")
            .collect(),
    )
    .await;
    assert_eq!(field_of(collected.values(), "nowhere"), [Value::Null]);
    assert!(collected.errors().is_empty());
}

#[tokio::test]
async fn should_keep_a_failed_field_read_as_an_error_rather_than_a_null() {
    let denied = ErrorValue::new(ErrorCode::IoPermissionDenied, "cannot read cpu");
    let collected = within(
        ValueStream::from_values([demo_unreadable(15, "delta", denied)])
            .transform(Select::new([SelectField::field("cpu")]).expect("distinct names"))
            .expect("`select` streams")
            .collect(),
    )
    .await;
    let projected = field_of(collected.values(), "cpu");
    assert!(
        matches!(&projected[0], Value::Error(error) if error.code() == ErrorCode::IoPermissionDenied),
        "spec §10.5 keeps `unknown` and `could not be read` apart, got {:?}",
        projected[0]
    );
}

#[tokio::test]
async fn should_project_a_computed_field() {
    let collected = within(
        ValueStream::from_values([demo(16, "epsilon", Some(4.0))])
            .transform(
                Select::new([SelectField::computed("doubled", |value: &Value| {
                    value
                        .follow(&[ono_value::FieldStep::required("cpu")])?
                        .mul(&Value::Int(2))
                })])
                .expect("distinct names"),
            )
            .expect("`select` streams")
            .collect(),
    )
    .await;
    assert_eq!(field_of(collected.values(), "doubled"), [Value::Float(8.0)]);
}

#[tokio::test]
async fn should_reject_a_projection_that_names_the_same_output_twice() {
    let error = Select::new([SelectField::field("pid"), SelectField::field("pid")])
        .expect_err("two columns cannot share a name");
    assert_eq!(error.code(), ErrorCode::TypeUnknownField);
}

// --- `take` / `skip` / `first` / `last` -----------------------------------------------------

#[tokio::test]
async fn should_take_the_first_n_values() {
    let collected = within(
        ValueStream::from_values(ints(10))
            .transform(Take::new(3))
            .expect("`take` streams")
            .collect(),
    )
    .await;
    assert_eq!(
        collected.values(),
        [Value::Int(0), Value::Int(1), Value::Int(2)]
    );
}

#[tokio::test]
async fn should_take_nothing_when_the_count_is_zero() {
    let collected = within(
        ValueStream::from_values(ints(10))
            .transform(Take::new(0))
            .expect("`take` streams")
            .collect(),
    )
    .await;
    assert!(collected.values().is_empty());
}

#[tokio::test]
async fn should_take_everything_when_the_stream_is_shorter_than_the_count() {
    let collected = within(
        ValueStream::from_values(ints(2))
            .transform(Take::new(9))
            .expect("`take` streams")
            .collect(),
    )
    .await;
    assert_eq!(collected.values(), [Value::Int(0), Value::Int(1)]);
}

#[tokio::test]
async fn should_skip_the_first_n_values() {
    let collected = within(
        ValueStream::from_values(ints(5))
            .transform(Skip::new(3))
            .expect("`skip` streams")
            .collect(),
    )
    .await;
    assert_eq!(collected.values(), [Value::Int(3), Value::Int(4)]);
}

#[tokio::test]
async fn should_skip_everything_when_the_stream_is_shorter_than_the_count() {
    let collected = within(
        ValueStream::from_values(ints(2))
            .transform(Skip::new(9))
            .expect("`skip` streams")
            .collect(),
    )
    .await;
    assert!(collected.values().is_empty());
}

#[tokio::test]
async fn should_yield_the_first_value_only() {
    let collected = within(
        ValueStream::from_values(ints(4))
            .transform(First::new())
            .expect("`first` streams")
            .collect(),
    )
    .await;
    assert_eq!(collected.values(), [Value::Int(0)]);
}

#[tokio::test]
async fn should_yield_the_last_value_only() {
    let collected = within(
        ValueStream::from_values(ints(4))
            .transform(Last::new())
            .expect("`last` needs input that ends, and this input ends")
            .collect(),
    )
    .await;
    assert_eq!(collected.values(), [Value::Int(3)]);
}

#[tokio::test]
async fn should_yield_nothing_for_first_and_last_over_an_empty_stream() {
    let first = within(
        ValueStream::from_values([])
            .transform(First::new())
            .expect("`first` streams")
            .collect(),
    )
    .await;
    let last = within(
        ValueStream::from_values([])
            .transform(Last::new())
            .expect("`last` is bounded")
            .collect(),
    )
    .await;
    assert!(first.values().is_empty());
    assert!(last.values().is_empty());
}

#[tokio::test]
async fn should_yield_the_single_element_for_first_and_last_over_one_value() {
    let first = within(
        ValueStream::from_values([Value::Int(42)])
            .transform(First::new())
            .expect("`first` streams")
            .collect(),
    )
    .await;
    let last = within(
        ValueStream::from_values([Value::Int(42)])
            .transform(Last::new())
            .expect("`last` is bounded")
            .collect(),
    )
    .await;
    assert_eq!(first.values(), [Value::Int(42)]);
    assert_eq!(last.values(), [Value::Int(42)]);
}

// --- `each` ---------------------------------------------------------------------------------

#[tokio::test]
async fn should_map_each_value_through_the_body() {
    let collected = within(
        ValueStream::from_values(ints(3))
            .transform(Each::new(|value: &Value| {
                Ok(vec![value.mul(&Value::Int(10))?])
            }))
            .expect("`each` streams")
            .collect(),
    )
    .await;
    assert_eq!(
        collected.values(),
        [Value::Int(0), Value::Int(10), Value::Int(20)]
    );
}

#[tokio::test]
async fn should_flatten_many_outputs_of_each_into_one_stream() {
    let collected = within(
        ValueStream::from_values(ints(2))
            .transform(Each::new(|value: &Value| {
                Ok(vec![value.clone(), value.clone()])
            }))
            .expect("`each` streams")
            .collect(),
    )
    .await;
    assert_eq!(
        collected.values(),
        [Value::Int(0), Value::Int(0), Value::Int(1), Value::Int(1)],
        "spec §53: many outputs flatten, they do not nest"
    );
}

#[tokio::test]
async fn should_drop_a_value_when_each_yields_nothing_for_it() {
    let collected = within(
        ValueStream::from_values(ints(4))
            .transform(Each::new(|value: &Value| {
                if value == &Value::Int(2) {
                    Ok(Vec::new())
                } else {
                    Ok(vec![value.clone()])
                }
            }))
            .expect("`each` streams")
            .collect(),
    )
    .await;
    assert_eq!(
        collected.values(),
        [Value::Int(0), Value::Int(1), Value::Int(3)]
    );
}

#[tokio::test]
async fn should_report_a_failing_each_body_per_value_and_keep_going() {
    let collected = within(
        ValueStream::from_values(ints(3))
            .transform(Each::new(|value: &Value| {
                if value == &Value::Int(1) {
                    Err(ErrorValue::new(ErrorCode::ProviderUnavailable, "gone"))
                } else {
                    Ok(vec![value.clone()])
                }
            }))
            .expect("`each` streams")
            .collect(),
    )
    .await;
    assert_eq!(collected.values(), [Value::Int(0), Value::Int(2)]);
    assert_eq!(collected.errors().len(), 1);
    assert_eq!(collected.errors()[0].code(), ErrorCode::ProviderUnavailable);
}

#[tokio::test]
async fn should_hold_no_more_than_the_bounded_channel_and_one_in_flight_frame() {
    // The `Done` line of v0.4.1 §58.2: "memory stays within bounded channel plus per-item frame
    // overhead". An item transform over an endless producer is the case where that either holds
    // or does not: nothing about the stage's own state may grow with how much the source is
    // willing to produce.
    //
    // The bound is arithmetic rather than a guess. Two channels of `capacity` stand around the
    // stage — the one it reads and the one it writes — and the stage itself holds one value while
    // it maps it. So by the time the consumer has read one value, the body can have run at most
    // `1 + 2 * capacity + 1` times, however long the producer was left alone to run.
    let capacity = 4;
    let mapped = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = std::sync::Arc::clone(&mapped);

    let mut stream = ValueStream::spawn(
        ono_pipeline::PipelineConfig::new().with_capacity(capacity),
        ono_pipeline::Boundedness::Unbounded,
        move |sink| async move {
            let mut next: i128 = 0;
            while sink.send(Value::Int(next)).await.is_ok() {
                next += 1;
            }
        },
    )
    .transform(Each::new(move |value: &Value| {
        counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(vec![value.clone()])
    }))
    .expect("`each` never requires finite input (Appendix E)");

    // Every chance to run ahead before the single read, so a stage that did accumulate would have
    // shown it here rather than in a later run on a busier machine.
    for _ in 0..256 {
        tokio::task::yield_now().await;
    }
    let first = within(stream.recv()).await;

    assert_eq!(
        first,
        Some(ono_pipeline::StreamEvent::Value(Value::Int(0))),
        "the first value the source produced is the first one out"
    );
    let held = mapped.load(std::sync::atomic::Ordering::SeqCst);
    assert!(
        (1..=1 + 2 * capacity + 1).contains(&held),
        "one read costs the two bounded channels and the value in flight, and no more: the body \
         ran {held} times with a capacity of {capacity}"
    );
}
