# ADR-0135: The relation graph is composed from provider facts, and says so when it is a derivation

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §2.5, §2.6, §2.16, §3.4, §3.5, §11.2, §11.3, §11.4, §11.5, §12–§18, §32, §42.3, §50 Phase S2
- Decided by: agent (autonomous, `S2`)

## Context

§50's Phase S2 asks for "core exact relations", and §11.2, §12–§18 and §3.5 enumerate them:
parent-of, owns (process→socket), opened (process→file), controls (service→process),
connected-to, backs (mount→directory), contains (container→process), owns (user→process),
member-of (cgroup), depends-on (service→service), linked-to (host→host). `relations.yaml` (S1)
declares the vocabulary; nothing produced an edge.

Three things had to be settled: **where the evidence comes from**, **what to do with an edge whose
far end is not a place yet**, and **what to do with the relations for which no provider has
evidence at all**.

## Decision

### Every edge is a record's own statement

`ono_spatial_index::bridge::facts_of` reads one record and returns what that record *said* about
its object's relations. Nothing is looked up, probed or inferred from the outside; §2.16 forbids
the spatial layer from becoming a source of truth, so the graph is composition and nothing else.

| relation | the fact it is composed from | confidence |
|---|---|---|
| `process.parent_of` | `ono.process/1.ppid` | exact |
| `process.owns_socket` | `ono.socket/1.process` | exact |
| `process.opened_file` | `ono.process-detail/1.open_files` | exact |
| `process.member_of_cgroup` | `ono.process-detail/1.cgroup` | exact |
| `process.in_namespace` | `ono.process/1.pid_namespace` (ADR-0134) | exact |
| `service.controls_process` | `ono.process/1.service` | exact |
| `user.owns_process` | `ono.process/1.user` | exact |
| `user.owns_file` | `ono.file/1.owner` | exact |
| `user.member_of_group` | `ono.user/1.primary_group` | exact |
| `container.contains_process` | `ono.process/1.container` | exact |
| `container.contains_process` | the runtime id inside `ono.process-detail/1.cgroup` | **strong**, with the path as evidence |
| `socket.connected_to` | `ono.socket/1.remote` | exact |
| `filesystem.mounted_at` | `ono.filesystem/1.target` | exact |
| `mount.backs_directory` | `ono.mount/1.target` | exact |
| `device.backs_filesystem` | `ono.filesystem/1.source` equalling the device node | **strong**, with the source as evidence |
| `interface.has_address` | `ono.interface-address/1.interface` | exact |
| `route.via_interface` | `ono.route/1.interface` | exact |

**`container.contains_process` is relaxed to `exact_or_provider_declared`** in `relations.yaml`
and `ono_spatial_core::relation`. The kernel does not report container membership. A runtime that
lists its own processes observes it; a container id read out of `/proc/<pid>/cgroup` — Docker's
`docker-<id>.scope`, Podman's `libpod-<id>.scope`, containerd's `cri-containerd-<id>.scope` — is
evidence that leaves no serious alternative, which §11.5 calls `strong`, and §22.2 forbids
presenting it as an observation. The join is to the engine's own 64-character id, never to a name
a user can change. `device.backs_filesystem` is `strong` for the same reason: two strings the
kernel spells the same way is a join, not a link the kernel drew.

### Places a provider names but does not serve

§42.3 allows an edge to reach "an explicit unresolved endpoint object" as well as a known object.
Three places exist only as a field of another object's record, and the bridge composes them
through the new `Projection::derive`, carrying the naming record's provenance unchanged:

- **`Endpoint`** — the far end of a connection (§14.4), named `address:port` or by socket path;
- **`Cgroup`** — the control group a process is accounted to (§12, §16.3). It gets a real,
  declared contract, `docs/contracts/schemas/cgroup.v1.yaml`, and `compute.cgroups` in
  `spaces.yaml` now names it. No provider answers `get cgroup`; the path is a fact
  `/proc/<pid>/cgroup` carries, and §16.3 asks for the hierarchy to be navigable, not for a new
  provider target;
- **`Namespace`** — the pid namespace a process runs in (§16.2), through the declared
  `ono.namespace/1`.

A derived place is built from the same identity components a served record would produce — the
scope chain plus the schema's identity field and its value — so a cgroup composed from a process
record and an `ono.cgroup/1` record a future provider emits reduce to the **same** `SpatialId`.
Deriving a place is therefore never a fork in identity.

`ObjectRef::derived` is the new constructor in `ono-provider-api` that makes such a reference
expressible. It names a declared schema, so a reader who inspects a derived place can look up what
it is.

### Discovery is not ordered

A socket can be listed before the process that owns it. An assertion whose far end nobody has
observed is kept in the bridge and settled the moment the far end arrives; §42.3 forbids an edge
to an unknown id, and losing the assertion would make edge existence depend on listing order. An
assertion whose *subject* has left the index is dropped with it.

### The Unix path tree is hierarchy, not a relation

§3.4 lists "Directory -> child Directory" among the hierarchical edges and §15.1 requires Ono to
preserve Unix path semantics, so no relation declares directory containment and no
`RelationshipEdge` carries it. `ono_spatial_core::hierarchy::PATH_PARENT` is a reserved id that
appears in a type's canonical-parent rule chain, and `canonical_parent_with` resolves it from the
enclosing directory the caller has observed. The chains become:

- `Directory`: `mount.backs_directory`, then `path.parent`, then `storage.directories` — so `up`
  from the directory a mount provides crosses the mount boundary, which §15.3 requires to be
  discoverable, and `up` from any other directory walks the path;
- `File`: `path.parent`, and nothing else. A file has no collection space, so a file whose
  directory nobody has observed has no parent — which `up` reports as `spatial.no_parent` rather
  than inventing one.

`SpatialIndex::set_path_parent` records it; `canonical_parent` (three arguments) is unchanged, so
nothing S1 wrote had to move.

### Two declared relations produce no edges, and that is stated rather than faked

- **`service.depends_on`** — `ono-provider-systemd` reads `ListUnits`, which carries no
  dependency information; `Requires`/`Wants`/`After` need a `Get` per unit over D-Bus. That is a
  provider feature with its own cost class, its own schema surface and its own acceptance case,
  and it is *not* delivered here. It is entered in `docs/STATE.md` under *Deferred*.
- **`socket.accepts_connection`** — neither `sock_diag` nor procfs relates an accepted connection
  to the listener it came from. Matching by local port would be a guess, and §11.5 has no value
  for a guess that a map may then draw.
- **`process.connects_to`** is not composed either: `process.owns_socket` to a `Connection` and
  that connection's `socket.connected_to` are the same two hops, observed rather than summarised.

Neither is claimed by any provider in `docs/contracts/providers/`, which `spec-check` now checks, so
the gap is visible in the contracts rather than only in this ADR.

## Consequences

- A process place has the exits §12 lists — parent, children, service, user, cgroup, namespaces,
  files, sockets, container — as soon as the records that name them have been read.
- Storage composes end to end: block device → filesystem → mount → directory (§15.2), and `up`
  from a file walks its own path.
- `compute.cgroups` stops being a declared-but-unserved space, and `ono.cgroup/1` joins the
  shipped schema registry.
- Every edge carries relation, source, target, direction, confidence, provenance, observation time
  and — where it is a derivation — the evidence, which is what `inspect relation` needs (§11.4).
- `container.contains_process` and `device.backs_filesystem` will be drawn as inferred rather than
  observed on a map (§11.5). That is correct and deliberate.

## Alternatives considered

- **Deriving `container.contains_process` at `exact` and leaving the registry alone.** Rejected:
  it would make the registry's promise untrue, and §22.2 forbids presenting a derivation as an
  observation.
- **Teaching `linux.procfs` to fill `ono.process/1.container` from the cgroup path.** Rejected: a
  `ref<>` field cannot carry a confidence or its evidence, so the inference would become invisible
  the moment it left the provider.
- **A `directory.contains_file` relation.** Rejected by §3.4: the path tree is hierarchy, and a
  relation for it would let `up` and the graph disagree about what containment means.
- **Dropping an assertion whose far end is unknown.** Rejected: it makes the graph depend on the
  order providers were listed in, which is not a fact about the system.
