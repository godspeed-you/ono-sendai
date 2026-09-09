# ADR-0801: The change, recovery and transaction families open E17, E18 and E19

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.6 §45; v0.2 §43; ADR-0006, ADR-0610
- Decided by: agent (autonomous)

## Context

v0.6 §45 lists thirty-five error names across three families — `change.*`, `recovery.*` and
`transaction.*` — and says "at minimum". It fixes no numbers, which is the opposite of the problem
v0.5 §34 posed (ADR-0610), and leaves two questions: which numeric blocks, and which of the
refusals the implementation needs are covered by "at minimum".

`docs/contracts/errors.yaml` and `ono_core::ErrorCode` are compared bidirectionally by
`xtask/src/contracts.rs::check_error_registry`, and the registry's opening rule is that a code is
never renumbered, never removed and never re-pointed. The blocks in use are E00–E13, E15, E16 and
the K11 namespace; E1401 is `spatial.cost_refused`, so E14 is partly spent.

## Decision

Three free blocks, one family each: **E17 for `change.*`, E18 for `recovery.*`, E19 for
`transaction.*`.** Forty-nine codes, in the order the specification introduces the concepts.

Fourteen of the forty-nine are beyond §45's list. Each is a refusal the specification requires
somewhere else and does not name in §45, and each exists because the alternative was to reuse a
code for two different meanings:

| Code | Why it exists |
|---|---|
| `change.plan_sealed` | §4.4 — editing a sealed plan is a different refusal from applying an unsealed one. |
| `change.plan_not_found`, `change.plan_reference_ambiguous` | §36.4 — a reference that resolves to nothing and one that resolves to two are different problems with different remedies. |
| `change.plan_state_invalid` | §4.1 — a transition the machine does not draw. |
| `change.plan_store_unavailable`, `change.plan_store_corrupt` | §36.1, §36.2 — the store is persistent and versioned, so it has the two failures every store has. |
| `change.verification_missing` | §23.1 — the rule that makes an unverifiable mutating plan unsealable needs a code to refuse with. |
| `change.auto_recovery_rejected` | §26.3 — "MUST be rejected at seal time" needs a refusal. |
| `change.privilege_required` | §43.3, §43.4 — and it carries whether the privilege is needed to change or to recover, which §43.4 makes a distinct fact. |
| `recovery.asset_not_found` | §37.5's counterpart to `change.plan_not_found`. |
| `recovery.asset_stale` | §18.3 — a stale asset is not an expired one, and the remedies differ. |
| `recovery.quiesce_failed`, `recovery.resume_failed` | §18.4 — "Failure to resume is a critical error and must be surfaced separately", which is only possible with a separate code. |
| `recovery.plan_incomplete` | §56.3 — fail closed needs something to fail with. |

`docs/contracts/recovery/errors.yaml` indexes all forty-nine by the question a reader is asking,
and carries a `nothing_changed` column per code. That column is the one worth arguing about, and
it is there because Appendix F's entire matrix turns on an operator being able to tell a refusal
from a partial apply. Every `true` row's help text in the global registry says so in words, and
`xtask/src/change.rs::check_errors` refuses a code that does not answer the question.

## Consequences

A reader holding §45 open finds every name it lists, at a number §45 does not state. The three
families are contiguous and readable at a glance from the code alone, which is what the numbering
is for.

Fourteen extra codes is a larger expansion than any previous tranche made to a specified family.
The alternative — folding, say, `resume_failed` into `quiesce_failed` — would have made §18.4's
"surfaced separately" impossible to satisfy, and each of the fourteen was added for a comparable
sentence rather than for tidiness.

## Alternatives considered

**Reuse E14, filling the gaps around `spatial.cost_refused`.** Rejected: a family that is not
contiguous is a family nobody can read out of a log.

**Number them by §45's order and stop at thirty-five, folding the rest into the nearest neighbour.**
Rejected, per the table above: several of the folds would make a sentence in the specification
unsatisfiable.
