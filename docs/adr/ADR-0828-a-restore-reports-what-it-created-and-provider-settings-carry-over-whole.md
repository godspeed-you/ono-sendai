# ADR-0828: A restore reports what it created, and provider settings carry over whole

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §11.1, §14.5, §53, Appendix D.3
- Decided by: agent (autonomous)

## Context

The Btrfs clone-and-copy and subvolume replacement derive a writable subvolume from a snapshot;
§14.5 makes it an asset of its own. `RecoveryProvider::restore_with` returned `()`, so the derived
subvolume could not reach the store and nothing would ever clean it up. Separately, §53's
`recovery.min_filesystem_free` can be a share, a quantity or both, and the ZFS provider's floor
knew only one at a time; and §53 has both a global and a Btrfs-specific read-only preference.

## Decision

- `restore_with` returns `RestoreOutcome`, which lists the assets the action created (empty by
  default). The shell records each against the recovery plan; a creation the store cannot record
  makes the action's outcome `unknown`, never plain success.
- The ZFS provider's `FreeSpaceFloor` gains `Both { share, bytes }`: the pool keeps whichever part
  demands more, and an unmeasured pool fails closed. The shell carries every part of the configured
  floor over, so no part is silently dropped.
- The Btrfs provider prefers read-only snapshots only while both `recovery.prefer_read_only_snapshots`
  and `recovery.btrfs.prefer_read_only_snapshots` say so: either key set to `false` takes effect,
  so neither is a setting that is read and then ignored. ZFS snapshots are read-only by nature, so
  the global key has no other reader. `recovery.btrfs.root_recovery` is converted by an exhaustive
  match so a new variant cannot fall through to a default.

## Consequences

Tests: `crates/ono-recovery-zfs/src/floor.rs` (three unit tests),
`crates/ono-recovery-btrfs/tests/restore_execution.rs`.

## Alternatives considered

A side-channel `take_created_assets` on the provider. Rejected: it couples the answer to call
order and hides it from the type of the call that produced it.
