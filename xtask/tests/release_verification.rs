//! The release verification sequence is documented, consistent and actually runs (issue #115,
//! v0.4.1 §47.1, §47.5, §67.7).
//!
//! §47.5 asks for instructions. The test that matters is the one that *runs* them: a documented
//! sequence nobody has executed is a sequence that works until the day somebody needs it.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "AGENTS.md §16: a test states its preconditions directly"
)]

use std::path::Path;
use std::process::Command;

use ono_testkit::{Scratch, scratch};
use xtask::verification::{Step, check_document, check_sequence, sequence};

mod support;
use support::repo;

/// A release directory holding two artifacts and a `SHA256SUMS` over them.
///
/// Not a mock of the manifest format: `sha256sum` writes it, and `sha256sum --check` reads it, so
/// the fixture and the documented command agree by construction rather than by assertion.
fn release() -> Scratch {
    let dir = scratch();
    dir.write("release/ono_0.4.1_amd64.deb", "the package bytes\n");
    dir.write(
        "release/ono-0.4.1-1.x86_64.rpm",
        "the other package bytes\n",
    );
    let manifest = Command::new("sh")
        .arg("-c")
        .arg("cd release && sha256sum ono_0.4.1_amd64.deb ono-0.4.1-1.x86_64.rpm")
        .current_dir(dir.path())
        .output()
        .expect("sha256sum runs");
    assert!(manifest.status.success(), "the fixture manifest is written");
    dir.write(
        "release/SHA256SUMS",
        String::from_utf8(manifest.stdout).expect("sha256sum writes text"),
    );
    dir
}

/// Runs one documented step in `directory`, exactly as the registry writes it.
fn run(directory: &Path, step: &Step) -> std::process::Output {
    Command::new("sh")
        .arg("-c")
        .arg(&step.command)
        .current_dir(directory)
        .output()
        .expect("the documented command runs")
}

fn executable_steps() -> Vec<Step> {
    sequence()
        .expect("the verification sequence parses")
        .steps
        .into_iter()
        .filter(|step| step.executable)
        .collect()
}

#[test]
fn should_execute_the_documented_verification_sequence_against_a_release_fixture() {
    let steps = executable_steps();
    assert!(
        !steps.is_empty(),
        "v0.4.1 §47.5's sequence has to have at least one step this repository can run; a \
         sequence that is only printed is a sequence nobody has checked"
    );
    let dir = release();
    let directory = dir.path().join("release");
    for step in &steps {
        let output = run(&directory, step);
        assert!(
            output.status.success(),
            "the documented `{}` step failed on an untampered release:\n{}\n{}",
            step.id,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
}

#[test]
fn should_fail_the_documented_verification_sequence_on_a_tampered_artifact() {
    // v0.4.1 §20's shape, applied to the release: a control is accepted only when an automated
    // negative test proves the forbidden thing is refused. One byte, after the manifest was made.
    let dir = release();
    let directory = dir.path().join("release");
    dir.write("release/ono_0.4.1_amd64.deb", "the package bytes altered\n");

    let refused = executable_steps()
        .iter()
        .map(|step| run(&directory, step))
        .any(|output| !output.status.success());
    assert!(
        refused,
        "an altered artifact passed every executable step of the documented sequence, so the \
         sequence would tell a reader the wrong thing (v0.4.1 §47.2, §47.5)"
    );
}

#[test]
fn should_leave_the_other_artifacts_verifiable_when_one_is_missing() {
    // A reader downloads the one package for their distribution, not all eight. The documented
    // command has to give a real answer about that one rather than failing on the seven absent
    // ones, which is what `--ignore-missing` is for and what makes the sequence copyable.
    let dir = release();
    let directory = dir.path().join("release");
    std::fs::remove_file(directory.join("ono-0.4.1-1.x86_64.rpm")).expect("one artifact removed");
    for step in &executable_steps() {
        let output = run(&directory, step);
        assert!(
            output.status.success(),
            "the documented `{}` step failed when a reader had downloaded only one package:\n{}",
            step.id,
            String::from_utf8_lossy(&output.stderr),
        );
    }
}

#[test]
fn should_declare_a_sequence_that_fits_in_a_document_and_needs_no_proprietary_service() {
    let problems = check_sequence();
    assert!(
        problems.is_empty(),
        "v0.4.1 §47.5: the documented sequence does not meet its own constraints:\n{}",
        problems
            .iter()
            .map(|p| format!("  {} — {}", p.location, p.detail))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn should_print_the_same_commands_in_the_readme_and_the_generated_reference() {
    // Three copies exist — the generated page, the README and the Wiki's Install page — and two of
    // them are written by hand. §47.5 puts the sequence in the installation documentation, and a
    // second copy that drifted is worse than none: a reader runs the wrong command and believes
    // the right thing. The Wiki is checked by `cargo xtask terminology --wiki <path>` (ADR-0536).
    let root = repo();
    for name in ["README.md", "docs/reference/release-verification.md"] {
        let text = std::fs::read_to_string(root.join(name))
            .unwrap_or_else(|error| panic!("{name} is readable: {error}"));
        let problems = check_document(name, &text);
        assert!(
            problems.is_empty(),
            "{name} prints the verification sequence differently from the registry:\n{}",
            problems
                .iter()
                .map(|p| format!("  {} — {}", p.location, p.detail))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}

#[test]
fn should_report_a_document_that_prints_a_command_the_registry_does_not() {
    let problems = check_document(
        "README.md",
        "Verify the download with `sha256sum -c SHA256SUMS` and then install it.",
    );
    assert!(
        !problems.is_empty(),
        "a document printing its own spelling of the sequence is reported"
    );
}
