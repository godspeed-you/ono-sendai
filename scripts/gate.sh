#!/usr/bin/env bash
# The quality gate of AGENTS.md section 10. Every increment must pass this before it is
# committed. Runs identically on a developer machine, in CI and inside the container.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

step() { printf '\n\033[1m== %s\033[0m\n' "$*"; }

# Implementation happens on a feature branch so the whole run stays disposable
# (AGENTS.md section 12.1). `main` holds the specification, the instructions and this harness.
branch="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo unknown)"
if [[ "$branch" == "main" && "${ONO_ALLOW_MAIN:-0}" != "1" ]]; then
  cat >&2 <<'GUARD'
gate: refusing to run on `main`.

Implementation belongs on the `implementation` branch, so that the run can be discarded and
restarted from a clean `main` at any time (AGENTS.md section 12.1):

    git switch implementation || git switch --create implementation main

If you are the user working on the harness, the specification or the instructions themselves,
run the gate with ONO_ALLOW_MAIN=1.
GUARD
  exit 1
fi

step "format"
cargo fmt --all -- --check

step "lint"
cargo clippy --all-targets --all-features -- -D warnings

step "test"
cargo test --workspace --all-features

# Spec §35.6: the parser, the serializers, the remote protocol, the plugin protocol and the
# procfs/netlink decoders, each hammered from its seed corpus. Bounded by a fixed number of
# iterations rather than a clock, so a loaded machine gets the same answer as an idle one and a
# finding here reproduces on a developer machine from the same seed (ADR-0313).
#
# The per-input ceiling is loose here on purpose. The default of two seconds is for a campaign on
# a quiet machine; the gate may run on a loaded one, and a red gate that means "the machine was
# busy" is worse than a slow input that the next campaign catches. What the gate is for is the
# crashes.
step "fuzz"
cargo run --quiet --package ono-fuzz --all-features -- run --iterations 400 --per-input-ms 10000

step "contracts"
cargo run --quiet --package xtask -- spec-check

step "docs"
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --quiet

printf '\n\033[1;32mgate: green\033[0m\n'
