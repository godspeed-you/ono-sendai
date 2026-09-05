# ADR-0585: A contributed relation runs between contributed kinds of place

- Status: accepted
- Date: 2026-09-06
- Spec refs: v0.4 §2.16, §3.3, §3.5, §6.2, §6.4, §11.4, §11.5, §22.2, §32.1, §35.2, §35.5, §36.1,
  §36.2, §41.2, §42.4, §53; v0.2 §31.5, §31.7, §31.22, §31.23, §31.25, §31.26, §31.64, §31.68;
  `docs/architecture/external-system-provider.md` §11.1, §12.6; ADR-0022, ADR-0126, ADR-0128,
  ADR-0194, ADR-0282, ADR-0583, ADR-0584
- Decided by: agent (autonomous)

## Context

ADR-0584 made a target a KUANG/11 package contributes into a kind of place, and named the gap it
left in the same breath:

> **`near` finds nothing and `follow` has nothing to follow.** Both are graph questions, and a
> contributed place has no edges. A package *can* contribute edges — `contributions.relations`,
> §36.1, wired in `crates/ono-cli/src/spatial/contributions.rs` — but a relation shape is written
> `<from>-><to>` in the declared vocabulary of §3.3, so today a package can only assert edges
> between *core* types. Letting a shape name a contributed type is the next gap, and it is a real
> one rather than an oversight: the shapes are read from the manifest at load time and the types
> are learned from the handshake, so the two have an ordering to settle.

That is the whole relationship half of the spatial model, and it was shut for every external-system
provider rather than for Kubernetes in particular: a package answers for pods and nodes, and the
one thing a person wants to ask — what is this pod *on* — is the one thing the shell had no
vocabulary for. Three separate closures held it shut:

- **the shape's vocabulary.** `contributions::adopt` resolved each end through `SpatialType::ALL`,
  so `dev.example.echo.place/1->dev.example.echo.zone/1` fell to `continue` — silently, which is
  the second half of the problem.
- **the relation registries.** `relation::exits_from`, `resolve_label` and `labels` all read the
  declared `RELATIONS` table alone. A relation `relation::contribute` had recorded beside it was
  findable by `spec(id)` and by nothing a user could type, so even a registered contributed
  relation opened no exit and `follow` refused its own id.
- **the edges.** `contributions::merge` was called from `map` and nowhere else, and
  `relations::observe` — which is what fills the index before `look`, `near` and `follow` read it —
  asks the v0.2 relationship providers, none of which has ever heard of a package's schema.

## Decision

**A relation shape may name a kind of place the package contributes, by the id of its schema; and
the shape is settled against what is on disk, before the package runs.**

### 1. The endpoint is a schema id, not a display name

Each end of a `<from>-><to>` shape is one of two things: a declared type of
`docs/contracts/spatial/spatial.yaml`, in whatever case the shape spelled it, exactly as before; or
the id of a schema one of *this package's* own `contributions.targets` documents declares —
`dev.example.echo.place/1`.

It is the schema id rather than the type's display name for the two reasons that decided ADR-0584's
naming from the other side. A display name is not the package's to rely on: `types::contribute`
renames a contributed type to its schema id when `EchoPlace` or `Pod` is already taken, so a shape
naming `Pod` would name a different thing depending on which other packages happen to be installed.
And a display name lives in the handshake, while a schema id is on disk — which is what the rest of
this decision rests on.

The registered relation is `<package.id>.<from>_to_<to>`, where a declared endpoint gives its own
name lower-cased and a schema id gives its local name: `dev.example.echo.place/1` gives `place`, so
the shape above registers `dev.example.echo.place_to_zone`. The publisher and package prefix is
already the namespace §31.5 reserves; repeating it inside the id would give
`dev.example.echo.dev.example.echo.place_to_…`. The id is derived from the shape's *text* rather
than from the resolved types, so the id a manifest implies and the id a loaded package's edges
resolve against are one string computed one way.

### 2. The ordering problem: check on disk, adopt after the handshake

ADR-0584 named the ordering and it is real: a shape is read from the manifest at load, a contributed
type is registered from the handshake, and a shape read before the handshake cannot be resolved.
The two halves of the question are settled in two different places, and the split is the decision:

- **The refusal is at manifest-read time, against the declarations on disk.** §31.68 already reads
  a package's `contributions.targets` documents without starting anything — that is how `get pod`
  has a registry placeholder before the package runs (ADR-0282). Those documents carry the schema
  id of every target, so the set of schema ids a shape may name is on disk beside the shape that
  names them. `crate::spatial::contributions::check_shapes` compares the two in
  `plugins::load`, **before the runtime is spawned**. A shape that is not a `<from>-><to>` pair, or
  whose endpoint is neither a declared type nor one of those schema ids, is `package.invalid`
  naming the shape and the endpoint, and the package does not load.
- **The adoption is after the handshake, against the types it registered.** Turning a shape into a
  `RelationSpec` needs the `SpatialType` a schema id stands for, and that exists only once
  `contribute_spatial_type` has run over the handshake's targets. `plugins::load` therefore mounts
  the package — `session.providers()`, which is what ADR-0583 and ADR-0584 already made the moment
  a package's targets enter the registries — and adopts the shapes afterwards.

So the two halves happen in the order they depend on, and the *refusal* does not wait for the
second one. This is the property that mattered: a package whose declaration is wrong learns so when
it is loaded, with the endpoint named, rather than when a user types `follow` and is told a word
means nothing.

`ono_kuang_testhost::check_spatial_package` reads the same two documents and answers the same
question without loading anything at all, so a package author gets the refusal — and the list of
relation ids the package would register — before publication (§31.73).

### 3. A contributed relation is a relation everywhere a user types one

`relation::exits_from`, `resolve_label` and `labels` now read the declared table **and** what
packages contributed this session. The declared table is untouched and `relations()` still answers
only what this build ships, which is what `spec-check` compares against
`docs/contracts/spatial/relations.yaml` in both directions — so the drift check is exactly as
meaningful as it was, for the same reason ADR-0584 could open `--type` without opening
`SpatialType::ALL`.

What cannot be checked by a gate — the shape grammar, the id, the labels, the confidence, the cost
and the origin obligation, because the names belong to packages nobody has installed — is written
into `relations.yaml` as a `contributed_relations` block and into `contributions.v1.yaml` as
`relation.manifest_shape`. That is the shape ADR-0584 chose for `contributed_types`, and it is
right here for the same reason: the file stays a closed document a closed list is compared against,
and the prose beside it says what the closed list deliberately does not hold.

### 4. The edges are read where the exits are read

`relations::observe` asks the contributing packages for their edges, resolves both ends through the
canonical providers as `map` already did, and records them — but only when a relation some package
contributed actually touches the kind of place being looked at. Asking a package is an invocation,
so a place no contributed relation could reach never pays for one and a `look` at a process costs
what it always did (§32.1). §35.5's filter has already run by then: a package without
`relation.write` contributed no relation, so there is nothing left to leave out of the answer.

The exits a merge answered are marked answered. Without that, the sweep that closes `observe` would
call them `unsupported` — §35.2's word for an exit nothing in this build can fill — about a
relation that had just been read, which is the false statement §42.4 forbids in its other direction.

### 5. The evidence and the origin stay on the edge

Nothing here weakens what a contributed edge has always had to carry, and the fixture now proves it
where a user meets it. `near` and `inspect relation` answer for a contributed edge with `provider`
= the package, `provenance.provider` = the package, `evidence.origin` = the package, and the
contributor's own word for the relation in `provider_relation`. The confidence is the package's
claim and is never raised: `exact` becomes `strong`, because the host did not observe the edge
(§22.2, §36.2). A contributed edge is therefore distinguishable from one the host derived by
looking at it, which is what §31.25 means by evidence remaining inspectable and what §53 means by a
plugin being unable to create untraceable truth.

## Consequences

What a package can now declare:

```yaml
contributions:
  targets: [contributions/targets.yaml]     # echo-place -> dev.example.echo.place/1
                                            # echo-zone  -> dev.example.echo.zone/1
  relations: ["dev.example.echo.place/1->dev.example.echo.zone/1"]
```

and, with `relation.write` granted, the shell registers `dev.example.echo.place_to_zone` between
the two kinds of place the package contributes.

What `look`, `near` and `follow` now reach:

- **`look`** at a contributed place lists the exit the shape opened, with the neighbours behind it,
  instead of `unsupported`.
- **`near`** answers with the far end as an ordinary `ono.spatial-neighbor/1` — a place with its
  own identity, its schema, its `canonical_ref` and the edge that reached it.
- **`follow <relation>`** traverses it, records the step in the trail with the relation and the
  word that was typed, and leaves the session standing on the far end.
- **`map`** is unchanged in behaviour and now draws contributed places joined by contributed edges,
  because the ends resolve to places rather than to nothing.

What is refused, and when:

- a shape that is not `<from>-><to>`, or whose endpoint is neither a declared type nor a schema id
  one of the package's own targets declares — **at load, before the runtime is spawned**, as
  `package.invalid` naming the shape and the endpoint. The package does not load.
- the same, **before publication**, from `check_spatial_package`, which reads the package directory
  and nothing else.
- a shape whose declaration is sound but which the *loaded* package then does not back with the
  schema it promised contributes no relation. That is a package contradicting its own manifest,
  which is a different defect from a wrong declaration and is not this ADR's to diagnose.

What still cannot be done:

- **A shape cannot name another package's contributed schema.** `contributions.v1.yaml` says a
  relation's `from_schema` "need not be the package's own", and that is right for the *edge*: a
  package may assert an edge whose ends are core places, and does. But a shape naming a schema this
  package does not declare a target for cannot be checked at load — the other package may not be
  installed, may be installed later, or may never be — and accepting it would put the refusal back
  where this ADR took it from. A package that wants to relate to another package's kind of place
  declares a dependency on it (§31.30); making that dependency carry the endpoint is a separate
  increment.
- **A contributed relation has no word of its own.** The id is the label and the group, so
  `follow dev.example.echo.place_to_zone` is what a user types.
  `contributions.v1.yaml`'s relation declaration already carries a `name`, a `summary` and a
  direction that the manifest shape does not, and a package that could declare those would get
  `follow zone` and a legend entry. The host does not invent a friendlier word than the one it was
  given, because a word it invented could collide with a declared label and change meaning when
  another package is installed.
- **The cost is not declared.** A contributed relation is `normal` and a merge is one invocation
  per contributing package. A relation that costs a network round trip per edge has no way to say
  so, which is the same gap ADR-0584 recorded for a contributed target's boundedness and the same
  increment closes both.

Which tests encode it — `crates/ono-cli/tests/spatial_contributed_relations.rs`, over the real
`ono` binary:

- `should_show_a_contributed_relation_among_the_exits_of_a_contributed_place` — `near` at
  `echo-place`'s `ledger` answers with the zone, along the relation the shape opened.
- `should_follow_a_contributed_relation_to_a_place_of_another_contributed_kind` — `follow
  dev.example.echo.place_to_zone` arrives at `dev.example.echo.zone/1` with the identity `z-1`.
- `should_name_the_contributing_package_in_the_evidence_of_a_contributed_edge` — the provider, the
  provenance and the evidence all name the package, the contributor's own word travels, and the
  confidence is not `exact`.
- `should_contribute_no_relation_between_contributed_places_without_the_capability` — without
  `relation.write` the exit does not exist. `relation.write` is never granted by default.
- `should_refuse_a_relation_shape_whose_endpoint_nobody_contributes` — `Pod->…` fails the load and
  names both the shape and `Pod`.

and `crates/ono-kuang-testhost/tests/spatial_package.rs`:

- `should_report_a_relation_between_two_kinds_of_place_the_package_contributes` — the id the
  package would register, read from the package directory.
- `should_refuse_a_shape_naming_a_schema_no_target_of_this_package_answers_with` — a schema id is
  not a licence to name anything.

The SDK's example package gained the schema `dev.example.echo.zone/1` and the target `echo-zone`,
and its `relations` command now asserts two edges rather than one: the `process->process` edge it
always asserted, and one between `dev.example.echo.place/1` and `dev.example.echo.zone/1`. A
package needs *two* contributed kinds of place before a shape between two of them proves anything —
a shape from a kind to itself would have passed with the endpoint resolution half-written. An
assertion whose shape the manifest never declared resolves to no relation and contributes nothing,
which is why the fixture can assert both and be read by suites that declare either shape.

## Alternatives considered

**Validate the shapes after the handshake, when the types are known.** Rejected, and it was the
tempting one because `adopt` already runs there and the check would have been three lines. It puts
the refusal after the package's code has run, which gives up §31.89's manifest-before-code rule for
nothing: the information needed to refuse is on disk, so refusing later is a choice to read it
later. It also makes the refusal arrive interleaved with the package's own output, where a
`package.invalid` about a manifest line is the least legible thing on the screen.

**Accept a shape naming an unknown type and refuse it at traversal.** Rejected outright. The user
who typed `follow` did not write the manifest, the message would have to describe a package's
declaration to someone who cannot fix it, and every session with the package loaded carries a
relation that will never work. It is the failure mode ADR-0584 asked this increment to avoid by
name.

**Let a package declare its contributed *types* on disk, as a `contributions.types` document.**
Rejected as a second declaration of something already declared. A target document already carries
the schema id of every target, and a type is the schema (ADR-0584 §2) — so a types document would
either repeat the target document or disagree with it, and §31.68's whole argument for reading
declarations from disk is that the disk copy and the handshake copy have one shape and cannot
disagree.

**Name the endpoint by the contributed type's display name — `EchoPlace->EchoZone`.** Rejected: the
display name is assigned at contribution time and falls back to the schema id when it collides, so
the same manifest would name different things on two machines. It is also not on disk, which would
have forced the refusal back to handshake time.

**Name the endpoint by the target — `echo-place->echo-zone`.** Rejected for ADR-0584's reason read
from the other side: several targets may answer one schema, and a shape between targets would be a
shape between ways of asking rather than between kinds of thing. The edges resolve on the schema a
record carries, so the shape has to speak the same vocabulary.

**Merge contributed edges on every observation, as `map` does.** Rejected: that is an invocation of
every loaded package on every `look`, including a `look` at a process no package has ever heard of,
and §32.1 forbids a default orientation from spending what it was not asked to. Merging only where
a contributed relation touches the kind of place being looked at costs nothing when nothing applies
and is exact when something does.

**Give a contributed relation a short label derived from the schema — `follow zone`.** Rejected:
the shape says nothing about what to call the edge, and a label the host invented could collide
with a declared label — `follow user` meaning one thing at a process and another at a contributed
place, depending on which packages are installed. §31.26's declaration has a `name` field for
exactly this, and letting a package fill it is the increment that should give the short word.
