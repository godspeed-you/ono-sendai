# ADR-0480: The stage that runs a block asks the thread that owns the session

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §2.5, §25.1–§25.7, §26.1, §28.1–§28.3, §58.2 (H6-WP1), §60.1, §65.7, §65.12,
  Appendix E; spec §19.4; ADR-0013 (the execution model), ADR-0070 (the caller's output context),
  ADR-0071 §1 (the block form of `each`), ADR-0431 (the failure proofs), ADR-0453, ADR-0457
- Decided by: agent (autonomous)

## Context

v0.4.1 §0.5.5 names the defect and §25.2 names the rule it broke: *"the normal `each`
implementation MUST NOT capture the complete upstream stream into a `Vec<Value>` before block
execution."* `eval.rs::run_each_block` did exactly that — it built a second `StageList` out of
everything in front of `each`, ran it as its own pipeline under `begin_capture()`/`end_capture()`,
then ran the block once per captured value and handed the collected results to
`native::run_seeded_from`. Two failure proofs stood red against it (ADR-0431), and the
differential beside them — the same source and the same `take 1` with `where` in place of `each` —
answered at once, which placed the defect precisely on `each`.

The obstacle was never the pipeline. It was that **a block is not an expression.** `where cpu > 20`
carries a predicate that `ono-command`'s own evaluator can apply to a value on any thread;
`each { restart service @ }` carries *statements*, which only the shell's evaluator can run, and
the shell's evaluator needs `&mut Session` — the bindings, the context stack, the job table, the
provider registry, the retained results. `Session` is neither `Send` nor `'static`, and
`ValueStream::stage` requires both.

Three shapes were available, and two of them are forbidden by the specification itself.

## Decision

**A block stage does not run its block. It asks, over a bounded channel of one, and the loop that
holds the session answers — between drains of the very pipeline the stage belongs to.**

### 1. `each { … }` is a stage of its pipeline, not a pipeline in front of one

`native::run_native_segment` now recognises a stage whose head is `each` and whose single argument
is a block, binds it exactly like any other stage — same contract (`ono.data.each`), same
arguments, same `Scope` — and substitutes `asking_stage` for the transform the registry would
otherwise have built. Everything upstream and everything downstream is one `ValueStream` chain, so
`each` inherits the pipeline's capacity, cancellation scope, diagnostics counters and
materialization limits, and there is no second pipeline anywhere.

`eval.rs` keeps one line of the old interception: `each { … }` as the *first* stage of a list has
no stream to transform, and that refusal is the language's, not the pipeline's.

### 2. The driver is the loop that holds the session

`run_native_segment` used to be one `runtime.block_on` over a future that both assembled the chain
and drained it. It is now two phases:

- **Assembly** — one `block_on`, which spawns every stage and hands back the final stream. The
  provider registry is borrowed only here, so the borrow of the session ends with it.
- **The driver** — a loop that is inside `block_on` only while it is *waiting*. It selects over
  three things: a block stage asking for an item, the pipeline producing a value or a failure, and
  the interrupt note. When a block asks, the loop **leaves** `block_on`, runs the block through
  `eval::run_each_item`, and replies.

Leaving `block_on` to run the block is the whole design. Tokio refuses to start a runtime from
within one, so a block that runs a pipeline of its own — `each { get process | count }` — would
panic if the block ran inside the driver's own `block_on`. Outside it, the block is ordinary
evaluator code that may do anything an evaluator does, including running another pipeline that
contains another `each { … }`.

The channel carries one request at a time. §25.3 keeps `each` serial, so a queue of pending items
would buy nothing, and §65.7 forbids the shape it would take: *"replacing a foreground `Vec` with
an unbounded background queue is not a streaming fix and is forbidden."*

### 3. One item's result is captured; no item's result waits for another's

`eval::run_each_item` binds `@`, runs the block, and hands back what that one invocation produced.
§25.4 permits precisely this — *"the implementation SHOULD model each block invocation as a small
streaming/capture scope only where the block semantics require knowing the block's complete result
for that individual item"* — and it is required here rather than merely convenient: a block's
control flow is only known when its last statement has run, so the stage cannot know whether to
forward, to skip or to stop until the item is finished. The capture is charged to §23.2's ceiling
through the same `Budget` as every other capture (ADR-0457), and it is recorded as
`semantic_materialization` in `docs/spec/hardening/streaming.yaml`. What is gone is the collection
*over all items*, which §25.4's next sentence forbids outright.

Whether the block's values are captured at all is still ADR-0070 point 3: with a stage after it
they stream into that stage, and with nothing after it the block's statements show their results
where they stand — which is why `get process | each { restart service @ }` still prints its
action rows as they happen, and why such a stage writes no result of its own.

### 4. Control flow is answered, not inferred

The reply carries `keep_going`. `continue` and an ordinary end both mean "forward what came back
and read the next item"; `break` means "forward what came back and stop reading", which the stage
does by returning — dropping its input, closing the upstream channel, and stopping the source at
its next send. `return`, `exit` and an unhandled error come back as a `Flow`, which stops the
driver and unwinds exactly where the old implementation unwound. §25.5 asked for "exact", and the
match arms are the same four the old loop had.

### 5. Cancellation follows the answer out

A stage that runs a block may still be waiting on a source that never ends after downstream has
had its answer — `each { … } | take 1` over a followed file. The driver therefore keeps the
pipeline's `CancelToken` and trips it when the drain is done, so §28.3's "cancellation wins" holds
for the shape this change introduced: a producer behind a stage nobody is reading stops rather
than continuing to enqueue.

## Consequences

Easy: `each` is now what Appendix E already said it was — an `item_transform` that never requires
finite input. `source | each { … } | take 1` answers from the first value the source produced;
an unbounded source is a legal input; `each { … } | each { … }` works at all, which it did not
before (the second one reached the transform engine and failed with `provider.unsupported`); and
a block may run a nested pipeline while its own pipeline is still streaming. Two files got
smaller: `run_each_block` and its second `StageList` are gone.

Hard, and worth stating plainly:

- **The driver is single-threaded by construction.** While a block runs, nothing else in this
  pipeline is drained. That is not a regression — §25.3 forbids a parallel `each` — but it means
  a slow block holds its pipeline's channels full, which is what backpressure is and what §28.2
  asks for.
- **A block whose own output is unbounded still blocks.** `each { tail file x --follow }` waits
  for the block's inner pipeline to end, because the per-item scope of §25.4 is a scope with an
  end. §18.3's live-stream rules apply inside the block as they do anywhere else, so this is a
  refusal rather than a hang, but it is a limitation and it is named here rather than discovered.
- **A backgrounded `each { … }` is still not supported.** `run_background` has no session to ask,
  and a job that needed one would need the driver to outlive the foreground. It fails as it did
  before. `docs/STATE.md` carries it under *Found, not yet filed*.
- **`run_native_segment` grew.** It now assembles, drives and drains, and it was already the
  longest function in the crate. Phase H9 (#96) decomposes the evaluator, and this function is the
  obvious seam: assembly, driving and result-writing are three things with three different sets of
  inputs. §65.12 forbids doing that in the same work package as this semantic change, so it is
  reported to H9 rather than done here.

Encoded by: `crates/ono-cli/tests/streaming.rs::should_emit_the_first_value_before_the_source_closes`,
`::should_run_the_block_for_one_item_before_the_next_item_is_required`,
`::should_keep_the_input_order_and_the_serial_execution_of_the_block`,
`::should_accept_a_source_declared_unbounded_without_refusing_it`,
`::should_keep_memory_flat_while_an_unbounded_source_is_consumed`, the two ADR-0431 proofs in
`crates/ono-cli/tests/each_streaming.rs` — un-ignored by this change with **no assertion altered**
— and acceptance case `193`.

## Alternatives considered

**Move the block into the spawned stage.** The obvious shape, and the one the trait bounds ask
for: `ValueStream::stage` wants `Send + 'static`. It requires a `Session` that can travel, and
every path to one is a redesign of the shell's state model — `Arc<Mutex<Session>>` would serialise
the evaluator behind a lock held across arbitrary user code, and a session split into a sendable
half and a resident half is exactly the decomposition §65.12 forbids combining with this change.

**Run the whole pipeline in a spawned task and drive the block from the foreground.** Workable —
`run_background` proves the pipeline can be made `'static`, since `ProviderRegistry` clones. It
costs a registry clone per pipeline and moves result writing behind a join handle, and it buys
nothing the driver loop does not already have. Held in reserve for #79, where the evaluator is the
*producer* rather than a stage in the middle and cannot be the thing that drains.

**Keep the capture and bound it by a budget.** §26.1 does allow "bounded and justified by ADR" —
for `implementation_convenience` captures. This one is not that: §25.2 forbids it by name, §25.6
makes accepting an unbounded source an acceptance criterion, and no budget makes an infinite
source finite. §65.8 is explicit that waiting forever for a declared-unbounded stream is itself
forbidden.

**A background collector feeding the block from a queue.** §65.7, verbatim: not a streaming fix,
and forbidden.

**Restricting the fix to blocks that are single expressions** — `each { @ }`, `each { @.name }` —
and leaving statement blocks captured. It would have passed both failure proofs and satisfied
neither §25.1 nor §2.5, and it would have left two `each`es in the language with different
streaming semantics and no way for a user to tell which one they had.
