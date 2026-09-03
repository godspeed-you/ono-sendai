# ADR-0447: The documentation says which boundary it means, and a gate holds it to that

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §0.5.4, §15.1, §15.2, §15.3, §17.3, §19.1, §19.2, §51.1, §51.2, §65.5,
  Appendix D; base spec §31.16; AGENTS.md §5.1 (the specs are immutable), §8 (accepted ADRs are
  not edited); ADR-0442, ADR-0448
- Decided by: agent (autonomous)

## Context

v0.4.1 §51.2 names one correction by name: *"The KUANG/11 README description MUST be changed so
native execution is not presented as an unspecified complete sandbox."* The README said:

> … under an explicit capability and isolation model: manifests, declared capabilities, sandboxed
> execution, an audit trail …

`PHILOSOPHY.md` said the same in its own words, and the Wiki went further: a section headed
*"Isolation is enforced, not declared"* and a sentence claiming of the filesystem and the network
that a package *"never reaches them directly"* — which is false of a process running as the user.
§65.5 names the class: *"Calling native plugins sandboxed without stating the missing
filesystem/network isolation is forbidden."*

§15.1 gives the three concepts that have to stay apart, §15.2 the statement documentation MUST
make, §19.1 the eight canonical terms, and §17.3 the rule about the word itself. What none of them
gives is how a repository stops the words creeping back.

## Decision

**The correction is made in every surface, and the two rules that make it become a gate check.**

**1. Three concepts, named, wherever the tier is described.** *Capability mediation* (Ono decides
what the protocol may ask Ono to do), *process confinement* (the process-level restrictions the
host installs), *kernel isolation* (kernel policy over the filesystem and the network). The native
tier provides the first two and not the third, and every corrected passage says so in that
vocabulary rather than in the word "sandbox".

**2. `help plugin-trust` is the `help` surface §19.1 names.** §19.1 requires the terms to be used
consistently "in README, Wiki, `help`, generated reference and architecture documentation", and
`help` had no page where a user could ask what a plugin can reach. The topic states the three
concepts, §15.2's statement, and §15.3's distinction —
`brokered capability: denied` is not `native direct OS access: not isolated by this execution
tier` — which is the confusion §15.3 exists to prevent and the one a denied capability invites.

**3. `xtask::terminology` checks phrases, not words.** Two rules:

- `ASSERTIONS` — `sandboxed execution`, `fully isolated`, `is sandboxed`, `never reaches them
  directly` and their neighbours — are the spellings that claim the boundary outright. §17.3
  permits the word "with an immediate qualifier explaining the boundary", so what is forbidden is
  the bare assertion. A sentence that uses the word in order to *deny* it, which is what §15.2's
  own statement does, is the intended shape and passes.
- `DISCLAIMERS` — a document that describes what a native KUANG/11 plugin executes as must contain
  one of a listed set of phrases carrying §15.2's meaning. §15.2 allows equivalent wording, so the
  set is a list; adding to it is a deliberate edit with a reviewer attached rather than a silent
  paraphrase.

Matching is on whitespace-normalised text, because every one of these phrases is a sentence
fragment and a document wraps its sentences wherever the margin falls. A rule that depended on the
line width would be a rule that fired on `fmt`.

**4. The scan covers the user-facing documents and the `help` page, and not the ADRs or the
specifications.** An accepted ADR is a historical record AGENTS.md §8 forbids editing, and the
narrative specifications are immutable under §5.1; holding either to today's terminology would
make the gate demand a rule violation. `docs/decisions/ADR-0422` contains the phrase "runs
sandboxed under the shell's uid" and stays as it is.

**5. The Wiki is corrected by hand and recorded here, because no gate can reach it.** It is a
separate checkout. The same two rules apply to it, and #112 owns the question of how a gate could
ever see it.

## Consequences

Easy: the phrase that started this — "sandboxed execution" in a list of security properties —
cannot come back without turning the gate red, in the README, in `PHILOSOPHY.md`, in
`CONTRIBUTING.md` or in the `help` page itself. The §15.2 statement exists in exactly one place in
code, `ExecutionTier::boundary()`, and `inspect plugin` renders it from there (§19.2), so the
record and the documentation cannot drift.

Hard: `DISCLAIMERS` is a list of accepted phrasings, which is a small tax on rewriting a paragraph
and a real constraint on translating one. That is the trade §15.2 asks for — "equivalent wording
MAY be used, but the security meaning MUST remain" — and a list is the only form of "equivalent"
a check can hold.

Also: this covers one of §19.1's eight terms. `authenticated`, `authorized`, `pinned`, `bounded`
and `streaming` are #112's, and `xtask::terminology` is where they go.

Encoded by: `xtask/tests/terminology.rs::should_reject_a_document_that_calls_the_native_tier_a_sandbox`,
`::should_find_the_native_isolation_disclaimer_in_every_document_that_describes_the_kuang_tier`,
`::should_report_this_repositorys_user_facing_documents_as_honest_about_the_native_tier`,
case `189-kuang-confinement-fail-closed`.

## Alternatives considered

**Ban the word "sandbox" outright.** §51.1 says the opposite in as many words: *"The goal is not
to ban these words. The goal is to ensure they refer to a defined contract."* A ban would also
make §15.2's own required sentence illegal, since it contains the word.

**Check the sentence containing the word for a negation.** More general than a phrase list and
much worse in practice: "rather than", "no", "never", "not" and "without" all negate, sentence
boundaries in Markdown are not what a period says they are, and the rule would pass a sentence
that negated something else.

**Put the §15.2 statement in a shared constant every document includes.** Markdown has no include.
The constant exists for the *code* paths (`ExecutionTier::boundary`), which is what §19.2 asks
for; prose gets a checked phrase list instead.

**Correct the README only, as §51.2 literally requires.** §19.1 names five surfaces and §65.5
names the class rather than the file. Correcting one of five and leaving the Wiki claiming that a
package "never reaches them directly" would satisfy the sentence and not the requirement.
