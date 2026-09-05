//! The rules that keep the set of narrative specifications honest.
//!
//! The specification is not one file any more: `v0.2` is the base and the user adds enhancement
//! specifications beside it. These checks are what stop an addition from going unnoticed — the
//! first one arrived and no agent instruction mentioned it, which is how it was found at all.

use ono_testkit::{Scratch, scratch};
use xtask::narrative::check;

const BASE: &str = "docs/specs/ono_sendai_shell_spec_v0.2.md";
const ENHANCEMENT: &str = "docs/specs/ono_sendai_shell_spec_v0.3_external_command_adapters.md";

/// A repository that satisfies every rule, which each test then breaks in exactly one way.
fn sound() -> Scratch {
    let repo = scratch();
    repo.write(BASE, "# base\n");
    repo.write(ENHANCEMENT, "# enhancement\n");
    repo.write(
        "docs/specs/spec.sha256",
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
    repo.write("docs/specs/spec.sha256", format!("0000  {BASE}\n"));
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
    repo.write("docs/specs/spec.sha256", format!("0000  {BASE}\n"));
    for file in ["AGENTS.md", "CLAUDE.md", "README.md"] {
        repo.write(file, format!("the spec is {BASE}\n"));
    }
    assert_eq!(check(repo.path()), Vec::new());
}

/// The name the v0.5 Temporal & Causal Systems Interface arrived under: no `shell_spec` infix.
/// Discovery keyed on that infix, so the document was neither checksummed nor enumerated and the
/// gate stayed green. The file has been renamed since; the shape has not stopped being possible,
/// which is what this fixture holds (ADR-0423).
const TEMPORAL: &str = "docs/specs/ono_sendai_spec_v0.5_temporal_causal_systems_interface.md";

#[test]
fn should_find_an_enhancement_whose_name_omits_the_shell_infix() {
    let repo = sound();
    repo.write(TEMPORAL, "# temporal\n");
    let problems = check(repo.path());
    assert_eq!(problems.len(), 2, "got {problems:?}");
    assert!(
        problems
            .iter()
            .any(|problem| problem.location.contains("spec.sha256")
                && problem.detail.contains(TEMPORAL)),
        "an unguarded specification must be reported however it is named, got {problems:?}"
    );
    assert!(
        problems
            .iter()
            .any(|problem| problem.location.contains("AGENTS.md")
                && problem.detail.contains(TEMPORAL)),
        "an unenumerated specification must be reported however it is named, got {problems:?}"
    );
}

#[test]
fn should_take_the_lowest_version_as_the_base_whatever_the_later_names_look_like() {
    let repo = sound();
    repo.write(TEMPORAL, "# temporal\n");
    repo.write(
        "docs/specs/spec.sha256",
        format!("0000  {BASE}\n1111  {ENHANCEMENT}\n2222  {TEMPORAL}\n"),
    );
    // Only AGENTS.md carries the enhancements; CLAUDE.md and README.md carry the base alone. If
    // the base were taken to be whatever sorts first, those two would be reported instead.
    repo.write(
        "AGENTS.md",
        format!("the specs are {BASE}, {ENHANCEMENT} and {TEMPORAL}\n"),
    );
    for file in ["CLAUDE.md", "README.md"] {
        repo.write(file, format!("the base is {BASE}\n"));
    }
    assert_eq!(check(repo.path()), Vec::new());
}

/// The specifications live in `docs/specs/`, and discovery is rooted there rather than in `docs/`.
///
/// This is the half of the immutability guarantee a path can silently take away. The checksum
/// file proves that a *discovered* specification was not edited; nothing proves that a
/// specification is discovered at all. So when the documents moved out of `docs/` into
/// `docs/specs/`, a discovery root left behind would have found nothing, reported nothing, and
/// left nine immutable documents unguarded with a green gate — the same failure ADR-0423 records,
/// arriving through a directory rename instead of a file rename. A specification at the old
/// location is not a specification this repository knows about, and saying so is what stops the
/// next move from being silent.
#[test]
fn should_not_discover_a_specification_left_outside_the_specs_directory() {
    let repo = scratch();
    repo.write("docs/ono_sendai_shell_spec_v0.2.md", "# base\n");
    for file in ["AGENTS.md", "CLAUDE.md", "README.md"] {
        repo.write(file, "the specs are somewhere\n");
    }
    let problems = check(repo.path());
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(
        problems[0].detail.contains("no narrative specification"),
        "a specification outside docs/specs/ is not covered by the checksum rule, and the gate \
         must say so rather than pass, got {problems:?}"
    );
}
