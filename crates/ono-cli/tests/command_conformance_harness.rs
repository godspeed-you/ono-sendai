//! The command conformance harness refuses drift (issue #149, ADR-0935).
//!
//! `command_conformance.rs` is generated, and a generated suite whose harness accepted anything
//! would be green forever. These cases seed the drift the issue describes — a command whose values
//! are not the schema it declares, or not as many as it declares — against a real command and
//! require the harness to fail; the last one requires it to pass the honest declaration.

#![cfg(feature = "full")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md §16)"
)]

mod conformance_harness;

use conformance_harness::{ExampleCase, assert_example_conforms};

/// Whether the harness fails `case`.
fn refuses(case: ExampleCase) -> bool {
    std::panic::catch_unwind(|| assert_example_conforms(&case)).is_err()
}

#[test]
fn should_fail_an_example_whose_records_are_of_another_schema_than_declared() {
    // `timeline` answered with an `ono.temporal-timeline/1` record while it declared
    // `stream<ono.temporal-event/1>`: the drift this suite exists for.
    assert!(refuses(ExampleCase {
        command: "ono.user.get",
        example: "get user",
        output: "stream<ono.group/1>",
    }));
}

#[test]
fn should_fail_an_example_that_produces_a_stream_where_one_value_is_declared() {
    assert!(refuses(ExampleCase {
        command: "ono.user.get",
        example: "get user",
        output: "ono.user/1",
    }));
}

#[test]
fn should_fail_an_example_whose_scalar_is_not_the_declared_type() {
    assert!(refuses(ExampleCase {
        command: "ono.data.count",
        example: "get user | count",
        output: "string",
    }));
}

#[test]
fn should_pass_an_example_that_produces_what_it_declares() {
    assert!(!refuses(ExampleCase {
        command: "ono.user.get",
        example: "get user",
        output: "stream<ono.user/1>",
    }));
}
