# ADR-0940: `explain` decides an option's class from the value it can read without running anything

- Status: accepted
- Date: 2026-10-07
- Spec refs: v0.4.1 §22.4, Appendix E; §15.3; ADR-0939, ADR-0953, ADR-0460
- Decided by: agent (autonomous)

## Context

ADR-0953 lets an option change the execution class of one invocation: `measure` folds in constant
state, `measure --percentiles [50]` holds the distribution. `CommandContract::execution_for` counted
an option written as an unevaluated expression as set, whatever it would evaluate to. With `$p`
null, `explain { … | measure cpu --percentiles $p }` reported `explicit_collect` while the run,
reading `null` as "no percentiles", stays constant-state (review C7b). `explain` and the runtime
disagreed, and v0.4.1 §22.4 calls showing the mode "a product feature of honesty".

`explain` cannot simply evaluate the expression: ADR-0939 forbids evaluating anything that can run
code while planning.

## Decision

1. The class of an invocation is decided from the values its class-deciding options take.
   `CommandContract::execution_deciding` asks the caller for the value of each option written as
   an expression; `execution_for` is that question with no answers.
2. The session-aware plan builder answers for every expression that only reads the session
   (ADR-0939's rule: literals, `$name`, `$name.field`, strings interpolating only those), so the
   plan of `--percentiles $p` follows `$p` exactly as the run will.
3. An expression that could run code stays unevaluated. The stage is planned with the class it has
   when the option is set — the class that may hold more, so a budget is never understated — and
   carries the note "the value of `--<option>` decides what this stage holds".

## Consequences

- `explain` and the runtime agree whenever the value can be known without running anything, and
  say so plainly when it cannot.
- Tests: `crates/ono-cli/tests/explain.rs` —
  `should_plan_constant_state_when_a_percentiles_variable_holds_null`,
  `should_plan_the_collecting_class_when_a_percentiles_variable_holds_a_list`,
  `should_say_the_value_decides_when_an_option_expression_is_not_evaluated`.

## Alternatives considered

- **A third class, "undecided".** Every consumer of the class (budget, refusal of unbounded input,
  the rendering) would have to learn it, for one rare spelling; the conservative class plus a note
  says the same thing.
- **Evaluate the expression.** Contradicts §15.3 and ADR-0939.
