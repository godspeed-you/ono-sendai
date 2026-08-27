#!/usr/bin/env bash
# Builds the installable packages of the `ono` binary into dist/ (ADR-0121, ADR-0123):
#
#     dist/ono_<version>_<amd64|arm64>.deb
#     dist/ono-<version>-1.<x86_64|aarch64>.rpm
#
# The binary is always built inside a container, never with the host toolchain, so the glibc
# a package requires is the one of the build image and not of whichever machine ran the script:
# the host's own target builds in the acceptance image base (`rust:1.94-slim-bookworm`, glibc
# 2.36), a foreign target in cross's toolchain image for it. Both go through `cross`, which
# mounts the pinned toolchain of rust-toolchain.toml into the container.
#
# usage: scripts/package.sh [--target <triple>] [--no-build]
#   --target    x86_64-unknown-linux-gnu (default: the host) or aarch64-unknown-linux-gnu
#   --no-build  package what target/<triple>/release/ono already holds
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

BUILD_IMAGE="rust:1.94-slim-bookworm"

host_triple="$(rustc -vV | sed -n 's/^host: //p')"
target="$host_triple"
no_build=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --target) target="$2"; shift 2 ;;
    --target=*) target="${1#--target=}"; shift ;;
    --no-build) no_build=1; shift ;;
    *) echo "usage: scripts/package.sh [--target <triple>] [--no-build]" >&2; exit 2 ;;
  esac
done

case "$target" in
  x86_64-unknown-linux-gnu)  deb_arch=amd64; rpm_arch=x86_64 ;;
  aarch64-unknown-linux-gnu) deb_arch=arm64; rpm_arch=aarch64 ;;
  *) echo "package: no package layout for target $target" >&2; exit 2 ;;
esac

for tool in cargo-deb cargo-generate-rpm; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "package: $tool is not installed — cargo install --locked $tool" >&2
    exit 127
  fi
done

version="$(cargo pkgid --package ono-cli | sed 's/.*[#@]//')"
# Both tools clamp file timestamps and the build time to this, so the same commit yields the
# same package bytes on every machine.
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct)}"

step() { printf '\n\033[1m== %s\033[0m\n' "$*"; }

if [[ $no_build -eq 0 ]]; then
  if ! command -v cross >/dev/null 2>&1; then
    echo "package: cross is not installed — cargo install --locked cross" >&2
    exit 127
  fi
  if [[ "$target" == "$host_triple" ]]; then
    step "building ono for $target in $BUILD_IMAGE"
    image_var="CROSS_TARGET_$(echo "$target" | tr 'a-z-' 'A-Z_')_IMAGE"
    export "$image_var=$BUILD_IMAGE"
  else
    step "building ono for $target in cross's toolchain image"
    cat <<NOTE
package: $target is foreign to this $host_triple host. The packages will carry the right
architecture and binary, but dpkg-shlibdeps and ldd cannot read a foreign ELF, so their library
dependencies stay undeclared; the packages a release ships are built on a native runner
(ADR-0123, .github/workflows/release.yml).
NOTE
  fi
  cross build --release --locked --target "$target" --package ono-cli
fi

binary="target/$target/release/ono"
if [[ ! -x "$binary" ]]; then
  echo "package: $binary does not exist; build it or drop --no-build" >&2
  exit 1
fi

mkdir -p dist
deb="dist/ono_${version}_${deb_arch}.deb"
rpm="dist/ono-${version}-1.${rpm_arch}.rpm"

# The release profile already strips symbols; cargo-deb's own strip would need the target's
# binutils on the host and add nothing.
step "packaging $deb"
cargo deb --package ono-cli --no-build --no-strip --target "$target" --output "$deb"

step "packaging $rpm"
cargo generate-rpm --package crates/ono-cli --target "$target" --arch "$rpm_arch" --output "$rpm"

step "packages"
ls -l "$deb" "$rpm"
