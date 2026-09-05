# ADR-0029: A process that no longer exists is not a failure

- Status: accepted
- Date: 2026-08-26
- Spec refs: §16.5, §23.1, §35.3
- Decided by: agent (autonomous)

## Context

`/proc` is enumerated in two steps: list the pids, then read each one. Between the two, processes
exit. The earlier decision was that this lands on the stream's error channel, on the reading of
spec §16.5 that a bulk operation must report what failed as well as what succeeded, and a test
pinned it: *"the disappearance is reported, not swallowed"*.

Running the shell showed what that means in practice. On this machine, typing

```text
get process | where pid == 1 | select name
```

printed two `Ono-Sendai-E0301 … the process was listed and then read; it exited in between`
before the one row the user asked for. On a busy machine it is worse. A shell that reports two
errors for a successful query teaches its user to stop reading errors, and that costs far more
than the reports were worth.

## Decision

**During an enumeration, `io.not_found` on a process is a process that no longer exists, and it
is skipped silently.** It is not part of the answer, and omitting something that is not there is
not the same as hiding a failure.

Three cases stay exactly as they were:

- **A process the user named.** `get process 812` pins a pid. That is a *target*, and a target
  that is not there is an answer the user needs, so it is reported.
- **A process that exists and cannot be read.** Anything other than `io.not_found` — a blocked
  `stat`, a permission refusal — means the process is there and something went wrong. Reported,
  with its identity, exactly as spec §16.5 requires.
- **A field that cannot be read within a process that can.** Unchanged: an unreadable `cmdline`
  is still an error value on that field rather than a fabricated `null` (spec §35.3).

This reverses the earlier reading of §16.5 for one case only. §16.5's example is
`97 succeeded, 3 failed` — an operation over targets the user named. An enumeration names no
targets; it asks what is there. "What is there" cannot fail to include something that is not.

## Consequences

- `get process` is usable on a real machine, which it was not.
- The distinction the shell now draws is one the user can act on: every error it prints about a
  process is a process that is still running and could not be read.
- Tests: `crates/ono-provider-linux/tests/process.rs` — the vanishing case asserts an empty error
  channel, and two new cases assert the named-target and the unreadable-process paths, so the
  split is pinned from both sides rather than the old assertion simply being dropped.
- Other providers enumerating a kernel namespace that changes underneath them — sockets, mounts,
  services — should follow the same rule when they hit it. None does today.

## Alternatives considered

- **Keep reporting, and let the user silence it.** Rejected: the default is what people live
  with, and a default that is wrong on every invocation is not fixed by an option.
- **Count the omissions and report a summary.** `ono-pipeline::Diagnostics` already tracks
  `excluded_unknown` and `skipped_null` for exactly this shape of thing. Rejected for now, not on
  principle: nothing surfaces those counters to the user yet, so it would have been a number
  written to a field nobody reads. It is the right home for this if the count ever matters, and
  `docs/STATE.md` carries it.
- **Re-check whether the pid is gone before deciding.** Rejected: it is the same race one layer
  down, costs a second syscall per vanished process, and answers a question the `ENOENT` already
  answered.
