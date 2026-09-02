//! The KUANG/11 half of the documentation terminology contract (issue #63, v0.4.1 §15, §17.3,
//! §51.1, §51.2, §65.5).

#![allow(
    clippy::panic,
    clippy::expect_used,
    reason = "AGENTS.md §16: a test states its preconditions directly"
)]

use std::path::Path;

use xtask::terminology::{check_documents, check_text};

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
