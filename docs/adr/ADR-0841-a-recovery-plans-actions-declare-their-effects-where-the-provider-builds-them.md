# ADR-0841: A recovery plan's actions declare their effects where the provider builds them

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §2.12, §3.3, §8.1, §8.2, §13.6, §14.4, §15.4, §19.4, §24.5; ADR-0839
- Decided by: agent (autonomous)

## Context

§8.2 gives every plan the list of effects its actions may have, and §2.12 makes a recovery a
change like any other. A recovery plan's list was empty: no first-party provider declared an
effect on its restore actions, and `ono_change_recovery::builder::recover_action`, which rebuilds
each provider action under the recovery plan's own identity (§3.3), dropped whatever an action
might have declared. `get plan <recovery>` therefore said a restore that overwrites a file would
do nothing.

## Decision

- `PlanAction::declaring(domain, kind, confidence, object, explanation)` builds the one effect an
  action has on one object from that action's own identity, where the action is built.
- `ProposedEffect::for_action` re-anchors an effect to a rebuilt action, and `recover_action`
  carries every declared effect across.
- The providers declare, each with `guaranteed` confidence — §8.1: the effect follows directly
  from the completed operation and the provider's contract:
  - files: a restore replaces the live object (§15.4);
  - ZFS: a copy out of the snapshot or the temporary clone replaces the live file, a rollback
    replaces the dataset's state, destroying newer history removes the snapshot or bookmark
    (§13.6), the temporary clone is created and removed (Appendix D.4);
  - Btrfs: a restore replaces the live file, a subvolume replacement replaces the live
    subvolume (§14.4), steering the next boot modifies the default subvolume or replaces the
    live name (Appendix D.9).

Destroying newer history is declared as a removal and is not marked irreversible on the effect.
§13.6 and §24.5 already refuse it until the operator accepts that loss by name, and the recovery
plan lists what would be lost in its newer-state analysis; marking the effect irreversible as
well would raise §19.4's separate irreversibility gate and ask the same question twice.

## Consequences

A recovery plan's effect list says what its actions will do, object by object, and callers that
assess a plan by its effects see a restore as the replacement it is. Tests: the provider suites
assert the declared effect on each restore action, and `crates/ono-change-recovery/tests/builder.rs`
asserts that a carried effect belongs to the rebuilt action.

## Alternatives considered

Deriving the effects in `recover_action` from each action's summary or execution. Rejected: the
provider is the one that knows what its operation does (§12.1), and a summary is prose.
