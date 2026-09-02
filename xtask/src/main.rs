//! Repository automation.
//!
//! `cargo xtask <task>` is the single entry point an agent uses to verify its work. Every task
//! is also runnable as a plain script so it works identically in CI and inside a container.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use xtask::{
    bindings, conformance, contracts, narrative, provenance, reference, scan, supply_chain,
    terminology,
};

fn main() -> ExitCode {
    let task = std::env::args().nth(1);
    let rest: Vec<String> = std::env::args().skip(2).collect();

    match task.as_deref() {
        Some("gate") => run_script("gate.sh", &rest),
        Some("acceptance") => run_script("acceptance.sh", &rest),
        Some("spec-check") => spec_check(),
        Some("state-check") => state_check(),
        Some("build-manifest") => build_manifest(&rest),
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
    eprintln!("  build-manifest write the release input manifest of Appendix H [--output <path>]");
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
            .map(|problem| format!("{} — {}", problem.location, problem.detail)),
    );

    problems.extend(check_command_bindings());
    problems.extend(check_generation_claims(&root));

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
