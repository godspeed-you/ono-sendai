# ADR-0955: A background job does not move the working directory

- Status: accepted
- Date: 2026-10-07
- Spec refs: spec §14.2, §18.4; v0.4 §30.2–§30.4; v0.4.1 §31.3; ADR-0952 §1
- Decided by: agent (autonomous)

## Context

ADR-0952 §1 lists the working directory among what a job's evaluator *copies*, "so they are the
job's own". The copy is the session's `cwd`; the session moves the process with it
(`EnvironmentState::set_cwd`, v0.4.1 §31.3), because every native command that takes a relative
path — `find file .`, `get file`, `remove file *.tmp` after glob expansion, a change target being
canonicalized — resolves it through the kernel, and the kernel keeps one working directory per
process.

A job's evaluator is a thread of the shell's own process. A `cd`, `enter dir …`, `enter <dir>` or
a `leave` that restores a directory, run by a job, therefore moved the foreground's kernel
directory while the foreground session's `cwd` stayed put, and the foreground's next relative path
was resolved wherever the job had gone (release review R2): `fn build() { cd ~/proj; make };
build &` followed by `find file .` listed `~/proj`.

Two honest shapes were on the table: a job-local working directory, or no movement in a job.

## Decision

A background job's session does not change the working directory. `cd`, `enter dir <path>`,
`enter <path>` naming a directory, and a `leave` that would restore a directory are refused in a
job with `type.mismatch` — the code the other "a background job cannot … yet" refusal (a job
inside a link frame, ADR-0952) uses — naming the statement and saying how to give a program its
directory instead (`make -C <dir>`, `sh -c 'cd <dir> && …'`). The refusal comes before anything
moves: the spatial place, `PWD` and the frame stack stay as they were, and the job ends with the
failure like any other failure in it (`fg` reports it, status 1).

`Session::set_cwd` never moves the process from a job's session, so a path that missed the
refusal still cannot move the foreground.

The job keeps the directory it was started in: its programs run there (they are given it
explicitly, as every child is), and its glob expansion and relative `cd` resolution read it.

ADR-0952 §1 stays as written for everything else it copies; for the working directory, "copied"
now means "fixed at the copy".

## Consequences

Easy: nothing a job does can move the foreground; the directory a job runs in is the one shown
when it was started.

Hard:

- `fn build() { cd ~/proj; make }; build &` is refused where a POSIX shell's subshell would run
  it. The refusal says what to write instead.
- A native command in a job with a relative path still resolves it through the process's
  directory, which is the *foreground's current* one: when the foreground moves after the job
  started, the job's relative native paths follow the foreground, not the job. That was so before
  this ADR and is not fixed by it — a working directory per evaluator needs every provider to be
  handed the directory it resolves against. Recorded as a finding outside this change.

Encoded by `crates/ono-cli/tests/jobs_blocks.rs`:
`::should_leave_the_shells_working_directory_where_it_was_when_a_job_tries_to_move`.

## Alternatives considered

**A job-local working directory: the job's `cd` moves only its session's `cwd`.** Its programs
would run in the right place, since every child is given the session's directory explicitly. Its
native commands would not: `fn clean() { cd /tmp/build; remove file *.o }; clean &` would expand
`*.o` in `/tmp/build` and remove the relative names wherever the foreground stands. A job whose
programs and native commands disagree about where they are is worse than a refusal.

**A per-thread filesystem context (`unshare(CLONE_FS)` on the job's thread).** Native stages run
as tasks on the shared pipeline runtime's worker threads (ADR-0952 shares the runtime because a
provider's sockets belong to its reactor), not on the job's thread, so they would not see it.

**A job of its own process.** What a POSIX shell does, and what ADR-0952 rejected: a block's
values, the retained results and the providers would all have to cross a process boundary.
