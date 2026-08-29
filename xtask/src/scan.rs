//! Repository-wide scans that enforce rules a reviewer would otherwise have to remember.
//!
//! AGENTS.md §7 requires that an `#[ignore]`d test carry a reason and an entry in
//! `docs/STATE.md`; AGENTS.md §16 requires the same of a `TODO`. Both are easy to write and
//! easy to forget, and both are exactly how a project acquires unfinished work nobody is
//! tracking. The gate checks them instead of trusting them.

use std::path::{Path, PathBuf};

/// A rule violation, phrased so the reader knows what to do about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// Where the problem is, relative to the repository root.
    pub location: String,
    /// What is wrong, and what would fix it.
    pub detail: String,
}

impl Problem {
    fn new(location: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            location: location.into(),
            detail: detail.into(),
        }
    }
}

/// Markers that mean "this is not finished", and must never reach a green tree.
///
/// AUTONOMOUS_IMPLEMENTATION.md §19 lists them: a build that compiles around a `todo!()` is not
/// a build that works. Unlike a `TODO` comment, these cannot be excused by a tracking entry —
/// they panic in front of a user.
const FORBIDDEN_MARKERS: &[&str] = &[
    "todo!(",
    "unimplemented!(",
    "unreachable!(\"not implemented",
];

/// Comment markers that are allowed only when `docs/STATE.md` tracks them.
const TRACKED_MARKERS: &[&str] = &["TODO", "FIXME", "XXX", "HACK"];

/// Checks the crate sources for unfinished-work markers.
///
/// `state` is the text of `docs/STATE.md`. A tracked marker is acceptable when the file it lives
/// in is named somewhere in that board, which is the cheapest check that cannot be satisfied by
/// writing the word "TODO" into the board and nothing else.
pub fn check_unfinished_work(root: &Path, state: &str) -> Vec<Problem> {
    let mut problems = unwalked_rust_trees(root);

    for file in rust_sources(root) {
        let relative = relative(root, &file);
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };

        for (number, line) in text.lines().enumerate() {
            let line_number = number + 1;

            if is_scanner_source(&relative) {
                continue;
            }

            for marker in FORBIDDEN_MARKERS {
                if line.contains(marker) {
                    problems.push(Problem::new(
                        format!("{relative}:{line_number}"),
                        format!(
                            "`{marker}` is a placeholder that panics in front of a user. Implement \
                             the behaviour or return a structured error (AGENTS.md §16)."
                        ),
                    ));
                }
            }

            for marker in TRACKED_MARKERS {
                if !comment_contains_marker(line, marker) {
                    continue;
                }
                if !state.contains(&relative) {
                    problems.push(Problem::new(
                        format!("{relative}:{line_number}"),
                        format!(
                            "a `{marker}` comment needs a matching entry in docs/STATE.md naming \
                             `{relative}` (AGENTS.md §16). Untracked leftover work is how a \
                             project forgets what it owes."
                        ),
                    ));
                }
            }
        }

        problems.extend(check_ignored_tests(&relative, &text, state));
    }

    problems
}

/// Every `#[ignore]`d test must carry a `// REASON:` comment and appear in `docs/STATE.md`.
fn check_ignored_tests(relative: &str, text: &str, state: &str) -> Vec<Problem> {
    let lines: Vec<&str> = text.lines().collect();
    let mut problems = Vec::new();

    for (number, line) in lines.iter().enumerate() {
        if !line.trim_start().starts_with("#[ignore") {
            continue;
        }
        let line_number = number + 1;
        let context_start = number.saturating_sub(4);
        let has_reason = lines[context_start..=number]
            .iter()
            .any(|candidate| candidate.contains("REASON:"));

        if !has_reason {
            problems.push(Problem::new(
                format!("{relative}:{line_number}"),
                "an ignored test needs a `// REASON:` comment saying why it cannot run yet \
                 (AGENTS.md §7)"
                    .to_owned(),
            ));
        }
        if !state.contains(relative) {
            problems.push(Problem::new(
                format!("{relative}:{line_number}"),
                format!(
                    "an ignored test needs an entry under *Deferred* in docs/STATE.md naming \
                     `{relative}` (AGENTS.md §7). A test nobody is tracking is a requirement \
                     nobody is meeting."
                ),
            ));
        }
    }

    problems
}

/// Whether `line` contains `marker` inside a comment, rather than inside a string or an
/// identifier. Keeps the scanner from reporting the word "TODO" in ordinary prose or in a test
/// that asserts something about markers.
fn comment_contains_marker(line: &str, marker: &str) -> bool {
    let Some(comment_start) = line.find("//") else {
        return false;
    };
    let comment = &line[comment_start..];
    let Some(position) = comment.find(marker) else {
        return false;
    };
    let after = &comment[position + marker.len()..];
    // `TODO:` and `TODO(` are markers; `TODOS` in prose is not.
    after
        .chars()
        .next()
        .is_none_or(|next| !next.is_alphanumeric() && next != '_')
}

/// The two files that necessarily name every marker: the scanner, and the test that drives it.
///
/// Excusing all of `xtask/tests/` would hide a `todo!()` in any other xtask test from the gate,
/// which is the one thing the scan exists to prevent.
fn is_scanner_source(relative: &str) -> bool {
    relative == "xtask/src/scan.rs" || relative == "xtask/tests/scan.rs"
}

/// The top-level trees that hold Rust: the crates, the automation, and the three the repository
/// layout of AGENTS.md §2 and spec §35.6 reserve for cross-crate suites, examples and fuzzing.
const RUST_TREES: &[&str] = &["crates", "xtask", "tests", "examples", "fuzz"];

/// Every `.rs` file under the trees of [`RUST_TREES`], excluding build output.
fn rust_sources(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for top in RUST_TREES {
        collect_rust(&root.join(top), &mut files);
    }
    files.sort();
    files
}

/// Reports a top-level directory that holds Rust the scan does not walk.
///
/// [`RUST_TREES`] is a fixed list, so a new tree — a `benches/`, a vendored crate — is scanned by
/// nothing and nobody finds out. The scan cannot decide whether such a tree belongs in the list;
/// it can insist that somebody decides.
fn unwalked_rust_trees(root: &Path) -> Vec<Problem> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut problems = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name == "target" || RUST_TREES.contains(&name.as_str()) {
            continue;
        }
        if !entry.path().is_dir() {
            continue;
        }
        let mut found = Vec::new();
        collect_rust(&entry.path(), &mut found);
        if found.is_empty() {
            continue;
        }
        problems.push(Problem::new(
            name.clone(),
            format!(
                "`{name}/` holds Rust the unfinished-work scan does not walk, so a \
                 `todo!()` there would reach a green tree. Add it to `RUST_TREES` in \
                 xtask/src/scan.rs, or move the code under `crates/`"
            ),
        ));
    }
    problems.sort_by(|left, right| left.location.cmp(&right.location));
    problems
}

fn collect_rust(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "target" || name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect_rust(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
