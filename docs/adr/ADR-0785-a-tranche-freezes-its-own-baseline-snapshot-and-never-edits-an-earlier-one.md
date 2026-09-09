# ADR-0785: A tranche freezes its own baseline snapshot and never edits an earlier one

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.4.1 §52.3, §57 phase H0; v0.5 §49
- Decided by: agent (autonomous)

## Context

`docs/baselines/v0.4.1.json` is the frozen snapshot v0.4.1 §57's phase H0 asked for. It holds no
figures of its own: it *names* every benchmark `docs/contracts/hardening/performance_baseline.json`
holds, so that a figure which quietly disappears from the regression baseline is visible as a
dangling reference (ADR-0548). `xtask::baseline::check` enforced that binding in both directions
against one file:

- every benchmark the snapshot names must still be in the regression baseline, with all six of
  §32.3's metrics;
- every benchmark the regression baseline holds must be named by the snapshot — *"a snapshot that
  names some of the figures is a snapshot of what somebody remembered"*.

v0.5 §49 requires release evidence for eight further measurements, and `cargo xtask perf
--write-baseline` duly added nine rows at profile `T` to the regression baseline. The second
direction then failed nine times over: `docs/baselines/v0.4.1.json — says nothing about
temporal.timeline_15m at profile T (warm)`.

Both available single-file answers are wrong. Re-capturing `v0.4.1.json` makes it claim the v0.4.1
tranche measured a temporal ledger it never had — the snapshot would say `"state": "the v0.4.1
tranche complete"` over figures taken from a v0.5 tree. Dropping the second direction throws away
the guarantee the file exists for.

## Decision

**`docs/baselines/` is a register with one frozen snapshot per tranche, and the completeness
guarantee is asked of the register rather than of any one file.**

- `cargo xtask baseline --write` writes `docs/baselines/v<workspace version>.json`. On this tree
  that is `v0.5.0.json`; `v0.4.1.json` is history and is never rewritten.
- Every snapshot in the directory is checked individually, exactly as before: schema, capture
  commit, note, counts, release inputs, artifact hashes, and every benchmark it names still
  resolving in the regression baseline with all six metrics.
- The completeness direction becomes: **every benchmark the regression baseline holds is named by
  at least one snapshot.** An absence is reported against the newest snapshot, because that is the
  file a newly measured figure belongs in.

The snapshot's `tranche`, `note` and `captured.state` are derived from the workspace version rather
than written into the generator, so a snapshot cannot claim to be a tranche it was not taken on.

## Consequences

Easy: a tranche that adds benchmarks adds one file, and the guarantee that no measured figure goes
unwritten survives untouched. A reader who wants to know what v0.4.1 measured reads `v0.4.1.json`
and gets v0.4.1's answer, not today's.

Hard: the register grows by a file per tranche, and a snapshot for a tranche nobody wrote is a
silent gap — the check cannot tell "this tranche has no snapshot yet" from "this tranche measured
nothing", because a benchmark named by no snapshot is reported regardless of which tranche
introduced it. That is the direction the guarantee needs, so it is the right way round.

Also hard: `xtask::baseline::PATH` still names `v0.4.1.json` for the tests that read the historical
snapshot directly. It is no longer *the* baseline path, only the first one.

Encoded by `xtask/tests/perf.rs::should_read_the_frozen_v041_baseline_and_find_every_metric_it_declares`,
`::should_report_a_frozen_baseline_naming_a_benchmark_nobody_measured`,
`::should_report_a_frozen_baseline_that_leaves_a_measured_benchmark_out` and
`::should_capture_the_frozen_baseline_from_the_sources_rather_than_from_a_second_list`.

## Alternatives considered

**Re-capture `v0.4.1.json` on every tranche.** One file, no new machinery — and a file whose own
`captured.state` lies about which tree it describes. v0.4.1 §2.6 forbids exactly this shape:
measuring today's tree and calling the figures yesterday's.

**Scope the completeness check to the benchmarks of the snapshot's own tranche.** Needs a record of
which tranche introduced each benchmark, which exists nowhere and would be a second hand-written
list of facts the tree already holds — the copy §52.2 exists to forbid.

**Drop the completeness direction.** Cheapest, and it removes the only reason the file is evidence
rather than decoration.
