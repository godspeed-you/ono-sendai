# ADR-0263: The spatial index is swept where it is filled

- Status: accepted
- Date: 2026-08-29
- Spec refs: v0.4 §33.2, §33.3, §34.2, §20.1, §20.3, §20.4, §45.2
- Decided by: agent (autonomous, `close-spat`)

## Context

v0.4 §33.2 says the index is a cache and the providers remain authoritative. Nothing implemented
the first half of that sentence: `SpatialIndex` only ever grew. Every observation registered what
a provider answered with, and nothing ever dropped what a provider had stopped answering for, so
on a host whose processes come and go the index accumulated one population per observation.

Measured on this host (701 live processes), six `map` projections of `COMPUTE` in one session:

```text
without a sweep   1411 → 1608 → 1687 → 1718 → 1844 objects
with a sweep      1394 → 1412 → 1428 → 1286 → 1384 objects
```

The projection ranks, clusters and looks up canonical parents over the whole index, so every
redraw did more work than the one before. §34.2 fixes a 16 ms frame target inside a rendered map
and prohibits unbounded graph rendering; a full-screen map that re-projects on every resize, live
tick and key press was getting monotonically slower for as long as it stayed open. The visible
symptom was
`spatial_interactive_missing::should_preserve_the_current_place_when_the_terminal_is_resized_with_a_place_open`
failing about half of all gate runs: under a concurrent gate the ESC that closes the view arrived
after the projection budget had already been spent redrawing a population that no longer existed.

§33.3 gives every object class a TTL but says nothing about how long the index *holds* what it no
longer trusts, and §45.2 lists "freshness state" among the index's responsibilities without saying
what expiry does. That gap is what this ADR closes.

## Decision

**A TTL says when the index stops trusting an entry; a retention says when it stops holding it.**

1. `FreshnessPolicy` gains `retention(object_type)`, the span an observation is kept for after the
   last provider answer. It is a multiple of that class's §33.3 TTL, two by default: one lifetime
   to go stale in, and one more in which a `back`, a `trail` or a pin can still arrive at the
   place. `with_retention(lifetimes)` tunes it; §33.3's "MAY be tuned without changing semantics"
   covers both numbers.
2. `SpatialIndex::forget_stale(now, protected)` drops every entry whose retention has run out,
   with its aliases and its provider-identity mapping, and answers with how many it dropped. An
   entry a subscription is delivering changes for is current by construction (§33.3's
   "event-driven") and is never dropped.
3. **The index does not decide what is worth keeping; the session does.** `forget_stale` takes the
   protected set explicitly, and `SpatialSessionState::sweep` builds it from the four things the
   session can still be asked about: the place it is standing on and every place in its trail
   (§20.1), every pinned place (§20.4), every place it holds a tombstone for (§20.3, §10.3) and
   the centre of the map it last drew. §40 distinguishes a place that ended from one nobody ever
   saw, and that distinction is only answerable while the entry exists.
4. **The sweep runs where the index is filled** — at the end of `SpatialSessionState::absorb`.
   An observation is the moment at which the providers said what is there, so it is also the
   moment at which what they stopped saying may be dropped. The per-place record cache and the
   §25.4 comparison baselines are pruned with it, since neither can outlive the entry it belongs
   to without becoming an answer about a place the index no longer holds.

## Consequences

- The index is bounded by the population observed within a retention window rather than by the
  session's age, so a long-lived full-screen map costs the same on its thousandth redraw as on its
  first.
- A place that has not been observed for two lifetimes and that nothing points at has to be
  observed again before it can be entered by name. That is what §33.2 asks for: the providers are
  the truth, and the index was never entitled to answer for them from a claim that old.
- Retention is a policy number, not a semantic one. Shortening it costs re-observation; lengthening
  it costs memory. Neither changes what any command answers.
- Encoded by `ono-spatial-index/tests/index.rs::should_forget_an_object_no_provider_has_answered_for_since_its_retention_ran_out`,
  `::should_keep_an_object_the_session_still_points_at_when_it_forgets_the_stale_ones` and
  `::should_stay_bounded_when_a_churning_population_is_observed_over_and_over`.

## Alternatives considered

- **Reconcile against a complete enumeration** — after an unnarrowed `get process`, every Process
  entry the answer omits is provably gone. Exact where it applies, and it applies to fewer answers
  than it looks: a narrowed query, a partially refused one and a relationship expansion are all
  incomplete, and treating one of them as complete would forget a live place. Rejected as the
  *only* mechanism; it remains available as a later refinement on top of retention.
- **A fixed maximum entry count with LRU eviction** — bounds memory without saying anything true
  about the objects, and would evict a live place while keeping a dead one.
- **Never sweeping, and making the projection cheaper instead** — treats the symptom. The index
  would still answer `enter <name>` from an observation made an hour ago, which §33.2 forbids in
  substance even where it does not name a mechanism.
