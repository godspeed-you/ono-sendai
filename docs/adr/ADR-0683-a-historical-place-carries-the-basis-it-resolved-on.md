# ADR-0683: A historical place carries the basis it resolved on

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §14.4, §14.3, §9.4; v0.2 §10.4, §31.5
- Decided by: agent (autonomous)

## Context

§14.4: "`find place` in historical context searches the historical index for the active time. If
the implementation uses present-day aliases to help resolve a historical object, it MUST
distinguish **resolution aid** from **historical existence evidence**."

`ono_temporal_query::search::find_place_at` already draws the distinction and ADR-0665 records why.
What was missing is the route from that distinction to the reader: `ono.spatial-place/1` is the
v0.4 contract every place answer uses, and it declares no field for it. Adding one would change a
shipped schema for a case only the historical spelling has.

## Decision

**A historical `find place` result is an ordinary `ono.spatial-place/1`, with the basis attached
under the reserved extension key `ono.temporal-resolution`.**

The map carries `as_of`, `basis` (`historical_evidence` or `resolution_aid`),
`is_evidence_of_existence`, `existed_at` and `anchor`. A match that is only an aid carries
`existed_at: null` and `anchor: null`, because there is nothing to point at.

This is the route §9.4's own metadata takes when a schema declares no `temporal` field: v0.2 §10.4
makes an extension key namespaced and §31.5 reserves `ono.*` to this project, so neither key can
collide with a field a provider owns. The place record itself is built from the **historical**
index, so a resolution aid produces a thin record — the place is not one the instant knows — and
the extension says why it is in the answer at all.

The same reasoning gives a historical `look` its §9.4 metadata: `ono.place-view/1` declares no
`temporal` field, so `ono_temporal_reconstruct::attach_temporal` writes it under `ono.temporal`.
That attachment is the reconstruction crate's one implementation, called here rather than repeated.

## Consequences

- A pipeline filters on the distinction: a caller who wants only what existed then reads
  `is_evidence_of_existence`, and §9.7's "place not known at requested time" is the answer when
  nothing but aids came back.
- No shipped schema changes, so v0.4 readers of `ono.spatial-place/1` are untouched.
- `anchor` is the event reference `at event` and `why event` take, so a reader can go from a name
  to the evidence in one step (§27.3).

## Alternatives considered

- **A `basis` field on `ono.spatial-place/1`.** Rejected: it would be null for every present-day
  answer, which is a v0.4 contract change paid for by a v0.5 case.
- **A separate `ono.historical-place/1` schema.** Rejected: §28.2 wants a historical object to stay
  usable by a pipeline written against the present, and a new schema id breaks exactly that.
- **Leaving aids out of the answer entirely.** Rejected: §14.4 permits them, and §20.1's discovery
  requires that a user need not know the exact identity before finding the thing.
