# ADR-0989: The performance baseline is measured again for 0.6.4, and every moved row says why

- Status: accepted
- Date: 2026-11-10
- Spec refs: v0.4.1 §32.3, §32.4, §33.2, §37.2, §37.3; v0.5 §49; ADR-0489, ADR-0926
- Issues: #187
- Decided by: agent (autonomous)

## Context

`docs/contracts/hardening/performance_baseline.json` held the rows ADR-0926 measured for 0.6.2
(`2bd5b3c8`, 2026-09-24). v0.6.4 changed the spatial sweep, the edge cache, keyed lookups and
the journal's predicate handover (ADR-0970, ADR-0977, ADR-0980, ADR-0986, ADR-0987), so the
baseline no longer described the product it guards. ADR-0926's rule stands: the baseline is
measured whole, on the reference machine, with the shell built as it ships — never edited by
hand, and never on a loaded host.

## Decision

The baseline is measured again on `ryzen-3900x-ubuntu-2604` from `20444562`, the release build
of `ono-cli` and a release `xtask`, by `cargo run --release -p xtask -- perf --write-baseline`,
inside a scratch `XDG_*` environment so the harness reads no developer configuration. The run
started at load 0.69 (1.28 when the first row was taken) and ended at 1.42, 16:14–17:04 CET; the
0.6.2 run started at 2.62. Every §33.2 target holds:

| Target | Budget | p95 |
|---|---|---|
| `spatial.look` S cache hit (basic cached look) | 50 ms | 1.1 ms |
| `spatial.query` M cold (first result) | 150 ms | 112.7 ms |
| `spatial.map_first_frame` M cold | 500 ms | 253.7 ms |
| `spatial.map_first_frame` L cold | 1,500 ms | 637.3 ms |

Every row, p95 in milliseconds, 0.6.2 → 0.6.4:

| Row | 0.6.2 | 0.6.4 | Why it moved |
|---|---|---|---|
| `shell.cold_start` S | 6.5 | 6.3 | noise |
| `spatial.look` S cold | 22.2 | 78.8 | the host, not the product — see below |
| `spatial.look` S cache hit | 3.4 | 1.1 | edge cache (ADR-0977) |
| `spatial.map_first_frame` S cold | 23.9 | 93.4 | the host, as `spatial.look` S cold |
| `spatial.selector_miss` S cold | 622.2 | 215.5 | concurrent observe, keyed lookups, name advice (ADR-0970, ADR-0986, ADR-0987) |
| `process.enumeration` S cold | 48.9 | 42.1 | lower load |
| `service.enumeration` S cold | 385.9 | 416.4 | more units on the host (655 at measurement) |
| `spatial.query` M cold / cache hit / warm | 124.9 / 3.9 / 111.5 | 112.7 / 1.5 / 97.0 | edge cache, lower load |
| `spatial.map_first_frame` M cold | 276.1 | 253.7 | as above |
| `spatial.selector_miss` M cold | 987.9 | 586.4 | as the S row; #187's 250 ms target is still not met |
| `spatial.selector_hit_by_sweep` M cold | 187.7 | 236.3 | **not attributed**: no change of this release adds work to a hit (ADR-0970 makes a class cost its slowest target rather than their sum), and this row was not measured A/B; the host's change is the likely cause, not a proven one |
| `spatial.map_first_frame` L cold | 642.1 | 637.3 | unchanged |
| `temporal.*` T, eight query rows | 0.3–71.4 | 0.2–61.6 | within ±15 % or faster; `startup_disabled` 6.8 → 8.3 is one slow iteration (first 6.2 → 6.6) |
| `temporal.retention_sweep` T | 468.4 | 3,689.9 | one iteration of twenty — first value 422.1 → 452.8 ms; see below |
| `completion.first_candidate` S | 9.2 | 7.5 | lower load |

**`spatial.look` and `map_first_frame` at Profile S are the host.** Built side by side on
2026-11-10, the release binaries of `41cc0f31` (v0.6.4's starting point, version 0.6.3) and
`20444562` take the same time on this machine: a cold `look --json` 0.07 s median of seven for
both, the first map frame 0.09 s and 0.08 s. Since 2026-09-24 the reference machine runs a
container engine with six containers and more units, and a cold look at the root counts what
each canonical space holds; the product did not regress.

**`temporal.retention_sweep` is one slow iteration.** Its first value moved 7 %; the p95 of
twenty is one sweep over the 1.8 GiB fixture ledger that waited on the shared disk. The figure is
recorded as measured, because a baseline row edited or re-picked is a figure nobody measured
(ADR-0926), and its consequence is stated instead: until the next re-measurement,
`perf --compare` holds this row to a lenient p95.

## Consequences

- `perf --compare` compares against 0.6.4 from now on. The Profile S cold `look` and map rows
  are about 3.5 times their 0.6.2 figures for a reason outside the product; a later regression
  is measured against what this machine does today.
- #187 keeps its debt: `spatial.selector_miss` M is 586.4 ms here, 576.7 ms in the earlier
  targeted run, against 250 ms.
- The next re-measurement is due when a release changes a measured path or the reference
  machine changes; the retention row is the first to look at.

## Alternatives considered

- **Leave the 0.6.2 rows.** Rejected: they describe neither this product nor this machine, and a
  comparison against them reports changes nobody can attribute.
- **Run `--write-baseline` again for a quieter retention row.** Rejected: re-running until a
  figure looks right is choosing the figure; the 50-minute run is the measurement.
