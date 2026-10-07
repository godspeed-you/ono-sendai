# ADR-0947: A global acquisition is declared in the contract and shown by `explain`

- Status: accepted
- Date: 2026-10-07
- Spec refs: v0.4.1 §34.1, §34.2, §34.4, §22.4; ADR-0494, ADR-0496, ADR-0576, ADR-0942
- Decided by: agent (autonomous)

## Context

v0.4.1 §34.4: "A local neighborhood query SHOULD NOT require construction of the complete system
graph when provider APIs can answer the neighborhood incrementally. Any unavoidable global build
MUST be visible in `explain` and covered by materialization/performance budgets." ADR-0496 left
the `explain` half owed, and `explain map` said nothing about it (issue #177).

Reading the code, no spatial or trace command answers from local lookups alone:

- `look`, `near` and `map` observe the whole space through `view::observe_space` — every target
  behind every exit, bounded per target by the orientation bound of ADR-0576 — and `map` then
  estimates and refuses beyond §34.1's interactive budget (E1401, ADR-0494);
- `find place` asks every planned target a whole-target query; `enter` and `jump` sweep the
  targets on an index miss, cheapest class first;
- every `trace <target>` answers from `SharedSnapshots::one`, which reads the whole target and
  filters it, and the socket, connection, file and mount kernels scan every `/proc/<pid>/fd`;
- `map-links` reads the session's own link table.

## Decision

1. **Contract first.** A command contract may declare `acquisition: {scope: global|local, cost:
   cheap|moderate|expensive|external}` — the scope of the build and the dominant §34.2 class.
   Declared: `look`, `near`, `map`, `find place`, `enter`, `jump` (global, moderate);
   `trace` of process, service, user, interface, route (global, moderate), of socket,
   connection, file, mount (global, expensive), of host, link, container (global, external);
   `map-links` (local, cheap). `ono_command::CommandContract::acquisition` reads it.
2. **`explain` shows it.** The plan carries a stage's acquisition as `{scope, cost}` (null where
   none is declared) and the rendering prints `acquisition  global, moderate` among §22.4's
   execution and budget rows. The budgets that cover each build are the existing ones: the
   orientation bound (`limits.orientation_objects`, `limits.orientation_ceiling`) for the spatial
   sweeps, §34.1's estimate and E1401 for `map`, and §22's materialization budget for every stage
   downstream of a trace.
3. **The cheapest honest check.** `spec-check` (`xtask::contracts::check_acquisitions`) holds the
   declarations to the code: the vocabulary is `ono_spatial_core::AcquisitionCost`'s; every
   `trace` command and every spatial sweep (named in `SPATIAL_SWEEPS` with the code that sweeps)
   must declare a **global** acquisition; and no declared cost may be cheaper than
   `ono_spatial_query::acquisition_of_target` says enumerating the command's target costs. The
   check cannot prove a command is local, so `local` is accepted only where no rule demands
   `global`.

## Consequences

- §34.4's "visible in `explain`" holds for every global build the shell has today, as data and as
  text. Making a build incremental later means changing the contract to `local` — and the check
  then demands the code be changed first, by removing the command from the trace or sweep rules.
- Tests: `crates/ono-cli/tests/explain.rs`
  (`should_show_a_global_acquisition_and_its_cost_class_when_explaining_map`,
  `should_carry_the_acquisition_of_a_trace_in_the_plan_value`,
  `should_show_no_acquisition_for_a_stage_that_asks_one_provider`); `xtask/tests/contracts.rs`
  (the repository's declarations pass; a trace without one, a local trace, a class cheaper than
  the target's, and words outside the vocabulary are refused); acceptance case
  `385-explain-shows-global-acquisition.case`.

## Alternatives considered

- **Compute the class from the code at plan time** (`acquisition_of_target`). Covers traces but
  not the sweeps, and a planner that cannot see the code's per-command shape would print a guess;
  the contract states it once and the gate holds it.
- **A runtime probe counting provider reads.** Honest but expensive, environment-dependent, and
  not something `spec-check` can run.
