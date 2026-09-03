# ADR-0559: A record is acted on by the provider its identity names

- Status: accepted
- Date: 2026-09-03
- Spec refs: §11.5, §17.1, §27.3, §28.3, §31.23, §35.3; ADR-0012, ADR-0422
- Decided by: agent (autonomous)

## Context

`ono.package/1` is identified by `provider + name`, and the contract says why: "a machine can
carry more than one package database and a name alone does not say which one answered". Two
providers serve the `package` target — `linux.packages` over dpkg, `linux.packages.rpm` over the
rpm database — and both are registered on every machine.

`ProviderRegistry::act` resolved the action's *target name* to a provider, and `provider_for`
answers with the first available one. Nothing consulted the identity. So on a machine carrying
both databases — Debian with `rpm` installed is the real case — a record the rpm provider made,
handed down a pipeline to `remove package`, would be acted on by dpkg. Both providers read only
the name out of the identity (`packages.rs::package_name`), so neither could notice.

Found while writing ADR-0422 and deliberately left out of it (AGENTS.md §4). It is not a defect
of either provider: each is right about its own database. It is a routing question, and routing
is the registry's.

The value the identity carries is not the provider's id. `linux.packages` writes `dpkg` and
`linux.packages.rpm` writes `rpm`, because *the database* is what tells two records apart, and
one provider serves that database on both Red Hat and SUSE. So the registry cannot match a
record's `provider` field against `Provider::id()`; the token has to be stated.

## Decision

**A provider states the token its records name it by, and the registry routes by it.**

`Provider::identity_token()` answers `Some(token)` for a provider whose records give that value
for a `provider` identity field, and `None` — the default — for one whose target no second
provider claims. `docs/spec/providers/linux-packages.yaml` declares `identity_token: dpkg` and
`identity_token: rpm` beside the two entries, and the generated conformance suite checks each
provider's own answer against its declaration, so the two cannot drift.

`ProviderRegistry::act` goes through `provider_of(target, object)`:

1. The object's schema is looked up among the schemas registered providers emit, and the value
   its identity gives for a field named `provider` is read from the position that schema declares
   it in. No such field, no routing: this is `provider_for` and nothing changed.
   **A partial identity is not a record's identity.** `add package curl` builds
   `ono.package/1[curl]` — the name a user typed, one value where the schema declares two — and a
   name says nothing about which database it belongs to. Only an identity with a value for every
   declared field is read for a token; anything shorter is a selector, and the ordinary
   resolution stands.
2. **If no provider claiming the target states a token, nothing changed either.** `ono.service/1`
   also identifies by `provider`, and systemd alone answers `service`; there the field is a note
   on the record rather than a choice between answerers, and demanding a token would be
   ceremony.
3. Otherwise the action goes to the provider that states that token, and to no other. Where that
   provider is here but cannot answer, the refusal is its own reason; where it is not here at
   all, the refusal names the token.

**Refusing by name is the answer, not a fallback.** A record made by rpm is about the rpm
database. Performing its removal through dpkg would change a system the record was never about —
the failure §17.1 calls "act on the object you named", and worse than answering nothing.

Reading and resolving are untouched. A `Query` and a `Selector` carry no identity, so there is
nothing there to route by; the first available provider still answers, which is what makes
`get package` work on a machine with one database and either.

## Consequences

- `remove package`, `set package` and `add package` on a piped record act on the database that
  record came from. `crates/ono-provider-api/tests/contract.rs::
  should_act_through_the_provider_the_records_identity_names` holds it with two providers over
  one target, counting which of them was asked.
- `add package curl` and every other command that names a package by hand is unchanged, which
  `crates/ono-provider-api/tests/contract.rs::
  should_act_through_the_first_available_provider_when_a_selector_named_the_object` holds and
  `containers_packages.rs::should_fail_with_permission_denied_when_adding_a_package_unprivileged`
  found the moment it was not true.
- A record from a machine whose provider this build does not have is refused by name rather than
  acted on by a neighbour: `::should_refuse_by_name_when_the_provider_a_record_names_is_not_here`.
- `Provider` gains one method with a default, so no provider outside this repository has to
  change, and a KUANG/11 package adding a second container runtime gets the same routing by
  stating its token (§31.23).
- **No acceptance case, and the reason is worth stating.** The container carries one package
  database, and the shell cannot be made to fabricate a record naming the other: `from json`
  produces maps, not records with an identity, which is the property that makes the pipeline
  trustworthy in the first place. What the product can be asked at the container is that a
  package record names its database, which case `060` already asserts. The routing is held by
  `crates/ono-provider-api/tests/contract.rs`, over two providers of one target, and the
  declaration is held against each provider's own answer by the generated conformance suite —
  which is the pair of referees a registry-level rule can actually have.
- The rule is the same question for every target two providers can claim, which is what the issue
  asked for: it is written once, in the registry, and neither package provider knows about it.

## Alternatives considered

- **Match the record's `provider` against `Provider::id()`.** It needs no new surface, and it is
  wrong: the contract says the field is "the package manager that answered, such as `dpkg`", and
  one provider serves `rpm` on two distribution families. Making the field carry the provider id
  instead would lose the distinction the identity exists for.
- **Have each provider refuse a record it did not make.** It is what the two package providers
  could do alone, and it leaves every future pair of providers to remember. The registry is the
  one place that knows more than one provider claims the target.
- **Route by the record's provenance** (`Provenance::provider`), which does carry the provider id.
  Rejected: an `Action` carries an identity, not a record, and provenance is an observation about
  where a value came from rather than a statement about which object it is.
