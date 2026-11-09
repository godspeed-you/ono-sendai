# ADR-0968: A container's processes are the ones in its cgroup

- Status: accepted
- Date: 2026-11-09
- Spec refs: spec §9.1, §14.3, §22.2; ADR-0114, ADR-0115, ADR-0422
- Decided by: agent (autonomous)

## Context

`trace container` promised a container's namespaces, cgroups, mounts, processes, sockets and
image, and yielded one edge: the image (#221). The container record the engine lists carries no
pid — `GET /containers/json` does not include `State.Pid`, and asking `inspect` for every
container on every trace would make the trace cost one engine round trip per container. The
provider declares the relation `container.contains_process` for the spatial layer, and nothing
produced it.

The same issue names two more gaps: `enter container` narrows later commands to the container
and is not an execution context (`container.exec`), and the package mutations had no privileged
acceptance case.

## Decision

**A container's processes are found through the cgroup the engine put them in.** The graph
kernel `linux.container-processes` reads `/proc/<pid>/cgroup` for every process and relates the
container to each process whose cgroup path carries the container's full id — `docker-<id>.scope`
and `/docker/<id>` for Docker, `libpod-<id>.scope` for Podman. The full id is sixty-four hex
digits no other cgroup path can carry, so the match is exact, and the edge is `exact` with the
cgroup path as its evidence. No engine socket is read for it: the kernel's own view of the
processes is the source, as it is for every other process relationship of §22.2.

The namespace, cgroup and mount edges are not delivered here. A namespace or a cgroup is a
`planned` target with no provider record to be the far end of an edge; #206 composes those
collections from process records in this same tranche, and the container's edges to them follow
from the processes found here once there is something to point at. The mounts the engine reports
for a container are the engine's, not the host's mount table, and need the inspect call this ADR
declines to make on every trace.

**`enter container` stays a narrowing frame**, as ADR-0114 decided. Running later commands inside
the container's namespaces would make every provider read the container's `/proc` and
filesystem rather than the host's, which is a different execution model, not a frame; spec §14.3
forbids a context that acts on state the user cannot see, and a frame that claimed to execute
inside the container while the providers read the host would be exactly that. The gap is
recorded, not closed.

**The package mutations' success path is proved privileged** by acceptance case 131, which holds
and releases a package as root and checks `apt-mark showhold` at each step — the one package
mutation a container with no network can complete.

## Consequences

Easy: `trace container web-1` shows the processes inside it, from which `trace` continues to
their sockets, files and children as for any process.

Hard: a container the engine lists whose processes this user may not read their cgroups of
shows no processes rather than an error — the scan skips what it cannot read, as the socket
owner scan does, and the engine's own view remains available to a privileged user.

Encoded by `crates/ono-graph/tests/relationships.rs::should_link_a_container_to_the_processes_in_its_cgroup`
and acceptance case 131.

## Alternatives considered

**Ask the engine's `inspect` for `State.Pid` and walk the process tree from it.** One round trip
per container per trace, and the tree misses processes the engine's exec put in the container
beside the init process; the cgroup has every one of them.

**Match the short id** (the twelve-character prefix) as well. A prefix can appear in another
container's full id; the full id cannot.

**Deliver `container.exec`** — see ADR-0114; the reasons have not changed.
