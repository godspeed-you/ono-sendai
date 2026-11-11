# ADR-1009: A contributed schema declares the scope its places sit in

- Status: superseded by ADR-1030 (in part: that the declaration is read from the plugin path the process started with)
- Date: 2026-11-10
- Spec refs: v0.4 §3.2, §2.18, §20.1, §36.1, §36.4, §42.1; external-system-provider §4.5, §9.1,
  §9.2, §9.4, §15.5; ADR-0584, ADR-0983
- Issues: #224 part (4)
- Decided by: agent (autonomous)

## Context

A contributed place's scope was always the local host: the provider bridges exist per host scope
(`SpatialSessionState::bridges`) and `Projection::scope_of` nested a scope only for a process
(its container, its pid namespace). For a resource in a cluster that is true of nowhere.
External-system-provider §9.1 makes scope "part of operational truth", §9.2 has each provider
define its scope dimensions (`cluster`, `namespace?`), and §9.4 wants entering another security
boundary to be visible. v0.4 §3.2 already lists `PluginScope` among the minimum scope kinds, and
`ScopeKind::Plugin` exists — nothing ever built one.

The question is where a package states its dimensions, in a form the host can read without a
second scope model.

## Decision

1. **The declaration is the schema's.** A package's `contributions.schemas` document may give a
   schema `scope: [<field>, …]` — the fields of its records that name the boundary an object sits
   in, outermost first. Scope is a property of the kind of resource (a Node has a cluster, a Pod a
   cluster and a namespace), and the schema is where the kind is described. The host reads it from
   disk when the package is loaded; the handshake's schema contribution is not extended.
2. **The scope is the existing model.** A place of that kind sits in the host it is reached from,
   then in one `ScopeKind::Plugin` scope per declared dimension the record names, nested
   outermost first, each scope's id carrying the dimensions above it:
   `host:pc12 → plugin:cluster=kind-ono → plugin:cluster=kind-ono/namespace=shop`. Two namespaces
   of one name in two clusters are two scopes; a dimension the record leaves null is no boundary
   (§2.17, external-system-provider §9.2's `namespace?`).
3. **Identity does not move.** The identity of a contributed place stays composed from its
   schema's identity fields in the projection's scope, exactly as a process's identity is
   unaffected by learning its container (§42.1, ADR-0983): declaring a scope never turns a pinned
   place into another place.
4. **It is shown where scopes are shown.** The place record's `scope` (what `look`, `near`,
   `find place` emit) names the innermost scope with its full path, and a movement between two
   places in different scopes records `scope_crossing` in the trail (§3.2, §20.1). The place path
   is unchanged: where a package declares spatial parents (ADR-0597) the path already climbs
   them, and repeating the scope there would print the cluster twice.

## Consequences

- The Kubernetes package states `scope: [cluster, namespace]` (or whatever its records call
  them) on its schemas and its places stop claiming to be on the local host. That is a change in
  the package, not in the shell.
- A scope dimension that names no field of the schema is never present on a record and so is no
  boundary; it is not refused at load, because the schema document is the package's own prose and
  the handshake does not carry it to check against.
- The declaration is read from the plugin path the process started with
  (`crate::spatial::lazy::declare_scopes`), the same set §31.68's placeholders are built from.
- Encoded by `crates/ono-cli/tests/spatial_contributed_targets.rs`
  (`should_place_a_contributed_place_in_the_scope_its_schema_declares`,
  `should_record_the_crossing_between_two_scopes_a_package_declares`,
  `should_keep_the_identity_of_a_contributed_place_whatever_scope_it_is_declared_in`,
  `should_leave_a_dimension_the_record_does_not_name_out_of_the_scope`) and
  `docker/acceptance/cases/169-contributed-place-scope.case`.

## Alternatives considered

**A `scope` field on the target contribution** (`contributions.targets`). The target document is
parsed strictly by `ono-kuang-protocol` (`deny_unknown_fields`) and crosses the handshake, so the
field would change the wire type, every package built against it and the SDK; and a scope is a
property of a kind of resource, which several targets may answer with.

**A root `plugin` scope instead of one nested in the host.** A cluster is not inside the host, but
the host is the boundary every observation of it is made from — its credentials and network — and
the scope model roots every chain at a host so that remote and local stay distinguishable (§19,
§43.7). Rooting elsewhere would make a contributed place look remote-or-nowhere to every consumer
of `host_scope`.

**One bridge (and one projection scope) per declared scope.** That is the larger change ADR-0584
recorded, and it would make the identity depend on the scope (the projection's scope chain is part
of a derived place's identity), which item 3 rules out.
