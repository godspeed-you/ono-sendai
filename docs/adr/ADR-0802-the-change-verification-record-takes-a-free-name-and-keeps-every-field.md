# ADR-0802: The change verification record takes a free schema name and keeps every field §23.3 fixes

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.6 §46.6, §23.3; v0.2 §31.36; ADR-0012, ADR-0611
- Decided by: agent (autonomous)

## Context

v0.6 §46.6 names the verification result schema `ono.verification-result/1`. That id is already in
`docs/contracts/schemas/verification-result.v1.yaml` and has been since the KUANG/11 tranche: it is
the output of `verify plugin`, and v0.2 §31.36 defines it as the answer to four separate questions
about a package artifact — integrity, signature, publisher trust and compatibility. It ships, it is
embedded in `ono-value`'s built-in registry, and `docs/contracts/commands/kuang.yaml` names it.

The two records have nothing in common. One says whether these are the bytes somebody signed; the
other says whether the state a plan intended exists. Merging them would produce a schema whose
required fields are the union of two unrelated questions, and §31.36's own reasoning against
collapsing its four answers into one `trusted: yes/no` applies with more force to collapsing two
schemas into one.

This is the same collision v0.5 §35 had with `ono.evidence/1`, resolved by ADR-0611 the same way.

## Decision

The v0.6 record is registered as **`ono.change-verification/1`**, in
`docs/contracts/schemas/change-verification.v1.yaml`, with every field §23.3 fixes —
`plan_id`, `check_id`, `class`, `status`, `observed`, `expected`, `evidence`, `timestamp` — plus
`subject`, `expression`, `equivalence_domain` and `detail`, which §23.3's example and §25.1 require
in practice.

`ono.verification-result/1` is untouched. Nothing about `verify plugin` moves.

`docs/contracts/change/plans.yaml` records the rename in its `schemas:` list, so a reader coming
from §46.6 finds the mapping in the registry rather than by grepping.

## Spec deviation

- Section: v0.6 §46.6
- Text: "`ono.verification-result/1`"
- Instead: the schema is registered as `ono.change-verification/1`, with every field §46.6 and
  §23.3 name.
- Why: v0.2 §31.36 holds the name for an unrelated record that ships. §40.4 treats a stable schema
  id as a compatibility promise, and re-pointing one at a different meaning would break every
  caller of `verify plugin` to satisfy a naming preference.

## Consequences

A script that reads §46.6 and matches on `ono.verification-result/1` will not match a change
verification. The registry entry and this ADR are where that is discoverable, and the schema's own
header states it in the first paragraph a reader sees.

`ono.change-verification/1` reads slightly better than the name it replaces, because the family it
belongs to is now visible in the id — the same argument that made `ono.temporal-evidence/1` an
improvement rather than a compromise.

## Alternatives considered

**Version the existing schema to `/2` with a union of both field sets.** Rejected: a schema whose
required fields answer two unrelated questions is the failure §31.36 spends a paragraph warning
against.

**Rename `verify plugin`'s output instead.** Rejected: it is the older claim, it ships, and §5.2's
authority order puts an enhancement below the base it layers on when the two collide over a name.
