# ADR-0496: A local question is answered from the place, not from the host

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §33.2, §33.3, §34.1, §34.2, §34.4, §35.2, §61.1; v0.4 §3.6, §29.3, §32.1,
  §33.1, §33.2; ADR-0431, ADR-0491, ADR-0492, ADR-0494; issues #85, #87
- Decided by: agent (autonomous)

## Context

§34.4 is one sentence and one obligation:

> A local neighborhood query SHOULD NOT require construction of the complete system graph when
> provider APIs can answer the neighborhood incrementally. Any unavoidable global build MUST be
> visible in `explain` and covered by materialization/performance budgets.

Issue #87 says this is "the structural cause behind several of the observed first-result
failures", and ADR-0491's measurements say where. Against Profile L's socket cardinality — a
hundred thousand listening sockets — `enter network; map --live --json | take 1 | to json` took
**23 s in a release build and 79 s in a debug one**, to draw thirty nodes.

Taking it apart on the reference environment gave two different answers, and only one of them was
what the issue assumed.

## Decision

### 1. The planner already answers locally, and now says so

`neighborhood_of` reads the centre's own edges and nothing else. Two tests hold it to that as an
outcome rather than by watching a call path: the same question asked of an index holding four
objects and of an index holding two thousand and four gives the same exits, the same counts and
the same hidden count, and the §34.1 estimate for it is about the three neighbours rather than
about the two thousand. A planner that consulted the whole index to answer about one place could
not pass either.

That is the half of §34.4 that was already true, and it was untested — which is why the issue
could name it as a cause.

### 2. Two thirds of the Profile L cost was not a global graph build; it was a quadratic insert

`MapHorizon::place` deduplicated by scanning the vector it was building. A horizon of a hundred
thousand places therefore cost five billion comparisons, and that — not the graph's size — was
sixty of the seventy-nine debug seconds. The previous commit indexes the insert; the figures fell
to **3.4 s release and 18.7 s debug**, with the dedup key and the ordering unchanged.

It is recorded here because it is the kind of finding an issue about architecture hides: §34.4
names a design property, the measurement named a data structure, and fixing the data structure was
five lines. The recorded baseline moved with it: `spatial.map_first_frame` at Profile L went from
**25 748 ms p95 to 3 557 ms**, and at Profile M from 1 090 ms to 818 ms.

### 3. What remains is an acquisition, and §34.1's refusal now covers it

The 3.4 s that is left is the *observation*: a hundred thousand socket records read from the
provider and absorbed into the index before anything is drawn. `enter network; look --json` costs
the same 3.4 s, which is how one can tell it is the observation rather than the map.

ADR-0494's estimate now catches it. `map` at Profile L answers

```
Ono-Sendai-E1401 spatial.cost_refused `map` would acquire about 808176 moderate relations over
101022 candidates, which is beyond the interactive budget of 250000
```

in 3.4 s instead of drawing a picture in 23 s. That is §33.3's rule — *"Ono MUST refuse or switch
to a bounded lower-detail strategy rather than silently appear hung"* — met at the cardinality
§33.3 names it at, and it is what un-ignores ADR-0491's Profile L watchdog.

### 4. What is still owed, stated rather than implied

Both of §33.2's remaining misses are now one thing: **an orientation query asks a provider for
every object when it needs a bounded view of them.** COMPUTE pays ~400 ms for the systemd
enumeration (569 units, three D-Bus round trips each, already made concurrent, and `external` by
construction); NETWORK at Profile L pays 3.4 s for a hundred thousand socket records. Neither is
the planner's, neither is the cardinality the profile is named for, and both are §34.4's second
half.

Fixing it means an observation that takes a bounded answer and still reports the true count — the
count is what an exit shows, and a bounded observation that reported a bounded count would trade a
latency defect for an honesty defect (§2.17, §3.6). That is a change to the provider observation
contract rather than to the planner, and it is left as the `#[ignore]`d
`should_hold_every_time_to_first_result_target_of_the_reference_targets_table` with the cause named
in the file.

### 5. `explain` is not touched, and that is a constraint rather than a decision

§34.4 requires any unavoidable global build to be visible in `explain`. `explain` lives in
`ono-command`, which this branch may not edit — it is being changed elsewhere in parallel. The
estimate that would populate it exists (`ono_spatial_query::cost`), and wiring it into
`StagePlan` is a small change in a crate this branch must leave alone. It is reported rather than
done.

## Consequences

Easy: the Profile L watchdog is a live test rather than an ignored one, and `map` at a
pathological cardinality refuses with a figure instead of drawing a picture a minute later.

Hard: the watchdog's margin is 1.5× — 20 s of §33.3's 30 s in a debug build — where ADR-0431's
Profile M watchdog had twenty. The refusal arrives *after* the observation, so the margin is the
observation's, and it closes when §4 above does. On the release build the reference environment
names, the same run is 3.4 s and the margin is eight.

Also hard: `Ono-Sendai-E1401` now fires on a real host, which means `map` at a place with a hundred
thousand members refuses where it used to answer slowly. That is the behaviour §33.3 asks for and
it is a behaviour change: a user who wants the picture asks for it with `--all`, which
ADR-0494 made the request path.

Encoded by `crates/ono-spatial-query/tests/cost.rs::should_answer_a_local_neighborhood_question_without_projecting_every_domain`,
`::should_keep_the_work_of_a_neighborhood_question_within_its_declared_cost_class`, and
`crates/ono-cli/tests/spatial_first_output.rs::should_answer_or_refuse_within_the_interactive_budget_on_the_profile_l_fixture`.

## Alternatives considered

**Bound what the observation absorbs, in this increment.** It is the fix, and it needs the true
count to survive the bound or the exits start lying about how many neighbours a place has. That is
a contract change to `Observed`/`Exit` in the middle of the most heavily tested area of the shell,
and it belongs in an increment of its own rather than at the end of one about the planner.

**Bound the *horizon* instead — take the first few hundred members and count the rest as hidden.**
Cheaper, and it fixes the wrong number: the horizon is built from an observation that has already
happened, so the 3.4 s is already spent by the time the bound would apply.

**Make the refusal fire before the observation.** §34.1's estimate has no candidate count before
anything has been observed, and inventing one from the profile would make the refusal a property
of the fixture rather than of the system. The cache state §34.1 lists as an input is exactly this,
and it only helps the second query.

**Leave the Profile L watchdog ignored because its margin is 1.5×.** An ignored test that would
pass is a requirement nobody is checking, and AGENTS.md §7 asks an `#[ignore]` to say why a test
*cannot* run. "It is close to its budget" is a finding to report, not a reason to stop running it.
