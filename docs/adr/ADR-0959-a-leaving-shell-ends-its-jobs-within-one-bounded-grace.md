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

At exit the shell ends every native job still in its table (since ADR-0958 every backgrounded
line is one, with an evaluator of its own) in four steps, for all jobs together:

1. **Ask.** Each running job's evaluator is cancelled, and the process group it waits on gets
   `SIGTERM` followed by `SIGCONT`, so a stopped program can act on the request.
2. **One grace period** (2 s) shared by all jobs. While it runs, the request is repeated for the
   jobs still running, which reaches a program a job started in the instant after the first one.
3. **Kill.** The groups of the jobs still running then get `SIGKILL`.
4. **Collect.** The evaluators are given a bounded moment (1 s) to reap what was killed — the
   job's own executor collects its children, as it does for any program that ends — and the shell
   exits. A job that has not ended by then is abandoned with the process: an evaluator thread
   ends with it.

Exit therefore takes at most one grace period plus the collection bound, whatever the number of
jobs; jobs that end on the request cost no more than the time they take.

**Ownership.** A group is signalled only through the job's `Canceller`, which knows a group only
while the job's evaluator is waiting on it and forgets it under the same lock in the same step that
collects its last member (review R10). A group whose members have all been collected, or whose id
the kernel has handed out again, is therefore never signalled; a group that disappears between two
looks is no error, and nothing here can panic. Jobs already collected — by `fg`, by `kill %N` once
they ended — are no longer in the table and are not touched.

**Scope.** This is the shutdown of the jobs the shell already stopped at exit. Program jobs
started with `&` in the shell's own executor (not inside a native job) are not signalled at exit,
as before; whether they should be is a separate question this ADR does not decide. `kill %N` keeps
its own bounded wait and its rule that a job that has not ended stays listed (review C2).

## Consequences

Easy: no program a native job started outlives the shell because it ignores `SIGTERM`; leaving a
shell with any number of stubborn jobs takes about three seconds at most, not two per job.

Hard: a program that needs more than two seconds to shut down cleanly after `SIGTERM` is killed
when the shell exits. A job meant to outlive the shell has to be started so that it is not a job
(`setsid`, a service manager) — Ono offers no `disown`.

Encoded by `crates/ono-cli/tests/jobs_shutdown.rs`:
`::should_leave_no_process_of_a_job_behind_when_the_shell_exits_and_its_child_ignores_term`,
`::should_end_several_stubborn_jobs_within_one_grace_period_rather_than_one_each`,
`::should_end_a_job_whose_program_was_stopped_when_the_shell_exits`,
`::should_leave_promptly_when_its_jobs_end_on_the_signal`; acceptance case `394`.

## Alternatives considered

**Keep the per-job wait and add `SIGKILL` after it.** It ends the orphans but keeps the exit at two
seconds per stubborn job, which the issue names as the second defect.

**Kill at once, without a grace period.** A program that handles `SIGTERM` to save its state would
never get to; the request stays first.

**Hang up the jobs (`SIGHUP`) like an interactive shell's terminal would.** A program that ignores
`SIGTERM` usually ignores `SIGHUP` too; the escalation that ends what remains is `SIGKILL` either
way, and `SIGTERM` is the signal `kill %N` already uses for the same request.
