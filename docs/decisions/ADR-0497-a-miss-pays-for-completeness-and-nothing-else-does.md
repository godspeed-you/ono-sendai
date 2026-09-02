# ADR-0497: A miss pays for completeness, and nothing else does

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §33.2, §34.2, §36.1, §37.4, §61.1; v0.4 §27.1, §27.3, §32.1, §33.1; ADR-0252,
  ADR-0416, ADR-0431, ADR-0491, ADR-0494, ADR-0496; issue #8
- Decided by: agent (autonomous)

## Context

Issue #8, in its own words: *"a selector that resolves stops at the first step of §27.1, and one
that does not runs to the last step, which consults the whole index and so projects all six
domains"*, measured at **1.40 s in release, of which only 0.27 s is CPU** — a miss ten times a
hit. It calls itself a design question and names the two options: a persistent index across
processes, or a bounded last step.

§36.1 states the rule and, in its last sentence, the choice:

> A selector miss MUST not be substantially more expensive than a hit **solely because the system
> scans an unnecessarily complete global candidate set**. … If the system cannot meet Profile L
> without indexing, v0.4.1 MUST add the necessary index or a bounded candidate strategy for
> canonical selectors.

The qualifier is the whole rule. A miss *has* to consult every candidate, or it is not a miss.
What it may not do is make every selector pay for that.

## Decision

**The bounded candidate strategy, not the persistent index. The sweep is asked cheapest first and
stops as soon as the selector resolves.**

`resolved_place`'s last escalation used to hand every affordable target to one `observe_targets`
call. It now walks §34.2's four acquisition classes in order, asking the targets of one class,
re-resolving, and stopping the moment the selector has an answer. A selector that resolves from
`/proc` never pays for the systemd bus; one that resolves from nowhere pays for everything,
because that is what "it is nowhere" costs to establish.

`ono_spatial_query::acquisition_of_target` is what makes the order available — ADR-0494's classes
applied to the search-cost table `targets_for` already used.

### Why not a persistent index across processes

It is the other option issue #8 names, and it is the larger answer: an index that outlived the
process would make a cold miss a lookup. It would also make the shell's answer depend on state
nobody can see, which §33.2 ("the index stays a cache and the providers stay authoritative") is
careful about; it needs an invalidation story for every provider; and it puts a file with a
system's topology in it on disk, which is a security surface this hardening release did not plan
for. §36.1 offers the bounded strategy as an alternative rather than a fallback, and the
measurement below says it is enough.

### What it measures

`cargo xtask perf`, 20 iterations, release build, `ryzen-3900x-ubuntu-2604`, Profile M:

| | before | after |
| --- | ---: | ---: |
| a selector that resolves through the sweep (`enter sleep`) | 530 ms | **182 ms p95** |
| a selector that does not (`enter no-such-place-1a2b3c`) | 530 ms | 914 ms p95 |

The two rows were the same figure, which is issue #8's title exactly: a miss swept everything a
hit never touches, and so did the hit. They are now five times apart, and the hit is inside
§36.1's 250 ms Profile M target.

The miss is not, and it is honestly *higher* than before, because the sweep now re-resolves
between classes. That is the cost of establishing absence, and §36.1's sentence is about the other
row. The remaining 914 ms is dominated by the same external acquisition ADR-0496 reports —
400 ms of systemd bus — and closes with it.

In the container, where there is no service manager, the same two rows are 131 ms and 182 ms, and
acceptance case `195` asserts the relation rather than the figures, because the container is not
§37.2's reference environment.

### The tests

`crates/ono-spatial-index/tests/index.rs::should_answer_a_selector_miss_from_a_bounded_candidate_set`
holds the *index* to the same rule, one layer down: five thousand objects whose names all contain
the needle, and the alias lookup §27.1's steps use still answers about the one that carries it.
The unbounded set is the approximate pass of §27.3, which "never acts alone" — so the two must not
be the same set, and the test asserts that they are not.

`crates/ono-spatial-query/tests/resolution.rs::should_not_make_a_selector_hit_pay_for_the_completeness_a_miss_needs`
reads the recorded baseline and requires the hit-through-the-sweep to be under half the miss. It
asserts recorded figures rather than running a stopwatch, for the reason ADR-0252 and ADR-0431
give.

`docs/ACCEPTANCE.md` §4.8.8 named it
`should_hold_the_profile_m_and_profile_l_selector_miss_targets`. §36.1 labels those two figures
*targets* and states its MUST in the sentence above them; the p95 figures are measured and
recorded by ADR-0490's harness and one of them is still missed, so a test named for holding them
would have to be red. The box now names the test that asserts the MUST, and ADR-0491's table is
where the targets are tracked.

## Consequences

Easy: every selector that resolves — which is almost every selector a person types — costs what
the provider that answers it costs, rather than what the slowest provider on the host costs. The
figure that closes issue #8's headline moved from parity with a miss to a fifth of it.

Hard: a true miss is now slower than before, by the cost of re-resolving between classes. Four
resolutions against an index that is growing is not free, and on a host with no cheap answer the
selector pays them all. The alternative — one sweep, one resolution — is what made a hit cost a
miss.

Also hard: the order is by acquisition class, which is a proxy for "likely to answer" and is not
one. A selector that only the systemd bus can answer pays for `/proc`, netlink and the mount table
first. That is the right default — the cheap classes are cheap — but a selector whose *shape* says
which provider owns it should skip ahead, and `type_hint` only recognises `<type>/<key>` today.

Encoded by the two tests above and by acceptance case `195`.

## Alternatives considered

**A persistent index across processes.** Issue #8's other option; see above. It is a larger change
with a security surface, and §36.1 does not require it if a bounded strategy is enough.

**Ask every class concurrently instead of in order.** It would make the miss as fast as its
slowest provider — about 400 ms rather than 914 — and it would make the hit pay for every class
again, because all of them are asked. Both are wanted, and the shape that gets both is concurrency
*within* a class, which needs `observe` split into a fetch phase and an absorb phase. That is a
change to the observation contract, and ADR-0496 already owes one.

**Stop sweeping `external` acquisitions for a bare selector.** It would bring the miss under
§36.1's 250 ms target immediately, and it would stop `enter <service-name>` resolving from a cold
shell — a capability, removed to make a number. §34.3's request path exists for relations; there is
no equivalent for resolution, and inventing one to hit a target is the wrong order.
