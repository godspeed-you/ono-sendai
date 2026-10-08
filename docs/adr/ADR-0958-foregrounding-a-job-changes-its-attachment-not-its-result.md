# ADR-0958: Foregrounding a job changes its attachment, not its result

- Status: accepted
- Date: 2026-10-08
- Spec refs: spec §16.5, §18.3, §18.4, §18.5, §20.2, §43; v0.4.1 §21.3, §21.4, §23.4; ADR-0008,
  ADR-0024, ADR-0952 §1 and §4, ADR-0954
- Decided by: agent (autonomous)

## Context

ADR-0952 gave a backgrounded line that holds a block, a function call or a program an evaluator of
its own, and left a line of native stages only on ADR-0024's task: the stream chain driven on the
runtime, every value collected into an uncapped vector, events folded into a row model. `fg` on
such a job while it ran went wrong both ways (issue #301):

- at a terminal it repainted the row model, which plain values never reach, until the job ended —
  and then returned 130 and printed nothing;
- without a terminal it aborted the task, printed what had been collected so far and returned 0.

The task also had no status at all (`fg` and `get job` substituted success, or a guess from its
failures), ran with an empty scope, ignored a redirection, and held a followed file's lines
without bound. The evaluator path had a quieter defect of its own: a job's captured serializer
document or program output became one string value, which `fg` rendered as a one-column table cut
to a cell's width.

The invariant: moving a running job to the foreground changes its attachment, not its result.
Output must not disappear, the completion status must stay truthful, and what a job retains obeys
Ono's capture limits.

## Decision

### 1. A backgrounded line is one kind of job

Every backgrounded line runs on an evaluator of its own (ADR-0952 §1), whatever its stages are.
ADR-0024's stream-chain task is gone, and with it the second way of running, binding, collecting
and reporting a line. ADR-0952 §1's sentence keeping native-only lines on the task no longer holds.
What a backgrounded native line does is what the same line does in the foreground: its scope, its
redirections, its failures and its status are the evaluator's.

Inside a link frame a line of native stages only — which asks providers and nothing else — still
runs on the link: the job is given the link's registry, shared as the session's own providers are.
A line with a block, a call or a program inside a link frame stays refused (ADR-0952), because it
would need the connection itself.

### 2. A job's result is its capture, bounded by §23.4

The job's line runs under one command's capture (ADR-0952 §4), charged to the command capture
ceiling (`limits.command_capture_bytes`, the item ceiling beside it). Past the ceiling the job ends
with the structured `resource` refusal the capture raises, and it keeps nothing beyond it.

A live stream at the end of the job's own line — which the foreground would show in place at a
terminal and refuses anywhere else (spec §18.3) — is the job's to keep instead: events (records of
a `*-event` schema) are folded into the job's table, one row per object, as the live view folds
them; everything else — plain records, projections of events, text — is captured as it arrives,
value by value against the same ceiling. A stream that never ends is never held whole; one that
reaches the ceiling ends there with the refusal. Nothing is cut to a screenful silently. A capture
the line opens itself (`let x = (watch …)`) is not the job's and is refused as in the foreground.

A program's output a capture keeps is charged the same way: it is read no further than the
ceiling leaves room for, and the pipe is then closed, so a program that writes without end gets
`SIGPIPE` and the line the structured refusal. A live stream that the program after it would have
to receive whole — nothing feeds it as it runs, because a capture is open around the line — is
refused (`stream.unbounded_operation`) instead of draining without end.

A redirection names a file whatever is capturing around the line, so a streaming serializer's
lines go to it as they come (ADR-0954) — `watch … | to jsonl > log &` writes while it runs.

### 3. What `fg` shows is what the foreground would have shown

A job's own capture keeps the bytes a serializer or a program wrote as bytes, and `fg` writes them
as they were; values are rendered and retained for `@-1` in the order the job produced them. A
program that wrote nothing contributes nothing. Inside the line, a capture is still a value, with
command substitution's trimmed text (ADR-0072 §4).

### 4. `fg` waits; a terminal only adds a presentation

`fg %N` waits for the job, with or without a terminal; nothing about the job changes because
nobody is watching it, and no consumer aborts it. At a terminal, a job whose table has rows has it
repainted in place while it runs (ADR-0024), at most once a frame. When the job has ended `fg`
shows its result once — the values it kept, and the table's last state where it was not already
on the screen — reports its failures (spec §43), and returns the status the line ended with
(ADR-0008). Ctrl-C under `fg` stops the job and its children and shows what it had kept so far;
`fg` then returns the status the job ended with — 130 when the interrupt cut it short, the line's
own when it answered the interrupt its own way — and 130 for a job that has not ended on it, which
stays listed (review C2). A table already painted is not printed again, but it is retained for
`@-1` like any result `fg` shows. `fg` drops no interrupt: one noted since its line began is the
line's (ADR-0782).

### 6. A failure nobody collects is still reported

A job that ends with a failure while nobody watches is named at the next prompt — number, status,
line — once, and `fg` still shows why. A shell that leaves with such a job uncollected reports its
failures, as the foreground would have, and its status, before it goes.

### 5. A missing status is a failure

A job's status is the one its evaluator recorded. An evaluator that ended without recording one
did not finish its line, and `fg` and `get job` report that as a failure, never as success.

## Consequences

Easy: `cmd &` then `fg` gives the same values, failures and status as `cmd`; a job can be
collected from a script, a pipe or a terminal alike; one code path binds, runs and reports every
job.

Hard:

- A backgrounded native line costs a thread and a copy of the session's state (ADR-0952's
  consequence), where it used to cost a task.
- A native line's binding errors — a glob that matches nothing — are reported when the job is
  collected, or at the next prompt / before the shell leaves (§6), instead of before `[%N]` is
  printed.
- A job ending in a bounded stream still drains it before charging its capture, as the foreground
  does before it renders; the ceiling bounds what is retained, not the drain.
- `fg` on a live job that never ends waits until Ctrl-C or `kill %N`, also without a terminal —
  exactly as `fg` waits for a program that never ends.

Encoded by `crates/ono-cli/tests/jobs_attach.rs`:
`::should_wait_for_a_running_native_job_and_show_all_its_values_when_fg_has_no_terminal`,
`::should_report_a_native_jobs_failure_status_when_fg_collects_it`,
`::should_refuse_structured_past_the_capture_ceiling_when_a_native_job_retains_too_much`,
`::should_end_a_live_native_job_with_the_structured_refusal_when_it_reaches_the_ceiling`,
`::should_collect_a_finished_native_job_once_with_its_status`,
`::should_show_what_a_jobs_serializer_and_programs_wrote_as_the_foreground_would`,
`::should_write_a_live_jobs_lines_to_its_redirection_as_they_come`, and the PTY proofs
`::should_show_every_value_of_a_running_native_job_once_when_it_is_foregrounded_at_a_terminal`
and `::should_end_a_foregrounded_native_job_with_ctrl_c_and_show_what_it_had`; and for the review:
`::should_run_a_backgrounded_native_line_inside_a_link_frame_against_the_link`,
`::should_refuse_past_the_ceiling_when_a_jobs_program_writes_without_end`,
`::should_refuse_a_live_stream_a_job_would_hand_to_a_program_whole`,
`::should_refuse_past_the_ceiling_when_a_live_job_keeps_plain_records`,
`::should_report_a_failed_job_nobody_collected_before_the_shell_ends`, and the PTY proofs
`::should_keep_a_live_jobs_painted_table_as_the_last_result_when_ctrl_c_ends_it`,
`::should_return_the_jobs_own_status_when_it_ends_on_the_ctrl_c_with_one_of_its_own`,
`::should_report_a_job_that_failed_unattended_when_the_prompt_returns`; the repaint of a
backgrounded watch stays encoded by `crates/ono-cli/tests/watch_live.rs`; acceptance case `393`.

## Alternatives considered

**Patch `fg` on the task path: wait instead of aborting, print values after the repaint.** It would
fix the two symptoms and keep the second runner: a status synthesised from failures, an empty
scope, ignored redirections and a collection with a ceiling of its own beside the evaluator's. The
issue is that the two kinds of job meant different things; a third patch on one of them keeps that.

**Stream a running job's values to the terminal as they arrive under `fg`.** A job's capture is the
evaluator's, which hands its values over when the line has ended; showing them as they arrive would
need a second, incremental delivery for jobs only. Waiting and showing them once is what `fg` does
for a block job already, and is exact.

**Keep the stream-chain task for live (unbounded) lines only.** A line's boundedness is known only
once its stages are assembled, inside the run; deciding the runner before that means guessing, and
the guess would bring the divergence back for exactly the jobs that run longest.
