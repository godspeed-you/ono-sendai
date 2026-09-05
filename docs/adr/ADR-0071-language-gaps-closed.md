# ADR-0071: The remaining language gaps — `each` blocks, prefix assignment, keyless `sort`, `kill %N`, timestamps, string `+`

- Status: accepted
- Date: 2026-08-27
- Spec refs: §6.3, §6.4, §10.2, §18.1, §18.4, §19.4, §53, §54; `docs/contracts/language.yaml`
  (`builtin_functions`, `current_value`), `docs/contracts/commands/data.yaml` (`ono.data.each`,
  `ono.data.sort`); ADR-0009, ADR-0011, ADR-0019, ADR-0024, ADR-0069, ADR-0070
- Decided by: agent (autonomous)

## Context

The RED suite `crates/ono-cli/tests/language_missing.rs` names six small language features
that the specification or the contracts declare and the shell did not run. Each is a
one-paragraph decision; recording them separately would produce six ADRs whose context sections
all say "the contract promised it". They are decided here together, one section each, so that
the greppable list of decisions stays one entry per subject.

## Decisions

### 1. `each { … }` is run by the evaluator (spec §19.4)

A block bound to `each`'s `body` is not an expression the transform engine can evaluate: a
block holds statements, and a statement may run a command. The shell therefore runs `each`
itself when its body is a block. The stages before it run first and their values are collected
(as ADR-0069 collects a bound pipeline); for every value the block runs once, in a fresh
scope, with `@` bound to the value (the "explicit item token" §19.4 asks for — the same `@`
that names an interactive selection, which the block's binding shadows for its duration); the
block runs in the caller's output context (ADR-0070 point 3), so with stages after `each` its
results are captured and stream on, and with none they are shown as they arise. A block that
shows nothing contributes nothing, so `each { restart service @ }` yields the action results
and `each { echo @.pid }` prints one line per item. An expression body (`each score`) stays with
the transform engine, unchanged.

For a block's statement to be a value — `each { @ * 2 }` — the parser reads a stage whose head
is a value (`@`, `$x`, `( … )`) followed by an infix operator as one expression, by the same
lookahead ADR-0009 uses to tell `(ls -la)` from `(a - b)`. `$hot | select …` and `@-1 | count`
keep their meaning, because `|` is not an infix operator.

### 2. Prefix assignment scopes an environment variable to one pipeline (spec §54)

`NAME=value command …` sets `NAME` in the environment for the pipeline the command belongs to
and restores the previous state afterwards — the variable is unset again if it was unset, and
`get env NAME` after the pipeline finds nothing. Several assignments may precede the command;
the value is a word (tilde and variables expanded, ADR-0019) or a quoted string written
directly after the `=`. The assignment reaches external programs and native commands alike,
because both read the session's environment. An assignment with no command after it is
`resolve.command_not_found` whose help names `set env NAME = value` and `let`: Ono has two
explicit spellings for a lasting binding, and the Bash meaning of a bare `NAME=value` — a
shell variable that is not exported — is exactly the kind of implicit state spec §12.1 avoids.

### 3. `sort` without a key orders values by themselves

`ono.data.sort` declares `key` as its first selector, and `from json | sort` on a stream of
numbers or strings has an obvious meaning that the contract's "needs a key" refused. The
identity is the default key: with no key the values are ordered by their own comparison
(ADR-0031's rules), and a bare `desc` or `asc` with no other key is the direction, not a field
named `desc`. A field genuinely called `desc` is sorted with an explicit direction: `sort desc
asc`. The pre-flight field check of spec §11.3 treats the two direction words the same way.

### 4. `kill %N` is the shell's (spec §18.1, §18.4)

`%N` is a job specifier and never reaches `/usr/bin/kill`. `kill %N` sends `SIGTERM` to an
external job's process group, exactly as `fg`/`bg` address it; for a backgrounded native
pipeline (ADR-0024) it aborts the task driving the stream, which drops every receiver and
stops the producers. Either way the job leaves the table. `kill` with any other argument is
untouched: `kill process 1234` is the native verb, `kill 1234` the program.

### 5. Timestamps: `now()` and the RFC 3339 literal (spec §6.3, §10.2)

`now()` is the one builtin function `language.yaml` declares; it evaluates to the current
wall-clock instant as a `timestamp` value, in the shell's expressions and inside `where` alike.
A call to any other name is `resolve.command_not_found`, as before. An operand of the form
`YYYY-MM-DDTHH:MM[:SS[.fraction]](Z|±HH:MM)` is a timestamp literal — the RFC 3339 profile of
ISO 8601, the spelling `to json` already emits — recognised only in expression operand position,
so `2000-01-01T00:00:00Z` in `where modified > …` is one value and never `2000 - 01 - …` nor a
field path `T00`. A date without a time and zone is not a literal: `2000-01-01` would otherwise
be indistinguishable from subtraction, and the shell never guesses.

### 6. `+` concatenates two strings (spec §6.3 "string operations")

`"a" + "b"` is `"ab"`. Only two strings concatenate: a string and a number stay a
`type.mismatch`, because `"1" + 1` has two defensible answers and a silent one would be wrong
for half its users.

## Consequences

Each of the six is proven by its cases in `crates/ono-cli/tests/language_missing.rs`; the
parser change of §1 by `crates/ono-parser/tests/parse_statements.rs`; the lexer change of §5 by
`crates/ono-parser/tests/lexer.rs`; and the family as a whole by the acceptance case
`035-scripting-language`. `docs/contracts/grammar.ebnf` and `docs/contracts/language.yaml` gain the
`timestamp` literal and the `alias` statement of ADR-0070 in the same increments.

## Alternatives considered

- **Running `each` blocks inside the transform engine** — rejected: the engine has no session,
  no executor and no scope chain, and giving it one would move the evaluator into a library
  that is meant to stay pure.
- **Prefix assignment scoped to the whole statement, or leaking into the session** — rejected:
  §54 is muscle memory for "this one command, with this environment", and anything wider is a
  different feature with an explicit spelling already.
- **`sort` refusing a missing key** — rejected: it is the behaviour the RED suite found wanting,
  and `stream<any>` of scalars is exactly the case a key cannot name.
- **A `kill` builtin** — rejected: it would shadow the program and the native verb for every
  other argument; only the `%N` form is the shell's.
- **Accepting date-only literals** — rejected, see §5.
