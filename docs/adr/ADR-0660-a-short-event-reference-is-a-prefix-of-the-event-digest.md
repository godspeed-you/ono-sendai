# ADR-0660: A short event reference is a prefix of the event digest

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §11.6, §12.2, §20.4, §34; ADR-0620
- Decided by: agent (autonomous)

## Context

§11.6 requires a rendered event to expose a reference a later command can use: `inspect event
@e42`, `at event @e42`, `why event @e42`. The identity it stands for is an [`EventId`], which
ADR-0620 made a SHA-256 content digest rendered as `e` plus twenty-four hex digits. Nobody types
that at a prompt, and §11.6's own example is four characters.

So a short form has to be allocated. There are two families of answer: a per-session counter
(`@e1`, `@e2`, …), or a prefix of the digest. The counter is shorter and reads better; it also
means a reference is meaningless outside the session that issued it, and `at event @e42` in a
script or a pasted transcript resolves to a different event or to nothing. The spec's own list of
refusals says which family it expected: `temporal.ambiguous_event` (§34) exists to answer "a
shortened reference names more than one event", and only a prefix can be ambiguous.

## Decision

**A short reference is the shortest prefix of the event's own digest that names one event, with a
floor of two hex digits.** `EventReferences` in `ono-temporal-query::search` mints them.

1. **Allocation.** The first time a session shows an event, it takes `e` plus two hex digits. If a
   reference already issued in this session is a prefix of that, or that of it, the new event takes
   one digit more, and so on until the two are distinguishable. The event it collided with keeps
   the reference the reader has already seen: **an issued reference never changes meaning inside a
   session.** The same event asked for twice gets the same reference.
2. **Resolution.** `resolve` looks in the session table first, so a reference the reader was given
   resolves to the event they were shown. Anything else is handed to `LedgerRead::event`, which
   already resolves a reference by prefix and already raises `temporal.ambiguous_event` naming the
   candidates. A reference that names no held event is `Ok(None)` — a fact rather than a failure —
   and text that is not an event reference at all is `temporal.invalid_time`.
3. **Nothing is stored.** No counter, no allocation table in the ledger, no coordination between
   sessions. The short form is derived from the event, so two sessions on two hosts reading one
   ledger issue compatible references without agreeing on anything.

## Consequences

- A reference survives its session. Pasting `@e42` from yesterday's transcript resolves against the
  ledger, and where the ledger has since grown an event sharing that prefix, the answer is
  `temporal.ambiguous_event` listing both — which is the honest answer and the one §34 has a code
  for. A counter scheme would silently resolve to the wrong event.
- A busy session's references grow a digit at a time rather than all at once. A session showing a
  handful of events sees `@e42`-shaped references, which is what §11.6 draws.
- The floor of two digits is a readability choice rather than a correctness one; one digit would
  work and reads as a typo.
- §20.4's completion reuses the same allocator, so a candidate list offers the reference the
  session will resolve.
- Encoded by `crates/ono-temporal-query/tests/search.rs`: resolution both ways, stability under a
  colliding prefix, the ambiguity refusal, and a reference the session never issued.

## Alternatives considered

- **A per-session counter.** Shortest and most readable, and it makes a reference a session-local
  name that looks global. `at event @e42` in a saved command means a different event tomorrow.
- **A prefix with no session table.** Simpler, and it lets an already-shown reference start naming
  two events as the session goes on. The table exists only to stop that.
- **Allocating in the ledger.** It would make short forms globally unique and put a write on the
  path of drawing a timeline, which §32.3's budget cannot absorb and §4.7's read-only historical
  context forbids outright.
