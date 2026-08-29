# ADR-0220: A closed reader is not a failure to report

- Status: accepted
- Date: 2026-08-29
- Spec refs: §4.6, §12.3, §16.4, §43
- Decided by: agent (autonomous, `close-data`)

## Context

```text
$ ono -c 'get process | to json' | head -c 100
[{"command":["/usr/lib/systemd/systemd", …
ono: Ono-Sendai-E0302 io.permission_denied the output could not be written: Broken pipe (os error 32)
```

`| head` is how a Unix user asks for the first page. Rust ignores `SIGPIPE`, so the write returns
`EPIPE`, and `write_failed` mapped every I/O error the closed §43 taxonomy has no specific code
for onto `io.permission_denied` — including this one. The user is told the shell was denied
permission to write, for doing exactly what the tool is for.

The rendered path never had the problem: `Sink::write` discards its write errors, so
`get process | head` was already quiet. Only the serialised path — `to json`, `to yaml`,
`to csv`, `to text` — complained, which also made the two paths disagree about the same event.

## Decision

**A `BrokenPipe` on the shell's own output ends the program quietly**, with the status a program
killed by `SIGPIPE` reports (`128 + 13 = 141`), through the existing `Flow::Exit` — no
diagnostic, no error value, nothing written after it.

Every other write failure is unchanged: `/dev/full` still answers
`Ono-Sendai-E0302 … No space left on device`, because a full disk is a failure the user needs to
be told about and a closed reader is not.

`SIGPIPE` stays ignored for the process. Restoring the default disposition would make the shell
die whenever a child it is feeding exits early — `yes | head` would kill the shell rather than
`yes` — so the condition is handled where it is observed, at the write.

## Consequences

- `ono -c '… | to json' | head`, `| less`, `| grep -q` all stop in silence, as in every other
  shell, and the rendered and serialised paths now agree.
- A script that inspects the status sees 141, which is what a `SIGPIPE`d program reports; nothing
  reports success for output that was not fully written.
- Acceptance case 034 asserts the silence, beside the determinism it already asserts.

## Alternatives considered

- **`ErrorCode::StreamClosed` (or another §43 code) with exit 1.** Rejected: it is still a
  diagnostic for something that is not an error, and the shell would be the only tool on the
  system that prints one.
- **Restore the default `SIGPIPE` disposition at startup.** Rejected: it kills the shell when a
  child of *its* pipeline goes away, which is a different event with the same signal.
- **Exit 0.** Rejected: the output was truncated, and a status that says otherwise is a lie a
  script cannot see through.
