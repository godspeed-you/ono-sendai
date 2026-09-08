# ADR-0680: A change found by comparing two observations carries the interval, not a point

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.5 §3.3, §6.2, §9.2, §18.1, §39.1, §39.2; v0.4 §25.1, §25.2, §25.3, §25.4;
  v0.2 §18.2; ADR-0024
- Decided by: agent (autonomous)

## Context

v0.5 §18.1 forbids two event models: "There MUST NOT be live diff model A / historical event
model B with subtly incompatible semantics." §39.1 keeps `ono-spatial-events` responsible for live
merge and snapshot diff and asks for "a small adapter seam from its canonical change output into
`ono-temporal-core` events".

That seam could not be built, because the canonical change output had no time in it.
`SpatialChange` carried a kind, a subject, a label and the places it touched. `EventMerge::absorb`
read the v0.2 watch envelope — `kind`, `at`, `changed`, `source` — and kept the first and the
last, discarding the instant the provider stated and the field list §6.2 calls `changed_fields`.
Anything downstream turning a change set into a temporal event therefore had to invent a
timestamp, which §3.3 forbids by keeping source time, observed time and ingestion time separate.
`crates/ono-cli/src/spatial/map.rs` shows what inventing it looks like in practice: it filled the
required `observed_at` of `ono.spatial-change/1` with `Timestamp::now()`, dating the record by
when it was built rather than by when anything was observed.

Restoring a timestamp is not enough, because the two ways a change is discovered know different
things. A provider that announces a change states the instant it saw it. A comparison of two
projections knows only that the difference arose after the earlier observation and by the later
one; §9.2 is explicit that between two observations Ono MUST NOT claim the state at a moment in
between. A single `Option<Timestamp>` cannot hold both answers: whatever instant it held for a
comparison would be a claim no evidence supports.

## Decision

### 1. `ObservedAt` is a three-way answer, and `SpatialChange` carries one

```rust
pub enum ObservedAt {
    At(Timestamp),                                   // a source announced it and said when
    Between { from: Timestamp, until: Timestamp },   // found by comparing two observations (§9.2)
    Unknown,                                         // nothing said when (§3.3)
}
```

`ObservedAt::instant()` answers `Some` only for `At`. A comparison has no instant to give, and the
type makes a caller that wants one confront that rather than receive a plausible number.
`earliest()` and `latest()` bound the change for a caller that needs to place it in a window.

`Unknown` is the third fact and never a substitute for the other two. An envelope that states no
`at` produces an event that states no `at`, and a change built from it says so.

### 2. `ChangeSet` carries the window it covers

`ObservationWindow { since, until }` is the period the set is a statement about. Outside it
nothing was looked at, so nothing outside it is claimed. `ObservationWindow::at(instant)` is the
degenerate window of a single observation — the opening value of a live stream, which compares to
nothing (§24.3) and covers the one moment it was made.

### 3. The observation instant belongs to the observation, not to the comparison call

`MapSnapshot::of` takes the instant from `SpatialMap::generated_at`; `PlaceSnapshot::of` takes it
from `Neighborhood::generated_at()`. `compare` and `compare_places` then read both ends of the
interval out of the two snapshots they are handed.

The alternative — passing two instants beside the two snapshots — was rejected for two reasons.
An instant handed in beside a snapshot can be handed in beside the wrong snapshot, and nothing
would catch it. And the baseline of `look --changes` is *stored* between two commands
(`SpatialSessionState::rebase`), so a design where the instant travels separately would have to
store the instant separately too, in session state that has nothing to do with time. An
observation that remembers when it was made survives being put down and picked up again.

No clock is read in either path: every instant arrives inside a projection the caller made
(§39.2, and the crate's own doc already promised it).

### 4. The observation instant is excluded from snapshot equality

`MapSnapshot` and `PlaceSnapshot` implement `PartialEq` by hand over the compared shape only.
v0.4 §25.2 makes change a property of the system — "Motion and visual updates MUST correspond to
actual topology or metric changes" — so time passing is not a difference. Two identical
projections an hour apart are equal and compare to nothing at all, which is the invariant §25.2
and §43.6 exist to protect.

## Consequences

- The seam §39.1 asks for is now buildable without a clock and without a fiction: a bridge reads
  `SpatialChange::observed()` and `ChangeSet::window()` and maps them onto `EventTimes`. Where the
  change is a `Between`, the bridge — not this crate — decides what an event with an uncertain
  observation time looks like, and it has the interval to say so with.
- A caller that wants one number from a comparison must now ask for `latest()` and know that it is
  asking "when was this *seen*" rather than "when did this *happen*". `map.rs` does exactly that
  for `ono.spatial-change/1`'s required `observed_at`, and the clock read there is gone.
- `ono.spatial-change/1` has no field for an interval and `ono.spatial-map/1` has none for the
  window. Both schemas are left untouched here; carrying the interval into structured output is a
  schema change for whoever owns those contracts.
- Constructing a `ChangeSet` now requires a window, so a caller with no observation time cannot
  build one. That is deliberate: it has not observed a period, so it cannot make a statement
  about one.
- Encoded by `crates/ono-spatial-events/tests/observation_time.rs` and by the two envelope tests
  in `crates/ono-spatial-events/tests/event_merge.rs`.

## Alternatives considered

- **`Option<Timestamp>` on `SpatialChange`.** Smallest change, and it makes the comparison case
  lie: any instant it carried would be a point inside an interval nothing was observed in, which
  is precisely §9.2's prohibition.
- **A timestamp plus a confidence word** (`exact` / `approximate`). Carries the same information
  in two fields that can disagree, and leaves the interval's other end unrecorded, so a consumer
  cannot bound the change at all.
- **Instants as parameters of `compare` / `compare_places`.** Rejected in §3 above: it separates
  an observation from its time at exactly the seam where the two get stored apart.
- **Deriving `PartialEq` including the instant.** Would make two snapshots of an unchanged system
  unequal while `compare` reports nothing, so the type would contradict the function.
