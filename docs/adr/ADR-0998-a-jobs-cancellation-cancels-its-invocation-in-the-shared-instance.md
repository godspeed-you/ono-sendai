# ADR-0998: A job's cancellation cancels its invocation in the session's instance

- Status: accepted
- Date: 2026-11-10
- Spec refs: spec §18.4, §18.5, §31.8, §31.14; ADR-0952, ADR-0958, ADR-0959, ADR-0995
- Decided by: agent (autonomous)

## Context

ADR-0995 made a background job run packages in the session's one KUANG/11 instance. Its
consequence for a job's end assumed that an invocation a job still had running "ends with the
shutdown" of the instance when the shell leaves. That held only for the shell's exit, and only
after a full grace period: nothing ended the invocation before.

A contributed command (`echo:count-forever`, or any command reached through the registry) and a
bounded contributed target are read to their end with `RunningInvocation::collect`. That read
watched no interrupt at all. So:

- `kill %N` set the job's cancellation flag, which the job's evaluator never read while it was
  inside the invocation. The job stayed `running`, `kill` said "job %1 has not stopped yet", the
  instance stayed `active`.
- The job's invocation went on streaming — and being collected — for the rest of the shell's life:
  the ~7 s of user CPU a shell with such a job spent leaving was the package and the host
  exchanging values at full speed until the instance was shut down, not a busy wait in the shell.
- A leaving shell (ADR-0959) waited out its whole grace period for a job that could never settle,
  then unloaded the instance under it.

Before ADR-0995 the same scenario never got this far: the job's own host had not loaded the
package, so `echo:count-forever` in a job was refused and the job ended at once. The missing
interrupt was there before; sharing the instance made it reachable.

## Decision

**An invocation is read under the interrupt of the evaluator that runs it.** While a contributed
command or a bounded contributed target is read to its end, the read races the evaluator's
interrupt — the same predicate every native pipeline polls (`interrupt_reached`): in a job, the
job's own cancellation (`kill %N`, Ctrl-C under `fg`, a leaving shell, a termination signal); in
the foreground, Ctrl-C. When the interrupt wins:

1. the invocation's stream is cancelled through the supervisor (`RunningInvocation::cancel`,
   `streams.cancel` with reason `operator`, spec §31.14) — the existing per-invocation
   cancellation, no new lifecycle model;
2. the package is given `shutdown::GRACE` (2 s) to answer the cancellation, so that once the line
   has unwound the instance no longer counts the invocation (`get plugin` reads `loaded` again);
   a package that does not answer is left to the supervisor, and `kill`'s own bounded wait then
   reports the job as not stopped yet, which is then true;
3. the line ends as an interrupted one (`stream.cancelled`, status 130), as a native pipeline
   does.

Only that invocation is cancelled. The instance, the foreground's invocations and other jobs'
invocations in it run on. Unbounded targets already went through `stream_of`, which cancels on the
pipeline's own cancellation, and are unchanged.

## Consequences

Easy: `kill %N` ends a job blocked on a package as it ends one blocked on a program; the instance
stays loaded and usable; a shell leaving with such a job leaves as soon as the job has unwound
instead of after a full grace period, and burns no CPU meanwhile. Ctrl-C now also ends a
contributed command read in the foreground, which nothing did before.

Hard: a command's answer is still collected whole before the rest of the line runs, so an endless
contributed command in the foreground grows the shell's memory until it is interrupted; making a
command's answer a stream is a contribution-contract change (ADR-0588 did it for targets only),
outside this decision.

Encoded by `crates/ono-cli/tests/plugin_instances.rs`:
`::should_end_a_killed_jobs_invocation_and_leave_the_instance_loaded_and_usable`,
`::should_end_only_the_killed_jobs_invocation_when_another_job_and_the_shell_use_the_instance`;
acceptance case `157-killing-a-job-cancels-its-package-invocation`.

## Alternatives considered

**Give each job an instance of its own again, killed with the job.** That is the second lifecycle
model ADR-0995 rejected: two instances of one package, unlisted, unloaded without a trail.

**Cancel every invocation in the instance, or shut the instance down, when a job is killed.** It
ends what the foreground and other jobs are running in it; the cancellation is the job's.

**Drop the invocation without cancelling it.** Spec §31.14 requires the cancel to be delivered; a
package left waiting for demand never finishes its handler and the instance stays `active`.

**Wait for the package's answer without a bound.** A package that ignores the cancellation would
hold the job, and with it the shell's exit, for ever.
