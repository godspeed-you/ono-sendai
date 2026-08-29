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

/// Checks that every acceptance case a document names actually exists (ADR-0401).
///
/// `docs/ACCEPTANCE.md` closes a box by naming the case that proves it, and an ADR records the
/// case that encodes its decision. Both are pointers into `docker/acceptance/cases/`, and a
/// pointer nobody follows rots the moment a case is renamed: the box stays ticked, the ADR stays
/// convincing, and the evidence is gone. This is the same class of defect as an unchecked tick,
/// so the gate resolves the pointers instead of trusting them.
///
/// A *reference* is a backticked token of the shape `NNN-kebab-case` — how every document in
/// this repository writes one — whose number falls inside the range the case suite actually
/// uses. Both halves matter. Backticks separate a name from prose about it, so a document that
/// has to record a name as *absent* writes it plain; a name inside a fenced code block is sample
/// output rather than a claim. The range separates a case number from a number: a `200-column`
/// terminal and a `512-byte` frame are shaped exactly like case names and are not cases, and no
/// wording rule could tell them apart.
///
/// Two documents are out of scope, for reasons that are not conveniences:
///
/// * `docs/STATE.md` — the board's session records deliberately name cases that never existed,
///   because recording that a name was wrong is how the board was corrected. Its own claims are
///   checked by `scripts/release-check.sh` (ADR-0402, the next decision in this series), not
///   here.
/// * the narrative specifications — immutable under AGENTS.md §5.1, so a dangling name in one
///   could never be fixed, and demanding it would only make the gate unpassable.
#[must_use]
pub fn check_acceptance_case_references(root: &Path) -> Vec<Problem> {
    let cases = root.join("docker").join("acceptance").join("cases");
    let Ok(entries) = std::fs::read_dir(&cases) else {
        return Vec::new();
    };
    let existing: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "case"))
        .filter_map(|entry| {
            entry
                .path()
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .collect();
    // The highest number the suite uses. A token above it is a number in prose, not a case that
    // went missing, and treating it as one is how a check earns the reputation of crying wolf.
    let Some(highest) = existing.iter().filter_map(|case| case_number(case)).max() else {
        return Vec::new();
    };

    let mut problems = Vec::new();
    for file in markdown_documents(root) {
        let location = relative(root, &file);
        if is_out_of_scope_for_case_references(&location) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let mut fenced = false;
        for (number, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
                continue;
            }
            if fenced {
                continue;
            }
            for name in case_references(line) {
                if existing.contains(&name) || case_number(&name).is_none_or(|n| n > highest) {
                    continue;
                }
                problems.push(Problem::new(
                    format!("{location}:{}", number + 1),
                    dangling_case_detail(&name, &existing),
                ));
            }
        }
    }
    problems.sort_by(|left, right| left.location.cmp(&right.location));
    problems
}

/// Explains a dangling reference, naming the case that carries the number where there is one.
///
/// A renamed case is the common cause, and the number survives the rename, so pointing at it
/// turns "this name is wrong" into "write this name instead".
fn dangling_case_detail(name: &str, existing: &[String]) -> String {
    let number = &name[..3];
    let same_number: Vec<&str> = existing
        .iter()
        .filter(|case| case.starts_with(number))
        .map(String::as_str)
        .collect();
    let hint = if same_number.is_empty() {
        String::new()
    } else {
        format!(
            " The case numbered {number} is `{}`.",
            same_number.join("`, `")
        )
    };
    format!(
        "names the acceptance case `{name}`, which does not exist in \
         `docker/acceptance/cases/`. A claim that points at a case nobody runs proves \
         nothing (AGENTS.md §15, docs/ACCEPTANCE.md §3).{hint}"
    )
}

/// The documents whose case references are not claims about this repository's referee.
fn is_out_of_scope_for_case_references(relative: &str) -> bool {
    relative == "docs/STATE.md"
        || (relative.starts_with("docs/ono_sendai_shell_spec_") && relative.ends_with(".md"))
}

/// The backticked `NNN-kebab-case` tokens of one line.
fn case_references(line: &str) -> Vec<String> {
    line.split('`')
        .skip(1)
        .step_by(2)
        .filter(|token| is_case_name(token))
        .map(str::to_owned)
        .collect()
}

/// The three-digit number a case-shaped name starts with.
fn case_number(name: &str) -> Option<u32> {
    name.get(..3).and_then(|digits| digits.parse().ok())
}

/// Whether a token has the shape every acceptance case file name has.
fn is_case_name(token: &str) -> bool {
    let Some((number, rest)) = token.split_at_checked(3) else {
        return false;
    };
    number.len() == 3
        && number.chars().all(|c| c.is_ascii_digit())
        && rest.starts_with('-')
        && rest.len() > 1
        && rest[1..]
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !rest.ends_with('-')
}

/// Every Markdown document in the repository, excluding build output.
fn markdown_documents(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_markdown(root, &mut files);
    files.sort();
    files
}

fn collect_markdown(dir: &Path, files: &mut Vec<PathBuf>) {
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
            collect_markdown(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "md") {
            files.push(path);
        }
    }
}

/// Checks the two claims `docs/ACCEPTANCE.md` makes about the work board (ADR-0402).
///
/// Three release boxes — §4.5 *Delivery*, §4.6.5 *Delivery* and §4.7.2 *No release-blocking known
/// defects remain* — are statements about `docs/STATE.md`, and until ADR-0402 nothing read that
/// file: `scripts/release-check.sh` grepped `docs/ACCEPTANCE.md` for its own unticked boxes and
/// stopped there. A box whose only proof is that somebody read a document is true at the moment
/// it is written and unexamined ever after.
///
/// Two properties are checked, and they are exactly what the three boxes assert:
///
/// * ***In progress* holds no claim.** An agent that has claimed work has unfinished work, and a
///   shell is not release-ready while somebody is in the middle of changing it (AGENTS.md §9,
///   §13).
/// * **Every *Deferred / blocked* entry names an ADR.** §4.7.2 requires each one to say "why it
///   does not block the release", and AGENTS.md §8 fixes the ADR as the only place that reasoning
///   may live. An entry without one is deferred work nobody defended.
///
/// *Next up* is deliberately **not** required to be empty. `docs/ACCEPTANCE.md` §4.5 calls it the
/// post-release backlog, so demanding an empty backlog would make the release line unreachable
/// and would contradict a box in the same file. The stopping rule is `docs/ACCEPTANCE.md` §4:
/// what must be closed before release is written there, in boxes, and *Next up* is what remains
/// afterwards.
///
/// This runs in `scripts/release-check.sh`, not in the gate: holding a claim mid-run is correct,
/// and a gate that refused it would forbid the working rhythm of AGENTS.md §7.
#[must_use]
pub fn check_release_board(state: &str) -> Vec<Problem> {
    let mut problems = Vec::new();

    match section(state, "In progress") {
        None => problems.push(Problem::new(
            "docs/STATE.md",
            "has no `## In progress` section, so the release boxes that claim it is empty \
             (docs/ACCEPTANCE.md §4.5, §4.6.5, §4.7.2) claim it of nothing (AGENTS.md §9)",
        )),
        Some(lines) => {
            let claims: Vec<usize> = lines
                .iter()
                .filter(|(_, line)| !line.trim().is_empty())
                .map(|(number, _)| *number)
                .collect();
            if let Some(first) = claims.first() {
                problems.push(Problem::new(
                    format!("docs/STATE.md:{first}"),
                    format!(
                        "*In progress* is not empty: {} lines of it stand under the heading. \
                         Work that is claimed is work in flight, and the shell is not \
                         release-ready while it is (docs/ACCEPTANCE.md §4.5, §4.6.5, §4.7.2). \
                         Land the claims, then clear the section",
                        claims.len()
                    ),
                ));
            }
        }
    }

    for (number, entry) in entries(state, "Deferred") {
        if adr_reference(&entry) {
            continue;
        }
        problems.push(Problem::new(
            format!("docs/STATE.md:{number}"),
            format!(
                "the *Deferred* entry {} names no ADR. A deferred item must say why it does not \
                 block the release, and that reasoning belongs in an ADR (AGENTS.md §8, \
                 docs/ACCEPTANCE.md §4.7.2)",
                summary(&entry)
            ),
        ));
    }

    problems
}

/// The numbered lines of the level-2 section whose heading starts with `title`.
fn section<'a>(state: &'a str, title: &str) -> Option<Vec<(usize, &'a str)>> {
    let mut lines = state
        .lines()
        .enumerate()
        .map(|(index, line)| (index + 1, line));
    lines.find(|(_, line)| {
        line.strip_prefix("## ")
            .is_some_and(|heading| heading.trim_start().starts_with(title))
    })?;
    Some(
        lines
            .take_while(|(_, line)| !line.starts_with("## "))
            .collect(),
    )
}

/// The top-level list items of a section, each as its own text block with the line it starts on.
fn entries(state: &str, title: &str) -> Vec<(usize, String)> {
    let Some(lines) = section(state, title) else {
        return Vec::new();
    };
    let mut entries: Vec<(usize, String)> = Vec::new();
    for (number, line) in lines {
        if line.starts_with("- ") || line.starts_with("* ") {
            entries.push((number, line.to_owned()));
        } else if let Some((_, current)) = entries.last_mut() {
            current.push('\n');
            current.push_str(line);
        }
    }
    entries
}

/// Whether a block of text names an ADR of this repository.
fn adr_reference(text: &str) -> bool {
    text.match_indices("ADR-").any(|(at, _)| {
        text[at + 4..]
            .chars()
            .take(4)
            .filter(|c| c.is_ascii_digit())
            .count()
            == 4
    })
}

/// The first line of an entry, shortened enough to name it in a message.
fn summary(entry: &str) -> String {
    let first = entry.lines().next().unwrap_or_default().trim();
    let first: String = first.chars().take(72).collect();
    format!("`{first}`")
}
