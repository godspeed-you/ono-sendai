# ADR-0007: `unsafe` is confined to `ono-process`

- Status: accepted
- Date: 2026-08-26
- Spec refs: §18.1, §29, §29.3
- Decided by: agent (autonomous)

## Context

The workspace sets `unsafe_code = "deny"`. Spec §18.1 and §29 require real terminal job control:
foreground process groups, `setsid`, a controlling terminal for PTY children, and signal
disposition reset across `exec`. On Unix these are only expressible between `fork` and `exec`,
which in Rust means `std::os::unix::process::CommandExt::pre_exec` — an `unsafe` function,
because the child is running in a post-`fork` address space where only async-signal-safe calls
are legal. AGENTS.md §16 requires an ADR when `unsafe` crosses a crate boundary.

## Decision

`ono-process` is the only crate permitted to use `unsafe`. It sets
`#![deny(unsafe_op_in_unsafe_fn)]` and overrides the workspace lint locally with a comment
naming this ADR. Every other crate keeps `unsafe_code = "deny"`.

Inside `ono-process`, `unsafe` is permitted only for:

1. `pre_exec` closures, which may call only async-signal-safe libc functions: `setsid`,
   `setpgid`, `ioctl(TIOCSCTTY)`, `sigprocmask`, `signal`/`sigaction` reset, `dup2`, `close`;
2. the `ioctl` wrappers `nix` does not provide safely (`TIOCSCTTY`).

Every `unsafe` block carries a `// SAFETY:` comment stating which async-signal-safety rule makes
the call legal. No allocation, no locking, no Rust I/O, no `String` formatting and no panicking
path is permitted inside a `pre_exec` closure.

Where a safe API exists it is used instead, and `unsafe` is not: process group placement uses
`CommandExt::process_group`, file descriptor plumbing uses `Stdio::from(OwnedFd)`, and PTY
allocation uses `nix::pty::openpty` rather than `forkpty`.

`ono-process` exposes only safe types. No `unsafe` API, no raw fd, and no post-`fork` callback
crosses its boundary.

## Consequences

Easy: auditing the shell's `unsafe` surface is `grep -rn unsafe crates/ono-process`; every other
crate can be reviewed as pure safe Rust; the security reviewer of AUTONOMOUS_IMPLEMENTATION.md
§16 has a single small file set to examine.

Hard: any later crate that needs `fork`-time behaviour (the KUANG/11 supervisor's isolation of
spec §31.10) must route through `ono-process` or amend this ADR rather than reach for `unsafe`
locally.

Encoded by: the crate-level attributes in `crates/ono-process/src/lib.rs`, and PTY/job-control
integration tests that exercise the code paths the `unsafe` serves.

## Alternatives considered

- `nix::pty::forkpty` — rejected: it is itself `unsafe` and gives less control over the child's
  signal disposition and process group than an explicit `openpty` + `pre_exec`.
- A separate `ono-unsafe-sys` crate — rejected as speculative generality; the surface is small
  and belongs with the only code that needs it.
- Spawning a helper binary to do the terminal setup — rejected: it doubles process count on
  every foreground command and would blow the startup and latency budgets of §34.
