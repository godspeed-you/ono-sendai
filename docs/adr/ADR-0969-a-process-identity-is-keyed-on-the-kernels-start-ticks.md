# ADR-0969: A process identity is keyed on the kernel's start ticks

- Status: accepted
- Date: 2026-11-09
- Spec refs: v0.4 §10.2, §42.1, §42.2; v0.5 §25.4, §25.5; ADR-0713
- Decided by: agent (autonomous)

## Context

Half of a process's spatial identity was its `started` instant, and the process provider
computes `started` as `btime + starttime / CLK_TCK`: `/proc/stat`'s boot second, a wall-clock
value the kernel recomputes when the clock is stepped, plus an offset. An NTP correction or a
manual step changes `btime`, so the same running process observed before and after the step
yielded two `started` values and therefore two `SpatialId`s. v0.5 §25.4 asks that a clock jump
not disturb what the ledger knows; this disturbed identity itself — a reconstruction across the
step saw one process end and another begin, with no evidence that anything happened (#152).

ADR-0713 gave the identity the kernel's boot id so that two boots are told apart without the
clock. The same thinking applies within a boot: the kernel's own start time for a process is
`starttime`, field 22 of `/proc/<pid>/stat`, a tick count measured from boot that nothing moves.
`started` is that count turned into an instant through `btime`; it is the derived value, and the
derivation is what the clock disturbs.

## Decision

**`ono.process/1` and `ono.process-detail/1` carry `start_ticks`**, the kernel's start time in
clock ticks since boot, beside `started`. **The spatial identity of a process is keyed on
`start_ticks` where the record carries it**: boot identity, pid, `ticks:<start_ticks>`, pid
namespace. `started` stays what it was — the instant a person reads — and stays half of the
record's declared identity for the object pipeline (`identity: [pid, started]`), where two reads
in one session agree because the provider reads `btime` once.

A record that knows only the instant — an adapted `ps`, whose `started` is inferred from the
elapsed time — is keyed on the provider's own text for it, as before: there the only requirement
is that it be the same for the same process and different for one that merely reused the pid
(§42.2), and the canonical twin of ADR-0193 reconciles it with the kernel's record.

Both process schemas carry the field, because the listing and the detail of one process must
project to one place: the detail record is built by copying the listing's fields that the detail
schema declares, and a field the detail lacked would have given the same process two identities —
which is exactly what the first version of this change did, and what
`spatial_identity.rs::should_keep_every_relationship_parent_while_naming_one_canonical_parent`
caught.

## Consequences

Easy: a process keeps its identity across a clock step, within a session and across sessions of
the same boot; the temporal ledger's reconstruction across the step sees one process.

Hard: every process `SpatialId` in the tree changes once — the identity component that was an
instant is now a tick count — so a pin stored before this change re-binds through its selector
(§20.4) rather than by id, which is what the selector is for. Two sessions reading the same
process with different `CLK_TCK` cannot happen on one kernel.

Encoded by `crates/ono-spatial-core/tests/projection.rs::should_keep_one_identity_for_a_process_when_the_wall_clock_is_stepped`
and `crates/ono-provider-linux/tests/process.rs::should_carry_the_kernels_start_ticks_beside_the_instant`.

## Alternatives considered

**Make `started` itself stable** by freezing `btime` — the provider already reads it once per
session, which is why a step inside one session did not split the identity; across sessions the
frozen value is whatever `btime` said when each started, and the split is the same.

**Change the record's declared identity to `[pid, start_ticks]`.** `started` is what a person
reads and types and what `inspect` shows as the identity; the spatial id is where the clock did
harm, and that is where the change belongs.

**Key the identity on the boot id alone** — it separates boots, not two processes that reused a
pid within one.
