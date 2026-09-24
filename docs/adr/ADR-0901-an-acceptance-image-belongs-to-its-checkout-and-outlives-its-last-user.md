# ADR-0901: An acceptance image belongs to its checkout, and is removed by its last user

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.4.1 §38 (a check that proves nothing must not read as a pass); AGENTS.md §10,
  §14; ADR-0846, ADR-0851; issue #185
- Decided by: agent (autonomous)

## Context

`scripts/acceptance.sh` named its image `ono-sendai:acceptance` unless `ONO_ACCEPTANCE_IMAGE`
said otherwise, and a run without `--keep-image` removed that image when it finished. With
phases running in parallel worktrees, that one name did two kinds of damage:

- **False red.** The first run to finish removed the image while the other was still running
  cases in it. The other run then reported each remaining case as exit 125 `Unable to find
  image`, sixteen spurious failures in one full run.
- **False green.** A run whose build finished before the other's ran its cases in the other
  worktree's image. H12 saw case 200 pass with the message from *before* its own change.

The filesystem image, `${IMAGE}-filesystems` (ADR-0846), was named from the same default and
had the same problem.

## Decision

**By default the image is named after the checkout, and a run removes it only if no other run
is still using it.**

1. **Name.** `ono-sendai:acceptance-<directory>-<digest>`. `<directory>` is the checkout's
   directory name, limited to tag characters and 48 characters, so a person can tell the images
   apart. `<digest>` is the first 12 hex digits of the SHA-256 of the checkout's absolute
   physical path, so two clones with the same directory name still get different images. The
   filesystem image is that name plus `-filesystems`. The same checkout gets the same name every
   time, so `--build-only` followed by `--no-build` still works, and the name still starts with
   `ono-sendai:acceptance`, which CI uses to find the images it packs. `ONO_ACCEPTANCE_IMAGE`
   still overrides it.
2. **Removal.** Every run holds a shared `flock` on a lock file for its image, in
   `$XDG_RUNTIME_DIR` (or `$TMPDIR`, or `/tmp`), from before it builds until it exits. At the end
   it removes the images only if it can turn that lock into an exclusive one without waiting,
   which means it is the last run using them. Otherwise it keeps them and says so. Two runs in
   one checkout, or two runs given the same `ONO_ACCEPTANCE_IMAGE`, share an image by
   definition, and the lock protects that case too.

## Consequences

- Runs in two worktrees build, run and remove their own images, and neither affects the other's
  result. `xtask/tests/harness.rs::should_build_and_remove_an_image_of_its_own_in_each_worktree`
  and `::should_leave_an_image_in_place_while_another_run_is_still_using_it` check this against
  a stand-in container runtime that records what it was asked to do.
- Every worktree leaves its own image behind when it uses `--keep-image`, so disk use grows with
  the number of worktrees. That is the cost of isolation. `docker image ls 'ono-sendai:acceptance*'`
  lists them, and each name contains the directory it came from.
- Two runs in one checkout still build to one tag. The later build replaces the tag, and the
  earlier run's remaining cases then use it. Both builds come from the same tree, so that is the
  same code unless someone edits the tree between the two builds. Handling that would need a tag
  per run, and then `--no-build` could not find the image an earlier `--build-only` built.
- The build cache is not affected. The Dockerfile's `target` cache mount is `sharing=locked`, so
  two builds cannot interleave inside it, and each build stamps its own sources before compiling
  (ADR-0251).

## Alternatives considered

- **A tag per run** (a random suffix). This isolates completely, but `--no-build` and CI's
  build-once, run-per-group flow (ADR-0851) need a name that can be computed again.
- **Refuse to remove an image any container is running.** Between two cases no container
  exists, so a concurrent run would be unprotected at exactly the moment the old code removed
  the image.
- **Never remove the image.** This moves the problem to disk space. Removing the image after a
  run is how a normal run avoids leaving an image behind.
