# ONO-SENDAI Specification v0.6

## Prospective Change, Protection & Recovery Interface

**Status:** Product and architecture extension specification  
**Scope:** Change planning, impact analysis, protection, execution, verification, recovery and transaction semantics  
**Relationship:** Standalone extension to the published Ono-Sendai baseline and the v0.3-v0.5 extension specifications  
**Normative language:** MUST, MUST NOT, SHOULD, SHOULD NOT, MAY

> **The future should become visible before it becomes real - and where possible, Ono should build the way back before taking the first step.**

---

# 0. Document Status and Relationship to Earlier Specifications

## 0.1 Standalone extension

This document is an independent specification for Ono-Sendai v0.6. It does not replace, rewrite, merge, or retrospectively modify the earlier specifications.

The intended progression is:

```text
v0.2  Structure
      Native system concepts remain typed objects.

v0.3  Interoperability
      Selected external Unix tools can participate in the same typed world.

v0.4  Space
      System objects and relationships become a navigable topology.

v0.5  Time and causality
      The topology gains evidence-backed history and causal explanation.

v0.6  Intent, protection and recovery
      Proposed changes become first-class objects that can be inspected,
      protected, applied, verified and, where evidence supports it, recovered.
```

Earlier specifications remain authoritative for the concepts they define. When v0.6 introduces additional behavior around an earlier object or command, the v0.6 behavior applies only to the prospective change lifecycle described here.

## 0.2 No retrospective editing

The original v0.2 specification and earlier published extension documents MUST NOT be altered merely to make this specification easier to implement.

If implementation reveals an ambiguity between specifications, it MUST be resolved through an ADR. The ADR MUST preserve the intent hierarchy rather than rewriting historical input.

## 0.3 Product intent

The purpose of v0.6 is not to turn Ono into another deployment framework, configuration-management system, orchestration language, or generic workflow engine.

The purpose is narrower and more fundamental:

> **A systems interface should let an operator inspect the consequences and recoverability of a change before making that change real.**

A shell is unusually close to mutations. It is where operators delete files, restart services, change routes, kill processes, edit configuration, install packages and invoke remote actions. Traditional shells treat the command line as the point of commitment. Ono MUST create an intermediate semantic layer between intent and irreversible effect.

That layer is the `ChangePlan`.

## 0.4 Why protection is central, not optional polish

A preview without a recovery story improves understanding but not necessarily confidence.

For many important operations, the underlying platform can create cheap recovery points before mutation:

- ZFS snapshots;
- Btrfs subvolume snapshots;
- LVM snapshots where appropriate;
- file/configuration archives;
- package-manager recovery metadata;
- VM or container snapshots where providers support them;
- database-native checkpoints or transactions through KUANG/11 providers;
- application-specific backup or quiesce mechanisms.

Ono MUST treat such mechanisms as first-class `RecoveryAsset` objects. The shell MUST make their scope, strength, limitations and lifetime visible.

A snapshot is never displayed merely as "rollback available". Ono MUST answer:

```text
What exactly is protected?
By which mechanism?
At what consistency level?
Which side effects are not protected?
Does recovery destroy newer state?
Does recovery require downtime or reboot?
How long will the recovery point remain available?
```

The user must be able to trust the answer.

---

# 1. Product Thesis

## 1.1 Proposed state is a real state domain

Ono already has concepts for present and historical state. v0.6 adds a third semantic domain: **proposed state**.

```text
past                     present                    proposed
 evidence                   |                         |
    |                       now                     plan
    |                        |                         |
    +---------------- system objects -----------------+
```

Proposed state is not a prediction. It is a projection derived from declared action semantics and the current known system graph.

Ono MUST never present proposed state as guaranteed future state unless a provider can prove the guarantee.

## 1.2 Plan is not dry-run

A conventional dry-run usually means one of:

- print commands without executing them;
- invoke a tool-specific `--dry-run` mode;
- perform incomplete validation;
- simulate part of an operation.

A `ChangePlan` is stronger. It is a typed object containing:

- resolved targets;
- ordered and dependent actions;
- preconditions;
- intended effects;
- known possible effects;
- unknown effects;
- risk annotations;
- protection opportunities;
- recovery assets and recovery methods;
- verification criteria;
- provider provenance;
- approval requirements;
- execution strategy;
- plan revision and integrity identity.

The plan is an inspectable contract between operator intent and execution.

## 1.3 The v0.6 truth rule

The previous design generations establish a consistent rule:

```text
v0.3  Do not invent structure.
v0.4  Do not invent topology.
v0.5  Do not invent history or causality.
v0.6  Do not invent the future or recoverability.
```

This rule is normative.

If Ono cannot establish an effect, it MUST classify it as unknown rather than likely.

If Ono cannot prove recovery coverage, it MUST NOT display the change as protected.

If a rollback would destroy unrelated newer state, Ono MUST surface that fact before recovery is approved.

## 1.4 Emotional thesis

The desired emotional effect is not "automation".

It is **confidence through visibility**.

The operator should feel:

> I can see what I am about to touch.  
> I can see what is connected to it.  
> I can see what Ono knows and does not know.  
> I can see whether a way back exists.  
> I can verify what actually happened afterwards.

The system should encourage deliberate exploration without encouraging recklessness.

---

# 2. Core Invariants

The following invariants are non-negotiable.

1. **Planning is side-effect free.** Creating or inspecting a plan MUST NOT mutate the target system.
2. **Protection is explicit in the plan.** Recovery preparation steps MUST be visible before execution.
3. **Protection happens before mutation.** If a required recovery asset cannot be created, mutation MUST NOT begin.
4. **Unknown is preserved.** Unknown impact or recovery behavior MUST NOT be silently promoted to expected behavior.
5. **Historical context remains read-only.** A change plan that will be applied MUST be resolved against present state.
6. **Targets are frozen at plan sealing.** Newly matching objects MUST NOT silently join a bulk plan at apply time.
7. **Preconditions are revalidated at apply time.** Material drift MUST stop execution unless the plan explicitly permits it.
8. **Provider claims are scoped.** A filesystem snapshot protects filesystem state, not process, network or remote state.
9. **Snapshot is not synonymous with backup.** Local CoW snapshots MUST be described as recovery points, not disaster-recovery backups.
10. **Rollback is not a universal verb.** Ono uses `recover` for cross-domain recovery and reserves `rollback` for provider semantics that truly implement rollback.
11. **Cross-provider changes are not atomic by default.** Ono MUST NOT imply distributed transaction guarantees it does not possess.
12. **Recovery itself is a change.** Recovery MUST be planned, impact-checked and verified.
13. **Irreversible effects remain visible after protection.** A protected filesystem does not hide an irreversible network call or process signal.
14. **Verification is separate from execution success.** A command returning success does not prove the intended system state exists.
15. **Cleanup never outruns recovery policy.** Recovery assets required by a retained plan MUST NOT be deleted silently.
16. **All lifecycle transitions are auditable.** v0.5 temporal history SHOULD receive plan, action, protection, verification and recovery events.
17. **No hidden shell scripts.** Provider operations MUST be structured execution plans, not interpolated shell command strings.
18. **The safe path must remain usable.** Protection MUST not require a ritual of obscure flags for normal interactive use.

---

# 3. Conceptual Model

## 3.1 Intent

`Intent` is the operator's requested change at the highest semantic level.

Examples:

```text
restart service nginx
replace file /etc/nginx/nginx.conf from ./nginx.conf
remove user alice
update package openssl
change route 10.40.0.0/16 via 10.0.0.1
```

Intent is not executable by itself. It must be resolved into a plan.

## 3.2 ChangePlan

A `ChangePlan` is a versioned immutable description of one proposed change after it has been sealed.

A plan contains a directed acyclic action graph unless a provider-specific local transaction encapsulates internal cycles.

A plan MUST have a stable `PlanId` and monotonically increasing revision.

## 3.3 PlanAction

A `PlanAction` is one semantically meaningful operation in a plan.

Action roles are:

```text
PREPARE       create protection, acquire leases, validate prerequisites
MUTATE        change target system state
VERIFY        establish whether expected state exists
RECOVER       restore or compensate after failure
CLEANUP       remove temporary resources after retention policy permits
```

An action MAY occupy more than one provider-specific internal step, but its public semantics must be stable.

## 3.4 ProposedEffect

A `ProposedEffect` describes a change Ono expects may result from an action.

Effect confidence classes are fixed:

```text
GUARANTEED
EXPECTED
POSSIBLE
UNKNOWN
```

Their exact meaning is defined in section 8.

## 3.5 Impact

`Impact` is the graph of objects and relationships that may be touched, changed, invalidated or indirectly affected by a plan.

Impact is derived from:

- action target semantics;
- v0.4 spatial relationships;
- provider-declared dependencies;
- v0.5 historical evidence where useful;
- adapter semantics from v0.3;
- KUANG/11 contributions.

Impact MUST retain provenance.

## 3.6 RecoveryAsset

A `RecoveryAsset` is a concrete resource that can contribute to restoring some state after mutation.

Examples:

```text
ZFS snapshot
Btrfs read-only subvolume snapshot
LVM snapshot
file archive
configuration backup
VM snapshot
database checkpoint
provider-local transaction savepoint
```

A recovery asset MUST declare what it protects and what it does not protect.

## 3.7 RecoveryProvider

A `RecoveryProvider` discovers, creates, validates, restores and cleans up recovery assets for a specific mechanism.

Examples:

```text
ono.recovery.zfs
ono.recovery.btrfs
ono.recovery.file-copy
kuang.postgresql.recovery
```

## 3.8 RecoveryPlan

A `RecoveryPlan` is a specialized `ChangePlan` produced to recover from a prior plan or selected recovery asset.

Recovery is never treated as a magical inverse.

A RecoveryPlan includes:

- the state or asset to recover toward;
- destructive consequences of recovery;
- newer state that would be lost;
- services that must stop or restart;
- reboot/offline requirements;
- verification criteria;
- uncompensated effects.

## 3.9 VerificationContract

A `VerificationContract` defines observable conditions that determine whether a plan achieved its intended outcome.

Execution success and verification success are independent.

## 3.10 ChangeSet

A `ChangeSet` is a collection of related plans or actions executed under one orchestration policy.

A ChangeSet is **not** automatically a transaction.

## 3.11 Transaction

The word `transaction` is reserved for a provider that can state concrete atomicity and rollback guarantees for its own domain.

Ono MUST NOT label a multi-provider ChangeSet transactional merely because it has compensating actions.

---

# 4. Plan Lifecycle State Machine

## 4.1 States

The canonical plan states are:

```text
DRAFT
  |
  v
RESOLVED
  |
  v
SEALED
  |
  +------> EXPIRED
  |
  v
PREPARING
  |
  +------> PREPARE_FAILED
  |
  v
PROTECTED          (when protection applies)
  |
  v
APPLYING
  |
  +------> APPLY_FAILED
  |
  v
VERIFYING
  |
  +------> DEGRADED
  +------> FAILED
  |
  v
VERIFIED
  |
  v
CLOSED
```

Recovery introduces a related branch:

```text
FAILED / DEGRADED / VERIFIED
          |
          v
   RECOVERY_PLANNED
          |
          v
    RECOVERING
          |
     +----+----+
     |         |
     v         v
 RECOVERED  RECOVERY_FAILED
     |
     v
 RECOVERY_VERIFIED
```

## 4.2 DRAFT

A draft may be edited, expanded, have actions added or removed, and have strategy or policy changed.

A draft MUST NOT be executable.

## 4.3 RESOLVED

Resolution turns selectors into concrete object identities and provider bindings.

For a bulk selector:

```text
get service | where state == failed | plan restart service
```

resolution freezes the current matching set.

If four services match at resolution time and a fifth fails before execution, the fifth service MUST NOT be added silently.

## 4.4 SEALED

A sealed plan is immutable.

Any user or provider change that would alter semantics creates a new revision.

The seal includes a canonical digest over:

- plan revision;
- target identities;
- action graph;
- provider identities and relevant versions;
- preconditions;
- protection policy;
- verification contracts;
- accepted risk overrides.

## 4.5 PREPARING

Preparation occurs immediately before the first mutating action.

It includes, where applicable:

- drift revalidation;
- privilege/capability validation;
- recovery asset creation;
- provider locks/leases;
- application quiesce;
- temporary checkpoints;
- final verification that required recovery assets are usable.

If a required prepare action fails, Ono MUST abort before mutation.

## 4.6 PROTECTED

A plan is `PROTECTED` only if all protection actions required by its policy have completed and the resulting assets have been validated.

The word MUST NOT be used as a marketing label for partial protection.

## 4.7 APPLYING

Mutating actions execute according to the plan dependency graph and strategy.

Every action result MUST be recorded independently.

## 4.8 VERIFYING

Verification starts after mutations complete, unless a plan includes intermediate verification gates.

Verification MAY cause the plan to become:

- `VERIFIED`;
- `DEGRADED` when the intended primary state exists but some expectations are violated or unknown;
- `FAILED` when required postconditions fail.

## 4.9 CLOSED

Closing a plan does not necessarily remove recovery assets.

Asset retention is independent and policy-driven.

---

# 5. Canonical Commands

v0.6 introduces the following public command families.

```text
plan <mutation>
plan { ... }

get plan [selector]
inspect plan <plan>
impact <plan>
protect <plan>
apply <plan>
verify <plan>
recover <plan|recovery-asset>

get recovery [selector]
inspect recovery <asset>
remove recovery <asset>

map --plan <plan>
timeline --plan <plan>
explain plan <plan>
```

The exact implementation MAY expose result references such as `@plan`, but the semantic commands above are normative.

## 5.1 Single-action planning

```text
plan restart service nginx
```

returns `ChangePlan` and does not restart nginx.

## 5.2 Multi-action planning

```text
plan {
    replace file /etc/nginx/nginx.conf from ./nginx.conf
    validate config nginx
    restart service nginx
    verify service nginx state == running
    verify socket :443 exists
}
```

The block is declarative in the sense that it describes actions. It is not a general-purpose workflow language.

The v0.6 plan block MUST NOT introduce loops, arbitrary functions, background jobs or unbounded runtime control flow.

## 5.3 Pipeline-generated plans

```text
get service
    | where state == failed
    | plan restart service
```

MUST produce one plan with frozen resolved targets, unless the user explicitly requests one plan per input object.

## 5.4 `impact`

```text
impact @plan
```

returns `ImpactGraph`.

## 5.5 `protect`

`protect @plan` materializes the protection portion of a plan before apply.

This is optional for the normal flow because `apply` performs required PREPARE actions just in time. It exists for operators who want to establish a recovery point earlier and inspect it before the change window.

Protection is a real mutation of the storage/control plane. It MUST be visible in history.

## 5.6 `apply`

`apply @plan` is the canonical commitment point.

It MUST refuse drafts and expired plans.

It MUST revalidate sealed plans before mutation.

## 5.7 `verify`

`verify @plan` MAY be run automatically after apply and manually later while required evidence remains available.

## 5.8 `recover`

`recover @plan` MUST **plan recovery**, not immediately perform it.

The result is a `RecoveryPlan`.

The operator then inspects and applies it:

```text
recovery = recover @plan
impact @recovery
apply @recovery
verify @recovery
```

This additional step is intentional because recovery may destroy newer state.

---

# 6. Plan Grammar and Command Eligibility

## 6.1 Plannable operations

An operation is plannable only if Ono can resolve it to a provider contract that declares:

- target schema;
- mutation semantics;
- required capabilities;
- preconditions;
- expected direct effects;
- execution method;
- idempotency class;
- recovery semantics or explicit lack thereof;
- verification options.

## 6.2 Opaque external commands

Arbitrary external commands are not automatically plannable.

```text
plan sh -c 'rm -rf /somewhere'
```

MUST fail by default because Ono cannot reason about target scope or side effects.

An external-command adapter from v0.3 MAY expose a plannable action if its contract is explicit.

## 6.3 Explicit opaque escape

For advanced use, v0.6 MAY support an `opaque action` plan entry, but it MUST require an explicit risk acknowledgement and MUST classify impact and reversibility as unknown unless separately protected.

Opaque actions MUST NOT receive a `PROTECTED` status merely because a filesystem snapshot exists somewhere on the host.

## 6.4 Historical contexts

A plan intended for application MUST be resolved against present state.

In a v0.5 historical context:

```text
local:// @12:17 [PAST] > plan restart service nginx
```

MUST fail with a structured error explaining that historical state can be inspected but not used as the executable target base.

A future separate counterfactual simulation feature MAY reason from historical state, but it is outside v0.6.

---

# 7. Target Resolution and Drift

## 7.1 Freeze targets

A plan stores stable object identities, not unresolved selectors.

For files, identity includes canonical path plus containing persistence domain and relevant inode/generation information where available.

For services, identity includes provider namespace and unit identity.

For remote targets, host/link identity is part of the target.

## 7.2 Preconditions

Each action MUST declare preconditions sufficient to detect material drift.

Examples:

```text
file hash still equals X
service generation/state still equals Y
package installed version still equals Z
route object still has identity R
filesystem mount still resolves to dataset D
recovery provider still available
```

## 7.3 Revalidation

Immediately before PREPARE, Ono re-resolves all targets and preconditions.

If material drift exists, default behavior is:

```text
plan.drift_detected
```

and no mutation occurs.

## 7.4 Non-material drift

Providers MAY define fields that do not invalidate a plan, such as changing CPU usage while planning a service restart.

Such tolerance MUST be contract-declared, not guessed.

## 7.5 Rebase

`rebase plan @plan` MAY create a new plan revision against current state.

Rebase MUST NOT mutate the sealed original.

---

# 8. Proposed Effects and Future Honesty

## 8.1 Effect classes

### GUARANTEED

The effect follows directly from the successfully completed operation and provider contract.

Example:

```text
successful `zfs snapshot pool/data@x`
=> snapshot object exists
```

A guarantee MUST be scoped to the provider's observable domain.

### EXPECTED

The provider has strong semantics for the effect, but external system behavior can still intervene.

Example:

```text
restart systemd service
=> expected new active process set
```

### POSSIBLE

The object is in the known impact graph or provider documents a possible effect, but Ono cannot assert that it will occur.

Example:

```text
active client connections may be interrupted by service restart
```

### UNKNOWN

Ono lacks a justified model.

Unknown MUST remain visible.

## 8.2 Effect schema

```text
ProposedEffect {
    id: EffectId
    action_id: ActionId
    object: ObjectRef?
    domain: EffectDomain
    kind: EffectKind
    confidence: GUARANTEED | EXPECTED | POSSIBLE | UNKNOWN
    before: Value?
    proposed: Value?
    evidence: List<ContractRef>
    explanation: String
}
```

## 8.3 No probability theatre

v0.6 MUST NOT invent percentages such as `82% likely` unless a provider supplies a documented statistical model with provenance.

Qualitative confidence classes are preferred.

---

# 9. Impact Analysis

## 9.1 Purpose

Impact answers:

> What known parts of the system could this plan touch directly or indirectly?

It is not synonymous with failure prediction.

## 9.2 Impact classes

```text
DIRECT_TARGET
DIRECT_EFFECT
DEPENDENT
TRANSITIVE_RELATED
EXTERNAL_SIDE_EFFECT
UNKNOWN_BOUNDARY
```

## 9.3 Spatial integration

v0.4 relationships are the primary topology source.

Example:

```text
nginx.conf
   |
   +-- read-by --> nginx process
                     |
                     +-- owns --> socket :80
                     +-- owns --> socket :443
```

A file replacement plan can therefore show the service and listeners as related impact without claiming they will fail.

## 9.4 Temporal integration

v0.5 evidence MAY strengthen impact relevance.

If the ledger has repeatedly observed that an action changes specific relationships, Ono MAY show that history as evidence, but MUST NOT upgrade correlation to causation unless a causal rule supports it.

## 9.5 Blast radius

`impact` MAY summarize counts, but MUST preserve object access.

```text
blast radius
  1 direct target
  2 direct dependents
  17 known transitive relations
  3 external boundaries
```

## 9.6 Unknown boundary

When the graph ends at an opaque boundary, that boundary MUST be visible.

Example:

```text
nginx -> outbound HTTPS request -> external API
                                  ? beyond Ono visibility
```

---

# 10. Protection Model

## 10.1 Protection is coverage, not a boolean

A plan MUST NOT use a single `reversible: true/false` flag.

Protection is evaluated per effect domain and target scope.

## 10.2 Protection levels

Canonical plan-level protection status:

```text
UNPROTECTED
COMPENSATABLE
PARTIALLY_PROTECTED
PROTECTED
TRANSACTIONAL
UNKNOWN
```

### UNPROTECTED

No usable recovery or compensation mechanism covers required persistent effects.

### COMPENSATABLE

No prior state image exists, but declared inverse/compensating actions can restore an acceptable semantic state.

### PARTIALLY_PROTECTED

Some important effects are covered, others are not.

### PROTECTED

All known persistent state mutations required by the plan are covered by validated recovery assets or provider restore contracts. Runtime and external effects MAY still be explicitly excluded.

### TRANSACTIONAL

The relevant provider guarantees atomic commit/rollback for the entire protected scope.

This status MUST only be used within that provider's transaction boundary.

### UNKNOWN

Ono cannot establish recovery properties.

## 10.3 Effect-domain coverage

A plan may be:

```text
filesystem state       PROTECTED
service configuration  PROTECTED
process runtime         UNPROTECTED
network sessions        UNPROTECTED
remote API side effect  UNPROTECTED
```

The plan-level summary must never hide this matrix.

## 10.4 Protected does not mean safe

A plan can be strongly protected and still have high operational risk.

Example: restoring a root filesystem snapshot may require reboot and discard later changes.

Risk and recoverability are separate axes.

---

# 11. RecoveryAsset Model

## 11.1 Canonical schema

```text
RecoveryAsset {
    id: RecoveryAssetId
    provider: ProviderId
    type: RecoveryAssetType
    host: HostRef
    scope: RecoveryScope
    created_at: Timestamp
    source_plan: PlanId?
    state: PROPOSED | CREATING | READY | INVALID | EXPIRED | REMOVED | FAILED
    consistency: ConsistencyClass
    restore_method: RestoreMethod
    validation: RecoveryValidation
    retention: RetentionPolicy
    estimated_cost: RecoveryCost
    dependencies: List<RecoveryAssetId>
    exclusions: List<RecoveryExclusion>
    provenance: Provenance
}
```

## 11.2 Recovery scope

Scope MUST be concrete.

Examples:

```text
ZFS dataset rpool/ROOT/debian
Btrfs subvolume id 256 mounted at /
file /etc/nginx/nginx.conf
VM id vm-42
PostgreSQL database appdb
```

A path MUST be mapped to its containing persistence domain before protection is claimed.

## 11.3 Consistency classes

Canonical consistency classes:

```text
BYTE_CONSISTENT
FILESYSTEM_CONSISTENT
CRASH_CONSISTENT
APPLICATION_CONSISTENT
TRANSACTION_CONSISTENT
UNKNOWN
```

### BYTE_CONSISTENT

A provider can restore captured bytes/files but does not claim filesystem- or application-wide consistency.

### FILESYSTEM_CONSISTENT

The storage mechanism captures a filesystem/subvolume/dataset point-in-time state according to its own atomicity semantics.

### CRASH_CONSISTENT

The resulting state is equivalent to storage observed after abrupt interruption, suitable only for applications able to recover from such a state.

### APPLICATION_CONSISTENT

The application/provider participated in quiesce/checkpoint semantics and declares the snapshot recoverable as an application state.

### TRANSACTION_CONSISTENT

A transaction-capable provider supplies its own atomic consistency guarantee.

## 11.4 Validation

Creating an asset is not sufficient.

The provider MUST validate at least:

- asset exists;
- identity matches planned source;
- restore path is syntactically/semantically available;
- scope matches expected target;
- required permissions are present at creation time.

Providers SHOULD perform stronger validation where inexpensive.

## 11.5 Snapshot is not backup

Ono MUST label local CoW snapshots as local recovery assets.

It MUST NOT imply protection from underlying device corruption, pool loss or destructive writes below the filesystem layer.

---

# 12. Recovery Providers

## 12.1 Provider interface

Conceptually:

```rust
trait RecoveryProvider {
    fn discover(&self, target: &ObjectRef) -> Result<Vec<RecoveryCandidate>>;
    fn plan_protection(&self, target: &ObjectRef, policy: &ProtectionPolicy)
        -> Result<Vec<ProtectionAction>>;
    fn create(&self, action: &ProtectionAction) -> Result<RecoveryAsset>;
    fn validate(&self, asset: &RecoveryAsset) -> Result<RecoveryValidation>;
    fn plan_recovery(&self, asset: &RecoveryAsset, current: &SystemState)
        -> Result<RecoveryPlanFragment>;
    fn cleanup(&self, asset: &RecoveryAsset) -> Result<ActionResult>;
}
```

The exact Rust signature may differ. The semantics are normative.

## 12.2 Capability declaration

Providers MUST declare:

```text
recovery.discover
recovery.prepare
recovery.restore
recovery.cleanup
recovery.estimate-cost
recovery.quiesce?      optional
recovery.transaction?  optional
```

## 12.3 No shell interpolation

A provider MUST use direct process APIs, libraries, DBus, ioctls or other structured interfaces.

It MUST NOT generate shell command strings from user-controlled values.

---

# 13. ZFS Recovery Provider

ZFS is a first-party v0.6 reference provider because its snapshot semantics map naturally to protected change.

## 13.1 Discovery

For every filesystem path targeted by a plan, Ono MUST resolve:

- mount;
- ZFS dataset;
- descendant dataset boundaries relevant to the target tree;
- existing snapshots relevant to rollback constraints;
- clones/bookmarks that may affect destructive rollback options;
- pool health sufficient for the proposed protection operation.

## 13.2 Snapshot semantics

A ZFS snapshot is a point-in-time read-only view of a dataset.

Snapshot creation is cheap initially due to copy-on-write behavior, but retained snapshots may cause additional space consumption as the live dataset diverges.

Ono MUST expose this lifecycle cost.

## 13.3 Recursive snapshots

When multiple descendant datasets must be protected at one logical point, Ono MAY use recursive snapshot creation when appropriate.

The resulting `RecoveryAsset` MUST record each dataset/snapshot identity individually even if they were created by one recursive operation.

Recovery MUST NOT assume that ZFS offers one magical recursive rollback operation for the whole tree. Recovery planning must reason about each relevant dataset and the consequences of rolling it back.

## 13.4 Dataset boundaries

If `/`, `/var`, `/home` and `/data` are different ZFS datasets, a snapshot of the root dataset does not automatically protect the others unless the plan explicitly includes them.

Example:

```text
TARGET
  /data/customer.db

PERSISTENCE
  dataset tank/data

RECOVERY
  snapshot tank/data@ono-plan-a82f

NOT PROTECTED BY
  rpool/ROOT/debian@ono-plan-a82f
```

## 13.5 Safer selective restore

For a plan that changes a small number of files, the provider SHOULD prefer a recovery method that minimizes unrelated rollback damage.

Possible methods include:

- restore selected file(s) from snapshot view;
- clone snapshot and copy selected state;
- full dataset rollback only when justified.

The RecoveryPlan MUST display which method will be used.

## 13.6 Full dataset rollback risks

ZFS rollback can discard all changes since the snapshot and may require destruction of newer snapshots/bookmarks or clones depending on the target snapshot and flags.

Ono MUST NEVER silently add destructive rollback flags equivalent to removing newer history.

If recovery requires destruction of newer snapshots, bookmarks or clones, the RecoveryPlan MUST enumerate them and require explicit acceptance.

## 13.7 Mounted/root datasets

If rollback requires unmount, forced unmount, reboot, boot-environment switch or offline recovery, the plan MUST say so before apply.

Ono MUST NOT promise online rollback merely because a snapshot exists.

## 13.8 ZFS protection rendering

```text
RECOVERY ASSET
  type          ZFS snapshot
  dataset       rpool/ROOT/debian
  snapshot      rpool/ROOT/debian@ono-a82f
  consistency   filesystem-consistent
  created       just-in-time before mutation
  restore       selective-file restore
  retained      24h after verification

excluded
  /home         separate dataset
  /data         separate dataset
  process state
  network sessions
```

---

# 14. Btrfs Recovery Provider

Btrfs is a first-party v0.6 reference provider, but its semantics MUST NOT be modeled as if it were ZFS.

## 14.1 Subvolume identity

The provider MUST resolve the containing Btrfs subvolume and stable subvolume ID for each protected target.

## 14.2 Snapshot is a subvolume

A Btrfs snapshot is itself a subvolume sharing extents initially through copy-on-write behavior.

For recovery assets, Ono SHOULD create read-only snapshots by default unless a provider-specific reason requires writable state.

## 14.3 Snapshots are not recursive across nested subvolumes

Nested subvolumes form snapshot boundaries.

A snapshot of a parent subvolume does not contain the live contents of nested subvolumes as ordinary recursively captured data.

Therefore Ono MUST inspect nested subvolume boundaries and create separate recovery assets where the plan requires them.

Example:

```text
/
  @root
  /var        -> @var subvolume
  /home       -> @home subvolume

plan changes:
  /etc/nginx/nginx.conf
  /var/lib/app/state.db

required protection:
  snapshot @root
  snapshot @var

not required:
  @home
```

## 14.4 Btrfs rollback is a recovery workflow

v0.6 MUST NOT present Btrfs as having a generic in-place `rollback snapshot` primitive equivalent to a database rollback.

Recovery may require:

- restoring selected files from the snapshot;
- replacing/renaming subvolumes;
- changing the default subvolume;
- remounting;
- rebooting into a restored root;
- application-specific restart/verification.

The RecoveryPlan MUST declare the actual method.

## 14.5 Read-only recovery assets

Ono SHOULD preserve recovery snapshots as read-only while retained.

If recovery requires a writable clone/subvolume derived from them, that derived object MUST be tracked separately.

## 14.6 Root rollback policy

For root filesystem recovery, Ono MUST distinguish:

```text
online selective restore
offline subvolume replacement
next-boot rollback
```

The chosen method depends on layout and provider capability. It MUST be shown before execution.

## 14.7 Snapshot is not backup

The Btrfs provider MUST explicitly state that snapshots share the same filesystem/storage failure domain unless a separate backup provider exists.

---

# 15. Generic File and Configuration Protection

Not every system uses snapshot-capable storage.

v0.6 MUST provide a first-party narrow file/config recovery provider.

## 15.1 Scope

The provider MAY protect:

- regular files;
- symlinks as symlinks;
- small directory trees within configured limits;
- ownership, mode, ACL and xattr metadata where supported.

## 15.2 Exclusions

It SHOULD NOT silently archive arbitrarily large data trees, databases, sockets, devices or pseudo-filesystems.

## 15.3 Storage location

Recovery copies MUST live in a protected Ono recovery store with permissions preventing other users from reading sensitive configuration.

## 15.4 Atomic replacement

Where possible, restoration SHOULD use temp-file + fsync + atomic rename semantics rather than truncating the live file in place.

## 15.5 Secret policy

Recovery copies may contain secrets. They MUST inherit the strictest secret/history policy and MUST NOT be rendered by default.

---

# 16. LVM, VM, Container and Application Providers

## 16.1 LVM

LVM snapshot support MAY be provided when the target can be mapped to an LV and operational constraints are understood.

The provider MUST expose capacity/overflow risk and MUST NOT claim protection if the snapshot can silently become invalid due to insufficient snapshot space.

## 16.2 Virtual machines

A VM provider MAY contribute snapshots, but MUST distinguish:

- memory-inclusive VM snapshot;
- disk-only snapshot;
- guest-quiesced snapshot;
- crash-consistent disk state.

## 16.3 Containers

Container checkpoint/restart semantics vary by runtime and kernel capability. Ono MUST NOT generalize them into guaranteed process rollback.

## 16.4 Databases and applications

KUANG/11 providers MAY contribute application-consistent recovery:

```text
quiesce
checkpoint / transaction boundary
snapshot
resume
```

The provider must own the claim that the resulting asset is application-consistent.

---

# 17. Protection Policy

## 17.1 Default policy

The default interactive policy is:

```text
protection.mode = prefer
```

`prefer` means:

- discover available low-cost protection;
- include protection actions in the plan;
- execute them automatically during PREPARE when `apply` is called;
- abort before mutation if a required planned protection action fails;
- never fabricate protection when no suitable provider exists.

## 17.2 Policy modes

```text
off
prefer
require
maximize
```

### off

Do not create automatic recovery assets. Still show available protection opportunities.

### prefer

Use protection for persistent mutations when an appropriate provider can create a bounded-cost asset without materially changing the intended operation.

### require

Refuse to apply if required mutation domains cannot reach the plan's required protection class.

### maximize

Attempt all non-conflicting available protection mechanisms that improve recovery coverage within configured cost limits.

`maximize` MUST NOT mean "snapshot everything on the host".

## 17.3 Per-plan override

```text
plan ... --protection require
```

MAY override configuration.

## 17.4 Scripts

Non-interactive scripts MUST never stop for a yes/no question.

They must use explicit policy and risk flags. If policy cannot be satisfied, the command fails with structured error.

---

# 18. Protection Preparation and Freshness

## 18.1 Just-in-time protection

For normal `apply`, recovery assets SHOULD be created immediately before mutation to minimize the unprotected time window.

## 18.2 Early `protect`

An operator MAY run:

```text
protect @plan
```

before the maintenance window.

The asset then exists earlier, but plan apply MUST assess whether it is still appropriate.

## 18.3 Freshness

Recovery assets have a `captured_state_id` or equivalent source-state fingerprint where available.

If current state drifted materially after protection, Ono MUST NOT pretend the old asset is a just-before-change recovery point.

It may:

- create a new asset;
- require explicit acceptance of stale protection;
- abort when policy requires fresh protection.

## 18.4 Quiesce windows

When application-consistent protection requires quiescing an application, PREPARE MUST bound the quiesce window and resume the application if snapshot creation fails.

Failure to resume is a critical error and must be surfaced separately.

---

# 19. Risk Model

## 19.1 Risk is separate from protection

A strongly protected plan may still be risky.

Risk dimensions include:

```text
scope
privilege
downtime
external side effects
irreversibility
unknown impact
bulk count
remote fanout
recovery complexity
reboot requirement
```

## 19.2 Risk classes

Canonical classes:

```text
LOW
MODERATE
HIGH
CRITICAL
UNKNOWN
```

These classes are rule-based, not AI-generated.

## 19.3 Examples

```text
replace one config file with ZFS snapshot
  risk        MODERATE
  protection  PROTECTED

SIGKILL database process
  risk        HIGH
  protection  UNPROTECTED

restart all 40 frontend nodes simultaneously
  risk        CRITICAL
  protection  PARTIALLY_PROTECTED
```

## 19.4 Gates

HIGH and CRITICAL plans require explicit interactive acknowledgement or non-interactive policy flag.

Plans containing known irreversible actions require an explicit `accept_irreversible` acknowledgement stored in the sealed revision.

---

# 20. User Experience and Plan Rendering

## 20.1 Default plan view

A plan must answer the operator's questions in a fixed order:

```text
1. What do you intend to change?
2. Which concrete objects will be touched?
3. What does Ono expect to happen?
4. What else may be affected?
5. What does Ono not know?
6. What protection will be created?
7. What remains unrecoverable?
8. What will Ono verify afterwards?
9. What special approval is required?
```

## 20.2 Example

```text
PLAN / a82f  rev 3

intent
  replace nginx configuration and restart service

targets
  /etc/nginx/nginx.conf
  nginx.service

planned
  1  snapshot rpool/ROOT/debian
  2  replace nginx.conf
  3  validate nginx configuration
  4  restart nginx.service
  5  verify service running
  6  verify listeners :80 and :443

impact
  direct        nginx.conf, nginx.service
  related       4 worker processes, :80, :443
  possible      14 active client connections
  unknown       application-level client retry behavior

protection   PROTECTED  [filesystem scope]
  <-> rpool/ROOT/debian@ono-a82f
  filesystem-consistent

not recoverable
  active TCP sessions
  requests already served externally

risk          MODERATE
reboot        no

PLAN NOT EXECUTED
```

## 20.3 Visual symbols

The compact visual language is fixed:

```text
+  proposed addition
-  proposed removal
~  proposed modification
?  unknown effect
!  risk / irreversible boundary
<-> recovery available
1/2 partial recovery coverage
```

Implementations MAY use better glyphs when terminal support is known, but ASCII fallback MUST exist.

## 20.4 Protection must be prominent

Protection MUST be visible near the top-level plan summary. It MUST NOT be hidden under a verbose inspector.

---

# 21. Spatial Projection of Proposed State

## 21.1 `map --plan`

v0.4 map views gain proposed-state overlay.

```text
map --plan @plan
```

## 21.2 Overlay semantics

The current world remains the base layer.

Proposed effects are overlays:

```text
CURRENT                         PROPOSED

nginx.service  [running]        nginx.service  [~ restart]
   |                               |
   +-- process/1842 [-]            +-- replacement [?]
   +-- socket :443                 +-- socket :443 [expected]
```

## 21.3 Recovery overlay

Objects covered by a recovery asset MUST show coverage.

Example:

```text
nginx.conf  ~  <-> zfs:rpool/ROOT@ono-a82f
```

## 21.4 No fake ghost world

Map rendering MUST NOT instantiate fully predicted future objects when their identity is unknown.

A service restart can show "replacement worker expected" rather than fabricating a PID.

---

# 22. Temporal Integration

## 22.1 Plans enter the evidence ledger

If v0.5 temporal history is enabled, Ono SHOULD record:

```text
PlanCreated
PlanSealed
PlanProtected
ActionStarted
ActionCompleted
VerificationObserved
PlanVerified
RecoveryPlanned
RecoveryStarted
RecoveryCompleted
RecoveryVerified
RecoveryAssetCreated
RecoveryAssetRemoved
```

## 22.2 Pre-plan checkpoint

Immediately before mutation, v0.6 SHOULD create a lightweight semantic checkpoint in the v0.5 ledger for plan targets and impact-relevant objects.

This checkpoint is independent of storage snapshots.

## 22.3 Post-plan comparison

Verification can compare pre-plan and post-plan known state.

## 22.4 Historical investigation

After a failure, the operator can use v0.5:

```text
at event @plan.apply.start
map

timeline --plan @plan
why service nginx failed
```

The plan ID is a causal anchor.

---

# 23. Verification Model

## 23.1 Verification is mandatory for planned changes

Every plan containing a MUTATE action MUST have at least one verification contract, even if the minimum verification is only provider-level state acknowledgement.

## 23.2 Verification classes

```text
REQUIRED
ADVISORY
OBSERVATIONAL
```

A REQUIRED postcondition failure makes the plan `FAILED`.

An ADVISORY failure may make it `DEGRADED`.

OBSERVATIONAL checks provide context only.

## 23.3 Verification result

```text
VerificationResult {
    plan_id
    check_id
    status: PASSED | FAILED | UNKNOWN | SKIPPED
    observed
    expected
    evidence
    timestamp
}
```

## 23.4 Example

```text
VERIFY / plan a82f

required
  service nginx running       PASS
  listener :80 exists         PASS
  listener :443 exists        PASS

advisory
  worker count == 4           PASS

observed
  postgres connection changed UNKNOWN RELATION

status
  VERIFIED
```

## 23.5 Timeout

Verification contracts MUST have explicit timeout semantics. Infinite waiting is forbidden.

---

# 24. Recovery Semantics

## 24.1 Recovery is a new plan

The command:

```text
recover @plan
```

produces a RecoveryPlan.

It does not immediately modify state.

## 24.2 Why recovery must be planned

Since the original plan, new state may exist:

- new files;
- new database writes;
- later package updates;
- newer snapshots;
- new users;
- changed network state;
- unrelated application data.

A naive rollback can destroy this state.

Therefore Ono MUST compare the recovery target with current state before recovery.

## 24.3 Recovery impact

The RecoveryPlan MUST show:

```text
state restored
newer state discarded
services stopped/restarted
snapshots/bookmarks/clones destroyed
reboot required
assets consumed
unrecoverable side effects
```

## 24.4 Example

```text
RECOVERY PLAN / r91c
source
  plan a82f
  recovery asset rpool/ROOT@ono-a82f

restore
  /etc/nginx/nginx.conf

method
  selective restore from snapshot

newer state preserved
  /etc/hosts
  /etc/ssh/sshd_config

runtime differences expected
  nginx worker PIDs
  TCP sessions

risk
  MODERATE
```

## 24.5 Full snapshot rollback example

```text
RECOVERY PLAN / r91d

method
  ZFS dataset rollback

dataset
  tank/data

will discard
  18 GiB changed blocks since snapshot
  2 newer snapshots

will destroy
  tank/data@later-1
  tank/data@later-2

requires
  explicit accept-newer-state-loss
```

No recovery execution occurs without the explicit gate.

---

# 25. Recovery Verification

## 25.1 Semantic equivalence, not naive identity

Recovery verification MUST distinguish:

```text
persistent-state equivalence
runtime-state equivalence
external-side-effect equivalence
```

A service restart may restore configuration while naturally creating new PIDs.

## 25.2 Example

```text
RECOVERY VERIFICATION

persistent state
  nginx.conf             RESTORED
  package version        RESTORED

runtime
  service state          RESTORED
  worker PIDs            DIFFERENT / EXPECTED
  TCP connections        NOT RECOVERABLE

external side effects
  1 webhook request      NOT RECOVERABLE

result
  PERSISTENT STATE VERIFIED
  FULL WORLD EQUIVALENCE NOT CLAIMED
```

## 25.3 Never say "rollback successful" globally

User-visible language MUST describe the verified scope.

---

# 26. Auto-Recovery Policy

## 26.1 Default

Automatic recovery after verification failure is OFF by default.

This is intentional.

## 26.2 Why

A failed verification does not prove that rollback is safer than leaving the new state in place.

## 26.3 Allowed auto-recovery

A plan MAY declare auto-recovery only when all of the following are true:

- recovery plan can be fully constructed before mutation;
- no known irreversible external side effects exist;
- recovery does not destroy unrelated newer state;
- protection is `PROTECTED` or `TRANSACTIONAL` for required mutation domains;
- recovery verification exists;
- user policy explicitly enables it.

Otherwise the declaration MUST be rejected at seal time.

---

# 27. Transaction Semantics

## 27.1 Provider-local transaction

A provider can expose:

```text
begin
prepare
commit
rollback
```

and declare atomicity over its own resource scope.

## 27.2 Cross-provider plans

A plan involving filesystem + systemd + remote HTTP is not atomic.

Even if each domain has recovery actions, Ono MUST call it a ChangePlan/ChangeSet, not a transaction.

## 27.3 Two-phase commit

Generic distributed two-phase commit is an explicit non-goal for v0.6.

KUANG/11 providers MAY implement domain-specific distributed transactions, but the provider owns the guarantee.

## 27.4 Compensation

Compensation attempts to restore semantics through inverse actions.

Examples:

```text
start service <-> stop service
add route <-> remove route
create user <-> remove user   (often incomplete)
```

Compensation MUST NOT be labeled rollback unless prior state is actually restored.

---

# 28. Bulk Changes

## 28.1 Bulk planning is first-class

```text
get service
    | where state == failed
    | plan restart service
```

## 28.2 Frozen membership

Membership freezes at resolution.

## 28.3 Bulk risk

Risk increases with scope and shared topology.

If all members of a service group are targeted, Ono SHOULD identify availability risk where topology proves shared role membership.

## 28.4 Strategies

v0.6 defines these execution strategies:

```text
sequential
batch N
canary N then batch N
parallel N
```

Unlimited parallel mutation is not a default strategy.

## 28.5 Strategy is part of seal

Changing strategy creates a new plan revision because it changes operational risk and temporal effects.

## 28.6 Canary verification

For canary strategy, required verification MUST pass before remaining batches continue.

---

# 29. Remote and Distributed Change

## 29.1 Per-host truth

Remote plans are decomposed into host/provider-local action fragments.

Ono MUST NOT imply global atomicity.

## 29.2 Protection per host

A 20-host plan may have:

```text
12 hosts  ZFS protected
 6 hosts  Btrfs protected
 2 hosts  unprotected
```

The plan-level status is therefore `PARTIALLY_PROTECTED` unless policy excludes the unprotected targets.

## 29.3 Link failure

If connectivity fails mid-plan, Ono MUST preserve exact per-host action state and verification state.

It MUST NOT mark unknown remote actions as failed or successful without evidence.

## 29.4 Recovery

Remote recovery is planned per host. Ono MUST show whether recovery can proceed for disconnected hosts.

---

# 30. Package Management

Package changes are a high-value v0.6 use case.

## 30.1 Adapter/provider semantics

First-party or v0.3 adapters MAY expose package mutations as plannable actions.

## 30.2 Protection

On snapshot-capable roots, package plans SHOULD propose filesystem protection.

## 30.3 Scope warning

A root snapshot may include application data unintentionally. Ono MUST show dataset/subvolume layout and excluded/included mutable data.

## 30.4 Verification

Package update verification SHOULD include:

- installed package version;
- package-manager consistency;
- affected service state where known;
- explicitly requested health checks.

## 30.5 Reboot requirement

The provider MAY mark reboot requirement or recommendation as a ProposedEffect. It MUST distinguish requirement from suggestion.

---

# 31. Configuration Change Workflow

This is a reference workflow for the complete v0.6 model.

```text
plan {
    replace file /etc/nginx/nginx.conf from ./nginx.conf
    validate config nginx
    restart service nginx
    verify service nginx state == running
    verify socket :443 exists
}
```

Resolution:

```text
path -> mount -> filesystem -> dataset/subvolume
service -> systemd provider
socket -> network provider
```

Protection:

```text
filesystem recovery asset
```

Impact:

```text
config -> service -> process -> listeners -> clients
```

Apply:

```text
create recovery point
replace file atomically
validate
restart
```

Verify:

```text
service and listeners
```

Recovery if required:

```text
restore config from recovery asset
restart service
verify restored state
```

---

# 32. Destructive Filesystem Operations

## 32.1 File deletion

A plan to remove a regular file on a protected snapshot-capable filesystem MAY be strongly protected for the file contents/metadata.

## 32.2 Directory deletion

For recursive deletion, Ono MUST calculate the relevant persistence boundaries and size/risk before plan sealing.

## 32.3 Mounted children

Recursive path operations MUST NOT assume mounted filesystems or nested subvolumes belong to the same recovery scope.

## 32.4 Pseudo-filesystems

`/proc`, `/sys`, `/dev`, runtime tmpfs and similar non-persistent domains MUST never be presented as snapshot-protected merely because their mountpoint path is beneath `/`.

---

# 33. Process and Signal Actions

## 33.1 Runtime state is generally not recoverable

Signals, especially `SIGKILL`, are normally irreversible in v0.6.

A filesystem snapshot does not change this classification.

## 33.2 Service-level compensation

Stopping a service may be compensatable by starting it again, but this does not restore process identity, in-memory state or connections.

## 33.3 Rendering

```text
plan kill process 4421 --signal SIGKILL

protection
  UNPROTECTED

irreversible
  process runtime state

possible impact
  open connections
  temporary data held only in memory
```

---

# 34. Network Actions

## 34.1 Routes/firewall/interfaces

Providers MAY expose inverse actions, but network changes often have remote-management lockout risk.

## 34.2 Session preservation

If a plan may remove the path used by the active remote Ono link, this MUST be a CRITICAL risk landmark.

## 34.3 Recovery path

For remote network changes, a provider SHOULD support timed/leased rollback mechanisms where the underlying platform allows it.

A timer-based recovery action is a `RecoveryAsset` only when it is concrete and validated.

---

# 35. External Side Effects

## 35.1 Definition

Examples:

```text
HTTP POST
email sent
message published
DNS provider update
cloud API mutation
payment request
```

## 35.2 Recovery

Such effects are usually not recoverable through local snapshots.

They MUST remain separately visible.

## 35.3 Provider compensation

A provider MAY define a compensating action, such as deleting a newly created cloud resource, but this is `COMPENSATABLE`, not rollback.

---

# 36. Plan Storage and References

## 36.1 Persistence

Sealed plans MUST be persisted so they survive shell exit and context compaction.

## 36.2 Default store

The reference implementation SHOULD use SQLite with versioned schema, consistent with v0.5 storage practice where practical.

## 36.3 Plan contents

Secrets MUST NOT be persisted in raw form when an opaque secret handle can be used.

## 36.4 References

Plan references MUST be stable enough for:

```text
get plan a82f
apply plan/a82f
recover plan/a82f
```

---

# 37. Recovery Asset Retention and Cleanup

## 37.1 Default retention

Default v0.6 temporary recovery retention:

```text
24 hours after successful verification
```

This is configurable.

## 37.2 Failure retention

Assets for FAILED, DEGRADED or RECOVERY_FAILED plans MUST NOT be automatically deleted by ordinary success retention rules.

## 37.3 Cleanup preview

Before deleting an asset whose removal changes recovery capability, Ono SHOULD show which plans become unrecoverable.

## 37.4 Storage pressure

If recovery assets cause storage pressure, Ono may surface landmarks and recommend cleanup, but MUST NOT delete assets early without policy authorization.

## 37.5 `get recovery`

```text
ID        TYPE    PLAN   AGE   COST      EXPIRES   STATUS
r-a82f    zfs     a82f   14m   312 MiB   23h46m    ready
r-91aa    btrfs   91aa   3h    1.8 GiB   21h       ready
```

Cost numbers MUST be labeled estimated where filesystem accounting is not exact.

---

# 38. Recovery Cost Model

## 38.1 Dimensions

```text
initial latency
retained storage growth
I/O overhead
quiesce duration
reboot/downtime requirement
cleanup cost
```

## 38.2 Snapshot cost

Ono MUST NOT display "free" for CoW snapshots.

It MAY display:

```text
initial creation   near-instant
initial space      minimal
future growth      depends on changed blocks
retention risk     moderate
```

## 38.3 Policy limits

Configuration can bound automatic protection by:

- estimated size;
- target scope;
- quiesce duration;
- snapshot count;
- filesystem free-space floor.

---

# 39. Provider Consistency and Application Quiesce

## 39.1 Filesystem consistency is not application consistency

This distinction MUST be visible everywhere.

## 39.2 Database example

A filesystem snapshot containing PostgreSQL files may be crash-consistent and recoverable by PostgreSQL's own WAL semantics, but Ono MUST NOT independently label it `APPLICATION_CONSISTENT` unless a PostgreSQL-aware provider asserts that guarantee.

## 39.3 Quiesce protocol

A KUANG/11 application recovery provider may define:

```text
prepare_quiesce
verify_quiesced
create_storage_asset
resume
verify_resumed
```

Failures at each step must have compensation rules.

---

# 40. Approval and Human Interaction

## 40.1 `apply` is intentional commitment

For LOW/MODERATE plans without irreversible actions, invoking `apply` on a sealed plan is sufficient user intent.

## 40.2 High-risk gates

HIGH/CRITICAL or irreversible plans require additional acknowledgement.

The confirmation MUST summarize the actual risk reason, not display generic "Are you sure?".

Example:

```text
CRITICAL CHANGE

18/18 frontend nodes will restart.
No healthy serving member is excluded.
Protection does not preserve active client sessions.

Type: apply plan/a82f --accept-service-outage
```

## 40.3 Script behavior

Scripts MUST specify required acknowledgements as flags/policy and MUST fail rather than prompt.

---

# 41. Idempotency and Resume

## 41.1 Action idempotency

Each action contract declares:

```text
IDEMPOTENT
RETRY_SAFE_WITH_TOKEN
NON_IDEMPOTENT
UNKNOWN
```

## 41.2 Interrupted apply

On shell crash/restart, plan state MUST be reconstructable from persisted action records and provider evidence.

Ono MUST NOT blindly rerun unknown/non-idempotent actions.

## 41.3 Resume

`resume plan/a82f` MAY continue only actions whose prior status and idempotency permit it.

Otherwise a new recovery or rebase decision is required.

---

# 42. Concurrency and Locks

## 42.1 Optimistic by default

Ono uses preconditions and drift detection rather than broad global locks.

## 42.2 Provider locks

Providers MAY acquire narrow locks/leases when the system offers meaningful semantics.

## 42.3 Lock lifetime

Locks MUST be bounded and released on failure.

## 42.4 Multiple Ono sessions

Plan store MUST prevent two sessions from applying the same sealed plan concurrently without an explicit provider-safe mechanism.

---

# 43. Security Model

## 43.1 Principle

v0.6 increases the shell's mutation orchestration power and therefore increases the consequences of contract bugs.

Security is part of correctness.

## 43.2 Capabilities

Planning MAY be available without mutation capability.

Applying requires target action capabilities plus protection provider capabilities.

## 43.3 Privilege escalation

The plan MUST show which actions require privilege and when elevation will occur.

## 43.4 Recovery privilege

Recovery may require stronger privileges than the original mutation. This MUST be discovered before protection is advertised as usable.

## 43.5 TOCTOU

Paths, symlinks and identities MUST be revalidated using safe filesystem APIs. Protection of one object followed by mutation of a replaced symlink target is unacceptable.

## 43.6 Snapshot names

Provider-generated asset names MUST be sanitized and not contain user-controlled command syntax.

## 43.7 Plugin trust

KUANG/11 change/recovery providers require explicit capabilities and are subject to protocol/audit isolation.

---

# 44. Privacy and Sensitive Recovery Data

Recovery assets may contain:

- credentials;
- private keys;
- database files;
- shell configuration;
- application secrets.

Therefore:

1. metadata MAY appear in plan history;
2. contents MUST NOT appear in default rendering;
3. local recovery stores MUST use restrictive permissions;
4. plugins MUST receive scoped access only;
5. remote recovery metadata MUST not disclose paths/secrets beyond policy;
6. deletion MUST remove Ono-owned copies according to provider guarantees.

---

# 45. Structured Error Family

v0.6 defines at minimum:

```text
change.plan_not_sealed
change.plan_expired
change.plan_already_applying
change.plan_drift_detected
change.target_unresolved
change.target_changed
change.action_not_plannable
change.opaque_action_forbidden
change.historical_context_read_only
change.precondition_failed
change.prepare_failed
change.apply_failed
change.verification_failed
change.verification_unknown
change.irreversible_not_accepted
change.risk_not_accepted
change.bulk_guard_failed
change.remote_state_unknown

recovery.provider_unavailable
recovery.asset_create_failed
recovery.asset_invalid
recovery.asset_expired
recovery.scope_mismatch
recovery.coverage_insufficient
recovery.consistency_unknown
recovery.newer_state_conflict
recovery.destructive_history_not_accepted
recovery.requires_offline
recovery.requires_reboot
recovery.apply_failed
recovery.verification_failed
recovery.cleanup_blocked
recovery.storage_pressure

transaction.atomicity_unavailable
transaction.cross_provider_not_atomic
```

Errors MUST be structured values with provenance and remediation hints where safe.

---

# 46. Canonical Public Schemas

## 46.1 `ono.change-plan/1`

Required conceptual fields:

```text
id
revision
state
created_at
sealed_at?
intent
targets
actions
effects
impact_summary
protection_summary
risk
strategy
verification_contracts
preconditions
provider_bindings
accepted_risk_overrides
digest
```

## 46.2 `ono.plan-action/1`

```text
id
role
target
provider
depends_on
preconditions
execution
idempotency
proposed_effects
recovery_semantics
verification
status
```

## 46.3 `ono.proposed-effect/1`

As defined in section 8.

## 46.4 `ono.recovery-asset/1`

As defined in section 11.

## 46.5 `ono.recovery-plan/1`

```text
id
source_plan
source_assets
target_state
restore_actions
newer_state_impact
unrecoverable_effects
risk
verification_contracts
requires_reboot
requires_offline
```

## 46.6 `ono.verification-result/1`

As defined in section 23.

## 46.7 `ono.impact-graph/1`

MUST preserve canonical object refs and relationship provenance from v0.4.

---

# 47. Machine-Readable Contract Set

The v0.6 implementation MUST create machine-readable public contracts under an appropriate immutable-versioned namespace, conceptually:

```text
docs/spec/change/
  plans.yaml
  actions.yaml
  effects.yaml
  risk.yaml
  verification.yaml
  strategies.yaml

docs/spec/recovery/
  providers.yaml
  assets.yaml
  consistency.yaml
  policies.yaml
  errors.yaml
```

These contracts SHOULD generate:

- help;
- completion;
- schema docs;
- conformance fixtures;
- provider SDK types;
- error registries;
- test matrices.

CI MUST detect drift between stable contracts and registered runtime behavior.

---

# 48. KUANG/11 Change and Recovery Extensions

## 48.1 Purpose

KUANG/11 is the primary extension point for domain-specific plan intelligence without bloating Ono core.

## 48.2 Plugin contribution types

A plugin MAY contribute:

```text
ActionProvider
ImpactProvider
RecoveryProvider
VerificationProvider
RiskRule
ChangeView
```

## 48.3 Capability model

Example capabilities:

```text
change.plan.read
change.plan.contribute
change.action.execute
recovery.discover
recovery.prepare
recovery.restore
verification.observe
```

## 48.4 No authority escalation

A plugin that can describe impact MUST NOT automatically gain permission to execute the change.

## 48.5 Application provider example

A PostgreSQL plugin could contribute:

- database restart semantic impact;
- checkpoint/quiesce action;
- application-consistency validation;
- recovery verification;
- transaction-local rollback.

---

# 49. AI / Model Broker Integration

## 49.1 AI proposes intent, not privileged execution

A model may produce `ProposedIntent` or request creation of a plan.

It MUST NOT bypass planning, capabilities, protection or approval.

## 49.2 Safe flow

```text
user request
  -> model interpretation
  -> ProposedIntent
  -> Ono resolves ChangePlan
  -> Ono computes impact/protection/risk
  -> user/automation policy approves
  -> Ono executes
  -> Ono verifies
```

## 49.3 Model statements are not provider truth

An AI suggestion that a change is reversible MUST NOT change `RecoveryCoverage` unless a real recovery provider proves it.

---

# 50. Reference Crate Architecture

v0.6 SHOULD be implemented in dedicated crates instead of expanding `ono-cli` into a change-management god object.

Reference architecture:

```text
ono-change-core
  canonical plan/action/effect/risk types

ono-change-plan
  plan builder, resolution, sealing, revisions

ono-change-impact
  spatial/temporal impact derivation

ono-change-protection
  recovery discovery and policy

ono-change-executor
  prepare/apply state machine, idempotency, resume

ono-change-recovery
  recovery-plan construction and verification

ono-change-render
  CLI/TUI projections

ono-recovery-zfs
ono-recovery-btrfs
ono-recovery-files
```

## 50.1 Dependency rules

- core types do not call providers;
- renderers do not mutate state;
- recovery providers do not own plan orchestration;
- `ono-cli` wires components but does not implement plan semantics;
- v0.4 spatial and v0.5 temporal crates are dependencies through stable APIs, not duplicated logic.

---

# 51. Change Provider API

Conceptual API:

```rust
trait ChangeProvider {
    fn supports(&self, intent: &Intent) -> Support;
    fn resolve(&self, intent: &Intent, world: &WorldSnapshot) -> Result<PlanFragment>;
    fn revalidate(&self, action: &PlanAction) -> Result<PreconditionResult>;
    fn execute(&self, action: &PlanAction, ctx: &ExecutionContext) -> Result<ActionResult>;
    fn verify(&self, contract: &VerificationContract) -> Result<VerificationResult>;
}
```

Providers MUST NOT mutate state during `supports` or `resolve`.

---

# 52. Performance Requirements

## 52.1 Plan creation

Typical single-host single-service plan creation SHOULD complete in less than 150 ms excluding explicitly slow provider discovery.

## 52.2 Impact

Impact derivation over an already indexed v0.4 world SHOULD begin rendering within 100 ms and stream additional graph detail when needed.

## 52.3 Protection discovery

Filesystem recovery discovery SHOULD not perform expensive full-tree scans merely to map a path to dataset/subvolume.

## 52.4 Apply overhead

The planning framework itself SHOULD add negligible overhead relative to intentional protection operations.

## 52.5 Large plans

Plans with thousands of targets MUST use bounded memory and streaming resolution, but final sealed target identity lists must be durable.

---

# 53. Configuration

Reference configuration:

```toml
[change]
default_protection = "prefer"
default_strategy = "sequential"
high_risk_requires_ack = true
critical_risk_requires_ack = true
allow_opaque_actions = false

[change.bulk]
warn_targets = 10
high_risk_targets = 50

[recovery]
retention = "24h"
max_auto_snapshot_count = 32
min_filesystem_free = "10%"
prefer_read_only_snapshots = true

[recovery.zfs]
enabled = true
prefer_selective_restore = true
allow_destructive_rollback = false

[recovery.btrfs]
enabled = true
prefer_read_only_snapshots = true
root_recovery = "next-boot"
```

Configuration MUST NOT silently weaken explicit plan requirements.

---

# 54. Test Strategy

## 54.1 Unit tests

Must cover:

- lifecycle transitions;
- sealing/digest stability;
- target freezing;
- effect confidence propagation;
- protection matrix reduction;
- risk rules;
- recovery coverage;
- drift detection;
- strategy planning;
- retention logic.

## 54.2 Property tests

Property tests SHOULD verify:

- sealed plans are immutable;
- no target can appear after seal without revision change;
- `PROTECTED` implies required domains have validated assets;
- unknown coverage never upgrades automatically;
- cleanup never removes required assets before policy permits;
- recovery plans never omit known newer-state destruction.

## 54.3 Fuzzing

Fuzz:

- plan parser/block grammar;
- plan/asset deserialization;
- provider protocol messages;
- snapshot names and paths;
- impact graph inputs;
- recovery metadata.

## 54.4 Integration tests

Use real ZFS/Btrfs where CI environment permits, not only mocks.

Mock providers remain useful for deterministic lifecycle testing.

## 54.5 Failure injection

Tests MUST inject failures at every lifecycle boundary:

```text
snapshot creation fails
snapshot validates wrong scope
mutation fails after protection
shell crashes mid-apply
provider disconnects
verification times out
recovery fails halfway
cleanup fails
storage fills
remote host disappears
```

## 54.6 Security tests

Cover:

- symlink swap races;
- path traversal;
- privilege mismatch;
- malicious plugin provider;
- shell injection attempts;
- tampered plan store;
- tampered recovery asset metadata;
- concurrent apply.

---

# 55. Acceptance Scenarios

The release acceptance suite MUST include at least the following black-box scenarios.

## 55.1 Planning

1. `plan restart service` creates no mutation.
2. Sealed bulk target membership remains fixed after new objects appear.
3. Plan digest changes when strategy or target changes.
4. Historical context cannot create executable plan against the past.
5. Opaque external mutation is rejected by default.

## 55.2 Drift

6. File changes after seal -> apply refuses.
7. Service target disappears -> apply refuses.
8. Non-material CPU drift -> plan remains valid.
9. Rebase creates new revision, original unchanged.

## 55.3 ZFS

10. File target resolves to correct dataset.
11. Separate child dataset is not falsely covered.
12. Snapshot is created before first mutation.
13. Snapshot creation failure prevents mutation.
14. Selective restore recovers one changed file without reverting unrelated later files.
15. Full rollback requiring newer snapshot destruction is blocked without acceptance.
16. Root rollback requiring reboot is reported before execution.

## 55.4 Btrfs

17. Target resolves to correct subvolume.
18. Nested subvolume boundary is detected.
19. Required nested subvolume receives separate snapshot.
20. Read-only snapshot is retained.
21. Recovery plan describes subvolume replacement/restore method rather than fake in-place rollback.
22. Root recovery requiring reboot is explicit.

## 55.5 Generic recovery

23. Non-snapshot filesystem uses file/config protection for small config mutation.
24. Secret config recovery asset is not rendered.
25. `get recovery` shows retention and scope.
26. Cleanup refuses to remove asset required by failed plan without explicit override.

## 55.6 Protection truth

27. Filesystem protected + SIGKILL action -> plan remains partially/unprotected for process runtime.
28. Local snapshot + HTTP POST -> external side effect remains irreversible.
29. Unknown provider recovery semantics remain UNKNOWN.
30. Transactional status appears only for transaction-capable provider scope.

## 55.7 Apply/verify

31. Required PREPARE failure means zero mutate actions execute.
32. Mutation succeeds but required verification fails -> plan FAILED.
33. Advisory verification fails -> plan DEGRADED.
34. Successful service/config workflow -> VERIFIED.

## 55.8 Recovery

35. `recover @plan` does not immediately mutate state.
36. RecoveryPlan shows newer state that would be lost.
37. Selective recovery preserves unrelated newer file.
38. Recovery verification distinguishes persistent vs runtime equivalence.
39. Recovery failure retains recovery asset and exact state.

## 55.9 Resume/idempotency

40. Crash after idempotent action can resume safely.
41. Crash after unknown non-idempotent action does not blindly retry.
42. Same plan cannot apply concurrently from two sessions.

## 55.10 Bulk/remote

43. Canary stops after first batch verification failure.
44. Remote per-host protection matrix is accurate.
45. Disconnected host is UNKNOWN, not automatically failed.
46. Network plan threatening active remote link is CRITICAL.

## 55.11 Temporal/spatial integration

47. `map --plan` uses real object identities and no fabricated future PID.
48. Plan lifecycle events appear in v0.5 timeline.
49. Pre/post plan state can be compared.
50. Recovery event chain is auditable.

---

# 56. ZFS/Btrfs Destructive-Recovery Safety Checklist

Because storage rollback can cause severe data loss, the following checklist is mandatory before enabling a first-party destructive recovery path.

## 56.1 ZFS

The implementation MUST prove:

- exact dataset identity;
- exact snapshot identity;
- target snapshot still exists;
- whether it is the latest relevant snapshot;
- all newer snapshots/bookmarks affected by rollback;
- all clones affected by destructive flags;
- child dataset boundaries;
- mount/unmount requirement;
- reboot/offline requirement;
- expected discarded live data;
- sufficient privilege;
- explicit acceptance for history destruction.

## 56.2 Btrfs

The implementation MUST prove:

- exact filesystem and subvolume ID;
- target snapshot exists and is valid;
- nested subvolume boundaries;
- whether selected restore is possible;
- whether subvolume replacement is required;
- default-subvolume/boot impact;
- mount/reboot requirement;
- later files/state that would be discarded;
- correct treatment of read-only snapshot;
- no assumption of recursive snapshot coverage.

## 56.3 Fail closed

If any critical recovery fact cannot be established, destructive recovery MUST be blocked rather than guessed.

---

# 57. Implementation Sequence

The phases below are dependency-respecting production steps, not MVP scope reductions.

## Phase P1 - Contracts and pure core model

Deliver:

- plan/action/effect/recovery schemas;
- lifecycle state machine;
- protection/risk enums;
- machine-readable registries;
- serialization and digest rules.

Success: plans can be constructed and validated in pure tests with no execution.

## Phase P2 - Plan builder and target resolution

Deliver:

- `plan` single action;
- plan blocks;
- pipeline planning;
- target freezing;
- revisions/sealing;
- drift preconditions.

Success: side-effect-free plans accurately bind current objects.

## Phase P3 - Impact integration

Deliver:

- v0.4 graph impact;
- unknown boundaries;
- risk rules;
- `impact`;
- `map --plan` basic overlay.

## Phase P4 - Executor and verification

Deliver:

- PREPARE/APPLY/VERIFY lifecycle;
- persisted action states;
- idempotency;
- resume;
- required/advisory verification.

No storage snapshot provider is required to complete this phase, but protection states must remain honest.

## Phase P5 - Generic file/config recovery

Deliver:

- small-scope file protection;
- secure recovery store;
- recovery plans;
- persistent-state verification.

## Phase P6 - ZFS provider

Deliver the complete section 13 contract and destructive-recovery safety checklist.

## Phase P7 - Btrfs provider

Deliver the complete section 14 contract and destructive-recovery safety checklist.

## Phase P8 - Protection policy and retention

Deliver:

- prefer/require/maximize;
- JIT protection;
- early protect;
- freshness;
- cost limits;
- cleanup/retention.

## Phase P9 - Recovery hardening

Deliver:

- newer-state conflict analysis;
- recovery verification;
- recovery failure resume;
- explicit destructive gates;
- reboot/offline plans.

## Phase P10 - Bulk and remote strategies

Deliver:

- sequential/batch/canary/parallel-N;
- remote per-host plan fragments;
- disconnect semantics;
- availability risk rules.

## Phase P11 - KUANG/11 SDK

Deliver plugin contracts for actions, impact, recovery and verification.

## Phase P12 - TUI and earned coolness

Deliver:

- plan map overlay;
- recovery coverage symbols;
- interactive plan inspector;
- execution/verification progress;
- timeline linkage.

## Phase P13 - Hardening and release proof

Deliver:

- failure injection;
- real filesystem acceptance;
- security review;
- performance proof;
- complete release gate.

---

# 58. Spec-Driven Work Packages

A code-generating agent can derive work items such as:

```text
CHG-001   PlanId and revision model
CHG-002   Plan lifecycle state machine
CHG-003   Plan canonical digest
CHG-004   Draft -> resolve -> seal
CHG-005   Target freezer
CHG-006   Drift revalidation
CHG-007   Plan block parser
CHG-008   Pipeline plan collector

EFF-001   ProposedEffect schema
EFF-002   Effect confidence lattice
IMP-001   ImpactGraph integration
IMP-002   Unknown boundary representation
IMP-003   Spatial blast-radius rules

RSK-001   Risk dimension registry
RSK-002   High/critical gates
RSK-003   Bulk risk rules

PROT-001  Protection policy
PROT-002  RecoveryCandidate discovery
PROT-003  RecoveryAsset schema
PROT-004  JIT protection orchestration
PROT-005  Asset freshness
PROT-006  Asset retention
PROT-007  Cost model

EXEC-001  PREPARE engine
EXEC-002  APPLY dependency scheduler
EXEC-003  Action persistence
EXEC-004  Idempotency model
EXEC-005  Resume after crash
EXEC-006  Concurrent apply guard

VER-001   VerificationContract
VER-002   Required/advisory semantics
VER-003   Pre/post checkpoint integration
VER-004   Verification timeout

REC-001   RecoveryPlan builder
REC-002   Newer-state conflict analyzer
REC-003   Recovery apply lifecycle
REC-004   Recovery verification
REC-005   Destructive recovery gate

ZFS-001   Path -> ZFS dataset resolver
ZFS-002   Snapshot candidate planner
ZFS-003   Snapshot creation/validation
ZFS-004   Recursive dataset snapshot inventory
ZFS-005   Selective file restore
ZFS-006   Full rollback conflict analysis
ZFS-007   Snapshot/bookmark/clone destruction guard
ZFS-008   Root/offline recovery semantics

BTR-001   Path -> Btrfs subvolume resolver
BTR-002   Nested subvolume boundary scanner
BTR-003   Read-only snapshot creation
BTR-004   Multi-subvolume protection set
BTR-005   Selective restore
BTR-006   Subvolume replacement recovery
BTR-007   Root/default-subvolume recovery

FIL-001   Secure file recovery store
FIL-002   Metadata-preserving backup
FIL-003   Atomic file restore

BULK-001  Sequential strategy
BULK-002  Batch strategy
BULK-003  Canary strategy
BULK-004  Parallel-N scheduler

REM-001   Remote plan fragments
REM-002   Remote protection matrix
REM-003   Unknown disconnect state

K11C-001  ChangeProvider SDK
K11C-002  RecoveryProvider SDK
K11C-003  VerificationProvider SDK
K11C-004  Plugin capability policy

TUI-001   Plan inspector
TUI-002   Map proposed-state overlay
TUI-003   Recovery coverage rendering
TUI-004   Apply/verify progress view

TEST-001  Lifecycle property suite
TEST-002  Failure-injection harness
TEST-003  ZFS container/VM acceptance
TEST-004  Btrfs loopback acceptance
TEST-005  Crash-resume acceptance
TEST-006  Destructive-recovery safety suite
```

Each work package MUST link back to stable section IDs or machine contracts.

---

# 59. Dogfooding Scenarios

## 59.1 Nginx configuration

Goal: replace config and restart service with confidence.

Expected flow:

```text
plan {...}
impact @plan
apply @plan
verify @plan
```

Success requires visible filesystem protection and explicit TCP-session exclusion.

## 59.2 Package update on ZFS root

Goal: update a package with snapshot protection.

Ono must show which datasets are included/excluded and whether rollback is online or reboot-dependent.

## 59.3 Btrfs root with separate `/var`

Goal: change `/etc` and `/var/lib/app`.

Ono must create separate subvolume snapshots and must not claim the root snapshot covers `/var`.

## 59.4 Irreversible process kill

Goal: `SIGKILL` one process.

Even on ZFS, protection remains UNPROTECTED for runtime state.

## 59.5 Remote fleet restart

Goal: restart nginx on 20 hosts.

Ono must show per-host recovery coverage and support canary strategy.

## 59.6 Recovery after unrelated later changes

Goal: recover nginx config two hours later.

Ono must prefer selective restore and preserve unrelated newer `/etc` state.

## 59.7 Recovery asset storage pressure

Goal: verify that protection does not silently fill the filesystem.

Ono surfaces cost/retention and never deletes failed-plan recovery assets unexpectedly.

---

# 60. Resolved Design Decisions

The following decisions are closed for v0.6 and MUST NOT be reopened by implementation agents without a formal ADR showing a direct contradiction with higher-authority requirements.

| Question | Decision | Intent |
|---|---|---|
| Is planning just dry-run? | No. Plan is a first-class typed object. | Make intent inspectable and composable. |
| Does `plan` mutate? | Never. | Preserve trust. |
| When are snapshots created? | During PREPARE immediately before mutation by default. | Minimize stale recovery windows. |
| Can snapshots be created early? | Yes, via `protect`, with freshness revalidation. | Support maintenance preparation. |
| Is `reversible` boolean? | No. Coverage is per domain and mechanism. | Avoid false safety. |
| Does filesystem snapshot restore runtime? | No. | Scope truthfully. |
| Is snapshot a backup? | No. | Avoid false disaster-recovery claim. |
| Is Btrfs rollback modeled like ZFS? | No. Recovery workflow is provider-specific. | Respect actual semantics. |
| Are nested Btrfs subvolumes recursively protected? | No. Each required subvolume is explicit. | Prevent data-loss assumptions. |
| Can ZFS rollback destroy newer snapshots automatically? | No. Explicit destructive gate required. | Protect newer history. |
| Is full dataset rollback preferred? | No. Prefer least-destructive recovery that satisfies intent. | Preserve unrelated newer state. |
| What does `recover` do? | Creates RecoveryPlan; does not execute immediately. | Recovery is itself risky. |
| Is auto-recovery default? | No. | Avoid making failure worse automatically. |
| Are multi-provider changes transactions? | No. | No fake atomicity. |
| Can provider-local transactions be shown? | Yes, scoped. | Preserve real guarantees. |
| Are plan targets dynamic at apply? | No, frozen at seal. | Prevent accidental scope expansion. |
| What happens on drift? | Fail closed; rebase creates new revision. | Prevent stale intent. |
| Is unknown impact hidden? | No. | Future honesty. |
| Does apply always prompt? | No; apply is commitment. High/critical/irreversible gets explicit gate. | Keep normal flow usable. |
| Can scripts prompt? | No. | Determinism. |
| Does verification equal exit code? | No. | Verify actual state. |
| Is recovery verification full world identity? | No. Domain-specific equivalence. | Avoid impossible promises. |
| Where does core logic live? | Dedicated change/recovery crates, not `ono-cli`. | Preserve architecture. |
| Can AI declare protection? | No. Only recovery providers establish coverage. | Keep authority deterministic. |

---

# 61. Explicit Non-Goals

v0.6 MUST NOT become:

- Ansible/Puppet/Salt replacement;
- a generic YAML deployment DSL;
- Terraform clone;
- generic workflow engine;
- global distributed ACID transaction manager;
- automatic disaster-recovery system;
- backup product;
- predictive AI SRE;
- universal rollback layer;
- service mesh;
- package manager;
- filesystem manager;
- opaque command recorder that claims reversibility;
- automatic "undo everything" button.

The goal is a safe prospective interface for changes made through Ono and explicit providers.

---

# 62. Failure Modes to Avoid

## 62.1 Snapshot theatre

Bad:

```text
rollback available
```

when only `/` was snapshotted and the changed data lives in `/var`.

## 62.2 Universal undo button

Bad:

```text
undo last command
```

when the command sent network requests or killed processes.

## 62.3 Hidden protection mutation

Bad: plan creation silently creates snapshots.

Planning must be side-effect free.

## 62.4 Fake atomicity

Bad: calling a sequence of compensating operations a transaction.

## 62.5 Destructive rollback convenience

Bad: automatically using storage flags that destroy newer snapshots/history because it makes rollback work.

## 62.6 Protection as permission to be reckless

The UI must communicate residual risk, not turn recovery into a green safety badge.

## 62.7 Snapshot everything

Protection should follow planned mutation scope and policy, not blanket-snapshot every filesystem.

## 62.8 Recovery without drift analysis

Recovering hours later without considering newer state is unacceptable.

## 62.9 Exit-code verification

A successful command does not prove intended outcome.

## 62.10 Logic accretion in `ono-cli`

v0.6 is too complex to live as more branches inside the CLI integration crate.

## 62.11 AI-generated causal/prospective truth

AI may explain or propose. It may not upgrade unknown to guaranteed.

---

# 63. Release Definition

v0.6 is release-ready only when all of the following are true:

1. Plan creation is demonstrably side-effect free.
2. Sealed plans are immutable and digest-verified.
3. Target freezing and drift detection work under race tests.
4. PREPARE failure proves no mutation occurred.
5. Apply state survives crash/restart.
6. Verification is independent of action exit status.
7. Recovery always produces a RecoveryPlan before mutation.
8. ZFS scope/rollback semantics pass real acceptance tests.
9. Btrfs nested-subvolume semantics pass real acceptance tests.
10. Protection matrix never overstates coverage in adversarial fixtures.
11. Irreversible and unknown effects remain visible.
12. Destructive recovery of newer state requires explicit acceptance.
13. Recovery verification reports domain-specific equivalence.
14. Bulk/canary behavior is deterministic.
15. Remote unknown state remains unknown.
16. KUANG/11 providers cannot escalate capability.
17. All machine-readable contracts match runtime registration.
18. Security/fuzz/failure-injection suites pass.
19. Container/VM acceptance uses the real `ono` binary and real supported filesystems.
20. No release-blocking known defects remain.

---

# 64. End-to-End Reference Interaction

The following interaction defines the intended product feel.

```text
local:// > plan {
    replace file /etc/nginx/nginx.conf from ./nginx.conf
    validate config nginx
    restart service nginx
    verify service nginx state == running
    verify socket :443 exists
}

PLAN / a82f

intent
  update nginx configuration

targets
  /etc/nginx/nginx.conf
  nginx.service

impact
  2 direct targets
  6 known related objects
  14 active TCP connections may be interrupted

protection
  PROTECTED <->

  planned recovery asset
    ZFS snapshot
    rpool/ROOT/debian@ono-a82f
    consistency: filesystem-consistent

  covered
    /etc/nginx/nginx.conf

  not covered
    process memory
    active TCP sessions
    completed external requests

risk
  MODERATE

verification
  nginx.service == running
  socket :443 exists

PLAN NOT EXECUTED

local:// > map --plan @a82f

                 nginx.service  ~
                       |
            +----------+----------+
            |                     |
      nginx.conf ~ <->        worker set ?
            |                     |
        zfs snapshot           socket :443 expected

local:// > apply @a82f

PREPARING
  [1/1] create ZFS recovery point            PASS

PROTECTION READY
  rpool/ROOT/debian@ono-a82f

APPLYING
  [1/3] replace nginx.conf                   PASS
  [2/3] validate nginx config                PASS
  [3/3] restart nginx.service                PASS

VERIFYING
  service running                            PASS
  listener :443                              PASS

PLAN VERIFIED

recovery point retained
  recovery/r-a82f
  expires in 24h

local:// > timeline --plan @a82f

14:03:11.001  plan apply started
14:03:11.084  recovery asset created
14:03:11.102  nginx.conf replaced
14:03:11.131  config validation passed
14:03:11.180  service restart requested
14:03:11.943  old worker disappeared
14:03:12.114  new worker appeared
14:03:12.309  listener :443 observed
14:03:12.311  plan verified
```

Later, after another unrelated `/etc` change:

```text
local:// > recover @a82f

RECOVERY PLAN / r91c

source
  plan a82f
  recovery/r-a82f

recommended method
  selective restore

will restore
  /etc/nginx/nginx.conf

will preserve
  /etc/ssh/sshd_config   changed after plan
  /etc/hosts             changed after plan

runtime
  nginx restart required
  TCP sessions cannot be restored

risk
  MODERATE

RECOVERY NOT EXECUTED
```

The operator can inspect and apply the recovery plan without pretending the entire machine can be rewound.

---

# 65. Final Product Principle

v0.6 succeeds when the operator no longer experiences a dangerous mutation as a single irreversible command line.

The desired mental model is:

```text
INTENT
   |
   v
PLAN
   |
   +--> IMPACT
   |
   +--> PROTECTION
   |
   v
PREPARE
   |
   v
APPLY
   |
   v
VERIFY
   |
   +--> success --> retain recovery --> cleanup later
   |
   +--> failure --> RECOVERY PLAN --> APPLY --> VERIFY
```

The defining principle is:

> **The future should become visible before it becomes real - and where possible, Ono should build the way back before taking the first step.**

Protection is not a promise that nothing can go wrong.

It is a precise, inspectable statement of what Ono can preserve, what it can restore, what it can only compensate, and what remains irreversible.

That precision is what makes powerful change control usable instead of frightening.

---

# Appendix A. Protection Coverage Algorithm

This appendix is normative. It defines how Ono determines whether a plan is unprotected, partially protected or protected.

## A.1 Determine mutation domains

For each MUTATE action, Ono MUST derive one or more `MutationDomain` records.

Canonical domains include:

```text
filesystem-persistent
block-storage-persistent
application-persistent
process-runtime
kernel-runtime
network-runtime
remote-system
external-side-effect
identity/security-state
provider-transaction-state
unknown
```

A single action may touch several domains.

Example:

```text
restart service nginx

mutation domains
  process-runtime
  network-runtime

related persistent domain
  none, unless the plan also changes configuration
```

Example:

```text
replace nginx.conf and restart nginx

mutation domains
  filesystem-persistent
  process-runtime
  network-runtime
```

## A.2 Determine required recovery objective

Each domain receives a recovery objective based on plan intent and policy.

```text
PRESERVE_EXACT
RESTORE_SEMANTIC
COMPENSATE
NO_RECOVERY_REQUIRED
UNKNOWN
```

`PRESERVE_EXACT` is appropriate for file/config persistent state when the operator expects exact prior bytes/metadata.

`RESTORE_SEMANTIC` is appropriate when identity may legitimately change, such as service worker PIDs.

`COMPENSATE` means an inverse action is acceptable but not equivalent to prior state.

## A.3 Discover candidates

For each required domain, the protection engine asks registered RecoveryProviders for candidates.

Candidates are not yet assets.

```text
RecoveryCandidate {
    provider
    target_scope
    objective_supported
    consistency
    estimated_cost
    creation_requirements
    restore_requirements
    exclusions
}
```

## A.4 Candidate dominance

When multiple candidates protect the same domain, Ono SHOULD prefer the least invasive candidate that satisfies the required objective and policy.

Preference order is not simply "strongest snapshot wins".

A small configuration-file backup may dominate a root-dataset rollback for one file because it has a smaller recovery blast radius.

Reference preference considerations:

```text
1. satisfies required objective
2. smallest affected recovery scope
3. strongest useful consistency
4. lowest recovery destructiveness
5. lowest creation cost
6. lowest retained cost
7. lowest downtime
```

## A.5 Compose coverage

A plan can be `PROTECTED` only if every required persistent mutation domain has at least one validated protection path satisfying its objective.

Runtime or external effects that cannot be recovered remain visible as exclusions even if they do not prevent the persistent portion from being called protected.

The plan summary MUST therefore include both:

```text
protection class
coverage exclusions
```

## A.6 Coverage example

```text
plan
  replace /etc/nginx/nginx.conf
  restart nginx

mutation domain                  recovery objective       coverage
filesystem-persistent            PRESERVE_EXACT           ZFS snapshot
process-runtime                  RESTORE_SEMANTIC         restart compensation
network-runtime                  NO exact recovery        excluded

summary
  persistent protection: PROTECTED
  runtime recovery:      COMPENSATABLE
  full-world recovery:   NOT AVAILABLE
```

This is displayed as `PROTECTED` only with the explicit exclusions shown. Ono MUST NOT render a global green "safe" indicator.

## A.7 Unknown domain

If a provider declares an unknown side-effect domain, plan-level protection cannot be stronger than `PARTIALLY_PROTECTED` unless policy explicitly declares that domain irrelevant to the requested recovery objective.

This prevents an opaque command from inheriting false safety from an unrelated snapshot.

---

# Appendix B. Persistence-Domain Resolution

Protection quality depends on mapping targets to the storage object that actually contains their persistent state.

This appendix is normative for first-party Linux support.

## B.1 Resolution pipeline

For a filesystem path, Ono MUST resolve:

```text
path
 -> namespace-visible mount
 -> mount ID
 -> filesystem type
 -> filesystem root
 -> backing persistence object
 -> snapshot/recovery boundaries
 -> provider candidates
```

The result MUST be tied to the process/mount namespace in which the mutation will occur.

## B.2 Mount namespaces

A path seen inside a container or alternate mount namespace may map differently from the host path.

Ono MUST perform persistence resolution in the same namespace context as the intended mutation or through a provider that can establish equivalent identity.

## B.3 Bind mounts

A bind mount does not by itself create a separate persistence domain.

The resolver MUST trace it to the underlying filesystem/subvolume/dataset identity where possible.

## B.4 Overlay filesystems

For overlay/union filesystems, Ono MUST identify the writable upper layer for mutations.

It MUST NOT claim that snapshotting a visible merged mount protects data if the writable layer resides elsewhere and is not included.

## B.5 Container layers

Changes inside a container writable layer may be ephemeral and container-specific.

The provider must distinguish:

```text
image layer
container writable layer
host bind mount
named volume
remote volume
```

A host ZFS snapshot protects only domains actually stored in the snapped dataset.

## B.6 Network filesystems

For NFS, SMB, CephFS and other remote filesystems, local snapshot providers MUST NOT claim protection.

A remote/provider-specific recovery mechanism is required.

The map may show:

```text
/data
  filesystem: nfs
  server: nas01
  recovery: unknown locally
```

## B.7 Pseudo and volatile filesystems

The following are never automatically considered persistent recovery domains:

```text
procfs
sysfs
devtmpfs
tmpfs
cgroupfs
tracefs
debugfs
```

Mutation semantics for these resources belong to runtime/provider actions, not filesystem snapshot protection.

## B.8 ZFS path mapping

The ZFS provider MUST use actual dataset/mount metadata and MUST not infer dataset identity from path naming conventions.

## B.9 Btrfs path mapping

The Btrfs provider MUST resolve subvolume/root IDs and nested subvolume boundaries. A subdirectory named like a subvolume is not sufficient evidence.

## B.10 Resolution rendering

`inspect plan` SHOULD permit expansion of persistence resolution:

```text
/etc/nginx/nginx.conf
  mount        /
  filesystem   zfs
  dataset      rpool/ROOT/debian
  recovery     zfs snapshot available

/var/lib/app/state.db
  mount        /var
  filesystem   btrfs
  subvolume    id=258 @var
  recovery     btrfs snapshot available
```

This view is essential when the operator wants to understand why multiple recovery assets are planned.

---

# Appendix C. Recovery Method Selection

Recovery is not synonymous with restoring the largest possible snapshot.

## C.1 Least-destructive principle

When several methods can satisfy the requested recovery objective, Ono MUST prefer the method that minimizes unrelated state loss.

Reference order for file/config recovery:

```text
1. provider-native object restore
2. selective restore from recovery asset
3. clone/mount recovery asset and copy selected state
4. subvolume/dataset replacement
5. full dataset/filesystem rollback
6. offline/root recovery
```

A lower item MAY be chosen if upper items cannot preserve required metadata or application semantics.

## C.2 Recovery goal

The operator's goal is not automatically "return the whole persistence domain to the snapshot timestamp".

For a failed nginx configuration plan, the normal goal is:

```text
restore the changed configuration object
restore the service to a healthy semantic state
```

not:

```text
rewind every file in /etc and /var to 14:03
```

## C.3 Newer-state conflict model

The RecoveryPlan computes changes since asset creation within the candidate restore scope.

Each newer object/change is classified:

```text
PRESERVED_BY_METHOD
DISCARDED_BY_METHOD
CONFLICTING
UNKNOWN
```

## C.4 Selective restore conflicts

If the same target file was edited again after the failed plan, selective recovery would discard that newer edit.

Ono MUST show this as a target-level conflict.

Example:

```text
/etc/nginx/nginx.conf
  plan a82f wrote at 14:03
  user edited again at 15:12

recovery target
  snapshot from 14:02

conflict
  recovery would discard 15:12 edit
```

## C.5 Recovery merge

Automatic semantic merging of configuration files is an explicit non-goal for the core recovery engine.

A domain-specific KUANG/11 provider MAY offer merge support, but must present it as a distinct RecoveryPlan action with its own verification.

## C.6 Recovery of directories

For directory restore, the provider MUST specify deletion semantics for files that exist now but did not exist in the recovery asset.

Default selective directory restore MUST NOT delete newer extra files unless the recovery objective explicitly requires exact-tree equivalence.

## C.7 Ownership and metadata

Recovery must define whether it restores:

```text
content
mode
owner/group
ACL
xattrs
capabilities
SELinux labels
hardlink relationships
```

Missing metadata support reduces recovery coverage and MUST be visible.

---

# Appendix D. Storage-Specific Provider Contracts

## D.1 ZFS protection action

Conceptual protection action:

```text
ZfsSnapshotAction {
    pool
    datasets: List<DatasetRef>
    snapshot_name
    recursive_creation: bool
    properties?
    plan_id
}
```

The provider MUST return an asset list with one concrete snapshot reference per dataset covered.

## D.2 ZFS snapshot naming

First-party snapshots SHOULD use a collision-resistant but human-readable namespace:

```text
ono-<plan-short-id>-<utc-timestamp>
```

The exact syntax must comply with ZFS naming rules and be sanitized.

## D.3 ZFS space guard

Before auto-protection, the provider SHOULD inspect pool capacity and existing snapshot pressure.

If the pool is already below configured free-space floor, automatic snapshot creation under `prefer` policy SHOULD fail closed into a plan requiring explicit operator decision rather than worsening storage exhaustion.

`require` policy MUST fail apply.

## D.4 ZFS recovery choices

The provider MUST distinguish:

```text
SELECTIVE_FILE_RESTORE
CLONE_AND_COPY
DATASET_ROLLBACK
OFFLINE_ROOT_ROLLBACK
```

## D.5 ZFS rollback planning

Before `DATASET_ROLLBACK`, provider MUST enumerate:

- newer snapshots;
- bookmarks affected by required destructive semantics;
- clones whose existence affects rollback;
- changed live data estimate where available;
- child datasets not covered;
- mount state.

## D.6 Btrfs protection action

Conceptual:

```text
BtrfsSnapshotAction {
    filesystem_uuid
    source_subvol_id
    source_path
    destination_path
    read_only: true
    plan_id
}
```

## D.7 Btrfs multi-subvolume set

When a logical plan spans multiple subvolumes, the protection engine creates a `RecoveryAssetSet`.

The set MUST record that its member snapshots were created sequentially unless a higher-level mechanism can prove a common atomic point.

Ono MUST not invent cross-subvolume atomicity.

## D.8 Btrfs snapshot location

The provider should store snapshots in a predictable recovery namespace on the same Btrfs filesystem by default.

The location must not be underneath a source subvolume in a way that causes recursive operational confusion.

## D.9 Btrfs root method

Reference root policy:

```text
online mutation
 -> create read-only root snapshot
 -> apply
 -> verify

if recovery required
 -> create writable recovery subvolume derived from snapshot OR
    select preserved snapshot according to boot layout
 -> set up next-boot target
 -> reboot
 -> verify after boot
```

Exact implementation depends on distribution layout, but the public plan semantics above are fixed.

## D.10 LVM snapshot invalidation

An LVM provider MUST monitor or validate that a snapshot has not become unusable due to capacity exhaustion. An invalid snapshot is `RecoveryAsset.state = INVALID` and can no longer satisfy protection policy.

---

# Appendix E. Detailed Interaction and TUI Semantics

## E.1 `plan` output is intentionally calm

The plan view should feel like an engineering instrument, not a warning dialog.

Color is secondary to structure.

The user must be able to scan:

```text
intent -> impact -> protection -> residual risk -> verification
```

in seconds.

## E.2 Collapsed default

Long plans render one line per action with expandable details.

```text
PLAN a82f / 83 actions

PREPARE
  20 recovery assets                              ready-to-create

APPLY
  20 update config
  20 restart service

VERIFY
  40 service/listener checks

risk        HIGH
protection  18 protected, 2 partial
strategy    canary 1, then batch 3
```

## E.3 Interactive inspector keys

Reference bindings:

```text
j/k or arrows   move
Enter           inspect selected item
Space           expand/collapse
I               impact
P               protection
R               recovery preview
V               verification contracts
M               map --plan
T               timeline context
A               apply (opens gate if required)
Esc             back
```

Bindings MAY be configurable, but these meanings should guide discoverability.

## E.4 Apply progress

Progress must preserve lifecycle boundaries.

```text
PREPARE  4/4
APPLY    7/9
VERIFY   pending
```

Do not show one generic progress bar that hides whether protection is complete.

## E.5 Failure display

If action 7 fails:

```text
PLAN APPLY FAILED

completed
  6 mutate actions

failed
  action 7: restart service api-04

not executed
  2 actions

protection
  all created assets retained

next
  inspect plan
  recover plan/a82f
  rebase plan/a82f
```

Ono MUST NOT immediately suggest recovery as the only correct next step.

## E.6 Recovery plan view

Recovery uses a visually distinct title and always shows newer-state impact above action details.

```text
RECOVERY PLAN r91c

NEWER STATE AT RISK
  3 files changed after recovery point
  1 newer snapshot would be destroyed

RESTORE TARGET
  ...
```

## E.7 Recovery symbols

Compact map/table symbols:

```text
<-> strong restore asset available
1/2 partial coverage
~> compensation only
!  irreversible / destructive boundary
?  unknown
```

## E.8 No green shield

The product MUST NOT reduce protection to a universal green shield icon. Such an icon encourages users to infer total safety.

Coverage summaries must show exclusions.

---

# Appendix F. Apply-Time Failure Matrix

This matrix defines mandatory behavior.

| Failure point | Mutation occurred? | Default state | Required behavior |
|---|---:|---|---|
| target revalidation | no | SEALED | fail, no prepare |
| privilege check | no | SEALED | fail, no prepare |
| recovery discovery | no | SEALED | fail if policy requires |
| recovery asset creation | no | PREPARE_FAILED | retain already-created sibling assets until cleanup decision |
| recovery validation | no | PREPARE_FAILED | mark invalid asset, no mutation |
| application quiesce | maybe runtime-only | PREPARE_FAILED | attempt resume, record critical result |
| first mutate action | yes/unknown | APPLY_FAILED | stop dependency chain, preserve evidence |
| middle mutate action | yes | APPLY_FAILED | do not blindly reverse; offer RecoveryPlan |
| remote disconnect | unknown | APPLYING/UNKNOWN | preserve unknown, query when link returns |
| verification required failed | yes | FAILED | preserve protection, offer investigation/recovery |
| verification timeout | yes | DEGRADED or FAILED per contract | do not treat as success |
| recovery action fails | yes | RECOVERY_FAILED | preserve remaining assets, exact partial state |
| recovery verification fails | yes | RECOVERY_FAILED | do not claim recovered |
| cleanup fails | no new target mutation | CLOSED_WITH_ASSETS | surface retained asset |

## F.1 Partial prepare cleanup

If 4 of 5 snapshots are created and the fifth fails, Ono MUST NOT mutate the plan targets.

The four assets may be cleaned up automatically only if:

- they were created solely for this failed prepare;
- cleanup is itself safe and provider-declared;
- no quiesce/recovery dependency requires retention.

Cleanup failure must not obscure the original prepare failure.

## F.2 Unknown execution outcome

For a remote or non-idempotent action where execution outcome cannot be established, state MUST be `UNKNOWN`, not guessed.

Recovery planning must treat unknown outcome as an uncertainty boundary.

---

# Appendix G. Provider Conformance Requirements

A RecoveryProvider cannot be considered stable until it passes a conformance suite.

## G.1 Required provider fixtures

Each provider supplies fixtures/scenarios for:

```text
resource discovery
scope boundaries
successful protection
failed protection
asset validation
restore planning
restore execution
newer-state conflict
cleanup
permission failure
capacity failure
```

## G.2 Truth tests

Tests MUST intentionally present misleading layouts and verify the provider refuses false coverage.

Examples:

- path under ZFS mount but actually separate child dataset;
- Btrfs nested subvolume;
- bind mount crossing to ext4;
- NFS mount below snapshotted root;
- container bind mount;
- vanished snapshot;
- ZFS clone/newer snapshot preventing safe rollback;
- read-only filesystem preventing restore.

## G.3 Destructive tests

Destructive recovery tests MUST run only in disposable loopback/VM/container test environments specifically created for the suite.

Production host filesystems MUST never be used for test rollback.

## G.4 Version variance

Providers MUST test supported filesystem/tool versions and degrade to unsupported/unknown rather than execute semantics they have not validated.

## G.5 Conformance metadata

A provider advertises a conformance version:

```text
ono.recovery-provider/1
```

Breaking behavioral changes require versioning.

---

# Appendix H. Policy Profiles

Profiles are convenience presets over explicit policy. They MUST expand to inspectable settings and MUST NOT hide semantics.

## H.1 `interactive`

Default desktop/operator profile:

```text
protection        prefer
risk gate          high+
retention          24h
strategy           sequential
opaque actions     disabled
```

## H.2 `cautious`

```text
protection        require
risk gate          moderate+
retention          72h
strategy           sequential
opaque actions     disabled
minimum free space 15%
```

## H.3 `fleet`

```text
protection        prefer
risk gate          high+
strategy           canary 1 then batch 10%
remote unknown     stop new batches
retention          24h
```

## H.4 `scripted`

No prompts. Every required acknowledgement must be supplied through policy/flags.

## H.5 Profiles are not authority

A plan may impose stricter requirements than a profile. A profile MUST NOT weaken provider-declared safety constraints.

---

# Appendix I. Additional End-to-End Scenarios

## I.1 ZFS-protected package upgrade

```text
local:// > plan update package openssl

PLAN / p31f

target
  openssl 3.x -> newer candidate

persistence
  root dataset rpool/ROOT/debian

protection
  PROTECTED
  planned snapshot rpool/ROOT/debian@ono-p31f

impact
  package files
  7 known dependent services
  reboot requirement: unknown until package metadata/provider result

verification
  installed version
  package DB consistency
```

On apply, snapshot creation failure aborts before package manager mutation.

After apply, if two services fail, plan is `FAILED` even if package manager exits zero.

Recovery planning SHOULD first determine whether package/provider-specific downgrade can safely restore semantics. Full root rollback is not automatically preferred.

## I.2 Btrfs system with nested mutable data

```text
@root  /
@var   /var
@home  /home
```

Plan:

```text
replace /etc/app/config
migrate /var/lib/app/schema
restart app
```

Protection analysis:

```text
@root  required
@var   required
@home  not relevant
```

If the database migration is not known to be crash/application-consistent with a Btrfs snapshot, persistent protection remains `PARTIALLY_PROTECTED` or `UNKNOWN` for application semantics until a domain provider contributes a stronger contract.

## I.3 Network route change over remote link

```text
prod-router:// > plan set route default via 10.0.0.254
```

Ono detects that the current remote link depends on the existing default route.

```text
risk
  CRITICAL

reason
  proposed route may remove transport path used by this active Ono link

recovery candidate
  timed route reversion provider available
  lease: 120s

protection
  COMPENSATABLE
```

The timed reversion must be established before applying the route change. If it cannot be established, `require` protection policy blocks apply.

## I.4 Irreversible mixed plan

```text
plan {
    replace file /etc/app/config
    restart service app
    call external webhook deployment-complete
}
```

Even with ZFS:

```text
filesystem           PROTECTED
service runtime      COMPENSATABLE
webhook side effect  UNPROTECTED / IRREVERSIBLE

plan protection
  PARTIALLY_PROTECTED
```

The webhook cannot hide behind snapshot protection.

## I.5 Recovery after later package update

A plan changed config at 10:00. At 12:00 another package changed many `/etc` files. At 13:00 the operator wants the 10:00 state back for one config.

Ono MUST choose selective file restore if it satisfies the objective and MUST show that full root rollback would discard the 12:00 package changes.

This is a core acceptance scenario because it proves recovery is goal-oriented rather than snapshot-oriented.

---

# Appendix J. Reference Notes for Storage Semantics

The first-party providers must be implemented against current authoritative storage documentation and tested behavior rather than assumptions inherited from another filesystem.

At specification time, the relevant stable facts include:

- ZFS snapshots are point-in-time read-only dataset states and initially cheap due to copy-on-write behavior.
- Recursive ZFS snapshot creation can create snapshots for descendant datasets at one logical point, but recovery still requires reasoning about the individual datasets and rollback constraints.
- ZFS rollback may require removal of newer snapshots/bookmarks or clones depending on requested target and options; Ono must never choose destructive forms silently.
- Btrfs snapshots are subvolumes and are not recursive across nested subvolumes.
- Btrfs snapshots share the same storage failure domain and are recovery points, not independent backups.
- Btrfs root recovery is a subvolume/boot workflow rather than a generic universal in-place rollback primitive.

The implementation MUST pin tested provider assumptions in machine-readable compatibility metadata and update them when supported filesystem/tool behavior changes.
