//! Helpers shared by the `xtask` test suites.
//!
//! One definition per job, because five suites had written the same `repo()` and two of them had
//! already drifted into spelling its return type differently (v0.4.1 §39.1, ADR-0427, ADR-0515).

#![allow(dead_code, reason = "not every helper is used by every test binary")]
#![allow(
    clippy::panic,
    clippy::expect_used,
    reason = "AGENTS.md §16: a helper shared by tests states its preconditions the same way a test does"
)]

use std::path::Path;

/// The workspace root.
pub fn repo() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask sits in the workspace")
        .to_path_buf()
}

/// The text of a repository file, or a panic naming the one that would not open.
pub fn read(relative: &str) -> String {
    let path = repo().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{relative} is readable: {error}"))
}

/// Every `` `file.rs::test_name` `` a passage names, with the file each one belongs to.
///
/// The checklist writes a file once and then lists several of its tests as bare `::name`, the
/// way a reader reads it; the file in force is carried along.
pub fn named_tests(passage: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut current: Option<String> = None;
    for token in passage.split('`').skip(1).step_by(2) {
        let Some((file, name)) = token.split_once("::") else {
            if token.ends_with(".rs") && !token.contains(' ') {
                current = Some(token.to_owned());
            }
            continue;
        };
        if !file.is_empty() {
            current = Some(file.to_owned());
        }
        let name = name.trim();
        if name.is_empty() || !name.starts_with("should_") {
            continue;
        }
        let Some(file) = current.clone() else {
            panic!("`::{name}` is named before any file it could belong to");
        };
        found.push((file, name.to_owned()));
    }
    found
}

/// Where a test file named in a checklist actually lives, or `None` when nothing answers to the
/// name.
///
/// A checklist names some of them by their whole path and some by their bare file name; both have
/// to resolve to exactly one file that the workspace's `cargo test` runs. A bare name is looked
/// for under `xtask/tests/` as well as under each crate's, because the guards that hold the
/// checklists themselves live there and §4.8 names them the same short way it names the rest.
///
/// A name that resolves to *several* files is reported as a failure rather than as an absence: it
/// is ambiguous, and picking one of them would make the answer depend on directory order.
///
/// # Panics
///
/// Panics when the name resolves to more than one file.
pub fn locate(what: &str, file: &str) -> Option<std::path::PathBuf> {
    for candidate in [
        repo().join(file),
        repo().join("crates").join(file),
        repo().join("xtask/tests").join(file),
    ] {
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let mut hits: Vec<std::path::PathBuf> = Vec::new();
    for crate_dir in std::fs::read_dir(repo().join("crates"))
        .expect("the crates directory exists")
        .flatten()
    {
        let candidate = crate_dir.path().join("tests").join(file);
        if candidate.is_file() {
            hits.push(candidate);
        }
    }
    assert!(
        hits.len() <= 1,
        "{what} names `{file}` as a proof, and several crates carry a test file of that name: \
         {hits:?}"
    );
    hits.into_iter().next()
}

/// Whether `source` declares `name` as a test, and whether that test is ignored.
pub fn declared(source: &str, name: &str) -> Option<bool> {
    let needle = format!("fn {name}(");
    let at = source.find(&needle)?;
    let before = &source[..at];
    let ignored = before
        .lines()
        .rev()
        .take_while(|line| {
            let line = line.trim_start();
            line.starts_with('#') || line.starts_with("//") || line.is_empty()
        })
        .any(|line| line.trim_start().starts_with("#[ignore"));
    Some(ignored)
}

/// Every proof `passage` names exists, runs where the gate runs it, and is not `#[ignore]`d.
///
/// `least` guards the harvester against reading nothing: a passage that stopped matching would
/// otherwise pass by naming no proofs at all.
///
/// # Panics
///
/// Panics naming every proof that is missing or ignored, which is the report the caller wants.
pub fn assert_proofs_exist(passage: &str, what: &str, least: usize) {
    let mut missing = Vec::new();
    let mut ignored = Vec::new();
    let named = named_tests(passage);
    assert!(
        named.len() >= least,
        "{what} names at least {least} tests; the harvester found {} — it has stopped reading \
         what it is meant to read",
        named.len()
    );
    for (file, name) in named {
        let Some(path) = locate(what, &file) else {
            missing.push(format!("{file}::{name} — no such file"));
            continue;
        };
        // `crates/*`, `fuzz` and `xtask` are the workspace members, so `cargo test --workspace`
        // runs every suite under them and nothing else (ADR-0313 put `fuzz` there for exactly
        // that reason).
        assert!(
            ["crates", "fuzz", "xtask"]
                .iter()
                .any(|member| path.starts_with(repo().join(member))),
            "{what} names `{file}::{name}`, which is outside the suites the gate runs"
        );
        let source = std::fs::read_to_string(&path).expect("a named test file is readable");
        match declared(&source, &name) {
            None => missing.push(format!("{file}::{name}")),
            Some(true) => ignored.push(format!("{file}::{name}")),
            Some(false) => {}
        }
    }
    assert!(
        missing.is_empty(),
        "{what} names proofs that do not exist — rename them there in the increment that renames \
         the test: {missing:?}"
    );
    assert!(
        ignored.is_empty(),
        "{what} names proofs that are `#[ignore]`d, so they prove nothing: {ignored:?}"
    );
}

/// One job of a GitHub workflow file: everything from `  <name>:` to the next job at that indent.
///
/// # Panics
///
/// Panics when the workflow declares no job of that name, which is the failure a caller wants
/// reported rather than an empty string it would then assert against.
pub fn workflow_job(workflow: &str, name: &str) -> String {
    let mut lines = workflow
        .lines()
        .skip_while(|line| *line != format!("  {name}:"));
    let head = lines.next().unwrap_or_else(|| panic!("no `{name}` job"));
    std::iter::once(head)
        .chain(lines.take_while(|line| line.starts_with("   ") || line.trim().is_empty()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every problem a scan reported, as one message a failing assertion can print.
///
/// Two suites wanted this and wrote it twice, and `check_duplicate_helpers` said so — which is the
/// rule of v0.4.1 §39.1 working on the commit that introduced the copy (ADR-0515). One definition,
/// called from both.
pub fn report(problems: &[xtask::scan::Problem]) -> String {
    problems
        .iter()
        .map(|problem| format!("  {} — {}", problem.location, problem.detail))
        .collect::<Vec<_>>()
        .join("\n")
}
