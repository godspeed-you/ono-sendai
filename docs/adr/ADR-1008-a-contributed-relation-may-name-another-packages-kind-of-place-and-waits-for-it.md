# ADR-1008: A contributed relation may name another package's kind of place, and waits for it

- Status: accepted
- Date: 2026-11-10
- Spec refs: v0.2 §31.5, §31.7, §31.26 ("Plugins MAY add graph edges without owning either
  endpoint schema"); v0.4 §6.4, §36.1; external-system-provider §14.5; ADR-0585
- Issues: #224 part (1)
- Decided by: agent (autonomous)

## Context

ADR-0585 let a shape name a declared type or a schema of the package's *own* targets, and refused
everything else at load, because both halves of that check are on disk. It named the case it left
open: a shape naming *another* package's schema, "because the other package may not be installed
and the refusal would move back to traversal time". §31.26 is explicit that a relation needs to own
neither end, and a Kubernetes package relating a Pod to a node of an inventory package, or an
inventory package relating a host to a cluster, is exactly that.

## Decision

1. **At load** an endpoint is accepted when it is a declared type, a schema of the package's own
   targets, or a well-formed schema id (`<publisher>.<package>.<name>/<version>`) in **another
   package's namespace**. A schema in the package's own namespace that none of its targets declares
   is still `package.invalid` — that half is still checkable, and still a mistake.
2. **At adoption** a shape both of whose ends are kinds of place is registered as before. A shape
   with an end in another namespace that no loaded package contributes yet *waits*
   (`ono_spatial_core::relation::await_shape`), with the name and cost it declared (ADR-1006).
   Every load of any package settles the waiting shapes whose ends now exist, so the relation is
   registered the moment the other package's handshake contributes the kind — in either load
   order.
3. **At traversal** `follow <name|id>` from a place of the known end:
   - first loads an installed, not yet loaded package whose on-disk `contributions.targets`
     declare the awaited schema, as a contributed command loads its package (ADR-1005);
   - where the relation is still waiting, refuses with `spatial.unsupported` naming the relation's
     package and the schema it waits for — external-system-provider §14.5 keeps "target known but
     provider not installed" distinguishable from a relation that does not exist
     (`spatial.no_relation`).
4. The relation id is unchanged — `<package>.<from>_to_<to>`, the other package's schema giving
   its local name — so §31.5's namespacing and ADR-0600's permission scopes hold.

## Consequences

- A package can relate its places to another package's without either depending on the other's
  presence; the refusal is at the moment somebody asks and says what is missing.
- `ono-kuang-testhost` accepts the same shapes the shell accepts, so a package's own conformance
  check does not refuse what will load.
- Encoded by `crates/ono-spatial-core/tests/relations.rs`
  (`should_register_a_relation_waiting_for_another_packages_kind_once_that_kind_is_contributed`,
  `should_tell_another_packages_schema_from_one_of_its_own_and_from_a_type_name`),
  `crates/ono-cli/tests/spatial_contributed_relations.rs`
  (`should_load_a_package_whose_shape_names_another_packages_kind_of_place`,
  `should_refuse_following_a_relation_whose_far_kind_no_loaded_package_contributes`,
  `should_still_refuse_a_shape_naming_a_schema_of_its_own_package_it_does_not_declare`),
  `crates/ono-kuang-testhost/tests/spatial_package.rs::should_accept_a_shape_naming_a_schema_of_another_packages_namespace`
  and `docker/acceptance/cases/168-contributed-relation-to-another-package.case`.
- **Not proven end to end:** the shell-level path where the second package is installed and loads.
  The only fixture runtime, `kuang-example-plugin`, answers for one package id
  (`dev.example.echo`), so no test can lay out a second package that contributes a kind of place.
  The registration step is proven at the registry (the first core test above); the step that
  calls it is the same `adopt` every load runs. A second fixture package in `ono-kuang-sdk` would
  close this.

## Alternatives considered

**Refuse at load unless the other package is installed.** It makes one package's load depend on
another's presence and on load order, and an installed but unloaded package would still have to be
loaded to know its kinds — the refusal would be wrong both ways.

**Register the relation with a placeholder kind for the missing end.** A `SpatialType` is
constructed only by the contribution registry (ADR-0584); inventing one for a kind nobody has
contributed is the phantom §36.2 forbids.
