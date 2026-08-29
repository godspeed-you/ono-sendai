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

# Three boxes of the checklist are claims about the work board rather than about the shell:
# section 4.5 Delivery, section 4.6.5 Delivery and section 4.7.2 "No release-blocking known
# defects remain" all assert that `docs/STATE.md` holds no claim and no unexplained deferral.
# Until ADR-0402 nothing read that file, so those boxes were true on the day they were written
# and unexamined afterwards. The gate does not run this: holding a claim mid-run is correct.
printf '\n\033[1m== the work board\033[0m\n'
if ! cargo run --quiet --package xtask -- state-check; then
  printf '\n\033[31mrelease-check: docs/STATE.md says the work is not finished\033[0m\n'
  exit 1
fi

printf '\n\033[1;32mrelease-check: the shell is release-ready\033[0m\n'
