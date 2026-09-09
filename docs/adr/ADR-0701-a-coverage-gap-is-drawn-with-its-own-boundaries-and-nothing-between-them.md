# ADR-0701: A coverage gap is drawn with its own boundaries and nothing between them

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §7.5, §11.7, §18.5, §18.6, §45.1, §55.5; v0.2 §35.3
- Decided by: agent (autonomous)

## Context

§55.5 names a silent gap as a trust-destroying failure, and §11.7 makes the rule concrete:

> A gap MUST not be hidden simply because events exist on both sides.

§18.6 asks for the same fact in a different place — a view whose cursor steps into a gap — and adds
the prohibition that makes it necessary: "The map MUST NOT continue showing the last state with a
silently advancing timestamp." §45.1 forbids fake tape-scrubbing, random visual noise in historical
mode, invented frames between unsupported states and `DECRYPTING PAST...` by name.

`ono.temporal-gap/1` carries `from`, `until`, `reason`, `source`, `capability` and a nullable
`detail`. It carries no rendered text, which is right: a gap is a typed object and its wording is
presentation.

## Decision

**1. In a timeline, a gap is two lines placed by its own start instant.**

```text
12:20:00      ---- coverage gap: recorder offline 4m 12s ----
12:24:12
```

The gap is merged into the row sequence before the first event at or after its `from`, so it stands
between the events on either side of it. Nothing filters a gap by whether the interval looks busy;
`timeline` draws every entry of the record's `gaps` list.

**2. In a view, a gap is §18.6's frame.**

```text
HISTORY GAP
12:40:18 - 12:44:30
recorder disconnected

last supported state shown at 12:40:18
```

The frame states the interval, why nothing is known there, and the last instant anything was
actually known — which is `from` and never a clock reading.

**3. The words come from the record.** `detail` where the producer wrote one, else the closed
`reason` vocabulary of §7.5 with its underscores read as spaces. v0.2 §35.3 forbids inventing a
value for what is unknown, so a gap with neither reads `not recorded`, which is honest.

**4. No instant appears that the record did not state.** The only clocks in a gap frame are `from`,
`until` and `from` again. `crates/ono-temporal-render/tests/gap_frame.rs` scans the rendered text
for anything shaped like a wall clock and fails on a third value, which is how §18.5's "no
interpolated frame" is checked rather than asserted.

**5. Nothing is animated, because nothing can be.** Every function in the crate is a pure function
of a record, a width and its options, so there is no frame counter to advance and no clock to read.
A determinism test renders the same input twice and compares.

## Consequences

- A gap survives at 40 columns: the marker text is written so that `coverage gap` falls inside the
  first 33 characters of the line.
- `timeline` never needs to know what "material" means (§11.7). The producer decides which gaps are
  in the window; the renderer draws all of them. That keeps the judgement in the query path, where
  the coverage composition of §8.5 lives.
- The frame is the same shape whichever view opens it, so a gap reads the same in `timeline`, in
  the full-screen timeline and in a rewound map.

## Alternatives considered

- **Drawing the gap only when it exceeds a threshold.** Rejected: §11.7 sets no threshold, and a
  renderer-side threshold is exactly the silent gap of §55.5.
- **Filling the gap with a dimmed repeat of the last state.** Rejected by §18.6 and §45.1; it is
  the failure both sections name.
- **Reporting the gap once in a summary line rather than in place.** Rejected: a summary count
  cannot say *when* the hole is, and §11.7's example puts it between the rows.
