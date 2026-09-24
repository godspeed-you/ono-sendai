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
#     pass (ADR-0866).
#
# usage: scripts/binary-size.sh --triple <triple> <binary>...
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

LIMITS="docs/contracts/hardening/limits.yaml"

triple=""
binaries=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --triple) triple="${2:-}"; shift 2 ;;
    -*) echo "usage: scripts/binary-size.sh --triple <triple> <binary>..." >&2; exit 2 ;;
    *) binaries+=("$1"); shift ;;
  esac
done
if [[ -z "$triple" || ${#binaries[@]} -eq 0 ]]; then
  echo "usage: scripts/binary-size.sh --triple <triple> <binary>..." >&2
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

done
exit "$failed"
