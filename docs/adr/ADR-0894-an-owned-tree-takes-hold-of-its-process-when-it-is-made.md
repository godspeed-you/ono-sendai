# ADR-0894: An owned tree takes hold of its process when it is made

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.4.1 §38.1
- Issues: #162
- Related: ADR-0892, ADR-0893
- Decided by: agent (autonomous)

## Context

`ono_testkit::OwnedTree::of(pid)` (ADR-0892) stored a pid and called `kill_tree(pid)` when it
was dropped. It guards a process whose handle is something other than a `std::process::Child`,
such as `ono_process::PtySession`. That handle can reap the leader on its own: `wait`,
`try_wait` and `wait_timeout` all do. The guard cannot tell. A guard dropped after the session
was waited for would walk and kill whatever process had taken the number by then, together with
that process's children.

## Decision

**`OwnedTree::of` opens a pidfd on the process at once and keeps it. Its `Drop` walks the tree
from that pidfd.** Once the leader has been reaped, the pidfd answers `ESRCH` to every signal.
Under ADR-0893's bracketing rule no `/proc` read is trusted for it, so nothing is signalled. The
guard is made immediately after the process is started, while the process is still unreaped;
the documentation says so. On a kernel without pidfds the guard does nothing, and the handle's
own `Drop` still applies.

`kill_tree(pid)` falls back to a plain kill by number only when the kernel lacks `pidfd_open`
(`ENOSYS`). It no longer does so for a process that is simply gone.

## Consequences

The guard can be embedded in shared PTY helpers that hand the session to tests. A test may wait
for the session, or not, and either way the guard stays correct. Encoded by
`crates/ono-testkit/tests/harness.rs::should_kill_the_tree_of_the_process_it_was_made_for_even_when_that_process_changed_its_name`
and `should_do_nothing_when_the_process_it_was_made_for_has_already_been_reaped`.

The reuse itself cannot be provoked from an unprivileged test: making a new process land on a
chosen pid needs `ns_last_pid`, which requires `CAP_SYS_ADMIN`. The second test therefore shows
only that a reaped leader is handled without a signal and without a panic. The guarantee against
reuse rests on the pidfd semantics that ADR-0893 records.

## Alternatives considered

**Take the handle itself (`OwnedTree::of(&PtySession)`).** `ono-testkit` does not depend on
`ono-process`, and the handle's wait methods would still be callable through the guard.

**Compare `/proc/<pid>/stat` start times at drop.** This narrows the window, but a check
followed by a signal is still two steps.
