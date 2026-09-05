# ADR-0139: What the spatial query layer decides, and what it may not

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §3.6, §6.2, §6.8, §9, §26.4, §27, §29.3, §32.1, §32.2, §33, §34, §35.2, §42.4,
  §45.2, §45.3; ADR-0124, ADR-0131, ADR-0135, ADR-0136
- Decided by: agent (autonomous)

## Context

§45.3 gives `ono-spatial-query` seven responsibilities — `look` plans, neighborhood ranking,
semantic zoom, map graph selection, `find` resolution, cluster construction, cost-aware lazy
queries — and no rules for how they relate to the index below it (§45.2) or the renderer above it
(§45.4). Three questions had to be answered before any of them could be written.

**1. Where does a search get its objects?** §33.1's index "is a cache. Providers remain
authoritative" (§33.2). A search over an empty cache answers nothing, and a search that fills the
cache with everything spends §34's whole 100 ms budget on a directory walk.

**2. What happens when §27.1's order and §27.3's rule disagree?** §27.1 puts a *fuzzy visible
match* (step 4) above the *current-host index* (step 5). §27.3 says a fuzzy match may be followed
only "after interactive confirmation/picking". A script has nobody to confirm, so a selector that
matched approximately at step 4 and exactly at step 5 would resolve to nothing.

**3. What does ranking do with a group that was refused?** §42.4 forbids denied information from
arriving as an empty collection, and S2 gives such a group a §35.2 state and `total() == None`
(ADR-0136). Ranking by size, filtering by type and bounding by budget all assume a count.

## Decision

### 1. The query layer reads the index; the caller fills it, and fills only what the query needs

Nothing in `ono-spatial-query` calls a provider. `neighborhood_of` and `find_places` take a
`&SpatialIndex` and read it; the session (§46) or the command decides what has been observed.

What a search *needs* is planned here, in `discovery::targets_for`, and the plan is narrow by
construction:

- `--type <type>` asks only the targets that serve that spatial type (§42's join table);
- a predicate asks only the targets whose schema declares every root field the predicate reads —
  `--where local.port == 8080` is a question about sockets and about nothing else;
- `dir` and `file` are `query-driven` (§33.3) and `CostClass::Expensive` (§32.1), so a search
  reaches them only when it was asked to by type. `find place nginx` never becomes a filesystem
  walk.

`TargetPlan` keeps the targets it skipped **and the reason**, because §2.17 makes what was not
looked at part of the answer rather than a silent omission.

### 2. An exact answer anywhere outranks an approximate answer earlier

Resolution collects §27.1's steps in order and stops at the first step with an *exact* match:
visible child, visible neighbour, canonical identifier, current-host index, and — only when the
caller asked — a linked host. One match resolves; several are `Resolution::Ambiguous`, which is
§27.2's picker interactively and `spatial.ambiguous_selector` in a script (§29.3).

Approximate matches are collected only when no step matched exactly, and they are returned as
`Resolution::Fuzzy` — a value that **never** becomes a destination. `Resolution::require` turns it
into `spatial.not_found` whose help lists the near misses, which is §40's "actionable next steps"
and the one use §27.3 allows a fuzzy match outside a picker.

### 3. A refused group is carried, never counted

`NeighborhoodGroup`s with no total pass through ranking, filtering and bounding untouched. They
keep their declared position among the exits, they survive a `--type` filter (a filter that hid
them would be exactly the false-empty rendering §42.4 forbids), and no budget applies to them.
`Neighborhood::completeness` then reports `partial`, which is how a user learns that something is
missing rather than absent.

### 4. Ranking is total, and the last two keys are name and identity

Every ordering in this crate — group order, member order, search results — ends in
`(display_name, spatial_id)`. §29.3 requires a script to see a deterministic answer, and
`find place x | take 1 | enter` is only a defined selection if the same index answers the same way
twice.

The ranks above it are the §3.6 inputs: a pin first (§26.4: a pin outranks every heuristic), then
a landmark the index holds, then recency of observation, then the name. For a search the second
key is match quality — exact name, then prefix, then substring — because a user who typed the
whole name meant it.

### 5. The `landmarks` field is real and empty of invention

§3.6 puts `landmarks` on every neighborhood. The landmark *engine* — §26.2's rules and §26.3's
thresholds — is a later phase. Until then the field carries exactly two things: the landmarks the
index was told (`SpatialIndex::set_landmarks`), and a `user_pinned` landmark for every pinned
member, which is the one reason this phase can state on its own because the user stated it
(§26.4). Nothing is inferred, which is §2.16.

## Consequences

- A search is cheap by construction and can say what it did not look at. `explain` and a
  `spatial.not_found` diagnostic can both use `TargetPlan::skipped`.
- A fuzzy match can never move a script, and a fuzzy match can never be silently preferred over
  an exact one. The interactive picker of §27.2 (S6) consumes `Resolution::Fuzzy` and
  `Resolution::Ambiguous` unchanged — it is a rendering of a value this layer already produces.
- Every `near` view of a process is `partial` rather than `bounded`, because a process's `file`
  exit is expensive and stays unloaded until asked for (§32.2, ADR-0135). That is the honest
  answer, and the tests state it.
- The session-held index of §33.1 and §46 is **not** delivered here: `find place` builds the index
  its own query needs and discards it. Repeated views answered from a warm session index are
  §46's session state, which the navigation phase owns.
- Tests encoding it: `crates/ono-spatial-query/tests/{resolution,neighborhood,search}.rs`.

## Alternatives considered

- **Let the query layer call providers.** Rejected: §45.2 makes the index the provider seam and
  §2.16 forbids the spatial layer from becoming a source of truth; a query crate holding a
  provider registry would be a second one.
- **Follow §27.1's order literally, so a fuzzy visible match ends resolution.** Rejected: with
  §27.3 it makes a fully typed name unresolvable in a script whenever anything nearby resembles
  it. The ADR records the departure and the reason; §27.1 is a SHOULD.
- **Give a refused group a count of zero so ranking is uniform.** Rejected outright: it is the
  exact rendering §42.4 and §35.2 forbid.
- **Leave `landmarks` empty until the engine exists.** Rejected: a pin *is* a landmark by §26.4,
  and an always-empty field would teach every caller to ignore it.
