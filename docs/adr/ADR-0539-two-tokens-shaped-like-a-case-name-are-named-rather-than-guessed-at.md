# ADR-0539: Two tokens shaped like a case name are named rather than guessed at

- Status: accepted
- Date: 2026-09-03
- Spec refs: AGENTS.md §8, §14, §15; supplements ADR-0401 and ADR-0463
- Decided by: agent (autonomous)

## Context

ADR-0401 made every backticked `NNN-kebab-case` token in a Markdown document resolve to a file in
`docker/acceptance/cases/`, so a box cannot stay ticked after the case it names is renamed away.
It had to tell a case name from a number written the same way, and its answer was the suite's own
range: a token numbered above the highest case is a number in prose. ADR-0401 called this "one
documented hole" and named the two tokens that fall through it — a `200-column` terminal and a
`512-byte` frame — in that sentence, in backticks, because naming the counter-example is how the
decision argues. ADR-0463 repeats both, for the same reason.

Issue #119 added `200-refusals-name-the-deciding-boundary`, the case `docs/ACCEPTANCE.md` §4.8.12
reserves for the milestone. The moment it existed, 200 was inside the range, and the gate reported
four dangling references to `200-column` — in the two accepted decision records that exist to
explain why `200-column` is not a case.

AGENTS.md §8 forbids editing an accepted decision record, so the sentences cannot be rewritten.
AGENTS.md §14 forbids weakening the harness, so the check cannot simply stop reading decision
records: fifty-one of the fifty-three case references in `docs/adr/` are real, and a rule
that skipped the directory would drop all of them to fix two.

## Decision

`xtask::scan::NOT_A_CASE` names the two tokens: `200-column` and `512-byte`. A reference matching
one of them is not resolved.

A named list rather than a heuristic. No wording rule tells `200-column` from
`200-refusals-name-the-deciding-boundary` — ADR-0401 established that and it is still true — so
the alternative to naming them is guessing at English inside a gate. The list is short, it is
where a reviewer sees it, and adding to it is a deliberate act with an argument attached, which is
the same shape `ASSERTIONS` and `DISCLAIMERS` already have in `xtask::terminology` (ADR-0447).

`512-byte` is on the list although nothing reports it today: 512 is above the range and always
will be. It is there because it is the other half of the same sentence in both records, and
leaving it out would mean the next agent who reads those two ADRs finds one of the two tokens
handled and the other not, with nothing saying why.

## Consequences

- Case `200` exists and the gate is green, without either accepted record being touched.
- The cost is stated rather than hidden: were a case ever numbered and named `200-column`, a
  reference to it that went dangling would be ignored. Nothing in the suite's naming convention
  makes that likely — a case name describes what it proves — and the alternative costs fifty-one
  real references.
- The range heuristic keeps its job everywhere else. This list is the exception, not a
  replacement.
- Encoded by `xtask/tests/scan.rs::should_ignore_a_token_the_decision_records_name_as_not_being_a_case`,
  which also asserts that a genuinely dangling reference at the same number is still reported.

## Alternatives considered

- **Number the case something other than 200.** Rejected: `docs/ACCEPTANCE.md` §4.8.12 and §4.8.13
  both reserve `200` for this milestone and name it in a box, so moving the case would move the
  problem into the checklist.
- **Exempt `docs/adr/` from the check.** Rejected under AGENTS.md §14. Fifty-one of the
  fifty-three references in that directory are real pointers into the case suite, and an ADR that
  named a deleted case would stop being reported.
- **Supersede ADR-0401 and ADR-0463.** Rejected: both are correct about everything else they
  decide, and marking two accepted records superseded to fix two words in each would lose more
  than it gains. This ADR supplements them and says so.
- **Read the sentence around the token.** Rejected: "shaped exactly like case names" is prose
  about the token, and a rule that tried to recognise prose about a token would be a rule that
  fails differently rather than less.
