#!/usr/bin/env bash
# Builds the release container and runs every acceptance case against the real `ono` binary
# inside it. This is the referee for "does the shell actually work", as opposed to "do the unit
# tests pass" (docs/ACCEPTANCE.md).
#
# usage: scripts/acceptance.sh [--keep-image] [case-name ...]
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

IMAGE="${ONO_ACCEPTANCE_IMAGE:-ono-sendai:acceptance}"
CASE_DIR="docker/acceptance/cases"
KEEP_IMAGE=0
SELECTED=()

for arg in "$@"; do
  case "$arg" in
    --keep-image) KEEP_IMAGE=1 ;;
    *) SELECTED+=("$arg") ;;
  esac
done

runtime=""
for candidate in docker podman; do
  if command -v "$candidate" >/dev/null 2>&1; then runtime="$candidate"; break; fi
done
if [[ -z "$runtime" ]]; then
  echo "acceptance: neither docker nor podman is available" >&2
  exit 127
fi

printf '\n\033[1m== building %s with %s\033[0m\n' "$IMAGE" "$runtime"
if ! build_log="$("$runtime" build --file docker/Dockerfile --tag "$IMAGE" . 2>&1)"; then
  echo "$build_log" >&2
  echo "acceptance: the image did not build" >&2
  exit 1
fi

if [[ ${#SELECTED[@]} -gt 0 ]]; then
  cases=()
  for name in "${SELECTED[@]}"; do cases+=("$CASE_DIR/$name.case"); done
else
  mapfile -t cases < <(find "$CASE_DIR" -name '*.case' | sort)
fi

if [[ ${#cases[@]} -eq 0 ]]; then
  echo "acceptance: no cases found in $CASE_DIR" >&2
  exit 1
fi

passed=0
failed=0
failed_names=()

for file in "${cases[@]}"; do
  name="" ; run="" ; want_exit="0" ; want_match="" ; want_contains=""
  while IFS= read -r line; do
    case "$line" in
      \#*|"") continue ;;
      case:*)            name="${line#case:}" ;;
      run:*)             run="${line#run:}" ;;
      exit:*)            want_exit="${line#exit:}" ;;
      stdout-matches:*)  want_match="${line#stdout-matches:}" ;;
      stdout-contains:*) want_contains="${line#stdout-contains:}" ;;
      *) echo "acceptance: unknown directive in $file: $line" >&2; exit 1 ;;
    esac
  done < "$file"

  name="${name## }" ; run="${run## }" ; want_exit="${want_exit## }"
  want_match="${want_match## }" ; want_contains="${want_contains## }"

  set +e
  output="$("$runtime" run --rm --network=none "$IMAGE" bash -lc "$run" 2>&1)"
  code=$?
  set -e

  problem=""
  [[ "$code" != "$want_exit" ]] && problem="expected exit $want_exit, got $code"
  if [[ -z "$problem" && -n "$want_match" ]] && ! grep -Eq "$want_match" <<<"$output"; then
    problem="output does not match /$want_match/"
  fi
  if [[ -z "$problem" && -n "$want_contains" ]] && ! grep -Fq "$want_contains" <<<"$output"; then
    problem="output does not contain '$want_contains'"
  fi

  if [[ -z "$problem" ]]; then
    printf '  \033[32mpass\033[0m  %s\n' "$name"
    passed=$((passed + 1))
  else
    printf '  \033[31mFAIL\033[0m  %s\n        %s\n        command: %s\n        output:  %s\n' \
      "$name" "$problem" "$run" "${output//$'\n'/ | }"
    failed=$((failed + 1))
    failed_names+=("$name")
  fi
done

[[ $KEEP_IMAGE -eq 0 ]] && "$runtime" image rm --force "$IMAGE" >/dev/null 2>&1 || true

printf '\nacceptance: %d passed, %d failed\n' "$passed" "$failed"
if [[ $failed -gt 0 ]]; then
  printf 'failed cases: %s\n' "${failed_names[*]}"
  exit 1
fi
printf '\033[1;32macceptance: green\033[0m\n'
