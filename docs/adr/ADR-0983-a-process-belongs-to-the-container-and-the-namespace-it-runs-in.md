# ADR-0983: A process belongs to the container and the namespace it runs in

- Status: accepted
- Date: 2026-11-10
- Spec refs: v0.4 §2.16, §2.18, §3.2, §10.2, §16.2, §20.1, §21.3, §26.2, §35.3, §42.1, §42.4; ADR-0135, ADR-0968, ADR-0974; issue #211
- Decided by: agent (autonomous)

## Context

§21.3: "Privilege, remote and namespace changes MUST be visually recognizable even in minimal
colorless terminals", with `container:api /process/441` and `ns:net/4026533331 /socket/443` as
examples. §3.2: "Crossing a scope boundary MUST be observable in the navigation trail and
prompt/HUD." §16.2: "Entering a namespace MUST show the boundary explicitly."

The scope model had `Container` and `Namespace` kinds, the trail recorded a crossing whenever two
places' scopes differed, and the landmark engine promoted an object behind a boundary. None of
it could fire: the bridge projected every observation into the session's host scope, so no place
ever carried another (#211). And the process provider left `container` null, although the
control group the engine puts a container's processes in carries its full id — the fact
ADR-0968 and ADR-0974 already read for `trace container`.

## Decision

**The process record names its container.** `linux.procfs` fills `ono.process/1`'s `container`
from `/proc/<pid>/cgroup`: the innermost path component that carries a run of exactly sixty-four
lowercase hex digits — `docker-<id>.scope`, `libpod-<id>.scope`, `/docker/<id>`,
`cri-containerd-<id>.scope`. A process in no such group states none, and none is null.

**An observed process belongs to the scopes its record names**, nested outermost first inside
the host it was observed on:

- `container:<short id>` where the record names a container — the twelve-character form every
  container tool prints;
- `namespace:pid:[<inode>]` where its pid namespace is another than the shell's own. The shell
  learns its own by asking the process provider about itself, once; until it knows, and on a
  linked host, no process is placed in a namespace scope (§2.17).

**The identity does not move.** A process's `SpatialId` is derived in the scope it was observed
in — the host — as before; the scope it lives in is a property beside it. A process is the same
place before and after its container is known, and the index keys a provider object by the
observation scope (§42.1).

**The boundary is shown where §3.2, §16.2 and §21.3 put it:**

- *Prompt and concise path*: `local container:4f9c0a2b7e1d /process/nginx`, and
  `ns:pid/4026532279` for a namespace — words between the host and the place.
- *Trail*: every movement onto or off such a place records the crossing, `enter <target>`
  included, which recorded none.
- *On the movement*: one line on the diagnostic stream, `crossing scope: host:web01 ->
  container:4f9c0a2b7e1d`, where `jump` states a crossed link.
- *Landmark*: the existing `security_boundary` rule fires for an object behind the boundary.

**A process's `container` exit is the engine's to answer.** Where the record names a container
the `container` target is asked, so the exit is the container, or the engine's refusal — never
the empty group a named and unread container would leave (§42.4). A record that names its
container asserts `container.contains_process` once: the bridge's composition from the cgroup
path (ADR-0135, `strong`) applies only to a record that names none.

## Consequences

Easy: standing on a containerised process, the prompt says so; `trail` shows where a walk
entered and left a container; a process list on a container host marks what is behind a
boundary.

Hard: every process of every container is a `security_boundary` landmark when seen from the
host — a `notice`, bounded by the view's landmark budget. Only the pid namespace is compared:
a process that shares the pid namespace and differs in its network or mount namespace carries
no marker, because `ono.process/1` states the pid namespace alone. An unprivileged shell
cannot read the namespace of another user's process, and an unreadable namespace is no
boundary claim. The edge `container.contains_process` from a record's own statement is
`exact`, following ADR-0968's reading of the full id.

Encoded by `crates/ono-provider-linux/tests/process.rs::should_name_the_container_a_process_runs_in_when_its_cgroup_carries_the_engines_id`,
`crates/ono-spatial-core/tests/projection.rs::should_place_a_process_in_the_container_its_record_names_without_changing_which_process_it_is`,
`::should_place_a_process_in_its_pid_namespace_when_that_is_not_the_observers_own`,
`crates/ono-spatial-query/tests/resolution.rs::should_mark_the_container_and_the_namespace_between_the_host_and_the_place`,
`crates/ono-spatial-index/tests/relations.rs::should_show_one_container_when_a_process_record_names_it_and_its_cgroup_carries_the_id`
and acceptance case `144-container-and-namespace-boundaries`, which makes both boundaries as
root and walks across them with the real binary.

## Alternatives considered

**Project container processes into the container's scope, identity included.** The same
process would be two places depending on whether its cgroup was readable when it was listed,
and every edge to it would dangle between them.

**Ask the engine which processes a container holds.** One round trip per container, a socket
most users may not open, and no answer at all for a namespace.

**Mark only the prompt.** §3.2 names the trail as well, and a script has no prompt.
