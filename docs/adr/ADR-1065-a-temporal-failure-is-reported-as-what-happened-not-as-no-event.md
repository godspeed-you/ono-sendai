# ADR-1065: A temporal failure is reported as what happened, not as "no event retained"

- Status: accepted
- Date: 2026-11-24
- Spec refs: v0.5 §4.4, §10.2, §10.3, §10.7, §11.6, §12.2, §34, §43.4, §44.3, §55.5
- Issues: #142
- Decided by: agent (autonomous)

## Context

CI once answered `at event @efa3` with `temporal.invalid_time … no event with that reference is
retained` one shell after `inspect event @efa3` had resolved the same reference. Three code paths
produced exactly that answer without it being true:

1. `temporal::configure_from` applied the session's configuration — the zone, the configured
   session ledger and, with `temporal.recording.enabled`, the recorder — only when
   `session_state().try_lock()` succeeded at once. A held state skipped all of it, and the shell
   then read §10.7's empty session ledger.
2. It discarded `start_recorder`'s result (`Ok(_) | Err(_) => {}`).
3. `LedgerAnchors::instant_of` dropped the ledger's error with `.ok()`, so an ambiguous prefix
   (§11.6) or a store that could not be read became "not retained". The trait it implemented,
   `ono_temporal_core::EventAnchors`, had no way to say anything else: it returned `Option`.

The third is the likely cause of the CI failure. Every recording shell appends its own coverage
events to the store, so a four-digit reference minted in one shell can name two events in the
next; `inspect event` raised `temporal.ambiguous_event` for that, `at event` said "not retained".

## Decision

1. **`EventAnchors::instant_of` returns `Result<Option<Timestamp>, ErrorValue>`.** `Ok(None)` is
   "no such event"; an error is carried to the reader as the ledger raised it. The session
   ledger's own implementation refuses an ambiguous prefix by name, as its `event` does.
2. **One resolution for an event reference** — `temporal::coordinate::retained_event` — used by
   `at event`, `inspect event` and `why event`. It distinguishes, with codes of §34's family:

   | What happened | Answer |
   |---|---|
   | the ledger holds exactly one matching event | the event |
   | the reference names several | `temporal.ambiguous_event` (E1306), with the candidates |
   | the store could not be read | the store's refusal (`temporal.store_unavailable` E1307, `temporal.store_corrupt` E1308, …) |
   | this session showed the event and the ledger no longer holds it | `temporal.out_of_retention` (E1303), with the event's instant and the earliest instant retained |
   | no event matches, and persistent recording was asked for and could not be brought up | `temporal.store_unavailable` (E1307), naming the reason the store did not open |
   | no event matches | `temporal.invalid_time` (E1301), as before |

   A session-issued reference resolves through the session first, so it keeps naming the event
   the reader saw (§11.6); that is what makes the "shown, no longer retained" case knowable. The
   reference therefore keeps the instant of the event it names.
3. **The configuration is never skipped.** `configure_from` hands it to the temporal state:
   applied at once where the state is free, and otherwise by the next holder of the state —
   `temporal_session()` and `try_temporal_session()` apply a pending configuration before they
   return the guard. It never waits for the lock, because its caller is synchronous and may run
   under a guard of the same state further up its own stack.
4. **A refused start is kept.** Where the start the configuration asked for is refused, the
   refusal becomes the `diagnostic` `get recorder` reports, until a later start succeeds. A store
   that could not be opened was already reported there as `health: failed` with its reason
   (§44.3); a test now pins it.

## Consequences

- The next failure of `timeline::should_carry_a_rendered_reference_into_inspect_at_and_why`, if
  there is one, names its cause in the CI log: ambiguity, a store refusal, retention, or an
  event that truly is not there.
- `EventAnchors` is a library trait; its two implementations in the workspace and the fuzz target
  changed with it. A third-party implementation would have to return `Ok(...)`.
- Tests: `crates/ono-cli/tests/event_reference_truth.rs` (all four answers through the binary),
  `crates/ono-temporal-core/tests/time_selector.rs`
  (`should_carry_the_ledger_refusal_when_the_event_anchor_cannot_be_read`), and the unit test
  `temporal::session::tests::should_apply_the_configuration_when_the_state_was_busy_as_it_arrived`
  — the only one below the binary, because whether the state is held at the instant
  configuration arrives cannot be arranged from outside the process.
- The refused-start diagnostic (decision 4) has no end-to-end test: configuration files may not
  run commands (ADR-0010), so the only refusal `Recorder::start` raises, E1314, needs two starts
  with different settings, which the startup path never makes; and a store that opens and then
  refuses §44.1's first writes could not be produced deterministically (a read-only store file
  is written through its write-ahead log).

### Neighbouring paths read for the same shape

Fixed here: the three above. Left, with the reason:

- `Ledger::retention()` (`ono-temporal-ledger` store) reads its count and bounds with `.ok()`, so
  a store that fails that query reports zero events. `LedgerRead::retention` is infallible by
  signature; making it fallible changes 27 call sites and 40 test call sites across six crates.
  The aggregates are read over the connection the same command has just used, so a broken store
  surfaces on that command's own read first. Recorded as a finding rather than changed in a
  stabilization increment.
- The write paths — `record_action`'s `let _ = record_into(...)`, the observation `append` and
  `record_coverage` in `temporal::events`, and `maintain`'s `let _ = maintenance(...)` — drop a
  refused *write*. That is deliberate (§16.5: losing history is not a reason to lose the
  command's result), but nothing then records the lost stretch as a gap, so a later timeline can
  read it as quiet. Making it visible is a recorder-health feature of its own; recorded as a
  finding.
- `carry_over` in `temporal::session` drops a failed read of the ledger being replaced. It is
  replaced in #147's increment (ADR-1068), which stops reading the store there at all.

## Alternatives considered

- **Wait for the lock in `configure_from` (`blocking_lock`).** Deadlocks where the caller already
  holds the state up its stack, and panics inside the runtime.
- **One code (`temporal.invalid_time`) with a `reason` field for all four.** Keeps scripts that
  match on the code working, but a script that branches on `temporal.store_unavailable` to retry
  (it is `retryable`) could not, and §34 already has a code for each case.
- **`temporal.not_recorded` for a shown-and-dropped event.** It says nothing was recorded, and
  this event was: the reader saw it.
