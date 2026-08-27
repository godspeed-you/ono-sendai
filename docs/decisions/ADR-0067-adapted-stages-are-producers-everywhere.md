# ADR-0067: An adapted stage is a producer on every surface — planning, checking, completing, remembering

- Status: accepted
- Date: 2026-08-27
- Spec refs: v0.3 §1.2 (invariants 5, 6, 10), §1.53, §1.58, §1.59, §1.61, §1.70, §1.71, §2.4;
  v0.2 §11.3, §20.1, §34; ADR-0052, ADR-0056, ADR-0057, ADR-0059
- Decided by: agent (autonomous)

## Context

Through ADR-0057 an adapted program produced typed records at run time, but the surfaces
around the run still treated it as bytes: the pre-flight field check of spec §11.3 stopped at
the first external stage, `type "lsblk | where type == \"disk\""` answered `stream<any>`,
completion after `ps aux |` knew nothing, and a history entry did not say that an adapter had
taken part. Spec v0.3 §1.61 asks that "introspection commands make layer membership visible"
so that no "invisible second command-resolution system" arises; §1.59 asks completion to know
the schema after an adapted pipeline while inventing nothing before the pipe; §1.58 asks history
to remember that a command was adapted. Each of those needs the same fact — *which stages does
an adapter give a schema, and which schema* — in a place that until now only the executor knew.

## Decision

1. **The plan carries the schema, and the plan is the one source.** `ono_command::plan_with`
   threads an adapted stage's schema into the stages after it: the adapted stage's output
   becomes `stream<schema>` and every later native stage is planned again over that type.
   `ExecutionPlan::adapted_schemas()` exposes the per-stage answer. Nothing else re-derives it.
2. **Pre-flight checks adapted stages like native producers.** `check_pipeline_with` takes the
   per-stage schemas from the plan, so `lsblk | where colour == "blue"` fails with
   `type.unknown_field` against `ono.block-device/1` before `lsblk` runs — exactly as
   `get process | where colour` fails. The shell computes the plan for the check only outside
   link frames; inside one the remote decides (ADR-0066) and nothing is claimed locally.
3. **`type` plans with the registry.** An `Invocation` can carry the adapter registry and a
   `PATH` resolver (`Invocation::with_adapters`); `type <pipeline>` uses them, so its answer is
   the plan's answer. The registry is therefore shared (`Arc`) between the session, the running
   pipeline and the line editor, and gains packs behind a lock rather than by `&mut`.
4. **Completion invents nothing.** After `<stage> |`, an expression-mode selector (`where`,
   `select`, `sort`) is completed with the fields of the schema the plan gives the producer —
   the same code path for `get process |` and for `ps aux |`. Before the pipe, a `-` prefix on
   a stage whose head an adapter claims offers only the flags the adapter's invocation contracts
   declare (`Registry::declared_flags`); an undeclared flag is neither offered nor refused —
   the program still runs raw with it.
5. **History records the adapter and the plan.** `ono_history::Outcome::adapted_by` stores each
   adapter's full id and the argv it planned; `Entry::adapters()` and `Entry::explain()` answer
   from the record without re-running anything (spec §20.1). The session collects adaptations
   per statement (`note_adaptation` / `take_adaptations`); a remote adaptation is recorded as
   its negotiation state `on <host>`.
6. **Determinism is a property of the plan, not the surface.** Because `-c`, a script file, a
   redirected pipeline and the interactive loop all run the same `plan_with`, the same
   invocation selects the same adapter everywhere; only the *demand* differs, and it differs
   by the consumer alone (ADR-0052). A program at the end of a redirected line stays raw bytes.
7. **Text tools stay raw by contract.** No first-party pack names `cat`, `grep`, `sed`, `awk`,
   `head`, `tail`, `sort`, `uniq`, `wc`, `less`, `more`, `cut`, `tr`, `tee`, editors or REPLs;
   the registry answers `NotApplicable` for them, and a test pins that list (v0.3 §1.70).

## Consequences

- `Registry` is `Sync` with interior mutability; `add_pack` takes `&self`. `Session::adapters_mut`
  is gone; `Session::shared_adapters` hands out the `Arc`.
- Completion plans the upstream pipeline with a synthetic structured consumer (`| count`) on
  every completion request after a pipe; version probes are cached per registry, so the cost
  after the first request is a lookup (§34: first results < 50 ms).
- The history file format gains two optional arrays (`adapters`, `plans`); older entries read
  unchanged.
- Tests: `ono-cli/tests/builtins.rs`
  (`should_answer_type_with_the_adapters_schema_and_check_fields_before_running`),
  `ono-cli/tests/adapters.rs`
  (`should_complete_fields_of_the_adapted_schema_after_the_pipe_and_declared_flags_before_it`,
  `should_record_the_adapter_in_history`), `ono-history/tests/history.rs`
  (`should_remember_that_a_command_was_adapted_and_explain_it`), `ono-adapter/tests/negotiation.rs`
  (`should_leave_text_tools_raw_by_design`), acceptance cases `085`, `086`, `087`.

## Alternatives considered

- A second schema lookup in the checker and in `type`, keyed by program name — rejected: it
  would be the "invisible second resolution system" §1.61 warns about, and it would disagree
  with the plan the moment an invocation matcher or a version constraint decided differently.
- Completing every flag the program's `--help` prints — rejected by §1.59: the adapter can only
  vouch for what its contract declares; the rest is the tool's business and stays raw.
- Recording only "adapted: true" in history — rejected: §1.58 wants `history --explain` to say
  which adapter and which plan, so that the same command behaving raw on another machine is
  explainable.
