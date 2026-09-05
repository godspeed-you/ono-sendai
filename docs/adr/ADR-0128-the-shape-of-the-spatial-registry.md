# ADR-0128: The shape of the four spatial registry documents

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §3.3, §3.5, §3.7, §4, §7, §8.1, §11.5, §22, §26, §32.1, §33.4, §34, §35.2,
  §41, §41.1, §41.2, §41.3, §47; v0.2 §27, §47
- Decided by: agent (autonomous)

## Context

ADR-0126 fixed *where* the spatial registry lives (`docs/contracts/spatial/`). §41.1 and §41.2 fix
the required fields of `spaces.yaml` and `relations.yaml` by example, and §41 names
`spatial.yaml` and `landmarks.yaml` without saying what is in them. §41.3 says what all four are
for: "completion relation names; `help spatial` content; parser fixtures; relation compatibility
checks; map legends; SDK enums; conformance tests; documentation tables".

That list only works if the four documents share one vocabulary. §41's Intent is explicit about
the failure mode: without machine contracts, "renderer, provider, parser and documentation drift
into different definitions of the world".

## Decision

1. **`spatial.yaml` is the subsystem's vocabulary document.** It carries every closed list the
   other three and the implementation must agree on, each with its spec reference: the object
   types (§3.3), identity tiers (§10.1), scope kinds (§3.2), movements (§20.1), confidence
   (§11.5), directions (§41.2), permission states (§35.2), freshness states, completeness, zoom
   levels (§8.1), cost classes (§32.1), the §47 settings with their defaults, and the §34
   budgets. It also records where the spatial error family lives (ADR-0125).

2. **`spaces.yaml` declares `object_type` as §41.1's example spells it** — for a collection, the
   type of its members (`compute.services` / `Service`); for the root and the six domains, the
   aggregate place type of §3.3 (`System`, `Compute`, …). Two further fields carry what §41.1's
   example leaves implicit:
   - `member_type`: what a user finds *inside* the place, or null for a place that contains only
     other places. For a collection it equals `object_type`; `containers` and `devices` hold
     their objects with no intervening collection, so theirs differ.
   - `schema`: the `docs/contracts/schemas/` id of the records the place is built from, or **null**
     where no provider answers for them yet. `compute.cgroups` is declared with a null schema
     rather than omitted, because §4 requires an unavailable part of the geography to stay
     visible with a state instead of disappearing.
   `kind` (root | domain | collection) records the zoom level of §8.1 the place sits at.

3. **`relations.yaml` carries §41.2's seven fields plus `cost_class`** (§32.1), because §32.2's
   lazy expansion and §34.2's view budgets both need to know before fetching whether a relation
   is cheap. `confidence` takes the §11.5 vocabulary or §41.2's own `exact_or_provider_declared`,
   which means "exact where the provider observed the edge, the provider's own claim otherwise".
   **Relation labels are unique per source type, not globally**: `follow process` means the
   obvious thing from a service, a user and a container alike, and `follow socket` / `follow
   owner` are the two readable ends of one edge.

4. **`landmarks.yaml` declares exactly the fourteen reasons of §3.7 and no more.** The built-in
   set is closed; a further reason comes from a KUANG/11 package and identifies its source
   (§26.5). Each entry carries its domain, its evidence, its severity and — where the rule is a
   measurement rather than a state — a threshold with the metric, the comparison, the default
   and **the setting that changes it**, which is how §26.3's "inspectable and configurable"
   becomes true rather than aspirational.

5. **`cargo run -p xtask -- spec-check` enforces all of it**: parents resolve, ids are unique,
   exactly one space has no parent, every type comes from `spatial.yaml`'s vocabulary, every
   schema exists under `docs/contracts/schemas/`, every direction/confidence/cost class is from the
   fixed list, the fourteen reasons are present and closed, and every threshold default equals
   the default of the setting that configures it.

## Consequences

- The registry is checkable without the spatial crates existing, so the contract could land
  before the implementation, as AGENTS.md §7 step 1 asks.
- The drift check against the *implementation* — a served space nothing declares, a declared
  relation nothing serves — is a second, separate check; it arrives with `ono-spatial-core` and
  with the commands that serve them (§41, tested by
  `spatial_contracts_missing.rs::should_serve_exactly_the_canonical_spaces_the_registry_declares`).
- Five `spatial.landmarks.*` settings exist beyond §47's eleven required keys. §47 is a
  required-minimum list, not an exhaustive one, and §26.3 requires these to be configurable.
- Adding an object type means editing `spatial.yaml` first. That is the intended friction.

## Alternatives considered

- **`object_type: ProcessCollection` for collections**, keeping `object_type` strictly "the type
  of the place itself". Rejected: §41.1's own example writes `object_type: Service` under
  `id: compute.services`, and inventing twenty-one collection types would put a vocabulary in
  the registry that no provider, schema or SDK ever names.
- **Omitting spaces no provider serves** (`compute.cgroups`, `compute.workloads`). Rejected for
  cgroups: §7.2 makes it a MUST and §4 requires it visible with a state. Accepted in spirit for
  workloads, which §7.2 makes a MAY — it is declared with `status: planned` so the shape is
  recorded without promising a space nothing serves.
- **A `spatial-errors.yaml`, as §41 recommends.** Already rejected by ADR-0125; `spatial.yaml`
  points at `errors.yaml` instead so a reader following §41's file list still arrives.
