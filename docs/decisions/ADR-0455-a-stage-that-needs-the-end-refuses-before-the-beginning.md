# ADR-0455: A stage that needs the end refuses before the beginning

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §22.3, §52.1, §52.3, §53.1, §53.2, §54.1, §65.8, Appendix E; v0.2 §11.1;
  ADR-0125
- Decided by: agent (autonomous)

## Context

§22.3 and §65.8 say the same thing twice, which is how a specification signals that it has seen
the mistake made:

> If the stream is marked `Unbounded`, the operation MUST refuse immediately with an error
> explaining that it requires finite input. It MUST NOT wait forever to discover that an
> unbounded stream never ends.

> **65.8 Unbounded input to global operation.** Waiting forever for an explicitly unbounded stream
> to finish is forbidden. Refuse early.

Ono already refused early. `ValueStream::transform` reads the declared boundedness and answers
`stream.unbounded_operation` before a task is spawned, and it has since v0.2 §11.1. Two things
were missing, and neither was the refusal.

The first is the **sentence**. §54.1 writes it out — *"sort requires finite input; upstream is
declared unbounded"* — and what Ono said was *"`sort` needs input that ends, and this stream may
not end"*: true, and it names neither the requirement nor what the upstream declared itself to be.

The second is the **classification**. Appendix E ends with a requirement disguised as an
observation: *"If a command cannot be placed in this matrix, its execution semantics are
underspecified and MUST be resolved before release."* Nothing in the repository could fail that
sentence, and nothing knew which stages required finite input except the transforms themselves,
in Rust, out of reach of `explain` and of the gate.

## Decision

### 1. The refusal is reshaped, not renumbered

`stream.unbounded_operation` (`Ono-Sendai-E0801`) stays the code. The message becomes §54.1's:

```
`sort` requires finite input; the upstream is declared unbounded
```

with `stage`, `requires` and `upstream` in the metadata, so §53.2's rule — automation matches
codes and fields, not prose — holds for the parts a script would want. Why the code does not
change is ADR-0453 §4 and its `Spec deviation` heading.

The refusal happens in `ValueStream::transform`, from the declared boundedness alone, so it is
true of a stream nobody has read. `materialize` checks the same thing before its first `recv`, so
an evaluator that collects without a `Transform` cannot become the path that waits.

**The proof does not measure time.** `should_refuse_an_unbounded_upstream_before_waiting_when_the_operation_requires_finite_input`
asserts against a source that has not been asked for anything; the CLI proof follows a file that
is never written to and asserts afterwards that the file still holds one line. That is ADR-0431's
discipline, and it is the only way "before waiting" can be written as something a test can read
rather than as a stopwatch reading.

### 2. Appendix E becomes a machine-readable contract

`docs/spec/hardening/streaming_classification.yaml` declares Appendix E's eight classes with the
two properties the hardening layer acts on — `requires_finite_input` (§22.3) and `may_materialize`
(§22.1) — and places every pipeline operation in one.

Each command's contract carries `execution: <class>`, so the classification travels with the
command rather than beside it, and `cargo xtask spec-check` enforces three things in both
directions:

- a command whose declared `input` is a stream and which names no class fails the gate — Appendix
  E's closing sentence, as something that can go red;
- a class the registry declares and `ono_command::ExecutionClass` does not know fails, and the
  reverse fails too;
- a class whose two properties disagree between the registry and the implementation fails.

Two placements are worth defending because the example names in Appendix E do not match:

- **`measure` is `explicit_collect`.** Its `count`, `sum`, `mean`, `min` and `max` are
  constant-state, but an exact percentile is defined over the whole distribution and holds a
  sample per value. A stage is placed by its properties, not by the closest example word, and the
  properties here are "requires finite input, materializes within budget". Splitting the two
  halves would move the first to `incremental_aggregate`; that is a real improvement and a
  different change.
- **`join` and `diff` are `explicit_collect`.** Each streams one side past a collection it holds,
  and the collection is what the budget charges (ADR-0454). Appendix E has no relational row, and
  inventing a ninth class for two commands would break the closed set it declares.

### 3. `tail` is not a materializer

`tail` is `incremental_aggregate`, and stating it is the point. It keeps the last `n` values in a
ring of the declared length — bounded state, not a materialization — so it does not require finite
input the way `sort` does, and a plan that showed it a materialization budget would be claiming a
constraint it does not have.

## Consequences

Easy: adding a transform now means saying what it does with its input, in the contract, or the
gate refuses it. `explain` (#69) and the finiteness refusal (§22.3) both read that one answer.

Hard: the eight classes are Appendix E's and the set is closed, so a genuinely new execution shape
needs an ADR that says which row it belongs in and why — as `measure`, `join` and `diff` needed
above. That is the cost of Appendix E's own closing sentence, and it is the intended cost.

Constrains H6: `each` is `item_transform`, which declares `requires_finite_input: false` and
`may_materialize: false`. The streaming repair of #75 and #76 has to make that true; today the
classification is a claim the implementation does not yet honour, and `each_streaming.rs`'s two
`#[ignore]`d proofs are where the gap is recorded (ADR-0431). Nothing in this ADR closes it, and
the matrix is deliberately written as the target rather than as the present.

Encoded by `crates/ono-pipeline/tests/boundedness.rs::should_refuse_an_unbounded_upstream_before_waiting_when_the_operation_requires_finite_input`
and `::should_refuse_before_a_value_is_drawn_when_a_materializer_meets_an_unbounded_upstream`,
`crates/ono-cli/tests/resource_limits.rs::should_name_the_finiteness_requirement_and_the_declaring_stage_in_the_refusal`,
and `xtask/tests/contracts.rs::should_place_every_pipeline_operation_in_the_streaming_classification_matrix`
with `::should_reject_a_stream_consuming_command_the_matrix_does_not_place`.

## Alternatives considered

**A second error code for the materializer's version of the refusal.** It is one condition, and
§53.2's premise is that a code identifies a condition. Two names for it would make a script that
matched one of them silently wrong.

**Deriving the classification from `streaming: false` in the contract.** It is already there and
it conflates two classes: `count` and `reduce` are `streaming: false` and materialize nothing,
while `sort` is `streaming: false` and holds everything. A byte budget shown for `count` would be
a lie, and a finiteness refusal from `tail` would be a bug.

**Keeping the matrix in the transforms' Rust, as `InputRequirement`.** It is the implementation's
own answer, and `explain` and the gate cannot read it: the contracts are what the shell publishes
and what `spec-check` compares. `InputRequirement` stays, and is now the thing the classification
describes rather than the place it lives.
