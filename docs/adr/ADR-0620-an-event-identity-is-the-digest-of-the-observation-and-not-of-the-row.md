# ADR-0620: An event identity is the digest of the observation and not of the row

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.5 §3.3, §5.5, §6.7, §6.8, §17.3, §25.5, §42; v0.4 §3.1, §10; ADR-0129
- Decided by: agent (autonomous)

## Context

v0.5 §6.8 requires the ledger to deduplicate "where sources provide stable sequence IDs or event
IDs" and forbids the obvious shortcut in the same breath: "deduplication MUST NOT collapse two
distinct source events merely because their rendered text is equal". §3.3 adds that an event "MUST
have stable identity independent from its rendered timeline row".

Two things follow. Deduplication cannot be a comparison of what a timeline draws, and identity
cannot be a counter, because a counter gives one observation two identities as soon as two sources
report it or as soon as a recorder restarts and re-ingests a journal it has already read.

The spatial layer already answered the same question for objects: `SpatialIdentity::spatial_id` is
a SHA-256 over named components, with `0x1f` between parts and `0x1e` between a name and its value,
rendered as a hex prefix (ADR-0129). The question here is what the components of an *observation*
are.

## Decision

`EventId::of(&EventSeed)` is a SHA-256 content digest, in the spatial layer's own shape, over
exactly these named parts:

```
source            the provenance provider that reported it
kind              the §6.1 top-level class
subject           resolved: the SpatialId; unresolved: the source and the text it used
source_time       when the source says it happened, or an explicit absence marker
observed_at       when the observing component saw it
source_sequence   the source's own number in its stream, or an explicit absence marker
host              the clock domain's host
boot_id           the clock domain's boot, or an explicit absence marker
body              a second digest over subtype, scope, related, before, after,
                  changed_fields and payload
```

The first twelve bytes are rendered as hex behind an `e`, so an id is `e` plus twenty-four
lowercase hex digits and a reference reads `@e1a2b…`.

Three consequences are the point of the decision:

1. **`ingested_at` is not in the digest.** When a ledger received a report is a fact about the
   ledger, not about the world. A recorder that re-reads a journal after a restart produces the
   same ids and §6.8's deduplication works across the restart rather than only within one run.
2. **The source is in the digest.** procfs and journald reporting one instant are two
   observations, and §3.4 keeps the source on the record precisely because they are. They
   deduplicate against themselves, never against each other.
3. **An unresolved subject digests differently from the resolved one it might have been.** §5.5
   requires an event Ono could not reconcile to stay visibly unresolved; making it collide with the
   resolved event would be the guess §5.5 forbids, performed by the hash function.

`EvidenceId`, `ActionId`, `CausalLinkId` and `CheckpointId` are built the same way over their own
components: an observation's source, instant, scope, subject and claim; a request's session,
instant, operation and target (§17.3 mints it *before* execution, so nothing from the result is in
it); a link's rule, class and two ends; a checkpoint's scope and instant. `ActionId` carries eight
bytes because §17.3's own example, `ono:a91f`, is a token a person quotes.

`EventId::parse` accepts a **shortened** reference — `e` followed by one to twenty-four hex digits
— because §11.6's `@e42` is a reference a later command has to be able to use. Resolving it is the
ledger's job, and a reference that names more than one event is `temporal.ambiguous_event` (§34
E1306), which is the error that would otherwise have nothing to raise it. The other four ids parse
only at full length: nobody types one.

## Consequences

- §6.8 is a property of the type rather than a routine in the store. Every ledger — the session
  one here, the SQLite one in `ono-temporal-ledger` — deduplicates identically, because they all
  compare the same identity.
- A correction is a new event (§6.7), and it necessarily has a different id, because it differs in
  content. There is no way to write a "corrected" event with the same identity, which is what
  append-only means when identity is content.
- Two ledgers that saw the same source report agree on identity without coordinating, which is
  what a remote link needs (§24.1) and what a KUANG/11 contribution needs (§37.3).
- The digest covers the `EventSeed`, so any future field added to an event changes every id. That
  is a schema break by construction, and it is visible: `ono.temporal-event/1` would become `/2`.
- Encoded in `crates/ono-temporal-core/tests/identity.rs` and in the deduplication property test
  in `tests/properties.rs`.

## Alternatives considered

- **A UUID per ingest.** Simple, and it makes §6.8 impossible: two reports of one observation get
  two ids, and the only remaining way to notice is to compare rendered text, which §6.8 forbids by
  name.
- **A digest over the whole event including `ingested_at`.** Deduplicates within one run and
  duplicates everything across a restart, which is the case that matters — a recorder restart is
  §44.1's normal path.
- **A digest over the rendered timeline row.** Directly forbidden by §3.3 and §6.8, and it would
  collapse two distinct events whose row happens to read alike.
- **A per-source sequence as the identity.** Only some sources have one (§21.5), so the identity
  would exist for netlink and not for procfs. The sequence is instead one component among several,
  which is where §25.4 wants it.
