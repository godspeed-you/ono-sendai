#!/usr/bin/env bash
# Record the README's GIFs against a real `ono` binary and render them.
#
# By default everything happens inside the demo container (docker/demo.Dockerfile): a clean
# Debian with the release binary, an unprivileged login shell and one ordinary web server on
# 8080 — so the frames show a machine anyone can rebuild, not a developer's laptop.
#
#   scripts/demo/make.sh                     # build the image, record and render every tape
#   scripts/demo/make.sh --local             # record against target/release/ono instead
#   scripts/demo/make.sh --no-build spatial  # re-render one tape against the existing image
#
# usage: scripts/demo/make.sh [--local] [--no-build] [--keep-casts] [tape-name ...]
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/../.."

IMAGE="${ONO_DEMO_IMAGE:-ono-sendai:demo-recording}"
BASE_IMAGE="${ONO_DEMO_BASE_IMAGE:-ono-sendai:demo}"
OUT_DIR="docs/assets"
CAST_DIR="${ONO_DEMO_CASTS:-$(mktemp -d)}"
LOCAL=0
BUILD=1
KEEP_CASTS=0
SELECTED=()

for arg in "$@"; do
  case "$arg" in
    --local)      LOCAL=1 ;;
    --no-build)   BUILD=0 ;;
    --keep-casts) KEEP_CASTS=1 ;;
    -h|--help)    sed -n '2,12p' "$0"; exit 0 ;;
    *)            SELECTED+=("$arg") ;;
  esac
done

command -v python3 >/dev/null || { echo "demo: python3 is required" >&2; exit 127; }
python3 -c 'import PIL' 2>/dev/null || { echo "demo: pillow is required (apt install python3-pil)" >&2; exit 127; }

if [[ $LOCAL -eq 0 ]]; then
  command -v docker >/dev/null || { echo "demo: docker is required, or pass --local" >&2; exit 127; }
  if [[ $BUILD -eq 1 ]]; then
    printf '\n\033[1m== building %s\033[0m\n' "$IMAGE"
    docker build --file docker/Dockerfile --tag "$BASE_IMAGE" .
    docker build --file docker/demo.Dockerfile --tag "$IMAGE" .
  fi
  RECORD_ARGS=(--exec "docker run --rm -i -t --hostname deck $IMAGE ono")
else
  [[ -x target/release/ono ]] || { echo "demo: cargo build --release -p ono-cli first" >&2; exit 1; }
  RECORD_ARGS=(--shell target/release/ono)
fi

tapes=()
if [[ ${#SELECTED[@]} -gt 0 ]]; then
  for name in "${SELECTED[@]}"; do tapes+=("scripts/demo/tapes/${name%.tape}.tape"); done
else
  while IFS= read -r found; do tapes+=("$found"); done < <(find scripts/demo/tapes -name '*.tape' | sort)
fi

mkdir -p "$OUT_DIR" "$CAST_DIR"
for tape in "${tapes[@]}"; do
  name="$(basename "$tape" .tape)"
  printf '\n\033[1m== %s\033[0m\n' "$name"
  python3 scripts/demo/record.py "$tape" -o "$CAST_DIR/$name.cast" "${RECORD_ARGS[@]}"
  title="$(sed -n 's/^title //p' "$tape" | head -1)"
  python3 scripts/demo/render.py "$CAST_DIR/$name.cast" -o "$OUT_DIR/$name.gif" --title "$title"
done

if [[ $KEEP_CASTS -eq 1 ]]; then
  echo "casts kept in $CAST_DIR"
elif [[ -z "${ONO_DEMO_CASTS:-}" ]]; then
  rm -rf "$CAST_DIR"
fi
