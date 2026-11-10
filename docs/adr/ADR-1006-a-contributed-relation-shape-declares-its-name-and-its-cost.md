# ADR-1006: A contributed relation shape declares its name and its cost

- Status: accepted
- Date: 2026-11-10
- Spec refs: v0.2 §31.7, §31.26; v0.4 §6.4, §24.2, §32.1, §36.1; ADR-0584, ADR-0585
- Issues: #224 parts (2) and (3)
- Decided by: agent (autonomous)

## Context

A package contributes a relation by declaring a v0.2 §31.7 shape, `<from>-><to>`, in its
manifest's `contributions.relations`; the host registers it as `<package>.<from>_to_<to>`
(ADR-0585). Two things the specification gives a relation had nowhere to be written:

- **its name.** §31.26 names relations `opens`, `emits`, `reaches`, and
  `contributions.v1.yaml`'s relation declaration lists a `name`. The id was the only word, so a
  user typed `follow io.github.godspeed-you.kubernetes.pod_to_node`. The Kubernetes package's
  manifest says it outright: "A shape carries no word of its own".
- **its cost.** v0.4 §32.1: "Default `look` and `map` MUST avoid expensive relationships unless
  cached or already available." A contributed relation was `normal` by construction, so a package
  whose edges cost a scan of a cluster could not keep a default `look` from paying for it.

The manifest field is a list of strings, and its parser belongs to `ono-kuang-protocol`; every
caller that derives a relation id from a shape — the load-time check, the permission scope, the
conformance test host — reads that string.

## Decision

A shape string may carry attributes after its two ends, `;`-separated:

```text
<from>-><to>[; name=<word>][; cost=<cheap|normal|expensive|privileged|remote>]
```

- `name` is one kebab-case word. It becomes the relation's `canonical_label` and
  `canonical_group`: `follow` takes it, `look` and `near` print it at the source end. The **id is
  unchanged** — it is still derived from the ends alone — and stays a word `follow` takes from the
  source end, so a script or a permission scope written against the id keeps working.
- `cost` is a §32.1 class. A default `look`/`map` expansion asks the contributing packages only for
  relations that are `cheap`/`normal` or that the caller asked for (`--all`, `near --type`,
  `follow <word>`); a declined one is recorded as `available on request`, the state a declared
  expensive relation already has (ADR-0421). Absent, the class is `normal`, as before.
- An attribute that is not `name` or `cost`, one declared twice, a name that is not a word, or a
  cost that is not a class is `package.invalid` at load, naming the shape — the same moment and
  code as every other shape check (ADR-0585).

The inverse end keeps `<id>.inverse`: §31.26 gives a relation one name, and inventing a second
word for the far end is not something the host may do on the package's behalf.

## Consequences

- `follow scheduled-on` from a Pod, once the Kubernetes package declares it; a package can keep
  an expensive relation out of every repaint of a map.
- `parse_shape` ignores the attributes, so the ids `kuang_permissions` and `ono-kuang-testhost`
  derive are unchanged and no protocol type changes.
- An older host meets `a->b; name=x` as an endpoint `b; name=x` that names no kind of place, and
  refuses the package at load — fail-closed, as §31.7 asks of unknown mandatory content.
- Encoded by `crates/ono-cli/tests/spatial_contributed_relations.rs`
  (`should_follow_a_contributed_relation_by_the_name_its_shape_declares`,
  `should_show_the_declared_name_of_a_contributed_relation_among_the_exits`,
  `should_still_follow_a_named_contributed_relation_by_its_id`,
  `should_refuse_a_relation_name_that_is_not_a_word`,
  `should_leave_an_expensive_contributed_relation_on_request_in_a_default_look`,
  `should_read_an_expensive_contributed_relation_when_it_is_asked_for`,
  `should_refuse_a_relation_cost_that_is_not_a_cost_class`) and
  `docker/acceptance/cases/166-contributed-relation-name-and-cost.case`.

## Alternatives considered

**A mapping per manifest entry** (`{shape: a->b, name: x, cost: expensive}`). Clearer to read, but
it changes the type of a manifest field owned by `ono-kuang-protocol`, and with it every consumer
of the field; v0.2 §31.7 shows the field as a list of strings.

**A separate relation document** carrying `contributions.v1.yaml`'s full declaration (`name`,
`from_schema`, `to_schema`, `direction`, `inferred`, `summary`). That is the complete answer and a
larger one: a new document kind, a new manifest path and a second place a relation is declared,
for two fields. It stays open beside this decision; the attribute form maps onto it field for
field if it is ever built.

**Make the name the id.** It would rename every existing relation and break the permission scopes
(ADR-0600) that name ids.
