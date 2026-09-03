#!/usr/bin/env bash
# Verifies a release the way a reader verifies one (spec §47.5, §67.7, ADR-0529).
#
# Three questions, in the order a reader asks them:
#
#   1. do the files I downloaded hash to what SHA256SUMS says?
#   2. was SHA256SUMS signed by this project's release workflow?
#   3. does the provenance bind every one of those digests to that build?
#
# Nothing here needs a proprietary service and nothing here needs this repository: coreutils and
# cosign, against files a reader already has. It is the same script the release workflow runs on
# itself before it publishes anything, so what a reader checks is what the release checked.
#
# usage: scripts/verify-release.sh [--dir <dir>] [--without-signature] [--without-provenance]
#
#   --dir                 the directory holding the release assets (default: dist)
#   --without-signature   check the digests only. For a local release check, which has no OIDC
#                         identity to sign with; never for a published release.
#   --without-provenance  skip the provenance cross-check, for the same reason.
set -euo pipefail

# The signing identity of this project: its release workflow, on a version tag, authenticated by
# GitHub's OIDC issuer. A verification without this accepts a signature from anybody Fulcio has
# ever issued a certificate to (spec §47.3).
IDENTITY='^https://github\.com/godspeed-you/ono-sendai/\.github/workflows/release\.yml@refs/tags/v'
ISSUER='https://token.actions.githubusercontent.com'

MANIFEST="SHA256SUMS"
BUNDLE="SHA256SUMS.sigstore.json"
PROVENANCE="build-provenance.json"

dir="dist"
signature=1
provenance=1
while [[ $# -gt 0 ]]; do
  case "$1" in
    --dir) dir="$2"; shift 2 ;;
    --dir=*) dir="${1#--dir=}"; shift ;;
    --without-signature) signature=0; shift ;;
    --without-provenance) provenance=0; shift ;;
    *) echo "usage: scripts/verify-release.sh [--dir <dir>] [--without-signature] [--without-provenance]" >&2; exit 2 ;;
  esac
done

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# The release tooling, however this machine has it. A reader with the repository checked out
# builds it; the acceptance container and a release runner are handed one that is already built,
# because neither has a workspace to compile.
release_tool() {
  if [[ -n "${ONO_XTASK:-}" ]]; then
    "$ONO_XTASK" "$@"
  else
    (cd "$repo" && cargo run --locked --quiet --package xtask -- "$@")
  fi
}

cd "$dir"
dir="$PWD"

failed=0
step() { printf '\n\033[1m== %s\033[0m\n' "$*"; }
ok()   { printf '  \033[32mok\033[0m   %s\n' "$*"; }
fail() { printf '  \033[31mFAIL\033[0m %s\n' "$*" >&2; failed=1; }

# --- 1. the digests ------------------------------------------------------------------------

step "$MANIFEST"
if [[ ! -f "$MANIFEST" ]]; then
  fail "$MANIFEST is not here. A release publishes it beside the packages (spec §47.1)"
else
  # The command spec §67.7 documents, run as documented. `--strict` so a malformed line is a
  # failure rather than a warning nobody reads.
  if output="$(LC_ALL=C sha256sum --check --strict "$MANIFEST" 2>&1)"; then
    ok "every artifact hashes to what $MANIFEST records"
  else
    printf '%s\n' "$output" >&2
    fail "$MANIFEST does not describe the files beside it"
  fi
fi

# --- 2. the signature ----------------------------------------------------------------------

if [[ $signature -eq 1 ]]; then
  step "$BUNDLE"
  if [[ ! -f "$BUNDLE" ]]; then
    fail "$BUNDLE is not here, so nothing says who produced $MANIFEST. A published release \
carries a verifiable signature over its checksum manifest (spec §47.1, §2.3)"
  elif ! command -v cosign >/dev/null 2>&1; then
    fail "cosign is not installed — see https://docs.sigstore.dev/cosign/installation/ — and \
without it the signature over $MANIFEST cannot be checked (spec §47.3)"
  elif output="$(cosign verify-blob \
        --bundle "$BUNDLE" \
        --certificate-identity-regexp "$IDENTITY" \
        --certificate-oidc-issuer "$ISSUER" \
        "$MANIFEST" 2>&1)"; then
    ok "$MANIFEST was signed by this project's release workflow on a version tag"
  else
    printf '%s\n' "$output" >&2
    fail "the signature over $MANIFEST does not verify against the published identity"
  fi
else
  step "$BUNDLE"
  printf '  skipped — asked for with --without-signature. A published release is not verified\n'
  printf '  until this step passes (spec §47.1).\n'
fi

# --- 3. the provenance ---------------------------------------------------------------------

if [[ $provenance -eq 1 ]]; then
  step "$PROVENANCE"
  if [[ ! -f "$PROVENANCE" ]]; then
    fail "$PROVENANCE is not here, so nothing binds these digests to the build that made them \
(spec §47.1, §47.4)"
  elif output="$(release_tool provenance --dir "$PWD" --verify 2>&1)"; then
    printf '%s\n' "$output"
    ok "every digest appears in both $MANIFEST and $PROVENANCE"
  else
    printf '%s\n' "$output" >&2
    fail "the provenance does not account for every published artifact"
  fi
else
  step "$PROVENANCE"
  printf '  skipped — asked for with --without-provenance.\n'
fi

echo
if [[ $failed -ne 0 ]]; then
  printf '\033[31mverify-release: red — do not install these files\033[0m\n' >&2
  exit 1
fi
printf '\033[1;32mverify-release: green\033[0m\n'
