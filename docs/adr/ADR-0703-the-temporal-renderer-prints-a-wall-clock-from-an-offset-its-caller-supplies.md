# ADR-0703: The temporal renderer prints a wall clock from an offset its caller supplies

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §25.2, §25.3, §39.2, §39.3, §45.2; v0.2 §50; ADR-0612
- Decided by: agent (autonomous)

## Context

§39.3 forbids a renderer from calling a provider, and the dependency graph is how that is
enforced: `ono-temporal-render` depends on `ono-value` and nothing else, and
`docs/contracts/hardening/module_architecture.yaml` places it in the `runtime` layer.

Every line this crate draws starts with a wall clock. §25.2 stores instants in UTC; §25.3 says
"Interactive display defaults to the user's current configured timezone". The obvious way to honour
that is `jiff::tz::TimeZone`, which is what `ono-render` takes — and `jiff` is a leaf value library
rather than a layer above this one, so adding it would break no rule the architecture check
enforces.

It would break the sentence the interface contract states about this crate, though, and the value
of that sentence is that it is checkable by reading one line of one manifest.

## Decision

**1. The manifest stays `ono-value` and nothing else.** The rule is worth more as an invariant
somebody can verify at a glance than as a convenience.

**2. Time of day is computed, not formatted by a calendar library.** An instant reaches the crate
as `ono_value::Value::Timestamp`, whose nanosecond reading is available through the public value
model. Every clock this crate prints is a time of day, so the arithmetic is a shift and two
remainders and needs no calendar:

```text
shifted = instant + utc_offset
day     = shifted / 1e9 mod 86400
```

`div_euclid`/`rem_euclid` keep the remainder positive, so an instant before the epoch floors the
same way any other does.

**3. The zone arrives as data.** `RenderOptions::utc_offset` is an `ono_value::Duration` the caller
sets from the session zone. §39.2 applies the same discipline to the clock in the pure crates: time
arrives as a parameter and nobody reads the system. The renderer therefore stays a pure function of
its record, its width and its options, which is what makes output deterministic when it is
redirected (v0.2 §50).

**4. Three precisions, each where the specification uses it.** `HH:MM` for the window ends of
§19.2, `HH:MM:SS` for the coordinate of §4.6 and the gap boundaries of §18.6, `HH:MM:SS.mmm` for an
event row (§11.5).

## Consequences

- A caller that passes no offset gets UTC, which is honest and wrong for nobody: the default is the
  storage zone rather than an invented local one.
- A rendering that needed a date — a window spanning midnight, a row from yesterday — cannot have
  one from this crate. No layout in §11, §16, §18 or §19 asks for a date; a layout that starts to
  will need either a date field on the record or `jiff` here, and this ADR is where that trade is
  recorded.
- A daylight-saving transition inside a rendered window shifts every row by one offset, because the
  offset is a scalar rather than a zone. The instants themselves are untouched, and `inspect event`
  is where §25.3 puts canonical UTC.

## Alternatives considered

- **`jiff` as a second dependency, and a `TimeZone` parameter.** Rejected for the manifest rule,
  and recorded here because it is the change to make if a date is ever needed. It is a one-line
  manifest edit and a two-line code change, and nothing in the layering forbids it.
- **Rendering the RFC 3339 text and slicing it.** Rejected: it is the same arithmetic with a
  string in the middle, and it cannot apply an offset at all.
- **Passing pre-formatted strings in the record.** Rejected: it puts presentation in the query path
  and makes `timeline --json` carry a rendering.
