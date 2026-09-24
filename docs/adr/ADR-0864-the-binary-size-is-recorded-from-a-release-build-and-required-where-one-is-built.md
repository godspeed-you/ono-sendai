# ADR-0864: The binary size is recorded from a release build and required where one is built

- Status: accepted — corrected by ADR-0866, ADR-0867, ADR-0868
- Date: 2026-09-24
- Spec refs: v0.4.1 §35.3, §44.2, §50.1–§50.3, §52.1, §52.2, Appendix H; ADR-0451, ADR-0563,
  ADR-0853, ADR-0863, ADR-0870, ADR-0910, ADR-0912
- Issues: #125
- Decided by: agent (autonomous)

## Context

ADR-0863 §2 decided that the stripped size of the release binary is a metric with a budget:
`metrics` records it per target triple, `build-manifest` carries it into the release input
manifest, a budget sits beside the other limits, and the gate fails when the release binary
exceeds it. It left open how, and three facts make the "how" a decision rather than a detail.

- **The figure needs a release build, and the gate does not make one.** A release build of
  `ono-cli` at the profile of #124 (`opt-level = "s"`, fat LTO, one codegen unit) took 6m58s from
  cold in the pinned build image and 9m50s on the host under a load average near 30, of which the
  final fat-LTO link of `ono` alone was 3.6 to 4.8 minutes (ADR-0865). The
  gate runs before every increment and builds debug artifacts only; adding a fat-LTO build to it
  would roughly double a local gate run to measure a number almost no increment moves.
- **Everything that reads the figure must be checkable without one.** `spec-check` compares the
  README's generated block against the tree on every gate run (§50.3), and `build-manifest` runs
  in the release workflow's `inputs` job, which builds nothing (ADR-0451).
- **A missing measurement must not pass silently.** A check that is green because there was
  nothing to measure is the fail-open shape v0.4.1 §38 forbids for skipped tests, and it is how
  the 15 MB of fcd8ce7 got through: nothing measured them.

CI already builds the shipping profile on every run in one place that also has the release tool
beside it: the builder stage of `docker/Dockerfile`, which compiles `ono` on its own and installs
it, then `kuang-sign`, `kuang-compile`, `kuang-example-plugin` and `xtask` (installed as
`ono-release-tool`), all with `--release`, for the acceptance image.

Since #126 (ADR-0870) a release ships two binaries: the shell, and `kuang-compile`, which holds
wasmtime's compiler so the shell does not. Cargo unifies features across the packages of one
invocation, so an `ono` built together with `kuang-compile` — which is what `cargo build --release`
over the whole workspace does — links the compiler and is several megabytes larger than any `ono`
a release ships. The `installable packages` job builds the release binary too, but in a bare
`rust` container with no `xtask`, and compiling `xtask` there would compile the whole workspace a
second time, in debug.

## Decision

1. **The figures live in a record.** `docs/baselines/binary-size.yaml` holds the stripped size of
   each shipped binary — `ono` and `kuang-compile` — in bytes per target triple. `cargo xtask
   metrics --write` writes it from every release build it finds under the target directory —
   `target/<triple>/release/<binary>` from `scripts/package.sh`, which is what ships, before
   `target/release/<binary>` for the host — and keeps the figure of a binary or triple nobody
   built on the recording machine. The README block (`stripped_bytes.<binary>.<triple>=…`) and the
   manifest (`binaries.<binary>.stripped_bytes`, `binaries.<binary>.budget_bytes`) read the
   record, so all three carry the same number and none of them needs a build to be checked. The
   file is generated and never hand-edited.
2. **A binary is measured only when it is current.** Cargo writes a dep-info file (`ono.d`) beside
   every binary, naming every source it was built from. A binary older than any of those, or than
   `Cargo.toml`, `Cargo.lock` or `rust-toolchain.toml` (where the profile and the graph live), or
   whose dep-info is missing, is **not measured**. Its size describes some other tree, and
   recording or checking it would be a stale figure presented as a current one. `scripts/package.sh`
   builds in a container with the checkout at `/project`, so a dep-info path that does not exist
   here is re-rooted at the checkout by its longest tail that does. An `ono` whose dep-info names
   `cranelift-codegen` is **not measured** either: it is the feature-unified build described
   above, not the shell a release ships, and the message says to rebuild with `-p ono-cli`.
3. **The budget** is `build.ono_stripped_bytes` (`binary: ono`) under a `build_budgets` list in
   `docs/contracts/hardening/limits.yaml`. Not under `limits`: those are runtime configuration
   keys, and `resource_limits.rs` holds that list against the shell's catalogue in both
   directions. The first budget is the #124 figure for x86_64 plus ten percent (ADR-0863 §2),
   measured on the tree that carries #126 as well, because that is the tree the budget first
   holds:

   | tree, profile `opt-level = "s"` + fat LTO, `scripts/package.sh` | stripped `ono` |
   |---|---|
   | fe1508dc + #124 (compiler still in the shell) | 28 708 064 bytes |
   | 15bf0386 + #124 (#126: compiler in `kuang-compile`) | 22 134 048 bytes |
   | 3e549678 + #124 (#126, and #127's feature split) | 22 136 640 bytes |

   The budget is 22 136 640 × 1.1: **24 350 304 bytes**. Against the pre-#126 figure the same
   rule would have given 31 578 871 bytes, which the shell no longer needs; a later reduction
   lowers the budget in an ADR of its own.

   `kuang-compile` is recorded and not budgeted. Its size is Cranelift's by design — ADR-0870
   moved the compiler into it precisely so that it would carry those bytes — it runs once per
   package install rather than as anybody's login shell, and #125 was filed about the shell. Its
   figure is in the README and the manifest, so growth is visible in review. The first one,
   7 305 360 bytes for x86_64, comes from a host `cargo build --release --locked -p ono-kuang-sdk
   --bin kuang-compile` on 15bf0386, because `scripts/package.sh` does not build it at this
   commit; a budget can
   be added as one more `build_budgets` row, and every check here then applies to it unchanged.

   **The core build's budget is a row of the same list.** #127's core build (ADR-0910, ADR-0912)
   is `ono` for `x86_64-unknown-linux-musl`, without the enhancements, and its exit test gave it a
   budget of its own: "under 8 MB", read as under 8 000 000 bytes. It had been a constant in
   `scripts/build-core.sh`; it is now `build.ono_core_stripped_bytes` (`binary: ono`,
   `triple: x86_64-unknown-linux-musl`, 7 999 999 bytes inclusive — the same rule). A row that
   names a triple is that triple's budget; the row without one covers the binary's other
   triples. The record, the README block and the manifest carry the core figure beside the full
   one (7 063 744 bytes at this commit), because the musl triple is where the core build lands,
   and `binary-size` holds it to its own row. `scripts/build-core.sh` reads the row itself — with
   `awk`, because the `core-build` job has no `xtask` and compiling one there would compile the
   whole workspace in debug — and a registry it cannot read fails the build rather than
   defaulting. One place holds every size budget; the two enforcement points stay where the two
   builds happen.
4. **`cargo xtask binary-size`** holds every measured release binary against its budget. Over the
   budget is a failure. A budget no measured binary answered — its binary missing, stale or not
   the shipping build — prints `not measured` and why, and passes; with `--require`, a run that
   measured nothing budgeted fails. (`--require` asks for the binary the caller has just built,
   not for every row: the acceptance image builds the full shell and not the core one.) A
   registry without a budget for the shell always fails.
5. **Where it runs:**
   - `scripts/gate.sh` runs it without `--require` as the last static step. A developer who built
     the release binary gets the check; one who did not is told in one line that nothing was
     measured and how to measure it. The gate never builds the release binary itself.
   - `docker/Dockerfile` runs it with `--require --binary /usr/local/bin/ono` — the shell built
     on its own and installed, before the release tool is built beside it — in the builder
     stage. Every CI run builds that image, so **every push is held to the budget**, and a build
     that somehow produced no binary to measure fails rather than passes. Locally,
     `scripts/acceptance.sh` runs the same stage.
   - `scripts/build-core.sh` holds the core binary to its row, in the `core-build` job on every
     push (ADR-0912).
   - `spec-check` fails when the record holds no figure while a budget exists, and when a
     recorded figure is over the budget, so the committed number cannot silently drift past it.
6. **The frozen snapshots are not asked for it.** `docs/baselines/v0.4.1.json` and `v0.5.0.json`
   are history (ADR-0785), and `baseline::check` asked every snapshot for every figure `metrics`
   and `build-manifest` produce. A snapshot cannot hold a figure first produced after it was
   taken, so `baseline.rs` lists the figures this record adds with the tranche that added them
   (`stripped_bytes.<binary>.<triple>` and the manifest's `binaries`, both 0.6.2), and a snapshot
   of an earlier tranche is not asked for them. It is still asked for everything that existed when it
   was captured; rewriting the snapshots to add the field would have made them lie about their
   own date.

## Consequences

- A change that grows the binary past the budget fails the acceptance image build on its first
  CI run, and locally for anyone who builds the release binary before the gate. Raising the
  budget is an edit to `limits.yaml` in the same commit as an ADR that says what the bytes buy.
- The recorded figure can lag the tree between two `metrics --write` runs: the record is what a
  release build measured last, not a live value. The live value is checked where a release build
  happens; the record is held to the budget by `spec-check`.
- aarch64 is budgeted by the same key but not measured in CI: its release binaries are built only
  by the tag-triggered release workflow, on runners without `xtask`. Its figure is recorded when
  somebody runs `scripts/package.sh --target aarch64-unknown-linux-gnu` and `metrics --write`.
  The x86_64 check is the one that runs on every push, and no aarch64 figure has been measured
  yet; the record carries none rather than a guess (§35.3). Enforcing it in the release workflow
  would mean carrying `xtask` onto the aarch64 runners, and is left for the day a figure shows the
  two architectures apart.
- A developer who builds the whole workspace in release gets `not measured` for `ono` rather than
  a red gate for a binary nothing ships.
- Tests: `xtask/tests/metrics.rs` (recording per triple, `kuang-compile` recorded and not held to
  the shell's budget, staleness, the compiler-linked `ono`, over and within budget, the announced
  and the required missing measurement, the missing budget, the record against the budget, the
  container-built binary dated against this checkout, the command the image build runs), `xtask/tests/provenance.rs` (the manifest carries the recorded figure and the budget),
  `xtask/tests/perf.rs::should_not_ask_a_frozen_snapshot_for_a_figure_added_after_its_tranche`.

## Alternatives considered

- **Build the release binary inside the gate.** Honest, and several minutes of fat LTO on every
  increment for a number almost no increment moves. Rejected for the local gate; CI builds it
  anyway, and that is where the check is required.
- **Skip silently when no release binary exists.** The failure mode #125 was filed about.
- **Require the measurement in the gate's CI job.** `quality gate (static)` has no release binary,
  so this would force the build into that job and duplicate the acceptance image's.
- **Measure in the `installable packages` job.** It holds the binary that ships, but not `xtask`;
  building `xtask` there compiles the workspace in debug on top of the release build. The image
  build has both already.
- **Strip a copy and measure that.** The release profile strips, and `scripts/package.sh` packages
  with `--no-strip`, so the file's size is what a package installs. A profile that stopped
  stripping would ship the unstripped binary, and the check should see that size, not hide it.
