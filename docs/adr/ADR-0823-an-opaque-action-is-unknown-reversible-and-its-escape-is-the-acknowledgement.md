# ADR-0823: An opaque action is unknown-reversible, and its escape is the acknowledgement

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §6.2, §6.3, §8.1, §9.6, §19.4
- Decided by: agent (autonomous)

## Context

§6.3 says an opaque action "MUST require an explicit risk acknowledgement and MUST classify impact
and reversibility as unknown". The plan showed `nothing was recorded as irreversible` and an impact
graph that claimed to be complete. Marking the effect irreversible would make §19.4 demand
`--accept-irreversible` for an action nobody knows to be irreversible — a claim Ono cannot make.

## Decision

- The opaque effect carries domain, kind and confidence `unknown`, and is **not** marked
  irreversible: §19.4 gates *known* irreversible actions. The views list it under
  `not recoverable` as `reversibility unknown` and name the command.
- An effect in the unknown domain or of unknown confidence ends the impact graph at an
  `UnknownBoundary` and marks the graph incomplete (`ImpactGraph::truncated` now covers "an effect
  Ono has no model of" as well as a budget).
- **§6.3's explicit risk acknowledgement is the escape itself**: `--opaque`, admitted only where
  `change.allow_opaque_actions` is set, written per plan and stored in the sealed plan as the
  action's `Execution::Opaque`. The plan's risk is `UNKNOWN` (ADR-0806 ranks it below `HIGH`), so
  no second flag is added at apply.

## Consequences

An opaque plan is visibly incomplete and unknown-reversible without inventing a HIGH risk.
Tests: `crates/ono-change-impact/tests/impact.rs` (two unknown-boundary tests),
`crates/ono-cli/tests/change_opaque.rs`, acceptance case 284.

## Alternatives considered

- *Irreversible.* Rejected: states something Ono does not know, and trains operators to type
  `--accept-irreversible` for actions that may be trivially reversible.
- *A new "reversibility unknown" field on `ProposedEffect`.* Not needed: kind and confidence
  `unknown` already say it, and the views derive the row from them.
