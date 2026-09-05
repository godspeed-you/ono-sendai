# ADR-0194: A package asserts an edge; it never creates a place

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §36, §36.1, §36.2 (KUANG/11 spatial extensions), §35.5 (plugin contributions are
  filtered by capability before the merge), §11.4, §11.5 (an edge explains itself and carries
  confidence), §22 (`MapEdge`), §41.2 (the relation registry), §53 (the settled answers), §2.16;
  v0.2 §31.5 (the reserved namespace), §31.7 (`<from>-><to>` contributions), §31.17 (degraded
  loading), §31.22/§31.23 (contributed commands and targets), §31.64 (every registry entry records
  its origin)
- Decided by: agent (autonomous), delivering v0.4 §50 Phase S9

## Context

§36 lets KUANG/11 "extend the spatial world" while "Ono core retains control of identity, security
and rendering contracts", and §36.1 lists relationship providers among what a package may
contribute. §35.5 states the boundary in one sentence: "The spatial host MUST filter plugin
nodes/edges according to capability scope **before** merging them into maps."

Nothing in the shell could act on any of that. A manifest could declare
`contributions.relations: ["process->process"]` — v0.2 §31.7's shape — and the declaration went
nowhere: no relation was registered, no package was ever asked for an edge, and the two RED tests
for this phase passed vacuously, because a contribution that cannot exist also cannot leak.

Three questions had to be answered to make the contribution real.

**What names a contributed relation?** The manifest gives shapes, not names. **How does a package
say what it actually connects?** A declaration is not data, and a host that drew an edge from a
declaration alone would be inventing the very truth §2.16 forbids it to own. **What stops a
package from putting things on the map that are not there?** §36.2 forbids "uninspectable phantom
edges" and §53 settles that a plugin "cannot create untraceable truth".

## Decision

1. **A contributed relation lives in the package's own namespace.** Each `<from>-><to>` shape
   becomes the relation `<package.id>.<from>_to_<to>` — `dev.example.echo.process_to_process` —
   registered at load through `relation::contribute`, which records the package as the entry's
   **origin** (§31.64). §31.5 reserves that namespace to its owner, so a contributed relation can
   never collide with a core relation or with another package's.
2. **The declared vocabulary is unchanged by contribution.** `docs/contracts/spatial/relations.yaml` and
   `relation::relations()` remain the relations this build ships, which is what `spec-check`
   compares; a contributed relation is found by `relation::spec` and listed by
   `relation::contributed_relations`. A package extends the world at run time; it does not edit the
   contract.
3. **`--relations <word>` names a relation or the package that contributed one.** The relation ids
   inside a package's namespace are the host's spelling, not something a user typed;
   `map --relations dev.example.packet-eye` is the question a reader actually has (§6.9).
4. **A package asserts its edges as data, through a contributed command.** The new canonical schema
   `ono.spatial-relation/1` — already the provenance schema of every spatial edge — is the record:
   the two ends by the key their own provider names them by, the package's own word for the
   relation, and a §11.5 confidence. A package contributes a relationship provider by contributing
   a command whose target is `spatial-relation` (§31.22, §31.23), and `map` asks it.
5. **The host resolves both ends; the package never creates a place.** Each end is looked up
   through the canonical provider for its §3.3 kind, and an end nothing answers to contributes no
   edge. A package can therefore say that two objects are related and can never say that an object
   exists — which is what makes §36.2's phantom edge unconstructible rather than merely forbidden.
6. **`relation.write` gates the whole contribution, before the merge.** A package that does not
   hold it contributes no relation, is registered as no contributor, and is degraded with the
   capability named (§31.17). §35.5's "filter before merging" is satisfied by there being nothing
   to filter: the map cannot drop what was never contributed, and a denied package's edges cannot
   reach a map by any path — including a map drawn by another package's request.
7. **The host never raises a contributed confidence to `exact`.** A package may state `exact`; the
   host did not observe the edge, so it travels as `strong`. Every other value travels as stated
   (§11.5), and the package is on the edge as its provider and its `origin` attribute — §36.2's
   "appear exact without provenance" is unreachable in both halves.

## Consequences

- A package can make Ono's world larger — the Kubernetes hierarchy of §36.4 is the same mechanism
  with more shapes — and every edge it adds is attributable, capability-gated and drawn between
  places the providers answer for.
- The example plugin gained one command, `dev.example.echo.command.relations`, which asserts the
  one relation its manifest declares between the two processes it can honestly name: itself and
  the shell that started it. It is a fixture that tells the truth, so the acceptance case proves
  the mechanism rather than the fixture.
- Asking a package for its edges costs one invocation per contributing package per map, and one
  provider lookup per end. Only packages that hold `relation.write` are asked at all.
- Contributed relations do not yet appear in `look`'s exits or in `follow`'s completion. They are
  edges on the map and in the index, where `inspect relation` explains them; making a contributed
  word a navigable exit is a further increment, and it is named in `docs/STATE.md` rather than
  half-built here.
- Encoded by `spatial_contracts_missing::should_keep_a_package_relation_out_of_the_map_until_its_capability_is_granted`,
  `::should_carry_the_contributing_package_as_the_origin_of_every_plugin_edge`, and
  `docker/acceptance/cases/110-spatial-contributions.case` (s9-a … s9-g).

## Alternatives considered

- **Name the contributed relation after the package alone** (`dev.example.echo`), so `--relations`
  matches it exactly with no origin lookup. Rejected: a package that declares two shapes then has
  two relations wanting one name, and the shape would have to be smuggled into the name anyway.
- **Extend the wire protocol with a `relations` contribution kind** carrying the edges in `hello`.
  Rejected: the edges are observations, not metadata — they change while the package runs, and a
  hello that carried them would be stale the moment it was read.
- **Draw the edge from the declared shape alone**, treating `process->process` as an assertion
  about every pair. Rejected outright: it is the undocumented second source of truth §2.16 forbids,
  and it would make every declaration a lie about the system.
