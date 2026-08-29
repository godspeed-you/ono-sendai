# ADR-0168: A host's canonical geography is its own, and the process stands in one of them

- Status: accepted
- Date: 2026-08-28
- Spec refs: §4, §7.1, §19.2, §3.2, §43.7, §42.1, §29.2, §46
- Decided by: agent (autonomous, phase S8)

## Context

§19.2 says a `jump` across a link "MUST produce a new `SystemPlace` for the remote host", and
§43.7 lists "no accidental local/remote identity merge" among the cases the remote work must
answer for. Until S8 the canonical geography of §4 was a single table of twenty-odd spaces whose
`SpatialId`s were derived from the space id alone (`SpatialIdentity::space("compute")`), so every
host in the world shared one `COMPUTE` and one `SYSTEM`. Two hosts' roots would have been the
same place, `home` could not have meant "the root for the current host" (§6.6), and a trail
crossing a link would have recorded two `enter compute` steps with the same destination.

Roughly twenty call sites ask `space_of(id)`, `space.spatial_id()` or `space.label` without
having a scope to hand — `up`, `back`'s liveness check, the breadcrumb, the place path, the
selector resolver's visible children, the map's node labels. Threading a scope through all of
them would have been a wide, mechanical change to code four other phases are also touching.

## Decision

The geography is **per host**, and the *process* stands in one of them at a time.

- `SpatialIdentity::space_in(space_id, scope)` composes a canonical space's identity. A local
  scope (or `None`) adds nothing, so every id built before this ADR is unchanged (§42.1); a
  `RemoteHostScope` adds a `host` component, so `testbox`'s `COMPUTE` and this machine's are two
  ids.
- `ono_spatial_core::space` owns the geographies this process knows: `stand_in(scope)` moves into
  one, `learn(scope)` registers one without moving into it, `standing_in()` says which one is
  current, and `space_of_id(id)` answers which space an id names *and whose*.
- `CanonicalSpace::spatial_id()` therefore answers for the host the process is standing in. Every
  existing call site becomes correct without being touched: `home` reaches the current host's
  root, `enter compute` resolves the current host's `COMPUTE`, `up` climbs inside one host.
- A host's root place is labelled by the host (§7.1: the root place *is* the system of that
  host); everything below it keeps its plain label, because the host is already the first segment
  of the place path (§27.2's `testbox/compute/processes`).

The state is process-global. That is the same scope the spatial session already has: §29.2 makes
the current place script-local, and `ono-cli` implements that by keeping `SpatialSessionState` per
process. A session is a process, so "which geographies this session has entered" is process state.

## Consequences

- Local ids, place paths and labels are bit-for-bit what they were; no existing test changed.
- `ono-spatial-core` gains mutable process state, which it did not have. It is additive only
  (a geography is learned, never forgotten) and it is not observation: nothing here reads a
  provider, a file or a clock.
- A host this session has never linked to has no geography here, so an id of one is honestly not
  a place this session can describe — which is what §2.17 asks for.
- Encoded by `spatial_remote_missing::should_give_a_linked_host_a_root_place_distinct_from_the_local_root`,
  `…should_record_the_host_and_the_scope_crossing_of_every_step_in_the_trail` and
  `…should_return_home_to_the_local_root_from_a_remote_place`.

## Alternatives considered

- **Thread a `SpatialScope` through every geography call.** Correct, and a large simultaneous edit
  to files S6 and S7 are working in. Rejected for that reason, and because most call sites have no
  honest scope to pass: `back`'s liveness check asks about a place on the trail, which may belong
  to a host the session is no longer standing on.
- **Keep one geography and put the host on the object instead.** Then two hosts' `COMPUTE` are one
  place with two sets of contents, which is exactly the merge §43.7 forbids.
- **Give the remote root an observed identity from the far side's hostname.** Rejected: see
  ADR-0169.
