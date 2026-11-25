# ADR-1125: The whole-file check runs at defined checkpoints, off the prompt

- Status: accepted
- Date: 2026-11-25
- Spec refs: v0.5 §10.8, §31.5, §31.7, §31.9, §43.4, §44.1, §44.3; ADR-0633, ADR-1068
- Issues: #147 (review finding COR-T1, TST-5, SEC-C-2)
- Decided by: agent (autonomous)
- Amends: ADR-1068 decision 1

## Context

ADR-1068 took `PRAGMA integrity_check` out of `LedgerStore::open` because it reads every page and
made opening cost as much as the store is large (2.6 s at 100k events). What replaced it,
`check_structure`, reads the root and first and last leaf of every b-tree, and a read meets damaged
*pages* as `SQLITE_CORRUPT`. Neither ever finds a table and its indexes that disagree: rows missing
from an index, or index keys pointing at rows that changed, read back without any error — a query
that uses the index silently skips the rows. The review reproduced it: a store whose five newest
events are absent from every `events` index opened `healthy`, `timeline --since 1h | count`
answered 8 and `find event | count` answered 20 in the same shell, with no refusal and no gap.
v0.6.5's open refused that store. So since ADR-1068 such damage was detected *never*.

A whole-file check cannot go back into the open (#147), and ADR-1068 rejected the recorder's
maintenance turn because §31.8 bounds that turn and the check is not bounded work. What is left is
to run it somewhere that does not block anybody, at moments that are defined.

## Decision

1. **The store records the check.** Four metadata keys, each read in one primary-key step:
   `integrity_checked_at` and `integrity_checked_by` (instant and Ono version of the last check
   that *passed*), `integrity_failed` (what the last failed check found; present means the store is
   condemned), and `writer_open_since` (set by a recorder when it starts writing, deleted by a
   clean `stop recorder`).
2. **The check is due** (`LedgerStore::whole_file_check_due`, constant cost) when, in this order:
   a writer's mark is still set (an unclean shutdown — a crash, a kill, or a shell that ended
   without `stop recorder`); no check has passed (a new store, or one only an Ono before this ADR
   wrote); the last pass was by another version (an upgrade, a downgrade, or a migration); or the
   last pass is older than seven days (`WHOLE_FILE_CHECK_PERIOD`).
3. **The recorder runs it off the prompt.** `Recorder::start` asks whether it is due *before* it
   sets its own writer mark; the first maintenance turn at least 30 s after the start
   (`WHOLE_FILE_CHECK_GRACE`) begins it on a thread of its own, reading through a
   read-only connection of its own (`PRAGMA integrity_check(16)`), so the store's connection — every
   read and write the shell makes — is never held by it. `stop recorder` cancels it
   (`sqlite3_interrupt`) and waits for the thread; a cancelled or unfinished check records nothing
   and stays due. A running recorder whose last pass grows older than the period starts one from
   its maintenance turn. The open itself is unchanged: constant cost.
4. **A failed check condemns the store.** The finding goes into the store's integrity report as
   fatal and into `integrity_failed`. From then on every read and write through that handle is
   refused with `temporal.store_corrupt` naming the store and the guidance (`remove
   temporal-history`); the recorder becomes `running: false`, `health: failed`, with the refusal as
   its diagnostic; reads through the session (which shares the recorder's ledger) are refused
   rather than answered; and every later open refuses the store at open, as v0.6.5 did, at the cost
   of one metadata row. `at`/`inspect event`/`why event` over a shell whose recorder failed are
   answered with the recorder's own refusal (`temporal.store_corrupt` here,
   `temporal.store_unavailable` for a store that could not be opened or written).
5. **No new command.** The verb–target grammar has no command that fits an explicit check
   (`test` evaluates conditions over ports, paths and permissions; `inspect`/`get recorder` read).
   None is invented for it.

### What is detected when, and what is not

| Damage | Detected |
|---|---|
| a damaged root, first or last leaf of any b-tree | at open (ADR-1068), refused `store_corrupt` |
| a damaged page anywhere else | by the read that reaches it, refused `store_corrupt`; by the next due whole-file check |
| a table and its indexes disagreeing; a damaged freelist; any other `integrity_check` finding | by the next due whole-file check — after an unclean shutdown, a new store, another version or a migration, or a week after the last pass — and not before; the recorder appends to such a store until that check finishes |
| a row whose payload no longer decodes | by the read that meets it (a gap, ADR-0633) |
| damage made *by another process* to a store that was checked, stopped cleanly and is younger than a week | not until the period elapses |

Every recording shell that ends without `stop recorder` leaves its mark set, so in practice the
check is due at every recording shell's start. It begins only after the grace: a `-c` shell or
script that ends sooner never begins it and pays nothing, and an interactive shell begins it at its
first maintenance turn (after a command) half a minute in. Measured on the first version of this
change, which began the check inside `start`: on this 2-core host under a load of 5–6 the second
thread reading a 100k-event store cost a `-c 'start recorder'` about 60 ms of wall time (median
136 ms against 76 ms with the check not due), from contention rather than waiting — hence the
grace. A store only ever written by short scripts is therefore checked only when an interactive
shell, or a script running longer than the grace, records into it.

## Consequences

- Opening stays constant-cost. Measured as ADR-1068 measured it: release build, stores the shell
  built (`start recorder` then `stop process 4294967294` per line, four events each), `hyperfine -N
  --warmup 3 --runs 20`, `ono --no-config -c 'start recorder'`, medians; i7-6600U, 2 cores, load
  average 4.9–5.4 from another workstream building. "Not due": the writer mark removed before
  each run (`--prepare`); "due": the mark left set, as every recording shell leaves it.

  | events | ADR-1068 after | not due | due |
  |---:|---:|---:|---:|
  | 1,000 | 44 ms | 48 ms | 48 ms |
  | 20,000 | 53 ms | 70 ms | 54 ms |
  | 100,000 | 64 ms | 79 ms | 71 ms |

  The differences between columns are within this host's run-to-run noise (the 20k "not due"
  minimum is 50 ms); the open did not grow with the store. A `get recorder` that begins the check
  answers in 89 ms (debug build, 100k) and `stop recorder` cancels it in 19 ms.
- A recording shell finds index/table disagreement within one check of starting, rather than
  never; the open after that refuses the store.
- `crates/ono-temporal-ledger/tests/corruption.rs` pins: rows missing from an index are found by
  the check, the store is then unhealthy, refuses reads and appends as corrupt and is refused at the
  next open; middle pages damaged *before* the open leave the open succeeding and the read refusing
  (a whole-file check back in the open turns it red); due after an unclean shutdown and by age, not
  after a clean one; a cancelled check records nothing. `crates/ono-recorder/tests/lifecycle.rs`
  pins the recorder half: after an unclean shutdown the check runs, the recorder fails with the
  store named, appends and reads are refused as corrupt; a passed check and a clean stop leave
  nothing due.

## Alternatives considered

- **Bound the check by a deadline and give up when it runs out.** A large store would then never
  finish one; cancellation by `stop recorder` and by process exit is the bound instead, and an
  unfinished check stays due.
- **Run it in the open for small stores.** Gives #147's linear term back below the threshold; the
  1k-event open would cost twice what it does.
- **`PRAGMA quick_check`.** Does not cross-check indexes against tables, which is the damage this
  ADR is for.
- **A `test temporal-history` command.** A new command for one check, which the brief rules out;
  the recorder's checkpoints already cover every store that is being written.
