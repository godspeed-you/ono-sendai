# ADR-0778: The timeline is a stream, and its window is a rendering

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §11.2, §11.4, §11.5, §11.7, §8.5, §19.2, §39.3, §56.9
- Decided by: agent (autonomous)

## Context

v0.5 §11.4 fixes the output type of `timeline` in three lines and leaves no room:

```text
Stream<TemporalEvent>
```

> The timeline renderer is only a presentation.
>
> This MUST work:
>
> ```text
> timeline --since 1h
>     | where kind == "object.changed"
>     | where subject.type == "service"
> ```

§56.9 makes it a release criterion — "`timeline` is typed and pipeline-compatible" — and
`docs/contracts/commands/temporal.yaml` has declared `output: stream<ono.temporal-event/1>` since
the command was registered, with that pipeline among its examples.

The implementation answered with **one** `ono.temporal-timeline/1` record carrying the events under
an `events` field, alongside `from`, `until`, `coverage`, `gaps`, `truncated` and `groups`. So the
specified pipeline failed:

```text
ono: Ono-Sendai-E0202 type.unknown_field no field `kind` on this record
  `ono.temporal-timeline/1` declares no such field
```

The pre-flight type check of v0.2 §11.3 passed, because it validates against the *declared* type;
the runtime value was a different schema. That is contract drift `spec-check` cannot see: it
compares the registry against the command's declaration, not against the value the command builds.

The record existed for a reason, and the reason is also normative. §11.7 obliges the text renderer
to draw a coverage gap inside the window — "A gap MUST not be hidden simply because events exist on
both sides" — and §8.5 obliges a view to say what its coverage is rather than implying completeness
by saying nothing. Neither fact is on an event.

## Decision

Both rules hold, in the places each of them names.

**The value is the events.** `timeline` returns `Stream<TemporalEvent>` — `ono.temporal-event/1`
rows and nothing wrapped around them — so `where`, `take`, `group` and `to json` are ordinary
stages over it and §11.4's MUST is met.

**The window is a presentation.** The command publishes the composed `ono.temporal-timeline/1` —
its bounds, its coverage, its gaps, whether a limit cut it — into one slot in `crate::sink`, and
the sink draws the events it is handed against it. §11.4 calls the renderer "only a presentation",
and this is what that costs: the presentation is given its context rather than deriving it.

**The default rendering states the interval and the coverage.** A trailing line —
`window: 08:46 - 09:46  evidence: procfs, systemd   coverage: partial, 1 gap` — says what §19.2's
full-screen timeline says in a header and an evidence line. A reader who sees rows and no interval
has been told the rows are everything, which is the implied completeness §8.5 exists against.

**A gap is a row.** `ono_temporal_render::timeline` already interleaves gap entries between the
events on either side of them; that is unchanged and is what §11.7 asks for.

A filtering stage narrows what is drawn and leaves the window intact. That is the honest reading
rather than a compromise: the gap was in the interval whether or not a `where` kept the events on
either side of it, and a reader filtering to `action.executed` is still looking at the same hour.

## Consequences

- `timeline --since 1h | where kind == "object.changed" | where subject.type == "service"` runs.
  §56.9 holds and the command registry and the runtime value now agree.
- `timeline … | to json` answers a JSON array of events. Anything reading `events`, `gaps`,
  `coverage`, `from`, `until` or `truncated` off that document reads the rendering instead —
  `docker/acceptance/cases/236-timeline-relevance.case` and `238-timeline-gaps.case` were rewritten
  accordingly, and both got stronger for it: 238 previously proved only that a `"gaps"` *field*
  existed, which an empty array satisfied.
- The published window is process-local and per-turn. Two timelines in one script leave the second
  one's window standing, which is what a reader of the second one wants.
- A stream of `ono.temporal-event/1` from somewhere else — `find event`, a saved list — renders as
  a timeline when a window has been published in the same process, and as ordinary rows otherwise.
  Drawing events as a timeline is not a claim about where they came from.
- Encoded by `crates/ono-cli/tests/timeline.rs::should_stay_a_stream_a_later_stage_can_filter_when_the_timeline_is_piped`
  and `::should_state_the_window_and_the_coverage_the_answer_rests_on`, and by
  `docker/acceptance/cases/238-timeline-gaps.case::t6ak`, `::t6ak2`, `::t6al`.

## Alternatives considered

- **Keep the record and make `where` reach into `events`.** Rejected: it would make one schema's
  field a special case in the evaluator, and §11.4 names the type rather than the convenience.
- **Emit the window as a first value in the stream.** Rejected: the stream would no longer be
  `Stream<TemporalEvent>`, and every consumer would have to know to skip a head element — the same
  defect in a costume that type-checks.
- **Drop the gaps.** Rejected outright: §11.7 is a MUST and §55.5 names the silent gap as the
  failure that destroys operator trust.
