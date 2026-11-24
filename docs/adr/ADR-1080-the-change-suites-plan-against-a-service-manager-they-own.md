# ADR-1080: The change suites plan against a service manager they own

- Status: accepted
- Date: 2026-11-24
- Spec refs: v0.6 §4.3, §19.4, §40.1–§40.3, §53; v0.4.1 §38.1, §38.2, §38.4; AGENTS.md §11
  ("never rely on the developer machine's real processes unless the fixture creates them")
- Issues: #300
- Decided by: agent (autonomous)

## Context

`crates/ono-cli/tests/change_gates.rs` and `change_views.rs` built their HIGH-risk plans from
`get service | take 50 | plan restart service` on the real host. A unit that exists when
`get service` lists it and is gone when the plan freezes it — docker's transient
`docker-<id>.scope` and `run-docker-netns-….mount` — makes the plan refuse with
`change.target_unresolved` (E1711), and §4.3 is right to: a vanished target cannot be frozen. The
suites' verdict was therefore the host's timing, not the risk gate's. Commit `71b1f2fb` narrowed
the listing to units with a unit file, a stopgap that still read the host and still depended on
what the host happened to run.

Reading the host had a second, quieter cost: `should_get_past_the_gate_when_the_risk_is_acknowledged`
and `should_seal_the_acknowledgement_into_a_new_revision` run `apply --accept-risk --confirm` on
that plan, which asks the host's systemd to restart fifty real units. Only the missing privilege of
the test user stood between the suite and a host's services.

ADR-0880 already chose the remedy for the spatial service tests: a private `dbus-daemon` serving
`org.freedesktop.systemd1` with a population the test chose (`spatial_orientation_bound.rs`).

## Decision

1. The fixture moves out of `spatial_orientation_bound.rs` into
   `crates/ono-cli/tests/support/service_manager.rs`, included with `#[path]` by every suite that
   needs it, so there is one fixture service manager and not two. It gains what a change suite
   needs and nothing else: each unit has a unit file (`FragmentPath`, `UnitFileState`), the job
   methods `StartUnit`, `StopUnit`, `RestartUnit` and `ReloadUnit` are accepted and recorded
   (nothing on the host happens), and every request is counted at the boundary
   (`Requests { list_units, load_unit, unit_reads, jobs }`), so a test can state how often the
   shell asked the service manager anything without knowing which code path asked.
2. `change_gates.rs` plans `get service | plan restart service` against a fixture of exactly fifty
   units — §53's `change.bulk.high_risk_targets` — and runs every `apply` of that plan against the
   same fixture. `change_views.rs`' gate view does the same. The suites assert, in addition to what
   they asserted before, that planning queued no job and that a refused gate queued none.
3. A host without `dbus-daemon` skips with `external_tool_unavailable`; the eight declared rows of
   `docs/contracts/hardening/expected_test_skips.yaml` move from `fixture_not_applicable` (no
   systemd unit) to that category. The canonical CI image and this run's host both have
   `dbus-daemon`, so no skip is expected there.

## Consequences

- Both suites pass while containers start and stop on the host (verified on 2026-11-24 with a loop
  of `docker run --rm` beside `cargo test --test change_gates --test change_views`: 12/12 and 6/6).
- They still fail when the risk gate stops refusing: with `gates::enforce` short-circuited, four
  tests of `change_gates.rs` went red (`should_refuse_a_high_risk_plan_when_the_risk_was_not_acknowledged`,
  `should_name_the_rule_that_fired_when_a_risk_gate_refuses`,
  `should_change_nothing_when_a_risk_gate_refuses`, `should_name_the_flag_that_answers_the_gate`).
- The fixture's counters are the measuring instrument of #140 (ADR-1081).
- `change_planning.rs` still plans `get service | take 3` against the host; it is not part of #300
  and is reported rather than changed here.

## Alternatives considered

- A stable synthetic target (a `process` the test starts) — the gate under test is the bulk rule
  over services, and a different target would test a different rule's path.
- Filtering the host's listing harder (only `*.service` with a unit file) — still the host's
  population, still its timing.
- A second fixture inside `ono-testkit` — `ono-testkit` carries neither zbus nor tokio, and adding
  them to every crate's test build to serve one crate's suites is a cost with no second user.
