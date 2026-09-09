# ADR-0633: Corruption is bounded to a segment, and what is discarded becomes a gap

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §7.5, §10.8, §31.7, §34, §43.2, §55.5; ADR-0630
- Decided by: agent (autonomous)

## Context

§31.7 lists five obligations when corruption is detected, and they are five because each one is
separately breakable:

1. refuse to present affected history as valid;
2. identify the affected store or segment;
3. preserve current shell functionality;
4. offer diagnostic or repair guidance;
5. mark temporal coverage gaps resulting from discarded corrupt data.

Obligations 1 and 5 pull against each other in the obvious implementation. Dropping a row that will
not decode satisfies the first and silently breaks the fifth, and §55.5 names that failure mode:
"silent gaps". Obligation 3 rules out the other obvious implementation, which is to refuse the whole
store on the first bad byte.

There are also two different kinds of damage. `PRAGMA integrity_check` finds the kind SQLite can
see — a scribbled page, a broken B-tree. It does not find a row whose CBOR payload someone has
overwritten, because to SQLite that is a perfectly good blob.

## Decision

### 1. Damage the store cannot survive is refused at open, and names the store

`PRAGMA integrity_check` runs before any migration. Anything other than `ok` is a fatal finding, and
`LedgerStore::open` returns `temporal.store_corrupt` carrying the store path as metadata and, as
help, the guidance of obligation 4: the shell keeps working without persistent history, and `remove
temporal-history` discards the damaged store so recording can start again.

The shell above is unaffected, because a temporal store that will not open is a ledger that is not
there, and §10.7's session ledger is what a session has when there is no recorder.

### 2. Damage bounded to a row fails that row, and the query still answers

A payload that does not decode is refused rather than rendered with its broken fields blanked, which
is obligation 1. The query it was part of answers with the rows that did decode, which is obligation
3. The refusal is recorded as a non-fatal `IntegrityFinding` whose segment names the table and the
row identity — `events:e91c4a7d…` — which is obligation 2.

`IntegrityReport` therefore grows during reads rather than being fixed at open. That is not an
accident of implementation: obligations 2 and 5 are about *segments*, and a database SQLite opens
happily may still hold one unreadable payload, discovered only by the read that needed it.

### 3. Every discarded interval becomes a coverage gap with reason `corrupt_segment`

`LedgerStore::gaps` returns one `TemporalGap` per refused row, over the interval the row covered, in
the scope the row belonged to, with the `corrupt_segment` reason §7.5 defines. That is obligation 5,
and it is what stops §55.5's silent gap: a timeline over a damaged stretch draws a gap rather than a
quiet morning.

The same method also returns the gaps of §43.2 — a break in a sequence a source declared contiguous
— because a caller drawing a timeline wants every interval nothing can be said about, whatever made
it that way.

### 4. Nothing repairs a payload

There is no salvage path that reconstructs a partly-decodable event. §31.7 says refuse, identify,
keep working, guide and mark; it does not say guess, and a half-decoded event presented as history
is exactly the overstatement the whole specification is written against.

## Consequences

A store with one damaged row is a working store with a gap in it, which is the outcome §31.7
describes and the one an operator can act on. A store with a damaged page is a refused store and a
working shell.

`integrity()` returns a snapshot rather than a borrow, because the report is behind a lock that read
paths take. A caller that wants the current state asks again.

The guidance names `remove temporal-history` rather than a repair tool, because §30.9 makes whole-
ledger removal the granularity v0.5 offers and there is no repair Ono can perform that SQLite's own
recovery cannot.

Tests: `tests/corruption.rs` — a deliberately scribbled file refused with the store named and the
guidance offered, a scribbled payload that costs one row and produces one `corrupt_segment` gap
while the store stays writable, a truncated file, and a healthy store that reports itself healthy.

## Alternatives considered

**Refuse the whole store on any decode failure.** Rejected: it breaks obligation 3 for a fault
bounded to one row, and it turns a lost minute into a lost day.

**Drop the row silently.** Rejected: obligation 5, and §55.5 names it as a failure mode by itself.

**Quarantine the row into a side table for later repair.** Rejected as speculative: nothing in v0.5
repairs a payload, and a quarantine nothing reads is a table that only grows.
