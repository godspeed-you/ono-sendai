# ADR-0774: A coordinate in the future is refused, and an unresolvable one is not the epoch

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §2.1, §4.2, §4.4, §12.1, §25.6, §34; ADR-0622
- Decided by: agent (autonomous)

## Context

An independent review reproduced two defects in `TimeSelector::resolve`, both of which produced a
session that looked like it was working.

`at 23:59` typed at eight in the morning resolved to today at 23:59 — sixteen hours ahead. The
session then reported `@23:59:00 [PAST?]`, reconstructed nothing (correctly, there being no
evidence for a time that has not happened), and refused every mutation with
`temporal.read_only`. Every one of those behaviours is right for a historical coordinate and
none of them is right here, because the coordinate is not historical. §4.4 refuses a relative
future selector by name and ADR-0622 implemented that; the absolute, local-date-time and
local-time forms went unchecked, and `now` was a parameter already in hand.

Separately, `Offset::to_timestamp` is fallible — a wall time near the end of jiff's representable
range leaves it in any negative-offset zone — and all four arms of the local resolution mapped
the failure to `Timestamp::UNIX_EPOCH`. `at 9999-12-31 23:00:00` in `America/New_York` therefore
resolved to 1970-01-01 and was reported as a successfully resolved historical coordinate.

## Decision

### 1. A resolved instant after `now` is `temporal.invalid_time`, for every form

`resolve` computes the instant as before and then refuses it if it lies after `now`. §4.4's rule
is stated for the relative form; the reason it gives is about historical context and applies to
every spelling of a coordinate. §2.1 requires Ono to be able to say whether a query is evaluated
at `now` or at a historical coordinate, and a future instant is neither.

The refusal quotes the selector back as it was written, so `at 23:59` says `23:59` rather than
the instant it computed — a user who mistyped a time reads their own words.

`TimeResolution::Ambiguous` and `Skipped` are unaffected: both are refusals already, and §25.6's
disambiguation is a separate question from whether the disambiguated instant is in the past.

### 2. A wall time no instant answers to is refused, not substituted

`resolve_local` returns `Option<TimeResolution>`, and `None` becomes `temporal.invalid_time`.
§12.1 requires an invalid selector to leave the session's coordinate exactly where it was, and a
substituted instant is the one answer that cannot: it moves the session somewhere real-looking
and wrong. The three surviving fallbacks — a fold or a gap where only one of the two offsets
applies — still resolve, because there one instant genuinely answers.

## Consequences

- Three tests, each of which fails against the previous code: a local time later today, an
  absolute instant next year in two spellings, and a wall time at the end of the range in a
  negative-offset zone.
- `at` can no longer be used to look at the future, which was never a feature and read as one.
- A session that asks for an instant a fraction of a second ahead — a clock read twice across a
  command — is refused rather than admitted. That is the correct side to err on: §12.3's refusal
  names what each source can reach, so the user learns something either way.

## Alternatives considered

- **Clamping a future coordinate to `now`.** It answers a question the user did not ask, and
  §2.1's requirement is that Ono can *say* which coordinate it is at. Silently moving one is the
  opposite of saying.
- **Refusing only in `at` and allowing `--at`.** They are one code path by §4.5's own
  requirement, and two rules for one engine is how the second historical code path §4.5 forbids
  starts.
