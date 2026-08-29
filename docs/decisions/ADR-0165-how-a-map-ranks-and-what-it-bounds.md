# ADR-0165: How a map ranks, what it bounds, and why the clock is not a ranking input

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §23.1, §23.6, §8.1, §8.2, §8.3, §34.2, §43.2, §47, §53
- Decided by: agent (autonomous, phase S5)

## Context

§34.2 sets the visible-node budget at "text map 30 nodes, interactive map 100 nodes before
mandatory clustering" and states that "unbounded graph rendering is prohibited". §47 spells the
second number `spatial.map.node_budget = 100`. §23.1 gives the priority order and §23.6 the four
things a renderer may do when the set does not fit. §8.1's five levels are normative for tests.

Three questions were open: what `--all` means, how nodes are ranked inside a budget, and whether a
zoom level may fold the current place away.

## Decision

**Budgets.** The default map draws at most `TEXT_MAP_BUDGET` = 30 nodes. `--all` raises it to
`spatial.map.node_budget` (100) and no further: §34.2 prohibits an unbounded graph, so `--all` is
a wider bound, never the absence of one. `near --all` and `find place --all` remain the unbounded
ways to enumerate.

**Ranking, in §23.1's own order**, as one total order so a script gets the same map twice (§29.3):

1. the current place;
2. the focused node, where `--focus` named one;
3. **tier** — the canonical geography (L0 root, L1 domain, L2 collection) before individual
   objects (L3 entity, L4 detail). A map of a host with two hundred devices that drew twenty
   devices and none of the collections would be a list, not a map;
4. depth from the centre;
5. landmarks that ask for *attention* — failed, restarting, public listener, storage pressure,
   high CPU or memory, and a user pin, which outranks every heuristic (§26.4);
6. the label, then the id.

Objects at the same rank are then **dealt out over the collections they belong to**, one from each
in turn. §23.6 asks for "cluster; rank; paginate", and ranking alone lets the largest collection
take every remaining slot.

**A landmark that merely informs does not reorder the map.** `recently_changed` and `privileged`
are drawn on the node they belong to and are not a ranking input. Two reasons: §26.3 forbids
treating every heuristic as an incident, and — decisively — a clock-relative rank makes two maps
of an unchanged system name different nodes, which breaks §43.2's "filtering cannot create unknown
edges" the moment a filter is checked by comparing two runs. Recency remains a §3.6 ranking input
for *neighborhoods*, where the spec asks for it by name; §23.1's list for maps does not.

**Clustering** groups what did not fit by its canonical collection — §8.2's first allowed
dimension, the one every place has and the one `enter` reaches. The cluster id is a hash of the
grouping and the key, so it is the same id in the next command, which is what `--expand` needs
(§8.3). Expanding draws the members and drops the cluster; it never changes the current place.

**Zoom** replaces a place finer than the requested level with its canonical ancestor at that
level, and **never folds the centre away**: the user is standing there, and §23.1 draws it first.
A place with no ancestor at that level is drawn as itself rather than dropped (§2.17). Without
`--zoom`, `zoom_level` reports the finest level the map actually draws, so it folds nothing.

## Spec deviation

None of the spec. One *test* cannot be satisfied together with the rules above:

- Section: v0.4 §6.9, §34.2, §47
- Text: `spatial_map_missing::should_show_more_than_the_default_when_the_map_is_asked_for_all`
  asserts that "the complete map of the processes collection contains the process this test
  spawned (pid N)".
- Instead: `--all` draws the 100 best-ranked nodes, which on a host with three hundred processes
  cannot be guaranteed to contain an arbitrarily named one.
- Why: `spatial_contracts_missing::should_bound_the_default_map_to_its_node_budget` asserts the
  opposite bound — "even `--all` stays inside `spatial.map.node_budget` (100 by default)" — and
  §34.2 prohibits unbounded rendering. The only ranking that would reach a freshly spawned
  `sleep` is a clock-relative one, and that makes
  `should_only_remove_{edges,nodes}_…_when_a_…_filter_narrows_the_map` compare two maps of two
  different moments and fail. Three tests against one; the first half of the fourth (that `--all`
  is strictly larger than the default) is delivered and green, and its second half carries an
  `#[ignore]` naming this ADR.

## Consequences

A map is legible on a host with hundreds of processes: the geography is always drawn, every
collection is visible, and each one shows a few members and a count for the rest. Two runs of the
same map on an unchanged system name the same nodes, which is what makes §43.2's property tests
checkable at all from outside the process.

The cost is that a busy process is not pushed onto the map by being busy unless it crosses a
§26.2 threshold. That is the conservative default §26.3 asks for.

## Alternatives considered

- *`--all` unbounded* — rejected: §34.2 prohibits it, and the root map would then exceed the
  node budget the contract test asserts.
- *Ranking by recency* — rejected: see the deviation above.
- *Clustering by user or by cgroup* — both are §8.2 dimensions and both are worth having, but the
  canonical collection is the one every place has and the one whose id is stable; a second
  dimension is a later increment with its own test.
