# ADR-0829: Three ZFS judgements: nothing to lose, zfs allow, and an unhealthy pool

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §13.6, §17.2, §40.1, §43.4, §50, §56.1, Appendix D.3
- Decided by: agent (autonomous)

## Context

The ZFS provider's recovery execution (ADR-0822) and privilege probe forced three decisions the
specification leaves open.

## Decision

1. **A rollback that would lose nothing needs no acceptance.** When a rollback destroys no newer
   snapshot, bookmark or clone and `zfs get written` reports zero bytes since the recovery point,
   §56.1's `HistoryDestructionAccepted` is established without `--accept-newer-state-loss`. The
   gate does not ask in that case (§40.1), and a provider that then refused would refuse an
   operator who was never asked. Everything else that rollback destroys needs the acceptance.
2. **`zfs allow` is read as an adapter fallback.** §43.4 needs to know whether an unprivileged user
   holds `rollback`, `destroy` and `mount`; `zfs allow` has no machine-readable output, so its text
   is parsed (§50 permits a documented adapter fallback). Group delegations are not evaluated and
   the evidence says so; a listing that cannot be read is a missing fact, which blocks.
3. **An unhealthy pool is refused where the mode decides.** A pool that is not ONLINE, reports
   data errors, or whose health could not be read is refused in `plan_protection` under `prefer`
   and `maximize` (the plan shows why), and at `create` under `require`, so a `require` plan fails
   at preparation with `recovery.asset_create_failed` rather than being sealed as if the pool
   could be trusted — mirroring Appendix D.3's handling of space.

## Consequences

Tests: `crates/ono-recovery-zfs/tests/recovery.rs`, `safety_checklist.rs`. Composed fixture rows
(a newer `tank/data` snapshot, clones, orphaned bookmarks, degraded pools, `zfs allow` output) are
labelled as composed; `scripts/fs-fixtures/zfs.sh` holds the real scenarios, which create pools on
the host and are run only by an operator who chooses to.

## Alternatives considered

Always requiring acceptance for a rollback. Rejected: it would gate the one rollback that loses
nothing, which §40.1 exists to keep usable.
