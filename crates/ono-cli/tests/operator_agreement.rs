//! The two places an expression is evaluated answer every operator the same way (issue #137).
//!
//! An expression runs either inside a native pipeline stage — `where`, `each (…)`, `sort`, where
//! the command layer evaluates it against the record in hand (ADR-0005) — or in the session
//! itself — `let x = (…)`, `if`, `while` — where the evaluator also reaches its own variables
//! and runs `$(…)` and parenthesised pipelines. The language has one meaning for `+`, `%`, `in`,
//! `~=`, `and`, `x == null` and the rest (spec §10.5, ADR-0014), so the two sites must agree on
//! every one of them, including on which operand combinations are refused.
//!
//! Each case is evaluated once through `each (…)` in a pipeline and once through `let` in the
//! session; the condition cases once through `where` and once through `if`. The outcome compared
//! is what reaches the command line: the value `to json` printed, or the error code refused with.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_testkit::ono;

/// Every binary operator, the Kleene connectives, the null identity test, the unit literals the
/// operators combine, indexing, and the operand combinations each operator refuses.
const CASES: &[&str] = &[
    // Arithmetic, over every numeric shape and the quantities of spec §10.6.
    "1 + 2",
    "1.5 + 1",
    "7 - 10",
    "6 * 7",
    "7 / 2",
    "7.0 / 2",
    "7 % 2",
    "-7 % 2",
    "5 % 0",
    "7.5 % 2",
    "1KiB + 1KiB",
    "1.5MB",
    "2s * 3",
    "1h - 30m",
    "1.5ms",
    "50% + 1%",
    "1KiB + 1s",
    "\"a\" + 1",
    "\"a\" + \"b\"",
    "1 + null",
    // Comparison, including the cross-numeric and the refused ones.
    "1 == 1.0",
    "1 != 2",
    "\"a\" < \"b\"",
    "3 >= 3",
    "2 <= 1",
    "2 > 1",
    "1KiB < 1MiB",
    "2000-01-01T00:00:00Z < 2001-01-01T00:00:00Z",
    "10.0.0.1 == 10.0.0.1",
    "1 < \"a\"",
    "1 > null",
    // `x == null` is an identity test; everything else with an unknown is unknown (ADR-0014).
    "null == null",
    "1 == null",
    "null != 1",
    "null != null",
    "null < 1",
    // Kleene connectives, with the short circuits that keep a failing operand unevaluated.
    "true and false",
    "null and false",
    "null and true",
    "true or null",
    "null or false",
    "false and (5 % 0)",
    "true or (5 % 0)",
    "true and (5 % 0)",
    "1 and true",
    // Membership.
    "2 in [1, 2]",
    "3 not in [1, 2]",
    "\"ab\" in \"xaby\"",
    "1 in \"x1y\"",
    "1 in 5",
    "null in [1]",
    // Regex matching, with flags, and a right side that is not a pattern.
    "\"abc\" ~= /b/",
    "\"abc\" !~= /B/",
    "\"abc\" ~= /B/i",
    "5 ~= /5/",
    "\"a\" ~= \"a\"",
    // Indexing.
    "[1, 2][1]",
    "[1, 2][5]",
    "{a: 1}[\"a\"]",
    "{a: 1}[\"b\"]",
    "[1][-1]",
    "1[0]",
];

/// Conditions, whose truth decides a `where` and an `if` alike (only `true` admits, ADR-0014).
const CONDITIONS: &[&str] = &[
    "1 < 2",
    "2 < 1",
    "null == null",
    "null > 1",
    "null and true",
    "null or true",
    "not null",
    "\"abc\" ~= /c$/",
    "3 in [1, 2]",
];

/// What the command line saw: the error code a run was refused with, or what `to json` printed.
fn outcome(run: &ono_testkit::Run) -> String {
    match run.stderr().find("Ono-Sendai-E") {
        Some(at) => run.stderr()[at..]
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned(),
        None => run.stdout().trim().to_owned(),
    }
}

#[test]
fn should_answer_every_operator_alike_when_evaluated_in_a_pipeline_stage_and_in_the_session() {
    let mut disagreements = Vec::new();
    for case in CASES {
        let in_stage = ono(&format!("let one = [1]; $one | each ({case}) | to json"));
        let in_session = ono(&format!("let r = ({case}); $r | to json"));
        let (stage, session) = (outcome(&in_stage), outcome(&in_session));
        assert!(
            !stage.is_empty() && !stage.starts_with("Ono-Sendai-E0001"),
            "`{case}` is an expression the language reads; got {:?}",
            in_stage.output()
        );
        if stage != session {
            disagreements.push(format!(
                "`{case}`: pipeline stage {stage:?}, session {session:?}"
            ));
        }
    }
    assert!(
        disagreements.is_empty(),
        "spec §10.5, ADR-0014: an operator means one thing wherever the expression runs; \
         these differ:\n{}",
        disagreements.join("\n")
    );
}

#[test]
fn should_admit_the_same_conditions_when_where_filters_and_when_if_branches() {
    let mut disagreements = Vec::new();
    for condition in CONDITIONS {
        let filtered = ono(&format!(
            "let one = [1]; $one | where ({condition}) | count | to json"
        ));
        let branched = ono(&format!("if ({condition}) {{ echo 1 }} else {{ echo 0 }}"));
        // `count | to json` prints the one count as a one-element array.
        let admitted = outcome(&filtered)
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_owned();
        let taken = outcome(&branched);
        assert!(
            admitted == "0" || admitted == "1",
            "`where ({condition})` over one value keeps it or not; got {:?}",
            filtered.output()
        );
        if admitted != taken {
            disagreements.push(format!(
                "`{condition}`: where kept {admitted}, if took the {} branch",
                if taken == "1" { "then" } else { "else" }
            ));
        }
    }
    assert!(
        disagreements.is_empty(),
        "ADR-0014: only `true` admits, in a `where` and an `if` alike; these differ:\n{}",
        disagreements.join("\n")
    );
}
