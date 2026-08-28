# ADR-0178: Live spatial state is its own crate, and its events are the watch runtime's

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §2.12, §2.16, §3.7, §24.3, §25.1, §25.2, §25.3, §25.4, §26, §43.6, §45.5
- Decided by: agent (autonomous, phase S7)

## Context

§45.5 gives `ono-spatial-events` five responsibilities — "provider event merge, snapshot diff,
change state, landmark recalculation triggers, live map update messages" — and §50's S7 phase
delivers them. Three questions had to be settled before any of it could be written.

**Where do the events come from?** §25.1 requires `map --live` to "subscribe to available
provider events and/or explicit polling sources". The shell already has such a runtime: v0.2
§18.2's `watch <target>`, whose semantics ADR-0024 fixes — a stream begins with `snapshot` events
carrying the current state, then reports `added`, `changed` and `removed`, and every event says
through `source` whether it was seen by `subscription` or by `poll`. §2.16 forbids the spatial
layer from becoming a second source of system truth, and a second event runtime beside the first
would be exactly that: two loops asking the same providers, disagreeing about when something
happened.

**What does the change model look like?** §3.7's landmark reasons are a closed vocabulary and
three of them are differences rather than facts — `new_object`, `removed_object`,
`recently_changed`. §25.1 lists six things a live map *may* visualize. §25.2 forbids motion that
does not correspond to a change, and §43.6 forbids a test passing "based only on timer animation".

**What may a live view claim about itself?** §25.3 fixes five words — `event-driven`, `polled`,
`cached`, `stale`, `partial` — and §25.4 requires a change built by comparing snapshots to say so.

## Decision

1. **`crates/ono-spatial-events` holds the change model and nothing else.** It reaches no
   provider, no terminal and no clock. A caller hands it two projections, or a stream of events,
   and gets back what differs. That is what makes a live view testable against a real change
   rather than against a frame counter (§43.6).

2. **The event envelope is the v0.2 watch runtime's, unchanged.** `EventMerge::absorb` reads an
   `ono.<target>-event/1` record: `kind`, `at`, the object under the target's own field name, and
   `source`. No spatial event type is defined beside it (§2.16). The object field is found as the
   one record field the envelope carries beside the four it declares for itself, so a new watch
   target needs no entry in a table here.

3. **Freshness is derived from the sources, and a view is as live as its least live source.**
   `subscription` → `event_driven`, `poll` → `polled`, and a view that has seen no event at all is
   `cached` — never `event_driven`, which would promise a liveness nothing delivers (§2.12).
   `Freshness::as_str` writes `event_driven`, not the spec's prose `event-driven`, so every
   freshness word in the structured output is one identifier a script can compare.

4. **The snapshot comparison compares what a change can be about, and nothing else.**
   `MapSnapshot::of` keeps the drawn node ids with their label, provider state and landmark
   reasons, and the edge ids with the places they join. It deliberately drops `generated_at`,
   `map_id`, the ranking order and the hidden counts: a comparison that noticed those would report
   change on every tick, which is the decorative motion §25.2 forbids. Two identical projections
   produce an empty `ChangeSet`.

5. **`ChangeSource` travels with every change set**, and `compare` always stamps
   `snapshot_comparison` — §25.4's "the provenance must identify that the change was inferred from
   snapshots", as a value rather than as prose.

6. **`ChangeSet::affected()` is the landmark recalculation trigger of §26.** It names the places a
   change touched, both ends of an edge included, because a connection appearing is what a
   `connection_spike` is made of. Only the three §3.7 change reasons are produced; a core rule may
   not invent a reason (§3.7), so an edge appearing is reported through the places it joins.

## Consequences

- `map --live` (ADR-0180) and `look --changes` (ADR-0181) share one definition of "changed", so
  the live view and the still view cannot disagree about what happened.
- The day a provider grows a real subscription, `watch` switches its `source` to `subscription`
  and every live spatial view says `event_driven` without another change here.
- The spatial observation seam now takes `&ProviderRegistry` rather than `&Invocation` (commit
  `refactor(spatial): the observe seam takes the provider registry, not the invocation`), because
  a live loop outlives the invocation that started it. That was a precondition, not a decision.
- Encoded by `crates/ono-spatial-events/tests/snapshot_comparison.rs` (7 outcome tests) and
  `crates/ono-spatial-events/tests/event_merge.rs` (8).

## Alternatives considered

- **A spatial event type of its own, with providers publishing to it.** Rejected: it is the second
  source of truth §2.16 forbids, and it would need every provider changed before any of it worked.
- **Diffing the whole `SpatialMap` record.** Rejected: `generated_at` and `map_id` move on every
  projection, so every tick would be a change — §25.2's forbidden "continuous motion simply to
  appear cyberpunk", arrived at by accident rather than by design.
- **Diffing metric values (cpu, memory, byte counters) as well.** Not rejected on principle —
  §25.1 lists "metric changes when relevant to landmark status" — but a metric only becomes a
  spatial change when it crosses a landmark threshold, and that crossing *is* diffed, as a
  landmark appearing or disappearing. Diffing the raw numbers would emit on every tick of a busy
  system.
