# ADR-0959: A leaving shell ends its jobs within one bounded grace period

- Status: accepted
- Date: 2026-10-08
- Spec refs: spec §18.1, §18.4; v0.4.1 §28.3, §28.4; ADR-0952 §3, ADR-0958; review R10 (v0.6.3)
- Decided by: agent (autonomous)

## Context

ADR-0952 §3 says a shell that exits stops its evaluator jobs. `Session::drop` did it one job at a
time: `SIGTERM` to the process group the job's evaluator was waiting on, the same signal again
every 10 ms for up to two seconds, then on to the next job — and no `SIGKILL`. A job's programs
run in process groups of their own (`Executor::detached()`), so the terminal's hangup never
reaches them. A program that ignored or trapped `SIGTERM` outlived the shell as an orphan, and
every such job added two seconds to the exit (issue #303).

The invariant: processes Ono still owns as jobs at shell shutdown must not become orphaned merely
because they ignore `SIGTERM`, and shutdown stays globally bounded. Letting jobs survive the shell
(`disown`, a survive-exit mode) is not this decision.

## Decision

**Every process group the shell owns ends when the shell does, however it ends, within one
bounded grace period.** There is no `disown` and no mode in which a job survives the shell.

**What is owned.** Every executor of the process registers its cells (`ono_process::owned_groups`):
the group it waits on in the foreground — a program the line runs, a program whose records the
shell decodes as it runs (ADR-0059, now owned like any other: review R5), a native job's current
program — and each background job's group: a program backgrounded at the prompt (`prog &`), one a
function or a pipeline backgrounded, and those a job's own evaluator backgrounds. A cell names its
group while one of its members is uncollected and is cleared under its lock in the same step that
collects the last one (review R10 of v0.6.3), so a group that has ended, or whose id the kernel has
handed out again, is never signalled. `fg`, `kill %N` and `wait` take a job out of the table as
before; what is no longer in a cell is not touched.

**How it ends**, the same escalation whichever way the shell leaves:

1. **Ask once.** Each owned group gets `SIGTERM` and then `SIGCONT`, so a stopped program can act
   on it — once per group, not once per look (review R6). A native job's evaluator is cancelled.
2. **One grace period** (2 s) shared by every group. Ended members are collected as they end; a
   group that appears meanwhile — a job starting its next program in the instant before it saw
   the cancellation — is asked too.
3. **Kill** what is left with `SIGKILL`.
4. **Collect** for a bounded moment (1 s), then leave. A job whose evaluator has not ended by then
   is abandoned with the process; its threads end with it.

Leaving takes at most the two bounds, whatever the number of jobs; jobs that end on the request
cost only the time they take.

**Every way out runs it.** The end of a script or of `-c`, `exit`, and end of input run it from
`Session::drop`. A signal that ends the shell — `SIGHUP` (its terminal went away), `SIGTERM`, and
`SIGINT` outside an interactive session, where nothing else takes it — is noted by a handler that
only writes the signal's number into a pipe; a thread reading it runs the same escalation over
everything the process owns, restores the terminal's settings, and exits with `128 + N` (review
R2). The main thread does not exit first with a status of its own. An interactive shell's `SIGINT`
stays Ctrl-C (spec §18.5). This replaces ADR-0160's "`SIGHUP` keeps its default disposition" for
the shell process: it still ends on a hangup, after ending its jobs.

A job's own session, when its evaluator ends, ends its own jobs and its own executor's groups the
same way.

**Detaching** is starting a program so that it is not a job: `setsid --fork prog` runs it in a
session and a process group of its own, which no executor of the shell holds, and it outlives the
shell. `nohup prog &` is still a job — it ignores `SIGHUP` only — and is ended like one; so is a
program that merely traps `SIGTERM`.

`kill %N` keeps its own bounded wait and its rule that a job that has not ended stays listed
(review C2), now asking each group once.

## Consequences

Easy: no program the shell started as a job outlives it by accident; leaving with any number of
stubborn jobs takes about three seconds at most; a closed terminal or a `kill` of the shell ends
its jobs as `exit` does.

Hard:

- **Behaviour change:** a script that backgrounds a server and ends — `ono -c 'server &'` — no
  longer leaves the server running; it has to be detached (`setsid --fork server`). This is the
  rule for this shell: a job is the shell's to end.
- A program that needs more than two seconds to shut down cleanly after `SIGTERM` is killed when
  the shell leaves.
- A signal-driven exit skips the rest of a normal exit — history flushing, the audit write of
  spec §31.37 and hanging up links (ADR-0161) — because the main thread may be anywhere.

Encoded by `crates/ono-cli/tests/jobs_shutdown.rs`:
`::should_leave_no_process_of_a_job_behind_when_the_shell_exits_and_its_child_ignores_term`,
`::should_end_several_stubborn_jobs_within_one_grace_period_rather_than_one_each`,
`::should_end_a_job_whose_program_was_stopped_when_the_shell_exits`,
`::should_leave_promptly_when_its_jobs_end_on_the_signal`,
`::should_end_a_program_backgrounded_at_the_prompt_when_the_shell_exits`,
`::should_leave_a_program_detached_with_setsid_fork_running_after_the_shell_exits`,
`::should_end_an_adapted_program_a_job_runs_when_the_shell_exits`,
`::should_ask_a_stubborn_program_to_stop_once_rather_than_over_and_over`,
`::should_end_its_jobs_when_the_shell_is_sent_sigterm`,
`::should_end_its_jobs_when_the_shells_terminal_hangs_up`,
`::should_end_its_jobs_when_a_script_is_interrupted`; acceptance case `394`.

## Alternatives considered

**Keep the per-job wait and add `SIGKILL` after it.** It ends the orphans but keeps the exit at two
seconds per stubborn job.

**Kill at once, without a grace period.** A program that handles `SIGTERM` to save its state would
never get to; the request stays first.

**Leave programs backgrounded at the prompt running, as a non-interactive POSIX shell does.** Two
kinds of job that end differently at exit, decided by how the line was spelled (`prog &` against
`{ prog } &`), is exactly the accident the issue is about. Detaching is explicit instead.

**Run the escalation in the signal handler.** It takes seconds and calls code that is not
async-signal-safe; the handler writes one byte and nothing else.

**Hang up the jobs (`SIGHUP`) like an interactive shell's terminal would.** A program that ignores
`SIGTERM` usually ignores `SIGHUP` too; the escalation that ends what remains is `SIGKILL` either
way.
