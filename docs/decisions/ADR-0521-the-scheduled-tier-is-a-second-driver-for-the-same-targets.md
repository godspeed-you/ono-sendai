# ADR-0521: The scheduled tier is a second driver for the same targets

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §41.1 (the fast tier stays in the gate), §41.2 (the coverage-guided tier and
  its seven entry points), §41.3 (schedule), §41.4 (corpus persistence), §41.5 (hangs), §5.2
  attacker classes 6–8, §43.1, §43.3, §44.3
- Issues: #92, #93
- Decided by: agent (autonomous)

## Context

`ono-fuzz` already exists (ADR-0313): five targets over spec §35.6's areas, a committed seed
corpus, committed crash artifacts, a bounded deterministic mutator and a per-input ceiling, run by
`scripts/gate.sh` for four hundred iterations a target on every increment. §41.1 says that tier
stays.

What it is not is coverage-guided. Four hundred iterations from a fixed seed reach what the seed
was already near; nothing feeds a branch that a mutation newly reached back into the mutator.
§41.2 requires *"a scheduled coverage-guided fuzzing tier using `cargo-fuzz`/libFuzzer or an
equivalent Rust-compatible engine"*, and names seven entry points, two of which had no target:
the **remote handshake decoder** read apart from the framing around it, and the **adapter
machine-readable decoders**, which are on no remote path at all. Those two are attacker classes 7
and 8 of §5.2 — an adapter producing malformed machine-readable output, and a pathological host.

The obstacle is the toolchain. libFuzzer needs `-Z sanitizer`, which is nightly, and
`rust-toolchain.toml` pins 1.94 because §44.3 says a release is built by a toolchain the
repository names.

## Decision

**One set of target bodies, two drivers, and the toolchain difference is kept out of the gate.**

`fuzz/coverage-guided/` is a cargo-fuzz crate with its own `[workspace]` table, so it is not a
member of this repository's workspace. Each of its seven targets is one line: it looks up the
target by name in `ono_fuzz::TARGETS` and calls its body. The gate's tier and the scheduled tier
therefore execute *the same code on the same corpora*, and a finding in either reproduces in the
other with `cargo run -p ono-fuzz -- repro <target> <file>` on the pinned stable toolchain. A
second implementation of a decoder harness would be two things to keep in step and two places for
a finding to be almost reproducible.

Being outside the workspace also keeps `libfuzzer-sys` out of `Cargo.lock`, out of
`cargo deny`'s graph and out of every gate build. Nothing it pulls in is ever shipped.

`.github/workflows/fuzz.yml` runs seven targets in a matrix, five minutes each, daily — thirty-five
minutes aggregate, which clears §41.3's thirty. `workflow_dispatch` takes a `minutes` input; §41.3
asks release qualification for two CPU-hours, which is eighteen. `fail-fast: false`, because a
campaign that stops at the first finding finds one thing a night.

**§41.4, the corpus.** The seeds and the crash reproducers stay committed and stay replayed on
every gate run, which is what "promoted into regression tests" means when the promotion is not
something somebody has to remember. The corpus libFuzzer *grows* is different: twenty seconds of
`remote-handshake` produced 964 files, which is too large to commit and too valuable to discard,
so it is cached per target and restored before each run. A campaign that starts from the seeds
every night re-discovers the same shallow inputs for five minutes and never gets past them.
Crash artifacts are uploaded on every path, `if: always()`, for ninety days.

**§41.5, hangs.** `-timeout=10` and `-rss_limit_mb=2048`. libFuzzer's timeout kills the process,
which is stronger than the gate tier's `--per-input-ms`, and the gate tier's is what a developer
machine can afford.

**The toolchain rule gains a narrow exception.** `check_tool_versions` refused any workflow asking
for a toolchain other than the pinned one, which would forbid three jobs the specification
requires. It now excuses a request that (a) names the section requiring it in a comment on the
line, and (b) lives in a workflow that builds no artifact — no `scripts/package.sh`, no
`release-check.sh`, no release upload. `ci.yml` packages, so nothing in it is excused. The reason
travels with the request instead of living in somebody's memory.

## Consequences

Easy: seven targets are fuzzed properly rather than sampled, and the two §41.2 entry points that
nothing reached now have targets and seed corpora of their own. The list in the workflow is held
against `ono_fuzz::TARGETS` by a gate test, so a target added and forgotten in the workflow fails
the gate rather than quietly never running.

Hard: **the scheduled tier is built by a nightly toolchain and nothing pins it.** `nightly` is a
moving reference, which is exactly what §65.11 calls a failure mode — for *release inputs*. This
builds no release input, and pinning a nightly date would mean bumping it by hand forever to keep
a fuzzer that finds nothing new. The trade is stated rather than hidden, and the scan makes the
exception visible at the line that takes it.

Also hard: `fuzz/coverage-guided/Cargo.lock` is a second lockfile that `cargo deny` does not read.
It is committed for reproducibility and its whole content is `libfuzzer-sys` and this repository.
A dependency audit that covered it would need `cargo deny` run in the fuzz workflow with a second
configuration, which is worth doing when that lockfile has something in it worth auditing.

Encoded by: `xtask/tests/supply_chain.rs::should_declare_a_scheduled_coverage_guided_fuzzing_job_for_every_declared_target`,
`::should_keep_the_deterministic_fuzz_tier_inside_the_gate`;
`fuzz/tests/corpus.rs::should_have_a_target_for_every_entry_point_the_coverage_guided_tier_must_cover`,
`::should_reload_the_persisted_corpus_for_every_target`,
`::should_record_an_input_that_exceeds_its_timeout_as_a_hang`.

Verified by running it: `cargo +nightly fuzz run --fuzz-dir fuzz/coverage-guided remote-handshake
-- -max_total_time=20` executed 599 253 inputs in twenty-one seconds and grew a 964-file corpus,
on this machine, before the workflow was written.

## Alternatives considered

**Make `ono-fuzz` itself coverage-guided.** `-C instrument-coverage` plus a feedback loop is a
fuzzing engine, and writing one to avoid a nightly toolchain is a large piece of unrelated
software. §41.2 names libFuzzer first for a reason.

**Put the cargo-fuzz targets in `fuzz/` beside the existing crate.** cargo-fuzz expects a crate of
its own with `cargo-fuzz = true` in its metadata, and making `ono-fuzz` that crate would pull
`libfuzzer-sys` into the workspace lockfile and into every gate build for a tier the gate does not
run.

**Commit the grown corpus.** 964 files after twenty seconds, and a nightly campaign would add to
it forever. §41.4 offers "committed *or* stored as durable CI artifacts" precisely so this can be
the second.

**Run the campaign on pull requests instead of on a schedule.** §41.3 asks for daily on the
default branch. A five-minute campaign on every push would cost more and find less: coverage-guided
fuzzing pays off over hours, and the corpus is what carries the progress between them.
