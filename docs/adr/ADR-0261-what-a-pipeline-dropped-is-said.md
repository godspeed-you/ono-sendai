# ADR-0261: What a pipeline dropped is said, not only counted

- Status: accepted
- Date: 2026-08-29
- Spec refs: §10.5, §13.3, §16.5, §35.3; ADR-0014, ADR-0029
- Decided by: agent (autonomous, `close-data`)

## Context

ADR-0014 requires a pipeline to count what it drops rather than dropping it silently, "so a user
who is surprised by a row count has somewhere to look that is not the source code".
`ono_pipeline::Diagnostics` counts two things: a row `where` excluded because its condition
evaluated to *unknown* rather than to false (spec §10.5), and a null an aggregate skipped rather
than counting as a zero (spec §35.3). `streaming_transforms.rs` and `blocking_transforms.rs` pin
both counters, its module doc says "`explain` and the REPL read it" — and nothing did. The
somewhere to look did not exist.

The cost is concrete. `get process | where cpu > 20` on a freshly started shell answers nothing,
because the first reading of a process has no previous sample to compute a share from and `cpu`
is null. The condition is unknown, not false; the rows are excluded correctly; and the user is
told nothing at all.

## Decision

**The shell says what the run dropped, on stderr, once per pipeline, only when it dropped
something.**

```text
$ ono -c 'from json | where a > 1 | count'
note: 1 value excluded because the condition could not be decided on it (spec §10.5)
VALUE
1
```

A second line reports skipped nulls where an aggregate skipped any. Both name the section that
explains why, because the reason is a language rule and not a wrinkle of this build.

It goes to **stderr**, so it never joins the answer: a script's stdout is unchanged, `to json`
still writes one document, and §4.6's guarantee that redirected output does not depend on who is
watching holds — the note is written whether or not anyone is watching, exactly like an error.

It is written for **the pipeline whose result the user is reading**, not for a captured one: a
`let` binding or an `each` block's body would otherwise print a note per iteration, which is the
noise ADR-0029 removed from a different code path for the same reason.

## Consequences

- `where` over a field the provider could not read says so, instead of answering an empty stream
  that looks like "nothing matched".
- A run that drops nothing prints nothing, so the note cannot become background noise.
- The counters have their consumer, and the module doc that promised one is true.

## Alternatives considered

- **Print it only at a terminal.** Rejected: it makes the shell's diagnostics depend on who is
  watching, which spec §4.6 exists to prevent, and a script's author is exactly the person who
  needs to know a condition was undecidable.
- **Put the counts in the answer** — a trailer row, a field on the result. Rejected: it is not
  data the query asked for, and it would break every consumer of `to json`.
- **A `--diagnostics` option.** Rejected on ADR-0029's reasoning: the default is what people live
  with, and an answer that silently omits rows is not fixed by an option nobody knows to write.
