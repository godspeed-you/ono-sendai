# ADR-0962: A busy file is not a refusal of access

- Status: accepted
- Date: 2026-11-09
- Spec refs: spec §16.1, §16.5, §43; ADR-0006, ADR-0008, ADR-0520
- Decided by: agent (autonomous)

## Context

A user runs a script somebody else is still writing. `execve` answers `ETXTBSY` — text file
busy — and `spawn::exec_failure`'s catch-all arm turned it into exit 126 with the structured
error `Ono-Sendai-E0302 io.permission_denied`, whose help text says the current user lacks a
permission and should elevate. The file is executable and nobody refused anything; the help sends
the user to `chmod` or `sudo` for a condition that ends by itself when the writer closes the file.
ADR-0520 recorded the wording problem when it fixed the test race that kept producing it, and
#186 carries it.

The cause is `code_for_io` in `ono-process`: spec §43's taxonomy has four `io.*` codes — not
found, permission denied, already exists, not a directory — and no word for "in use", so every
unclassified system failure, `ETXTBSY` and `EBUSY` among them, falls back to `io.permission_denied`.
The fallback is right for what it was written for (an `EMFILE` is closer to a refusal than to
anything else in the list); it is wrong for a resource another process holds, which is a
different thing a user does something different about.

## Decision

The taxonomy gains **`Ono-Sendai-E0305 io.busy`**, kind `io`: *the resource is in use by another
process and cannot be used this way right now*. `code_for_io` maps `io::ErrorKind::ExecutableFileBusy`
(`ETXTBSY`) and `io::ErrorKind::ResourceBusy` (`EBUSY`) to it; everything else keeps its mapping.

The exit status does not change: a busy program is still POSIX's 126, "found and not executable"
in the sense that matters to a script's `$?` — the shell found the program and could not run it.
What changes is what the structured error says and what its help tells the user to do: wait for
the writer, or find who holds the file, rather than change a mode or elevate.

The taxonomy is closed and additive (ADR-0006): the new code takes the next free number in the
`io` family, no existing code moves or changes meaning, and `docs/contracts/errors.yaml` carries
it with `docs/reference/errors.md` regenerated from it.

## Consequences

Easy: a script that failed because its author was still saving it says so; `EBUSY` from a mount
or device operation gets the same honest code where it reaches `from_io`.

Hard: a `catch io.permission_denied` written around a busy file stops matching. No shipped case
or document relied on that — the message always carried "Text file busy", and the code was the
defect.

Encoded by `crates/ono-process/tests/external_command.rs::should_name_the_busy_file_rather_than_a_refusal_when_a_writer_still_holds_the_script`
and `crates/ono-process/src/error.rs::tests::should_report_busy_when_the_system_says_the_resource_is_held`.

## Alternatives considered

**Keep `io.permission_denied` and only reword the message.** The message already said "Text file
busy"; the code and its help were the problem, and a script branches on the code.

**Report `external.exit_nonzero` or a new `external.*` code.** Nothing external ran; the failure
is the shell's own attempt to start a program, which is an I/O failure like the other `exec`
failures ADR-0008 classifies.

**Retry until the writer closes the file**, as ADR-0520 does inside the test harness. The harness
knows the writer is its own thread and will finish within milliseconds; the shell knows neither,
and a retry that may never end is not an answer to a user.
