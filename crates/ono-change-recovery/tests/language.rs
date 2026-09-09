//! §25.3, as an invariant with teeth: nothing this crate can emit claims the world came back.
//!
//! *"User-visible language MUST describe the verified scope."* Everything this crate produces is
//! read by somebody — a refusal's message and help, a newer-state item's detail, a rejected
//! method's reason — and every one of those sentences is a string literal in `src/`. So the check
//! is a grep, and it lives in the suite because that is what keeps it true as the crate grows.
//!
//! The scan skips whole-line comments, so a future reader may still explain the rule in prose.
//! Anything the compiler could put in front of a person is fair game.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::path::{Path, PathBuf};

/// The sentences §25.3 forbids: each claims a scope wider than any recovery can verify.
const FORBIDDEN: &[&str] = &[
    "rollback successful",
    "fully recovered",
    "fully restored",
    "completely restored",
    "undone",
];

fn sources() -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    files
}

fn collect(directory: &Path, files: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(directory).expect("the crate's own source directory is there");
    for entry in entries {
        let path = entry.expect("a readable directory entry").path();
        if path.is_dir() {
            collect(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

/// Every line of code, with whole-line comments removed.
fn code_lines(path: &Path) -> Vec<(usize, String)> {
    let text = std::fs::read_to_string(path).expect("the source file is readable");
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim_start().starts_with("//"))
        .map(|(number, line)| (number + 1, line.to_lowercase()))
        .collect()
}

#[test]
fn should_find_the_crates_own_sources_to_scan() {
    assert!(
        sources().len() >= 7,
        "the scan is worthless if it silently found nothing to scan"
    );
}

#[test]
fn should_never_claim_a_rollback_was_successful() {
    assert_absent("rollback successful");
}

#[test]
fn should_never_claim_the_system_was_fully_recovered() {
    assert_absent("fully recovered");
}

#[test]
fn should_never_claim_the_system_was_fully_restored() {
    assert_absent("fully restored");
}

#[test]
fn should_never_claim_the_system_was_completely_restored() {
    assert_absent("completely restored");
}

#[test]
fn should_never_claim_a_change_was_undone() {
    assert_absent("undone");
}

#[test]
fn should_carry_no_unscoped_recovery_claim_anywhere_in_the_crate() {
    for phrase in FORBIDDEN {
        assert_absent(phrase);
    }
}

fn assert_absent(phrase: &str) {
    let mut found: Vec<String> = Vec::new();
    for path in sources() {
        for (number, line) in code_lines(&path) {
            if line.contains(phrase) {
                found.push(format!("{}:{number}", path.display()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "§25.3: user-visible language MUST describe the verified scope, and `{phrase}` appears at \
         {found:?}"
    );
}

#[test]
fn should_scan_lines_that_are_not_whole_line_comments() {
    // A guard on the scanner itself: if `code_lines` ever returned nothing, every assertion above
    // would pass for the wrong reason.
    let module = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/verify.rs");
    let lines = code_lines(&module);
    assert!(
        lines.iter().any(|(_, line)| line.contains("pub fn verify")),
        "the scanner reads code, and it found none in verify.rs"
    );
}
