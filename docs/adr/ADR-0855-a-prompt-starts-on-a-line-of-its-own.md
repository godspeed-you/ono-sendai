# ADR-0855: A prompt starts on a line of its own

- Status: accepted
- Date: 2026-09-12
- Spec refs: v0.6.1 §8, §27; v0.2 §4.2, §29.3; ADR-0013
- Decided by: agent (autonomous)

## Context

Issue #132: `printf abc` — or `curl ifconfig.me` — printed its output and the next prompt erased
it at once. A frame of the line editor starts with `ESC [ 1 G`, `ESC [ J`: column one, clear to the
end of the screen. After output without a trailing newline the cursor still stands on the row
that holds it, so that row was cleared with the old frame. The shell cannot know where the cursor
is on its own, because a foreground program writes to the terminal directly.

v0.6.1 §8 requires the output to stay visible, no blank line after output that did end in a
newline, and correct behaviour for empty output, repeated commands and redraws; it leaves the
mechanism open.

## Decision

1. Before the **first** frame of each prompt the REPL asks the terminal for the cursor position
   (`ESC [ 6 n`, through crossterm). Anywhere but the first column, the prompt moves to a new line
   (`\r\n`) first. In the first column nothing is written, so newline-terminated and empty output
   gain no blank line.
2. Redraws of the same prompt — every key, completion, a resize — never ask: the editor itself
   put the cursor where it is.
3. No marker is drawn. zsh's inverse `%` says *the output had no newline*; v0.6.1 §8 asks only
   that the output stay visible, and a marker would be one more thing on every such screen.
4. The question is asked only when stdout is a terminal and `TERM` is not `dumb`, which would
   print the question. A terminal that leaves it unanswered costs crossterm's bounded two-second
   wait once; the session does not ask again, and without an answer nothing is written — the
   behaviour before this decision, rather than a guess.

## Consequences

- `Renderer::start_prompt` and `cursor_column` in `ono-editor` carry the rule; `read_line` in
  `crates/ono-cli/src/repl.rs` calls them once per prompt.
- A pseudo-terminal without an emulator — the test suites, the acceptance container — does not
  answer, so each such session pays the wait once at its first prompt.
- Tests: `crates/ono-cli/tests/unterminated_output.rs`, which replays the shell's bytes through a
  minimal screen model and answers the report from the model's cursor.

## Alternatives considered

- zsh's `PROMPT_SP` trick without a query — marker, then a terminal width of spaces, then `\r` —
  depends on the terminal's deferred-wrap behaviour and on the width being exact, and it cannot be
  checked from the byte stream without emulating that wrap.
- An unconditional `\r\n` before every prompt — a blank line after every correctly terminated
  output, which v0.6.1 §8 forbids.
- Reading the report from the terminal directly with a shorter timeout — keys typed ahead during
  the wait would be read and lost; crossterm keeps them.
