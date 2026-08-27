//! The rules that keep the set of narrative specifications honest.
//!
//! The specification is not one file any more: `v0.2` is the base and the user adds enhancement
//! specifications beside it. These checks are what stop an addition from going unnoticed — the
//! first one arrived and no agent instruction mentioned it, which is how it was found at all.

use ono_testkit::{Scratch, scratch};
use xtask::narrative::check;

const BASE: &str = "docs/ono_sendai_shell_spec_v0.2.md";
const ENHANCEMENT: &str = "docs/ono_sendai_shell_spec_v0.3_external_command_adapters.md";

/// A repository that satisfies every rule, which each test then breaks in exactly one way.
fn sound() -> Scratch {
    let repo = scratch();
    repo.write(BASE, "# base\n");
    repo.write(ENHANCEMENT, "# enhancement\n");
    repo.write(
        "docs/spec.sha256",
        format!("0000  {BASE}\n1111  {ENHANCEMENT}\n"),
    );
    for file in ["AGENTS.md", "CLAUDE.md", "README.md"] {
        repo.write(file, format!("the specs are {BASE} and {ENHANCEMENT}\n"));
    }
    repo
}

#[test]
fn should_accept_a_base_specification_beside_its_enhancements() {
    assert_eq!(check(sound().path()), Vec::new());
}

#[test]
fn should_report_a_repository_with_no_narrative_specification_at_all() {
    let repo = scratch();
    repo.write("AGENTS.md", "");
    let problems = check(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(
        problems[0].detail.contains("no narrative specification"),
        "got {problems:?}"
    );
}

#[test]
fn should_report_a_specification_that_no_checksum_would_catch_a_change_to() {
    let repo = sound();
    repo.write("docs/spec.sha256", format!("0000  {BASE}\n"));
    let problems = check(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(
        problems[0].location.contains("spec.sha256"),
        "got {problems:?}"
    );
    assert!(
        problems[0].detail.contains(ENHANCEMENT),
        "the problem must name the unguarded file, got {problems:?}"
    );
}

#[test]
fn should_report_an_enhancement_the_agent_instructions_never_mention() {
    let repo = sound();
    repo.write("AGENTS.md", format!("the spec is {BASE}\n"));
    let problems = check(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(
        problems[0].location.contains("AGENTS.md"),
        "got {problems:?}"
    );
    assert!(problems[0].detail.contains(ENHANCEMENT), "got {problems:?}");
}

#[test]
fn should_report_instructions_that_point_at_a_base_specification_that_is_not_there() {
    for file in ["AGENTS.md", "CLAUDE.md", "README.md"] {
        let repo = sound();
        repo.write(file, format!("only the enhancement: {ENHANCEMENT}\n"));
        let problems = check(repo.path());
        assert_eq!(problems.len(), 1, "for {file}, got {problems:?}");
        assert!(problems[0].location.contains(file), "got {problems:?}");
        assert!(problems[0].detail.contains(BASE), "got {problems:?}");
    }
}

#[test]
fn should_accept_a_repository_whose_only_specification_is_the_base() {
    let repo = scratch();
    repo.write(BASE, "# base\n");
    repo.write("docs/spec.sha256", format!("0000  {BASE}\n"));
    for file in ["AGENTS.md", "CLAUDE.md", "README.md"] {
        repo.write(file, format!("the spec is {BASE}\n"));
    }
    assert_eq!(check(repo.path()), Vec::new());
}
