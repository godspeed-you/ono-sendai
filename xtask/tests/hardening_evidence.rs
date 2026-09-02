//! The release evidence of `docs/ACCEPTANCE.md` §4.8: the checklist of the v0.4.1 tranche is held
//! against the tree, so it cannot rot.
//!
//! `xtask/tests/spatial_evidence.rs` does this for §4.7 and stops at the `### 4.8` heading, so
//! the two checklists are read apart and neither harvester can silently answer for the other's
//! boxes (`docs/ACCEPTANCE.md` §4.8.1). This file is the v0.4.1 half.
//!
//! Everything here is a statement about *evidence* rather than about the shell.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "AGENTS.md §16: a helper shared by tests states its preconditions the same way a test does"
)]

mod support;
use support::{read, repo};

/// The fourteen families of v0.4.1 §40.3, in the order the specification lists them, typed out
/// from the specification rather than read from the checklist.
///
/// Reading them from the document the test is checking would make the test agree with whatever
/// the document said. Each entry is the phrase §4.8.13's box opens with.
const FAMILIES: [&str; 14] = [
    "Direct mutual TLS authentication",
    "Unknown client refusal",
    "Authorization-constrained capability negotiation",
    "Unauthorized action refusal",
    "Authorized exact action success",
    "Changed client key refusal",
    "Malformed authorization store fails closed at startup",
    "KUANG mandatory confinement setup failure",
    "`each` streams an unbounded source",
    "Materialization item and byte limits refuse",
    "Result-history truncation is visible",
    "Profile M spatial first result",
    "Live map cancellation under load",
    "Package signature, checksum and provenance",
];

/// The text of one `####` subsubsection of `docs/ACCEPTANCE.md`.
fn subsection(heading: &str) -> String {
    let acceptance = read("docs/ACCEPTANCE.md");
    let start = acceptance
        .find(heading)
        .unwrap_or_else(|| panic!("docs/ACCEPTANCE.md carries `{heading}`"));
    let end = acceptance[start..]
        .find("\n#### ")
        .map_or(acceptance.len(), |offset| start + offset);
    acceptance[start..end].to_owned()
}

/// One checklist box: its whole text, and whether it is ticked.
fn boxes(passage: &str) -> Vec<(bool, String)> {
    let mut found = Vec::new();
    for line in passage.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("- [x] ") {
            found.push((true, rest.to_owned()));
        } else if let Some(rest) = trimmed.strip_prefix("- [ ] ") {
            found.push((false, rest.to_owned()));
        } else if let Some((_, text)) = found.last_mut() {
            text.push(' ');
            text.push_str(trimmed);
        }
    }
    found
}

/// Every `NNN-kebab-name` a passage carries, whether or not it is inside backticks.
///
/// ADR-0401 reads a backticked name as a claim and a plain one as a name recorded absent, so both
/// spellings are collected here and the difference is what the test asserts about.
fn case_names(text: &str) -> Vec<(String, bool)> {
    let mut found = Vec::new();
    let bytes: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index + 4 <= bytes.len() {
        if bytes[index..index + 3].iter().all(char::is_ascii_digit) && bytes[index + 3] == '-' {
            let start = index;
            let mut end = index + 4;
            while end < bytes.len()
                && (bytes[end].is_ascii_alphanumeric() || bytes[end] == '-' || bytes[end] == '_')
            {
                end += 1;
            }
            let name: String = bytes[start..end].iter().collect();
            if name.contains(|c: char| c.is_ascii_alphabetic()) {
                let quoted =
                    start > 0 && bytes[start - 1] == '`' && end < bytes.len() && bytes[end] == '`';
                found.push((name, quoted));
            }
            index = end;
            continue;
        }
        index += 1;
    }
    found
}

#[test]
fn should_find_a_case_for_every_one_of_the_fourteen_acceptance_families() {
    // v0.4.1 §40.3 names fourteen families and the exit test of #91 is that each appears as a
    // numbered case. §4.8.13 holds one box per family, in §40.3's order, and this resolves every
    // pointer in it: a family whose case exists names it in backticks, and a family whose case
    // some later phase still owes names it in prose, which is §4.8's own convention (ADR-0401).
    // Nothing here reads a tick — the acceptance run is what proves a case green, and this is
    // what proves the checklist points at something.
    let passage = subsection("#### 4.8.13 The fourteen acceptance families");
    let families = boxes(&passage);
    assert_eq!(
        families.len(),
        FAMILIES.len(),
        "§40.3 names {} acceptance families and §4.8.13 holds {} boxes",
        FAMILIES.len(),
        families.len()
    );

    let mut owed = Vec::new();
    for (index, expected) in FAMILIES.iter().enumerate() {
        let (_, text) = &families[index];
        assert!(
            text.starts_with(&format!("**{expected}**")),
            "§4.8.13's box {} is §40.3's `{expected}` family, got {text:?}",
            index + 1
        );
        let named = case_names(text);
        let (name, quoted) = named.first().unwrap_or_else(|| {
            panic!("the `{expected}` family names no acceptance case: {text:?}")
        });
        let path = repo()
            .join("docker/acceptance/cases")
            .join(format!("{name}.case"));
        if path.is_file() {
            assert!(
                *quoted,
                "case `{name}` exists, so the `{expected}` family claims it in backticks rather \
                 than recording it absent (ADR-0401)"
            );
        } else {
            assert!(
                !*quoted,
                "the `{expected}` family claims case `{name}` in backticks and there is no such \
                 file. Write the case, or record the name in prose until the increment that owes \
                 it lands (ADR-0401)"
            );
            owed.push((expected, name.clone(), text.clone()));
        }
    }

    for (family, name, text) in &owed {
        assert!(
            text.contains("H11") || text.contains("H12"),
            "the `{family}` family's case `{name}` is not written yet, so the box says which \
             phase owes it — otherwise a family nobody delivered reads like one nobody noticed: \
             {text:?}"
        );
    }
    assert!(
        owed.len() <= 1,
        "at most one of §40.3's families may still be owed by a later phase, and {} are: {:?}",
        owed.len(),
        owed.iter().map(|(family, _, _)| family).collect::<Vec<_>>()
    );
}

#[test]
fn should_find_a_finite_timeout_on_every_v041_case() {
    // §40.4: "Every acceptance case MUST have a finite timeout. A timeout is a failure unless the
    // case explicitly asserts timeout behavior as its product result." The harness applies one to
    // every case; the v0.4.1 cases state their own, because a case that runs a benchmark under a
    // container is not a case the default was chosen for.
    let harness = read("scripts/acceptance.sh");
    assert!(
        harness.contains("want_timeout=\"30\""),
        "the harness gives every case a finite default timeout"
    );
    assert!(
        harness.contains("timeout --kill-after=5 \"$want_timeout\""),
        "the harness enforces the budget with `timeout`, so a case cannot outlive it"
    );
    assert!(
        harness.contains("problem=\"timed out after ${want_timeout}s\""),
        "§40.4: a timeout is reported as a failure of the case, not as a slow pass"
    );

    let cases = repo().join("docker/acceptance/cases");
    let mut without = Vec::new();
    for entry in std::fs::read_dir(&cases).expect("the case directory is readable") {
        let path = entry.expect("a directory entry").path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.ends_with(".case") {
            continue;
        }
        let number: u32 = name
            .split('-')
            .next()
            .and_then(|digits| digits.parse().ok())
            .unwrap_or_else(|| panic!("every case is numbered, and `{name}` is not"));
        if !(170..=200).contains(&number) {
            continue;
        }
        let text = read(&format!("docker/acceptance/cases/{name}"));
        let declared = text
            .lines()
            .find_map(|line| line.strip_prefix("timeout: "))
            .and_then(|value| value.trim().parse::<u32>().ok());
        match declared {
            Some(seconds) if seconds > 0 => {}
            _ => without.push(name.to_owned()),
        }
    }
    assert!(
        without.is_empty(),
        "§40.4: every v0.4.1 case states a finite, positive timeout of its own, and these do \
         not: {without:?}"
    );
}
