# ADR-0549: A diagnostic nobody is reading is not a reason to die

- Status: accepted
- Date: 2026-09-03
- Spec refs: v0.4.1 §2.7 (a test reports execution truth), §11.2 (the listening agent's startup
  summary), §12.6 (one peer's failure is not the agent's), §14.1 (the audit trail goes to stderr);
  ADR-0220 (a reader that closed the pipe is not a failure to report), ADR-0517
- Decided by: agent (autonomous)

## Context

`crates/ono-cli/tests/client_keys.rs::should_refuse_the_next_connection_after_a_client_key_is_removed`
failed about one run in thirty inside the full suite and never in isolation, with

```text
Ono-Sendai-E0601 remote.unreachable 127.0.0.1:42885 could not be reached over the ono transport:
Connection refused (os error 111)
```

*after* the harness had already read `ono: listening on …` from the agent's stderr. `TlsListener::bind`
precedes `local_addr` precedes the summary, and a bound socket accepts into its backlog before
anything calls `accept`, so the announcement was not premature. The agent was dying.

It was dying of its own startup summary. §11.2 asks a listening agent to print nine lines before
it accepts anybody; the fixture reads two of them — the address and the fingerprint — and then
lets the channel its reader thread sends on go out of scope, which ends the thread and closes the
read end of the pipe. On a machine quiet enough that all nine lines are already in the pipe buffer
nothing happens. On a busy one the agent is still writing when the pipe closes, the next
`eprintln!` fails with `EPIPE`, and **`eprintln!` panics on a failed write**. The process that
failed was the agent, on its main thread, at exit code 101, between announcing the socket and
serving it.

Reproduced without the suite: spawn `ono --agent --listen 127.0.0.1:0`, read one line of stderr,
close the pipe — **11 of 30 agents exited 101**. Reproduced deterministically by handing the agent
a `/dev/full` for standard error, where every write fails with `ENOSPC` instead of racing:
exit 101, port refused, every time.

The same class was already decided once in this repository and only half applied. `StderrAudit`
writes its lines with `writeln!` and discards the result, under a comment saying "a failure to
write an audit line must not take the connection down with it". ADR-0220 made the same call for
standard output, where a closed pipe is `… | head` and the shell answers it in silence. The
startup summary and the per-connection diagnostics of `serve_one` were never brought along.

## Decision

**A diagnostic is what a program says about its work; it is never the work. A write that fails
costs the line and nothing else.**

`ono_core::diagnostic!` takes `eprintln!`'s arguments, writes one line to standard error, and
discards the result. Every line an agent writes — the §11.2 startup summary, the ceilings, the
per-connection refusals of `serve_one`, the message the accept loop ends with, the stdio agent's
own error — goes through it. `StderrAudit` already did this by hand and is unchanged.

The replacement stops at the agent. `ono`'s usage error and `--print-peer-key` still use
`eprintln!`, because there the diagnostic *is* the last thing the process does and a failed write
changes an exit status rather than killing a service. That the shell's own one-liner path can
still panic on `ono -c … 2>&1 | head -0` is recorded for the board rather than swept up here
(AGENTS.md §4).

**What this is not: a licence to lose audit lines.** §14.1's trail is written the same way it
always was, to the same stream, and a host that wants it kept gives the agent a stream that keeps
it. What changes is only that the agent does not resign when the operator closes the console.

## Consequences

Easy: a listening agent survives its log going away — a pipe nobody drains, a rotated file, a
closed terminal, a supervisor that stopped reading. That is what a service is, and it is what
§12.6 already says about a peer: one connection's failure is not the agent's.

Hard, and worth stating: a diagnostic that could not be written is now silently gone. There is no
second channel that reports the loss, because a report about a failed report has the same problem.
The audit trail is a stream the operator chooses, and choosing `/dev/full` for it is choosing not
to have one.

Also worth stating: this fix was only ever visible as a flake. One run in thirty of one test was
the whole of the evidence that `ono --agent` could be killed by closing its log, which is
v0.4.1 §2.7 working as intended — the suite was reporting execution truth and the truth was a
product fault.

Encoded by `crates/ono-cli/tests/agent_startup.rs::should_keep_listening_when_its_diagnostics_cannot_be_written`:
an agent whose standard error refuses every write still accepts a connection on the port it was
given. It skips with `fixture_not_applicable` on a host with no `/dev/full`, declared in
`docs/contracts/hardening/expected_test_skips.yaml`.

## Alternatives considered

**Fix the fixture instead: keep the receiver alive so the pipe stays open.** `authenticated_link.rs`
already does exactly that, with a comment explaining why. It would have made this one test green
and left the product able to die on any operator who closes a log — the flake was the only thing
reporting a real fault, and answering it in the test would have deleted the report and kept the
fault.

**Ignore `SIGPIPE` differently, or restore the default disposition.** Rust already sets `SIGPIPE`
to `SIG_IGN`, which is why the write returns `EPIPE` rather than killing the process outright.
Restoring the default would trade a panic for a signal death: the same outcome, less legible.

**Route agent diagnostics to a log file instead of standard error.** §14.1 puts the audit trail on
standard error and §11.2 puts the summary there, so this would be a contract change to avoid an
`unwrap`. The stream is right; panicking on it was not.

**Sweep every `eprintln!` in the workspace.** Ninety-nine call sites, most of them the last thing
their process does. AGENTS.md §4: fix the bug in front of you, record the rest.
