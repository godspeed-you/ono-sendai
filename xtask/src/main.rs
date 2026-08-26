//! Repository automation.
//!
//! `cargo xtask <task>` is the single entry point an agent uses to verify its work. Every task
//! is also runnable as a plain script so it works identically in CI and inside a container.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use xtask::{contracts, scan};

fn main() -> ExitCode {
    let task = std::env::args().nth(1);
    let rest: Vec<String> = std::env::args().skip(2).collect();

    match task.as_deref() {
        Some("gate") => run_script("gate.sh", &rest),
        Some("acceptance") => run_script("acceptance.sh", &rest),
        Some("spec-check") => spec_check(),
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
            .map(|problem| format!("{} — {}", problem.location, problem.detail)),
    );

    let spec_docs = find_narrative_spec(&root);
    match spec_docs.as_slice() {
        [] => problems.push("no narrative specification found under docs/".to_owned()),
        [single] => {
            for file in ["AGENTS.md", "CLAUDE.md", "README.md"] {
                let Ok(text) = std::fs::read_to_string(root.join(file)) else {
                    problems.push(format!("{file} is missing"));
                    continue;
                };
                if !text.contains(single) {
                    problems.push(format!(
                        "{file} does not reference the current specification `{single}`"
                    ));
                }
            }
        }
        many => problems.push(format!(
            "several narrative specifications found ({}); exactly one is authoritative",
            many.join(", ")
        )),
    }

    if root.join("docs").join("spec").is_dir() {
        problems.extend(
            contracts::check_contracts(&root)
                .into_iter()
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
             section 5.1): restore it with `git checkout -- docs/*_shell_spec_*.md` and record \
             the decision in an ADR instead. If the user replaced the specification \
             deliberately, they update docs/spec.sha256"
                .to_owned(),
        ],
        Err(error) => vec![format!(
            "cannot verify the specification checksum: {error}. `sha256sum` must be available"
        )],
    }
}

fn find_narrative_spec(root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root.join("docs")) else {
        return Vec::new();
    };
    let mut found: Vec<String> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.contains("shell_spec") && name.ends_with(".md"))
        .collect();
    found.sort();
    found
}
