# ADR-0893: A process tree is signalled through pidfds, leaves first

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.4.1 §2.4, §38.1
- Issues: #204, #162
- Amends: ADR-0892 (its claims about pid reuse and kill order; its decision to kill the tree and
  not a process group stands)
- Decided by: agent (autonomous)

## Context

ADR-0892 made `ono_testkit::kill_tree` stop every process in a tree with `SIGSTOP`, reading each
stopped process's `/proc/<pid>/task/<tid>/children`, and then send `SIGKILL` to every process in
the order it was found. It claimed: "no pid the walk signals can have been reused, because a
stopped process cannot reap its children". Two independent reviews showed that claim is wrong,
and found three more defects:

* **A stopped parent is not the only reaper.** A parent that ignores `SIGCHLD` has its children
  reaped by the kernel the moment they exit, stopped or not. The pid is free at once, and a
  signal sent to it by number can reach whatever process takes it.
* **Root first is the wrong order.** Once a parent is dead, its children are reparented, and a
  zombie or a child in uninterruptible sleep can be reaped and its pid reused before the loop
  reaches it. A parent's death can also orphan a process group whose members are stopped; the
  kernel then sends them `SIGHUP` and `SIGCONT`. A descendant that ignores the hangup resumes
  and can fork outside the walk.
* **A process that refused `SIGSTOP` was tried again on every pass.** An example is a child that
  changed its uid (`EPERM`). It stayed listed under its stopped parent and was never frozen, so
  the walk never ended.
* **Only the thread-group leader's state was checked.** The leader can show as stopped while
  another thread is still running, and that thread can fork.

## Decision

1. **Every process is signalled through a pidfd opened when it is found, never by its number.**
   `rustix::process::pidfd_open` and `pidfd_send_signal` are safe wrappers. `rustix` with its
   `process` feature is already a workspace dependency in `Cargo.lock`. A pidfd names one process
   for as long as the descriptor lives. Once that process is reaped, a signal sent through the
   pidfd answers `ESRCH` and reaches no other process.
2. **A child is opened only while it is provably that child.** Its pid is read from the parent's
   children list and a pidfd is opened on it. The child is kept only if it is still listed under
   the parent afterwards. The parent is stopped and cannot fork, so a pid still listed there
   names the same child.
3. **`/proc` reads by number are bracketed by the pidfd.** A process's children list is trusted
   only if the process answers a signal through its pidfd both before and after the read.
   Reaping cannot be undone, so a process alive at both ends owned the number for the whole
   read. Zombies are harmless under this rule: a zombie answers the probe, lists no children,
   and a `SIGKILL` sent to it through its pidfd is a no-op.
4. **The walk remembers every pid it tried, stoppable or not.** A process that refuses the stop
   is not frozen and not killed. It cannot be killed anyway, and its children are not walked.
5. **`SIGKILL` goes out leaves first.** Discovery order puts every process after the one it was
   found under, so the reverse order kills every child before its parent. No process loses its
   parent while it is still waiting to be killed. A `SIGCONT` from an orphaned group cannot
   resume a process that already has `SIGKILL` pending.
6. **A process counts as stopped only when every thread of it is stopped.** The walk reads
   `/proc/<pid>/task/<tid>/stat` for every thread, not just the leader.

On a kernel without `pidfd_open` (before Linux 5.3), `kill_tree` kills the root alone by its
number. The caller's unreaped handle keeps that number from being reused. This is the behaviour
before ADR-0892.

## Consequences

The walk's contract is now tested in isolation, over a process table the tests control, in
`crates/ono-testkit/src/reap.rs::tests`:

- `should_end_and_try_each_process_once_when_a_descendant_cannot_be_signalled`
- `should_kill_every_process_before_the_process_it_descends_from`
- `should_signal_no_process_it_could_not_open_as_a_child_of_a_frozen_parent`

The real-process tests of ADR-0892 still hold.

`ono-testkit` depends on `rustix`. No crate is added to `Cargo.lock`.

## Alternatives considered

**Leaves first without pidfds.** This closes the reparenting window and the orphaned-group
`SIGCONT`. It does not close the kernel reaping a `SIGCHLD`-ignoring parent's children.

**Comparing `/proc/<pid>/stat` start times before each signal.** This narrows the window but
does not close it: a check followed by a signal is still two steps.

**`nix` for pidfds.** `nix` 0.31 has no `pidfd_open`.
