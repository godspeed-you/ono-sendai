# ADR-0621: The ledger contract lives in core and the session ledger lives beside it

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.5 §6.7, §7.5, §10.7, §11.7, §39, §39.4, §43.2; ADR-0612
- Decided by: agent (autonomous)

## Context

§39 splits the temporal system into six crates and puts SQLite persistence in
`ono-temporal-ledger`, reconstruction in `ono-temporal-reconstruct` and query planning in
`ono-temporal-query`. §39.4 adds a prohibition: "`ono-temporal-core` MUST not expose SQLite types
or SQL semantics."

Read narrowly that is satisfied by any arrangement where core does not depend on `rusqlite`. Read
for what it is protecting, it says something stronger: reconstruction and query planning must be
expressible without a database, or the two crates above the store cannot be tested without one and
the store's shape leaks upward into their design.

Meanwhile §10.7 requires a bounded in-memory ledger *in every session*, recorder or no recorder,
holding 100 000 events by default. That is a second implementation of whatever the first one's
interface turns out to be.

## Decision

**The read and write contract is `ono-temporal-core`'s, and it names no store.**

`LedgerRead` and `LedgerWrite` speak only §3's vocabulary — events, evidence, coverage intervals,
causal links, actions, checkpoints, retention — plus `EventQuery`, `CoverageQuery`, `TimeRange`
and `QueryOrder`. No method mentions a connection, a statement, a transaction, a table, a file or
a path, and no type in the crate does either. `ono-temporal-reconstruct` and `ono-temporal-query`
depend on the trait; only `ono-temporal-ledger` depends on SQLite.

`LedgerWrite` has no method that rewrites an event, because §6.7 makes the ledger append-only:
"corrections are represented by new events or evidence records referencing the prior event". The
absence is the enforcement.

**`SessionLedger` — §10.7's bounded in-memory ledger — lives in `ono-temporal-core`, beside the
contract it implements.** It is not a test double; it is the ledger a default installation runs
with, since §10.2 disables persistent recording by default. Putting it here means the contract
ships with a working implementation, so nothing above it is written against an interface nobody has
yet satisfied, and a session with no recorder needs no dependency on the SQLite crate at all.

**Eviction is recorded, never silent.** When the ceiling is reached the oldest events go first, and
the ledger keeps a `TemporalGap` with reason `retention_expired` spanning what it dropped, plus a
`TemporalCoverage` interval marked `unavailable` over the same span so that
`CoverageSummary::compose` turns it into a gap for anything asking about that window. §7.5 makes a
gap a typed object and §11.7 forbids hiding one "simply because events exist on both sides"; a
ledger that dropped its morning quietly would make a long session's timeline lie by omission, in
exactly the way §43.2 forbids for a dropped queue.

The mutex behind `SessionLedger` ignores poisoning. The state is a queue of immutable events, so a
panic elsewhere leaves it exactly as valid as before, and turning every later temporal query into a
refusal would be a worse answer than the correct one.

## Consequences

- `ono-temporal-reconstruct` and `ono-temporal-query` can be developed and tested against
  `SessionLedger` with no database, no temporary directory and no migration.
- Two implementations of one contract exist from the start, which is the cheapest way to find out
  that a method signature had a store's assumptions baked into it.
- A remote or KUANG/11-contributed history provider (§24, §37.5) implements `LedgerRead` and
  nothing has to change above it.
- `SessionLedger` holds whole `TemporalEvent`s in memory. At the default ceiling that is bounded
  by §10.7's own number rather than by a byte budget; a session that needs a byte budget wants the
  recorder, which has one (§10.4).
- Encoded in `crates/ono-temporal-core/tests/session_ledger.rs`.

## Alternatives considered

- **The contract in `ono-temporal-ledger`.** Makes `-reconstruct` and `-query` depend on SQLite to
  see a trait, which is what §39.4 exists to prevent.
- **The session ledger in `ono-temporal-ledger` beside the SQLite one.** A default installation
  would then pull the store crate in to run with no store. It also separates the trait from its
  only free-standing implementation, so the trait stops being exercised where it is defined.
- **Evicting silently and reporting only a count.** A count answers "how much did I lose" and not
  "when"; §11.7's gap has to be drawable at a position on a timeline, which needs the interval.
- **Refusing to append past the ceiling.** Turns a busy minute into a failed command. §10.7 asks
  for a bounded ledger, not a full one.
