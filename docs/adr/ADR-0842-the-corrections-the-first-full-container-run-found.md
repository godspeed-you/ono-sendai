# ADR-0842: The corrections the first full container run found

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §2.3, §4.1, §22.2, §22.3, §23.3, §24.1, §28.6, §36.1, §37.3, §41.2, §41.3, §42.3,
  §42.4, v0.2 §43; ADR-0825, ADR-0833
- Decided by: agent (autonomous)

## Context

The first run of `scripts/acceptance.sh` over the whole v0.6 tranche passed 240 cases and failed
eight: 294, 295, 298, 303, 304, 306, 311 and 316. Each failure reproduced on the host against the
same binary, so none was the container's doing. One of them was the safety failure the
specification names most often: a plan sealed as protected mutated after its protection could not
be created. This record states what each correction decided, the way ADR-0833 did for the first
audit.

## Decision

1. **A preparation keeps the protection the operator approved (294, §2.3).** Two gaps let a plan
   sealed as protected mutate unprotected when its recovery store became unusable. The observed
   one: before judging protection, `apply` asked whether the plan had drifted, and counted any
   revalidation finding as drift — including one the executor treats as non-blocking and applies
   through. Drift made it skip ADR-0825's check, on the understanding that the executor would
   refuse; the executor did not, and ran with no protection action at all. The pre-check now asks
   the executor's own question — a blocking finding on a mutating action, or a revalidation that
   could not be answered — so one of the two always refuses. The second gap was in `prepare`
   itself, which treated the failure of an optional protection action as a shortfall, and under
   §17.1's default `prefer` every first-party action is optional: a domain the sealed plan showed
   protected, that the preparation set out to protect, now ends with a validated asset or the
   apply refuses with `change.prepare_failed`. A domain protected earlier by `protect` (§18.2) is
   outside that rule, because the preparation had no action for it; a failed `maximize` extra
   beside a created primary is outside it too.
2. **`verify` fails a script when a required check does not hold (295, §23.3, v0.2 §43).** The
   command still answers with every per-check result, and its stream ends with
   `change.verification_failed` when a required check failed or could not be answered. A pipeline
   receives the results and the exit status says what they mean. An advisory failure fails nothing.
3. **`get plan` of a recovery plan carries its analysis (298, §24.1, §36.1).** The stored
   `ono.recovery-plan/1` travels as the namespaced extension `ono.change/recovery`, so the plan a
   later process reads states the method it will run.
4. **An apply claim is bounded by its lease and by the life of its holder (303, §42.3).** The store
   (version 3) records the process that took a claim. A claim whose process no longer exists is
   taken over at once, so `resume` after a crash does not wait out the five-minute lease; a live
   holder's claim is never taken. A claim from an older store, with no process recorded, is treated
   as live.
5. **The claim names the holder before the state is judged (304, §42.4).** A plan another session is
   applying reads `applying`, and `apply` refused it as unsealed before it tried the claim. For an
   in-flight state the claim is tried first, so the refusal is `change.plan_already_applying`
   naming the session, with its process id as metadata; the durable state is checked under the claim.
6. **A canary gate checks the batches that ran (306, §28.6).** The gate evaluated every required
   contract after the canary wave, including the contracts about targets no wave had reached, and
   stopped a canary that did everything right. It now evaluates the contracts about the targets
   the waves have reached, and the plan-wide ones; the verification after the last wave evaluates
   them all.
7. **The state before a plan is on its timeline (311, §22.2, §22.3).** The ledger checkpoint serves
   reconstruction and is not an event. The digest of every file target, taken immediately before
   mutation, is now recorded as `ono.plan.checkpoint` beside the plan's `verification.observed`,
   so the state before and after compare as two events on one timeline.
8. **A blocked cleanup names the plans it protects (316, §37.3).** `recovery.cleanup_blocked` said
   how many plans an asset's removal would strand; it now names them, as its caller's comment
   already said it did.

## Consequences

Each correction has a test through the real binary: `crates/ono-cli/tests/change_prepare_failure.rs`
(1), `change_scripting.rs` (the failing `verify`), `change_recovery.rs` (the read-back analysis),
`change_claims.rs` (the named holder and the resume after a killed applier), `change_strategy.rs`
(the canary), `change_recovery_assets.rs` (the named plan); the store's own claim tests hold the
metadata. The eight cases run again in the container before any of their boxes is ticked.

## Alternatives considered

For 2, refusing without the results. Rejected: a script that pipes `verify` into `to json` reads
the per-check answers, and §23.3 makes them the result. For 4, a shorter lease renewed in the
background. Rejected: a single long action outlives any lease a heartbeat does not renew, and a
second applier could then take a plan that is still running.
