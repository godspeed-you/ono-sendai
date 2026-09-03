#!/usr/bin/env bash
# Signs the checksum manifest of a release, keyless (spec §47.1, §47.3, ADR-0529).
#
# There is no private key here and none in the repository's secrets. cosign asks GitHub's OIDC
# provider for a short-lived token proving *which workflow of which repository on which ref* is
# asking, exchanges it at Fulcio for a certificate that lives for ten minutes, signs with it, and
# records the signature in Rekor. What a reader verifies against afterwards is that identity —
# `.github/workflows/release.yml@refs/tags/v…` — rather than a key somebody has to keep.
#
# It is therefore only runnable inside the release workflow, whose `publish` job holds
# `id-token: write`. Nothing a fork can start reaches it: the workflow triggers on a tag push
# (spec §43.4, ADR-0433).
#
# The run verifies its own signature before it returns. A signature that cannot be checked is
# worse than none, and it must fail the release rather than reach a reader.
#
# usage: scripts/sign-release.sh [--dir <dir>]
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

dir="dist"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --dir) dir="$2"; shift 2 ;;
    --dir=*) dir="${1#--dir=}"; shift ;;
    *) echo "usage: scripts/sign-release.sh [--dir <dir>]" >&2; exit 2 ;;
  esac
done

if ! command -v cosign >/dev/null 2>&1; then
  echo "sign-release: cosign is not installed; the release workflow installs it at a pinned version" >&2
  exit 127
fi
if [[ -z "${ACTIONS_ID_TOKEN_REQUEST_URL:-}" ]]; then
  echo "sign-release: no OIDC identity is available in this environment." >&2
  echo "sign-release: keyless signing proves which workflow of which repository signed, and only" >&2
  echo "sign-release: a run holding \`id-token: write\` can prove that. A local signature would" >&2
  echo "sign-release: attest to a developer machine (spec §47.3)." >&2
  exit 1
fi

cd "$dir"
if [[ ! -f SHA256SUMS ]]; then
  echo "sign-release: SHA256SUMS is missing; there is nothing to sign (spec §47.1)" >&2
  exit 1
fi

printf '\n\033[1m== signing SHA256SUMS\033[0m\n'
cosign sign-blob --yes --bundle SHA256SUMS.sigstore.json SHA256SUMS

printf '\n\033[1m== checking what was just signed\033[0m\n'
bash "$repo/scripts/verify-release.sh" --dir "$PWD" --without-provenance

printf '\033[1;32msign-release: SHA256SUMS carries a verifiable signature\033[0m\n'
