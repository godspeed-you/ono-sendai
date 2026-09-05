# ADR-0131: The spatial index refuses stale answers rather than serving them

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §2.16, §2.17, §3.1, §9, §12, §13, §20.4, §26.3, §27.1, §27.3, §29.3, §32.2,
  §33.1, §33.2, §33.3, §33.4, §35.2, §42.1, §42.3, §45.2, §49.4
- Decided by: agent (autonomous)

## Context

§45.2 gives `ono-spatial-index` six responsibilities and one rule: "It MUST treat providers as
truth and revalidate mutation targets." §33.2 repeats it: "The index is a cache. Providers remain
authoritative. Actions MUST resolve/revalidate live objects before mutation."

The index does not own providers — the provider registry is asynchronous and lives in `ono-cli`
— so it cannot revalidate anything by itself. Something has to decide what "revalidate" means for
a synchronous cache that cannot call out.

Three smaller questions come with it: what a place is *called* (§3.1 keeps the display name out of
identity but does not say what it is), what it *answers to* (§9's discovery-before-naming does not
mean naming is impossible), and how an exit that is expensive to answer is shown (§32.2).

## Decision

1. **`resolve_for_action` refuses rather than answers.** A mutation target whose observation is
   older than its class's TTL comes back as `spatial.stale` (§40) naming when it was last seen;
   one the index does not hold comes back as `spatial.not_found`. The caller re-observes through
   the provider and registers the result, and the same call then succeeds. That is the honest
   shape of "revalidate" for a cache: it cannot look again, so it declines to pretend it has.
2. **Registration is where §42.1 is enforced.** The index keys every provider reference by
   `(scope, ObjectRef)`. A second observation of that pair with a different `SpatialId` is
   `spatial.identity_conflict`, refused at the door rather than held as two places — which would
   give `back`, pins and every map edge two answers to one question. A *different* scope is a
   different object, so the same uid or unit name in two containers registers cleanly as two
   (§16.2).
3. **The display name is the schema's own word for the thing** — `name`, then `path`, `target`,
   `address`, `destination`, `id`, then the provider reference's label. §12 prints
   `PROCESS / nginx / 1842` and §13 prints `SERVICE / nginx.service`; the first default-view
   column outside the identity (what `ObjectRef::of` gives) is a service's *state*, which is a
   fact about it and not its name. This is the spatial layer's answer to the open label question
   in `docs/STATE.md`, scoped to spatial places; it does not change `ObjectRef`.
4. **An object answers to its display name and to every value of its identity fields** — the pid,
   the unit name, the inode — lowercased. The scope and the boot identity are excluded: a boundary
   is not a name, and indexing it would make every object on the host answer to one word.
   Alias lookup is exact and case-insensitive; fuzziness is the query layer's (§27.3), because the
   index must not make a candidate disappear before §27.2's ambiguity rules have seen it.
5. **Search results are ordered by identity, not by relevance.** Ranking is `ono-spatial-query`'s
   job (§45.3), and a deterministic order here is what makes §29.3's "deterministic ambiguity"
   possible at all.
6. **A relation summary lists every declared exit of the object's type, including the empty ones.**
   §2.17: an exit missing from a place view is indistinguishable from an exit that does not exist.
   An `expensive` relation with no known edges is listed as a discoverable but unloaded exit —
   `PermissionState::Unknown` with "available on request" — which is §32.2 exactly, and is
   deliberately not `Empty`, because §35.2 keeps those distinct.
7. **An edge whose other end the index does not hold is not summarised as a neighbour.** §42.3
   forbids dangling internal ids and §43.2 requires every rendered edge to reference an existing
   node or an explicit off-map endpoint. Until off-map endpoints exist as objects (§14.5, a later
   phase), such an edge is held but not shown.

## Consequences

- The seam between the index and the providers is a refusal, which means it is testable without a
  live system: `crates/ono-spatial-index/tests/index.rs` proves both the refusal and the recovery.
- `ono-spatial-index` depends on `ono-provider-api` for `ObjectRef` and on nothing that observes.
  It cannot become "an undocumented source of system truth" (§2.16) because it has no source.
- Callers must re-register after acting. That is a small burden on `ono-cli` and the price of the
  index never being wrong about how old its answer is.
- Point 3 leaves `ObjectRef::of`'s label rule alone, so the pre-existing inconsistency noted in
  `docs/STATE.md` between `ObjectRef::of` and `ono_graph::label_of` is unchanged rather than made
  worse — a third rule was not added; the spatial one is derived from the schema's own fields.

## Alternatives considered

- **Serving a stale entry with a freshness flag and letting the caller decide.** Rejected for
  mutations specifically: §33.2 says MUST revalidate, and a flag that a caller may ignore is not a
  revalidation. Reads *do* get the flag — `SpatialIndex::freshness` — because a stale read is
  honest as long as it says so, and §33.4 requires `inspect` to reveal exactly that.
- **Holding an asynchronous provider handle in the index.** Rejected: it would make the index a
  second place providers are called from, which is the concentration §45 exists to avoid, and it
  would make every index operation async for the sake of one.
- **Fuzzy matching in the index.** Rejected: §27.2's ambiguity rules and §27.3's fuzzy matching are
  one design, and splitting them across two crates would let the index silently pick a winner
  before the rules that make ambiguity visible had run.
