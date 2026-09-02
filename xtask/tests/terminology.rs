//! The KUANG/11 half of the documentation terminology contract (issue #63, v0.4.1 §15, §17.3,
//! §51.1, §51.2, §65.5).

#![allow(
    clippy::panic,
    clippy::expect_used,
    reason = "AGENTS.md §16: a test states its preconditions directly"
)]

use std::path::Path;

use xtask::terminology::{check_decision, check_decisions, check_documents, check_text};

#[test]
fn should_reject_a_document_that_calls_the_native_tier_a_sandbox() {
    // v0.4.1 §65.5, verbatim: "Calling native plugins sandboxed without stating the missing
    // filesystem/network isolation is forbidden."
    let problems = check_text(
        "README.md",
        "KUANG/11 loads extensions under an explicit capability and isolation model: manifests, \
         declared capabilities, sandboxed execution, an audit trail.",
    );
    assert!(
        problems
            .iter()
            .any(|problem| problem.detail.contains("sandboxed execution")),
        "the phrase the README actually carried is reported, got {problems:?}"
    );
}

#[test]
fn should_find_the_native_isolation_disclaimer_in_every_document_that_describes_the_kuang_tier() {
    // §15.2: the security meaning is not negotiable, so a document that says what a plugin
    // executes as has to say what it is not.
    let silent = check_text(
        "README.md",
        "KUANG/11 is the extension runtime. A native plugin executes with the capabilities its \
         manifest declares, under process confinement the host applies before it starts.",
    );
    assert!(
        silent
            .iter()
            .any(|problem| problem.detail.contains("§15.2")),
        "a description with no disclaimer is reported, got {silent:?}"
    );

    let stated = check_text(
        "README.md",
        "KUANG/11 is the extension runtime. A native plugin executes as a process of the Ono \
         user under process confinement and brokered capabilities, and native execution is not a \
         complete filesystem or network sandbox.",
    );
    assert_eq!(stated, Vec::new(), "the stated form passes");
}

#[test]
fn should_report_this_repositorys_user_facing_documents_as_honest_about_the_native_tier() {
    // The rule applied to the tree it exists for: README, PHILOSOPHY, CONTRIBUTING and the
    // `help plugin-trust` page §19.1 names as one of the three surfaces.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root");
    let problems = check_documents(root);
    assert!(
        problems.is_empty(),
        "a document overstates what the native KUANG/11 tier is (v0.4.1 §15.2, §17.3, §65.5):\n{}",
        problems
            .iter()
            .map(|p| format!("  {} — {}", p.location, p.detail))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

// --- accepted decision records (ADR-0465) -------------------------------------------------------

#[test]
fn should_hold_an_accepted_decision_record_to_the_same_terminology() {
    let record = "\
# ADR-0500: Something

- Status: accepted

## Alternatives considered

- **A KUANG/11 plugin.** Rejected: package mutations need root while a plugin runs sandboxed
  under the shell's uid.
";
    let problems = check_decision("ADR-0500", record);
    assert_eq!(problems.len(), 1, "got {problems:?}");
    assert!(
        problems[0].detail.contains("runs sandboxed"),
        "the reason names the phrase: {}",
        problems[0].detail
    );
}

#[test]
fn should_leave_a_superseded_decision_record_alone() {
    // AGENTS.md §8 forbids editing an accepted ADR, so a record that has been superseded cannot
    // be corrected in place. Holding it to today's terminology would make the gate demand a rule
    // violation; the superseding record is where the correction lives.
    let record = "\
# ADR-0500: Something

- Status: superseded by ADR-0501

A plugin runs sandboxed under the shell's uid.
";
    assert_eq!(check_decision("ADR-0500", record), Vec::new());
}

#[test]
fn should_leave_a_partly_superseded_decision_record_alone() {
    let record = "\
# ADR-0500: Something

- Status: superseded by ADR-0501 (in part: the sentence about isolation)

A plugin runs sandboxed under the shell's uid.
";
    assert_eq!(check_decision("ADR-0500", record), Vec::new());
}

#[test]
fn should_let_a_decision_record_quote_the_phrase_it_is_deciding_about() {
    // ADR-0447 defines this vocabulary and has to name it. A phrase inside quotation marks or
    // backticks is a mention rather than a claim, and a rule that caught the mention would
    // delete the record that carries the rule.
    let record = "\
# ADR-0500: Something

- Status: accepted

The README said `sandboxed execution`, and the Wiki said a package
*\"never reaches them directly\"* — both false of a process running as the user.
";
    assert_eq!(check_decision("ADR-0500", record), Vec::new());
}

#[test]
fn should_report_this_repositorys_decision_records_as_honest_about_the_native_tier() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root");
    let problems = check_decisions(root);
    assert!(
        problems.is_empty(),
        "an accepted decision record asserts isolation the native tier does not have:\n{}",
        problems
            .iter()
            .map(|p| format!("  {} — {}", p.location, p.detail))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
