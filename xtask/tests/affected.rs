//! The gate tests the packages an increment can break, and every package when it cannot tell
//! (ADR-0853).
//!
//! The selection is only worth having if it never leaves out a package a change can fail, so
//! most of what is asserted here is where it has to give up and cover everything.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "AGENTS.md §16: a test states its preconditions directly"
)]

use std::collections::BTreeSet;
use std::path::Path;

use xtask::affected::{HARNESS, HARNESS_PACKAGE, Package, Selection, Workspace, select};
use xtask::scan::rust_sources;

mod support;
use support::repo;

fn package(name: &str, directory: &str, dependencies: &[&str]) -> Package {
    Package {
        name: name.to_owned(),
        directory: directory.to_owned(),
        dependencies: dependencies.iter().map(|&name| name.to_owned()).collect(),
    }
}

/// A small workspace shaped like this one: a core everything uses, a shell that uses nearly
/// everything, and the harness on top of the shell.
fn workspace() -> Workspace {
    Workspace::new(vec![
        package("ono-core", "crates/ono-core", &[]),
        package("ono-value", "crates/ono-value", &["ono-core"]),
        package("ono-editor", "crates/ono-editor", &["ono-core"]),
        package("ono-change", "crates/ono-change", &["ono-core"]),
        package("ono-change-core", "crates/ono-change-core", &["ono-core"]),
        package(
            "ono-cli",
            "crates/ono-cli",
            &["ono-value", "ono-editor", "ono-change", "ono-change-core"],
        ),
        package("ono-kuang-sdk", "crates/ono-kuang-sdk", &["ono-core"]),
        package("xtask", "xtask", &["ono-cli"]),
    ])
}

fn changed(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|&path| path.to_owned()).collect()
}

fn packages(selection: &Selection) -> BTreeSet<String> {
    match selection {
        Selection::Packages(names) => names.clone(),
        Selection::Everything { reason } => {
            panic!("expected a set of packages, got every package because {reason}")
        }
    }
}

fn names(list: &[&str]) -> BTreeSet<String> {
    list.iter().map(|&name| name.to_owned()).collect()
}

#[test]
fn should_cover_every_package_when_the_tree_matches_its_commit() {
    // A gate run on a clean tree is a re-run after the commit, and that run is asked about the
    // whole tree rather than about an increment it can no longer see.
    let selection = select(&workspace(), &[]);
    assert!(
        matches!(selection, Selection::Everything { .. }),
        "nothing changed, so everything is covered. Got {selection:?}"
    );
}

#[test]
fn should_cover_a_changed_package_and_every_package_that_depends_on_it() {
    let selection = select(&workspace(), &changed(&["crates/ono-editor/src/lib.rs"]));
    assert_eq!(
        packages(&selection),
        names(&["ono-editor", "ono-cli", "xtask"]),
        "the editor's own tests, and those of everything built on it, directly or not"
    );
}

#[test]
fn should_leave_out_a_package_the_change_cannot_reach_when_it_depends_on_nothing_changed() {
    let selection = packages(&select(
        &workspace(),
        &changed(&["crates/ono-editor/tests/keys.rs"]),
    ));
    for untouched in ["ono-core", "ono-value", "ono-change"] {
        assert!(
            !selection.contains(untouched),
            "{untouched} neither changed nor depends on the editor. Got {selection:?}"
        );
    }
}

#[test]
fn should_not_mistake_one_package_for_another_when_its_directory_prefixes_the_others() {
    let selection = packages(&select(
        &workspace(),
        &changed(&["crates/ono-change-core/src/lib.rs"]),
    ));
    assert!(
        selection.contains("ono-change-core") && !selection.contains("ono-change"),
        "`crates/ono-change` is a prefix of `crates/ono-change-core` and a different package. Got \
         {selection:?}"
    );
}

#[test]
fn should_cover_the_harness_when_the_changed_package_has_no_dependents() {
    // xtask's tests read the sources of every package — the metrics count their tests, the scans
    // read their code — so no change is out of their sight, even one nothing depends on.
    let selection = select(&workspace(), &changed(&["crates/ono-kuang-sdk/src/lib.rs"]));
    assert_eq!(
        packages(&selection),
        names(&["ono-kuang-sdk", HARNESS_PACKAGE]),
    );
}

#[test]
fn should_cover_only_the_harness_when_only_harness_files_changed() {
    let selection = select(
        &workspace(),
        &changed(&[
            "docs/STATE.md",
            "docs/adr/ADR-9999-a-decision.md",
            "scripts/gate.sh",
            "docker/acceptance/cases/000-login.case",
        ]),
    );
    assert_eq!(
        packages(&selection),
        names(&[HARNESS_PACKAGE]),
        "these files are read by xtask's tests alone"
    );
}

#[test]
fn should_cover_every_package_when_a_file_no_rule_places_changed() {
    // The lockfile and the toolchain change every build; the contracts are compiled into
    // ono-value, ono-command and ono-adapter by their build scripts and read by tests across the
    // workspace; the migration guide is read by one of ono-cli's tests, the fuzz workflow by the
    // fuzz crate's. Nothing here may be guessed, so each covers the workspace.
    for path in [
        "Cargo.lock",
        "Cargo.toml",
        "rust-toolchain.toml",
        "docs/contracts/commands/get.yaml",
        "docs/MIGRATION.md",
        ".github/workflows/fuzz.yml",
        "a-file-nobody-has-seen-before.txt",
    ] {
        let selection = select(&workspace(), &changed(&["docs/STATE.md", path]));
        match selection {
            Selection::Everything { reason } => assert!(
                reason.contains(path),
                "the reason names the file that forced it. Got {reason:?}"
            ),
            Selection::Packages(names) => {
                panic!("{path} covers every package, got only {names:?}")
            }
        }
    }
}

#[test]
fn should_hand_cargo_one_package_argument_per_selected_package() {
    assert_eq!(
        Selection::Packages(names(&["ono-cli", "xtask"])).cargo_arguments(),
        ["--package", "ono-cli", "--package", "xtask"],
    );
    assert_eq!(
        Selection::Everything {
            reason: "Cargo.lock changed".to_owned()
        }
        .cargo_arguments(),
        ["--workspace"],
    );
}

#[test]
fn should_read_this_workspace_with_the_harness_on_top_of_the_shell() {
    let workspace = Workspace::read(&repo()).expect("cargo metadata describes this workspace");
    let selection = packages(&select(
        &workspace,
        &changed(&["crates/ono-editor/src/lib.rs"]),
    ));
    assert!(
        selection.contains("ono-editor")
            && selection.contains("ono-cli")
            && selection.contains(HARNESS_PACKAGE),
        "the editor, the shell built on it and the harness built on the shell. Got {selection:?}"
    );
    assert!(
        !selection.contains("ono-core"),
        "the editor depends on ono-core, not the other way round. Got {selection:?}"
    );
}

/// The string literals of one Rust source, as far as a path could be one of them.
fn literals(source: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut current: Option<String> = None;
    let mut escaped = false;
    for character in source.chars() {
        match current.as_mut() {
            None if character == '"' => current = Some(String::new()),
            None => {}
            Some(_) if escaped => escaped = false,
            Some(_) if character == '\\' => escaped = true,
            Some(literal) if character == '"' => {
                found.push(std::mem::take(literal));
                current = None;
            }
            Some(literal) => literal.push(character),
        }
    }
    found
}

/// Whether a literal names a harness path, however many directories up it starts.
fn names_a_harness_path(literal: &str) -> bool {
    if literal.chars().any(char::is_whitespace) {
        return false;
    }
    let mut path = literal.trim_start_matches('/');
    while let Some(rest) = path.strip_prefix("../").or_else(|| path.strip_prefix("./")) {
        path = rest;
    }
    HARNESS.iter().any(|harness| {
        path == harness.trim_end_matches('/')
            || (harness.ends_with('/') && path.starts_with(harness))
    })
}

#[test]
fn should_find_a_harness_path_however_a_package_spells_it() {
    assert!(names_a_harness_path("../../docs/STATE.md"));
    assert!(names_a_harness_path("/../../scripts/gate.sh"));
    assert!(names_a_harness_path("README.md"));
    assert!(!names_a_harness_path(
        "AGENTS.md §16: a test states its preconditions directly"
    ));
    assert!(!names_a_harness_path("../../docs/MIGRATION.md"));
}

#[test]
fn should_keep_every_harness_path_out_of_every_package_but_the_harness() {
    // HARNESS is a claim about the whole repository: nothing outside xtask reads these files. A
    // package that names one of them makes the claim false, and an increment touching that file
    // would skip the package's tests. So the claim is checked here, against every literal of
    // every source outside xtask, and a package that starts reading such a file turns this red
    // until the path leaves HARNESS.
    let root = repo();
    let harness = root.join(HARNESS_PACKAGE);
    let mut offenders = Vec::new();
    for file in rust_sources(&root) {
        if file.starts_with(&harness) {
            continue;
        }
        let source = std::fs::read_to_string(&file).expect("a source is readable");
        for literal in literals(&source) {
            if names_a_harness_path(&literal) {
                offenders.push(format!("{} names {literal:?}", relative(&root, &file)));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "these packages read a path xtask::affected::HARNESS says only xtask reads:\n{}",
        offenders.join("\n")
    );
}

fn relative(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .display()
        .to_string()
}
