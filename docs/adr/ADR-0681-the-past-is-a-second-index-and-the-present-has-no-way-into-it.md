# ADR-0681: The past is a second index, and the present has no way into it

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §14.1, §14.3, §55.2, §55.9, §9.1, §45.6 (v0.4)
- Decided by: agent (autonomous)

## Context

§14.1 makes nine v0.4 spatial commands evaluate against historical state. §14.3 attaches the rule
that decides whether they really do: "an exit shown by `look` or `near` at time `T` MUST correspond
to a relation or hierarchy supported at `T`. Current-only exits MUST not leak into a historical
neighborhood." §55.2 names the failure from the other side — "rendering today's graph with an old
timestamp is prohibited" — and §55.9 says what a feature that fails it is worth: nothing.

The obvious implementation is to keep drawing from the session's live index and to filter out what
the coordinate does not support. Every such filter is a place a later change can forget, and the
thing being filtered is the default: a missed filter shows the present, which looks right.

## Decision

**The historical world is a separate `SpatialIndex`, built from a `ReconstructedWorld` and from
nothing else, and no code path can add the present to it.**

`crate::spatial::historical::HistoricalWorld` has exactly one constructor,
`reconstruct(&dyn LedgerRead, &SpatialScope, Timestamp)`. Its parameters are the ledger, a boundary
and an instant. There is no argument, field, method or builder through which a live `SpatialIndex`,
a `SpatialSessionState` or a `ProviderRegistry` can reach it, and `index()` yields a shared
reference, so nothing can absorb into it after construction. The properties §14.3 asks for then
hold by construction rather than by vigilance:

- objects are `ReconstructedWorld::objects()` filtered on `Presence::Present`;
- edges are `ReconstructedWorld::relations()`, which yields only edges supported at the instant —
  the reconstruction crate made that an API property, and this module re-derives nothing;
- an edge is recorded only where **both** ends are places this world holds, so a historical exit
  always leads to a historical place;
- ranking sees an empty `PinRegistry`, because a pin is a bookmark the reader made today and
  ranking a reconstructed map by one would be a present-day fact shaping a past answer.

The nine commands take a historical branch that reaches for the world and never for the session's
index. `ono_spatial_query::{neighborhood_of, project as project_map, find_places, resolve}` are
pure functions of an index, a request and a `now`, so they answer historically unchanged: the
historical instant is passed as `now`, and the index freshness policy is uniform and wide, because
a reconstruction is exactly as old as the instant it reconstructs and a wall-clock TTL would call
the whole past stale.

`back` is unchanged. §14.1 says it "continues to traverse the actual session spatial trail", and
every place it lands on is rendered by the next `look`, which is evaluated at the coordinate.

## Consequences

- The leak §14.3 forbids is not a bug that can be reintroduced by forgetting a filter; it needs a
  new constructor or a new field, which is a visible change.
- `crates/ono-cli/tests/spatial_historical.rs` encodes it: a present index holding an object and a
  relation that exist only today is handed to the search path, and neither reaches the historical
  neighbourhood, the historical map or the historical index.
- A historical `look`, `near`, `map`, `find place`, `enter`, `follow` and `up` ask no provider at
  all. A provider answers about now; an answer from one would be the present reached through a past
  question.
- A place the reconstruction does not support is refused rather than drawn (§9.7), so a historical
  command can fail where its present-day spelling succeeds. That is the point.

## Alternatives considered

- **Filtering the live index by the coordinate.** Rejected: the default of a missed filter is to
  show the present, and §55.2 is precisely about how convincing that looks.
- **A `Timestamp` field on `SpatialIndex` that every read honours.** Rejected: it puts temporal
  logic into a v0.4 capability crate, changes every existing call site, and still leaves the live
  entries in the same collection as the reconstructed ones.
