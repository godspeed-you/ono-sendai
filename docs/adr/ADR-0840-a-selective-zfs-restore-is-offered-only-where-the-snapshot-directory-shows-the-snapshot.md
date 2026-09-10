# ADR-0840: A selective ZFS restore is offered only where the snapshot directory shows the snapshot

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §13.5, Appendix D.4, §56.3, §54.4, Appendix G.3, Appendix F
- Decided by: agent (autonomous)

## Context

The live-pool suite (`crates/ono-recovery-zfs/tests/real_zfs.rs`), run for the first time against
a real pool in a privileged container, found that `<mountpoint>/.zfs/snapshot/<name>` exists there
and is empty: the snapshot is mounted on first access, and the kernel module does not mount into a
container's namespace. `zfs clone` and an explicit `mount -t zfs` of the snapshot both work. The
provider offered `SELECTIVE_FILE_RESTORE` on the strength of `selective_is_possible()` — dataset
mounted, writable, absolute mount point — and the plan it showed failed at apply with
`cp: cannot stat`. A plan that names a method which cannot run is the failure §13.5 exists to
prevent: *"The RecoveryPlan MUST display which method will be used."*

The same run found that OpenZFS cannot open a file vdev from inside a container either; a loop
device over the same file works.

## Decision

At planning time the provider lists the snapshot's directory with `ls -A` (`LS`, run through the
provider's `ToolRunner` like every other program). `SELECTIVE_FILE_RESTORE` is offered only when
the listing shows the snapshot's contents. An empty listing, a refused one, or no `ls` to ask
leaves the fact unestablished (§56.3), and Appendix D.4's `CLONE_AND_COPY` is planned instead; its
`zfs clone` action states that the selective restore was set aside and why. The check is evidence
for the choice of method and not a thirteenth §56.1 fact, whose list is the specification's.

The live harness backs its disposable pool with a loop device over its sparse file, falling back
to the file where `losetup` is absent.

## Consequences

A host that mounts snapshots on access keeps the selective restore; a container gets a clone,
and the plan says which one it is getting before anything runs. `ls` is an optional program: a
host without it plans the clone. Tests: `crates/ono-recovery-zfs/tests/recovery.rs` (an empty and
a refused listing), `real_zfs.rs` (either copying method, the changed file back, the unrelated
file untouched).

## Alternatives considered

Falling back to a clone at restore time. Rejected: the operator would have been shown one method
and had another run. Mounting the snapshot explicitly for the selective route. Rejected for now:
it needs a mount point and mount privilege of its own, which the clone route already provides,
together with a cleanup the provider owns.
