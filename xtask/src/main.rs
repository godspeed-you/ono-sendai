//! Repository automation.
//!
//! `cargo xtask <task>` is the single entry point an agent uses to verify its work. Every task
//! is also runnable as a plain script so it works identically in CI and inside a container.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use xtask::{
    architecture, baseline, bindings, change, conformance, contracts, metrics as repo_metrics,
    narrative, notices, perf, provenance, reference, reproducibility, scan, supply_chain, temporal,
    terminology, verification,
};

fn main() -> ExitCode {
    let task = std::env::args().nth(1);
    let rest: Vec<String> = std::env::args().skip(2).collect();

    match task.as_deref() {
        Some("gate") => run_script("gate.sh", &rest),
        Some("acceptance") => run_script("acceptance.sh", &rest),
        Some("spec-check") => spec_check(),
        Some("state-check") => state_check(),
        Some("skip-check") => skip_check(&rest),
        Some("terminology") => terminology(&rest),
        Some("metrics") => metrics(&rest),
        Some("build-manifest") => build_manifest(&rest),
        Some("compare-builds") => compare_builds(&rest),
        Some("checksums") => checksums(&rest),
        Some("provenance") => provenance_task(&rest),
        Some("perf") => perf(&rest),
        Some("baseline") => frozen_baseline(&rest),
        Some("docs") => generate_docs(),
        Some("licenses") => licenses(&rest),
        Some("conformance") => generate_conformance(),
        Some("release-check") => run_script("release-check.sh", &rest),
        Some(other) => {
            eprintln!("xtask: unknown task `{other}`");
            usage();
            ExitCode::FAILURE
        }
        None => {
            usage();
            ExitCode::FAILURE
        }
    }
}

/// The frozen v0.4.1 baseline of spec section 57 phase H0 (ADR-0548).
///
/// Reads the sources rather than a second list of the same facts: the counts from `metrics`, the
/// benchmarks from the regression baseline H7 wrote, the release inputs from the manifest
/// generator H10 wrote.
fn frozen_baseline(args: &[String]) -> ExitCode {
    let root = repo_root();
    if args.iter().any(|argument| argument == "--write") {
        return match baseline::write(&root) {
            Ok(path) => {
                println!("baseline: wrote {path}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("baseline: {error}");
                ExitCode::FAILURE
            }
        };
    }
    match baseline::capture(&root) {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("baseline: {error}");
            ExitCode::FAILURE
        }
    }
}

fn usage() {
    eprintln!("usage: cargo xtask <task>");
    eprintln!();
    eprintln!("tasks:");
    eprintln!("  gate           format, lint, test, contract check, docs (AGENTS.md section 10)");
    eprintln!("  spec-check     contract drift between docs/contracts and the implementation");
    eprintln!("  state-check    the claims docs/ACCEPTANCE.md makes about docs/STATE.md");
    eprintln!(
        "  skip-check     a test log's SKIPPED markers against the declared expectation \
(spec section 38.3) <log>"
    );
    eprintln!("  build-manifest write the release input manifest of Appendix H [--output <path>]");
    eprintln!(
        "  compare-builds two directories of build artifacts, byte for byte \
(spec section 46.5) <first> <second>"
    );
    eprintln!(
        "  checksums      write SHA256SUMS over a release directory, or check it \
(spec section 47.2) [--dir <path>] [--verify] [--tested <path>]"
    );
    eprintln!(
        "  provenance     write build provenance over a release directory, or check it \
(spec section 47.4) [--dir <path>] [--inputs <path>] [--verify]"
    );
    eprintln!(
        "  perf           run the performance benchmarks of spec section 37.1 \
[--profile S|M|L] [--iterations N] [--compare <path>] [--write-baseline] \
[--skip-temporal]"
    );
    eprintln!(
        "  terminology    the documentation terminology contract of section 19.1 over this \
repository, and over a Wiki checkout when one is named [--wiki <path>]"
    );
    eprintln!(
        "  metrics        the generated repository metrics of section 50 [--write] to update the \
README block"
    );
    eprintln!("  baseline       the frozen v0.4.1 baseline of spec section 57 phase H0 [--write]");
    eprintln!("  docs           regenerate docs/reference/ from the contracts (spec section 36.2)");
    eprintln!(
        "  conformance    regenerate the provider conformance suite from docs/contracts (spec section 35.3)"
    );
    eprintln!("  acceptance     build the container and run the acceptance suite");
    eprintln!("  release-check  the full release gate of docs/ACCEPTANCE.md");
}

fn repo_root() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path
}

fn run_script(name: &str, args: &[String]) -> ExitCode {
    let script = repo_root().join("scripts").join(name);
    let status = Command::new("bash").arg(&script).args(args).status();

    match status {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(status) => {
            eprintln!("xtask: {name} failed with {status}");
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("xtask: cannot run {}: {error}", script.display());
            ExitCode::FAILURE
        }
    }
}

/// Writes the release input manifest of Appendix H (spec section 43.2, ADR-0451).
///
/// The release workflow runs this before it publishes anything, so the file states what the
/// build was given rather than what the artifacts turned out to be.
fn build_manifest(args: &[String]) -> ExitCode {
    let mut output = None;
    let mut rest = args.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--output" => match rest.next() {
                Some(path) => output = Some(PathBuf::from(path)),
                None => {
                    eprintln!("build-manifest: --output needs a path");
                    return ExitCode::FAILURE;
                }
            },
            other => match other.strip_prefix("--output=") {
                Some(path) => output = Some(PathBuf::from(path)),
                None => {
                    eprintln!("build-manifest: unknown argument `{other}`");
                    return ExitCode::FAILURE;
                }
            },
        }
    }
    match provenance::write(&repo_root(), output.as_deref()) {
        Ok(path) => {
            println!("build-manifest: wrote {}", path.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("build-manifest: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Writes or verifies the checksum manifest of a release directory (spec section 47.2, ADR-0528).
///
/// `--verify` is the half `sha256sum -c` cannot do: it also fails when an artifact is present and
/// unlisted, which is how an asset reaches a release unattested.
fn checksums(args: &[String]) -> ExitCode {
    let mut directory = PathBuf::from("dist");
    let mut tested: Option<PathBuf> = None;
    let mut verify = false;
    let mut rest = args.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--dir" => match rest.next() {
                Some(path) => directory = PathBuf::from(path),
                None => return usage_error("checksums: --dir needs a path"),
            },
            "--tested" => match rest.next() {
                Some(path) => tested = Some(PathBuf::from(path)),
                None => return usage_error("checksums: --tested needs a path"),
            },
            "--verify" => verify = true,
            other => match other.strip_prefix("--dir=") {
                Some(path) => directory = PathBuf::from(path),
                None => match other.strip_prefix("--tested=") {
                    Some(path) => tested = Some(PathBuf::from(path)),
                    None => return usage_error(&format!("checksums: unknown argument `{other}`")),
                },
            },
        }
    }

    if verify {
        let mut problems = provenance::check_checksums(&directory);
        if let Some(records) = &tested {
            problems.extend(provenance::check_tested_bytes(&directory, records));
        }
        if problems.is_empty() {
            println!(
                "checksums: {}/{} covers every artifact beside it{}",
                directory.display(),
                provenance::CHECKSUM_MANIFEST,
                if tested.is_some() {
                    ", and every package is the one validation installed"
                } else {
                    ""
                }
            );
            return ExitCode::SUCCESS;
        }
        for problem in &problems {
            eprintln!("checksums: {} — {}", problem.location, problem.detail);
        }
        return ExitCode::FAILURE;
    }

    match provenance::write_checksums(&directory) {
        Ok(path) => {
            println!("checksums: wrote {}", path.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("checksums: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Writes or verifies the build provenance of a release directory (spec section 47.4, ADR-0530).
fn provenance_task(args: &[String]) -> ExitCode {
    let mut directory = PathBuf::from("dist");
    let mut inputs: Option<PathBuf> = None;
    let mut verify = false;
    let mut rest = args.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--dir" => match rest.next() {
                Some(path) => directory = PathBuf::from(path),
                None => return usage_error("provenance: --dir needs a path"),
            },
            "--inputs" => match rest.next() {
                Some(path) => inputs = Some(PathBuf::from(path)),
                None => return usage_error("provenance: --inputs needs a path"),
            },
            "--verify" => verify = true,
            other => match other.strip_prefix("--dir=") {
                Some(path) => directory = PathBuf::from(path),
                None => match other.strip_prefix("--inputs=") {
                    Some(path) => inputs = Some(PathBuf::from(path)),
                    None => return usage_error(&format!("provenance: unknown argument `{other}`")),
                },
            },
        }
    }

    if verify {
        let problems = provenance::check_provenance(&directory);
        if problems.is_empty() {
            println!(
                "provenance: {}/{} binds every published artifact",
                directory.display(),
                provenance::PROVENANCE
            );
            return ExitCode::SUCCESS;
        }
        for problem in &problems {
            eprintln!("provenance: {} — {}", problem.location, problem.detail);
        }
        return ExitCode::FAILURE;
    }

    match provenance::write_provenance(&directory, inputs.as_deref()) {
        Ok(path) => {
            println!("provenance: wrote {}", path.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("provenance: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Compares two directories of build artifacts (spec section 46.5, ADR-0527).
///
/// The two directories come from `scripts/rebuild-check.sh`, which builds every artifact twice.
/// This half only reads bytes, so it can also be pointed at a published release and a local
/// rebuild of the same tag.
fn compare_builds(args: &[String]) -> ExitCode {
    let [first, second] = args else {
        eprintln!("compare-builds: needs exactly two directories");
        return ExitCode::FAILURE;
    };
    let (first, second) = (PathBuf::from(first), PathBuf::from(second));
    match reproducibility::compare(&first, &second) {
        Err(error) => {
            eprintln!("compare-builds: {error}");
            ExitCode::FAILURE
        }
        Ok(differences) if differences.is_empty() => {
            match reproducibility::inventory(&first) {
                Ok(inventory) => {
                    for (name, digest) in &inventory {
                        println!("compare-builds: {digest}  {name}");
                    }
                }
                Err(error) => {
                    eprintln!("compare-builds: {error}");
                    return ExitCode::FAILURE;
                }
            }
            println!(
                "compare-builds: {} and {} are byte-for-byte identical",
                first.display(),
                second.display()
            );
            ExitCode::SUCCESS
        }
        Ok(differences) => {
            for difference in &differences {
                eprintln!(
                    "compare-builds: {} — {}",
                    difference.artifact, difference.detail
                );
            }
            eprintln!(
                "compare-builds: {} artifact(s) differ between two builds of one commit; a \
                 release built from these inputs is not reproducible (spec section 46.1, 46.5)",
                differences.len()
            );
            ExitCode::FAILURE
        }
    }
}

/// Runs the performance benchmarks of v0.4.1 §37.1 and reports their §32.3 records.
///
/// > The repository SHOULD expose performance fixtures through `xtask` … Exact syntax MAY differ,
/// > but benchmark execution must be discoverable and reproducible.
///
/// Reproducible means three things here: the host is put at a declared cardinality before a
/// benchmark runs (§32.2, ADR-0488), the figure names the environment it was measured on (§37.2),
/// and it names how many iterations produced it (§37.4). A run against a debug binary may be
/// inspected and may not be written into the baseline — §37.2's environment includes the release
/// build flags, so a debug figure is a figure about a different build.
fn perf(args: &[String]) -> ExitCode {
    let mut profile: Option<String> = None;
    let mut iterations = perf::MIN_ITERATIONS;
    let mut compare: Option<PathBuf> = None;
    let mut write = false;
    let mut sample_completion = false;
    let mut sample_temporal: Option<String> = None;
    let mut sample_index = 0u32;
    let mut skip_temporal = false;

    let mut rest = args.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--profile" => match rest.next() {
                Some(name) => profile = Some(name.to_uppercase()),
                None => return usage_error("perf: --profile needs a name (S, M or L)"),
            },
            "--iterations" => match rest.next().and_then(|count| count.parse::<u32>().ok()) {
                Some(count) if count > 0 => iterations = count,
                _ => return usage_error("perf: --iterations needs a positive number"),
            },
            "--compare" => match rest.next() {
                Some(path) => compare = Some(PathBuf::from(path)),
                None => return usage_error("perf: --compare needs a path"),
            },
            "--write-baseline" => write = true,
            // One cold completion, printed for the parent that spawned this process. §36.2's
            // budget is about the *first* completion, and a completer caches what it read, so a
            // second sample in the same process would be a different measurement (§37.3).
            "--sample-completion" => sample_completion = true,
            // One sample of one of v0.5 §49's rows, printed for the parent that spawned this
            // process. §37.3: a temporal query is warm the instant it has been asked once, so a
            // sample is a whole process rather than an iteration inside one.
            "--sample-temporal" => match rest.next() {
                Some(name) => sample_temporal = Some(name.clone()),
                None => return usage_error("perf: --sample-temporal needs an operation"),
            },
            "--sample-index" => match rest.next().and_then(|count| count.parse::<u32>().ok()) {
                Some(count) => sample_index = count,
                None => return usage_error("perf: --sample-index needs a number"),
            },
            // v0.5 §49's fixture ledger takes minutes to write and hundreds of megabytes to hold.
            // A run that only wants the v0.4.1 rows says so rather than paying for it.
            "--skip-temporal" => skip_temporal = true,
            other => return usage_error(&format!("perf: unknown argument `{other}`")),
        }
    }

    if sample_completion {
        let (milliseconds, offered) = perf::sample_completion();
        println!("{milliseconds} {offered}");
        return ExitCode::SUCCESS;
    }

    if let Some(name) = sample_temporal {
        return sample_temporal_row(&name, sample_index);
    }

    let root = repo_root();
    let environment = match perf::reference_environment(&root) {
        Ok(environment) => environment,
        Err(error) => {
            eprintln!("perf: {error}");
            return ExitCode::FAILURE;
        }
    };
    let (binary, build) = match built_binary(&root) {
        Some(found) => found,
        None => {
            eprintln!("perf: no `ono` binary is built; run `cargo build --release` first");
            return ExitCode::FAILURE;
        }
    };
    if write && build != "release" {
        eprintln!(
            "perf: --write-baseline needs a release build. v0.4.1 §37.2 names the release build \
             flags as part of the reference environment, so a debug figure is a figure about a \
             different build"
        );
        return ExitCode::FAILURE;
    }

    let commit = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&root)
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().to_owned())
        .unwrap_or_default();

    let runner = perf::Runner::new(binary, environment.id.clone(), commit)
        .iterations(iterations)
        .build(&build);

    println!(
        "perf: {} build on `{}`, {iterations} iterations (v0.4.1 section 37.4 wants at least {})",
        build,
        environment.id,
        perf::MIN_ITERATIONS
    );

    // Profile `T` is v0.5 §49's fixture ledger rather than one of Appendix F's host topologies,
    // so naming it asks for the temporal table and nothing else.
    let temporal_only = profile.as_deref() == Some("T");
    let wanted: Vec<&perf::Benchmark> = if temporal_only {
        Vec::new()
    } else {
        perf::BENCHMARKS
            .iter()
            .filter(|benchmark| {
                profile
                    .as_deref()
                    .is_none_or(|name| benchmark.profile == name)
            })
            .collect()
    };
    if wanted.is_empty() && !temporal_only {
        eprintln!("perf: no declared benchmark runs at Profile {profile:?}");
        return ExitCode::FAILURE;
    }

    let mut measurements = Vec::new();
    for benchmark in wanted {
        // The host is held at the declared cardinality for the whole of the benchmark, and the
        // population is dropped — killed and reaped — before the next one starts.
        let Some(declaration) = ono_testkit::declared_profiles()
            .into_iter()
            .find(|declaration| declaration.id == benchmark.profile)
        else {
            eprintln!("perf: no profile is declared as `{}`", benchmark.profile);
            return ExitCode::FAILURE;
        };
        let at = declaration.profile();
        // Each axis is built where its declaration says it can be: Profile L's ten thousand
        // processes are the container's and its hundred thousand sockets are not (ADR-0488).
        let processes = (declaration.built_by != ono_testkit::BuiltBy::Container)
            .then(|| ono_testkit::ProcessPopulation::of(at));
        let sockets = (declaration.sockets_built_by != ono_testkit::BuiltBy::Container)
            .then(|| ono_testkit::SocketPopulation::of(at));
        let measured = runner.run(benchmark);
        drop(sockets);
        drop(processes);

        println!(
            "  {:<28} {:<3} {:<9} first {:>9.3} ms  p95 {:>9.3} ms  complete {:>9.3} ms",
            measured.benchmark,
            measured.profile,
            measured.temperature.as_str(),
            measured.metric("time_to_first_ms").unwrap_or_default(),
            measured.p95_ms,
            measured.metric("time_to_complete_ms").unwrap_or_default(),
        );
        measurements.push(measured);
    }

    // v0.5 §49's eight release measurements, over §49's deterministic fixture ledger. They are
    // skipped when a profile was named, because §49's ledger is not a topology profile and a
    // `--profile M` run is asking about the host rather than about the history.
    if !skip_temporal && (profile.is_none() || temporal_only) {
        match temporal_measurements(&root, &runner) {
            Ok(mut found) => measurements.append(&mut found),
            Err(error) => {
                eprintln!("perf: {error}");
                return ExitCode::FAILURE;
            }
        }
    }

    // §36.2's completion budget, measured by calling the completer rather than by timing a
    // thousand registry lookups beside it (issue #21, ADR-0498). One cold sample per process, so
    // the samples are re-runs of this executable rather than iterations in it.
    if !temporal_only {
        let measured = runner.run_completion();
        println!(
            "  {:<28} {:<3} {:<9} first {:>9.3} ms  p95 {:>9.3} ms  candidates {}",
            measured.benchmark,
            measured.profile,
            measured.temperature.as_str(),
            measured.metric("time_to_first_ms").unwrap_or_default(),
            measured.p95_ms,
            measured.values,
        );
        measurements.push(measured);
    }

    let mut failed = false;
    if let Some(path) = compare {
        match std::fs::read_to_string(&path)
            .map_err(|error| error.to_string())
            .and_then(|text| {
                perf::Baseline::parse(&text).map_err(|problems| {
                    problems
                        .into_iter()
                        .map(|problem| problem.detail)
                        .collect::<Vec<_>>()
                        .join("; ")
                })
            }) {
            Ok(baseline) => {
                for measured in &measurements {
                    match baseline.compare(measured, perf::Tolerance::Absolute) {
                        perf::Comparison::Held => {}
                        other => {
                            println!("perf: {} — {other:?}", measured.benchmark);
                            if matches!(other, perf::Comparison::Regressed(_)) {
                                failed = true;
                            }
                        }
                    }
                }
            }
            Err(error) => {
                eprintln!("perf: cannot compare against {}: {error}", path.display());
                return ExitCode::FAILURE;
            }
        }
    }

    if write && profile.is_some() {
        eprintln!(
            "perf: --write-baseline writes the whole baseline, so it cannot be combined with \
             --profile: the run would replace every record with the subset it measured. Run \
             without --profile, or compare with --compare instead"
        );
        return ExitCode::FAILURE;
    }
    if write {
        let path = root.join(perf::BASELINE);
        if let Err(error) = perf::write_baseline(&path, environment.id, &measurements) {
            eprintln!("perf: {error}");
            return ExitCode::FAILURE;
        }
        println!("perf: wrote {}", path.display());
    }

    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Builds v0.5 §49's fixture ledger and measures every row of [`perf::TEMPORAL_BENCHMARKS`]
/// against it.
///
/// A row this repository cannot measure yet reports why and contributes no record, which leaves
/// the target it answers `Unmeasured` — and `perf::verdicts` reports an unmeasured target as a
/// failure rather than a pass (§65.10). That is the honest state of a measurement whose subject
/// has not been written.
fn temporal_measurements(
    root: &Path,
    runner: &perf::Runner,
) -> Result<Vec<perf::Measurement>, String> {
    let declaration = ono_testkit::temporal::declared_temporal_profiles()
        .into_iter()
        .next()
        .ok_or_else(|| {
            "docs/contracts/hardening/performance_profiles.yaml declares no temporal profile \
             (v0.5 section 49)"
                .to_owned()
        })?;
    let profile = declaration.profile();
    println!(
        "perf: v0.5 section 49 fixture ledger — {} events, {} objects, {} relation changes, {} \
         actions, seed {}",
        profile.events, profile.objects, profile.relation_changes, profile.actions, profile.seed
    );
    let fixture = perf::fixture::build(profile, &root.join("target").join("perf"))?;
    println!(
        "  fixture {} — {} events, {} evidence, {} actions, {} checkpoints, {:.1} MiB on disk, \
         built in {:.1} s{}, digest {}",
        fixture.path().display(),
        fixture.events(),
        fixture.evidence(),
        fixture.actions(),
        fixture.checkpoints(),
        fixture.bytes() as f64 / (1024.0 * 1024.0),
        fixture.built_in().as_secs_f64(),
        if fixture.reused() { " (reused)" } else { "" },
        fixture.digest(),
    );

    let mut measurements = Vec::new();
    for benchmark in perf::TEMPORAL_BENCHMARKS {
        match runner.run_temporal(benchmark, &fixture) {
            Ok(measured) => {
                println!(
                    "  {:<28} {:<3} {:<9} first {:>9.3} ms  p95 {:>9.3} ms  values {}",
                    measured.benchmark,
                    measured.profile,
                    measured.temperature.as_str(),
                    measured.metric("time_to_first_ms").unwrap_or_default(),
                    measured.p95_ms,
                    measured.values,
                );
                measurements.push(measured);
            }
            Err(reason) => println!("  {:<28} {:<3} unmeasured — {reason}", benchmark.id, "T"),
        }
    }
    Ok(measurements)
}

/// Takes one sample of one v0.5 §49 row and prints `<milliseconds> <values> <peak_rss_bytes>`.
fn sample_temporal_row(name: &str, index: u32) -> ExitCode {
    let Some(operation) = perf::TemporalOperation::from_name(name) else {
        return usage_error(&format!("perf: no temporal benchmark measures `{name}`"));
    };
    let root = repo_root();
    let Some(declaration) = ono_testkit::temporal::declared_temporal_profiles()
        .into_iter()
        .next()
    else {
        eprintln!("perf: no temporal profile is declared (v0.5 section 49)");
        return ExitCode::FAILURE;
    };
    let fixture =
        match perf::fixture::build(declaration.profile(), &root.join("target").join("perf")) {
            Ok(fixture) => fixture,
            Err(error) => {
                eprintln!("perf: {error}");
                return ExitCode::FAILURE;
            }
        };
    let binary = built_binary(&root).map(|(path, _)| path);
    match perf::sample::take(operation, index, &fixture, binary.as_deref()) {
        Ok(sample) => {
            println!(
                "{} {} {}",
                sample.elapsed_ms,
                sample.values,
                peak_rss_of_self().unwrap_or(0)
            );
            ExitCode::SUCCESS
        }
        Err(reason) => {
            eprintln!("perf: {reason}");
            ExitCode::FAILURE
        }
    }
}

/// This process's peak resident set, or `None` where `/proc` does not say.
fn peak_rss_of_self() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .and_then(|value| value.split_whitespace().next()?.parse::<u64>().ok())
        .map(|kib| kib * 1024)
}

/// The built `ono` binary this run should measure, preferring the release one.
fn built_binary(root: &Path) -> Option<(PathBuf, String)> {
    for build in ["release", "debug"] {
        let candidate = root.join("target").join(build).join("ono");
        if candidate.is_file() {
            return Some((candidate, build.to_owned()));
        }
    }
    None
}

/// A usage mistake, reported the way the other tasks report theirs.
fn usage_error(message: &str) -> ExitCode {
    eprintln!("{message}");
    ExitCode::FAILURE
}

/// Regenerates `docs/reference/` from the machine-readable contracts (spec section 36.2).
fn generate_docs() -> ExitCode {
    match reference::write(&repo_root()) {
        Ok(written) => {
            for path in written {
                println!("docs: wrote {path}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("docs: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Regenerates the third-party licence notices from the lockfile (ADR-0608).
fn licenses(arguments: &[String]) -> ExitCode {
    let root = repo_root();
    let write = match arguments.first().map(String::as_str) {
        None => false,
        Some("--write") => true,
        Some(other) => return usage_error(&format!("licenses: unknown argument `{other}`")),
    };
    if !write {
        let problems = notices::check_committed(&root);
        for problem in &problems {
            eprintln!("licenses: {} — {}", problem.location, problem.detail);
        }
        if problems.is_empty() {
            println!("licenses: {} agrees with Cargo.lock", notices::NOTICES_FILE);
            return ExitCode::SUCCESS;
        }
        return ExitCode::FAILURE;
    }
    match notices::write(&root) {
        Ok(true) => {
            println!("licenses: wrote {}", notices::NOTICES_FILE);
            ExitCode::SUCCESS
        }
        Ok(false) => {
            println!("licenses: {} is already current", notices::NOTICES_FILE);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("licenses: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Regenerates the provider conformance suite from the registries (spec section 35.3).
fn generate_conformance() -> ExitCode {
    match conformance::write(&repo_root()) {
        Ok(written) => {
            for path in written {
                println!("conformance: wrote {path}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("conformance: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Checks the contracts that exist today against the rules in AGENTS.md.
///
/// The registries of spec section 47 do not exist yet. Until they do, this task enforces the
/// invariants that already apply, so it can never silently pass once contracts appear.
fn spec_check() -> ExitCode {
    let root = repo_root();
    let mut problems: Vec<String> = Vec::new();

    if root.join("spec").is_dir() {
        problems.push(
            "a top-level `spec/` directory exists; agent-readable contracts belong in `docs/contracts/` (AGENTS.md section 2)".to_owned(),
        );
    }

    problems.extend(check_spec_is_untouched(&root));

    let state = std::fs::read_to_string(root.join("docs").join("STATE.md")).unwrap_or_default();
    if state.is_empty() {
        problems.push(
            "docs/STATE.md is missing; it is the shared work board (AGENTS.md §9)".to_owned(),
        );
    }
    problems.extend(
        scan::check_unfinished_work(&root, &state)
            .into_iter()
            .chain(scan::check_acceptance_case_references(&root))
            .chain(scan::check_silent_skips(&root))
            .chain(scan::check_unannounced_skips(&root))
            .chain(scan::check_expected_skips(&root))
            .chain(scan::check_duplicate_helpers(&root))
            .chain(scan::check_release_notes(&root))
            .chain(scan::check_pty_resize_assertions(&root))
            .chain(scan::check_confinement_syscalls(&root))
            .chain(scan::check_evaluator_captures(&root))
            .chain(scan::check_bounded_channels(&root))
            .chain(scan::check_authentication_flags(&root))
            .chain(terminology::check_documents(&root))
            .chain(terminology::check_decisions(&root))
            .chain(architecture::check(&root))
            .chain(temporal::check(&root))
            .chain(change::check(&root))
            .map(|problem| format!("{} — {}", problem.location, problem.detail)),
    );

    problems.extend(
        supply_chain::check_action_pins(&root)
            .into_iter()
            .chain(supply_chain::check_image_digests(&root))
            .chain(supply_chain::check_workflow_permissions(&root))
            .chain(supply_chain::check_dependency_policy(&root))
            .chain(supply_chain::check_dependency_justifications(&root))
            .chain(supply_chain::check_tool_versions(&root))
            .chain(supply_chain::check_locked_builds(&root))
            .chain(notices::check_committed(&root))
            .chain(provenance::check_manifest_is_emitted(&root))
            .map(|problem| format!("{} — {}", problem.location, problem.detail)),
    );

    problems.extend(
        narrative::check(&root)
            .into_iter()
            .chain(narrative::check_readme_examples(&root))
            .chain(verification::check_sequence())
            .chain(check_release_verification_documents(&root))
            .chain(reference::check_migration_guide(&root))
            .chain(baseline::check(&root))
            .chain(repo_metrics::check_readme(&root))
            .map(|problem| format!("{} — {}", problem.location, problem.detail)),
    );

    problems.extend(check_command_bindings());
    problems.extend(check_generation_claims(&root));

    if root.join("docs").join("contracts").is_dir() {
        problems.extend(
            contracts::check_contracts(&root)
                .into_iter()
                .chain(contracts::check_examples(&root))
                .chain(reference::check_committed(&root))
                .chain(conformance::check_committed(&root))
                .map(|problem| format!("{} — {}", problem.location, problem.detail)),
        );
    } else {
        println!("spec-check: docs/contracts/ does not exist yet (expected before phase D)");
    }

    if problems.is_empty() {
        println!("spec-check: ok");
        ExitCode::SUCCESS
    } else {
        for problem in &problems {
            eprintln!("spec-check: {problem}");
        }
        ExitCode::FAILURE
    }
}

/// Checks the claims `docs/ACCEPTANCE.md` makes about the work board (ADR-0402).
///
/// Separate from `spec-check` on purpose: three release boxes assert that nobody is in the middle
/// of changing the shell, and that is a statement about the moment of release, not about an
/// increment. A gate that refused a held claim would forbid the working rhythm of AGENTS.md §7,
/// so `scripts/release-check.sh` runs this and the gate does not.
fn state_check() -> ExitCode {
    let root = repo_root();
    let Ok(state) = std::fs::read_to_string(root.join("docs").join("STATE.md")) else {
        eprintln!(
            "state-check: docs/STATE.md is missing; it is the shared work board (AGENTS.md §9)"
        );
        return ExitCode::FAILURE;
    };
    let problems = scan::check_release_board(&state);
    if problems.is_empty() {
        println!("state-check: ok");
        return ExitCode::SUCCESS;
    }
    for problem in &problems {
        eprintln!("state-check: {} — {}", problem.location, problem.detail);
    }
    ExitCode::FAILURE
}

/// The skip-verification step of v0.4.1 §38.3.
///
/// §38.3 makes the gate bidirectional: *"A test that becomes skipped when it was expected to run
/// MUST fail the CI gate or an explicit skip-verification step."* This is that step. It reads a
/// test run's output and compares the `SKIPPED` markers in it against
/// `docs/contracts/hardening/expected_test_skips.yaml`, in both directions — an undeclared skip fails,
/// and a declared skip that did not happen fails too.
///
/// It is a separate task rather than part of `spec-check` because it needs an observation: the
/// gate's static half already refuses a skip the registry does not declare, and only a run can
/// say which of them actually happened.
fn skip_check(arguments: &[String]) -> ExitCode {
    let Some(log_path) = arguments.first() else {
        return usage_error(
            "skip-check: name the file holding a test run's output, as in \
             `cargo test --workspace --all-features 2>&1 | tee test.log`",
        );
    };
    let root = repo_root();
    let expected = match scan::ExpectedSkips::read(&root) {
        Ok(expected) => expected,
        Err(message) => {
            eprintln!("skip-check: {message}");
            return ExitCode::FAILURE;
        }
    };
    let log = match std::fs::read_to_string(log_path) {
        Ok(log) => log,
        Err(error) => {
            eprintln!("skip-check: cannot read {log_path}: {error}");
            return ExitCode::FAILURE;
        }
    };
    let problems = scan::verify_observed_skips(&expected, &log);
    if problems.is_empty() {
        println!(
            "skip-check: ok — {} declared skip(s) observed, none undeclared",
            expected.canonical_ci.len()
        );
        return ExitCode::SUCCESS;
    }
    for problem in &problems {
        eprintln!("skip-check: {} — {}", problem.location, problem.detail);
    }
    ExitCode::FAILURE
}

/// The documentation terminology contract of v0.4.1 §19.1, run on demand.
///
/// `spec-check` already holds every surface a gate run can reach — the repository's user-facing
/// documents, every rendered `help` page, every generated reference page and the accepted decision
/// records. **The Wiki is a separate git repository**, so no gate run can reach it: this task takes
/// the checkout as an argument, which is the only honest way to check it (ADR-0536).
fn terminology(arguments: &[String]) -> ExitCode {
    let root = repo_root();
    let mut wiki: Option<PathBuf> = None;
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--wiki" => match rest.next() {
                Some(path) => wiki = Some(PathBuf::from(path)),
                None => return usage_error("terminology: --wiki needs the path of a checkout"),
            },
            other => match other.strip_prefix("--wiki=") {
                Some(path) => wiki = Some(PathBuf::from(path)),
                None => return usage_error(&format!("terminology: unknown argument `{other}`")),
            },
        }
    }

    let mut problems = terminology::check_documents(&root);
    problems.extend(terminology::check_decisions(&root));
    match wiki.as_deref() {
        Some(checkout) => {
            problems.extend(terminology::check_wiki(checkout));
            problems.extend(terminology::check_wiki_remote_trust(checkout));
            let install = checkout.join("Install.md");
            match std::fs::read_to_string(&install) {
                Ok(text) => problems.extend(verification::check_document("Install.md", &text)),
                Err(error) => eprintln!(
                    "terminology: Install.md cannot be read from the named Wiki checkout: {error}"
                ),
            }
        }
        None => println!(
            "terminology: no --wiki given, so the Wiki is unchecked. It is a separate git \
             repository and the gate cannot reach it (v0.4.1 section 19.1, ADR-0536)"
        ),
    }

    if problems.is_empty() {
        println!(
            "terminology: ok — {} term(s) of section 19.1 held across every surface checked",
            terminology::terms().len()
        );
        return ExitCode::SUCCESS;
    }
    for problem in &problems {
        eprintln!("terminology: {} — {}", problem.location, problem.detail);
    }
    ExitCode::FAILURE
}

/// The generated repository metrics of v0.4.1 §50.
///
/// §50.2 asks `xtask` to compute the volatile counts, and §50.3 lets the README keep them as long
/// as the gate fails when they disagree — which `spec-check` does. This task prints them, and
/// `--write` puts them back into the README's generated block.
fn metrics(arguments: &[String]) -> ExitCode {
    let root = repo_root();
    let write = match arguments.first().map(String::as_str) {
        None => false,
        Some("--write") => true,
        Some(other) => return usage_error(&format!("metrics: unknown argument `{other}`")),
    };
    print!("{}", repo_metrics::measure(&root).render());
    if !write {
        return ExitCode::SUCCESS;
    }
    match repo_metrics::write_readme(&root) {
        Ok(true) => println!("metrics: README.md updated"),
        Ok(false) => println!("metrics: README.md already agrees"),
        Err(error) => {
            eprintln!("metrics: {error}");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

/// The documents that carry v0.4.1 §47.5's verification sequence, held against the registry.
///
/// The generated page is compared by `reference::check_committed` like every other generated
/// page; this is the hand-written copy. The Wiki's is `cargo xtask terminology --wiki <path>`'s,
/// for the reason ADR-0536 records.
fn check_release_verification_documents(root: &Path) -> Vec<scan::Problem> {
    match std::fs::read_to_string(root.join("README.md")) {
        Ok(text) => verification::check_document("README.md", &text),
        Err(_) => Vec::new(),
    }
}

/// Every "generated from" claim in `docs/ACCEPTANCE.md` names something that is generated.
///
/// The checklist is the definition of done; a box describing machinery nobody built is a claim
/// the reader has no reason to doubt and no way to check.
fn check_generation_claims(root: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(root.join("docs").join("ACCEPTANCE.md")) else {
        return vec!["docs/ACCEPTANCE.md is missing; it is the definition of done".to_owned()];
    };
    let mut generated: Vec<String> = reference::generate(root)
        .map(|pages| pages.into_iter().map(|page| page.path).collect())
        .unwrap_or_default();
    generated.extend(
        conformance::generate(root)
            .map(|pages| pages.into_iter().map(|page| page.path).collect::<Vec<_>>())
            .unwrap_or_default(),
    );
    reference::check_generation_claims(&text, &generated)
        .into_iter()
        .map(|problem| format!("{} — {}", problem.location, problem.detail))
        .collect()
}

/// Spec §27.2: every stable command of a delivered phase is bound to an implementation.
///
/// The registry is written before the code, so a stable command with nothing behind it is drift
/// the contract alone cannot show. The list of deliberate exceptions lives beside the check.
fn check_command_bindings() -> Vec<String> {
    let Ok(registry) = ono_command::CommandRegistry::embedded() else {
        return vec![
            "the embedded command contracts do not parse, so spec §27.2 cannot be checked"
                .to_owned(),
        ];
    };
    let table = ono_command::builtin_commands(registry);
    bindings::check_bindings(registry, |id| table.contains(id))
        .into_iter()
        .map(|problem| format!("{} — {}", problem.location, problem.detail))
        .collect()
}

/// The regression baseline of v0.4.1 §32.4 is a set of complete §32.3 results.
///
/// §32.3: *"A single total runtime number is insufficient for streaming operations."* A baseline
/// holding a record that dropped one of the six metrics is a baseline a later run cannot be
/// compared against on that metric, and nothing would say so — the comparison would simply skip
/// it and report "held". So the file is parsed on every gate run, and a record that is not a
/// benchmark result turns the gate red where it was written rather than where it is read.
/// Verifies that the immutable narrative specification has not been modified.
///
/// The specification is read-only for every agent (AGENTS.md section 5.1): ambiguities are
/// resolved in ADRs, never by editing the source of truth. A written rule is easy to forget
/// halfway through a long run, so the rule is checked rather than trusted.
fn check_spec_is_untouched(root: &Path) -> Vec<String> {
    let checksum = root.join("docs").join("specs").join("spec.sha256");
    if !checksum.is_file() {
        return vec![
            "docs/specs/spec.sha256 is missing; the specification can no longer be proven untouched"
                .to_owned(),
        ];
    }

    let output = Command::new("sha256sum")
        .arg("--check")
        .arg("--status")
        .arg(&checksum)
        .current_dir(root)
        .status();

    match output {
        Ok(status) if status.success() => Vec::new(),
        Ok(_) => vec![
            "the narrative specification has been modified. It is IMMUTABLE (AGENTS.md \
             section 5.1): restore it with `git checkout -- docs/specs/ono_sendai_*spec_v*.md` and \
             record the decision in an ADR instead. If the user replaced the specification \
             deliberately, they update docs/specs/spec.sha256"
                .to_owned(),
        ],
        Err(error) => vec![format!(
            "cannot verify the specification checksum: {error}. `sha256sum` must be available"
        )],
    }
}
