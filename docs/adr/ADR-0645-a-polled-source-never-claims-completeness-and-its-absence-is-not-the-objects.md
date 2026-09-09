# ADR-0645: A polled source never claims completeness, and its absence is not the object's

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §6.3, §7.4, §8.1, §8.3, §21.5, §22.1, §43.2, §44.1
- Decided by: agent (autonomous)

## Context

§22.1 gives the recorder a licence and three conditions in one sentence: it "MAY create process
appearance/disappearance events by comparing complete-enough snapshots, but provenance MUST say
`snapshot_diff` and coverage MUST reflect polling limitations."

§6.3 adds the condition that is easiest to lose, because it is about the case where nothing
happened: a polling gap MUST NOT emit a disappearance unless the provider contract makes
missing-from-a-complete-snapshot meaningful. §7.4 is the same rule from the evidence side — an
absence is a claim, and a claim needs coverage that can carry it.

The failure this prevents is specific and bad: a recorder that was busy for a minute comes back,
finds a process it saw before is no longer in the snapshot, and writes `object.disappeared`. The
timeline then says a process died at an instant it was alive, and every causal explanation
downstream is built on it.

## Decision

### 1. `SourceProfile::polled` clears `exhaustive_events`, and no builder puts it back

§21.5: "providers MUST NOT advertise `exhaustive_events` merely because events usually arrive."
A source that is asked every five seconds has no sequence continuity to support the claim, so
`polled` clears the flag, `with_capabilities` re-clears it for a polled profile, and
`exhaustive(true)` on a polled profile answers no rather than erroring. `completeness()` is
therefore `partial` for every polled source and `complete` only where a provider advertised the
strong claim, which is what §8.3 means and what `CoverageSummary::can_prove_absence` reads.

### 2. Coverage carries the sampling interval

`Delivery::Polled { interval }` is the sampling interval on every coverage row the source
produces, and `Delivery::Subscribed` produces `None`, which §3.5 defines as an event stream. §22.1's
"coverage MUST reflect polling limitations" is then a field rather than a caveat.

### 3. A disappearance needs three things, and any one missing makes it a gap

`SnapshotDiff::observe` emits `object.disappeared` only when all three hold:

1. the provider said the snapshot was **complete** — a partial read says nothing about what is
   missing from it;
2. the profile declares `meaningful_disappearance` — §6.3's "unless the provider contract makes
   missing-from-a-complete-snapshot meaningful", as a declaration rather than an inference;
3. the round followed the previous one within `MISSED_ROUNDS_BEFORE_GAP` polling intervals — one
   interval is the cadence, two is a late round, and beyond that the recorder was not looking.

Where any fails, the object stays in the differ's memory and the interval becomes a `TemporalGap`
instead. §43.2's preference applied to a polling source: an explicit gap over pretended
continuity, in the direction where the wrong answer is a process the timeline says died and did
not.

### 4. The provenance carries two names, and they answer different questions

`provenance.provider()` is the §7.1 evidence source class — `linux.procfs` — because
`ono_temporal_reconstruct` reads it to attribute a reconstructed field to a source, and an
unrecognised name falls back to `ono.session`, which would credit the shell with what procfs saw.
`provenance.source()` is `snapshot_diff` for a derived event and the provider's own id otherwise,
which is §22.1's requirement and §22.3's for the adapter path.

### 5. Existence coverage is filed under the name reconstruction looks it up by

`ono_temporal_reconstruct::capability::{existence, field, relation}` builds every capability name
and nothing in this crate spells one. Object presence is gated on `<type>.existence`: a checkpoint
plus events with no existence coverage reconstructs to `Presence::Unknown` rather than to a
present object, so `Recorder::checkpoint` adds the interval for every type the capture carries and
for every source the recorder holds, and `SourceProfile::existence_capability` is what an intake,
a coverage marker and a gap all use.

## Consequences

A recorder over procfs alone reports `process.existence` as `partial`, so
`ReconstructedWorld::can_prove_absence` refuses, and "was there a process X at 12:05" answers
`Unknown` rather than "no". That is the honest answer and it is §1.3's whole point: "history is
not omniscience."

A provider that genuinely can support absence says so by declaring `exhaustive_events` and
`meaningful_disappearance`, and the recorder then believes it. The claim is the provider's, made
once, inspectable through `get temporal-source`.

Tests: `crates/ono-recorder/tests/sources.rs` (all four disappearance cases, the `snapshot_diff`
provenance, the evidence source class, the sampling interval),
`crates/ono-recorder/tests/checkpoints.rs::should_record_existence_coverage_when_a_checkpoint_is_written`.

## Alternatives considered

- **Emit the disappearance and mark the coverage partial.** Rejected: a partial coverage row does
  not stop a rendered timeline from drawing a death, and §6.3 is a prohibition on the event rather
  than a caveat beside it.
- **Infer `meaningful_disappearance` from `exhaustive_events`.** Rejected: they are different
  claims. A source can number its events contiguously and still not be the authority on what is
  absent from a snapshot, and §21.5 already makes the strong flag hard to earn.
