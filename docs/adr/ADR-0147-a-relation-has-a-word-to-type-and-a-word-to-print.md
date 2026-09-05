# ADR-0147: A relation has a word to type and a word a place prints

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §6.4, §11.2, §12, §13, §14.3, §14.4, §24.2, §41.2, §41.3, §44.2
- Decided by: agent (autonomous, phase S4b)

## Context

§41.2 fixes a relation's `canonical_label` and `inverse_label` as "the word `follow` takes".
§12 then prints a process place with the exits `parent`, `children`, `sockets`, `files`,
`namespaces`, `cgroup`, `service`, and §13 prints a service with `processes`, `dependencies`,
`dependents`. Those are not the same words: `follow socket :443` is singular because it
traverses one edge, and the exit above it is plural because it holds several.

ADR-0143 made the group label the word `enter` takes (§24.2). With one label per relation the
registry had to choose, and it chose the singular — so a place printed `socket` where §12 prints
`sockets`, and `enter children` had nothing to reach.

## Decision

`RelationSpec` and `docs/contracts/spatial/relations.yaml` gain **`canonical_group` and
`inverse_group`**: the word `look` prints for that exit at each end. `follow` and `enter` accept
either word, so every exit a place shows is an exit a user can type, and the singular that names
one edge keeps its meaning.

`RelationshipEdge::label_from` stays the `follow` label — that is what a neighbour reports as its
`relation` and what a trail step records — and `RelationshipEdge::group_from` is the printed one.
`exits_from` yields the printed words, because it builds the groups of a place view.

Three consequences of writing the §12/§13/§14 vocabulary down changed the registry itself:

- **`user.owns_process` becomes `process.run_by_user`** (source `Process`, target `User`,
  `canonical_label: user`). §12 lists `user` among a process's exits, and §41.2 makes the
  canonical label the word at the source end; a relation oriented from the user made `user` an
  inverse label, which no registry consumer can find. The relation is the same fact.
- **`process.owns_socket` prints `process` at the socket end.** §14.3 names that exit "owner
  process/service" and §44.2 walks it as `follow process`; `owner` remains its `follow` label.
- **`service.in_cgroup` is declared.** §13 lists a service's control group among its exits. No
  installed provider states it, so the exit answers `unsupported` — which is the answer §35.2
  exists to make possible, and is different from a service with no cgroup.

**`socket.accepts_connection` is `exact_or_provider_declared`, not `exact`.** The kernel reports
two sockets sharing a local endpoint; it never reports that one accepted the other. The bridge
composes the edge with `strong` confidence and the shared endpoint as its evidence, and §11.5
forbids calling that an observation.

**Labels resolve by `is_a`, not by type equality.** §14.3 and §14.4 make a listener and a
connection two kinds of socket; a relation declared to reach a `Socket` reaches both. Before
this, a listener place had no exits at all.

## Consequences

- `cargo run -p xtask -- spec-check` compares the two new fields in both directions, so a word a
  place prints cannot drift from the registry that generates help and completion (§41.3).
- Four tests that asserted the singular group names were rewritten to the §12/§13 words in the
  same commit as the registry change: `ono-spatial-core/tests/relations.rs`,
  `ono-spatial-index/tests/{index,conformance}.rs`, `ono-spatial-query/tests/neighborhood.rs`.
  No behaviour they assert changed; the vocabulary the contract declares did.
- `docs/contracts/providers/linux-procfs.yaml` names the renamed relation.

## Alternatives considered

- **One label, plural everywhere.** `follow sockets :443` reads wrong and §12 writes `follow
  socket :443` outright.
- **Pluralise mechanically.** `child` → `children` is irregular and `cgroup` must not pluralise
  at all; a rule that has to be corrected per relation is a declaration with extra steps.
- **Keep the group label singular and add an alias table in the shell.** The registry generates
  help, completion and the map legend (§41.3); a word only the shell knows is undocumented
  surface.
