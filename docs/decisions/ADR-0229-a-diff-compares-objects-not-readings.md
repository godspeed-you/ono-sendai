# ADR-0229: A diff compares objects, not readings

- Status: accepted
- Date: 2026-08-29
- Spec refs: §10.7, §26, §53
- Decided by: agent (autonomous, `close-data`)

## Context

```text
$ ono -c 'get user root | diff (get user root) | to json'
[{"change":"changed","key":0,"left":{…},"right":{…}}]
```

The two sides serialise to the same bytes. Nothing about root changed; the two readings did. A
`RecordValue` derives `PartialEq` over schema, fields, extensions **and provenance**, and
provenance carries the instant the observation was made (spec §26). Two snapshots taken a
millisecond apart are therefore never equal, so `diff` reported every object in both snapshots
as changed and its own `unchanged` case was unreachable for provider records.

The existing test that pinned the unchanged case compared a *variable* against itself — one
reading, one provenance — so it passed while the behaviour a user meets did not.

## Decision

**`diff` compares the data.** `RecordValue::same_data` compares schema, declared fields and
provider extensions and ignores provenance; `Value::same_data` is the same for every other value
and descends through lists and maps, so a record nested inside one is compared by its data too.
`Diff` uses it to decide whether an object present in both snapshots changed.

Ordinary `==` is unchanged and still compares provenance. It answers a different question — "is
this the same observation?" — which `inspect` (spec §10.7) and the event layer both need.

## Consequences

- `get user root | diff (get user root)` answers `[]`. So does any diff of two readings of an
  unchanged object, which is what makes `diff` usable against live providers at all.
- A field that moved is still `changed`, an object that appeared is still `added`, and one that
  went is still `removed`.
- A change in *provenance alone* — the same object read from a different provider or over a
  different link — is not a change. That is the right answer for a diff of snapshots; a question
  about where an answer came from is `inspect`'s.

## Alternatives considered

- **Exclude provenance from `RecordValue`'s `PartialEq`.** Rejected: equality would stop being
  able to distinguish two observations, which the event and remote layers rely on, and the change
  would be invisible at every call site rather than stated at the one that means it.
- **Exclude volatile fields by name** — the board's older wording. Rejected on the evidence:
  nothing volatile was visible in the output, because nothing volatile was the cause.
