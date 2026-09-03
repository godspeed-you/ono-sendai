//! The KUANG/11 half of the documentation terminology contract (issue #63, v0.4.1 §15, §17.3,
//! §51.1, §51.2, §65.5).

#![allow(
    clippy::panic,
    clippy::expect_used,
    reason = "AGENTS.md §16: a test states its preconditions directly"
)]

use std::path::Path;

use ono_testkit::scratch;
use xtask::terminology::{
    check_decision, check_decisions, check_documents, check_text, check_wiki, terms,
};

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

// --- the eight canonical terms of §19.1 (issue #112, ADR-0536) -----------------------------------

#[test]
fn should_define_every_canonical_term_of_the_specification() {
    // §19.1's table, and the gate reads the same rows the generated page prints (§19.2).
    let defined: Vec<&str> = terms().iter().map(|term| term.term.as_str()).collect();
    for canonical in [
        "authenticated",
        "authorized",
        "pinned",
        "confined",
        "isolated",
        "sandboxed",
        "bounded",
        "streaming",
    ] {
        assert!(
            defined.contains(&canonical),
            "v0.4.1 §19.1 fixes `{canonical}`; the registry defines {defined:?}"
        );
    }
    assert_eq!(
        terms()
            .iter()
            .find(|term| term.term == "authenticated")
            .map(|term| term.meaning.as_str()),
        Some("cryptographic peer proof was verified"),
        "the meaning is §19.1's, verbatim"
    );
}

#[test]
fn should_report_a_document_that_overstates_a_security_boundary() {
    // Not the KUANG/11 half: §65.2 names the client's self-reported identity deciding
    // authorization a forbidden failure mode, and §9.1 says why — holding a private key proves
    // who you are, never that the operator wants you here.
    let problems = check_text(
        "Remote-Links.md",
        "The agent reads the peer's Identity frame: a client is authorized by the identity it \
         reports, and the store is only advisory.",
    );
    assert!(
        problems.iter().any(|problem| problem
            .detail
            .contains("authorized by the identity it reports")),
        "the phrase is named back, got {problems:?}"
    );
    assert!(
        problems
            .iter()
            .any(|problem| problem.detail.contains("§65.2")),
        "the reason cites the failure mode it implements, got {problems:?}"
    );
}

#[test]
fn should_not_read_a_negated_word_as_the_claim_it_denies() {
    // `authorized by the identity it reports` sits inside `unauthorized by the identity it
    // reports`, which says the opposite. Matching without word boundaries would report the
    // sentence that gets it right.
    assert_eq!(
        check_text(
            "Remote-Links.md",
            "A client stays unauthorized by the identity it reports; only the store admits it.",
        ),
        Vec::new()
    );
}

#[test]
fn should_report_this_repositorys_documents_as_using_the_canonical_terms() {
    // Every surface §19.1 names that the gate can reach: the repository's user-facing documents,
    // every rendered `help` page and every generated reference page. The Wiki is a separate
    // checkout and is checked by `check_wiki` when a path is given (ADR-0536).
    let problems = check_documents(repo_root());
    assert!(
        problems.is_empty(),
        "a document uses a §19.1 term for a boundary this build does not enforce:\n{}",
        problems
            .iter()
            .map(|p| format!("  {} — {}", p.location, p.detail))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn should_check_a_wiki_checkout_when_one_is_given() {
    // The Wiki is a separate git repository, so no gate run can reach it on its own. What the
    // repository can own is the rule; `check_wiki` applies it to whatever checkout it is handed.
    let wiki = scratch();
    wiki.write(
        "Plugins-KUANG-11.md",
        "A native KUANG/11 plugin is sandboxed: it runs under the host's own limits.",
    );
    let problems = check_wiki(wiki.path());
    assert!(
        problems
            .iter()
            .any(|problem| problem.location == "Plugins-KUANG-11.md"
                && problem.detail.contains("is sandboxed")),
        "the wiki page is reported by name, got {problems:?}"
    );
}

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
}
