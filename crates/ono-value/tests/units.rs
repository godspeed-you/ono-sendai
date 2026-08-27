//! Observable behaviour of the semantic scalars and their arithmetic (spec §10.6, §13.4).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed precondition in a test should abort the test loudly"
)]

use std::cmp::Ordering;

use ono_core::ErrorCode;
use ono_value::{ByteSize, ByteUnit, Duration, DurationUnit, Percent, Value};

#[test]
fn should_convert_units_when_comparing_two_byte_sizes_written_differently() {
    let half_gibibyte = Value::ByteSize(ByteSize::parse("512MiB").unwrap());
    let gibibyte = Value::ByteSize(ByteSize::parse("1GiB").unwrap());

    assert_eq!(
        half_gibibyte.compare_to(&gibibyte).unwrap(),
        Ordering::Less,
        "512MiB must compare as less than 1GiB, not as an error"
    );
}

#[test]
fn should_convert_units_when_comparing_decimal_and_binary_byte_units() {
    let kilobyte = ByteSize::parse("1KB").unwrap();
    let kibibyte = ByteSize::parse("1KiB").unwrap();

    assert_eq!(kilobyte.bytes(), 1000, "KB is a power of ten");
    assert_eq!(kibibyte.bytes(), 1024, "KiB is a power of two");
    assert!(kilobyte < kibibyte, "1KB must be smaller than 1KiB");
}

#[test]
fn should_reject_the_comparison_when_the_dimensions_differ() {
    let duration = Value::Duration(Duration::parse("10s").unwrap());
    let size = Value::ByteSize(ByteSize::parse("512MiB").unwrap());

    let error = duration
        .compare_to(&size)
        .expect_err("comparing a duration with a byte size must not succeed");

    assert_eq!(
        error.code(),
        ErrorCode::TypeInvalidUnit,
        "an incompatible dimension is a unit error, got {error}"
    );
}

#[test]
fn should_reject_the_addition_when_the_dimensions_differ() {
    let duration = Value::Duration(Duration::parse("10s").unwrap());
    let size = Value::ByteSize(ByteSize::parse("512MiB").unwrap());

    let error = duration
        .add(&size)
        .expect_err("adding a byte size to a duration must not produce a number");

    assert_eq!(
        error.code(),
        ErrorCode::TypeInvalidUnit,
        "adding incompatible dimensions is a unit error, got {error}"
    );
}

#[test]
fn should_add_two_values_of_the_same_dimension() {
    let sum = Value::ByteSize(ByteSize::parse("512MiB").unwrap())
        .add(&Value::ByteSize(ByteSize::parse("512MiB").unwrap()))
        .unwrap();

    assert_eq!(sum, Value::ByteSize(ByteSize::parse("1GiB").unwrap()));
}

#[test]
fn should_yield_a_duration_when_subtracting_two_timestamps() {
    let earlier = Value::Timestamp("2026-08-26T08:00:00Z".parse().unwrap());
    let later = Value::Timestamp("2026-08-26T08:00:10Z".parse().unwrap());

    assert_eq!(
        later.sub(&earlier).unwrap(),
        Value::Duration(Duration::parse("10s").unwrap())
    );
}

#[test]
fn should_yield_a_timestamp_when_adding_a_duration_to_a_timestamp() {
    let start = Value::Timestamp("2026-08-26T08:00:00Z".parse().unwrap());
    let shifted = start
        .add(&Value::Duration(Duration::parse("90m").unwrap()))
        .unwrap();

    assert_eq!(
        shifted,
        Value::Timestamp("2026-08-26T09:30:00Z".parse().unwrap())
    );
}

#[test]
fn should_scale_a_byte_size_when_multiplied_by_a_number() {
    let doubled = Value::ByteSize(ByteSize::parse("2MiB").unwrap())
        .mul(&Value::Int(3))
        .unwrap();

    assert_eq!(doubled, Value::ByteSize(ByteSize::parse("6MiB").unwrap()));
}

#[test]
fn should_reject_multiplying_two_byte_sizes() {
    let error = Value::ByteSize(ByteSize::from_bytes(2))
        .mul(&Value::ByteSize(ByteSize::from_bytes(3)))
        .expect_err("bytes times bytes has no dimension this shell models");

    assert_eq!(error.code(), ErrorCode::TypeInvalidUnit);
}

#[test]
fn should_yield_a_ratio_when_dividing_two_byte_sizes() {
    let ratio = Value::ByteSize(ByteSize::parse("1GiB").unwrap())
        .div(&Value::ByteSize(ByteSize::parse("512MiB").unwrap()))
        .unwrap();

    assert_eq!(ratio, Value::Float(2.0));
}

#[test]
fn should_render_a_byte_size_the_way_the_spec_shows_it() {
    assert_eq!(ByteSize::from_bytes(1_288_490_188).to_string(), "1.20 GiB");
    assert_eq!(ByteSize::from_bytes(128).to_string(), "128 B");
}

#[test]
fn should_render_a_duration_the_way_the_spec_shows_it() {
    assert_eq!(Duration::parse("4d 3h").unwrap().to_string(), "4d 03h");
    assert_eq!(Duration::parse("843ms").unwrap().to_string(), "843ms");
}

#[test]
fn should_render_a_negative_duration_with_a_leading_sign() {
    assert_eq!(Duration::parse("-843ms").unwrap().to_string(), "-843ms");
}

#[test]
fn should_parse_a_compound_duration_as_the_sum_of_its_terms() {
    assert_eq!(
        Duration::parse("1h30m").unwrap(),
        Duration::parse("90m").unwrap()
    );
}

#[test]
fn should_parse_a_fractional_unit_literal() {
    assert_eq!(
        ByteSize::parse("3.5GiB").unwrap().bytes(),
        3_758_096_384,
        "3.5GiB must be exactly 3.5 * 2^30 bytes"
    );
}

#[test]
fn should_reject_an_unknown_unit_suffix() {
    let error = ByteSize::parse("10furlongs").expect_err("`furlongs` is not a byte unit");
    assert_eq!(error.code(), ErrorCode::TypeInvalidUnit);

    let error = Duration::parse("10furlongs").expect_err("`furlongs` is not a duration unit");
    assert_eq!(error.code(), ErrorCode::TypeInvalidUnit);
}

#[test]
fn should_reject_a_negative_byte_size() {
    let error = ByteSize::parse("-1KiB").expect_err("a byte size is never negative");
    assert_eq!(error.code(), ErrorCode::TypeInvalidUnit);
}

#[test]
fn should_reject_a_subtraction_that_would_make_a_byte_size_negative() {
    let error = Value::ByteSize(ByteSize::from_bytes(1))
        .sub(&Value::ByteSize(ByteSize::from_bytes(2)))
        .expect_err("a byte size cannot go below zero");

    assert_eq!(error.code(), ErrorCode::TypeInvalidUnit);
}

#[test]
fn should_expose_every_byte_unit_the_spec_lists() {
    let suffixes: Vec<&str> = ByteUnit::ALL.iter().map(|unit| unit.suffix()).collect();

    assert_eq!(
        suffixes,
        vec![
            "B", "KiB", "MiB", "GiB", "TiB", "PiB", "KB", "MB", "GB", "TB", "PB"
        ]
    );
}

#[test]
fn should_expose_every_duration_unit_the_spec_lists() {
    let suffixes: Vec<&str> = DurationUnit::ALL.iter().map(|unit| unit.suffix()).collect();

    assert_eq!(suffixes, vec!["ns", "us", "ms", "s", "m", "h", "d", "w"]);
}

#[test]
fn should_compare_percentages_and_reject_a_percentage_against_a_duration() {
    let low = Value::Percent(Percent::new(20.0));
    let high = Value::Percent(Percent::new(24.8));

    assert_eq!(low.compare_to(&high).unwrap(), Ordering::Less);

    let error = low
        .compare_to(&Value::Duration(Duration::parse("1s").unwrap()))
        .expect_err("a percentage is not a duration");
    assert_eq!(error.code(), ErrorCode::TypeInvalidUnit);
}

#[test]
fn should_compare_an_integer_against_a_float_numerically() {
    assert_eq!(
        Value::Int(2).compare_to(&Value::Float(2.5)).unwrap(),
        Ordering::Less
    );
    assert_eq!(
        Value::Float(2.0).compare_to(&Value::Int(2)).unwrap(),
        Ordering::Equal
    );
}

#[test]
fn should_reject_arithmetic_between_types_that_have_none() {
    let error = Value::Bool(true)
        .add(&Value::Int(1))
        .expect_err("a boolean is not a number");

    assert_eq!(error.code(), ErrorCode::TypeMismatch);
}
