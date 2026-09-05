# ADR-0079: `watch mount` and what `trace mount` relates

- Status: accepted
- Date: 2026-08-27
- Spec refs: §10.5, §18.2, §22.2–§22.4, §23.5, §28.6; ADR-0034, ADR-0078
- Decided by: agent (autonomous)

## Context

storage.yaml promises `trace mount` shows "a mount's device, filesystem, propagation peers and
the processes using it", and `ono-graph` had only the device half (`MountDevices`). The mount
watch needed its event schema. ADR-0078 fixed the shape of both; this ADR records the edge set
for mounts and the one rule that keeps it honest.

## Decision

### `ono.mount-event/1` carries the mount under `mount`

The envelope of ADR-0078, keyed `mount`; identity is the mount point (mount.v1.yaml), so a
remount is `changed` with `options`/`read_only` named and a device moved to another target is
`removed` + `added`. Polled from `/proc/self/mountinfo` at ADR-0034's cadence.

### Edge set

| Subject | Relation | Target | Read from | Confidence |
|---|---|---|---|---|
| `ono.mount/1` | `backed-by` | `ono.file/1` | the `/dev/...` source (ADR-0021, unchanged) | exact |
| `ono.mount/1` | `filesystem` | `ono.filesystem/1` | the filesystem record at the same target | exact |
| `ono.mount/1` | `root` | `ono.process/1` | `/proc/<pid>/root` lies on this mount | exact |
| `ono.mount/1` | `cwd` | `ono.process/1` | `/proc/<pid>/cwd` lies on this mount | exact |

"Lies on this mount" is the kernel's own rule: the mount whose target is the path's longest
prefix. A process working in `/home/x` on a separate `/home` mount is not a user of `/`.

Descriptors are not followed here: `trace mount` already reaches a process's open files one
hop later through `OpenFiles`, and scanning every descriptor of every process for a prefix would
make `trace mount /` cost what `trace process 1` costs times the process count.

Propagation peers stay unimplemented: `mountinfo` names peer groups, but relating two mounts
through a peer-group id with no object to stand for the group would be an edge to nothing;
it joins *Next up* in `docs/STATE.md`.

### Only this mount namespace

`/proc/<pid>/root` and `cwd` of a process in another mount namespace are paths in *that*
namespace. Comparing them with this shell's mount table would relate the wrong objects, so a
process whose `ns/mnt` differs from the reader's is skipped — not guessed at (spec §22.4:
"never fabricate an edge"). Processes whose links this user may not read are counted and
reported once on the graph's failure channel, as `SocketOwners` does.

### Not found is an error, not an empty graph

`trace mount /definitely/not/a/mount` is `resolve.target_not_found` (E0102) from the trace
command's subject resolution, unchanged from `trace process`; no graph is emitted.

## Consequences

- `trace mount /` on a workstation relates the root filesystem and, as an unprivileged user,
  every process of that user (their roots are `/`); the node cap of `DEFAULT_MAX_NODES` bounds
  it, and the shared per-trace snapshots keep the process table to one read.
- Tests: `crates/ono-cli/tests/storage_missing.rs` (watch mount, trace mount, not-found) and
  `crates/ono-graph/tests/relationships.rs` (`MountFilesystems`, `MountUsers` against a fixture
  `/proc` and stated mount tables).

## Alternatives considered

- **Relating processes by open descriptors as well.** Rejected for cost, above; reachable in
  one more hop anyway.
- **Skipping the namespace check and trusting the path text.** Rejected: a containerised
  process reports `/` as its root and would be drawn as a user of the host's root mount.
