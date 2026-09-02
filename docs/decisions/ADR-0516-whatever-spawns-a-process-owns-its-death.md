# ADR-0516: Whatever spawns a process owns its death

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §2.4 (bounded means bounded in relevant dimensions), §38.1 (a test reports
  execution truth), §39.3 (a shared helper documents its timeout behaviour), §65.10
- Decided by: agent (autonomous)

## Context

A sweep on 2026-09-02 killed **331** leaked processes on the development machine, every one of
them `/bin/sh /tmp/ono-test-<pid>-N/journalctl --output=json --no-pager --follow`, the oldest five
days and two hours old. Each `/tmp/ono-test-*` directory had already been removed, so each was a
follower pointing at a script that no longer existed, parked in `do_wait` with no child. Nothing
in the repository would have killed them.

It is the same class the spatial work found while writing issue #22's fixture — two `ono` shells
holding a pipeline open for seven hours at about 2.2 GiB each — and the answer there was
`ono_testkit::run_bounded`, which kills *and reaps* at the deadline. The two remaining spawn
paths never got it:

* **`ono_process::PtySession` had no `Drop` at all.** It owns a session leader on its own
  terminal, so a caller that dropped one without waiting orphaned the leader and everything under
  it. Every PTY test in the tree drops its session at the end of the test.
* **`ono_testkit::Shell::try_run` reported the overrun and walked away.** The child was moved into
  a worker thread calling `wait_with_output`, which has no deadline; the test thread timed out on
  a channel and returned, and the worker went on waiting for a program that was never going to
  finish.

A suite that leaves 331 processes behind over five days is not reporting its own execution
truthfully, whatever its exit code says. That makes this a test-truthfulness defect (§38.1) as
much as a resource one (§2.4).

## Decision

**A type that starts a process ends it: killed and reaped, on every path out, including the path
where nobody asked.**

`PtySession` gains a `Drop` that signals `SIGKILL` to the **process group** — the leader is not
the only thing running under that terminal — and then waits, so a killed child cannot become a
zombie the next `waitpid` in the process has to step over. A caller that already waited holds a
status, and the `Drop` does nothing; the shell's own `relay()` path is therefore unchanged.

`Shell::try_run` keeps the child, drains both pipes on workers the way `run_bounded` does, and
polls the deadline itself. On overrun it kills, reaps, and *then* reports `RunError::Timeout`.

The two helpers still differ in what they do about an overrun, and ADR-0431's reason for that
stands: `Shell`'s subject is expected to answer, so it panics; `run_bounded`'s subject *is* the
hang, so it returns what the run managed to say. What they no longer differ in is whether the
child survives them.

## Consequences

Easy: a PTY test cannot leak its session any more, and an overrunning `Shell` run cannot leak its
child. Both are asserted by reading `/proc/<pid>/stat` for the pid the fixture recorded, so the
proof is the same observation the sweep made by hand.

Hard, and stated rather than implied: `Shell::try_run` kills the **child**, not a process group,
because `ono-testkit` is `#![forbid(unsafe_code)]` and putting the child in its own group needs
`CommandExt::pre_exec`. A grandchild the child started in a group of its own therefore survives —
which is a narrower hole than the one that was there, and a real one. The PTY path does not have
it: killing the session leader's group and closing the master leaves the kernel to hang up the
terminal on anything still attached. Closing the remaining half means either an `unsafe` block
behind a documented `// SAFETY:` in the testkit, or spawning through `ono-process`, which already
has the machinery; neither belongs in a fix.

Encoded by: `crates/ono-process/tests/terminal_control.rs::should_kill_and_reap_the_terminal_session_when_the_session_is_dropped`,
`crates/ono-testkit/tests/harness.rs::should_kill_and_reap_the_program_when_a_run_exceeds_its_budget`.

## Alternatives considered

**A sweep at the end of the suite.** A `Drop` that runs where the process was started is local,
composes, and cannot be forgotten by the next suite. A sweep is a second mechanism that has to
know every name a fixture might have used, and it is what somebody did by hand on 2026-09-02.

**Have the fixtures kill their own children instead.** The five PTY suites would each need the
same code, which is the divergence §39.2 exists to prevent (ADR-0515). The type that holds the
descriptor is the one that knows the pid.

**SIGTERM rather than SIGKILL in `Drop`.** A `Drop` cannot wait politely — the caller is on its
way out, and a fixture that took a second to shut down would slow every PTY test by a second. The
programs under these terminals are test children with nothing to flush.
