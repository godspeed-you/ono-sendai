# ADR-0852: CI runs the gate in parts, on runners of their own

- Status: accepted
- Date: 2026-09-11
- Spec refs: AGENTS.md §10, §14; v0.4.1 §38.3 (skip verification), §43.3 (least privilege),
  §44.2 (exact tool versions); ADR-0563, ADR-0851
- Decided by: agent, at the user's request

## Context

After ADR-0851 split the acceptance suite, CI still took about twenty minutes, and the `quality
gate` job was the whole of it. Measured on run 34567215461 (`main`, b11e3f3e):

- **The gate job took 20m07.** The acceptance chain beside it took 14m24 (image 9m49, slowest
  group `spatial` 4m38), the installable packages 8m42.
- **The test step was 16m41 of the gate job.** `cargo test` compiled for 2m49 and then ran its
  568 test binaries one after another for 12m24. Only the tests inside one binary run in
  parallel.
- **Two packages hold nearly all of that time.** The binaries of `ono-cli` took 385 seconds, those
  of `xtask` 281 — `xtask/tests/packaging.rs` alone 259 — and every other package about 80
  together.
- **The static steps were three minutes:** lint 57 s, fuzz 22 s, docs 31 s, contracts 14 s, the
  skip verification 23 s.
- **The Actions cache was full.** The repository held 9.9 of its 10 GB, about 6 GB of it gate
  caches under keys no run asked for again, so fresh caches were evicted. The acceptance image
  of the run above found no cache and compiled from scratch.

## Decision

1. **`scripts/gate.sh` runs in parts on request.** Bare, it runs every step in order, as before.
   `--static` runs format, lint, fuzz, supply chain, contracts and docs. `--tests [SELECTION]`
   runs the test step over a cargo package selection, `--workspace` when none is given. A part
   builds every binary of the workspace, `--locked`, before its tests, because tests find the
   binaries of other packages in the target directory in both directions: `xtask/tests/perf.rs`,
   `xtask/tests/adapter_evidence.rs` and `crates/ono-testkit/tests/harness.rs` drive `ono`
   through `ono_testkit::ono_binary()`, and `crates/ono-cli/tests/acquisition.rs` installs
   `kuang-example-plugin` from beside it. A workspace run builds each binary for the tests of its
   own package; a selection builds only its own, and the first CI run of the parts (34571056830)
   failed on exactly that, from both sides. Listing the binaries instead would go stale with the
   next test that reaches for one. Every tool a test runs is installed in the part that runs it —
   `xtask/tests/supply_chain.rs` runs `cargo deny` over its fixtures.
2. **CI runs the parts as jobs.** `quality gate (static)` runs `--static`. `quality gate (tests of
   …)` is a matrix of two parts cut at the package boundary: `--package ono-cli`, and `--workspace
   --exclude ono-cli`. Each part is a runner of its own, so the tests share a machine with no
   other part, exactly as they did in the single job.
3. **The skip verification reads both parts as one run.** §38.3's check compares the whole
   expected-skip register with a run, in both directions, so a part that saw only some of the
   tests would report every declared skip of the other part as one that stopped happening. Each
   part therefore uploads its log, and `quality gate (skip verification)` concatenates them and
   runs `xtask skip-check` over the result. The binary comes from the part that tests `xtask` and
   has just built it. A part run locally says that the verification is left to that job.
4. **Only `main` and `implementation` save gate caches.** Each gate job has a key of its own
   (`gate-static`, `gate-tests-ono-cli`, `gate-tests-rest`) and saves it only on those two
   branches; a side branch or a pull request restores what they saved. The six stale caches were
   deleted by hand on 2026-09-11, taking the repository from 9.9 to 3.7 GB.
5. **A slow acceptance group splits into neighbouring ranges.** `spatial` becomes
   `spatial-journeys` (090–099) and `spatial-surface` (100–119). Five terminal and live-map cases
   carry 230 of its 255 seconds of cases; the split puts two of them, about 115 seconds, on one
   side and three, about 135 seconds, on the other. The rule of ADR-0851 stands: every case is in
   exactly one group, and the cases of a group run one after another.

## Consequences

- The gate's critical path becomes one test part — setup, a compile of its packages and about six
  minutes of tests — instead of every step in sequence. The acceptance chain, the image build plus
  its slowest group, is then the longest path of a run.
- More runner minutes per push: every part compiles for itself. The repository is public, so the
  standard runners cost nothing.
- **The parts resolve features for their own selection.** Cargo unifies features over the packages
  it was asked for, so a part can compile an external dependency with fewer features than a
  workspace run does. No workspace crate declares a feature of its own, so only third-party
  features can differ, and the bare `scripts/gate.sh` on a developer machine still runs the
  workspace as one. If a test ever passes in one mode and fails in the other, this is where to
  look.
- The first run under the new keys compiles without a cache.
- A package that grows slow tests moves the balance between the parts. The part's selection in
  `ci.yml` is the place to move it; a third part costs another compile.
- `scripts/release-check.sh` still runs the bare gate, the whole suite in one process.

## Alternatives considered

- **`cargo-nextest`**, which runs the tests of every binary side by side. The largest lever in
  theory, and three conflicts in practice: it runs each test in a process of its own, which
  undoes the in-process locks `ono-temporal-ledger/tests/cancellation.rs` and
  `ono-process/tests/support` serialise with; it hides a passing test's standard error, where
  `ono_testkit::skipped` writes the marker §38.3 reads; and it puts the load-sensitive tests
  beside more neighbours on the same machine.
- **Moving `xtask/tests/packaging.rs` into the `installable packages` job.** About four and a
  half minutes, with the same need to merge logs for the skip verification, and still a serial
  run of every other binary.
- **Compiling once and handing the test binaries to the parts.** Saves a compile per part, at the
  cost of an archive format for test binaries and their fixtures, which is what nextest's archive
  is for.
- **Larger runners.** Paid, and a four-core runner already sits idle through most of a serial
  test run.
