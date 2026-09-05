# ADR-0201: `find place --where` reads the index, not only what a provider just answered

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §6.8, §33.1, §35.2, §37.1, §42.3; ADR-0140, ADR-0193, ADR-0197
- Decided by: agent (autonomous), delivering v0.4 §50 Phase S11

## Context

§6.8: the spatial search "MUST search the spatial index **and** provider registries rather than
blindly grep rendered text". ADR-0140 settled that `--where` reads the *object's* fields.

The implementation read only half of that sentence. `find` asked the providers its plan named,
evaluated the predicate against those records, and made the survivors the subjects of the search.
A place the session was already holding that no canonical provider serves was therefore findable
by name and invisible to a property:

```text
ip addr | count | to text                                  → 23
find place --type address | count | to text                → 23
find place --type address --where family == "inet" | count → 0
```

The same gap hides every service on a host whose service manager only a v0.3 adapter can read —
the situation `docker/acceptance/cases/091-spatial-unknown-web-service.case` puts the shell in,
and the one §44.2 asks the operator to walk by selecting a unit on `state == "active"` without
ever typing its name.

## Decision

**A place the session holds is a subject of the search when the predicate holds for what the
provider last said about it.** After the planned targets have been asked, `find` puts the same
predicate to `SpatialSessionState::record_of` for every index entry that is not already a
subject, and adds those that satisfy it.

Three properties this keeps:

- **Nothing is re-observed.** The record is the one the object was projected from — §2.16 keeps
  the fact with the provider, and the search does not read the system a second time.
- **A record the predicate cannot be evaluated against does not match**, exactly as for a
  provider's answer: an absent field is not an error, it is a place the search says nothing about
  (§4, §35.2).
- **Ranking and bounding are unchanged.** The extra subjects join the same
  `find_places` request, so §34's budget and §26.4's pins decide what is shown.

## Consequences

- `find place --where` answers about adapted observations, tombstoned-but-not-ended places and
  anything else the session has seen, which is what makes §44.2 walkable on a host without the
  canonical provider.
- The predicate is evaluated once per index entry that no target answered for. The index is the
  session's own working set, so this is bounded by what the user has already looked at.
- Encoded by `spatial_contracts_missing::should_find_a_place_by_its_properties_when_the_index_holds_it_and_no_provider_serves_it`
  and by `docker/acceptance/cases/091-spatial-unknown-web-service.case` (`44.2d`).

## Alternatives considered

- **Re-observe each index entry through its canonical reference before testing the predicate.**
  Rejected: it is a query per place, it breaks §34, and for the case that motivated this — an
  object no canonical provider serves — it answers nothing.
- **Keep the records on the index rather than beside it in the session.** Rejected as unnecessary:
  the session already holds the last record per place for the relationship graph and §24.1's
  summary, and one owner of that memory is better than two.
