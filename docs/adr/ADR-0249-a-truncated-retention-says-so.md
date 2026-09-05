# ADR-0249: A truncated retention says so

- Status: accepted
- Date: 2026-08-29
- Spec refs: §20.2 ("Ono-Sendai MAY retain **bounded** recent structured results"), §6.4, §16.2,
  §33.2; ADR-0069, ADR-0072 §4
- Decided by: agent (autonomous)

## Context

`Session::retain_result` bounds retention twice — sixteen results, ten thousand values each — and
did both silently. The second bound is the one a user can walk into: a pipeline that shows twelve
thousand rows and is then reused as `@-1` yields ten thousand, and nothing anywhere connects the
two numbers. The screen said one thing, the reuse says another, and the shell said nothing.

Spec §20.2 is a MAY with one adjective — *bounded* — and no word about announcing the bound.
Deciding it is this ADR's job (AGENTS.md §5.1).

The retention limits had no test at all before this increment, which is how a silent truncation
survives: nothing drove either bound, so nobody read the code that applies them.

## Decision

**`Session::retain_result` returns how many values it could not keep, and every caller shows it.**

The message is a notice, not an error: the pipeline succeeded and its output is correct. It goes
to standard error, because spec §33.2 keeps standard output for the answer and this is the shell
talking about the answer — so a redirected or piped run is byte-identical to what it was, and a
person at a terminal is told.

```
ono: retained the first 10000 of 10005 values for reuse; `@-1` sees 10000 (spec §20.2)
```

The moment of the notice is the retention, not the later reuse. Saying it when `@-1` is reached
would be a warning about something that happened long enough ago that the user has moved on, and
would repeat once per reuse.

The two bounds become `session::RETAINED_RESULTS` and `session::RETAINED_VALUES` rather than
constants local to the function, because `native::stage_scope` had already written `1..=16` by
hand — the same bound spelled twice, which is how the two drift apart.

`Reporter::notice` is the new surface: dim, prefixed like every other line the shell writes about
itself, and sanitised for the same reason an error message is (ADR-0245 T1).

## Consequences

`@-1` can no longer be quietly shorter than what was shown. The notice fires from the four places
that retain a result — the pipeline sink, both exits of `view`, and the context/jobs listing — so
a truncation is announced however the values reached the screen.

A run that stays inside the bound is unchanged, including its stderr, so no existing case or test
sees anything new.

Encoded by `crates/ono-cli/tests/native.rs::should_say_so_when_a_result_is_too_large_to_retain_whole`
and `::should_evict_the_oldest_retained_result_when_the_seventeenth_arrives`.

## Alternatives considered

- **Raising or removing the value bound.** Rejected: §20.2 says bounded, and a `get file /` that
  printed a million rows must not pin a million values for the session's lifetime.
- **Refusing to retain a result that does not fit.** Rejected: the first ten thousand rows are
  usually exactly what the user wants to reuse, and losing them entirely to protect them from
  being partial helps nobody.
- **Putting the notice on stdout beside the rows**, the way the renderer's `... N more` line
  sits under a truncated table. Rejected: `... N more` is about what was *drawn* and belongs to
  the drawing; this is about what the shell kept, and it must not enter a redirected file
  (spec §33.2, case `034`).
