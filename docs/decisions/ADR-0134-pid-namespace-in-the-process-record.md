# ADR-0134: `ono.process/1` carries the pid namespace, because §10.2's identity needs four parts

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §2.8, §10.2, §16.2, §42.1, §42.2, §50 Phase S2; v0.2 §10.4, §23.1, §28.1, §35.3
- Decided by: agent (autonomous, `S2`)

## Context

v0.4 §10.2 fixes what a local Linux process identity is:

> host boot identity / pid / process start time / pid namespace identity

`ono_spatial_core::ProcessIdentity` takes all four (ADR-0129), and `Projection::project_as` reads
the last one from a `pid_namespace` field of the record. No provider produced that field, so
every process projected with `pid_namespace: unknown` — which means a container's pid 1 and the
host's pid 1 reduced to the *same* `SpatialId` as soon as both had the same start time, and
`enter` on one would have arrived at the other. §42.1 ("repeated observations of the same live
object MUST resolve to the same `SpatialId`") is only half the contract; the other half is that
two objects never share one, and without the namespace this identity could not keep it.

The S1 handover named this explicitly: *"Where a provider does not supply what the identity needs,
extend that provider to read it rather than degrade the identity silently."*

## Decision

`ono.process/1` and `ono.process-detail/1` gain a **nullable `int` field `pid_namespace`**: the
inode of `/proc/<pid>/ns/pid`, which the kernel spells `pid:[4026531836]`. `linux.procfs` reads
the link in the same pass that reads `stat`, `status` and `cgroup`, and names it in the record's
provenance.

Three rules govern the value:

- **A namespace nobody could read is `null`, never the root namespace.** Guessing `4026531836`
  would make every hidden process look like a host process, which is exactly the fabrication
  spec v0.2 §35.3 forbids. `hidepid`, a kernel without namespaces and a process that exits
  between enumeration and read all produce `null`.
- **A refusal is an error value in the field**, not `null`, when the kernel answers `EACCES` —
  so the permission propagation of §35.2 has something to carry (ADR-0136).
- **It is not part of the schema's `identity`.** `ono.process/1` stays identified by
  `(pid, started)`, which is the v0.2 contract ADR-0015 T13 fixed and what a signal resolves
  through. The namespace is part of the *spatial* identity, which is a different and stronger
  thing: §10.2's four parts, composed by `ono-spatial-core`, not by the provider.

The field is nullable and appended, so it is an additive change and `ono.process/1` stays at
version 1 (spec v0.2 §10.4: a *breaking* change takes the next version). Every existing consumer
reads the same fields it read before.

## Consequences

- A process observed as `ono.process/1` and the same process observed as `ono.process-detail/1`
  now agree on all four identity parts and therefore reduce to one `SpatialId` — the
  reconciliation §50's S2 gate asks for, proven by
  `should_carry_the_pid_namespace_into_the_detail_record_as_well` and by the bridge conformance
  suite.
- `get process` costs one extra `readlink(2)` per process. The `linux.procfs` cost class stays
  `cheap`: it is a syscall on a path the provider already has open, in the same loop as the four
  reads it already does.
- Two assertions that pinned the process contract field-for-field changed in this commit, both by
  appending the new field:
  `ono-value/tests/builtin_schemas.rs::should_define_the_process_schema_exactly_as_the_spec_does`
  and
  `ono-provider-linux/tests/schemas.rs::should_declare_the_process_contract_exactly_as_the_registry_fixes_it`.
  They assert a contract, and the contract changed here, deliberately and for the reason above.
- §16.2's namespace boundary now has a fact to stand on: a process record says which pid
  namespace it was read in, so crossing one is observable rather than assumed.

## Alternatives considered

- **Reading the namespace only in `ono.process-detail/1`.** Rejected: `get process` is the
  listing the spatial index is fed from, and an identity that is only correct after `inspect` is
  not an identity.
- **Composing the namespace in the spatial layer from `/proc` directly.** Rejected by §2.16:
  "Providers own facts. Ono's spatial layer composes provider data; it MUST NOT become an
  undocumented source of system truth."
- **Adding the namespace to the schema's `identity` list.** Rejected: it would change what
  `kill process` resolves through and what a `diff` considers the same row — a breaking change to
  a v0.2 contract, to solve a v0.4 problem that `ono-spatial-core` already solves above the
  provider.
- **Storing the raw link text `pid:[4026531836]`.** Rejected: `ono.namespace/1` identifies a
  namespace by its inode as an `int`, and two spellings of one identity is how joins stop
  working.
