# ADR-0552: A snapshot pair needs a host that held still, and says so when it did not

- Status: accepted
- Date: 2026-09-03
- Spec refs: v0.4 §6.9 ("`map --json` returns `SpatialMap` and MUST not depend on terminal
  rendering"), §43.2 ("map coordinates never affect semantic identity"), §34.2 (view budgets),
  §53 (the default map is bounded and relevance-ranked); v0.4.1 §38.2, §38.4 (the skip taxonomy),
  §2.7; AGENTS.md §11; ADR-0145, ADR-0513, ADR-0514, ADR-0517
- Decided by: agent (autonomous)

## Context

`crates/ono-cli/tests/spatial_map.rs::should_return_the_same_node_identities_when_the_terminal_width_changes`
failed once under a full parallel workspace run and passed three times in isolation immediately
afterwards.

It read `map --json` at `COLUMNS=40` and again at `COLUMNS=200` and required the two documents to
name the same nodes. `COLUMNS` is read from the process environment, so those are two `ono` runs
— and the default map is relevance-ranked and cut at §34.2's thirty-node budget, on a host that
holds far more than thirty objects. A process that started between the two reads can take the
last place from one that ended, and then the two documents disagree for a reason that has nothing
to do with the width.

Reproduced without the suite, by reading the two widths in a loop while short-lived
CPU-hungry processes came and went: **1 pair in 25 disagreed**, by exactly one node each time —
`ono:stable:2e31661f…` in the narrow read against `ono:stable:2337ea9e…` in the wide one, both
processes ranked into the last places of the budget. On a quiet machine, 0 in 40.

The file already knew about this trap. `maps_at` sits forty lines above the failing test with a
doc comment saying it: "Two `ono` runs see two different systems: processes come and go between
them, and which hundred of three hundred a bounded map draws (§34.2) then differs for reasons that
have nothing to do with what the test is measuring." Every other multi-map assertion in the suite
uses it. This one could not, because the input it varies is a process-level environment variable.

## Decision

**Agreement settles the question. A disagreement is read again at the first width, and the pair of
identical reads is what makes the third read's difference attributable.**

- Read at 40 columns, read at 200. If they name the same nodes, the assertion has passed and the
  test is done — same two runs, same cost, same assertion as before.
- If they differ, read 40 columns once more. Two identical reads at 40 columns *bracket* the
  200-column read, so nothing moved across it and the width is what is left: the test fails,
  naming the nodes each width produced.
- If the two 40-column reads also differ, the host moved and this run cannot answer the question.
  Try again, up to three times, and then announce `SKIP(fixture_not_applicable)` saying which two
  reads disagreed.

**The third read is evidence, not a retry.** It happens only when there is a disagreement to
explain, it can only ever turn a difference into a *failure*, and the assertion is never re-run
after it fails. What is retried is the fixture's precondition — a moment in which the host holds
still — which is the same kind of thing as reserving a free port, and is why the loop is around
the acquisition rather than around the assertion.

**The skip is declared in both registries.** `declared:` carries the row §38.3's static half
requires. `canonical_ci.permitted_skips` carries the condition, because whether a host holds still
is a property of that host: requiring the skip would be red on a quiet runner and forbidding it
would be red on a churning one, and ADR-0517 already established that the honest statement in that
case is the condition (§38.2). Three attempts were never exhausted on a machine at load 124 across
8 processors, so the entry is a statement about hosts busier than any this repository has met.

## Consequences

Easy: the test fails for one reason — a width that changed the structured map — and its failure
message now carries the evidence that the host was not moving, which is what the previous message
could not say. A busy host produces a skip that names what it saw instead of a red result that
names the wrong cause.

Hard: on a host churning fast enough to exhaust three attempts, §6.9 goes unchecked for that run.
That is honest and it is bounded: the acceptance container is a two-process host where nothing
churns, and `docker/acceptance/cases/` measures the same contract there.

Also hard: the happy path is two runs and the unhappy one is up to nine. A machine that is
constantly disagreeing pays three times as much for a skip. It is the machine that made the
question unanswerable.

Verified: 15 runs at a load average of 124 on 8 processors, 0 failures and 0 skips; and by
replacing the wide read with `map --json --zoom 1`, which genuinely names different nodes on a
still host — red at the width assertion, with both node sets in the message.

## Alternatives considered

**Take both widths inside one `ono` run, the way `maps_at` does.** The width comes from the
process environment and there is no statement that changes it mid-session. Adding one to make a
test possible would be adding a product surface for a test's convenience.

**Compare only the stable geography — the six domains and the canonical collections — and ignore
the ranked objects.** It removes the churn and most of the assertion with it: the interesting half
of §6.9 is precisely whether the bounded, ranked part of the map is chosen the same way at any
width.

**Assert at a place whose population the fixture owns.** There is none. Every place the map can
open is the host's, and the objects a fixture adds are added *to* the host rather than instead of
it.

**Widen the comparison to "mostly the same nodes".** A tolerance on an identity set asserts
nothing: §43.2 says identity is not layout, and a map that renamed one node in thirty at another
width would satisfy any threshold anybody would dare write.
