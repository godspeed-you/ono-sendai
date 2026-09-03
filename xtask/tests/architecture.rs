//! The decomposition of phase H9, held against the tree it produced.
//!
//! H9 cut three files into modules under one rule: no test may change (AGENTS.md §11, v0.4.1
//! §65.12). That is what makes the result trustworthy, and it is also what leaves it undefended —
//! a decomposition whose evidence is an *unchanged* suite has, by construction, no test of its
//! own, and a layout nobody checks reassembles itself.
//!
//! These are outcome tests about the repository's shape, which §66.6 makes a release criterion.
//! Every rule is proved twice: against a fixture that must be reported, so the rule is known to
//! bite, and against this repository, so it is known to hold here.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "AGENTS.md §16: a helper shared by tests states its preconditions the same way a test does"
)]

use std::path::Path;

use ono_testkit::{Scratch, scratch};
use xtask::architecture::check;

mod support;
use support::report;

/// The declaration this repository ships, so a fixture can start from something real.
fn registry() -> String {
    std::fs::read_to_string(repository().join("docs/spec/hardening/module_architecture.yaml"))
        .expect("the architecture registry")
}

/// The repository root.
fn repository() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
}

/// A throwaway repository carrying a registry and whichever files the test needs.
fn fixture(files: &[(&str, &str)]) -> Scratch {
    let repo = scratch();
    repo.write("docs/spec/hardening/module_architecture.yaml", registry());
    for (path, contents) in files {
        repo.write(path, contents);
    }
    repo
}

// --- §29.2, the parser ---------------------------------------------------------------------------

#[test]
fn should_find_every_parser_responsibility_in_its_own_module() {
    let problems: Vec<_> = check(repository())
        .into_iter()
        .filter(|problem| problem.location.contains("ono-parser"))
        .collect();
    assert!(
        problems.is_empty(),
        "the parser's declared responsibilities and its modules disagree:\n{}",
        report(&problems)
    );
}

#[test]
fn should_report_a_parser_responsibility_that_lost_its_module() {
    // Only the declaration is written, so every module it names is absent. A responsibility whose
    // module moved without the declaration following it is how the map stops matching the ground.
    let repo = fixture(&[]);
    let problems = check(repo.path());
    assert!(
        problems
            .iter()
            .any(|problem| problem.location.ends_with("statements.rs")),
        "a missing module is reported by name:\n{}",
        report(&problems)
    );
}

#[test]
fn should_report_a_parser_module_no_responsibility_claims() {
    let mut files = declared_modules("src/parser", "ono-parser");
    files.push((
        "crates/ono-parser/src/parser/leftovers.rs".to_owned(),
        "pub(super) fn helper() {}\n".to_owned(),
    ));
    let repo = fixture(&borrow(&files));
    let problems = check(repo.path());
    assert!(
        problems
            .iter()
            .any(|problem| problem.location.ends_with("leftovers.rs")),
        "the module nobody claimed is the one a split file reassembles through:\n{}",
        report(&problems)
    );
}

// --- §30.2, the evaluator ------------------------------------------------------------------------

#[test]
fn should_find_every_evaluator_responsibility_in_its_own_module() {
    let problems: Vec<_> = check(repository())
        .into_iter()
        .filter(|problem| problem.location.contains("src/eval"))
        .collect();
    assert!(
        problems.is_empty(),
        "the evaluator's declared responsibilities and its modules disagree:\n{}",
        report(&problems)
    );
}

#[test]
fn should_find_no_domain_logic_moved_up_into_the_composition_root() {
    let problems: Vec<_> = check(repository())
        .into_iter()
        .filter(|problem| {
            problem.location.starts_with("crates/ono-cli/src/")
                && !problem.location.contains("/eval")
        })
        .collect();
    assert!(
        problems.is_empty(),
        "the composition root holds a module nobody declared:\n{}",
        report(&problems)
    );
}

#[test]
fn should_report_a_module_added_to_the_composition_root_without_a_decision() {
    let mut files = declared_modules("src/parser", "ono-parser");
    files.extend(declared_modules("src/eval", "ono-cli"));
    files.extend(declared_modules("src/eval/native", "ono-cli"));
    files.extend(composition_root_modules());
    files.push((
        "crates/ono-cli/src/spatial_index.rs".to_owned(),
        "pub fn place() {}\n".to_owned(),
    ));
    let repo = fixture(&borrow(&files));
    let problems = check(repo.path());
    assert!(
        problems
            .iter()
            .any(|problem| problem.location.ends_with("spatial_index.rs")),
        "§30.4: a module that appears in the composition root is a decision, not a diff:\n{}",
        report(&problems)
    );
}

// --- §31.2, the session --------------------------------------------------------------------------

#[test]
fn should_find_every_session_state_group_the_specification_names() {
    let problems: Vec<_> = check(repository())
        .into_iter()
        .filter(|problem| problem.location.ends_with("session.rs"))
        .collect();
    assert!(
        problems.is_empty(),
        "§31.2's state groups and the session disagree:\n{}",
        report(&problems)
    );
}

#[test]
fn should_report_a_session_whose_state_has_no_owner() {
    // The flat field list §31.3 asks to be replaced: a session with no group at all.
    let repo = fixture(&[(
        "crates/ono-cli/src/session.rs",
        "pub struct Session { cwd: String, jobs: Vec<u32> }\n",
    )]);
    let problems = check(repo.path());
    assert!(
        problems.iter().any(|problem| {
            problem.location.ends_with("session.rs")
                && problem.detail.contains("ResultHistoryState")
        }),
        "a missing state group is reported by name:\n{}",
        report(&problems)
    );
}

// --- §56, the crate graph ------------------------------------------------------------------------

#[test]
fn should_hold_the_crate_graph_against_the_declared_layering() {
    let problems: Vec<_> = check(repository())
        .into_iter()
        .filter(|problem| problem.location.ends_with("Cargo.toml"))
        .collect();
    assert!(
        problems.is_empty(),
        "a dependency edge points at a layer above its own:\n{}",
        report(&problems)
    );
}

#[test]
fn should_report_a_new_dependency_edge_that_inverts_a_declared_boundary() {
    // `ono-value` is foundation and `ono-protocol` is capability. An edge from the first to the
    // second is the inversion §30.4 and §56 forbid, and it is exactly the shape a refactor takes
    // when a lower crate reaches upward for something convenient.
    let repo = fixture(&[(
        "crates/ono-value/Cargo.toml",
        "[package]\nname = \"ono-value\"\n\n[dependencies]\nono-core.workspace = true\n\
         ono-protocol.workspace = true\n",
    )]);
    let problems = check(repo.path());
    assert!(
        problems.iter().any(|problem| {
            problem.location == "crates/ono-value/Cargo.toml"
                && problem.detail.contains("ono-protocol")
        }),
        "an upward edge is reported with both layers named:\n{}",
        report(&problems)
    );
}

#[test]
fn should_report_a_crate_the_layering_does_not_place() {
    let repo = fixture(&[(
        "crates/ono-newcomer/Cargo.toml",
        "[package]\nname = \"ono-newcomer\"\n",
    )]);
    let problems = check(repo.path());
    assert!(
        problems
            .iter()
            .any(|problem| problem.detail.contains("ono-newcomer")),
        "a crate outside the layering is a crate the rule cannot hold:\n{}",
        report(&problems)
    );
}

// --- fixture material ----------------------------------------------------------------------------

/// The modules this repository actually has under `directory`, as fixture files.
///
/// A fixture that invented its own module names would test the check against a repository that
/// does not exist; these are the real ones, so a fixture failure is about the rule under test.
fn declared_modules(directory: &str, krate: &str) -> Vec<(String, String)> {
    let base = repository().join("crates").join(krate).join(directory);
    let Ok(entries) = std::fs::read_dir(&base) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| {
            entry.path().is_dir() || entry.path().extension().is_some_and(|ext| ext == "rs")
        })
        .map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = if entry.path().is_dir() {
                format!("crates/{krate}/{directory}/{name}/mod.rs")
            } else {
                format!("crates/{krate}/{directory}/{name}")
            };
            (path, "// fixture\n".to_owned())
        })
        .collect()
}

/// The composition root's declared modules, as fixture files.
fn composition_root_modules() -> Vec<(String, String)> {
    let mut files = declared_modules("src", "ono-cli");
    files.push((
        "crates/ono-cli/src/session.rs".to_owned(),
        session_with_every_group(),
    ));
    files
}

/// A session carrying every group §31.2 names, so a fixture about something else stays quiet.
fn session_with_every_group() -> String {
    [
        "EnvironmentState",
        "ScopeState",
        "ExecutionState",
        "NavigationState",
        "ResultHistoryState",
        "JobState",
        "ProviderState",
        "PresentationState",
    ]
    .iter()
    .map(|group| format!("struct {group} {{}}\n"))
    .collect()
}

/// Borrows owned fixture rows into the shape [`fixture`] takes.
fn borrow(files: &[(String, String)]) -> Vec<(&str, &str)> {
    files
        .iter()
        .map(|(path, contents)| (path.as_str(), contents.as_str()))
        .collect()
}
