# ADR-0946: Completion offers the session's functions and aliases from a live view

- Status: accepted
- Date: 2026-10-07
- Spec refs: §15.1, §19.3, §6.5; ADR-0011, ADR-0070, ADR-0945, ADR-0951
- Decided by: agent (autonomous)

## Context

Steps 2 and 3 of the resolution order (ADR-0011) are the user's functions and aliases, and
completion never offered either (issue #223, part 2): `CandidateKind` had no kind for them and
the prompt's completer, built once when the REPL starts, had no way to see a name defined after
that. Since ADR-0951 a function stands at any stage position, so it is a candidate after a pipe
too.

## Decision

1. `ono_command::CandidateKind` gains `Function` and `Alias` (`function`, `alias` in
   `ono.completion/1`).
2. The session publishes its **outermost** scope's functions and aliases into a shared, locked
   view (`Session::shared_definitions`), republished by `Session::define` whenever it defines at
   that scope. The prompt only ever stands there, so names a function body defines for itself
   never leak into completion; a background job builds a session of its own with its own view.
3. The completer holds the view and, at every head position — the start of a line and after a
   `|` — offers the names that begin with the word, before the commands on `PATH`: a function
   with its declaration signature as its doc (`fn top(limit)`), an alias with its expansion.
4. `ono --complete` (ADR-0945) sees the same names: those the configuration defines.

## Consequences

- A function or alias typed at the prompt a moment ago completes on the next line.
- Tests: `crates/ono-cli/tests/completion.rs::should_complete_a_function_defined_after_the_prompt_started`
  (PTY), `crates/ono-cli/tests/complete_surface.rs` (function and alias with their docs, a
  function after a pipe); acceptance case `384-functions-and-aliases-complete.case`.

## Alternatives considered

- **Rebuild the completer after every line.** Rescans `PATH` and rebuilds the provider completer
  per prompt for two maps that rarely change.
- **Give the completer a reference to the session.** The session is mutably borrowed by the
  evaluator while the editor owns the completer.
