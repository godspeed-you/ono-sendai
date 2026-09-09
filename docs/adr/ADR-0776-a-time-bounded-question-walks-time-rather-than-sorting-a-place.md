# ADR-0776: A time-bounded question walks time rather than sorting a place

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §32.2, §32.3, §49, §9.1, §16.3; v0.4.1 §37.3
- Decided by: agent (autonomous)

## Context

Two of §32.3's budgeted rows missed against the §49 fixture — a real store of 1,000,000 events,
100,000 objects, 500,000 relation changes and 10,000 actions, 1.6 GiB on disk:

```
temporal.why                 p95  227.8 ms   budget 200 ms
temporal.reconstruct_recent  p95  198.5 ms   budget 150 ms
```

Phase timing found the cost in one place and not where it looked. `why` spends 221.9 ms of 222.4
on the candidate query and 0.5 ms on the evidence, the causal context and the explanation.
`reconstruct_recent` spends 110 ms of 135 ms inside SQLite and 19 ms decoding the 3,472 rows it
read.

`EXPLAIN QUERY PLAN` says why:

```
SEARCH events USING INDEX events_by_place (scope_path>? AND scope_path<?)
USE TEMP B-TREE FOR ORDER BY
```

A scope is a **prefix range**, because a place includes everything under it, and `events_by_place`
leads on `scope_path`. An index led by a range gives no usable order for the
`ORDER BY presentation_nanos` that every temporal query ends with, so SQLite reads every candidate
row, sorts all of them, and then takes the hundred or five hundred it was asked for. For `why`
that was 62,500 rows sorted to answer a question about 100; for the reconstruction the root scope
covers every place, so the scope predicate excluded nothing and the scan was the whole index.

Two smaller costs sat beside it: `unseal` deep-cloned each decoded event body out of its frame,
and `read_record` cloned the half-built record and the value once per field to find out whether
the schema declared the name.

## Decision

**A third index, and store version 3.** `events_by_time_place (presentation_nanos, scope_path)`
leads on the instant and carries the scope beside it, so a scoped query in time order is an
ordered walk with the scope tested from the index.

**The query builder names it when the question is bounded.** `event_selection` adds
`INDEXED BY events_by_time_place` when the query gives a scope **and** a limit, a `from` or an
`until`. The cost model this fixes is the one a reader can predict:

- a question that names an instant or a count costs **the window or the count** — `changes --since
  1h` costs an hour of events whatever the ledger holds;
- a question that names neither, wanting every event of a place, costs **the place**, and the
  planner's own choice is right for it.

Leaving the choice to SQLite is not an option here: without `ANALYZE` statistics it prefers
`events_by_place` on every one of these queries, and it prefers it by a factor of 250 in the wrong
direction (83.1 ms against 0.32 ms for the same hundred rows on the fixture).

**Two decoding costs removed.** `unseal` moves the body out of its frame instead of cloning it;
`read_record` asks `Schema::position_of` whether a name is declared instead of cloning the builder
to find out by trying.

## Consequences

Measured on the §49 fixture, release build, one process per sample:

| row | before | after | budget |
|---|---|---|---|
| `temporal.why` | 222 ms | **1.9 ms** | 200 ms |
| `temporal.reconstruct_recent` | 162 ms | **54.4 ms** | 150 ms |
| `temporal.timeline_15m` | 6.6 ms | 6.0 ms | 100 ms |
| `temporal.changes_1h` | 40.6 ms | 38.0 ms | 150 ms |
| `temporal.find_event` | 33.6 ms | 36.2 ms | 150 ms |
| `temporal.map_historical_l1` | 0.20 ms | 0.20 ms | 150 ms |

- An existing store migrates: `STORE_VERSION` is 3 and V3 creates the index. `EventId`,
  `EvidenceId` and `ActionId` are untouched, which is what §31.5's migration rule asks.
- One more index to maintain on write. The retention sweep, which deletes in bulk, measures
  unchanged within its run-to-run spread.
- `INDEXED BY` is a hard requirement rather than a hint: if the index were dropped the query would
  fail to prepare rather than silently get slow. The migration owns it, so that is the honest
  coupling — and it makes the plan a fact of this repository rather than a property of whichever
  SQLite the host ships.
- The known bad case is stated rather than hidden: a scope holding very few events, asked over a
  window holding very many, walks the window. That is the same cost as asking the window without
  the scope, and a caller who wants the place should not bound the time.

## Alternatives considered

- **`ANALYZE`, and let the planner decide.** The principled answer, and it would adapt per query.
  Rejected for now: the statistics have to be built and maintained over a store that grows by
  design, a 1.6 GiB `ANALYZE` is not something a shell may do on the way to answering `why`, and
  the resulting plan would vary with the store's shape — which makes §32.3's budgets unrepeatable.
  Worth revisiting with `PRAGMA optimize` on a recorder that is already doing periodic work.
- **Bound `why`'s candidate window instead.** It would have fixed `why` and not the reconstruction,
  and it would have made the budget hold by asking a smaller question rather than by answering the
  same one faster.
- **Drop `events_by_place`.** It still serves the unbounded case, which is a real one.
