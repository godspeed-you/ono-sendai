# ADR-0634: Retention sweeps one bounded batch and leaves nothing pointing at what is gone

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §7.3, §10.4, §12.3, §30.8, §30.9, §31.8, §33, §34, §53; ADR-0630
- Decided by: agent (autonomous)

## Context

§10.4 and §53 fix the policy: 24 hours or 512 MiB, "whichever bound removes data first". §31.8 fixes
the mechanics: "retention cleanup MUST run in bounded background work", and "deleting expired events
MUST also handle orphaned evidence/checkpoints/causal links without leaving invalid references".

Two things in that are decisions rather than transcriptions. What "bounded" means for a caller, and
what makes an evidence record an orphan.

The second is subtler than it looks. Evidence is cited by events, by causal links and by the
derivation chains of other evidence (§7.3). A recorder also writes evidence *before* the event that
will cite it. So "evidence nothing cites" cannot mean "delete on sight" — that would delete every
record between the two writes.

## Decision

### 1. A sweep removes at most one batch and reports whether the bounds now hold

`LedgerStore::sweep(now)` removes at most `RetentionPolicy::batch` events, default 2048, in chunks
of 256 each in its own transaction, and returns `Swept { …, complete }`. `complete: false` means
call again. A caller drives it from a timer and is never held for an unbounded time, which is what
§31.8 asks and what keeps §31.9's "checkpoints MUST not block the interactive prompt" true of
retention too.

`now` is a parameter. Nothing below the recorder reads the clock (§39.2), and a retention test that
cannot choose the instant is a test that measures the machine it runs on.

### 2. Both bounds are evaluated on every pass, and the one that removes data first wins

Age removes everything older than `now - max_age`. Size removes the oldest events until the store
fits, measured as SQLite's used pages times the page size — pages in use, not the file length, and
without the write-ahead log, which is a transient buffer whose size says when the last checkpoint
happened rather than how much history is retained. The chunk is returned to the filesystem before
the next one is chosen, so the bound is measured against reclaimed space and removal stops when it
should.

A size bound smaller than an empty store is unattainable, and a sweep with no events left reports
itself complete whatever the bound says. Reporting otherwise would ask a caller to sweep for ever.

### 3. Nothing may point at what has gone, and the rules differ per kind

- **Causal links** go with either end, by `ON DELETE CASCADE` on both `cause` and `effect`, and a
  belt-and-braces delete of any link whose ends are missing. §31.8's "without leaving invalid
  references" is then structural rather than remembered.
- **Join rows** — subjects, evidence citations, link citations, derivation chains — cascade from
  their owner.
- **Evidence** goes when it is older than the retained boundary *and* nothing surviving cites it:
  not an event, not a causal link, and not another record's derivation chain. The age condition is
  what protects evidence a recorder has written and not yet attached.
- **Checkpoints, actions and coverage intervals** go when they are older than the retained boundary,
  which is the oldest surviving event. A checkpoint older than every event is a projection nothing
  can be replayed from.

### 4. The retained boundary is what `at` refuses beyond, and expiry is a different fact from absence

`RetentionState::earliest` is the oldest surviving event, which is what §12.3's refusal names.
`Ledger::is_out_of_retention` is true only when there is retained history to be older than: a ledger
holding nothing cannot claim that something expired. §34 gives the two cases different codes —
`temporal.out_of_retention` says history existed and went, `temporal.not_recorded` says it was never
there — and this predicate is what keeps a caller from confusing them.

### 5. Removal is the whole ledger or nothing

`LedgerStore::remove_all` implements §30.8's `remove temporal-history`. §30.9 makes that the
granularity v0.5 offers, because removing one event from an evidence chain destroys the integrity of
every claim derived from it.

## Consequences

Raising `temporal.retention.max_age` does not resurrect expired data, because expired data is gone
from the store. §33 requires exactly that.

A caller that never sweeps grows past its bounds. Retention is work someone has to drive; the store
does not run a thread of its own, because a library that starts a background thread on open is a
library that has decided something about the process it is in.

`evicted` is kept in `metadata` and survives a restart, so §10.7's eviction counter means the same
thing on the persistent ledger as it does on the session one.

Tests: `tests/retention.rs` — age alone, size alone, both together, nothing dangling afterwards, a
batch that reports more to do, an unbounded policy that removes nothing, and `remove_all`.

## Alternatives considered

**Delete everything expired in one transaction.** Rejected: §31.8 says bounded, and a day of events
on a busy host is not a bounded transaction.

**Delete uncited evidence regardless of age.** Rejected: it deletes evidence between the write that
creates it and the event that cites it.

**Count the write-ahead log in the size bound.** Rejected: the bound would then depend on
checkpoint timing rather than on retained history, and a store could be over its bound with no
history in it.

**A background thread inside the store.** Rejected: the recorder owns the schedule, and §32.6
requires the work to be cancellable by the caller's own means.
