//! The release evidence of `docs/ACCEPTANCE.md` §4.12: the checklist of the v0.6 tranche is held
//! against the tree, so it cannot rot — the way `xtask/tests/temporal_evidence.rs` holds §4.11 and
//! `xtask/tests/hardening_evidence.rs` holds §4.8.
//!
//! §4.12 is written before the tranche it defines, so today every one of its hundred and ninety-two
//! boxes is open and every proof they name is a file the delivering increment still owes. That is
//! the condition this harvester has to survive: a guard that resolves nothing looks exactly like a
//! guard that works, and a checklist nobody resolves is a checklist that rots. So each check below
//! is stated twice — once against the real `docs/ACCEPTANCE.md`, and once against a scratch copy
//! mutated to carry the defect, which proves the check bites rather than merely passing.
//!
//! Everything here is a statement about *evidence* rather than about the shell:
//!
//! * the passage read is §4.12 and neither §4.11 above it nor the stopping rule below it;
//! * every proof a *ticked* box names exists — a test file with the named function declared in it,
//!   an acceptance case the referee collects, an `xtask` task that is really dispatched;
//! * no proof a ticked box names is `#[ignore]`d, because a box ticked by an ignored test is a box
//!   ticked by nothing;
//! * the subsection keeps its shape: boxes at the left margin where `scripts/release-check.sh`'s
//!   `^- \[ \]` can see them, bolded titles, and `#### 4.12.N` headings in order;
//! * the case numbers 280–329 belong to this tranche and to nothing else;
//! * §2's eighteen core invariants and §63's twenty release criteria each have a box, typed out
//!   from the specification so the test agrees with it rather than with the document;
//! * and the tranche's progress is reported rather than judged — whether an open box may stay open
//!   is `scripts/release-check.sh`'s verdict, not this suite's.
//!
//! The rules the v0.5 suite borrows from `xtask::evidence` are shared where they are written for
//! any tranche — reading the boxes, reading the proofs a box names, resolving a ticked box and
//! counting progress. The two that name §4.11 in their own text, the subsection's shape and its
//! case block, are stated here for §4.12.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "AGENTS.md §16: a helper shared by tests states its preconditions the same way a test does"
)]

use std::path::Path;

use ono_testkit::{Scratch, scratch};
use xtask::evidence::{Proof, check_ticked_proofs, checkboxes, progress, proofs};

mod support;
use support::{read, repo, report, section};

/// The heading §4.12 opens with.
const HEADING: &str = "### 4.12 The v0.6 tranche";

/// Where §4.12's passage ends: at the next tranche, or at the stopping rule that follows the last
/// one. §4.12 is the last today, so the second marker is the one in force.
const ENDS: [&str; 2] = ["\n### 4.13", "\n## 5. Stopping rule"];

/// The lowest and highest acceptance-case number §4.12's preamble claims for the tranche.
const BLOCK: std::ops::RangeInclusive<u32> = 280..=329;

/// Where the acceptance cases live.
const CASES: &str = "docker/acceptance/cases";

/// The fifteen subsubsections §4.12 is divided into, from its contracts to its release criteria.
const SUBSUBSECTIONS: u32 = 15;

/// §2's eighteen core invariants, typed out from the specification rather than counted from the
/// document, so the test agrees with the specification and not with whatever the checklist says.
const INVARIANTS: [u32; 18] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18,
];

/// §63's twenty release criteria, typed out for the same reason.
const RELEASE_CRITERIA: [u32; 20] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
];

/// The text of §4.12 in the `docs/ACCEPTANCE.md` under `root`, or `None` when the document carries
/// no such subsection.
fn checklist(root: &Path) -> Option<String> {
    let acceptance = std::fs::read_to_string(root.join("docs/ACCEPTANCE.md")).ok()?;
    section(&acceptance, HEADING, &ENDS)
}

/// The text of §4.12 as the repository carries it today.
fn tranche() -> String {
    checklist(&repo()).expect("docs/ACCEPTANCE.md carries §4.12, the v0.6 tranche")
}

/// A stand-in `docs/ACCEPTANCE.md` with a neighbour, a §4.12 of three boxes, and a stopping rule.
///
/// The boxes on either side name proofs no tree will ever carry, so a harvester that read past
/// either end of the subsection would report them and be caught doing it.
const DOCUMENT: &str = "\
### 4.11 The v0.5 tranche — Temporal & Causal Systems Interface

- [x] **A box of the tranche before.** —
      `crates/ono-elsewhere/tests/nowhere.rs::should_belong_to_the_neighbour`.

### 4.12 The v0.6 tranche — Prospective Change, Protection & Recovery

Conventions this subsection relies on:

- The case numbers **280–329** belong to this tranche.

#### 4.12.1 Contracts, vocabulary and the drift referee (§45, §46, §47)

- [x] **The drift referee is held to the registries.** Each rule breaks one copied registry —
      `xtask/tests/change_contracts.rs::should_reject_an_edge_from_prepare_failed_to_applying`,
      case `280-plan-creates-nothing`.
- [ ] **A box the tranche has not delivered.** Its proof is a file the delivering increment
      writes — `crates/ono-change-plan/tests/nothing_yet.rs`,
      case 281-plan-block.

#### 4.12.2 Planning (§5, §6, §55.1)

- [x] **Startup stays inside its budget.** — `cargo run -p xtask -- perf`.

## 5. Stopping rule

- [x] **A box of the stopping rule.** —
      `crates/ono-elsewhere/tests/beyond.rs::should_belong_to_the_stopping_rule`.
";

/// A scratch repository whose §4.12 names only proofs its tree really carries.
fn resolved() -> Scratch {
    let tree = scratch();
    tree.write("docs/ACCEPTANCE.md", DOCUMENT);
    tree.write(
        "xtask/tests/change_contracts.rs",
        "#[test]\nfn should_reject_an_edge_from_prepare_failed_to_applying() {}\n",
    );
    tree.write(
        "docker/acceptance/cases/280-plan-creates-nothing.case",
        "name: plan creates nothing\ntimeout: 30\n",
    );
    tree.write(
        "xtask/src/main.rs",
        "fn main() {\n    match task {\n        Some(\"perf\") => perf(&rest),\n    }\n}\n",
    );
    tree
}

/// The same repository with `docs/ACCEPTANCE.md` rewritten: `was` becomes `now`.
fn mutated(was: &str, now: &str) -> Scratch {
    let tree = resolved();
    let document = DOCUMENT.replace(was, now);
    assert_ne!(document, DOCUMENT, "the mutation `{was}` changed nothing");
    tree.write("docs/ACCEPTANCE.md", document);
    tree
}

/// The passage §4.12 occupies in a scratch repository.
fn passage(tree: &Scratch) -> String {
    checklist(tree.path()).expect("the scratch document carries §4.12")
}

/// Every proof problem a passage's ticked boxes carry, as one report a reader can scan.
fn ticked_proofs(root: &Path, passage: &str) -> String {
    report(&check_ticked_proofs(root, passage))
}

/// Asserts that a report names `needle`, printing the whole of it when it does not.
fn assert_names(found: &str, needle: &str) {
    assert!(
        found.contains(needle),
        "§4.12's evidence rules must report this; expected a problem mentioning {needle:?}, and \
         the harvester said:\n{found}"
    );
}

/// Asserts that a report leaves `needle` alone.
fn assert_silent(found: &str, needle: &str) {
    assert!(
        !found.contains(needle),
        "nothing about {needle:?} is a defect, and the harvester reported it:\n{found}"
    );
}

/// One problem, in the shape [`support::report`] writes them.
fn problem(location: &str, detail: String) -> String {
    format!("  {location} — {detail}")
}

// --- the shape rules §4.12 needs for itself ----------------------------------------------------

/// Checks that the subsection keeps the shape its readers depend on.
///
/// `scripts/release-check.sh` greps `^- \[ \]` and fails on the first hit, so a box indented under
/// something else exempts itself from the stopping rule without anyone deciding that it may. The
/// bolded title is what `xtask::scan`'s release-notes check quotes, and the `#### 4.12.N` headings
/// are the reader's map of the tranche: one per area, in order, with nothing missing between them.
///
/// The report is a string rather than a `Vec<Problem>` because `xtask::scan::Problem` is built
/// inside `xtask` and read outside it; the lines are written in the shape [`support::report`]
/// gives the checks this suite shares with `xtask::evidence`, so a reader sees one format.
fn check_shape(passage: &str) -> String {
    let mut problems: Vec<String> = Vec::new();
    let mut subsection = String::from("4.12");
    let mut expected = 1_u32;
    let mut headed = false;
    for line in passage.lines() {
        if let Some(heading) = line.strip_prefix("#### ") {
            let number = heading.split_whitespace().next().unwrap_or_default();
            if number != format!("4.12.{expected}") {
                problems.push(problem(
                    &format!("docs/ACCEPTANCE.md §{subsection}"),
                    format!(
                        "`#### {heading}` follows §{subsection}, and §4.12's subsubsections run \
                         from 4.12.1 upwards without gaps — §4.12.{expected} is where this one \
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
            problems.push(problem(
                &at,
                format!(
                    "`{trimmed}` is a box away from the left margin, where \
                     `scripts/release-check.sh`'s `^- \\[ \\]` cannot see it — an indented box \
                     exempts itself from the stopping rule"
                ),
            ));
            continue;
        }
        if !line.starts_with("- [x] ") && !line.starts_with("- [ ] ") {
            problems.push(problem(
                &at,
                format!(
                    "`{trimmed}` is not a box the checklist's readers share: a box is written \
                     `- [x] ` or `- [ ] `"
                ),
            ));
            continue;
        }
        if !headed {
            problems.push(problem(
                &at,
                format!("`{trimmed}` stands above §4.12's first `####` heading"),
            ));
        }
    }
    for checkbox in checkboxes(passage) {
        if checkbox.title.is_empty() {
            problems.push(problem(
                &format!("docs/ACCEPTANCE.md §{}", checkbox.subsection),
                format!(
                    "`{}` has no bolded title, and a box is read by the phrase it opens with",
                    checkbox.text
                ),
            ));
        }
    }
    problems.join("\n")
}

/// Checks that the acceptance-case numbers §4.12 claims belong to it and to nothing else.
///
/// The claim is exclusive in both directions. A case file inside the block that no box names is a
/// scenario the checklist forgot; a case the checklist still owes outside the block is a scenario
/// numbered into another tranche's range. ADR-0401 fixes how a case is written on the way: in
/// prose while the increment that writes it is owed, in backticks once the file exists.
fn check_case_block(root: &Path, passage: &str) -> String {
    let at = "docs/ACCEPTANCE.md §4.12";
    let mut problems: Vec<String> = Vec::new();
    let collected = collected_cases(root);
    let mentioned: Vec<(String, bool)> = proofs(passage)
        .into_iter()
        .filter_map(|proof| match proof {
            Proof::Case { name, claimed } => Some((name, claimed)),
            Proof::Tree { .. } | Proof::Command { .. } => None,
        })
        .collect();

    let mut numbered: Vec<(u32, String)> = Vec::new();
    for (name, claimed) in &mentioned {
        let exists = collected.iter().any(|case| case == name);
        let number = case_number(name);
        if let Some(number) = number {
            numbered.push((number, name.clone()));
        }
        match (claimed, exists) {
            (true, false) => problems.push(problem(
                at,
                format!(
                    "claims case `{name}` in backticks and `{CASES}/{name}.case` is not there. \
                     Write the case, or record the name in prose until the increment that owes it \
                     lands (ADR-0401)"
                ),
            )),
            (false, true) => problems.push(problem(
                at,
                format!(
                    "records case {name} as a name no file answers to, and `{CASES}/{name}.case` \
                     is there. A case gains its backticks in the increment that writes it \
                     (ADR-0401)"
                ),
            )),
            (false, false) => {
                if number.is_none_or(|number| !BLOCK.contains(&number)) {
                    problems.push(problem(
                        at,
                        format!(
                            "owes case {name}, which is numbered outside the {}–{} block §4.12 \
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
            problems.push(problem(
                at,
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
            problems.push(problem(
                &format!("{CASES}/{case}.case"),
                format!(
                    "is numbered inside the {}–{} block §4.12 claims for the v0.6 tranche, and no \
                     box of §4.12 names it — a scenario the referee runs and the checklist does \
                     not know about",
                    BLOCK.start(),
                    BLOCK.end()
                ),
            ));
        }
    }
    problems.join("\n")
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

// --- the passage --------------------------------------------------------------------------------

#[test]
fn should_read_the_v06_checklist_apart_from_the_v05_one_and_the_stopping_rule() {
    // §4.8.1's precedent, two tranches on: each harvester reads one checklist, so a proof named in
    // §4.11 is never counted as evidence for a box in §4.12 and the stopping rule's prose — which
    // talks *about* boxes — is never read as one.
    let passage = tranche();
    assert!(
        passage.starts_with(HEADING),
        "§4.12's passage begins at its own heading"
    );
    assert!(
        !passage.contains("### 4.11 ") && !passage.contains("## 5. Stopping rule"),
        "§4.12's passage reaches into a neighbouring section"
    );
    assert!(
        passage.contains("#### 4.12.1 ") && passage.contains("#### 4.12.15 "),
        "§4.12's passage carries the whole subsection, from its first heading to its last"
    );

    let acceptance = read("docs/ACCEPTANCE.md");
    let stopping = &acceptance[acceptance
        .find("## 5. Stopping rule")
        .expect("docs/ACCEPTANCE.md carries the stopping rule")..];
    assert!(
        !passage.contains(stopping),
        "§4.12's passage swallowed the stopping rule"
    );
    assert!(
        passage.len() > 30_000,
        "§4.12 is a whole tranche's checklist and the harvester read {} bytes",
        passage.len()
    );
}

#[test]
fn should_report_no_checklist_when_the_document_carries_no_subsection() {
    // The harvester says so rather than reading an empty passage and passing over nothing.
    let tree = scratch();
    tree.write(
        "docs/ACCEPTANCE.md",
        "### 4.11 The v0.5 tranche\n\n- [x] **A box.**\n",
    );
    assert!(
        checklist(tree.path()).is_none(),
        "a document without §4.12 has no v0.6 checklist to harvest"
    );
}

#[test]
fn should_stop_at_the_stopping_rule_when_the_subsection_is_the_last_one() {
    // The bound the real document actually uses: §4.12 is the last tranche, so its passage ends at
    // `## 5. Stopping rule` rather than at a following `### 4.13`.
    let tree = resolved();
    let passage = passage(&tree);
    assert!(
        passage.contains("The drift referee is held to the registries"),
        "the passage is §4.12's own boxes"
    );
    assert!(
        !passage.contains("the tranche before") && !passage.contains("the stopping rule"),
        "the passage keeps to §4.12: {passage}"
    );
}

// --- the proofs a ticked box names ---------------------------------------------------------------

#[test]
fn should_find_every_proof_a_ticked_box_of_the_v06_checklist_names() {
    // `docs/ACCEPTANCE.md` §3 and §4.12's own preamble: a box is ticked by a named automated proof
    // — a test that runs un-ignored in `scripts/gate.sh`, or a case that runs in
    // `scripts/acceptance.sh`. A proof that was never written, renamed away or left ignored leaves
    // a box ticked by nothing.
    //
    // Every open box is skipped: the proofs it names are what its delivering increment owes, and
    // `scripts/release-check.sh` is what refuses to call the tranche finished until they exist. So
    // today this passes over an empty set, and the count below is what keeps that honest — the
    // harvester must still be reading the two hundred and twenty-five paths §4.12 names.
    let passage = tranche();
    let paths = proofs(&passage)
        .into_iter()
        .filter(|proof| matches!(proof, Proof::Tree { .. }))
        .count();
    assert!(
        paths >= 200,
        "§4.12 names a proof under every one of its boxes and the harvester found {paths} paths — \
         it has stopped reading what it is meant to read"
    );
    let ticked = progress(&passage).ticked;
    println!("docs/ACCEPTANCE.md §4.12 — {ticked} ticked boxes resolved against the tree");
    let problems = ticked_proofs(&repo(), &passage);
    assert!(
        problems.is_empty(),
        "§4.12 ticks boxes whose proofs the tree does not carry:\n{problems}"
    );
}

#[test]
fn should_report_a_box_of_the_real_checklist_ticked_on_a_proof_nobody_wrote() {
    // The test above passes over an empty set today, and the scratch fixtures below are written in
    // a tidier hand than the document is. This one runs the rule over §4.12 as it really stands —
    // its six-space continuations, its em dashes, its hundred and ninety-two boxes — with one more
    // box appended and ticked on a suite that does not exist. If the reader ever stops
    // understanding the checklist's own formatting, this is where it says so.
    let mutant = format!(
        "{}\n- [x] **A box ticked before its increment landed.** — \
         `crates/ono-change-plan/tests/a_suite_nobody_will_ever_write.rs`.\n",
        tranche()
    );
    assert_names(
        &ticked_proofs(&repo(), &mutant),
        "a_suite_nobody_will_ever_write.rs",
    );
}

#[test]
fn should_resolve_every_kind_of_proof_when_the_tree_carries_them_all() {
    // The other half of the test above, which today passes over an empty set: a ticked box whose
    // test file, test function, acceptance case and `xtask` task all exist is accepted. Without
    // this, a harvester that reported everything and one that reported nothing would both look
    // green while §4.12 held no ticked box.
    let tree = resolved();
    let problems = ticked_proofs(tree.path(), &passage(&tree));
    assert!(
        problems.is_empty(),
        "every proof the scratch checklist ticks is in its tree:\n{problems}"
    );
}

#[test]
fn should_report_a_ticked_box_whose_proof_is_ignored() {
    // A box ticked by an `#[ignore]`d test is a box ticked by nothing: the gate runs the suite and
    // the proof does not run with it.
    let tree = resolved();
    tree.write(
        "xtask/tests/change_contracts.rs",
        "#[test]\n#[ignore = \"needs a pool\"]\nfn should_reject_an_edge_from_prepare_failed_to_applying() {}\n",
    );
    assert_names(&ticked_proofs(tree.path(), &passage(&tree)), "`#[ignore]`d");
}

#[test]
fn should_report_a_ticked_box_whose_acceptance_case_the_referee_does_not_collect() {
    // §2: a capability without a passing acceptance case is not delivered. A box ticked on a case
    // name no file answers to claims a referee's verdict nobody obtained.
    let tree = mutated("`280-plan-creates-nothing`", "`280-plan-creates-none`");
    assert_names(
        &ticked_proofs(tree.path(), &passage(&tree)),
        "280-plan-creates-none",
    );
}

#[test]
fn should_report_a_ticked_box_whose_command_names_a_task_xtask_does_not_have() {
    // A bare command is resolved as far as it sensibly can be: `cargo run -p xtask -- <task>` is a
    // claim that `xtask` dispatches that task, and a renamed task leaves the box pointing at
    // nothing.
    let tree = mutated(
        "cargo run -p xtask -- perf",
        "cargo run -p xtask -- measure",
    );
    assert_names(
        &ticked_proofs(tree.path(), &passage(&tree)),
        "dispatches no `measure` task",
    );
}

#[test]
fn should_report_a_ticked_box_that_names_no_proof_at_all() {
    // §3's rule read from the other end: a box ticked on prose alone is ticked by judgement, which
    // is the one thing `docs/ACCEPTANCE.md` forbids everywhere.
    let tree = mutated(
        "— `cargo run -p xtask -- perf`.",
        "— the team reviewed it and agreed.",
    );
    assert_names(
        &ticked_proofs(tree.path(), &passage(&tree)),
        "names no automated proof",
    );
}

#[test]
fn should_leave_an_open_box_alone_when_its_proofs_do_not_exist_yet() {
    // §4.12 was written before its tranche, so today all 192 of its boxes are open and name files
    // their increments still owe. Reporting those would make a checklist written in advance
    // unusable — the harvester holds what is *claimed*, and an open box claims nothing. This is
    // what makes the checklist writable ahead of the code, so it is tested rather than assumed.
    let tree = resolved();
    let passage = passage(&tree);
    assert!(
        passage.contains("nothing_yet.rs"),
        "the scratch checklist keeps an open box naming a file nobody wrote"
    );
    assert_silent(&ticked_proofs(tree.path(), &passage), "nothing_yet.rs");
}

// --- the shape of the subsection -----------------------------------------------------------------

#[test]
fn should_keep_every_box_of_the_v06_checklist_in_the_shape_the_release_check_reads() {
    // `scripts/release-check.sh` greps `^- \[ \]` and fails on the first hit, so an indented box is
    // invisible to the stopping rule and exempts itself from it. A box also carries a bolded title
    // — that phrase is what `xtask::scan`'s release-notes check quotes — and the `#### 4.12.N`
    // headings run in order, so a subsubsection cannot be lost by being renumbered around.
    let problems = check_shape(&tranche());
    assert!(
        problems.is_empty(),
        "§4.12 has lost the shape a script can read:\n{problems}"
    );
}

#[test]
fn should_report_a_box_indented_out_of_the_release_checks_sight() {
    let tree = mutated(
        "- [x] **Startup stays inside its budget.**",
        "  - [x] **Startup stays inside its budget.**",
    );
    assert_names(&check_shape(&passage(&tree)), "left margin");
}

#[test]
fn should_report_a_box_without_a_bolded_title() {
    let tree = mutated(
        "- [x] **Startup stays inside its budget.**",
        "- [x] Startup stays inside its budget.",
    );
    assert_names(&check_shape(&passage(&tree)), "no bolded title");
}

#[test]
fn should_report_a_box_written_with_a_marker_no_script_reads() {
    let tree = mutated(
        "- [x] **Startup stays inside its budget.**",
        "- [X] **Startup stays inside its budget.**",
    );
    assert_names(&check_shape(&passage(&tree)), "- [X]");
}

#[test]
fn should_keep_the_subsubsection_headings_in_order_and_numbered_without_a_gap() {
    // The count is the reader's map of the tranche: §4.12.1 … §4.12.15, one per area of the
    // specification, with nothing missing between them.
    let passage = tranche();
    let headings: Vec<&str> = passage
        .lines()
        .filter(|line| line.starts_with("#### "))
        .collect();
    assert_eq!(
        headings.len(),
        SUBSUBSECTIONS as usize,
        "§4.12 is {SUBSUBSECTIONS} subsubsections and the harvester found {}: {headings:#?}",
        headings.len()
    );
    for (index, heading) in headings.iter().enumerate() {
        let expected = format!("#### 4.12.{} ", index + 1);
        assert!(
            heading.starts_with(&expected),
            "§4.12's subsubsections run in order without a gap, and `{heading}` stands where \
             `{expected}` should"
        );
    }
}

#[test]
fn should_report_a_subsubsection_heading_out_of_order() {
    let tree = mutated("#### 4.12.2 Planning", "#### 4.12.4 Planning");
    assert_names(&check_shape(&passage(&tree)), "4.12.4");
}

// --- the case-number block -----------------------------------------------------------------------

#[test]
fn should_keep_the_case_numbers_of_the_v06_checklist_inside_its_own_block() {
    // §4.12's preamble claims 280–329 for the tranche. The claim is exclusive in both directions:
    // a case file in the block that no box names is a scenario the checklist forgot, and a case
    // the checklist owes outside the block is a scenario numbered into another tranche's range.
    let passage = tranche();
    let named: Vec<String> = proofs(&passage)
        .into_iter()
        .filter_map(|proof| match proof {
            Proof::Case { name, .. } => Some(name),
            Proof::Tree { .. } | Proof::Command { .. } => None,
        })
        .collect();
    assert!(
        named.len() >= 40,
        "§4.12 names the tranche's cases 280–323 and the harvester found {} — it has stopped \
         reading what it is meant to read: {named:?}",
        named.len()
    );
    let problems = check_case_block(&repo(), &passage);
    assert!(
        problems.is_empty(),
        "§4.12 and `{CASES}/` disagree about the 280–329 block:\n{problems}"
    );
}

#[test]
fn should_report_a_case_in_the_block_that_the_checklist_never_names() {
    let tree = resolved();
    tree.write(
        "docker/acceptance/cases/295-apply-and-verify.case",
        "name: apply and verify\ntimeout: 30\n",
    );
    assert_names(
        &check_case_block(tree.path(), &passage(&tree)),
        "295-apply-and-verify",
    );
}

#[test]
fn should_report_a_case_the_checklist_owes_outside_its_block() {
    let tree = mutated("case 281-plan-block", "case 381-plan-block");
    assert_names(
        &check_case_block(tree.path(), &passage(&tree)),
        "owes case 381-plan-block",
    );
}

#[test]
fn should_report_a_case_recorded_absent_that_the_referee_already_collects() {
    // ADR-0401's convention, and §4.12's own: a case gains its backticks in the increment that
    // writes it. A file that exists under a name still written in prose leaves
    // `xtask::scan::check_acceptance_case_references` resolving nothing where it could resolve
    // something.
    let tree = resolved();
    tree.write(
        "docker/acceptance/cases/281-plan-block.case",
        "name: plan block\ntimeout: 30\n",
    );
    assert_names(&check_case_block(tree.path(), &passage(&tree)), "backticks");
}

#[test]
fn should_report_two_different_cases_sharing_one_number() {
    let tree = mutated("case 281-plan-block", "case 280-plan-settings");
    assert_names(
        &check_case_block(tree.path(), &passage(&tree)),
        "names two cases under the number 280",
    );
}

// --- the two lists the tranche is written from ---------------------------------------------------

#[test]
fn should_name_a_proof_for_every_one_of_section_two_s_eighteen_invariants() {
    // §4.12.14 carries one box per core invariant of §2, numbered 1 to 18. The numbers are typed
    // out above rather than counted from the document, so an invariant that lost its box is a
    // failure here rather than a smaller number nobody noticed.
    let passage = tranche();
    let boxes: Vec<_> = checkboxes(&passage)
        .into_iter()
        .filter(|checkbox| checkbox.subsection == "4.12.14")
        .collect();
    assert_eq!(
        boxes.len(),
        INVARIANTS.len(),
        "§4.12.14 is §2's eighteen core invariants and the harvester found {}",
        boxes.len()
    );
    for (invariant, checkbox) in INVARIANTS.into_iter().zip(&boxes) {
        assert!(
            checkbox.title.starts_with(&format!("{invariant}. ")),
            "§2's invariant {invariant} stands where §4.12.14 writes `{}` — the boxes are \
             numbered 1 to 18 in order and without a gap",
            checkbox.title
        );
        assert!(
            !proofs(&checkbox.text).is_empty(),
            "§2's invariant {invariant} names an automated proof, and `{}` names none",
            checkbox.title
        );
    }
}

#[test]
fn should_name_a_proof_for_every_one_of_section_sixty_three_s_release_criteria() {
    // §63 lists twenty conditions v0.6 is release-ready under. Nineteen of them are cited by
    // number in §4.12.15's boxes; the twentieth — no release-blocking known defects remain — is
    // the verdict `scripts/release-check.sh` itself reaches, so it is carried by the box that
    // names the script rather than by a proof of its own.
    let passage = tranche();
    let criteria = section(&passage, "#### 4.12.15 ", &["\n#### "])
        .expect("§4.12 carries the release-criteria subsubsection");
    for criterion in RELEASE_CRITERIA {
        if criterion == 20 {
            assert!(
                criteria.contains("**`scripts/release-check.sh` is green.**"),
                "§63.20 — no release-blocking known defects remain — is what \
                 `scripts/release-check.sh` decides, and §4.12.15 carries its box"
            );
            continue;
        }
        assert!(
            criteria.contains(&format!("§{}.{criterion} ", 63)),
            "§63.{criterion} has a box in §4.12.15, cited by number so a criterion cannot lose \
             its evidence by having its box rewritten"
        );
    }
    assert!(
        !checkboxes(&criteria).is_empty(),
        "§4.12.15 is a checklist and the harvester read no boxes from it"
    );
}

// --- progress ------------------------------------------------------------------------------------

#[test]
fn should_report_how_far_the_v06_tranche_has_come() {
    // Where the tranche stands, printed rather than judged. An open box is not a failure here:
    // `scripts/release-check.sh` owns that verdict and fails on the first `- [ ]`, while this
    // suite runs in the gate on every increment and would otherwise make a tranche in progress
    // unbuildable. `cargo test -p xtask -- --show-output` puts these lines in front of a reader.
    let passage = tranche();
    let progress = progress(&passage);
    println!(
        "docs/ACCEPTANCE.md §4.12 — the v0.6 change tranche: {} of {} boxes ticked, {} open",
        progress.ticked,
        progress.total(),
        progress.open
    );
    println!("{}", ticked_proofs(&repo(), &passage));
    assert!(
        progress.total() >= 150,
        "§4.12 is the definition of done for a whole tranche and the harvester counted {} boxes — \
         it has stopped reading what it is meant to read",
        progress.total()
    );
    assert_eq!(
        progress.open,
        passage
            .lines()
            .filter(|line| line.starts_with("- [ ] "))
            .count(),
        "the harvester counts the open boxes `scripts/release-check.sh` greps for"
    );
    assert_eq!(
        progress.ticked + progress.open,
        checkboxes(&passage).len(),
        "every box is either ticked or open"
    );
}

#[test]
fn should_count_a_ticked_box_when_the_checklist_ticks_one() {
    // The counter's own guard: a number that never moves reports nothing.
    let tree = resolved();
    let before = progress(&passage(&tree));
    assert_eq!(
        (before.ticked, before.open),
        (2, 1),
        "the scratch checklist ticks two of its three boxes"
    );

    let tree = mutated(
        "- [ ] **A box the tranche has not delivered.**",
        "- [x] **A box the tranche has not delivered.**",
    );
    let after = progress(&passage(&tree));
    assert_eq!(
        (after.ticked, after.open),
        (3, 0),
        "ticking a box moves the count"
    );
}
