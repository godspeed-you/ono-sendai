//! Repository-wide scans that enforce rules a reviewer would otherwise have to remember.
//!
//! AGENTS.md §7 requires that an `#[ignore]`d test carry a reason and an entry in
//! `docs/STATE.md`; AGENTS.md §16 requires the same of a `TODO`. Both are easy to write and
//! easy to forget, and both are exactly how a project acquires unfinished work nobody is
//! tracking. The gate checks them instead of trusting them.

use std::collections::BTreeMap;
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
    /// A problem at `location`, phrased so the reader knows what to do about it.
    pub(crate) fn new(location: impl Into<String>, detail: impl Into<String>) -> Self {
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

/// Every `.rs` file under the trees of `RUST_TREES`, excluding build output.
///
/// Public so `xtask::metrics` counts the same tree the scanners read: a metric measured over a
/// different set of files from the one the gate walks is a metric about a different repository.
#[must_use]
pub fn rust_sources(root: &Path) -> Vec<PathBuf> {
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

/// Checks that a test which gives up on its precondition says so through the shared helper.
///
/// A test that cannot meet its precondition on this host has to return early, and `cargo test`
/// has no outcome for that: it counts as `ok`, beside the tests that actually asserted
/// something. The suite then reports coverage it does not have, which is the same class of
/// defect as a box ticked by nothing.
///
/// There is no way to add a third outcome to the harness, so the rule is that the skip has to be
/// visible: `ono_testkit::skipped` prints one marker naming the test and the reason, and that
/// marker is greppable in a log. A hand-written `eprintln!("skipped …")` is the same information
/// in a shape nothing can count, and each one drifts a little from the last — which is how eight
/// of them came to have eight formats.
///
/// The rule is deliberately narrow. It fires on a test announcing a skip its own way, not on a
/// test *deciding* to skip: choosing to return early is the test's business, and no scanner
/// could tell a precondition from ordinary control flow. What the gate can insist on is that the
/// decision leaves a record.
#[must_use]
pub fn check_silent_skips(root: &Path) -> Vec<Problem> {
    let mut problems = Vec::new();
    for file in rust_sources(root) {
        let relative = relative(root, &file);
        if !relative.contains("/tests/") || is_scanner_source(&relative) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        for (number, line) in lines.iter().enumerate() {
            if !announces_a_skip(line, lines.get(number + 1).copied().unwrap_or("")) {
                continue;
            }
            problems.push(Problem::new(
                format!("{relative}:{}", number + 1),
                "announces a skip with its own `eprintln!`. A skip that nothing can count is a                  test the summary reports as `ok` without asserting anything; call                  `ono_testkit::skipped(reason)` instead, which prints the one marker a log can                  be grepped for (ADR-0428)"
                    .to_owned(),
            ));
        }
    }
    problems.sort_by(|left, right| left.location.cmp(&right.location));
    problems
}

/// Whether `line` prints a skip notice by hand.
///
/// Matched on the printed text rather than on the macro, because the defect is the announcement:
/// `println!` reaches the same reader, and a test that writes "skipping" says the same thing as
/// one that writes "skipped".
fn announces_a_skip(line: &str, next: &str) -> bool {
    let trimmed = line.trim_start();
    if !(trimmed.starts_with("eprintln!(") || trimmed.starts_with("println!(")) {
        return false;
    }
    // A macro whose first argument is long enough is written with the string on the next line,
    // and the announcement is the same announcement. Reading only the opening line let one of
    // them sit in `spatial_map.rs` for a release (v0.4.1 §65.10).
    let quoted = match trimmed.split('"').nth(1) {
        Some(quoted) => quoted,
        None if trimmed.ends_with('(') => match next.trim_start().split('"').nth(1) {
            Some(quoted) => quoted,
            None => return false,
        },
        None => return false,
    };
    let lowered = quoted.trim_start().to_ascii_lowercase();
    lowered.starts_with("skip")
}

/// Checks that no command-line flag could switch client authentication off (v0.4.1 §7.4).
///
/// §7.4 leaves the canonical agent one listening mode and it authenticates: *"No unauthenticated
/// network mode reachable from the CLI."* `crates/ono-cli/tests/listening_agent.rs` proves the
/// absence today by enumerating fourteen spellings and asserting each is a usage error, and
/// ADR-0440 records why that test is worth having while it is green — the day somebody adds
/// `--allow-anonymous` for one awkward deployment, it goes red. This is the other half ADR-0440
/// left open: the test sees a flag that reaches `Invocation`, and this sees one written anywhere
/// in a crate's source.
///
/// A *flag* is a string literal beginning with `--`, which is how every one of them is written.
/// Reading only literals is what keeps the rule from firing on the word "authenticated" in a doc
/// comment, and there is no other way to spell a flag, so nothing escapes by being written
/// differently.
///
/// A flag is refused when its name carries `insecure`, `anonymous`, `unauthenticated` or
/// `noauth`, or when it turns something off: `no-…-auth`, `disable-…-auth`, `no-…-verify`,
/// `skip-…-verify`. `--print-peer-key` and `--host-key` are not caught, and neither is
/// `--no-config`, because none of them says *whether* to authenticate.
///
/// `tests/` is out of scope. The guard of ADR-0440 has to name the flags it refuses, and a rule
/// that caught the guard would delete it.
#[must_use]
pub fn check_authentication_flags(root: &Path) -> Vec<Problem> {
    let mut problems = Vec::new();
    for file in rust_sources(root) {
        let location = relative(root, &file);
        if location.contains("/tests/") || location.starts_with("tests/") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        for (number, line) in text.lines().enumerate() {
            for flag in flag_literals(line) {
                if !disables_authentication(&flag) {
                    continue;
                }
                problems.push(Problem::new(
                    format!("{location}:{}", number + 1),
                    format!(
                        "names the flag `--{flag}`. v0.4.1 §7.4 leaves the canonical agent one \
                         listening mode and it authenticates, so a flag that says whether to \
                         authenticate is an unauthenticated network mode reachable from the \
                         CLI (§65.1, ADR-0440). Adding one needs an ADR that supersedes that \
                         decision, and this check with it."
                    ),
                ));
            }
        }
    }
    problems.sort_by(|left, right| left.location.cmp(&right.location));
    problems
}

/// Every `"--…"` string literal on a line, without its quotes or leading dashes.
fn flag_literals(line: &str) -> Vec<String> {
    let mut flags = Vec::new();
    let bytes: Vec<char> = line.chars().collect();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != '"' {
            index += 1;
            continue;
        }
        let start = index + 1;
        let mut end = start;
        while end < bytes.len() && bytes[end] != '"' {
            end += 1;
        }
        if end < bytes.len() {
            let literal: String = bytes[start..end].iter().collect();
            if let Some(name) = literal.strip_prefix("--")
                && !name.is_empty()
            {
                flags.push(name.to_owned());
            }
        }
        index = end + 1;
    }
    flags
}

/// Whether a flag name says *whether* to authenticate rather than where or which.
fn disables_authentication(flag: &str) -> bool {
    let name = flag.to_ascii_lowercase();
    if ["insecure", "anonymous", "unauthenticated", "noauth"]
        .iter()
        .any(|word| name.contains(word))
    {
        return true;
    }
    let turns_off =
        name.starts_with("no-") || name.starts_with("disable") || name.starts_with("skip");
    turns_off && (name.contains("auth") || name.contains("verify"))
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
/// wording rule could tell them apart. Where the range no longer can either — case `200` exists
/// now — `NOT_A_CASE` names the two tokens outright.
///
/// An **unticked box names what its increment must write**, so its references are not resolved.
/// `docs/ACCEPTANCE.md` §4.7 established the convention and §4.8 depends on it: a checklist is
/// written from the specification before the work, and every case it promises is by definition
/// absent. Ticking the box is what turns the sentence from a plan into evidence, and from that
/// moment the pointer is resolved like any other. The box's continuation lines are part of it —
/// a box in this file runs over several indented lines and names its proofs on the last one.
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
        let mut open_box = false;
        for (number, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
                continue;
            }
            if fenced {
                continue;
            }
            open_box = continues_an_open_box(line, open_box);
            if open_box {
                continue;
            }
            for name in case_references(line) {
                if existing.contains(&name)
                    || NOT_A_CASE.contains(&name.as_str())
                    || case_number(&name).is_none_or(|n| n > highest)
                {
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

/// Tokens shaped like an acceptance case name that are not one, and will not become one.
///
/// ADR-0401 built the check on a range — a token numbered above the highest case is a number in
/// prose — and called it "one documented hole". Case `200` opened it. Both ADR-0401 and ADR-0463
/// write `200-column` in backticks, as the counter-example they are *about*: they are the two
/// records that decided how a case name is told from a figure, and naming the figure is how they
/// argue. `512-byte` is the other half of the same sentence in both.
///
/// AGENTS.md §8 forbids editing an accepted decision record, so the sentences cannot be rewritten
/// and the range can no longer tell them apart. Naming them here is the correction: one reviewed
/// list, in the checker, rather than a heuristic that guesses at English. The cost is stated —
/// were a case ever numbered and named `200-column`, a reference that went dangling would be
/// ignored (ADR-0539).
const NOT_A_CASE: &[&str] = &["200-column", "512-byte"];

/// Whether this line is inside an unticked checklist box, given whether the previous one was.
///
/// A box starts at `- [ ]` and runs over the indented lines that continue it, which is how every
/// box in `docs/ACCEPTANCE.md` is written. It ends at the first line that is not such a
/// continuation: a blank line, the next box, or prose returning to the left margin. `- [x]` ends
/// one too, so a ticked box is read like ordinary text and its pointers are resolved.
fn continues_an_open_box(line: &str, was_open: bool) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with("- [ ]") {
        return true;
    }
    if trimmed.starts_with("- [x]") || trimmed.starts_with("- [X]") {
        return false;
    }
    was_open && !trimmed.is_empty() && line.starts_with(char::is_whitespace)
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
        || relative
            .strip_prefix("docs/")
            .is_some_and(crate::narrative::is_narrative_spec)
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

/// The crate whose sources establish process confinement (v0.4.1 §56.5).
const CONFINEMENT_SOURCES: &str = "crates/ono-kuang-supervisor/src";

/// Reports a confinement syscall whose return value is thrown away (v0.4.1 §16.2, §65.4).
///
/// §16.2 requires *every* syscall used to establish a mandatory security or resource control to
/// have its return value checked, and §65.4 names the opposite as a failure mode of this release:
/// *"Calling a confinement syscall, discarding its result and executing the plugin anyway is
/// forbidden."* §0.5.3 found seven of them in one closure.
///
/// A rule about every member of an open set is a rule a review cannot hold, because the next
/// member is added by someone who has not read §16.2. So the gate holds it instead, and it holds
/// it on the *defect* rather than on the shape of correct code: a `libc::` call in statement
/// position, or bound to a discarded name, is a call whose result nothing can have looked at.
/// Everything else — an argument to `checked(…)`, a named binding, a `match` scrutinee — leaves
/// the value somewhere a reader can follow.
///
/// The scan is deliberately narrow. It runs over one crate, the one §56.5 makes responsible for
/// fail-closed pre-exec setup, because a dropped `libc::close` in an unrelated file is a
/// different question and a scanner that reports both would be turned off.
#[must_use]
pub fn check_confinement_syscalls(root: &Path) -> Vec<Problem> {
    let mut problems = Vec::new();
    let mut files = Vec::new();
    collect_rust(&root.join(CONFINEMENT_SOURCES), &mut files);
    files.sort();

    for file in files {
        let relative = relative(root, &file);
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        for (line, call) in dropped_syscalls(&text) {
            problems.push(Problem::new(
                format!("{relative}:{line}"),
                format!(
                    "`libc::{call}` is called and its result thrown away. v0.4.1 §16.2 requires \
                     every syscall that establishes a security or resource control to have its \
                     return value checked, and §65.4 names discarding one a forbidden failure \
                     mode. Hand it to `checked(…)`, or bind it to a name something reads."
                ),
            ));
        }
    }
    problems
}

/// Every `libc::` call in `text` whose value is dropped, as `(line number, function name)`.
///
/// Comments and string literals are blanked first, so the rule does not fire on prose that
/// mentions a call. What remains is decided by reading backwards from the call: see
/// [`value_is_dropped`].
fn dropped_syscalls(text: &str) -> Vec<(usize, String)> {
    let code = without_comments_and_strings(text);
    let mut found = Vec::new();
    let mut search = 0_usize;

    while let Some(offset) = code[search..].find("libc::") {
        let at = search + offset;
        search = at + "libc::".len();
        let Some(name) = call_name(&code[search..]) else {
            continue;
        };
        if value_is_dropped(&code, at) {
            found.push((code[..at].lines().count().max(1), name));
        }
    }
    found
}

/// Whether the call at `at` stands where its value goes nowhere.
///
/// Decided by reading backwards over what carries a value through unchanged — whitespace, and an
/// `unsafe { … }` block, whose value is the value of its last expression. What is found first
/// after that answers the question:
///
/// - a `;`, a `}`, a block-opening `{`, or the start of the file: the call is a statement, and a
///   statement's value is discarded;
/// - an `=` whose left-hand name begins with `_`: bound to a name nothing can read, which is the
///   same discard with a signature on it;
/// - anything else — an argument's `(` or `,`, an ordinary binding, a `return`: the value is
///   somewhere a reader can follow, and this scan has no opinion about it.
fn value_is_dropped(code: &str, at: usize) -> bool {
    let bytes = code.as_bytes();
    let mut index = at;
    loop {
        index = trim_back(code, index);
        if index == 0 {
            return true;
        }
        match bytes[index - 1] {
            b'{' => {
                let before_brace = trim_back(code, index - 1);
                if code[..before_brace].ends_with("unsafe") {
                    index = before_brace - "unsafe".len();
                    continue;
                }
                return true;
            }
            b';' | b'}' => return true,
            b'=' => {
                let name_end = trim_back(code, index - 1);
                let name_start = code[..name_end]
                    .rfind(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .map_or(0, |boundary| boundary + 1);
                return code[name_start..name_end].starts_with('_');
            }
            _ => return false,
        }
    }
}

/// The byte index of the first non-whitespace character before `index`.
fn trim_back(code: &str, index: usize) -> usize {
    code[..index].trim_end().len()
}

/// The function name in `setsid()`.
///
/// `None` when what follows `libc::` is not a call, which is how a constant such as
/// `libc::RLIMIT_DATA` and a type such as `libc::rlimit { … }` are left alone.
fn call_name(rest: &str) -> Option<String> {
    let name: String = rest
        .chars()
        .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_')
        .collect();
    if name.is_empty() {
        return None;
    }
    rest[name.len()..]
        .trim_start()
        .starts_with('(')
        .then_some(name)
}

/// Replaces every comment and string literal with spaces, so offsets and line numbers survive.
fn without_comments_and_strings(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut in_string = false;
    let mut escaped = false;

    while let Some(c) = chars.next() {
        if in_line_comment {
            if c == '\n' {
                in_line_comment = false;
                out.push('\n');
            } else {
                out.push(' ');
            }
            continue;
        }
        if in_block_comment {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_block_comment = false;
                out.push_str("  ");
            } else {
                out.push(if c == '\n' { '\n' } else { ' ' });
            }
            continue;
        }
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            out.push(if c == '\n' { '\n' } else { ' ' });
            continue;
        }
        match c {
            '/' if chars.peek() == Some(&'/') => {
                chars.next();
                in_line_comment = true;
                out.push_str("  ");
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                in_block_comment = true;
                out.push_str("  ");
            }
            '"' => {
                in_string = true;
                out.push(' ');
            }
            other => out.push(other),
        }
    }
    out
}

// --- the evaluator capture inventory (v0.4.1 §26.1, §65.7) -------------------------------------

/// The evaluator execution paths §26.1's inventory covers.
///
/// Every file of `ono-cli`'s evaluator, plus the three it hands a drained result to: `eval/` runs
/// statements, expressions, blocks and pipelines and `eval/native/` assembles, drives and drains
/// the value stream; `session.rs` owns the capture stack and the retained history; `report.rs`
/// and `view.rs` are what a drained result reaches. A command implementation that materializes is
/// placed by Appendix E instead — `docs/spec/hardening/streaming_classification.yaml` — because
/// there the class is a property of the operation's contract rather than of the evaluator's
/// structure.
///
/// The list follows the code: v0.4.1 §30.2 split `eval.rs` and `native.rs` into the modules below
/// (ADR-0507), and a scan that had kept pointing at the two old paths would have gone quiet
/// rather than red. The two old paths are still listed because the fixtures of
/// `xtask/tests/scan.rs` are written against them and a fixture test states the rule rather than
/// the tree; a path this repository does not have costs one failed `read_to_string`.
const EVALUATOR_SOURCES: &[&str] = &[
    "crates/ono-cli/src/eval.rs",
    "crates/ono-cli/src/native.rs",
    "crates/ono-cli/src/eval/mod.rs",
    "crates/ono-cli/src/eval/block.rs",
    "crates/ono-cli/src/eval/control.rs",
    "crates/ono-cli/src/eval/expression.rs",
    "crates/ono-cli/src/eval/function.rs",
    "crates/ono-cli/src/eval/materialize.rs",
    "crates/ono-cli/src/eval/pipeline.rs",
    "crates/ono-cli/src/eval/statement.rs",
    "crates/ono-cli/src/eval/native/mod.rs",
    "crates/ono-cli/src/eval/native/bind.rs",
    "crates/ono-cli/src/eval/native/drive.rs",
    "crates/ono-cli/src/eval/native/external.rs",
    "crates/ono-cli/src/eval/native/foreground.rs",
    "crates/ono-cli/src/eval/native/remote.rs",
    "crates/ono-cli/src/eval/native/result.rs",
    "crates/ono-cli/src/eval/native/segment.rs",
    "crates/ono-cli/src/session.rs",
    "crates/ono-cli/src/report.rs",
    "crates/ono-cli/src/view.rs",
];

/// Where the inventory lives.
const CAPTURE_INVENTORY: &str = "docs/spec/hardening/streaming.yaml";

/// What a capture looks like in source: a collection of pipeline values, or the capture stack
/// that holds one for the evaluator.
const CAPTURE_MARKERS: &[&str] = &["Vec<Value>", "begin_capture(", "end_capture("];

/// The three classes v0.4.1 §26.1 defines, and no fourth.
const CAPTURE_CLASSES: &[&str] = &[
    "semantic_materialization",
    "implementation_convenience",
    "history_cache",
];

/// Checks that every capture in the evaluator is classified, and that the inventory names no
/// capture that is not there (v0.4.1 §26.1).
///
/// §65.7 names "streaming via background collection" a forbidden failure mode, and an inventory
/// is what stops a removed capture from reappearing one stage later under another name. The
/// check runs in both directions on purpose: an unclassified capture fails the gate, and so does
/// an entry whose site no longer holds one, so removing a capture removes its entry rather than
/// leaving a classification of code nobody can find.
#[must_use]
pub fn check_evaluator_captures(root: &Path) -> Vec<Problem> {
    let mut problems = Vec::new();
    let mut found: Vec<(String, String)> = Vec::new();

    for source in EVALUATOR_SOURCES {
        let path = root.join(source);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for site in capture_sites(&text) {
            let key = ((*source).to_owned(), site);
            if !found.contains(&key) {
                found.push(key);
            }
        }
    }

    let inventory_path = root.join(CAPTURE_INVENTORY);
    let Ok(text) = std::fs::read_to_string(&inventory_path) else {
        problems.push(Problem::new(
            CAPTURE_INVENTORY,
            "v0.4.1 §26.1 requires an inventory of every capture in the evaluator, and this file \
             is where the gate reads it. Write it, with one entry per capture site.",
        ));
        return problems;
    };
    let entries = match inventory_entries(&text) {
        Ok(entries) => entries,
        Err(detail) => {
            problems.push(Problem::new(CAPTURE_INVENTORY, detail));
            return problems;
        }
    };

    for (file, site, class, adr) in &entries {
        if !CAPTURE_CLASSES.contains(&class.as_str()) {
            problems.push(Problem::new(
                format!("{CAPTURE_INVENTORY} ({file} :: {site})"),
                format!(
                    "`{class}` is not a class v0.4.1 §26.1 defines. It names three — {} — and a \
                     capture that fits none of them is a capture whose semantics are not settled.",
                    CAPTURE_CLASSES.join(", ")
                ),
            ));
        }
        if class == "implementation_convenience" && adr.is_none() {
            problems.push(Problem::new(
                format!("{CAPTURE_INVENTORY} ({file} :: {site})"),
                "v0.4.1 §26.1: an `implementation_convenience` capture on pipeline data MUST be \
                 removed, or bounded and justified by ADR. Name the ADR that bounds it, or remove \
                 the capture."
                    .to_owned(),
            ));
        }
        if !found
            .iter()
            .any(|(held_file, held_site)| held_file == file && held_site == site)
        {
            problems.push(Problem::new(
                format!("{CAPTURE_INVENTORY} ({file} :: {site})"),
                format!(
                    "the inventory classifies `{site}` in {file}, and no capture stands there \
                     any more. Remove the entry: a classification of code that is not present \
                     tells a later reader nothing about the code that is."
                ),
            ));
        }
    }

    for (file, site) in &found {
        if !entries
            .iter()
            .any(|(held_file, held_site, _, _)| held_file == file && held_site == site)
        {
            problems.push(Problem::new(
                format!("{file} ({site})"),
                format!(
                    "`{site}` holds a collection of pipeline values that {CAPTURE_INVENTORY} does \
                     not classify. v0.4.1 §26.1 requires every capture in an evaluator execution \
                     path to be named there as `semantic_materialization`, \
                     `implementation_convenience` or `history_cache`."
                ),
            ));
        }
    }

    problems.sort_by(|left, right| {
        (&left.location, &left.detail).cmp(&(&right.location, &right.detail))
    });
    problems
}

/// The items of `text` that hold a capture, in the order they are declared.
///
/// A site is an item name rather than a line number, so moving a function does not invalidate
/// its entry and renaming one does. Comments and string literals are blanked first, so prose
/// about a capture is not one.
fn capture_sites(text: &str) -> Vec<String> {
    let code = without_comments_and_strings(text);
    let mut item = String::from("<file scope>");
    let mut sites = Vec::new();
    for line in code.lines() {
        if let Some(name) = item_name(line) {
            item = name;
        }
        if CAPTURE_MARKERS.iter().any(|marker| line.contains(marker)) && !sites.contains(&item) {
            sites.push(item.clone());
        }
    }
    sites
}

/// The name this line declares, when it opens a function, a struct or an enum.
fn item_name(line: &str) -> Option<String> {
    let mut tokens = line
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .peekable();
    while let Some(token) = tokens.next() {
        if matches!(token, "fn" | "struct" | "enum") {
            let name = tokens.find(|candidate| !candidate.is_empty())?;
            return name
                .chars()
                .next()
                .is_some_and(|first| first.is_alphabetic() || first == '_')
                .then(|| name.to_owned());
        }
    }
    None
}

/// The inventory as `(file, site, class, adr)` rows.
fn inventory_entries(text: &str) -> Result<Vec<(String, String, String, Option<String>)>, String> {
    let document: serde_yaml_ng::Value = serde_yaml_ng::from_str(text)
        .map_err(|error| format!("the capture inventory is not readable YAML: {error}"))?;
    let Some(captures) = document
        .get("captures")
        .and_then(|value| value.as_sequence())
    else {
        return Err("the capture inventory has no `captures:` sequence".to_owned());
    };
    let mut entries = Vec::new();
    for capture in captures {
        let field = |name: &str| {
            capture
                .get(name)
                .and_then(|value| value.as_str())
                .map(str::to_owned)
        };
        let (Some(file), Some(site), Some(class)) = (field("file"), field("site"), field("class"))
        else {
            return Err(format!(
                "a capture entry is missing `file`, `site` or `class`: {capture:?}"
            ));
        };
        entries.push((file, site, class, field("adr")));
    }
    Ok(entries)
}

// --- bounded channels stay mandatory (v0.4.1 §28.1, §28.2, §65.7) ------------------------------

/// Where the pipeline's data path is written, and where an unbounded channel would undo it.
const PIPELINE_SOURCES: &[&str] = &["crates/ono-pipeline/src", "crates/ono-cli/src"];

/// The spellings of an unbounded Tokio channel.
const UNBOUNDED_CHANNELS: &[&str] = &["unbounded_channel(", "UnboundedSender", "UnboundedReceiver"];

/// Checks that no unbounded channel carries pipeline data (v0.4.1 §28.1).
///
/// §28.1 permits the reference capacity to be tuned "through an ADR and benchmark evidence", and
/// forbids the other change outright: "replacing bounded flow with unbounded channels is
/// forbidden". §28.2 says the same thing about the streaming paths this tranche added — they
/// "MUST NOT solve materialization by inserting an unbounded task queue" — and §65.7 names the
/// result a forbidden failure mode. All three are one rule a scan can hold.
#[must_use]
pub fn check_bounded_channels(root: &Path) -> Vec<Problem> {
    let mut problems = Vec::new();
    let mut files = Vec::new();
    for source in PIPELINE_SOURCES {
        collect_rust(&root.join(source), &mut files);
    }
    files.sort();

    for file in files {
        let relative = relative(root, &file);
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let code = without_comments_and_strings(&text);
        for (number, line) in code.lines().enumerate() {
            let Some(spelling) = UNBOUNDED_CHANNELS
                .iter()
                .find(|spelling| line.contains(**spelling))
            else {
                continue;
            };
            problems.push(Problem::new(
                format!("{relative}:{}", number + 1),
                format!(
                    "`{spelling}` carries pipeline data on an unbounded channel. v0.4.1 §28.1: \
                     \"replacing bounded flow with unbounded channels is forbidden\", §28.2 \
                     forbids solving materialization with an unbounded task queue, and §65.7 \
                     names background collection a failure mode. Size the channel, and change \
                     the reference capacity only through an ADR with benchmark evidence."
                ),
            ));
        }
    }
    problems
}

/// Checks that a test never returns before its assertion path without saying it skipped.
///
/// v0.4.1 §65.10 names the defect: *"A test returning before its assertion path without an
/// explicit skip outcome is forbidden."* Appendix G writes the same rule as a shape —
///
/// ```text
/// if prerequisite_missing() {
///     return;
/// }
/// ```
///
/// — *"is prohibited unless the return path has already emitted the canonical explicit skip
/// signal that the gate recognizes."* ADR-0428 stopped short of this and said so: a precondition
/// guard and an ordinary `return` are the same Rust, and a scanner that guessed would cry wolf.
/// What makes the rule holdable is that the specification names two escapes, and both are
/// readable from the source.
///
/// A bare `return;` inside a `#[test]` function is accepted when any of these holds:
///
/// * **it is not the test's return.** A `return` inside a closure or an `async` block leaves that
///   block, not the test — `sink.send(..).await.is_err()` is flow control in a fixture;
/// * **the block it sits in asserted something.** A branch that asserts and then returns has
///   reached an assertion path, which is precisely what §65.10 requires it to reach;
/// * **the skip has been announced on the path.** Either the block calls
///   [`skipped`](ono_testkit::skipped) or [`require`](ono_testkit::require) itself, or the guard
///   that opened the block calls a helper in the same file that does. `unprivileged()` announcing
///   the skip and returning `false` is the canonical signal already emitted, and the caller's
///   `if !unprivileged() { return; }` is the shape Appendix G permits.
///
/// Everything else is a test whose summary line says `ok` for a run in which it asserted nothing.
#[must_use]
pub fn check_unannounced_skips(root: &Path) -> Vec<Problem> {
    let mut problems = Vec::new();
    for file in rust_sources(root) {
        let relative = relative(root, &file);
        if !relative.contains("/tests/") || is_scanner_source(&relative) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        problems.extend(unannounced_skips_in(&relative, &text));
    }
    problems.sort_by(|left, right| left.location.cmp(&right.location));
    problems
}

/// One block of a test function, and what has happened inside it so far.
#[derive(Clone, Copy)]
struct Block {
    /// Whether this block is a closure or an `async` block, so a `return` leaves it and not the
    /// test.
    detached: bool,
    /// Whether the guard that opened it called a helper that announces a skip.
    guarded_by_announcer: bool,
    /// Whether something inside it has asserted, announced a skip, or required a prerequisite.
    reached_its_path: bool,
}

/// The rule of [`check_unannounced_skips`], applied to one file's text.
fn unannounced_skips_in(relative: &str, text: &str) -> Vec<Problem> {
    let announcers = skip_announcing_helpers(text);
    let mut problems = Vec::new();
    let mut stack: Vec<Block> = Vec::new();
    let mut in_test = false;
    let mut pending_test = false;
    let mut pending_announcer = false;

    for (number, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if !in_test {
            if is_test_attribute(trimmed) {
                pending_test = true;
            } else if pending_test && trimmed.contains("fn ") {
                in_test = true;
                pending_test = false;
                stack.clear();
                stack.push(Block {
                    detached: false,
                    guarded_by_announcer: false,
                    reached_its_path: false,
                });
                continue;
            }
            continue;
        }

        if calls_an_announcer(line, &announcers) {
            pending_announcer = true;
        }
        if reaches_an_assertion_path(line)
            && let Some(block) = stack.last_mut()
        {
            block.reached_its_path = true;
        }
        if trimmed == "return;" {
            let detached = stack.iter().any(|block| block.detached);
            let excused = stack
                .last()
                .is_none_or(|block| block.reached_its_path || block.guarded_by_announcer);
            if !detached && !excused {
                problems.push(Problem::new(
                    format!("{relative}:{}", number + 1),
                    "returns from a test before its assertion path without announcing a skip. A                  test that gives up on a precondition calls                  `ono_testkit::require(condition, SkipReason::…, detail)` or                  `ono_testkit::skipped(SkipReason::…, detail)` first, so the run says                  SKIP(reason) instead of counting as a pass that asserted nothing (v0.4.1                  §38.1, §65.10, Appendix G)"
                        .to_owned(),
                ));
            }
        }

        let opened = opens_a_detached_block(line);
        let (opens, closes) = brace_delta(line);
        for _ in 0..opens {
            stack.push(Block {
                detached: opened,
                guarded_by_announcer: pending_announcer,
                reached_its_path: false,
            });
        }
        for _ in 0..closes {
            stack.pop();
        }
        if opens > 0 || trimmed.ends_with(';') {
            pending_announcer = false;
        }
        if stack.is_empty() {
            in_test = false;
        }
    }
    problems
}

/// Whether a line carries the attribute that makes the next function a test.
fn is_test_attribute(trimmed: &str) -> bool {
    trimmed == "#[test]" || (trimmed.starts_with("#[") && trimmed.contains("test]"))
}

/// Whether a line opens a closure or an `async` block, from which `return` leaves the block
/// rather than the test.
fn opens_a_detached_block(line: &str) -> bool {
    if line.contains("async move {") || line.contains("async {") {
        return true;
    }
    let bytes = line.as_bytes();
    let mut bars = 0usize;
    let mut in_string = false;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' if in_string => index += 1,
            b'"' => in_string = !in_string,
            b'|' if !in_string => bars += 1,
            _ => {}
        }
        index += 1;
    }
    // `|a, b|` and `||` are two bars on one line; `a || b` is caught by the same count and is
    // not worth separating, because a line holding a boolean `or` and opening a block that a
    // test returns from is not a shape this repository writes.
    bars >= 2
}

/// Whether a line reaches an assertion — the path §65.10 wants a test to reach before it leaves.
fn reaches_an_assertion_path(line: &str) -> bool {
    const REACHED: [&str; 8] = [
        "assert!",
        "assert_eq!",
        "assert_ne!",
        "debug_assert!",
        "panic!",
        "unreachable!",
        "skipped(",
        "require(",
    ];
    REACHED.iter().any(|marker| line.contains(marker))
}

/// Whether a line calls the canonical helper, or one of the file's own skip-announcing helpers.
fn calls_an_announcer(line: &str, announcers: &[String]) -> bool {
    if line.contains("require(") || line.contains("skipped(") {
        return true;
    }
    announcers
        .iter()
        .any(|name| line.contains(&format!("{name}(")))
}

/// The names of the functions in `text` whose bodies announce a skip.
///
/// `unprivileged()` in `network.rs` prints the marker and returns `false`; its callers write
/// `if !unprivileged() { return; }`. That return path *has* emitted the canonical signal, which
/// is the escape Appendix G names, and the only way to see it is to read the helper.
fn skip_announcing_helpers(text: &str) -> Vec<String> {
    let mut announcers = Vec::new();
    let mut current: Option<(String, bool)> = None;
    let mut depth = 0usize;
    for line in text.lines() {
        if current.is_none()
            && let Some(name) = function_name(line)
        {
            current = Some((name, false));
            depth = 0;
        }
        if let Some((_, announces)) = current.as_mut()
            && (line.contains("skipped(") || line.contains("require("))
        {
            *announces = true;
        }
        let (opens, closes) = brace_delta(line);
        depth += opens;
        depth = depth.saturating_sub(closes);
        if current.is_some()
            && depth == 0
            && (opens > 0 || closes > 0)
            && let Some((name, announces)) = current.take()
            && announces
        {
            announcers.push(name);
        }
    }
    announcers
}

/// The name a `fn` line declares, if it declares one.
fn function_name(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix("pub ").unwrap_or(trimmed);
    let rest = rest.strip_prefix("pub(crate) ").unwrap_or(rest);
    let rest = rest.strip_prefix("async ").unwrap_or(rest);
    let rest = rest.strip_prefix("fn ")?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

/// How many braces a line opens and closes, ignoring those inside string literals.
fn brace_delta(line: &str) -> (usize, usize) {
    let mut opens = 0usize;
    let mut closes = 0usize;
    let mut in_string = false;
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' if in_string => index += 1,
            b'"' => in_string = !in_string,
            b'{' if !in_string => opens += 1,
            b'}' if !in_string => closes += 1,
            _ => {}
        }
        index += 1;
    }
    (opens, closes)
}

// --- v0.4.1 §38.2 and §38.3: the declared skip set, and the two ways a run may break it --------

/// The path, below the repository root, of the `expected_test_skips` registry of §52.1.
pub const EXPECTED_TEST_SKIPS: &str = "docs/spec/hardening/expected_test_skips.yaml";

/// A test that can announce a skip, and the §38.4 category it announces.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SkipSite {
    /// `crates/<crate>/tests/<file>.rs::<test>` — the form `docs/ACCEPTANCE.md` names a proof by.
    pub id: String,
    /// The §38.4 category, as `SkipReason::category` spells it.
    pub category: String,
}

/// The `expected_test_skips` registry: every skip the tree can take, and the ones the canonical
/// CI environment is expected to take.
#[derive(Debug, Clone, Default)]
pub struct ExpectedSkips {
    /// Every test that can announce a skip, with its category.
    pub declared: Vec<SkipSite>,
    /// The test ids §38.2 expects the canonical CI environment to skip. §38.2 prefers this
    /// empty; §38.3 makes anything outside it a failure in both directions.
    pub canonical_ci: Vec<String>,
    /// The test ids whose skip depends on a host capability the repository does not control, each
    /// with the condition that decides it. Neither required nor forbidden — but listed with its
    /// reason, which is what §38.2 asks of an intentional skip.
    pub permitted: Vec<String>,
}

impl ExpectedSkips {
    /// Reads the registry from `root`.
    ///
    /// # Errors
    ///
    /// Returns a message naming the file when it is missing or does not hold the two lists.
    pub fn read(root: &Path) -> Result<Self, String> {
        let path = root.join(EXPECTED_TEST_SKIPS);
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("{EXPECTED_TEST_SKIPS}: {error}"))?;
        Self::parse(&text)
    }

    /// Reads the registry from its text.
    ///
    /// # Errors
    ///
    /// Returns a message when the document does not hold the two lists in the declared shape.
    pub fn parse(text: &str) -> Result<Self, String> {
        let document: serde_yaml_ng::Value = serde_yaml_ng::from_str(text)
            .map_err(|error| format!("{EXPECTED_TEST_SKIPS} is not readable YAML: {error}"))?;
        let mut declared = Vec::new();
        let rows = document
            .get("declared")
            .and_then(serde_yaml_ng::Value::as_sequence)
            .ok_or_else(|| format!("{EXPECTED_TEST_SKIPS} declares no `declared:` list"))?;
        for row in rows {
            let id = row
                .get("id")
                .and_then(serde_yaml_ng::Value::as_str)
                .ok_or_else(|| format!("{EXPECTED_TEST_SKIPS}: a declared skip has no `id`"))?;
            let category = row
                .get("category")
                .and_then(serde_yaml_ng::Value::as_str)
                .ok_or_else(|| format!("{EXPECTED_TEST_SKIPS}: `{id}` declares no `category`"))?;
            declared.push(SkipSite {
                id: id.to_owned(),
                category: category.to_owned(),
            });
        }
        let canonical_ci = document
            .get("canonical_ci")
            .and_then(|section| section.get("expected_skips"))
            .and_then(serde_yaml_ng::Value::as_sequence)
            .ok_or_else(|| {
                format!("{EXPECTED_TEST_SKIPS} declares no `canonical_ci.expected_skips:` list")
            })?
            .iter()
            .filter_map(|entry| entry.as_str().map(ToOwned::to_owned))
            .collect();
        let permitted = document
            .get("canonical_ci")
            .and_then(|section| section.get("permitted_skips"))
            .and_then(serde_yaml_ng::Value::as_sequence)
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| {
                        row.get("id")
                            .and_then(serde_yaml_ng::Value::as_str)
                            .map(ToOwned::to_owned)
                    })
                    .collect()
            })
            .unwrap_or_default();
        declared.sort();
        Ok(Self {
            declared,
            canonical_ci,
            permitted,
        })
    }
}

/// Every test in the tree that can announce a skip, with the category it announces.
///
/// A test announces directly when its body names a [`SkipReason`](ono_testkit::SkipReason), and
/// indirectly when a guard calls a helper in the same file that does — the shape `unprivileged()`
/// gives twenty-one callers. Both are read here, because §38.2's registry has to hold what a run
/// can actually produce rather than what is written at one of the two places.
#[must_use]
pub fn skip_sites(root: &Path) -> Vec<SkipSite> {
    let mut sites = Vec::new();
    for file in rust_sources(root) {
        let relative = relative(root, &file);
        if !relative.contains("/tests/") || is_scanner_source(&relative) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        sites.extend(skip_sites_in(&relative, &text));
    }
    sites.sort();
    sites.dedup();
    sites
}

/// The skip sites of one file's text.
fn skip_sites_in(relative: &str, text: &str) -> Vec<SkipSite> {
    let helpers = announcing_helper_categories(text);
    let mut sites = Vec::new();
    let mut current: Option<(String, Vec<String>)> = None;
    let mut depth = 0usize;
    let mut pending_test = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if current.is_none() {
            if is_test_attribute(trimmed) {
                pending_test = true;
            } else if pending_test && let Some(name) = function_name(trimmed) {
                current = Some((name, Vec::new()));
                pending_test = false;
                depth = 0;
            }
        }
        if let Some((_, categories)) = current.as_mut() {
            categories.extend(categories_named_on(line));
            for (helper, category) in &helpers {
                if line.contains(&format!("{helper}(")) {
                    categories.push(category.clone());
                }
            }
        }
        let (opens, closes) = brace_delta(line);
        depth += opens;
        depth = depth.saturating_sub(closes);
        if current.is_some()
            && depth == 0
            && (opens > 0 || closes > 0)
            && let Some((name, mut categories)) = current.take()
        {
            categories.sort();
            categories.dedup();
            for category in categories {
                sites.push(SkipSite {
                    id: format!("{relative}::{name}"),
                    category,
                });
            }
        }
    }
    sites
}

/// The six categories of v0.4.1 §38.4, as `SkipReason::category` spells them.
pub const SKIP_CATEGORIES: [&str; 6] = [
    "missing_kernel_feature",
    "missing_privilege",
    "unsupported_arch",
    "unsupported_distribution",
    "external_tool_unavailable",
    "fixture_not_applicable",
];

/// The §38.4 categories a line names, as `SkipReason::MissingPrivilege` names one.
///
/// Only the six count: `SkipReason::ALL` and `SkipReason::from_category` are the type's own
/// surface rather than a category, and a taxonomy that grew whatever an identifier happened to
/// spell would not be a taxonomy.
fn categories_named_on(line: &str) -> Vec<String> {
    let mut categories = Vec::new();
    for (index, _) in line.match_indices("SkipReason::") {
        let variant: String = line[index + "SkipReason::".len()..]
            .chars()
            .take_while(char::is_ascii_alphanumeric)
            .collect();
        let category = snake_case(&variant);
        if SKIP_CATEGORIES.contains(&category.as_str()) {
            categories.push(category);
        }
    }
    categories
}

/// `MissingKernelFeature` as `missing_kernel_feature`, the token §38.4 fixes.
fn snake_case(variant: &str) -> String {
    let mut out = String::new();
    for (index, character) in variant.chars().enumerate() {
        if character.is_uppercase() && index > 0 {
            out.push('_');
        }
        out.extend(character.to_lowercase());
    }
    out
}

/// The functions in `text` that announce a skip, and the category each announces.
///
/// A helper announces when its body calls `skipped` or `require`; the category is the first
/// `SkipReason::…` its body names, which is where rustfmt puts it whether the call fits on one
/// line or not.
fn announcing_helper_categories(text: &str) -> Vec<(String, String)> {
    let mut helpers = Vec::new();
    let mut current: Option<(String, bool, Option<String>)> = None;
    let mut depth = 0usize;
    for line in text.lines() {
        if current.is_none()
            && let Some(name) = function_name(line)
        {
            current = Some((name, false, None));
            depth = 0;
        }
        if let Some((_, announces, category)) = current.as_mut() {
            if line.contains("skipped(") || line.contains("require(") {
                *announces = true;
            }
            if category.is_none() {
                *category = categories_named_on(line).into_iter().next();
            }
        }
        let (opens, closes) = brace_delta(line);
        depth += opens;
        depth = depth.saturating_sub(closes);
        if current.is_some()
            && depth == 0
            && (opens > 0 || closes > 0)
            && let Some((name, announces, Some(category))) = current.take()
            && announces
        {
            helpers.push((name, category));
        }
    }
    helpers
}

/// Checks that the `expected_test_skips` registry names exactly the skips the tree can take.
///
/// §38.2 requires the expected skip set to be declared and its reasons listed in a
/// machine-readable file. A registry that has fallen behind the tree cannot do that: a skip
/// nobody declared is one nobody decided to permit, and a declaration whose test no longer skips
/// is a permission granted to nothing.
#[must_use]
pub fn check_expected_skips(root: &Path) -> Vec<Problem> {
    let expected = match ExpectedSkips::read(root) {
        Ok(expected) => expected,
        Err(message) => {
            return vec![Problem::new(EXPECTED_TEST_SKIPS, message)];
        }
    };
    let observed = skip_sites(root);
    let mut problems = Vec::new();
    for site in &observed {
        if !expected.declared.contains(site) {
            problems.push(Problem::new(
                site.id.clone(),
                format!(
                    "announces a `{}` skip that `{EXPECTED_TEST_SKIPS}` does not declare. A skip \
                     nobody declared is a skip nobody decided to permit (v0.4.1 §38.2)",
                    site.category
                ),
            ));
        }
    }
    for site in &expected.declared {
        if !observed.contains(site) {
            problems.push(Problem::new(
                site.id.clone(),
                format!(
                    "is declared in `{EXPECTED_TEST_SKIPS}` as a `{}` skip, and no test of that \
                     name announces one. Delete the row, or restore the skip it permits (v0.4.1 \
                     §38.2)",
                    site.category
                ),
            ));
        }
    }
    for id in expected
        .canonical_ci
        .iter()
        .chain(expected.permitted.iter())
    {
        if !expected.declared.iter().any(|site| &site.id == id) {
            problems.push(Problem::new(
                id.clone(),
                format!(
                    "is named under `canonical_ci` and is not in `{EXPECTED_TEST_SKIPS}`'s \
                     `declared:` list, so its reason is undeclared (v0.4.1 §38.2)"
                ),
            ));
        }
    }
    problems.sort_by(|left, right| left.location.cmp(&right.location));
    problems
}

/// Compares the skips a run actually took against the ones the canonical CI environment declares.
///
/// §38.3 is bidirectional and this is both halves of it: a skip outside the declared set fails,
/// *and* a declared skip that did not happen fails. The second is the one #14 needed — five
/// acceptance cases that had never run were green for as long as nobody counted them.
///
/// `log` is the output of a test run. The marker `ono_testkit::skipped` writes is
/// `SKIPPED <test>: <category>: <detail>`, so the run's own output is the observation and no
/// second mechanism has to agree with it.
#[must_use]
pub fn verify_observed_skips(expected: &ExpectedSkips, log: &str) -> Vec<Problem> {
    let observed = observed_skips(log);
    let mut problems = Vec::new();
    for (test, category) in &observed {
        let suffix = format!("::{test}");
        let known = expected
            .canonical_ci
            .iter()
            .chain(expected.permitted.iter())
            .any(|id| id.ends_with(&suffix));
        if !known {
            problems.push(Problem::new(
                test.clone(),
                format!(
                    "skipped with `{category}` and is not in `{EXPECTED_TEST_SKIPS}`'s \
                     `canonical_ci.expected_skips:` list. Either the environment stopped \
                     supplying a prerequisite, or the skip is intended and belongs in the list \
                     with its reason (v0.4.1 §38.2, §38.3)"
                ),
            ));
        }
    }
    for id in &expected.canonical_ci {
        let test = id.rsplit("::").next().unwrap_or(id);
        if !observed.iter().any(|(name, _)| name == test) {
            problems.push(Problem::new(
                id.clone(),
                "is expected to skip in this environment and did not. A test that starts \
                 running again is good news, and the expectation has to say so — remove the row \
                 (v0.4.1 §38.3)",
            ));
        }
    }
    problems.sort_by(|left, right| left.location.cmp(&right.location));
    problems
}

/// The `SKIPPED <test>: <category>: <detail>` markers a run left in `log`.
fn observed_skips(log: &str) -> Vec<(String, String)> {
    let mut observed = Vec::new();
    for line in log.lines() {
        let Some(rest) = line.trim_start().strip_prefix("SKIPPED ") else {
            continue;
        };
        let Some((test, rest)) = rest.split_once(": ") else {
            continue;
        };
        let category = rest.split(':').next().unwrap_or_default().trim();
        observed.push((test.trim().to_owned(), category.to_owned()));
    }
    observed.sort();
    observed.dedup();
    observed
}

// --- v0.4.1 §39.1 and §39.2: one helper per job ------------------------------------------------

/// A helper function defined in test code, and what it does with its arguments.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Helper {
    /// Where it is defined, as `path.rs::name`.
    id: String,
    /// The crate whose `tests/` tree it lives in — the home §39.1 would move it to.
    home: String,
    /// Its parameter and return types with the names stripped, and its body with comments,
    /// whitespace and formatting removed: what §39.2 means by "semantics/signatures rather than
    /// fragile exact source strings".
    shape: String,
}

/// Checks that no two test helpers with the same home do the same job under two definitions.
///
/// §39.1 puts the common helpers — invoking `ono`, decoding rows, constructing fixtures — in
/// shared modules *"rather than dozens of near-identical local variants"*, and §39.2 asks the
/// gate for a structural check that stops the variants coming back.
///
/// ADR-0427 is the standing decision on when a helper may be shared, and this rule holds it
/// rather than going past it. Three properties have to hold before a pair is reported, and each
/// of them is one of ADR-0427's reasons written as code:
///
/// * **the signature and the body match** once comments, whitespace and formatting are removed.
///   A variant is not a duplicate: two suites that need different budgets, different panic
///   messages or a different tolerance for stderr keep two helpers, and unifying them would
///   change what a test does — the one thing a refactor may not do (AGENTS.md §11);
/// * **the body is self-contained**, calling no other function its own file defines. ADR-0427
///   left `files.rs::single_result` where it was for exactly this: its body is identical to three
///   others and it calls `files.rs::text`, which names an `ActionResult` field in its panic. Two
///   identical bodies over two different callees are two behaviours, and moving one would change
///   the diagnostic a reader depends on;
/// * **the copies share a home.** §39.2 asks for the check *"where a canonical helper exists"*,
///   and a home is `crates/<crate>/tests/support/mod.rs` for one crate's suites or
///   `xtask/tests/support/mod.rs` for the automation's. A pair spanning two crates has no home
///   that does not put a crate's own types into `ono-testkit`, which ADR-0427 rejected because
///   the testkit is meant to be neutral about the crates it serves.
///
/// The name is not part of the comparison. ADR-0427 found one name over eleven behaviours; this
/// is the same defect from the other end — one behaviour under two names is a helper somebody
/// could not find, so they wrote it again.
///
/// A body under eighty normalised characters is left alone. A two-line accessor
/// is not a helper anybody consolidates, and a rule that reported it would be a rule people turn
/// off.
#[must_use]
pub fn check_duplicate_helpers(root: &Path) -> Vec<Problem> {
    let mut helpers: Vec<Helper> = Vec::new();
    let mut seen_paths: Vec<String> = Vec::new();
    for file in rust_sources(root) {
        let relative = relative(root, &file);
        if !relative.contains("/tests/") || is_scanner_source(&relative) {
            continue;
        }
        // A `support/mod.rs` is `#[path]`-included into several test binaries; it is one file and
        // one definition, however many suites read it.
        if seen_paths.contains(&relative) {
            continue;
        }
        seen_paths.push(relative.clone());
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        helpers.extend(helpers_in(&relative, &text));
    }

    let mut groups: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for helper in helpers {
        groups
            .entry((helper.home, helper.shape))
            .or_default()
            .push(helper.id);
    }

    let mut problems = Vec::new();
    for ((home, _), mut members) in groups {
        if members.len() < 2 {
            continue;
        }
        members.sort();
        members.dedup();
        if members.len() < 2 {
            continue;
        }
        let named = members.join(", ");
        for member in &members {
            problems.push(Problem::new(
                member.clone(),
                format!(
                    "is one of {} definitions of the same helper: {named}. §39.1 wants one shared \
                     definition per job; move it to `{home}/tests/support/mod.rs` and call it from \
                     each suite. If the copies must differ, they are not copies — make the \
                     difference visible in the body and record why (ADR-0427, §39.2, §39.4)",
                    members.len()
                ),
            ));
        }
    }
    problems.sort_by(|left, right| left.location.cmp(&right.location));
    problems
}

/// The shortest normalised body worth comparing. Below it a match is a coincidence of shape
/// rather than a helper somebody duplicated.
const TRIVIAL_HELPER_BODY: usize = 80;

/// The tree a helper's copies would share a canonical module in, or `None` when they would not.
fn helper_home(relative: &str) -> Option<String> {
    if let Some(rest) = relative.strip_prefix("crates/") {
        let crate_name = rest.split('/').next()?;
        return Some(format!("crates/{crate_name}"));
    }
    relative.starts_with("xtask/").then(|| "xtask".to_owned())
}

/// Every non-test, self-contained function defined in one file's text, with its shape.
fn helpers_in(relative: &str, text: &str) -> Vec<Helper> {
    let Some(home) = helper_home(relative) else {
        return Vec::new();
    };
    let lines: Vec<&str> = text.lines().collect();
    let declared: Vec<String> = lines
        .iter()
        .filter_map(|line| function_name(line))
        .collect();

    let mut helpers = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let Some(name) = function_name(lines[index]) else {
            index += 1;
            continue;
        };
        // A test is not a helper: two tests asserting the same thing are a duplicated *test*,
        // which is a different finding and not this rule's.
        if lines[index.saturating_sub(6)..index]
            .iter()
            .any(|above| is_test_attribute(above.trim()))
        {
            index += 1;
            continue;
        }
        let (signature, body, end) = function_text(&lines, index);
        index = end.max(index) + 1;
        let Some(body) = body else { continue };
        let normalised = normalise_source(&body);
        if normalised.len() < TRIVIAL_HELPER_BODY {
            continue;
        }
        // A body that calls one of its own file's helpers means whatever that helper means, so
        // two identical bodies over two different callees are two behaviours (ADR-0427).
        if declared
            .iter()
            .any(|other| *other != name && body.contains(&format!("{other}(")))
        {
            continue;
        }
        helpers.push(Helper {
            id: format!("{relative}::{name}"),
            home: home.clone(),
            shape: format!(
                "{}|{normalised}",
                normalise_source(&signature_shape(&signature))
            ),
        });
    }
    helpers
}

/// The signature, the body and the last line of the function that starts at `start`.
fn function_text(lines: &[&str], start: usize) -> (String, Option<String>, usize) {
    let mut signature = String::new();
    let mut body = String::new();
    let mut depth = 0usize;
    let mut started = false;
    for (offset, current) in lines[start..].iter().enumerate() {
        let (opens, closes) = brace_delta(current);
        if started {
            body.push_str(current);
            body.push('\n');
        } else {
            signature.push_str(current);
            if opens > 0 {
                started = true;
                if let Some((_, rest)) = current.split_once('{') {
                    body.push_str(rest);
                    body.push('\n');
                }
            }
        }
        depth += opens;
        depth = depth.saturating_sub(closes);
        if started && depth == 0 {
            return (signature, Some(body), start + offset);
        }
    }
    (signature, None, start)
}

/// A signature with its parameter *names* removed, leaving what the helper takes and answers.
fn signature_shape(signature: &str) -> String {
    let Some(open) = signature.find('(') else {
        return signature.to_owned();
    };
    signature[open..]
        .split(',')
        .map(|parameter| parameter.rsplit(':').next().unwrap_or(parameter))
        .collect::<Vec<_>>()
        .join(",")
}

/// Source with its comments, its whitespace and its formatting removed.
fn normalise_source(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let code = match line.find("//") {
            Some(position) if !line[..position].contains('"') => &line[..position],
            _ => line,
        };
        out.extend(code.split_whitespace());
    }
    out
}

// --- v0.4.1 §65.10 at a terminal: an assertion an earlier repaint can satisfy ------------------

/// How far after a resize the assertion about it may be written.
const RESIZE_ASSERTION_WINDOW: usize = 30;

/// Checks that a test which resizes a terminal asserts on the size it resized to.
///
/// Issue #6 is the defect this rule is made of. `should_preserve_the_current_place_when_the_terminal_is_resized_with_a_place_open`
/// resized a pseudo-terminal and then waited for *new output naming the place* — which the
/// repaint the earlier arrow key was still producing already satisfied. Tracing the map loop
/// during a green run found sessions whose whole key history was `Down`, `Esc`, with no resize
/// event in them at all. The test passed, §43.4 was held by nothing, and a test that can pass
/// without exercising its subject is §65.10's skip-as-pass in a different costume: it is worse
/// than one that flakes, because nobody looks at it.
///
/// The rule is narrow enough to be mechanical. A resize is written
/// `resize(WindowSize::new(<rows>, <columns>))`, and the assertion that follows it must name the
/// row count it resized to — or the resize itself, as the signal it delivers or the size the
/// terminal now reports — within thirty lines. A frame at a stated row count
/// is something only a resize produces; "new output" is not.
///
/// What it cannot check is whether the assertion is a *good* one — a scanner cannot tell a frame
/// from a comment. What it can insist on is that the new size appears in the assertion at all,
/// which is exactly what the old test did not do.
#[must_use]
pub fn check_pty_resize_assertions(root: &Path) -> Vec<Problem> {
    let mut problems = Vec::new();
    for file in rust_sources(root) {
        let relative = relative(root, &file);
        if !relative.contains("/tests/") || is_scanner_source(&relative) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        for (number, line) in lines.iter().enumerate() {
            let Some(rows) = resized_rows(line) else {
                continue;
            };
            let window =
                &lines[number + 1..(number + 1 + RESIZE_ASSERTION_WINDOW).min(lines.len())];
            if window
                .iter()
                .any(|later| mentions_number(later, rows) || names_the_resize_itself(later))
            {
                continue;
            }
            problems.push(Problem::new(
                format!("{relative}:{}", number + 1),
                format!(
                    "resizes the terminal to {rows} rows and asserts nothing about that number. \
                     An assertion that only asks for new output is satisfied by the repaint an \
                     earlier key was still producing, so it passes on a run where the resize \
                     never arrived — which is what issue #6 found. Assert on the frame at the new \
                     row count (v0.4.1 §43.4, §65.10, ADR-0519)"
                ),
            ));
        }
    }
    problems.sort_by(|left, right| left.location.cmp(&right.location));
    problems
}

/// Whether a line asserts on the resize rather than on the size: the signal the kernel sends, or
/// the size the terminal now reports.
///
/// `should_deliver_sigwinch_to_the_child_when_the_window_changes` waits for a `SIGWINCH` trap to
/// fire, which is something only a resize produces and which no row count appears in. The rule is
/// that the assertion names the resize; the row count is the usual way and not the only one.
fn names_the_resize_itself(line: &str) -> bool {
    line.contains("WINCH") || line.contains("window_size(")
}

/// The row count a `resize(WindowSize::new(<rows>, <columns>))` call asks for.
fn resized_rows(line: &str) -> Option<u32> {
    let at = line.find("resize(WindowSize::new(")?;
    let rest = &line[at + "resize(WindowSize::new(".len()..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// Whether a line names `number` as a number rather than as part of a longer one.
fn mentions_number(line: &str, number: u32) -> bool {
    let needle = number.to_string();
    let bytes = line.as_bytes();
    line.match_indices(&needle).any(|(at, _)| {
        let before = at.checked_sub(1).map(|index| bytes[index]);
        let after = bytes.get(at + needle.len()).copied();
        !before.is_some_and(|byte| byte.is_ascii_digit() || byte == b'_')
            && !after.is_some_and(|byte| byte.is_ascii_digit() || byte == b'_')
    })
}
