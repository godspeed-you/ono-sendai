# ADR-0161: A shell ends the agent processes it started

- Status: accepted
- Date: 2026-08-28
- Spec refs: §21.4, §21.1, §18.1, §18.5, §29.3
- Decided by: agent (autonomous)

## Context

`link host <name> --transport local` (and `--transport ssh`) starts a child process to serve the
link: `ono --agent` locally, `ssh … ono --agent` remotely. Until now nothing ended it. The
transport documented the omission as deliberate — "The child is not killed when the transport
goes away: closing its stdin is the hang-up signal" — and it is true that closing the pipe ends
the agent loop. What was missing is *when*. Nobody waited.

Measured on this branch before the change, with a process that had set
`PR_SET_CHILD_SUBREAPER` and started `ono -c 'link host testbox --transport local; …'`:

```
(pid, 'shell',              166.9 ms, status 0)
(pid, 'ORPHAN(reparented)', 166.9 ms, status 0)
```

The shell was reaped first; a second process — the agent — was still alive at that moment, was
reparented to the subreaper, and only then exited. Three reasons combine to produce it:

1. `Link`'s hang-up is a queue entry for the writer task, so the agent's stdin only closes when
   that task next runs;
2. `Session`'s `runtime` field is declared before its `links` field, so at session teardown the
   runtime is dropped *first* and the queued hang-up is never delivered at all — the agent's
   input closes only because the kernel closes the exiting process's descriptors;
3. no code path waited for the child, so the shell's own exit always won the race.

Where the shell is the container's PID 1 lineage — `bash -lc 'script …'` execs `script`, so
`script` *is* PID 1 — the orphan reparents onto `script`, whose `SIGCHLD` handling takes it for
its own child, tears the pty down and hangs up the `bash` running under it. That is acceptance
case `049-remote-link` ending at exit 129 (128+SIGHUP) under load, with `ono` itself having
exited 0.

The specification does not spell the rule out. §21.4 describes the agent as the far end of a
link; §21.1 calls a link a thing with metadata and a lifetime; §18.1 requires the shell to
handle "foreground processes, background jobs, signals, terminal process groups" the way normal
interactive Unix software does; §18.5 requires cancellation to "translate to appropriate signals
for external processes". None of them says a shell may leave a process behind.

## Decision

**An agent process is a resource of the link that started it, and a link's teardown ends it.**

- `Link::hangup` and `RemoteLink::hangup` are public: the goodbye is said explicitly, not left
  to a drop, because a mounted `RemoteProvider` may still hold the connection open.
- `SubprocessTransport::child` hands out a `ChildProcess` handle that outlives the transport
  (the transport is consumed by `Link::connect`). `ChildProcess::end(grace)` waits for the child
  and returns its status; the child is given `grace` to notice the closed input, then `SIGTERM`,
  then after another `grace` `SIGKILL`. The escalation happens inside the task that owns the
  `Child`, so a pid can never be signalled after it has been waited for.
- `Session::hang_up(link)` performs the whole teardown: hang up, release the providers, wait for
  the agent. Every path that lets a link go uses it — `remove link`, `detach link` of a one-shot
  connection, `leave` of a one-shot frame, `add link` replacing a name, and a handshake that
  failed after the child was already spawned.
- `impl Drop for Session` ends every link the session still holds. Drop runs before the fields
  are dropped, so the runtime is still there to wait on — which is exactly what the field order
  otherwise prevents.
- The grace is 2 s per step. It is never reached in practice: closing the agent's input ends it
  in about a millisecond, and a normal exit costs the same as before (10 linked runs took
  1.64 s before and after the change; 10 unlinked runs, 0.97 s).

What this is not: no reaper thread, and no blanket kill. The shell signals exactly the processes
it started, only after they have been told to leave and have not, and only through the handle
that owns them.

## Consequences

- Nothing of `ono` outlives an `ono`. The container's PID 1 no longer receives a `SIGCHLD` for a
  process it never started, which is what case 049's exit 129 was.
- `exit` and end-of-input now cost a bounded wait per established link. Measurably zero for a
  live agent; at worst 4 s for one that ignores both end of input and `SIGTERM`.
- `ono-remote` gains a `nix` dependency (`signal`), the syscall crate ADR-0005 already chose.
- Encoded by `crates/ono-cli/tests/session_lifetime.rs`
  `should_end_the_agent_it_started_before_it_exits`: the `ono --agent` children of a shell must
  not exist at the instant the shell has been reaped.

## Alternatives considered

- **Reorder `Session`'s fields so `links` drops before `runtime`.** Fixes the delivery of the
  hang-up and nothing else: the shell would still exit without waiting, and the correctness of
  the shell's exit would rest on the declaration order of two struct fields.
- **`kill_on_drop(true)` on the child.** Kills the agent at an arbitrary point, including while
  it is answering, and skips the graceful hang-up the agent loop is built around.
- **`docker run --init` in the acceptance harness.** Measured green 30/30, but it hides the
  defect rather than removing it: a shell that leaks a process leaks it on every machine whose
  PID 1 is not a reaper, not only in the harness. Fixing the harness instead of the shell would
  also break the rule that the referee is never weakened to get a green result (AGENTS.md §14).
- **A reaper thread that sweeps orphans.** Explicitly rejected: the shell knows exactly which
  processes it started and can end those, so nothing needs sweeping.
