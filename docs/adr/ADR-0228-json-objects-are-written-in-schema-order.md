# ADR-0228: JSON objects are written in schema order

- Status: accepted
- Date: 2026-08-29
- Spec refs: §12.3, §33.5; ADR-0016, ADR-0030
- Decided by: agent (autonomous, `close-data`)

## Context

Spec §33.5 prints a serialised process as

```json
[{"pid": 812, "name": "postgres", "cpu": 18.1, "memory": 1288490188, "user": {…}}]
```

— the schema's own field order, which is the order the table shows and the order a person reads.
`to json` wrote `{"command": …, "cpu": …, "memory": …, "name": …, "pid": …}` instead, because
`serde_json::Map` is a `BTreeMap` and sorts its keys. ADR-0030 recorded that as "known and
accepted … deferred to its own increment on `docs/STATE.md`". This is that increment.

Alphabetical order is not wrong so much as *unrelated*: `select pid name cpu` asks for three
columns in an order and gets them back in another, and a record whose first field is its identity
buries it in the middle.

## Decision

**`serde_json`'s `preserve_order` feature is enabled for the workspace**, so a JSON object is
written in the order its keys were inserted — which for a record is the schema's declared field
order, extensions after the declared fields, and for a `select` projection the order the fields
were written.

It applies to both encodings, deliberately. The interop encoding of ADR-0030 gains §33.5's order;
the lossless tagged codec and the KUANG/11 protocol gain their own insertion order, which is
equally deterministic and which no reader depends on — every decoder reads by key. YAML and CSV
were already written in field order.

Three KUANG/11 errors box what made them wide: `preserve_order` makes `serde_json::Map` an
`IndexMap`, which is wider than a `BTreeMap`, and all three travel in a `Result` on the
protocol's hot paths. `KuangError` and `WireError` box their `metadata` map — it is empty on
almost every error and does not belong inline in every `Err` — and `EmitError::Refused` boxes
its `WireError` payload, because a refusal is rare. The wire form is unchanged: a `Box`
serialises as what it holds, so no message on the protocol looks different.

## Consequences

- `get process | select pid name cpu | to json` answers `{"pid":…,"name":…,"cpu":…}`.
- Output stays deterministic — insertion order is fixed by the schema, not by a hash — so the
  §4.6 guarantee that redirected output does not depend on who is watching is unaffected.
- Assertions that pinned the alphabetical order changed to the order this decision gives, with
  the same exactness: four unit assertions
  (`language_missing.rs` ×2, `builtins.rs`, `remote_missing.rs`) and thirty-eight lines across
  ten acceptance cases. Two acceptance assertions were made order-independent instead, because
  their subject was never the order: case 047 greps the serialised graph for `"edges"` rather
  than reading the first 200 bytes of it, and case 042 compares two spellings of a target rather
  than pinning the host's mount device. The whole suite — 88 cases — passes.
- A future field added to a schema appears where the schema puts it, not where the alphabet
  does. That is a visible change for a reader and none at all for a parser.

## Alternatives considered

- **Hand-write the interop JSON encoder.** Rejected: it would duplicate string escaping and
  pretty-printing, which is the part of a serialiser most worth not writing twice, and it would
  leave the tagged codec sorting its keys differently from the one beside it.
- **Leave it alphabetical.** Rejected: it is the specification's own example that disagrees, and
  every `select` in the language chooses an order that was being discarded.
