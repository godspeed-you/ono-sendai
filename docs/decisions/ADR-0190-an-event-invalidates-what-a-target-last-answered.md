# ADR-0190: An event invalidates what a target last answered

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §2.12, §25.1, §25.2, §25.3, §33.1, §33.2, §33.3, §34, §45.4
- Decided by: agent (autonomous, integration of S6, S7, S8)

## Context

Two accepted decisions met for the first time when their branches were merged, and they
contradict each other on one path:

- **ADR-0186** — a repeated view is *read* rather than asked for again. A provider target that
  answered inside its §33.3 lifetime is recalled from `SpatialSessionState::recall`, which is how
  §34's warm-`look` budget is met and what the §25.3 word `cached` reports.
- **ADR-0180** — `map --live` re-projects through the still map's own path
  (`crate::spatial::map::project_at`, §45.4) whenever an event says the system moved, and emits
  the difference.

Composed naively, the live map re-projects by *reading the answer from before the change*: the
horizon is observed through `view::observe_space`, which recalls, so a connection that closed
inside the target's lifetime is still drawn as open. §2.12 — "Motion and visual updates MUST
correspond to actual topology or metric changes" — is then violated in the other direction: the
screen fails to move when the machine did, and the value that is emitted describes a moment that
has passed.

## Decision

**An event is the statement that the assumption behind the cache no longer holds.** Before a live
re-projection, `crate::spatial::live::reproject` calls
`SpatialSessionState::forget_targets`, which drops what every target last answered, so the
horizon is observed from the providers again.

The still `map`, `look` and `near` are untouched: they have no event telling them anything moved,
and for them §33.3's lifetime is exactly the right assumption. §33.2 decides the conflict —
"the providers are authoritative" and the index is a cache — and ADR-0186 itself says a stale
target is asked again. This makes "an event arrived" one of the things that makes a target stale.

## Consequences

- `spatial_relationships_missing::should_show_the_connection_edge_appear_and_vanish_when_the_connection_opens_and_closes`
  and `docker/acceptance/cases/108-spatial-live.case` observe the change they were opened for.
- A live map costs one observation per *change*, not per tick, which is what ADR-0180 already
  budgeted for; a quiet system still costs nothing.
- The invalidation is whole-session rather than per-target. The events a live map subscribes to
  are exactly the targets its horizon reads (`live::targets_of`), so the distinction has no
  practical content here, and a narrower rule would be a second model of which target answers
  for which object.

## Alternatives considered

- **Give `project_at` a "fresh" flag.** Rejected: the caller that needs fresh data is the one
  that knows something changed, and threading a boolean through the shared projection path would
  put the decision in the function that must not make it (§45.4).
- **Shorten the §33.3 lifetime for socket-like objects.** Rejected: it would make every ordinary
  `look` more expensive to fix a case that is not about time passing at all.
- **Let the live loop observe the providers itself.** Rejected outright by §2.16 and by ADR-0180
  point 4: there is one projection path, and both views take it.
