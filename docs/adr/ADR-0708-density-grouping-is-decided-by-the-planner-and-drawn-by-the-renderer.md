# ADR-0708: Density grouping is decided by the planner and drawn by the renderer

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §19.4, §19.5, §11.4, §39.3, §43.3; ADR-0662, ADR-0702
- Decided by: agent (autonomous)

## Context

§19.4 lists four grouping dimensions and one requirement:

> Grouping MUST preserve hidden counts and time span.

§19.5 adds the second: "A grouped event MUST be expandable to individual retained events where they
exist."

Both were implemented, twice. `ono_temporal_query::timeline::group` forms `EventGroup`s over the
four dimensions, enumerates their members and counts what a provider had already folded (ADR-0662).
`Timeline::to_record` wrote the events flat and dropped the groups, so
`ono_temporal_render::timeline_view` formed its own runs by a second rule (ADR-0702) — a narrower
one, keyed on three kind names, unable to see a provider's own aggregated count.

A record that says nothing about grouping leaves the renderer to guess, and the two rules disagree:
the planner groups connection churn by object type, the renderer does not; the planner reports a
provider's 417 folded samples, the renderer counts the rows in front of it and says 1.

## Decision

**1. `ono.temporal-timeline/1` declares a nullable `groups`.** Each row states the representative
`event_id`, every `members` id, the `hidden` count, the `from` and `until` of the span, and the
`reason` (§19.4's dimension) the run was grouped under.

**2. The planner fills it. `timeline` groups the window it just answered** with
`DEFAULT_AGGREGATION_WINDOW`, so a record always carries the judgement of the crate that owns it.

**3. The renderer draws the producer's rows, and keeps its own rule only as the fallback** for a
record whose `groups` is null. §39.3 makes a renderer a presentation of query output; two grouping
rules made it a second query planner.

**4. The count and the span are read, never recomputed.** §19.4's fourth dimension is "cluster-level
events already aggregated by a provider", where the hidden count is larger than the members the
ledger retained. A renderer that counted the rows it can see would under-report exactly the case
the dimension exists for.

**5. §19.5's expansion still reaches the individuals.** `members` is a list of ids, and the renderer
resolves each against the events the same record carries — no second query, and a member the window
did not retain is simply not drawn, which is what "where they exist" means.

**6. A stated row that hides nothing is an ordinary row.** `hidden: 0` with one member draws as a
plain event line, so the planner's decision not to group is drawn as not grouping.

**7. `temporal.ui`'s `group_repeats` still gates grouping.** §11.5's default text rendering is a row
per event; grouping belongs to §19.4's dense full-screen view and never happens behind a reader's
back.

## Consequences

- One rule decides density, and it is the one with the events, the subjects and the provider
  payloads in front of it.
- `Timeline` gained `groups`, filled on every answer, so the record is self-describing.
- The renderer's own rule stays and stays tested: a hand-built record, a KUANG/11-contributed
  timeline or an older producer still draws sensibly.
- A group row whose members are all outside the window is dropped rather than drawn empty.

## Alternatives considered

- **Delete the renderer's rule.** Rejected: a record carrying no `groups` is legal — the field is
  nullable — and a firehose is what §19.4 forbids.
- **Make `groups` required.** Rejected: schema changes here are additive only, and a producer with
  no opinion about density should not have to invent one.
- **Have the renderer regroup from `reason` alone.** Rejected: it reintroduces the second rule, and
  the hidden count of a provider cluster is unrecoverable from the rows.
