# ADR-0685: A cursor in a gap draws the gap, and no key uncovers the map

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §18.6, §18.5, §7.5, §55.5; v0.4 §23.3
- Decided by: agent (autonomous)

## Context

§18.6: "If the user steps into a gap, the view MUST display the gap explicitly", and "the map MUST
NOT continue showing the last state with a silently advancing timestamp". §18.5 forbids
interpolated frames: "semantic state changes only at supported event positions". §55.5 makes a gap
that is not represented as one a trust failure rather than a cosmetic one.

The map view already has an overlay mechanism for the `?` table and the `i` detail panel. Both are
dismissed by any key, which is right for them and wrong here: the state behind a gap frame is a
state the cursor's instant has no evidence for, so uncovering it on a key press is exactly what
§18.6 forbids.

## Decision

**A gap is view state, not an overlay.**

`MapView::set_gap(Some(lines))` replaces the body of the frame with those lines and keeps them
there through every key press, every focus move and every resize. `set_gap(None)` is the only thing
that restores the drawing, and the shell calls it when the cursor moves to an instant the coverage
supports. `MapView::in_gap()` reports the state.

The lines are `ono_temporal_render::gap_frame` over the `ono.temporal-gap/1` record of the gap the
cursor is standing in, so the wording is §18.6's own — the interval, the reason, and the last
instant anything was actually known — and the shell composes none of it.

The gap the cursor is in is found by `TemporalCursor::gap_in`, which scans the reconstruction's own
`gaps()` for the interval containing the cursor. So a gap exists in the view exactly where the
coverage composition says there is one; the view invents none and hides none.

## Consequences

- A rewind into an uncovered stretch shows `HISTORY GAP`, the interval, the reason and the last
  supported instant, and shows nothing else. There is no frame in which a stale topology is drawn
  under a moving clock.
- §18.5 needs no separate enforcement in the view: the view has no notion of an intermediate frame
  at all. It draws the reconstruction at the cursor's instant, and the cursor only ever moves to an
  instant a key asked for.
- `crates/ono-spatial-render/tests/view.rs` encodes the persistence: a key press while a gap frame
  is up leaves the gap up, and clearing it brings the drawing back.

## Alternatives considered

- **Reusing the detail overlay.** Rejected: any key dismisses it, and the frame underneath is the
  one §18.6 names as the failure.
- **Drawing the gap as a banner above the map.** Rejected for the same reason — the stale topology
  would still be on the screen, which is what "MUST NOT continue showing the last state" rules out.
