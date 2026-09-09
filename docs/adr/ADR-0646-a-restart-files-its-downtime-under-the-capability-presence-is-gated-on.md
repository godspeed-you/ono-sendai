# ADR-0646: A restart files its downtime under the capability presence is gated on

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §8.1, §9.5, §11.7, §21.5, §25.5, §43.2, §44.1, §44.2, §44.3, §55.5
- Decided by: agent (autonomous)

## Context

§44.1's five steps are a procedure, and the third is the one the other four exist to make honest:
"mark any unobserved downtime as a coverage gap". §55.5 names what happens when it is skipped — a
"silent gap" that reads as a quiet morning.

Two questions the steps leave open. *Which* capability the gap is filed under, and what a reboot
does to the sequence numbers step two just restored.

The first matters more than it looks. `ono-temporal-reconstruct` gates object presence on
`<type>.existence` coverage: a checkpoint plus events with no existence coverage over the window
reconstructs to `Presence::Unknown` rather than to a present object. A downtime gap filed under
some other name is a gap a reconstruction never consults, which is a silent gap with extra steps.

## Decision

### 1. The procedure runs in §44.1's order, at every start, before anything new is written

`downtime::restart` validates the store's metadata and version, reads back every source sequence,
computes the gaps, and reports `checkpoint_due`. `Recorder::start` calls it after opening the store
and before opening any coverage of its own, so nothing this run writes can be mistaken for
something the last run saw.

### 2. The downtime gap is one per source's `<type>.existence`, plus `temporal.events`

The interval is `RetentionState::latest` — the last event the store holds — to `now`. The source is
`ono.recorder`, not the provider: the provider was not unavailable, the recorder was not running,
and `ono_temporal_core::gap_detail` renders exactly that distinction as `recorder not running`,
which is the phrase §11.7 draws.

`temporal.events` goes beside them because the ledger already uses that name for a sequence break
and a corrupt segment, so a reader asking "what could this ledger say about events here" has one
capability to ask about whatever made the interval empty.

### 3. A boot boundary is reported, and no sequence is resumed across it

§44.2: "a host reboot creates a new boot clock domain ... monotonic/sequence semantics do not
cross the boot boundary unless a provider supplies explicit continuity."
`RestartPlan::resumable_sequence` answers only for a sequence whose `ClockDomain` is comparable to
this run's — same host *and* same boot — and `boot_changed` says when it is not. A source that
starts numbering at 1 after a reboot is then a fresh sequence rather than a hole after 4711.

### 4. Continuity is declared at every start, for exactly the sources that promised it

`LedgerStore::declare_contiguous` is what turns a hole in a source's numbering into a §43.2
coverage gap, and without the declaration a hole is not a loss. The recorder is the only component
that knows which sources promised continuity, so it declares one per profile whose
`exhaustive_events` is set, in this run's clock domain, at the instant of the start.
`Recorder::declared_contiguous` reports the list, so what was promised is inspectable.

A polled source is never declared, because §21.5 will not let it claim the flag in the first place
(ADR-0645).

### 5. A store that will not open is §44.3, not an error

§44.3: "if migration cannot complete safely, current shell operation continues with temporal
persistent functionality disabled and an explicit diagnostic." `Recorder::start` therefore answers
`Ok` with `health: failed`, no store, the session ledger still in place and
`StartOutcome::diagnostic` set. The store's own migration is `LedgerStore::open`'s (ADR-0632); what
this crate adds is the graceful degradation around it.

### 6. Gaps are written down, and read back from the markers rather than recomputed

Each gap becomes a coverage interval with `completeness: unavailable` and a `coverage.ended` event
carrying the whole gap in its payload (ADR-0643). `Recorder::gaps` reads the markers back rather
than inferring a reason from an interval, because a reason inferred from an empty interval is a
guess about why it is empty.

## Consequences

`at 12:15` over a stretch the recorder was down answers with `Presence::Unknown` for every object
type it collected, and a timeline over the same stretch draws `coverage gap: recorder not running`.
Both come from the same rows.

A restart always asks for a fresh checkpoint. Whatever the store holds was projected before the
interval nobody watched, so §9.1 has nothing nearer to stand on, and taking one is cheap relative
to reconstructing across the gap without it.

Tests: `crates/ono-recorder/tests/downtime.rs` — the interval, the detail, the capability, the
resumed sequence, the boot boundary, the fresh checkpoint, the selective continuity declaration,
and the gap reaching the ledger.

## Alternatives considered

- **File the downtime under `temporal.events` alone.** Rejected: it is the ledger's own capability
  and no reconstruction consults it for object presence, so the gap would be invisible exactly
  where §9.5 needs it.
- **Use the provider as the gap's source.** Rejected: it reads as "procfs was unavailable", which
  is false and which `gap_detail` would render as `linux.procfs unavailable`. The recorder was the
  thing that was not there.
- **Resume sequences across a reboot when the numbers look continuous.** Rejected: §44.2 forbids
  it without explicit provider continuity, and "the numbers look continuous" is precisely the
  coincidence §26.1 refuses to treat as evidence.
