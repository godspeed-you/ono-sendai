#!/usr/bin/env bash
# Builds the release container and runs every acceptance case against the real `ono` binary
# inside it. This is the referee for "does the shell actually work", as opposed to "do the unit
# tests pass" (docs/ACCEPTANCE.md).
#
# usage: scripts/acceptance.sh [--keep-image] [--no-build] [--group NAME]... [--fail-fast]
#                              [name-fragment ...]
#        scripts/acceptance.sh --build-only
#        scripts/acceptance.sh --list-groups
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

IMAGE="${ONO_ACCEPTANCE_IMAGE:-ono-sendai:acceptance}"
CASE_DIR="docker/acceptance/cases"
GROUPS_FILE="docker/acceptance/groups"
KEEP_IMAGE=0
NO_BUILD=0
BUILD_ONLY=0
LIST_GROUPS=0
FAIL_FAST=0
SELECTED=()
SELECTED_GROUPS=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --keep-image)  KEEP_IMAGE=1 ;;
    --no-build)    NO_BUILD=1; KEEP_IMAGE=1 ;;
    # CI builds the images once and hands them to one job per group (ADR-0851).
    --build-only)  BUILD_ONLY=1; KEEP_IMAGE=1 ;;
    --list-groups) LIST_GROUPS=1 ;;
    --fail-fast)   FAIL_FAST=1 ;;
    --group)
      if [[ $# -lt 2 ]]; then
        echo "acceptance: --group needs the name of a group in $GROUPS_FILE" >&2
        exit 1
      fi
      SELECTED_GROUPS+=("$2")
      shift ;;
    *) SELECTED+=("$1") ;;
  esac
  shift
done

if [[ $BUILD_ONLY -eq 1 && ( $NO_BUILD -eq 1 || ${#SELECTED[@]} -gt 0 || ${#SELECTED_GROUPS[@]} -gt 0 ) ]]; then
  echo "acceptance: --build-only builds the images the whole suite needs, and takes no selection" >&2
  exit 1
fi

# --- groups ----------------------------------------------------------------------------------
#
# Every case belongs to exactly one group of $GROUPS_FILE, by the number its file name starts
# with. The check runs on every invocation, whatever is selected: a case outside every range
# would otherwise drop out of the CI job that should run it without anyone noticing.

group_names=(); group_first=(); group_last=()
while IFS= read -r line; do
  read -r group first last rest <<<"$line"
  [[ -z "$group" || "$group" == \#* ]] && continue
  if [[ -n "$rest" || ! "$first" =~ ^[0-9]+$ || ! "$last" =~ ^[0-9]+$ ]] || (( 10#$first > 10#$last )); then
    echo "acceptance: $GROUPS_FILE: \`$line\` is not a \`<name> <first> <last>\` line" >&2
    exit 1
  fi
  if [[ " ${group_names[*]} " == *" $group "* ]]; then
    echo "acceptance: $GROUPS_FILE names the group \`$group\` twice" >&2
    exit 1
  fi
  group_names+=("$group"); group_first+=($((10#$first))); group_last+=($((10#$last)))
done < "$GROUPS_FILE"

# Sets `owners` to the indices of the groups whose range holds the case file's number.
case_groups() {
  local stem="${1##*/}" number index
  number="${stem%%-*}"
  owners=()
  [[ "$number" =~ ^[0-9]+$ ]] || return 0
  for index in "${!group_names[@]}"; do
    if (( 10#$number >= group_first[index] && 10#$number <= group_last[index] )); then
      owners+=("$index")
    fi
  done
}

all_cases=()
while IFS= read -r found; do all_cases+=("$found"); done < <(find "$CASE_DIR" -name '*.case' | sort)

group_problem=""
for file in "${all_cases[@]}"; do
  case_groups "$file"
  if [[ ${#owners[@]} -eq 0 ]]; then
    group_problem+="  $file is in no group"$'\n'
  elif [[ ${#owners[@]} -gt 1 ]]; then
    group_problem+="  $file is in more than one group"$'\n'
  fi
done
if [[ -n "$group_problem" ]]; then
  printf 'acceptance: every case belongs to exactly one group of %s (ADR-0851):\n%s' \
    "$GROUPS_FILE" "$group_problem" >&2
  exit 1
fi

if [[ $LIST_GROUPS -eq 1 ]]; then
  printf '%s\n' "${group_names[@]}"
  exit 0
fi

for group in "${SELECTED_GROUPS[@]}"; do
  if [[ " ${group_names[*]} " != *" $group "* ]]; then
    echo "acceptance: there is no group \`$group\`; $GROUPS_FILE has: ${group_names[*]}" >&2
    exit 1
  fi
done

runtime=""
for candidate in docker podman; do
  if command -v "$candidate" >/dev/null 2>&1; then runtime="$candidate"; break; fi
done
if [[ -z "$runtime" ]]; then
  echo "acceptance: neither docker nor podman is available" >&2
  exit 127
fi

cases=()
if [[ ${#SELECTED[@]} -gt 0 ]]; then
  for fragment in "${SELECTED[@]}"; do
    while IFS= read -r found; do cases+=("$found"); done \
      < <(find "$CASE_DIR" -name "*${fragment}*.case" | sort)
  done
else
  cases=("${all_cases[@]}")
fi

if [[ ${#SELECTED_GROUPS[@]} -gt 0 ]]; then
  in_groups=()
  for file in "${cases[@]}"; do
    case_groups "$file"
    if [[ " ${SELECTED_GROUPS[*]} " == *" ${group_names[${owners[0]}]} "* ]]; then
      in_groups+=("$file")
    fi
  done
  cases=("${in_groups[@]}")
fi

if [[ ${#cases[@]} -eq 0 ]]; then
  echo "acceptance: no cases found in $CASE_DIR" >&2
  exit 1
fi

# The real-filesystem cases run in the stage whose userland the recovery providers validate
# (ADR-0846). It is built only when a selected case asks for it.
FS_IMAGE="${IMAGE}-filesystems"
fs_wanted=0
if [[ $NO_BUILD -eq 0 ]] && grep -qx 'image: filesystems' "${cases[@]}"; then
  fs_wanted=1
fi
fs_built=0

if [[ $NO_BUILD -eq 0 ]]; then
  # The filesystem stage's packages need nothing from the builder, and on a CI runner they are a
  # download of three minutes or more from the Ubuntu archive. So `filesystems-base` is built in
  # the background while the main image compiles, and the filesystem image below finds it ready.
  base_pid=""
  base_log=""
  if [[ $fs_wanted -eq 1 ]]; then
    base_log="$(mktemp)"
    "$runtime" build --file docker/Dockerfile --target filesystems-base . >"$base_log" 2>&1 &
    base_pid=$!
  fi
  printf '\n\033[1m== building %s with %s\033[0m\n' "$IMAGE" "$runtime"
  if ! build_log="$("$runtime" build --file docker/Dockerfile --tag "$IMAGE" . 2>&1)"; then
    echo "$build_log" >&2
    echo "acceptance: the image did not build" >&2
    if [[ -n "$base_pid" ]]; then
      wait "$base_pid" || true
      rm -f "$base_log"
    fi
    exit 1
  fi
  if [[ -n "$base_pid" ]]; then
    if ! wait "$base_pid"; then
      cat "$base_log" >&2
      rm -f "$base_log"
      echo "acceptance: the filesystem image's packages did not install" >&2
      exit 1
    fi
    rm -f "$base_log"
  fi
fi

if [[ $fs_wanted -eq 1 ]]; then
  printf '\n\033[1m== building %s with %s\033[0m\n' "$FS_IMAGE" "$runtime"
  if ! build_log="$("$runtime" build --file docker/Dockerfile --target runtime-filesystems \
      --tag "$FS_IMAGE" . 2>&1)"; then
    echo "$build_log" >&2
    echo "acceptance: the filesystem image did not build" >&2
    exit 1
  fi
  fs_built=1
fi

if [[ $BUILD_ONLY -eq 1 ]]; then
  printf '\nacceptance: built %s' "$IMAGE"
  if [[ $fs_built -eq 1 ]]; then printf ' and %s' "$FS_IMAGE"; fi
  printf '\n'
  exit 0
fi

# The cases with a short declared budget run first. On the CI run this ordering was measured
# against, every case declaring 60 seconds or less finished within six, and every case that took
# half a minute or more declared 90 or more — so a red quick case shows in the first minutes of
# a run rather than after the slow ones, and `--fail-fast` stops there (ADR-0851).
ordered=()
while IFS=$'\t' read -r _ found; do ordered+=("$found"); done < <(
  for file in "${cases[@]}"; do
    printf '%s\t%s\n' "$(awk '/^timeout:/ { budget = $2 } END { print budget ? budget : 30 }' "$file")" "$file"
  done | sort -t $'\t' -k1,1n -k2,2 -u
)
cases=("${ordered[@]}")

# --- case file parsing ---------------------------------------------------------------------
#
# A case is a flat key/value file. A value is either the rest of the line, or a bare `|`
# followed by lines indented two spaces, so a case can carry a whole script without inventing
# quoting rules of its own. Assertion keys are repeatable; every one of them must hold.
# The full directive list lives in docker/README.md.

declare -a assert_kind assert_arg want_env want_caps want_security

parse_case() {
  local file="$1" key value collecting="" line
  name=""; run=""; stdin_text=""; want_exit="0"; want_pty="0"; want_timeout="30"
  want_cols=""; want_lines=""; want_user=""; want_privileged=""; want_image=""
  assert_kind=(); assert_arg=(); want_env=(); want_caps=(); want_security=()

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
      # A case that proves a privileged path says so itself, so the privilege is visible in the
      # case and nowhere else; every other case stays unprivileged (docs/ACCEPTANCE.md, ADR-0237).
      capability)           want_caps+=("$value") ;;
      user)                 want_user="$value" ;;
      # A case that builds its own disposable loop filesystem (v0.6 Appendix G.3) needs the
      # host's loop devices, which only a privileged container is given. The case says so
      # itself, like `capability`, and no other case runs privileged (ADR-0843).
      privileged)           want_privileged="$value" ;;
      # A case whose filesystem needs a userland the main image does not carry names the image it
      # runs in; `filesystems` is the only one (ADR-0846).
      image)                want_image="$value" ;;
      security)             want_security+=("$value") ;;
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

# --- announced skips -------------------------------------------------------------------------
#
# A case that cannot observe what it is about in this environment says so with v0.4.1 §38.1's
# marker, `SKIPPED <name>: <category>: <detail>`, and carries on to its closing line. Read on its
# own, such a case passes — which is exactly how a check that proves nothing ends up ticking a box
# in docs/ACCEPTANCE.md. So a skip is held to the same registry `cargo xtask skip-check` holds the
# cargo tests to (§38.2, §38.3), in both directions:
#
#   - a marker the registry's `acceptance.expected_skips` does not list, with that category, fails
#     the case: a skip nobody declared is a skip nobody decided to permit;
#   - a listed skip that did not happen fails the case too: the environment started supplying the
#     prerequisite, or the case stopped checking for it, and either way the row is now wrong.
#
# The image is the one environment the suite runs in, so there is no "permitted either way" list:
# every skip it takes is expected, and every expected skip is taken. A row's id is
# `<case file without .case>::<name the marker announces>`.

SKIP_REGISTRY="docs/contracts/hardening/expected_test_skips.yaml"
SKIP_CATEGORIES="missing_kernel_feature missing_privilege unsupported_arch unsupported_distribution external_tool_unavailable fixture_not_applicable"

# Every `id<TAB>category` row of the registry's `acceptance:` section.
declared_acceptance_skips() {
  awk '
    /^[^[:space:]#]/ { section = ($0 ~ /^acceptance:/); id = ""; next }
    !section { next }
    /^[[:space:]]*- id:/ {
      id = $0; sub(/^[^"]*"/, "", id); sub(/".*$/, "", id); next
    }
    /^[[:space:]]*category:/ && id != "" {
      category = $0; sub(/^[^:]*:[[:space:]]*/, "", category); sub(/[[:space:]]*(#.*)?$/, "", category)
      print id "\t" category; id = ""
    }
  ' "$SKIP_REGISTRY"
}

if [[ ! -f "$SKIP_REGISTRY" ]] || ! grep -q '^acceptance:' "$SKIP_REGISTRY"; then
  echo "acceptance: $SKIP_REGISTRY has no \`acceptance:\` section to hold announced skips to" >&2
  exit 1
fi
declared_rows="$(declared_acceptance_skips)"
registry_problem=""
while IFS=$'\t' read -r row_id row_category; do
  [[ -z "$row_id" ]] && continue
  if [[ " $SKIP_CATEGORIES " != *" $row_category "* ]]; then
    registry_problem+="  $row_id declares \`$row_category\`, which is not one of v0.4.1 §38.4's six categories"$'\n'
  fi
  if [[ "$row_id" != *::* || ! -f "$CASE_DIR/${row_id%%::*}.case" ]]; then
    registry_problem+="  $row_id names no case file in $CASE_DIR, so it permits a skip nothing can take"$'\n'
  fi
done <<<"$declared_rows"
if [[ -n "$registry_problem" ]]; then
  printf 'acceptance: the acceptance rows of %s are malformed:\n%s' "$SKIP_REGISTRY" "$registry_problem" >&2
  exit 1
fi

# The skip problems of one case's output, or nothing when its skips are exactly the declared ones.
check_skips() {
  local stem="$1" output="$2" line rest test category key observed="" problems="" row_id row_category
  while IFS= read -r line; do
    line="${line%$'\r'}"
    [[ "$line" == "SKIPPED "* ]] || continue
    rest="${line#SKIPPED }"
    test=""; category=""
    if [[ "$rest" == *": "*":"* ]]; then
      test="${rest%%: *}"
      rest="${rest#*: }"
      category="${rest%%:*}"
    fi
    if [[ -z "$test" || " $SKIP_CATEGORIES " != *" $category "* ]]; then
      problems+="a SKIPPED line that is not \`SKIPPED <name>: <category>: <detail>\` with a §38.4 category: '$line'; "
      continue
    fi
    key="$stem::$test"
    observed+="$key"$'\t'"$category"$'\n'
    if ! grep -qxF -- "$key"$'\t'"$category" <<<"$declared_rows"; then
      problems+="skipped \`$key\` as \`$category\`, and $SKIP_REGISTRY does not declare that skip (v0.4.1 §38.2); "
    fi
  done <<<"$output"
  while IFS=$'\t' read -r row_id row_category; do
    [[ "${row_id%%::*}" == "$stem" ]] || continue
    if ! grep -qxF -- "$row_id"$'\t'"$row_category" <<<"$observed"; then
      problems+="\`$row_id\` is declared to skip as \`$row_category\` and did not — delete the row or restore the skip (v0.4.1 §38.3); "
    fi
  done <<<"$declared_rows"
  printf '%s' "$problems"
}

# --- running ---------------------------------------------------------------------------------

# Wall-clock seconds between two $EPOCHREALTIME readings, to a tenth. It is shown, never judged:
# a case that must be fast says so with its own budget. The radix is the locale's, so either.
seconds_between() {
  local from="${1/[.,]/}" to="${2/[.,]/}" tenths
  tenths=$(( (10#$to - 10#$from) / 100000 ))
  printf '%d.%ds' $((tenths / 10)) $((tenths % 10))
}

passed=0
failed=0
failed_names=()
not_run=0

for index in "${!cases[@]}"; do
  file="${cases[$index]}"
  parse_case "$file"

  case "$want_image" in
    "")          image="$IMAGE" ;;
    filesystems) image="$FS_IMAGE" ;;
    *)
      echo "acceptance: $file names image \`$want_image\`; the only one is \`filesystems\` (ADR-0846)" >&2
      exit 1 ;;
  esac

  runtime_args=(run --rm --interactive --network=none)
  for pair in "${want_env[@]}"; do runtime_args+=(--env "$pair"); done
  for capability in "${want_caps[@]}"; do runtime_args+=(--cap-add "$capability"); done
  for option in "${want_security[@]}"; do runtime_args+=(--security-opt "$option"); done
  if [[ -n "$want_user" ]]; then runtime_args+=(--user "$want_user"); fi
  if [[ "$want_privileged" == "true" ]]; then runtime_args+=(--privileged --volume /dev:/dev); fi
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

  started="$EPOCHREALTIME"
  set +e
  output="$(printf '%s' "$stdin_text" \
    | timeout --kill-after=5 "$want_timeout" "$runtime" "${runtime_args[@]}" "$image" "${inner[@]}" 2>&1)"
  code=$?
  set -e
  elapsed="$(seconds_between "$started" "$EPOCHREALTIME")"

  problem=""
  if [[ $code -eq 124 || $code -eq 137 ]]; then
    problem="timed out after ${want_timeout}s"
  elif [[ "$code" != "$want_exit" ]]; then
    problem="expected exit $want_exit, got $code"
  else
    problem="$(check_assertions "$output")"
    if [[ -z "$problem" ]]; then
      problem="$(check_skips "$(basename "$file" .case)" "$output")"
    fi
  fi

  if [[ -z "$problem" ]]; then
    printf '  \033[32mpass\033[0m %7s  %s\n' "$elapsed" "$name"
    # A declared skip is still a check that did not run, so it is shown beside the pass.
    if grep -q '^SKIPPED ' <<<"$output"; then
      grep '^SKIPPED ' <<<"$output" | sed 's/^/        declared: /'
    fi
    passed=$((passed + 1))
  else
    printf '  \033[31mFAIL\033[0m %7s  %s\n        %s\n        case:    %s\n        output:  %s\n' \
      "$elapsed" "$name" "$problem" "$file" "${output//$'\n'/ | }"
    failed=$((failed + 1))
    failed_names+=("$name")
    if [[ $FAIL_FAST -eq 1 ]]; then
      not_run=$(( ${#cases[@]} - index - 1 ))
      break
    fi
  fi
done

if [[ $KEEP_IMAGE -eq 0 ]]; then
  "$runtime" image rm --force "$IMAGE" >/dev/null 2>&1 || true
  if [[ $fs_built -eq 1 ]]; then
    "$runtime" image rm --force "$FS_IMAGE" >/dev/null 2>&1 || true
  fi
fi

printf '\nacceptance: %d passed, %d failed' "$passed" "$failed"
if [[ $not_run -gt 0 ]]; then printf ', %d not run (--fail-fast)' "$not_run"; fi
printf '\n'
if [[ $failed -gt 0 ]]; then
  printf 'failed cases: %s\n' "${failed_names[*]}"
  exit 1
fi
printf '\033[1;32macceptance: green\033[0m\n'
