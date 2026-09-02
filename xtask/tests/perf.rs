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

use xtask::perf::{Baseline, Benchmark, Comparison, Runner, Tolerance};

/// A record carrying everything §32.3 and Appendix F.4 require.
fn complete_record(benchmark: &str) -> String {
    format!(
        r#"{{
          "benchmark": "{benchmark}",
          "profile": "M",
          "commit": "0000000000000000000000000000000000000000",
          "environment": "reference-2026-09",
          "temperature": "cold",
          "iterations": 20,
          "build": "release",
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

// --- §37: the benchmark command, the reference environment, warm and cold --------------------

#[test]
fn should_run_the_declared_benchmarks_and_write_their_records() {
    // §37.1: "benchmark execution must be discoverable and reproducible". The runner is what
    // `cargo xtask perf` calls, so exercising it here exercises the command; what this test may
    // not do is run the real declared set, which builds populations and takes minutes.
    let home = ono_testkit::scratch();
    let runner = Runner::new(ono_testkit::ono_binary(), environment(), "0".repeat(40))
        .iterations(3)
        .build("debug");

    let measured = runner.run(&Benchmark::probe());

    assert_eq!(
        measured.iterations, 3,
        "a record states how many iterations produced it (§37.4)"
    );
    assert!(
        measured
            .metric("time_to_first_ms")
            .is_some_and(|ms| ms > 0.0),
        "a benchmark that produced output has a time to first value, got {:?}",
        measured.metric("time_to_first_ms")
    );
    assert!(
        measured.metric("values_per_second").is_some(),
        "§32.3 requires values per second of every benchmark"
    );

    // And the record round-trips through the baseline file the runner writes.
    let path = home.path().join("baseline.json");
    xtask::perf::write_baseline(&path, environment(), std::slice::from_ref(&measured))
        .expect("the runner writes its records");
    let written =
        Baseline::parse(&std::fs::read_to_string(&path).expect("the baseline was written"))
            .expect("what the runner writes is a valid baseline");
    assert_eq!(
        written.measurements,
        vec![measured],
        "a record written and read back is the record that was measured"
    );
}

#[test]
fn should_name_the_reference_environment_on_every_recorded_figure() {
    // §37.2: the release documentation names CPU, cores, RAM, kernel, image, toolchain and
    // release build flags. The registry is where those live, and a figure that did not name one
    // of them is a figure about an unknown machine (§32.4).
    let declared = xtask::perf::reference_environment(&repository_root())
        .expect("docs/spec/hardening/performance_environment.yaml names the environment of §37.2");
    for field in [
        "cpu_model",
        "cpu_cores",
        "ram_bytes",
        "kernel",
        "distribution",
        "rust_toolchain",
        "release_build_flags",
    ] {
        assert!(
            declared.states(field),
            "v0.4.1 §37.2 requires the reference environment to name `{field}`"
        );
    }

    let baseline =
        Baseline::parse(&std::fs::read_to_string(baseline_path()).expect("the baseline exists"))
            .expect("the baseline parses");
    assert_eq!(
        baseline.environment, declared.id,
        "the baseline is tied to the environment the registry names (§32.4)"
    );
    for record in &baseline.measurements {
        assert_eq!(
            record.environment, declared.id,
            "`{}` was measured on `{}` and filed in a baseline for `{}`",
            record.benchmark, record.environment, declared.id
        );
    }
}

#[test]
fn should_distinguish_a_warm_measurement_from_a_cold_one() {
    // §37.3: "A warm-cache number MUST not be advertised as cold performance." So temperature is
    // part of a record's identity, and comparing across it is not a comparison at all.
    let cold = complete_record("spatial.look");
    let warm = complete_record("spatial.look")
        .replace(r#""temperature": "cold""#, r#""temperature": "warm""#);
    assert_ne!(cold, warm, "the fixture must actually differ");

    let baseline = Baseline::parse(&baseline_of(&[cold])).expect("the baseline parses");
    let measured = Baseline::parse(&baseline_of(&[warm]))
        .expect("the measurement parses")
        .measurements
        .remove(0);

    assert!(
        matches!(
            baseline.compare(&measured, Tolerance::percent(10.0)),
            Comparison::Unmeasured
        ),
        "a warm figure has no cold baseline to be held against; §37.3 forbids advertising one as \
         the other"
    );

    // The declared set carries both, so the distinction is measured rather than merely possible.
    let temperatures: std::collections::BTreeSet<_> = xtask::perf::BENCHMARKS
        .iter()
        .map(|benchmark| benchmark.temperature)
        .collect();
    assert!(
        temperatures.len() >= 2,
        "§37.3 requires benchmarks to distinguish cold from warm, and the declared set is all \
         {temperatures:?}"
    );

    // §37.4: "Single-run best-case timings MUST NOT define release success."
    let single = Baseline::parse(&baseline_of(&[
        complete_record("spatial.look").replace(r#""iterations": 20"#, r#""iterations": 1"#)
    ]))
    .expect("the measurement parses")
    .measurements
    .remove(0);
    assert!(
        matches!(
            baseline.compare(&single, Tolerance::Absolute),
            Comparison::Underpowered { .. }
        ),
        "one iteration cannot qualify a release (§37.4)"
    );
}

/// The environment the checked-in baseline is tied to.
fn environment() -> String {
    xtask::perf::reference_environment(&repository_root())
        .expect("the reference environment is declared")
        .id
}

/// The repository root.
fn repository_root() -> std::path::PathBuf {
    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path
}

#[test]
fn should_measure_every_time_to_first_result_target_of_the_reference_targets_table() {
    // §33.2 states four targets "on the release reference environment". A target nobody measured
    // there is not a target that holds, and the failure this test exists to prevent is the one
    // §65.10 names: an unmeasured row reaching the summary as a pass.
    let baseline =
        Baseline::parse(&std::fs::read_to_string(baseline_path()).expect("the baseline exists"))
            .expect("the baseline parses");

    let verdicts = xtask::perf::verdicts(&baseline);
    assert_eq!(
        verdicts.len(),
        4,
        "v0.4.1 §33.2 states four reference targets"
    );
    for (target, verdict) in &verdicts {
        assert_ne!(
            *verdict,
            xtask::perf::TargetVerdict::Unmeasured,
            "v0.4.1 §33.2's \"{}\" has no measurement on `{}`; the row is answered by `{}` at \
             Profile {} ({}), and `cargo xtask perf` is what measures it",
            target.spec,
            baseline.environment,
            target.benchmark,
            target.profile,
            target.temperature.as_str()
        );
    }

    // §33.3 is the floor underneath all four, and §61.3 makes it a watchdog. It is asserted here
    // against the recorded figures rather than by running anything, so the check is a comparison
    // of two numbers in a file: deterministic, and never a verdict about the machine that
    // happens to be running `cargo test` (ADR-0252, ADR-0431).
    for record in &baseline.measurements {
        let first = record
            .metric("time_to_first_ms")
            .expect("every record carries a time to first value");
        assert!(
            first < xtask::perf::HARD_INTERACTIVE_BUDGET_MS,
            "v0.4.1 §33.3: `{}` at Profile {} ({}) recorded a first result after {first:.0} ms, \
             and a supported interactive operation must not spend {} ms producing neither output \
             nor progress",
            record.benchmark,
            record.profile,
            record.temperature.as_str(),
            xtask::perf::HARD_INTERACTIVE_BUDGET_MS
        );
    }
}
