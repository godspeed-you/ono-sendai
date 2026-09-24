#!/usr/bin/env bash
# Builds the core `ono` (#127, ADR-0912): the object shell without its enhancements, statically
# linked against musl, for container images and embedded Linux targets.
#
#     target/x86_64-unknown-linux-musl/release/ono
#
# It is the exit test of #127 as a command:
#
#     cargo build --release -p ono-cli --no-default-features --features core \
#       --target x86_64-unknown-linux-musl
#
# at the release profile of ADR-0863 (`opt-level = "s"`, fat LTO, one codegen unit, stripped; no
# `opt-level = "z"`, no `panic = "abort"`), and then the three things the exit test claims about
# the result are checked rather than assumed: the binary is static, it runs, and it is under its
# size budget.
#
# The host toolchain builds it, deliberately. A static musl binary depends on no libc of the
# machine that built it, the toolchain is the one `rust-toolchain.toml` pins, and the core
# dependency graph contains no C code — no `ring`, no SQLite amalgamation — so no musl C compiler
# is needed; rustup's `x86_64-unknown-linux-musl` target carries the C runtime it links. The
# script adds that target when it is missing.
#
# usage: scripts/build-core.sh [--stage <dir> [--run-image]] [--no-build]
#   --stage <dir>  also lay out the build context of docker/core/Dockerfile in <dir>
#   --run-image    build the deliverable `core` stage from <dir>, run it as it ships — its own
#                  ENTRYPOINT and USER, no harness in it — and remove it again (ADR-0925)
#   --no-build     check and stage what target/x86_64-unknown-linux-musl/release/ono already holds
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

TARGET="x86_64-unknown-linux-musl"
STAGE=""
NO_BUILD=0
RUN_IMAGE=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --stage)
      if [[ $# -lt 2 ]]; then
        echo "build-core: --stage needs a directory" >&2
        exit 1
      fi
      STAGE="$2"
      shift ;;
    --no-build) NO_BUILD=1 ;;
    --run-image) RUN_IMAGE=1 ;;
    *) echo "build-core: unknown argument $1" >&2; exit 1 ;;
  esac
  shift
done

if [[ $RUN_IMAGE -eq 1 && -z "$STAGE" ]]; then
  echo "build-core: --run-image builds the image from the staged context, so it needs --stage" >&2
  exit 1
fi

BINARY="${CARGO_TARGET_DIR:-target}/$TARGET/release/ono"

if [[ $NO_BUILD -eq 0 ]]; then
  if ! rustup target list --installed 2>/dev/null | grep -qx "$TARGET"; then
    rustup target add "$TARGET"
  fi
  # The release profile of ADR-0863 §1. Stated here as well as in Cargo.toml so the core build
  # is measured at that profile whatever the workspace's own profile says on this branch.
  CARGO_PROFILE_RELEASE_OPT_LEVEL=s \
  CARGO_PROFILE_RELEASE_LTO=fat \
  CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 \
  CARGO_PROFILE_RELEASE_STRIP=symbols \
    cargo build --release --locked --package ono-cli --no-default-features --features core \
      --target "$TARGET"
fi

if [[ ! -f "$BINARY" ]]; then
  echo "build-core: $BINARY does not exist; run without --no-build" >&2
  exit 1
fi

# Static: no program interpreter, so nothing on the target machine is asked to load it.
if ! command -v readelf >/dev/null 2>&1; then
  echo "build-core: readelf is needed to prove the binary static (binutils)" >&2
  exit 127
fi
if readelf --program-headers --wide "$BINARY" | grep -q 'INTERP'; then
  echo "build-core: $BINARY names a program interpreter, so it is not statically linked" >&2
  exit 1
fi
if readelf --dynamic --wide "$BINARY" | grep -q '(NEEDED)'; then
  echo "build-core: $BINARY needs shared libraries:" >&2
  readelf --dynamic --wide "$BINARY" | grep '(NEEDED)' >&2
  exit 1
fi

# It runs, and it says which build it is.
version="$("$BINARY" --version)"
if ! grep -qx 'build: core (without .*)' <<<"$version"; then
  echo "build-core: $BINARY does not identify itself as the core build:" >&2
  echo "$version" >&2
  exit 1
fi

# Under its budget: `build.ono_core_stripped_bytes`, #127's "a static binary under 8 MB", which
# lives with every other size budget in the hardening limits registry and is read by the script
# the package builds use too (ADR-0864, ADR-0866).
scripts/binary-size.sh --triple "$TARGET" "$BINARY"
echo "build-core: $BINARY is static"

if [[ -n "$STAGE" ]]; then
  # The image's whole filesystem, with the modes it will have: `COPY` keeps them, and a mode
  # set with `COPY --chmod` would apply to the directories it creates too.
  rm -rf "$STAGE"
  mkdir -p "$STAGE/rootfs/etc" "$STAGE/rootfs/usr/local/bin" "$STAGE/rootfs/tmp" "$STAGE/home"
  chmod 0755 "$STAGE/rootfs" "$STAGE/rootfs/etc" "$STAGE/rootfs/usr" "$STAGE/rootfs/usr/local" \
    "$STAGE/rootfs/usr/local/bin" "$STAGE/home"
  chmod 1777 "$STAGE/rootfs/tmp"
  install -m 0755 "$BINARY" "$STAGE/rootfs/usr/local/bin/ono"
  install -m 0644 docker/core/passwd docker/core/group "$STAGE/rootfs/etc/"
  echo "build-core: staged the build context of docker/core/Dockerfile in $STAGE"
fi

if [[ $RUN_IMAGE -eq 1 ]]; then
  runtime=""
  for candidate in docker podman; do
    if command -v "$candidate" >/dev/null 2>&1; then runtime="$candidate"; break; fi
  done
  if [[ -z "$runtime" ]]; then
    echo "build-core: --run-image needs docker or podman" >&2
    exit 127
  fi
  # Unique per run, so two checkouts never share or remove each other's image; always removed.
  DELIVERABLE_IMAGE="ono-sendai:core-deliverable-$$"
  trap '"$runtime" image rm --force "$DELIVERABLE_IMAGE" >/dev/null 2>&1 || true' EXIT
  "$runtime" build --quiet --file docker/core/Dockerfile --target core \
    --tag "$DELIVERABLE_IMAGE" "$STAGE" >/dev/null
  # The image as it ships: its ENTRYPOINT is `ono`, its USER is `case`, and nothing else is in it.
  shipped="$("$runtime" run --rm --network=none "$DELIVERABLE_IMAGE" --version)"
  if ! grep -qx 'build: core (without .*)' <<<"$shipped"; then
    echo "build-core: the core image does not start \`ono\` as its entrypoint:" >&2
    echo "$shipped" >&2
    exit 1
  fi
  # PID 1 is `ono` itself, so what it reports about PID 1 is what the image runs as and where.
  answered="$("$runtime" run --rm --network=none "$DELIVERABLE_IMAGE" \
    -c 'get process | where pid == 1 | select name executable user cwd | to json')"
  if ! grep -q '"name":"ono","executable":"/usr/local/bin/ono","user":{"uid":1000,"name":"case"' <<<"$answered" \
      || ! grep -q '"cwd":"/home/case"' <<<"$answered"; then
    echo "build-core: the core image does not run \`ono\` as \`case\` in /home/case: $answered" >&2
    exit 1
  fi
  echo "build-core: the core image runs as shipped: ono as its entrypoint, as \`case\`"
fi
