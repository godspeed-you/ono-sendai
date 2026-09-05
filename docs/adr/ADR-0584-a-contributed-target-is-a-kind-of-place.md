# ADR-0584: A contributed target is a kind of place

- Status: accepted
- Date: 2026-09-05
- Spec refs: v0.4 §3.1, §3.3, §6.8, §7, §10.1, §27.2, §32.1, §33.3, §34, §36.1, §36.4, §41.1;
  §31.14, §31.23, §31.64, §31.68; `docs/architecture/external-system-provider.md` §11.1, §11.2,
  §12.4, §12.6, §15.5, §35.4; ADR-0582, ADR-0583
- Decided by: agent (autonomous)

## Context

ADR-0583 put a loaded package into the `ProviderRegistry` and said, in its own Consequences,
exactly what it had not done:

> **The spatial commands still do not reach a contributed target**, and this ADR does not claim
> they do. `enter`, `near` and `find` plan over `SpatialType::ALL`, a closed vocabulary in
> `ono-spatial-query`; a contributed noun is not in it, so the plan never asks for the target
> even though the registry would now answer.

That was three closed lists, not one, and each of them shut a different door:

- **`SpatialType`** is a `#[non_exhaustive]` enum. `--type`, the `<type>/<key>` selector of §11.2
  and the target/type join all searched `SpatialType::ALL`, so no word a package brought could be
  a kind of place at all.
- **`spatial_type_of`** in `ono-spatial-index` maps a schema id to a type through a literal
  `match`. Every schema a package declares fell to its `_ => None` arm, which is the arm that
  means "§7 gives this no place" — the same answer it gives an environment variable.
- **`SPATIAL_TARGETS`** in `ono-spatial-query::discovery` is the list a place search plans over.
  A target not in it is a target the search never asks, however well the registry answers for it.

The closedness is load-bearing and it was worth reading before touching. `SpatialType::ALL` is
the vocabulary `cargo run -p xtask -- spec-check` compares against `object_types` in
`docs/contracts/spatial/spatial.yaml`, in both directions: a name the contract carries and the
enum does not is a space or relation nothing can serve, and a name the enum carries and the
contract does not is undocumented surface (ADR-0126, ADR-0128). `spaces.yaml` and `relations.yaml`
may name nothing outside it, and §41.3 generates one SDK enum from all four documents. A
vocabulary a package could add a name to is a vocabulary no gate can check.

So the question was never whether to open `SpatialType::ALL`. It was where the second list goes.

## Decision

**The declared vocabulary stays closed, and a contributed kind of place lives beside it.**

This is the shape `ono_spatial_core::relation` already has for the same problem (§36.1 lets a
package contribute relationship types, and `relation::contribute` records them beside the declared
table without entering it). Types now have it too.

### 1. `SpatialType::Contributed(&'static str)`, registered at runtime

`ono_spatial_core::types::contribute` records a kind of place and returns the type. Nothing else
constructs a `Contributed`, because a type nobody registered has no schema, no target and no
identity field, and every part of the spatial layer that meets one reads at least one of those.

`SpatialType::ALL` is unchanged and is now documented as the *declared* list. `types::known()` is
the union, and it is what a command that must accept a word **the user typed** reads:
`find place --type`, `near --type`, the `<type>/<key>` selector. The drift check keeps reading
`ALL`, so it compares a closed list against a closed document and is exactly as meaningful as it
was. What can still be checked is everything that could be checked before: nothing moved out of
`spatial.yaml` and nothing was added to it that no code implements.

What can *not* be checked by a gate — because the names belong to packages nobody has installed —
is written into `spatial.yaml` as a `contributed_types` block: how such a type is named, what
composes its identity, which tier the host may claim for it, its cost class, and the two things it
does not get. That block is prose about a boundary, and
`crates/ono-cli/tests/spatial_contributed_targets.rs` is what holds the implementation to it.

### 2. The kind of place is the *schema*, not the target

A package may answer one schema under several targets — the SDK's example package answers
`dev.example.echo.item/1` under `echo-item`, `echo-tick` and `echo-refusal`. Those are three ways
of asking one question, and a record that arrives through a pipe carries its schema and never the
target it was asked for. So the type is registered once per schema and a second target naming that
schema joins it.

The name is the schema's display name — `EchoPlace`, `Pod` — which is what a user types after
`--type`. Where that name is already taken, by a declared type or by another package's schema, the
schema id is the name instead: two kinds of place answering to one word would make `--type` a
question with two answers (§27.2).

The consequence is stated rather than hidden: `target_of` — how a place is *re-read* from its
provider (§33.2) — answers `None` where more than one target contributes the schema. The host has
nothing to choose between them by, and re-reading a live place through `echo-refusal` would report
it gone (§2.17, §10.3). A package with one target per schema, which is the shape the
external-system-provider specification is written in, gets the ordinary behaviour.

### 3. Identity is the schema's, the tier is the host's

`Projection::identity_of` already composes a place's identity from the fields the record's schema
calls its `identity`, so a contributed place needed nothing new: `metadata.uid`-shaped identity
works because it is the same mechanism a `uid` or a unit name goes through. §3.1's "identity MUST
NOT be the display name" therefore holds by construction — two resources of one name are two
places, and a renamed resource is one place (external-system-provider §11.1, §11.2, and §35.4's
"spatial entry/traversal does not alter resource identity").

The **tier** is not the package's to state. §10.1 lets a tier be weakened and never strengthened,
and the strongest thing a host can prove about a resource it did not observe is that the identity
lasts as long as the resource does, which is Tier B. `identity_tier` is therefore `Lifetime` for
every contributed type. A `metadata.uid` outliving its pod is a claim for the package's own
`identity_doc` to make, not one the shell may make on its behalf.

`ono.spatial-place/1`'s `canonical_ref` now resolves a contributed schema's identity field names
through the contribution rather than through `builtin_schemas()` alone. A place whose reference
was null could not be revalidated by an action (§33.2), and a place that cannot be revalidated is
not a place §35.4 would accept.

### 4. A contributed target is `expensive`, and it is read under a bound

Enumerating an external system may be a network round trip per object.
`docs/contracts/kuang/contributions.v1.yaml` gives a target contribution a name, a schema, a
summary and an identity note — and no cost and no boundedness. The external-system-provider
specification asks a provider to declare expensive discovery (§12.6) and forbids a "hidden
background inventory service" (§2.4), so the conservative answer is the only honest one:

- a contributed target is `CostClass::Expensive`, so a search reaches it only when it was asked to
  — by `--type`, exactly as `dir` and `file` are reached (§32.1, §33.3). `find place nginx` does
  not fan out to every loaded package.
- a search reads at most `discovery::CONTRIBUTED_SEARCH_OBJECTS` (1024) objects from one, and
  `PluginProvider::snapshot` now honours `Query::max` — it stops taking values and cancels the
  invocation, because §31.14 wants cancellation delivered rather than inferred. Without this a
  package that contributes an endless target hangs `find place` outright, which the example
  package's `echo-tick` proves.

The bound is far above the hundred places a search answers with, so a package whose target holds
fewer objects than that never notices it and the ranking still chooses among more candidates than
it can show. It is a *silent* bound, and that is the one thing here that is owed: the honest
version states that it was bounded, the way ADR-0576's orientation does. That needs a target
contribution that can declare boundedness and cost, which is a change to
`contributions.v1.yaml` and to the handshake, and it is a separate increment.

## Consequences

What a contributed target can now do:

- **`find place --type <Kind> [<text>]`** plans for it, asks it, and answers with places carrying
  the schema the package declared and the provenance the host stamped.
- **`enter`** reaches one both ways §6.3 and §28.2 give: `get <target> | where … | enter` projects
  the record into a place, and `enter <name>` resolves the word against the index once a search or
  a `get` has put the place there.
- **identity binds the resource**, not the word a person reads. `look` on the place reports
  `identity: {uid: …}`, `identity_tier: lifetime`, and a `canonical_ref` an action can revalidate
  through.
- `--type` and the `<type>/<key>` selector accept the contributed name, and the `spatial.unsupported`
  refusal lists it among the types, so it is discoverable rather than only documented.
- `back`, `trail`, pins and `find place --near` work on it, because they work on whatever the
  index holds.

What it still cannot do, and why:

- **`up` has nowhere to go.** A contributed target declares no aggregate space, so the place sits
  in no collection. §36.4 is the declaration that would give it one — id, label, parent domain,
  membership query, supported relations, cost, permissions — and a package cannot make it. `up`
  therefore refuses with `spatial.no_parent` saying *that*, rather than the sentence it used to
  say, which claimed the user had reached the top of this host. A place off this host is not the
  top of it (§2.17).
- **`near` finds nothing and `follow` has nothing to follow.** Both are graph questions, and a
  contributed place has no edges. A package *can* contribute edges — `contributions.relations`,
  §36.1, wired in `crates/ono-cli/src/spatial/contributions.rs` — but a relation shape is written
  `<from>-><to>` in the declared vocabulary of §3.3, so today a package can only assert edges
  between *core* types. Letting a shape name a contributed type is the next gap, and it is a real
  one rather than an oversight: the shapes are read from the manifest at load time and the types
  are learned from the handshake, so the two have an ordering to settle. `near` and `follow`
  reaching a contributed place is that increment, not this one.
- **The scope is the local host.** A contributed place is stamped `host:<this host>` and its
  `place_path` is `local`, which is true of nowhere for a resource in a cluster. The
  external-system-provider specification gives scope a model of its own (§9), and it needs a
  `ScopeKind` the seven of `spatial.yaml` do not carry. Separate increment; nothing here should be
  read as claiming a contributed place says where it really is.
- **A package the session never loaded contributes no type**, exactly as ADR-0583 decided for the
  provider. `find place --type Pod` before `load plugin` refuses with `spatial.unsupported`,
  because a declaration cannot answer a query.

Which tests encode it — `crates/ono-cli/tests/spatial_contributed_targets.rs`, over the real `ono`
binary:

- `should_find_the_objects_of_a_contributed_target_as_places` — `find place ledger --type EchoPlace`
  answers one place carrying `dev.example.echo.place/1`.
- `should_keep_two_contributed_resources_of_one_name_apart_by_identity` — two resources named
  `checkout` are two places with two `spatial_id`s and the `uid`s their records carried. A shell
  that bound a place to its name would answer one.
- `should_enter_a_place_of_a_contributed_target` — `get echo-place | where uid == "u-2" | enter`,
  and `look` reports the place whose identity is `u-2`.
- `should_bind_the_lifetime_identity_of_a_contributed_place_and_not_its_name` — the identity
  carries `uid` and not `name`, the tier is `lifetime`, and `canonical_ref` names the schema.
- `should_not_enumerate_a_contributed_target_that_nothing_asked_for` — an untyped search does not
  fan out to a loaded package.
- `should_finish_a_search_over_a_contributed_target_that_never_ends` — a search over `echo-tick`
  returns.
- `should_say_that_no_domain_holds_a_contributed_place_when_up_has_nowhere_to_go` — the refusal
  names the reason it actually has.

The SDK's example package gained the target `echo-place` and the schema
`dev.example.echo.place/1`: identity `uid`, and `name` a separate field, with two of its three
resources sharing a name. The fixture is the assertion — `echo-item`'s identity is its `seq`,
which is also the only thing about it, so it can prove nothing about a name that is not identity.
`lay_out_echo_package` and `last_json_document` moved into `crates/ono-cli/tests/support/mod.rs`,
because v0.4.1 §39.1 allows a test helper only one definition and two suites now lay the same
package out.

## Alternatives considered

**Open `SpatialType::ALL` and let a package push a name into it.** Rejected: it is the list the
drift check compares against `docs/contracts/spatial/spatial.yaml`, and a list whose contents
depend on which packages are installed makes that check answer a different question on every
machine. Keeping the declared list closed is what lets the second list exist at all.

**One contributed type per contributed *target*.** Rejected on the evidence of the fixture: a
record carries its schema and not the target it was asked for, so `get <target> | enter` and every
other pipe into the spatial layer would have had no way to decide which of a package's targets a
record belonged to. Keying on the schema makes schema-to-type a function, which is what the bridge
needs it to be.

**Name the type after the target.** Rejected for the same reason: three targets answering one
schema would be three names for one kind of place, and `--type` would then be a question about how
the answer was fetched rather than about what it is.

**Give a contributed place a canonical parent — the root, or a synthetic domain.** Rejected:
filing a cluster resource under this host's geography states something about the host that is not
true, and §36.4 already says what a plugin-defined aggregate space must declare before it can hold
anything. An honest `spatial.no_parent` naming the missing declaration is better than a parent
nobody chose.

**Treat a contributed target as `normal` cost, like `service` or `socket`.** Rejected: it makes
every `find place <word>` fan out to every loaded package, which is §2.4's hidden inventory
service arriving through the back door, and it is unrecoverable from the user's side because
nothing in the search says it happened. `expensive` costs the untyped search and is what §12.6
asks for in the absence of a declaration.

**Claim `stable` identity for a contributed type, since a `metadata.uid` is not reused.**
Rejected: §10.1 lets a provider claim only a weaker tier than the ceiling, and the ceiling is a
statement the *host* makes. The host observed nothing; the package's `identity_doc` is where a
stronger claim belongs, and a renderer implying persistence the host cannot vouch for is precisely
what §10.1 forbids.
