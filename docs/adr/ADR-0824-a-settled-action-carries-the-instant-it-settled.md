# ADR-0824: A settled action carries the instant it settled

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §4.7, §39.2, §41.2, Appendix C.4
- Decided by: agent (autonomous)

## Context

The executor stamped every action status with the instant `apply` began. Appendix C.4 separates
the plan's own write from a later edit by when the write happened, and every write happens after
`apply` began — so the plan's own change always looked like newer state, and every ordinary
recovery would have been gated.

## Decision

`ApplyRequest::stamping_with(&dyn Fn() -> Timestamp)` supplies the clock the executor reads when an
action settles. Without one the executor keeps using the instant it was given, so scripted tests
stay deterministic (§39.2); the shell passes `Timestamp::now`. `PlanStore::applied_at` answers the
latest settle instant of an action that may have written (succeeded, failed, unknown).

## Consequences

Test: `crates/ono-change-executor/tests/lifecycle.rs`
(`should_stamp_each_action_with_the_instant_it_settled_rather_than_when_apply_began`),
`crates/ono-change-plan/tests/plan_store.rs` (`should_answer_when_a_plans_actions_last_settled`).

## Alternatives considered

A tolerance window around the apply start. Rejected: any window is either too small for a slow
action or large enough to hide a real edit.
