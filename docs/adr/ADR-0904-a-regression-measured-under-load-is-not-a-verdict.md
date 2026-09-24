# ADR-0904: A regression measured under load is not a verdict

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.4.1 §32.4, §37.2, §37.4, §65.10; ADR-0489; issue #169
- Decided by: agent (autonomous)

## Context

`Baseline::compare` has several answers that are not a pass, and `ForeignEnvironment`
(ADR-0489) is the one that says "no verdict": a figure from another machine does not decide a
regression on this one. The reference
environment (`docs/contracts/hardening/performance_environment.yaml`) is eight cores of a machine
shared with the developer's other work, and its notes say absolute targets are "stated with the
load average of the run beside them". `write_baseline` records that load average for the
document. Nothing read it back. On 2026-09-03 all eight Profile S benchmarks came out three to
five times their baseline (`shell.cold_start` at 132 ms against 26 ms) while a second build tree
was loading the machine, and the comparison reported regressions. It had the right machine, but
not the conditions the baseline was measured under.

## Decision

**Every record states the load average it was measured under. A comparison that would report a
regression answers `LoadedEnvironment` instead when that load exceeds the baseline's load
average by more than `LOAD_MARGIN` (2.0).**

- `Measurement::load_average` is the machine's one-minute load average over the row: the higher
  of one `/proc/loadavg` read before the samples and one after. That is two reads of one file
  per row, whatever the row costs. It is serialised as `load_average` and optional, so records
  written before this change still parse, with the load unknown. `Baseline::load_average` is the
  document's own field, which `write_baseline` already wrote.
- `Runner::load_reading` replaces where the reading comes from. That is how a test puts a real
  benchmark run under a load it can name without depending on what else the machine is doing.
- `compare` still answers `ForeignEnvironment`, `Underpowered` and `Unmeasured` first, and
  `Held` when nothing moved. It answers `LoadedEnvironment { load_average, allowed, regressions }`
  only when metrics moved the wrong way and the run's load is above the baseline's plus 2.0. The
  regressions are kept in the answer so the reader can see what to measure again. A baseline with
  no recorded load is treated as measured at 0.
- `xtask perf --compare` prints the answer as "not adjudicated" with both loads and does not fail
  on it, the same as `ForeignEnvironment`. The run's header prints the load average it starts at.

### Why 2.0, and why only for a regression

The benchmarks are mostly one `ono` process each. They slow down when they have to compete for a
core, not simply because the load number is higher. Two runnable tasks above the baseline's load
is a quarter of the reference environment's eight cores, well short of contention for a
single-process benchmark. The case the issue reports, a build tree occupying the machine, reads
eight and more. This run's own real comparison read 20 to 52.

A figure that held its baseline under load held under harder conditions than the baseline's, so
it is still a pass. Only a failure is withheld, because only a failure is the claim the load
can explain.

## Consequences

- On a shared machine, `xtask perf --compare` says it cannot judge a slowdown when that is the
  truth, instead of reporting a regression in the shell. Tested by
  `xtask/tests/perf.rs::should_report_a_loaded_machine_rather_than_a_regression_when_the_run_was_under_load`
  and `::should_record_the_load_a_benchmark_ran_under_and_withhold_a_verdict_it_cannot_give`.
  The second one runs the probe benchmark with an injected load and checks that the load survives
  a trip through the baseline file.
- A real regression measured on a loaded machine is also withheld. That is the cost: it has to be
  measured again on a quiet machine, and the message says so and keeps the numbers.
- The one-minute load average is a smoothed, system-wide figure and responds slowly to short
  spikes. Taking the higher of two readings catches load that starts or stops during a row, not a
  spike inside a short one.
- The checked-in baseline records gain a `load_average` field the next time
  `--write-baseline` runs. Nothing here rewrites them.

## Alternatives considered

- **Pressure stall information (`/proc/pressure/cpu`).** It measures contention more directly,
  but it is not available on every kernel or container, and the baseline already records load
  average. Changing the metric would leave the existing baseline without a reference value.
- **Wait for the machine to go quiet before measuring.** That can wait forever on a machine that
  is never quiet, and the result is still untrustworthy if the load starts halfway through.
- **Scale the tolerance with the load.** That makes up a model of how load slows each benchmark,
  and then presents the model's output as a measurement.
- **Fail the run when loaded.** A loaded run shows nothing about the shell in either direction,
  and `ForeignEnvironment` already sets the precedent that a comparison nobody can draw a
  conclusion from is reported, not failed.
