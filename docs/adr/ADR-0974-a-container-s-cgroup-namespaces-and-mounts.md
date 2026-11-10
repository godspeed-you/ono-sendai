# ADR-0974: A container's cgroup, namespaces and mounts

- Status: accepted
- Date: 2026-11-10
- Spec refs: spec §9.1, §10.5, §22.2, §35.3; ADR-0112, ADR-0968, ADR-0971
- Decided by: agent (autonomous)

## Context

`trace container` promises "a container's namespaces, cgroups, mounts, processes, sockets and
image" (§9.1). ADR-0968 delivered the processes and left three edges open (#221): a namespace
and a cgroup were `planned` targets with no record to stand at the far end, and the mounts were
thought to need one `inspect` call per container.

Both reasons have gone. ADR-0971 made `cgroup` and `namespace` provider targets. And the
engine's listing carries the mounts: `GET /containers/json` answers with a `Mounts` array of the
same shape `GET /containers/{id}/json` gives — read from Docker's API on this build's host, where
`pcw-db-1` lists a bind and a named volume.

## Decision

**The cgroup and the namespaces are read from the container's processes.** The graph kernel
`linux.container-placement` finds the processes the way ADR-0968 does — the cgroup path that
carries the container's full id — and relates the container to

- each distinct cgroup those processes are in (`cgroup`), with `get cgroup`'s record at the far
  end, and
- each distinct namespace their `/proc/<pid>/ns/*` links name (`namespace`), with
  `get namespace`'s record of that inode and kind at the far end.

Both edges are `exact`: procfs states them. A far end procfs named and no provider served is a
failure of the trace (`io.not_found`), as is a namespace kind every process of the container
refused to name — one failure for the container, carrying the kernel's reason. A process that
exited during the scan says nothing.

**The mounts are the engine's.** `ono.container/1` gains the nullable field `mounts`: one map per
entry of the engine's `Mounts` array — `type`, `source`, `destination`, `read_only`, `name` —
and null where the answer carried no array (an event record, an engine that did not say). The
graph kernel `container.mounts` relates the container to the host path of each mount that has
one (`mount`), with the file provider's record of that path at the far end and the destination,
type and writability on the edge. The edge is `exact`, as the image edge is: the engine's own
bookkeeping. A `tmpfs` has no host path and is no edge. A host path the caller may not read —
a volume under `/var/lib/docker` for an unprivileged user — is a failure, never a missing mount.

`enter container` as an execution context stays declined (ADR-0114, ADR-0968).

## Consequences

Easy: `trace container web-1` answers where the container lives in the kernel and what of the
host it can touch; `get container | where mounts != null` is an ordinary pipeline over the
mount list.

Hard: the namespaces of a root-owned container are closed to an unprivileged caller, so the
usual unprivileged trace shows the cgroup, the mounts it may stat and one failure for the
namespaces. `get namespace` is read whole once per traced container — the cost of `lsns`.

The far end of a mount edge is a host path, never an `ono.mount/1`: the container's own mount
table lives in its mount namespace, and its mount points (`/`, `/proc`) would collide with the
host's under `ono.mount/1`'s identity, the mount point.

The acceptance container runs no container engine (case 046 asserts the refusals), so the
proof through the real binary is the CLI suite's fake engine socket.

Encoded by `crates/ono-graph/tests/relationships.rs::should_link_a_container_to_the_cgroup_and_the_namespaces_its_processes_are_in`,
`::should_report_a_namespace_it_cannot_describe_rather_than_drop_it`,
`::should_report_the_namespaces_it_could_not_read_rather_than_show_a_container_without_any`,
`::should_link_a_container_to_the_host_paths_the_engine_mounted_into_it`,
`::should_claim_no_mounts_for_a_container_whose_engine_reported_none`,
`crates/ono-provider-container/src/record.rs` (`should_carry_the_mounts_the_engine_reports_as_data`)
and `crates/ono-cli/tests/containers_packages.rs::should_relate_a_container_to_the_host_paths_mounted_into_it_when_tracing_it`.

## Alternatives considered

**Read the container's mount table from `/proc/<pid>/mountinfo`.** It lists what the container
sees — its overlay root, its `/proc` — as mount points inside another mount namespace. Records
of those under `ono.mount/1` would share identities with the host's mounts, and the question an
operator asks of a container is what of the host it mounts, which is the engine's list.

**Ask `get namespace` once per namespace with an `id` selector.** Eight enumerations of every
process where one suffices; the provider aggregates the whole table to answer any of them.

**Relate the container to its namespaces through the engine's `inspect`.** The engine reports
namespace *modes* (`host`, `container:<id>`), never the inodes that identify a namespace.
