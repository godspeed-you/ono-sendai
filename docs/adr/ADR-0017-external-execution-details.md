# ADR-0017: Reaching status 126, and mapping errno onto the error taxonomy

- Status: accepted
- Date: 2026-08-26
- Spec refs: §16.4, §29, §43, ADR-0008
- Decided by: agent (autonomous)

## Context

ADR-0008 gives status 126 the meaning "found but not executable". Implementing it revealed that
the obvious implementation can never produce it: `execvp` does not fail on a file it cannot
execute. When `execve` returns `ENOEXEC`, `execvp` silently re-runs the file through `/bin/sh`,
so a JPEG marked executable becomes a shell script full of syntax errors and the user gets `/bin/sh`'s
status and `/bin/sh`'s complaints instead of "this is not a program".

Separately, spec §43's `io.*` family has four codes — `not_found`, `permission_denied`,
`already_exists`, `not_directory` — and no general one. Real I/O produces `EMFILE`, `ENOSPC`,
`EROFS`, `EIO` and a dozen more, all of which have to become one of those four or nothing.

## Decision

### The executable format is checked in the parent

Before spawning, the resolved file's first bytes are read:

- `#!` — the kernel will handle it. Run it.
- ELF magic — the kernel will handle it. Run it.
- neither, but the content is text — run it through `/bin/sh`, which is what `execvp` would have
  done and what every Bourne-family shell does, so a `sh` script without a shebang keeps working.
- neither, and the content is binary — **status 126** with a structured error, without spawning.

The check happens in the parent because that is the only place its answer can still be reported.
After `fork`, the child can do nothing but exit with a number.

### `permission_denied` is the fallback for an I/O failure with no more precise code

`io::ErrorKind::NotFound`, `AlreadyExists` and `NotADirectory` map to their codes. Everything
else becomes `io.permission_denied` — "the operating system refused access to the resource" —
with the operating system's own wording preserved in the message.

This is the least wrong of the available options. Inventing an `io.other` would be the first
addition to a taxonomy spec §43 states as closed (ADR-0006), and `io.not_found` for `ENOSPC`
would be an outright lie. The kind stays `permission` in every case, which is what a script
branching on kind needs: "the system would not do this for me", as against "the thing is not
there".

The message carries the truth. `EMFILE` reads as "too many open files", not as "permission
denied", so nothing is hidden from the person reading it — only the machine-readable code is
coarser than reality.

## Consequences

Easy: 126 is actually reachable, so `docs/ACCEPTANCE.md`'s status requirements can be proven
rather than asserted; a script sees a stable coarse code and a person sees the precise cause.

Hard: a script cannot distinguish "disk full" from "permission denied" by code alone. When a
concrete need for that appears, the answer is a new code in the `io.*` family with a
supersession of this ADR — not a silent widening of an existing one.

Encoded by: `crates/ono-process/tests/external_command.rs` (126 for a binary that is not a
program, for a directory and for a file without the executable bit; 127 for an unresolvable
name) and the errno mapping unit tests.

## Alternatives considered

- **Letting `execvp` do it** — rejected: the failure surfaces as `/bin/sh` syntax errors, which
  is both wrong and confusing, and ADR-0008's 126 would be dead code.
- **Checking after `fork`** — rejected: a child can only exit with a number, so the structured
  error would be lost exactly when it is most useful.
- **Adding `io.other` to the taxonomy** — rejected: spec §43's list is closed and additive
  (ADR-0006), and a catch-all code is one every future decision would be tempted to reach for.
