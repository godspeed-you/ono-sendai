# ADR-0909: A comparison that cannot adjudicate does not exit 0, and a baseline holds every row

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.4.1 §32.4, §37.2, §37.4, §65.10; ADR-0489, ADR-0904, ADR-0908; issues #151, #169
- Decided by: agent (autonomous)

## Context

A review of ADR-0904 and ADR-0908 found four gaps:

1. `xtask perf --compare` failed only on `Regressed`. A run whose rows were all
   `LoadedEnvironment`, `ForeignBuild`, `ForeignEnvironment`, `Underpowered` or `Unmeasured`
   exited 0. A genuine regression measured under load therefore read as success to anything
   that only looks at the exit status.
2. With a debug xtask beside a release `ono`, the in-process rows (eight temporal rows plus
   completion) are refused (#151). `--write-baseline` still wrote the baseline without them and
   exited 0. `--compare` dropped them without saying so.
3. The load reading was the higher of one taken before and one taken after the row. A regression
   that uses many cores raises the load during its own row, and that reading then excused the
   row as "measured under load".
4. The margin was a flat +2.0. On eight cores that is a quarter of the machine; on sixty-four it
   is noise.

## Decision

**`perf --compare` has four exit statuses, and it ends with a summary line
`perf: N held, M regressed, K not adjudicated`:**

| Status | Meaning |
| --- | --- |
| `0` | every compared row held (and a run without `--compare` judges nothing and exits 0) |
| `1` | the run could not be made: a usage error, no `ono`, an unreadable baseline, a refused write |
| `3` (`perf::EXIT_REGRESSED`) | at least one row regressed under the baseline's conditions |
| `4` (`perf::EXIT_NOT_ADJUDICATED`) | nothing regressed, and at least one row could not be adjudicated |

A row counts as "not adjudicated" when it is `LoadedEnvironment`, `ForeignBuild`,
`ForeignEnvironment`, `Underpowered` or `Unmeasured`, and also when it was asked for but not
measured (a refused in-process row). When the run has both a regression and non-verdicts, the
regression decides the status. `perf::Outcome` does the counting and the mapping.

**A baseline holds every declared row.** `Baseline::missing_declared_rows` lists every row of
`BENCHMARKS`, `TEMPORAL_BENCHMARKS` and the completion row that the baseline has no record for,
and `spec-check` reports each one (`perf::check_registries`). `perf::write_refusal` refuses
`--write-baseline` before anything is measured in three cases: the run's label differs from
`SAMPLER_BUILD`, `--skip-temporal` is given, or the new `--skip-completion` is given. A debug
xtask can still compare against a release `ono` with `--skip-temporal --skip-completion`;
without those flags the refused rows make the comparison exit 4.

**The load is read before each row only, and the allowance scales with the cores.** A record
states `load_average` as read before the row and `cores` from `available_parallelism`. The run
is "loaded" when its load exceeds the baseline's by more than `LOAD_MARGIN_PER_CORE` (0.25) × its
cores. That is still 2.0 on the reference environment's eight cores. A record without `cores`
is treated as having eight.

## Consequences

- A script or CI step that runs `perf --compare` can tell apart a held run, a regression, and a
  run nobody can draw a conclusion from. Before this change the last case looked like a pass.
- The one-minute load average read before a row still includes the previous row. It no longer
  includes the row being judged, and a row cannot raise its own figure.
- Tests (`xtask/tests/perf.rs`): `should_exit_with_distinct_statuses_for_held_regressed_and_not_adjudicated`,
  `should_report_a_baseline_that_leaves_a_declared_row_out`,
  `should_refuse_to_write_a_baseline_that_would_leave_rows_out`,
  `should_read_the_load_before_a_row_so_a_row_cannot_excuse_its_own_load`,
  `should_scale_the_load_allowance_with_the_cores_the_run_had`.
- The CLI itself is not run in the gate; a Profile S comparison takes minutes. It was run by hand
  (debug `ono` against the release baseline: `perf: 0 held, 0 regressed, 7 not adjudicated`,
  exit 4).

## Alternatives considered

- **Exit 1 for anything but held.** That merges "the tool broke", "the shell regressed" and
  "nothing can be concluded", and those need three different responses.
- **Subtract the row's own CPU time from the load.** Load average counts runnable tasks, not CPU
  seconds, so the subtraction would be a model rather than a measurement. Reading before the row
  removes the row's own contribution without one.
