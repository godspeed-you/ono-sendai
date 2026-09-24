# ADR-0919: The image lock lives where every run of the checkout agrees

- Status: accepted
- Date: 2026-09-24
- Spec refs: ADR-0901, ADR-0907, ADR-0913; issue #185
- Decided by: agent (autonomous)

## Context

ADR-0901 made runs of one checkout share a lock, so that a run removes its images only when it
can take the lock exclusively. A review found four gaps:

- The lock was a file per image tag in `$XDG_RUNTIME_DIR`, which differs per user. Image tags
  belong to the container daemon, which all users share. A `sudo` run and a user run of one
  checkout built the same tag with two different locks, so either could remove the image while
  the other was using it. The fallback, a shared `/tmp`, had its own problems: planted symlinks,
  and `fs.protected_regular` refusing an `O_CREAT` open of another user's file.
- The background `build … &` of the filesystem stage inherited the lock descriptor. If that
  child outlived the run, it kept holding the images against removal.
- The background build's `mktemp` log had no cleanup when the run was stopped.
- `--build-only` builds images for the `--no-build` runs that come after it. A full run of the
  same checkout that finished in between removed them. The core profile locked on `$IMAGE`
  while building and removing `$CORE_IMAGE`.

## Decision

1. **One lock per checkout, shared by every user.** `scripts/image-lock.sh` (used by
   `acceptance.sh` and `package-check.sh`) picks the location:
   - When the checkout is the top of a git repository, the lock is
     `<git common dir>/ono-<tool>.lock`. The owner, root and every worktree of the repository
     all reach that directory.
   - Otherwise, the lock is `<checkout>/.ono-<tool>.lock`.
   - It is never in a per-user runtime directory, and never in `/tmp`.

   The file is created once with `noclobber` (exclusive, not through a planted symlink). Every
   run opens it read-only, because `flock` does not need write access, so a lock file root
   created can still be locked by the owner. A path that is not a regular file is refused.
   Because the lock covers every image of the checkout, the core-profile mismatch goes away.
2. **The background build does not get the lock.** Its descriptor is closed for that child only
   (`{image_lock}<&-`).
3. **Cleanup on exit.** An `EXIT` trap removes the background build's log, and `TERM` and `INT`
   end the run through that trap.
4. **`--build-only` pins the images.** It writes `<lock>.pinned`. No later run removes the
   images while the pin exists; a run that keeps them says so and names the file to delete.
   `--no-build` never removes images; that was already the case, because it implies
   `--keep-image`.

## Consequences

- Tests (`xtask/tests/harness.rs`):
  - `should_leave_an_image_in_place_while_another_run_is_still_using_it`: the second run now has
    another user's runtime directory and `TMPDIR`.
  - `should_keep_the_image_lock_in_the_repository_every_run_of_the_checkout_shares`: covers the
    git path and the plain-checkout fallback.
  - `should_keep_an_image_built_for_later_runs_until_it_is_released`
  - `should_hand_the_background_build_no_copy_of_the_image_lock`
  - `should_leave_no_build_log_behind_when_a_run_is_stopped`
- A pinned image stays until someone deletes the pin file and a later run removes the image, or
  until someone runs `docker image rm`. That is the price of `--build-only` followed by
  `--no-build`, which is a flow for a developer driving the runs by hand; CI runs those two
  steps on separate runners.
- One lock per checkout rather than per tag is more cautious: two runs of one checkout with
  different explicit tags now keep each other's images until the last one finishes.
- A stopped run's cleanup happens only after the foreground command returns, because bash
  defers traps until then. Stopping the run during a long build therefore cleans up when that
  build ends.

## Alternatives considered

- **A 1777 directory under `/tmp`.** It is shared by everyone, but it needs defences against
  planted symlinks and `protected_regular`, and any user could hold the lock to prevent a
  removal.
- **A tag per run.** Rejected in ADR-0901, and still incompatible with `--no-build`.
