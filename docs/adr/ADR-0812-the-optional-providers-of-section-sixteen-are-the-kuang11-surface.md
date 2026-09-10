# ADR-0812: §16's optional providers are the KUANG/11 surface, and the vocabulary is ready for them

- Status: accepted — corrected by ADR-0833
- Date: 2026-09-09
- Spec refs: v0.6 §0.4, §16, §48.2, §48.5, §63, Appendix D.10, Appendix G
- Decided by: agent (autonomous)

## Context

§0.4 lists eight mechanisms Ono should treat as first-class recovery assets: ZFS snapshots, Btrfs
subvolume snapshots, LVM snapshots, file and configuration archives, package-manager recovery
metadata, VM or container snapshots, database checkpoints through KUANG/11, and application-specific
backup or quiesce mechanisms.

§16 then says what each one owes, and the modal verbs are not the same:

- **§16.1, LVM**: "LVM snapshot support **MAY** be provided when the target can be mapped to an LV
  and operational constraints are understood."
- **§16.2, virtual machines**: "A VM provider **MAY** contribute snapshots, but MUST distinguish
  [four kinds]."
- **§16.3, containers**: "Container checkpoint/restart semantics vary by runtime and kernel
  capability. Ono MUST NOT generalize them into guaranteed process rollback."
- **§16.4, databases and applications**: "KUANG/11 providers **MAY** contribute
  application-consistent recovery."

§63's release definition, which is what "done" means, names ZFS and Btrfs and nothing else:
criteria 8 and 9 require their real acceptance tests, and no criterion mentions LVM, a VM, a
container or a database.

## Decision

**v0.6 ships three first-party recovery providers — ZFS (§13), Btrfs (§14) and files (§15) — and
treats §16's four as the KUANG/11 extension surface (§48.2's `RecoveryProvider` contribution).**

What that means concretely, and it is more than "we did not build them":

1. **The vocabulary carries them.** `RecoveryAssetType` has `LvmSnapshot`, `VmSnapshot`,
   `ContainerCheckpoint`, `DatabaseCheckpoint`, `TransactionSavepoint`, `TimedReversion` and
   `PackageRecoveryMetadata`, each with its `shares_failure_domain` answer, and each is registered
   in `docs/contracts/recovery/assets.yaml`. A provider written outside this repository does not
   have to extend a closed enum to be describable.
2. **The consistency rules that constrain them are enforced.** §16.2's four VM snapshot kinds are
   distinguished by `ConsistencyClass` plus the memory-inclusion fact a provider states; §16.3's
   prohibition on generalising container checkpoints into guaranteed process rollback is
   `ConsistencyClass::CrashConsistent` and the ownership rule of ADR-0809, which forbids a
   non-application provider claiming `application-consistent`; §16.4's quiesce protocol is §39.3's
   five steps, with `recovery.quiesce_failed` and `recovery.resume_failed` as separate refusals.
3. **Appendix D.10's LVM rule is implementable rather than implemented.** "An LVM provider MUST
   monitor or validate that a snapshot has not become unusable due to capacity exhaustion. An
   invalid snapshot is `RecoveryAsset.state = INVALID`" — `AssetState::Invalid` exists, the
   coverage algorithm treats an invalid asset as covering nothing, and there is a test for it. The
   MUST binds a provider that exists, and none does.
4. **Appendix G's conformance suite is the contract they are held to.** Eleven required fixtures,
   eight truth tests, the destructive-test environment rule and the
   `ono.recovery-provider/1` conformance version, all in
   `docs/contracts/recovery/providers.yaml` and checked by `xtask/src/change.rs`.

**Package recovery is the one §16 case that is partly first-party.** §30 asks package plans to
propose filesystem protection on snapshot-capable roots, which the ZFS and Btrfs providers already
do, and §30.4's verification — installed version, package-manager consistency, affected service
state — is declared in `docs/contracts/change/actions.yaml`'s `plannable_operations`. What is not
first-party is a `PackageRecoveryMetadata` asset that could restore a version without a filesystem
snapshot; §30.1 makes that a `MAY` for an adapter.

## Consequences

A machine with LVM and no ZFS or Btrfs gets file-and-configuration protection and an honest
`UNPROTECTED` for anything larger, rather than a claim nobody validated. That is the right failure:
§62.1's snapshot theatre is worse than an absence.

The KUANG/11 work of §48 is therefore load-bearing rather than decorative — it is how the remaining
half of §0.4's list arrives — and §48.4's no-escalation rule is what makes accepting one safe.

If a first-party LVM provider is wanted later, nothing in this decision has to be undone: the type,
the registry row, the conformance suite and Appendix D.10's rule are already there, and the work is
a crate beside `ono-recovery-zfs`.

## Alternatives considered

**Ship an LVM provider.** Rejected on evidence rather than on effort: Appendix D.10's central
requirement is to notice that a snapshot has silently become invalid through capacity exhaustion,
and testing that honestly needs a device-mapper setup and a way to fill it. Without that test the
provider would be exactly the thing §62.1 names — protection that looks available and is not.

**Ship stubs that answer `Unsupported`.** Rejected: a registered provider that always refuses is
noise in `get recovery` and in every coverage analysis, and `ProviderAvailability::Unavailable`
already says the same thing for a provider that is present and cannot run.
