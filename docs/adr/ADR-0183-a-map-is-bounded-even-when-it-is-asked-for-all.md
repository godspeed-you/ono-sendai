# ADR-0183: A map is bounded even when it is asked for all

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §6.9 (`map --all`), §6.2 (`near --all`), §8.2 (clustering), §22 (`SpatialMap`,
  `MapCluster`, `HiddenSummary`), §23.6 (large maps), §34.2 (view budgets), §9.3 (`find`), §2.9
- Decided by: agent (autonomous), at the user's instruction to settle it
- Supersedes the open question recorded in ADR-0165 §"Spec deviation"

## Context

`crates/ono-cli/tests/spatial_map_missing.rs::should_show_more_than_the_default_when_the_map_is_asked_for_all`
asks two things of `map --all` at the processes collection of a host with ~300 processes:

1. `--all` draws strictly more nodes than the default — delivered and green;
2. the resulting document contains, **as a node**, one specific process the test spawned a
   moment earlier.

The second cannot be true while three other requirements hold:

- `spatial_contracts_missing::should_bound_the_default_map_to_its_node_budget` requires
  `map --json --all` to stay inside `spatial.map.node_budget` (100), from §34.2;
- §34.2 prohibits unbounded graph rendering outright, and §23.6 forbids drawing an unreadable
  all-node graph;
- §22's `MapCluster` carries `members: Int` — a **count**, not a list — so an object a cluster
  stands for is not named anywhere in the document.

The only implementation that satisfies it is a clock-relative ranking that promotes the newest
object, and that makes `should_only_remove_edges_when_a_relation_filter_narrows_the_map` and its
sibling compare two maps of two different moments: measured, 19 of 100 nodes differed between two
consecutive runs. One test would pass and three would fail, and the shell would rank by recency
rather than by relevance (§23.1).

## Decision

1. **`--all` widens the bound; it never abolishes it.** It means what §6.2 says for `near --all`:
   ask for the complete currently known one-hop set, and accept that it may be expensive. The
   projection then still ranks (§23.1), still clusters (§8.2) and still stops at
   `spatial.map.node_budget`, because §34.2 permits no unbounded rendering and §23.6 permits no
   unreadable graph.
2. **Completeness is preserved by accounting, not by enumeration.** Everything the projection saw
   and did not draw is counted in `hidden` (`count`, `clustered`, `aggregated`). A map may
   therefore never *silently* drop anything — §23.6's actual requirement — but it does not
   promise that a named object appears in it.
3. **A map is not a lookup.** Finding a specific object is `find place` (§9.3) or entering the
   collection it lives in. A map answers "what is around here and what matters", which is why
   §23.1 ranks by current place, exits, landmarks and strongest relationships rather than by
   arrival time.
4. **Where the budget is not exceeded, the map does contain every member** — that is the same
   claim the test was making, and it holds at any place whose neighbourhood fits, which is the
   ordinary case for an object place.

## Consequences

- The test is corrected in the commit that carries this ADR, and keeps both of its claims:
  `--all` draws strictly more than the default at the large collection *and* discloses what it
  left out (`hidden.count > 0` where the budget bites); and at a place whose neighbourhood fits —
  the test's own process, whose twelve children are well inside the budget — `map --all`
  contains the spawned child **as a node**, verbatim the property the original assertion was
  reaching for. What it can no longer demand is that a 300-object collection name one arbitrary
  member, which §22 and §34.2 make impossible for any implementation.
- `spatial.map.node_budget` stays the one number that bounds every map, `--all` included, so a
  user who needs more raises the setting rather than discovering that one flag ignores it.
- The three tests that measure filtering and budgets keep a stable, relevance-ranked map.

## The second half of the decision: expansion is a view action, and observes nothing

While correcting the test, two more of the map suite's tests turned out to flake for one shared
reason: each `map` invocation observes the system again, so two maps — even two written by one
shell run — describe two moments. On a build host that meant a compiler process appearing between
them, and then `filtered ⊆ complete` was false about objects no filter had touched, and
"expanding yields exactly the members" was false by the eight processes that had started
meanwhile.

§8.3 already settles it: **expansion is a view action**, and `enter` is navigation. A view action
projects the observation the reader is looking at. So `map --expand <cluster>` reuses the
observation the session's last map of that place was drawn from (`SpatialSessionState::remember_map`
/ `remembered_map`); every other map observes and remembers. Where no such map exists — the first
command of a session expanding a cluster id it cannot have seen — it observes, which is the honest
answer to a question about a map that was never drawn.

The two filter tests are moved to a place whose neighbourhood the test itself owns (its own
process and its children) rather than the host's process collection, because what §43.2 measures —
that filtering removes and never invents — needs a stable population, not three hundred processes
of which a hundred are drawn.

## Alternatives considered

- **Rank the newest object first.** Rejected: it satisfies one assertion, breaks three, and
  replaces §23.1's priority order with arrival time — a map that reorders itself every second is
  not an orientation aid.
- **Let `--all` ignore the budget.** Rejected: §34.2 prohibits unbounded rendering and
  `should_bound_the_default_map_to_its_node_budget` reads `--all` explicitly. It would also make
  `map --all` at SYSTEM an unbounded enumeration of every object on the host, which §2.9 and
  §35.1 both refuse.
- **Give `MapCluster` a member list.** Rejected as a contract change to §22 that the spec spells
  out as a count, and it would make a cluster of 200 processes carry 200 names into every
  document — the opposite of what clustering is for. `--expand` already yields the members when a
  reader asks for them (§8.3).
