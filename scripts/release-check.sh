#!/usr/bin/env bash
# The release gate of docs/ACCEPTANCE.md. An agent run ends when this passes - not before.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

# A release is a commit. The packages are built from the working tree, so a release-check over
# uncommitted edits or untracked files qualifies bytes no commit describes — a README nobody
# committed, a reference page nobody generated from the registries. Refused before anything runs.
# `ONO_RELEASE_ALLOW_DIRTY=1` lets a developer rehearse the gate over work in progress, and says
# so; a release is never cut that way.
if ! changes="$(git status --porcelain --untracked-files=all 2>&1)"; then
  printf 'release-check: git cannot describe this tree, so no commit is being released:\n%s\n' "$changes" >&2
  exit 1
fi
if [[ -n "$changes" ]]; then
  if [[ "${ONO_RELEASE_ALLOW_DIRTY:-0}" == "1" ]]; then
    printf '\033[33mrelease-check: ONO_RELEASE_ALLOW_DIRTY=1 — qualifying a tree with uncommitted changes; this is a rehearsal, not a release:\033[0m\n%s\n' "$changes"
  else
    printf 'release-check: the tree has uncommitted changes, and a release is a commit:\n%s\n' "$changes" >&2
    printf 'release-check: commit or remove them; ONO_RELEASE_ALLOW_DIRTY=1 rehearses the gate anyway\n' >&2
    exit 1
  fi
fi

printf '\033[1m== quality gate\033[0m\n'
scripts/gate.sh

printf '\n\033[1m== containerised acceptance\033[0m\n'
scripts/acceptance.sh

# The packages of the host architecture, built and installed in fresh containers
# (docs/ACCEPTANCE.md section 4.5, ADR-0121). The other architecture is proven the same way on a
# native runner in .github/workflows/release.yml (ADR-0123).
#
# Into a directory this run owns and empties first, not into dist/: a second run at another
# version found the first run's packages and manifest still in dist/, validated the new packages
# against the old manifest and refused (issue #145). What it builds here is the release it
# qualified, and nothing else.
dist="target/release-check/dist"
rm -rf "$dist"
mkdir -p "$dist"
printf '\n\033[1m== installable packages\033[0m\n'
scripts/package.sh --dist "$dist"
scripts/package-check.sh --dist "$dist"

# §46.5: every publishable artifact, built twice in two clean environments that disagree about
# locale, timezone, umask and directories, and compared byte for byte (ADR-0527). The release
# workflow runs the same comparison across two *runners*, per architecture (§46.6).
printf '\n\033[1m== two builds of this commit\033[0m\n'
scripts/rebuild-check.sh

# §47.1-§47.2: the digest of every downloadable artifact, in one manifest, in deterministic
# order — and the check that runs in both directions, so an artifact nobody hashed fails the
# release rather than shipping unattested (ADR-0528).
printf '\n\033[1m== checksum manifest\033[0m\n'
cargo run --quiet --package xtask -- checksums --dir "$dist"
cargo run --quiet --package xtask -- checksums --dir "$dist" --verify
printf 'release-check: the packages and their manifest are in %s\n' "$dist"

printf '\n\033[1m== release checklist\033[0m\n'
if grep -n '^- \[ \]' docs/ACCEPTANCE.md; then
  printf '\n\033[31mrelease-check: open items remain in docs/ACCEPTANCE.md\033[0m\n'
  exit 1
fi

# Three boxes of the checklist are claims about the work board rather than about the shell:
# section 4.5 Delivery, section 4.6.5 Delivery and section 4.7.2 "No release-blocking known
# defects remain" all assert that `docs/STATE.md` holds no claim and no unexplained deferral.
# Until ADR-0402 nothing read that file, so those boxes were true on the day they were written
# and unexamined afterwards. The gate does not run this: holding a claim mid-run is correct.
printf '\n\033[1m== the work board\033[0m\n'
if ! cargo run --quiet --package xtask -- state-check; then
  printf '\n\033[31mrelease-check: docs/STATE.md says the work is not finished\033[0m\n'
  exit 1
fi

printf '\n\033[1;32mrelease-check: the shell is release-ready\033[0m\n'
