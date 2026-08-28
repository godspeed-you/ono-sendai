# ADR-0166: The text map — a ranked tree, an ASCII fallback, and the seam S6 attaches to

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §23.1, §23.2, §23.3, §23.5, §23.6, §39.1, §39.2, §39.3, §43.5, §45.4, §52.1
- Decided by: agent (autonomous, phase S5)

## Context

§23.2 is a MUST: "Every terminal MUST have a non-fullscreen textual map representation", and
§52.1 makes "map text rendering works without full-screen TUI" a release criterion. §23.2's
example draws a small graph with box characters and then says "The exact ASCII/Unicode line
characters are presentation details. ASCII fallback MUST exist." §39.3 allows a map to "collapse
into ranked tree/list projections rather than drawing graphs" at narrow widths, with identical
semantics. §45.4 forbids the renderer from inventing a node or an edge.

## Decision

`ono_spatial_render::spatial_map(map, width, charset)` renders any `ono.spatial-map/1`:

1. a heading — the place, the zoom level, how many nodes, how complete;
2. the hierarchy as a **ranked tree**, drawn from the map's own `hierarchy` edges. The renderer
   decides nothing about who contains whom; a place the tree does not reach is listed under
   *also here* rather than dropped (§45.4, §2.17);
3. the relationships that are not hierarchy, each with its direction, relation and confidence;
4. the landmarks, each with its reason and its evidence;
5. one closing line saying what the bound left out (§23.6).

**The tree is the projection at every width**, not only at forty columns. §39.3 permits it, the
semantics are identical to a drawn graph, and one renderer that is legible everywhere is worth
more than two that disagree. Every line is fitted to the width the caller states; no snapshot of
the layout is a contract (§43.5).

**Width comes from `COLUMNS` wherever it is stated, including for redirected output.** Everywhere
else in the shell a redirected stream is laid out at a fixed 80 columns for byte reproducibility
(v0.2 §4.6); a map is the one view whose whole point is to fit, §39.3 says so, and the environment
is part of a deterministic run.

**Charset**: Unicode box drawing when the locale promises UTF-8 and `TERM` is not `dumb`; ASCII
otherwise — the branches, the arrows, the landmark mark and the dash all have an ASCII spelling.
Guessing wrong here prints mojibake, which is worse than a plainer drawing.

**No colour at all, in this phase.** §39.1 forbids colour being *required* to distinguish the
current node, an inferred edge, a failed state, a remote boundary, root privilege or focus, so
every one of those is carried by a word or a glyph. Colour is presentation the interactive view
of §23.3 owns.

## Consequences

`map` works on a pipe, on `TERM=dumb`, at forty columns and with `NO_COLOR`, and it shows the
nodes `map --json` reports — the suite holds the two against each other.

**The seam S6 attaches to** is `spatial_map`'s inputs, not its output: the full-screen view of
§23.3 takes the same `ono.spatial-map/1` record — the same nodes, edges, clusters and landmarks,
already ranked and bounded by `ono-spatial-query` — and adds a viewport, a focus cursor and the
key bindings of §23.3. It must not re-select or re-rank; `MapRequest::focus` already carries focus
into the projection and `SpatialMap::focus` already travels beside `center`, so moving the cursor
is a new request with a new focus and no movement of the place (§23.4). The interactive budget of
100 nodes is `spatial.map.node_budget`, which is the same number `--all` already uses.

## Alternatives considered

- *Drawing §23.2's centred graph literally* — rejected for now: it is legible for five nodes and
  unreadable for thirty, and §39.3 explicitly blesses the tree. A graph layout is presentation the
  interactive view can add without changing any semantics.
- *Honouring `COLUMNS` for every redirected view* — rejected: it would change table output the
  rest of the suite pins at 80 columns, and that is a separate change with its own tests.
