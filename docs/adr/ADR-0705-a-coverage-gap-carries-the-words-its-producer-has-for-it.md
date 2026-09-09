# ADR-0705: A coverage gap carries the words its producer has for it

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §7.5, §10.8, §11.7, §35; v0.2 §35.3; ADR-0701
- Decided by: agent (autonomous)

## Context

§11.7 writes the line a timeline draws for a gap:

```text
12:20:00        ---- coverage gap: recorder offline 4m12s ----
12:24:12
```

`recorder offline` is not a reason word. §7.5's reason vocabulary is
`not_recorded retention_expired provider_unavailable permission_denied source_disconnected
clock_uncertain corrupt_segment unsupported`, and none of them says it. `docs/contracts/schemas/temporal-gap.v1.yaml`
has declared a nullable `detail` since the contract was written — "what a renderer adds to the
reason, `recorder offline` in §11.7's own example" — and `ono_temporal_core::value::gap_record`
never set it. Every production gap therefore rendered its bare reason with the underscores
replaced: `provider unavailable`, where §11.7 asks for `recorder offline`.

The renderer already prefers `detail` over `reason` (ADR-0701). Nothing was ever putting one there.

## Decision

**1. `TemporalGap` carries `detail: Option<Arc<str>>`, and `gap_record` writes it.** The phrase is
data on the gap rather than a rule in a renderer, so `timeline`, the §18.6 gap frame, the §16.6
`coverage gap` section and `temporal.coverage_gap`'s message all say one thing.

**2. The producer that knows why the interval is empty writes the phrase.** A gap is made in four
places, and each states what it knows: `CoverageSummary::compose` composes intervals and knows the
source and the reason; `SessionLedger` evicts and says so; the ledger's integrity scan knows a
segment is corrupt; reconstruction knows it refused for want of historical structure evidence.

**3. The composition table is `coverage::gap_detail(source, reason)`, and it is small on purpose.**
It states a phrase only where §7.5 and §10 give one — the recorder's own absence (§10.8 makes it a
process that can be stopped), retention, and a named source that dropped or was unavailable — and
answers `None` otherwise. `None` is not a defect: a reason word on its own is honest, and an
invented phrase is not (v0.2 §35.3).

**4. A renderer still falls back to the reason.** `detail` is nullable and a record from an older
producer carries none, so the fallback is the contract rather than a leftover.

## Consequences

- §11.7's worked example is reproduced by the production path rather than only by a fixture.
- `TemporalGap` gained a field, so the four literal construction sites outside `ono-temporal-core`
  state it. Each states what it knows; none states `None` for want of a better idea.
- `gap_detail` is public, so a producer outside the crate — the recorder, a provider — reaches the
  same phrases rather than writing its own.
- The phrase is English and untranslated, like every other string this tree renders.

## Alternatives considered

- **Derive the phrase in the renderer from `source` and `reason`.** Rejected: the renderer would
  need the §7.1 source vocabulary, which is exactly the knowledge §39.3 keeps out of it, and two
  renderers would derive differently.
- **Make `detail` required.** Rejected: it is additive-only as a nullable field, and a producer
  with nothing to add would have to repeat the reason as prose.
- **Put the phrase in `capability`.** Rejected: `capability` is the state class that went
  uncovered and is compared against `ono.temporal-coverage/1`'s own.
