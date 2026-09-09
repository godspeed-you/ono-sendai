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
    let text = std::fs::read_to_string(baseline_path()).expect(
        "docs/contracts/hardening/performance_baseline.json is the baseline of v0.4.1 §32.4",
    );
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
    path.push("docs/contracts/hardening/performance_baseline.json");
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
    let declared = xtask::perf::reference_environment(&repository_root()).expect(
        "docs/contracts/hardening/performance_environment.yaml names the environment of §37.2",
    );
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
    let spatial: Vec<_> = verdicts
        .iter()
        .filter(|(target, _)| target.profile != "T")
        .collect();
    assert_eq!(
        spatial.len(),
        4,
        "v0.4.1 §33.2 states four reference targets"
    );
    for (target, verdict) in &spatial {
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

#[test]
fn should_measure_the_completion_budget_directly_rather_than_through_a_proxy() {
    // Issue #21: v0.4.1 §36.2's first-completion budget "is asserted as a 1 000-iteration
    // in-process proxy … and never measured". The proxy is
    // `ono-command/tests/completion.rs::should_stay_far_inside_the_first_completion_budget`, which
    // passes `None` where the value completer goes — so it measures registry lookups and touches
    // no provider at all, which is the half §36.2 budgets.
    //
    // What replaces it is a call to the completer the line editor actually installs, one cold
    // sample per process, twenty processes, recorded on the reference environment.
    let baseline =
        Baseline::parse(&std::fs::read_to_string(baseline_path()).expect("the baseline exists"))
            .expect("the baseline parses");

    let record = baseline
        .record(xtask::perf::COMPLETION_BENCHMARK, "S")
        .unwrap_or_else(|| {
            panic!(
                "v0.4.1 §36.2's completion budget has no measurement on `{}`; \
                 `cargo xtask perf` is what records one",
                baseline.environment
            )
        });

    assert!(
        record.values > 0.0,
        "a completion that offered no candidate consulted no provider, which is the proxy issue \
         #21 is about, not a measurement of §36.2"
    );
    assert!(
        record.iterations >= xtask::perf::MIN_ITERATIONS,
        "§37.4 wants at least {} iterations, and this figure has {}",
        xtask::perf::MIN_ITERATIONS,
        record.iterations
    );

    // Appendix A and §36.2: `completion.hard_budget = 150 ms`. The figure is the p95 of the first
    // completion, which is the moment the budget is about.
    assert!(
        record.p95_ms < 150.0,
        "v0.4.1 §36.2 gives interactive completion a hard budget of 150 ms, and the first \
         completion measured {:.1} ms p95 on `{}`",
        record.p95_ms,
        baseline.environment
    );
}

// --- §57 H0: the frozen v0.4.1 baseline (issue #30, ADR-0548) ----------------------------------

/// The tranche snapshot of `docs/baselines/v0.4.1.json`.
fn frozen_baseline() -> serde_json::Value {
    let text = std::fs::read_to_string(repository_root().join("docs/baselines/v0.4.1.json"))
        .expect("v0.4.1 §57 H0's frozen baseline is a file in the repository");
    serde_json::from_str(&text).expect("the frozen baseline is JSON")
}

#[test]
fn should_read_the_frozen_v041_baseline_and_find_every_metric_it_declares() {
    // §57 H0 asks for a baseline "a machine-readable baseline file in the repository that H7 and
    // H11 both consume rather than re-derive". H7 wrote the regression baseline and H11 the
    // release input manifest, so this file binds the two and states what neither holds; what it
    // must not do is restate their figures (§52.2). This is the check that every figure it names
    // still resolves where the figure lives, with all six of §32.3's metrics present.
    let snapshot = frozen_baseline();
    assert_eq!(
        snapshot["schema"].as_str(),
        Some("ono.baseline.v1"),
        "the snapshot names its own shape"
    );
    let named = snapshot["performance"]["measurements"]
        .as_array()
        .expect("the snapshot names the benchmarks it froze");
    assert!(
        !named.is_empty(),
        "a baseline naming no benchmark is not a baseline"
    );

    let problems = xtask::baseline::check(&repository_root());
    assert!(
        problems.is_empty(),
        "the frozen baseline does not resolve against what it froze:\n{}",
        problems
            .iter()
            .map(|p| format!("  {} — {}", p.location, p.detail))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn should_report_a_frozen_baseline_naming_a_benchmark_nobody_measured() {
    // A snapshot is only evidence while its references resolve. A benchmark it names that the
    // regression baseline does not hold is a figure nobody can look up.
    let scratch = ono_testkit::scratch();
    let root = repository_root();
    for file in [
        "docs/contracts/hardening/performance_baseline.json",
        "docs/contracts/hardening/performance_environment.yaml",
    ] {
        scratch.write(
            file,
            std::fs::read_to_string(root.join(file)).expect("a performance registry"),
        );
    }
    let mut snapshot = frozen_baseline();
    snapshot["performance"]["measurements"]
        .as_array_mut()
        .expect("the measurements")
        .push(serde_json::json!({
            "benchmark": "spatial.imaginary",
            "profile": "S",
            "temperature": "cold"
        }));
    scratch.write(
        "docs/baselines/v0.4.1.json",
        serde_json::to_string_pretty(&snapshot).expect("the snapshot serialises"),
    );
    let problems = xtask::baseline::check(scratch.path());
    assert!(
        problems
            .iter()
            .any(|problem| problem.detail.contains("spatial.imaginary")),
        "a benchmark the regression baseline does not hold is reported: {problems:?}"
    );
}

#[test]
fn should_report_a_frozen_baseline_that_leaves_a_measured_benchmark_out() {
    // The other direction: a snapshot that names three of five benchmarks is a snapshot of
    // whatever somebody remembered, which is the failure §32.4's baseline exists against.
    let scratch = ono_testkit::scratch();
    let root = repository_root();
    for file in [
        "docs/contracts/hardening/performance_baseline.json",
        "docs/contracts/hardening/performance_environment.yaml",
    ] {
        scratch.write(
            file,
            std::fs::read_to_string(root.join(file)).expect("a performance registry"),
        );
    }
    let mut snapshot = frozen_baseline();
    snapshot["performance"]["measurements"]
        .as_array_mut()
        .expect("the measurements")
        .truncate(1);
    scratch.write(
        "docs/baselines/v0.4.1.json",
        serde_json::to_string_pretty(&snapshot).expect("the snapshot serialises"),
    );
    let problems = xtask::baseline::check(scratch.path());
    assert!(
        problems
            .iter()
            .any(|problem| problem.detail.contains("says nothing about")),
        "a measured benchmark the snapshot omits is reported: {problems:?}"
    );
}

#[test]
fn should_report_a_frozen_baseline_that_leaves_an_absent_artifact_hash_unexplained() {
    // v0.4.1 §2.6 and spec §35.3: unknown stays unknown, never fabricated and never merely
    // absent. No v0.4.1 release has been published, so the artifact hashes §57 H0 asks for do
    // not exist — and a null that says nothing is a question nobody asked.
    let scratch = ono_testkit::scratch();
    let root = repository_root();
    for file in [
        "docs/contracts/hardening/performance_baseline.json",
        "docs/contracts/hardening/performance_environment.yaml",
    ] {
        scratch.write(
            file,
            std::fs::read_to_string(root.join(file)).expect("a performance registry"),
        );
    }
    let mut snapshot = frozen_baseline();
    snapshot["artifacts"]["reason"] = serde_json::Value::Null;
    scratch.write(
        "docs/baselines/v0.4.1.json",
        serde_json::to_string_pretty(&snapshot).expect("the snapshot serialises"),
    );
    let problems = xtask::baseline::check(scratch.path());
    assert!(
        problems
            .iter()
            .any(|problem| problem.detail.contains("reason")),
        "an unexplained absence is reported: {problems:?}"
    );
}

#[test]
fn should_capture_the_frozen_baseline_from_the_sources_rather_than_from_a_second_list() {
    // #30's exit test is a file "that H7 and H11 both consume rather than re-derive", and
    // ADR-0451 asked for the same thing from the other side: "its baseline should be a captured
    // manifest rather than a second, hand-written list of the same facts". So the snapshot is
    // written by a command that reads the sources, and running it twice at one commit produces
    // one answer.
    let root = repository_root();
    let first = xtask::baseline::capture(&root).expect("the snapshot can be captured");
    let second = xtask::baseline::capture(&root).expect("the snapshot can be captured twice");
    assert_eq!(
        first, second,
        "a capture of one commit is one answer, or the file churns for no reason"
    );

    let captured: serde_json::Value = serde_json::from_str(&first).expect("the capture is JSON");
    let manifest = xtask::provenance::build_inputs(&root);
    assert_eq!(
        captured["release_inputs"]["at_capture"]["schema"], manifest["schema"],
        "Appendix H's manifest is captured from the generator, not retyped"
    );
    let measured = Baseline::parse(
        &std::fs::read_to_string(baseline_path()).expect("the regression baseline"),
    )
    .expect("the regression baseline parses");
    assert_eq!(
        captured["performance"]["measurements"]
            .as_array()
            .expect("the captured measurements")
            .len(),
        measured.measurements.len(),
        "every figure H7 measured is named, and none is invented"
    );
}

// ------------------------------------------------------------------------------------------
// v0.5 §32, §49 and work packages TEST-001, TEST-002, TEST-003.
// ------------------------------------------------------------------------------------------

/// A small fixture built from §49's declaration, so the generator is proven where the gate can
/// afford to run it.
///
/// v0.5 §49's own cardinality is a million events; the profile declares itself `benchmark` for
/// that reason and `cargo xtask perf` builds it. What a gate run can prove is the property that
/// makes the large one worth building — that the generator is deterministic and that the store it
/// writes is a real ledger — and that is proven at a size a laptop writes in under a second.
fn small_fixture(seed: u64, root: &std::path::Path) -> xtask::perf::fixture::FixtureLedger {
    let profile = ono_testkit::temporal::TemporalProfile {
        name: "T",
        events: 4_000,
        objects: 400,
        relation_changes: 2_000,
        actions: 40,
        seed,
    };
    xtask::perf::fixture::build(profile, root).expect("the fixture builds")
}

/// A scratch directory under `target/`, because this host's `/tmp` is quota'd.
fn scratch(name: &str) -> std::path::PathBuf {
    let path = repository_root()
        .join("target")
        .join("perf-tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("a scratch directory");
    path
}

#[test]
fn should_write_the_same_history_when_the_fixture_is_built_twice_from_one_seed() {
    // §49's word is "deterministic", and this is what it has to mean for a ledger: the same seed
    // produces the same events with the same identities, so two measurements a fortnight apart
    // are measurements of one history. The digest is over every `EventId` in write order, which
    // is a digest over the content of every event — an `EventId` is the SHA-256 of the fields
    // that make it that observation.
    let root = scratch("determinism");
    let first = small_fixture(11, &root.join("a"));
    let second = small_fixture(11, &root.join("b"));

    assert_eq!(
        first.digest(),
        second.digest(),
        "two builds from seed 11 wrote two different histories, so no figure measured against \
         either can be compared with the other"
    );
    assert_eq!(first.events(), second.events());
    assert_eq!(first.evidence(), second.evidence());
    assert_eq!(first.actions(), second.actions());
    assert_eq!(first.checkpoints(), second.checkpoints());

    let third = small_fixture(12, &root.join("c"));
    assert_ne!(
        first.digest(),
        third.digest(),
        "a different seed must produce a different history, or the seed is not a seed"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn should_hold_the_declared_cardinality_in_a_real_store_when_the_fixture_is_built() {
    // §49's fixture is a ledger, not a file: it is read back through `LedgerRead`, its retention
    // state counts what it holds, and its query plans are SQLite's own. A hand-written file would
    // pass none of this.
    use ono_temporal_core::LedgerRead as _;

    let root = scratch("cardinality");
    let fixture = small_fixture(21, &root);
    let profile = fixture.profile();

    assert_eq!(
        fixture.events(),
        profile.events as u64,
        "the fixture wrote fewer events than it declares"
    );
    assert_eq!(fixture.actions(), profile.actions as u64);
    assert!(
        fixture.bytes() > 0,
        "a fixture ledger that occupies no disk is not a store"
    );

    let store = fixture
        .open(ono_temporal_ledger::RetentionPolicy::unlimited())
        .expect("the fixture opens");
    let retention = store.retention();
    assert_eq!(
        retention.events, profile.events as u64,
        "the store holds a different number of events than the generator wrote"
    );

    // §49 counts relation changes separately, and they are events of the two relation kinds.
    let relations = store
        .events(&ono_temporal_core::EventQuery {
            kinds: vec![
                ono_temporal_core::EventKind::RelationAdded,
                ono_temporal_core::EventKind::RelationRemoved,
            ],
            ..ono_temporal_core::EventQuery::in_range(ono_temporal_core::TimeRange::all())
        })
        .expect("the relation query answers");
    assert_eq!(
        relations.len(),
        profile.relation_changes,
        "§49 asks for {} relation changes and the fixture holds {}",
        profile.relation_changes,
        relations.len()
    );

    // §49's hundred thousand lifetimes, at this size four hundred: every object appears exactly
    // once, so the count of appearance events is the count of lifetimes.
    let appearances = store
        .events(&ono_temporal_core::EventQuery {
            kinds: vec![ono_temporal_core::EventKind::ObjectAppeared],
            ..ono_temporal_core::EventQuery::in_range(ono_temporal_core::TimeRange::all())
        })
        .expect("the appearance query answers");
    let distinct: std::collections::BTreeSet<_> = appearances
        .iter()
        .filter_map(|event| event.subject.as_ref())
        .filter_map(|reference| match reference {
            ono_temporal_core::SpatialRef::Resolved { id, .. } => Some(id.clone()),
            ono_temporal_core::SpatialRef::Unresolved { .. } => None,
        })
        .collect();
    assert_eq!(
        distinct.len(),
        profile.objects,
        "§49 asks for {} object lifetimes and the fixture holds {}",
        profile.objects,
        distinct.len()
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn should_answer_every_measured_query_from_an_index_rather_than_a_scan() {
    // §32.3's budgets are met by the plan or not at all, and SQLite will say which. `EXPLAIN
    // QUERY PLAN` answering "SCAN events" over a million rows is the architectural scaling
    // failure §49 exists to catch, so it is asserted rather than hoped for.
    let root = scratch("plans");
    let fixture = small_fixture(31, &root);
    let store = fixture
        .open(ono_temporal_ledger::RetentionPolicy::default())
        .expect("the fixture opens");

    for (what, plan) in [
        ("timeline", store.explain_timeline_plan()),
        ("changes", store.explain_changes_plan()),
        ("at (checkpoint)", store.explain_checkpoint_plan()),
        ("why (causal links)", store.explain_causal_plan()),
        ("find event", store.explain_event_plan()),
        ("retention sweep", store.explain_retention_plan()),
    ] {
        let rows = plan.unwrap_or_else(|error| panic!("the {what} plan is unavailable: {error}"));
        assert!(
            !rows.is_empty(),
            "SQLite reported no plan at all for the {what} query"
        );
        assert!(
            rows.iter()
                .any(|row| row.contains("USING INDEX") || row.contains("USING COVERING INDEX")),
            "v0.5 §32.3 budgets the {what} query, and SQLite plans it as {rows:?} — no index is \
             used, so the cost grows with the whole ledger rather than with the answer"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn should_declare_a_benchmark_for_every_release_measurement_v05_requires() {
    // §49's list, typed out from the specification rather than read from the table it checks.
    const SECTION_49: [&str; 8] = [
        "temporal.startup_disabled",
        "temporal.recorder_idle",
        "temporal.timeline_15m",
        "temporal.changes_1h",
        "temporal.reconstruct_recent",
        "temporal.map_historical_l1",
        "temporal.why",
        "temporal.retention_sweep",
    ];

    let declared: Vec<&str> = xtask::perf::TEMPORAL_BENCHMARKS
        .iter()
        .map(|benchmark| benchmark.id)
        .collect();
    for wanted in SECTION_49 {
        assert!(
            declared.contains(&wanted),
            "v0.5 §49 requires release evidence for `{wanted}` and no benchmark is declared for \
             it; the table declares {declared:?}"
        );
    }

    // Every §32 target names a benchmark somebody can run. A target pointing at nothing is a
    // budget that can never be measured, which is worse than a missing budget.
    for target in xtask::perf::TARGETS {
        if target.profile != "T" {
            continue;
        }
        assert!(
            declared.contains(&target.benchmark),
            "the target \"{}\" is answered by `{}`, which no benchmark measures",
            target.spec,
            target.benchmark
        );
    }
}

#[test]
fn should_report_an_unmeasured_temporal_target_as_a_failure_rather_than_a_pass() {
    // The property the whole package rests on (§65.10). A baseline with no temporal record must
    // answer `Unmeasured` for every v0.5 row — never `Held`.
    let baseline = Baseline::parse(&baseline_of(&[complete_record("spatial.look")]))
        .expect("the baseline parses");
    for (target, verdict) in xtask::perf::verdicts(&baseline) {
        if target.profile != "T" {
            continue;
        }
        assert_eq!(
            verdict,
            xtask::perf::TargetVerdict::Unmeasured,
            "\"{}\" has no record in this baseline and must not read as held",
            target.spec
        );
    }
}

#[test]
fn should_state_a_startup_target_as_an_increase_rather_than_as_an_absolute_figure() {
    // v0.5 §32.1: "MUST add less than 5 ms p95 to Ono interactive startup". A 5 ms budget read
    // against an absolute startup figure would be a budget the shell has already spent, so the
    // row names what it is an increase over and the verdict subtracts.
    let target = xtask::perf::TARGETS
        .iter()
        .find(|target| target.benchmark == "temporal.startup_disabled")
        .expect("v0.5 §32.1 is a declared target");
    assert_eq!(
        target.relative_to,
        Some("shell.cold_start"),
        "§32.1 budgets an addition, so the row must name what it is added to"
    );

    // A startup that costs 4.8 ms without a ledger and 6.0 ms with one added 1.2 ms, which holds.
    let held = Baseline::parse(&baseline_of(&[
        record_at("shell.cold_start", "S", "cold", 4.8),
        record_at("temporal.startup_disabled", "T", "cold", 6.0),
    ]))
    .expect("the baseline parses");
    assert_eq!(
        verdict_for(&held, "temporal.startup_disabled"),
        xtask::perf::TargetVerdict::Held { p95_ms: 1.2 }
    );

    // The same shell paying 12 ms for the ledger's presence added 7.2 ms, which does not.
    let missed = Baseline::parse(&baseline_of(&[
        record_at("shell.cold_start", "S", "cold", 4.8),
        record_at("temporal.startup_disabled", "T", "cold", 12.0),
    ]))
    .expect("the baseline parses");
    assert!(
        matches!(
            verdict_for(&missed, "temporal.startup_disabled"),
            xtask::perf::TargetVerdict::Missed { .. }
        ),
        "7.2 ms of added startup is outside §32.1's 5 ms and must be reported as missed"
    );

    // Half a subtraction is not a smaller difference.
    let alone = Baseline::parse(&baseline_of(&[record_at(
        "temporal.startup_disabled",
        "T",
        "cold",
        6.0,
    )]))
    .expect("the baseline parses");
    assert_eq!(
        verdict_for(&alone, "temporal.startup_disabled"),
        xtask::perf::TargetVerdict::Unmeasured,
        "without the figure it is an increase over, the increase is unknown"
    );
}

/// The verdict the row answered by `benchmark` reads on `baseline`.
fn verdict_for(baseline: &Baseline, benchmark: &str) -> xtask::perf::TargetVerdict {
    xtask::perf::verdicts(baseline)
        .into_iter()
        .find(|(target, _)| target.benchmark == benchmark)
        .map(|(_, verdict)| verdict)
        .expect("the row is a declared target")
}

/// A complete record at a stated profile, temperature and p95.
fn record_at(benchmark: &str, profile: &str, temperature: &str, p95_ms: f64) -> String {
    format!(
        r#"{{
          "benchmark": "{benchmark}",
          "profile": "{profile}",
          "commit": "0000000000000000000000000000000000000000",
          "environment": "reference-2026-09",
          "temperature": "{temperature}",
          "iterations": 20,
          "build": "release",
          "time_to_first_ms": {p95_ms},
          "time_to_complete_ms": {p95_ms},
          "p95_ms": {p95_ms},
          "peak_rss_bytes": 41943040,
          "values": 1,
          "values_per_second": 100.0,
          "estimated_bytes": 512,
          "cancel_ms": null
        }}"#
    )
}

#[test]
fn should_declare_the_fixture_ledger_in_the_registry_and_in_the_testkit_alike() {
    // The property `crates/ono-spatial-query/tests/profiles.rs` keeps for Appendix F, kept here
    // for §49: a fixture the registry declares and nothing can build is a failure, and so is a
    // fixture the code knows and the registry omits.
    //
    // §49's numbers are typed out from the specification rather than read from either side, so
    // the check has something to disagree with.
    const SECTION_49: (usize, usize, usize, usize) = (1_000_000, 100_000, 500_000, 10_000);

    let declared = ono_testkit::temporal::declared_temporal_profiles();
    assert_eq!(
        declared.len(),
        1,
        "v0.5 §49 describes one fixture ledger; the registry declares {declared:?}"
    );
    let row = &declared[0];
    assert_eq!(
        (row.events, row.objects, row.relation_changes, row.actions),
        SECTION_49,
        "the registry declares a fixture at a cardinality v0.5 §49 does not state"
    );
    assert_eq!(
        row.built_by,
        ono_testkit::BuiltBy::Benchmark,
        "a million-event store is minutes of writing; §49's fixture is built by the benchmark \
         command rather than on every gate run"
    );

    let constant = row.profile();
    assert_eq!(
        (
            constant.events,
            constant.objects,
            constant.relation_changes,
            constant.actions
        ),
        SECTION_49,
        "`TEMPORAL_PROFILE_T` and its declaration are two different fixtures; §52.2 allows one \
         home for a number"
    );
    assert_eq!(
        constant.seed, row.seed,
        "the seed is part of the declaration, because \"deterministic\" is a claim about one \
         stream"
    );
}

#[test]
fn should_agree_with_the_kuang_test_host_on_the_instant_a_fixture_starts_at() {
    // Two origins would make two suites' fixtures incomparable for no gain. The test host pins
    // spec §31.73's virtual time; the temporal harness agrees with it rather than choosing again.
    assert_eq!(
        ono_testkit::temporal::VIRTUAL_NOW,
        ono_kuang_testhost::VIRTUAL_NOW,
        "the temporal harness and the KUANG/11 test host must read the same clock"
    );
}
