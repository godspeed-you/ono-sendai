# ADR-0654: Replay orders inside an evidence group and presents between them

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §9.1, §25.1, §25.4, §25.5, §26.1, §26.3, §47.2
- Decided by: agent (autonomous)

## Context

§9.1 step 2 applies "ordered compatible events" from a checkpoint through `T`, and §47.2 asks for
the property that makes that meaningful: "applying an event sequence to a checkpoint is
deterministic", and "replaying persisted events yields the same reconstruction as before restart".

§26.1 says what an order may rest on — "stronger evidence than wall time" — and
`ono_temporal_core::happens_before` implements it: a monotonic reading inside one clock domain, a
source sequence inside one domain and one provider, and `Concurrent` for everything else. §26.3
then permits a stable display order while forbidding any claim that it means anything.

`happens_before` is not a total order, so it cannot be a `sort_by` comparator: Rust's sort
requires one, and feeding it a partial relation is undefined behaviour of the "wrong answers or a
panic" kind. Replay nonetheless needs a total order, because applying events in a different order
may produce a different state.

## Decision

**Group by what can order the events, sort each group by its own evidence, merge the groups by
presentation instant.**

An event's group is the thing `happens_before` would answer from:

- a known boot and a monotonic reading — the group is the clock domain, ranked by the reading;
- otherwise a source sequence — the group is the clock domain and the reporting provider, ranked
  by the sequence;
- otherwise the event is alone.

Groups are then merged by taking, at each step, the queue head with the smallest presentation
instant, broken by event id. Intra-group order is never disturbed by the merge, so an event
sequence stays in its own order through a backward wall-clock step — §47.2's "ordering is stable
under wall-clock jumps when source sequence exists". Between groups, position is presentation
order and nothing else.

The result is a function of the event *set* alone: no arrival order, no ledger insertion order and
no clock reading takes part. That is §47.2's determinism, and it is what makes a reconstruction
after a restart equal to the one before it.

`replay_plan` returns, for every step, the `OrderEvidence` that placed it relative to the step
before, or `None`. `None` is the honest answer for a step placed by presentation order, and it is
what an `inspect` view must print instead of an arrow — §26.3: "`inspect` MUST not claim semantic
ordering".

## Consequences

- Replay is O(n log n) in the number of events in the window, with one grouping pass, and needs no
  pairwise comparison; `happens_before` is consulted through the grouping rule rather than n²
  times.
- Two concurrent events land in a defined position and carry no claim about which happened first,
  which is the pair of requirements §26.3 sets and the reason the order and the evidence are two
  separate answers.
- A source that reports a sequence gets its own order honoured even when its clock is wrong, which
  is the case §25.4 exists for.
- Encoded in `crates/ono-temporal-reconstruct/tests/determinism.rs`.

## Alternatives considered

- **`sort_by` with a comparator that calls `happens_before`.** The comparator is not a total order
  and the standard library says so; the sort is entitled to panic.
- **A full topological sort over a happens-before DAG.** Correct and quadratic in edge
  construction, for a relation that is empty between groups by construction. The grouping is the
  same answer at O(n log n).
- **Sort by presentation instant alone.** Loses a source's own sequence exactly when it matters —
  a backward clock step — which §47.2 names as a property to test.
- **Sort by ingest order.** Makes the answer depend on how the events arrived, which is the one
  thing §47.2's determinism forbids.
