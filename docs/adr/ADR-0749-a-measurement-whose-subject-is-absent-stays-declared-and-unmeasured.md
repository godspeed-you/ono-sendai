# ADR-0749: A measurement whose subject is absent stays declared and unmeasured

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §49, §32.3, §12; v0.4.1 §38.1, §38.4, §65.10; ADR-0517
- Decided by: agent (autonomous)

## Context

v0.5 §49 requires release evidence for eight measurements. The tranche implementing v0.5 is being
built by parallel packages, and at the moment this one ran, the historical spatial world — the
second `SpatialIndex` a `map --at` would be drawn from — was not in the tree. There is nothing to
measure and no honest way to produce a figure for `historical map L1`.

The repository has met this shape before and has a rule for it. §38.1 and §38.4 say a host that
cannot supply a fixture has not found a defect in the product, and `SocketPopulation::try_of`
reports a `missing_privilege` skip naming both numbers rather than panicking (ADR-0517). §65.10
says the opposite failure is worse: a skip that reaches the summary as a pass.

The tempting shortcuts are all worse than the honest answer. Deleting the row loses the
requirement. Measuring something adjacent and labelling it `map historical L1` is a figure about a
different thing. Widening the budget until whatever exists fits inside it is what §12 of the run's
instructions forbids in terms — performance is not solved by narrowing the semantics.

## Decision

**A benchmark this repository cannot measure yet keeps its row, names what it is waiting for, and
contributes no record.** `TemporalBenchmark::blocked_on` carries the reason as prose:

```rust
TemporalBenchmark {
    id: "temporal.map_historical_l1",
    temperature: Temperature::CacheHit,
    spec: "v0.5 §32.3, §49",
    operation: TemporalOperation::HistoricalMapL1,
    blocked_on: Some("the historical spatial world … is not in the tree, so there is no second
                      `SpatialIndex` to draw an L1 map from. …"),
}
```

`cargo xtask perf` prints the reason where the figure would have been and writes nothing to the
baseline. `perf::verdicts` then answers `Unmeasured` for §32.3's `map historical L0/L1 cached`,
and `Unmeasured` is not a pass — it is the same answer `Baseline::compare` gives a benchmark with
no baseline record, for the same reason.

The reason is prose rather than an enum because it is addressed to the next reader, and what that
reader needs is which package unblocks it. A reason that said `Blocked` would leave them concluding
the measurement had been forgotten.

## Consequences

The release evidence for v0.5 §49 is complete in its list and incomplete in its figures, and it
says which is which in the same table. The unmeasured row is visible in three places: the perf
run's own output, the absence of a record in `performance_baseline.json`, and `verdicts`' answer.

Removing a `blocked_on` is a one-line change, and the measurement it unblocks then runs on the next
`cargo xtask perf`. Nothing forces that line to be removed on the day the blocking package lands.
The alternative — a test that fails until it is — would put a red gate in front of every other
agent for a dependency none of them own, which trades one honest gap for a broken referee, and
AGENTS.md §14 ranks a working referee above every feature.

`xtask/tests/perf.rs::should_declare_a_benchmark_for_every_release_measurement_v05_requires`
asserts §49's eight rows exist, typed out from the specification. That is the part a test can keep
honest without owning another package's schedule: the *list* cannot silently shrink, whatever the
figures say.

## Alternatives considered

- **Fail the gate until every §49 row is measured.** Rejected: a red gate on a dependency this
  package does not own, blocking every other agent. §14's referee outranks the feature, and a
  referee that cannot run is worse than an incomplete measurement that says so.
- **Measure something adjacent and call it the row.** Rejected under §12 and §49's own last line:
  the purpose is to expose architectural scaling failures, and a figure about the wrong subject
  hides one.
- **Delete the row until its subject exists.** Rejected: §49 requires the measurement, and a
  requirement nobody can see is a requirement nobody will meet.
