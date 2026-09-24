# ADR-0918: A cached package layer ages out weekly, and says where it came from

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.4.1 §44.1; ADR-0900, ADR-0903, ADR-0846; issue #139
- Decided by: agent (autonomous)

## Context

Two reviews of ADR-0903 found that three of its statements are wrong.

1. **"A cache here cannot make a build wrong."** The `filesystems-base` stage runs
   `apt-get install btrfs-progs zfsutils-linux …` without pinned versions. A layer cache hit
   reuses the package set the archive served when the layer was written. With the cache re-read
   on every push, CI keeps using that package set indefinitely: security updates and archive
   drift that a local build sees do not reach CI, and nothing reports it. It does not make one
   build inconsistent, but it can leave CI testing a userland that no longer exists.
2. **"Every branch reads the cache; only `main` and `implementation` write it."** GitHub's
   Actions cache, which the buildx `gha` backend also uses, restores entries written by the run's
   own ref, by the default branch (`main`), and for a pull request by its base branch. A push to
   `implementation-<slug>` does not read `implementation`'s entries, and `implementation`'s
   entries serve only `implementation` and pull requests based on it. ADR-0900 made the same
   claim for the gate caches ("a sub-branch run reads their caches"), and `ci.yml` repeated it in
   two comments.
3. The run gave no sign of whether the stage came from the cache: `acceptance.sh` threw away the
   base build's log on success.

## Decision

- **The filesystem stage declares `ARG ONO_ARCHIVE_WEEK` before it installs.**
  `scripts/acceptance.sh` passes the current ISO week (`date -u +%G-W%V`) to that stage's build
  and to the filesystem image built on it, locally and in CI alike. The argument is part of the
  install layer's cache key, so the layer is built again from the archive at least once a week.
  Within a week it may be reused. At most a week of archive drift is hidden, and the week is
  printed.
- **The run says where the stage came from.** After the base build it prints
  `filesystems-base (archive week …) came from the layer cache` or
  `… was installed from the archive`. It works this out from BuildKit's plain progress: whether
  the install step's `#N` is followed by `#N CACHED`.
- **The cache claims are corrected** here and in `ci.yml`'s comments. A run reads what its own
  ref and `main` wrote, plus the base's entries for a pull request. Only `main` and
  `implementation` write the gate and layer caches, so a sub-branch starts from `main`'s. That
  keeps the 10 GB budget as intended, but it does not share `implementation`'s warm caches.
  ADR-0903's and ADR-0900's sentences stand corrected by this record.

Pinning package versions was considered and rejected, because it would freeze the userland
deliberately rather than by accident.

## Consequences

- The first CI run of each week pays the three-minute download. The rest of the week is a cache
  hit, and the log shows which.
- Local builds also rebuild the stage weekly, where previously it was rebuilt only when the
  local BuildKit cache was pruned.
- `xtask/tests/harness.rs::should_rebuild_the_filesystem_stage_weekly_and_say_whether_it_came_from_the_cache`
  checks, against the stand-in runtime, the week argument on both builds, the report of a cache
  hit, and the `ARG` in the Dockerfile ahead of the install.
- Whether GitHub actually serves the `gha` layer entry to a given ref can only be seen in CI.

## Alternatives considered

- **Pin every package version.** Deterministic, but it needs a bump process and still hides
  security fixes until the next bump. The weekly key keeps the stage current.
- **A cache scope per week.** Same effect, but old scopes pile up until GitHub evicts them. A
  changed build argument invalidates within one scope.
- **Drop the layer cache.** That brings back the three-minute download on every run, which is
  what #139 set out to remove.
