//! The release evidence of `docs/ACCEPTANCE.md` §4.11, as rules a test can run.
//!
//! §4.11 is the v0.5 temporal tranche's definition of done: a hundred and twenty-three boxes, each
//! naming the automated proof that closes it. It was written before the tranche, which is what
//! makes it worth holding — a checklist whose proofs nobody resolves is a checklist that rots, and
//! §4.8 grew `xtask/tests/hardening_evidence.rs` for exactly that reason.
//!
//! The rules live here rather than in the test binary so that each of them can be run twice: once
//! over the repository, where today they pass over an open checklist, and once over a scratch copy
//! mutated to carry the defect, which is the only way to tell a guard that works from a guard that
//! resolves nothing. `xtask/tests/temporal_evidence.rs` is the suite that does both.
//!
//! What a box may name as its proof is fixed by §3 and by §4.11's own preamble:
//!
//! * a path in the tree, optionally with the `::item` it must declare — a test file and the test
//!   in it, a scanner and the check it defines, a contract registry, a document;
//! * an acceptance case, backticked once the file exists and written in prose while it does not,
//!   which is ADR-0401's convention and the one `xtask::scan::check_acceptance_case_references`
//!   reads;
//! * a command, resolved as far as a command sensibly can be: `cargo run -p xtask -- <task>` is a
//!   claim that `xtask` dispatches that task. Whether the run then passes is the gate's verdict,
//!   not this module's.

use std::path::{Path, PathBuf};

use crate::scan::Problem;

/// The heading §4.11 opens with.
const HEADING: &str = "### 4.11 The v0.5 tranche";

/// Where §4.11's passage ends: at the next tranche, or at the stopping rule that follows the last
/// one. §4.11 is the last today, so the second marker is the one in force.
const ENDS: [&str; 2] = ["\n### 4.12", "\n## 5. Stopping rule"];

/// The lowest and highest acceptance-case number §4.11's preamble claims for the tranche.
const BLOCK: std::ops::RangeInclusive<u32> = 230..=279;

/// The top-level trees a proof may name a path in.
const ROOTS: [&str; 6] = ["crates/", "xtask/", "fuzz/", "docker/", "docs/", "scripts/"];

/// The workspace members `cargo test --workspace` runs, so a Rust proof outside them is a proof
/// the gate never executes (ADR-0313 put `fuzz` there for that reason).
const MEMBERS: [&str; 3] = ["crates/", "xtask/", "fuzz/"];

/// Where the acceptance cases live.
const CASES: &str = "docker/acceptance/cases";

/// The text of §4.11 in the `docs/ACCEPTANCE.md` under `root`, or `None` when the document carries
/// no such subsection.
///
/// The passage begins at §4.11's own heading and ends at the earliest of the markers that follow
/// it, so this harvester and §4.9's read one checklist each and neither answers for the other's
/// boxes. The stopping rule is excluded deliberately: it talks *about* boxes, and reading its
/// prose as a checklist would have §4.11 proving whatever §5 happens to mention.
#[must_use]
pub fn checklist(root: &Path) -> Option<String> {
    let acceptance = std::fs::read_to_string(root.join("docs/ACCEPTANCE.md")).ok()?;
    let start = acceptance.find(HEADING)?;
    let end = ENDS
        .iter()
        .filter_map(|marker| acceptance[start..].find(marker))
        .min()
        .map_or(acceptance.len(), |offset| start + offset);
    Some(acceptance[start..end].to_owned())
}

/// One box of the checklist: whether it is ticked, where it stands, and what it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkbox {
    /// Whether the box is `- [x]` rather than `- [ ]`.
    pub ticked: bool,
    /// The `4.11.N` subsubsection the box stands under.
    pub subsection: String,
    /// The bolded phrase the box opens with, or an empty string when it has none.
    pub title: String,
    /// The whole box, its continuation lines joined onto the first.
    pub text: String,
}

/// Every box of a checklist passage, with the continuation lines folded into the box they belong
/// to.
///
/// A box's continuations are indented under it; a heading or a paragraph at column zero belongs to
/// the subsection rather than to the box above it, and reading it as one would have a box naming
/// whatever the prose beside it happened to mention.
#[must_use]
pub fn checkboxes(passage: &str) -> Vec<Checkbox> {
    let mut found: Vec<Checkbox> = Vec::new();
    let mut subsection = String::from("4.11");
    let mut open = false;
    for line in passage.lines() {
        if let Some(heading) = line.strip_prefix("#### ") {
            subsection = heading
                .split_whitespace()
                .next()
                .unwrap_or("4.11")
                .to_owned();
            open = false;
            continue;
        }
        let ticked = match line.trim_start() {
            rest if rest.starts_with("- [x] ") => Some(true),
            rest if rest.starts_with("- [ ] ") => Some(false),
            _ => None,
        };
        if let Some(ticked) = ticked {
            let text = line.trim_start()[6..].trim().to_owned();
            found.push(Checkbox {
                ticked,
                subsection: subsection.clone(),
                title: bolded_title(&text),
                text,
            });
            open = true;
            continue;
        }
        let trimmed = line.trim_start();
        if open && !trimmed.is_empty() && line.starts_with(char::is_whitespace) {
            if let Some(last) = found.last_mut() {
                last.text.push(' ');
                last.text.push_str(trimmed);
                last.title = bolded_title(&last.text);
            }
        } else {
            open = false;
        }
    }
    found
}

/// The phrase between a box's first pair of `**`, or an empty string when it has none.
fn bolded_title(text: &str) -> String {
    text.strip_prefix("**")
        .and_then(|rest| rest.split_once("**"))
        .map(|(title, _)| title.trim().to_owned())
        .unwrap_or_default()
}

/// What a box names as the automated proof that closes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Proof {
    /// A path in the tree, and the `::item` the file must declare where the box names one.
    Tree {
        /// The path, relative to the repository root.
        path: String,
        /// The function the file must declare, where the box names one.
        item: Option<String>,
    },
    /// An acceptance case the referee collects.
    Case {
        /// The case's `NNN-kebab-name`, without the `.case` suffix.
        name: String,
        /// Whether the checklist claims the case in backticks (ADR-0401) rather than recording it
        /// as a name no file answers to yet.
        claimed: bool,
    },
    /// A command, named as the thing that produces the evidence.
    Command {
        /// The command as the checklist writes it.
        line: String,
    },
}

/// Every proof a passage names, in the order it names them.
#[must_use]
pub fn proofs(text: &str) -> Vec<Proof> {
    let mut found = Vec::new();
    for (segment, quoted) in segments(text) {
        if !quoted {
            found.extend(case_names(segment).into_iter().map(|name| Proof::Case {
                name,
                claimed: false,
            }));
            continue;
        }
        if let Some(name) = case_names(segment).into_iter().next()
            && name == segment
        {
            found.push(Proof::Case {
                name,
                claimed: true,
            });
        } else if segment.starts_with("cargo ") {
            found.push(Proof::Command {
                line: segment.to_owned(),
            });
        } else if let Some(proof) = tree_proof(segment) {
            found.push(proof);
        }
    }
    found
}

/// A passage's text as alternating unquoted and backticked segments.
fn segments(text: &str) -> Vec<(&str, bool)> {
    text.split('`')
        .enumerate()
        .map(|(index, segment)| (segment, index % 2 == 1))
        .collect()
}

/// The path proof a backticked token carries, or `None` when the token is prose.
fn tree_proof(token: &str) -> Option<Proof> {
    if token.contains(char::is_whitespace) {
        return None;
    }
    let (path, item) = match token.split_once("::") {
        Some((path, item)) if is_identifier(item) => (path, Some(item.to_owned())),
        Some(_) => return None,
        None => (token, None),
    };
    let named = ROOTS.iter().any(|root| path.starts_with(root))
        || (path.ends_with(".rs") && !path.contains('/'));
    named.then(|| Proof::Tree {
        path: path.to_owned(),
        item,
    })
}

/// Whether a token is a Rust identifier, which is what a `::item` suffix has to be.
fn is_identifier(token: &str) -> bool {
    !token.is_empty()
        && token.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !token.starts_with(|c: char| c.is_ascii_digit())
}

/// Every `NNN-kebab-name` in a segment of text.
///
/// A case name is three digits, a hyphen and lowercase kebab; a number range such as `230-234`
/// carries no letter and is not one, which is what keeps a figure in prose from being read as a
/// scenario nobody wrote.
fn case_names(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut found = Vec::new();
    let mut index = 0;
    while index + 4 <= chars.len() {
        let digits = chars[index..index + 3].iter().all(char::is_ascii_digit);
        let boundary = index == 0 || !chars[index - 1].is_ascii_alphanumeric();
        if digits && boundary && chars[index + 3] == '-' {
            let mut end = index + 4;
            while end < chars.len()
                && (chars[end].is_ascii_lowercase()
                    || chars[end].is_ascii_digit()
                    || chars[end] == '-')
            {
                end += 1;
            }
            let name: String = chars[index..end].iter().collect();
            if name.contains(|c: char| c.is_ascii_lowercase()) && !name.ends_with('-') {
                found.push(name);
                index = end;
                continue;
            }
        }
        index += 1;
    }
    found
}

/// How far the tranche has come.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// The boxes a proof has closed.
    pub ticked: usize,
    /// The boxes still open, which is what `scripts/release-check.sh` fails on.
    pub open: usize,
}

impl Progress {
    /// Every box the checklist holds.
    #[must_use]
    pub const fn total(&self) -> usize {
        self.ticked + self.open
    }
}

/// How many of a checklist's boxes are ticked, and how many are still open.
#[must_use]
pub fn progress(passage: &str) -> Progress {
    let boxes = checkboxes(passage);
    let ticked = boxes.iter().filter(|checkbox| checkbox.ticked).count();
    Progress {
        ticked,
        open: boxes.len() - ticked,
    }
}

/// Checks that every proof a *ticked* box names exists, runs where the gate runs it, and is not
/// `#[ignore]`d.
///
/// An open box is left alone. §4.11 was written before the tranche it defines, so the proofs its
/// open boxes name are what their delivering increments owe; the box that may not name a proof
/// nobody wrote is the box that claims one already did.
#[must_use]
pub fn check_ticked_proofs(root: &Path, passage: &str) -> Vec<Problem> {
    let mut problems = Vec::new();
    for checkbox in checkboxes(passage) {
        if !checkbox.ticked {
            continue;
        }
        let at = format!("docs/ACCEPTANCE.md §{}", checkbox.subsection);
        let title = &checkbox.title;
        let found = proofs(&checkbox.text);
        if found.is_empty() {
            problems.push(Problem::new(
                at,
                format!(
                    "`{title}` is ticked and names no automated proof. §3 closes a box with a \
                     test that runs un-ignored in `scripts/gate.sh` or a case that runs in \
                     `scripts/acceptance.sh` — never by judgement"
                ),
            ));
            continue;
        }
        for proof in found {
            problems.extend(resolve(root, &at, title, &proof));
        }
    }
    problems
}

/// Every problem one proof of a ticked box carries.
fn resolve(root: &Path, at: &str, title: &str, proof: &Proof) -> Vec<Problem> {
    match proof {
        Proof::Tree { path, item } => resolve_tree(root, at, title, path, item.as_deref()),
        Proof::Case { name, .. } => {
            if root.join(CASES).join(format!("{name}.case")).is_file() {
                Vec::new()
            } else {
                vec![Problem::new(
                    at.to_owned(),
                    format!(
                        "`{title}` is ticked on case `{name}`, and `{CASES}/{name}.case` is not a \
                         file the referee collects. §2: a capability without a passing acceptance \
                         case is not delivered"
                    ),
                )]
            }
        }
        Proof::Command { line } => resolve_command(root, at, title, line),
    }
}

/// Every problem a path proof carries: the file, and the item it must declare.
fn resolve_tree(
    root: &Path,
    at: &str,
    title: &str,
    path: &str,
    item: Option<&str>,
) -> Vec<Problem> {
    let Some(resolved) = locate(root, path) else {
        return vec![Problem::new(
            at.to_owned(),
            format!("`{title}` is ticked on `{path}`, which is not in the tree"),
        )];
    };
    if path.ends_with(".rs") && !MEMBERS.iter().any(|member| path.starts_with(member)) {
        return vec![Problem::new(
            at.to_owned(),
            format!(
                "`{title}` is ticked on `{path}`, which is outside the workspace members \
                 `cargo test --workspace` runs"
            ),
        )];
    }
    let Some(item) = item else {
        return Vec::new();
    };
    let Ok(source) = std::fs::read_to_string(&resolved) else {
        return vec![Problem::new(
            at.to_owned(),
            format!("`{title}` is ticked on `{path}`, which cannot be read"),
        )];
    };
    match declaration(&source, item) {
        None => vec![Problem::new(
            at.to_owned(),
            format!(
                "`{title}` is ticked on `{path}::{item}`, and the file declares no `fn {item}`. \
                 Rename it in the checklist in the increment that renames the test"
            ),
        )],
        Some(true) => vec![Problem::new(
            at.to_owned(),
            format!(
                "`{title}` is ticked on `{path}::{item}`, which is `#[ignore]`d — a box ticked by \
                 an ignored test is a box ticked by nothing"
            ),
        )],
        Some(false) => Vec::new(),
    }
}

/// Every problem a command proof carries.
///
/// `cargo run -p xtask -- <task>` is resolved as far as the task being dispatched by
/// `xtask/src/main.rs`. Anything else is left alone: a harvester that guessed at a shell line
/// would report what it failed to understand rather than what is wrong.
fn resolve_command(root: &Path, at: &str, title: &str, line: &str) -> Vec<Problem> {
    let Some(rest) = line.split_once("cargo run -p xtask -- ") else {
        return Vec::new();
    };
    let Some(task) = rest.1.split_whitespace().next() else {
        return Vec::new();
    };
    let Ok(main) = std::fs::read_to_string(root.join("xtask/src/main.rs")) else {
        return Vec::new();
    };
    if main.contains(&format!("Some(\"{task}\")")) {
        return Vec::new();
    }
    vec![Problem::new(
        at.to_owned(),
        format!("`{title}` is ticked on `{line}`, and `xtask` dispatches no `{task}` task"),
    )]
}

/// Where a path a checklist names actually lives, or `None` when nothing answers to the name.
///
/// A trailing slash names a directory — §4.11 points at `crates/ono-temporal-render/tests/` and at
/// `docs/reference/` that way — and everything else names a file. A bare `file.rs`, the short form
/// §4.8 uses, is looked for under each crate's `tests/` and under `xtask/tests/`.
fn locate(root: &Path, path: &str) -> Option<PathBuf> {
    if path.contains('/') {
        let candidate = root.join(path);
        let found = if path.ends_with('/') {
            candidate.is_dir()
        } else {
            candidate.is_file()
        };
        return found.then_some(candidate);
    }
    let direct = root.join("xtask/tests").join(path);
    if direct.is_file() {
        return Some(direct);
    }
    std::fs::read_dir(root.join("crates"))
        .ok()?
        .flatten()
        .map(|entry| entry.path().join("tests").join(path))
        .find(|candidate| candidate.is_file())
}

/// Whether `source` declares `item`, and whether the declaration is `#[ignore]`d.
fn declaration(source: &str, item: &str) -> Option<bool> {
    let at = source.find(&format!("fn {item}("))?;
    let ignored = source[..at]
        .lines()
        .rev()
        .take_while(|line| {
            let line = line.trim_start();
            line.starts_with('#') || line.starts_with("//") || line.is_empty()
        })
        .any(|line| line.trim_start().starts_with("#[ignore"));
    Some(ignored)
}

/// Checks that the subsection keeps the shape its readers depend on.
///
/// `scripts/release-check.sh` greps `^- \[ \]` and fails on the first hit, so a box indented under
/// something else exempts itself from the stopping rule without anyone deciding that it may. The
/// bolded title is what `xtask::scan`'s release-notes check quotes, and the `#### 4.11.N` headings
/// are the reader's map of the tranche: one per area, in order, with nothing missing between them.
#[must_use]
pub fn check_checklist_shape(passage: &str) -> Vec<Problem> {
    let mut problems = Vec::new();
    let mut subsection = String::from("4.11");
    let mut expected = 1_u32;
    let mut headed = false;
    for line in passage.lines() {
        if let Some(heading) = line.strip_prefix("#### ") {
            let number = heading.split_whitespace().next().unwrap_or_default();
            if number != format!("4.11.{expected}") {
                problems.push(Problem::new(
                    format!("docs/ACCEPTANCE.md §{subsection}"),
                    format!(
                        "`#### {heading}` follows §{subsection}, and §4.11's subsubsections run \
                         from 4.11.1 upwards without gaps — §4.11.{expected} is where this one \
                         should be numbered"
                    ),
                ));
            }
            subsection = number.to_owned();
            expected = number
                .rsplit('.')
                .next()
                .and_then(|last| last.parse::<u32>().ok())
                .unwrap_or(expected)
                + 1;
            headed = true;
            continue;
        }
        let trimmed = line.trim_start();
        if !trimmed.starts_with("- [") {
            continue;
        }
        let at = format!("docs/ACCEPTANCE.md §{subsection}");
        if !line.starts_with("- [") {
            problems.push(Problem::new(
                at,
                format!(
                    "`{trimmed}` is a box away from the left margin, where \
                     `scripts/release-check.sh`'s `^- \\[ \\]` cannot see it — an indented box \
                     exempts itself from the stopping rule"
                ),
            ));
            continue;
        }
        if !line.starts_with("- [x] ") && !line.starts_with("- [ ] ") {
            problems.push(Problem::new(
                at,
                format!(
                    "`{trimmed}` is not a box the checklist's readers share: a box is written \
                     `- [x] ` or `- [ ] `"
                ),
            ));
            continue;
        }
        if !headed {
            problems.push(Problem::new(
                at,
                format!("`{trimmed}` stands above §4.11's first `####` heading"),
            ));
        }
    }
    for checkbox in checkboxes(passage) {
        if checkbox.title.is_empty() {
            problems.push(Problem::new(
                format!("docs/ACCEPTANCE.md §{}", checkbox.subsection),
                format!(
                    "`{}` has no bolded title, and a box is read by the phrase it opens with",
                    checkbox.text
                ),
            ));
        }
    }
    problems
}

/// Checks that the acceptance-case numbers §4.11 claims belong to it and to nothing else.
///
/// The claim is exclusive in both directions. A case file inside the block that no box names is a
/// scenario the checklist forgot; a case the checklist still owes outside the block is a scenario
/// numbered into another tranche's range. An earlier tranche's case cited as a regression proof —
/// §4.11.11 names v0.4's `060-performance-budgets` — is left alone, because it exists and is
/// claimed as ADR-0401 asks.
#[must_use]
pub fn check_case_block(root: &Path, passage: &str) -> Vec<Problem> {
    let mut problems = Vec::new();
    let mentioned = mentions(passage);
    let collected = collected_cases(root);

    let mut numbered: Vec<(u32, String)> = Vec::new();
    for (name, claimed) in &mentioned {
        let exists = collected.iter().any(|case| case == name);
        let number = case_number(name);
        if let Some(number) = number {
            numbered.push((number, name.clone()));
        }
        let at = "docs/ACCEPTANCE.md §4.11".to_owned();
        match (claimed, exists) {
            (true, false) => problems.push(Problem::new(
                at,
                format!(
                    "claims case `{name}` in backticks and `{CASES}/{name}.case` is not there. \
                     Write the case, or record the name in prose until the increment that owes it \
                     lands (ADR-0401)"
                ),
            )),
            (false, true) => problems.push(Problem::new(
                at,
                format!(
                    "records case {name} as a name no file answers to, and \
                     `{CASES}/{name}.case` is there. A case gains its backticks in the increment \
                     that writes it (ADR-0401)"
                ),
            )),
            (false, false) => {
                if number.is_none_or(|number| !BLOCK.contains(&number)) {
                    problems.push(Problem::new(
                        at,
                        format!(
                            "owes case {name}, which is numbered outside the {}–{} block §4.11 \
                             claims for the tranche",
                            BLOCK.start(),
                            BLOCK.end()
                        ),
                    ));
                }
            }
            (true, true) => {}
        }
    }

    numbered.sort();
    numbered.dedup();
    for pair in numbered.windows(2) {
        if pair[0].0 == pair[1].0 {
            problems.push(Problem::new(
                "docs/ACCEPTANCE.md §4.11".to_owned(),
                format!(
                    "names two cases under the number {}: `{}` and `{}`. One number is one \
                     scenario",
                    pair[0].0, pair[0].1, pair[1].1
                ),
            ));
        }
    }

    for case in &collected {
        let Some(number) = case_number(case) else {
            continue;
        };
        if BLOCK.contains(&number) && !mentioned.iter().any(|(name, _)| name == case) {
            problems.push(Problem::new(
                format!("{CASES}/{case}.case"),
                format!(
                    "is numbered inside the {}–{} block §4.11 claims for the v0.5 tranche, and no \
                     box of §4.11 names it — a scenario the referee runs and the checklist does \
                     not know about",
                    BLOCK.start(),
                    BLOCK.end()
                ),
            ));
        }
    }
    problems
}

/// Every acceptance case a passage names, and whether it claims it in backticks.
fn mentions(passage: &str) -> Vec<(String, bool)> {
    segments(passage)
        .into_iter()
        .flat_map(|(segment, quoted)| {
            let names = case_names(segment);
            let claimed = quoted && names.len() == 1 && names[0] == segment;
            names.into_iter().map(move |name| (name, claimed))
        })
        .collect()
}

/// The three-digit number a case name opens with.
fn case_number(name: &str) -> Option<u32> {
    name.get(..3).and_then(|digits| digits.parse().ok())
}

/// Every case the referee collects, by name.
fn collected_cases(root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root.join(CASES)) else {
        return Vec::new();
    };
    let mut cases: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "case"))
        .filter_map(|entry| {
            entry
                .path()
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .collect();
    cases.sort();
    cases
}
