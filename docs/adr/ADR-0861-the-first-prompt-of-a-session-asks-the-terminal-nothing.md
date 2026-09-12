# ADR-0861: The first prompt of a session asks the terminal nothing

- Status: accepted
- Date: 2026-09-12
- Spec refs: spec §34 (startup budget); v0.6.1 §8, §31; ADR-0855 (partly superseded here)
- Decided by: agent (autonomous)

## Context

ADR-0855 fixed issue #132 by asking the terminal for the cursor column (`ESC [ 6 n`) before the
first frame of **every** prompt, including the first prompt of a session. Its consequences named
the cost — a terminal that does not answer makes crossterm wait two seconds, once — but not where
that cost lands: at startup, inside the §34 budget.

The CI run for v0.6.1 (34701566431) proved it. Case `100-spatial-performance-budgets` measures
interactive startup to a usable prompt through a pseudo-terminal with no emulator behind it, and
`startup-to-prompt` failed its 150 ms budget: every session waited for an answer that never came.
A terminal that does answer still pays a round trip before the first prompt appears.

## Decision

The first prompt of a session does not ask. Until the first command runs, everything written to
the terminal is the shell's own — the identity line (`writeln!`) and the startup horizon (`look`,
rendered line by line) — so the cursor is known to stand in the first column, and the question
could only confirm it. From the prompt after the first command on, rule 1 of ADR-0855 applies
unchanged: the question is asked before the first frame of each prompt, a terminal that leaves it
unanswered is not asked again, and `TERM=dumb` is never asked.

## Consequences

- Startup is back to what it was before #132: no round trip, and no wait at a silent terminal.
- A silent terminal now pays its one bounded wait at the prompt after the first command instead
  of at startup; the output of that first command, if it ended without a newline, is then drawn
  over as before #132, because nothing is known — the same fallback ADR-0855 rule 4 describes.
- A shell started right after another program's unterminated output (`printf abc; ono`) keeps
  that output: the identity line is written after it on the same row and nothing clears that row.
- Encoded by `crates/ono-cli/tests/unterminated_output.rs::should_reach_the_first_prompt_without_asking_where_the_cursor_is`
  and by case `100-spatial-performance-budgets`.

## Alternatives considered

- **A shorter timeout for the report.** crossterm's two seconds are fixed, and reading the
  report without it would read and lose keys typed ahead (ADR-0855's third alternative).
- **Raise the startup budget.** §34 fixes it, and the question it would pay for answers nothing
  the shell does not already know.
