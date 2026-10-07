# ADR-0952: A job that runs a block has an evaluator of its own

- Status: accepted
- Date: 2026-10-07
- Spec refs: spec §18.1, §18.4, §18.5, §19.4, §43; v0.4.1 §23.4, §26.3, §28.3, §28.4;
  ADR-0008 (exit statuses), ADR-0024 (a backgrounded watch is a job), ADR-0071 §4 (`kill %N`),
  ADR-0480 (the block bridge), ADR-0782 (one interrupt per line)
- Decided by: agent (autonomous)

## Context

ADR-0480 made `each { … }` a stage that asks the thread owning the session to run its block, and
named what that left open: *"a backgrounded `each { … }` is still not supported. `run_background`
has no session to ask"* (issue #193). A backgrounded native pipeline was a task on the runtime
driving a stream chain; a block in it reached the transform engine and failed per item, and a
pipeline that also held a program was refused outright — *"a background job cannot mix native
stages with external programs yet"*.

Spec §18.4 asks that a backgrounded native pipeline be a job — listed, addressable, stoppable —
and ADR-0024 that it never be a hidden thread. A block holds statements; only an evaluator runs
statements; and the foreground's evaluator is busy with the next line.

## Decision

### 1. The job gets an evaluator of its own

A backgrounded line that holds an `each { … }` block, a user function call, or a program beside its
native stages becomes a job **with an evaluator of its own**, running on a thread of its own. A
line of native stages without a block keeps ADR-0024's stream-chain task, which folds a live
stream into the row model `fg` repaints.

The job's session is built on its thread from a `JobSnapshot` the shell takes when the line is
backgrounded (`Session::fork_for_job`):

- **copied, so they are the job's own:** the working directory, the environment, the scopes and
  the functions and aliases defined in them, the context frames, the retained results and the
  settings;
- **shared, because they are resources rather than state:** the pipeline runtime (a provider's
  sockets belong to the reactor that opened them), the providers, the adapters and the theme;
- **not carried:** links — a job inside a link frame is refused, because a connection cannot be
  shared with a job — the job table, open captures, and the terminal.

### 2. What a job's block binds stays in the job

The job runs on a copy of the scopes. A `let` in its blocks rebinds the copy, item after item —
the same rebinding a foreground block performs (ADR-0950) — and the session the job was started
from is never written by it. A job is started from what its line could read when it was typed,
and what it does cannot reach back into a shell that has moved on. That is §26.3's deterministic
binding, made deterministic across threads by not sharing the binding at all.

### 3. A job does not own the terminal or its interrupt

- Its executor is detached: its programs run in process groups of their own, are never handed the
  terminal, and read an empty standard input unless the pipeline gives them one; a native head
  stage of the job never reads the shell's standard input either (spec §18.4).
- Its interrupt is its own. The evaluator thread reads a cancellation flag where the foreground
  reads the terminal's Ctrl-C, and never takes the foreground's interrupt note away from the line
  that owns it (ADR-0782). `kill %N` sets the flag and sends `SIGTERM` to the process group the
  job's evaluator is waiting on; Ctrl-C under `fg %N` does the same with `SIGINT`. Either way the
  pipeline unwinds as an interrupted foreground pipeline does, its source is read no further, and
  its child is reaped by the job's own executor (§28.3, §28.4). Stopping re-signals while the job
  has not ended, so a child started in the instant after the first signal is stopped too, and the
  shell waits a bounded moment for that, never indefinitely.
- A shell that exits stops its evaluator jobs the same way.

### 4. What a job produces is collected by `fg`

The job's line runs under one command's capture: what it would have shown is what it hands over,
bounded by §23.4's ceiling while nobody collects it. `fg %N` waits for such a job — the shell holds
the terminal meanwhile — then shows what it produced, reports its failures through the same
reporter the foreground uses (code, name, help; spec §43), and ends with the status the line ended
with (ADR-0008), or `130` when Ctrl-C ended it. `jobs` and `get job` list it like any native job;
`get job` reports the status it recorded.

## Consequences

Easy: `src | each { … } &` is a job; so is a backgrounded line with a function call or a program in
it, which removes the "cannot mix native stages with external programs" refusal. A job's block can
run anything a foreground block can, including programs and nested pipelines, because it has an
evaluator.

Hard:

- A job holds a copy of the session's scopes and retained results for as long as it runs. Large
  retained results are shared values rather than deep copies, but the copy is real.
- A job with an evaluator collects its result rather than folding a live stream into rows, so a
  backgrounded *live* pipeline with a block in it is refused at its end like any captured live
  stream unless it ends in a block that shows its own results or is bounded. ADR-0024's live jobs
  are unchanged.
- A job inside a link frame is refused. Carrying a remote connection into a second evaluator is a
  separate design.

Encoded by `crates/ono-cli/tests/jobs_blocks.rs`:
`::should_run_a_backgrounded_block_as_a_job_that_fg_collects`,
`::should_keep_what_a_background_block_rebinds_inside_the_job`,
`::should_report_a_background_blocks_failure_structured_with_its_status`,
`::should_publish_a_finished_background_block_in_get_job_with_its_status`,
`::should_stop_a_background_block_and_its_child_when_the_job_is_killed`, and the two PTY proofs
`::should_not_let_a_background_block_read_the_terminal` and
`::should_interrupt_a_foregrounded_block_job_with_ctrl_c_and_reap_its_child`; acceptance case
`361`.

## Alternatives considered

**Let the foreground's evaluator answer the job's blocks between prompts.** No evaluator would be
free while a foreground line runs, and none at all in `ono -c` once the script has moved on; a job
would stall exactly when the user was busy.

**Share the session behind a lock.** ADR-0480 rejected it for the foreground block bridge, and the
reasons hold harder here: a lock held across arbitrary user code on two threads, and a job that
could rebind the foreground's variables while the foreground is reading them.

**Restrict background blocks to single expressions the transform engine can evaluate.** ADR-0480
rejected the same restriction for the foreground: two `each`es with different semantics and no way
to tell which one a user has.
