# ADR-0650: A field observed before the requested instant is unknown unless coverage carries it

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §1.3, §8.2, §8.4, §9.1, §9.2, §9.3, §35.3; v0.2 §10.5
- Decided by: agent (autonomous)

## Context

§9.2 states the refusal and the shape of the answer in one place. Between a reading of `running`
at 12:00 and one of `failed` at 12:10, "Ono MUST NOT claim the state at 12:05 unless evidence
supports it". It may report `state: unknown in interval 12:00..12:10`, with the last and next
readings named. It adds the one thing that changes the answer: "if an exhaustive event stream
proves no transition occurred until 12:08, the stronger coverage may narrow the uncertainty".

§9.3 permits `valid_from` and `valid_until` "when evidence supports a continuous state interval",
and forbids the obvious shortcut: "these intervals MUST derive from source semantics, not from
guessed midpoint interpolation".

A reconstruction has readings at instants and coverage over intervals. The question is which of
the two decides what a field holds at an instant that is neither.

## Decision

**Coverage decides; a reading alone never carries forward.**

For one field at `T`, with the strongest reading at or before `T` at `t`:

- `t == T` — the reading is the answer, with no interval. A point sample "can support state at
  that point" (§8.4) and supports nothing either side of it, so `valid_from` and `valid_until`
  are both null rather than both `t`.
- a provider's own historical answer whose declared validity interval contains `T` — the answer
  is the answer, and `valid_from`/`valid_until` are the provider's own bounds. This is §9.3's
  "source semantics" literally: the interval was stated by the source, not computed here.
- `t < T` and one source declared **complete** coverage of that field's capability over an
  interval containing `[t, T]` — the reading holds. `valid_from` is `t`, the instant of the
  reading. `valid_until` is the end of that complete interval, or the next reading's instant
  where one arrives first. Both ends are an observation or a source's own declaration.
- anything else — `FieldKnowledge::UnknownInInterval { last, next }`, carrying both readings with
  their instants and their sources. `to_value()` is `Value::Null`, which reads back as
  `FieldAccess::Unknown` (v0.2 §10.5) and never as a zero or an empty string.

`partial` coverage does not carry a value. §8.3 defines partial as evidence existing where
"absence cannot be interpreted as proof of non-existence"; a field is the same argument one level
down, because a partial source is precisely one that may have missed the transition.

The same rule decides existence and relation presence, keyed on `<type>.existence` and
`relation:<name>` instead of `<type>.<field>` — an object observed at `t` is present at `T` only
where coverage was complete in between, and absent likewise. That is what makes ADR-0651's third
answer necessary rather than decorative.

## Consequences

- Reconstruction is honest and often unhelpful, which is §1.3 as a property of the code: "history
  is not omniscience". A session with no declared coverage answers `unknown` for everything
  between its readings, and that is the correct answer.
- The recorder earns useful reconstructions by declaring coverage. A source that claims
  `complete` for a capability it samples is the one way to make this crate lie, which is why
  §21.5 forbids advertising `exhaustive_events` "merely because events usually arrive".
- Coverage is looked up per field, from the same intervals the summary is composed from, so a
  window covered completely for `service.state` and not at all for `process.existence` answers
  both questions separately (§8.5).
- Encoded in `crates/ono-temporal-reconstruct/tests/reconstruction.rs`.

## Alternatives considered

- **Carry the last reading forward.** The single most useful thing to do and the thing §9.2
  forbids by name. It also makes every historical map look complete, which §55's failure modes
  describe as the way this feature goes wrong.
- **Carry it forward under `partial` coverage.** Weaker but the same defect: partial means a
  transition may have been missed, so a value carried across partial coverage is a guess with a
  label on it.
- **Interpolate the midpoint of two readings, or split the interval.** §9.3 forbids it.
- **Report `valid_from`/`valid_until` for a point sample as `t..t`.** A degenerate interval reads
  as an interval. Null says the source supports none, which is true.
