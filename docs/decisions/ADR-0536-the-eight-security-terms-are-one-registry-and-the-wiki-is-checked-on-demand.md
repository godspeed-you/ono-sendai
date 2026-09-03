# ADR-0536: The eight security terms are one registry, and the Wiki is checked on demand

- Status: accepted
- Date: 2026-09-03
- Spec refs: v0.4.1 §19.1, §19.2, §51.1, §52.2, §65; AGENTS.md §5.1, §8
- Decided by: agent (autonomous)

## Context

ADR-0447 and ADR-0465 built the KUANG/11 half of the documentation terminology contract: two
hard-coded phrase lists in `xtask/src/terminology.rs`, checked against README, PHILOSOPHY,
CONTRIBUTING, the rendered `help plugin-trust` page and the accepted decision records. Issue #112
is the rest of it — the remaining terms of §19.1 and the generated reference — and three questions
had to be answered before any of it could be written.

**Where do the definitions live?** §19.2 says the security terms "SHOULD be generated from the
same registries rather than duplicated in prose". There was no registry; the definitions existed
only as §19.1's table in an immutable specification, which no gate may read as a contract because
AGENTS.md §5.1 forbids deriving machinery from a file that must never change shape.

**Which words does the gate refuse?** §51.1 lists five examples and then says the thing that
governs the whole design: *"The goal is not to ban these words. The goal is to ensure they refer
to a defined contract."* A phrase list invented by an agent is speculative generality, which
AGENTS.md §4 forbids, and it would produce exactly the false positives that teach a team to
ignore a gate.

**Can the gate reach the Wiki?** The Wiki is a second git repository, cloned beside this one on a
maintainer's machine and absent from every CI runner. ADR-0447 recorded that it is held by hand.
§19.1 names it as one of the five surfaces, so "held by hand" cannot be the final answer.

## Decision

**One registry.** `docs/spec/hardening/terminology.yaml` holds §19.1's eight terms, each with the
definition verbatim, the sections that fix it, prose explaining it, the phrases that overstate it
and the wordings that state the boundary instead. It is the eighth registry under
`docs/spec/hardening/` and it obeys §52.2 the same way the others do: `docs/reference/terminology.md`
is rendered from it by `cargo xtask docs`, and `xtask::terminology` reads the same rows before it
judges a document. There is no paraphrase to drift, because there is no paraphrase.

**The refused phrases are §65's list, not a new one.** Every phrase in an `overstates` list
answers a wording §65 names as a forbidden failure mode: §65.1 "TLS means authenticated", §65.2
self-reported authorization, §65.3 negotiation-only authorization, §65.5 "sandbox" as marketing
shorthand, §65.7 streaming via background collection. A term §65 names no wording failure for —
`pinned`, `confined`, `bounded` — carries a definition and no phrase list. The registry makes
§65's list checkable; it does not extend it.

**An overstatement is unconditional.** §17.3 permits "sandboxed" with "an immediate qualifier
explaining the boundary". A qualifier four hundred lines away in the same file is not immediate,
so `qualified_by` names what to write instead and never excuses a sentence already written. This
preserves ADR-0447's behaviour, where an assertion was reported whatever else the document said.

**Matching is on whole words.** The phrases are matched against whitespace-normalised, lowercased
text so a document's line width cannot decide the outcome, and against word boundaries so
`authorized by the identity it reports` is not found inside `unauthorized by the identity it
reports`, which says the opposite, and `no limit` is not found inside `Ono limits`. A rule that
reports the sentence getting it right is worse than no rule. A phrase inside backticks or
quotation marks stays a *mention* rather than a claim, as ADR-0465 decided for the decision
records — the generated terminology page prints every refused phrase, and a rule without the
mention exemption would delete the page that carries the rule.

**The gate reaches four of the five surfaces; the Wiki is an argument.** `cargo xtask spec-check`
now holds the repository's user-facing documents (README, PHILOSOPHY, CONTRIBUTING, SECURITY),
**every** page `help` can render — the overview, the nine browsing topics and each of the 193
commands — every generated reference page, and the accepted decision records. **The Wiki it
cannot reach, and it will not pretend to.** `cargo xtask terminology --wiki <path>` applies the
identical rules to a checkout the caller names, and the task says so out loud when no path is
given. Two alternatives were rejected: guessing a sibling `ono-sendai-wiki/` would make the gate
pass or fail by what happens to be on the machine, and requiring the checkout would make every CI
run depend on a second clone that the release workflow — owned elsewhere — would have to provide.

## Consequences

- §19.1's definitions have one home. Changing what `confined` means is a change to one file, and
  the generated page, the gate and the tests move together.
- A new refused phrase is a registry row with a `§65` justification, reviewed as data. Adding one
  cannot be done by editing a Rust literal in passing.
- `help` became a surface rather than a document. A claim on the page for one command is now as
  visible to the gate as one in the README.
- The Wiki's status is now stated rather than assumed: unchecked unless someone names it. The
  honest cost is that a CI run cannot fail on a Wiki page, and the release checklist has to carry
  that as a manual step until the release workflow clones the Wiki.
- The rule found one real overstatement on the day it landed. `PHILOSOPHY.md` said an
  interpretation was "isolated by the KUANG/11 capability model", which is §15.3's forbidden
  reading of a manifest declaration — the capability model governs what a package may ask Ono
  *for*, never what its own process may do. The sentence now says capability mediation and says
  which it is.
- Encoded by `xtask/tests/terminology.rs::should_define_every_canonical_term_of_the_specification`,
  `::should_report_a_document_that_overstates_a_security_boundary`,
  `::should_not_read_a_negated_word_as_the_claim_it_denies`,
  `::should_report_this_repositorys_documents_as_using_the_canonical_terms`,
  `::should_check_a_wiki_checkout_when_one_is_given` and
  `xtask/tests/reference.rs::should_render_the_security_terms_into_the_generated_reference`.

## Alternatives considered

- **Keep the two hard-coded lists and add six more.** Rejected: §19.2 asks for the terms to be
  generated from a registry, and eight lists in a Rust file is the prose duplication the section
  exists to prevent.
- **Require every document that uses a canonical term to link the glossary.** Rejected as the
  primary rule: it is trivially satisfiable by adding a link and says nothing about whether the
  sentence is true. The generated page still exists and is still linked, but the rule that has
  teeth is the one about claims.
- **Invent a phrase list per term from first principles.** Rejected under AGENTS.md §4. §65 is
  the specification enumerating the wordings it considers failures; anything beyond it is an
  agent's guess about documentation nobody has written.
- **Copy the Wiki into `docs/wiki/` so the gate can read it.** Rejected: it is published from its
  own repository by GitHub, and a second copy would be a mirror somebody has to keep true — the
  exact failure §50.1 names for volatile counts, applied to twenty thousand words.
