# ADR-0180: `map --live` moves only when the machine does

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §2.12, §2.16, §6.9, §22, §25.1, §25.2, §25.3, §25.4, §34, §43.6, §44.9, §45.4
- Decided by: agent (autonomous, phase S7)

## Context

§25.1 requires `map --live` to "subscribe to available provider events and/or explicit polling
sources" and lists six things it may visualize. §25.2 forbids artificial delay or activity by
name, §2.12 makes it an invariant — "Motion and visual updates MUST correspond to actual topology
or metric changes" — and §43.6 states it from the test side: "No test may pass based only on timer
animation." §22's `live_capable` had to stop being the honest `false` S5 left it as.

## Decision

1. **The event source is the v0.2 watch runtime, not a second one.** `ono_command::watch_events`
   is the same loop `watch <target>` runs (ADR-0024), now callable. A live map opens one watch per
   provider target its horizon reads, which are exactly the targets the still projection asks —
   `crate::spatial::live::targets_of`. §2.16 forbids the spatial layer from becoming a second
   source of system truth, and two loops asking the same providers would disagree about when
   something happened. The day a provider grows a real subscription, `watch` switches its `source`
   to `subscription` and the live map says `event_driven` without another change here.

2. **The loop waits on events, never on a clock.** Nothing is emitted while nothing happens. An
   event about an object outside the horizon changes no picture and produces no value, because
   what decides emission is the projection, not the event.

3. **A moment is drained before it is drawn.** Once something has moved, the rest of that tick's
   events are read (60 ms of quiet ends it) before the map is re-projected. A connection closing
   is a removal *and*, in the kernel's table, an appearance under a new identity; a picture that
   showed one without the other would be of no moment that ever existed.

4. **Re-projection is the still `map`'s own path.** `crate::spatial::map::project_at` is called by
   both, so §45.4's "the interactive and non-interactive views must agree" holds by construction
   rather than by discipline.

5. **What is emitted is a difference, decided by `ono-spatial-events`.** `MapSnapshot::of` reduces
   a projection to node identity, label, provider state, landmark reasons and edges — never
   `generated_at`, `map_id` or the ranking order, which move without the system moving. An
   identical projection is not emitted at all.

6. **A removal is a tombstone.** An event that says an object is gone records it (ADR-0179) and
   drops the relationships it was an end of, before the horizon is read again — so the next
   projection draws what is there rather than what was, and the change set says `node_removed`
   with §3.7's `removed_object` on it.

7. **Every value says how it knows.** `freshness` in §25.3's five words, `change_source` in
   §25.4's two, `live: true`, and `changes` — the `ono.spatial-change/1` list §45.5 calls "live
   map update messages". A `map` asked once carries `live: false`, `freshness: polled`, a null
   `change_source` and no changes: §24.3 forbids inventing a change section, and §2.17 requires
   "nothing was watching" to look different from "nothing changed".

8. **`live_capable` is answered, not assumed.** True when a target the horizon reads has both an
   event contract and a provider.

9. **`spatial.live.interval` (default 500 ms)** is what polling costs where nothing subscribes.
   Fast enough that a connection open for two seconds is seen while it is open, slow enough to
   cost nothing anyone notices (§34).

## Consequences

- `spatial_relationships_missing::should_show_the_connection_edge_appear_and_vanish_when_the_connection_opens_and_closes`
  is green, and `docker/acceptance/cases/108-spatial-live.case` proves the same behaviour in the
  container, including the negative half: a live view of a place nothing is changing cannot
  produce five values in six seconds.
- An unbounded stream still has to be bounded and serialised to reach stdout (v0.2 §18.3), so
  `map --live --json | take N | to json` is the scripted spelling and `to json` yields its array
  when the Nth value arrives. A live view that is cut off before then prints nothing — which is
  why case 108 bounds each window by the change it causes rather than by a timeout.
- The full-screen live view of §23.3 and the Ctrl-C of §43.4 are S6's: this delivers the stream
  and the change model, not the alternate screen.

## Alternatives considered

- **Polling the horizon directly on a timer.** Simpler, and rejected: it is a second runtime
  beside `watch` (§2.16), and it would redraw on a schedule rather than on a change, which is the
  shape §25.2 forbids even when the content happens to be right.
- **Emitting every projection and letting the consumer diff.** Rejected outright by §25.2 and
  §2.12: that *is* motion without change.
- **Diffing metric values as well as topology.** Not rejected on principle — §25.1 lists "metric
  changes when relevant to landmark status" — and it is delivered in the form the spec ties to
  relevance: a metric crossing a threshold appears as a landmark, and landmark appearance and
  removal are diffed. Diffing the raw numbers would emit on every tick of a busy system.
