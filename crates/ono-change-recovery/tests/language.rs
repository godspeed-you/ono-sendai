//! §25.3, as an invariant with teeth: nothing this crate can emit claims the world came back.
//!
//! *"User-visible language MUST describe the verified scope."* Everything this crate produces is
//! read by somebody — a refusal's message and help, a newer-state item's detail, a rejected
//! method's reason. Those sentences are string literals in this crate's `src/` and, for the
//! refusals, in `ono-change-core`'s `error.rs`, which is where this crate's `ErrorValue`s are
//! built. So the check is a grep over both, and it lives in the suite because that is what keeps
//! it true as either grows.
//!
//! A literal continued across lines with a trailing `\` is joined before it is matched, so a
//! phrase split by the line break is still found. The scan skips whole-line comments, so a future
//! reader may still explain the rule in prose. Anything the compiler could put in front of a
//! person is fair game.

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
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    collect(&manifest.join("src"), &mut files);
    // The refusals this crate returns are composed in the core crate's error module.
    let refusals = manifest.join("../ono-change-core/src/error.rs");
    assert!(
        refusals.is_file(),
        "the refusal messages this crate emits are in {}",
        refusals.display()
    );
    files.push(refusals);
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

/// Every logical line of code, lower-cased, with whole-line comments removed.
///
/// A line ending in `\\` continues a string literal on the next line, whose leading whitespace
/// the literal does not contain; the two are joined, numbered by the line the literal started on.
fn code_lines(path: &Path) -> Vec<(usize, String)> {
    let text = std::fs::read_to_string(path).expect("the source file is readable");
    let mut joined: Vec<(usize, String)> = Vec::new();
    let mut open: Option<(usize, String)> = None;
    for (index, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        let (number, mut logical) = match open.take() {
            Some((number, head)) => (number, head + line.trim_start()),
            None => (index + 1, line.to_owned()),
        };
        if let Some(head) = logical.strip_suffix('\\') {
            logical = head.to_owned();
            open = Some((number, logical));
            continue;
        }
        joined.push((number, logical.to_lowercase()));
    }
    if let Some((number, logical)) = open {
        joined.push((number, logical.to_lowercase()));
    }
    joined
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

#[test]
fn should_join_a_literal_continued_across_lines_before_matching_it() {
    // A guard on the joining: core's `error.rs` continues this sentence across a line break
    // (`... The \` / `metadata reports ...`), and the scan has to read it as one sentence.
    let module = Path::new(env!("CARGO_MANIFEST_DIR")).join("../ono-change-core/src/error.rs");
    let lines = code_lines(&module);
    assert!(
        lines
            .iter()
            .any(|(_, line)| line.contains("recovered. the metadata reports per")),
        "the scanner joins a continued literal, and found no joined sentence in error.rs"
    );
}
