# ADR-0173: A command that knows whether its values will be seen

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §23.3, §29.1, §29.3, §47, §52.1
- Decided by: agent S6 (autonomous)

## Context

§23.3 says `map` "MAY open a full-screen navigable view when terminal capability and user
configuration permit", and §52.1 makes that view a release criterion. §29.1 says the opposite
thing about the same command: `map --json`, `look --json`, `near`, `find` and `trail --json`
"MUST work in non-interactive mode", and §29.3 forbids a script from ever opening a picker.

Both are about one question a command cannot answer for itself: **is what I am about to produce
going to be looked at, or consumed?** `map | to json` runs in an interactive shell whose standard
output is a terminal, and must not take the screen. `map > file` must not either. `$(map)` must
not. `ono -c 'map'` must not, because a script is not a person. Testing `stdout().is_terminal()`
answers none of these: it is true in all four.

Only the evaluator knows. It plans every stage of a statement before running any of them, it
knows whether the statement has a redirection, whether the segment is the last one, and whether
the run is a capture.

## Decision

`ono_command::Invocation` carries one more fact: `displays()` — "the values this stage produces
will be shown to the user rather than consumed". `crate::native::run_native_segment` sets it for
the *last* stage of a foreground segment with no redirection that is not a capture, and for no
other stage.

Beside it, `crate::spatial::prompt::at_terminal()` answers a different and equally necessary
question: is this process the interactive loop, with a terminal on both standard streams? It is
set once by `repl::run`, before the first prompt.

A full-screen view opens only when **both** hold, and then `spatial.map.mode` decides:

| `spatial.map.mode` | when the view opens |
|---|---|
| `auto` (default) | `TERM` is not `dumb` |
| `text` | never |
| `fullscreen` | always — the user asked for it, so the `TERM` guess is not consulted |

Neither mode weakens the first two conditions: §29.1 holds whatever the configuration says.

The ambiguity picker of §27.2 asks `at_terminal()` alone, because `enter nginx` inside a
pipeline is still a person typing at a prompt; what §29.3 forbids is a *script* opening one.

## Consequences

- `map` in a pipeline, redirected, captured, backgrounded or in `ono -c` renders as text or JSON,
  which is exactly §29.1's list.
- The rule is one line in one place, and any later full-screen surface (a live `watch` view, an
  inspector) asks the same two questions rather than inventing a third.
- `Invocation::displays` is false by default, so every existing implementation and every test
  helper that builds an invocation is unaffected.
- A shell that one day runs stages concurrently must re-derive "the last stage"; the field is the
  place to change, not the commands.

## Alternatives considered

- **Decide in the sink.** `crate::sink::Sink` already knows it is writing to a terminal, and it
  is already where `ono.spatial-map/1` becomes text. Rejected: the view has to move the current
  place and re-ask the providers, and the sink has neither the spatial session nor a provider
  registry — it renders values and must keep doing only that.
- **`stdout().is_terminal()` inside the command.** Rejected: wrong for all four cases above.
- **An environment variable or a global set by the evaluator.** Rejected: a global would be read
  by a stage that is not the last one, which is the bug this decision exists to prevent.
