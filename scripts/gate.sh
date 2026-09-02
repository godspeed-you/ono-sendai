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
# The run's output is kept, because v0.4.1 §38.3 needs an observation: the static half of the
# rule refuses a skip `docs/spec/hardening/expected_test_skips.yaml` does not declare, and only a
# run can say which of the declared ones actually happened. `ono_testkit::skipped` writes its
# marker to the real standard error rather than through the captured macros, so a passing test's
# skip reaches this file (ADR-0513, ADR-0514).
TEST_LOG="${ONO_TEST_LOG:-target/gate-test.log}"
mkdir -p "$(dirname "$TEST_LOG")"
cargo test --workspace --all-features 2>&1 | tee "$TEST_LOG"

# §38.3: "A test that becomes skipped when it was expected to run MUST fail the CI gate or an
# explicit skip-verification step." The expectation is declared for the canonical CI environment,
# so it is enforced there and reported everywhere else — a developer machine without systemd
# legitimately skips what CI does not, and a gate that failed on it would teach people to skip
# the gate.
if [[ "${ONO_CANONICAL_CI:-0}" == "1" ]]; then
  step "skip verification"
  cargo run --quiet --package xtask -- skip-check "$TEST_LOG"
else
  observed="$(grep -c '^SKIPPED ' "$TEST_LOG" || true)"
  printf 'gate: %s test(s) announced a skip on this host; the declared expectation is enforced in CI (spec section 38.3)\n' "$observed"
fi

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

# Spec §45: the dependency policy of deny.toml — is anything here known to be vulnerable, is
# every licence one this project may ship, is the graph free of what we refuse, and did every
# crate come from a source we approved. It runs here rather than only in CI because a dependency
# is chosen on a developer machine, and an advisory found after the push is an advisory found
# after the decision (ADR-0449).
step "supply chain"
if ! command -v cargo-deny >/dev/null 2>&1; then
  echo "gate: cargo-deny is not installed — cargo install --locked cargo-deny@0.20.2" >&2
  exit 127
fi
cargo deny --locked --all-features check

step "contracts"
cargo run --quiet --package xtask -- spec-check

step "docs"
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --quiet

printf '\n\033[1;32mgate: green\033[0m\n'
