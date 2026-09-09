# ADR-0715: A unit transition is an event carrying the job that caused it

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.5 §3.3, §6.3, §15.2, §18.2, §21.3, §21.5, §21.6, §22.2, §22.8, §26.1;
  v0.2 §31.14, §50; ADR-0235, ADR-0561, ADR-0710, ADR-0711, ADR-0712
- Decided by: agent (autonomous)

## Context

v0.5 §22.2 is two clauses: the systemd provider "SHOULD contribute live unit state transitions and
job identity where available". ADR-0712 delivered the second — `StartUnit` and its siblings answer
with the job's object path, and `Unit.Job` names a job in flight — and left the first. Unit state
was read on demand, `watch service` fell back to the runtime's poll loop, and every event's time
was the poll's rather than the manager's.

That gap is what stops §15.2's strongest built-in rules from firing.
`docs/contracts/temporal/causality.yaml` declares `ono.systemd-job-result` and
`ono.systemd-job-to-unit-state` with `required_evidence_strengths` of `authoritative` on both
sides and an identity constraint joining a unit transition to a job path. Without a transition
that carries a job path, neither rule has an input, and the only remaining join is temporal
proximity — which §15.2 does not admit and §15.5 caps at `correlated_with`.

`org.freedesktop.systemd1` broadcasts exactly what the rules need, to any client that has called
`Manager.Subscribe`: `JobNew(id, job, unit)`, `JobRemoved(id, job, unit, result)`,
`UnitNew(unit, path)`, `UnitRemoved(unit, path)`, and `PropertiesChanged` on each unit's own
object path.

## Decision

**`SystemdBus` gains `subscribe_units`**, which calls `Manager.Subscribe` and answers with a
stream of `UnitSignal`. The default refuses, so a bus with no signal surface and every test double
keeps compiling and claims nothing. `SystemdProvider` implements `Provider::subscribe` over it and
advertises `live_events: true`.

Four rules fix what a signal becomes.

1. **A transition is attributed to the job in flight for that unit, and to nothing else.** `JobNew`
   records the job against the unit; a transition observed while it is recorded carries
   `systemd:<job path>` as the event's cause; `JobRemoved` carries the job it names and then
   clears it. Nothing is attributed by proximity. `ObjectEvent::with_cause` is the envelope, added
   to `ono-provider-api` beside `with_sequence` as INTERFACES §3 anticipated.

2. **A transition's time is `StateChangeTimestamp`, not the read-back's clock.** systemd records
   when the unit moved; the instant this provider got round to asking is a fact about the provider
   (§3.3). `ObjectEvent::with_observed_at` carries it. A `StateChangeTimestamp` of zero is a unit
   that has never moved, and stays absent rather than becoming the epoch (§35.3).

3. **`UnitNew` and `UnitRemoved` produce no object events.** They are the manager's own memory
   management — systemd loads a unit when something asks about it and garbage-collects it when
   nothing does — and neither says the service appeared or went away. Deriving a disappearance
   from `UnitRemoved` would be manufacturing one, which §6.3 forbids. The signals are still
   decoded, and `UnitRemoved` drops the unit from the subscription's baseline, so a unit that
   comes back is honestly reported as appearing rather than as changed against a stale record.

4. **The signal loop never calls `Manager.LoadUnit`.** It reads properties at the object path the
   signal carried, and where a signal carries none — `JobRemoved` does not — at the path
   `sd_bus_path_encode` gives a unit of that name, which `unit_object_path` computes. Decoding and
   encoding that path is reading a structured identifier systemd constructed, not parsing human
   output (§50).

Rules 3 and 4 are one finding, and it was the live test that found it. Against a real user manager
the first implementation read the unit back on `UnitNew`, the read loaded the unit, the manager
announced the load, the announcement caused another read — a reader turned into a writer, emitting
an unbounded alternation of `added` and `removed` for one transient unit. `should_report_a_real_
unit_transition_with_the_job_the_manager_gave_it` in `crates/ono-provider-systemd/tests/live_
signals.rs` is where that surfaced, and it is why the file exists: a recorded manager cannot
disagree with the code that recorded it.

**The live evidence is taken from the per-user manager.** §22.8 requires core v0.5 to be useful on
an ordinary account, so the proof that live transitions work must be obtainable without root. An
unprivileged account may queue jobs on `user@<uid>.service`, so `SystemBus::user()` opens the same
`Manager` interface on the session bus and the test drives a transient unit there. `get service`
still answers from the system manager; nothing about which manager a query reaches has changed.

**`exhaustive_events` stays false.** systemd coalesces `PropertiesChanged` and sends property
names rather than values, so two transitions inside one coalescing window arrive as one. §21.5
makes the claim one about sequence continuity strong enough to support an absence claim, and a
coalescing source cannot keep it.

## Consequences

- `watch service` is live rather than polled where a service manager answers, and falls back to
  polling where none does — `Provider::subscribe` refuses with `provider.unavailable` there, and
  `ono_command::watch_events` already treats a refusal as "poll instead" (§18.2).
- `ono.systemd-job-result` and `ono.systemd-job-to-unit-state` have an input. The causal engine can
  join on a job path from an `authoritative` source at both ends.
- A subscription costs one `GetAll` per signal for the units the query keeps. On an unnarrowed
  `watch service` over a busy manager that is real bus traffic, bounded by the signal rate and by
  the 256-message queue the match rules are registered with.
- **A service whose unit file is deleted is not reported as gone.** The only signal systemd sends
  for it is `UnitRemoved`, which rule 3 refuses to read as a disappearance, and the honest
  alternative — re-reading to see `LoadState: not-found` — is the read that caused the feedback
  loop. Object disappearance for services therefore remains the recorder's to derive from
  snapshots, with provenance `snapshot_diff` (§22.1's mechanism, applied to a different source).
- `SystemBus::user()` is public and no product path calls it. It is the D-Bus surface of the
  per-user manager, and the crate's own live test is its first consumer.

Encoded by `crates/ono-provider-systemd/tests/service.rs` —
`should_report_the_job_that_caused_a_unit_transition_when_one_was_in_flight`,
`should_time_a_transition_by_the_instant_systemd_recorded_it_rather_than_the_read_back`,
`should_not_report_a_unit_the_manager_merely_unloaded_as_having_gone_away`,
`should_ignore_a_transition_of_a_unit_the_subscription_did_not_ask_about`,
`should_claim_live_events_once_it_subscribes_to_the_managers_own_signals` — and by
`crates/ono-provider-systemd/tests/live_signals.rs` against a real manager.

## Alternatives considered

**Poll unit properties faster.** Cheaper to write and it would never carry a job identity: a poll
sees a state, never the transaction that produced it. §21.3 is about the source pushing, and
§15.2 is about the evidence, and polling fails both.

**Subscribe to `JobRemoved` only.** It names the unit and the result and would support
`ono.systemd-job-result` alone. It would miss every transition a job produces while it runs, which
is what `ono.systemd-job-to-unit-state` is for, and it would miss transitions no job caused —
a unit that failed on its own.

**Read the unit back through `Manager.LoadUnit`.** The obvious implementation, and the one the
live test rejected: loading a unit to observe it makes the manager announce the load, and the
announcement is another thing to observe.

**Derive appearance and disappearance from `UnitNew`/`UnitRemoved`.** Tempting, and false: they
report the manager's memory, not the machine's services. A `watch service` that announced dozens
of services vanishing because nobody had asked about them lately would be worse than one that
announces none.
