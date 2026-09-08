# ADR-0712: A queued systemd job answers with its identity

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.5 §15.2, §17.2, §17.3, §17.4, §21.6, §22.2; v0.2 §11.5, §16.5, §23.3, §50;
  ADR-0239, ADR-0561
- Decided by: agent (autonomous)

## Context

`org.freedesktop.systemd1.Manager.StartUnit` and its three siblings answer with the object path of
the job they created: `/org/freedesktop/systemd1/job/4821`. `zbus` decodes it, `SystemBus::queue_job`
received it, and the last line of that method was `.map(|_| ())`.

That path is the service manager's own transaction identity. v0.5 §17.3 requires the event ledger
to record the mapping from Ono's `ActionId` to it — the specification's example is exactly
`ActionId ono:a91f -> systemd job /org/freedesktop/systemd1/job/4821` — and §15.2 names a systemd
job result identifying a unit transition among the few pieces of evidence strong enough for
`caused_by`, in a list whose closing sentence is "Temporal proximity is insufficient". Without the
path, a restart and the state change that follows it are two events near each other in time, and
§15.2 says that is not a cause.

Nothing about acquiring it costs anything. The call is already made and the answer is already
decoded.

## Decision

### 1. `JobRef { path, id }`, in `bus.rs`

The path verbatim, and the numeric id parsed out of its final segment. The path is authoritative
and the number is a convenience for a consumer correlating a `JobRemoved` signal by id; where the
final segment is not a number the id is `None` rather than invented. `JobRef::token()` renders
`systemd:<path>`, which is the `external_transaction` form §17.4 asks an action record to carry,
prefixed by the authority that issued it so a ledger holding several never has to guess.

### 2. `SystemdBus::queue_job` returns `Result<JobRef, BusError>`

A widened signature rather than a defaulted second method. `unit_properties_at` is the
defaulted-method precedent in this trait and it exists for a genuine reason — a bus with no cheaper
path must still be correct — but there is no cheaper path here and no correct implementation that
does not have the job. A second method would let an implementation answer both ways about one
call. The three implementations in the tree are updated: `SystemBus`, `RecordedSystemd` and the
recorded manager in `ono-provider-linux/tests/storage.rs`.

### 3. The identity travels on the `ActionOutcome`

`ActionOutcome` gains `with_metadata(key, value)` and `metadata()`. A successful job carries
`systemd.job` (the path) and `systemd.job_id` (the number); a mount unit's job carries
`systemd.job`. Keys are namespaced by the authority that issued the value.

An outcome is the only thing a mutation produces, so an identity that is not on it is an identity
that was thrown away. It is deliberately **not** put on `ActionResult`: that is what a user reads,
and an external transaction id is evidence for the ledger rather than a column in a result table.
`ActionOutcome::into_record` therefore drops it, and says so.

A refused job carries no job reference, and neither does a skipped one. There was no transaction,
so there is no transaction identity, and a causal token in the ledger with nothing behind it is
worse than none.

### 4. `UnitProperties` gains `job`, because it is free

`org.freedesktop.systemd1.Unit.Job` arrives in the same `GetAll` that already carries `ActiveState`,
`SubState` and the rest — the provider reads the whole `Unit` interface in one call (ADR-0561), so
this is zero extra D-Bus traffic. It is the pair `(job id, job object path)`, and systemd writes
`(0, "/")` for a unit with no job in flight; both halves of that idle value are rejected, so "no
job" never becomes a job reference to the root path. A unit read while a job is pending can
therefore have a later transition attributed to that job, which is §15.2's evidence again from the
read side rather than the write side.

## Consequences

- `systemd` is the one provider in the tree that claims `causal_tokens` (§21.6, ADR-0711), and the
  claim is now backed on both the write path and the read path.
- `RecordedSystemd` issues ascending job ids from a fixed `FIRST_JOB_ID = 4821`, so an assertion
  about a job identity is a value rather than a shape, and two mutations in one test get two
  identities. `postgresql.service` is recorded with a job already in flight at
  `/org/freedesktop/systemd1/job/3107`, so the read side has a fixture too.
- Nothing user-visible changes. `ActionResult` is unchanged, `start`/`stop`/`restart`/`reload`
  answer exactly as before, and no schema moves.
- The consumer that records the mapping does not exist yet. §17.3's ledger row is agent B's and
  agent C's; this ADR makes the identity available to it, and the shape it will read is
  `outcome.metadata_value("systemd.job")`.
- Encoded by `crates/ono-provider-systemd/tests/service.rs`:
  `should_answer_with_the_job_the_service_manager_created_when_one_is_queued`,
  `should_carry_the_job_reference_on_the_outcome_of_a_mutation`,
  `should_carry_no_job_reference_when_the_service_manager_refused_the_job`,
  `should_carry_no_job_reference_when_the_unit_was_already_in_the_requested_state`,
  `should_give_two_mutations_two_job_identities`, and
  `should_report_the_job_a_unit_already_has_in_flight_when_its_properties_are_read`.

## Alternatives considered

- **Subscribing to `JobNew`/`JobRemoved` instead.** A signal match on the bus would give job
  results as well as job creation, which is what §22.2 eventually wants. It is a live subscription
  this provider does not have and a second connection to manage; the return value of a call
  already made is free, and it is the half §17.3 actually requires.
- **Putting the path in `ActionOutcome::message`.** A sentence a human reads is not a token a
  ledger can key on, and parsing it back out would be exactly the text-scraping spec §50 forbids.
- **A defaulted `queue_job_with_identity` beside the old method.** Two ways for one call to answer
  about one job, and the old one stays the easy one to call.
