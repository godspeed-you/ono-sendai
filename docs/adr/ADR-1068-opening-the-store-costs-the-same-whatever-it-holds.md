# ADR-1068: Opening the store costs the same whatever it holds

- Status: accepted
- Date: 2026-11-24
- Spec refs: v0.5 §10.7, §10.8, §31.5, §31.7, §32.2, §32.3, §44.1, §49; ADR-0633, ADR-0776
- Issues: #147
- Decided by: agent (autonomous)
- Supersedes: ADR-0633 decision 1 (`PRAGMA integrity_check` at every open)
- Amended by: ADR-1125 (the whole-file check runs at defined checkpoints, off the prompt)

## Context

§32.3's budgets hold at the engine level against §49's million-event fixture (ADR-0776), but a
person runs the shell, and whole-process the temporal path grew with the store: the issue measured
store *open* at 16 / 21 / 38 / 67 ms for 1k / 3k / 7k / 15k events and `changes --since 1h` at
351 ms for 20k against a 150 ms budget.

### Measured first

Release build (`cargo build --release -p ono-cli`), whole process, stores the shell itself built
(`stop process 4294967294` in a recording shell records four lifecycle events per line, §17.2),
`hyperfine -N --warmup 2 --runs 10`, medians. Machine: i7-6600U, 2 cores / 4 threads, shared with
another workstream building and testing (load average 3–6 throughout; the reference machine of the
issue was much faster, so absolute figures are not comparable with it). Commands:
`ono --no-config -c 'start recorder'` (open), `… ; changes --since 1h | count`,
`… ; timeline --since 15m | count`, `… ; changes --since 1m | count`. Each store was built in
the minutes before it was measured, so the 1h and 15m windows hold the whole store and the 1m
window holds a handful of events.

Where the time went, on the 100k store (Python `sqlite3`, warm): `PRAGMA integrity_check`
4,051 ms; `COUNT(*)` over `events` 40–270 ms; everything a query needs, a few ms.

1. **`LedgerStore::open` ran `PRAGMA integrity_check`** (ADR-0633 decision 1) — every page of the
   file, on every open. That is the linear term of the issue.
2. **Every open counted the store.** `Recorder::start` asks `downtime::restart` for the last
   observed instant through `retention()`, which also counts every event; `start recorder`'s
   status counted the same store twice more.
3. **A recording shell copied its whole store through memory.** With `temporal.recording.enabled`
   the configuration is applied twice (`config::load` and the first command), and the second
   application replaced the recorder's store with a session ledger — `carry_over` reading the
   *entire* store into memory — and the idempotent start then carried it all back. 241 s and
   347 MiB for one `timeline` over a 100k store.
4. `changes` asked `retention()` for `max_age` — a count to read a setting.

## Decision

1. **Open checks structure at a constant cost.** `check_structure` reads, for every table and
   index, the root and the first and last leaf (`SELECT … LIMIT 1` in both orders, `INDEXED BY`
   for indexes); a SQLite `CORRUPT`/`NOTADB` answer is a fatal finding and the open is refused
   with `temporal.store_corrupt`, as before. The whole-file `integrity_check` runs where a
   migration is about to rewrite the store (store version below current), which reads it whole
   anyway. Damage elsewhere is met by the read that reaches it: a `CORRUPT`/`NOTADB` answer on any
   read is now `temporal.store_corrupt` naming the store, where it used to be
   `temporal.store_unavailable`. §31.7's obligations still hold — refused, identified, the shell
   working, guidance offered — at the read rather than at the open. *Amended by ADR-1125:* a
   read never finds a table and its indexes that disagree (a missing or stale index entry silently
   skips rows), so as written this decision detected that damage never; ADR-1125 runs the whole-file
   check off the prompt at defined checkpoints and condemns a store that fails it. A file-length check against
   the header was tried and dropped: in WAL mode a healthy file is routinely shorter than its page
   count until a checkpoint, and it refused healthy stores.
2. **No count on the open path.** `LedgerStore::latest()` (`MAX(presentation_nanos)`, one index
   step) gives `downtime::restart` the last observed instant; `get recorder`/`start recorder`
   count the store once rather than twice (the recorder's figures are the session's when the
   session reads the recorder's ledger); `changes` reads `Ledger::max_age()`, the policy alone;
   the retention sweep asks whether an event exists (`EXISTS`) rather than how many.
3. **A running recorder's store is never swapped out by configuration**, and `carry_over` moves
   only what the session wrote — events since the session started observing, at most what the
   destination holds, newest first — and reports a failure instead of dropping it
   (`set_shared_ledger` returns it; `start recorder`/`stop recorder` raise it, the configuration
   keeps it for `get recorder`).

## Consequences

Measured as above, before (`3430ff06`+#142/#148, binary `ono-base`) and after (this change, with
#150's bound), medians in ms, on copies of the same seven stores:

| events | open before | open after | `changes 1m` before | after | `timeline 15m` before | after | `changes 1h` before | after |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1,000 | 49 | 44 | 92 | 42 | 68 | 62 | 60 | 55 |
| 3,000 | 115 | 43 | 211 | 43 | 213 | 74 | 254 | 89 |
| 7,000 | 236 | 43 | 181 | 50 | 257 | 81 | 528 | 175 |
| 15,000 | 202 | 57 | 244 | 58 | 500 | 78 | 894 | 296 |
| 20,000 | 427 | 53 | 186 | 48 | 209 | 66 | 720 | 384 |
| 40,000 | 355 | 53 | 394 | 50 | 382 | 74 | 982 | 677 |
| 100,000 | 2,637 | 64 | 2,952 | 94 | 2,635 | 192 | 4,377 | refused (#150), 1,151 |

Peak RSS of `changes --since 1h`: 17 MB → 17 MB at 1k, 61 → 61 MB at 20k, 107 → 107 MB at 40k,
245 MB → 130 MB at 100k (refused at #150's 50,000-event bound). `changes --since 1m`: ~15 MB
throughout, before and after. A recording-enabled shell (`ONO_TEMPORAL_RECORDING_ENABLED=true ono
-c 'timeline --since 15m | count'`): 6.3 s → 0.11 s at 20k, 241 s → 0.13 s at 100k.

- **Open is near-constant** (44 → 64 ms over a hundredfold store) and a query over a small window
  costs the window (42 → 94 ms). The residue at 100k is `start recorder`'s own answer, which
  counts the events it reports.
- **A query over a large window still costs the window.** `changes --since 1h` over a window of
  7,000 events is 175 ms and over 20,000 is 384 ms, against §32.3's 150 ms; on this host the budget
  holds for windows up to about 5,000 events. `timeline --since 15m` stays under 100 ms up to 40,000
  events (it reads `limit + 1`) and is 192 ms at 100,000. These are misses on this hardware,
  stated rather than hidden; the reference machine of the issue is several times faster.
- Damage deeper in a store is reported when a read reaches it, not at open; a store with a
  damaged page off the checked paths opens. Index/table disagreement is not met by any read; ADR-1125 says when it is found. `crates/ono-temporal-ledger/tests/corruption.rs`
  `should_name_the_store_as_corrupt_when_a_read_meets_damage_the_open_did_not_read` pins that; the
  six earlier corruption tests are unchanged and pass.
- **No harness row.** The repository pins whole-process startup with the §49 fixture linked at the
  canonical path (`temporal.startup_disabled`), with recording off. A recording row would write
  into the shared, hard-linked fixture — and, the fixture being dated in the past, the recorder's
  retention would sweep it. The measurement above is the evidence; its scripts are in the commit
  body.

## Alternatives considered

- **`PRAGMA quick_check`.** Skips index cross-checks but still reads every page: 8 ms against
  82 ms at 1k events, still linear.
- **Run the whole-file check in the recorder's maintenance turn.** §31.8 bounds that turn, and a
  4-second check (100k events; a million-event store takes far longer) is not bounded work.
- **Keep an event counter in the store's metadata** so `retention()` is constant too. A binary
  of an earlier version writing the same store would not maintain it, and a stale count is a false
  statement in `get recorder`; the count is only paid where a status is asked for.
