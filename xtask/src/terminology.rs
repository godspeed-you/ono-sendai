//! The documentation terminology contract (v0.4.1 §15.1, §15.2, §17.3, §19.1, §51.1, §51.2).
//!
//! §19.1 fixes eight terms and §51.1 asks for a check "for forbidden or qualified terms where they
//! could overstate implementation", with the goal stated plainly: *"The goal is not to ban these
//! words. The goal is to ensure they refer to a defined contract."*
//!
//! This module holds the half of that contract that belongs to KUANG/11, which §51.2 names as the
//! specific correction v0.4.1 requires: the README described native execution as "sandboxed
//! execution" in a list of security properties, and §65.5 calls that a forbidden failure mode —
//! *"Calling native plugins sandboxed without stating the missing filesystem/network isolation is
//! forbidden."*
//!
//! Two rules, and both are about phrases rather than about words:
//!
//! - **[`ASSERTIONS`]** are the spellings that claim the boundary outright. §17.3 permits
//!   "sandboxed" with "an immediate qualifier explaining the boundary" and forbids it bare, so
//!   what is checked is the bare assertion — `sandboxed execution`, `fully isolated`, `is
//!   sandboxed` — not every occurrence of the word. A sentence that uses the word in order to
//!   deny it, which is what §15.2's own statement does, is the intended shape and passes.
//! - **[`DISCLAIMERS`]** are the accepted ways to say §15.2's meaning. A document that describes
//!   what a KUANG/11 plugin executes as has to contain one of them. §15.2 allows equivalent
//!   wording — *"Equivalent wording MAY be used, but the security meaning MUST remain"* — so the
//!   set is a list rather than one string, and adding to it is a deliberate act with a reviewer
//!   attached rather than a silent paraphrase (ADR-0447).
//!
//! The Wiki lives in a separate checkout and cannot be read from here. It is held to the same two
//! rules by hand until a gate can reach it; #112 owns extending this to the generated reference
//! and to the remaining six terms of §19.1.

use std::path::Path;

use crate::scan::Problem;

/// The documents this repository shows a user, which therefore carry security claims.
///
/// Deliberately not the ADRs: an accepted ADR is a historical record that AGENTS.md §8 forbids
/// editing, so holding one to today's terminology would make the gate demand a rule violation.
/// Deliberately not the narrative specifications either — they are immutable (AGENTS.md §5.1).
const USER_FACING: &[&str] = &["README.md", "PHILOSOPHY.md", "CONTRIBUTING.md"];

/// Phrases that assert an isolation boundary rather than describing one.
///
/// §51.1's own examples, plus the two spellings the README and the Wiki actually used.
pub const ASSERTIONS: &[&str] = &[
    "sandboxed execution",
    "sandboxed native",
    "fully isolated",
    "is sandboxed",
    "are sandboxed",
    "runs sandboxed",
    "run sandboxed",
    "completely isolated",
    "cannot reach them directly",
    "never reaches them directly",
];

/// Accepted ways to state §15.2's meaning: what the native tier is not.
pub const DISCLAIMERS: &[&str] = &[
    "not a complete filesystem or network sandbox",
    "does not isolate it from the filesystem or the network",
    "not isolated by this execution tier",
    "no filesystem or network isolation",
];

/// Whether `text` describes what a KUANG/11 plugin executes as, and so owes the statement.
///
/// A document that merely links to the extension runtime does not: the obligation follows the
/// claim, and it is the claim about execution that §15.2 answers.
fn describes_the_native_tier(lower: &str) -> bool {
    if !lower.contains("kuang") {
        return false;
    }
    [
        "native plugin",
        "native kuang",
        "native execution",
        "native tier",
        "native-process",
    ]
    .iter()
    .any(|needle| lower.contains(*needle))
}

/// Reports a document that overstates the native tier or omits what it is not.
#[must_use]
pub fn check_documents(root: &Path) -> Vec<Problem> {
    let mut problems = Vec::new();
    for name in USER_FACING {
        let path = root.join(name);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        problems.extend(check_text(name, &text));
    }
    problems.extend(check_text("help plugin-trust", &plugin_trust_page()));
    problems
}

/// The two rules, applied to one document's text.
#[must_use]
pub fn check_text(location: &str, text: &str) -> Vec<Problem> {
    let mut problems = Vec::new();
    // Every phrase below is a sentence fragment, and a document wraps its sentences wherever the
    // margin falls. Matching on the text as typed would make the rule depend on the line width.
    let lower = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();

    for phrase in ASSERTIONS {
        if lower.contains(phrase) {
            problems.push(Problem::new(
                location.to_owned(),
                format!(
                    "`{phrase}` asserts an isolation boundary the native KUANG/11 tier does not \
                     have. v0.4.1 §17.3 allows the word only with an immediate qualifier stating \
                     the boundary, and §65.5 names the bare claim a forbidden failure mode. Say \
                     what is actually installed — capability mediation and process confinement — \
                     and say that kernel isolation is not part of this tier (§15.1)."
                ),
            ));
        }
    }

    if describes_the_native_tier(&lower) && !DISCLAIMERS.iter().any(|phrase| lower.contains(phrase))
    {
        problems.push(Problem::new(
            location.to_owned(),
            format!(
                "this document describes what a KUANG/11 plugin executes as and never says what \
                 that is not. v0.4.1 §15.2 requires the native trust statement, in these words or \
                 equivalent ones: \"{}\" — one of {DISCLAIMERS:?} has to appear.",
                "A native KUANG/11 plugin executes as a process of the Ono user. Ono limits its \
                 brokered capabilities and applies process confinement, but native execution in \
                 v0.4.1 is not a complete filesystem or network sandbox. Install native plugins \
                 only from sources you are willing to run as your user account."
            ),
        ));
    }
    problems
}

/// The rendered text of `help plugin-trust`, which is one of the three surfaces §19.1 names.
fn plugin_trust_page() -> String {
    match ono_command::CommandRegistry::load()
        .and_then(|registry| ono_command::help(&registry, None, "plugin-trust"))
    {
        Ok(page) => page.render(),
        Err(error) => format!("help plugin-trust is not answerable: {error}"),
    }
}
