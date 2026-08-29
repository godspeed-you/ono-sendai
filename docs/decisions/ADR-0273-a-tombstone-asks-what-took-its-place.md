# ADR-0273: A tombstone asks what took its place

- Status: accepted
- Date: 2026-08-29
- Spec refs: v0.4 §10.2, §10.3, §32.1, §40, §53, §2.17
- Decided by: agent (autonomous, `close-spat`)

## Context

§10.3's tombstone example shows a `replacement:` line, `ono.spatial-place/1` carries
`tombstone.replacement` and `tombstone.replacement_via`, and both always answered `null`.
`Tombstone::replaced_by` existed and was called from nowhere.

It cannot be answered at the moment the object ends. The process died; the unit that controlled it
has not been observed since, so the index holds nothing to name — and inventing one would be the
guess §2.17 and §53 both forbid ("the new process has new identity"). An earlier increment built
the machinery, could not make it answer, and reverted rather than ship half.

Worse, the information needed to *ask* was being destroyed: `record_removed` calls `forget_edges`,
which drops the inbound edges — including the one that says which source reached the dead object —
before anything can read them.

## Decision

**1. A tombstone keeps the sources that reached the object, with their relations.** Captured in
`record_removed` from the index entry's inbound edges, before `forget_edges` runs. At most eight,
because §32.1 bounds what a view may spend and this is spent on a render.

**2. The question is asked when the tombstone is rendered, not when the object ends.** `look` at a
tombstoned place — after the observation that discovers the place is gone — asks each remembered
source about the one relation it reached the dead object by. Each is a targeted observation of one
object (`relations::observe` with an `Interest` narrowed to that relation), never an enumeration.

**3. A candidate is offered only when the answer is unambiguous, twice over.** A source that now
reaches several live objects of the dead one's kind has produced a list, not a successor, and is
discarded. If more than one source names a different successor, none is offered. §2.17 and §53: a
choice among several is a guess, not a candidate.

**4. A candidate is recorded once and never revised.** §53 makes it a candidate for continuity
rather than a claim that the two objects are one; a candidate that changed under the reader would
be worse than none.

## Consequences

- `look --json` at the tombstone of a restarted service's process carries
  `tombstone.replacement` equal to the new process's `spatial_id` and `replacement_via` naming
  `service.controls_process` — which is `docs/STATE.md`'s B-spat-5 exit test, now asserted by
  `docker/acceptance/cases/096-spatial-identity-replacement.case` `44.7e`.
- The answer depends on the source being *answerable*. On a systemd host the systemd provider
  answers for the unit. In the acceptance container there is no systemd and the service manager is
  the `systemctl` fixture, which ADR-0193 forbids the spatial layer from running — so case 096 runs
  the `systemctl` the operator would type anyway after a restart, before looking. That is a fact
  about the container, not about the mechanism, and the case says so.
- Where nothing can answer, `replacement` stays `null`. That is still the honest word for a
  candidate nobody identified.
- Encoded by `ono-spatial-core/tests/trail.rs::should_keep_the_source_that_reached_a_place_so_a_candidate_can_be_asked_for_later`,
  `::should_name_the_replacement_once_one_has_been_identified`,
  `::should_keep_the_first_candidate_rather_than_revising_it`, and acceptance case 096 `44.7e`.

## Alternatives considered

- **Filling the tombstone lazily, when a later observation happens to record an edge from the same
  source** (the other route `docs/STATE.md` names) — costs nothing but answers only if something
  else happens to look at the source. In §44.7's own scenario nothing does, so the field would stay
  null exactly where the spec's example shows it filled. The render-time question is the one that
  can be relied on; the lazy route remains available as a refinement.
- **Re-observing at the moment of death** — the successor does not exist yet.
- **Matching by display name** — `nginx` and `nginx` are not evidence of continuity; the relation
  the source asserts is.
