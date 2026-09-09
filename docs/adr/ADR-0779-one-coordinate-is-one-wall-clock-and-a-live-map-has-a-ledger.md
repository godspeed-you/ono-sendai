# ADR-0779: One coordinate is one wall clock, and a live map has a ledger

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §4.1, §4.6, §18.2, §18.3, §18.4, §25.3
- Decided by: agent (autonomous)

## Context

Two defects in `crates/ono-cli/src/spatial/cursor.rs`, found by driving the temporal view over a
real pseudo-terminal rather than by reading it.

**One instant, two wall clocks.** `TemporalCursor::marker` formatted the coordinate with
`Timestamp::strftime`, which is UTC, and handed `RenderOptions::default()` — offset zero — to
`paused_marker` and to the gap frame. The prompt renders the same coordinate through the session's
zone. On a host at `Europe/Berlin` the two disagreed by two hours:

```text
local:// > at -3s
@09:26:18 [PAST?]
local:// > map
                                     @07:26:18 [PAST]
```

§25.3 is explicit — "interactive display defaults to the user's current configured timezone" — and
§4.6 gives the coordinate one marker rather than one per surface. A reader looking at both saw two
instants and had no way to tell which was the session's.

**A live map with no ledger.** `TemporalCursor::of` took *both* the coordinate and the ledger from
`spatial::historical::active()`, which is `None` whenever the session is in the present. Taking the
coordinate from there is right: §4.1 makes the present the absence of a coordinate. Taking the
ledger from there is not, and it disabled the step keys in exactly the case they exist for. §18.2
opens a **live** map and freezes it with `Space`; §18.4 then walks `[` and `]` back through the
significant events behind it. With no ledger every step answered "this session has recorded no
events, so there is nothing to step through" — while `get recorder` reported a healthy store in the
same session.

## Decision

**The marker is rendered, not formatted.** `ono-temporal-render` gains `past_marker`, the companion
of the existing `paused_marker`, and the cursor hands both of them the session's real
`RenderOptions` — the same ones `crate::sink` builds for every other temporal rendering, carrying
the session's UTC offset and `temporal.ui.show_source_tags`. `crate::sink::temporal_options` became
`pub(crate)` so there is one builder rather than two. The gap frame is rendered with the same
options for the same reason: §18.6's boundaries are wall clocks a person compares with the prompt.

**The cursor's ledger is the session's ledger.** `TemporalCursor::of` takes the coordinate from
`active()` and the ledger from `historical::ledger()`, which answers whenever any temporal evidence
is installed, present or past. It is the same ledger either way — `Active::ledger_handle` and
`historical::ledger()` return the same handle — so this removes a condition rather than adding a
source.

## Consequences

- The prompt, the map's HUD, the paused marker and the gap frame are one wall clock in the session's
  own zone. A `TZ` a person set is honoured everywhere or nowhere, which is the only state in which
  a reader can trust either.
- `[` and `]` work in a live paused map, which is what §18.2 and §18.4 describe. They still answer
  "no significant event that way" where the ledger holds none — an honest refusal rather than a
  structural one, and the cursor does not move.
- Nothing about *what* is rendered changed, so `crates/ono-temporal-render/tests/` and
  `crates/ono-cli/tests/spatial_temporal_view.rs` are untouched and green.
- Encoded by `crates/ono-cli/tests/temporal_view.rs`, whose eight tests drive the view over a real
  terminal, and which compares the two markers rather than the two instants precisely because the
  instants used to disagree.

## Alternatives considered

- **Give the cursor its own zone lookup.** Rejected: a second place that decides what time it is, is
  how the first disagreement happened. §39.2 hands a renderer its settings rather than letting it
  read them, and one builder is what that means in practice.
- **Leave the live cursor without a ledger and disable `[`/`]` explicitly.** Rejected: it would make
  a specified capability unreachable and call it a design. §18.4 names no precondition beyond a view
  and events behind it.
