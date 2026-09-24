#!/usr/bin/env bash
# Holds shipped binaries to the size budget of their target triple (issue #125, ADR-0866).
#
# The budgets are `build_budgets` in docs/contracts/hardening/limits.yaml. A row names a binary and,
# usually, a triple; a row without a triple covers the binary's triples that have no row of their
# own — the rule `cargo xtask binary-size` applies, and xtask/tests/metrics.rs holds the two
# readers to the same answer for every row. This script exists because the places that build what
# ships — scripts/package.sh on every release runner, scripts/build-core.sh — have no `xtask`, and
# compiling one there would compile the whole workspace a second time.
#
# For each binary it prints one line, and it fails when:
#   - the binary is over its budget;
#   - no row applies to the binary on this triple — a shipped triple nobody budgeted is not a
#     pass (ADR-0866);
#   - with --check-record: docs/baselines/binary-size.yaml records no figure for it, or one more
#     than one percent away from what was just built — the record is kept current where CI builds
#     what ships, and `cargo xtask metrics --write` after the build updates it (ADR-0867).
#
# usage: scripts/binary-size.sh --triple <triple> [--check-record] <binary>...
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

LIMITS="docs/contracts/hardening/limits.yaml"
RECORD="docs/baselines/binary-size.yaml"
# How far the record may sit from a fresh build before it is stale, in percent of the build. Two
# builds of one tree differ by the linker and the build image — 0.1 % measured — and a change to
# the code large enough to matter is well past one percent (ADR-0867).
RECORD_TOLERANCE_PERCENT=1

triple=""
check_record=0
binaries=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --triple) triple="${2:-}"; shift 2 ;;
    --check-record) check_record=1; shift ;;
    -*) echo "usage: scripts/binary-size.sh --triple <triple> [--check-record] <binary>..." >&2; exit 2 ;;
    *) binaries+=("$1"); shift ;;
  esac
done
if [[ -z "$triple" || ${#binaries[@]} -eq 0 ]]; then
  echo "usage: scripts/binary-size.sh --triple <triple> [--check-record] <binary>..." >&2
  exit 2
fi
if [[ ! -f "$LIMITS" ]]; then
  echo "binary-size: $LIMITS cannot be read, so there is nothing to hold the binaries to" >&2
  exit 1
fi

# The rows of `build_budgets`, one per line: `<key> <binary> <triple or -> <budget or ->`. The
# list's shape is fixed by the registry's own header; a row this cannot read has no budget.
budget_rows() {
  awk '
    function flush() { if (key != "") print key, binary, (triple == "" ? "-" : triple), (budget == "" ? "-" : budget) }
    /^build_budgets:/ { inside = 1; next }
    inside && /^[^ #]/ { flush(); key = ""; inside = 0 }
    !inside { next }
    /^  - key: / { flush(); key = $3; binary = ""; triple = ""; budget = ""; next }
    /^    binary: / { binary = $2; next }
    /^    triple: / { triple = $2; next }
    /^    budget: [0-9]+$/ { budget = $2; next }
    END { if (inside) flush() }
  ' "$LIMITS"
}

# The recorded figure of <binary> on <triple>, or nothing.
recorded() {
  [[ -f "$RECORD" ]] || return 0
  awk -v want_binary="$1" -v want_triple="$2" '
    /^stripped_bytes:/ { inside = 1; next }
    inside && /^[^ ]/ { inside = 0 }
    !inside { next }
    /^  [^ ].*:$/ { current = $1; sub(/:$/, "", current); next }
    /^    [^ ]+: [0-9]+$/ {
      found = $1; sub(/:$/, "", found)
      if (current == want_binary && found == want_triple) { print $2; exit }
    }
  ' "$RECORD"
}

rows="$(budget_rows)"
failed=0
for path in "${binaries[@]}"; do
  name="${path##*/}"
  if [[ ! -f "$path" ]]; then
    echo "binary-size: $path does not exist, so it cannot be measured" >&2
    failed=1
    continue
  fi
  size="$(stat --format=%s "$path")"
  row="$(awk -v b="$name" -v t="$triple" '$2 == b && $3 == t' <<<"$rows" | head -1)"
  if [[ -z "$row" ]]; then
    row="$(awk -v b="$name" '$2 == b && $3 == "-"' <<<"$rows" | head -1)"
  fi
  if [[ -z "$row" ]]; then
    echo "binary-size: $path ($triple) is $size bytes, and no row of \`build_budgets\` in $LIMITS applies to \`$name\` on $triple. A shipped triple nobody budgeted is not a pass (ADR-0866)" >&2
    failed=1
    continue
  fi
  read -r key _ _ budget <<<"$row"
  if [[ ! "$budget" =~ ^[0-9]+$ ]]; then
    echo "binary-size: \`$key\` in $LIMITS budgets \`$name\` without a whole number of bytes" >&2
    failed=1
    continue
  fi
  if (( size > budget )); then
    echo "binary-size: $path ($triple) is $size bytes, over its budget of $budget bytes by $(( size - budget )) — \`$key\` in $LIMITS. A binary this much larger needs the ADR that raises the budget (ADR-0863)" >&2
    failed=1
    continue
  fi
  echo "binary-size: $path ($triple) is $size bytes, within its budget of $budget bytes (\`$key\`)"

  if [[ $check_record -eq 1 ]]; then
    figure="$(recorded "$name" "$triple")"
    if [[ -z "$figure" ]]; then
      echo "binary-size: $RECORD records no figure for \`$name\` on $triple; \`cargo xtask metrics --write\` after this build records $size (ADR-0867)" >&2
      failed=1
    else
      difference=$(( size > figure ? size - figure : figure - size ))
      if (( difference * 100 > size * RECORD_TOLERANCE_PERCENT )); then
        echo "binary-size: $RECORD records $figure bytes for \`$name\` on $triple and this build is $size, more than $RECORD_TOLERANCE_PERCENT % apart. The record is stale: \`cargo xtask metrics --write\` after this build records it (ADR-0867)" >&2
        failed=1
      else
        echo "binary-size: $RECORD records $figure bytes for \`$name\` on $triple, within $RECORD_TOLERANCE_PERCENT % of this build"
      fi
    fi
  fi
done
exit "$failed"
