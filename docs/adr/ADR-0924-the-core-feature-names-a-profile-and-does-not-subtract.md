# ADR-0924: `core` names a profile and does not subtract; the recipe is the guard

- Status: accepted
- Date: 2026-09-24
- Spec refs: AGENTS.md §10 (the gate's `--all-features`); ADR-0910, ADR-0912
- Issues: #127
- Decided by: agent (autonomous)

## Context

ADR-0910 made `core` an empty feature, and `absent::is_full` looks only at the tier features.
Cargo features are additive, which has two consequences:

- `cargo build -p ono-cli --features core` without `--no-default-features` silently builds the
  full shell.
- `--all-features` enables `core` and every tier together. The quality gate runs
  `cargo clippy --all-features` and `cargo test --all-features` over the workspace.

The review asked for a decision: either a `compile_error!` for `core` together with any tier, or
documentation.

## Decision

`core` stays a name, not a subtraction, and a build with `core` and the tiers is the full shell.
There is no `compile_error!`: the gate's `--all-features` would hit it on every run, and working
around it would mean excluding `ono-cli` from the gate's feature set. That loses coverage of
every tier for the sake of a flag spelling.

The guard is the recipe, together with what the binary says about itself:

- `crates/ono-cli/Cargo.toml` states on the `core` feature that only
  `--no-default-features --features core` builds the core, and that `--features core` alone and
  `--all-features` build the full shell.
- `scripts/build-core.sh` is the one recipe for the core binary. It passes
  `--no-default-features --features core`, and it refuses a binary whose `--version` does not
  print the `build: core (without …)` line.
- `ono --version` names the profile a binary actually is. In a full build it prints the single
  line it always has, whatever features were spelled, so a mistaken build is visible from the
  binary itself.

## Consequences

- Someone who writes `--features core` alone gets a working full shell rather than a failed
  build, and `--version` shows which one they got.
- The gate is unchanged. The core profile is linted, tested and built by the CI `core-build` job
  (ADR-0913), not by the gate's `--all-features`.

## Alternatives considered

**`compile_error!` on `core` plus any tier.** This was rejected because it breaks the gate's
`--all-features`, as described above.

**Making the default feature set empty and `full` opt-in.** This was rejected: #127 requires
`cargo build -p ono-cli` to remain the full product.
