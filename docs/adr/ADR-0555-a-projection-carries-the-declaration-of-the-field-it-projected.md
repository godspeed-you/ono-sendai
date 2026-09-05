# ADR-0555: A projection carries the declaration of the field it projected

- Status: accepted
- Date: 2026-09-03
- Spec refs: §13.1, §13.2, §33.5, §35.3, §53; ADR-0419
- Decided by: agent (autonomous)

## Context

ADR-0419 made a cell read the declaration of the field it came from, because two of the things
spec §13 prints are not in the value: the unit is on the field, and a reference is not a copy. So
`get process 1` prints `CPU 2.1%` and `USER root`.

`get process 1 | select cpu` printed `1.9358021776832566`, and `select user` printed
`ono.user/1 {uid: 0}`. ADR-0419 recorded the cause and left it: `selection_schema` types every
projected field `FieldType::Any` with no unit, and the schema is built in `Select::new`, before
any record is seen, so there is no source declaration to copy at the point the schema is built.

Nothing else in the tree carries a unit the value does not — `unit: percent` on `process.cpu` and
`process-detail.cpu`, and `unit: bytes` on `interface.{mtu,rx_bytes,tx_bytes}` where the value is
already a `bytesize` or is meant to read as a number — so this was one field's worth of visible
damage over a general hole: every declaration a renderer reads was erased by a projection,
including the `ref<S>` rule of ADR-0419 §2 and the nested-record rule of §3.

## Decision

**A projection of a field carries that field's declaration.** `Select` derives its projection
schema from the schema of the record it is projecting, and remembers it per source `SchemaId`.

- A projected field that is **one top-level field of the source schema** copies that field's
  type, unit and documentation.
- A **nested path**, a **computed expression**, or a source value that is **not a record** keeps
  `any` with no unit: the value came from somewhere the source schema does not describe, and
  §35.3 forbids asserting a type the value may not have.
- **Nullability is not copied.** Every projected field stays nullable: a projection reads a value
  that may be absent, and a failed read is projected as the error it is (§10.5).
- The derived schema is built on first sight of a source schema and cached, because a stream is
  almost always homogeneous and building a schema per row would be a schema build per row. A
  heterogeneous stream gets one entry per schema it carries.

The projection schema keeps the anonymous id `ono.selection/1` it has always had, for the reason
the module header gives: a projection is the shape the user just asked for, not a registered
object type, and the schema travels with the record.

## Consequences

- `get process | select cpu` prints `2.1%`, `select user` prints `root`, and `select protocol
  local state` prints the endpoint — the README example ADR-0419 restored now survives a
  projection too.
- `to json`, `to yaml`, `inspect` and `canonical_text` are untouched: §33.5 keeps the
  serialisation and the human rendering apart, and this changes only what the schema declares,
  never what the value is.
- `inspect` over a projection now shows the projected field's real type rather than `any`, which
  is what a reader asking "what is this column" wants to know.
- A `where` or a `sort` after a `select` sees the same values it saw before. Nothing reads the
  projection schema to decide semantics; only rendering and `inspect` read it.
- One `Mutex` per `select` stage, taken once per row. It is uncontended — a stage is driven by
  one task — and the alternative, deriving per row, costs a schema build instead.

## Alternatives considered

- **Copy the source `FieldDef` wholesale, nullability included.** Rejected: a projection may
  produce null where the source field could not, so a `required` field copied into a projection
  would make every `validate` of a projected record a lie.
- **Give the renderer the source record.** Rejected: the projection is what travels down the
  pipeline; a renderer that needed the record a value came from would need the whole pipeline
  history, and `select` is not the only transform that builds records.
- **Resolve the declaration for nested paths too**, by following `ref<S>` into the registry.
  Rejected as speculative generality (AGENTS.md §4): no field in the tree that a nested path
  reaches carries a unit, and following a reference at schema-derivation time would make the
  shape of a projection depend on what a registry holds at that moment.
