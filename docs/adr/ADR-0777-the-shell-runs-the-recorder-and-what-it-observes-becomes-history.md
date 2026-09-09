# ADR-0777: The shell runs the recorder, and what a session observes becomes history

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §6.1, §6.3, §8.1, §8.3, §9.2, §10.2, §10.6, §10.7, §10.8, §17.2, §17.3, §17.4,
  §21.5, §22.1, §32.1, §39.1, §43.2, §44.1, §55.5, §56.7
- Decided by: agent (autonomous)

## Context

`crates/ono-recorder` was complete and unreachable. `crates/ono-cli` did not depend on it;
`start recorder` swapped a persistent `Ledger` into the session and called nothing else, and
`crate::temporal::ingest_changes` and `ingest_provider_events` — both fully written — had no caller
in the workspace. Three consequences were reproducible on a real shell:

1. **No object appearance or disappearance ever became an event.** `find event 'kind ==
   "object.appeared"'` answered `[]` on every machine, however much had moved on it. §6.1 declares
   the kinds and §48.2's seventh scenario is "a process appearing is a typed event".
2. **A recorder restart was never a coverage gap.** §44.1 fixes five steps at every recorder start
   and step three is "mark any unobserved downtime as a coverage gap"; nothing ran them, so two
   runs of a recording shell joined across the interval between them. §55.5 names that as the
   failure that destroys operator trust, and §56.7 is a release criterion.
3. **`get recorder` reported sources it did not collect from**, because nothing collected.

Four further defects sat in the same functions and are fixed here for the same reason: an action
whose every target failed was recorded as `action.completed` with `result: "succeeded"`; §17.3's
external-transaction mapping never reached the ledger; `get recorder`'s `since` could never be
non-null because the reader and the writer each declared their own function-local `static`; and
`temporal.session.max_events` was reported by `get recorder` without ever being applied.

## Decision

### The command layer calls the recorder, and calls nothing else

`crates/ono-recorder/src/lib.rs`'s module documentation is the contract, and
`crate::temporal::recorder` implements exactly it: build [`RecorderOptions`], call `start`,
`status` and `stop`, and give `maintenance` a turn. §44.1's five steps, §43.2's continuity
declarations, §31.8's bounded retention and §42.2's checkpoint scheduling stay where they are.

- **One process, one recorder,** built on first use. `Recorder::new` opens nothing, so §32.1's
  disabled path still touches no filesystem; the only callers are a command the user typed or
  `temporal.recording.enabled` being on.
- **The recorder owns the ledger and the session borrows it.** `TemporalState::set_shared_ledger`
  takes the recorder's `Arc<Ledger>`, so `get recorder` and `timeline` cannot disagree about how
  much history exists.
- **`start recorder` and `temporal.recording.enabled` are one path.** Both run
  `recorder::start_recorder`, which is `Recorder::start` plus the ledger swap. §10.8's idempotency
  and ADR-0640's E1314 are the recorder's, not the command's.
- **Maintenance runs on a turn of the shell.** `get recorder` and every recorded action give it
  one. §39.2 makes every timer in this system somebody's call, and those two are the moments the
  shell has one to spare. A refused flush is dropped: §16.5 does not make it a reason to lose the
  answer the user asked for.

### The sources are what the shell actually collects from

One `SourceProfile` per kind of place a session sweep yields — `process` and `service` — all of
them sourced `ono.session`, polled, not `exhaustive_events` (§21.5) and not
`meaningful_disappearance` (§6.3). `ono.recorder` is not among them: the recorder is not a source
of system observations, it is what writes down the coverage and the gaps of the ones it holds.
`get recorder`'s `sources` is that list, deduplicated, and it is the same list whether or not
persistence is on — because the shell's own observations reach §10.7's session ledger either way.

The profiles are load-bearing for §44.1: the capabilities a downtime gap is filed under are their
`<type>.existence` capabilities, and a reconstruction gates object presence on that exact string.

### What a session observes becomes history, at the seam where it already compares

`SpatialSessionState::remember` is where the shell records what one provider target answered, and
therefore where it can compare that answer with the previous one. The difference becomes an
`ono_spatial_events::ChangeSet`, which `crate::temporal::events::observe_sweep` feeds through the
already-written `ingest_changes` and appends with its evidence. `map --live` feeds the `ChangeSet`
it already computes through the same function.

Two rules bound it, and both are refusals rather than comments:

- **A bounded read is a sample, not a snapshot.** §34.4 lets an orientation stop at
  `limits.orientation_objects`, and comparing two samples would invent appearances and — far worse
  — disappearances. `compare_target` answers `None` where either observation was bounded or either
  target did not serve, so no change is derived from a partial read (§6.3, §21.5).
- **The interval is the claim.** The changes are dated `ObservedAt::Between` the two observations,
  which is what `ingest_changes` turns into an evidence claim over a range rather than at a point
  (§9.2).

`observe_sweep` records the coverage that backs the events, per observed `SpatialType`, under
`<type>.existence`, at `partial` and sourced `ono.session` — even for a sweep that found nothing,
because "we were watching and nothing moved" is the fact a later reader needs and the one a silent
ledger cannot supply (§55.5). §8.3 fixes the session at partial and nothing here may raise it.

### The session's own coverage joins a capability rather than inventing one

`SessionEvidence::coverage` used to append the session's lifetime as a `session.events` interval.
`CoverageSummary::headline()` is `Complete` only when *every* composed capability is, and §8.3
fixes `session.events` at `partial` for ever — so that one interval held every composition below
`complete` whatever a recorder had written, and ADR-0775's `empty` was unreachable from any real
shell. The session's interval now joins the capabilities the record already speaks about, and only
falls back to `session.events` where the ledger reported no coverage at all. It adds a source,
which is what a session is; it no longer adds a capability nobody else can ever complete.

### An action's terminal kind comes from its rows

`crate::temporal::record_action` takes the `ActionOutcome`s rather than their count.
`action.failed` where every target failed and nothing went through; `action.completed` otherwise,
because §11.5 keeps `97 succeeded, 3 failed` as two readable numbers rather than one verdict and
the rows carry the detail. §17.3's external transaction is read off the same outcomes: a metadata
key whose last segment is `job`, `transaction` or `txn` is a transaction identity by that name —
`systemd.job` is the D-Bus object path `StartUnit` answered with — and nothing else on an outcome
is one.

## Consequences

- `find event 'kind == "object.appeared"'` answers with the appearances a session observed, and a
  recorder restart is a `coverage.ended` gap a pipeline can find. Both are proven end to end by
  `crates/ono-cli/tests/recorder_wiring.rs`.
- A recording shell writes bookkeeping into its own ledger: two `coverage.started` markers per
  invocation, and the `coverage.ended` markers of whatever downtime preceded it. That is the point
  — §8.1 makes a stored stretch distinguishable from an unwatched one — but it means **the ledger
  of a recording shell is no longer only action events**. `crates/ono-cli/tests/action_causality.rs`
  `should_name_the_actor_the_session_and_what_was_asked_when_an_action_is_recorded` asserts an
  `action_id` on *every* event `find event` returns, which was true only while nothing else
  collected. It belongs to another agent; the assertion needs to select the `action.*` kinds it is
  about.
- `empty` becomes reachable in `look`'s change section where a recorder has written complete
  coverage over the window, which is what ADR-0775 says should happen and did not.
  `spatial_look_changes.rs::should_not_claim_nothing_changed_when_the_window_is_not_covered_end_to_end`
  seeds complete coverage for `process.existence`, `service.existence` and `existence` and then
  requires `unknown` — it encodes the poisoning as the contract, so it flips. The safety direction
  it is named for still holds: `empty` still requires end-to-end complete coverage of every
  capability composed over the window.
- Recording stays opt-in and bounded. Nothing above runs with `temporal.recording.enabled` false
  and no `start recorder` except the in-memory session ledger §10.7 gives every session anyway.
- `temporal.session.max_events` now bounds the session ledger it is reported for, and the settings
  a start applies are the session's resolved ones rather than `RecorderSettings::default()`.

## Alternatives considered

- **Ingest at `SpatialSessionState::absorb` rather than at `remember`.** Rejected: `absorb` is
  handed records without knowing which target answered them or whether the answer was bounded, so
  it cannot tell a complete snapshot from a sample and would derive disappearances §6.3 forbids.
- **Give the recorder `linux.procfs` and `linux.systemd-dbus` profiles.** Rejected: nothing in the
  shell subscribes to or polls those providers on the recorder's behalf, so naming them would
  repeat the third defect this ADR exists to close — a status that lists a collector that is not
  collecting.
- **Ask `CoverageSummary::can_prove_absence(capability)` at the change section instead.** It is the
  better question and `crates/ono-cli/src/spatial/commands.rs` is where it would be asked; that
  file belongs to another agent, so the fix here is confined to what
  `SessionEvidence::coverage` composes.
- **Take the first metadata entry as the external transaction.** Rejected: `systemd.unit` is
  metadata and is not a transaction. A named suffix is a rule a provider author can follow.
