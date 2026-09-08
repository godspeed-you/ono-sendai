---
title: "ONO-SENDAI"
subtitle: "KUANG/11 Plugin Installation, Resolution and Permission UX Specification"
author: "Project Specification"
date: "2026-09-07"
geometry: "margin=16mm"
fontsize: 11pt
colorlinks: true
linkcolor: black
urlcolor: black
toc: true
toc-depth: 3
numbersections: false
---

# 0. Document Status and Relationship to Existing Specifications

This document is the standalone **ONO-SENDAI KUANG/11 Plugin Installation, Resolution and Permission UX Specification**.

It is a cross-cutting architecture and product contract for the existing KUANG/11 extension runtime. It does **not** replace the immutable ONO-SENDAI base specification, the Spatial Systems Interface specification, the generic external-system provider specification, or provider-specific specifications. It defines the normative user-facing layer through which the already-existing KUANG/11 package lifecycle, trust model, capability broker and provider capabilities are exposed.

The repository convention places cross-cutting architecture specifications that are not tied to one numbered feature release under `docs/architecture/`. The recommended repository path for this document is therefore:

```text
docs/architecture/kuang11-plugin-installation-permissions.md
```

If the implementation records this specification as an immutable/checksummed narrative spec instead, it MAY be copied into the project's immutable spec location, but there MUST remain one canonical source.

## 0.1 Normative scope

The keywords **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, **MAY** and **RECOMMENDED** are normative within this document.

This specification defines:

- the mental model presented to users when discovering, installing and removing plugins;
- how a short plugin name resolves to a canonical package without requiring users to type reverse-DNS identifiers or source URIs in the ordinary case;
- how catalogs and explicit sources coexist;
- how package verification, publisher trust and runtime isolation are presented without conflation;
- how installation becomes a single user transaction while preserving the internal KUANG/11 lifecycle;
- the distinction between **user-facing permissions** and **internal KUANG/11 capabilities**;
- install-time recommended access profiles;
- just-in-time permission requests for conditional capabilities such as credential-helper execution;
- explicit opt-in for mutation authority;
- automatic treatment of extension-local capabilities such as bounded relationship contribution;
- the canonical permission inspection and modification interface;
- persistence, revocation, audit and upgrade behavior;
- non-interactive behavior and automation contracts;
- migration from the current `install` + `enable` + `load` + `grant capability` workflow;
- the required manifest metadata and machine-readable registries;
- security constraints specific to the current `native-process` execution tier;
- exact acceptance gates for the implementation.

This document deliberately leaves **no product-design questions open**. Implementation-specific data structures MAY vary only where they preserve all externally observable behavior and security properties defined here.

## 0.2 Current baseline

At the snapshot date of this specification, KUANG/11 already provides the following important primitives:

- reverse-DNS canonical package IDs;
- package names distinct from canonical IDs;
- package manifests and contributions;
- installation from an explicit `path:` source;
- explicit `INSTALLED`, `ENABLED`, `LOADED`, `degraded` and `quarantined` lifecycle states;
- package integrity and signature verification;
- publisher trust distinct from signature validity;
- brokered KUANG/11 capabilities with scope, duration, default deny, persistent policy and audit;
- `grant capability` and `revoke capability` as precise administrative primitives;
- process confinement for native plugins;
- a clear warning that `native-process` is **not** complete filesystem or network isolation;
- lazy plugin startup;
- contributed command/target/schema registration;
- relation contribution behind `relation.write`;
- the Kubernetes reference provider with `network.connect`, `provider.mutate`, `filesystem.read`, `secret.use`, `clock.read`, `state.persist`, `relation.write` and `process.exec` capability requirements.

These primitives are not mistakes. The problem addressed by this specification is that too many of them currently leak into the ordinary user workflow.

## 0.3 Central product thesis

The defining rule of this specification is:

> **A user grants intentions. KUANG/11 grants capabilities.**

Capabilities remain the precise security API. Permissions become the human interface to that API.

The normal user SHOULD think in statements such as:

```text
Connect to Kubernetes clusters
Read Kubernetes configuration
Run the AWS login helper when required
Change Kubernetes resources
```

The normal user SHOULD NOT have to translate those intentions into:

```text
network.connect
filesystem.read
process.exec
provider.mutate
relation.write
```

unless the user deliberately asks for technical details or is writing policy/automation.

# 1. Problem Statement

## 1.1 The current workflow exposes internal machinery

The current lifecycle is technically coherent:

```text
ABSENT -> INSTALLED -> ENABLED -> LOADED -> ACTIVE
```

The current capability system is also technically coherent:

```text
capability + plugin id + scope + duration + decision
```

The product problem arises when the ordinary installation path makes the user operate those models directly.

A workflow resembling the following is rejected as the primary experience:

```text
install plugin path:/some/long/source --confirm
set plugin io.github.example.plugin --enabled true
load plugin io.github.example.plugin
grant capability network.connect --plugin io.github.example.plugin --duration always
grant capability filesystem.read --plugin io.github.example.plugin --scope ... --duration always
grant capability relation.write --plugin io.github.example.plugin --duration always
```

This is a good administrative/debugging surface and a poor onboarding surface.

## 1.2 Capability fatigue is a security problem

A security model becomes weaker in practice when every plugin installation trains users to approve several low-level capability names they do not understand.

Repeated technical prompts create four failure modes:

1. users accept everything reflexively;
2. users deny essential permissions and conclude that the plugin is broken;
3. users grant scopes that are wider than necessary because the scope language is difficult;
4. users cannot distinguish harmless extension-local contributions from meaningful external side effects.

The solution MUST NOT be to remove fine-grained capabilities. The solution is to place a comprehensible, context-aware layer above them.

## 1.3 Package addressing is also an implementation detail

A reverse-DNS package ID is excellent for identity:

```text
io.github.godspeed-you.kubernetes
```

It is not the name a user should normally type.

Likewise, a source URI is excellent for explicit installation:

```text
path:/srv/packages/io.github.godspeed-you.kubernetes
```

It is not the ordinary discovery path.

The common path MUST use the package's human name:

```text
install plugin kubernetes
```

# 2. Goals and Non-Goals

## 2.1 Goals

The implementation MUST achieve all of the following:

1. Installing a known plugin by short name is one command.
2. The default installation flow leaves the plugin usable without a separate enable/load ceremony.
3. The user sees human-readable requested access before granting it.
4. The user can inspect the exact capabilities, scopes and policy underneath at any time.
5. Read-only access and mutation authority remain distinct.
6. Conditional permissions are requested only when their need becomes concrete.
7. Extension-local capabilities do not create needless consent prompts.
8. Non-interactive execution remains deterministic and never invents consent.
9. Signature validity, publisher trust and runtime isolation remain separate facts.
10. Native-process limitations are never hidden by friendly permission wording.
11. Upgrades cannot silently expand authority.
12. Existing low-level capability commands remain supported as an expert and automation surface.
13. Package identity remains canonical and collision-safe even when users type a short name.
14. The design scales from the Kubernetes provider to AWS, Azure, GCP and non-cloud plugins without domain-specific exceptions in core.

## 2.2 Non-goals

This specification does not:

- remove the capability broker;
- remove scope or duration from capability policy;
- abolish the internal KUANG/11 lifecycle;
- pretend that `native-process` is a complete sandbox;
- create a GUI marketplace;
- define billing, ratings or commercial distribution;
- create Kubernetes-specific core behavior;
- infer destructive intent from natural language;
- auto-grant mutation authority because a plugin is signed or first-party;
- treat a catalog entry as equivalent to publisher trust;
- require every plugin to have the same permission profile structure.

# 3. Core Invariants

Every conforming implementation MUST obey these invariants.

1. **Identity is not display name.** Short names are selectors. Canonical package IDs remain identity.
2. **Resolution happens before execution.** No plugin code may run while resolving a name, catalog entry, version or install plan.
3. **Verification precedes write.** Integrity/signature checks happen before package files become installed state.
4. **Trust is separate from signature.** A valid signature does not imply a trusted publisher.
5. **Permissions are projections.** Every user-facing permission resolves to one or more exact KUANG/11 capability requirements or to a bounded host-owned extension privilege.
6. **Capabilities remain inspectable.** No permission abstraction may hide the underlying grants from `inspect` or the advanced capability interface.
7. **No authority by wording.** Human-friendly text never grants authority by itself; only validated capability mappings do.
8. **No silent escalation.** Install, update, load or invocation MUST NOT create a broader persistent grant without explicit user consent, except the bounded extension-local privileges defined by this specification.
9. **Mutation is opt-in.** `provider.mutate` and any capability classified as mutating/destructive MUST NOT be part of the default recommended installation profile.
10. **Conditional access is contextual.** A capability such as `process.exec` SHOULD be requested when the concrete helper program and reason are known, not at install time.
11. **Non-interactive means no prompt.** If consent is required and no policy already covers it, non-interactive execution fails with a structured requirement.
12. **Native means native.** For `native-process`, permission UI MUST NOT imply that denied broker access prevents the process from direct OS access the execution tier does not isolate.
13. **The normal path is short.** A first-party or otherwise unambiguous catalog plugin MUST be installable by its short name.
14. **The expert path remains precise.** Canonical IDs, exact sources, capability IDs, scopes and durations remain accepted for administration and automation.
15. **Install is transactional.** Failure before commit leaves no partial installed package and no newly persisted permission grants.
16. **Permission changes are audited.** Both human permission decisions and generated capability grants are traceable.
17. **Upgrade authority is monotonic by consent.** Existing authority may be retained; expanded authority requires renewed consent.
18. **Removal does not create future consent.** Removed plugins do not leave grants that silently reactivate on a later reinstall.

# 4. Terminology

## 4.1 Plugin reference

A `PluginRef` is any user-supplied selector accepted by plugin commands.

Supported forms are:

```text
kubernetes                              # short name
io.github.godspeed-you.kubernetes       # canonical package id
./my-plugin                             # explicit local path
/srv/packages/my-plugin                 # explicit absolute local path
path:/srv/packages/my-plugin            # legacy explicit source form
catalog-name/kubernetes                 # disambiguated catalog selector
```

A future source resolver MAY add additional explicit source syntaxes, but short-name catalog resolution is normative now.

## 4.2 Canonical package ID

The canonical package ID remains the reverse-DNS `package.id` already used by KUANG/11.

It is immutable identity for the installed package lineage.

Example:

```text
io.github.godspeed-you.kubernetes
```

## 4.3 Short name

The short name is `package.name`.

Example:

```text
kubernetes
```

A short name is not globally unique by definition. Resolution rules handle ambiguity.

## 4.4 Permission

A **Permission** is a user-facing statement of access or behavior that maps to one or more capabilities and scopes.

Examples:

```text
Read Kubernetes configuration
Connect to Kubernetes clusters
Run external login helpers
Change Kubernetes resources
```

Permissions are package-local and have stable IDs.

## 4.5 Capability

A **Capability** is the exact KUANG/11 broker/policy primitive such as:

```text
filesystem.read
network.connect
process.exec
provider.mutate
relation.write
```

Capabilities remain the security and automation substrate.

## 4.6 Access profile

An **AccessProfile** is a named set of permission decisions presented as a coherent installation choice.

Every package that requests user-visible access MUST expose at least:

```text
recommended
minimal
```

A package MAY expose additional profiles such as `operate` when meaningful.

`recommended` MUST satisfy the safe-default requirements in this specification.

## 4.7 Extension-local privilege

An **ExtensionLocalPrivilege** is authority to contribute to Ono itself within a scope the host can fully constrain to that package and its declared contributions.

Examples may include bounded registration or relation contribution.

Extension-local privileges are distinct from authority to read/write arbitrary files, open arbitrary network connections, execute programs, use secrets or mutate external systems.

## 4.8 Just-in-time permission

A **JIT permission** is a permission whose concrete scope is known only during use.

Example:

```text
The selected kubeconfig context requires running /usr/bin/aws.
```

The prompt occurs at that moment rather than during installation.

# 5. User Mental Model

## 5.1 The ordinary vocabulary

For routine plugin use, the documentation MUST teach primarily:

```text
find plugin
install plugin
get plugin
inspect plugin
get permission
set permission
remove plugin
```

`grant capability`, `revoke capability`, `get capability`, explicit `load plugin`, explicit `unload plugin` and explicit lifecycle manipulation remain documented under advanced administration and policy.

They MUST NOT be the first-run tutorial for installing a normal plugin.

## 5.2 Canonical happy path

The target experience is:

```text
local://~ > install plugin kubernetes

Kubernetes 0.1.0
Publisher: io.github.godspeed-you
Signature: valid
Publisher trust: project-trusted
Runtime: native process

Recommended access:
  - Connect to Kubernetes clusters
  - Read Kubernetes configuration from ~/.kube/
  - Use credentials without exposing their values
  - Add Kubernetes relationships to Ono

Not granted by default:
  - Change Kubernetes resources
  - Run external login helpers (asked when needed)

Install with recommended access? [Y/n/details]
```

On `Y`:

```text
Installed kubernetes 0.1.0
Ready to use.
```

The exact typography MAY vary. The semantic content MUST not.

## 5.3 Details are one step away

Choosing `details` MUST show:

- canonical package ID;
- resolved source/catalog;
- artifact version and digest;
- signature status;
- publisher key/identity and trust source;
- runtime tier and isolation statement;
- each permission;
- each underlying capability;
- each concrete or derived scope;
- whether the decision is automatic, install-time, JIT or explicit-high-risk;
- persistence duration for the proposed decision.

No user who wants precision should lose it because the default UI is simpler.

# 6. Permission Model

## 6.1 Two-layer contract

The permission layer is not a second security engine.

The flow is:

```text
human intention
      |
      v
PermissionDecision
      |
      v
validated permission -> capability mapping
      |
      v
CapabilityGrant / bounded ExtensionLocalPrivilege
      |
      v
existing KUANG/11 policy and broker
```

The broker remains authoritative.

## 6.2 Permission descriptor

Every user-visible permission MUST be represented by a machine-readable descriptor equivalent to:

```text
PermissionDescriptor {
    id: PermissionId
    kind: PermissionKind
    title: String
    purpose: String?
    phase: PermissionPhase
    recommended: Bool
    risk: PermissionRisk
    grants: List<CapabilityGrantTemplate>
    scope_text: String?
}
```

The concrete serialization MAY be YAML in the package manifest or a referenced contribution document.

## 6.3 Stable permission IDs

Permission IDs are package-local stable slugs, for example:

```text
cluster-access
kubeconfig-read
credential-helper
cluster-mutation
spatial-relations
```

A permission ID MUST NOT be reused for materially different authority across versions.

Changing the capability mapping in a way that broadens authority constitutes an upgrade permission expansion and triggers renewed consent.

## 6.4 Host-controlled permission kinds

`PermissionKind` is host-controlled and MUST include at least:

```text
external-observe
external-change
filesystem-read
filesystem-write
execute-helper
secret-use
local-contribution
model-use
other-sensitive
```

Packages may provide titles and purposes, but the host MUST always render the host-controlled kind/risk meaning in details.

A malicious package therefore cannot label `filesystem.write` as harmless merely by providing friendly text.

## 6.5 Permission phases

`PermissionPhase` MUST support:

```text
automatic
install
jit
explicit
```

Meaning:

- `automatic`: bounded extension-local authority; no consent prompt;
- `install`: suitable for the recommended/minimal profile at installation;
- `jit`: ask only when concrete need/scope is known;
- `explicit`: never granted through the default recommended flow; user must deliberately enable it.

## 6.6 Permission risks

The host MUST derive a minimum risk from the underlying capabilities. A package MAY raise the risk but MUST NOT lower it.

Minimum categories:

```text
local
observe
sensitive-read
execute
mutate
destructive
```

`provider.mutate` has minimum risk `mutate`.

Any command already marked `risk: destructive` remains destructive even if the associated permission profile is called `operate`.

# 7. Capability Classification

The host MUST classify every KUANG/11 capability into one of the following consent classes.

## 7.1 Class A - extension-local

Authority that the host can constrain to the package's own declared contributions MAY be automatically granted.

For this specification, `relation.write` qualifies **only** when the host constrains it to:

- relations declared by that package;
- schemas/targets the package is allowed to reference;
- edges carrying that package as provenance/provider;
- existing spatial objects or package-owned projected objects accepted by the spatial contract.

A broad, unscoped relation-writing authority that could impersonate another provider MUST NOT be automatically granted.

The implementation MUST therefore either:

1. internally scope `relation.write` to the package contribution set; or
2. replace the user grant with a host-owned contribution privilege that has equivalent bounded semantics.

The user-facing result is:

```text
Add Kubernetes relationships to Ono
```

and this permission is shown as `included` rather than as a consent question.

## 7.2 Class B - recommended observation

Capabilities required for the plugin's core non-mutating purpose MAY be included in the recommended install profile if scopes are bounded and visible.

Typical examples:

```text
network.connect
filesystem.read
secret.use
clock.read
state.persist
```

Inclusion is not automatic merely because a capability belongs to this list. The package descriptor and host risk rules decide whether it belongs in `recommended`.

## 7.3 Class C - conditional/JIT

Capabilities whose concrete scope is normally unknown until use SHOULD be JIT.

Canonical example:

```text
process.exec
```

A Kubernetes installation SHOULD NOT ask for generic process execution merely because some kubeconfig contexts might use an exec credential plugin.

## 7.4 Class D - explicit mutation

Capabilities that permit changing external state are never in the default recommended profile.

Canonical example:

```text
provider.mutate
```

The user must deliberately choose an operating profile or set the permission later.

## 7.5 Class E - destructive or exceptional

A future capability that grants broad destructive authority, credential disclosure, arbitrary code execution outside a constrained helper scope, or comparable authority MUST require explicit high-risk consent and MUST NOT be enabled by `--yes`, `--confirm` or equivalent unattended acceptance of recommended access.

# 8. Manifest and Package Metadata

## 8.1 Backward-compatible extension

`kuang-package/1` manifests MUST remain parseable when the new permission metadata is absent.

A package without permission descriptors uses generated fallback permissions derived from its capability declarations. This preserves existing packages but produces less polished wording.

A future package format version is not required solely for this feature if the manifest schema already permits backward-compatible optional keys. If the current schema forbids them, the implementation MUST version the schema explicitly rather than silently accepting undeclared fields.

## 8.2 Recommended manifest shape

A package SHOULD be able to declare metadata equivalent to:

```yaml
permissions:
  profiles:
    minimal:
      title: Minimal
      permissions: [spatial-relations]

    recommended:
      title: Recommended
      permissions:
        - cluster-access
        - kubeconfig-read
        - credential-use
        - spatial-relations

    operate:
      title: Observe and change
      permissions:
        - cluster-access
        - kubeconfig-read
        - credential-use
        - spatial-relations
        - cluster-mutation

  requests:
    - id: cluster-access
      kind: external-observe
      title: Connect to Kubernetes clusters
      phase: install
      recommended: true
      grants:
        - capability: network.connect
          scope: runtime-derived

    - id: kubeconfig-read
      kind: filesystem-read
      title: Read Kubernetes configuration
      phase: install
      recommended: true
      grants:
        - capability: filesystem.read
          scope:
            paths:
              - ~/.kube/config
              - ~/.kube/*.yaml

    - id: credential-use
      kind: secret-use
      title: Use Kubernetes credentials without exposing their values
      phase: install
      recommended: true
      grants:
        - capability: secret.use

    - id: spatial-relations
      kind: local-contribution
      title: Add Kubernetes relationships to Ono
      phase: automatic
      recommended: true
      grants:
        - capability: relation.write
          scope: package-contributions

    - id: credential-helper
      kind: execute-helper
      title: Run an external login helper when a selected context requires it
      phase: jit
      recommended: false
      grants:
        - capability: process.exec
          scope: runtime-derived

    - id: cluster-mutation
      kind: external-change
      title: Change Kubernetes resources
      phase: explicit
      recommended: false
      grants:
        - capability: provider.mutate
          scope: provider-instance
```

This is conceptual syntax. The exact YAML key placement MAY be adjusted to fit the existing manifest implementation, but every semantic field above MUST be representable.

## 8.3 Package wording constraints

Permission `title` and `purpose` are untrusted package metadata.

The host MUST:

- escape terminal control sequences;
- limit length;
- refuse embedded newlines where they could spoof adjacent security text;
- render the host-owned permission kind and risk independently;
- show exact capability details on request;
- never let package text overwrite host labels such as `Signature`, `Publisher trust`, `Runtime` or `Isolation`.

# 9. Access Profiles

## 9.1 Required profiles

Any package with user-visible permission requests MUST expose:

### `minimal`

The smallest set that allows the package to be installed and contribute non-sensitive local structure. It MAY leave major features unavailable.

### `recommended`

The set expected to make the plugin useful for its primary non-mutating purpose.

`recommended` MUST NOT contain:

- `provider.mutate`;
- any permission with host-derived risk `mutate` or `destructive`;
- generic `process.exec` without a concrete program scope;
- filesystem write outside package-owned state;
- credential disclosure;
- a capability the host classifies as explicit-high-risk.

## 9.2 Optional profiles

A package MAY declare additional profiles.

For an external-system provider, the canonical optional profile is:

```text
operate
```

`operate` may add mutation authority but MUST trigger an explicit access summary before activation.

The default selection remains `recommended`.

## 9.3 No misleading profile names

Profile names are package metadata, but the host MUST append derived risk information when a profile contains mutating authority.

For example:

```text
Operate - can change external resources
```

not merely:

```text
Full
```

# 10. Plugin Name Resolution

## 10.1 Resolution order

`install plugin <PluginRef>` MUST resolve in the following order:

1. explicit local path (`./`, `../`, `/`);
2. legacy explicit `path:` URI;
3. exact canonical package ID among installed/configured catalog entries;
4. explicit `<catalog>/<short-name>` selector;
5. exact short name across enabled catalogs.

A bare short name MUST NOT be interpreted as a filesystem path unless it uses an explicit path form.

## 10.2 Unambiguous short name

If exactly one enabled catalog entry matches `package.name`, it resolves without further ceremony.

Example:

```text
install plugin kubernetes
```

resolves to:

```text
io.github.godspeed-you.kubernetes
```

## 10.3 Ambiguity

If multiple enabled catalogs provide different canonical IDs under the same short name:

- interactive mode opens a deterministic picker;
- non-interactive mode fails with `plugin.reference_ambiguous`;
- the error lists the candidate canonical IDs and catalog selectors;
- the user may retry with `<catalog>/<name>` or canonical ID.

The implementation MUST NOT silently choose by network timing, catalog order, popularity or publisher name.

## 10.4 Installed-name preference

Once a plugin is installed, commands using its short name SHOULD resolve to the installed canonical ID if that short name is unique among installed plugins.

Example:

```text
inspect plugin kubernetes
remove plugin kubernetes
get permission kubernetes
```

The user should not be forced back to the reverse-DNS ID for routine administration.

## 10.5 Upgrade pinning

After installation, upgrade identity is pinned to:

- canonical package ID;
- publisher identity/key lineage;
- selected source/catalog lineage.

A later catalog collision cannot redirect an installed plugin to a different canonical package.

# 11. Catalog Model

## 11.1 Purpose

A catalog is a signed, non-executable index that maps discoverable names to package release metadata.

It exists so that:

```text
install plugin kubernetes
```

can resolve without code execution and without requiring a source URI.

## 11.2 Catalog entry

Each entry MUST contain at least:

```text
CatalogEntry {
    canonical_id
    short_name
    description
    publisher
    releases[]
}

CatalogRelease {
    version
    platforms
    kuang_api_range
    ono_language_range
    artifact_location
    artifact_digest
    package_signature_metadata
}
```

Catalogs MAY additionally provide categories, documentation links and deprecation metadata.

## 11.3 Catalog trust is not package trust

The host MUST separately report:

```text
catalog verification
package integrity
package signature validity
publisher trust
```

A signed catalog saying that a package exists does not make the package publisher trusted.

## 11.4 Built-in bootstrap catalog

Ono MUST ship with a built-in bootstrap catalog containing first-party/reference plugins known at the core release date.

This ensures that first-run short-name installation does not depend on the user first configuring a package source.

The bootstrap catalog MAY point at network-hosted release artifacts. Its metadata is part of the Ono release and therefore covered by the Ono release integrity chain.

## 11.5 Remote catalog refresh

Ono SHOULD support a signed remote refresh of configured catalogs.

A failed refresh MUST NOT erase a previously verified catalog cache.

Catalog refresh is data-only and MUST NOT execute package code.

## 11.6 Search

`find plugin <query>` MUST search:

- installed packages;
- built-in catalog metadata;
- cached metadata of enabled remote catalogs;
- explicit local plugin paths already configured as sources.

Results MUST expose at least:

```text
NAME
ID
VERSION
SOURCE
INSTALLED
TRUST
```

The default rendering SHOULD emphasize `NAME` rather than `ID`, while `ID` remains available.

# 12. Installation Transaction

## 12.1 One user command, multiple internal steps

The canonical command is:

```text
install plugin <PluginRef>
```

The host internally performs:

```text
resolve
-> fetch/read
-> verify package structure
-> verify integrity/signature
-> evaluate publisher trust
-> validate compatibility
-> derive permission plan
-> present plan if consent is required
-> persist grants/decisions atomically with install
-> write package atomically
-> mark enabled
-> register contributions
-> make lazy activation available
-> report ready
```

The user does not need separate `set plugin --enabled true` or `load plugin` steps in the ordinary path.

## 12.2 Meaning of "ready"

`ready` is a derived UX state, not a replacement lifecycle state.

A plugin is `ready` when:

- it is installed;
- it is enabled;
- its manifest/contributions are registered;
- all required install-time permissions for the selected profile are satisfied;
- it is not quarantined;
- it may be activated according to its startup policy.

For `startup: lazy`, installation MUST NOT spawn the runtime merely to satisfy the word `ready`.

The first invocation that needs the runtime triggers normal lazy loading.

## 12.3 Lazy activation

An enabled, ready, lazy plugin MUST auto-load when the user invokes one of its contributions.

The user SHOULD NOT receive:

```text
plugin installed but not loaded; run load plugin ...
```

for a normal successfully installed plugin.

If loading fails, the invocation returns the normal structured KUANG/11 load failure and the plugin state reflects the failure.

Explicit `load plugin` remains available for debugging, prewarming and administration.

## 12.4 Atomicity

If installation fails before commit:

- no package directory is considered installed;
- no persistent permission grants created by that transaction remain;
- no enabled state remains;
- no runtime is started;
- the audit trail records the failed plan/transaction without implying success.

The package write SHOULD use a staging directory followed by atomic rename on the same filesystem.

# 13. Interactive Installation UX

## 13.1 Compact default view

The initial install view MUST answer:

1. What is this plugin?
2. Who published/signed it?
3. Is the publisher trusted?
4. What execution tier will run?
5. What meaningful access will it receive now?
6. What meaningful access will not be granted now?
7. Is there a native-process isolation caveat?

It MUST NOT dump the complete capability table by default.

## 13.2 Recommended prompt

For an ordinary trusted plugin:

```text
Kubernetes 0.1.0
Publisher: io.github.godspeed-you
Signature: valid
Publisher trust: project-trusted
Runtime: native process

Recommended access:
  - Connect to Kubernetes clusters
  - Read Kubernetes configuration from ~/.kube/
  - Use Kubernetes credentials without exposing their values
  - Add Kubernetes relationships to Ono

Asked only when needed:
  - Run an external login helper

Not granted:
  - Change Kubernetes resources

Install with recommended access? [Y/n/details]
```

## 13.3 Unknown/untrusted native publisher

For an unknown publisher using `native-process`, the warning becomes prominent:

```text
Runtime warning
This is a native plugin. It runs as your user account.
Ono can mediate brokered capabilities, but this execution tier does not prevent
direct filesystem or network access available to your user.

Publisher trust: unknown
```

Installation requires an explicit affirmative response. A generic `--yes` MUST NOT bypass this first-trust warning unless a policy has explicitly allowed that publisher/source for unattended native installation.

## 13.4 Quarantined or revoked publisher

A revoked publisher or invalid package MUST NOT be installable through an ordinary confirmation.

The host refuses with the existing trust/integrity error family.

There is no `install anyway` shortcut for a cryptographically invalid package.

Local unsigned development packages remain supported under the existing local-development trust semantics, with a clear warning.

# 14. Just-in-Time Permissions

## 14.1 Principle

Ono SHOULD ask for a permission when it can explain the exact need.

This is particularly important for `process.exec`.

## 14.2 Kubernetes credential helper example

When a selected kubeconfig context contains an exec credential provider:

```text
local://~ > get k8s-pod --context prod

Kubernetes needs to run:
  /usr/bin/aws eks get-token ...

Reason:
  authenticate to Kubernetes context "prod"

Allow this helper?
  [o] once
  [s] this session
  [a] always for this program
  [n] deny
  [d] details
```

The exact command line MAY redact secret arguments where appropriate, but MUST identify the executable and purpose.

## 14.3 JIT grant scope

Selecting `always for this program` MUST create the narrowest enforceable persistent scope, including at minimum:

- package canonical ID;
- capability `process.exec`;
- executable identity/path or validated program selector;
- applicable provider/plugin context when supported;
- decision duration.

The host MUST NOT translate `always for this program` into unrestricted `process.exec` for the package.

## 14.4 Once and session semantics

`once` applies to one attempted operation and expires immediately after that operation completes or fails.

`session` persists only for the Ono session.

`always` persists in the policy store.

The user-facing choices hide the word `duration` unless details are opened.

## 14.5 Denial

Denial MUST fail the operation with a structured error that states:

- which user-facing permission was denied;
- which capability remained unavailable;
- what operation triggered the need;
- how to inspect/change the decision later.

The plugin MUST NOT interpret denial as an empty result.

# 15. Mutation Permission

## 15.1 Read-only by default

An external-system provider's recommended profile MUST remain read-only with respect to the external system whenever the provider exposes a dedicated mutation capability such as `provider.mutate`.

For Kubernetes, recommended installation therefore grants/permits observation access but not `provider.mutate`.

## 15.2 Enabling operation

The user may deliberately enable mutation through the permission layer:

```text
set permission kubernetes --profile operate
```

or by changing the specific permission:

```text
set permission kubernetes cluster-mutation --decision allow
```

Before persisting a mutating permission interactively, Ono MUST summarize the effect:

```text
Kubernetes will be allowed to change resources in the clusters covered by its
network/provider scope. Individual mutate/destructive commands still keep their
own risk confirmation and dry-run behavior.

Allow changes? [y/N/details]
```

## 15.3 Defense in depth remains

Permission to mutate does not remove existing command safety.

A Kubernetes mutation still requires, as applicable:

- `network.connect`;
- `provider.mutate`;
- command-level `risk: mutate` or `risk: destructive` handling;
- dry-run defaults;
- provider preconditions and bounded operation semantics.

Permission is authority, not an instruction to perform a mutation.

# 16. Canonical Permission Commands

## 16.1 `get permission`

The canonical user-facing inspection target is `permission`.

Syntax:

```text
get permission
get permission <plugin-ref>
get permission <plugin-ref> --all
get permission <plugin-ref> --json
```

Default output for one plugin SHOULD resemble:

```text
PERMISSION                 STATE    WHEN        SCOPE
Connect to clusters        allowed  always      selected/approved endpoints
Read Kubernetes config     allowed  always      ~/.kube/config, ~/.kube/*.yaml
Use credentials            allowed  always      brokered handles
Add Ono relationships      included automatic   package contributions
Run login helper           ask      when-needed  exact program
Change resources           denied   explicit     provider instance
```

`--all` includes underlying capability IDs and policy source.

## 16.2 `set permission`

Syntax:

```text
set permission <plugin-ref> --profile <minimal|recommended|...>
set permission <plugin-ref> <permission-id> --decision <allow|deny|ask>
set permission <plugin-ref> <permission-id> --decision allow --duration <once|session|always>
```

Interactive use SHOULD permit shorter natural choices through completion and prompts.

`duration` remains available for precision but is not required in ordinary guided flows.

## 16.3 Advanced capability surface

Existing commands remain supported:

```text
get capability
grant capability
revoke capability
```

They are the canonical advanced/policy interface.

A grant created through `set permission` MUST appear in `get capability` with metadata identifying the originating permission decision.

A capability grant created manually MAY cause the corresponding permission to render `custom` if it does not exactly match a known profile mapping.

## 16.4 No new `permissions` verb

This specification deliberately does **not** introduce a `permissions` command verb.

Ono's existing verb-target language remains intact. Human-friendly UX is achieved through the `permission` target and guided install/JIT interactions, not by creating a second command grammar.

# 17. Internal Lifecycle vs User Lifecycle

## 17.1 Internal state remains

The internal state machine remains conceptually:

```text
ABSENT
  -> INSTALLED
  -> ENABLED
  -> LOADED
  -> ACTIVE
```

with `degraded` and `quarantined` as already defined.

## 17.2 Derived user states

User-facing summaries MAY add derived labels:

```text
ready
needs-permission
blocked
running
```

These labels are projections, not replacements for canonical state fields.

`inspect plugin --all` MUST expose the actual internal state.

## 17.3 Install orchestration

A successful ordinary `install plugin` ends with:

```text
installed = true
enabled = true
ready = true
```

For lazy runtime packages:

```text
loaded = false
```

is valid until first use.

This distinction MUST NOT be presented as an error.

## 17.4 Explicit disabling

A user who deliberately disables a plugin through:

```text
set plugin kubernetes --enabled false
```

has overridden the normal ready behavior.

Invocation MUST then refuse and explain that the plugin is disabled.

No auto-load may override an explicit disable.

# 18. Trust, Verification and Isolation UX

## 18.1 Four separate questions

The UI MUST distinguish:

1. **Integrity** - are the package bytes intact?
2. **Signature** - do the bytes match a cryptographic signer?
3. **Publisher trust** - does policy trust that signer/publisher?
4. **Isolation** - what can the runtime be prevented from doing outside the broker?

These facts MUST never collapse into one green `verified` label.

## 18.2 Native-process warning

For `native-process`, the permission screen MUST include or link directly to this semantic statement:

> This plugin runs as the Ono user. KUANG/11 mediates brokered host capabilities and applies process confinement, but this execution tier is not complete filesystem or network isolation.

The wording MAY be shortened in the compact view but the meaning MUST remain.

## 18.3 Future isolated tiers

If `native-isolated` or a WASM/component tier later enforces filesystem/network isolation, the same permission UX may report a stronger boundary.

The permission architecture MUST therefore carry an `enforcement` field sufficient to render, for each permission/capability:

```text
brokered
kernel-enforced
runtime-enforced
advisory
```

The UI MUST not claim stronger enforcement than actually exists.

# 19. Scope UX

## 19.1 Scopes remain precise internally

Capability scopes remain structured machine data.

The permission layer renders them into human language.

Examples:

```text
{paths: ["~/.kube/config", "~/.kube/*.yaml"]}
```

becomes:

```text
~/.kube/config and Kubernetes YAML files under ~/.kube/
```

## 19.2 Details preserve exact structure

`details`, `inspect plugin` and `get permission --all` MUST expose the exact structured scope alongside the friendly rendering.

## 19.3 Runtime-derived scopes

Some scopes cannot be known at package-build time.

Kubernetes examples include:

- the API host selected by a kubeconfig context;
- the credential helper executable named by that context.

The permission model MUST support `runtime-derived` scope templates.

A runtime-derived scope is not a wildcard. It means:

> the host derives the concrete scope from validated invocation/configuration data at the time of use and grants only that concrete value.

If the broker cannot enforce the derived scope, the UI MUST state the weaker enforcement honestly.

# 20. Non-Interactive and Automation Behavior

## 20.1 No implicit consent

In `ono -c`, scripts, CI, redirected stdin or any mode where interaction is unavailable, Ono MUST NOT invent answers to permission prompts.

## 20.2 Installing recommended access non-interactively

The canonical unattended form is:

```text
install plugin kubernetes --access recommended --confirm
```

or an equivalent structured automation argument chosen by the existing CLI conventions.

Normative semantics:

- `--confirm` confirms the resolved install plan;
- `--access recommended` selects only the safe recommended profile;
- neither option may grant an explicit/high-risk permission;
- unknown/untrusted native publishers still require pre-established trust policy.

The existing `--confirm` flag SHOULD remain compatible.

## 20.3 Missing JIT permission

If a non-interactive invocation reaches a JIT need with no existing policy, it fails with a structured error equivalent to:

```text
permission.required
plugin: io.github.godspeed-you.kubernetes
permission: credential-helper
capability: process.exec
requested_scope.program: /usr/bin/aws
reason: authenticate to Kubernetes context "prod"
```

The error MUST provide a deterministic remediation using either `set permission` or `grant capability`.

## 20.4 Policy-as-code remains capability-level

For machine-managed fleets, policy files MAY continue to use exact capability IDs and scopes.

The human permission layer does not replace low-level policy-as-code.

A future machine-readable permission policy MAY be added, but it MUST compile to the same capability policy model.

# 21. Upgrades

## 21.1 Identity continuity

An upgrade MUST verify:

- same canonical package ID;
- acceptable publisher identity/key lineage;
- compatible package/runtime format;
- integrity/signature of the new artifact.

## 21.2 No-new-permission upgrade

If the new version requests no broader authority than currently approved, the upgrade MAY retain existing decisions without a new prompt.

The final result SHOULD state:

```text
Updated kubernetes 0.1.0 -> 0.1.1
Permissions unchanged.
```

## 21.3 Permission expansion

An upgrade is permission-expanding if any of the following occurs:

- a new user-visible permission is added to the selected profile;
- a permission adds a new capability;
- a scope becomes broader;
- a permission risk increases;
- a permission changes from JIT/explicit to install-time automatic/recommended;
- an enforcement boundary becomes weaker;
- an existing permission ID is repurposed.

The host MUST show the delta and request consent before committing the upgrade.

Example:

```text
Kubernetes 0.2.0 requests additional access:
  + Read ~/.config/cloud/**

Existing access is unchanged.
Update and grant the new access? [y/N/details]
```

Declining MUST leave the existing installed version intact.

## 21.4 New mutation authority

An upgrade MUST NEVER silently add `provider.mutate` or equivalent mutation authority, even if the package marks it recommended.

The host rejects such a recommended profile definition as invalid or downgrades it to explicit according to host policy.

# 22. Removal and Reinstallation

## 22.1 Default removal

`remove plugin <ref>` removes:

- package files;
- enabled state;
- plugin-specific persistent permission decisions/grants;
- plugin-owned cached runtime state according to existing removal policy;
- catalog pinning for that installed instance.

The audit log remains.

## 22.2 Keep-permission escape hatch

An advanced explicit option MAY retain policy for fleet/provisioning workflows, for example:

```text
remove plugin kubernetes --keep-permissions
```

This MUST NOT be the default.

## 22.3 Reinstall

A normal reinstall after removal requests permission again.

No old grant may silently resurrect merely because the new package has the same short name.

If permissions were explicitly retained, they apply only when canonical package ID and publisher identity satisfy the stored policy binding.

# 23. Permission Persistence and Audit

## 23.1 Policy store

Existing capability policy persistence remains authoritative.

Permission decisions require additional metadata linking human intent to generated grants.

Conceptually:

```text
PermissionDecisionRecord {
    plugin_id
    permission_id
    decision
    duration
    profile_source?
    created_at
    updated_at
    capability_grant_ids[]
    package_version_at_consent
    publisher_identity_at_consent
}
```

## 23.2 Audit events

The audit stream MUST distinguish:

```text
permission.allow
permission.deny
permission.ask
permission.profile_apply
capability.grant
capability.revoke
```

A single user action MAY generate both a permission event and one or more capability-grant events.

They MUST be correlated by transaction/request ID.

## 23.3 Explainability

Given any active capability grant, the system MUST be able to answer whether it came from:

- a user-facing permission decision;
- a selected access profile;
- a manual `grant capability` command;
- system policy;
- user policy;
- session-only JIT consent.

# 24. Error Model

The implementation MUST add structured errors equivalent to the following semantic cases. Exact numeric Ono error codes are assigned according to the project's existing registry conventions.

## 24.1 Resolution

```text
plugin.not_found
plugin.reference_ambiguous
plugin.catalog_unavailable
plugin.release_not_compatible
```

## 24.2 Permission

```text
permission.required
permission.denied
permission.invalid_profile
permission.invalid_mapping
permission.scope_unavailable
permission.escalation_requires_confirmation
```

## 24.3 Trust

Existing package invalid/signature revoked/integrity invalid errors SHOULD be reused rather than duplicated.

## 24.4 Error wording

User-facing errors SHOULD lead with the human permission and reason, then show the capability in details.

Preferred:

```text
Kubernetes cannot run the AWS login helper because permission to run that helper
is denied.

details: capability process.exec, program /usr/bin/aws
```

Rejected primary wording:

```text
K11301 process.exec not granted
```

The low-level code remains present for automation and diagnosis.

# 25. Discovery, Help and Completion

## 25.1 Completion

Completion for:

```text
install plugin <TAB>
```

SHOULD show short names from installed/catalog metadata first.

Canonical IDs appear as secondary detail.

## 25.2 Help

`help install plugin` MUST teach the short-name flow first.

`help permissions` or the appropriate help topic MUST explain the permission/capability distinction.

`help capabilities` remains the advanced exact reference.

## 25.3 Inspect

`inspect plugin <ref>` MUST include:

- display name;
- canonical ID;
- version;
- source/catalog;
- verification/trust;
- internal lifecycle state;
- derived readiness;
- runtime tier and isolation;
- requested permissions;
- effective permission decisions;
- exact capability grants;
- contributions;
- runtime health if loaded.

# 26. Kubernetes Reference Mapping

The Kubernetes provider is the acceptance reference for this specification because it exercises observation, secrets, local contributions, conditional helper execution and mutation.

## 26.1 Required mapping

The provider's permission model MUST be equivalent to:

| User-facing permission | Capability/privilege | Phase | Default |
|---|---|---|---|
| Connect to Kubernetes clusters | `network.connect` | install | allow in recommended |
| Read Kubernetes configuration | `filesystem.read` scoped to kubeconfig paths | install | allow in recommended |
| Use Kubernetes credentials | `secret.use` | install | allow in recommended |
| Add Kubernetes relationships to Ono | bounded `relation.write` / local contribution privilege | automatic | included |
| Maintain plugin state / read clock where required | `state.persist`, `clock.read` | automatic or install-hidden support permission according to risk | included when host deems non-sensitive |
| Run external login helper | `process.exec` scoped to concrete helper | JIT | ask |
| Change Kubernetes resources | `provider.mutate` | explicit | deny |

## 26.2 Recommended install result

After:

```text
install plugin kubernetes
```

with recommended access accepted, this MUST work without additional relation grants:

```text
get k8s-pod --context prod
get k8s-pod --context prod | take 1 | enter
near
```

provided the context does not require an unapproved exec helper and network/filesystem/credential access succeeds.

## 26.3 Exec credential context

If the same command selects an EKS/GKE/AKS-style exec credential context, the first use prompts for the concrete helper.

The user was not asked for generic `process.exec` during install.

## 26.4 Mutation

Immediately after recommended installation:

```text
set k8s-resource ...
```

must fail or offer a deliberate permission elevation flow because `provider.mutate` is not granted.

It MUST NOT be silently enabled just because the plugin's package was accepted.

## 26.5 Operate profile

After:

```text
set permission kubernetes --profile operate
```

and explicit mutation confirmation, bounded Kubernetes mutation commands may proceed subject to their existing risk and dry-run rules.

# 27. Native-Process Security Constraint

## 27.1 Permission UI must not imply a sandbox that does not exist

This is a critical acceptance requirement.

The current native execution tier runs the plugin as the Ono user and does not provide complete filesystem/network kernel isolation.

Therefore this wording is forbidden:

```text
This plugin can only access ~/.kube/config.
```

unless an execution tier actually enforces that statement.

The correct semantic wording is:

```text
Ono grants this plugin brokered read access to ~/.kube/config.
This native execution tier is not complete filesystem isolation.
```

## 27.2 Why permission UX still matters

The limitation does not make permissions useless.

Permissions still govern:

- brokered host calls;
- host-provided secrets;
- provider mutation authorization;
- contributed relations and host model integration;
- audited process helper execution through the broker;
- future stronger execution tiers.

The UI must simply be honest about the boundary.

# 28. Migration from Existing KUANG/11 UX

## 28.1 Existing commands remain valid

The following remain supported:

```text
install plugin path:...
load plugin <id>
unload plugin <id>
set plugin <id> --enabled ...
grant capability ...
revoke capability ...
get capability ...
```

This specification changes what the project teaches and adds orchestration; it does not gratuitously break automation.

## 28.2 `install plugin ... --confirm`

Existing explicit-source installation with `--confirm` remains accepted.

For packages with permission descriptors, `--confirm` confirms the install plan but MUST NOT imply acceptance of explicit-high-risk permissions.

If access selection is not supplied non-interactively, the safe behavior is:

- apply `recommended` only if all its permissions satisfy the safe-default rules and policy allows unattended selection;
- otherwise fail with `permission.required` and a remediation.

## 28.3 Existing grants

On upgrade to a core implementing this specification, existing capability grants are preserved.

The host attempts to project them into known permissions.

Possible states:

```text
matched
custom
legacy
```

No grant is revoked merely because it cannot be mapped to a friendly permission.

## 28.4 Kubernetes migration

If an existing Kubernetes installation already has:

```text
network.connect
filesystem.read
secret.use
relation.write
process.exec
provider.mutate
```

grants, `get permission kubernetes` should represent them as permission decisions where possible.

A broad legacy `process.exec` grant that does not match the new narrow JIT scope MUST render `custom` and SHOULD trigger a recommendation to narrow it, but MUST NOT be silently rewritten.

# 29. Machine-Readable Contracts

The implementation MUST add or extend registries so that documentation, help, completion and tests derive from the same data.

At minimum there MUST be machine-readable schemas for:

```text
PermissionDescriptor
PermissionDecision
PermissionProfile
PermissionRisk
PermissionPhase
PluginResolution
CatalogEntry
InstallPlan
InstallPlanPermissionDelta
```

## 29.1 Install plan

The existing install plan SHOULD be extended to contain:

```text
InstallPlan {
    resolved_package
    verification
    trust
    compatibility
    runtime_tier
    isolation
    selected_profile
    permissions[]
    automatic_privileges[]
    denied_explicit_permissions[]
    writes[]
}
```

## 29.2 Structured output

`install plugin ... | to json` or the equivalent structured command result MUST not depend on terminal prompt wording.

Interactive questions are presentation over the structured plan.

# 30. Performance and Offline Behavior

## 30.1 Local discovery first

`find plugin` and short-name completion SHOULD answer from local/built-in/cached catalog metadata without blocking on network refresh.

## 30.2 Installation network work

Network fetch occurs only after resolution selects a release that is not already locally available.

## 30.3 Catalog cache

A verified catalog cache MUST remain usable when offline.

Offline installation may succeed if the required artifact is cached or the source is local.

## 30.4 No runtime startup during discovery

`find plugin`, name resolution, install planning, verification and permission rendering MUST NOT start plugin runtime code.

# 31. Documentation Requirements

The implementation is incomplete until documentation changes with it.

The project MUST update:

- the KUANG/11 wiki page;
- `help install plugin`;
- plugin trust/isolation help;
- capability help;
- migration documentation;
- the Kubernetes provider README install/use examples;
- SDK/package-author documentation for permission descriptors and profiles.

## 31.1 Documentation order

The first plugin tutorial MUST show:

```text
install plugin kubernetes
```

not a reverse-DNS ID, `path:` source and sequence of capability grants.

## 31.2 Advanced reference remains

The low-level capability commands remain fully documented in an advanced/reference section because they are essential for exact policy and debugging.

# 32. Implementation Phases

The implementation SHOULD proceed in this order. A coding agent MUST NOT stop after only the visible prompt changes.

## Phase 1 - permission domain model

Implement:

- permission descriptors;
- permission kinds/phases/risks;
- profile validation;
- capability mapping;
- decision persistence metadata;
- audit correlation.

## Phase 2 - capability classification and bounded local contributions

Implement:

- host capability risk classification;
- automatic bounded local-contribution treatment;
- package-scoped relationship contribution;
- rejection of misleading/unsafe automatic mappings.

## Phase 3 - resolution and catalog bootstrap

Implement:

- short-name resolution;
- canonical ID resolution;
- ambiguity handling;
- built-in bootstrap catalog;
- cached catalog model;
- `find plugin` integration;
- install source pinning.

## Phase 4 - transactional install orchestration

Implement:

- resolve/fetch/verify/permission/install/enable transaction;
- rollback;
- ready projection;
- lazy auto-load on first invocation.

## Phase 5 - interactive permission UX

Implement:

- compact install summary;
- details view;
- unknown-native warning;
- profile selection;
- `get permission`;
- `set permission`.

## Phase 6 - JIT permissions

Implement:

- permission request from runtime/broker path;
- once/session/always choices;
- narrow runtime-derived scopes;
- non-interactive `permission.required`.

## Phase 7 - upgrade/removal/migration

Implement:

- permission delta detection;
- no-silent-escalation upgrade transaction;
- grant cleanup on removal;
- legacy grant projection.

## Phase 8 - reference provider conversion

Update Kubernetes package metadata and tests so it becomes the canonical proof of the model.

## Phase 9 - documentation and release gates

Update all generated/manual docs and add acceptance gates to the project-wide gate scripts.

# 33. Acceptance Criteria

A conforming implementation MUST satisfy every gate below.

## Gate A - short-name installation

Given a clean Ono installation with the built-in catalog:

```text
install plugin kubernetes
```

resolves the canonical Kubernetes package without requiring reverse-DNS ID or source URI.

## Gate B - no code during resolution

Catalog search, resolution and install planning execute no plugin runtime bytes.

## Gate C - one-step readiness

After successful interactive installation with recommended access:

- package is installed;
- package is enabled;
- package is ready;
- user does not need a separate `load plugin` command;
- lazy runtime is not unnecessarily spawned during installation.

## Gate D - human permission rendering

The default install prompt contains human descriptions and does not require the user to understand capability IDs.

## Gate E - details preserve precision

`details` exposes exact capabilities, scopes, durations and verification/trust/isolation facts.

## Gate F - read-only default

The Kubernetes recommended profile does not grant `provider.mutate`.

A mutation attempt before elevation cannot change the cluster.

## Gate G - relations work without manual grant ceremony

Recommended Kubernetes installation allows its declared bounded spatial relations to appear without the user manually granting `relation.write`.

## Gate H - JIT process execution

A Kubernetes context not using exec credentials never asks for `process.exec`.

A context using `/usr/bin/aws` asks at first use and identifies that exact helper.

## Gate I - narrow JIT persistence

Choosing `always for this program` does not create unrestricted `process.exec` for the plugin.

## Gate J - non-interactive refusal

A script requiring a missing JIT permission fails deterministically with structured `permission.required`; it never waits for input or assumes consent.

## Gate K - advanced capability compatibility

Existing `grant capability`, `revoke capability` and `get capability` continue to work.

## Gate L - manual grant projection

A manually created capability grant appears as a matched or `custom` permission state rather than disappearing from the user-facing model.

## Gate M - native isolation honesty

The installation UI for a native plugin clearly states that broker permissions do not constitute complete filesystem/network isolation.

## Gate N - upgrade without escalation

An upgrade whose requested authority is unchanged can proceed without redundant consent.

## Gate O - upgrade with escalation

An upgrade that broadens capability or scope cannot commit without renewed permission consent.

## Gate P - remove cleans grants

Normal `remove plugin kubernetes` removes its persistent permission/grant state such that reinstall does not silently inherit it.

## Gate Q - ambiguity is deterministic

Two catalogs containing different packages named `example` produce a picker interactively and `plugin.reference_ambiguous` non-interactively.

## Gate R - canonical ID remains available

Every command accepting a short name also accepts the canonical package ID.

## Gate S - trust facts remain separate

Tests prove that `signature: valid`, `publisher trust: unknown` is representable and not rendered as one generic verified/trusted status.

## Gate T - invalid signature cannot be confirmed away

A tampered package is refused before installation regardless of ordinary confirmation flags.

## Gate U - transaction rollback

Injected failures at each pre-commit stage leave neither partial package state nor new persistent grants.

## Gate V - audit correlation

One permission decision and its generated capability grants share a correlation identifier visible in structured audit output.

## Gate W - help teaches the right path

Generated/manual help teaches `install plugin kubernetes` as the primary example and low-level capability grants as advanced policy.

## Gate X - provider implementation has no core domain exception

Kubernetes-specific wording/permission descriptors reside in the Kubernetes package metadata. Core contains generic permission kinds and rendering only.

# 34. Security Tests

Beyond functional acceptance, the implementation MUST test the following adversarial cases.

## 34.1 Misleading permission text

A package declaring:

```text
title: "Harmless theme access"
```

while mapping to `filesystem.write` still renders the host-owned write risk and cannot downgrade it.

## 34.2 Terminal escape injection

Package title/purpose/catalog text containing ANSI/control sequences cannot alter surrounding security UI.

## 34.3 Catalog name takeover

A new catalog entry with short name `kubernetes` cannot redirect upgrades of an already installed canonical Kubernetes package.

## 34.4 Publisher substitution

Same canonical ID signed by an unrelated publisher key is rejected or requires the project's explicit key-rotation trust process; it is not treated as a normal update.

## 34.5 Permission-ID reuse

An upgrade that reuses `kubeconfig-read` but changes it from `filesystem.read ~/.kube/**` to `filesystem.read ~/**` is detected as scope expansion and requires new consent.

## 34.6 Hidden mutation

A profile marked `recommended` that contains `provider.mutate` is rejected as unsafe regardless of package metadata.

## 34.7 Broad helper grant

A package may not turn a JIT request for `/usr/bin/aws` into persistent unrestricted process execution through profile metadata.

## 34.8 Stale removed grants

Deleting and reinstalling the package does not reactivate removed grants unless the user explicitly retained them.

# 35. Rejected Alternatives

## 35.1 Teach users capability names

Rejected.

Capability names are intentionally implementation/security vocabulary. Teaching them as the primary UX does not solve permission fatigue.

## 35.2 Grant every declared capability on install

Rejected.

It destroys least privilege and makes optional/helper/mutation capabilities indistinguishable.

## 35.3 Ask about every capability on install

Rejected.

It produces consent fatigue and asks questions before the user understands their context.

## 35.4 Replace capabilities with permissions

Rejected.

Permissions are not precise enough for broker enforcement, machine policy, scopes, duration and audit.

## 35.5 Make `relation.write` a normal security prompt

Rejected for package-bounded declared relations.

A plugin installed specifically to integrate its objects into Ono should not require a second cryptic consent merely to contribute relations the host can already constrain to that plugin.

Broad relation impersonation remains forbidden.

## 35.6 Auto-grant `process.exec` because cloud providers need it

Rejected.

Many contexts do not need it, and the exact helper is knowable at use time. JIT consent is both safer and clearer.

## 35.7 Auto-grant mutation to first-party plugins

Rejected.

Publisher trust and operational authority answer different questions.

## 35.8 Remove lifecycle states

Rejected.

The internal lifecycle is useful for lazy startup, disablement, quarantine, health and debugging. The problem is ceremony, not the state model.

## 35.9 Introduce a second plugin command grammar

Rejected.

Ono already has a controlled verb-target language. The design uses `install plugin`, `get permission`, `set permission`, `inspect plugin` and existing advanced capability verbs instead of a parallel package-manager mini-shell.

# 36. Complete Kubernetes UX Example

## 36.1 Discovery

```text
local://~ > find plugin kubernetes

NAME        VERSION  PUBLISHER                INSTALLED  SOURCE
kubernetes  0.1.0    io.github.godspeed-you   no         official
```

## 36.2 Installation

```text
local://~ > install plugin kubernetes

Kubernetes 0.1.0
Publisher: io.github.godspeed-you
Signature: valid
Publisher trust: project-trusted
Runtime: native process

Recommended access:
  - Connect to Kubernetes clusters
  - Read Kubernetes configuration from ~/.kube/
  - Use Kubernetes credentials without exposing their values
  - Add Kubernetes relationships to Ono

Asked only when needed:
  - Run an external login helper

Not granted:
  - Change Kubernetes resources

Install with recommended access? [Y/n/details]
> y

Installed kubernetes 0.1.0
Ready to use.
```

## 36.3 Ordinary token/certificate context

```text
local://~ > get k8s-pod --context local --namespace shop | take 2

NAME         NAMESPACE  PHASE
api-7d9f     shop       Running
worker-58bc  shop       Running
```

No helper permission prompt appears.

## 36.4 Spatial use

```text
local://~ > get k8s-pod --context local --namespace shop | take 1 | enter
k8s://local/shop/pod/api-7d9f > near

RELATION      OBJECT
scheduled-on  node/node-a
owned-by      replicaset/api-7d9f
runs-as       serviceaccount/api
```

No manual `relation.write` grant was required.

## 36.5 Exec-auth context

```text
local://~ > get k8s-pod --context eks-prod --namespace shop

Kubernetes needs to run:
  /usr/bin/aws eks get-token --cluster-name prod

Reason:
  authenticate to Kubernetes context "eks-prod"

Allow this helper?
  [o] once  [s] session  [a] always for this program  [n] deny  [d] details
> a

Allowed /usr/bin/aws for Kubernetes.
```

The command then continues.

## 36.6 Inspect permissions

```text
local://~ > get permission kubernetes

PERMISSION                  STATE    WHEN         SCOPE
Connect to clusters         allowed  always       approved endpoints
Read Kubernetes config      allowed  always       ~/.kube/config, ~/.kube/*.yaml
Use credentials             allowed  always       brokered handles
Add Ono relationships       included automatic    package contributions
Run external login helper   allowed  always       /usr/bin/aws
Change Kubernetes resources denied   explicit      provider instance
```

## 36.7 Attempt mutation before permission

```text
local://~ > set k8s-resource --context eks-prod --kind Deployment --name api --replicas 2

Kubernetes is installed with read-only external access.
Changing Kubernetes resources is not allowed.

Enable the "Change Kubernetes resources" permission to continue.
```

No cluster change occurs.

## 36.8 Enable operate profile

```text
local://~ > set permission kubernetes --profile operate

This profile adds permission to change Kubernetes resources.
Individual mutate/destructive commands keep their own confirmations and dry-run rules.

Allow changes? [y/N/details]
> y

Kubernetes access profile: operate
```

## 36.9 Advanced inspection

```text
local://~ > get permission kubernetes --all
```

shows mappings such as:

```text
cluster-access     -> network.connect
kubeconfig-read    -> filesystem.read {paths:[...]}
credential-helper  -> process.exec {programs:["/usr/bin/aws"]}
cluster-mutation   -> provider.mutate
spatial-relations  -> relation.write {package-contributions}
```

The expert interface still works:

```text
get capability --plugin io.github.godspeed-you.kubernetes
```

# 37. Product Consequence

This specification intentionally makes the visible plugin system smaller while making the implementation model richer.

The user learns:

```text
find
install
use
inspect when curious
allow something when Ono can explain why
remove
```

The implementation keeps:

```text
canonical identity
sources and catalogs
signatures and publisher trust
lifecycle states
lazy loading
capability negotiation
scopes
durations
policy persistence
audit
provider mutation boundaries
process confinement
runtime isolation tiers
```

That separation is the point.

KUANG/11 should feel like software loaded into the deck, not like a security framework the user must configure before the software becomes useful.

The final design rule is therefore repeated as the closing requirement:

> **A user grants intentions. KUANG/11 grants capabilities.**
