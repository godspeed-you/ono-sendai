# ADR-0114: `enter`, `watch` and `trace` for containers ride the generic roads

- Status: accepted
- Date: 2026-08-27
- Spec refs: §9.1 (Container), §14.1, §14.3, §18.2, §22.1–§22.3, §31.14; ADR-0023, ADR-0024,
  ADR-0075, ADR-0078, ADR-0112
- Decided by: agent (autonomous)

## Context

Spec §9.1 promises `enter container <id>` ("Push execution/container context"), `trace
container <id>` ("Show namespaces, cgroups, mounts, processes, sockets and image relation") and
the §52 matrix marks `container/watch` as a live stream. ADR-0075 delivers `enter` for every
provider-served target, ADR-0078 makes `watch` and `trace` generic over the registry with a
per-target event envelope and a relationship provider per edge set. The RED suite asserts a
frame of kind `container` with identity `web`, a watch that begins with a `snapshot`, and a
graph joining the container to its image by an `image` edge of `exact` confidence.

## Decision

### 1. `enter container <name>` pushes a frame of kind `container`

The `enter` contract's selector is `name` — the container's name or id, both of which the
engine resolves — so the frame's identity is what the prompt should show (`container:web`) and
not the 64-character id. `get context` reports the frame's `kind` as `container`, the value
`context.v1` already reserves for it; every other entered object stays `object`. Inside the
frame, later queries narrow exactly as ADR-0076 narrows them: a query for `container` narrows to
this container by its `id`, and a target whose schema carries no `container` field is refused
with the reason.

The frame is a *narrowing* context, not an *execution* one. Running later commands inside the
container's namespaces — `nsenter` semantics, `container.exec` — is not delivered: the provider
does not advertise `container.exec`, and the frame says truthfully what it narrows. Spec §14.3
forbids a context that acts on state the user cannot see, and a frame that pretended to
execute inside the container while reading the host's procfs would be exactly that.

### 2. `watch container` is the ADR-0078 poll over the engine listing

`ono.container-event/1` keys the object field `container`; the runtime compares
`GET /containers/json?all=1` listings at the two-second cadence, marks every event
`source: poll`, and begins with `snapshot` events (ADR-0024). The engine's `/events` stream
is the obvious subscription source and is left for the day a provider grows `subscribe`.

### 3. `trace container` contributes the image edge; the rest waits for a kernel view

`ono-graph` gains `ContainerImage`, a relationship provider from `ono.container/1` to
`ono.image/1`: the engine reports the digest of the image a container was created from
(`image_id`), the image provider is asked for that digest, and the edge `image` is `exact`
because it is the runtime's own bookkeeping. A container whose image the engine no longer
holds contributes no edge — absence, not a failed read.

Namespaces, cgroups, mounts, processes and sockets are not related yet. They need the
container's init pid (`State.Pid` from the inspection) joined to `/proc/<pid>/ns`,
`/proc/<pid>/cgroup` and `/proc/<pid>/mountinfo`, which is a kernel view the fake engine of
the suite cannot fake and this increment does not assert. The `note` on the contract and
`docs/STATE.md` → *Next up* record it.

## Consequences

- `enter container web; get context | to json` shows `{kind: container, target: container,
  identity: web}` above the ground frame and `leave` restores it; `watch container | take 1`
  is a `snapshot`; `trace container web | to json` is one graph with the container and image
  nodes joined by `image`/`exact`. Tests:
  `crates/ono-cli/tests/containers_packages_missing.rs::should_push_a_container_frame_when_entering_a_container`,
  `::should_pop_the_container_frame_when_leaving_it`,
  `::should_begin_with_a_snapshot_when_watching_containers`,
  `::should_relate_a_container_to_its_image_when_tracing_it`.
- `ono.container.watch` and `ono.container.trace` are bound by id in
  `crates/ono-command/src/impls/mod.rs`, like every other watch and trace.

## Alternatives considered

- **Keeping `id` as the `enter` selector and showing the id in the prompt.** Rejected: a
  64-character digest is not how anyone names a container, and the engine resolves a name on
  the same endpoint at the same cost.
- **Advertising `container.exec` to make `explain enter container` complete.** Rejected: the
  capability says "execute inside a container's namespaces", and nothing does.
- **Relating the container to processes by cgroup path matching now.** Rejected: it needs the
  host's procfs and a real container to prove it; a relationship written without a test that
  observes it is the guess spec §22.2 forbids.
