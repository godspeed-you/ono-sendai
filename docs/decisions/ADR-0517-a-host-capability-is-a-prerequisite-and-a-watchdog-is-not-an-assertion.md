# ADR-0517: A host capability is a prerequisite, and a watchdog is not an assertion

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §2.7 (tests report execution truth), §38.1 (three visible outcomes), §38.2
  (the declared expectation), §38.4 (the taxonomy), §52.2 (single source of truth), §65.10
  (skip-as-pass); AGENTS.md §11 (no wall-clock dependence)
- Issues: #88, #89
- Decided by: agent (autonomous)

## Context

Three red results on 2026-09-02, from three machines, all saying the same wrong thing.

**A descriptor limit, on the GitHub runner.** `SocketPopulation::of` panicked at socket 65 529 of
100 000: *"Too many open files"*. It passes on the development machine, whose `ulimit -n` is
524 288, and fails on the runner, whose soft limit is about 65 536. H7's fixture (ADR-0488)
assumed the machine it was written on.

**Two wall-clock budgets, under parallel load.** `storage.rs::should_trace_a_mount_to_what_it_sits_on_and_who_uses_it`
overran a 30 s budget while three cargo builds held the load average at 19–24; its command,
`trace mount / | to json`, answers in **2.79 s and 4.34 s** at load 1.97 — exit 0, 461 KB of JSON.
`processes.rs::should_trace_the_entered_process_without_a_selector` overran `ono_testkit`'s 20 s
default during a gate run sharing the machine with a worktree build; its whole suite passes in
**1.70 s** at load 3.27. Neither command hangs. **Both were first reported as a product hang**,
which is the cost of the defect rather than an anecdote about it. A fourth instance appeared while
this ADR was being written: `completion.rs::should_stop_discovery_at_the_hard_budget_and_answer_what_it_has`
gives a provider cache one fixed second to fill, and fails one workspace run in three while
passing every run in isolation.

§65.10 names a test that reports a pass it did not earn. These are the inverse — a test reporting a
defect nobody committed — and §2.7 forbids both by the same sentence: a test reports execution
truth. A red result meaning *"this runner has a lower rlimit"* or *"this machine was busy"* is not
that.

## Decision

**Two different failures, because they are two different things, and telling them apart is the
whole decision.**

### A capability the host cannot supply is a prerequisite, and its answer is SKIP(reason)

`SocketPopulation::try_of` replaces the panic:

1. It raises this process's soft `RLIMIT_NOFILE` toward the hard limit. That changes nothing about
   what the fixture measures — the descriptors were always allowed, the process was simply not
   asking for them — so it happens first and without ceremony, and on most hosts it is the whole
   fix.
2. Only where the **hard** limit is still short has the host genuinely refused. Then `try_of`
   returns a `DescriptorShortfall` naming what was needed, what the soft limit reached and what
   the hard limit is, and the caller announces `SKIP(missing_privilege)` with that sentence.
   `missing_privilege` because raising a hard limit is a privileged act, which is exactly what the
   test lacked.

The requirement lives in `docs/spec/hardening/performance_profiles.yaml` as a `descriptors:` field
beside the `sockets:` count that fixes it, so the number a fixture needs sits next to the
cardinality it comes from rather than inside a panic message (§52.2).

`SocketPopulation::of` still panics and is still what a profile every host can supply should use.
The difference is now a caller's choice rather than a fixture's assumption.

### A watchdog is not an assertion, so it scales with the load and says so when it fires

Nothing in this repository asserts that a command answered within twenty seconds. `Shell`'s budget
exists so a hung test fails instead of stalling the suite: it is a **watchdog**, and it carries no
claim about the product. Measuring a fixed wall clock against a machine whose load the test does
not control is therefore not measuring anything.

So the watchdog stretches by the load per processor, ceiling of `load1 / nproc`, clamped to
`1..=8`. The clamp is not decoration: without it a runaway load turns the watchdog off, and a
watchdog that never fires is the hang it exists to catch.

Scaling cannot weaken an assertion, because there is none to weaken. What it changes is how long a
busy machine waits before calling something a hang. And when the watchdog does fire, the message
carries the load, the processor count, the budget it scaled to and the budget the caller asked
for, so the next reader can tell a busy machine from a hang without re-deriving it:

```text
`ono -c trace mount / | to json` did not finish within 60s (a 30s watchdog scaled for a load
average of 21.40 on 16 processors). A watchdog carries no assertion: what fired here is either the
program hanging or a machine this run does not control — re-run it on a quiet one before reading
it as a defect
```

`ono_testkit::under_load` is the same scaling as a public helper, for a test that waits on
something other than a process. `completion.rs`'s cache window uses it; the budget that test
actually asserts — one nanosecond, admitting no discovery — is untouched, because that one is an
assertion and not a watchdog.

### What is deliberately not scaled

`run_bounded`'s budget, where the subject **is** the hang. ADR-0431 chose 30 s there as §33.3's own
number with a sixtyfold margin, and the observation is whether anything reached a stream at all.
And every performance *target* of §33.2 — those are assertions, they are measured by
`cargo xtask perf` on the named reference environment (ADR-0490), and a target that stretched under
load would measure nothing.

### The skip is declared, not absorbed

`expected_test_skips.yaml` gains a third list. `canonical_ci.permitted_skips` holds a skip whose
outcome is a property of the host rather than of this repository, each with the condition that
decides it: `ulimit -Hn` at least 101 024, for the two socket fixtures. `skip-check` neither
requires nor forbids these.

That is not a loophole and the shape is what keeps it from becoming one. Requiring the skip would
be red on a machine that *can* supply the descriptors; forbidding it would be red on one that
cannot; the honest statement is the condition, and §38.2 asks precisely that an intentional skip
be *"listed with its ID and reason in a machine-readable file"*. An entry names a capability a
reader can check in one command, and the `declared:` list still holds the category, so nothing
here escapes the gate's static half.

## Consequences

Easy: the CI gate stops reporting the runner's `ulimit` as a spatial defect, and a loaded
developer machine stops reporting itself as a product hang. Both failures now say which of the two
they are, in the run that produced them.

Hard: the scaled watchdog means a genuinely hung command is detected later on a busy machine —
up to eight times later. That is the trade the clamp bounds, and it is the right way round: a
suite that takes an extra minute to notice a hang costs a minute, and a suite that reports a busy
machine as a hang costs an investigation. Two were spent on 2026-09-02.

Also hard, and stated because it will be read: **the watchdog now depends on the machine's load,
which is a wall-clock dependence AGENTS.md §11 warns about.** The warning is about *assertions*,
and this is not one; the way to keep that true is that no test may assert on a `Shell` budget. The
one test that asserts a budget asserts a nanosecond one, in-process, with no child.

`SocketPopulation::of` and `try_of` are two functions where there was one, which ADR-0427 would
normally call a divergence. They are not: `of` is `try_of` with a panic, one line, and the
difference is which caller can honestly skip.

Encoded by: `crates/ono-testkit/tests/harness.rs::should_report_a_descriptor_limit_the_host_cannot_reach_rather_than_failing`,
`::should_raise_its_own_soft_descriptor_limit_before_reporting_a_shortfall`,
`::should_stretch_a_watchdog_for_the_load_the_test_does_not_control`,
`xtask/tests/scan.rs::should_neither_require_nor_forbid_a_skip_the_host_capability_decides`, and
the two call sites — `crates/ono-cli/tests/spatial_first_output.rs::should_answer_or_refuse_within_the_interactive_budget_on_the_profile_l_fixture`
and `crates/ono-spatial-query/tests/profiles.rs::should_build_every_declared_profile_at_the_cardinality_the_registry_states`
— both verified by lowering this machine's hard limit to 4096 and watching them skip with the two
numbers in the marker instead of panicking.

## Alternatives considered

**Lower Profile L's socket cardinality to something every runner can hold.** The number is
Appendix F.2's, and `performance_profiles.yaml` says in as many words that nothing there may be
lowered to make a suite pass. A profile is no less required for being expensive; it is measured
somewhere that can afford it.

**Skip the wall-clock overruns too, instead of scaling them.** A skip says the test did not run,
which loses coverage the machine could have provided — the commands answer in seconds, they were
simply not waited for. Scaling says the test ran and answered, which is what happened. A skip is
the right answer where the host cannot supply a *capability*; it is the wrong one where the host
is merely slow.

**Retry an overrun on a quiet machine automatically.** A retry that passes is a flake nobody
investigates, and the second run is on the same busy machine.

**Read the load once per process instead of once per run.** A `cargo test --workspace` starts
quiet and is at load twenty by the time the shell suites run. A factor decided at process start
describes a machine that no longer exists.

**Put the descriptor requirement in a constant in `ono-testkit`.** §52.2: a number that constrains
a declared cardinality belongs beside it, or the two drift and only one of them is read.
