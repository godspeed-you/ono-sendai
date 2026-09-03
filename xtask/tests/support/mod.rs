//! Helpers shared by the `xtask` test suites.
//!
//! One definition per job, because five suites had written the same `repo()` and two of them had
//! already drifted into spelling its return type differently (v0.4.1 §39.1, ADR-0427, ADR-0515).

#![allow(dead_code, reason = "not every helper is used by every test binary")]

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
