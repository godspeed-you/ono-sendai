# ADR-0272: An object map says why each neighbour is there

- Status: accepted
- Date: 2026-08-29
- Spec refs: v0.4 §23.5, §11.3, §11.4, §2.17
- Decided by: agent (autonomous, `close-spat`)

## Context

`enter process 1; map` drew an `also here` group of eighteen rows, none of which named the relation
it stood in and several of which shared a display name — `containerd-shim` four times, with nothing
to tell one from another. §23.5 says "Edges MUST show relation labels" and §11.4 makes a
relationship explainable; a list of eighteen bare names is not a view a neighbour can be chosen
from, which is the only thing the view is for. The root map is unaffected, because a canonical
space reaches its members by containment and the tree draws them.

The information was already there and unused: `map-edge.v1.yaml` requires `relation`, and
`map-node.v1.yaml` requires `object_ref` — "the schema it is served under and the values of that
schema's identity fields".

## Decision

**1. Every row outside the hierarchy names the relation that puts it on the map.** The renderer
reads it from the map's own edges, so it invents nothing (§45.4). Both ends of an edge are
recorded: `bash` is on a `sleep` process's map because of `process.parent_of` exactly as the
children of `systemd` are, and only the direction differs — which the `relations` section already
shows. Where two relations reach one node, both are named: "why is this here" has two answers and
giving one would be choosing.

**2. The canonical parent says `parent`.** It is on the map because it is the parent (§11.3, where
`up` goes), and no relationship edge reaches it. A row with no explanation at all was the one case
the edge scan could not cover.

**3. A display name two drawn things share carries the identity that tells them apart.** From the
node's own `object_ref`: the first field after the schema, rendered `pid 4711`, which is what the
provider identifies the object by. Unambiguous names are left alone — a map where every row carried
an id would be less legible, not more.

## Consequences

- The row is `containerd-shim (pid 171616)  sleeping  — process.parent_of`, which can be read and
  acted on: `enter process 171616`, `follow process.parent_of`.
- The disambiguation applies to the tree as well as to `also here`, since two children of one place
  can share a name just as easily.
- `docker/acceptance/cases/105-spatial-map.case` `s5ae` runs the object map against a real process
  in the container and requires every neighbour row to name something.
- Encoded by `ono-spatial-render/tests/object_map.rs::should_name_the_relation_every_neighbour_of_an_object_stands_in`
  and `::should_tell_two_neighbours_sharing_a_display_name_apart`.

## Alternatives considered

- **Grouping the rows under relation headings** — reads well for a map with three relations and
  badly for one with ten single-member groups; the per-row label is the same information without
  the layout risk at 40 columns (§39.3).
- **Always showing the identity** — makes every row longer to solve a problem most rows do not
  have.
- **Giving the projection a hierarchy edge for every relationship neighbour** — would make the
  tree claim containment that no provider asserted, which §2.6 and §11.1 forbid.
