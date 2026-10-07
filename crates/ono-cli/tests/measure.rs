//! `measure` holds nothing for the statistics that need nothing held (issue #174, ADR-0953).
//!
//! `count`, `sum`, `mean`, `min`, `max` and `stddev` fold into state of a fixed size; only the
//! median and the percentiles are defined over the whole distribution. So the default `measure` is
//! an incremental aggregate — no materialization limit applies to it, and an unbounded source is a
//! legal input it answers after every value — and `--median` or `--percentiles` is what makes an
//! invocation hold its input, within the limits, over input that ends.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::time::Duration;

use ono_testkit::scratch;

use support::run_bounded;

const BUDGET: Duration = Duration::from_secs(60);

/// `[1,2,…,n]` as one JSON document.
fn numbers(n: u32) -> String {
    let items: Vec<String> = (1..=n).map(|number| number.to_string()).collect();
    format!("[{}]", items.join(","))
}

/// The one JSON document a run wrote.
fn document(stdout: &str) -> serde_json::Value {
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|error| panic!("`to json` writes one document, got {stdout:?}: {error}"))
}

#[test]
fn should_measure_constant_state_statistics_past_the_materialization_limit() {
    // A ten-value ceiling and a thousand values: the statistics that fold into constant state
    // answer anyway, with the right figures, because nothing was held to be counted against it.
    let scratch = scratch();
    let script = format!(
        "set config limits.materialize_items = 10\n\
         echo \"{}\" | from json | measure @ | to json",
        numbers(1000)
    );

    let run = run_bounded(&scratch, &script, BUDGET);

    assert_eq!(run.code, Some(0), "{}", run.report());
    let answer = document(&run.stdout);
    let record = &answer[0];
    assert_eq!(answer.as_array().map(Vec::len), Some(1), "{}", run.report());
    assert_eq!(record["count"], 1000);
    assert_eq!(record["sum"], 500_500);
    assert_eq!(record["mean"], 500.5);
    assert_eq!(record["min"], 1);
    assert_eq!(record["max"], 1000);
    assert_eq!(
        record["median"],
        serde_json::Value::Null,
        "the median was not asked for, and holding it is what the limit would refuse"
    );
}

#[test]
fn should_still_refuse_a_percentile_past_the_materialization_limit() {
    // The distribution is held when it is asked for, and the ceiling applies to it as before.
    let scratch = scratch();
    for asked in ["--median", "--percentiles [90]"] {
        let script = format!(
            "set config limits.materialize_items = 10\n\
             echo \"{}\" | from json | measure @ {asked} | to json",
            numbers(1000)
        );

        let run = run_bounded(&scratch, &script, BUDGET);

        assert_eq!(run.code, Some(1), "{asked}: {}", run.report());
        assert!(
            run.stderr.contains("resource.item_limit"),
            "{asked} holds every sample, so the item limit refuses it. {}",
            run.report()
        );
    }
}

#[test]
fn should_report_the_median_and_the_percentiles_when_they_are_asked_for() {
    let scratch = scratch();

    let run = run_bounded(
        &scratch,
        &format!(
            "echo \"{}\" | from json | measure @ --median --percentiles [50, 90] | to json",
            numbers(10)
        ),
        BUDGET,
    );

    assert_eq!(run.code, Some(0), "{}", run.report());
    let answer = document(&run.stdout);
    assert_eq!(answer[0]["median"], 5.5, "{}", run.report());
    assert_eq!(
        answer[0]["percentiles"],
        serde_json::json!({"p50": 5, "p90": 9}),
        "{}",
        run.report()
    );
    assert_eq!(answer[0]["count"], 10);
}

#[test]
fn should_answer_a_measure_over_an_unbounded_source_after_every_value() {
    // Issue #174's exit test: "`measure count` over an unbounded source answers without
    // materializing". The source is a followed file of three numbers that never closes; the
    // statistics so far arrive after each value, and `take 3` ends the run while the file waits.
    let scratch = scratch();
    let source = scratch.write("measure/source.log", "1\n2\n3\n");

    let run = run_bounded(
        &scratch,
        &format!(
            "tail file {} --lines 3 --follow | each {{ @ | from json }} | measure @ | take 3 \
             | select count sum max | to json",
            source.display()
        ),
        BUDGET,
    );

    assert!(
        run.finished,
        "the answers arrived before the source ended, which it never does. {}",
        run.report()
    );
    assert!(
        !run.stderr.contains("unbounded_operation"),
        "an unbounded source is a legal input to constant-state statistics. {}",
        run.report()
    );
    assert_eq!(
        document(&run.stdout),
        serde_json::json!([
            {"count": 1, "sum": 1, "max": 1},
            {"count": 2, "sum": 3, "max": 2},
            {"count": 3, "sum": 6, "max": 3}
        ]),
        "one running record per value, in order. {}",
        run.report()
    );
    assert_eq!(
        std::fs::read_to_string(&source).unwrap_or_default(),
        "1\n2\n3\n",
        "and the source is as it was: nothing waited for it to end"
    );
}

#[test]
fn should_explain_measure_as_streaming_unless_a_percentile_is_asked_for() {
    // §22.4: the plan shows what the stage will hold. The constant-state statistics hold nothing,
    // so the plan says streaming; asking for the median or a percentile is what materializes.
    let scratch = scratch();

    let plain = run_bounded(&scratch, "explain get process | measure pid", BUDGET);
    assert!(
        plain.stdout.contains("execution    streaming")
            && !plain.stdout.contains("global materialization"),
        "no percentile was asked for, so nothing is held. {}",
        plain.report()
    );

    for asked in ["--median", "--percentiles [95]"] {
        let held = run_bounded(
            &scratch,
            &format!("explain get process | measure pid {asked}"),
            BUDGET,
        );
        assert!(
            held.stdout.contains("execution    global materialization")
                && held.stdout.contains("requires     finite input"),
            "{asked} holds the distribution, and the plan says so. {}",
            held.report()
        );
    }
}
