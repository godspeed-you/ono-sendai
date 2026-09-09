# ADR-0666: The stronger evidence wins a merge, and a tie is broken by recency

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §9.1, §7.2, §7.3, §8.5, §21.4, §3.4
- Decided by: agent (autonomous)

## Context

§21.4 lets a provider answer directly about the past — journald holds its own history, a cluster
API keeps its own record — while §9.1 replays the ledger to reach the same instant. Both answers
are legitimate and they can differ. §7.2 fixes the one thing that must not happen: "evidence
strength MUST NOT be automatically upgraded", so a merge may choose between claims and may never
manufacture a stronger one. §8.5 requires the coverage that survives to be per field.

What the specification does not say is what happens when two sources are equally strong, and the
obvious implementation — first one wins, or last one wins — makes the answer depend on the order a
caller passed its arguments.

## Decision

**`merge_fields` decides per field, by `EvidenceStrength` first and never by source order.**

1. The stronger `EvidenceStrength` wins, using the ordering `ono-temporal-core` already defines.
2. On an exact tie, the answer observed closer to the question wins: two equally strong readings
   differ only in how recently they were taken.
3. On a further tie, the source name decides lexicographically. It is arbitrary and it is
   *deterministic*, which is the property that matters — the same inputs give the same answer on
   every run and on every host.
4. The winner keeps **its own** strength, its own source, its own coverage and its own evidence
   chain. Nothing is combined upward. Two sources agreeing does not make a claim authoritative.
5. `contributors` names every source that answered the field, so `inspect` can show the ones that
   lost, and `conflicting` records that two sources gave different values. A disagreement is a
   fact about the evidence rather than an error; §3.4 keeps the losing record addressable, so a
   reader can go and look.

## Consequences

- The merge is a pure function of two slices and cannot be made to prefer the replayed side or the
  provider side by argument order — the property a test asserts directly by swapping them.
- Per-field provenance survives, which is what §9.4's temporal metadata on a reconstructed object
  needs to be truthful about a record assembled from two sources.
- There is no operation here that raises a strength, matching `EvidenceStrength`'s own shape:
  `weakest_of` exists and nothing else does.
- Encoded by `crates/ono-temporal-query/tests/changes.rs`.

## Alternatives considered

- **Provider answer always wins.** Reasonable-sounding and wrong: a polled provider's snapshot is
  `observational` and the recorder may hold an `authoritative` transition the provider has already
  forgotten.
- **Combining two agreeing sources into a stronger claim.** Exactly what §7.2 forbids.
- **Leaving a tie to argument order.** Cheap, and it makes the answer depend on a call site.
