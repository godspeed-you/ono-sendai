# ADR-0561: An enumeration uses the object path the listing already gave it

- Status: accepted
- Date: 2026-09-03
- Spec refs: v0.4.1 §32.3, §33.2, §37.1; spec §23.3, §50; ADR-0488
- Decided by: agent (autonomous)

## Context

Issue #9 measured `get service | count` at 3.01 s in a debug build against `get service <unit> |
to json` at 0.15 s, so the cost is in enumerating the units rather than in reading any one of
them. The checked-in benchmark record for `service.enumeration` at Profile S — release, the
reference environment — is 422 ms, which is the same shape in the units a release build works in.

The provider asks systemd for the list, and then, per unit:

1. `Manager.LoadUnit(name)` → the unit's D-Bus object path;
2. `Properties.GetAll(org.freedesktop.systemd1.Unit)`;
3. `Properties.GetAll(org.freedesktop.systemd1.Service)`, for a `.service` only.

`Manager.ListUnits` already answers with each unit's object path — it is the seventh column of
the row — and the provider was dropping it. So the first of the three round trips was asking
systemd for something it had already said.

The reads are already issued in a bounded window of 32 (ADR's `UNITS_IN_FLIGHT`), so the cost is
not serialisation; it is the number of round trips. On a host with 597 units, 568 of which the
enumeration reports and 225 of which are services, the three steps are 568 + 568 + 225 = 1361
calls.

## Decision

**`UnitListing` carries the object path `ListUnits` gave, and the enumeration reads properties at
that path.** `SystemdBus::unit_properties_at(unit, path)` is the by-path form; its default
implementation answers through `unit_properties(unit)`, so every existing implementation and
every test double is correct without changing, and the D-Bus one overrides it to skip `LoadUnit`.

The by-name path — `get service nginx`, and every action — is untouched: there is no listing
behind it, so it still asks `LoadUnit`, and it still tries the `.service` suffix a user left off.

**Nothing about the answer changes.** The same units are reported, in the same order, with the
same fields; only the number of messages on the bus is smaller. That is why this is a `perf`
change with no test change (AGENTS.md §4): the proof is the benchmark, and the existing suite is
what says the behaviour held.

A unit that appears in the listing and is gone by the time its properties are read still answers
`Ok(None)` and is skipped, exactly as before — a snapshot of a moving system, not a failure. What
changes is only *which* call notices: `GetAll` against a path that no longer exists rather than
`LoadUnit` against a name that no longer resolves.

## What the issue asked for, and what was already true

Issue #9's exit test is "`services_logs_missing.rs` green under the default test parallelism".
**It already is, and this change is not why.** The file is 20/20 green on its own and 20/20 green
inside a full `cargo test --package ono-cli`, measured before this change and after it. What
closed that half was the v0.4.1 tranche: ADR-0517 made `ono_testkit::Shell`'s watchdog scale with
the load the test does not control, on the grounds that a watchdog carries no claim about the
product. The two tests the issue named were reporting the machine, and they no longer do.

What remained was the cost itself, and that is what this ADR is about.

## Consequences

- 1361 D-Bus calls become 793 on the host above: 42% fewer, and the same fraction wherever the
  ratio of services to other units is similar.
- **Measured, `get service | count` over 569 units on the reference developer machine:**
  1.78 s → 1.20 s in a debug build, three runs each, minutes apart. The issue's 3.01 s is a debug
  figure of the same shape on a busier host.
- **In a release build the figure does not move: 0.40 s before and 0.40 s after.** Thirty-two
  reads are in flight at a time, so the removed round trip is hidden behind the ones beside it,
  and what is left is systemd's own latency — about 0.3 ms per call — rather than anything the
  shell spends. Saying so is the point of measuring: the work removed is real and the wall clock
  a release user sees is not where it shows. `cargo xtask perf --profile S` could not adjudicate
  it on 2026-09-03 either, because a second build tree held the machine at load 20–30 and every
  one of the eight benchmarks read three to five times its checked-in baseline, `shell.cold_start`
  included.
- Where the ratio of round trips matters is a bus under load and a machine slower than this one,
  and there a third fewer messages is a third fewer messages.
- `SystemdBus` gains one method with a default. `ono-provider-linux`'s storage provider drives
  the same trait for mount units and is unaffected.
- The listing's `path` is `Option<String>` because a row without one is a row this provider will
  not trust to be a path; it falls back to the by-name read, which is the behaviour that existed.

## Alternatives considered

- **Read only `GetAll(Unit)` and drop the Service interface.** It would remove a third call for
  every service, and it would drop `MainPID` — `ono.service/1`'s `pid` — from every record.
  Rejected: §35.3 forbids answering with a null the provider could have filled.
- **Build the record from `ListUnits` alone**, which carries name, description, load, active and
  sub state. Rejected for the same reason: `enabled`, `since`, `pid` and `unit_file` would all
  become null, and a cheaper answer that says less is not the same answer.
- **Raise `UNITS_IN_FLIGHT`.** It would hide latency rather than remove work, and §28.1 keeps
  every queue in this shell bounded on purpose.
- **Answer `get service` with `.service` units only.** It would remove 60% of the work, and it is
  a different command: `docs/spec/commands/service.yaml` says `get service` lists "service-manager
  units", and the mount units it reports are what `trace mount` relates to. A contract change is
  not a performance fix.
