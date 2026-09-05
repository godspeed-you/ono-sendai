# ADR-0200: An edge carries what the provider saw, not only who saw it

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §11.4, §11.5, §22, §37.2, §44.4; ADR-0146, ADR-0164
- Decided by: agent (autonomous), delivering v0.4 §50 Phase S11

## Context

§11.4 lists what an inspected relationship MUST include, and the list ends with **"raw
evidence/reference where safe"**. ADR-0164 decided that the edge itself is the "equivalent
structured selection" §11.4 allows, so `ono.map-edge/1` and `ono.spatial-neighbor/1` are the
records that must carry the list.

They carried eight of the nine items. The v0.2 relationship graph already reports the ninth —
`linux.open-files` records `{fd: 3, access: "read"}` beside the fact, `linux.process-sockets`
records `{fd: 10, inode: 106355915}` — and `relations.rs` already copies that metadata onto the
`RelationshipEdge` as attributes. The projection into a neighbour and into a map edge dropped it,
so an edge said *who* observed it and never *what they saw*.

## Decision

**Both edge records carry an `evidence` field holding the provider's own detail.**

- `ono.spatial-neighbor/1`: `evidence`, nullable. Null for a hierarchical member — containment is
  not an observation (§2.6, §3.4) — and the provider's attributes otherwise.
- `ono.map-edge/1`: `evidence`, required. Empty for a hierarchy edge, which §4.1 has the spatial
  layer declare rather than observe, and the provider's attributes for a relationship edge.

The relation's own word keeps its own field: `provider_relation` is not repeated inside
`evidence`, so the map holds each fact once.

Nothing new is observed for this. The evidence is what the v0.2 provider already reported, carried
forward unchanged — §2.16 keeps the fact with the provider and §37.2 keeps the spatial layer from
reading anything out of text.

## Consequences

- `near --type file | take 1 | to json` and `map --json` both answer "why do you think these two
  are related" completely: the relation, both ends, the direction, the provider, the provenance,
  the confidence, when it was seen, and the descriptor it was seen through.
- A provider that records no detail yields an empty evidence map rather than an invented one.
- Encoded by `spatial_relationships_missing::should_carry_the_raw_evidence_of_an_edge_when_a_neighbour_or_a_map_edge_is_read`
  and by `docker/acceptance/cases/093-spatial-process-file-process.case` (`44.4h`–`44.4j`).

## Alternatives considered

- **A separate `inspect relation` command that fetches the evidence.** Rejected by ADR-0164 and
  rejected again here: the fact is already in hand when the edge is drawn, and fetching it twice
  is how two answers to one question start to disagree.
- **Fold the evidence into `provenance`.** Rejected: provenance is v0.2's record of who observed
  what and when, shared with every other value in the shell; the descriptor an edge was read
  through is about the edge, not about the observation's origin.
