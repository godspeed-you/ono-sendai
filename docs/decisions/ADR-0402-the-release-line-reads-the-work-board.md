# ADR-0402: the release line reads the work board

- Status: accepted
- Date: 2026-08-29
- Spec refs: AGENTS.md §9, §13, §15; `docs/ACCEPTANCE.md` §3, §4.5, §4.6.5, §4.7.2, §5
- Decided by: agent `HON-check` (autonomous)

## Context

`scripts/release-check.sh` decides when the run ends. It ran the quality gate, the containerised
acceptance suite and the package check, and then made exactly one judgement of its own: it
grepped `docs/ACCEPTANCE.md` for a line beginning `- [ ]`.

Three ticked boxes are not statements about the shell at all. They are statements about
`docs/STATE.md`:

- §4.5 *Delivery* — "`docs/STATE.md` has an empty *In progress* section and no unexplained
  *Deferred* entries";
- §4.6.5 *Delivery* — "`docs/STATE.md` has an empty *In progress* …";
- §4.7.2 — "**No release-blocking known defects remain.** `docs/STATE.md` *In progress* is empty
  … and every *Deferred* entry names an ADR saying why it does not block the release".

Nothing read that file. All three were false at the moment this ADR was written — four agents held
claims under *In progress* in parallel worktrees — which is entirely correct mid-run and is
exactly the point: a box whose only proof is that somebody once read a document is true on the day
it is written and unexamined ever after. `docs/ACCEPTANCE.md` §3 forbids that: a box is closed by
an automated case or test, never by judgement.

## Decision

`scripts/release-check.sh` reads the board before it prints the release line.

`xtask::scan::check_release_board` is a pure function over the text of `docs/STATE.md`,
exposed as `cargo xtask state-check`, and it enforces exactly the two properties the three boxes
assert and no more:

1. ***In progress* holds no claim.** An agent that has claimed work has unfinished work
   (AGENTS.md §9, §13), and a shell is not release-ready while somebody is in the middle of
   changing it. The section must hold nothing but blank lines: a claim written as prose is still a
   claim, so the check does not try to recognise the *shape* of one.
2. **Every *Deferred / blocked* entry names an ADR.** §4.7.2 requires each entry to say "why it
   does not block the release", and AGENTS.md §8 fixes the ADR as the only place that reasoning
   may live. An entry without one is deferred work nobody defended.

The check runs in `scripts/release-check.sh` and **not** in `scripts/gate.sh`. Holding a claim
mid-increment is the working rhythm of AGENTS.md §7; a gate that refused it would forbid the
method the repository is built on. What must never happen is *finishing* while a claim stands.

### *Next up* is deliberately not required to be empty

The obvious extension — refuse while an unticked box remains under *Next up* — is rejected.
`docs/ACCEPTANCE.md` §4.5 already calls that list "the deliberate post-release backlog", and
AGENTS.md §15 locates the stopping rule in `docs/ACCEPTANCE.md` §4, not on the board: what must be
closed before release is written there, in boxes, and *Next up* is what remains afterwards.
Requiring an empty backlog would make the release line unreachable and would contradict a box in
the same file it is meant to enforce. Twelve entries stand there today, every one of them a
`Class B`/`Class C` line the triage of 2026-08-29 deliberately placed after the release.

One claim §4.5 made about *Next up* is dropped rather than checked: "with an exit test named per
item" is not true of the list as it stands — seven of the twelve entries name one — and a
justification that is not true is worse than no justification. The box's own assertion is
unaffected; only the sentence that described the backlog is corrected.

## Consequences

- The stopping rule is now mechanical on both sides. `release-check.sh` refuses with the line
  `release-check: docs/STATE.md says the work is not finished`, preceded by the file, the line
  number and the reason.
- While the four parallel agents hold their claims, `release-check.sh` legitimately refuses.
  That is the check working, not a defect, and the refusal names the section it is refusing over.
- Clearing *In progress* becomes part of finishing, which is what AGENTS.md §17's end-of-session
  checklist already asks for and nothing enforced.
- The check cannot tell a *stale* claim from a live one. A board nobody updated for a week still
  refuses the release, and that is the safe direction: it is easier to notice a refusal than a
  ticked box.
- Two clauses of §4.6.5 stay outside it: "CI is green on `implementation`" cannot be observed by a
  local script, and "the acceptance suite is green" is proven by `release-check.sh` running it
  rather than by reading the board. Both are stated that way in the box now.
- Encoded by `xtask/tests/scan.rs`:
  `should_accept_a_board_whose_in_progress_is_empty_and_whose_deferred_entries_name_an_adr`,
  `should_refuse_to_call_the_shell_ready_while_an_agent_holds_a_claim`,
  `should_refuse_to_call_the_shell_ready_while_a_claim_is_written_as_a_table_row`,
  `should_refuse_a_deferred_entry_that_explains_itself_with_no_adr`,
  `should_ignore_an_unticked_box_under_next_up_when_judging_the_board`,
  `should_refuse_a_board_that_has_no_in_progress_section_at_all`.

## Alternatives considered

- **Grep `docs/STATE.md` from the shell script.** Rejected: the section boundaries and the
  entry/continuation structure are the whole difficulty, and a regular expression that gets them
  right is a program written in the one language the repository cannot unit-test.
- **Refuse while any `- [ ]` remains under *Next up*.** Rejected above: it contradicts §4.5 and
  makes the release line unreachable by design.
- **Untick the three boxes instead.** Rejected: the claims are the right claims, and all three
  will be true at the moment of release. What was missing was the referee, not the requirement.
- **Move the check into `scripts/gate.sh`.** Rejected: it would turn every held claim into a red
  gate and make the TDD loop of AGENTS.md §7 unrunnable.
