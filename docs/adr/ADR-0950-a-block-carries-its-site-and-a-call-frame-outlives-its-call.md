# ADR-0950: A block carries its site, and a call's frame outlives the call

- Status: accepted
- Date: 2026-10-07
- Spec refs: v0.4.1 §2.5, §25.1–§25.5, §26.1–§26.3, §28.2, §28.3; spec §19.3, §19.4;
  ADR-0070 (the caller's output context), ADR-0480 (the block bridge), ADR-0481 (a body that is one
  pipeline is one pipeline)
- Decided by: agent (autonomous)

## Context

ADR-0481 continued a function body that is one native pipeline as a stream, and named two
limitations of its own. One of them is issue #192: *a body containing `each { … }` still collects*,
because ADR-0480's block bridge identified the block to run by its index in the stage list the
driver holds, and a body's stages are in another list, written in another source.

Working on it surfaced a defect in the continuation itself. The stages after a streamed call ran
while the call's invocation scope was still on the session, so a block among the *caller's* stages
read the callee's parameters: `fn f(x) { … }; let x = "outer"; f "inner" | each { $x }` printed
`inner`. v0.4.1 §26.3 forbids exactly that — *"streaming a block/function MUST NOT let lexical
scope references outlive their owning scope unsafely"* — and it is the same question #192 has to
answer from the other side: a body's block runs while the caller's pipeline drains, after the
call has returned, and must still read the call's parameters.

## Decision

### 1. A block request carries the block's site

`BlockRequest` no longer carries a stage index. It carries a `BlockSite`: the block itself, the
source its spans index (the caller's line, or the source the function was declared in), whether a
later stage consumes what it emits (ADR-0070 point 3), and the invocation scopes it was written in.
Whichever driver receives the request can run it, so one driver answers every block of one
pipeline — the caller's own and those of every body assembled into it. The channel is still one
request deep (§25.3, §65.7); a stream assembled before a segment hands its channel to that
segment's driver instead of the driver opening a second one.

### 2. A call's frame is detached when its body is assembled

A call whose body is assembled into the caller's stream pushes its invocation scope, binds its
parameters, assembles the body — every native stage snapshots the scope it reads, as ADR-0481 §3
already did — and then **takes the invocation scope off the session** (`Session::detach_scopes`)
into a cell shared by the block sites of that body. The caller's later stages therefore read the
caller's bindings, and the call is over in the sense §26.3 means.

When the driver runs a body's block for one item it puts the frames of the calls the block was
written in back on top of the session, outermost first, runs the item, and takes them off again.
So a parameter is what the block reads, and a `let` that advances it (ADR-0119's rebinding) is what
the next item reads — the mutation semantics of a collected body, kept by keeping the frame rather
than a copy of it. Bindings below the frame are the live session's, as they are for any call.

### 3. `return` in a streamed body ends the function's stream

A `return` inside a body's block ends the function, not the caller's pipeline: the item's values
and the returned value (when it is not `null`) are forwarded, and the block stage stops reading,
which stops the body's source (§25.5). That is what a collected body produced. Where the returning
block is *not* the body's last stage, a streamed body would send the returned value through the
stages after the block, which a collected body never did, so such a body is not assembled and
keeps collecting — the shape check names the reason.

### 4. Which bodies are assembled

A body is assembled when it is one pipeline (`continuable_body`) and every stage of it is a native
stage that hands objects on, an `each { … }` block, or a call whose own body passes the same
check — decided statically, from the contracts and declarations, so `explain` answers the same
question (§22.4). Heads the shell answers itself (`get config`, `at`, `plan`, link and plugin
management, builtins), aliases, redirections, serializers and programs are not, and recursion is
refused rather than assembled without end. Everything else collects, as ADR-0481 left it.

## Consequences

Easy: `fn f() { src | each { … } }` used as `f | take 1` answers from the first value the source
produced, exactly as `where` in the same place does, and `explain` says it streams. The scope leak
is gone. ADR-0481's other limitation — a call between two stages — becomes a question of routing,
because the assembler already takes an input stream (ADR-0951).

Hard: a body's frame lives as long as the stream it belongs to rather than as long as the call,
which is the point and also the cost: a pipeline that holds a streamed body holds its parameters.
The frame is attached for one item at a time, so nothing else can observe it.

Encoded by `crates/ono-cli/tests/streaming.rs`:
`::should_keep_the_callers_binding_in_the_stages_after_a_streamed_call`,
`::should_stream_a_function_whose_body_runs_a_block_like_one_whose_body_filters`,
`::should_run_a_streamed_body_block_in_the_invocation_scope_of_its_call`,
`::should_end_a_streamed_body_with_the_value_its_block_returns`,
`::should_say_in_explain_that_a_body_running_a_block_streams`, and the ADR-0480/0481 tests in the
same file, unchanged.

## Alternatives considered

**Keep the index and give each body its own driver.** A second driver would have to run inside the
first one's drain — the nested `block_on` ADR-0480 exists to avoid — or on another thread, which
needs a session that travels.

**Snapshot the caller-visible bindings into the site.** Simpler than detaching a frame, and it
breaks `let` rebinding: a block that advances a counter of the enclosing function would advance a
copy, and the next item would read the original.

**Leave the invocation scope on the session for the drain** — what ADR-0481 did. It is the leak.
