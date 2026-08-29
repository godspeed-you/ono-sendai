# ADR-0236: A peer group is a fact a mount states

- Status: accepted
- Date: 2026-08-29
- Spec refs: v0.2 §22.2–§22.4 (`trace`, and never fabricating an edge), §23.5 (structured mount
  facts), §28.6 (`ono.mount/1`), §10.5 (absence is not failure), §35.3;
  `docs/spec/commands/storage.yaml` (`trace mount`); supersedes the propagation paragraph of
  ADR-0079
- Decided by: agent (autonomous)

## Context

`storage.yaml` says `trace mount` shows "a mount's device, filesystem, propagation peers and the
processes using it". Three of the four were delivered. ADR-0079 left the peers out and gave the
reason:

> Propagation peers stay unimplemented: `mountinfo` names peer groups, but relating two mounts
> through a peer-group id with no object to stand for the group would be an edge to nothing.

The reason assumes the edge must run *to the group*. It does not. Propagation is a relation
between **mounts**: two mounts in the same `shared:N` group propagate to each other — a mount or
unmount under one appears under the other. The group is why they are related, not what they are
related to.

## Decision

**`ono.mount/1` carries `peer_group`,** the `shared:N` of `mountinfo(5)`'s optional fields, as a
nullable int. It is a fact the kernel states about the mount, and providers own facts (§2.16), so
it belongs on the record rather than being re-derived by whoever wants it. `null` means the mount
is private — it propagates nothing — which is an absence the kernel states rather than something
unknown (§10.5, §35.3). A mount defined in `/etc/fstab` and not mounted has none either: the
kernel assigns a group when the mount happens.

Reading it needs no new parsing rule. `mountinfo(5)` puts the optional fields between the mount
options and the `-`, and `parse_mountinfo_line` already finds that separator rather than counting
columns — precisely so the variable-length optional fields could be read one day.

**`ono-graph` gains `MountPeers`,** which relates a mount to every other mount stating the same
group, with the group carried on the edge as metadata. The edge is `exact`: both ends state the
same number, and nothing is inferred from paths, names or devices (§22.4). A mount with no group,
or the only member of its group in this namespace, contributes no edge and reports no failure.

A mount that is a slave of another group (`master:N`) is not a peer of it: propagation there runs
one way, and calling it a peer would say something the kernel does not. That relation is a
separate one, and this ADR does not invent it.

## Consequences

- `trace mount <target>` over a bind mount of a shared mount draws the peer, which is the last of
  the four things `storage.yaml` promises. On an ordinary host it usually draws none, and that is
  the true answer: peers of a host's shared mounts live in *other* mount namespaces, and §22.4
  forbids relating across that line for the same reason ADR-0079 gave about `/proc/<pid>/root`.
- `get mount | where peer_group != null` answers which mounts propagate, which nothing could ask
  before.
- `ono.mount/1` gains a nullable field. Additive, no version bump; the default view is unchanged,
  so no table grew a column.
- Encoded by `should_report_the_propagation_peer_group_of_a_shared_mount`,
  `should_link_a_mount_to_the_other_mounts_of_its_propagation_peer_group`,
  `should_relate_a_private_mount_to_no_peer_at_all`, and acceptance case
  `122-mount-propagation-peers`, which makes a real shared bind mount under `CAP_SYS_ADMIN`.

## Alternatives considered

- **An object for the peer group.** ADR-0079's premise. It would be an object with no properties
  of its own, no provider, and no way to be entered or acted on — a node that exists to be a
  hyphen between two mounts.
- **Keeping the group out of the record and reading `mountinfo` again in `ono-graph`.** It puts a
  second reader of the kernel's mount table in a crate whose whole design is that it composes
  what providers say (§2.16), and it would drift from the first the day the format is extended.
- **Calling `master:N` a peer relation as well.** Propagation to a slave runs one way; a symmetric
  word for an asymmetric fact is the kind of edge §22.4 exists to forbid.
