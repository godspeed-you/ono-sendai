# ADR-0419: A cell reads the declaration of the field it came from

- Status: accepted
- Date: 2026-08-31
- Spec refs: §10.5, §13.1, §13.2, §13.4, §23.6, §33.5
- Decided by: agent (autonomous)

## Context

Three cells rendered wrongly at `72aea1e`, all found by `readme-demo` while recording the figures
in `docs/assets/` and recorded on the board under *Found while recording the README figures*:

```text
PID  NAME     CPU                MEMORY     USER
  1  systemd  2.085903083563699  11.73 MiB  ono.user/1 {uid: 0}

PROTOCOL  LOCAL              STATE
tcp       ono.endpoint/1 {}  listen
```

`| to json` answers correctly in every one of them, so the values are right and the renderer is
what is wrong. It cost the README a documented example: the file claimed `select protocol local
state` printed `:5432`, which no build does.

One cause underneath: `Renderer::text` matched on the *value* alone, and two of the things spec
§13 prints are not in the value.

- **The unit is on the field.** `docs/contracts/schemas/process.v1.yaml` declares `cpu` as `float`
  with `unit: percent`. The value is a bare `f64`; nothing in it says percent, so it fell to
  `canonical_text` and printed all seventeen digits an `f64` happens to carry. Spec §13.2's own
  table prints `24.8%` for that field.
- **A reference is not a copy.** `user` is declared `ref<ono.user/1>`. The provider fills it with
  the account's record, and `Display for Value` renders a record as `<schema id> <identity>` —
  a diagnostic form that belongs in error metadata. Spec §13.2 prints `postgres` there.
- **A nested record had no arm at all.** `local` is declared `ono.endpoint/1`, `canonical_text`
  refuses a record by design (§12.3: a record of several fields has no single line), and the
  fallback was the same diagnostic `Display`. `ono.endpoint/1 {}` and an endpoint that really is
  empty read identically — the conflation §10.5 exists to prevent.

## Decision

**A cell rendered through a field is rendered through that field's declaration.**
`Renderer::field_cell` looks the field up in the record's schema and applies two rules before
falling back to the value-only rendering; every other path is unchanged.

1. **`unit: percent` on a numeric value renders as a percentage, to one decimal.** `2.0%`.
   Only `percent`. `bytes` and `seconds` have a value type of their own — `rx_bytes` is a
   `bytesize` and already renders `1.20 GiB` without help — and where a schema puts `unit: bytes`
   over a plain integer it is because the number is the point: `interface.v1.yaml`'s `mtu` reads
   as `1500`, never as `1.46 KiB`. `percent` is the one unit with no other spelling.
2. **`ref<S>` holding a record renders as what a person calls that object**: its `name` field
   when it has one and it resolved, and its identity otherwise — `{uid: 0}`, because §23.6 keeps
   the numeric identity of an unresolved id and a blank cell would throw it away.

And, independent of any declaration:

3. **A record-valued cell renders the record**: its default-view fields, or all of them when the
   schema declares none, spelled the way a record literal is — `{address: 127.0.0.1, port: 631}`.
   A null field stays the word `null` (§10.5). Which fields is the same question the table
   already answers for its columns, so it is the same answer, and `columns_of` is now the one
   place that answers it.
4. **`Value::Percent` rounds to one decimal too**, so a percentage reads the same however it
   reached the cell.

**The serialisation is untouched.** `canonical_text`, `to json`, `to yaml`, `View::Raw` and
`inspect` keep every digit and the whole nested record, because §33.5 wants canonical values
unless a human rendering was asked for, and this is only the human one.

## Consequences

- `get process` and `watch process` — the view a reader watches rather than reads — show
  `2.1%` and `root` where they showed a seventeen-digit float and `ono.user/1 {uid: 0}`.
- The README example `select protocol local state` shows the endpoint again and can be restored.
- Rounding is a rendering decision and a lossy one; the exact number stays one `to json` away.
  A percentage below 0.05 renders `0.0%` rather than as a smaller non-zero number. That is the
  same trade §13.4 already makes for `1.20 GiB` and `4d 03h`.
- A schema that adds `unit: percent` to a field changes how that field looks, without any
  renderer change. That is the intent: §13.1 point 1 makes the schema part of what a renderer
  reads.
- **Not fixed here, and now recorded on the board:** `select` erases the declaration.
  `ono-pipeline`'s `selection_schema` types every projected field `any` and carries no unit, so
  `get process | select cpu` still prints the raw float while `get process` prints `2.1%`. The
  schema is built once in `Select::new`, before any record is seen, so carrying the source
  declaration through is a change to the transform rather than to the renderer — a separate
  increment under §4.

## Alternatives considered

- **Render a reference as its whole record** (`{uid: 0, name: root}`) — uniform with rule 3 and
  one rule shorter, rejected because §13.2 prints `postgres` and because the `USER` column of a
  process table is read by eye down a list of rows.
- **Give the renderer a per-schema table of display names**, as `ono-spatial-core`'s
  `display_name` has for places — rejected: that table exists there because a place is named for
  a human before anything else, and a second copy of it in the renderer would drift.
- **Round every float in a table.** Rejected: nothing says an arbitrary float is a measurement,
  and §10.5's discipline is that a renderer never asserts more about a value than the value says.
- **Render the percentage in the provider.** Rejected: spec §13.1 forbids it — a provider
  produces values and never formats them, and `cpu` would stop being a number a `where` clause
  can compare.
