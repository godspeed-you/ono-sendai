# ADR-0481: A function body that is one pipeline is one pipeline

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §2.5, §22.4, §26.1, §26.2, §26.3, §65.8, §65.12; spec §19, §20.2;
  ADR-0011 (the resolution order), ADR-0019, ADR-0070 (the caller's output context),
  ADR-0457 (one command, one capture allowance), ADR-0480 (the streamed `each`)
- Decided by: agent (autonomous)

## Context

v0.4.1 §26.2 states a preference and a fallback in the same breath:

> A function used as a pipeline stage SHOULD be able to stream values to downstream stages when
> the function body itself streams. If function semantics currently require a complete function
> result before continuation, that limitation MUST be explicit in `explain` and MUST have a
> finite-input/budget guard. **The preferred v0.4.1 outcome is streaming continuation rather than
> preservation of an accidental capture architecture.**

`eval.rs::run_function_body` was the accidental capture architecture. A call with stages after it
opened a capture, ran the whole body into it, and handed the collected `Vec<Value>` to
`native::run_seeded`. `fn watched() { tail file x --follow }` used as `watched | take 1` could not
work at all: the body ran under a capture, the capture is not a terminal, and §18.3 refused the
unbounded stream before a value was drawn.

§26.3 constrains any fix: *"streaming a block/function MUST NOT let lexical scope references
outlive their owning scope unsafely"*, and *"the refactor MUST preserve deterministic variable
binding and mutation semantics."*

Two things were tried against the wall before this one. Splicing the body's stages into the
caller's stage list — evaluating `watched | take 1` as though the user had written the body
followed by `| take 1` — founders on spans: a function's stages carry offsets into the source it
was *declared* in, and the caller's carry offsets into the source being run. In a REPL, or with a
function loaded from configuration, those are different strings, and every error message, every
`explain` rendering and every argument expansion that resolves a span would be reading the wrong
text. Running the caller's pipeline in a spawned task while the evaluator produces into it founders
on the session, for the reason ADR-0480 sets out at length.

## Decision

**Where a function body is exactly one native pipeline, it is not collected: it is assembled into
a stream, and the stages after the call read that stream.**

### 1. The shape that can be continued, decided from the contracts

`native::continuable_body` asks whether the body is a single `Statement::Pipeline` with no `&&`/`||`
chain and no `&`. `native::continuable_list` asks whether every stage of it hands objects to the
next: no redirection, no `each { … }` block, no external program, no serializer, and a contract the
registry places. Both are static — no binding, nothing spawned — which is what lets `explain`
answer the same question without running anything (§22.4).

Everything else still collects. That is not a gap left open: a body of several statements has a
result of its own to compute, a serializer has already turned the values into bytes, and a block
belongs to the driver of the pipeline it was written in (ADR-0480).

### 2. What is assembled, and when

`native::stream_segment` binds the body's stages and assembles them into a `ValueStream` **without
draining it**, and `native::run_piped` starts the caller's remaining stages from that stream through
a new `Start::Pipe`. The unbounded-stream refusal of §18.3 lives in the *drain*, not the assembly,
which is precisely why an unbounded body can now be continued while a collected one is still
refused.

Nothing about the caller's half changes. The stream arrives as a `Seed` like any other, so the
block bridge, the interrupt race, the diagnostics counters, the materialization limits and the
result writing are the same code paths a pipeline without a function call takes.

### 3. §26.3 is satisfied by construction, not by care

Every expression the body's stages carry is bound, and the `Scope` they will read is snapshotted,
**while the invocation's scope is still on the session** — `stream_segment` is called from inside
`run_function_body`, between `push_scope` and `pop_scope`. What travels into the asynchronous
producers is the snapshot ADR-0013 already made them take. No stage holds a reference to a binding,
so no lexical reference can outlive the scope that owns it, and the parameter a body reads is the
parameter the call bound however long the stream runs.

### 4. A call that collects says so before it runs

`explain` now names which of the two a call is, in the line that already identified the function:
either *"its body streams into the stages after the call"* or *"its result is collected before the
stages after the call run, so its input must be finite"*. That is §26.2's MUST, and the guard
beside it is the existing §18.3 refusal, which §65.8 requires to arrive early rather than as a
shell that never answers.

## Consequences

Easy: `fn watched() { tail file x --follow }` is a pipeline stage. So is any body built out of
producers and transforms, which is what a function used as a stage almost always is. A pipeline
with a call in it now holds nothing between the call and the stage after it, and the call costs no
capture budget at all — which is the second-order effect worth naming, because §23.4's ceiling is
per shell command and a body that no longer captures no longer spends it.

Hard:

- **A call between two stages is still not a call.** `get process | mine | take 1` reads `mine` as
  an external program, because the language has never given a function an input stream — `call_function`
  is reached only for `list.stages[0]`. Giving functions a pipeline input is a language feature,
  not a streaming repair, and adding one here would be exactly the redesign §65.12 forbids
  combining with a behaviour-preserving change. It is reported to the board rather than done.
- **A body containing `each { … }` still collects.** The block bridge of ADR-0480 identifies a
  block by its position in the stage list the driver holds, and a body's stages are in a different
  list. Making the request carry the block site itself would let both halves stream; it is a small
  change and a real one, and it belongs with H9's decomposition rather than beside this.
- **One test changed its vehicle, not its assertion.**
  `resource_limits.rs::should_accumulate_nested_captures_against_the_one_per_command_ceiling`
  proved §23.4 by nesting a function-body capture inside a substitution's. The function body it
  used is now continued and captures nothing, so the same four records are held twice by a body
  the continuation does not apply to. The ceiling, the payload and every assertion are unchanged;
  the comment says why.
- **`stream_segment` repeats the binding and assembly `run_native_segment` does.** They differ in
  what they do afterwards — one drains, renders and retains; the other hands the stream back — and
  merging them means splitting `run_native_segment` into bind/assemble/drive/write, which is H9's
  work and is reported to it.

Encoded by: `crates/ono-cli/tests/streaming.rs::should_forward_values_from_a_function_as_it_produces_them`,
`::should_keep_a_pipeline_streaming_when_a_function_sits_in_the_middle_of_it`,
`::should_drop_the_invocation_scope_when_the_function_call_ends`,
`::should_say_in_explain_which_calls_stream_and_which_collect`,
`::should_refuse_an_unbounded_body_the_call_would_have_to_collect`.

## Alternatives considered

**Splice the body's stages into the caller's stage list.** The smallest change by line count, and
wrong for a reason that only shows up in the error messages: two stage lists that carry spans into
two different source strings cannot be one stage list. It would also have changed what a
multi-statement body means, since only the last statement can be spliced.

**Spawn the caller's pipeline and produce into it from the evaluator.** The symmetric answer to
ADR-0480's, and the one that would also cover a call between two stages. It needs the whole
downstream — binding, draining, live view, result writing — to be `'static`, which means owning the
provider registry per pipeline and moving `write_result` behind a join handle. Deferred to H9 on
purpose: the seam it needs is the seam H9 is scheduled to cut.

**Leave the capture and satisfy only §26.2's MUST** — the `explain` line and the finite-input
guard, with no continuation. Defensible on a literal reading, since the streaming half is a SHOULD.
Rejected because the same sentence says which outcome is preferred, and because the guard alone
leaves `watched | take 1` over a live source a refusal rather than an answer, which is the thing
§2.5 says streaming means.

**Continue any body by capturing all but its last statement.** It generalises the rule to
multi-statement bodies, and it cannot be undone once the leading statements have run: if they
produce values, those values must precede the stream, and there is nowhere to put them. The single
statement rule is the shape where no such choice arises.
