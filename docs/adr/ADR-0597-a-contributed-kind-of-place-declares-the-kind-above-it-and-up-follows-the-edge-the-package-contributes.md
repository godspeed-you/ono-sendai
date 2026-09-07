# ADR-0597: A contributed kind of place declares the kind above it, and `up` follows the edge the package contributes

- Status: accepted
- Date: 2026-09-07
- Spec refs: v0.4 §2.17, §3.4, §6.6, §11.1, §11.3, §35.5, §36.1, §36.4, §40, §43.2;
  `docs/contracts/kuang/contributions.v1.yaml` (`target.parent`),
  `docs/contracts/spatial/spatial.yaml` (`contributed_types.hierarchy`);
  `docs/architecture/external-system-provider.md` §15.2; ADR-0584, ADR-0585
- Decided by: agent (autonomous)

## Context

ADR-0584 made a contributed target a kind of place and said in its Consequences that `up` had
nowhere to go from one: "A contributed target declares no aggregate space, so the place sits in
no collection. §36.4 is the declaration that would give it one … and a package cannot make it."
The refusal was honest, and it left every external-system provider with a hierarchy the shell
could draw and not climb. For the Kubernetes provider the intended hierarchy is explicit and
small — provider instance, namespace, resource — and its specification's §35.6 is equally explicit
that `up` "is a spatial/context operation, not an owner-reference shortcut": a namespace is a
Pod's spatial parent even though a Deployment is its semantic owner through a ReplicaSet.

§36.4 lists what a plugin-defined aggregate space must declare: id, label, parent domain,
membership query, supported relations, cost/freshness, permissions. Read as a contract it is a
second spatial model beside the first. Read as intent — "the plugin MAY provide this richer
hierarchy without replacing core host-level topology" — it asks for exactly two things a package
can already almost say: which kind of place is above each kind, and how a place reaches it.

## Decision

**A target contribution may declare `parent`: the schema id of the kind of place that is this
kind's canonical spatial parent. The parent is reached along the relation the package contributes
for the pair, and `up` from a contributed place follows that edge and no other.**

- `parent` names a schema one of the *same package's* targets declares, never the target's own.
  The manifest must declare the shape `<schema>-><parent>` in `contributions.relations`, because
  the edge that carries the containment is an ordinary contributed relation (ADR-0585), answered
  by the package under `relation.write` and resolved by the host to a lifetime identity. Both
  halves are on disk, so both are settled at load, before the runtime is spawned, exactly as a
  shape naming nobody is (`check_parents` beside `check_shapes`); the handshake settles them
  again against what the package actually contributed.
- The canonical parent of a contributed place is computed where every canonical parent is —
  `canonical_parent_with` — as the far end of an edge whose relation is
  `<package>.<schema>_to_<parent>`, with `HierarchyKind::Containment`. Nothing else on the place
  is a parent: a second relation between the same two kinds — ownership, selection — is a
  relationship `follow` traverses and `up` never does (§43.2). A contributed place is never filed
  under a domain of this host (§2.17).
- `up` observes the contributed edges once, only for a kind that declared a parent and only when
  the index does not yet hold one, the way `look` observes them before drawing exits. Then the
  ordinary movement runs.
- Three refusals, each naming its reason rather than "the top of this host": the kind declares a
  parent and the package contributed no edge (naming `relation.write`); the kind declares no
  parent, which is the top of what its package contributes; a kind nobody contributed at all,
  which is ADR-0584's case unchanged.

## Consequences

Against the Kubernetes provider: `enter` a Pod, `up` lands on its namespace, `up` again on the
cluster, and `up` from the cluster refuses as the top of what the provider contributes — while
`follow` along the owner relation still reaches the ReplicaSet, because the two are two relations.
`back` and the trail are untouched: `up` is a `Movement::Up` step like any other.

The declaration is generic. A future provider with its own hierarchy — account, region, resource
— declares three `parent`s and three shapes and gets the same `up`. §36.4's remaining fields
(label, cost, permissions) are what the parent kind's own target and schema already say about
it, and its "membership query" is the inverse of the contributed edge, which `near` already lists.

`crates/ono-cli/tests/spatial_contributed_targets.rs`, over the real `ono` binary:

- `should_go_up_from_a_contributed_place_to_the_parent_its_package_declared`;
- `should_refuse_up_from_the_top_of_what_a_package_contributes`;
- `should_refuse_a_declared_parent_whose_shape_the_manifest_does_not_carry` — refused at load;
- `should_say_that_no_domain_holds_a_contributed_place_when_up_has_nowhere_to_go` — now the
  case of a declared parent whose edge was not contributed, naming the grant.

## Alternatives considered

**Implement §36.4's aggregate-space declaration in full.** Rejected as the first step: it is a
second model of spaces, membership queries and permissions beside the canonical geography, and
every field of it that matters for `up` is expressible with a kind, a shape and an edge the
package already contributes. The richer declaration remains open for a package that needs a
space with no object behind it; nothing here forecloses it.

**Let the host pick a parent from whichever contributed edge arrives first.** Rejected: §11.3
requires the canonical parent to be deterministic, and the Kubernetes specification's §35.6 is a
standing example of why — the first edge to arrive is as likely to be ownership as containment.

**Register the parent from the handshake only.** Rejected: ADR-0585 settled the ordering for
shapes by checking on disk before the runtime runs, and a parent that could only be checked after
the handshake would be a package that loads and then cannot climb, discovered by the user who
typed `up`.
