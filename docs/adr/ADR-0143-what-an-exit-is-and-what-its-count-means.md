# ADR-0143: What an exit of a canonical place is, and what its count means

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §2.9, §2.17, §3.6, §4, §5, §6.2, §7, §24.2, §32.1, §33.3, §34.2, §35.2, §42.4,
  §45.3, §45.6, §47; ADR-0126, ADR-0128, ADR-0139
- Decided by: agent (autonomous)

## Context

`ono_spatial_query::neighborhood_of` projects what surrounds an *observed object*: the index knows
its edges and the ranking decides which to show. A canonical space has no edges. It is declared
geography (§4.1), it is in no index, and `SpatialIndex::relation_summary` answers `[]` for it — so
the root, the six domains and their collections had no neighborhood at all.

§7 says what each domain "MUST provide access to" but not what a group of a place *is*: whether a
group named `services` at COMPUTE stands for the services collection, or for the services. The two
readings give different counts, different `hidden_count`s, and different answers to `near`.

## Decision

**An exit is named after the place it leads into, and its members are the places inside that
place.** §24.2 fixes it:

> When `look` displays `children 14`, those group labels MUST be valid navigation or query targets
> where practical: `enter children`.

The group is labelled `children`, it counts fourteen children, and `enter children` enters the
collection. So, for a canonical space:

- one exit per served child space, labelled with the child's own label;
- its members are the places *inside* that child — the objects the child holds where it holds
  objects, its own served children where it holds only places;
- plus, where the space itself holds objects rather than places, one exit of its own contents.

At the root the six exits are the domains and their members are the collections each domain holds;
at COMPUTE the exits are `processes`, `services`, `jobs` and `cgroups` and their members are the
processes, the services, the jobs; at `compute/processes` the one exit is the collection's own
contents. A count is therefore always the number of members and `hidden_count` is always what the
budget left out — there is no second meaning for either.

**A count and a state are read together.** `ono.neighborhood-group/1` carries both, and a count is
a claim about the system only where the state is `available` or `empty`. Where the objects behind
an exit could not be read the state says why, the count is `0` because that is how many places the
answer carries, and the text renderer prints the state in the count's place — "services —
unsupported: no provider answers for services", never "services 0" (§35.2, §42.4). The exit itself
stays: an unavailable domain "remains visible" (§4), and `enter containers` is still a move.

**`navigable` is data.** §24.2 forbids a renderer from implying an exit that is not one, so the
group states whether `enter <label>` is a move rather than letting the renderer guess. A
collection's own contents group is not an exit out of itself.

**The view budget is divided among the exits.** §34.2 budgets the *view* — "interactive map 100
nodes", which §47 spells `spatial.map.node_budget = 100` — not the group. A place with one exit
spends the budget on it (standing in `identity/users` and being shown eight of forty accounts is a
list that hides its subject); a place with six divides it, which is what keeps the root horizon
bounded however many devices a host has. `--limit` is the user's number and wins; `--all` lifts the
bound; `near --limit <n>` bounds the *answer*, not each exit of it.

**Which provider target feeds which space is a planning decision** and lives in
`ono_spatial_query::discovery::source_of_space`, beside the search planner that already answers
"which targets can hold this". The shell asks; the plan says what to ask. A space whose objects no
target serves — `network/addresses`, `compute/cgroups`, `network/namespaces` — declares an empty
target list and reports `unsupported`, which is how the place stays visible and honest. A space
whose enumeration is expensive — `storage/directories`, which §33.3 makes query-driven — is not
walked by an orientation command and reports `unknown: available on request`.

## Consequences

- `look` at the root costs two provider queries (containers, devices); the other four domains are
  answered from the declared geography alone, which is what §34's 50 ms `look` budget needs.
- `near` at the root answers with the collections behind the six domains, each naming the domain it
  was reached through; `near` at a collection answers with its objects.
- The type table `source_of_space` matches spatial types exactly rather than through
  `SpatialType::is_a`, because `ono.socket/1` feeds both `listeners` and `connections` and a
  listener is not a connection (§14.3, §14.4). Where a space genuinely holds a family — DEVICES
  holds every kernel-visible device (§7.7) — the family is written out.
- Declared geography keeps the registry's order; observed objects are ranked (§3.6).
- Tests that encode it: `spatial_topology_missing::should_list_exactly_the_six_canonical_domains_when_looking_at_the_system_root`,
  `…::should_offer_the_{compute,network,storage,identity}_groups_the_spec_names_when_entering_*`,
  `…::should_bound_the_root_horizon_instead_of_listing_every_known_object`,
  `…::should_bound_the_neighborhood_and_count_what_it_hides_when_a_place_has_many_neighbors`,
  `…::should_distinguish_an_unavailable_group_from_an_empty_one_when_a_domain_has_no_provider`,
  `…::should_show_the_users_the_user_provider_answers_for_when_entering_identity_users`,
  `spatial_map_missing::should_mark_a_group_as_an_exit_only_when_it_can_be_entered_when_look_lists_groups`.

## Alternatives considered

- **A group's member is the child place itself, and its count is what lies behind it** — rejected:
  it gives `count` and `members` two different subjects, so `hidden_count` would report neighbours
  the view never hid.
- **A group's members are the child's contents, recursively** — rejected at the root: the six
  domains would list every process on the host, which §7.1 forbids in one sentence.
- **A fixed eight members per group** — rejected: it is a per-group budget where §34.2 states a
  per-view one, and it hides `root` among forty accounts.
- **Leaving a space with no serving target out of the view** — rejected by §4 and §2.17: an
  unavailable domain remains visible with its state.
