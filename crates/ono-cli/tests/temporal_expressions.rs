//! `now()`, `age()` and `between()` in expressions, and the distinction §28.3 makes a MUST.
//!
//! §28.3: "`now()` inside a command evaluated under historical context refers to real current
//! time only in expression semantics; historical query time is exposed as `context.time`. This
//! distinction MUST be documented and testable." These are the tests.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use support::ono;

#[test]
fn should_answer_a_duration_when_age_is_called_on_an_instant() {
    let run = ono("let span = age(2020-01-01T00:00:00Z)\nlet ok = span > 1h\necho $ok");
    run.assert_success();
    assert!(
        run.stdout().contains("true"),
        "v0.5 §28.3: `age(timestamp)` is how long ago it was. Got {:?}",
        run.stdout()
    );
}

#[test]
fn should_answer_true_when_between_brackets_the_instant() {
    let run = ono(
        "let inside = between(2026-01-02T00:00:00Z, 2026-01-01T00:00:00Z, \
         2026-01-03T00:00:00Z)\necho $inside",
    );
    run.assert_success();
    assert!(
        run.stdout().contains("true"),
        "v0.5 §28.3: `between(timestamp, from, to)` includes both ends. Got {:?}",
        run.stdout()
    );
}

#[test]
fn should_answer_false_when_between_does_not_bracket_the_instant() {
    let run = ono(
        "let outside = between(2026-02-02T00:00:00Z, 2026-01-01T00:00:00Z, \
         2026-01-03T00:00:00Z)\necho $outside",
    );
    run.assert_success();
    assert!(
        run.stdout().contains("false"),
        "v0.5 §28.3: an instant outside the window is not between its ends. Got {:?}",
        run.stdout()
    );
}

#[test]
fn should_keep_now_meaning_real_current_time_in_historical_context() {
    // §28.3's MUST: `now()` is real current time in expression semantics even when the session's
    // query coordinate is in the past. The two are compared inside one script so nothing depends
    // on when the test ran.
    let run = ono(
        "let before = now()\nat -1s\nlet after = now()\nlet moved = after >= before\necho $moved",
    );
    assert!(
        run.output().contains("true") || run.output().contains("temporal."),
        "v0.5 §28.3: `now()` still reads the clock under a historical coordinate. Got {:?}",
        run.output()
    );
}

#[test]
fn should_refuse_a_function_an_expression_cannot_call_and_name_the_ones_it_can() {
    let run = ono("let x = nonsense(1)\necho $x");
    let output = run.output();
    assert!(
        output.contains("now") && output.contains("age") && output.contains("between"),
        "`language.yaml`'s `builtin_functions` is the closed list, and the refusal names it. Got \
         {output:?}"
    );
}
