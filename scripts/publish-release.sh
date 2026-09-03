#!/usr/bin/env bash
# Publishes a release, in the order §49.1 and §49.4 draw it (ADR-0532).
#
#     verify -> draft -> upload -> check what is attached, by digest -> publish
#
# The order is the whole point. A release that becomes visible before its inventory has been
# checked is the partially populated release §49.4 asks the workflow to avoid: it looks complete,
# and somebody downloads a package that is not there or not whole.
#
# Nothing here builds anything. The bytes it attaches were produced by the `package` job,
# installed and proven by `scripts/package-check.sh` in that same job, and carried here unchanged;
# §48.4 forbids rebuilding after the tests and uploading the rebuild. `--tested` is where that is
# checked, by hash, before this script runs.
#
# usage: scripts/publish-release.sh --tag <tag> [--dir <dir>]
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

tag=""
dir="dist"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --tag) tag="$2"; shift 2 ;;
    --tag=*) tag="${1#--tag=}"; shift ;;
    --dir) dir="$2"; shift 2 ;;
    --dir=*) dir="${1#--dir=}"; shift ;;
    *) echo "usage: scripts/publish-release.sh --tag <tag> [--dir <dir>]" >&2; exit 2 ;;
  esac
done
if [[ -z "$tag" ]]; then
  echo "publish-release: --tag names the release being published" >&2
  exit 2
fi
if ! command -v gh >/dev/null 2>&1; then
  echo "publish-release: the GitHub CLI is not installed" >&2
  exit 127
fi

cd "$dir"
dir="$PWD"
step() { printf '\n\033[1m== %s\033[0m\n' "$*"; }

# --- 1. verify, before anything exists on the release ---------------------------------------

step "verifying $tag before publishing it"
bash "$repo/scripts/verify-release.sh" --dir "$dir"

# --- 2. a draft, so nothing half-populated is ever visible -----------------------------------

step "drafting $tag"
if gh release view "$tag" >/dev/null 2>&1; then
  echo "publish-release: $tag already exists; uploading into it"
else
  gh release create "$tag" --draft --generate-notes --title "$tag"
fi

# --- 3. upload everything --------------------------------------------------------------------

step "uploading the assets of $tag"
mapfile -t assets < <(find . -maxdepth 1 -type f -printf '%f\n' | LC_ALL=C sort)
if [[ ${#assets[@]} -eq 0 ]]; then
  echo "publish-release: $dir holds nothing to publish" >&2
  exit 1
fi
gh release upload "$tag" "${assets[@]}" --clobber

# --- 4. the asset inventory ------------------------------------------------------------------
#
# By digest rather than by name: an upload that truncated has the right name and the wrong bytes,
# and a count of files cannot tell the difference (§62.6).

step "checking the asset inventory of $tag"
published="$(mktemp -d)"
trap 'rm -rf "$published"' EXIT
mapfile -t attached < <(gh release view "$tag" --json assets --jq '.assets[].name' | LC_ALL=C sort)
if [[ "${assets[*]}" != "${attached[*]}" ]]; then
  echo "publish-release: the release holds [${attached[*]}] and $dir holds [${assets[*]}]" >&2
  echo "publish-release: the draft stays a draft (spec §49.4)" >&2
  exit 1
fi
gh release download "$tag" --dir "$published" --clobber
for asset in "${assets[@]}"; do
  here="$(sha256sum "$dir/$asset" | cut -d' ' -f1)"
  there="$(sha256sum "$published/$asset" | cut -d' ' -f1)"
  if [[ "$here" != "$there" ]]; then
    echo "publish-release: $asset is $there on the release and $here here" >&2
    echo "publish-release: the draft stays a draft (spec §49.4, §62.6)" >&2
    exit 1
  fi
done
( cd "$published" && sha256sum --check --strict SHA256SUMS >/dev/null )
echo "publish-release: ${#attached[@]} assets, each byte-identical to what was verified here"

# --- 5. and only now is it a release ---------------------------------------------------------

step "publishing $tag"
gh release edit "$tag" --draft=false
printf '\033[1;32mpublish-release: %s published, %d assets\033[0m\n' "$tag" "${#attached[@]}"
