# ADR-0130: The canonical parent is a fixed, ordered rule per spatial type

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §2.6, §3.4, §6.6, §11.1, §11.3, §12, §13, §43.2, §53
- Decided by: agent (autonomous)

## Context

§11.3 says a spatial object "MAY have one canonical parent for `up` while participating in many
relationships", that "the canonical parent MUST be deterministic for a given view profile", and
that choosing one "does not claim that other relationships are less real". It does not say how the
one is chosen.

§53 fixes what `up` is for — "`back` follows history; `up` follows canonical hierarchy" — and
§43.2 makes the consequence a property to test: "up never traverses arbitrary graph edges."

A process is the hard case. It belongs to a service, a container, a user, a cgroup and several
namespaces at once, and it has a parent process. All six are real relationships; exactly one of
them has to be where `up` goes.

## Decision

1. **Each spatial type has a fixed, ordered list of parent rules**, in
   `ono_spatial_core::hierarchy::parent_rules`. The first rule with a neighbour wins; the order is
   part of the code, not of the data, so the result cannot depend on the order edges were
   discovered in. That is §11.3's determinism, made structural rather than promised.

   | Type | Ordered rules | Then |
   |---|---|---|
   | Process | `service.controls_process`, `container.contains_process` | `compute.processes` |
   | Socket, Listener | `process.owns_socket` | `network.listeners` |
   | Connection | `socket.accepts_connection` | `network.connections` |
   | Address | `interface.has_address` | `network.addresses` |
   | Mount | `filesystem.mounted_at` | `storage.mounts` |
   | Filesystem | `device.backs_filesystem` | `storage.filesystems` |
   | Directory, File | `mount.backs_directory` | `storage.directories` |
   | everything else | — | its collection space |

2. **A process's canonical parent is its service before its container.** §11.1's own path is
   `SYSTEM -> COMPUTE -> SERVICES -> nginx.service`, and §13 makes the service "the stable
   service-manager concept rather than a single process lifetime" — the place that survives the
   restart §53 describes. A container is where a process *runs*; a service is what it *is*.

3. **`process.parent_of` is not a parent rule.** The process tree is a relationship (`follow
   parent`), not the spatial hierarchy: `up` from a worker should reach its service, not `systemd`
   by way of six intermediate shells. §2.6 keeps the two apart, and this is where that separation
   is worth the most.

4. **Every object falls back to the collection space of its type.** `up` from a process with no
   service and no container arrives at `compute.processes`. §11.3 allows an object to have no
   operational parent; it does not follow that `up` should refuse, and a refusal here would leave
   a user stranded in a place they can only leave by `home`.

5. **The root is the only place with no parent.** `up` there is `spatial.no_parent` (§40), which
   is a refusal a script can branch on rather than a silent no-op — §2.2 keeps location explicit.

## Consequences

- `crates/ono-spatial-core/tests/properties.rs::should_never_let_a_graph_edge_change_where_up_arrives`
  generates every relation that is *not* on a type's list and asserts `up` does not move. §43.2's
  property is therefore checked against the whole relation registry rather than against an
  example, and stays checked as relations are added.
- The index recomputes an object's canonical parent whenever an edge is recorded, so a process
  discovered before its service starts under `compute.processes` and moves under the service the
  moment the edge is known. `up` is never wrong, only ever less specific.
- Adding a relation does not change `up` anywhere unless it is deliberately added to this table.
  That is the intended friction: it makes "where does `up` go" a decision rather than an emergent
  property of discovery order.
- A "view profile" that reordered these rules is possible later (§11.3 anticipates one); nothing
  in the model assumes there is only ever one, because the rules are a function of the type rather
  than a constant of the object.

## Alternatives considered

- **Container before service.** Rejected: a containerised service would then have its processes
  under the container and its own place under COMPUTE, so `up` twice from a worker would not reach
  the thing that owns it.
- **The parent process.** Rejected under §2.6 as above, and because the process tree's root is
  `systemd`, which would make `up` from anything a walk through init.
- **Letting the provider declare the canonical parent per object.** Rejected: §11.3 requires
  determinism for a given view profile, and per-object declarations from several providers would
  make the answer depend on which one answered first — the exact failure the ordered list removes.
