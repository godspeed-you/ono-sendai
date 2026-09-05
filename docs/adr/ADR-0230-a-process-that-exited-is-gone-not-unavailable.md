# ADR-0230: A process that exited is gone, not unavailable

- Status: accepted
- Date: 2026-08-29
- Spec refs: v0.2 §16.1 and §43 (the error taxonomy), §16.5 (partial failure), §35.3 (unknown is
  not fabricated), §11.5 (enumeration); ADR-0029 (a vanished row is not a failure)
- Decided by: agent (autonomous)

## Context

`ono -c 'get process | count'` exits 1 on a host that is churning processes. Under a load of
four shells forking `/bin/true` in a loop it failed twice in forty runs, with:

```text
ono: Ono-Sendai-E0401 provider.unavailable /proc/44012/stat: No such process (os error 3)
  the process was listed and then read; it exited in between
```

The help line already says what happened, and ADR-0029 already decided what it means: a process
that exits between the enumeration of `/proc` and the read of its `stat` is not part of the
answer, and `ProcessProvider::snapshot` drops it — but only when the failure carries
`io.not_found`. The failure here carries `provider.unavailable`, so the row became a partial
failure, and a partial failure is exit 1 (§16.5).

The cause is the translation, not the enumeration. `common::io_error` classified a failed read by
`std::io::ErrorKind`, and `ErrorKind` has no name for `ESRCH`: it arrives as
`ErrorKind::Uncategorized` and fell into the catch-all `provider.unavailable`. `common::errno_error`,
the same translation for a failed *syscall*, has always mapped `ESRCH` to `io.not_found`. The
product therefore answered two different conditions for one kernel condition, depending on
whether it had used `read(2)` or a `nix` wrapper.

`ESRCH` is exactly the condition ADR-0029 describes. The kernel answers it for a read of
`/proc/<pid>/…` in the window between a task leaving the `/proc` listing and its directory being
removed; `ENOENT` is the same disappearance seen a moment later. That the two are told apart at
all is an artifact of procfs, not a distinction a user has any use for.

Three tests depend on a complete enumeration succeeding, and were the observed symptom:
`options_and_selectors_missing::should_nest_children_under_their_parents_when_tree_is_requested`,
`spatial_topology_missing::should_bound_the_root_horizon_instead_of_listing_every_known_object`
and `remote_missing::should_answer_again_from_a_detached_link_when_it_is_entered_again`.

## Decision

**An errno decides which condition a failed read reports, not `std::io::ErrorKind`.**
`common::io_error` and `common::errno_error` share one table, `condition_of`, so the same errno
produces the same `ErrorCode` whichever call site observed it. `ErrorKind` is consulted only for
an `io::Error` that carries no errno at all — a synthesised one, which no kernel read produces.

Consequently `ESRCH` from a procfs read is `io.not_found`, not retryable, and enumeration omits
the row exactly as ADR-0029 requires: **a disappeared row is not a failed target.** A process the
user *named* — `get process --pid 12`, `signal process 12` — is a target, and its absence is still
reported, unchanged: the enumerating/named distinction is made in `snapshot`, over the same code.

None of the three tests changes. They asserted a true thing about the shell, and the shell was
wrong.

## Consequences

- `get process | count`, `get process --tree` and every question that enumerates the process table
  answer on a churning host. Sixty consecutive runs of `get process | count` and twenty-five of
  `get process --tree` under the same four-shell fork load: no failure, against two in forty
  before.
- The two translations can no longer drift apart; `should_translate_a_read_failure_the_same_way_a_syscall_failure_is_translated`
  holds them together for `ENOENT`, `ESRCH`, `EACCES` and `ENOTDIR`.
- Every other errno keeps its behaviour, including the catch-all: `EISDIR` on a `stat` that is a
  directory stays `provider.unavailable` and retryable, which is what
  `should_report_a_process_it_is_not_allowed_to_read_while_enumerating` asserts.
- No other errno changes answer. `ENOENT`, `EACCES`, `EPERM`, `EEXIST` and `ENOTDIR` are the
  errnos `std::io::ErrorKind` already named, and the shared table gives them the same codes it
  gave before; `ESRCH` is the one condition `ErrorKind` had no name for.
- Encoded by `common::tests::should_read_esrch_as_the_object_being_gone_when_a_procfs_read_fails`
  and `common::tests::should_translate_a_read_failure_the_same_way_a_syscall_failure_is_translated`.

## Alternatives considered

- **Change the three tests to ask a bounded question instead** (`get process --max 20 | count`).
  Rejected: it would have hidden the defect rather than fixed it, and left `get process | count`
  — the plainest question the shell can be asked — failing on any busy machine.
- **Special-case `ESRCH` in `ProcessProvider::read` alone.** It fixes the one symptom and leaves
  every other procfs reader reporting a vanished object as an unavailable provider; the storage,
  environment and descriptor readers all use `io_error` on `/proc` paths.
