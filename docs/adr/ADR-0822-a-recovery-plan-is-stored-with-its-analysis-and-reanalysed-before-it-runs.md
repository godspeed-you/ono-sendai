# ADR-0822: A recovery plan is stored with its analysis and re-analysed before it runs

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §2.12, §7.3, §12.2, §24.1, §24.5, §56.1, §62.8, Appendix C.3, Appendix C.4
- Decided by: agent (autonomous)

## Context

The plan store kept only a recovery plan's `ChangePlan`. The newer-state analysis, the source
assets and the chosen method — everything §24.5's gate is about — were gone by the time `apply`
ran, so the gate was never consulted and `--accept-newer-state-loss` was parsed and ignored. A
file edited after the change was overwritten by `apply <recovery>` without any acknowledgement.
Recovery actions that a provider planned as programs (ZFS) were also run directly by the shell,
skipping the provider's own safety re-checks. ADR-0818 said the first-party providers plan their
recovery as `RecoveryOperation`s; the ZFS provider never did.

## Decision

1. **The analysis is stored.** Plan-store version 2 adds a `recovery` column beside each revision
   row. `PlanStore::put_recovery` writes the recovery-plan record; `get_recovery` reads it back
   onto the latest revision. A revision written without one (an acknowledgement, §19.4) carries
   the previous revision's analysis forward.
2. **`apply` re-analyses.** Before a recovery plan runs, the shell reruns Appendix C.3 with the
   stored inputs (source plan, source assets, goal, method, restore set). A loss — a conflicting or
   discarded object, an unknown one, a destroyed snapshot — that the stored plan did not show
   refuses with `recovery.newer_state_conflict`, whatever was accepted: an acceptance covers only
   the losses an operator was shown (§7.3 applied to recovery, §62.8).
3. **The gate is §24.5's.** `--accept-newer-state-loss` marks the current analysis
   `destruction_accepted`, and `ono_change_recovery::gate::check` decides. An analysis that could
   not complete still blocks with the flag given (§56.3). Accepting a loss requires `--confirm`
   outside a terminal (§40.3).
4. **Every recovery action goes to its provider.** Whatever its `Execution` variant, a recovery
   plan's action is executed by `RecoveryProvider::restore_with(action, asset, acceptance)` on the
   provider that owns the source asset (the one the action names, or the only one it can mean).
   The acceptance travels to the act. The snapshot providers re-prove §56.1's facts there and
   refuse a destruction nobody accepted even if the world moved after the gate. The file provider
   destroys no history; it re-checks its stored copy's digests and restores atomically, and for it
   the newer-state check is the shell's re-analysis at the start of `apply`.
5. **Appendix C.4 is decided on evidence.** A provider hands the digests its asset captured to
   the analysis (`RecoveryPlanFragment::capturing`); the file provider does so from its manifest.
   `recover` passes when the source plan's actions settled (`PlanStore::applied_at`), and the
   observer reports a file's mtime, so the plan's own write is not newer state and a later edit is.
   Assets are chosen newest-first per provider and domain, so an early `protect` does not shadow
   the asset taken just before the change (§18.2).

This corrects ADR-0818's statement about the first-party providers: whatever form an action has,
the provider runs it.

## Consequences

`apply <recovery>` on an unchanged file restores with nothing to type (§40.1); a later edit gates
on `--accept-newer-state-loss`; a world that moved after `recover` refuses and asks for `recover`
again. Tests: `crates/ono-change-plan/tests/plan_store.rs` (recovery round trip, revision
carry-over, `applied_at`), `crates/ono-change-recovery/tests/builder.rs`
(`should_decide_newer_state_from_the_digests_the_provider_captured`),
`crates/ono-cli/tests/change_recovery_apply.rs` (the end-to-end cases, including the ledger
record of a verified recovery and a resumed recovery passing the same gate).

## Alternatives considered

- *Re-derive the analysis at apply and skip storing it.* Rejected: the acceptance would then be an
  acceptance of whatever the analysis says now, which is exactly the loss nobody was shown.
- *Keep running program actions in the shell.* Rejected: it skips the provider's §56.1 re-checks.
