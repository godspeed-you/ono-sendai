//! The release evidence of `docs/ACCEPTANCE.md` §4.11: the checklist of the v0.5 tranche is held
//! against the tree, so it cannot rot — the way `xtask/tests/hardening_evidence.rs` holds §4.8 and
//! `xtask/tests/permission_evidence.rs` holds §4.9 and §4.10.
//!
//! §4.11 is written before the tranche it defines, so today every one of its boxes is open and
//! every proof they name is a file the delivering increment still owes. That is the condition this
//! harvester has to survive: a guard that resolves nothing looks exactly like a guard that works,
//! and a checklist nobody resolves is a checklist that rots. So each check below is stated twice —
//! once against the real `docs/ACCEPTANCE.md`, and once against a scratch copy mutated to carry
//! the defect, which proves the check bites rather than merely passing.
//!
//! Everything here is a statement about *evidence* rather than about the shell:
//!
//! * the passage read is §4.11 and neither its neighbour nor the stopping rule;
//! * every proof a *ticked* box names exists — a test file with the named function declared in it,
//!   an acceptance case the referee collects, an `xtask` task that is really dispatched;
//! * no proof a ticked box names is `#[ignore]`d, because a box ticked by an ignored test is a box
//!   ticked by nothing;
//! * the subsection keeps its shape: boxes at the left margin where `scripts/release-check.sh`'s
//!   `^- \[ \]` can see them, bolded titles, and `#### 4.11.N` headings in order;
//! * the case numbers 230–279 belong to this tranche and to nothing else;
//! * and the tranche's progress is reported rather than judged — whether an open box may stay open
//!   is `scripts/release-check.sh`'s verdict, not this suite's.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "AGENTS.md §16: a helper shared by tests states its preconditions the same way a test does"
)]

use ono_testkit::{Scratch, scratch};
use xtask::evidence::{
    Proof, check_case_block, check_checklist_shape, check_ticked_proofs, checkboxes, checklist,
    progress, proofs,
};

mod support;
use support::{read, repo};

/// The text of §4.11 as the repository carries it today.
fn tranche() -> String {
    checklist(&repo()).expect("docs/ACCEPTANCE.md carries §4.11, the v0.5 tranche")
}

/// A stand-in `docs/ACCEPTANCE.md` with a neighbour, a §4.11 of three boxes, and a stopping rule.
///
/// The boxes on either side name proofs no tree will ever carry, so a harvester that read past
/// either end of the subsection would report them and be caught doing it.
const DOCUMENT: &str = "\
### 4.10 The KUANG/11 plugin package acquisition and system distribution addendum (K11A)

- [x] **K11A 1 · A box of the tranche before.** —
      `crates/ono-elsewhere/tests/nowhere.rs::should_belong_to_the_neighbour`.

### 4.11 The v0.5 tranche — Temporal & Causal Systems Interface

Conventions this subsection relies on:

- The case numbers **230–279** belong to this tranche.

#### 4.11.1 Contracts and vocabulary (T1, §34, §35, §36)

- [x] **Coverage is first-class.** The type carries it and the test says so —
      `crates/ono-temporal-core/tests/coverage.rs::should_compose_coverage_per_capability`,
      case `230-temporal-at-and-now`.
- [ ] **A box the tranche has not delivered.** Its proof is a file the delivering increment
      writes — `crates/ono-temporal-core/tests/nothing_yet.rs`,
      case 231-temporal-configuration.

#### 4.11.2 Temporal context (§4, §12)

- [x] **Startup stays inside its budget.** — `cargo run -p xtask -- perf`.

## 5. Stopping rule

- [x] **A box of the stopping rule.** —
      `crates/ono-elsewhere/tests/beyond.rs::should_belong_to_the_stopping_rule`.
";

/// A scratch repository whose §4.11 names only proofs its tree really carries.
fn resolved() -> Scratch {
    let repo = scratch();
    repo.write("docs/ACCEPTANCE.md", DOCUMENT);
    repo.write(
        "crates/ono-temporal-core/tests/coverage.rs",
        "#[test]\nfn should_compose_coverage_per_capability() {}\n",
    );
    repo.write(
        "docker/acceptance/cases/230-temporal-at-and-now.case",
        "name: temporal at and now\ntimeout: 30\n",
    );
    repo.write(
        "xtask/src/main.rs",
        "fn main() {\n    match task {\n        Some(\"perf\") => perf(&rest),\n    }\n}\n",
    );
    repo
}

/// The same repository with `docs/ACCEPTANCE.md` rewritten: `was` becomes `now`.
fn mutated(was: &str, now: &str) -> Scratch {
    let repo = resolved();
    let document = DOCUMENT.replace(was, now);
    assert_ne!(document, DOCUMENT, "the mutation `{was}` changed nothing");
    repo.write("docs/ACCEPTANCE.md", document);
    repo
}

/// The passage §4.11 occupies in a scratch repository.
fn passage(repo: &Scratch) -> String {
    checklist(repo.path()).expect("the scratch document carries §4.11")
}

/// Every problem a scan reported, as the lines a failing assertion prints.
fn reported(problems: &[xtask::scan::Problem]) -> Vec<String> {
    problems
        .iter()
        .map(|problem| format!("{} — {}", problem.location, problem.detail))
        .collect()
}

// --- the passage --------------------------------------------------------------------------------

#[test]
fn should_read_the_v05_checklist_apart_from_the_v041_one_and_the_stopping_rule() {
    // §4.8.1's precedent, one tranche on: each harvester reads one checklist, so a proof named in
    // §4.10 is never counted as evidence for a box in §4.11 and the stopping rule's prose — which
    // talks *about* boxes — is never read as one.
    let passage = tranche();
    assert!(
        passage.starts_with("### 4.11 The v0.5 tranche"),
        "§4.11's passage begins at its own heading"
    );
    assert!(
        !passage.contains("### 4.10") && !passage.contains("## 5. Stopping rule"),
        "§4.11's passage reaches into a neighbouring section"
    );
    assert!(
        passage.contains("#### 4.11.1 ") && passage.contains("#### 4.11.13 "),
        "§4.11's passage carries the whole subsection, from its first heading to its last"
    );

    let acceptance = read("docs/ACCEPTANCE.md");
    let stopping = &acceptance[acceptance
        .find("## 5. Stopping rule")
        .expect("docs/ACCEPTANCE.md carries the stopping rule")..];
    assert!(
        !passage.contains(stopping),
        "§4.11's passage swallowed the stopping rule"
    );
    assert!(
        passage.len() > 20_000,
        "§4.11 is a whole tranche's checklist and the harvester read {} bytes",
        passage.len()
    );
}

#[test]
fn should_report_no_checklist_when_the_document_carries_no_subsection() {
    // The harvester says so rather than reading an empty passage and passing over nothing.
    let repo = scratch();
    repo.write(
        "docs/ACCEPTANCE.md",
        "### 4.10 The addendum\n\n- [x] **A.**\n",
    );
    assert!(
        checklist(repo.path()).is_none(),
        "a document without §4.11 has no v0.5 checklist to harvest"
    );
}

#[test]
fn should_stop_at_the_stopping_rule_when_the_subsection_is_the_last_one() {
    // The bound the real document actually uses: §4.11 is the last tranche, so its passage ends at
    // `## 5. Stopping rule` rather than at a following `### 4.12`.
    let repo = resolved();
    let passage = passage(&repo);
    assert!(
        passage.contains("Coverage is first-class"),
        "the passage is §4.11's own boxes"
    );
    assert!(
        !passage.contains("the tranche before") && !passage.contains("the stopping rule"),
        "the passage keeps to §4.11: {passage}"
    );
}

// --- the proofs a ticked box names ---------------------------------------------------------------

#[test]
fn should_find_every_proof_a_ticked_box_of_the_v05_checklist_names() {
    // `docs/ACCEPTANCE.md` §3 and §4.11's own preamble: a box is ticked by a named automated proof
    // — a test that runs un-ignored in `scripts/gate.sh`, or a case that runs in
    // `scripts/acceptance.sh`. A proof that was never written, renamed away or left ignored leaves
    // a box ticked by nothing.
    //
    // What this deliberately does not resolve is whether a named command *passes*: a box naming
    // `cargo run -p xtask -- perf` is resolved as far as the task being dispatched, and the
    // measurement it produces is the gate's business, not this harvester's. Every open box is
    // skipped: the proofs it names are what its delivering increment owes, and
    // `scripts/release-check.sh` is what refuses to call the tranche finished until they exist.
    let passage = tranche();
    let paths = proofs(&passage)
        .into_iter()
        .filter(|proof| matches!(proof, Proof::Tree { .. }))
        .count();
    assert!(
        paths >= 80,
        "§4.11 names a proof under every one of its boxes and the harvester found {paths} paths — \
         it has stopped reading what it is meant to read"
    );
    let problems = check_ticked_proofs(&repo(), &passage);
    assert!(
        problems.is_empty(),
        "§4.11 ticks boxes whose proofs the tree does not carry:\n{}",
        reported(&problems).join("\n")
    );
}

#[test]
fn should_report_a_box_of_the_real_checklist_ticked_on_a_proof_nobody_wrote() {
    // The test above passes over an empty set today, and the scratch fixtures below are written in
    // a tidier hand than the document is. This one runs the rule over §4.11 as it really stands —
    // its six-space continuations, its em dashes, its hundred and twenty-three boxes — with one
    // more box appended and ticked on a suite that does not exist. If the reader ever stops
    // understanding the checklist's own formatting, this is where it says so.
    let mutant = format!(
        "{}\n- [x] **A box ticked before its increment landed.** — \
         `crates/ono-temporal-core/tests/a_suite_nobody_will_ever_write.rs`.\n",
        tranche()
    );
    let problems = reported(&check_ticked_proofs(&repo(), &mutant));
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("a_suite_nobody_will_ever_write.rs")),
        "a box of the real §4.11 ticked on a suite nobody wrote is reported: {problems:?}"
    );
}

#[test]
fn should_resolve_every_kind_of_proof_when_the_tree_carries_them_all() {
    // The other half of the test above, which today passes over an empty set: a ticked box whose
    // test file, test function, acceptance case and `xtask` task all exist is accepted. Without
    // this, a harvester that reported everything and one that reported nothing would both look
    // green while §4.11 held no ticked box.
    let repo = resolved();
    let problems = check_ticked_proofs(repo.path(), &passage(&repo));
    assert_eq!(
        reported(&problems),
        Vec::<String>::new(),
        "every proof the scratch checklist ticks is in its tree"
    );
}

#[test]
fn should_report_a_ticked_box_whose_test_function_nobody_wrote() {
    // The defect this file exists against: a box ticked on a test name that does not exist.
    let repo = resolved();
    repo.write(
        "crates/ono-temporal-core/tests/coverage.rs",
        "#[test]\nfn should_do_something_else_entirely() {}\n",
    );
    let problems = reported(&check_ticked_proofs(repo.path(), &passage(&repo)));
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("should_compose_coverage_per_capability")),
        "a ticked box naming a test nobody wrote is reported: {problems:?}"
    );
}

#[test]
fn should_report_a_ticked_box_whose_test_file_nobody_wrote() {
    // The same defect one level up: the file itself was renamed away or never written.
    let repo = mutated("tests/coverage.rs", "tests/no_such_file.rs");
    let problems = reported(&check_ticked_proofs(repo.path(), &passage(&repo)));
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("no_such_file.rs")),
        "a ticked box naming a file nobody wrote is reported: {problems:?}"
    );
}

#[test]
fn should_report_a_ticked_box_whose_proof_is_ignored() {
    // A box ticked by an `#[ignore]`d test is a box ticked by nothing: the gate runs the suite and
    // the proof does not run with it.
    let repo = resolved();
    repo.write(
        "crates/ono-temporal-core/tests/coverage.rs",
        "#[test]\n#[ignore = \"needs a ledger\"]\nfn should_compose_coverage_per_capability() {}\n",
    );
    let problems = reported(&check_ticked_proofs(repo.path(), &passage(&repo)));
    assert!(
        problems.iter().any(|problem| problem.contains("ignore")),
        "a ticked box naming an ignored test is reported: {problems:?}"
    );
}

#[test]
fn should_report_a_ticked_box_whose_acceptance_case_the_referee_does_not_collect() {
    // §2: a capability without a passing acceptance case is not delivered. A box ticked on a case
    // name no file answers to claims a referee's verdict nobody obtained.
    let repo = mutated("`230-temporal-at-and-now`", "`230-temporal-nothing`");
    let problems = reported(&check_ticked_proofs(repo.path(), &passage(&repo)));
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("230-temporal-nothing")),
        "a ticked box claiming a case nobody wrote is reported: {problems:?}"
    );
}

#[test]
fn should_report_a_ticked_box_whose_command_names_a_task_xtask_does_not_have() {
    // A bare command is resolved as far as it sensibly can be: `cargo run -p xtask -- <task>` is a
    // claim that `xtask` dispatches that task, and a renamed task leaves the box pointing at
    // nothing.
    let repo = mutated(
        "cargo run -p xtask -- perf",
        "cargo run -p xtask -- measure",
    );
    let problems = reported(&check_ticked_proofs(repo.path(), &passage(&repo)));
    assert!(
        problems.iter().any(|problem| problem.contains("measure")),
        "a ticked box naming a task `xtask` does not dispatch is reported: {problems:?}"
    );
}

#[test]
fn should_report_a_ticked_box_that_names_no_proof_at_all() {
    // §3's rule read from the other end: a box ticked on prose alone is ticked by judgement, which
    // is the one thing `docs/ACCEPTANCE.md` forbids everywhere.
    let repo = mutated(
        "— `cargo run -p xtask -- perf`.",
        "— the team reviewed it and agreed.",
    );
    let problems = reported(&check_ticked_proofs(repo.path(), &passage(&repo)));
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("names no automated proof")),
        "a box ticked on prose alone is reported: {problems:?}"
    );
}

#[test]
fn should_leave_an_open_box_alone_when_its_proofs_do_not_exist_yet() {
    // §4.11 was written before its tranche, so today all 123 of its boxes are open and name files
    // their increments still owe. Reporting those would make a checklist written in advance
    // unusable — the harvester holds what is *claimed*, and an open box claims nothing.
    let repo = resolved();
    let passage = passage(&repo);
    assert!(
        passage.contains("nothing_yet.rs"),
        "the scratch checklist keeps an open box naming a file nobody wrote"
    );
    let problems = reported(&check_ticked_proofs(repo.path(), &passage));
    assert!(
        !problems
            .iter()
            .any(|problem| problem.contains("nothing_yet.rs")),
        "an open box's unwritten proof is owed, not broken: {problems:?}"
    );
}

#[test]
fn should_read_a_files_tests_and_cases_out_of_a_box_when_it_names_several() {
    // The reader itself, stated as a behaviour: a box names its proofs in backticks, sometimes a
    // file and a function, sometimes a bare command, and names a case the way ADR-0401 asks —
    // backticked once the file exists, in prose while it does not.
    let found = proofs(
        "**A box.** — `crates/ono-temporal-core/tests/coverage.rs::should_hold`, \
         `cargo run -p xtask -- perf`, case `230-a-written-case`, case 279-an-owed-case.",
    );
    assert_eq!(
        found,
        vec![
            Proof::Tree {
                path: "crates/ono-temporal-core/tests/coverage.rs".to_owned(),
                item: Some("should_hold".to_owned()),
            },
            Proof::Command {
                line: "cargo run -p xtask -- perf".to_owned(),
            },
            Proof::Case {
                name: "230-a-written-case".to_owned(),
                claimed: true,
            },
            Proof::Case {
                name: "279-an-owed-case".to_owned(),
                claimed: false,
            },
        ],
        "a box's proofs are read as what they are"
    );
}

// --- the shape of the subsection -----------------------------------------------------------------

#[test]
fn should_keep_every_box_of_the_v05_checklist_in_the_shape_the_release_check_reads() {
    // `scripts/release-check.sh` greps `^- \[ \]` and fails on the first hit, so an indented box is
    // invisible to the stopping rule and exempts itself from it. A box also carries a bolded title
    // — that phrase is what `xtask::scan`'s release-notes check quotes — and the `#### 4.11.N`
    // headings run in order, so a subsubsection cannot be lost by being renumbered around.
    let problems = check_checklist_shape(&tranche());
    assert!(
        problems.is_empty(),
        "§4.11 has lost the shape a script can read:\n{}",
        reported(&problems).join("\n")
    );
}

#[test]
fn should_report_a_box_indented_out_of_the_release_checks_sight() {
    let repo = mutated(
        "- [x] **Startup stays inside its budget.**",
        "  - [x] **Startup stays inside its budget.**",
    );
    let problems = reported(&check_checklist_shape(&passage(&repo)));
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("left margin")),
        "an indented box is reported: {problems:?}"
    );
}

#[test]
fn should_report_a_box_without_a_bolded_title() {
    let repo = mutated(
        "- [x] **Startup stays inside its budget.**",
        "- [x] Startup stays inside its budget.",
    );
    let problems = reported(&check_checklist_shape(&passage(&repo)));
    assert!(
        problems.iter().any(|problem| problem.contains("bolded")),
        "a box without a bolded title is reported: {problems:?}"
    );
}

#[test]
fn should_report_a_box_written_with_a_marker_no_script_reads() {
    let repo = mutated(
        "- [x] **Startup stays inside its budget.**",
        "- [X] **Startup stays inside its budget.**",
    );
    let problems = reported(&check_checklist_shape(&passage(&repo)));
    assert!(
        problems.iter().any(|problem| problem.contains("- [X]")),
        "a box written in a spelling the checklist's readers do not share is reported: \
         {problems:?}"
    );
}

#[test]
fn should_report_a_subsubsection_heading_out_of_order() {
    let repo = mutated(
        "#### 4.11.2 Temporal context",
        "#### 4.11.4 Temporal context",
    );
    let problems = reported(&check_checklist_shape(&passage(&repo)));
    assert!(
        problems.iter().any(|problem| problem.contains("4.11.4")),
        "a heading that skips a number is reported: {problems:?}"
    );
}

#[test]
fn should_number_every_subsubsection_of_the_v05_checklist_in_order() {
    // The count is the reader's map of the tranche: §4.11.1 … §4.11.13, one per area of the
    // specification, with nothing missing between them.
    let passage = tranche();
    let headings: Vec<&str> = passage
        .lines()
        .filter(|line| line.starts_with("#### "))
        .collect();
    assert_eq!(
        headings.len(),
        13,
        "§4.11 is thirteen subsubsections and the harvester found {}: {headings:#?}",
        headings.len()
    );
}

// --- the case-number block -----------------------------------------------------------------------

#[test]
fn should_keep_the_case_numbers_of_the_v05_checklist_inside_its_own_block() {
    // §4.11's preamble claims 230–279 for the tranche. The claim is exclusive in both directions:
    // a case file in the block that no box names is a scenario the checklist forgot, and a case
    // the checklist owes outside the block is a scenario numbered into another tranche's range.
    // The rule leaves earlier tranches' cases alone where §4.11 cites one as a regression proof —
    // `060-performance-budgets` and `100-spatial-performance-budgets` are v0.4's and exist.
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
        "§4.11 names the tranche's cases 230–272 and the harvester found {} — it has stopped \
         reading what it is meant to read: {named:?}",
        named.len()
    );
    let problems = check_case_block(&repo(), &passage);
    assert!(
        problems.is_empty(),
        "§4.11 and `docker/acceptance/cases/` disagree about the 230–279 block:\n{}",
        reported(&problems).join("\n")
    );
}

#[test]
fn should_report_a_case_in_the_block_that_the_checklist_never_names() {
    let repo = resolved();
    repo.write(
        "docker/acceptance/cases/244-retention-boundary.case",
        "name: retention boundary\ntimeout: 30\n",
    );
    let problems = reported(&check_case_block(repo.path(), &passage(&repo)));
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("244-retention-boundary")),
        "a case in the tranche's block that no box names is reported: {problems:?}"
    );
}

#[test]
fn should_report_a_case_the_checklist_owes_outside_its_block() {
    let repo = mutated(
        "case 231-temporal-configuration",
        "case 331-temporal-configuration",
    );
    let problems = reported(&check_case_block(repo.path(), &passage(&repo)));
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("331-temporal-configuration")),
        "a case numbered outside the tranche's block is reported: {problems:?}"
    );
}

#[test]
fn should_report_a_case_recorded_absent_that_the_referee_already_collects() {
    // ADR-0401's convention, and §4.11's own: a case gains its backticks in the increment that
    // writes it. A file that exists under a name still written in prose leaves
    // `xtask::scan::check_acceptance_case_references` resolving nothing where it could resolve
    // something.
    let repo = resolved();
    repo.write(
        "docker/acceptance/cases/231-temporal-configuration.case",
        "name: temporal configuration\ntimeout: 30\n",
    );
    let problems = reported(&check_case_block(repo.path(), &passage(&repo)));
    assert!(
        problems.iter().any(|problem| problem.contains("backticks")),
        "a written case still recorded absent is reported: {problems:?}"
    );
}

#[test]
fn should_report_two_different_cases_sharing_one_number() {
    let repo = mutated(
        "case 231-temporal-configuration",
        "case 230-temporal-settings",
    );
    let problems = reported(&check_case_block(repo.path(), &passage(&repo)));
    assert!(
        problems.iter().any(|problem| problem.contains("230")),
        "two names under one case number are reported: {problems:?}"
    );
}

// --- progress ------------------------------------------------------------------------------------

#[test]
fn should_report_how_far_the_v05_tranche_has_come() {
    // Where the tranche stands, printed rather than judged. An open box is not a failure here:
    // `scripts/release-check.sh` owns that verdict and fails on the first `- [ ]`, while this
    // suite runs in the gate on every increment and would otherwise make a tranche in progress
    // unbuildable. `cargo test -p xtask -- --show-output` puts the line in front of a reader.
    let passage = tranche();
    let progress = progress(&passage);
    println!(
        "docs/ACCEPTANCE.md §4.11 — the v0.5 temporal tranche: {} of {} boxes ticked, {} open",
        progress.ticked,
        progress.total(),
        progress.open
    );
    assert!(
        progress.total() >= 100,
        "§4.11 is the definition of done for a whole tranche and the harvester counted {} boxes — \
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
    let repo = resolved();
    let before = progress(&passage(&repo));
    assert_eq!(
        (before.ticked, before.open),
        (2, 1),
        "the scratch checklist ticks two of its three boxes"
    );

    let repo = mutated(
        "- [ ] **A box the tranche has not delivered.**",
        "- [x] **A box the tranche has not delivered.**",
    );
    let after = progress(&passage(&repo));
    assert_eq!(
        (after.ticked, after.open),
        (3, 0),
        "ticking a box moves the count"
    );
}
