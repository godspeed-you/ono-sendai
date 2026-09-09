# ADR-0702: A grouped timeline row keeps its count and its span and hides no lifecycle change

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §6.3, §11.4, §18.4, §19.4, §19.5, §26.3, §43.3; ADR-0662, ADR-0668
- Decided by: agent (autonomous)

## Context

§19.4 requires large event sets to be "semantically grouped rather than rendered as an unreadable
firehose", lists the four dimensions a grouping may use, and ends with "Grouping MUST preserve
hidden counts and time span". §19.5 adds that a grouped event must be expandable to the individual
retained events.

Two questions had to be answered. Where does grouping happen, and how does a row that stands for
seven events say so without becoming a second data model?

§11.4 settles half of the first: `timeline` returns a stream of events and the text rendering is a
presentation of it, so the typed stream stays one record per event whatever else happens.

`ono-temporal-query` groups too. ADR-0662 gives it an `EventGroup` over `TemporalEvent` with a
representative, every member, a hidden count, a span and a reason. `Timeline::to_record`
(ADR-0668) writes `events` as a flat list and carries no groups, so nothing of that reaches a
renderer: the two crates cannot exchange groups today, and a full-screen timeline that honoured
§19.4 had to form its own or not honour it.

## Decision

**1. Grouping is off by default and belongs to the dense view.** `RenderOptions::group_repeats` is
false, so §11.5's default text rendering is a row per event. `timeline_view` is where a caller
turns it on, because §19.4 is a section about the full-screen timeline.

**2. A group is a run of adjacent rows that are the same row.** Consecutive events group when they
share a subject label, a kind and the set of field names they changed. Adjacency is what makes the
group safe: the events keep the presentation order the query gave them (§26.3), and no event is
moved to join a group.

**3. Three kinds may be grouped and no others.** `object.observed`, `object.changed` and
`provider.event`. An appearance, a disappearance, a relation change, an operator action, a
checkpoint and a landmark are never folded, because §6.3 already forbids inventing a disappearance
and hiding one does the same damage from the other side, and §18.4 makes exactly those events the
ones a reader is stepping through.

**4. A grouped row states both numbers §19.4 asks for.**

```text
12:18:01.000  socket/8080            observed  x4 over 3.00s  @e18000000
```

`x4` is the count of events the row stands for and `over 3.00s` is the span from the first to the
last. The row's own clock is the first member's, so the group has a start, a length and a count,
and nothing about it is hidden.

**5. Expansion is data, not state.** `RenderOptions::expanded` holds event references, and a group
whose first member's id one of them prefixes is drawn as its header followed by its members,
indented. The reference is the `@e…` the row itself printed, so a view stores what it saw.

**6. A truncated list says so.** `truncated` on `ono.temporal-timeline/1` renders as its own line,
because §19.4 requires a reader to be able to tell a quiet interval from a cut one, and grouping
makes that distinction easier to lose.

## Consequences

- **Two grouping implementations exist, and the lead should collapse them.** The honest fix is a
  `groups` field on `ono.temporal-timeline/1` carrying ADR-0662's rows — representative, members,
  hidden, span, reason — after which this renderer draws the producer's groups and its own rule
  becomes the fallback for a record that carries none. Flagged rather than decided here, because
  the field is a contract the lead owns.
- No configuration key is invented. §19.4 mentions "a configured aggregation window" and §33's key
  list is closed, so grouping uses adjacency rather than a window nobody declared, and prints the
  span so the reader sees how long the run took.
- `timeline --json` and every pipeline stage are unaffected: grouping exists only in the rendered
  text.
- A group of two is still a group. That is deliberate — the row is shorter than two rows and states
  everything two rows would.
- Expansion needs no round trip to the ledger, so §39.3 holds: the members are already in the
  record.

## Alternatives considered

- **Grouping in `ono-temporal-query`.** Rejected by §11.4: it would change the typed stream a
  `where` clause filters.
- **Grouping by a time window rather than by adjacency.** Rejected: it needs a configured window,
  §33's key list is closed for v0.5, and a window can group across an event it should not.
- **A `hidden_count` field on the timeline record.** Rejected: the count belongs to a group the
  renderer formed, and the producer formed no groups.
