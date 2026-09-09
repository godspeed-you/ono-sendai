# ADR-0635: A caller holds one ledger whether or not recording is on

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §10.2, §10.7, §30.2, §31.1, §32.1, §39; ADR-0621
- Decided by: agent (autonomous)

## Context

§10.2 makes persistent recording disabled by default. §10.7 gives every session a bounded in-memory
ledger regardless. §32.1 requires that with recording disabled the whole of v0.5 adds "less than 5
ms p95 to Ono interactive startup", and that "temporal storage initialization MUST be lazy when
recording is disabled".

`ono-temporal-core` already owns the in-memory ledger as `SessionLedger` (ADR-0621), and it already
satisfies `LedgerRead` and `LedgerWrite`. What was missing was one thing a caller can hold without
knowing which of the two it is, and a guarantee that holding it costs nothing when recording is off.

## Decision

`Ledger` is an enum over `SessionLedger` and `LedgerStore`, implementing both traits by delegation.
`Ledger::default()` is `Ledger::session()`, which constructs the in-memory ledger and touches no
filesystem: no directory is created, no file is opened, no SQLite library is initialised. Nothing
opens a store until `Ledger::persistent(&StoreOptions)` is called, which is what `start recorder`
does.

The session ledger is used rather than reimplemented. It is §10.7's own bounded ledger, it already
records its eviction boundary as a `TemporalGap`, and a second implementation would be a second set
of eviction semantics to keep in step.

`Ledger::sweep` on a session ledger removes nothing and says it is complete, because the ceiling is
enforced on every append. `Ledger::store()` answers `None` when recording is off, so a caller that
genuinely needs the store — the recorder, `get recorder`, `remove temporal-history` — asks for it
and is told plainly.

## Consequences

Nothing above this crate branches on whether recording is on in order to read history; it holds a
`Ledger`. The branch that remains is the one that matters: opening the store is an explicit act.

§32.1's budget is met by construction rather than by measurement, and
`tests/persistence.rs::should_touch_no_filesystem_when_recording_is_disabled` asserts the outcome
that makes it reachable — after appending through a default `Ledger`, neither the store nor its
directory exists.

The path is resolved by `ledger_path(env, home)` with both the environment and the home as
parameters, mirroring `ono-cli`'s `local_sources`. A test needs neither a real environment nor a
real home, and §31.1's canonical path is a test rather than a comment.

The directory is created `0700` and the database `0600` before SQLite opens it, rather than narrowed
afterwards. §30.2 gives the modes; creating them that way is what closes the window in which the
file was readable. A zero-length file is a valid empty database, so making it first costs nothing,
and SQLite gives the write-ahead log and the shared-memory file the mode of the database they belong
to, so they follow.

Tests: `tests/permissions.rs` (real filesystem metadata for the directory, the database and every
file beside it), `tests/persistence.rs` (laziness, and the retention boundary).

## Alternatives considered

**`Box<dyn LedgerRead + LedgerWrite>`.** Rejected: it hides `store()`, which the recorder needs, and
it costs a virtual call on every read for no gain — there are exactly two implementations and both
ship here.

**A single `LedgerStore` with an in-memory SQLite backend for the disabled case.** Rejected: opening
an in-memory database still initialises SQLite and runs the migrations, against §32.1's lazy
requirement, and it would duplicate §10.7's eviction semantics in SQL.

**Opening the store on first write rather than on `start recorder`.** Rejected: §10.2 makes
recording an explicit choice, and a store that appears because something happened to write is a
store the user did not ask for.
