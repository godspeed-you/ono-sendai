//! The release evidence of `docs/ACCEPTANCE.md` §4.6.5: every first-party adapter is exercised
//! live in the container, and the README's examples of the adapter layer parse and run.

#![allow(
    clippy::panic,
    clippy::expect_used,
    reason = "AGENTS.md §16: a helper shared by tests states its preconditions the same way a test does"
)]

use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask sits in the workspace")
        .to_path_buf()
}

#[test]
fn should_have_a_live_acceptance_case_for_every_first_party_adapter() {
    // Spec v0.3 §1.48: first-party adapters run against the real tools in a container; a
    // contract nobody runs live is a claim, not a proof.
    let cases_dir = repo().join("docker/acceptance/cases");
    let cases: Vec<String> = std::fs::read_dir(&cases_dir)
        .expect("the acceptance cases exist")
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "case"))
        .map(|entry| std::fs::read_to_string(entry.path()).expect("a case is readable"))
        .collect();
    let mut missing = Vec::new();
    for pack in ono_adapter::first_party() {
        for adapter in pack.adapters() {
            let id = adapter.full_id();
            if !cases.iter().any(|case| case.contains(&id)) {
                missing.push(id);
            }
        }
    }
    assert!(
        missing.is_empty(),
        "no acceptance case names these adapters: {missing:?}"
    );
}

fn readme_examples() -> Vec<String> {
    let readme = std::fs::read_to_string(repo().join("README.md")).expect("README.md exists");
    xtask::narrative::ono_examples(&readme)
}

#[test]
fn should_find_ono_examples_in_the_readme_that_parse() {
    let examples = readme_examples();
    assert!(
        examples.len() >= 5,
        "the README shows the adapter layer in runnable `ono` fences; found {examples:?}"
    );
    for example in &examples {
        let parsed = ono_parser::parse(example);
        assert!(
            !parsed.has_errors() && parsed.is_complete(),
            "README example does not parse: {example}"
        );
    }
}

#[test]
fn should_run_every_readme_example_of_the_adapter_layer() {
    // The README's adapter examples are run with the real binary, in the repository, so a
    // documented line that stopped working fails the gate rather than the reader.
    for example in readme_examples() {
        let output = std::process::Command::new(ono_testkit::ono_binary())
            .args(["--no-config", "-c", &example])
            .current_dir(repo())
            .env("NO_COLOR", "1")
            .output()
            .expect("the ono binary runs");
        let stderr = String::from_utf8_lossy(&output.stderr);
        // A tool absent on this machine is the machine's business (the container has them all,
        // case 088); anything else is the example's.
        if stderr.contains("Ono-Sendai-E0101") {
            continue;
        }
        assert!(
            output.status.success(),
            "README example `{example}` failed: {stderr}"
        );
    }
}

#[test]
fn should_report_a_readme_example_that_does_not_parse() {
    let broken = "# Title\n\n```ono\nget process | where (\n```\n";
    let problems = xtask::narrative::check_examples_in(broken, "README.md");
    assert_eq!(
        problems.len(),
        1,
        "one broken example, one problem: {problems:?}"
    );
    assert!(problems[0].location.contains("README.md"));
}
