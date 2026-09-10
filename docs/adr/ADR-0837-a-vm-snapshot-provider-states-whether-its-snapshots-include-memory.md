# ADR-0837: A VM-snapshot provider states whether its snapshots include memory

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §16.2, §16.3, §48; ADR-0812, ADR-0833
- Decided by: agent (autonomous)

## Context

§16.2 requires a VM snapshot provider to distinguish memory-inclusive from disk-only snapshots,
and guest-quiesced from crash-consistent ones. ADR-0812 said a "memory-inclusion fact a provider
states" made that distinction; no such fact existed (ADR-0833, item 8). No first-party VM provider
ships — VM snapshots arrive as KUANG/11 recovery-provider contributions.

## Decision

`RecoveryProviderContribution` carries `memory_inclusion`: `memory-inclusive`, `disk-only`, or
`both` (the provider can take either kind). The supervisor refuses a package at load
(`package.invalid`, naming §16.2) when a contribution offering `vm-snapshot` does not state it,
when one offering any other asset type states it, or when the word is none of the three. The
contribution's `asset_type` is also held to the closed `RecoveryAssetType` vocabulary rather than
merely being non-empty. The quiesced/crash-consistent half of §16.2 is already the contribution's
`consistency`. The KUANG/11 test host applies the same rules, so a package that passes it loads.

## Consequences

The fact is stated and enforced where a VM provider enters the system. It does not yet travel onto
each `RecoveryAsset` or into the renderer; a VM provider's asset carries its consistency class, and
carrying memory inclusion per asset is the step to take when a VM provider exists to exercise it.
Tests: `crates/ono-kuang-supervisor/src/change.rs` (six fixtures),
`crates/ono-kuang-protocol/tests/change_contributions.rs`.

## Alternatives considered

Inferring memory inclusion from the consistency class. Rejected: §16.2 lists them as separate
distinctions, and a disk-only snapshot can be guest-quiesced.
