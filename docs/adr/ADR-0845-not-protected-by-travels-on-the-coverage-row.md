# ADR-0845: `NOT PROTECTED BY` travels on the coverage row, from the provider that knows it

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §13.4, §10.3, §2.5, §55.3 case 11, Appendix B.8; ADR-0842
- Decided by: agent (autonomous)

## Context

§13.4 prints a block for a target on a separate dataset: the dataset that holds it, the snapshot
that protects it, and under `NOT PROTECTED BY` the snapshot an operator might have assumed covers
it — the root dataset's, in its own example. The ZFS provider could already print that block for
one path (`BoundaryReport`), and its candidates already named a child dataset as an exclusion of
the parent's snapshot. Nothing carried the other direction into a plan: a plan over a file in a
child dataset showed the child's snapshot and never said what the enclosing dataset's snapshot
would not do. Case 289's `zb3c` asks `get plan` for that block.

Which datasets belong in the list is a ZFS fact. A dataset encloses another in two ways an
operator can mistake for coverage: by name (`tank/data` above `tank/data/customer`), and by mount
path (`rpool/ROOT/debian` at `/` above `tank/data` at `/data`, which is §13.4's own example). The
first is visible only in `zfs list` — `rpool/ROOT` is `canmount=off` and in no mount table — and
the second only in the mount table.

## Decision

1. A `RecoveryCandidate` may name the objects whose snapshot does not reach what it protects
   (`outside_of`, read back as `not_protecting`). The ZFS provider fills it for every candidate it offers, from both
   relations: the datasets that are name ancestors of the target's dataset, and the mounted ZFS
   datasets whose mount point contains the target dataset's mount point. The target's own dataset
   and any dataset the candidate itself captures are never named.
2. The coverage row (`DomainCoverage`, `ono.protection-coverage/1`) carries the union over the
   actions it rests on as `not_protected_by`, less every object a chosen candidate covers. The
   field is optional in the schema and reads back as empty from a plan stored before it existed.
   It is not part of the seal digest, like the row's exclusions: it explains the level and does
   not change it.
3. The protection block and the coverage matrix print §13.4's `NOT PROTECTED BY` after the rows,
   one line per object — `a snapshot of <dataset>` — naming the row's domain. `get plan` shows it
   through the plan view.

## Consequences

A plan over a file in a child dataset says what the parent's snapshot does not do, in the plan
view and in the record a script reads. The tests: `crates/ono-recovery-zfs/tests/discovery.rs`
(the candidate names the name ancestor and the path ancestor, and never itself), the coverage
test in `crates/ono-change-protection/tests/coverage.rs` (the row carries it, less what is
captured), `crates/ono-change-core/tests/value_records.rs` (the round trip, and the old record
without the field), `crates/ono-change-render/tests/protection.rs` (the block). Case 289 proves it
end to end where a pool can be created, and the live ZFS suite does on a loop device.

Btrfs's nested subvolumes (§14.3) are stated as exclusions of the parent's snapshot already; the
reverse list for a nested subvolume is not filled by this build's Btrfs provider.

## Alternatives considered

Deriving the list in the shell from the mount table. Rejected: an unmounted ancestor is absent from
it, and dataset identity is read from the provider's resolution, never from paths (Appendix B.8).
Putting the list on `PersistenceDomain`. Rejected: the resolution in the coverage path is the
shell's generic one unless a provider resolves, while every ZFS candidate passes through the
provider that has `zfs list` in hand.
