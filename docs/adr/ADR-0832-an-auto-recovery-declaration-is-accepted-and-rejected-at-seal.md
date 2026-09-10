# ADR-0832: An auto-recovery declaration is accepted, and rejected at seal

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §4.5, §26.1, §26.2, §26.3, §53
- Decided by: agent (autonomous)

## Context

§26.3 lets a plan declare automatic recovery after verification failure only when six conditions
hold, and otherwise requires the declaration to be "rejected at seal time". The predicate existed
(`ono_change_recovery::admits_auto_recovery`), but no plan could declare anything, so the rule was
tested and never applied. Two of the six conditions cannot hold at seal in this build:

- "recovery plan can be fully constructed before mutation" — a recovery plan is built against a
  validated recovery asset (§11.4), and the plan's assets are created by `apply` at §4.5,
  immediately before mutation (§18.1). At seal nothing exists to restore from;
- "user policy explicitly enables it" — §53, the reference configuration, defines no key for it,
  and §26.1 makes automatic recovery off by default "intentionally".

## Decision

`plan --auto-recover` is the declaration. At seal it runs `admits_auto_recovery(plan, None,
false)`: the recovery plan is `None` because none can be built before the protection exists, and
no policy enables it. The declaration is therefore always refused with
`change.auto_recovery_rejected`, naming every unmet condition, and nothing is sealed. No setting is
invented to switch it on: §26.2's reason — a failed verification does not prove rollback is safer
— is the one a future ADR would have to answer before adding one.

## Consequences

The box is true of the product: a declaration is rejected at seal, with the conditions it failed.
Automatic recovery never runs, which is §26.1's default. Tests: `crates/ono-change-recovery/tests/auto.rs`,
and the CLI test that plans with `--auto-recover`.

## Alternatives considered

- *Admit the declaration and check again after preparation.* Rejected: §26.3 places the decision
  at seal, and a plan sealed as "will recover automatically" that later declines is exactly the
  misleading state the lifecycle forbids.
- *Add `recovery.auto_recovery = true` to the configuration.* Rejected for now: it would switch on
  a behaviour whose first condition this build cannot meet at seal.
