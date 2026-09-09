# ADR-0804: The plan lifecycle is a total function, and the edge that is absent is the invariant

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.6 §2.3, §4.1, §4.5, §4.7, Appendix F, Appendix F.2
- Decided by: agent (autonomous)

## Context

§4.1 draws the plan lifecycle as two diagrams — the main path and the recovery branch — with
nineteen states between them. Appendix F then defines a mandatory behaviour matrix keyed on where
a failure happened, and the whole matrix turns on one distinction: whether anything was mutated.

That distinction is not decoration. `PREPARE_FAILED` means the operator's system is exactly as it
was; `APPLY_FAILED` means what ran, ran. §2.3 states it as an invariant — "If a required recovery
asset cannot be created, mutation MUST NOT begin" — and the whole of §55.7 case 31 is a test that
zero mutate actions executed.

An implementation can express that as a state variable the executor assigns, and most would. The
problem with a state variable is that the invariant lives in the assignments: every `plan.state =
Applying` is a place where §2.3 can stop holding, and there is no way to check that none of them
is wrong except by reading all of them.

## Decision

`PlanState::after(LifecycleEvent) -> Option<PlanState>` is a **total function** over
`(state, event)`, and it is the only way a plan's state changes. Every edge §4.1 draws is an arm;
everything else answers `None`, and `ChangePlan::advance` turns that into
`change.plan_state_invalid` rather than a silent assignment.

Three consequences are the point:

1. **§2.3 is the absence of an arm.** There is no `(PrepareFailed, BeginApply)` arm, so a plan
   whose preparation failed cannot be told to apply. `xtask/src/change.rs::check_transitions`
   asserts that absence directly, as its own check, beside the table comparison — because an arm
   somebody adds in good faith is exactly how a safety invariant stops holding, and a
   bidirectional table comparison would happily accept a new edge that was also added to the
   registry.
2. **Appendix F is a predicate, not a convention.** `PlanState::has_mutated()` answers for every
   state, and the registry declares the same answer, and the gate compares them. A refusal that
   says "nothing was changed" and a state that says otherwise cannot coexist.
3. **The diagram is checkable.** `docs/contracts/change/plans.yaml` writes out all twenty-nine
   edges, and the gate compares them against the machine in both directions. A reader who wants to
   know whether §4.1 is implemented reads the registry rather than the executor.

Appendix F.2 gets the same treatment one level down. `ActionStatus::Unknown` answers
`may_have_mutated() == true`, because for a remote or non-idempotent action whose outcome cannot
be established, the conservative reading is the only one that cannot lose data — and
`Idempotency::Unknown` answers `permits_blind_retry() == false`, because §41.2 forbids blindly
rerunning it and a default of "probably fine" is that blind rerun.

## Consequences

The executor cannot invent a transition. Where it needs one §4.1 does not draw — and there was one
such case, resuming a failed recovery — the edge is added to `state.rs`, to the registry, and to
this record, which is three deliberate acts rather than one line.

An error message can name the state and the transition that was attempted, which is what
`change.plan_state_invalid` carries.

The cost is that a transition is slightly more work to add than an assignment. That is the intended
cost, and it is small: `PlanState::after` is one `match`.

## Alternatives considered

**A state variable with an `assert!` at each assignment.** Rejected: the assertions are the same
distributed thing as the assignments, and `panic!` is lint-denied in library code for good reason.

**A typestate encoding, where each state is a Rust type.** Rejected: a plan is loaded from a store
at a state known only at run time (§41.2), so the executor would spend its life in a `match` that
re-erases the types. The total function is the same guarantee at the boundary where it is needed.
