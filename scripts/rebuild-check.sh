#!/usr/bin/env bash
# Builds every publishable artifact twice and compares the bytes (spec §46.1, §46.5, ADR-0527).
#
# Two builds of one commit must produce identical packages. What differs between the two runs is
# chosen to be everything a build is allowed to see and not allowed to embed: locale, language,
# timezone, umask, temporary directory, build directory and output directory. `scripts/package.sh`
# fixes the first four before its own first tool runs (§46.2-§46.4), so a difference here is a
# difference in the artifacts and not in the shell that launched them.
#
# The comparison itself is `cargo xtask compare-builds`, which names the differing archive member
# rather than only the differing hash (§46.5).
#
# usage: scripts/rebuild-check.sh [--target <triple>] [--binary <path>] [--work <dir>]
#        scripts/rebuild-check.sh --compare <first-dir> <second-dir>
#
#   --target   the package layout to build (default: the host)
#   --binary   the `ono` to package (default: $CARGO_TARGET_DIR/<triple>/release/ono)
#   --work     where the two builds happen (default: target/reproducibility)
#   --compare  compare two directories that already exist and build nothing — how the release
#              workflow compares two *runners*, which is the freshest clean environment there is
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

target=""
binary=""
work="target/reproducibility"
compare_only=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --target) target="$2"; shift 2 ;;
    --target=*) target="${1#--target=}"; shift ;;
    --binary) binary="$2"; shift 2 ;;
    --binary=*) binary="${1#--binary=}"; shift ;;
    --work) work="$2"; shift 2 ;;
    --work=*) work="${1#--work=}"; shift ;;
    --compare) compare_only=("$2" "$3"); shift 3 ;;
    *) echo "usage: scripts/rebuild-check.sh [--target <triple>] [--binary <path>] [--work <dir>] | --compare <a> <b>" >&2; exit 2 ;;
  esac
done

step() { printf '\n\033[1m== %s\033[0m\n' "$*"; }

compare() {
  step "comparing $1 with $2"
  if cargo run --quiet --package xtask -- compare-builds "$1" "$2"; then
    printf '\033[1;32mrebuild-check: green — two builds of this commit are byte-for-byte identical\033[0m\n'
    return 0
  fi
  printf '\033[31mrebuild-check: red — this commit does not rebuild to the same bytes (spec §46.1)\033[0m\n' >&2
  return 1
}

if [[ ${#compare_only[@]} -eq 2 ]]; then
  compare "${compare_only[0]}" "${compare_only[1]}"
  exit $?
fi

target="${target:-$(rustc -vV | sed -n 's/^host: //p')}"
binary="${binary:-${CARGO_TARGET_DIR:-target}/$target/release/ono}"
if [[ ! -f "$binary" ]]; then
  echo "rebuild-check: $binary does not exist — build it, or name one with --binary" >&2
  exit 1
fi

# One commit, one date. §46.2 derives it once and both builds are handed the same value: two
# builds of one commit that disagreed about the date would be comparing two commits.
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct)}"

rm -rf "$work"
mkdir -p "$work"
# Absolute from here on: the two builds are handed directory names, and a relative one would be
# read against whatever directory each of them happens to be started in.
work="$(cd "$work" && pwd)"
binary="$(cd "$(dirname "$binary")" && pwd)/$(basename "$binary")"

# Each build gets its own everything, and a deliberately different environment. The second one is
# hostile on purpose: a German locale, a timezone at +08:45, a private umask. None of it may
# reach an artifact.
build_once() {
  local slot="$1" locale="$2" zone="$3" mask="$4"
  local root="$work/$slot"
  mkdir -p "$root/target/$target/release" "$root/dist" "$root/tmp"
  cp "$binary" "$root/target/$target/release/ono"

  step "build $slot — LC_ALL=$locale TZ=$zone umask=$mask"
  (
    umask "$mask"
    env \
      LC_ALL="$locale" LANG="$locale" LANGUAGE="${locale%%.*}" TZ="$zone" \
      TMPDIR="$root/tmp" \
      CARGO_TARGET_DIR="$root/target" \
      SOURCE_DATE_EPOCH="$SOURCE_DATE_EPOCH" \
      bash scripts/package.sh --target "$target" --no-build --dist "$root/dist"
  )
}

build_once a C.UTF-8 UTC 022
build_once b de_DE.UTF-8 Australia/Eucla 077

compare "$work/a/dist" "$work/b/dist"
