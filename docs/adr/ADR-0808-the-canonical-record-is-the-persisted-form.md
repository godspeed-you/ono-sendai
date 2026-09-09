# ADR-0808: The canonical record is the persisted form, and the digest is the round-trip proof

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.6 §36.1, §36.2, §36.3, §41.2, §46, §63.2; v0.5 §31; ADR-0030, ADR-0571
- Decided by: agent (autonomous)

## Context

§36.1 requires sealed plans to survive shell exit. §41.2 requires plan state to be reconstructable
from persisted action records after a crash. §46 defines the public schemas a consumer reads. §63.2
requires sealed plans to be immutable and digest-verified.

Those are two serialisations if you let them be: the public record a caller sees through
`| to json`, and a private encoding the store uses. v0.5 took that route for the temporal ledger,
where the two are genuinely different problems — the ledger stores millions of events and indexes
them relationally, and CBOR beside the indexed scalars is the right shape for that.

A plan store is not that. It holds tens of plans, reads them whole, and its consumers are `get
plan`, `inspect plan`, and the executor. Two encodings there would buy nothing and cost the one
thing that matters: they can disagree, and the disagreement surfaces as a plan that comes back
from disk subtly different from the one that went in.

## Decision

**The `ono.change-plan/1` record is the persisted form.** `ono_change_core::value` encodes a plan
to a `RecordValue` and decodes one back; the store writes `ono_value::to_json` of it — the lossless
tagged encoding of ADR-0030, not the interop form — and reads it back through the same bridge.

Three consequences are deliberate:

1. **The wire and the disk cannot drift**, because they are the same bytes through the same code.
   A field the schema does not carry is a field the store loses, which is a test failure rather
   than a silent difference.
2. **The digest is the round-trip proof.** §63.2 asks for sealed plans to be digest-verified;
   `ChangePlan::digest_holds()` recomputes §4.4's digest over the decoded plan and compares it to
   the stored one. A round trip that loses anything the seal covers fails that check, and the
   tests exercise it directly.
3. **A schema gap is a real defect, not a rendering nicety.** Three were found and closed this way:
   Appendix A.7's policy declaration of irrelevance, §38.1's measured creation latency and quiesce
   duration, and §23.3's `expression`. Each had been dropped on the way to the record; each made a
   plan come back sealing to a different digest, or an asset come back looking cheaper than the one
   that was created.

Reconstruction needs to produce a plan in a state the public builders cannot reach — `APPLYING`,
with a digest it did not just compute — so `ChangePlan`, `PlanAction`, `RecoveryAsset`,
`RecoveryPlan`, `ProposedEffect` and `NewerStateImpact` each carry a `pub(crate) restore`
constructor reachable only from `value`. §41.2 is why: a plan that came back as a fresh draft would
have lost exactly the fact the operator needs after a crash.

§36.3's secret rule rides on the same path: redaction happens before encoding, so the handle is
what is digested and the seal still verifies after the round trip.

## Consequences

The store is small: a table of plans keyed by id, a table of action statuses, a table of assets,
and the apply claim. Migrations follow `crates/ono-temporal-ledger/src/migrate.rs` — a `STEPS` list,
a `STORE_VERSION`, each step in its own transaction, and a store at an unknown version refused
rather than downgraded.

Reading a plan requires the schema registry, which means the store cannot be read by a build whose
`ono.change-plan/1` has moved incompatibly. That is the intended behaviour and the reason the
schema is versioned.

`NewerStateImpact::complete` travels rather than being re-derived, because §62.8 makes "the
analysis did not run" a distinct state from "the analysis found nothing", and only the writer knows
which.

## Alternatives considered

**CBOR of a serde mirror, as the ledger does.** Rejected: two encodings that can disagree, for a
store whose read pattern does not need the second one.

**Store the plan as its digest plus a rebuild recipe.** Rejected: §41.2 needs the state after a
crash, and a recipe re-run against a changed world does not reproduce it.
