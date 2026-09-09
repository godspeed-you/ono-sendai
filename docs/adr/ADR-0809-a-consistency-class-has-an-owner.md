# ADR-0809: A consistency class has an owner, and a storage provider does not own the application's

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.6 §11.3, §16.4, §18.4, §39.1, §39.2, §39.3, Appendix D.7
- Decided by: agent (autonomous)

## Context

§39.1: "Filesystem consistency is not application consistency. This distinction MUST be visible
everywhere." §39.2 gives the case: a filesystem snapshot containing PostgreSQL files may be
crash-consistent and recoverable through PostgreSQL's own WAL semantics, and Ono MUST NOT
independently label it `APPLICATION_CONSISTENT` unless a PostgreSQL-aware provider asserts that
guarantee.

"MUST NOT independently label" is a rule about who may say a thing, and a vocabulary of six strings
carries no such rule. A ZFS provider that snapshots a dataset containing a database has every local
reason to feel it captured the database, and nothing in a plain enum stops it saying so.

Appendix D.7 is the same problem across mechanisms rather than across layers: when a plan spans
several Btrfs subvolumes, the members are snapshotted sequentially, and "Ono MUST not invent
cross-subvolume atomicity."

## Decision

**Every consistency class declares an owner**, in `docs/contracts/recovery/consistency.yaml`:

| class | owner |
|---|---|
| `transaction-consistent` | transaction-provider |
| `application-consistent` | application-provider |
| `filesystem-consistent` | storage-provider |
| `crash-consistent` | storage-provider |
| `byte-consistent` | file-provider |
| `unknown` | nobody |

`xtask/src/change.rs::check_consistency` requires each class to name one, and the first-party
providers declare only classes they own. A ZFS or Btrfs snapshot of a running database is
`crash-consistent`, which is the honest class and the one §39.2 names.

**Composition is `weakest_of` with no counterpart**, and the registry declares
`lattice.strengthening_operation: null` — checked, not assumed. Appendix D.7 falls out of it: a
`RecoveryAssetSet` spanning two subvolumes snapshotted one after the other is as consistent as its
weakest member, and there is no operation that could produce anything stronger.

**A candidate of `unknown` consistency satisfies no objective.** It is not treated as a weak yes;
it makes the domain's recovery properties `UNKNOWN` rather than `UNPROTECTED`, because §55.6 case
29 asks for exactly that distinction — "unknown provider recovery semantics remain UNKNOWN" is a
different answer from "nothing covers this".

**§39.3's quiesce protocol is five steps and §18.4's resume failure is its own error.**
`recovery.quiesce_failed` means the application could not be paused and nothing was mutated;
`recovery.resume_failed` means the application is still paused. §18.4 calls the second "a critical
error [that] must be surfaced separately", and separately is only possible with a separate code.

## Consequences

A KUANG/11 application provider is the only thing that can raise a snapshot's claim to
`application-consistent`, and it does so by participating in the quiesce protocol rather than by
declaring a string. §16.4 and §48.5 both describe that shape.

An operator reading a ZFS snapshot's consistency sees `crash-consistent` beside a database, which
is less reassuring than the alternative and is the point. §39.1 asks for the distinction to be
visible everywhere; this is what visible looks like.

The owner column is checked at the registry rather than at run time, so a provider that starts
claiming a class it does not own fails the gate rather than a test somebody thought to write.

## Alternatives considered

**A `claimed_by` field on the asset, checked at run time.** Rejected: it moves the check to the
moment the claim is made, which is after the plan showed it to the operator.

**Letting a storage provider claim `application-consistent` when a plugin vouches for the
application.** Rejected as an unnecessary indirection: the plugin is the provider in that case, and
§16.4 already gives it the shape to be one.
