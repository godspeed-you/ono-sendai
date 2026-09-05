# ADR-0401: a document that names an acceptance case must name one that exists

- Status: accepted
- Date: 2026-08-29
- Spec refs: v0.2 §50; AGENTS.md §10, §15; `docs/ACCEPTANCE.md` §3
- Decided by: agent `HON-check` (autonomous)

## Context

`docs/ACCEPTANCE.md` ticks a box by naming the case in `docker/acceptance/cases/` that proves it,
and an ADR records the case that encodes its decision. Both are pointers, and nothing resolved
them. `docs/STATE.md`'s phase lists had already been found naming eight cases that never existed
(028-prompt-shows-context, 040-process-provider … 046-service-provider — written plain here,
because a name set in backticks is a claim that it resolves); a scan of every
Markdown document in the repository found two more that no correction had reached:

- `ADR-0013` (the execution model) cited 035-interop-boundary as one of the cases encoding the
  §12.3 object/byte boundary. No such case exists — 035 is `035-scripting-language` — and the
  boundary is in fact proven by `040-object-pipeline`, which `docs/ACCEPTANCE.md` §4.4 already
  names for exactly that.
- `ADR-0236` (propagation peer groups) cited 122-privileged-network-and-mount. The case it
  describes is real and runs; its name is `122-mount-propagation-peers`.

A pointer at a case nobody runs is the same defect as a ticked box nobody checks: it reads as
evidence and is not. The names were correct when written and were renamed away afterwards, which
is precisely the failure a human reader cannot be expected to catch.

## Decision

Two things.

1. **The two dangling references are corrected in place**, to the case that carries the number
   and does the work. This edits accepted ADRs, which AGENTS.md §8 otherwise forbids; the rule
   there protects the *decision* and its history — superseding is done by a new ADR, never by
   rewriting an old one. Repairing a pointer to evidence changes no decision, no reasoning and no
   outcome; leaving it would preserve the text of the record at the cost of its truth.

2. **`spec-check` resolves every such pointer on every gate run.**
   `xtask::scan::check_acceptance_case_references` reads `docker/acceptance/cases/` and requires
   every *reference* in every Markdown document to name a file that is there. A reference is a
   backticked token of the shape `NNN-kebab-case` — the form every document in this repository
   already uses — whose number falls inside the range the suite actually uses. The problem it
   reports names the case that carries the number, so a rename is repaired rather than merely
   reported.

   Both halves of that definition are load-bearing, and this ADR is the evidence for the second:
   a `200-column` terminal and a `512-byte` frame are written in backticks and are shaped exactly
   like case names. No wording rule tells them apart from a case; the suite's own number range
   does, because it has never numbered a case above the hundred-and-twenties. The cost is one
   documented hole — deleting every case of a given number would stop references to it from being
   checked — and it is worth paying for a check with no false positives, because a check people
   learn to work around is worse than no check.

   Backticks carry the other half. A document that has to record a name as *absent* — this ADR,
   above — writes it plain, because setting a name in backticks is what makes it a claim that the
   name resolves.

   Two documents are out of scope, and neither exclusion is a convenience:

   - **`docs/STATE.md`.** Its session records deliberately name cases that never existed,
     because saying "these seven names were wrong" is how the board was corrected. Requiring the
     board to name only existing cases would forbid it from recording that a name was wrong. What
     the board itself claims is a separate question, answered by the next decision in this
     series (ADR-0402).
   - **The narrative specifications.** They are immutable (AGENTS.md §5.1), so a dangling name in
     one could never be fixed and the gate would be unpassable. They carry none today.

   Prose is not scanned for the *shape*: a `200-column` terminal and a 512-byte frame are written
   without backticks and are not references, and a name inside a fenced code block is sample
   output rather than a claim.

## Consequences

- Renaming an acceptance case now turns the gate red until every document that pointed at it is
  updated — which is the point: a rename is exactly when the evidence silently disappears.
- Adding a case reference costs nothing; writing one that does not exist costs a red gate with the
  right name in the message.
- The check is a *static* one: it proves the file exists, not that the case asserts what the
  sentence around it claims. That stronger property is what `xtask/tests/spatial_evidence.rs`
  does for §4.7 by naming assertions, and it does not generalise cheaply to the whole checklist.
- Encoded by `xtask/tests/scan.rs`:
  `should_reject_a_document_that_names_an_acceptance_case_that_does_not_exist`,
  `should_name_the_case_that_carries_the_number_when_a_reference_was_renamed_away`,
  `should_ignore_a_case_name_that_is_not_written_as_a_reference`,
  `should_ignore_a_case_name_inside_a_fenced_code_block`,
  `should_ignore_the_board_and_the_narrative_specifications_when_scanning_case_references`,
  `should_report_this_repository_as_naming_only_acceptance_cases_that_exist`.

## Alternatives considered

- **Scan `docs/STATE.md` too, with an allowlist of names known to be deliberate.** Rejected: an
  allowlist of wrong names is a second thing to keep true, and the board's corrections are written
  in prose that a list cannot follow.
- **Match any three-digit token, backticked or not, and require every one to exist.** Rejected,
  and this ADR was the counter-example: written that way, the check flagged nine tokens in this
  file alone — four of them the names it exists to record as absent, three of them measurements.
  Both restrictions above came out of that failure rather than out of anticipation.
- **Check case references in Rust sources as well.** Rejected as unnecessary today: a test that
  names a case file and cannot open it already fails. The Markdown documents are the ones where a
  dangling name survives silently.
