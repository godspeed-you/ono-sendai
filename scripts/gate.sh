#!/usr/bin/env bash
# The quality gate of AGENTS.md section 10. Every increment must pass this before it is
# committed. Runs identically on a developer machine, in CI and inside the container.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

step() { printf '\n\033[1m== %s\033[0m\n' "$*"; }

step "format"
cargo fmt --all -- --check

step "lint"
cargo clippy --all-targets --all-features -- -D warnings

step "test"
cargo test --workspace --all-features

step "contracts"
cargo run --quiet --package xtask -- spec-check

step "docs"
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --quiet

printf '\n\033[1;32mgate: green\033[0m\n'
