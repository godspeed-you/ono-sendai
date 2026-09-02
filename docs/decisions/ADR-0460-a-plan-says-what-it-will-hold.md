# ADR-0460: A plan says what it will hold

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §22.1, §22.2, §22.4, §53.2, §55.1, Appendix A, Appendix E; v0.2 §15.3, §42.1;
  ADR-0455, ADR-0456
- Decided by: agent (autonomous)

## Context

§22.4 is short and unusually emphatic about what kind of thing it is asking for:

> `explain` MUST expose materialization when it affects execution semantics.
>
> ```
> sort memory desc
>   execution: global materialization
>   requires: finite input
>   budget: 100000 values / 128 MiB
> ```
>
> This is a product feature of honesty, not merely debug output.

Ono's `explain` already showed a `streaming` line, taken from the contract's `streaming:` flag.
That flag cannot answer §22.4's question: `count` and `reduce` are `streaming: false` and hold
nothing, while `sort` is `streaming: false` and holds everything. A plan built on it would mark
`count` a materializer and tell the user it had a 128 MiB budget it will never spend.

## Decision

**The plan derives all three lines from the execution classification the contract declares
(ADR-0455), and from the limits the pipeline would really run under (ADR-0456). It restates
nothing.**

`StagePlan` gains `execution: Option<ExecutionClass>` and `budget: Option<(u64, u64)>`, with four
accessors — `execution()`, `execution_mode()`, `materializes()`, `requires_finite_input()`,
`budget()` — and the rendering of §22.4's example:

```
2. sort memory desc
   command      ono.data.sort
   …
   streaming    no
   execution    global materialization
   requires     finite input
   budget       100000 values / 128 MiB
```

Three rules the shape follows:

1. **A stage that materializes nothing shows no budget and no requirement.** A budget printed
   beside `where` would be noise pretending to be a guarantee, and §22.4 asks for honesty rather
   than for completeness. `execution:` appears for every classified stage, because "streaming" is
   itself the answer to the question; `requires:` and `budget:` appear only where they bind.

2. **The budget shown is the configured one.** `PlanContext` carries a `MaterializationLimits`,
   and the `explain` builtin reads the session's settings, so a user who has narrowed
   `limits.materialize_bytes` to 4 MiB is told 4 MiB. A plan that always printed Appendix A's
   figure would be documentation rather than a plan.

3. **It is in the value, not only in the rendering.** §22.4 calls it a product feature, and §53.2's
   rule is that automation reads fields. `to_value()` carries `execution`, `requires`,
   `budget_items` and `budget_bytes`, so a script can ask "what will this pipeline hold" without
   parsing a table.

An unclassified stage — a producer, an external program, a plugin contribution — shows nothing of
this, because nothing is known: §22.4's line is about pipeline operations, and a plan that guessed
would be worse than a plan that is silent.

## Consequences

Easy: `explain get process | sort memory desc` answers §22.4's example, in §22.4's words, with the
user's own limits. Adding a materializing transform makes it appear there automatically, because
the classification is the contract's and the gate refuses a stream-consuming command that has none
(ADR-0455).

Hard: `PlanContext` gained a field, so every construction site states it. Three are in `ono-cli`
and read the session's settings; two are in `ono-command`'s own `explain`/`check` implementations
and use Appendix A's defaults, because a command running inside the registry has no session to
ask. That is a real gap of one hop — `explain` invoked as `ono.meta.explain` inside a pipeline
shows Appendix A's figures rather than the session's — and it is recorded rather than papered
over: closing it means threading the limits through `Invocation`, which is surface this increment
does not need.

Constrains H6: when `each` streams (#75), its classification is already `item_transform` and its
plan already says `execution: streaming` with no budget. If the repair makes `each` capture
anything, the plan will be wrong, and the plan is what §22.4 makes the user-visible half of
"inventory and classify every `Vec<Value>` capture in the evaluator".

Encoded by `crates/ono-command/tests/explain.rs::should_mark_every_materializing_stage_in_the_plan`,
`::should_name_the_finiteness_requirement_and_the_budget_of_each_materializing_stage`,
`::should_carry_the_materialization_of_every_stage_into_the_structured_plan`.

## Alternatives considered

**Reusing the `streaming:` flag.** Rejected above: it answers a different question, and building
§22.4 on it would put a materialization budget on `count`.

**A per-command hook that each implementation answers.** `explain` is entirely declarative today —
`plan_stage` reads the contract and executes nothing, which is what lets it be tested against a
provider that counts how often anybody asked it to do work. A hook would make the plan depend on
the code it is describing, and the first buggy hook would make `explain` lie in the direction
nobody checks.

**Showing the budget on every stage, materializing or not.** Uniform, and it tells the user that
`where` has a 128 MiB budget, which is false. §22.4's example shows three lines under one stage,
not under all of them.
