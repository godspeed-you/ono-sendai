//! The performance record and the regression baseline (v0.4.1 §32.3, §32.4, Appendix F.4).
//!
//! §32.3 lists six metrics and then says why one number is not enough:
//!
//! > A single total runtime number is insufficient for streaming operations.
//!
//! That is the whole of this suite's subject. A streaming operation that answers in 200 ms and
//! finishes in 40 s is a different product from one that is blank for 39 s and then dumps
//! everything, and a total runtime cannot tell them apart — which is exactly the confusion
//! §0.5.7 records and issue #22 lived inside for a release cycle.
//!
//! §32.4 adds the second half: a figure means nothing without the machine it was measured on.
//!
//! > Performance results MUST be stored in a machine-readable baseline file tied to the
//! > reference environment.
//!
//! So a record names its environment, and a comparison between two environments is reported as
//! uncomparable rather than quietly passing.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use xtask::perf::{Baseline, Comparison, Tolerance};

/// A record carrying everything §32.3 and Appendix F.4 require.
fn complete_record(benchmark: &str) -> String {
    format!(
        r#"{{
          "benchmark": "{benchmark}",
          "profile": "M",
          "commit": "0000000000000000000000000000000000000000",
          "environment": "reference-2026-09",
          "time_to_first_ms": 120.0,
          "time_to_complete_ms": 900.0,
          "p95_ms": 1000.0,
          "peak_rss_bytes": 41943040,
          "values": 5000,
          "values_per_second": 5555.5,
          "estimated_bytes": 2097152,
          "cancel_ms": 3.0
        }}"#
    )
}

/// A baseline document holding `records`, for the environment every record names.
fn baseline_of(records: &[String]) -> String {
    format!(
        r#"{{
          "version": 1,
          "environment": "reference-2026-09",
          "measurements": [{}]
        }}"#,
        records.join(",")
    )
}

#[test]
fn should_record_all_six_required_metrics_for_every_benchmark() {
    // The checked-in baseline is the one that has to hold, not a fixture: a record that stopped
    // carrying one of §32.3's six is a record nobody can read the regression out of.
    let text = std::fs::read_to_string(baseline_path())
        .expect("docs/spec/hardening/performance_baseline.json is the baseline of v0.4.1 §32.4");
    let baseline = Baseline::parse(&text).unwrap_or_else(|problems| {
        panic!("the checked-in baseline must parse: {problems:#?}");
    });
    assert!(
        !baseline.environment.is_empty(),
        "v0.4.1 §32.4 ties a baseline to a named reference environment; this one names none"
    );

    // And the parser is what enforces it, so a record written by hand cannot omit a metric.
    let parsed = Baseline::parse(&baseline_of(&[complete_record("spatial.map_live")]))
        .expect("a complete record parses");
    let record = parsed
        .record("spatial.map_live", "M")
        .expect("the record is filed under its benchmark and profile");
    assert_eq!(
        record.metrics().len(),
        6,
        "v0.4.1 §32.3 requires six metrics per benchmark and this record offers {}",
        record.metrics().len()
    );
}

#[test]
fn should_fail_when_a_benchmark_reports_only_a_total_runtime() {
    // §32.3: "A single total runtime number is insufficient for streaming operations."
    let only_a_total = r#"{
      "version": 1,
      "environment": "reference-2026-09",
      "measurements": [
        {
          "benchmark": "spatial.map_live",
          "profile": "M",
          "commit": "0000000000000000000000000000000000000000",
          "environment": "reference-2026-09",
          "time_to_complete_ms": 900.0
        }
      ]
    }"#;

    let problems = Baseline::parse(only_a_total)
        .expect_err("a record carrying only a total runtime is not a benchmark result");
    let detail = problems
        .iter()
        .map(|problem| problem.detail.clone())
        .collect::<Vec<_>>()
        .join("\n");
    for metric in ["time_to_first_ms", "values_per_second", "cancel_ms"] {
        assert!(
            detail.contains(metric),
            "the refusal must name `{metric}` as one of the metrics §32.3 requires; it said:\n\
             {detail}"
        );
    }
}

#[test]
fn should_compare_a_benchmark_result_against_the_baseline_for_its_reference_environment() {
    let baseline = Baseline::parse(&baseline_of(&[complete_record("spatial.map_live")]))
        .expect("the baseline parses");

    // Same environment, same figures: held.
    let measured = Baseline::parse(&baseline_of(&[complete_record("spatial.map_live")]))
        .expect("the measurement parses")
        .measurements
        .remove(0);
    assert!(
        matches!(
            baseline.compare(&measured, Tolerance::percent(10.0)),
            Comparison::Held
        ),
        "a result equal to its baseline holds"
    );

    // A deliberate regression, on the metric §32.3 names first.
    let slower = Baseline::parse(&baseline_of(&[complete_record("spatial.map_live")
        .replace("\"time_to_first_ms\": 120.0", "\"time_to_first_ms\": 900.0")]))
    .expect("the measurement parses")
    .measurements
    .remove(0);
    let Comparison::Regressed(regressions) = baseline.compare(&slower, Tolerance::percent(10.0))
    else {
        panic!("a first-value time seven times the baseline is a regression");
    };
    assert!(
        regressions
            .iter()
            .any(|regression| regression.metric == "time_to_first_ms"),
        "the regression must name the metric that moved, got {regressions:#?}"
    );

    // A figure from another machine is not a verdict about this one (§32.4).
    let elsewhere = Baseline::parse(
        &baseline_of(&[complete_record("spatial.map_live")]).replace(
            "\"environment\": \"reference-2026-09\"",
            "\"environment\": \"some-ci-runner\"",
        ),
    )
    .expect("the measurement parses")
    .measurements
    .remove(0);
    assert!(
        matches!(
            baseline.compare(&elsewhere, Tolerance::percent(10.0)),
            Comparison::ForeignEnvironment { .. }
        ),
        "a result measured somewhere else must be reported as uncomparable, never as a pass"
    );

    // A benchmark the baseline has never seen is not a pass either.
    let unknown = Baseline::parse(&baseline_of(&[complete_record("spatial.selector_miss")]))
        .expect("the measurement parses")
        .measurements
        .remove(0);
    assert!(
        matches!(
            baseline.compare(&unknown, Tolerance::percent(10.0)),
            Comparison::Unmeasured
        ),
        "a benchmark with no baseline record is unmeasured, not held"
    );
}

/// The checked-in baseline of v0.4.1 §32.4.
fn baseline_path() -> std::path::PathBuf {
    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.push("docs/spec/hardening/performance_baseline.json");
    path
}
