# ADR-0951: A function between two stages reads its input through its body

- Status: accepted
- Date: 2026-10-07
- Spec refs: spec §6.5, §19.3, §20.2; v0.4.1 §2.5, §22.4, §26.2, §28.2, §65.8; ADR-0011 (the
  resolution order), ADR-0070 (the calling convention), ADR-0481, ADR-0950
- Decided by: agent (autonomous)

## Context

ADR-0011 puts a user function at step 2 of the resolution order, and the evaluator only ever
applied that step to the first stage of a list: `get process | mine | take 1` read `mine` as a
program and failed with `type.mismatch`, and `explain` reported "`mine` is not a native command"
(issue #191). ADR-0481 recorded the gap and why it was left: giving a function an input stream is
a language feature, not a streaming repair.

The specification gives functions no input of their own. Spec §19.3 declares
`fn hot-processes(limit: Float = 20) -> Stream<Process> { get process | where cpu > $limit }` and
shows it at the head. A call between two stages has to mean *something* the language can say
without a new construct, and it has to keep streaming — §2.5's definition and §28.2's
backpressure apply to it as to any stage.

## Decision

### 1. Resolution does not depend on position

A stage whose head names a user function (bare, or `fn:`) is that function in every position of a
pipeline. The evaluator looks for calls before anything else claims the list, except that
`explain` in front of a pipeline explains it rather than running it, and a backgrounded line is the
job's to run.

### 2. The input arrives at the body's first stage

A call after another stage is assembled where it stands: its arguments are read in the caller's
scope, its parameters bound in an invocation scope of its own, and its **body's first stage reads
the stream in front of the call** — spliced, not collected, so backpressure and cancellation cross
the call exactly as they cross any other stage, and the frame outlives the call as ADR-0950
describes. So `fn mine() { where pid > 0 }` between two stages filters the stream that reaches it,
and `fn doubled() { each { @ * 2 } }` maps it.

This requires a body that is one pipeline whose first stage reads a stream: a transform, an
`each { … }` block, or a call of the same shape. That is checked statically, before anything runs.

### 3. Any other body is refused, by name and shape

A body of several statements, a body whose first stage produces values of its own (`get process`),
a serializer, a program or a redirection cannot take the stream, and the call is refused with
`type.mismatch` (`Ono-Sendai-E0201`) naming the function and the reason, before any stage is bound
or spawned — so an unbounded source in front of it is never waited on (§65.8). `explain` states
the same reason on the call's stage.

### 4. What stands in front of the first such call

The stages before the first call between two stages are assembled too when they can be — a native
producer, a streamed call at the head. When they cannot — a value at the head, a program, a
shell-answered command, a head function whose body collects — they run first under a capture,
and their values seed the rest. That collection is §26.2's permitted one: it is a capture, so it
is charged to §23.2's ceiling and an unbounded producer in front of it is refused rather than
waited for. Everything from the first call between two stages up to the last call is assembled,
and the stages after the last call run as the rest of the pipeline over that stream.

### 5. `explain` names the call

The planner is told, for each head word that is a user function, where it was declared and whether
a call of it streams at the head and after a stage. A call's stage reports
`resolution  user function \`mine\` — step 2 of the resolution order`, whether it streams, and
either *"its body reads the stream in front of the call and streams into the stages after it"* or
*"it cannot read the stream in front of it: <reason>"*.

## Consequences

Easy: a function is a stage wherever it is written, so a user can name a filter or a block and
use it the way they use `where`: `get process | busy | take 5`. A call between two stages costs
nothing: no collection, no capture budget.

Hard: a body that wants to do several things with its input — bind it, then filter it — has no
way to say so; the language has no input variable, and inventing one belongs to a language
release, not to a streaming repair. The refusal says which shape is accepted.

Encoded by `crates/ono-cli/tests/streaming.rs`:
`::should_hand_the_stream_to_a_function_called_between_two_stages`,
`::should_answer_through_a_function_between_two_stages_before_the_source_ends`,
`::should_stream_through_a_call_at_the_head_and_a_call_between_two_stages`,
`::should_seed_a_call_between_two_stages_with_what_a_program_wrote`,
`::should_refuse_a_call_between_two_stages_whose_body_cannot_read_a_stream`,
`::should_name_a_function_between_two_stages_in_explain_and_say_that_it_streams`, and acceptance
case `360`.

## Alternatives considered

**Collect the input under the materialization budget and seed the body's first pipeline.** It
would accept every body, and it is the accidental capture architecture §26.2 prefers not to
preserve: an unbounded upstream could never pass a call, and "the body's first pipeline" is not a
well-defined place in a body that branches or loops.

**An implicit input variable (`$in`).** A new name in the language's scope rules and its contract,
for a problem the one-pipeline shape already solves for the common case. Left to a language
release.

**Drop the input and run the body as a producer.** What a Unix function that ignores stdin does,
and silent: `get process | mine` would answer with whatever `mine` produces, unrelated to the stage
before it.
