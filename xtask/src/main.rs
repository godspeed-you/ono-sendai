//! Repository automation.
//!
//! `cargo xtask <task>` is the single entry point an agent uses to verify its work. Every task
//! is also runnable as a plain script so it works identically in CI and inside a container.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use xtask::{
    bindings, conformance, contracts, narrative, perf, provenance, reference, scan, supply_chain,
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
        Some("build-manifest") => build_manifest(&rest),
        Some("perf") => perf(&rest),
        Some("docs") => generate_docs(),
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

fn usage() {
    eprintln!("usage: cargo xtask <task>");
    eprintln!();
    eprintln!("tasks:");
    eprintln!("  gate           format, lint, test, contract check, docs (AGENTS.md section 10)");
    eprintln!("  spec-check     contract drift between docs/spec and the implementation");
    eprintln!("  state-check    the claims docs/ACCEPTANCE.md makes about docs/STATE.md");
    eprintln!(
        "  skip-check     a test log's SKIPPED markers against the declared expectation \
(spec section 38.3) <log>"
    );
    eprintln!("  build-manifest write the release input manifest of Appendix H [--output <path>]");
    eprintln!(
        "  perf           run the performance benchmarks of spec section 37.1 \
[--profile S|M|L] [--iterations N] [--compare <path>] [--write-baseline]"
    );
    eprintln!(
        "  terminology    the documentation terminology contract of section 19.1 over this \
repository, and over a Wiki checkout when one is named [--wiki <path>]"
    );
    eprintln!("  docs           regenerate docs/reference/ from the contracts (spec section 36.2)");
    eprintln!(
        "  conformance    regenerate the provider conformance suite from docs/spec (spec section 35.3)"
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
            other => return usage_error(&format!("perf: unknown argument `{other}`")),
        }
    }

    if sample_completion {
        let (milliseconds, offered) = perf::sample_completion();
        println!("{milliseconds} {offered}");
        return ExitCode::SUCCESS;
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

    let wanted: Vec<&perf::Benchmark> = perf::BENCHMARKS
        .iter()
        .filter(|benchmark| {
            profile
                .as_deref()
                .is_none_or(|name| benchmark.profile == name)
        })
        .collect();
    if wanted.is_empty() {
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

    // §36.2's completion budget, measured by calling the completer rather than by timing a
    // thousand registry lookups beside it (issue #21, ADR-0498). One cold sample per process, so
    // the samples are re-runs of this executable rather than iterations in it.
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
            "a top-level `spec/` directory exists; agent-readable contracts belong in `docs/spec/` (AGENTS.md section 2)".to_owned(),
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
            .chain(scan::check_pty_resize_assertions(&root))
            .chain(scan::check_confinement_syscalls(&root))
            .chain(scan::check_evaluator_captures(&root))
            .chain(scan::check_bounded_channels(&root))
            .chain(scan::check_authentication_flags(&root))
            .chain(terminology::check_documents(&root))
            .chain(terminology::check_decisions(&root))
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
            .chain(provenance::check_manifest_is_emitted(&root))
            .map(|problem| format!("{} — {}", problem.location, problem.detail)),
    );

    problems.extend(
        narrative::check(&root)
            .into_iter()
            .chain(narrative::check_readme_examples(&root))
            .chain(verification::check_sequence())
            .chain(check_release_verification_documents(&root))
            .map(|problem| format!("{} — {}", problem.location, problem.detail)),
    );

    problems.extend(check_command_bindings());
    problems.extend(check_generation_claims(&root));
    problems.extend(check_performance_baseline(&root));

    if root.join("docs").join("spec").is_dir() {
        problems.extend(
            contracts::check_contracts(&root)
                .into_iter()
                .chain(contracts::check_examples(&root))
                .chain(reference::check_committed(&root))
                .chain(conformance::check_committed(&root))
                .map(|problem| format!("{} — {}", problem.location, problem.detail)),
        );
    } else {
        println!("spec-check: docs/spec/ does not exist yet (expected before phase D)");
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
/// `docs/spec/hardening/expected_test_skips.yaml`, in both directions — an undeclared skip fails,
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
fn check_performance_baseline(root: &Path) -> Vec<String> {
    let path = root.join(perf::BASELINE);
    let Ok(text) = std::fs::read_to_string(&path) else {
        // The baseline arrives with the benchmark command; a missing one is not an error
        // (AGENTS.md section 14).
        return Vec::new();
    };
    match perf::Baseline::parse(&text) {
        Ok(_) => Vec::new(),
        Err(problems) => problems
            .into_iter()
            .map(|problem| format!("{} — {}", problem.location, problem.detail))
            .collect(),
    }
}

/// Verifies that the immutable narrative specification has not been modified.
///
/// The specification is read-only for every agent (AGENTS.md section 5.1): ambiguities are
/// resolved in ADRs, never by editing the source of truth. A written rule is easy to forget
/// halfway through a long run, so the rule is checked rather than trusted.
fn check_spec_is_untouched(root: &Path) -> Vec<String> {
    let checksum = root.join("docs").join("spec.sha256");
    if !checksum.is_file() {
        return vec![
            "docs/spec.sha256 is missing; the specification can no longer be proven untouched"
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
             section 5.1): restore it with `git checkout -- docs/ono_sendai_*spec_v*.md` and \
             record the decision in an ADR instead. If the user replaced the specification \
             deliberately, they update docs/spec.sha256"
                .to_owned(),
        ],
        Err(error) => vec![format!(
            "cannot verify the specification checksum: {error}. `sha256sum` must be available"
        )],
    }
}
