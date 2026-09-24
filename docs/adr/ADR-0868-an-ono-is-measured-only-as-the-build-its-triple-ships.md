# ADR-0868: An `ono` is measured only as the build its triple ships

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.4.1 §50.1; ADR-0864, ADR-0870, ADR-0910, ADR-0912
- Issues: #125, #127
- Decided by: agent (autonomous)

## Context

ADR-0864 §2 measures a release binary only when it is newer than everything its dep-info names,
plus the root `Cargo.toml`, `Cargo.lock` and `rust-toolchain.toml`, and refuses an `ono` that
links wasmtime's compiler. An independent review found two ways a binary that is not the shipped
build still passed as current:

- `cargo build --release -p ono-cli --no-default-features --features core` without `--target`
  writes `target/release/ono`, where the full shell of the host lands. It is newer than its
  sources, so `metrics --write` recorded the core build's seven megabytes as the full x86_64
  shell's, and `binary-size` held it to the full budget.
- A member's feature list lives in its own `Cargo.toml` (`crates/ono-cli/Cargo.toml` holds
  `full` and `core`). Editing it rebuilds the binary without touching a source the dep-info names,
  and no member manifest was an input, so the old binary stayed "current".

## Decision

1. **Each triple ships one build of `ono`, and a binary that is the other one is not measured.**
   `x86_64-unknown-linux-musl` is the core build's triple (ADR-0912); every other triple ships
   the full shell. The dep-info tells them apart by a workspace crate only the full build links:
   `crates/ono-kuang-supervisor/` (the KUANG/11 tier, which `core` leaves out — ADR-0910). An
   `ono` without it on a full triple, or with it on the core triple, is **not measured**, and the
   reason names the command that builds the right one. This sits beside ADR-0864's compiler rule,
   which catches the other wrong build of the same binary.
2. **Every member manifest is an input.** `crates/*/Cargo.toml`, `xtask/Cargo.toml` and
   `fuzz/Cargo.toml` join the root manifests: a binary older than any of them is stale.

## Consequences

- Recording or checking a figure needs the build its triple ships; a developer who built the core
  binary without `--target` is told so instead of recording it.
- An edit to any member manifest makes the release binary stale until it is rebuilt, including
  edits that could not change it (a test-only dependency of an unrelated crate). The gate then
  says "not measured" rather than checking a size; a false "stale" costs a rebuild, a false
  "current" cost the record its truth.
- The marker is a crate path. If the KUANG/11 supervisor ever moves into the core build, the
  marker moves with this ADR's successor; `should_not_measure_a_core_build_left_where_the_full_shell_goes`
  and `should_not_measure_a_full_build_on_the_core_triple` fail first.
- Tests: `xtask/tests/metrics.rs` — the two above and
  `should_not_measure_a_binary_older_than_a_member_manifest`.

## Alternatives considered

- **Read the build's features from cargo's fingerprint files.** They are cargo's private format;
  the dep-info is the documented one.
- **Mark the core binary by running `ono --version`**, which names the core build. The record is
  per triple and may be written for a foreign architecture the host cannot execute.
