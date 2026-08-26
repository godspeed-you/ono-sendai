//! The transforms of spec §11.1 and §53 that need input which ends: `sort`, `group`, `count`,
//! `measure`, `reduce`, `join` and `diff`.

mod common;

use common::{demo, field_of, ints, within};
use ono_core::ErrorCode;
use ono_pipeline::{Count, Diff, Group, Join, JoinKind, Measure, Reduce, Sort, ValueStream};
use ono_value::{ByteSize, ErrorValue, FieldStep, Value};

fn cpu(value: &Value) -> Result<Value, ErrorValue> {
    value.follow(&[FieldStep::optional("cpu")])
}

fn pid(value: &Value) -> Result<Value, ErrorValue> {
    value.follow(&[FieldStep::optional("pid")])
}

// --- `sort` ---------------------------------------------------------------------------------

#[tokio::test]
async fn should_order_ascending_by_the_key() {
    let collected = within(
        ValueStream::from_values([Value::Int(3), Value::Int(1), Value::Int(2)])
            .transform(Sort::new(|value: &Value| Ok(value.clone())))
            .expect("finite input")
            .collect(),
    )
    .await;
    assert_eq!(
        collected.values(),
        [Value::Int(1), Value::Int(2), Value::Int(3)]
    );
}

#[tokio::test]
async fn should_order_descending_when_asked() {
    let collected = within(
        ValueStream::from_values([Value::Int(3), Value::Int(1), Value::Int(2)])
            .transform(Sort::new(|value: &Value| Ok(value.clone())).descending())
            .expect("finite input")
            .collect(),
    )
    .await;
    assert_eq!(
        collected.values(),
        [Value::Int(3), Value::Int(2), Value::Int(1)]
    );
}

#[tokio::test]
async fn should_keep_equal_keys_in_input_order_in_both_directions() {
    let input = [
        demo(1, "a", Some(5.0)),
        demo(2, "b", Some(5.0)),
        demo(3, "c", Some(5.0)),
    ];
    let ascending = within(
        ValueStream::from_values(input.clone())
            .transform(Sort::new(cpu))
            .expect("finite input")
            .collect(),
    )
    .await;
    let descending = within(
        ValueStream::from_values(input)
            .transform(Sort::new(cpu).descending())
            .expect("finite input")
            .collect(),
    )
    .await;

    assert_eq!(
        field_of(ascending.values(), "pid"),
        [Value::Int(1), Value::Int(2), Value::Int(3)],
        "a stable sort does not reshuffle equal keys"
    );
    assert_eq!(
        field_of(descending.values(), "pid"),
        [Value::Int(1), Value::Int(2), Value::Int(3)],
        "stability survives the direction"
    );
}

#[tokio::test]
async fn should_place_nulls_last_ascending_and_first_descending() {
    let input = [
        demo(1, "known", Some(2.0)),
        demo(2, "unknown", None),
        demo(3, "known", Some(1.0)),
    ];
    let ascending = within(
        ValueStream::from_values(input.clone())
            .transform(Sort::new(cpu))
            .expect("finite input")
            .collect(),
    )
    .await;
    let descending = within(
        ValueStream::from_values(input)
            .transform(Sort::new(cpu).descending())
            .expect("finite input")
            .collect(),
    )
    .await;

    assert_eq!(
        field_of(ascending.values(), "pid"),
        [Value::Int(3), Value::Int(1), Value::Int(2)],
        "unknown is never the smallest value (ADR-0014)"
    );
    assert_eq!(
        field_of(descending.values(), "pid"),
        [Value::Int(2), Value::Int(1), Value::Int(3)],
        "unknown is never the largest value either"
    );
}

#[tokio::test]
async fn should_sort_an_empty_and_a_single_element_stream() {
    let empty = within(
        ValueStream::from_values([])
            .transform(Sort::new(|value: &Value| Ok(value.clone())))
            .expect("finite input")
            .collect(),
    )
    .await;
    let single = within(
        ValueStream::from_values([Value::Int(7)])
            .transform(Sort::new(|value: &Value| Ok(value.clone())))
            .expect("finite input")
            .collect(),
    )
    .await;
    assert!(empty.values().is_empty());
    assert_eq!(single.values(), [Value::Int(7)]);
}

#[tokio::test]
async fn should_report_and_drop_a_value_whose_sort_key_cannot_be_read() {
    let collected = within(
        ValueStream::from_values([Value::Int(2), Value::string("x"), Value::Int(1)])
            .transform(Sort::new(|value: &Value| {
                value.follow(&[FieldStep::required("cpu")])
            }))
            .expect("finite input")
            .collect(),
    )
    .await;
    assert!(collected.values().is_empty());
    assert_eq!(
        collected.errors().len(),
        3,
        "every key failure is reported on its own, never collapsed (spec §16.5)"
    );
}

// --- `group` --------------------------------------------------------------------------------

#[tokio::test]
async fn should_group_by_the_key_and_keep_the_members() {
    let collected = within(
        ValueStream::from_values([
            demo(1, "alpha", None),
            demo(2, "beta", None),
            demo(3, "alpha", None),
        ])
        .transform(Group::new(|value: &Value| {
            value.follow(&[FieldStep::optional("name")])
        }))
        .expect("finite input")
        .collect(),
    )
    .await;

    assert_eq!(
        field_of(collected.values(), "key"),
        [Value::string("alpha"), Value::string("beta")],
        "groups appear in the order their key was first seen"
    );
    assert_eq!(
        field_of(collected.values(), "count"),
        [Value::Int(2), Value::Int(1)]
    );
    let members = field_of(collected.values(), "items");
    match &members[0] {
        Value::List(items) => assert_eq!(items.len(), 2),
        other => panic!("a group carries its members, got {other}"),
    }
}

#[tokio::test]
async fn should_group_unknown_keys_into_their_own_group() {
    let collected = within(
        ValueStream::from_values([demo(1, "a", Some(1.0)), demo(2, "b", None)])
            .transform(Group::new(cpu))
            .expect("finite input")
            .collect(),
    )
    .await;
    assert_eq!(
        field_of(collected.values(), "key"),
        [Value::Float(1.0), Value::Null],
        "`group` answers `which of these are unknown` rather than hiding them"
    );
}

#[tokio::test]
async fn should_group_an_empty_stream_into_no_groups() {
    let collected = within(
        ValueStream::from_values([])
            .transform(Group::new(|value: &Value| Ok(value.clone())))
            .expect("finite input")
            .collect(),
    )
    .await;
    assert!(collected.values().is_empty());
}

// --- `count` --------------------------------------------------------------------------------

#[tokio::test]
async fn should_count_the_values_of_a_finite_stream() {
    let collected = within(
        ValueStream::from_values(ints(7))
            .transform(Count::new())
            .expect("finite input")
            .collect(),
    )
    .await;
    assert_eq!(collected.values(), [Value::Int(7)]);
}

#[tokio::test]
async fn should_count_zero_for_an_empty_stream() {
    let collected = within(
        ValueStream::from_values([])
            .transform(Count::new())
            .expect("finite input")
            .collect(),
    )
    .await;
    assert_eq!(collected.values(), [Value::Int(0)]);
}

#[tokio::test]
async fn should_skip_nulls_when_counting_and_report_how_many() {
    let collected = within(
        ValueStream::from_values([Value::Int(1), Value::Null, Value::Int(2), Value::Null])
            .transform(Count::new())
            .expect("finite input")
            .collect(),
    )
    .await;
    assert_eq!(collected.values(), [Value::Int(2)]);
    assert_eq!(
        collected.diagnostics().skipped_null(),
        2,
        "a count is never quietly taken over a different population (ADR-0014)"
    );
}

// --- `measure` ------------------------------------------------------------------------------

#[allow(
    clippy::expect_used,
    reason = "a helper shared by the `measure` tests states its precondition the way a test body does"
)]
async fn measured(values: Vec<Value>) -> Vec<Value> {
    let collected = within(
        ValueStream::from_values(values)
            .transform(Measure::new(|value: &Value| Ok(value.clone())))
            .expect("finite input")
            .collect(),
    )
    .await;
    collected.values().to_vec()
}

#[tokio::test]
async fn should_report_the_statistics_of_a_numeric_stream() {
    let values = measured(vec![
        Value::Int(2),
        Value::Int(4),
        Value::Int(4),
        Value::Int(4),
        Value::Int(5),
        Value::Int(5),
        Value::Int(7),
        Value::Int(9),
    ])
    .await;
    assert_eq!(field_of(&values, "count"), [Value::Int(8)]);
    assert_eq!(field_of(&values, "sum"), [Value::Int(40)]);
    assert_eq!(field_of(&values, "min"), [Value::Int(2)]);
    assert_eq!(field_of(&values, "max"), [Value::Int(9)]);
    assert_eq!(field_of(&values, "mean"), [Value::Float(5.0)]);
    assert_eq!(field_of(&values, "median"), [Value::Float(4.5)]);
    match field_of(&values, "stddev").first() {
        Some(Value::Float(deviation)) => assert!(
            (deviation - 2.0).abs() < 1e-9,
            "the population standard deviation of that set is 2, got {deviation}"
        ),
        other => panic!("stddev is a number, got {other:?}"),
    }
}

#[tokio::test]
async fn should_take_the_median_of_an_odd_and_an_even_count() {
    let odd = measured(vec![Value::Int(5), Value::Int(1), Value::Int(3)]).await;
    let even = measured(vec![
        Value::Int(4),
        Value::Int(1),
        Value::Int(3),
        Value::Int(2),
    ])
    .await;
    assert_eq!(field_of(&odd, "median"), [Value::Int(3)]);
    assert_eq!(
        field_of(&even, "median"),
        [Value::Float(2.5)],
        "an even count takes the midpoint of the two middle values"
    );
}

#[tokio::test]
async fn should_skip_nulls_when_measuring_and_report_how_many() {
    let values = measured(vec![
        Value::Int(10),
        Value::Null,
        Value::Int(20),
        Value::Null,
        Value::Null,
    ])
    .await;
    assert_eq!(field_of(&values, "count"), [Value::Int(2)]);
    assert_eq!(field_of(&values, "skipped"), [Value::Int(3)]);
    assert_eq!(
        field_of(&values, "mean"),
        [Value::Float(15.0)],
        "an average is never computed over a population the user did not ask for (ADR-0014)"
    );
}

#[tokio::test]
async fn should_measure_an_empty_stream_without_fabricating_numbers() {
    let values = measured(Vec::new()).await;
    assert_eq!(field_of(&values, "count"), [Value::Int(0)]);
    assert_eq!(field_of(&values, "sum"), [Value::Null]);
    assert_eq!(field_of(&values, "mean"), [Value::Null]);
    assert_eq!(field_of(&values, "median"), [Value::Null]);
    assert_eq!(field_of(&values, "min"), [Value::Null]);
    assert_eq!(field_of(&values, "max"), [Value::Null]);
}

#[tokio::test]
async fn should_measure_a_single_value() {
    let values = measured(vec![Value::Int(3)]).await;
    assert_eq!(field_of(&values, "count"), [Value::Int(1)]);
    assert_eq!(field_of(&values, "median"), [Value::Int(3)]);
    assert_eq!(field_of(&values, "stddev"), [Value::Float(0.0)]);
}

#[tokio::test]
async fn should_keep_the_unit_of_what_it_measures() {
    let values = measured(vec![
        Value::ByteSize(ByteSize::from_bytes(1024)),
        Value::ByteSize(ByteSize::from_bytes(3072)),
    ])
    .await;
    assert_eq!(
        field_of(&values, "sum"),
        [Value::ByteSize(ByteSize::from_bytes(4096))],
        "spec §53: the values remain typed"
    );
    assert_eq!(
        field_of(&values, "mean"),
        [Value::ByteSize(ByteSize::from_bytes(2048))]
    );
}

#[tokio::test]
async fn should_report_the_requested_percentiles() {
    let collected = within(
        ValueStream::from_values((1..=100).map(Value::Int).collect::<Vec<_>>())
            .transform(
                Measure::new(|value: &Value| Ok(value.clone())).with_percentiles([50.0, 95.0]),
            )
            .expect("finite input")
            .collect(),
    )
    .await;
    let percentiles = field_of(collected.values(), "percentiles");
    match &percentiles[0] {
        Value::Map(map) => {
            assert_eq!(map.get("p50"), Some(&Value::Int(50)));
            assert_eq!(map.get("p95"), Some(&Value::Int(95)));
        }
        other => panic!("percentiles are a map of label to typed value, got {other}"),
    }
}

#[tokio::test]
async fn should_report_a_value_that_cannot_be_measured_and_keep_the_rest() {
    let collected = within(
        ValueStream::from_values([Value::Int(1), Value::string("nope"), Value::Int(3)])
            .transform(Measure::new(|value: &Value| Ok(value.clone())))
            .expect("finite input")
            .collect(),
    )
    .await;
    assert_eq!(field_of(collected.values(), "count"), [Value::Int(2)]);
    assert_eq!(collected.errors().len(), 1);
    assert_eq!(collected.errors()[0].code(), ErrorCode::TypeMismatch);
}

// --- `reduce` -------------------------------------------------------------------------------

#[tokio::test]
async fn should_fold_a_stream_into_one_value() {
    let collected = within(
        ValueStream::from_values(ints(5))
            .transform(Reduce::new(|acc: &Value, value: &Value| acc.add(value)))
            .expect("finite input")
            .collect(),
    )
    .await;
    assert_eq!(collected.values(), [Value::Int(10)]);
}

#[tokio::test]
async fn should_seed_the_fold_with_an_explicit_initial_value() {
    let collected = within(
        ValueStream::from_values(ints(3))
            .transform(
                Reduce::new(|acc: &Value, value: &Value| acc.add(value))
                    .with_initial(Value::Int(100)),
            )
            .expect("finite input")
            .collect(),
    )
    .await;
    assert_eq!(collected.values(), [Value::Int(103)]);
}

#[tokio::test]
async fn should_yield_the_initial_value_when_folding_an_empty_stream() {
    let collected = within(
        ValueStream::from_values([])
            .transform(
                Reduce::new(|acc: &Value, value: &Value| acc.add(value))
                    .with_initial(Value::Int(0)),
            )
            .expect("finite input")
            .collect(),
    )
    .await;
    assert_eq!(collected.values(), [Value::Int(0)]);
}

#[tokio::test]
async fn should_report_an_empty_fold_without_an_initial_value() {
    let collected = within(
        ValueStream::from_values([])
            .transform(Reduce::new(|acc: &Value, value: &Value| acc.add(value)))
            .expect("finite input")
            .collect(),
    )
    .await;
    assert!(collected.values().is_empty());
    assert_eq!(collected.errors().len(), 1);
}

#[tokio::test]
async fn should_fold_a_single_element_to_itself() {
    let collected = within(
        ValueStream::from_values([Value::Int(9)])
            .transform(Reduce::new(|acc: &Value, value: &Value| acc.add(value)))
            .expect("finite input")
            .collect(),
    )
    .await;
    assert_eq!(collected.values(), [Value::Int(9)]);
}

// --- `join` ---------------------------------------------------------------------------------

#[tokio::test]
async fn should_join_matching_records_on_the_key() {
    let right = vec![demo(1, "right-one", None), demo(2, "right-two", None)];
    let collected = within(
        ValueStream::from_values([demo(1, "left-one", None), demo(2, "left-two", None)])
            .transform(Join::new(right, pid))
            .expect("finite input")
            .collect(),
    )
    .await;
    assert_eq!(
        field_of(collected.values(), "key"),
        [Value::Int(1), Value::Int(2)]
    );
    assert_eq!(collected.values().len(), 2);
}

#[tokio::test]
async fn should_drop_an_unmatched_row_in_an_inner_join() {
    let right = vec![demo(1, "right", None)];
    let collected = within(
        ValueStream::from_values([demo(1, "left", None), demo(9, "lonely", None)])
            .transform(Join::new(right, pid))
            .expect("finite input")
            .collect(),
    )
    .await;
    assert_eq!(field_of(collected.values(), "key"), [Value::Int(1)]);
}

#[tokio::test]
async fn should_keep_an_unmatched_left_row_in_a_left_join() {
    let right = vec![demo(1, "right", None)];
    let collected = within(
        ValueStream::from_values([demo(1, "left", None), demo(9, "lonely", None)])
            .transform(Join::new(right, pid).with_kind(JoinKind::Left))
            .expect("finite input")
            .collect(),
    )
    .await;
    assert_eq!(
        field_of(collected.values(), "key"),
        [Value::Int(1), Value::Int(9)]
    );
    assert_eq!(
        field_of(collected.values(), "right")[1],
        Value::Null,
        "an unmatched left row has no right side, and that is null, not a fabrication"
    );
}

#[tokio::test]
async fn should_keep_an_unmatched_right_row_in_a_right_join() {
    let right = vec![demo(1, "right", None), demo(5, "orphan", None)];
    let collected = within(
        ValueStream::from_values([demo(1, "left", None)])
            .transform(Join::new(right, pid).with_kind(JoinKind::Right))
            .expect("finite input")
            .collect(),
    )
    .await;
    assert_eq!(
        field_of(collected.values(), "key"),
        [Value::Int(1), Value::Int(5)]
    );
    assert_eq!(field_of(collected.values(), "left")[1], Value::Null);
}

#[tokio::test]
async fn should_keep_both_unmatched_sides_in_an_outer_join() {
    let right = vec![demo(5, "orphan", None)];
    let collected = within(
        ValueStream::from_values([demo(1, "lonely", None)])
            .transform(Join::new(right, pid).with_kind(JoinKind::Outer))
            .expect("finite input")
            .collect(),
    )
    .await;
    assert_eq!(
        field_of(collected.values(), "key"),
        [Value::Int(1), Value::Int(5)]
    );
}

#[tokio::test]
async fn should_pair_every_combination_when_a_key_repeats() {
    let right = vec![demo(1, "r-a", None), demo(1, "r-b", None)];
    let collected = within(
        ValueStream::from_values([demo(1, "l-a", None), demo(1, "l-b", None)])
            .transform(Join::new(right, pid))
            .expect("finite input")
            .collect(),
    )
    .await;
    assert_eq!(
        collected.values().len(),
        4,
        "two left rows and two right rows on one key make four pairs"
    );
}

#[tokio::test]
async fn should_never_match_an_unknown_key() {
    let right = vec![demo(1, "right", None)];
    let collected = within(
        ValueStream::from_values([demo(1, "left", None), demo(2, "left", None)])
            .transform(Join::new(right, cpu).with_kind(JoinKind::Left))
            .expect("finite input")
            .collect(),
    )
    .await;
    assert_eq!(collected.values().len(), 2);
    assert!(
        field_of(collected.values(), "right")
            .iter()
            .all(Value::is_null),
        "an unknown key joins to nothing, the way SQL treats null"
    );
}

#[tokio::test]
async fn should_join_an_empty_stream_to_nothing() {
    let collected = within(
        ValueStream::from_values([])
            .transform(Join::new([demo(1, "right", None)], pid))
            .expect("finite input")
            .collect(),
    )
    .await;
    assert!(collected.values().is_empty());
}

// --- `diff` ---------------------------------------------------------------------------------

#[tokio::test]
async fn should_report_added_removed_and_changed_objects() {
    let previous = vec![
        demo(1, "kept", Some(1.0)),
        demo(2, "changed", Some(1.0)),
        demo(3, "removed", Some(1.0)),
    ];
    let collected = within(
        ValueStream::from_values([
            demo(1, "kept", Some(1.0)),
            demo(2, "changed", Some(2.0)),
            demo(4, "added", Some(1.0)),
        ])
        .transform(Diff::new(previous, pid))
        .expect("finite input")
        .collect(),
    )
    .await;

    assert_eq!(
        field_of(collected.values(), "change"),
        [
            Value::string("changed"),
            Value::string("added"),
            Value::string("removed")
        ],
        "an unchanged object is not a change"
    );
    assert_eq!(
        field_of(collected.values(), "key"),
        [Value::Int(2), Value::Int(4), Value::Int(3)]
    );
}

#[tokio::test]
async fn should_report_no_changes_between_identical_snapshots() {
    let snapshot = vec![demo(1, "same", Some(1.0))];
    let collected = within(
        ValueStream::from_values(snapshot.clone())
            .transform(Diff::new(snapshot, pid))
            .expect("finite input")
            .collect(),
    )
    .await;
    assert!(collected.values().is_empty());
}

#[tokio::test]
async fn should_report_everything_as_added_against_an_empty_snapshot() {
    let collected = within(
        ValueStream::from_values([demo(1, "new", None)])
            .transform(Diff::new(Vec::new(), pid))
            .expect("finite input")
            .collect(),
    )
    .await;
    assert_eq!(
        field_of(collected.values(), "change"),
        [Value::string("added")]
    );
}

#[tokio::test]
async fn should_report_everything_as_removed_when_the_stream_is_empty() {
    let collected = within(
        ValueStream::from_values([])
            .transform(Diff::new(vec![demo(1, "gone", None)], pid))
            .expect("finite input")
            .collect(),
    )
    .await;
    assert_eq!(
        field_of(collected.values(), "change"),
        [Value::string("removed")]
    );
}
