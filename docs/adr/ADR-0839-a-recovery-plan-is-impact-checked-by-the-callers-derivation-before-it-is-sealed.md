# ADR-0839: A recovery plan is impact-checked by the caller's derivation before it is sealed

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §2.12, §9.1, §9.5, §24.1, §63 (twelfth release criterion); ADR-0822
- Decided by: agent (autonomous)

## Context

§2.12 makes recovery itself a change: it is planned, impact-checked and verified. The recovery's
`ChangePlan` is built by `ono-change-recovery`, which reads no topology (§24.1's design: the world
arrives as values). It was sealed with an empty impact graph, and `impact <recovery>` answered
`complete: true` with no node while the plan would overwrite a file — a graph calling itself
complete that nobody derived (§9.5). Writing `crates/ono-cli/tests/change_recovery.rs` found it.

## Decision

`RecoveryRequest::deriving_impact` takes the caller's derivation, a
`Fn(&[FrozenTarget], &[PlanAction]) -> ImpactGraph`. The builder calls it once the recovery's
targets and actions are in and before the seal, because a sealed plan is immutable (§2.6, §4.4).
`recover` in the shell passes the same `ono_change_impact::derive::derive` over the session's
spatial index that `plan` runs. Without a derivation, the plan names each object it restores as a
direct target and the graph is marked truncated with a §9.5 reason. The re-analysis `apply` runs
before a recovery (ADR-0822) passes none: it weighs losses only, and the stored plan keeps the
impact the operator was shown.

## Consequences

`impact` and `inspect plan` show what a recovery touches before it runs, from the same walk every
other plan gets. A recovery's actions still declare no proposed effects, so its effect list is
empty while its targets and impact are not; that is recorded under *Found, not yet filed*. Tests:
`crates/ono-change-recovery/tests/builder.rs` (two), `crates/ono-cli/tests/change_recovery.rs`.

## Alternatives considered

A dependency from `ono-change-recovery` on `ono-change-impact` with the spatial index in the
request. Rejected: it pulls the v0.4 topology into a crate whose contract is to read nothing.
Setting the impact in the shell after the seal. Rejected: that edits a sealed plan.
