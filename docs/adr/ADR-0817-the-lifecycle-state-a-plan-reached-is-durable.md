# ADR-0817: The lifecycle state a plan reached is durable

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §2.7, §4.1, §4.5, §4.7, §22.2, §41.1, §41.2, Appendix F.2
- Decided by: agent (autonomous)

## Context

v0.6 §4.1 gives a plan nineteen states and §2.7 states the rule they exist to keep:

> A sealed plan applies once.

Driving the real binary showed that it did not. `apply <plan>` mutated the target, verified it and
returned; the store still held `sealed`, and a second `apply` of the same plan mutated the target
again. Every part of the run was correct in memory — the executor reached `VERIFIED`, the action
statuses were persisted, the recovery asset was created — and none of it reached the `plans.state`
column, because nothing wrote it.

The rule is a statement about the store rather than about one process. §41.2 reads the state back
after an interruption, §42.4's claim is released when the process ends, and §22.2's checkpoint is
about durability. A state that lives only in an `ApplyOutcome` satisfies none of them.

## Decision

**`PlanStore::record_state` writes the state, and the executor calls it at each transition** —
`PREPARING` before the first recovery asset, `APPLYING` before the first mutating action, and the
outcome's own state on every exit from `apply`, including every early return.

The column and the stored record are written together. `PlanStore::get_revision` decodes the
record, so a column nobody mirrored into it would be read back as the state the plan was written
with — the column would be right and every reader would be wrong.

Writing at the transition rather than at the end is the part that matters. A shell killed between
the asset and the mutation is found at `PREPARING`; one killed during the mutation is found at
`APPLYING`, which is what §41.1 resumes from and what Appendix F.2's uncertainty boundary is drawn
against. A single write at the end would leave every interrupted run indistinguishable from one
that never started.

Two related answers follow from the same finding, and both are about not describing an earlier run
as this one:

- **`ApplyOutcome::has_mutated` is a fact about this run.** It was `self.state.has_mutated()`, and
  a pre-flight refusal leaves the plan in the state it was already in — so refusing to re-apply a
  `VERIFIED` plan answered "yes, the system may have changed", about the run before. It is now
  `FailurePoint::may_have_mutated` wherever there is a failure point, which is Appendix F's second
  column asked of the row it belongs to.
- **`ApplyOutcome::is_success` is false whenever there is an error.** It matched on the state
  alone, so the same refusal reported success — §2.14 and §62.9's mistake one layer up, and the
  reason the shell printed `APPLY 1/1` above a refusal that reached no provider.

## Consequences

- A second `apply` of an applied plan is `change.plan_not_sealed`, naming the state it is in.
  `error::plan_not_sealed`'s help is chosen from that state: the advice for a plan that already
  ran is `inspect` and `recover`, and telling its operator about §4.2's draft was worse than
  saying nothing.
- `crates/ono-change-executor/tests/lifecycle.rs::should_remember_that_a_plan_applied_so_the_next_apply_refuses_it`
  is the regression test. It applies twice through one store and asserts the provider was reached
  exactly once.
- Every write is `let _ = …`: a store that cannot be written must not turn a successful apply into
  a failure, and the failure it would report is about the store rather than about the change. The
  next `apply` then re-reads `sealed` and would run again — which is the behaviour this ADR
  removes, so a durable store is a precondition for §2.7 rather than a nicety. §36.2 already makes
  an unwritable store a refusal at open.
- `ono-change-plan`'s `overlay_statuses` and this write are two paths to the same record. They do
  not overlap — one replaces action statuses on read, the other replaces the plan state on write —
  but a third writer of that record should use one of them rather than a third.

## Alternatives considered

- **Write the state from `ono-cli` after `apply` returns.** One call site instead of three, and it
  leaves nothing durable for the interrupted case, which is most of what §41 is about.
- **Derive the state from the persisted action statuses on read.** The store already overlays
  those, so the plumbing exists. It cannot distinguish `PREPARE_FAILED` from `SEALED` — both have
  no settled action — and Appendix F makes that difference the whole answer.
