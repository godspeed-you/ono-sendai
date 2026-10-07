# ADR-0942: `explain` is a producer of one plan, and a delimited subject composes

- Status: accepted
- Date: 2026-10-07
- Spec refs: §9.1 (Meta), §11.3, §15.3, §42.1, §42.2, §4.6, §13.1; v0.4.1 §22.4, §26.2; ADR-0011,
  ADR-0015, ADR-0814, ADR-0911, ADR-0935, ADR-0951
- Decided by: agent (autonomous)

## Context

Spec §9.1 lists `explain <command>` with the output `ExecutionPlan`, and §15.3 writes its subject
unquoted: `explain get process | where cpu > 20 | stop process`, where the pipes belong to the
pipeline being explained. The shell implemented that reading and only that one. The `explain`
builtin swallowed the rest of the line, so `explain get process | sort pid | to json` explained
`to json` as stage 3, and `explain "get process | sort pid" | to json` planned an empty stage:
the plan could never become data, although `ExecutionPlan::to_value` existed (issue #173).

Three more problems sat beside it:

- **Two explain semantics.** `ono-command` carried a second, session-less `explain`
  (`impls/meta.rs`, `Kind::Explain`) that knew no alias, no user function, no link and no
  compiled-out tier, reachable as `ono:explain` and in library use.
- **Text the value did not have.** The builtin printed lines the plan value did not carry — an
  expanded alias, the EXECUTION CONTEXT and MUTATION blocks of §42.2, which program a word
  resolves to, a user function's declaration and streaming shape — so the rendering and the data
  already disagreed.
- **No schema.** `ono.execution-plan/1` was listed in `schemas/deferred.yaml`, so the conformance
  suite of ADR-0935 could not hold `explain`'s examples to anything.

## Decision

1. **A delimited subject makes `explain` an ordinary producer.** When `explain` has exactly one
   argument and it is a quoted string, a `{ … }` block or a `$variable`, the subject is that
   argument — a block's source between its braces, a string or variable as it evaluates — and
   `explain` produces one `ono.execution-plan/1` record into the stages after it:
   `explain "get process | sort pid" | to json`, `explain { get process } | select stages`,
   `let p = (explain "…")`.
2. **Bare words keep §11.3's meaning.** Any other spelling — `explain get process | where cpu >
   20 | stop process` — takes the rest of the stage list as its subject, ends the stage list, and
   renders its plan. A stage list ends at `&&`, `||`, `;` and `)`, so `(explain get process | sort
   pid)` is still a value. A single stage written with bare words is its words as the shell
   expands them, so a glob names its files (§17.3), as before.
3. **One session-aware builder.** `crates/ono-cli/src/explain.rs` builds every plan: the sealed
   change plan of ADR-0814, alias expansion in execution's order, compiled-out tiers (ADR-0911),
   `PATH` resolution, user functions and their streaming shape (ADR-0951), the link's execution
   context, a mutation's operation, and the remote agent's adaptation answer. Everything it learns
   is a field of the plan (`aliases`, `environment`, `context`, per-stage `operation`, `path`,
   `remote_adaptation`, `notes`). `ono-command`'s `Kind::Explain` is removed; the contract
   `ono.meta.explain` is bound by the evaluator, as `ono.config.get` is, and `xtask`'s
   `BOUND_ELSEWHERE` register says so. `ono:explain` is claimed by the same code as `explain`.
4. **The rendering is presentation of the value.** `ono_command::render_plan` draws the PIPELINE
   layout of §42.1, the EXECUTION CONTEXT and MUTATION blocks of §42.2 and the notes from the
   record alone, and `ExecutionPlan::render` is that function over `to_value`. The shell's sink
   renders an `ono.execution-plan/1` that reaches the end of a pipeline this way, neutralising
   every line (ADR-0015 T1), wherever the record came from.
5. **The schema is written.** `docs/contracts/schemas/execution-plan.v1.yaml` declares the
   record and is removed from `deferred.yaml`. The contract's examples gain the two delimited
   forms; the unquoted `explain get process` is exempted from the conformance suite with that
   reason, because the suite's `| inspect | to json` would become part of its subject.
6. **Configuration mode refuses `explain`** in either form, as it refused the single-stage
   builtin before (ADR-0010).

## Consequences

- `explain` composes: a plan can be serialised, selected, stored and compared, and the conformance
  suite holds it to `ono.execution-plan/1`.
- §11.3's unquoted form keeps working and keeps rendering the same text.
- The pre-flight field check of §11.3 skips the stages of an unquoted subject: they are planned,
  never run, so nothing flows between them to check.
- A stage written after `explain` in pipeline position (`get process | explain "…"`) is no longer
  answered by a library implementation; nothing reaches `explain` through a pipe.
- Tests: `crates/ono-cli/tests/explain.rs` (`should_serialise_the_plan_when_a_quoted_subject_is_
  piped_into_to_json`, `should_let_a_stage_read_the_plan_when_the_subject_is_a_block`,
  `should_bind_the_plan_to_a_variable_when_explain_is_parenthesised`, `should_render_the_plan_
  when_a_quoted_subject_stands_alone`, `should_explain_the_whole_line_when_the_subject_is_written_
  without_quotes`, `should_carry_an_expanded_alias_in_the_plan_value`, `should_carry_a_user_
  function_in_the_plan_value`, `should_render_the_same_resolution_sentences_it_carries`); the
  existing explain suites (`builtins.rs`, `adapters.rs`, `remote.rs`, `files.rs`, …) unchanged;
  the generated `command_conformance.rs`; acceptance case `380-explain-composes.case`.

## Alternatives considered

- **Parenthesised subject (`explain (get process | sort pid)`).** Parentheses already mean "run
  this and use its value" (ADR-0009); `explain` would have to special-case evaluation, and a
  reader could not tell from the line that nothing runs.
- **Always delimit; drop the unquoted form.** Contradicts §15.3 and §42, which write the subject
  unquoted, and every user's habit.
- **A `--json` option on `explain`.** A second output path for one command; composition is what
  the shell's pipeline already does for every other value.
- **Keep the library `explain` and inject the session through `Invocation`.** The session lives on
  the evaluator's thread and native stages run as tasks; a callback carrying it would cross that
  boundary for a command that never needs input.
