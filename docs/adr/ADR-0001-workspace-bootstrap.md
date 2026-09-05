# ADR-0001: Workspace bootstrap

- Status: accepted
- Date: 2026-08-26
- Spec refs: 24.1, 24.2, 34, 50
- Decided by: agent (autonomous)

## Context

The specification describes the product but fixes nothing about how the repository starts.
Before phase A can begin test-first, there must be a workspace that compiles, a gate that runs,
and a green baseline — otherwise the first agent cannot tell its own red from an inherited one
(AGENTS.md section 17).

## Decision

- Cargo workspace with `crates/*` and `xtask`, `resolver = "3"`, **edition 2024**, `rust-version
  1.94`. The spec requires "2021 or newer"; 2024 is current and avoids a migration later.
- Toolchain **pinned** in `rust-toolchain.toml` to `1.94` with `rustfmt` and `clippy`, so the
  developer machine, CI and the container compile identically. A pinned toolchain makes the
  acceptance results reproducible; bumping it is a deliberate increment.
- Three crates now, not the seventeen of spec section 24.2: `ono-cli` (the `ono` binary),
  `ono-core`, `ono-testkit`. The rest are created when a phase needs them. Creating empty crates
  upfront produces structure without contracts and invites speculative APIs (AGENTS.md
  section 4).
- Workspace lints deny `unsafe_code` and warn on `missing_docs`, `unwrap_used`, `expect_used`
  and `panic`. The gate runs clippy with `-D warnings`, so those warnings are effectively
  errors, which is what spec section 16 requires of library code paths. `ono-testkit` allows
  `expect_used` crate-wide with a stated reason, because it is linked only into tests.
- **No third-party dependencies yet**, including for argument parsing. The scaffolding binary
  matches its two flags by hand. Choosing a CLI, parser or async library is a phase A decision
  that deserves its own ADR rather than being smuggled in by the bootstrap.
- Release profile uses thin LTO, one codegen unit and stripped symbols, because spec section 34
  makes startup latency a product requirement and the container measures the release binary.

## Consequences

- The gate is green from the first commit and stays the honest signal it is meant to be.
- The scaffolding binary is deliberately almost empty: `--version`, `--help`, and a usage error
  for anything else. Its three outcome tests are a floor, not a design — the first agent that
  implements real argument handling replaces the implementation and keeps the tests passing.
- Adding a crate later is cheap; removing a speculative one is not. The workspace grows with the
  phases.
- A pinned toolchain means a Rust release does not silently change acceptance results, and also
  that upgrading is visible work.

## Alternatives considered

- **Scaffold all crates from spec section 24.2 now** — rejected: seventeen empty crates are
  seventeen guesses about boundaries the spec explicitly calls "suggested".
- **`channel = "stable"`** — rejected: the container and the acceptance budgets in spec section
  34 need a fixed compiler to be comparable across runs.
- **Add `clap` immediately** — rejected: it is a phase A decision about the shell's own argument
  model, not a bootstrap detail.
