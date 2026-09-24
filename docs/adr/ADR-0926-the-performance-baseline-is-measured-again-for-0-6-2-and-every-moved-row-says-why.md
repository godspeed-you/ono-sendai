# ADR-0926: The performance baseline is measured again for 0.6.2, and every moved row says why

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.4.1 §32.3, §32.4, §37.2, §37.3; v0.5 §49; ADR-0489, ADR-0863, ADR-0865,
  ADR-0904, ADR-0909
- Issues: #124
- Decided by: agent (autonomous)

## Context

#124's exit test asks that `xtask perf --profile S` "holds every §34 target it held before". An
interleaved A/B on one tree at load below 3 (three rounds each, the pre-0.6.2 release profile
against `opt-level = "s"` with fat LTO) showed every §34 target held and put the profile's own
cost at 4–10 % on the CPU-bound first rows of Profile S (`process.enumeration` first row 40.7 ms
against 42.5–45.3 ms, `spatial.look` cold 20.1–20.4 against 20.7–21.3), and nothing on cold
start. But `perf --compare docs/contracts/hardening/performance_baseline.json` answered
`Regressed` on six rows — for **both** profiles. The checked-in baseline was written on
2026-09-09 (commit 4d128bd7) and the tree has moved since; zero tolerance against it measured two
weeks of product change, not #124. An independent review called that unresolved, and a comparison
that fails on every run is one nobody reads.

## Decision

The baseline is measured again, whole, on the reference machine of §37.2
(`ryzen-3900x-ubuntu-2604`), with the shell built as it ships (`cargo build --release -p ono-cli`)
and the sampler at the release tool's profile (ADR-0865), by `xtask perf --write-baseline` —
never edited by hand. A first measurement started at load 5.6 right after a build and was
discarded, because its first Profile S rows carried that load (`shell.cold_start` complete
8.8 ms against 6.6 ms quiet); the committed one started at load 2.62 and ended at 3.14, which is
this machine's idle floor. Every row that moved is attributed below, so the new figures are a
decision rather than a reset.

## What moved, and why

load_average: old 1.84, new 3.14

| row | time to first, ms | time to complete, ms | peak RSS, MiB |
|---|---|---|---|
| `shell.cold_start` S cold | 5.6 → 5.7 (+3 %) | 6.6 → 6.6 (+0 %) | 13.1 → 14.3 (+9 %) |
| `spatial.look` S cold | 57.2 → 21.0 (-63 %) | 59.9 → 23.6 (-61 %) | 23.7 → 22.6 (-4 %) |
| `spatial.look` S cache_hit | 1.4 → 3.1 (+130 %) | 63.5 → 31.9 (-50 %) | 25.0 → 24.4 (-3 %) |
| `spatial.map_first_frame` S cold | 61.1 → 21.5 (-65 %) | 64.2 → 23.7 (-63 %) | 24.5 → 23.1 (-5 %) |
| `spatial.selector_miss` S cold | 571.4 → 615.0 (+8 %) | 571.4 → 615.0 (+8 %) | 48.7 → 60.4 (+24 %) |
| `process.enumeration` S cold | 39.2 → 44.9 (+15 %) | 42.5 → 48.8 (+15 %) | 27.2 → 27.3 (+1 %) |
| `service.enumeration` S cold | 363.3 → 378.2 (+4 %) | 366.7 → 381.4 (+4 %) | 23.3 → 22.9 (-2 %) |
| `spatial.query` M cold | 119.5 → 121.3 (+2 %) | 123.6 → 124.9 (+1 %) | 27.1 → 25.4 (-6 %) |
| `spatial.query` M cache_hit | 1.5 → 3.4 (+130 %) | 127.5 → 131.3 (+3 %) | 27.5 → 27.2 (-1 %) |
| `spatial.query` M warm | 106.2 → 106.6 (+0 %) | 126.9 → 129.1 (+2 %) | 27.1 → 25.8 (-5 %) |
| `spatial.map_first_frame` M cold | 268.1 → 265.6 (-1 %) | 274.9 → 272.5 (-1 %) | 35.4 → 34.2 (-3 %) |
| `spatial.selector_miss` M cold | 916.1 → 979.7 (+7 %) | 916.1 → 979.7 (+7 %) | 121.8 → 133.5 (+10 %) |
| `spatial.selector_hit_by_sweep` M cold | 200.3 → 176.8 (-12 %) | 200.3 → 176.8 (-12 %) | 40.9 → 40.1 (-2 %) |
| `spatial.map_first_frame` L cold | 604.4 → 633.9 (+5 %) | 718.2 → 745.1 (+4 %) | 341.4 → 347.3 (+2 %) |
| `temporal.startup_disabled` T cold | 6.1 → 6.2 (+1 %) | 6.1 → 6.2 (+1 %) | 6.8 → 7.6 (+12 %) |
| `temporal.recorder_idle` T warm | 4.0 → 3.5 (-13 %) | 4.0 → 3.5 (-13 %) | 21.9 → 13.6 (-38 %) |
| `temporal.timeline_15m` T warm | 6.2 → 7.5 (+21 %) | 6.2 → 7.5 (+21 %) | 14.4 → 14.5 (+1 %) |
| `temporal.changes_1h` T warm | 37.2 → 44.4 (+19 %) | 37.2 → 44.4 (+19 %) | 22.2 → 22.4 (+1 %) |
| `temporal.reconstruct_recent` T warm | 54.6 → 68.5 (+26 %) | 54.6 → 68.5 (+26 %) | 24.9 → 25.1 (+1 %) |
| `temporal.map_historical_l1` T cache_hit | 0.2 → 0.3 (+48 %) | 0.2 → 0.3 (+48 %) | 137.2 → 137.6 (+0 %) |
| `temporal.why` T warm | 1.8 → 2.2 (+25 %) | 1.8 → 2.2 (+25 %) | 13.2 → 13.5 (+3 %) |
| `temporal.find_event` T warm | 33.4 → 41.4 (+24 %) | 33.4 → 41.4 (+24 %) | 13.1 → 13.3 (+1 %) |
| `temporal.retention_sweep` T warm | 455.3 → 422.1 (-7 %) | 455.3 → 422.1 (-7 %) | 19.7 → 12.1 (-39 %) |
| `completion.first_candidate` S cold | 6.4 → 8.6 (+34 %) | 6.4 → 8.6 (+34 %) | — |

- **Rows that run the real `ono` (Profiles S, M, L).** #124's profile explains 4–10 % on
  CPU-bound first rows and nothing on cold start (the A/B above, same tree, both profiles). The
  larger moves predate the profile and were already in the A/B's old-profile figures: `spatial.look`
  and `map_first_frame` cold at 21 ms instead of 57–61 ms, `look`/`query` cache-hit first rows at
  3.1–3.4 ms instead of 1.4–1.5 ms, `selector_miss` at +7–8 % with more resident memory. They are
  product changes between 4d128bd7 and this tree, not attributed commit by commit here.
- **In-process rows (v0.5 §49's temporal rows and the completion row).** These are sampled inside
  the xtask process, not through `ono` (ADR-0909 refuses a sampler whose build differs from the
  claim). The old rows were sampled by an xtask at the old release profile (`opt-level = 3`, thin
  LTO); these by the release tool's profile (`opt-level = "s"`, no LTO, 16 codegen units,
  ADR-0865). Their +19–48 % is mostly the sampler's code generation, not the shell's; a
  comparison is meaningful only against a sampler built the same way, which is how `perf`
  records the build it ran.
- **§34 targets** are absolute and unaffected: cold start p95 under 7 ms against the 50 ms target,
  `spatial.look` cache-hit p95 under 4 ms against 50 ms.

## Consequences

- The baseline describes this tree, so what `perf --compare` reports from here on is movement
  since 0.6.2 rather than since 2026-09-09. It does not make the comparison pass: `--compare`
  holds every metric to the baseline figure itself (`Tolerance::Absolute`, ADR-0489), and the same
  binary measured again minutes later on the same quiet machine read 0.2–2 % above it on most
  Profile S rows (`perf: 0 held, 8 regressed`). Zero tolerance measures the noise of the machine,
  which is recorded in `docs/STATE.md` as its own problem rather than hidden by a baseline
  written to pass.
- The baseline now records the load it was written at (3.14) and, since ADR-0909, the machine's
  cores; the allowance for a later run is that load plus a quarter of the cores.
- `docs/STATE.md`'s entry for the stale baseline leaves the board.

## Alternatives considered

**Re-measure only Profile S.** `--write-baseline` writes the whole record, and a record that mixes
two trees is the problem this ADR removes.

**Sample the in-process rows with a fat-LTO xtask.** It would put the temporal rows at the
shipping profile, at the cost of the 14-minute link ADR-0865 took off the serial path; a sampler
profile recorded and held constant answers the regression question equally well.
