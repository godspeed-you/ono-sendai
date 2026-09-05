# ADR-0164: An edge explains itself, so `inspect relation` needs no second command

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §11.4, §11.5, §2.5, §2.6, §3.4, §3.5, §22, §23.5
- Decided by: agent (autonomous, phase S5)

## Context

§11.4 requires every displayed relationship to support inspection, spells one possible syntax —
`inspect relation @edge-17` — and then says "or equivalent structured selection". The result MUST
include relation, source, target, direction, provider, provenance, confidence, observed_at and
"raw evidence/reference where safe".

v0.2 already has `inspect`, and it inspects a *value*. A map edge is a value.

## Decision

**The equivalent structured selection is the edge itself.** `ono.map-edge/1` carries every field
§11.4 lists, so `map --json | from json | select edges` already answers the question, and
`inspect` over one of those records answers it in a terminal. No `inspect relation` target is
added: it would be a second spelling of a record the map already hands over, and §2.5's
requirement ("MUST expose why two objects are considered related") is met by the data rather than
by a command that fetches it again.

Two fields beyond §22's list, and both are required rather than optional:

- **`kind`: `hierarchy` or `relationship`.** §2.6 forbids confusing containment with an
  operational relationship and §3.4 says a hierarchical edge "MUST NOT assert operational
  dependency". A map draws both kinds, so it must say which each one is. A hierarchy edge's
  `relation` is the §3.4 grouping (`grouping`, `containment`), its confidence is `exact`, and its
  provider is `ono.spatial` — the canonical geography is asserted by the spatial layer itself
  (§4.1), not by a provider that never claimed it.
- **`source_label` and `target_label`.** §11.4's result must be inspectable *by a person*, and an
  edge whose two ends are opaque `SpatialId`s is not. The ids stay beside the names for anything
  that resolves them.

§23.5's "Inferred edges MUST be visually distinguishable from exact edges" is met twice over: the
`confidence` word travels on the edge, and the text renderer draws an inferred edge with a
different arrow (`~~>` against `-->`) rather than with colour, which §39.1 forbids relying on.

## Consequences

`spatial_relationships_missing::should_explain_every_edge_with_relation_provider_and_confidence_when_mapping_a_process`
is green against the map alone. A relationship edge keeps the provider and the confidence the v0.2
graph reported for the same fact (ADR-0146), so `map` and `trace` cannot disagree (§31.3, §2.16).

If a later phase wants the literal `inspect relation @edge-17` spelling, it is a lookup by edge id
over the same records, not a new source of truth.

## Alternatives considered

- *A separate `inspect relation` command* — rejected for now: it would need an edge registry to
  resolve `@edge-17` against, and every fact it could show is already on the edge.
- *Labels only, ids dropped* — rejected: §43.2's property is about ids, and two objects can share
  a name.
