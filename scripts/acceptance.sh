#!/usr/bin/env bash
# Builds the release container and runs every acceptance case against the real `ono` binary
# inside it. This is the referee for "does the shell actually work", as opposed to "do the unit
# tests pass" (docs/ACCEPTANCE.md).
#
# usage: scripts/acceptance.sh [--keep-image] [--no-build] [name-fragment ...]
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

IMAGE="${ONO_ACCEPTANCE_IMAGE:-ono-sendai:acceptance}"
CASE_DIR="docker/acceptance/cases"
KEEP_IMAGE=0
NO_BUILD=0
SELECTED=()

for arg in "$@"; do
  case "$arg" in
    --keep-image) KEEP_IMAGE=1 ;;
    --no-build)   NO_BUILD=1; KEEP_IMAGE=1 ;;
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

if [[ $NO_BUILD -eq 0 ]]; then
  printf '\n\033[1m== building %s with %s\033[0m\n' "$IMAGE" "$runtime"
  if ! build_log="$("$runtime" build --file docker/Dockerfile --tag "$IMAGE" . 2>&1)"; then
    echo "$build_log" >&2
    echo "acceptance: the image did not build" >&2
    exit 1
  fi
fi

cases=()
if [[ ${#SELECTED[@]} -gt 0 ]]; then
  for fragment in "${SELECTED[@]}"; do
    while IFS= read -r found; do cases+=("$found"); done \
      < <(find "$CASE_DIR" -name "*${fragment}*.case" | sort)
  done
else
  while IFS= read -r found; do cases+=("$found"); done \
    < <(find "$CASE_DIR" -name '*.case' | sort)
fi

if [[ ${#cases[@]} -eq 0 ]]; then
  echo "acceptance: no cases found in $CASE_DIR" >&2
  exit 1
fi

# --- case file parsing ---------------------------------------------------------------------
#
# A case is a flat key/value file. A value is either the rest of the line, or a bare `|`
# followed by lines indented two spaces, so a case can carry a whole script without inventing
# quoting rules of its own. Assertion keys are repeatable; every one of them must hold.
# The full directive list lives in docker/README.md.

declare -a assert_kind assert_arg want_env

parse_case() {
  local file="$1" key value collecting="" line
  name=""; run=""; stdin_text=""; want_exit="0"; want_pty="0"; want_timeout="30"
  want_cols=""; want_lines=""
  assert_kind=(); assert_arg=(); want_env=()

  while IFS= read -r line || [[ -n "$line" ]]; do
    if [[ -n "$collecting" ]]; then
      if [[ -z "${line//[[:space:]]/}" ]]; then
        printf -v "$collecting" '%s\n' "${!collecting}"
        continue
      fi
      if [[ "$line" == "  "* ]]; then
        printf -v "$collecting" '%s%s\n' "${!collecting}" "${line:2}"
        continue
      fi
      collecting=""
    fi

    [[ -z "$line" || "$line" == \#* ]] && continue

    if [[ "$line" != *:* ]]; then
      echo "acceptance: unparsable line in $file: $line" >&2
      exit 1
    fi
    key="${line%%:*}"
    value="${line#*:}"
    value="${value# }"

    if [[ "$value" == "|" ]]; then
      case "$key" in
        run)   collecting=run;        run="" ;;
        stdin) collecting=stdin_text; stdin_text="" ;;
        *)
          echo "acceptance: block values are only valid for run and stdin, not $key (in $file)" >&2
          exit 1 ;;
      esac
      continue
    fi

    case "$key" in
      case)                 name="$value" ;;
      run)                  run="$value" ;;
      stdin)                stdin_text="$value" ;;
      exit)                 want_exit="$value" ;;
      pty)                  want_pty="$value" ;;
      timeout)              want_timeout="$value" ;;
      columns)              want_cols="$value" ;;
      lines)                want_lines="$value" ;;
      env)                  want_env+=("$value") ;;
      stdout-matches)       assert_kind+=(matches);      assert_arg+=("$value") ;;
      stdout-not-matches)   assert_kind+=(not-matches);  assert_arg+=("$value") ;;
      stdout-contains)      assert_kind+=(contains);     assert_arg+=("$value") ;;
      stdout-not-contains)  assert_kind+=(not-contains); assert_arg+=("$value") ;;
      stdout-equals)        assert_kind+=(equals);       assert_arg+=("$value") ;;
      *) echo "acceptance: unknown directive in $file: $key" >&2; exit 1 ;;
    esac
  done < "$file"

  if [[ -z "$name" ]]; then
    echo "acceptance: $file has no case line" >&2
    exit 1
  fi
  if [[ -z "${run//[[:space:]]/}" ]]; then
    echo "acceptance: $file has no run script" >&2
    exit 1
  fi
}

# --- assertions ------------------------------------------------------------------------------

check_assertions() {
  local output="$1" index kind arg
  for index in "${!assert_kind[@]}"; do
    kind="${assert_kind[$index]}"
    arg="${assert_arg[$index]}"
    case "$kind" in
      matches)
        if ! grep -Eq -- "$arg" <<<"$output"; then
          printf 'output does not match /%s/' "$arg"; return
        fi ;;
      not-matches)
        if grep -Eq -- "$arg" <<<"$output"; then
          printf 'output unexpectedly matches /%s/' "$arg"; return
        fi ;;
      contains)
        if ! grep -Fq -- "$arg" <<<"$output"; then
          printf "output does not contain '%s'" "$arg"; return
        fi ;;
      not-contains)
        if grep -Fq -- "$arg" <<<"$output"; then
          printf "output unexpectedly contains '%s'" "$arg"; return
        fi ;;
      equals)
        if [[ "$(tr -d '\r' <<<"$output")" != "$arg" ]]; then
          printf "output is not exactly '%s'" "$arg"; return
        fi ;;
    esac
  done
}

# --- running ---------------------------------------------------------------------------------

passed=0
failed=0
failed_names=()

for file in "${cases[@]}"; do
  parse_case "$file"

  runtime_args=(run --rm --interactive --network=none)
  for pair in "${want_env[@]}"; do runtime_args+=(--env "$pair"); done
  if [[ -n "$want_cols" ]];  then runtime_args+=(--env "COLUMNS=$want_cols"); fi
  if [[ -n "$want_lines" ]]; then runtime_args+=(--env "LINES=$want_lines"); fi

  if [[ "$want_pty" == "true" || "$want_pty" == "1" ]]; then
    # `script` gives the command a real controlling terminal, which is the only way to prove
    # the PTY behaviour of spec section 29.3 rather than to assume it.
    script_body="stty rows ${want_lines:-24} cols ${want_cols:-80} 2>/dev/null
$run"
    runtime_args+=(--env "ONO_CASE_SCRIPT=$script_body")
    inner=(bash -lc 'script --quiet --return --command "eval \"\$ONO_CASE_SCRIPT\"" /dev/null')
  else
    runtime_args+=(--env "ONO_CASE_SCRIPT=$run")
    inner=(bash -lc 'eval "$ONO_CASE_SCRIPT"')
  fi

  set +e
  output="$(printf '%s' "$stdin_text" \
    | timeout --kill-after=5 "$want_timeout" "$runtime" "${runtime_args[@]}" "$IMAGE" "${inner[@]}" 2>&1)"
  code=$?
  set -e

  problem=""
  if [[ $code -eq 124 || $code -eq 137 ]]; then
    problem="timed out after ${want_timeout}s"
  elif [[ "$code" != "$want_exit" ]]; then
    problem="expected exit $want_exit, got $code"
  else
    problem="$(check_assertions "$output")"
  fi

  if [[ -z "$problem" ]]; then
    printf '  \033[32mpass\033[0m  %s\n' "$name"
    passed=$((passed + 1))
  else
    printf '  \033[31mFAIL\033[0m  %s\n        %s\n        case:    %s\n        output:  %s\n' \
      "$name" "$problem" "$file" "${output//$'\n'/ | }"
    failed=$((failed + 1))
    failed_names+=("$name")
  fi
done

if [[ $KEEP_IMAGE -eq 0 ]]; then
  "$runtime" image rm --force "$IMAGE" >/dev/null 2>&1 || true
fi

printf '\nacceptance: %d passed, %d failed\n' "$passed" "$failed"
if [[ $failed -gt 0 ]]; then
  printf 'failed cases: %s\n' "${failed_names[*]}"
  exit 1
fi
printf '\033[1;32macceptance: green\033[0m\n'
