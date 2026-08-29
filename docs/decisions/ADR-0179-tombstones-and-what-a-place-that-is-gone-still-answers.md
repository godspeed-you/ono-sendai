# ADR-0179: Tombstones — when a place becomes one, and what it still answers

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §2.17, §10.3, §20.3, §33.2, §35.2, §40, §42.2, §44.7, §53
- Decided by: agent (autonomous, phase S7)

## Context

§10.3 is four sentences: recently removed objects MAY remain as short-lived tombstones in
navigation history and live maps; a tombstone MUST be visually distinct; and it MUST NOT accept
actions that require a live object. §20.3 adds that `back` resolves one where it is available and
retains the original trail record. §53 states the case: "Old process tombstones; stable service
remains; new process has new identity."

`ono-spatial-core` has held `Tombstone`, `Liveness` and `TombstoneRegistry` since S1. Nothing
recorded one, and the consequence was the §33.2 violation this ADR removes: a second `look` at a
process that had exited answered `state: sleeping` out of the index, because the only path that
re-read the object fell back to the last record whenever the provider answered nothing.

Four questions had to be settled.

## Decision

1. **A place becomes a tombstone when a provider that was asked about it does not answer for
   it.** The observation is `crate::spatial::relations::refresh`, which every `look`, `near`,
   `follow` and `map` at an object place already performs, and `crate::context::enter_object`,
   which asks the provider directly. An empty answer counts only when the provider *read* the
   system: `io.not_found` and `resolve.target_not_found` are the object saying it is gone
   (`/proc/1842/stat: No such file or directory` is what a process exiting looks like from
   outside), and every other error is a reading failure, which §35.2 and §42.4 forbid rendering as
   absence. A provider that ignores an identity selector and answers with everything is handled by
   the same rule: the place is gone exactly when nothing it answered with projects onto it.

2. **The index keeps the entry; the session keeps the fact that it ended.** `SpatialIndex::
   mark_ended` closes the object's lifetime and `SpatialIndex::forget_edges` drops the
   relationships nobody asserts any more, both from *both* ends. The entry itself stays, because
   "the identity is retained" is precisely what tells a tombstone from a place that never existed
   (§40), and because §20.3 makes `back` arrive at one. `SpatialIndex::remove` remains the other
   answer — forget the place entirely — and nothing in this increment calls it.

3. **What a tombstone answers, and what it refuses.** `look` and `near` describe it: they are
   reading, and §2.17 requires the difference between "gone" and "never there" to be visible.
   `back` arrives at it (§20.3's first clause), and `still_a_place` now skips a place only once
   its tombstone has expired. `follow` refuses with `spatial.destination_gone`, because the edges
   of a place that is gone are the ones it *had*: traversing one requires the live object (§10.3).
   `enter <target> <identity>` refuses the same way when this session watched the place go, and
   with `spatial.not_found` when it never saw it — §40 keeps those two conditions apart and this
   is what makes the difference observable.

4. **"Short-lived" is a setting, and a minute is the default.** `spatial.tombstone.lifetime`
   (§26.3's rule applied to §10.3: a threshold in the product must be inspectable). A minute is
   long enough that a `back` onto a process that has just exited arrives at it, and short enough
   that no place returns from the dead in the middle of an investigation, which §10.3's Intent
   names as the disorientation tombstones exist to prevent. Beyond it the place is `Gone`: `back`
   skips it with a notice, and the trail keeps its record.

5. **The tombstone is a field, and a line.** `ono.spatial-place/1` gains a nullable `tombstone`
   record — `state` in §10.3's own words ("exited 12s ago"), `removed_at`, `age`, `replacement`,
   `replacement_via` — and the place's `state` becomes the word that is true of a gone object of
   that kind: `exited` for a process or a job, `stopped` for a service or a container, `closed`
   for a socket, `unmounted` for a mount, `removed` otherwise. The text renderer prints
   ` tombstone — exited 12s ago` under the heading, in words rather than in colour, because §39.1
   forbids colour from carrying meaning alone. That is §10.3's "visually distinct", in both the
   structured and the rendered form.

## Consequences

- The index stops being able to answer with a live state for a dead object, which is §33.2 held
  from the side that was open.
- `map --live` (ADR-0180) gets removal for free: an event that says an object is gone records its
  tombstone and drops its edges, and the next projection no longer draws it.
- Green now, all previously ignored: `spatial_identity_missing::should_report_a_tombstone_rather_than_a_live_place_when_the_visited_process_has_exited`,
  `…should_refuse_to_traverse_a_relationship_when_the_place_is_a_tombstone`,
  `…should_never_resolve_a_tombstoned_place_to_a_live_object`,
  `…should_return_the_tombstone_and_keep_the_trail_record_when_back_points_at_a_dead_place`.
  Plus five crate-level outcome tests in `crates/ono-spatial-core/tests/trail.rs` and
  `crates/ono-spatial-index/tests/index.rs`.
- `place_record_of` takes one more argument. Every caller but the current place passes `None`: a
  neighbour is not re-read when a place is looked at, so the session has no evidence about it and
  §2.17 forbids guessing.

## Spec deviation

- Section: v0.4 §40, as `spatial_identity_missing::should_distinguish_a_tombstone_from_a_place_that_never_existed`
  reads it.
- Text: "Required error codes include: `spatial.not_found` … `spatial.destination_gone`".
- Instead: `spatial.destination_gone` is delivered for the v0.2 spelling `enter <target>
  <identity>` and names the tombstone and its age. `spatial.not_found` is **not** substituted for
  v0.2 §14.3's `resolve.target_not_found` on the same spelling: an identity nothing answers to
  still refuses with `Ono-Sendai-E0102`, which `identity_missing::should_refuse_to_enter_a_user_that_does_not_exist`
  pins. The v0.4 spelling `enter <selector>` answers `spatial.not_found` as it already did.
- Why: §40 adds the *gone* condition to a shell that already had a refusal for the *never there*
  one, and nothing in §40 says the earlier one is wrong. Re-spelling every `enter <target>`
  refusal is a contract change with its own test surface, and doing it inside a tombstone
  increment would have been exactly the mixing AGENTS.md §4 forbids.
- Also not delivered: `!never.status().is_success()` in that test. The refused `enter` leaves the
  place where it was — §40 requires it and `spatial_storage_missing::should_refuse_a_path_that_does_not_exist_with_a_structured_error`
  asserts it — so the following `look --json` succeeds, and ADR-0008 makes a script's status its
  last statement's, as every Bourne-family shell does. Making that run fail needs either a
  script-wide "any statement failed" status, which reverses ADR-0008 and would make
  `ono -c 'false; true'` exit 1, or a refused movement aborting the script, which breaks the
  storage test above. The test stays ignored with that analysis on it, so whoever owns the
  script-status contract takes the decision deliberately rather than it being taken silently here.

## Alternatives considered

- **Removing the place from the index when it goes.** Rejected: §20.3 needs `back` to arrive at
  it and §40 needs "gone" to be distinguishable from "never there", and both need the identity.
- **A time-to-live on the index entry instead of a tombstone registry.** Rejected: freshness and
  liveness are different questions. A place can be stale and alive, and §33.4 requires the first
  to be visible without implying the second.
- **Refusing `look` at a tombstone.** Rejected: §10.3 forbids actions that need a live object, and
  reading what a place *was* is not one. It is also the only way a user finds out what happened.
