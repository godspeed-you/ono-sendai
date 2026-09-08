//! The release evidence of `docs/ACCEPTANCE.md` §4.9: the checklist of the KUANG/11 plugin
//! installation, resolution and permission layer (K11P) is held against the tree, so it cannot
//! rot — the way `xtask/tests/hardening_evidence.rs` holds §4.8.
//!
//! Everything here is a statement about *evidence* rather than about the shell:
//!
//! * every test §4.9 names as a proof exists, runs where the gate runs it, and is not
//!   `#[ignore]`d;
//! * every acceptance case §4.9 claims in backticks is a file the referee collects;
//! * every gate of K11P §33 and every security test of §34 has a box, so a criterion cannot lose
//!   its evidence by having its box rewritten away;
//! * no box is open;
//! * core carries no provider wording (Gate X): the layer is generic, and the reference
//!   provider's words live in its own package.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "AGENTS.md §16: a helper shared by tests states its preconditions the same way a test does"
)]

mod support;
use support::{assert_proofs_exist, read, repo};

/// The twenty-four gates of K11P §33, typed out from the specification rather than read from the
/// checklist, so the test agrees with the specification and not with whatever the document says.
const GATES: [&str; 24] = [
    "Gate A", "Gate B", "Gate C", "Gate D", "Gate E", "Gate F", "Gate G", "Gate H", "Gate I",
    "Gate J", "Gate K", "Gate L", "Gate M", "Gate N", "Gate O", "Gate P", "Gate Q", "Gate R",
    "Gate S", "Gate T", "Gate U", "Gate V", "Gate W", "Gate X",
];

/// The eight security tests of K11P §34.
const SECURITY: [&str; 8] = [
    "§34.1", "§34.2", "§34.3", "§34.4", "§34.5", "§34.6", "§34.7", "§34.8",
];

/// The text of `docs/ACCEPTANCE.md` §4.9, from its heading to the next tranche's or the stopping
/// rule, so this harvester and §4.8's read one checklist each.
fn checklist() -> String {
    section(
        "### 4.9 The KUANG/11 plugin installation",
        &["\n### 4.10", "\n## 5. Stopping rule"],
    )
}

/// The text of §4.10, the acquisition and system-distribution addendum (K11A).
fn addendum() -> String {
    section(
        "### 4.10 The KUANG/11 plugin package acquisition",
        &["\n### 4.11", "\n## 5. Stopping rule"],
    )
}

fn section(heading: &str, ends: &[&str]) -> String {
    let acceptance = read("docs/ACCEPTANCE.md");
    let start = acceptance
        .find(heading)
        .unwrap_or_else(|| panic!("docs/ACCEPTANCE.md carries `{heading}`"));
    let end = ends
        .iter()
        .find_map(|marker| acceptance[start..].find(marker))
        .map_or(acceptance.len(), |offset| start + offset);
    acceptance[start..end].to_owned()
}

/// Every box of §4.9: whether it is ticked, and its text.
fn boxes() -> Vec<(bool, String)> {
    boxes_of(&checklist())
}

fn boxes_of(passage: &str) -> Vec<(bool, String)> {
    let mut found = Vec::new();
    for line in passage.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("- [x] ") {
            found.push((true, rest.to_owned()));
        } else if let Some(rest) = trimmed.strip_prefix("- [ ] ") {
            found.push((false, rest.to_owned()));
        }
    }
    found
}

#[test]
fn should_find_every_test_the_k11p_checklist_names_as_a_proof() {
    assert_proofs_exist(&checklist(), "docs/ACCEPTANCE.md §4.9", 40);
}

#[test]
fn should_find_every_acceptance_case_the_k11p_checklist_claims() {
    let cases = repo().join("docker").join("acceptance").join("cases");
    let passage = checklist();
    let mut claimed = Vec::new();
    for (start, _) in passage.match_indices("`2") {
        let rest = &passage[start + 1..];
        let end = rest.find('`').unwrap_or(rest.len());
        let name = &rest[..end];
        if name.len() > 4
            && name.as_bytes()[3] == b'-'
            && name[..3].chars().all(|c| c.is_ascii_digit())
        {
            claimed.push(name.to_owned());
        }
    }
    assert!(
        claimed.len() >= 7,
        "§4.9 claims the seven cases 220–226, found {claimed:?}"
    );
    let missing: Vec<&String> = claimed
        .iter()
        .filter(|name| !cases.join(format!("{name}.case")).is_file())
        .collect();
    assert!(
        missing.is_empty(),
        "§4.9 claims cases no file answers to: {missing:?}"
    );
}

#[test]
fn should_hold_a_ticked_box_for_every_gate_and_every_security_test() {
    let boxes = boxes();
    for gate in GATES.iter().chain(SECURITY.iter()) {
        let matched: Vec<&(bool, String)> = boxes
            .iter()
            .filter(|(_, text)| text.starts_with(&format!("**{gate} ")))
            .collect();
        assert_eq!(
            matched.len(),
            1,
            "§4.9 holds exactly one box for {gate}, got {matched:?}"
        );
        assert!(
            matched[0].0,
            "{gate} is open, and K11P §33 leaves no gate optional: {}",
            matched[0].1
        );
    }
    assert!(
        boxes.iter().all(|(ticked, _)| *ticked),
        "§4.9 holds an open box: {:?}",
        boxes
            .iter()
            .filter(|(ticked, _)| !ticked)
            .map(|(_, text)| text)
            .collect::<Vec<_>>()
    );
}

// --- §4.10, the acquisition addendum (K11A §25) ---------------------------------------------------

#[test]
fn should_find_every_test_the_k11a_checklist_names_as_a_proof() {
    assert_proofs_exist(&addendum(), "docs/ACCEPTANCE.md §4.10", 12);
}

#[test]
fn should_find_every_acceptance_case_the_k11a_checklist_claims() {
    let cases = repo().join("docker").join("acceptance").join("cases");
    let passage = addendum();
    let missing: Vec<String> = passage
        .match_indices("case `")
        .map(|(start, _)| {
            let rest = &passage[start + 6..];
            rest[..rest.find('`').unwrap_or(rest.len())].to_owned()
        })
        .filter(|name| !cases.join(format!("{name}.case")).is_file())
        .collect();
    assert!(
        missing.is_empty(),
        "§4.10 claims cases no file answers to: {missing:?}"
    );
    assert!(
        passage.contains("case `227-") && passage.contains("case `228-"),
        "§4.10 claims its own cases"
    );
}

#[test]
fn should_hold_a_ticked_box_for_every_one_of_the_forty_k11a_tests() {
    // K11A §25: "The implementation is not complete until automated tests cover at least the
    // following" — forty, numbered, and every one of them a box here.
    let boxes = boxes_of(&addendum());
    for item in 1..=40 {
        let matched: Vec<&(bool, String)> = boxes
            .iter()
            .filter(|(_, text)| text.starts_with(&format!("**K11A {item} ")))
            .collect();
        assert_eq!(
            matched.len(),
            1,
            "§4.10 holds exactly one box for K11A test {item}"
        );
        assert!(
            matched[0].0,
            "K11A test {item} is open, and §25 leaves none optional: {}",
            matched[0].1
        );
    }
    assert_eq!(
        boxes.len(),
        40,
        "§4.10 is the forty tests of K11A §25 and nothing else"
    );
}

/// Gate X: the layer is generic. A provider's words — `kubernetes`, `kubeconfig`, `k8s` — may
/// appear in core only as data (the bootstrap catalog names the reference provider) and in
/// comments that cite decisions; never in the code that classifies, plans, prompts or decides.
#[test]
fn should_keep_provider_wording_out_of_core_code() {
    let crates = repo().join("crates");
    let mut offending = Vec::new();
    for krate in [
        "ono-kuang-protocol",
        "ono-kuang-supervisor",
        "ono-kuang-testhost",
        "ono-cli",
    ] {
        let source_root = crates.join(krate).join("src");
        let mut stack = vec![source_root];
        while let Some(directory) = stack.pop() {
            for entry in std::fs::read_dir(&directory)
                .expect("a source directory")
                .flatten()
            {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_none_or(|extension| extension != "rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).expect("a source file");
                // A unit test module at the end of a file exercises the layer with the reference
                // provider's names as data, which is exactly what the bootstrap catalog is.
                let code_only = text.split("#[cfg(test)]").next().unwrap_or(&text);
                for (number, line) in code_only.lines().enumerate() {
                    let code = line.split("//").next().unwrap_or(line).to_lowercase();
                    if code.contains("kubernetes")
                        || code.contains("kubeconfig")
                        || code.contains("k8s")
                    {
                        offending.push(format!("{}:{}", path.display(), number + 1));
                    }
                }
            }
        }
    }
    assert!(
        offending.is_empty(),
        "Gate X, K11P §33: provider wording in core code rather than in the provider's package: \
         {offending:?}"
    );
}
