# ADR-0825: Apply keeps the protection the sealed plan showed, or refuses

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §2.3, §4.5, §10.3, §17.2, §18.2
- Decided by: agent (autonomous)

## Context

A sealed plan carries its coverage matrix, not the actions that produce it, so `apply` recomputes
them. When the recomputation came back empty — a provider gone, a mount changed — a plan sealed as
PROTECTED applied with no protection at all, and nothing said so. That is a silent downgrade from
protected to unprotected execution.

## Decision

`ono_change_protection::coverage::protection_lost(sealed, fresh)` names every domain the sealed
matrix showed as satisfied that the fresh analysis no longer satisfies. `apply` refuses before
anything is prepared, whatever the protection mode: the protection the plan showed cannot be
created, which is a preparation that failed, so the refusal is `change.prepare_failed` and the plan
is recorded `PREPARE_FAILED` (§4.1: no edge to APPLYING; §55.3 case 13). A plan whose world moved is
refused as drift first, so a vanished target is reported as the drift it is; the help points at `rebase plan <id>`, which produces a revision showing what protection is
available now (§7.5). A row the sealed plan did not show as protected is not a promise: an
unprotected plan applies unprotected, as it said it would.

## Consequences

Tests: `crates/ono-change-protection/tests/sealed_promise.rs`. The CLI change suites are unchanged,
because an ordinary plan's recomputation agrees with its seal.

## Alternatives considered

Refuse only under `require` (what `check_discovery` already did). Rejected: `prefer` and
`maximize` plans are shown as protected too, and the lifecycle forbids a silent downgrade for
every mode.
