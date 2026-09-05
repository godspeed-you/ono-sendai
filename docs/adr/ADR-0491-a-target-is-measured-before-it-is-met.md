# ADR-0491: A target is measured before it is met

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §33.1, §33.2, §33.3, §34.2, §34.4, §36.1, §37.3, §37.4, §61.1, §61.3, §65.9,
  §65.10; ADR-0252, ADR-0431, ADR-0488, ADR-0489, ADR-0490; issues #8, #20, #22, #85, #86, #87
- Decided by: agent (autonomous)

## Context

§33.2 states four targets and §33.3 states the floor underneath them:

> A supported interactive operation MUST NOT spend 30 seconds producing neither output nor
> progress on the reference Profile M/L fixtures.

Until this increment nobody knew whether any of the four held, because §37.2's reference
environment did not exist. It does now (ADR-0490), so the first thing to do with it is find out.

**What the reference environment says.** `cargo xtask perf`, twenty iterations each, release
build, `ryzen-3900x-ubuntu-2604`:

| §33.2 row | benchmark | measured p95 | budget | |
| --- | --- | ---: | ---: | --- |
| basic cached look/near first result | `spatial.look` S, cache hit | 1.288 ms | 50 ms | holds, by a factor of 39 |
| spatial query Profile M first result | `spatial.query` M, cold | 580.5 ms | 150 ms | **3.9× over** |
| map live Profile M initial visible frame | `spatial.map_first_frame` M, cold | 1 090.3 ms | 500 ms | **2.2× over** |
| map live Profile L initial progress/summary | `spatial.map_first_frame` L, cold | 25 748 ms | 1.5 s | **17.2× over** |

Load average 1.9 while it ran, recorded in the baseline beside the figures.

**And neither cause is the cardinality the profile is named for**, which is the finding that
matters, because it is not what issue #85 assumes.

The Profile M rows are dominated by a *fixed* cost. `enter compute; look --json` measured 942 ms
with no extra processes, 946 ms with a hundred, 1 135 ms with sixteen hundred: a curve that is
almost flat in the number of processes and sits at 0.9 s from the origin. The 0.9 s was the
systemd service enumeration — 569 units × three D-Bus round trips, issued one after another.
ADR-0490's commit made those reads concurrent and the figure fell to 0.405 s, which is the whole
of `look` inside COMPUTE now. What remains is an *external* acquisition in §34.2's sense: a round
trip to another daemon, per object, that no amount of concurrency removes.

The Profile L row is the opposite shape and is cardinality-driven: against a hundred thousand
listening sockets, `enter network; look --json` takes 3.4 s and
`enter network; map --live --json | take 1 | to json` takes 23 s in release and 79 s in a debug
build — to draw thirty nodes. That is §34.4's *"A local neighborhood query SHOULD NOT require
construction of the complete system graph"*, and it is issue #87.

## Decision

**This increment measures §33.2's four targets on the named environment, enforces §33.3's floor
over what it measured, and states the three that do not hold as red tests owned by the increments
that close them. It does not move a budget and it does not choose a benchmark to fit one.**

### 1. §33.2's table is data, and each row names what answers it

`xtask::perf::TARGETS` carries the four rows with the benchmark, profile and §37.3 temperature
that answers each. "Basic **cached** look/near" is §37.3's cache hit — the same query answered
again — and the other three are cold, because an interactive operation is budgeted from where the
user pressed return.

`xtask::perf::verdicts` answers `Held`, `Missed` or **`Unmeasured`** per row. The third is not a
pass, for the reason `Baseline::compare` has `Unmeasured` and `ForeignEnvironment`: §65.10's
defect is a run that reaches the summary without having checked anything, and a target with no
measurement behind it is exactly that.

### 2. The green test asserts what is true: every row is measured, and §33.3's floor holds

`xtask/tests/perf.rs::should_measure_every_time_to_first_result_target_of_the_reference_targets_table`
requires each of the four rows to have a record on the baseline's environment, and requires every
recorded first result to be inside §33.3's thirty seconds — §61.3's watchdog, applied to the whole
recorded set rather than to one command.

It asserts against *recorded* figures and runs nothing, so it is a comparison of two numbers in a
checked-in file. That is deliberate: ADR-0252 and ADR-0431 are this repository's standing record
that a wall-clock assertion in `cargo test` measures the machine that happened to run it. Here the
wall clock ran once, on the named environment, under §37.4's rule, and the test reads the result.

### 3. The three that do not hold are red tests, not omissions

`crates/ono-cli/tests/spatial_first_output.rs::should_hold_every_time_to_first_result_target_of_the_reference_targets_table`
asserts §33.2's budgets against the recorded baseline and is `#[ignore]`d, with the two causes
named in the file. It goes green when §34.2's cost classes (#86) and §34.4's bounded neighbourhood
(#87) land, and it needs no re-measurement to change its verdict: regenerating the baseline is
what changes it, which is the right coupling.

`::should_answer_or_refuse_within_the_interactive_budget_on_the_profile_l_fixture` is §33.3's
watchdog at the second reference profile, built from Profile L's socket axis (ADR-0488), and is
`#[ignore]`d for the same reason: 79 s of silence in a debug build against a thirty-second budget.
It sits beside the Profile M watchdog ADR-0431 wrote, in the suite named for the subject.

### 4. Why the benchmarks were not moved to where the budget would be met

`enter compute; look --json` pays 405 ms for services. `enter processes; look --json` does not, and
would put the Profile M row inside its budget today. It was written, measured and not kept: §33.2's
row is about what an interactive user does at Profile M, and a user standing in COMPUTE and typing
`look` is the canonical case. Choosing the collection instead would be moving the target, which
§32.1 forbids in the fixture and which is no better done in the benchmark.

The honest fix is §34.2's, and the specification already describes it: an external acquisition is
not paid for by an orientation query, the exit is discoverable and unloaded, and §34.3's request
path obtains it. That is issue #86, and it is the next increment but one.

## Consequences

Easy: every later performance claim in this phase has a number to move rather than an impression.
The two causes are separated and each is attached to the issue that owns it, so #86 and #87 have
a red test each that turns green without any new measurement code.

Hard: three of §33.2's four targets are red, in a phase whose §66.4 bullet says
"time-to-first-result is measured". Measured is what this increment delivers; met is what #86 and
#87 deliver. The `#[ignore]`s are the honest record of the difference, and §4.8.8's box stays
unticked until they come off.

Also hard: the reference environment is shared, and the baseline records the load average of the
run that produced it. A regeneration under a heavier load will move every figure. §37.4's twenty
iterations and the p95 absorb scheduling noise; they do not absorb a machine doing something else
entirely, and the recorded load is how a reader notices.

Encoded by
`xtask/tests/perf.rs::should_measure_every_time_to_first_result_target_of_the_reference_targets_table`
(green),
`crates/ono-cli/tests/spatial_first_output.rs::should_hold_every_time_to_first_result_target_of_the_reference_targets_table`
and `::should_answer_or_refuse_within_the_interactive_budget_on_the_profile_l_fixture` (both
`#[ignore]`d, red at HEAD).

## Alternatives considered

**Assert §33.2's budgets with a live stopwatch in `cargo test`.** The targets are 50 ms and 500 ms;
ADR-0431 already rejected exactly this and said why — the green side of the assertion would be the
flake. Reading recorded figures keeps the determinism and loses nothing, because the measurement
was made under §37.4's rule on the named machine.

**Widen the Profile M budget because the reference environment is a shared virtual machine.** The
budget is the specification's, and the gap is a factor of three on a machine that is idle enough
to measure `spatial.look` at 1.1 ms. The cost is real work being done in the wrong place, not
noise.

**Fix §34.2's cost classes here, in the same increment.** It is the fix, and it changes what
`look` shows inside COMPUTE — a user-visible behaviour change with its own acceptance surface,
which AGENTS.md §4 keeps out of an increment whose subject is measurement. It is issue #86 and it
has its own box.

**Leave the failing targets unmentioned until #86 and #87.** They would be three requirements with
no test and no record, which is the state §66.4 exists to end.
