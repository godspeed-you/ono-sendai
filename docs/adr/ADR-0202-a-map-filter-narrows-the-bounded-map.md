# ADR-0202: A map filter narrows the bounded map, it does not re-select what fills it

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §6.9 (`map`), §8.2 (clustering), §22 (the map contract), §34.2 (view budgets),
  §43.2 (the properties), §2.9 (the horizon is bounded); `docs/ACCEPTANCE.md` §4.7.2, §4.7.4
- Decided by: agent (autonomous)

## Context

v0.4 §43.2 requires the property "filtering cannot create unknown edges", and
`docs/ACCEPTANCE.md` §4.7.2 requires it as a seeded property test. Writing it made a defect
`docs/STATE.md` had recorded from a busy host reproducible at a fixed seed:
`ono_spatial_query::map::project` filtered the *candidates* by `--type` and then bounded what
was left to `spatial.map.node_budget`. On any horizon larger than the budget the two runs
therefore ranked two different populations, and `map --all --type listener` drew listeners the
unfiltered `map --all` had cut — a filter that *adds* nodes, and with them edges the unfiltered
map never held.

The two readings of what `--type` means are mutually exclusive on a bounded view:

- a filter *selects*: it asks the whole horizon for objects of that type, and the budget then
  bounds that answer — useful, but not a narrowing, because the filtered map is not a subset;
- a filter *narrows*: the map is what it is, and the filter removes from it — a subset, but a
  user asking for sockets at a place whose top-ranked objects are processes sees few of them.

## Decision

**A `--type` filter narrows the bounded map.** `project` selects and ranks the horizon without
regard to the filter, bounds it to the node budget, expands whatever `--expand` named, and only
then removes the nodes the filter does not keep. The consequences are fixed by that order:

- `map … --type t` is always a subset of the same `map …` without the filter, in nodes and, by
  construction, in edges: the edges of a map are built from the nodes it draws and the clusters
  standing for what it hid, so removing nodes can only remove edges (`edges_of` drops an edge
  whose endpoint nothing represents).
- What the filter removed is not silently gone: it is counted in `hidden.count`, exactly like
  everything else the projection knows and does not draw (§23.6).
- Clusters are not filtered. A cluster is the disclosure of what was *hidden*, not a drawn
  object of a type, and filtering its members would change its identity (§8.2, `--expand`).
- `--relations` was already a narrowing — it is applied when the edges are built, after the
  nodes are bounded — and is unchanged.

`docs/contracts/commands/spatial.yaml` states this on the `type` option in the same commit.

## Consequences

- The §43.2 property holds for every horizon, not only for one that fits the budget:
  `crates/ono-spatial-query/tests/properties.rs::should_keep_every_node_and_edge_a_filter_left_alone_and_invent_none`
  is red on the old projection at seed 1 and green on this one.
- `crates/ono-cli/tests/spatial_map_missing.rs::should_only_remove_nodes_and_leave_no_dangling_edge_when_a_type_filter_narrows_the_map`
  and `::should_only_remove_edges_when_a_relation_filter_narrows_the_map` — which passed only on
  hosts small enough for the budget — now hold for the reason they claim rather than by luck.
  `docs/STATE.md`'s *Next up* entry for that defect is closed by this ADR.
- A user who wants everything of one type at a crowded place gets fewer nodes than before, and
  `hidden` says how many. `--all` and `spatial.map.node_budget` (§47) are the ways to widen it.
  This is the price of §2.9 and §43.2 holding together; a view that answers a different set of
  objects depending on how it is filtered is not a map of one place.
- Zoom folding and the `known` count now see the unfiltered horizon, which is what they were
  always meant to describe: the projection's own knowledge, not the view's.

## Alternatives considered

- **Carry the unfiltered ranking into the filtered projection** (`docs/STATE.md`'s second
  option). It is the same thing said differently: a subset can only be obtained by cutting the
  ranked list at the position the unfiltered map cut it, which is this decision.
- **Leave the selection semantics and weaken the property to edges only.** Rejected: §43.2's
  sentence is about edges, but an edge appearing because a node appeared is precisely the
  "unknown edge" it forbids, and two acceptance-level tests already read it as a subset.
- **Make `--all` unbounded.** Rejected: §34.2 prohibits unbounded graph rendering outright.
