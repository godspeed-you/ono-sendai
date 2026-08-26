//! The transforms of spec §53, each driven the way a user writes it.
//!
//! The fixture provider holds three objects — one below a kibibyte, one above it, and one whose
//! size is unknown — so every test here also says what the transform does with the unknown.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod fixture;

use fixture::{FixtureProvider, providers, run};
use ono_core::ErrorCode;
use ono_value::{ByteSize, Value};

/// The `name` field of every record that came out, in order.
fn names(ran: &fixture::Ran) -> Vec<String> {
    ran.values()
        .iter()
        .map(|value| {
            value
                .follow(&[ono_value::FieldStep::required("name")])
                .expect("every record has a name")
                .as_str()
                .expect("a name is text")
                .to_owned()
        })
        .collect()
}

// --- where -----------------------------------------------------------------------------------

#[tokio::test]
async fn should_keep_only_the_rows_a_predicate_decided_are_true() {
    let ran = run(
        "get process | where size > 1KiB",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    assert_eq!(
        names(&ran),
        ["beta"],
        "ADR-0014: the row whose size is unknown is not reported as being over the threshold"
    );
}

#[tokio::test]
async fn should_report_exactly_the_rows_whose_value_is_unknown_when_asked_for_them() {
    let ran = run(
        "get process | where size == null",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    assert_eq!(names(&ran), ["gamma"]);
}

#[tokio::test]
async fn should_keep_nothing_from_an_empty_input() {
    let ran = run(
        "get process 99 | where size > 1KiB",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    assert!(ran.values().is_empty());
}

// --- select ----------------------------------------------------------------------------------

#[tokio::test]
async fn should_project_the_named_fields_in_the_order_they_were_written() {
    let ran = run(
        "get process | select name size",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    let first = ran.values()[0].as_record().expect("a record");
    assert_eq!(
        first
            .schema()
            .fields()
            .iter()
            .map(ono_value::FieldDef::name)
            .collect::<Vec<_>>(),
        ["name", "size"]
    );
    assert_eq!(first.get("name"), Some(&Value::string("alpha")));
}

#[tokio::test]
async fn should_project_a_computed_field_under_the_name_it_was_given() {
    let ran = run(
        "get process | take 1 | select name {kib: size / 1KiB}",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    let record = ran.only().as_record().expect("a record");
    assert_eq!(record.get("kib"), Some(&Value::Float(0.5)));
}

#[tokio::test]
async fn should_project_a_failed_read_as_the_error_it_is() {
    let ran = run(
        "get process | select nowhere",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    let record = ran.values()[0].as_record().expect("a record");
    assert!(
        record.get("nowhere").is_some_and(Value::is_error),
        "spec §10.5: `could not be read` stays in the field rather than becoming null"
    );
}

// --- sort ------------------------------------------------------------------------------------

#[tokio::test]
async fn should_order_ascending_by_default_and_put_the_unknown_last() {
    let ran = run(
        "get process | sort size",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    assert_eq!(
        names(&ran),
        ["alpha", "beta", "gamma"],
        "ADR-0014: an unknown is never mistaken for the smallest value"
    );
}

#[tokio::test]
async fn should_order_descending_when_asked() {
    let ran = run(
        "get process | sort size desc",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    assert_eq!(names(&ran), ["gamma", "beta", "alpha"]);
}

#[tokio::test]
async fn should_refuse_a_direction_that_is_not_one() {
    let error = run(
        "get process | sort size sideways",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect_err("`sideways` is not a direction");

    assert_eq!(error.code(), ErrorCode::TypeMismatch);
}

#[tokio::test]
async fn should_leave_a_single_row_alone() {
    let ran = run(
        "get process 2 | sort size desc",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    assert_eq!(names(&ran), ["beta"]);
}

// --- group -----------------------------------------------------------------------------------

#[tokio::test]
async fn should_group_into_records_rather_than_headings() {
    let ran = run(
        "get process | group owner",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    let groups: Vec<(Value, Value)> = ran
        .values()
        .iter()
        .map(|value| {
            let record = value.as_record().expect("a group is a record (spec §53)");
            (
                record.get("key").cloned().unwrap_or(Value::Null),
                record.get("count").cloned().unwrap_or(Value::Null),
            )
        })
        .collect();

    assert_eq!(
        groups,
        [
            (Value::string("root"), Value::Int(2)),
            (Value::string("ono"), Value::Int(1)),
        ],
        "groups come out in the order their key was first seen"
    );
}

// --- take and skip ----------------------------------------------------------------------------

#[tokio::test]
async fn should_take_the_first_rows_and_no_more() {
    let ran = run("get process | take 2", &providers(FixtureProvider::new()))
        .await
        .expect("the pipeline runs");

    assert_eq!(names(&ran), ["alpha", "beta"]);
}

#[tokio::test]
async fn should_skip_the_first_rows_and_keep_the_rest() {
    let ran = run("get process | skip 2", &providers(FixtureProvider::new()))
        .await
        .expect("the pipeline runs");

    assert_eq!(names(&ran), ["gamma"]);
}

#[tokio::test]
async fn should_bound_an_endless_stream_so_a_blocking_transform_can_run_over_it() {
    let ran = run(
        "get process | take 4 | count",
        &providers(FixtureProvider::new().endless()),
    )
    .await
    .expect("`take` bounds the stream, which is what makes `count` legal");

    assert_eq!(ran.only(), &Value::Int(4));
}

// --- each ------------------------------------------------------------------------------------

#[tokio::test]
async fn should_map_every_value_through_the_body() {
    let ran = run(
        "get process | each name",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    assert_eq!(
        ran.values(),
        [
            Value::string("alpha"),
            Value::string("beta"),
            Value::string("gamma")
        ]
    );
}

// --- reduce ----------------------------------------------------------------------------------

#[tokio::test]
async fn should_fold_a_stream_into_one_value() {
    let ran = run(
        "get process | each size | take 2 | reduce $acc + @",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    assert_eq!(ran.only(), &Value::ByteSize(ByteSize::from_bytes(2560)));
}

#[tokio::test]
async fn should_report_an_empty_fold_rather_than_answering_with_a_zero() {
    let error = run(
        "get process 99 | reduce $acc + @",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs")
    .failures()
    .first()
    .cloned()
    .expect("an empty fold with no seed has no answer");

    assert_eq!(error.code(), ErrorCode::TypeMismatch);
}

#[tokio::test]
async fn should_seed_the_fold_with_the_written_initial_value() {
    // `--initial` between expressions is an option, not a double unary minus (ADR-0032), and
    // its expression seeds the accumulator before anything flows.
    let ran = run(
        "get process | each size | take 2 | reduce $acc + @ --initial 1B",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    assert_eq!(ran.only(), &Value::ByteSize(ByteSize::from_bytes(2561)));
}

#[tokio::test]
async fn should_answer_the_initial_value_for_an_empty_stream_when_one_is_given() {
    let ran = run(
        "get process 99 | reduce $acc + @ --initial 0",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    assert_eq!(
        ran.only(),
        &Value::Int(0),
        "a seeded fold over nothing is its seed, not an error"
    );
}

// --- count -----------------------------------------------------------------------------------

#[tokio::test]
async fn should_count_the_values_of_a_finite_stream() {
    let ran = run("get process | count", &providers(FixtureProvider::new()))
        .await
        .expect("the pipeline runs");

    assert_eq!(ran.only(), &Value::Int(3));
}

#[tokio::test]
async fn should_count_an_empty_stream_as_zero() {
    let ran = run("get process 99 | count", &providers(FixtureProvider::new()))
        .await
        .expect("the pipeline runs");

    assert_eq!(ran.only(), &Value::Int(0));
}

// --- measure ---------------------------------------------------------------------------------

#[tokio::test]
async fn should_measure_a_numeric_field_and_say_how_many_it_skipped() {
    let ran = run(
        "get process | measure size",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    let record = ran.only().as_record().expect("a measurement is a record");
    assert_eq!(record.get("count"), Some(&Value::Int(2)));
    assert_eq!(
        record.get("skipped"),
        Some(&Value::Int(1)),
        "ADR-0014: an average is never quietly computed over a different population"
    );
    assert_eq!(
        record.get("sum"),
        Some(&Value::ByteSize(ByteSize::from_bytes(2560))),
        "spec §53: the values stay typed"
    );
    assert_eq!(
        record.get("max"),
        Some(&Value::ByteSize(ByteSize::from_bytes(2048)))
    );
}

#[tokio::test]
async fn should_measure_a_single_value_without_inventing_a_spread() {
    let ran = run(
        "get process 2 | measure size",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    let record = ran.only().as_record().expect("a record");
    assert_eq!(record.get("count"), Some(&Value::Int(1)));
    assert_eq!(record.get("min"), record.get("max"));
}

#[tokio::test]
async fn should_answer_an_empty_measurement_with_nulls_rather_than_zeros() {
    let ran = run(
        "get process 99 | measure size",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    let record = ran.only().as_record().expect("a record");
    assert_eq!(record.get("count"), Some(&Value::Int(0)));
    assert_eq!(
        record.get("mean"),
        Some(&Value::Null),
        "spec §35.3: unknown data is null, never a fabricated zero"
    );
}

// --- boundedness ------------------------------------------------------------------------------

#[tokio::test]
async fn should_refuse_a_blocking_transform_over_a_stream_that_may_never_end() {
    let error = run(
        "get process | sort size",
        &providers(FixtureProvider::new().endless()),
    )
    .await
    .expect_err("`sort` needs input that ends (spec §11.1)");

    assert_eq!(error.code(), ErrorCode::StreamUnboundedOperation);
}

#[tokio::test]
async fn should_let_a_streaming_transform_run_over_a_stream_that_may_never_end() {
    let ran = run(
        "get process | where size > 1KiB | take 2",
        &providers(FixtureProvider::new().endless()),
    )
    .await
    .expect("`where` and `take` are streaming (spec §11.1)");

    assert_eq!(names(&ran), ["beta", "beta"]);
}

// --- a transform with nothing in front of it ---------------------------------------------------

#[tokio::test]
async fn should_refuse_a_transform_with_nothing_piped_into_it() {
    let error = run("where size > 1KiB", &providers(FixtureProvider::new()))
        .await
        .expect_err("a transform transforms something");

    assert_eq!(error.code(), ErrorCode::TypeMismatch);
}
