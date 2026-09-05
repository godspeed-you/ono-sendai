# ADR-0563: The gate runs the packaging suite when something it packages moved

- Status: accepted
- Date: 2026-09-03
- Spec refs: §38.1, §38.2, §38.3, §38.4, §44.2, §48.1–§48.3; ADR-0121, ADR-0122, ADR-0513, ADR-0514, ADR-0531
- Decided by: user and agent

## Context

`scripts/gate.sh` runs before every increment (AGENTS.md section 10), so its cost is paid once per
increment by whoever is working. Measured on the reference developer machine with a fully warm
`target/` — `cargo test --workspace --all-features --no-run` returns in half a second, so none of
what follows is compilation:

```
cargo test --workspace --all-features                       4m24
```

The time is not spread across the workspace. Per package:

| package    | test time |
| ---------- | --------- |
| ono-cli    | 242,8 s   |
| xtask      | 125,0 s   |
| the other 28 crates together | 22,3 s |

and inside `xtask`, one target is almost all of it: `xtask/tests/packaging.rs` takes 75,8 s of a
264 s run — the most expensive target in the repository, and the only one that exercises none of
the Rust the workspace ships. It drives `cargo deb` and `cargo generate-rpm` over a stand-in
binary (its own test executable) and reads `crates/ono-cli/Cargo.toml`, the release scripts,
`docker/Dockerfile` and `.github/workflows/release.yml`. Its subject is the packaging metadata and
the release harness, and it is unmoved by any change to the shell itself.

## What the obvious answer would have been, and why it is not this

The question that started this was the general one: run only the tests that cover what the commit
changed. Measured against this repository, that answer does not pay.

**Selection by reverse dependency does nothing.** `ono-cli` is the top of the workspace graph and
holds 243 s of the 264 s. Of the last sixty commits, twenty-four touch `ono-cli` directly and
every other code commit touches a crate whose reverse-dependency closure contains it. A
crate-level selection would select `ono-cli` on essentially every increment and leave the bill
unchanged. The twenty-eight commits that touch no crate at all are documentation and specification
work — which is exactly what the `xtask` registries test, so they do not get a cheap run either.

**Parallelising across test binaries does nothing.** `cargo test` runs one test binary at a time,
which suggests headroom. There is none where it matters: the six slowest `spatial_*` suites take
127 s run one after another and 153 s run all at once, because each already saturates the machine
on its own (8 cores, 8m30 of CPU in 2m33 of wall clock). Cross-binary parallelism buys negative
time here.

**In CI it would buy nothing at all.** The three jobs of `ci.yml` run in parallel, and the last
green run measured `quality gate` 9m19, `containerised acceptance` 11m12, `installable packages`
5m18. The critical path is the acceptance container, which rebuilds the release image
(`lto = "thin"`, `codegen-units = 1`) from scratch with no layer cache. Driving the gate's test
step to zero would not move the wall clock.

So this ADR is not test selection. It is one target, chosen because it is expensive, separable and
already covered elsewhere.

## Decision

**The gate runs `xtask/tests/packaging.rs` when an input of it moved, and always in CI.**

The inputs are two lists in `scripts/gate.sh`, and they differ in what counts as a change, because
the suite asks two different questions of them.

`PACKAGING_INPUTS` is *read*: `Cargo.toml`, `Cargo.lock`, `crates/ono-cli/Cargo.toml`,
`crates/ono-cli/packaging`, `docker/Dockerfile`, `.github/workflows/release.yml`, the four release
scripts the suite executes, the three `xtask/src` modules it drives, and the suite and its support
module themselves. An edit anywhere in one of these can change what the packagers produce or what
the suite asserts about it.

`PACKAGING_ASSETS` is only *shipped*: `LICENSE`, `README.md` and `docs/reference`. The suite
asserts that these arrive at their path inside the package, so what matters is that they exist
under the names the metadata globs — a run is selected by their addition, deletion or rename, and
not by their prose changing. This is the difference between selecting on half of recent commits
and selecting on nearly all of them: `README.md` and `docs/reference` change often and their
content is nothing the packagers read.

The baseline is the working tree against `HEAD`, because section 10 puts the gate *before* the
commit: what it is asked about is the increment on its way in. Every way of failing to get an
answer — no repository, no `HEAD`, a `git` that errors — selects the suite, so an unanswered
question never costs coverage. `ONO_PACKAGING=always` and `ONO_PACKAGING=never` override the
decision, and `ONO_CANONICAL_CI=1` selects unconditionally.

**Not selected is not skipped, and the mechanism says so.** `ono_testkit::skipped` and
`docs/contracts/hardening/expected_test_skips.yaml` are the register of what a *host* could not supply,
and none of §38.4's six categories describes "this increment did not touch it" — announcing a
`SKIPPED` marker here would put a sentence into that register that is not about the host, and
§38.3's reverse check would then be enforcing a skip that depends on a working tree. So the
mechanism is libtest's own filter: the gate passes `-- --exact --skip <name>` for each test of the
suite, and the run reports them as `filtered out` in the summary `target/gate-test.log` keeps.
§38.3's observation is untouched, because a filtered test announces nothing and `skip-check` runs
only where the suite is always selected.

The names are read out of `xtask/tests/packaging.rs` by the gate rather than listed in it, so a
test added to the suite cannot be left behind by a list nobody updated; an extraction that finds
nothing selects the suite.

## Consequences

- Measured back to back on the same machine: `4m24` unfiltered against `3m17` filtered, and
  `packaging.rs` from 75,8 s to 0,00 s. The run reports `13 filtered out` in exactly one binary
  and `3508` passing tests against `3521` — the thirteen of the suite, and nothing else.
- Against the last sixty commits the input set selects the suite on thirty of them. So the saving
  is the whole 75 s on about half of increments rather than on all of them.
- The packages are still built and installed for real on every push: `ci.yml`'s `installable
  packages` job runs `scripts/package.sh` and `scripts/package-check.sh` in fresh containers, and
  the gate job runs the suite unconditionally because it sets `ONO_CANONICAL_CI=1`. What can now
  reach a commit without the suite having run is a local increment that touched none of its
  inputs, and CI answers that before it reaches `main`.
- The input lists are the thing that can go wrong. A new file the packagers read, or a new
  `xtask` module the suite drives, has to be added to `PACKAGING_INPUTS` or the gate goes blind to
  it locally. The lists sit beside the suite's own comment naming what it reads, which is the
  closest the two get to being one place.
- A gate run on a clean tree — re-running it after the commit — selects nothing, because the
  increment it would ask about is already in `HEAD`. `ONO_PACKAGING=always` is the answer when
  that is the run that matters.

## Alternatives considered

- **A new `SkipReason` for "not selected".** It would make the omission visible through the
  machinery §38 already has. Rejected: §38.4's categories are about what a host can supply, and a
  seventh that is about the working tree would make `expected_test_skips.yaml` answer two
  different questions at once — and make the canonical CI expectation depend on what a developer
  had checked out.
- **Splitting the run into `--workspace --exclude xtask` plus a narrower `-p xtask`.** It reads
  more cleanly than a filter. Rejected on measurement: `-p xtask` resolves a different feature
  unification than `--workspace` does, which produced a second set of artifacts with different
  hashes for every `xtask` test target. The gate would then test `xtask` under a feature set CI
  does not use, and pay rebuild churn against every other cargo invocation on the machine.
- **`#[ignore]` on the thirteen tests, run with `--ignored` when selected.** Rejected: it makes a
  plain `cargo test --workspace` — what a developer types outside the gate — silently stop
  covering packaging, and the second invocation would also pick up the unrelated `#[ignore]`d
  watchdog of `spatial_first_output.rs`.
- **Doing nothing and attacking `ono-cli` instead**, which is 243 s against packaging's 76. That
  is the larger prize and it stays open: roughly 150 s of it is the `spatial_*` suites, whose cost
  is `ono_testkit::ProcessPopulation` building a §32.2 reference profile out of real processes.
  Making that fixture shareable across suites is worth more than this ADR, and it is a change to
  test code rather than to the gate.
