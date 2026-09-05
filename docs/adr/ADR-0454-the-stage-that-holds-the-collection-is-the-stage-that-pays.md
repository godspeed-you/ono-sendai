# ADR-0454: The stage that holds the collection is the stage that pays

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §2.4, §6.2, §22.1, §22.2, §22.3, §30.2, §55.1, §60.4, §60.5, §65.6,
  Appendix A, Appendix E; v0.2 §11.1, §53
- Decided by: agent (autonomous)

## Context

§22.1 permits a global operation to materialize a finite upstream, and §22.2 fixes what it may
spend: 100 000 values and 128 MiB, both applying, the first reached winning. §6.2 says where the
enforcement belongs — *"byte-budget enforcement belongs in the materialization primitive, not in
each caller independently"* — and §30.2 says the helper belongs in one module so no caller
recreates it.

The shell already had the materialization and none of the budget. `Sort` buffered
`Vec<(Value, Value)>`, `Group` a `HashMap<KeyRepr, (Value, Vec<Value>)>`, `Measure` a
`Vec<Value>` of samples for its percentiles, `Join` its right side and `Diff` its earlier
snapshot — five unbounded collections, each of them exactly §65.6's shape once the input is
declared bounded: a count that says nothing about the bytes behind it.

So there were two questions. Where does the limit live, and which stage is charged for what.

## Decision

### 1. The limits ride on the pipeline; the budget belongs to the stage

`PipelineConfig` carries a `MaterializationLimits` — the configured pair — and every stream built
from it carries it onward through `ValueStream::stage`, so a limit set once at the top of a
pipeline reaches the tenth stage without being threaded by hand.

A stage that materializes calls `input.budget_for("sort")` and spends **its own** `Budget`.
Appendix A says "hard per materializer", so two `sort`s in one pipeline each get 128 MiB rather
than sharing 64. §23.4's aggregate ceiling is a different mechanism, for captures, and is not
this one.

This is the seam configuration reaches through: `limits.materialize_items` and
`limits.materialize_bytes` (#74) set `MaterializationLimits` on the config the evaluator builds,
and nothing else has to know they exist.

### 2. The stage that *holds* the collection is charged for it

Appendix E classifies operations, and the classification is about retention rather than about
whether an operation needs finite input:

| Stage | What it retains | What is charged |
| --- | --- | --- |
| `sort` | every value, to reorder them | every value from the upstream |
| `group` | every value, in its bucket | every value from the upstream |
| `measure` | one sample per value, for §53's percentiles | every value from the upstream |
| `join` | the **right** side; the left streams past it | the right side, before a row is emitted |
| `diff` | the **previous** snapshot; the new state streams past it | the previous snapshot, first |

`join` and `diff` are the ones worth stating explicitly. Both declare
`InputRequirement::Bounded`, which reads as "this stage materializes", and neither materializes
the stream it is applied to: they materialize the side handed to them. Charging their streamed
input would have bounded throughput rather than memory, and would have refused a `get process |
join (get socket)` over ten million processes whose retained cost is one `get socket`.

`join` and `diff` are charged **before** they emit anything, so a snapshot too large to hold is
refused at the start rather than after the left side has been half-consumed.

### 3. `materialize` is the one helper, and it refuses an unbounded upstream first

`ono_pipeline::materialize(stream, budget)` is the primitive §6.2 and §30.2 ask for. It checks
boundedness before it consumes a value (§22.3), then admits value by value, refusing the moment a
ceiling is crossed. The stream is dropped at that point, which cancels the stages above it — a
materializer that had gone on draining to be polite would be spending the budget it just refused.

Charging per value rather than measuring the collection at the end is what makes the refusal
early. §60.4's 100 001 values must fail *"before storing unbounded additional data"*, and a
measure-then-check design stores all of it first. The cost is that shared payload is counted once
per value instead of once per collection (ADR-0452 §Consequences); the over-count is in the safe
direction.

### 4. `materialize_with` hands the spent budget back

The nesting of §23.4 needs a parent to learn what a child spent. `materialize_with` returns the
budget beside the values so a caller can `parent.absorb(child)`; `materialize` is the same thing
for a caller with no parent. Two functions rather than one because the common case should not
carry a value it discards.

## Consequences

Easy: `get x | sort` is bounded in bytes today, for every `x`, with no caller change. A new
blocking transform is one `budget_for` call away from being bounded, and
`should_bound_every_transform_that_buffers_its_whole_input` fails if it forgets.

Hard: a user whose `sort` used to succeed over 200 000 rows now gets a refusal. That is the
product decision §22.2 makes, and the refusal names `limits.materialize_items` so raising it is
one line. It also means the default has to be right; 100 000 and 128 MiB are Appendix A's, not
this ADR's.

Also hard: `measure` is charged like `sort` although only its percentiles need the samples.
Splitting the constant-state statistics from the distribution ones is a real improvement and a
different change — recorded for `docs/STATE.md`'s *Found, not yet filed*, not done here (AGENTS.md
§4).

Constrains H6: the streaming repair (#75–#81) must not route a forwarding stage through
`materialize`. The helper's contract is *"turn this finite stream into memory"*, and a stage that
calls it has by definition stopped streaming — §65.7's failure mode with the budget's blessing.

Encoded by `crates/ono-pipeline/tests/budget.rs`:
`should_refuse_the_hundred_thousand_and_first_value_a_global_operation_collects` (§60.4),
`should_refuse_on_the_byte_ceiling_when_a_few_large_values_exceed_it` (§60.5),
`should_bound_every_transform_that_buffers_its_whole_input`,
`should_admit_everything_a_finite_stream_holds_when_it_fits_the_budget` and
`should_refuse_an_unbounded_stream_before_it_consumes_a_value`.

## Alternatives considered

**A `Budget` field on each transform, set by a builder.** It makes every construction site decide
a limit, which is how five sites end up with four answers, and it puts the number in the
evaluator instead of in the configuration layer that owns it (§55.1).

**One budget shared by every stage of a pipeline.** Appendix A's "hard per materializer" says
otherwise, and it would make a pipeline's behaviour depend on how many stages precede the one
that fails — `sort | group` refusing where `sort` alone succeeded, for the same data.

**Charging `join` and `diff` for their streamed side as well.** Symmetric, and it would bound
what they do not retain. `join`'s left side is forwarded row by row; a limit on it is a limit on
how much data may pass through the shell, which is not a resource budget and would make the two
stages the only ones in the pipeline that cap throughput.

**Measuring the whole collection once it is built, instead of per value.** One estimate instead of
N, and it measures memory the process has already committed. §60.4 requires the refusal *before*
that, and §21.3 requires stopping rather than reporting afterwards.
