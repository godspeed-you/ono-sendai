# ADR-0162: The `SpatialMap` as records — what §22's contract becomes on the wire

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §22, §6.9, §8.2, §23.4, §29.4, §43.2, §53
- Decided by: agent (autonomous, phase S5)

## Context

§22 gives the `SpatialMap` shape as a "recommended contract" in pseudo-types: `Uuid`, `SpatialId`,
`ObjectRef`, `TypeId`, `EdgeId`, `ClusterId`, `HiddenSummary`, `Completeness`. v0.2's public
surface is `docs/contracts/schemas/*.yaml`, so the pseudo-types have to become five concrete schemas or
`map --json` has no contract a consumer can hold anyone to. §22 also forbids one thing outright:
"Screen coordinates MUST NOT appear in the semantic `SpatialMap` contract."

## Decision

Five schemas, and `map --json` writes the first of them:

- `ono.spatial-map/1` — every field §22 names, in §22's own spelling, plus `focus`.
- `ono.map-node/1`, `ono.map-edge/1`, `ono.map-cluster/1`, `ono.hidden-summary/1`.

Five points where §22 left the shape open:

1. **`focus` sits beside `center`.** §23.4 and §53 both turn on the two being different things
   ("Does focus move the shell? No"), and a contract in which they are the same field cannot say
   so. `center` is the current place; `focus` is the node the view is centred on, or null.
2. **`map_id` is derived from the map, not drawn at random.** It is a UUID (version 8, RFC 4122
   variant) over the centre, the zoom level, the generation instant and the drawn node ids. Two
   identical projections therefore carry one id, and no dependency is added for randomness.
3. **A cluster id and an edge id are not `SpatialId`s.** A `SpatialId` is opaque and is minted only
   from an identity (ADR-0129); a cluster and an edge are things the *map* draws, not objects the
   system has. They are `cluster:<digest>` and `edge:<digest>`, hashed from the grouping key and
   from the endpoints — stable between two runs, which is what `--expand <cluster>` needs (§8.3)
   and what `--relations` filtering is checked against (§43.2).
4. **An edge endpoint is an id as text, because §8.2 lets a cluster stand for an object.** An edge
   to something the budget clustered points at the cluster; §43.2's property ("all rendered edges
   reference existing rendered nodes or explicit off-map endpoints") is satisfied by the cluster
   being explicit, not by inventing an endpoint.
5. **`hidden` counts three different things**, because they mean different things to a reader:
   `clustered` is one command away, `aggregated` is visible inside a coarser node, and `count` is
   the whole of what is not drawn as its own node.

A node carries `space`: the registry id of the canonical space it *is*, or null for an observed
object. That is what lets `spec-check`'s third party — what the commands actually serve — be held
against `docs/contracts/spatial/spaces.yaml` in both directions.

## Consequences

`map --json` is one JSON document, readable with `from json` like any other (§29.4). No field at
any depth names a row, a column, a width or a position, and the property test in
`spatial_map_missing::should_omit_screen_coordinates_when_map_json_returns_the_semantic_contract`
holds the whole document to it. The renderer receives the same record the JSON is made of, so the
text map and `--json` cannot drift.

Adding a field to `ono.spatial-map/1` later is a schema version bump, which `spec-check` enforces.

## Alternatives considered

- *A hand-rolled JSON writer* — rejected: the map would then be the only command whose output is
  not a schema-bound record, and `map --json | from json | where …` would have no contract.
- *`map_id` as a random UUID* — rejected: it needs a dependency, and a random id makes two
  identical projections look different for no reader's benefit.
- *Cluster ids as `SpatialId`s* — rejected: it would mean minting an identity for something the
  system does not contain, which is exactly what ADR-0129 keeps `SpatialId` closed against.
