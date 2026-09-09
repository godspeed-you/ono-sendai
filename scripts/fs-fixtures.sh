#!/usr/bin/env bash
# Regenerates the recorded real ZFS and Btrfs tool output the v0.6 storage providers parse
# against, in `crates/ono-recovery-{zfs,btrfs}/tests/fixtures/`.
#
# Spec v0.6 §54.4 asks for real ZFS and Btrfs where the environment permits, and Appendix G.2
# requires the providers to be shown misleading layouts and to refuse false coverage. A fixture
# somebody wrote by hand is a claim about what the tools print; these are a recording of what they
# actually printed, taken from a filesystem this script builds from nothing.
#
# It needs a container runtime, the kernel modules on the host, and `--privileged` to create a
# loop device. Nothing it does touches a filesystem outside the disposable image it creates.
#
# usage: scripts/fs-fixtures.sh [zfs|btrfs|all]
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

runtime=""
for candidate in docker podman; do
  if command -v "$candidate" >/dev/null 2>&1; then runtime="$candidate"; break; fi
done
if [[ -z "$runtime" ]]; then
  echo "fs-fixtures: no container runtime found; install docker or podman" >&2
  exit 127
fi

what="${1:-all}"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
image="${ONO_FS_FIXTURE_IMAGE:-ubuntu:26.04}"

run() {
  local name="$1" script="$2" out="$3"
  printf '\n\033[1m== %s fixtures\033[0m\n' "$name"
  mkdir -p "$out"
  "$runtime" run --rm --privileged \
    --volume /lib/modules:/lib/modules:ro \
    --volume /dev:/dev \
    --volume "$scratch":/pool \
    --volume "$PWD/$out":/out \
    --volume "$script":/gen.sh:ro \
    "$image" bash -c 'bash /gen.sh; chown -R '"$(id -u)":"$(id -g)"' /out'
}

if [[ "$what" == "zfs" || "$what" == "all" ]]; then
  run zfs "$PWD/scripts/fs-fixtures/zfs.sh" crates/ono-recovery-zfs/tests/fixtures
fi
if [[ "$what" == "btrfs" || "$what" == "all" ]]; then
  run btrfs "$PWD/scripts/fs-fixtures/btrfs.sh" crates/ono-recovery-btrfs/tests/fixtures
fi

printf '\n\033[1;32mfs-fixtures: recorded\033[0m\n'
