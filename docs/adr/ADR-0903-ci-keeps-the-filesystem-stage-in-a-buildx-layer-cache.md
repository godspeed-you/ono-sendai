# ADR-0903: CI keeps the filesystem stage in a buildx layer cache

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.4.1 §43.3, §43.4, §44.1; ADR-0251, ADR-0846, ADR-0851, ADR-0852; issue #139
- Decided by: agent (autonomous)

## Context

The `acceptance image` CI job builds two images. The filesystem one (ADR-0846) starts from the
stage `filesystems-base`, which is `ubuntu:26.04` plus `apt-get install btrfs-progs
zfsutils-linux …` from `archive.ubuntu.com`. Two diagnostic runs timed that stage at 3m38. Most
of it (212 s) was the download: 27 s before the first response, then about three minutes for 50
packages. Since 08e89270 the download runs in parallel with the Rust build, so the job pays only
the part that outlasts the builder. It still runs on every CI run, and how long it takes depends
on the archive. The job's Actions caches hold cargo's registry and target directory
(buildkit-cache-dance). No layer is cached.

## Decision

**The `filesystems-base` stage is read from and written to buildx's GitHub Actions cache backend
(`type=gha`) in its own scope. Every branch reads it, only `main` and `implementation` write it,
and a run that does not ask for it builds exactly as before.**

- `scripts/acceptance.sh` reads `ONO_ACCEPTANCE_LAYER_CACHE`:
  - `gha` builds through `docker buildx build`, adds
    `--cache-from type=gha,scope=ono-acceptance-filesystems-base` and
    `--cache-to type=gha,mode=max,scope=ono-acceptance-filesystems-base,ignore-error=true` to
    the `filesystems-base` build, and adds `--load` to the two tagged builds.
  - `gha-read` does the same without `--cache-to`.
  - Unset keeps the previous commands. Any other value is refused.
- The acceptance image job sets up a `docker-container` builder
  (`docker/setup-buildx-action`, BuildKit pinned by digest, §44.1) *before* buildkit-cache-dance,
  so the cargo cache mounts are injected into the builder that builds.
  `crazy-max/ghaction-github-runtime` puts the Actions cache service's token and URLs into the
  environment of the build step, where `docker buildx` reads them. Both actions are pinned by
  commit.
- The build step sets `gha` on `main` and `implementation` and `gha-read` everywhere else, the
  same policy as the gate's `save-if`.

### Why a cache here cannot make a build wrong

- A layer cache hit means the same instruction on the same parent layers. The parent is
  `ubuntu:26.04@sha256:…`, pinned by digest, and the instruction is the `RUN apt-get …` text. If
  either changes, the key changes and the stage is built from scratch. What a hit reuses is the
  package set as the archive served it when the layer was written. That is the purpose of the
  cache, and it is what any Docker layer cache on a workstation already does.
- A miss builds the stage. `--cache-from` for a scope with nothing in it is not an error.
- `ignore-error=true` on the export means a full or unreachable cache never fails a build that
  succeeded.
- Only the filesystem stage has a scope. The Rust build still depends on its sources: they are
  stamped before compiling (ADR-0251), and nothing in `gha` holds build output.
- GitHub scopes cache writes to the branch that ran. A pull request, even one that edits the
  workflow to write, can only write caches visible to that pull request, never to `main` or
  `implementation`.

## Consequences

- After one run on `main` or `implementation` has written the scope, the stage is restored from
  the cache instead of downloaded. The first run after the Dockerfile's filesystem stage changes
  pays the full download once.
- The scope costs about the size of the stage: the Ubuntu base plus the btrfs/zfs userland,
  hundreds of MB of the repository's 10 GB, written only by the two long-lived branches.
- The acceptance image is now built by the `docker-container` driver. `--load` copies both
  tagged images into the runner's image store, which costs a few seconds per image.
  `docker save` and the group jobs are unchanged.
- This is proven only in CI. Locally, `xtask/tests/harness.rs::should_read_and_write_the_filesystem_stage_through_the_layer_cache_only_when_asked`
  checks the commands the harness issues in each mode and the job's wiring. Whether a cache hit
  actually shortens the job can only be seen in the CI timings.

## Alternatives considered

- **Cache every stage (`mode=max` on the main build).** That exports the builder stage's layers
  too, hundreds of MB more each run, to save work the cargo cache mounts already save.
- **`actions/cache` over a local `type=local` cache directory.** Same effect, but with a
  directory to manage and a cache key to maintain. `type=gha` stores and addresses layers by
  content itself.
- **Mirror the packages or bake a base image into a registry.** A second artifact with its own
  publishing, pinning and trust story, for a three-minute download.
- **Leave it.** This was judged not worth it on 2026-09-11 while nobody waited on CI. The
  milestone makes CI time a goal, so it is worth doing now.
