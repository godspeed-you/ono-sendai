# ADR-0069: `let` and `( … )` capture a pipeline's value — materialised, not replayed

- Status: accepted
- Date: 2026-08-27
- Spec refs: §6.2, §10.5, §12.3, §18.3, §19.2, §20.2, §26.2; ADR-0009, ADR-0019, ADR-0028
- Decided by: agent (autonomous)

## Context

Spec §19.2 binds a pipeline to a name — `let hot = get process | where cpu > 50` — and then
consumes it: `$hot | select pid name cpu`. It says the laziness of the binding "MUST be
explicit" and suggests binding "a lazy pipeline object only if it can be replayed; otherwise the
language may require `collect`". ADR-0009's grammar gives the same question a second face:
`paren_value` is "the value of a nested pipeline", and a double-quoted string interpolates
`$( … )`. Until this decision the evaluator ran the pipeline as a statement — rendering its
table at the terminal — and bound its *exit status*, so `let n = get process | count; $n`
printed the count and then `0`. That is a binding of the wrong thing.

## Decision

1. **A bound pipeline is materialised.** `let x = <pipeline>` and `( <pipeline> )` run the
   pipeline once, to completion, with everything it would have shown collected instead of shown:
   a native pipeline's values as values; a serializer's document as one string; a program's
   stdout as one string with its trailing newlines removed, exactly as `$( … )` has meant in
   every shell. Nothing is rendered and nothing is retained for `@-1`, because nothing was shown.
2. **One value is that value; several are a list; none is the empty list.** A list splices back
   into several values when it starts a pipeline (ADR-0019), so `let hot = get process | where …`
   followed by `$hot | select pid` streams the records again, and `let n = … | count` binds the
   number itself rather than a one-element list. An empty result is `[]`, not `null`: the
   pipeline is *known* to have produced nothing, and spec §10.5 reserves `null` for what is not
   known.
3. **No lazy pipeline objects.** The binding never holds an unrun pipeline. A replayable lazy
   stream would have to re-run the producer on every use — `$hot` would answer a different set
   of processes each time — and the spec's own example uses the binding as a snapshot to look at
   again. `collect` is therefore never required, because the binding already is one.
4. **An unbounded pipeline cannot be bound.** `let x = watch process` is refused with
   `stream.unbounded_operation` (E0801) and the help names `take` and a serializer, as spec
   §18.3 already does for a live stream with nobody watching it. A live view has no value.
5. **The status is the pipeline's.** `let x = <pipeline>` sets `$?` to the pipeline's exit
   status, so `let out = some-tool; if $? != 0 { … }` works. A bare value — `let n = 3` — is
   success.
6. **Word arguments and interpolation use the value's text.** `echo (get process | count)`
   hands the count's canonical text to `echo`; `"$(echo hi)"` splices the captured text.
   Objects stay objects until they meet a program, which is where spec §12.3 puts the boundary.

## Consequences

Easy: the specification's own `let hot = …; $hot | select …` example runs; `( … )` is a value
everywhere ADR-0009 allows it; a script can hold a result and ask several questions of it.

Hard: a binding of a large result holds it all in memory. That is the explicit choice spec
§19.2 asks for, and the same choice `@-1` already makes (spec §20.2); a user who wants a stream
writes the pipeline where the stream is consumed.

Encoded by: `crates/ono-cli/tests/language_missing.rs` (the `let` and `paren_value` cases) and
the acceptance case `035-scripting-language`.

## Alternatives considered

- **Bind a lazy, replayable pipeline object** — rejected: replay means re-running the producer,
  so the binding is not a snapshot; and detecting "can be replayed" needs a judgement about
  every producer that no contract makes today.
- **Always bind a list** — rejected: `let n = … | count; $n * 2` would fail on a one-element
  list, and every use of a single value would need an index.
- **Keep binding the exit status** — rejected: it is what a `let` cannot mean, and every test in
  the RED suite says so.
