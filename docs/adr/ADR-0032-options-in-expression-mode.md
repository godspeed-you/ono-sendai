# ADR-0032: `--name`, written adjacently, is an option in expression mode too

- Status: accepted
- Date: 2026-08-26
- Spec refs: §6.2; ADR-0009; docs/contracts/commands/data.yaml (ono.data.reduce)
- Decided by: agent (autonomous)

## Context

`reduce $acc + @ --initial 10` could not be written. In expression mode `--initial` lexed as two
minus tokens — a double unary negation of a field named `initial` — so the parser swallowed it
into the folding expression and `10` became a second selector: "`reduce` takes 1 selector(s)".
Yet the contract declares `--initial`, and ADR-0009 gives every expression-mode command its
options. The two readings genuinely collide: `-` is subtraction, and `--x` *is* a wellformed
double negation.

The same investigation found the contract's example for `reduce` was `@acc + @`, which the
grammar has never allowed — `@` takes only digits after it (spec §6.4). The accumulator has
always been the variable `$acc`.

## Decision

**In expression mode, `--` followed immediately by an identifier character is a long option, at
every token position.** The lexer produces one option token for it; the expression parser
therefore ends the expression before it, and the stage parser reads it as an option whose value
is the following expression, paired at binding exactly as in words mode.

The costs, accepted deliberately:

- `--x` no longer means "negate `x` twice". A spaced `- -x` still does, and a test pins that.
  Nobody writes `--x` for its value; everybody writes `--initial` for its optionness.
- `x--y` (subtract negative `y`) now reads as `x` followed by the option `--y` where `y` starts
  with a letter. `x - -y` says the same thing legibly.

The `--name=value` spelling stays words-mode only for now; expression mode pairs the option with
the expression after it. The board carries the gap.

This is the ADR-0009 principle applied once more: the *shape* of the token decides — adjacency
is spelling, not a heuristic over meaning.

## Consequences

- `reduce $acc + @ --initial 10` folds from 10; `--initial 0` makes an empty fold answer `0`
  instead of erroring, which is what makes `reduce` usable in scripts over possibly-empty
  streams.
- The contract example becomes `reduce $acc + @ --initial 0`, which parses — doc examples are
  checked by the gate, which is how the wrong example survived: it was in a file whose examples
  were valid YAML but had never been executed. (`spec-check` runs examples through the parser;
  `@acc` parsed as `@` beside `acc`, so nothing flagged it. The binder now proves the semantics
  in `ono-command/tests/transforms.rs` instead.)
- An option's expression is evaluated against no current value — it seeds the pipeline before
  anything flows, so there is nothing for it to refer to.

## Alternatives considered

- **Keep options out of expression mode; require `reduce ($acc + @) --initial=10`.** Rejected:
  the parenthesised spelling is not what the contract examples or spec §53 write, and an option
  syntax that changes per argument mode is a rule users would have to memorise.
- **Disambiguate by whitespace ("`--` after a space is an option").** Rejected: ADR-0009 forbids
  whitespace-decides-structure, and this would be exactly that.
- **A dedicated option sigil in expression mode.** Rejected: two spellings for one concept.
