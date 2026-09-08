# ADR-0622: A time selector resolves against parameters and a DST fold is two answers

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.5 §4.4, §12.2, §25.2, §25.3, §25.6, §34, §39.2, §47.1
- Decided by: agent (autonomous)

## Context

§4.4 defines five selector forms and two rules about them: "relative future selectors are invalid
for historical context", and "ambiguous local times during DST transitions MUST require
disambiguation by offset when both instants exist". §25.6 repeats the second.

§39.2 constrains how any of it may be implemented: "pure comparison/query logic SHOULD accept time
as parameters rather than call the system clock directly. This preserves deterministic tests."

A local wall time in a zone with daylight saving has three possible outcomes, not one: it maps to
one instant, to two (a fold), or to none (a gap). A function returning `Result<Timestamp, _>`
cannot express the middle case without picking one instant, and picking is precisely what §4.4
forbids.

## Decision

`TimeSelector::resolve(&self, zone, now, anchors)` takes the zone, the instant that stands for
now, and a ledger to resolve `event @e42` through. **Nothing in `ono-temporal-core` calls
`Timestamp::now()` or reads the system zone.** The recorder and the CLI read the clock; every
crate below them is handed the reading.

The return type has three arms:

```rust
enum TimeResolution {
    Resolved(Timestamp),
    Ambiguous { earlier: Timestamp, later: Timestamp },
    Skipped { gap_from: Timestamp, gap_until: Timestamp },
}
```

`Ambiguous` names both instants, which is what §4.4's "disambiguation by offset" needs: a user
cannot choose an offset they have not been shown. `error::ambiguous_local_time` turns it into
`temporal.invalid_time` (§34 E1301) carrying both instants in the message and in the metadata.

`Skipped` names the two instants the requested wall time maps to under the offsets on either side
of the jump. The wall time falls between them and belongs to neither, so the pair brackets the span
that swallowed it, which is what a message like "the local clock jumped between 00:30Z and 01:30Z"
needs. `error::skipped_local_time` carries them.

Ambiguity is detected with `jiff`'s `TimeZone::to_ambiguous_timestamp`, which reports `Fold` and
`Gap` natively with the offsets on both sides. Nothing here reimplements a transition search.

A **relative** selector naming the future is refused twice: at `parse`, so `at +10m` fails before
anything else happens, and again at `resolve`, because `TimeSelector::Relative` is a public variant
a caller can build. A bare `10m` parses as a positive duration and is refused with the same
message, which is more use than "unrecognised". Absolute and local forms in the future are *not*
refused: §4.4 restricts only relative ones, and `at 2026-08-31 12:17` on a machine whose clock is
behind is a legitimate question.

§4.4 defines no date-only selector, so `2026-08-31` is refused rather than read as midnight — a
time the user did not ask for.

## Consequences

- Every temporal test is deterministic: `resolve("-10m", now = 2026-08-31T12:17:14Z)` is the same
  answer on every machine in every zone, which is what §47.1's "time selector parsing" and DST
  cases need in order to be tests rather than weather reports.
- The DST cases can use a real zone with real transitions — `Europe/Berlin`, 2026-03-29 and
  2026-10-25 — instead of a synthetic one.
- A caller must handle three outcomes. That is the visible cost of §4.4's rule, and it is paid at
  the one place that resolves a selector.
- Storage stays UTC (§25.2) and display is the caller's zone (§25.3); this crate never renders one.
- Encoded in `crates/ono-temporal-core/tests/time_selector.rs`.

## Alternatives considered

- **Returning `Result<Timestamp, ErrorValue>` and refusing a fold outright.** Loses both instants,
  so the refusal cannot tell the user which offsets to choose between, and §4.4 asks for exactly
  that.
- **Picking the earlier instant on a fold, as `jiff`'s `compatible` disambiguation does.** A
  reasonable default for a calendar, and a silent wrong answer for an incident timeline: the two
  instants are an hour apart and the events in between are different events.
- **Reading the system zone inside `resolve`.** Fails §39.2 and makes every DST test depend on
  where the machine is.
