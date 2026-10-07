# ADR-0936: No diagnostic write can turn a refusal into a crash

- Status: accepted
- Date: 2026-10-07
- Spec refs: §43 (errors are typed, and the exit status says which kind), v0.4.1 §2.7; ADR-0549
  (a diagnostic nobody is reading is not a reason to die), ADR-0220
- Decided by: agent (autonomous)

## Context

ADR-0549 introduced `ono_core::diagnostic!` — `eprintln!` that ignores a failed write — and
applied it to the listening agent. Every other diagnostic in the product kept `eprintln!`, which
panics when standard error is a pipe whose reader is gone. `ono --bogus 2>&1 | head -c0` therefore
exited 101, Rust's panic status, where the shell meant to refuse with the usage status 2 (issue
#163). Anything reading the status — a script, a supervisor, CI — saw a crash. Converting the
remaining call sites fixes them today; nothing stopped the next one from being written.

## Decision

1. Product code — every crate the `ono` binary or a shipped tool is built from — writes
   diagnostics with `ono_core::diagnostic!`, the `Reporter`, or a direct `write!` to
   `std::io::stderr()` whose result is ignored. A crate that does not depend on `ono-core` (the
   `ono-kuang-sdk` binaries) defines the same three-line macro locally rather than taking the
   dependency for it.
2. The workspace `clippy.toml` lists `std::eprintln` and `std::eprint` under `disallowed-macros`,
   so `cargo clippy -- -D warnings`, and with it the gate, refuses a new one.
3. Code that may keep the panicking form says so with
   `#[allow(clippy::disallowed_macros, reason = "…")]` at the narrowest scope that covers it:
   build scripts (cargo always reads their stderr), `xtask` and `ono-fuzz` (developer tools run at
   a terminal or in CI, never the shell a user runs). Test code has no use for the allowance: a
   test's diagnostics are captured by the harness.

Standard *output* is out of this decision. `println!` on a closed stdout still panics, and the
fix there is not the same — a closed stdout means the consumer is gone, which ADR-0220 already
treats as a reason to stop — so it stays its own increment.

## Consequences

- `crates/ono-cli/tests/closed_stderr.rs` runs the real binary for a usage error, an unknown
  command, a failing pipeline, a type error, a failing `--print-peer-key` and a failing script
  with standard error a pipe whose read end is closed, and requires the same status the refusal
  has with standard error read.
- A new `eprintln!` in product code is a clippy error naming this rule; the allowance is visible
  and carries its reason.
- `ono --print-peer-key 2>&1 | head -c0` still exits 101 when the identity is usable: the
  fingerprint goes to stdout with `println!`. That is the stdout case above, not a diagnostic.

## Alternatives considered

- Reset `SIGPIPE` to its default so the process dies of the signal instead of panicking — rejected:
  the status becomes 141 instead of the refusal's, and a closed stderr must not stop a process
  whose work (stdout, a listener) is still being consumed (ADR-0549).
- Convert the call sites without a lint — rejected: the issue existed because 95 sites survived
  ADR-0549's partial sweep; only a gate keeps the rule.
- A `scan` rule in `xtask` instead of clippy — rejected: clippy already resolves macro paths
  through re-exports and `#[allow]` gives every exception a reason in place.
