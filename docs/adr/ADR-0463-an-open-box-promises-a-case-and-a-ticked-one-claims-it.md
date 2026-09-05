# ADR-0463: An open box promises a case, and a ticked one claims it

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §57 (the H0–H12 sequence), §40.3; AGENTS.md §9, §10;
  `docs/ACCEPTANCE.md` §3, §4.7, §4.8; ADR-0401 (the check this amends), ADR-0429 (which reserved
  the case numbers)
- Decided by: agent (autonomous)

## Context

ADR-0401 made `docs/ACCEPTANCE.md` resolve its own pointers: a box closes by naming the
acceptance case that proves it, and `xtask::scan::check_acceptance_case_references` refuses a name
that no file in `docker/acceptance/cases/` carries. A ticked box pointing at nothing is a claim
with the evidence deleted, and that is the defect the check exists for.

The check has to tell a case name from a number, because `200-column` and `512-byte` are written
exactly like `171-authenticated-link-refuses-a-changed-key`. ADR-0401's answer was the range: a
token counts as a reference only when its number is at or below the highest number the suite
actually uses. On 2026-09-02 the highest was `171`.

ADR-0429 then wrote §4.8 — 118 boxes for a tranche that had not started — and reserved case
numbers **180–200** for it, ascending with the phase order. The reservation was chosen precisely
so the numbers sat above `171` and the check stayed quiet while the cases were written one at a
time.

That reasoning was wrong, and it took four hours to show it. The H4 increment wrote
`docker/acceptance/cases/189-kuang-confinement-fail-closed.case`. `highest` moved from `171` to
`189`, and every reserved number below it — `180` through `188`, named by boxes for work that has
not been done — became a dangling reference in the same instant.
`xtask/tests/scan.rs::should_report_this_repository_as_naming_only_acceptance_cases_that_exist`
went red, and the gate with it, on a tree in which nothing was wrong.

The defect is not in the reservation. It is in the range heuristic being asked to answer a
question it cannot see: *is this sentence a claim, or a plan?*

## Decision

**A case reference inside an unticked box is not resolved. A case reference inside a ticked box
is resolved, whatever its number.**

`docs/ACCEPTANCE.md` §4.7 already established the convention this reads from, in as many words:

> Where a test named here does not exist yet, the box names the file and behaviour the delivering
> increment must create; writing it is part of that increment, not of a later one.

An unticked box is a commitment. The case it names is absent **by definition** — that is what
makes it work to do. Resolving that pointer would make it impossible to write a checklist ahead of
the work, which is the one thing a checklist is for. Ticking the box is exactly the moment the
sentence stops being a plan and becomes evidence, and from that moment the pointer is resolved
like any other.

A box runs over the indented lines that continue it, and names its proofs on the last of them, so
`continues_an_open_box` carries the state across the continuation. `- [x]` ends a box as surely as
a blank line does: a ticked box is read like ordinary prose.

The range heuristic stays for everything outside a box. It is still what tells `200-column` from a
case, and prose is where such a number appears.

## Consequences

- The check gets **stronger**, not weaker. Before this, a ticked box naming a case above `highest`
  was skipped silently — the exact case ADR-0429's reservation would have created had a box been
  ticked early. Now a ticked box is resolved whatever number it names.
- §4.8 can name cases `180`–`200` for as long as the tranche takes, and each box's tick is what
  demands the file.
- ADR-0429's reservation survives, and its stated rationale does not: the numbers were chosen to
  sit above `171`, and that is no longer why it works. It works because the boxes are open.
- A box ticked before its case is written now fails the gate on the tick rather than on some later
  unrelated increment. That is the failure landing where the mistake is.
- Nothing changes for `docs/STATE.md` or the narrative specifications; both were already out of
  scope, for the reasons ADR-0401 gives.

## Proof

`xtask/tests/scan.rs::should_let_an_open_box_name_the_case_the_delivering_increment_must_write`
and `::should_read_an_open_box_to_the_end_of_the_lines_that_continue_it` were red before the
change — the second one showing that a rule reading only the line with the bracket would police
the continuation and let the first line through, which is backwards.
`::should_still_resolve_the_case_a_ticked_box_claims_as_its_proof` was green on arrival and is
kept as a guard: it is the assertion that must never stop holding.
`::should_report_this_repository_as_naming_only_acceptance_cases_that_exist` is green again.
