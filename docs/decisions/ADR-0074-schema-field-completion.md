# ADR-0074: Completion offers the fields of the schema flowing into the stage

- Status: accepted
- Date: 2026-08-27
- Spec refs: §11.3, §15.1, §15.2, §34; ADR-0013, ADR-0067
- Decided by: agent (autonomous)

## Context

Spec §15.1's own example is `get process | where <tab>` showing `pid ppid name user cpu memory
state started …`. Completion answered verbs, targets, options and closed value sets from the
registry, and stopped there: after `where` it offered nothing, because `StageContext` kept only
the stage under the cursor and threw the pipeline before the pipe away. The pre-flight check of
spec §11.3 already computes exactly the fact completion lacks — which schema reaches which stage
— from the contracts alone. `crates/ono-command/tests/completion_missing.rs` states the
expectation; this ADR states the rule.

## Decision

1. **The stage context remembers what precedes the pipe.** `StageContext::from_line` keeps the
   text before the `|` that opened the stage (`upstream`). A `;` or `&&` starts the stage from
   nothing and hands no schema on. `StageContext::new` — the parts constructor — has no
   upstream, so callers that assemble a context by hand get the old behaviour unchanged.
2. **The schema is read from the contracts, never from a run.** The upstream text is parsed and
   walked stage by stage with the same output-type rule the check uses (`check::schema_after`):
   a stage that names an output schema hands it on, a stage whose output is open (`where`,
   `sort`, `take`, …) passes the upstream schema through, and a stage that is not a native
   command or reshapes the stream leaves the schema unknown. Unknown means no field candidates,
   not a union of every schema. Nothing runs, so the spec §34 budget holds: the existing
   budget test (a thousand completions, including `get process | where `, under a second) stays
   green.
3. **Fields are offered where an expression reads them.** When the next selector of an
   expression-mode command carries values — `where`'s predicate, `select`'s fields, `sort`'s
   key, `group`'s key — and the upstream schema is known, the candidates are that schema's
   fields starting with the typed prefix, each carrying the field's `doc` from
   `docs/spec/schemas/*.v1.yaml` (spec §15.2: help derives from metadata). A string parameter
   is vocabulary, not a field, exactly as the check treats it (`sort cpu desc`).
4. **A field is its own kind of candidate.** `CandidateKind::Field` lets an editor present a
   field differently from a verb or an option without inspecting the text.

## Consequences

- The REPL builds its context with `from_line`, so a user gets field completion at the prompt
  with no further change; `docker/acceptance/cases/044-semantic-completion.case` keeps proving
  completion on a real terminal.
- Adapted stages (ADR-0067) are not yet seen by `schema_after`: after `lsblk |` the schema is
  unknown to completion here, as it was before; threading the adapter plan into completion is
  the adapter family's seam, and the CLI's own completion test for adapted schemas is
  unaffected.
- Tests: `crates/ono-command/tests/completion_missing.rs` (six tests), and
  `crates/ono-command/tests/completion.rs` unchanged.

## Alternatives considered

- **Complete the union of every schema's fields.** Rejected by the test and by §15.1: a
  `File` has no `cpu`, and offering it is a wrong answer given confidently.
- **Run the pipeline's head to learn its schema.** Rejected: §34 budgets 50 ms and §15.1 wants
  completion to be *metadata* lookup; running `get process` to complete `where` is neither.
- **Let the caller pass the schema in.** Rejected for now: every caller would have to
  reimplement the walk the check already does; the seam exists (`schema_after`) if one needs it.
