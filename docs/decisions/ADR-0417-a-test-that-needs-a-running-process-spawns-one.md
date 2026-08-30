# ADR-0417: A test that needs a running process spawns one

- Status: accepted
- Date: 2026-08-30
- Spec refs: v0.4 §6.8 (`find place` with a predicate), §29.4 (a normal structured stream);
  AGENTS.md §14 (a referee that fails one run in two is not a referee); STATE.md, the
  host-premise flake family recorded by S11a/S11c
- Decided by: agent (autonomous)

## Context

`spatial_navigation_missing.rs::should_stream_places_with_scope_and_provenance_when_find_searches_with_a_predicate`
asserts that `find place --where state == "running" | take 5` streams at least one place. To
back that up it spawned a `SleepChild` — and a sleeping child sits in state S. The assertion
therefore rested on a premise about the host: that *some* process happens to be in state R at
the sampling instant.

On a developer machine that premise holds by accident, permanently. On an otherwise idle CI
runner it failed on 2026-08-30: run 33318207211 (attempt 1, commit `1cee6cb`, a README-only
change) answered the search with `[]` and the quality gate went red; attempt 2 of the same
commit was green. The shell behaved as specified both times — an empty answer to a search
nothing matches is correct. The test claimed more than it had arranged.

## Decision

The premise is established by the test itself. A `BusyChild` (`sh -c 'while :; do :; done'`,
killed on drop) burns CPU for as long as the test holds it, so at least one process is runnable
whenever the provider samples the table. The assertions are untouched.

This is the same treatment the family received before (STATE.md, S11a/S11c; ADR-0230 for the
sibling condition): fix the premise the test failed to establish, and keep the referee.

## Consequences

- The test is deterministic on an idle host; the gate stops flaking on this name.
- The busy child costs one core for roughly the three seconds the test runs, in one test of one
  suite.
- `SleepChild` remains the right tool everywhere the test wants a quiet, unremarkable process;
  `BusyChild` exists for the one claim that needs state R.

## Alternatives considered

- **Search for `state == "sleeping"` instead.** Keeps the sleeping child, and weakens the case:
  the point of the scenario is that a predicate search over live state answers from the
  providers, and `running` is the state the spec's own examples use. Rejected.
- **Retry the search until it answers.** Papers over the premise and slows every honest
  failure. Rejected.
- **Tolerate the flake and rerun CI.** AGENTS.md §14 rules it out; a referee that fails on
  quiet hosts referees the weather. Rejected.
