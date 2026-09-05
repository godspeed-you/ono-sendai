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
# rule refuses a skip `docs/contracts/hardening/expected_test_skips.yaml` does not declare, and only a
# run can say which of the declared ones actually happened. `ono_testkit::skipped` writes its
# marker to the real standard error rather than through the captured macros, so a passing test's
# skip reaches this file (ADR-0513, ADR-0514).
TEST_LOG="${ONO_TEST_LOG:-target/gate-test.log}"
mkdir -p "$(dirname "$TEST_LOG")"

# `xtask/tests/packaging.rs` is 114 s of a 385 s workspace run — the most expensive target in the
# repository, and the only one that exercises none of the Rust the workspace ships. It drives
# `cargo deb` and `cargo generate-rpm` over a stand-in binary and reads the packaging metadata,
# the release scripts, the Dockerfile and the release workflow. So this increment runs it when
# one of those inputs moved, and always in CI, where the `installable packages` job builds and
# installs the real packages regardless (ADR-0563).
#
# The two lists differ in what counts as a change, because the tests ask two different questions
# of them. `PACKAGING_INPUTS` is read: an edit anywhere in one of these can change what the
# packagers produce or what the suite asserts about them. `PACKAGING_ASSETS` is only shipped —
# the suite asserts that these arrive at their path inside the package, so it is their existence
# and their names that matter, and a run is selected by their addition, deletion or rename rather
# than by their prose changing.
PACKAGING_INPUTS=(
  Cargo.toml
  Cargo.lock
  crates/ono-cli/Cargo.toml
  crates/ono-cli/packaging
  docker/Dockerfile
  .github/workflows/release.yml
  scripts/package.sh
  scripts/package-check.sh
  scripts/rebuild-check.sh
  scripts/release-check.sh
  xtask/src/main.rs
  xtask/src/provenance.rs
  xtask/src/reproducibility.rs
  xtask/tests/packaging.rs
  xtask/tests/support
)
PACKAGING_ASSETS=(LICENSE README.md docs/reference)

# The baseline is the working tree against `HEAD`, because section 10 puts the gate *before* the
# commit: what it is asked about is the increment on its way in. A `git` that cannot answer —
# no repository, a broken index, a detached state — selects the suite, so an unanswered question
# never costs coverage.
packaging_selected() {
  case "${ONO_PACKAGING:-auto}" in
    always) return 0 ;;
    never) return 1 ;;
  esac
  [[ "${ONO_CANONICAL_CI:-0}" == "1" ]] && return 0
  git rev-parse --verify --quiet HEAD >/dev/null 2>&1 || return 0
  local changed
  changed="$(
    git diff --name-only HEAD -- "${PACKAGING_INPUTS[@]}" &&
      git ls-files --others --exclude-standard -- "${PACKAGING_INPUTS[@]}" &&
      git diff --name-only --diff-filter=ADR HEAD -- "${PACKAGING_ASSETS[@]}" &&
      git ls-files --others --exclude-standard -- "${PACKAGING_ASSETS[@]}"
  )" || return 0
  [[ -n "$changed" ]]
}

# Not selected is not skipped. `ono_testkit::skipped` and `expected_test_skips.yaml` are the
# register of what a *host* could not supply, and none of §38.4's six categories describes "this
# increment did not touch it" — so the honesty here is libtest's own filter, which reports the
# unselected tests as `filtered out` in the summary this file keeps. §38.3's observation is
# unaffected: a filtered test announces nothing, and `skip-check` runs only in CI, where the
# suite is always selected.
test_filter=()
if packaging_selected; then
  printf 'gate: the packaging suite is selected\n'
else
  # Read out of the file rather than listed here, so a test added to the suite cannot be left
  # behind by a list nobody updated. An extraction that finds nothing selects the suite.
  mapfile -t packaging_tests < <(
    awk '/^#\[test\]/ { want = 1; next }
         want && /^fn / { name = $2; sub(/\(.*/, "", name); print name; want = 0 }
         want && !/^#\[/ { want = 0 }' xtask/tests/packaging.rs
  )
  if [[ ${#packaging_tests[@]} -eq 0 ]]; then
    printf 'gate: the packaging suite is selected — its tests could not be enumerated\n'
  else
    test_filter=(-- --exact)
    for packaging_test in "${packaging_tests[@]}"; do
      test_filter+=(--skip "$packaging_test")
    done
    printf 'gate: %s packaging test(s) are not selected — this increment touches no input of xtask/tests/packaging.rs (ADR-0563). ONO_PACKAGING=always runs them.\n' \
      "${#packaging_tests[@]}"
  fi
fi

cargo test --workspace --all-features "${test_filter[@]}" 2>&1 | tee "$TEST_LOG"

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
