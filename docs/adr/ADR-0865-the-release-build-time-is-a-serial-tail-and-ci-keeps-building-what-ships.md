# ADR-0865: The release build time is a serial tail, and CI keeps building what ships

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.4.1 §44.2, §46.5, §46.6; ADR-0121, ADR-0123, ADR-0251, ADR-0851, ADR-0852,
  ADR-0863, ADR-0864, ADR-0870
- Issues: #218, #124, #126
- Decided by: agent (autonomous)

## Context

The release build in CI takes about seven minutes, and the only earlier figures were a local full
build of 207 s at the old profile and 398 s with fat LTO (#124). #124 changes the profile to
`opt-level = "s"` and `lto = "fat"`, which was expected to make it slower. #218 asks where the
time goes, and whether CI's release builds should keep the shipping profile.

CI builds the release profile in two jobs of `.github/workflows/ci.yml` on every push:

- **`installable packages`** runs `scripts/package.sh`: one `cargo build --release -p ono-cli` in
  the pinned `rust` image, with `CARGO_HOME` and the target directory under `target/` of a fresh
  runner — **no cache at all**, by design: it builds "exactly the way the release workflow does"
  (ADR-0121, ADR-0123), and the release builds from clean so that §46.5's two builds are two clean
  environments.
- **`acceptance image`** runs the builder stage of `docker/Dockerfile` with the registry and the
  target directory restored from `actions/cache` into BuildKit cache mounts (ADR-0251, ADR-0851).
  The workspace sources are stamped before the build, so every workspace crate recompiles and only
  the registry's crates are reused.

## Measurements

**CI, run 34965742280 (fe1508dc, old profile, `ubuntu-24.04`, 4 vCPU), from the job logs:**

| job | step | time |
|---|---|---|
| installable packages | `cargo build` in `scripts/package.sh` | 7m10s cold |
| — of which | dependencies and workspace crates below `ono-cli` | 11:53:54 → 11:58:19, 4m25s |
| — of which | `ono-cli` alone (crate at `codegen-units = 1`, then the LTO link of `ono`) | 11:58:19 → 12:01:27, 3m08s |
| acceptance image | `scripts/acceptance.sh --build-only` | 5m22s with warm dependencies |

The "seven minutes" is the packaging job's cold build; the image build is a little over five.

**Locally, `cargo build --timings`** (8 cores, toolchain 1.94, lld as linker through an untracked
`.cargo/config.toml`; load average 14–41 from other builds throughout, so single figures move by
tens of percent — the shape is what is reliable, not the seconds):

| build | old: `opt-level = 3`, thin LTO | new: `opt-level = "s"`, fat LTO |
|---|---|---|
| `-p ono-cli`, cold, host | 645 s wall, 1122 s CPU | 590 s wall, 785 s CPU |
| — `ono-cli` crate (one codegen unit) | 117 s | 80 s |
| — link of `ono` (LTO) | 241 s | 289 s |
| image builder stage, warm (sources stamped; `-p ono-cli -p ono-kuang-sdk -p xtask`) | 508 s | 484 s |
| — `ono-cli` crate | 198 s | 84 s |
| — link of `ono` | 257 s | 349 s |
| — link of `xtask`, beside it | 204 s | 294 s |
| `scripts/package.sh`, cold, pinned image | CI: 7m10s | 6m58s (`ono` link 217 s) |
| stripped `ono` | 46 094 688 bytes | 28 708 064 bytes |

**After #126 (ADR-0870), on 15bf0386 + #124,** the image's builder stage runs two invocations —
`ono` alone, then `ono-kuang-sdk` and `xtask` — because building them together would link the
compiler into the shell. Measured the same way (sources stamped, warm dependencies; load average
9–41):

| image builder stage, warm, after #126 | wall |
|---|---|
| `cargo build --release -p ono-cli` (crate 44 s, link of `ono` 147 s) | 223 s |
| `cargo build --release -p ono-kuang-sdk -p xtask` | 887 s |
| — of which the fat-LTO link of `xtask` | 817 s |
| — of which the link of `kuang-compile` | 138 s |

The shell's own link got shorter — Cranelift is no longer in it — and the release tool's became
the longest step of the stage, now on the serial path after the shell instead of beside it.
`xtask` links the whole shell *and* the compiler, at fat LTO, single-threaded. At the
`release-tool` profile of the decision below (no LTO, sixteen codegen units), the same tool
built in 594 s from cold dependencies and 317 s with warm ones under a load average near 50, the
link no longer among the five longest units. The image builds with the change
(`scripts/acceptance.sh --build-only`, 707 s here with a shared BuildKit cache of unknown
warmth, so not a comparison), and the cases that run the release tool and `kuang-compile` in it
pass.

## Where the time goes

1. **A serial tail at the end of every build.** `ono-cli` is the top of the graph: its crate
   compiles as one codegen unit, and then the whole program is linked with LTO. Neither step can
   start before everything below it is done, and neither uses more than one or two cores. On every
   measurement above, this tail is half or more of the wall time — 3m08s of the CI packaging
   build's 7m10s, 358 s of the local cold build's 645 s, and 455 s of the warm image build's
   508 s.
2. **In the packaging job, dependencies that never change.** The other four minutes are the
   registry's crates — `cranelift-codegen` alone was the longest unit of every cold build before
   #126, then `wasmtime`, `libsqlite3-sys`'s build script, `zbus` — compiled again on every push,
   because the job keeps no cache.
3. **In the image build, the workspace.** Dependencies are cached there; the stamp that keeps the
   referee honest (ADR-0251) recompiles every workspace crate, and then the same tail follows.

**Fat LTO does not double it here.** It moves time from the crate into the link: `opt-level = "s"`
compiles `ono-cli` in well under half the time, and the single-threaded fat link takes longer
than the thin one. The wall time is about the same (−9 % cold, −5 % warm, within the noise of the
machine), and the CPU time is 30 % lower. #124's 207 s → 398 s compared thin and fat LTO at
`opt-level = 3`; at `opt-level = "s"` the crate compiles cheaply enough to pay for the longer link.

## Decision

1. **Every release build in CI of a binary that ships keeps the shipping profile.** The
   acceptance image grades the `ono` and the `kuang-compile` it builds, and the packaging job
   installs the packages it builds; both exist to test what ships, so neither may build something
   else. A cheaper profile there would make the acceptance suite and the size budget of ADR-0864
   grade a different binary — the failure the image's source stamp was introduced to prevent
   (ADR-0251).
2. **The release tool does not.** `xtask`, installed in the image as `ono-release-tool`, ships in
   no package and is not what any case grades; it runs the checksum, provenance and size checks.
   It is built at a `release-tool` profile (`inherits = "release"`, `lto = false`,
   `codegen-units = 16`) in an invocation of its own. Its dependencies are compiled once more for
   that profile — minutes on a cold cache, nothing on the warm one CI restores — and its link
   stops being the stage's longest step. `kuang-sign` and `kuang-example-plugin` stay in the
   `ono-kuang-sdk` invocation at the shipping profile: they share it with `kuang-compile`, and
   their links are short.
3. **The packaging job publishes its `--timings` report.** `scripts/package.sh` passes
   `--timings` to its build (it changes the report, not the artifact), and `ci.yml` uploads
   `target/cargo-timings/cargo-timing.html` as the artifact `release-build-timings` for fourteen
   days. The job is the one cold, single-package build of the shipping profile in CI, so its
   report answers "where did this push's release build spend its time" without a local rebuild.
4. **The packaging job stays uncached.** Caching its dependencies would save roughly the four
   minutes of point 2 above, and it would make the job stop building the way the release builds, which is
   the job's reason to exist (ADR-0121, ADR-0123); the repository's 10 GB of caches is also
   already shared by the gate's three caches and the acceptance image's. The minutes are the
   price of the job's claim.

## Consequences

- A slow release build in CI now comes with its own evidence: the timings artifact of the push.
- The serial tail is the lever left, and it is structural: fewer bytes in `ono` (#126 took the
  compiler out; the budget of ADR-0864 keeps them out) shorten the link; splitting `ono-cli` into
  crates that compile in parallel shortens the crate. Neither is a CI setting.
- The acceptance image builds three times in a row: `ono`, then `ono-kuang-sdk`, then `xtask` at
  the tool profile. The image's cargo cache holds a second set of dependency artifacts for the
  tool profile, which makes it larger; if the 10 GB of repository caches start evicting each
  other, this is the first place to look.
- Measurements under load are recorded as ranges with the load beside them; the coordinator
  repeats the profile comparison on a quiet machine before the release.

## Alternatives considered

- **Build every CI release build at a cheaper profile.** Rejected in (1): the image's `ono` and
  `kuang-compile` are the tested artifacts.
- **Keep `xtask` at the shipping profile.** Before #126 its link ran beside the shell's and cost
  nothing on the critical path; since #126 it runs after it and is the longest step of the
  stage. The profile bought nothing for a tool whose size and speed nobody measures.
- **Build `xtask` in the same invocation as `ono`.** It would run beside the shell's link again,
  and cargo would unify its dependencies' features into the shell's, which is what ADR-0870's
  separate invocation exists to prevent.
- **`codegen-units` above one, or thin LTO, for speed.** Both undo part of what #124 bought in
  size; the profile is chosen for the artifact, and CI's minutes are not the artifact.
- **Cache the packaging job.** Rejected in (3).
