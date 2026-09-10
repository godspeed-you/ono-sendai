# ADR-0831: Early protection is assessed object by object, and the risk policy flag is honoured

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §18.1, §18.2, §18.3, §19.4, §53, §56.3
- Decided by: agent (autonomous)

## Context

§18.2: after an early `protect @plan`, "plan apply MUST assess whether it is still appropriate".
`apply` never looked at the early asset: it created fresh protection (which §18.1 prefers) and
ignored the earlier one, and `--accept-stale-protection` was parsed and never read. The freshness
module compared an asset's `captured_state` with a source-state fingerprint, but the providers
store the asset's own identity there (the file archive's manifest fingerprint, a snapshot's GUID),
so that comparison could not mean what it claimed. Separately, §53's
`change.high_risk_requires_ack` and `critical_risk_requires_ack` were read and never used, though
§19.4 names "non-interactive policy flag" as the alternative to an interactive acknowledgement.

## Decision

1. **Assessment is object by object.** `freshness::assess_captured` holds the digests an asset's
   provider says it captured (`RecoveryPlanFragment::captured`) against each object's digest now.
   Fresh only where every captured object still holds those bytes; an object that changed or
   cannot be read, and an asset that says nothing about what it captured, are stale — or
   must-replace under `require` (§18.3's third option).
2. **`apply` creates fresh protection as before (§18.1).** Where that recomputation cannot keep a
   row the sealed plan showed as protected (ADR-0825), an earlier asset attributed to the plan and
   covering every file it targets keeps the promise if it is fresh, or — with
   `--accept-stale-protection`, §18.3's second option — if it is stale, and the note says it is not
   a just-before-change point. Otherwise the refusal stands, and its help names the stale asset and
   the flag.
3. **The risk policy flag is honoured.** With `change.high_risk_requires_ack = false` (or the
   critical one), a plan of that class is sealed with its risk acknowledged by policy — stored in
   the sealed revision like any acknowledgement (§19.4). Irreversibility stays a gate of its own.

## Consequences

Tests: `crates/ono-change-protection/tests/freshness.rs` (five `assess_captured` cases),
`crates/ono-cli/tests/change_settings.rs` (the waiver and the default).

## Alternatives considered

Reusing an early fresh asset instead of creating a new one. Rejected for now: §18.1 prefers
protection immediately before mutation, and a second asset costs retention space rather than
safety. The early asset is still what keeps the promise when a fresh one cannot be made.
