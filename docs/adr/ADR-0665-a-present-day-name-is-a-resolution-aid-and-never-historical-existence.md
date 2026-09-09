# ADR-0665: A present-day name is a resolution aid and never historical existence

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §14.4, §14.3, §9.7, §5.5, §27.3, §39.3
- Decided by: agent (autonomous)

## Context

§14.4 lets `find place` in historical context use present-day aliases to reach a historical object,
and then attaches a MUST: the implementation "MUST distinguish **resolution aid** from
**historical existence evidence**". The failure it is guarding against is the one §14.3 also names
from the other side — a present-day fact leaking into a historical answer, so that the past is
rendered as the present with an old timestamp on it.

Finding `nginx` in today's index is a fact about today. It is a legitimate way to *reach* a
candidate identity, and it is no evidence at all that anything called `nginx` existed at 12:17.

## Decision

**Every historical place match carries the basis it resolved on, and the two bases carry different
fields.**

- `ResolutionBasis::HistoricalEvidence` — an event at or before the active instant named a subject
  whose label matches. The match carries the `EventId` that supports it and the instant that event
  places it at, so `at event`, `why event` and `map --at event` reach the same event (§27.3).
- `ResolutionBasis::ResolutionAid` — today's index answered and the historical index did not. The
  match carries **no anchor and no instant**, because there is nothing to point at. `existed_at` is
  `None` and `is_evidence_of_existence()` is false.

The historical index is searched first and it wins: an identity supported by an event keeps
`HistoricalEvidence` even where today's index also matched it, because the alias helped find it and
the event is why it is there.

The present-day index arrives through a `PresentAliases` trait the caller implements. This crate
reaches for no live state (§39.3), so what the caller passes is exactly what the shell already
knows, and a test passes an empty one to prove the historical answer stands alone.

## Consequences

- A renderer can draw the difference, and a pipeline can filter on it. A caller that wants only
  what existed then filters on `is_evidence_of_existence()`; §9.7's "place not known at requested
  time" is the answer when nothing but aids came back.
- Two failures become impossible rather than unlikely: a present-only object cannot be reported as
  having been somewhere (§14.3), and a historical answer cannot silently inherit today's names.
- A match with an unresolved subject (§5.5) never becomes a place, because only a resolved
  `SpatialRef` carries the identity a place needs.
- Encoded by `crates/ono-temporal-query/tests/search.rs`, including the case where today's index
  and the ledger both answer for one identity.

## Alternatives considered

- **Merging both into one match list with a confidence number.** A number invites a threshold, and
  a threshold turns "we found the name today" into "it probably existed", which is the sentence
  §14.4 forbids.
- **Refusing present-day aliases entirely.** Safe and it loses the discovery §20.1 asks for: a user
  must not need to know the exact identity before being able to find the thing.
