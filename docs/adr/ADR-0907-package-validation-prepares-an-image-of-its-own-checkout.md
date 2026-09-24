# ADR-0907: Package validation prepares an image of its own checkout

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.4.1 §48.1; ADR-0531, ADR-0901
- Decided by: agent (autonomous)

## Context

`scripts/package-check.sh` prepares `fedora:latest` with util-linux under the fixed tag
`ono-package-check:fedora`, and removes it with `image rm --force` when it finishes unless
`--keep-image` was given. This is the defect ADR-0901 fixed for the acceptance images. When
package validation runs in two worktrees, the first to finish removes the image the other is
still installing its RPM in, so the other fails with a missing image.

## Decision

**ADR-0901's rule, applied to this image.** The tag is
`ono-package-check:fedora-<directory>-<12 hex of the SHA-256 of the checkout's path>`. Every run
holds a shared `flock` on a per-image lock file in `$XDG_RUNTIME_DIR` (or `$TMPDIR`, or `/tmp`)
from before its first container until it exits. At the end it removes the image only if it can
take that lock exclusively without waiting. Otherwise it keeps the image and says so.

## Consequences

`xtask/tests/harness.rs::should_give_package_validation_an_image_of_its_own_and_keep_one_another_run_uses`
runs the script against a stand-in container runtime. It checks that two checkouts prepare two
different tags, that each run removes only its own tag, and that a run which finishes while
another run in the same checkout is still mid-container keeps the image, leaving the last run to
remove it. The tag-derivation code is duplicated in `scripts/acceptance.sh` and here: three
lines, which both files attribute to ADR-0901.

## Alternatives considered

The same alternatives as ADR-0901 (a tag per run, checking whether a container is running, never
removing the image), rejected for the same reasons.
