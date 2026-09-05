# ADR-0427: A test helper is shared only when it is identical

- Status: accepted
- Date: 2026-09-01
- Spec refs: AGENTS.md §11 (a pure refactor leaves the suite green and unchanged), §16 (a test
  states its preconditions directly)
- Decided by: agent (autonomous)

## Context

`fn ono(script)` was declared by hand in 24 integration suites, `fn text(row, field)` in sixteen,
`fn rows(run)` in fifteen, `fn registry()` in seven. The obvious reading — one helper copied
everywhere — is wrong: `ono` had **eleven** distinct implementations, `rows` thirteen, `text`
eleven. They are the same name over different behaviour, which is worse than duplication, because
two suites asserting the same contract can disagree about what the contract is and neither will
say so.

Measured across `crates/*/tests/`, byte-for-byte identical helper bodies accounted for 394 lines
in groups of three files or more. The rest are genuine variants: different budgets, different
panic messages, different tolerance for stderr.

## Decision

**A helper moves to a shared module only when every copy of it is byte-for-byte identical.** A
variant stays where it is.

The rule is not tidiness, it is provability. Unifying two helpers that differ picks one of them
for callers that were using the other, which changes what a test does — a refactor that cannot be
shown to leave the suite unchanged (AGENTS.md §11). Where the difference is real, two helpers
with one name is the honest state, and the file that needs its own reading keeps it.

Homes, nearest first:

* `ono-testkit` for what every crate wants: `ono(script)` and `ono_within(script, budget)`.
  `ono_within` exists because seven suites ran on a 30 s budget rather than the default 20 s, and
  flattening that would have shortened a budget somebody had a reason for.
* `crates/<crate>/tests/support/mod.rs` for what one crate's suites want — the pattern ten crates
  already used. `ono-cli` and `ono-command` gained one; `ono-process` already had one.

What deliberately stayed put: `files.rs::text` (names an ActionResult field in its panic),
`storage.rs::rows` (reports stderr differently), and therefore `single_result` and
`assert_failed_row`, which call them. Moving those would change what a failing test prints, which
is the diagnostic a reader depends on.

## Consequences

Easy: one definition of "run this script and read the answer". A reader of a suite sees its
subject rather than 30 lines of preamble, and a suite that wants the shared reading gets it by
importing rather than by copying.

Hard: a test file no longer shows every helper it uses in its own text, so `support::` is one
indirection to follow. The remaining 152 lines of identical helpers in groups of three are left
for the crates that have no support module yet — adding one for six lines would cost more than it
saves.

Encoded by: `crates/ono-testkit/tests/harness.rs::should_run_a_script_through_the_shell_when_asked_for_one`,
`::should_take_a_wider_budget_than_the_default_when_a_script_is_given_one`.

## Alternatives considered

**Unify every same-named helper on one implementation** — 24 suites would silently change which
`ono` they run, including seven whose budget would shrink. Exactly the change AGENTS.md §11
forbids in a refactor.

**Leave all of it alone** — the drift is already measurable, and each new suite copied whichever
neighbour it was written next to.

**Put everything in `ono-testkit`** — `registry()` reads `ono-command`'s embedded contracts, so
the testkit would have to depend on the crate it is meant to be neutral about.
