# ADR-0176: A live map that polls, and says so

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §25.1, §25.2, §25.3, §25.4, §29.1, §39.4, §43.4, §47
- Decided by: agent S6 (autonomous)

## Context

§43.4 requires a PTY test proving "Ctrl-C exits live map without killing the shell", and the test
suite S6 must turn green contains it. §50 gives the live map to S7, whose deliverable is the event
aggregator, the snapshot diff and the change highlighting. S6 therefore has to answer what
`map --live` *is* before the event source exists, without faking one.

§25.1 answers it: "`map --live` MUST subscribe to available provider events **and/or explicit
polling sources**." §25.3 adds the honesty requirement: a live view "MUST expose whether updates
are event-driven, polled, cached, stale or partial". §25.2 forbids motion that is not a state
change.

## Decision

`map --live` opens the full-screen view with a **polling source**, and the header says `live
polled` — §25.3's word, never `event driven`. Every second the shell asks
`crate::spatial::map::projection` for the same map again and hands it to the view, which keeps the
cursor on the node it was on. `w` turns the subscription on and off inside the view;
`spatial.map.live = true` turns it on for every map (§47).

Where there is no terminal to draw into, `map --live` is refused with `spatial.unsupported` and a
help line pointing at `map --json`. §29.1's list does not contain `map --live`, and a live view
written to a pipe would be neither live nor a view.

**A frame identical to the one on the screen is not written at all.** That is how §39.4's
`reduced_motion` requirement is met: this renderer draws no transition animation of any kind
(§25.2 forbids decorative motion outright), and it additionally refuses to repaint an unchanged
screen, so nothing moves unless the machine moved. `spatial.reduced_motion` therefore has nothing
to disable in this build; it is declared, inspectable, and it becomes the switch for S7's change
highlighting when there is a change to highlight.

**Ctrl-C is bound to the same action as Esc: close the view.** §43.4 requires it to leave the view
and not the session. Inside the view the terminal is in raw mode, so Ctrl-C arrives as a key press
rather than a signal, and the guards give the screen and the line discipline back on the way out.

## Consequences

- A user watching `map --live` sees real change, at a one-second resolution, and is told that it
  is polled. Nothing on the screen suggests a subscription that does not exist.
- Every poll is a full re-observation of the horizon, which is the same cost as pressing `r`. S7
  replaces it with an event subscription and a diff; the view does not change, only what feeds it.
- `map --live` is declared in `docs/spec/commands/spatial.yaml`, so `spec-check` and the generated
  reference carry it.

## Spec deviation

- Section: v0.4 §25.1
- Text: "It may visualize: node appearance/removal; state transitions; edge appearance/removal;
  landmark appearance/removal; metric changes when relevant to landmark status; replacement of
  lifetime objects such as restarted processes."
- Instead: this build visualises the *current* topology as it changes, and marks none of those six
  events as an event. They are differences between two observations (§25.4), which is S7's
  deliverable; `MapEdge.changed` is already a field and is null until then.
- Why: the list is permissive ("may visualize"), and inventing a change marker without a
  comparison behind it would be exactly the fabricated state §2.17 and §25.3 forbid.

## Alternatives considered

- **Refuse `map --live` entirely until S7.** Rejected: §25.1 names polling as a source in its own
  right, and §43.4 requires the PTY behaviour now. A refusal would have left a normative test
  ignored for a capability the specification already permits.
- **Poll faster.** Rejected: §34's budgets and §25.2's "no busy screen" both point at the slowest
  interval a person still reads as live.
