# ADR-0643: An overflow gap is a disconnection whose words are `dropped events`

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §7.5, §11.7, §32.5, §43.1, §43.2, §43.3, §43.4, §55.5; ADR-0633
- Decided by: agent (autonomous)

## Context

§43.2 shows what an overflow must produce, rendered:

```text
coverage gap
  source    linux.netlink
  reason    dropped events
  interval  14:03:12.100 .. 14:03:13.411
```

`dropped events` is not one of §7.5's eight gap reasons, and §7.5's list is closed. So the example
is either a ninth reason the vocabulary has to grow by, or it is the *words* beside a reason the
vocabulary already has. `ono-temporal-core` answered the question while this work was in flight by
giving `TemporalGap` a `detail` field for exactly this — §11.7's own worked example is
`---- coverage gap: recorder offline 4m12s ----`, and no reason word spells `recorder offline`
either.

That leaves the reason. Of the eight, `not_recorded` says nothing was collecting, which is untrue:
the recorder was collecting and could not keep up. `provider_unavailable` says the source was not
there, which is also untrue: the source was there and produced more than the queue could hold.
`source_disconnected` says "the link or subscription dropped mid-interval", which is what
happened, from the consumer's end.

## Decision

### 1. A bounded queue drops; it never blocks and never grows

`Intake::offer` is `try_send` on a `tokio::sync::mpsc` channel with a capacity. §43.1 forbids the
unbounded channel and `xtask::scan::check_bounded_channels` refuses one in the tree; blocking the
producer instead was the other option and it is worse, because a netlink reader that is held stops
draining the kernel's own socket buffer and the loss moves somewhere nothing can see it.

### 2. The gap is `source_disconnected` with the detail `dropped events`

`Intake::overflow_gap` produces the §43.2 gap: the source, the `<type>.existence` capability the
loss actually costs, the interval from the first drop to the last, `GapReason::SourceDisconnected`
and `detail: Some("dropped events")`. A renderer prints the detail where it has one and the reason
otherwise, so §43.2's example comes out verbatim without a ninth reason word.

### 3. The interval is first drop to last drop, and it closes when the queue catches up

A gap running to the present would claim loss over an interval the queue has since been keeping up
with. `Recorder::note_overflow` writes the gap down and then clears the interval, keeping the
count: `dropped` in `ono.recorder-status/1` is cumulative and the gap is not.

### 4. The gap reaches the ledger as a `coverage.ended` event carrying the whole gap

§55.5 names the failure mode this avoids — a "silent gap" reads as a quiet morning. So the loss is
written down twice: as a coverage interval with `completeness: unavailable`, which is what a
composition reads, and as a `coverage.ended` event whose payload carries the capability, the
reason, the detail and both ends of the interval, which is what a timeline draws.

The whole gap rides in the payload because `ono.temporal-event/1` has no column for a capability,
a reason or an interval (INTERFACES §2.2 item 6). `normalize::gap_payload` writes it and
`normalize::gap_of` reads it back, together in one module for the same reason `relation_payload`
and `relation_of` are together in `ono-temporal-core`.

### 5. §43.4's health is `degraded`, and it is in the status rather than in a log line

A recorder that has dropped anything reports `health: degraded` and a non-zero `dropped`. §43.4
asks for a landmark visible at root `look`; the status record is where the count lives and the
landmark rule reads it.

### 6. Aggregation folds declared fields and can never fold a lifecycle change

§43.3 permits folding "high-frequency metrics-like changes ... when they are not semantically
relevant to exact topology" under two conditions: the rules are declared, and they "must not hide
object/relation lifecycle changes". `AggregationRules::declared` is the first condition as data.
The second is structural: `may_fold` returns false for anything that is not an `object.changed`
with a non-empty field list every one of whose fields is on the list, so an appearance, a
disappearance, a relation event and a change touching one undeclared field are unfoldable by
construction rather than by a check somebody has to remember.

The default list is the samples of continuously moving quantities — cpu, memory, byte and packet
counters, filesystem usage — which is §22.6's own point: "filesystem usage percentage is not a
default high-frequency time series."

## Consequences

`GapReason` did not have to grow, and §7.5's closed list stays closed. A reader who wants to know
*why* an interval is empty gets the word and the phrase, and a producer that has no phrase to add
sets `detail: None` and the reason stands alone.

Tests: `crates/ono-recorder/tests/backpressure.rs` — the bounded drop, the gap's source, reason,
detail and interval, the gap reaching the ledger, the degraded health, the folding rules, and the
interval closing when the queue drains.

## Alternatives considered

- **A ninth `GapReason::DroppedEvents`.** Rejected: §7.5's list is closed and shared with the
  ledger, the reconstruction engine and two contracts, and `detail` exists precisely so a producer
  can say more than the reason without the vocabulary growing per producer.
- **`not_recorded`.** Rejected: it is what §44.1's downtime gap is, and using the same word for
  "nothing was collecting" and "collecting and losing" would make the two indistinguishable in the
  one place the distinction matters.
- **Blocking the producer until there is room.** Rejected: see §1 above, and §32.5's batching
  answers the throughput question the block was meant to answer.
