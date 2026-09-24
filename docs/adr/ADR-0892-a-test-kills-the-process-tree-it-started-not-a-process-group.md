# ADR-0892: A test kills the process tree it started, not a process group

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.4.1 §2.4 (bounded means bounded in relevant dimensions), §38.1 (a test reports
  execution truth), §39.3
- Issues: #204, #162
- Related: ADR-0431 (`run_bounded` and `Shell` differ in what an overrun means), ADR-0516
  (whatever spawns a process owns its death)
- Decided by: agent (autonomous)

## Context

ADR-0516 made `ono_testkit::Shell` and `run_bounded` kill and reap the child when a run overruns,
and stated the residual: the *child* is killed, not what it started, because `ono-testkit` is
`#![forbid(unsafe_code)]` and putting the child in a process group of its own was thought to need
`pre_exec` (issue #204). Issue #162 is the rest of the same class: 331, then 158, then 46 leaked
`journalctl --follow` stand-ins, and eight `ono -c '… map --live …'`, left behind by tests that
failed or ended early.

Two facts decide the shape of the fix.

**A process group does not reach what `ono` starts.** `ono` is a job-control shell: the executor
puts every external pipeline in a process group of its own (`ono-process`, `spawn_stage`), and
the journal provider starts `journalctl` with `process_group(0)` so a Ctrl-C reaches the shell's
pipeline first. Killing the group of the `ono` a test started — which is what #204 asks for —
kills `ono` and nothing it ran. The leaked followers of #162 were exactly such processes: the
`PtySession` `Drop` of ADR-0516 already kills the session's process group, and the follower, in a
group of its own and without the terminal, survived it.

**Moving the child into a group of its own is not free.** `std::os::unix::process::CommandExt::
process_group(0)` is safe and stable, so `unsafe` is not the obstacle. But `ono` opens
`/dev/tty` whenever it has one, whatever its standard streams are, and hands the terminal to its
jobs and back to *its own* process group. Run from a developer's terminal, an `ono` in a group of
its own would take the terminal away from `cargo test` and leave it with a group that no longer
exists; and a Ctrl-C at that terminal would no longer reach the children at all.

## Decision

**A test helper that ends a process ends the process tree under it: every process is stopped with
`SIGSTOP` before its children are read from `/proc/<pid>/task/<tid>/children`, the reading is
repeated until no new child appears, and only then is every process in the tree sent `SIGKILL`.**
The child is then reaped by its owner as before. The child's process group is left as it is.

* `ono_testkit::kill_tree(pid)` is that walk.
* `Shell::try_run` and `run_bounded` use it at the deadline (#204).
* `ono_testkit::OwnedChild` wraps a `std::process::Child` and kills its tree and reaps it on
  `Drop`, so a test that spawns a process and then fails an assertion leaves nothing behind (#162).
  It dereferences to the `Child`; a child the test already waited for is left alone.
* `ono_testkit::OwnedTree::of(pid)` kills the tree under a pid whose handle is not a `Child` — a
  pseudo-terminal session — and is declared after that handle so it runs first.

No signal is sent to a pid the walk did not find as a child of a process it had already stopped,
and a stopped process cannot reap its children, so no pid the walk signals can have been reused.
All of it is `nix::sys::signal::kill` and `/proc` reads; the crate stays `#![forbid(unsafe_code)]`.

## Consequences

Easy: an overrunning run, and a failing test holding a guarded child, leave neither the child nor
anything it started — including the jobs `ono` puts in groups of their own and a process that
called `setsid`. Encoded by `crates/ono-testkit/tests/harness.rs` —
`should_kill_what_the_program_started_when_a_run_exceeds_its_budget`,
`should_kill_the_jobs_the_shell_started_when_a_bounded_run_is_cut_off`,
`should_leave_no_process_behind_when_a_test_holding_one_panics` and
`should_leave_no_process_under_a_session_behind_when_a_test_holding_it_panics` — and by
`crates/ono-cli/tests/adapters.rs::should_follow_the_journal_live_at_the_terminal_until_interrupted`,
which now asserts the follower's death instead of the prompt's return.

Stated residuals, not closed here:

* A process that was already *reparented away* before the kill — a daemon that double-forked,
  or a background job that outlived a parent which has exited — is no longer in the tree. `Shell`
  and `run_bounded` reach it only on the overrun path, where the child is still alive; a run that
  finishes normally leaves any background job `ono` started to finish on its own, as before.
* A test process that is itself killed (`SIGKILL`, a Ctrl-C at the terminal) runs no `Drop`.
  What the kernel does then is what it did before this decision: `ono` stays in the terminal's
  foreground group and receives the Ctrl-C, and its jobs are its own business.
* The walk relies on `/proc/<pid>/task/<tid>/children` (`CONFIG_PROC_CHILDREN`), which every
  distribution kernel this project is tested on enables. Without it the walk kills the root only,
  which is what the helpers did before.

## Alternatives considered

**`process_group(0)` on the child and `killpg` at the deadline** (the issue's own suggestion).
It reaches none of the processes that leaked — `ono`'s jobs are in groups of their own — and it
disturbs terminal ownership when the suite runs on a terminal (above).

**`PR_SET_PDEATHSIG` in the child.** Needs `pre_exec`, hence `unsafe`, and reaches only the direct
child; a grandchild's parent is not the test.

**Make the test process a child subreaper.** Process-wide state in a test binary whose tests run
in parallel threads: every orphan of every test would become this process's to reap, and a
test cannot tell its orphans from a neighbour's.

**Change `ono_process::PtySession`'s `Drop` to kill the tree.** `PtySession` is product code; the
product's own use of it is not what leaked. The testkit guard keeps the change on the test side.
