# ADR-0834: The two ZFS recovery settings decide, and preference never overrides least destruction

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §13.5, §13.6, §53, Appendix C.1
- Decided by: agent (autonomous)

## Context

§53 lists `recovery.zfs.prefer_selective_restore = true` and `recovery.zfs.allow_destructive_rollback
= false` and says nothing else about them; the shell parsed both and nothing used them. §13.6
requires every destroyed snapshot, bookmark and clone to be enumerated and explicitly accepted.
Appendix C.1 requires the least destructive method that satisfies the goal, and permits a lower
one only where the upper ones cannot keep what the goal requires.

## Decision

- `allow_destructive_rollback = false` (the default) is a policy the acceptance flag cannot answer:
  a rollback that would destroy newer history — or whose destruction could not be enumerated — is
  not planned. A less destructive method is planned where it meets the goal; otherwise planning
  refuses with `recovery.plan_incomplete`, naming the setting and what the rollback would destroy.
  `restore_with` checks the setting again at the act, before acceptance. `true` allows such a
  rollback behind `--accept-newer-state-loss`, as §13.6 describes.
- `prefer_selective_restore = false` is read as Appendix C.1's exception, not as a licence to be
  more destructive: a dataset rollback is chosen ahead of a selective restore only where every
  destructive fact is proven and the rollback keeps what a file copy cannot (hard links, file
  capabilities, security labels), and the plan says the setting chose it. An offline or next-boot
  root recovery is never promoted by it.
- A provider built without the policy uses §53's defaults.

## Consequences

Tests: `crates/ono-recovery-zfs/tests/recovery.rs` (eight cases). The shell wires both settings in
`crates/ono-cli/src/change/session.rs`.

## Alternatives considered

Reading `prefer_selective_restore = false` literally as "prefer rollback". Rejected: it would let a
configuration make recovery more destructive than Appendix C.1 permits.
