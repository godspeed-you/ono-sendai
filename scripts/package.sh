#!/usr/bin/env bash
# Builds the installable packages of the `ono` binary, and of the `kuang-compile` that
# `install plugin` runs beside it (ADR-0870, ADR-0905), into dist/ (ADR-0121, ADR-0123):
#
#     dist/ono_<version>_<amd64|arm64>.deb
#     dist/ono-<version>-1.<x86_64|aarch64>.rpm
#
# The binary is always built inside a container, never with the host toolchain, so the glibc
# a package requires is the one of the build image and not of whichever machine ran the script:
# the host's own target builds in the acceptance image base (`rust:1.94-slim-bookworm`, glibc
# 2.36) run directly, a foreign target in cross's toolchain image for it. The native build does
# not go through `cross`: cross installs an `x86_64` toolchain inside whatever image it runs,
# which fails on an arm64 runner where the image and the target are aarch64 (ADR-0123).
#
# usage: scripts/package.sh [--target <triple>] [--no-build] [--dist <dir>] [--print-determinism]
#   --target             x86_64-unknown-linux-gnu (default: the host) or aarch64-unknown-linux-gnu
#   --no-build           package what $CARGO_TARGET_DIR/<triple>/release/{ono,kuang-compile}
#                        already hold
#   --dist <dir>         write the packages here instead of dist/ — one rebuild of §46.5 per dir
#   --print-determinism  print the four inputs of spec §46.2-§46.4 and exit, building nothing
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

BUILD_IMAGE="rust:1.94-slim-bookworm@sha256:cf9dd0ec73e75f827fe59123fff9dc65af1a1c8363c3c31ee8d7f8ad0b6a5fb2"

# --- determinism inputs (spec §46.2-§46.4, ADR-0526) ---------------------------------------
#
# Fixed here, before any tool has a chance to read the environment it was started in. Every one
# of these can reach an artifact field: a locale decides how a tool formats a number into a
# control file, a timezone decides what a timestamp renders as, a umask decides the mode of a
# staged file, and the build time decides mtimes. A release that inherits them from whoever ran
# it is a release nobody else can rebuild (§65.11).
export LC_ALL=C.UTF-8
export LANG=C.UTF-8
export LANGUAGE=C
export TZ=UTC
umask 022

# §46.4: the identity the packages record. Both packaging tools write root:root; stating it here
# is what makes the rule visible to the reader and to the test that holds it.
PACKAGE_UID=0
PACKAGE_GID=0

# §46.2: derived from the release commit, never from the clock. `${VAR-}` rather than `${VAR:-}`
# on purpose - an empty value the caller set explicitly is not a value this script may replace,
# it is a value it must refuse.
SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH-}"
if [[ -z "$SOURCE_DATE_EPOCH" ]]; then
  SOURCE_DATE_EPOCH="$(git log -1 --format=%ct 2>/dev/null || true)"
fi
export SOURCE_DATE_EPOCH

# Refuses rather than falls back. A wall-clock default here would make every later comparison
# in §46.5 fail for a reason nobody could see from the artifacts.
require_determinism() {
  local missing=()
  [[ "$SOURCE_DATE_EPOCH" =~ ^[0-9]+$ ]] || missing+=("SOURCE_DATE_EPOCH")
  [[ "${LC_ALL:-}" == "C.UTF-8" ]]       || missing+=("LC_ALL")
  [[ "${TZ:-}" == "UTC" ]]               || missing+=("TZ")
  [[ "$(umask)" == "0022" ]]             || missing+=("umask")
  if [[ ${#missing[@]} -gt 0 ]]; then
    echo "package: determinism input not set: ${missing[*]}" >&2
    echo "package: spec §46.2-§46.4 fixes SOURCE_DATE_EPOCH, LC_ALL, TZ and the umask before a" >&2
    echo "package: release build, and no wall-clock time may stand in for a missing one. Set" >&2
    echo "package: SOURCE_DATE_EPOCH to the release commit's timestamp, or build from a checkout" >&2
    echo "package: git can date." >&2
    exit 1
  fi
}

target=""
no_build=0
print_determinism=0
# Where the binary is looked for and where the packages are written. Two rebuilds of one commit
# need two of each, and §46.5 needs them not to share a directory (ADR-0527).
target_dir="${CARGO_TARGET_DIR:-target}"
dist_dir="dist"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --target) target="$2"; shift 2 ;;
    --target=*) target="${1#--target=}"; shift ;;
    --no-build) no_build=1; shift ;;
    --dist) dist_dir="$2"; shift 2 ;;
    --dist=*) dist_dir="${1#--dist=}"; shift ;;
    --print-determinism) print_determinism=1; shift ;;
    *) echo "usage: scripts/package.sh [--target <triple>] [--no-build] [--dist <dir>] [--print-determinism]" >&2; exit 2 ;;
  esac
done

require_determinism

if [[ $print_determinism -eq 1 ]]; then
  echo "SOURCE_DATE_EPOCH=$SOURCE_DATE_EPOCH"
  echo "LC_ALL=$LC_ALL"
  echo "LANG=$LANG"
  echo "TZ=$TZ"
  echo "umask=$(umask)"
  echo "owner=$PACKAGE_UID:$PACKAGE_GID"
  exit 0
fi

host_triple="$(rustc -vV | sed -n 's/^host: //p')"
target="${target:-$host_triple}"

case "$target" in
  x86_64-unknown-linux-gnu)  deb_arch=amd64; rpm_arch=x86_64 ;;
  aarch64-unknown-linux-gnu) deb_arch=arm64; rpm_arch=aarch64 ;;
  *) echo "package: no package layout for target $target" >&2; exit 2 ;;
esac

# The exact versions of Cargo.toml's [workspace.metadata.release-tools] (spec §44.2, ADR-0450).
# These two lay out the package, so a different version is a different artifact from the same
# commit — the script refuses rather than producing something the release cannot reproduce.
require_tool() {
  local tool="${1%@*}" want="${1#*@}" have
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "package: $tool is not installed — cargo install --locked $tool@$want" >&2
    exit 127
  fi
  # Each of them prints `<its own name> <version>` first; `cross` prints rustup chatter around it.
  if [[ "$tool" == cargo-* ]]; then
    have="$(cargo "${tool#cargo-}" --version 2>/dev/null)"
  else
    have="$("$tool" --version 2>/dev/null)"
  fi
  have="$(printf '%s\n' "$have" | sed -n "s/^$tool \\([0-9][^ ]*\\).*/\\1/p" | head -1)"
  if [[ "$have" != "$want" ]]; then
    echo "package: $tool is $have and the release is built with $want — cargo install --locked $tool@$want" >&2
    echo "package: the packaging tool decides what the artifact is; two versions are two packages (spec §44.2)" >&2
    exit 127
  fi
}

require_tool cargo-deb@3.7.0
require_tool cargo-generate-rpm@0.21.0

version="$(cargo pkgid --package ono-cli | sed 's/.*[#@]//')"

step() { printf '\n\033[1m== %s\033[0m\n' "$*"; }

# A release directory holds one build (issue #145). Anything already in it that this run does
# not write — packages of another version or architecture, a manifest, its signature, provenance,
# build inputs, a stray file — would sit beside what this run writes: `xtask checksums` covers
# every file there, and package validation compares the new digests with an old manifest. So the
# run refuses first, before a build it could not use, and names what is in the way. The two files
# it writes itself, from an earlier run of this version and architecture, it replaces.
if [[ -d "$dist_dir" ]]; then
  foreign=()
  shopt -s dotglob nullglob
  for found in "$dist_dir"/*; do
    case "${found##*/}" in
      "ono_${version}_${deb_arch}.deb" | "ono-${version}-1.${rpm_arch}.rpm") ;;
      *) foreign+=("$found") ;;
    esac
  done
  shopt -u dotglob nullglob
  if [[ ${#foreign[@]} -gt 0 ]]; then
    echo "package: $dist_dir already holds artifacts of another build:" >&2
    printf 'package:   %s\n' "${foreign[@]}" >&2
    echo "package: this run writes $version for $deb_arch, and a release directory holds one build — remove" >&2
    echo "package: them, or write this one elsewhere with --dist <dir>" >&2
    exit 1
  fi
fi

# Two cargo invocations, never one: cargo unifies features across the packages of an invocation,
# and `ono-kuang-sdk` turns on the supervisor's `compiler` feature for `kuang-compile`. Built
# together, `ono` would link the Cranelift ADR-0870 took out of it; package-check.sh looks for it
# in the packaged shell (ADR-0905).
build_both="cargo build --release --locked --timings --target $target --package ono-cli \
&& cargo build --release --locked --timings --target $target --package ono-kuang-sdk --bin kuang-compile"

if [[ $no_build -eq 0 ]]; then
  if [[ "$target" == "$host_triple" ]]; then
    step "building ono for $target in $BUILD_IMAGE"
    runtime=""
    for candidate in docker podman; do
      if command -v "$candidate" >/dev/null 2>&1; then runtime="$candidate"; break; fi
    done
    if [[ -z "$runtime" ]]; then
      echo "package: neither docker nor podman is available" >&2
      exit 127
    fi
    # The image is multi-architecture, so this is the same command on an x86_64 and on an
    # arm64 runner. Cargo writes as the invoking user into a cache under target/, so nothing
    # the container leaves behind is root's. `--timings` writes target/cargo-timings/, the answer
    # to where the release build's minutes go; CI publishes it (issue #218, ADR-0865). It changes
    # what is reported, not what is built.
    "$runtime" run --rm \
      --user "$(id -u):$(id -g)" \
      --volume "$PWD:/project" \
      --workdir /project \
      --env CARGO_HOME=/project/target/container-cargo \
      --env CARGO_INCREMENTAL=0 \
      --env "SOURCE_DATE_EPOCH=$SOURCE_DATE_EPOCH" \
      --env LC_ALL=C.UTF-8 --env LANG=C.UTF-8 --env TZ=UTC \
      "$BUILD_IMAGE" \
      sh -c "$build_both"
    # The container wrote the checkout's own target/, whatever CARGO_TARGET_DIR says on this
    # host: package what was just built, not what happens to sit there.
    target_dir="target"
  else
    require_tool cross@0.2.5
    step "building ono for $target in cross's toolchain image"
    cat <<NOTE
package: $target is foreign to this $host_triple host. The packages will carry the right
architecture and binary, but dpkg-shlibdeps and ldd cannot read a foreign ELF, so their library
dependencies stay undeclared; the packages a release ships are built on a native runner
(ADR-0123, .github/workflows/release.yml).
NOTE
    cross build --release --locked --timings --target "$target" --package ono-cli
    cross build --release --locked --timings --target "$target" --package ono-kuang-sdk --bin kuang-compile
  fi
fi

binary="$target_dir/$target/release/ono"
compiler="$target_dir/$target/release/kuang-compile"
for built in "$binary" "$compiler"; do
  if [[ ! -x "$built" ]]; then
    echo "package: $built does not exist; build it or drop --no-build" >&2
    echo "package: the packages ship \`ono\` and the \`kuang-compile\` that \`install plugin\` runs" >&2
    echo "package: beside it (ADR-0870); a shell without it cannot install a component package" >&2
    exit 1
  fi
done

mkdir -p "$dist_dir"
deb="$dist_dir/ono_${version}_${deb_arch}.deb"
rpm="$dist_dir/ono-${version}-1.${rpm_arch}.rpm"

# Both packages are built from one staged copy: the files git tracks — not what the working tree
# merely holds, untracked pages and dist/ included — and the two binaries, every mtime the epoch
# (§46.4; issues #145, #146; ADR-0902). A tree that is not the top of a repository — the copy
# rebuild-check.sh packages — is taken as it is: it was made from the tracked files.
stage="$(mktemp -d "${TMPDIR:-/tmp}/ono-package.XXXXXX")"
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/src" "$stage/target/$target/release"
top="$(git rev-parse --show-toplevel 2>/dev/null || true)"
if [[ -n "$top" && "$(cd "$top" && pwd -P)" == "$(pwd -P)" ]]; then
  git ls-files -z --cached | tar --null --files-from=- --ignore-failed-read -cf - 2>/dev/null \
    | tar -xf - -C "$stage/src"
else
  tar -cf - . | tar -xf - -C "$stage/src"
fi
cp "$binary" "$stage/target/$target/release/ono"
cp "$compiler" "$stage/target/$target/release/kuang-compile"
find "$stage" -exec touch -h -d "@$SOURCE_DATE_EPOCH" {} +
deb="$(cd "$(dirname "$deb")" && pwd)/$(basename "$deb")"
rpm="$(cd "$(dirname "$rpm")" && pwd)/$(basename "$rpm")"

# The release profile already strips symbols; cargo-deb's own strip would need the target's
# binutils on the host and add nothing.
step "packaging $deb"
(
  cd "$stage/src"
  CARGO_TARGET_DIR="$stage/target" \
    cargo deb --package ono-cli --no-build --no-strip --target "$target" --output "$deb"
)

# §46.4 for the RPM: cargo-generate-rpm *clamps* an asset's mtime to SOURCE_DATE_EPOCH rather than
# setting it, so a source file older than the commit would carry its own mtime into
# RPMTAG_FILEMTIMES and the payload (issue #146, ADR-0902). The staged copy's every mtime is the
# epoch; cargo-deb sets every member to it anyway.
step "packaging $rpm"
(
  cd "$stage/src"
  cargo generate-rpm --package crates/ono-cli --target-dir "$stage/target" --target "$target" \
    --arch "$rpm_arch" --output "$rpm"
)

step "packages"
ls -l "$deb" "$rpm"
