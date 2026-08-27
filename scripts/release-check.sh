#!/usr/bin/env bash
# The release gate of docs/ACCEPTANCE.md. An agent run ends when this passes - not before.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

printf '\033[1m== quality gate\033[0m\n'
scripts/gate.sh

printf '\n\033[1m== containerised acceptance\033[0m\n'
scripts/acceptance.sh

# The packages of the host architecture, built and installed in fresh containers
# (docs/ACCEPTANCE.md section 4.5, ADR-0121). The other architecture is proven the same way on a
# native runner in .github/workflows/release.yml (ADR-0123).
printf '\n\033[1m== installable packages\033[0m\n'
scripts/package.sh
scripts/package-check.sh

printf '\n\033[1m== release checklist\033[0m\n'
if grep -n '^- \[ \]' docs/ACCEPTANCE.md; then
  printf '\n\033[31mrelease-check: open items remain in docs/ACCEPTANCE.md\033[0m\n'
  exit 1
fi

printf '\n\033[1;32mrelease-check: the shell is release-ready\033[0m\n'
