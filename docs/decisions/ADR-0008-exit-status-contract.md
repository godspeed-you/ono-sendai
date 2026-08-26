# ADR-0008: Exit-status contract

- Status: accepted
- Date: 2026-08-26
- Spec refs: §16.4, §16.5, §29, §38
- Decided by: agent (autonomous)

## Context

Spec §16.4 requires that external exit status stays available and that Ono "MUST not translate
arbitrary non-zero exit codes into misleading native error categories". It does not say what
status `ono` itself returns for its own failures, and scripts, CI and `&&`/`||`-style
composition in surrounding tooling depend on it. Spec §38 rules out POSIX *syntax*
compatibility but says nothing about status conventions, which are an interop surface rather
than a syntax one.

## Decision

`ono` uses the Bourne-family status conventions, because every tool that consumes a shell's
status already assumes them:

| Status | Meaning |
|---|---|
| 0 | success |
| 1 | the command ran and failed — a native structured error, or a false test |
| 2 | the command line could not be understood: usage error, or a parse error (E0001/E0002) |
| 126 | the external command was found but could not be executed (not executable, `ENOEXEC`) |
| 127 | the command could not be resolved at all (E0101) |
| 128+N | the foreground process was terminated by signal N (E0502) |
| 130 | interrupted by SIGINT (the `128+N` rule, named because it is the common case) |

An external command's own status is passed through **unchanged**, including statuses in
126/127/128+N that the program chose itself. Ono only originates those values when it is Ono
that failed to execute or that observed the signal. This satisfies §16.4: no translation.

A pipeline's status is the status of its **last** stage. The full vector of stage statuses is
retained on the history entry (§20.1) and is reachable as structured data; it is not collapsed
into a single boolean (§16.5). Ono does not adopt `pipefail` as a mode: the structured record
is always there, so a hidden global mode is not needed.

A native command that produced `ActionResult`s exits 0 when every result is `success` or
`skipped`, and 1 when any is `failed` (§11.5).

Cancellation of a foreground native pipeline by Ctrl-C exits 130, matching what the user's
muscle memory and surrounding scripts expect.

## Consequences

Easy: `ono -c '...'` drops into existing Makefiles, CI steps and `bash` scripts without special
handling; a user's mental model transfers.

Hard: status 1 is deliberately coarse. Anything needing detail reads the structured error, which
is why the error value carries the code of §43. This is the intended direction: statuses are for
Unix, error values are for Ono.

Encoded by: `crates/ono-core/src/exit.rs` tests and the acceptance cases covering exit status.

## Alternatives considered

- Distinct statuses per error family (e.g. 3 = type error, 4 = provider error) — rejected: it
  collides with the exit codes external programs already use and gives scripts a second, weaker
  copy of information the error value already carries precisely.
- `pipefail`-by-default — rejected: it silently changes the meaning of a pipeline's status,
  which is exactly the ambiguity §16.5 objects to. The per-stage vector is explicit instead.
