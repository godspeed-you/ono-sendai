# ADR-0851: The acceptance suite runs in groups, one case after another

- Status: accepted
- Date: 2026-09-10
- Spec refs: AGENTS.md §10, §14; v0.4.1 §40.4 (every case has a finite timeout), §44.1 (images
  pinned by digest); ADR-0251, ADR-0563
- Decided by: agent, at the user's request

## Context

The `containerised acceptance` job took 22 to 23 minutes: CI run 34457050938 (205 cases) 22m40,
run 34515787631 (248 cases) 23m13. Measured from the timestamps of their logs:

- **The image build was 7 to 8 minutes of it, on every run.** `docker/Dockerfile` keeps cargo's
  registry and target directory in BuildKit cache mounts, and a fresh GitHub runner has neither,
  so every dependency compiled again.
- **The cases were the other 15 minutes, one after another.** 176 of the 205 cases of the first
  run took five seconds or less, with a median of 0.9 seconds; ten PTY, live-map and budget cases
  took eight minutes between them, the full-screen timeline alone 134 seconds.
- **A failure surfaced at the end of the job.** A case that broke at position 200 was reported
  after twenty minutes, and a quick case that broke early was visible no sooner, because the job
  turned red only when it finished.

## Decision

1. **Every case belongs to one group.** `docker/acceptance/groups` names seven groups by the
   tranche that introduced their cases — `core` (000–089), `spatial`, `hardening`, `packages`,
   `temporal`, `temporal-ledger`, `change` — each an inclusive range of the number a case file's
   name starts with. `scripts/acceptance.sh` checks the assignment on every invocation and
   refuses to run while a case is in no group or in two, so the case that opens a new tranche
   arrives with a new line. `--group NAME` selects a group, `--list-groups` prints them.
2. **CI builds once and fans out.** The job `acceptance image` runs
   `scripts/acceptance.sh --build-only`, saves both images into one zstd tarball and uploads it as
   an artifact kept for a day. One `containerised acceptance (<group>)` job per group loads it and
   runs `--no-build --group <group>`. The matrix is read from `--list-groups`, so the groups file
   is the only list, and `fail-fast: false` lets every group report.
3. **Cases run one after another.** Each group job is a runner VM of its own, so a case shares CPU,
   memory and loop devices with no other case, exactly as before.
4. **The build's cargo caches survive between runs.** `actions/cache` and
   `reproducible-containers/buildkit-cache-dance` carry the two cache mounts from one run to the
   next, keyed on `Cargo.lock` and `rust-toolchain.toml`; when the key moves, the newest older
   cache is the starting point. ADR-0251 is what makes this safe: the build stamps the workspace
   sources before compiling, so a restored artifact never stands in for a changed source. The
   utility image the action pulls is pinned by digest, like every other image here (§44.1).
5. **Quick cases first, with their time shown.** A run orders its cases by declared `timeout:`,
   file name second. On both runs above, every case declaring 60 seconds or less finished within
   5.3 seconds, and every case that took 30 seconds or more declared at least 90 — so the quick
   majority runs first and an early failure among them shows in the first minutes. Each result
   line carries the case's wall-clock time. `--fail-fast` stops at the first failing case and
   says how many did not run. CI leaves it off: a group that stopped at its first failure would
   hide the second one until the next push.

The expected wall-clock time is the build (seven minutes cold, less once the cache is warm), plus
loading the images, plus the slowest group — `spatial`, 263 seconds of cases on run 34515787631.

## Consequences

- A red job names the area that broke, and the other groups report independently.
- `scripts/acceptance.sh` without arguments still runs the whole suite in one process, as
  `scripts/release-check.sh` uses it. Its output is ordered by budget rather than by number.
- The group sums move as cases are added. When one group dominates the wall-clock time, its range
  is split; the per-case times in the job logs say where.
- The cargo cache counts against the repository's Actions cache quota, beside the gate's
  `rust-cache`.
- The images travel between jobs as an artifact of a few hundred megabytes per run.

## Alternatives considered

- **Running cases concurrently inside a job.** Rejected. About half the cases carry
  time-dependent assertions — budgets, bounded waits, CPU share, cancellation windows — and the
  gate already has two wall-clock tests that fail beside a release build. Concurrency would turn
  load into red cases. It could return as an opt-in a case declares after repeated runs under
  load; the groups make it unnecessary for now.
- **Balanced shards by count or measured time (`--shard i/N`).** Balances better, but a red
  shard names no area, and the assignment would shift whenever a case is added.
- **Building the binary on the host with `rust-cache` and copying it in.** Simpler to cache, but
  the image would stop building what it grades (ADR-0251), and the acceptance binary would stop
  being the one the Dockerfile's release build produces.
- **Layer caching (`cache-to: type=gha`).** Any source change invalidates the build's one
  expensive layer, so it would cache the apt layers and little else.
