# ADR-0908: A figure from another build is not compared

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.4.1 §32.4, §37.2; ADR-0489, ADR-0904; issue #151
- Decided by: agent (autonomous)

## Context

Every benchmark record states the build that produced it (`"build": "release"` or `"debug"`),
and §37.2 counts the release build flags as part of the reference environment. `Baseline::compare`
ignored that field. A debug `ono`, measured on the reference machine and compared with the
release baseline, was reported as `Regressed`. Issue #151 fixed the *label* of in-process rows;
the comparison still judged a debug figure against a release one.

## Decision

**`Baseline::compare` answers `Comparison::ForeignBuild { baseline, measured }` when the baseline
record for the benchmark names a different build from the result.** The check comes after the
environment, iteration and existence checks, because it needs the baseline record, and before any
metric is compared. `xtask perf --compare` prints it and does not fail, the same as
`ForeignEnvironment`.

## Consequences

- `xtask/tests/perf.rs::should_report_a_figure_from_another_build_as_uncomparable_rather_than_as_a_regression`
  covers the new answer.
- `::should_record_the_load_a_benchmark_ran_under_and_withhold_a_verdict_it_cannot_give` runs a
  debug probe, so its baseline record now also says `debug`. Before this change it passed only
  because the build field was ignored.
- A debug run compared with the checked-in (release) baseline now says only that the builds
  differ, which is the only thing such a run can honestly say.

## Alternatives considered

- **Fail the run.** A debug run shows nothing about the release budget in either direction.
  Failing on it would make `--compare` unusable during development.
- **Fold the build into `ForeignEnvironment`.** That hides which of the two facts differs, and
  the reader needs to know that to fix it.
