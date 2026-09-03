//! The documentation terminology contract (v0.4.1 §15.1, §15.2, §17.3, §19.1, §19.2, §51.1,
//! §51.2, §65).
//!
//! §19.1 fixes eight terms and §51.1 asks for a check "for forbidden or qualified terms where they
//! could overstate implementation", with the goal stated plainly: *"The goal is not to ban these
//! words. The goal is to ensure they refer to a defined contract."*
//!
//! The contract they refer to is `docs/spec/hardening/terminology.yaml`, which holds §19.1's eight
//! definitions once. `docs/reference/terminology.md` is rendered from that file (§19.2) and this
//! module reads the same rows, so the definition a reader is shown and the rule a document is held
//! to cannot drift apart.
//!
//! Two rules, and both are about phrases rather than about words:
//!
//! - **overstatement.** A phrase in a term's `overstates` list claims a boundary this build does
//!   not enforce, and is reported unless one of that term's `qualified_by` wordings appears in the
//!   same document. §17.3 permits "sandboxed" with "an immediate qualifier explaining the
//!   boundary" and forbids it bare, so what is checked is the bare assertion — a sentence that
//!   uses the word in order to deny it, which is what §15.2's own statement does, is the intended
//!   shape and passes. Every phrase in the registry answers a wording §65 names as a forbidden
//!   failure mode; the registry makes §65's list checkable rather than inventing prohibitions.
//! - **the native trust statement.** A document that describes what a KUANG/11 plugin executes as
//!   has to contain one of the `isolated` term's qualifiers. §15.2 allows equivalent wording —
//!   *"Equivalent wording MAY be used, but the security meaning MUST remain"* — so the set is a
//!   list rather than one string, and adding to it is a deliberate act with a reviewer attached
//!   rather than a silent paraphrase (ADR-0447).
//!
//! The surfaces §19.1 names are README, Wiki, `help`, generated reference and architecture
//! documentation. [`check_documents`] covers every one of them a gate run can reach: this
//! repository's user-facing documents, every rendered `help` page and every generated reference
//! page. **The Wiki is a separate git repository and no gate run can reach it**, so it is
//! [`check_wiki`]'s argument rather than a path this module guesses — `cargo xtask terminology
//! --wiki <path>` applies the same rules to a checkout, and ADR-0536 records why the gate cannot
//! require one.

use std::path::Path;
use std::sync::LazyLock;

use serde::Deserialize;

use crate::scan::Problem;

/// The registry, compiled in so a phrase list and the document it judges cannot be handed
/// different copies of the contract.
const REGISTRY: &str = include_str!("../../docs/spec/hardening/terminology.yaml");

/// The six remote trust concepts of §51.3, compiled in for the same reason.
const REMOTE_TRUST: &str = include_str!("../../docs/spec/hardening/remote_trust.yaml");

/// One of §19.1's canonical terms.
#[derive(Debug, Clone, Deserialize)]
pub struct Term {
    /// The word itself, as §19.1 spells it.
    pub term: String,
    /// §19.1's definition, verbatim. This is what the generated page prints.
    pub meaning: String,
    /// The specification sections that fix the term.
    pub spec: String,
    /// Why the term means what it means, for the generated page.
    #[serde(default)]
    pub doc: String,
    /// Phrases that use the term to claim a boundary this build does not enforce.
    #[serde(default)]
    pub overstates: Vec<String>,
    /// The wordings that state the boundary, and so make such a claim defined.
    #[serde(default)]
    pub qualified_by: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Registry {
    terms: Vec<Term>,
}

/// One of v0.4.1 §51.3's six remote trust concepts.
#[derive(Debug, Clone, Deserialize)]
pub struct RemoteConcept {
    /// The id, used for the generated page's anchors and by the gate.
    pub concept: String,
    /// The heading a reader sees.
    pub name: String,
    /// The §6.1 boundary it belongs to, absent where it is metadata rather than a boundary.
    #[serde(default)]
    pub boundary: Option<String>,
    /// The sections that fix it.
    pub spec: String,
    /// The commands that operate it.
    #[serde(default)]
    pub commands: Vec<String>,
    /// What a reader may conclude when it is in place.
    pub establishes: String,
    /// What a reader may not conclude — the conflation it is listed against.
    pub does_not: String,
    /// The shortest phrase that proves a page made the distinction.
    pub distinguisher: String,
}

#[derive(Debug, Deserialize)]
struct RemoteTrust {
    concepts: Vec<RemoteConcept>,
}

/// §51.3's six concepts, or `None` if the registry did not parse.
///
/// `None` rather than an empty list, so the generated page says the model is undeclared instead
/// of silently rendering a page with nothing on it.
#[must_use]
pub fn remote_trust() -> Option<Vec<RemoteConcept>> {
    serde_yaml_ng::from_str::<RemoteTrust>(REMOTE_TRUST)
        .ok()
        .map(|registry| registry.concepts)
}

/// The six concepts §51.3 requires a remote page to keep apart.
const REMOTE_CONCEPTS: [&str; 6] = [
    "ssh_transport",
    "tls_transport",
    "host_pinning",
    "client_authorization",
    "runtime_identity",
    "capability_negotiation",
];

/// Reports remote documentation that does not keep §51.3's six concepts apart.
///
/// §51.3 lists six things and requires the documentation to distinguish them. "Distinguish" is
/// checkable as a shape: the page names each concept, and for each one it says what that concept
/// does *not* establish — because every entry on §51.3's list is there for being mistakable for
/// another, and a page that describes five of them and lets the sixth be inferred is the page the
/// section exists against.
///
/// `pages` are the documents that carry the remote model. The generated reference page is one and
/// is checked by the gate; the Wiki's is [`check_wiki_remote_trust`]'s, for the reason ADR-0536
/// records.
#[must_use]
pub fn check_remote_trust(location: &str, text: &str) -> Vec<Problem> {
    let Some(concepts) = remote_trust() else {
        return vec![Problem::new(
            "docs/spec/hardening/remote_trust.yaml",
            "does not parse, so v0.4.1 §51.3's six concepts cannot be checked".to_owned(),
        )];
    };
    let mut problems = Vec::new();
    for id in REMOTE_CONCEPTS {
        if !concepts.iter().any(|concept| concept.concept == id) {
            problems.push(Problem::new(
                "docs/spec/hardening/remote_trust.yaml",
                format!("does not declare `{id}`, which v0.4.1 §51.3 lists"),
            ));
        }
    }

    let lower = normalise(text);
    for concept in &concepts {
        let named = mentions(&lower, &normalise(&concept.name));
        let distinguished = mentions(&lower, &normalise(&concept.distinguisher));
        if named && distinguished {
            continue;
        }
        problems.push(Problem::new(
            location.to_owned(),
            format!(
                "does not keep `{}` apart from the other five. v0.4.1 §51.3 requires remote-link \
                 documentation to distinguish {REMOTE_CONCEPTS:?}, and every one of them is on \
                 that list for being mistakable for another — so the page names it ({}) and says \
                 what it does not establish ({}): `{}`.",
                concept.concept,
                if named { "it does" } else { "it does not" },
                if distinguished {
                    "it does"
                } else {
                    "it does not"
                },
                concept.distinguisher,
            ),
        ));
    }
    problems
}

/// Reports a Wiki checkout whose remote page does not keep §51.3's six concepts apart.
///
/// Same argument as [`check_wiki`]: the Wiki is a separate git repository and no gate run reaches
/// it, so the checkout is named rather than guessed (ADR-0536).
#[must_use]
pub fn check_wiki_remote_trust(checkout: &Path) -> Vec<Problem> {
    let page = checkout.join("Remote-Links.md");
    match std::fs::read_to_string(&page) {
        Ok(text) => check_remote_trust("Remote-Links.md", &text),
        Err(error) => vec![Problem::new(
            "Remote-Links.md",
            format!("cannot be read from the named Wiki checkout: {error}"),
        )],
    }
}

/// §19.1's eight terms, as the registry declares them.
///
/// Empty if the registry did not parse, which [`check_documents`] reports rather than passing
/// silently: a terminology gate with no terms in it is a gate that agrees with everything.
#[must_use]
pub fn terms() -> &'static [Term] {
    static TERMS: LazyLock<Vec<Term>> = LazyLock::new(|| {
        serde_yaml_ng::from_str::<Registry>(REGISTRY)
            .map(|registry| registry.terms)
            .unwrap_or_default()
    });
    &TERMS
}

/// The registry itself, which has to hold every term §19.1 fixes.
///
/// §19.1's table is in an immutable specification, so it cannot be read as a contract
/// (AGENTS.md §5.1). This is the check that the copy which *is* a contract still says all of it.
fn check_registry() -> Vec<Problem> {
    const CANONICAL: [&str; 8] = [
        "authenticated",
        "authorized",
        "pinned",
        "confined",
        "isolated",
        "sandboxed",
        "bounded",
        "streaming",
    ];
    CANONICAL
        .iter()
        .filter(|canonical| !terms().iter().any(|term| term.term == **canonical))
        .map(|canonical| {
            Problem::new(
                "docs/spec/hardening/terminology.yaml",
                format!(
                    "does not define `{canonical}`, which v0.4.1 §19.1 fixes. A terminology gate                      missing a term is a gate that agrees with everything said about it."
                ),
            )
        })
        .collect()
}

/// The wordings that state what the native KUANG/11 tier is not (v0.4.1 §15.2).
fn disclaimers() -> &'static [String] {
    static EMPTY: Vec<String> = Vec::new();
    terms()
        .iter()
        .find(|term| term.term == "isolated")
        .map_or(&EMPTY, |term| &term.qualified_by)
}

/// The documents this repository shows a user, which therefore carry security claims.
///
/// Deliberately not the ADRs: an accepted ADR is a historical record that AGENTS.md §8 forbids
/// editing, so holding one to today's terminology would make the gate demand a rule violation —
/// [`check_decisions`] holds them to the narrower rule that survives that. Deliberately not the
/// narrative specifications either: they are immutable (AGENTS.md §5.1).
const USER_FACING: &[&str] = &[
    "README.md",
    "PHILOSOPHY.md",
    "CONTRIBUTING.md",
    "SECURITY.md",
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

/// Reports a document that overstates a boundary or omits what the native tier is not.
///
/// Every surface of §19.1 a gate run can reach: this repository's user-facing documents, every
/// rendered `help` page and every generated reference page. The Wiki is [`check_wiki`]'s.
#[must_use]
pub fn check_documents(root: &Path) -> Vec<Problem> {
    let mut problems = check_registry();
    for name in USER_FACING {
        let Ok(text) = std::fs::read_to_string(root.join(name)) else {
            continue;
        };
        problems.extend(check_text(name, &text));
    }
    for (location, page) in help_pages() {
        problems.extend(check_text(&location, &page));
    }
    for page in crate::reference::generate(root).unwrap_or_default() {
        problems.extend(check_text(&page.path, &page.contents));
        // §51.3 applies to the page that carries the remote trust model, and the generated one is
        // the copy a gate run can reach.
        if page.path == "docs/reference/remote-trust.md" {
            problems.extend(check_remote_trust(&page.path, &page.contents));
        }
    }
    problems
}

/// Reports a page of a Wiki checkout that overstates a boundary.
///
/// The Wiki is a separate git repository, so this takes the checkout rather than deriving it: a
/// gate that guessed a sibling directory would pass or fail by what happens to be on the machine,
/// and one that required the checkout would make every CI run depend on a second clone
/// (ADR-0536). `cargo xtask terminology --wiki <path>` is how a maintainer runs it.
#[must_use]
pub fn check_wiki(checkout: &Path) -> Vec<Problem> {
    let Ok(entries) = std::fs::read_dir(checkout) else {
        return vec![Problem::new(
            checkout.display().to_string(),
            "is not a readable directory, so the Wiki pages cannot be held to v0.4.1 §19.1"
                .to_owned(),
        )];
    };
    let mut pages: Vec<_> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
        .collect();
    pages.sort();

    let mut problems = Vec::new();
    for path in pages {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        problems.extend(check_text(&name, &text));
    }
    problems
}

/// Reports an accepted decision record that asserts isolation the native tier does not have.
///
/// AGENTS.md §8 makes an accepted ADR immutable, which is why `USER_FACING` leaves the decision
/// records out: a rule that reported one would demand a correction the rules forbid making.
/// ADR-0422 showed the cost of stopping there — it says a plugin "runs sandboxed under the
/// shell's uid", which is the claim §65.5 forbids, and nothing in the gate would ever have said
/// so.
///
/// The way out is that superseding is the one correction AGENTS.md §8 *does* allow. So an
/// accepted record is held to the terminology, and a record whose `Status` names a superseding
/// ADR is not: the correction has been written, and it lives in the newer record.
///
/// Only the overstatement rule applies here. A decision record is not a description of the
/// runtime, so requiring §15.2's statement in every ADR that mentions the native tier would be
/// asking a decision to carry documentation prose. Asserting a boundary that does not exist is
/// false wherever it is written; omitting a disclaimer is only a gap in a document a user reads.
#[must_use]
pub fn check_decisions(root: &Path) -> Vec<Problem> {
    let directory = root.join("docs").join("decisions");
    let Ok(entries) = std::fs::read_dir(&directory) else {
        return Vec::new();
    };
    let mut records: Vec<_> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .collect();
    records.sort();

    let mut problems = Vec::new();
    for path in records {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        problems.extend(check_decision(&name, &text));
    }
    problems
}

/// The overstatement rule, applied to one decision record.
///
/// A record that has been superseded, in whole or in part, is out of scope — see
/// [`check_decisions`].
#[must_use]
pub fn check_decision(name: &str, text: &str) -> Vec<Problem> {
    if !is_accepted(text) {
        return Vec::new();
    }
    let claimed = without_mentions(text);
    let location = format!("docs/decisions/{name}.md");
    let mut problems = Vec::new();
    for term in terms() {
        for phrase in &term.overstates {
            if !mentions(&claimed, phrase) {
                continue;
            }
            problems.push(Problem::new(
                location.clone(),
                format!(
                    "{} An accepted decision record cannot be edited (AGENTS.md §8), so the \
                     correction is a new ADR and a `Status: superseded by ADR-XXXX` line here — \
                     which is also what takes this record out of the rule's scope.",
                    reason(term, phrase)
                ),
            ));
        }
    }
    problems
}

/// Whether a record's `Status` is plain `accepted` rather than superseded by a later one.
fn is_accepted(text: &str) -> bool {
    text.lines()
        .find_map(|line| line.trim().strip_prefix("- Status:"))
        .is_some_and(|status| status.trim() == "accepted")
}

/// The text with every backticked and quoted span blanked out.
///
/// A phrase inside backticks or quotation marks is a *mention* — the record is deciding about the
/// phrase rather than claiming it. ADR-0447 defines this vocabulary and has to name every term in
/// it, and a rule that caught the naming would delete the record carrying the rule.
fn without_mentions(text: &str) -> String {
    let normalised = normalise(text);
    let mut kept = String::with_capacity(normalised.len());
    let mut inside: Option<char> = None;
    for character in normalised.chars() {
        match inside {
            Some(opener) if character == opener => {
                inside = None;
                kept.push(' ');
            }
            Some(_) => kept.push(' '),
            None if character == '`' || character == '"' => {
                inside = Some(character);
                kept.push(' ');
            }
            None => kept.push(character),
        }
    }
    kept
}

/// The two rules, applied to one document's text.
#[must_use]
pub fn check_text(location: &str, text: &str) -> Vec<Problem> {
    // Every phrase below is a sentence fragment, and a document wraps its sentences wherever the
    // margin falls. Matching on the text as typed would make the rule depend on the line width.
    let lower = normalise(text);
    // A phrase inside backticks or quotation marks is a *mention*: the document is naming the
    // wording rather than claiming it. The generated terminology page prints every refused phrase
    // and `docs/reference/schemas.md` quotes `sandboxed: true` in order to reject it, and a rule
    // that reported those would delete the documents that carry the rule.
    let claimed = without_mentions(text);
    let mut problems = Vec::new();

    for term in terms() {
        for phrase in &term.overstates {
            if mentions(&claimed, phrase) {
                problems.push(Problem::new(location.to_owned(), reason(term, phrase)));
            }
        }
    }

    if describes_the_native_tier(&lower)
        && !disclaimers().iter().any(|phrase| mentions(&lower, phrase))
    {
        problems.push(Problem::new(
            location.to_owned(),
            format!(
                "this document describes what a KUANG/11 plugin executes as and never says what \
                 that is not. v0.4.1 §15.2 requires the native trust statement, in these words or \
                 equivalent ones: \"{}\" — one of {:?} has to appear.",
                "A native KUANG/11 plugin executes as a process of the Ono user. Ono limits its \
                 brokered capabilities and applies process confinement, but native execution in \
                 v0.4.1 is not a complete filesystem or network sandbox. Install native plugins \
                 only from sources you are willing to run as your user account.",
                disclaimers()
            ),
        ));
    }
    problems
}

/// Why one phrase is an overstatement, phrased for whoever has to rewrite the sentence.
///
/// Each phrase is a *bare* claim, which is what makes reporting it unconditional rather than
/// conditional on a disclaimer somewhere in the same file. §17.3 permits the word with "an
/// immediate qualifier explaining the boundary", and a qualifier four hundred lines away is not
/// immediate — a document that says the honest thing in one paragraph and the false thing in
/// another has still said the false thing. So `qualified_by` names what to write instead; it does
/// not excuse the sentence that is already there.
fn reason(term: &Term, phrase: &str) -> String {
    let remedy = if term.qualified_by.is_empty() {
        "No wording qualifies this claim; state the boundary that does exist instead.".to_owned()
    } else {
        format!(
            "Say what is enforced and what is not, in the sentence itself — one of {:?}.",
            term.qualified_by
        )
    };
    format!(
        "`{phrase}` uses `{}` for a boundary this build does not enforce. {} means \"{}\" ({}). \
         {remedy}",
        term.term, term.term, term.meaning, term.spec
    )
}

/// Whitespace-normalised, lowercased text, so a phrase is found however the document wrapped it.
fn normalise(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Whether `haystack` contains `phrase` as whole words.
///
/// Without the boundary check `authorized by the identity it reports` is found inside
/// `unauthorized by the identity it reports`, which says the opposite, and `no limit` is found
/// inside `Ono limits`. A rule that reported the sentence getting it right is worse than no rule.
fn mentions(haystack: &str, phrase: &str) -> bool {
    let bounded = |index: usize| {
        let before = haystack[..index].chars().next_back();
        let after = haystack[index + phrase.len()..].chars().next();
        let free = |character: Option<char>| {
            character.is_none_or(|character| !character.is_alphanumeric())
        };
        free(before) && free(after)
    };
    haystack
        .match_indices(phrase)
        .any(|(index, _)| bounded(index))
}

/// Every page `help` can render, which is one of the surfaces §19.1 names.
///
/// The overview, each browsing topic and each command's own page: `help` is a surface rather than
/// a single document, and a claim on the page for one command is as visible as one in the README.
fn help_pages() -> Vec<(String, String)> {
    let Ok(registry) = ono_command::CommandRegistry::load() else {
        return vec![(
            "help".to_owned(),
            "help is not answerable: the command registry does not load".to_owned(),
        )];
    };
    let mut pages = Vec::new();
    let mut render = |topic: &str| {
        if let Ok(page) = ono_command::help(&registry, None, topic) {
            pages.push((format!("help {topic}").trim_end().to_owned(), page.render()));
        }
    };
    render("");
    for (topic, _) in ono_command::topics() {
        render(topic);
    }
    for command in registry.commands() {
        render(command.id());
    }
    pages
}
