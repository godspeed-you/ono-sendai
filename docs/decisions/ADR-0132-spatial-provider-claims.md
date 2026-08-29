# ADR-0132: The §42 provider claims live in the provider registry, and a claim may only be weaker than the type allows

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §10.1, §11.3, §26, §32, §35.2, §41, §42, §50 Phase S2; v0.2 §47
- Decided by: agent (autonomous, `S2`)

## Context

v0.4 §42 says that "a provider that exposes objects to spatial navigation MUST pass additional
conformance tests beyond ordinary schema validity" and lists eight required claims — identity
strategy, canonical parent strategy, supported relationships, freshness strategy, event support,
permission behaviour, cost class and landmark-relevant metrics/states. It fixes no file, no
shape and no vocabulary for them, and §41's registry describes the *world* (spaces, relations,
landmarks) rather than the providers that fill it.

Three things had to be decided: where a claim lives, what its values may be, and what a claim is
checked against — because a claim nobody checks is documentation, and §42's own Intent is
conformance.

## Decision

**Placement.** The claims are a `spatial:` block on the provider's own entry in
`docs/spec/providers/*.yaml`, beside its targets, capabilities and schemas. That registry is
already level 6 of the authority order and already the thing `crates/ono-cli/tests/providers.rs`
holds against the built registry; a second file would let two documents disagree about one
provider. A provider whose targets name no spatial object type declares no `spatial:` block.

**Which providers must declare.** `ono_spatial_core::types_of_target` is the single join between
the v0.2 target vocabulary (`process`, `socket`, `dir`) and `SpatialType`. A provider must
declare claims exactly when that function maps one of its targets to a spatial type. `env`,
`package`, `dns`, `log`, `journal`, `image`, `plugin` and `port` map to nothing: §7 gives them no
place, so they are values in the typed shell and never places.

**Vocabularies.** `identity_strategy` is one of `identity_tiers`; `cost_class` one of
`cost_classes`; `permissions` a subset of `permission_states`; `freshness` one of a new
three-value vocabulary declared in `docs/spec/spatial/spatial.yaml` under `provider_claims`
(`on_demand`, `cached`, `event`); `events` a boolean; `canonical_parent` a mapping from spatial
type to the ordered chain `up` follows; `relationships` a list of ids from `relations.yaml`;
`landmark_metrics` a list of field names of the provider's own schemas.

**The identity rule.** *A provider may claim a weaker identity tier than
`SpatialType::identity_tier()` allows, never a stronger one.* Where a provider serves several
spatial types the ceiling is the **weakest** of them, because one claim is a promise about every
object the provider exposes. `linux.mountinfo` therefore claims `lifetime` although a filesystem
UUID is Tier A: its mounts are Tier B, and an entry that served both could not honestly promise
more. `ono.shell` claims `lifetime` for the same reason (jobs are Tier B, hosts Tier A).

**Enforcement.** `xtask::contracts::check_provider_claims` runs in every `spec-check`, and it
checks the things a reader cannot: the tier rule above; that the declared canonical-parent chain
equals `parent_rules(type)` followed by the collection space the geography falls back to; that
every claimed relation is declared in `relations.yaml` and touches a type the provider serves;
that `permissions` is non-empty and drawn from the six states; that every landmark metric is a
field of a schema the provider declares.

**Specialised types answer to their general type's relations.** `SpatialType::is_a` makes a
`Listener` and a `Connection` a `Socket`, and a `Directory` a `File`. §41.2's relation table
names the general type (`process.owns_socket` runs to a `Socket`) while §14.3, §14.4, §15.4 and
§15.5 make the *places* the specialisations. Declaring one relation per specialisation would
double the table and lose the fact that they are one relation.

## Consequences

- `crates/ono-cli/tests/spatial_contracts_missing.rs::should_declare_the_spatial_claims_on_every_provider_that_feeds_the_spatial_index`
  is green and no longer ignored.
- A new provider that serves a spatial target cannot be merged without stating how its objects
  are identified, where `up` goes, what it relates, what it costs and how it behaves when
  refused. `spec-check` fails until it does.
- Changing a canonical-parent rule in `ono-spatial-core` now fails `spec-check` until every
  provider that serves the type restates the chain — which is the point: `up` means one thing.
- `process.connects_to`, `service.depends_on` and `socket.accepts_connection` are declared in
  `relations.yaml` and claimed by nobody. They are recorded as underived in ADR-0135; the first
  is redundant with `process.owns_socket` to a `Connection`, and the other two have no provider
  evidence today.

## Alternatives considered

- **A separate `docs/spec/spatial/providers.yaml`.** Rejected: two documents that describe the
  same provider will disagree, and the existing registry already carries the provider's contract.
- **A per-type identity claim instead of one per provider entry.** Rejected: the test §42's own
  wording implies (`identity_strategy` a single string) and the registry entry are both
  provider-shaped, and splitting a registry entry to sharpen a claim would break the
  declaration-versus-registry equality `providers.rs` enforces.
- **Taking the ceiling as the *strongest* type served.** Rejected: it would let
  `linux.mountinfo` claim `stable` and thereby promise that a mount point survives being
  unmounted, which §10.1 forbids.
