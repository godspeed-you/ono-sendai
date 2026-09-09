# ADR-0631: `synchronous = NORMAL` is the durability the specification asks for

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §31.1, §31.5, §10.8, §32.4, §32.5, §44.1
- Decided by: agent (autonomous)

## Context

§31.5 states the durability contract in two sentences: "default durability SHOULD tolerate process
crashes without losing committed events", and "a sudden power loss may lose the most recent
unflushed interval but MUST not corrupt prior committed history".

Those two sentences are not the same requirement, and SQLite offers a setting for each. In WAL mode:

- `synchronous = FULL` fsyncs the write-ahead log on every commit. Nothing is lost on a power cut.
- `synchronous = NORMAL` fsyncs the log at a checkpoint rather than at every commit. A process crash
  loses nothing, because the log is a file the kernel holds whether or not Ono is alive. A power cut
  may lose the transactions written since the last sync, and cannot corrupt what came before,
  because WAL recovery replays whole frames and a torn frame fails its checksum and is discarded.
- `synchronous = OFF` may lose a committed transaction to a *process* crash, which §31.5 forbids.

`NORMAL` is the first sentence exactly and the second sentence exactly. `FULL` is stronger than
asked, and the price is a device flush per commit against §32.5's batching and §32.4's 1%-of-a-core
budget on a machine that may be recording continuously.

## Decision

The store opens with `PRAGMA journal_mode = WAL` and `PRAGMA synchronous = NORMAL`, and refuses to
open if WAL was not granted — §31.1 makes WAL normative, so silently running in rollback-journal
mode would be a quieter failure than a refusal.

`Durability::Full` is offered through `StoreOptions::with_durability` and is not the default. An
installation on unreliable power may want the stronger setting; §31.5 does not require it, and
imposing it would cost every installation for the benefit of some.

`flush()` runs `PRAGMA wal_checkpoint(TRUNCATE)`. §10.8 requires `stop recorder` to flush, and a
truncating checkpoint is what makes the committed history durable and readable by a second process
rather than merely committed. Retention checkpoints too, before it measures, because otherwise the
pages it freed would still be on disk and the size bound would never be reached.

`busy_timeout` is five seconds. Two Ono processes may hold the same store — a shell and the recorder
— and a writer that refuses instantly on contention would report `temporal.store_unavailable` for
something that resolves itself in milliseconds.

`PRAGMA foreign_keys = ON`, so §31.8's "no invalid references" is enforced by the store rather than
by remembering to delete in the right order.

## Consequences

A `kill -9` of the recorder, an out-of-memory kill, or a panic loses nothing that was committed. A
power cut may lose up to one checkpoint interval of events, and §44.1's restart procedure turns that
into a coverage gap rather than into pretended continuity — which is the honest outcome and the one
§43.2 asks for.

The measured size of the store excludes the write-ahead log, because the log is a transient buffer
whose size depends on when the last checkpoint happened rather than on how much history is retained.
Retention checkpoints before it measures, so the two never disagree for long.

Tests: `tests/persistence.rs` (events survive a close and a reopen, with every field),
`tests/corruption.rs` (a store with one refused row is still writable).

## Alternatives considered

**`synchronous = FULL`.** Rejected: stronger than §31.5 requires, at a device flush per commit
against §32.4's and §32.5's budgets. Available as `Durability::Full` for an installation that wants
it.

**`synchronous = OFF`.** Rejected outright: it can lose a committed transaction to a process crash,
which is the one thing §31.5 says must not happen.

**A rollback journal.** Rejected: §31.1 makes WAL normative, and a rollback journal also blocks
readers during a write, which §31.9 forbids for checkpoints.
