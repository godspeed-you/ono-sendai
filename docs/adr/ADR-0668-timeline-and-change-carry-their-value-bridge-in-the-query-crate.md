# ADR-0668: `Timeline` and `TemporalChange` carry their value bridge in the query crate

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §11.4, §13.2, §28.1, §35, §36.4, §39; INTERFACES §1.12, §2
- Decided by: agent (autonomous)

## Context

The lead's interface contract puts every temporal-to-Ono-value conversion in
`ono_temporal_core::value`, so that "no other crate re-invents the field names" and §36.4's drift
check has one producer to compare against `docs/contracts`. That rule was written for the types
`ono-temporal-core` owns.

`ono.temporal-timeline/1` and `ono.temporal-change/1` are registered schemas whose Rust types are
not in core. §39 puts timeline planning and changes in `ono-temporal-query`, and `Timeline` and
`TemporalChange` are computed answers rather than ledger records — a timeline has no identity at
all, and a change's identity is a digest of the answer (ADR-0663).

## Decision

**A type's record bridge lives with the type.** `Timeline::to_record` and
`TemporalChange::to_record` are in `ono-temporal-query`, and every nested piece goes through
`ono_temporal_core::value` — `event_record`, `gap_record`, `coverage_summary`, `spatial_ref`,
`field_change`. Only the two top-level field lists are spelled here, once each, and every shared
sub-record still has exactly one producer.

## Consequences

- The two schemas gain a producer in the crate that computes them, which is where a drift between
  the computation and the contract would show up first.
- The core rule is not weakened: a field name that appears in more than one schema is still spelled
  in one place, because those are all sub-records core already produces.
- If the lead prefers these in `ono_temporal_core::value`, moving them is mechanical — the
  functions take `&self` and a schema id and nothing else. Flagged to the lead rather than decided
  silently.

## Alternatives considered

- **Putting them in `ono-temporal-core`.** It would mean core knows the `Timeline` and
  `TemporalChange` types, which §39 places in `ono-temporal-query`, so core would depend on the
  crate that depends on it, or the types would move down into a crate that plans nothing.
- **Returning a `MapValue` and letting the renderer build the record.** §39.3 forbids a renderer
  from deciding a data contract, and §28.1 wants a schema-bound record a pipeline can filter.
