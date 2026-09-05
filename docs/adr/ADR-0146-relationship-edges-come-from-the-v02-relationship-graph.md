# ADR-0146: A place's relationship edges are the v0.2 relationship graph's

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §2.16, §3.5, §11.2, §11.4, §12–§18, §31.3, §32.1, §32.2, §45.2
- Decided by: agent (autonomous, phase S4b)

## Context

`follow` traverses a relationship edge (§6.4) and `near` streams the neighbours those edges
reach (§6.2). Until this increment the only source of edges was
`ono_spatial_index::bridge::facts_of`, which reads the *fields of one record*: a process's
`ppid`, its `cgroup`, its `user`. That reaches four of the ten exits §12 names and none of the
ones that need a second observation — the files a process holds open, the sockets it owns, the
processes that hold a file open.

Meanwhile v0.2 §22 already has a relationship subsystem that answers exactly those questions:
`ono-graph`'s `RelationshipProvider`s, which `trace` walks. They read `/proc/<pid>/fd`, the
kernel socket tables and the account database, and every edge they produce carries the provider
that asserted it and the confidence it claimed.

§2.16 forbids the spatial layer from becoming "an undocumented second source of truth", and
§31.3 says `map` and `trace` share the underlying graph. Deriving a second, weaker set of
process→file edges from record fields would be that second source.

## Decision

**The relationship edges of an object place are the edges the v0.2 relationship providers assert
about the same object.** `crate::spatial::relations::observe` re-reads the object from its own
provider, builds the `ono_graph::Node` for it, asks every relationship provider that expands its
schema, and translates each answer into the spatial vocabulary:

- the far end's record is absorbed through the provider bridge, so it becomes a place with the
  identity §42.1 requires;
- the pair *(provider id, the provider's own relation word)* selects the declared relation of
  `docs/contracts/spatial/relations.yaml` — `("linux.open-files", "reads")` is `process.opened_file`;
- the provider's word travels on the edge as `provider_relation`, its id as the edge's
  provenance, and its confidence as the edge's confidence, never raised above what the relation
  declares. So a neighbour reports the same relation and the same provider `trace` reports for
  the same edge (§31.3, invariant 2.16).

A pair the table does not translate — a container's image, a DNS answer, the sockets bound to an
interface — is a relation v0.4 declares no place for. It stays reachable through `trace` and is
not invented here.

The record-field bridge keeps the relations no relationship provider serves: a process's cgroup,
its pid namespace, its container, the listener a connection was accepted by. The two sources
agree on identity, so an edge both produce is one edge; the later observation wins its
provenance, which is why `SpatialIndex::record_edge` now replaces an edge of the same identity
rather than dropping the newer one (§33.2 keeps the providers authoritative).

**Cost is a property of the end, not only of the relation.** Reading one process's descriptors is
one directory; finding every process that holds one file is every process on the host. Providers
of the second kind (`linux.file-holders`, `linux.user-processes`, `linux.mount-users`,
`linux.socket-owners`) are asked only when the caller asked for that exit — `--all`, `near
<relation>`, `near --type <type>` or `follow <relation>` — which is §32.1's rule and §32.2's
"discoverable but unloaded exits".

**An exit nothing in this build can fill is `unsupported`, not `empty`** (§35.2, §2.17). After
the providers have been asked, every declared exit of the place's type that neither a
relationship provider nor the record bridge can fill is recorded as withheld/`unsupported` with
the reason. A refusal replaces a count only when nobody could take one: a provider that answered
about some ends and was refused others has answered, and the projection's completeness carries
the rest.

## Consequences

- `near`, `look --all` and `follow` at a process reach all ten exits §12 names, with the
  provider and the relation word `trace` uses.
- The spatial layer asks providers twice for one place — once for the object, once per
  relationship provider. §34's budget is respected because the broad providers are skipped
  unless asked for; the §34 measurement test remains S4e's.
- A new v0.2 relationship provider becomes spatially navigable by adding one row to the
  translation table, and by nothing else.
- Tests: `spatial_relationships_missing::should_name_the_same_relation_and_provider_as_trace_…`,
  `…should_enter_the_open_file_when_following_it_from_the_holding_process`,
  `…should_name_the_holding_process_among_the_file_neighbors_when_the_file_is_the_place`,
  `spatial_identity_missing::should_carry_source_provenance_and_confidence_on_every_relationship_edge`.

## Alternatives considered

- **Extend `facts_of` to read more fields.** It cannot: the process record does not name the
  processes that hold a file open, and no field of it ever will.
- **Run the `Tracer` at depth 1.** It would work, but it hides which provider failed on which
  exit, and §35.2 needs exactly that to tell `permission_denied` from `empty`.
- **Ignore the v0.2 graph and observe the kernel directly.** Forbidden by §2.16.
