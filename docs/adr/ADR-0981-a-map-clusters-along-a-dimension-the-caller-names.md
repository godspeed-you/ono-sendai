# ADR-0981: A map clusters along a dimension the caller names

- Status: accepted
- Date: 2026-11-10
- Spec refs: v0.4 §2.16, §8.2, §8.3, §23.6, §34.2; ADR-0976; issue #209
- Decided by: agent (autonomous)

## Context

§8.2 lists nine dimensions a map may cluster along — canonical collection, service, container,
user, cgroup, endpoint group, directory subtree, filesystem, provider-defined group. The
projection had the first, and since ADR-0976 the directory subtree. On a host with three
hundred processes the map said "270 more in processes", which is true and says nothing about
whose they are (#209).

## Decision

**`map --cluster <dimension>` groups what the budget did not draw along `user`, `service` or
`container`.** Each is a relation the object's own record states — `process.run_by_user`,
`service.controls_process`, `container.contains_process` — so a cluster groups by an observed
fact. The shell asks the target that serves the groups (`user`, `service`, `container`) so the
relation has a place to settle on.

- A cluster is named after the collection and the group — `processes (user root)` — and its
  `grouping` is the dimension's word. Its id is derived from both, so `map --expand <id>`
  reaches it in a later run (§8.3).
- Only the eight largest groups get a cluster. The smaller ones, and every object whose record
  states no group, stay counted with their canonical collection. Nothing is dropped (§23.6).
- Without the option the dimension is the canonical collection, as before. Choosing one is the
  caller's: a default that clustered by service on one host and by user on another would make
  two maps of similar systems unlike each other.

## Consequences

Easy: `enter compute; enter processes; map --cluster user` answers "whose are these";
`--cluster service` separates the processes of the big units from the rest.

Hard: an orientation read of a collection is bounded (ADR-0576), so the clusters count the
processes that were read; `--all` widens the read. `container` forms clusters only where the
process records state their container, which the native provider does not yet do for every
runtime. Cgroup, endpoint group and filesystem are not offered: no record states them for the
members of a collection without a per-object read.

Encoded by `crates/ono-spatial-query/tests/map.rs::should_cluster_by_the_user_when_the_map_is_asked_to`,
`::should_keep_the_largest_groups_and_count_the_rest_with_the_collection`,
`crates/ono-cli/tests/spatial_map.rs::should_cluster_the_processes_by_their_user_when_the_map_is_asked_to`
and acceptance case `139-map-clusters-by-user`.

## Alternatives considered

**Pick the dimension automatically** — whichever splits the rest most evenly. The map of one
host would then differ in kind from the map of the next, and §29.3 wants a script to know what
it will get.

**A cluster per group, however many** — forty services of one process each are forty lines
that say less than the count they replace (§34.2).

**Nested clusters** (collection, then user). `ono.map-cluster/1` is flat, and §8.3's expansion
names one cluster.
