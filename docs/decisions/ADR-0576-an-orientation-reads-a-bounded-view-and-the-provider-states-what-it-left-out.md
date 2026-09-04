# ADR-0576: An orientation reads a bounded view, and the provider states what it left out

- Status: accepted
- Date: 2026-09-04
- Spec refs: v0.4.1 §2.17, §33.2, §33.3, §34.1, §34.4, §36.1, §55.1, §66.4, Appendix A; v0.4 §32.1,
  §35.2, §42.4; ADR-0418, ADR-0431, ADR-0491, ADR-0494, ADR-0496, ADR-0561
- Issues: none open (ADR-0496 §4 left this as the tranche's remaining owed work)
- Decided by: agent (autonomous)

## Context

ADR-0496 ended by naming exactly one thing as still owed, and stated it rather than implying it:

> Both of §33.2's remaining misses are now one thing: **an orientation query asks a provider for
> every object when it needs a bounded view of them.** … Fixing it means an observation that takes
> a bounded answer and still reports the true count — the count is what an exit shows, and a
> bounded observation that reported a bounded count would trade a latency defect for an honesty
> defect (§2.17, §3.6).

Three of §33.2's four reference targets were missed on the reference environment because of it:
`enter compute; look` at **595 ms against 150**, its live map at **805 against 500**, and the
Profile L live map at **3 514 against 1 500**. The COMPUTE figure is six hundred systemd units at
three D-Bus round trips each, paid on every orientation; the Profile L figure is a hundred
thousand socket records decoded, projected and absorbed before anything is drawn. Neither is the
cardinality the profile is named for. Both are §34.4's sentence: *"A local neighborhood query
SHOULD NOT require construction of the complete system graph when provider APIs can answer the
neighborhood incrementally."*

## Decision

### 1. The orientation asks for a bounded view

`crates/ono-cli/src/spatial/view.rs` asks each target `Query::target(t).limit(n)` instead of
`Query::target(t)`. The bound is `limits.orientation_objects`, declared in
`docs/spec/hardening/limits.yaml` and in the shell's catalogue like every other limit (§55.1,
§52.2), and it defaults to **128** — above §34.2's hundred-node view budget, so no view ever shows
fewer places because of it. It bounds an orientation and nothing else: `get service` is a question
about services and still answers with all of them.

### 2. The provider states the population it did not answer with

A bounded answer that says nothing about its own size is the honesty defect ADR-0496 refused to
trade for. So `ono_pipeline::Diagnostics` carries a population beside its counters, a provider
records it on the stream's sink, and the shell reads it off the collected stream. Three providers
state one, and each states it only where the figure is free and exact:

- **systemd** (`ono-provider-systemd`) counts what `ListUnits` enumerated, less the `not-found`
  stubs it would have refused anyway. `ListUnits` is one round trip and has already happened; the
  per-unit `GetAll` reads are what the bound is for.
- **procfs** (`ono-provider-linux`) counts the pid list it has already read.
- **sock_diag** (`ono-provider-netlink`) counts the diag messages in each dump without building a
  record for any of them — `count_diag_sockets`, about ten milliseconds where decoding the same
  hundred thousand sockets is about a second. A dump it cannot walk to its `NLMSG_DONE` has no
  count at all: a figure nobody can stand behind is worse than none, because a caller would show
  it.

A query that *filters* — `get connection`, `--listening`, a selector, a port — states no
population: counting what would survive needs the records, which is the work the bound exists to
avoid. `None` then means "nobody said", and it is not zero.

### 3. A count is shown only where it is the count of the thing being counted

`Exit` carries the stated population and whether it was bounded, and the group built from it
reports:

- the **population** where the target serves exactly one kind of place, so the figure is the
  count of that exit;
- **no count at all**, with the reason, where the target serves several. `socket` answers both
  `network.listeners` and `network.connections`, and no split of one number between them is
  anything but invented (§2.17). `count` is then `null` and the group says *"read as far as the
  orientation bound; the whole count is not known"*, which is §42.4's rule that a missing figure
  is never reported as zero.

The population also survives a bound and not a *filter*: once `--type` or a predicate has refused
one of the members the shell holds, nobody knows how many of the ones it never read would have
been refused too, and the count falls back to what is really in hand.

### 4. A target whose count cannot be kept true is bounded far higher

Reporting no count is right at a hundred thousand sockets and wrong at a thousand, where the old
behaviour cost nothing anybody noticed. So an unattributable target is bounded by
`limits.orientation_ceiling`, **16 384** — above what any ordinary machine has and below what a
pathological one has. Below the ceiling nothing changes at all; above it the exits report no count
and say why, which is §33.3's bounded lower-detail strategy rather than a fabricated figure.

## Consequences

Easy: all four of §33.2's reference targets hold on the reference environment, measured at twenty
iterations by `cargo xtask perf` and recorded in the baseline —

| §33.2 target | budget | before | after |
|---|---|---|---|
| basic cached `look`/`near` first result | 50 ms | 1.0 ms | 1.1 ms |
| Profile M spatial query first result | 150 ms | 595 ms | **122 ms** |
| Profile M `map --live` first frame | 500 ms | 805 ms | **220 ms** |
| Profile L `map --live` initial progress | 1 500 ms | 3 514 ms | **572 ms** |

`crates/ono-cli/tests/spatial_first_output.rs::should_hold_every_time_to_first_result_target_of_the_reference_targets_table`
is un-ignored and green; it was the last of §57 H0's four failure proofs still red, and the last
`#[ignore]` ADR-0496 left in the workspace.

Hard, and the reason this is an ADR rather than a commit message: **the bound is a property of the
question, not of the observation.** The first form of this change bounded every read that went
through `view::observe`, and `crates/ono-cli/tests/spatial_contracts.rs::should_refuse_an_ambiguous_selector_in_a_script_rather_than_open_a_picker`
caught what that means: `enter <name>` resolves by sweeping the targets that could hold the name,
through the same function, and a sweep that stops at 128 objects reports `not_found` about a
process that is running. A latency budget had been turned into a correctness defect.

So the observation now carries its `Purpose`. An **orientation** builds a view — it counts, ranks
and shows at most a hundred places — and may take a bounded answer. A **resolution** is looking
for one named object, is never bounded, and will not reuse a cached observation that was: reading
a bounded answer back out of the session index would answer a search with a sample. The selector
figures are therefore unchanged from the released behaviour — a Profile M miss at 900 ms and a hit
at 197 ms — and §36.1's 250 ms target stays missed by the miss, as it was before this change. It
is recorded under *Found, not yet filed* in `docs/STATE.md`: what is left there is the cost of the
sweep's last acquisition class, which is a different increment from this one (§4).

Also hard: `look` inside NETWORK on a host with more than 16 384 sockets shows no listener or
connection count. That is the honest answer at that cardinality and it is a visible change; a user
who wants the figure asks `get socket | count`, which is unbounded and exact.

Encoded by `crates/ono-cli/tests/spatial_orientation_bound.rs` (three outcome tests: the counted
target, the uncountable one, and that a direct question is not bounded),
`crates/ono-cli/tests/spatial_contracts.rs::should_refuse_an_ambiguous_selector_in_a_script_rather_than_open_a_picker`,
which is what holds a resolution to completeness,
`crates/ono-provider-systemd/tests/service.rs::should_state_the_whole_population_when_a_query_bounds_what_it_answers`,
`crates/ono-provider-netlink/tests/socket_decoding.rs::should_count_a_dump_without_building_a_record_for_any_of_it`,
`::should_refuse_to_count_a_dump_it_cannot_walk_to_the_end`, and the reference-targets test above.

## Alternatives considered

**Answer the orientation from `ListUnits` alone, with no bound.** It is cheaper still — ten
milliseconds — and loses nothing, because everything an orientation needs of a unit is in the
listing. It also means answering with a record whose remaining fields are neither known nor
unknown but *unread*, which `ono.service/1` has no way to say and §35.3's null does not mean. That
is a change to what a record is, in the most heavily tested area of the shell, and it is a larger
question than this increment.

**Split `network.listeners` and `network.connections` into two queries** so each exit asks the
question it reports and gets its own exact population. `network.connections` could ask the
existing `connection` target; `network.listeners` has no target of its own, and `--listening` is
not the same predicate the index uses to call a socket a listener — a UDP socket has no listen
state and is still not a connection. Making the two agree means changing one of them, which is a
change to what a listener *is*, and that is not a performance increment.

**Raise the bound so the index keeps holding every name.** At 512 the COMPUTE orientation costs
370 ms against §33.2's 150 ms budget. The budget and index-resident name resolution are in tension
at Profile M, and §33.2 is the normative one.

**Report the sample's size as the count.** The defect ADR-0496 refused in advance: it trades a
latency defect for an honesty defect, and this whole ADR is the alternative to it.
