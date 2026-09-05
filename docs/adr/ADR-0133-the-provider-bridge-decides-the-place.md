# ADR-0133: The provider bridge decides which place a record is, from the record

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §2.16, §3.1, §7, §14.3, §14.4, §15.4, §15.5, §18, §37.1, §42.1, §45.2, §50 Phase S2
- Decided by: agent (autonomous, `S2`)

## Context

`ono_spatial_core::Projection::project_as(record, object_type)` takes the type from its caller,
and ADR-0130 says why: a schema does not determine a place. `ono.socket/1` is §14.3's listener or
§14.4's connection; `ono.file/1` is §15.4's directory or §15.5's file; `ono.device/1` is a block
device STORAGE holds (§7.4, §18) or a character device DEVICES holds (§7.7). The S1 handover left
"deciding which" as S2's job, and `Projection::project` — which reads the type out of the
geography — is wrong for exactly those three schemas and silently wrong for `ono.file/1`, whose
only collection space is `storage.directories`.

A second problem sat beside it. §50's gate for this phase is *"provider objects can be reconciled
into one graph without duplicate identity for known-equal objects"*, and §37.1 extends that to
adapter output. Nothing existed that fed records into the index at all.

## Decision

A new module, `ono_spatial_index::bridge`, holds two things.

**`spatial_type_of(record) -> Option<SpatialType>`** is the single table from a shipped schema to
a place, and where a schema carries more than one kind of place the *record* decides:

| schema | rule |
|---|---|
| `ono.socket/1` | `state == listen`, or no peer endpoint → `Listener`; a peer → `Connection` |
| `ono.file/1` | `kind == dir` → `Directory`; anything else → `File` |
| `ono.device/1` | `kind == block` → `BlockDevice`; anything else → `Device` |
| `ono.process/1`, `ono.process-detail/1` | `Process` |
| `ono.block-device/1` | `BlockDevice` |

A bound UDP socket with no peer is a `Listener`, not a `Connection`: §14.3 describes a listener as
the place traffic arrives at, and a socket with no far end has no connection to be one end of.

`None` — no place — is the answer for `ono.package/1`, `ono.env-var/1`, `ono.log-record/1`,
`ono.dns-record/1`, `ono.image/1`, `ono.link/1`, `ono.plugin/1` and every other schema §7 gives no
domain. **A v0.2 provider target that no canonical domain holds is not a spatial object.** Images,
links and plugins are values in the typed shell; making them places would be inventing geography
the specification does not have.

**`ProviderBridge`** holds the scope's `Projection` and a reference table, and `absorb` registers
a batch of records into a `SpatialIndex`. It separates three outcomes that must not be one:
`added`, `reconciled` (§42.1's identity test holding), and — kept apart — `unplaced` (a schema
with no domain, counted once, not an error) and `refused` (a record that names a place and could
not become one, carrying the provider's own diagnostic).

**Reference keys are not identities.** A socket names its owner by pid, a route names its
interface by name, a process names its service by unit name, a file names its owner by uid — none
of which is the object's spatial identity. `reference_field` gives, per type, the one field
another record names it by, and `ProviderBridge::resolve(type, key)` maps it back to the place.
An interface is registered under both its name and its index, because `ono.route/1` carries
whichever the kernel could resolve; a container under its full id and the first twelve characters,
because that is what a cgroup path and a `docker ps` line show. Resolution walks the
specialisation chain of `SpatialType::is_a`, so a reference that names a `Socket` reaches the
`Listener` it actually is.

A reference to something nobody has observed resolves to `None` and produces no edge. §42.3
forbids dangling internal ids, and no edge is a better answer than a broken one.

## Consequences

- The reconciliation gate of §50 Phase S2 is met and tested twice over: one process seen through
  `ono.process/1` and `ono.process-detail/1` is one place, and one disk seen through `linux.sysfs`
  (`ono.device/1`) and the util-linux `lsblk` adapter (`ono.block-device/1`) is one place — the
  §37.1 adapter merge, working because identity is built from the facts that make the object that
  object and never from the schema that carried them.
- `ono.device/1` now feeds two domains. That is what §7.4 and §18 ask for: STORAGE holds
  "volumes/devices where known" and a block device is the thing that backs a filesystem, while
  §7.7 keeps the rest of the kernel's devices in DEVICES.
- `Projection::project` (the type-from-geography path) stays for callers with a single-candidate
  schema, but nothing in the shell should use it for sockets, files or devices. The bridge is the
  entry point.
- The reference table grows with the objects one session observes. It is bounded by the index it
  mirrors and is discarded with it.

## Alternatives considered

- **Putting the decision in `ono-spatial-core`.** Rejected: §45.1 gives that crate the data model
  and §45.2 gives this one "provider record → spatial object", which is what this is. The core
  would also have had to know the shipped schema catalogue.
- **Adding a `place` hint field to the schemas.** Rejected: it would make providers responsible
  for spatial semantics, and `state`, `kind` and the peer endpoint are already the facts the
  decision needs.
- **Registering `ono.image/1`, `ono.link/1` and `ono.plugin/1` as places.** Rejected: §7 places
  none of them, `SpatialType` (ADR-0128) has no name for them, and a place with no domain has no
  `up`.
