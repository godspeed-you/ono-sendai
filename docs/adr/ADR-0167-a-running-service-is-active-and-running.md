# ADR-0167: A "running" service is `state == "active"` and `substate == "running"`

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §44.2, §9.3; v0.2 §28.3 (`ono.service/1`), §23.3
- Decided by: agent (autonomous, agent id `fixtures`)

## Context

v0.4 §44.2 — the "unknown nginx" acceptance scenario — asks the operator to "select the running
web service by visible metadata", enter it and follow one of its processes, without ever having
been told the unit's name. The integration form of that scenario,
`crates/ono-cli/tests/spatial_topology_missing.rs::should_reach_a_running_service_by_its_visible_state_when_a_service_manager_answers`,
expressed the selection as `find place --where state == "running"`.

That predicate cannot ever be satisfied. `running` is not a value of `state` in the inherited v0.2
contract. `docs/contracts/schemas/service.v1.yaml` (`ono.service/1`, spec v0.2 §28.3) declares two
distinct fields:

- `state` — an enum, `active | reloading | inactive | failed | activating | deactivating |
  unknown`: "the high-level activity state", the systemd active states of v0.2 §23.3;
- `substate` — a string, "the provider-specific sub-state (`running`, `exited`, `dead`, …). A
  string rather than an enum because the set is the provider's, not Ono's."

So the word the scenario uses, "running", is a *substate*, and on a systemd host
`state == "running"` selects nothing at all. The test was asserting the existence of something the
contract says cannot exist, and was therefore red on every host with a working service manager —
not because the shell was missing a capability, but because the fixture's predicate contradicted
the schema.

v0.4 does not redefine `ono.service/1`; §37 and §42 layer the spatial index on top of the
providers' existing canonical objects. The v0.2 contract therefore governs, and the test is what
has to be corrected.

## Decision

**In Ono, "a running service" means `state == "active" and substate == "running"`, and that pair
is what a discovery-by-visible-state selector spells.**

- `state == "active"` is the contract's word for "this unit is up".
- `substate == "running"` is the provider's word for "and it is executing", as opposed to
  `exited` (a finished oneshot), `dead`, `plugged` or `listening`.

Both halves are needed for §44.2 and neither may be dropped:

- `state` alone admits `active`/`exited` oneshot units and `active`/`plugged` `.device` units,
  which have no process — and the scenario's next step is "follow one of its processes";
- `substate` alone is a provider-defined string with no guarantee of being scoped to a unit that
  the high-level state calls up.

`running` MUST NOT be introduced as a `state` value. Adding it would break `ono.service/1` for
every v0.2 consumer and would fabricate a state no service manager reports (v0.2 §35.3: unknown
data is `null`, never invented).

The predicate lives in one place in the suite,
`spatial_topology_missing.rs::RUNNING_SERVICE`, so that the reading is stated once and every
script that discovers a running service uses the same one.

## Consequences

- The test's subject is unchanged: a running service is still reached *by its visible state*,
  never by naming it, still through `find place` → `enter @-1` → `follow`. Only the spelling of
  "running" changed, from an impossible `state` to the two fields the contract actually has.
- The correction is strictly narrowing, not weakening: the old predicate matched nothing, the new
  one matches exactly the units the scenario describes.
- The acceptance case for §44.2 must select its fixture service the same way; a container whose
  service manager reports `active`/`exited` for a fixture unit is not a running service and must
  not be treated as one.
- Where no service manager answers at all, nothing changes: §35.2 and §2.17 still require the
  services place to report `unknown`/`permission_denied`/`unsupported`/`stale` rather than an
  empty system, and the test asserts that instead.

## Alternatives considered

- **`state == "active"` alone** — the most literal repair of the predicate, and it does select
  running services. Rejected because it also selects every `.device`, `.target`, `.mount` and
  oneshot unit on the host, and `enter @-1` then lands on a place with no process, which is not
  the object §44.2 describes and leaves the "follow one of its processes" step vacuous.
- **`substate == "running"` alone** — closest to the scenario's word. Rejected because `substate`
  is by contract a free string owned by the provider; pinning the scenario to it alone makes the
  test's meaning depend on a vocabulary Ono does not define.
- **Adding `running` to the `state` enum of `ono.service/1`** — would make the original predicate
  work. Rejected: it breaks an inherited v0.2 contract to accommodate a test, invents a state no
  provider reports, and confuses the two axes the schema deliberately separates.
- **Leaving the test `#[ignore]`d** — rejected. The capability is delivered; only the fixture's
  predicate was wrong, and AGENTS.md §11 makes a test coupled to something other than the
  behaviour the defect.
