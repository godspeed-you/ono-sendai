# ADR-0172: `map links` keeps its target word, and the federated map is its own command

- Status: accepted
- Date: 2026-08-28
- Spec refs: §19.3, §6.9, §22, §11.5, §19.4, §35.4, §8.4; ADR-0124, ADR-0162
- Decided by: agent (autonomous, phase S8)

## Context

§19.3 writes the federated map as `map links`, in the plural, and pairs it with the rule that
"the default root map SHOULD NOT automatically expand all remote graphs". ADR-0124 fixed the
opposite convention for every other spatial verb: they take the bare name, and only `find` keeps a
target word because bare `find` belongs to findutils. `map` already exists as `ono.place.map` with
a `selector` argument (§6.9's `map <selector>`), so `map links` would otherwise parse as a request
to map a place called `links` — which is how it failed before this ADR: `spatial.not_found`.

## Decision

`map links` is a second command, `ono.place.map-links`, declared with verb `map` and target
`links`, exactly as `find place` sits beside `find file`. `links` is added to
`docs/contracts/targets.yaml` under the same rule the registry already applies to `link` and `context`:
the registry records every `<verb> <target>` pair the specification actually writes down.

It is a separate command rather than a magic selector because it answers a different question and
takes different arguments: no `--depth`, `--zoom`, `--focus` or `--expand`, because there is
nothing to zoom or expand — the map is the hosts and the links between them, at one hop, and
walking a linked host is what `jump` is for (§35.4).

What it draws:

- **Nodes**: this host's root place, and the root place of every host this session holds a link
  to. §4 gives every host the canonical root space, so the far root exists as soon as the link
  does; `space::learn` registers that geography without standing in it (ADR-0168).
- **Edges**: one `host.linked_to` edge per link — the relation `docs/contracts/spatial/relations.yaml`
  already declares for exactly this. Its confidence is the evidence's (§19.4, §11.5): `exact` for
  a link this session negotiated, `user_declared` for a definition nobody has connected. §19.4
  allows a one-sided observation to be displayed and requires it to carry the right confidence,
  so it is never quietly promoted.

The projection itself is S5's: the horizon is handed to `ono_spatial_query::project_map` and comes
back ranked, bounded and clustered like any other map, and the answer is the same
`ono.spatial-map/1` record of ADR-0162.

## Consequences

- `map` unchanged draws no remote graph. The default root map mentions no linked host at all,
  which satisfies §19.3's "SHOULD NOT automatically expand" with room to spare.
- The federated map is centred on the *local* root whatever place the session is standing in,
  including a place on the far side of one of the links it draws.
- `host.linked_to` is declared `Host → Host` while the nodes here are the hosts' root places.
  §7.1 makes a host's root place the system of that host, so the two ends are the hosts; the
  registry entry is left as S1 wrote it.
- Encoded by `spatial_remote_missing::should_show_the_linked_hosts_when_the_federated_map_is_asked_for`
  and `…should_not_expand_a_remote_graph_into_the_default_root_map`.

## Alternatives considered

- **`map --links`.** A flag would read as a filter on the ordinary map rather than as a different
  map, and it is not what §19.3 writes.
- **Treat the selector `links` specially inside `ono.place.map`.** A branch that exists because a
  word looks a certain way, with no contract entry, no help and no completion behind it.
