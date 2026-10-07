# ADR-0943: `explain` strips a prefix assignment as execution does, and states the environment

- Status: accepted
- Date: 2026-10-07
- Spec refs: §54, §15.3, §42; ADR-0011, ADR-0015, ADR-0071 §2, ADR-0942
- Decided by: agent (autonomous)

## Context

Spec §54 lets `NAME=value command …` set a variable for one pipeline, and the evaluator strips
the assignments with `eval/statement.rs::prefix_assignments` before it resolves the stage
(ADR-0071 §2). `explain` never called that function: `explain FOO=1 get process` looked the word
`FOO=1` up as a program and reported "`FOO=1` is not a native command" — a plan of something that
would never run (issue #223, part 1).

## Decision

1. **The same function.** The plan builder of ADR-0942 calls `prefix_assignments` — the function
   execution uses — on the subject's first stage list before it expands an alias, in the order
   `run_stage_list` applies them. The stage is planned as the command after the assignments, so
   `explain FOO=1 get process` plans `ono.process.get`, and `explain FOO=1 ls` plans the program
   `ls` and its `PATH` resolution.
2. **The environment is part of the plan.** Each assignment becomes an entry of the plan's
   `environment` (`name`, `value`), in the order written, with the value evaluated exactly as
   execution evaluates it (`$var` expanded). The rendering states it under `PIPELINE` as
   `environment  NAME=value`, for native and external stages alike — the variables reach every
   stage of the list, which is what execution does.
3. **Sanitised on the way to a terminal.** The value is kept verbatim in the record — a script
   reading the plan gets what the stage would get — and every rendered line is neutralised by the
   sink (ADR-0015 T1), so a value cannot drive the terminal the plan is shown on.
4. A stage that is nothing but assignments is refused by `explain` with the same error execution
   gives, because it is the same function.

## Consequences

- `explain` and execution cannot disagree about what `NAME=value cmd` runs.
- Evaluating an assignment's value runs what the value names — `$( … )` in it would run — exactly
  as typing the line would before anything else ran; the subject itself still never runs.
- Tests: `crates/ono-cli/tests/explain.rs` —
  `should_plan_the_command_after_a_prefix_assignment_when_explaining_it`,
  `should_carry_the_environment_of_a_prefix_assignment_in_the_plan_value`,
  `should_evaluate_a_prefix_assignment_value_as_execution_does`,
  `should_neutralise_control_characters_in_an_environment_value_when_rendering_the_plan`;
  acceptance case `381-explain-prefix-assignment.case`.

## Alternatives considered

- **Leave the assignment in the stage's source and only drop it from resolution.** The stage
  source still quotes it, which is what was typed; but without an `environment` field a script
  could not tell what the stage would run with.
- **Redact values whose names look secret.** The user typed the value on the same line; the plan
  is what that line would do, and a redacted plan would be a plan of something else. History and
  the action ledger keep their own redaction (§17.5).
