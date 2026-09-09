# ADR-0613: Temporal metadata rides as a namespaced extension, and a temporal landmark is a schema

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §9.4, §27, §28.2, §35, §43.4; v0.2 §10.4, §25.1; v0.4 §3.7; ADR-0012, ADR-0022
- Decided by: agent (autonomous)

## Context

Two questions arrived from two packages at once, and both are about where a v0.5 record ends up
in a v0.2 value model.

**§9.4** says a reconstructed object "retains its canonical schema plus temporal metadata" —
`as_of`, `coverage`, `reconstructed`, `sources`, `gaps` — and that "the temporal metadata MUST
NOT collide with provider fields". It writes the field `_temporal`. This tree has no underscore
convention and one field of that name exists nowhere.

**§27** defines temporal landmarks with twelve built-in candidates and requires each to keep its
event reference, and §43.4 puts one of them — a recorder falling behind — at the root `look`.
§35's schema list does not name a schema for them, and §35 opens with "at least".

## Decision

### 1. Temporal metadata is the extension key `ono.temporal`

`RecordValue` already carries provider extensions as dotted, publisher-namespaced keys —
`systemd.load_state`, `ono.graph.failures` (v0.2 §10.4). A reconstructed object's temporal
metadata is one more: the key `ono.temporal`, holding the map
`ono_temporal_core::value::temporal_metadata` builds.

It cannot collide with a provider field, which is what §9.4 asks for, and it leaves the canonical
schema exactly as it is, which is what "retains its canonical schema" says most directly. §28.2's
requirement is served in full: a historical `Process` is an `ono.process/1` with the same fields,
so `get process --at -1h | where cpu > 20 | select pid name` is the same pipeline it was against
the present.

The alternative was tried first and reverted: a nullable `temporal` record field declared on the
twenty-three schemas a checkpoint can hold. It would be followable in a `where` clause, which the
extension is not, and that is the one thing it buys. Against it: every live record of every one
of those schemas would carry a permanently null field for a feature that applies only in
historical mode, and eight tests that pin each schema to the field list v0.2 fixed would have to
be told that the list has grown — weakening, one schema at a time, the guard that keeps the
provider surface honest. A minority case does not get to change the shape of the majority.

The metadata reaches a reader through `inspect`, which shows extensions, and through the
reconstruction API, which returns it as a value. If a later increment needs it followable, the
answer is a declared field on the temporal *view* records rather than on every object schema.

### 2. A temporal landmark is `ono.temporal-landmark/1`

Registered as a schema, because a landmark reaches a reader: the timeline view marks them, the
significant-event stepper walks them, and §43.4 puts one at the root `look`. A value a user sees
is a contract in this tree, and one built as an untyped map inside a crate is a contract nobody
can inspect, complete or filter on.

It is a second schema rather than a widening of v0.4's `ono.landmark/1`, because the two answer
different questions with disjoint vocabularies. v0.4 §3.7's closed list — `high_cpu`, `failed`,
`recently_changed`, `user_pinned` and the rest — says what makes a *place* worth noticing now.
§27.1's list — `service_failure`, `restart_loop`, `mount_change`, `recorder_gap`, `remote_link`
and the rest — says what makes a *moment* worth navigating to. No word appears in both, and
neither list generalises the other.

## Consequences

- No object schema changes, and the eight tests that pin each one to v0.2's field list keep
  meaning what they meant.
- `docs/ACCEPTANCE.md` §4.11.4's box names the extension key, because a box that named a field
  the tree does not have would be a box nothing could close.
- `ono.temporal-landmark/1` is embedded like every other schema and validated by the same two
  tests. A rule that emits a landmark outside the twelve declared kinds fails the contract.
- The `ono.temporal` key is reserved. Nothing else may claim it.

## Spec deviation

- Section: v0.5 §9.4
- Text: "`_temporal { as_of: Timestamp … }`"
- Instead: the extension key `ono.temporal`, carrying the same five members.
- Why: this tree namespaces record extensions with dotted publisher-qualified keys and has no
  underscore-prefixed field anywhere. `_temporal` would be the only one, and the property §9.4
  actually requires — that the metadata cannot collide with a provider field — is what the
  namespaced key guarantees by construction.

## Alternatives considered

- **A declared nullable `temporal` field on every object schema.** Implemented, measured against
  the suite, reverted. See §1 above.
- **A `HistoricalProcess` type.** §28.2 forbids it by name, and it would break every pipeline.
- **Widening `ono.landmark/1` with §27.1's words.** It would put two closed vocabularies in one
  enum and make `reason` a field whose meaning depends on which subsystem produced the row.
