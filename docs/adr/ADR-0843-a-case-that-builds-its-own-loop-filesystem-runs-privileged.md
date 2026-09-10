# ADR-0843: A case that builds its own loop filesystem runs privileged

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §14, §54.4, §56.2, §63.9, Appendix G.3; AGENTS.md §10, §15; ADR-0237
- Decided by: agent (autonomous)

## Context

Cases 290, 291 and 319 prove §14's Btrfs semantics against a filesystem they build on a loop file,
as Appendix G.3 requires. They run as root with `CAP_SYS_ADMIN` and AppArmor unconfined, and
`mkfs.btrfs` succeeds, but the container's device cgroup hands out no `/dev/loop*` node, so every
run ended in a declared skip. Ten boxes of `docs/ACCEPTANCE.md` §4.12 cite these cases and the ZFS
ones, and AGENTS.md §15 lets a box be ticked only by a proof that runs in the gate or in the
acceptance suite: a skip proves nothing. The live suites (`real_btrfs.rs`, `real_zfs.rs`) did run on
2026-09-10 in a privileged container built by hand, which is evidence and not a referee.

## Decision

`scripts/acceptance.sh` gains the per-case directive `privileged: true`, which runs that one case
with `--privileged` and the host's `/dev`, so `losetup` finds the host's loop devices. It is for a
case that builds its own disposable loop filesystem and nothing else; like `capability:`, the
privilege is visible in the case that needs it, the network stays `none`, and every other case
runs unprivileged. Cases 290, 291 and 319 declare it, and their rows leave
`expected_test_skips.yaml`, so a host that cannot give them a loop device fails the run rather than
passing on checks that never ran.

The ZFS cases 289 and 318 keep their declared skips. The acceptance image is Debian, whose ZFS
userland lives in `contrib` and is not guaranteed to match the kernel module of whatever host runs
the suite; a mismatched userland would prove nothing about the provider. Their live proof is
`crates/ono-recovery-zfs/tests/real_zfs.rs`, run where ZFS exists, and the boxes that only it can
prove stay open as recorded exclusions.

## Consequences

The acceptance run proves nested-subvolume boundaries, the recovery workflows and read-only
snapshots against a real Btrfs. A runner has to allow privileged containers and carry the btrfs
module; GitHub-hosted Ubuntu runners do both. A privileged container has the host's devices, which
is why the directive is confined to cases that act only on loop files they create and remove.

## Alternatives considered

Passing individual loop nodes with `--device`. Rejected: node numbers are the host's, and a node
`loop-control` creates on demand does not appear in the container's own `/dev`. Keeping the skips.
Rejected: ten boxes would stay unprovable for a reason the harness can remove.
