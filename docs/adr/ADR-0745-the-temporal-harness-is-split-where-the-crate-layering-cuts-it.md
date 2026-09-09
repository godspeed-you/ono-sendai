# ADR-0745: The temporal harness is split where the crate layering cuts it

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §39.2, §51 (TEST-001, TEST-002); v0.4.1 §56;
  `docs/contracts/hardening/module_architecture.yaml`; ADR-0517
- Decided by: agent (autonomous)

## Context

v0.5 §51 asks for two harness packages that read as one piece of work. TEST-002 is a deterministic
clock: a fixed origin, a way to advance it, and helpers that place a plausible event sequence at
stated instants. TEST-001 is a set of builders for a `TemporalEvent`, an `Evidence`, a
`Checkpoint`, a `CoverageSummary` and a `CausalLink` that are valid by construction, so a test
about retention spends its lines on retention.

The obvious home for both is `ono-testkit`, which every crate already dev-depends on. That home is
available for one of them and closed to the other, and the thing that closes it is a contract:

- `docs/contracts/hardening/module_architecture.yaml` places `ono-testkit` in the **foundation**
  layer — *"values, errors, parsing and the test harness — no knowledge of what a system is"*.
- It places `ono-temporal-core` and `ono-temporal-ledger` in the **capability** layer.
- §56's rule, which `xtask::architecture::check_layering` enforces on every gate run, is that a
  crate may depend on its own layer or below and **never upwards**.
- `workspace_dependencies` reads every line of a `Cargo.toml` ending in `.workspace = true`,
  including the ones under `[dev-dependencies]`. So even a dev-only edge from `ono-testkit` to
  `ono-temporal-core` is a layering violation and turns the gate red.

Three ways out were available. Move `ono-testkit` up a layer: that inverts the whole graph, because
`ono-value` and `ono-parser` dev-depend on it. Declare the temporal dependency optional, so the
string `.workspace = true` does not appear and the check does not see it: that is defeating the
referee, which AGENTS.md §14 forbids in terms. Or accept that the layering is right and let it cut
the harness where it cuts.

## Decision

**The clock is a foundation thing and lives in `ono-testkit`. The contract builders are a
capability thing and live beside the fixture that needs them.**

`crates/ono-testkit/src/temporal.rs` holds what has no knowledge of what a system is:

- `VIRTUAL_NOW`, the fixed origin, which is the same string `ono_kuang_testhost::VIRTUAL_NOW`
  carries — spec §31.73's virtual time, `2026-08-26T12:00:00Z`. A second origin beside it would
  make two suites' fixtures incomparable for no gain, so the harness agrees with the shipped
  constant rather than choosing again. `xtask/tests/perf.rs` asserts the two are equal.
- `Clock`: an origin, a cursor, `advance`, `at_second` and `cadence`. It reads no system clock,
  which is what makes a suite built on it reproducible on a machine in another year. Its whole
  dependency is `jiff`, which is not an `ono-` crate and therefore not a layer.
- The declaration of §49's fixture ledger — numbers parsed out of
  `docs/contracts/hardening/performance_profiles.yaml`, which is YAML rather than a temporal type.

`xtask/src/perf/fixture.rs` holds TEST-001's builders — `event_seed`, `resolved`, `field_evidence`,
`coverage_interval`, `causal_link`, `action`, `object_record`, `checkpoint` — beside the million-
event fixture that is their first and largest consumer. `xtask` is not under `crates/`, so it is
outside the layering and may reach for whatever it measures.

## Consequences

`cargo xtask perf` and the perf suite get builders that are valid by construction. The three
temporal crates keep the per-suite fixtures they already have
(`crates/ono-temporal-{ledger,query,reconstruct}/tests/{common,support}/mod.rs`), which are the
same shapes written three times. That duplication is the cost of this decision and it is real.

The way to remove it is not to weaken §56. It is for `ono-temporal-core` — the crate that owns
these types — to expose them itself, as a `fixture` module gated behind a Cargo feature its
siblings enable in `[dev-dependencies]`. That is a change to a crate this package does not own, so
it is reported to the lead rather than made here.

Every crate that wants a deterministic instant gets one today, from one place, agreeing with the
KUANG/11 test host. That is the half of the harness that was actually missing; the builders
already existed three times over.

## Alternatives considered

- **Move `ono-testkit` to the capability layer.** Rejected: `ono-value` and `ono-parser` dev-depend
  on it, so the edge would point upwards from foundation and the check would fail at the other end.
- **An optional dependency, so the check's string match does not see it.** Rejected outright. The
  check would be evaded rather than satisfied, and the layering it enforces would still have been
  broken — AGENTS.md §14: never weaken the harness to get a green result.
- **Put the builders in `ono-temporal-core` behind a feature.** The right long-term answer, and
  not this package's file to write (INTERFACES §0). Recorded here so the next agent finds the
  reasoning rather than the duplication.
