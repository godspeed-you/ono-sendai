# ADR-0227: `--name=value` reads the same in expression mode as in words mode

- Status: accepted
- Date: 2026-08-29
- Spec refs: §6.2; ADR-0009, ADR-0032
- Decided by: agent (autonomous, `close-data`)

## Context

ADR-0032 gave expression-mode stages their long options — `--` followed immediately by an
identifier character is an option token, at every position — and paired each with the expression
that follows it. It left one spelling out, deliberately and temporarily: "The `--name=value`
spelling stays words-mode only for now; expression mode pairs the option with the expression
after it. The board carries the gap."

The gap is a syntax error where a user reasonably expects a value:

```text
from json | reduce $acc + @ --initial=10
Ono-Sendai-E0001 expected a value, found `=`
```

An option syntax that changes with the argument mode is exactly the rule ADR-0032 rejected in its
own alternatives — "an option syntax that changes per argument mode is a rule users would have to
memorise".

## Decision

**An `=` written against a long option is punctuation between the option and its value, in
expression mode as in words mode.** The parser consumes it and reads the value as an expression,
so `--initial=10`, `--initial=(1 + 2)` and `--initial 10` are the same argument, bound the same
way.

The shape of the token decides, as ADR-0009 requires. `=` must start exactly where the option
ends: `--initial = 10` is unchanged and still a syntax error, because `=` is not an operator in
this language and nothing makes it one at a distance. Nothing may follow the `=` but a value:
where the stage ends there, the option is written without one, as before.

## Consequences

- Every expression-mode option takes either spelling. `reduce $acc + @ --initial=0` seeds an
  empty fold, which is what makes `reduce` usable over a possibly-empty stream (ADR-0032).
- The value belongs to the option rather than standing beside it, so the stage carries one
  argument fewer — which is what binding already did with the spaced form.
- ADR-0032's cost list is unchanged: `--x` is still an option and not a double negation, and a
  spaced `- -x` is still a negation.

## Alternatives considered

- **Lex `--name=value` as one word in expression mode.** Rejected: the value may be a
  parenthesised expression or a string, and the lexer would have to re-implement the expression
  grammar to find where it ends.
- **Leave the gap and require the spaced form.** Rejected: it is the rule-per-mode ADR-0032
  refused, and the error the user gets says nothing about which mode they are in.
