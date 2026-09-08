---
title: "ONO-SENDAI"
subtitle: "KUANG/11 Plugin Package Acquisition and System Distribution Addendum"
author: "Project Specification Addendum"
date: "2026-09-08"
geometry: "margin=16mm"
fontsize: 11pt
colorlinks: true
linkcolor: black
urlcolor: black
toc: true
toc-depth: 3
numbersections: false
---

# 0. Document Status and Relationship to the Base Plugin UX Specification

This document is a **normative additive specification** to the existing **ONO-SENDAI KUANG/11 Plugin Installation, Resolution and Permission UX Specification**.

It does **not** replace, revise, invalidate or reopen the decisions of that specification. The base specification remains authoritative for:

- plugin identity and short-name resolution;
- the distinction between permissions and capabilities;
- install-time permission profiles;
- JIT permission requests;
- capability enforcement and audit;
- publisher trust and package verification;
- lifecycle behavior (`INSTALLED`, `ENABLED`, `LOADED`, degraded, quarantined);
- transactional installation;
- upgrade permission escalation;
- removal and migration behavior;
- Kubernetes as the first reference provider.

This addendum fills one deliberately separate concern that was not fully specified before implementation started: **how a plugin package physically reaches a host and how Ono integrates with operating-system package managers without weakening KUANG/11 security semantics**.

The recommended repository path is:

```text
docs/architecture/kuang11-plugin-package-acquisition-system-distribution.md
```

The original plugin installation and permission specification MUST remain unchanged when this addendum is added. Implementations MUST satisfy both documents.

## 0.1 Normative language

The keywords **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, **MAY** and **RECOMMENDED** are normative.

## 0.2 Central rule

The defining rule of this addendum is:

> **Package acquisition may be delegated. Trust, permission and activation decisions may not.**

In particular, installation by `apt`, `dpkg`, `dnf`, `yum`, `rpm` or another privileged operating-system mechanism MUST NOT automatically confer KUANG/11 trust or capabilities.

# 1. Problem Statement

The base specification already defines a concise user flow such as:

```text
install plugin kubernetes
```

and a catalog model that can resolve a short name to a signed release artifact. It also retains explicit local paths such as:

```text
install plugin ./my-plugin
install plugin /srv/packages/my-plugin
install plugin path:/srv/packages/my-plugin
```

That leaves an important deployment question open for Linux and enterprise environments:

- Can the plugin be downloaded directly by Ono from the network?
- Can it be installed from local media?
- Can a Linux distribution or enterprise repository deliver it as a `.deb` or `.rpm`?
- What happens when `apt upgrade` or `dnf upgrade` changes the package?
- Does `root` installing a package imply plugin trust?
- Can the system package grant permissions or enable the plugin?
- Which source wins when both the Ono catalog and the OS package manager provide the same plugin?
- Can the entire workflow operate offline?

This addendum answers those questions without turning KUANG/11 into a distribution-specific package manager.

# 2. Goals

This addendum MUST enable all of the following deployment models:

1. **Ono catalog / network acquisition**

   ```text
   install plugin kubernetes
   ```

   Ono resolves a catalog entry, downloads or reads the referenced KUANG/11 release artifact, verifies it, asks for required consent, and installs it transactionally.

2. **Explicit local acquisition**

   ```text
   install plugin ./kubernetes.kuang
   install plugin /srv/packages/kubernetes/
   install plugin path:/srv/packages/kubernetes/
   ```

   Ono obtains the same logical KUANG/11 package from local storage.

3. **Operating-system package acquisition**

   ```text
   apt install ono-plugin-kubernetes
   dnf install ono-plugin-kubernetes
   ```

   The OS package manager places a KUANG/11 package payload into a well-defined system-provided source area. Ono then discovers and activates that payload under its normal trust and permission model.

4. **Offline deployment**

   A plugin delivered by removable media, local package repository, internal APT/DNF repository, image build, configuration-management system or pre-provisioned filesystem MUST be usable without contacting an external Ono catalog.

5. **Enterprise-controlled provenance**

   An administrator MUST be able to make an OS-managed package source authoritative for a plugin so that Ono does not silently bypass enterprise package selection by downloading a different build from the public catalog.

# 3. Non-Goals

This addendum does not:

- make Ono a replacement for APT, DNF, RPM, dpkg, Nix, Homebrew or other system package managers;
- require Ono to produce distribution packages itself at runtime;
- define a graphical marketplace;
- make Linux package signatures equivalent to KUANG/11 package signatures;
- make `root` ownership equivalent to publisher trust;
- allow package maintainer scripts to grant capabilities, permissions or trust;
- require direct OS-package-manager integration for platforms that do not have such a manager;
- require all distributions to use the same outer package name;
- change the canonical KUANG/11 package identity from `package.id`;
- define container-image distribution as a separate plugin execution model.

# 4. Terminology

## 4.1 Acquisition

**Acquisition** is the process by which a KUANG/11 package payload becomes available to Ono for verification and installation.

Examples:

- downloading a catalog artifact;
- reading a local file or directory;
- discovering a payload provisioned by a system package manager.

Acquisition grants no runtime authority.

## 4.2 Provider source

A **provider source** is a location or mechanism from which Ono can obtain a KUANG/11 package payload.

Source kinds defined by this addendum are:

```text
catalog-network
local-path
system-package
```

Implementations MAY support additional source kinds in the future if they preserve the security invariants in this document.

## 4.3 System-provided package

A **system-provided package** is a canonical KUANG/11 package payload made available by an operating-system package manager or system provisioning mechanism in a host-controlled source directory.

It is **not yet an installed Ono plugin merely because the files exist there**.

## 4.4 Activation

For this addendum, **activation** refers to Ono taking an acquired package through the normal KUANG/11 installation transaction: verification, compatibility checks, permission planning, state registration, copying or binding the package into Ono-managed installed state, enabling it, registering contributions, and making lazy loading available.

The base specification remains authoritative for lifecycle terminology.

# 5. One Logical KUANG/11 Package, Multiple Delivery Mechanisms

There MUST be one logical KUANG/11 package contract regardless of acquisition mechanism.

The distribution model is:

```text
                         KUANG/11 release payload
                                  |
                 +----------------+----------------+
                 |                |                |
          catalog/network     local source     DEB/RPM wrapper
                 |                |                |
                 +----------------+----------------+
                                  |
                         Ono package verification
                                  |
                         publisher trust policy
                                  |
                         permission evaluation
                                  |
                         transactional install
                                  |
                              activation
```

A `.deb` or `.rpm` MUST therefore be treated as an **outer deployment wrapper**, not as a second plugin format.

The payload delivered by the wrapper MUST contain the same manifest, canonical package ID, version, compatibility metadata, contribution metadata, package integrity information and KUANG/11 publisher-signature material that Ono would expect from a catalog or local source.

Distribution-specific metadata MAY exist outside the KUANG/11 payload.

# 6. Catalog and Network Acquisition

## 6.1 Catalog behavior remains normative

The catalog model from the base specification remains unchanged.

A catalog release identifies at least:

- canonical package ID;
- short name;
- version;
- platform compatibility;
- KUANG/11 API compatibility;
- Ono language compatibility where relevant;
- artifact location;
- artifact digest;
- package signature metadata.

The built-in bootstrap catalog MAY reference network-hosted release artifacts.

## 6.2 Network fetch

For a network artifact, Ono MUST:

1. resolve metadata without executing plugin code;
2. fetch into a temporary or content-addressed staging location;
3. verify the artifact digest;
4. verify package structure;
5. verify the KUANG/11 package signature or applicable local-development trust mode;
6. evaluate publisher trust separately from signature validity;
7. validate compatibility;
8. derive the permission plan;
9. obtain consent when required;
10. commit installed state atomically.

No downloaded artifact may become executable plugin state before the relevant verification and trust checks complete.

## 6.3 Network protocol requirements

Remote catalog and artifact locations SHOULD use authenticated transport such as HTTPS.

Plain unauthenticated HTTP MUST NOT be enabled for ordinary production catalog artifacts by default. A development or explicitly configured enterprise source MAY relax transport requirements only when artifact digest and package-signature verification remain enforced and the user/admin policy explicitly permits the source.

## 6.4 Caching

Ono MAY cache downloaded artifacts by digest.

A cache hit MUST NOT bypass:

- digest verification;
- package-signature verification;
- compatibility validation;
- permission delta evaluation;
- publisher trust evaluation where policy has changed.

# 7. Local Acquisition

Local paths remain a first-class acquisition mechanism.

Supported forms from the base specification remain valid:

```text
./my-plugin
../my-plugin
/absolute/path/to/my-plugin
path:/absolute/path/to/my-plugin
```

An implementation MAY additionally support a canonical archive extension such as `.kuang`, but this addendum does not require a new archive format if the existing package-directory representation is sufficient.

Local acquisition MUST follow the same package verification, compatibility, permission and transaction rules as catalog acquisition.

Being local does not imply being trusted.

Local unsigned development packages MAY continue to use the explicit local-development trust semantics defined by KUANG/11.

# 8. Operating-System Package Acquisition

## 8.1 Supported model

A Linux package manager MAY install an outer package such as:

```text
ono-plugin-kubernetes
```

through mechanisms such as:

```text
apt install ono-plugin-kubernetes
dpkg -i ono-plugin-kubernetes_0.1.1_amd64.deb
dnf install ono-plugin-kubernetes
rpm -i ono-plugin-kubernetes-0.1.1-1.x86_64.rpm
```

The exact distribution package name is RECOMMENDED to follow:

```text
ono-plugin-<short-name>
```

but the distribution package name is **not** KUANG/11 identity.

The authoritative identity remains the package manifest's canonical `package.id`.

## 8.2 System source directory

System packages MUST place their KUANG/11 payload into a host-owned, read-only-to-normal-users source tree.

The default Linux path SHOULD be:

```text
/usr/lib/ono-sendai/plugin-sources/
```

with a layout equivalent to:

```text
/usr/lib/ono-sendai/plugin-sources/
└── io.github.godspeed-you.kubernetes/
    └── 0.1.1/
        ├── manifest.yaml
        ├── package signature/integrity metadata
        ├── executable/runtime payload
        └── other package files
```

Distribution-specific multiarch layout MAY use an equivalent configured root such as `/usr/lib/<triplet>/ono-sendai/plugin-sources/` if required by platform policy.

Ono MUST support a configured list of system source roots rather than hard-coding exactly one path internally.

The ordinary user MUST NOT need to configure the standard system source root manually.

## 8.3 Why system packages are sources, not active runtime state

A system package manager owns the files it installs. Ono MUST NOT treat those files as its mutable installed package store.

Instead, a system package payload is an **acquisition source**.

When the user runs:

```text
install plugin kubernetes
```

and resolution selects a system-provided candidate, Ono SHOULD copy or materialize the verified payload into its normal transactional installed-package store.

This design is normative because it prevents `apt upgrade` or `dnf upgrade` from replacing an already active plugin underneath Ono without:

- compatibility validation;
- permission-delta analysis;
- publisher-lineage validation;
- upgrade consent where required;
- a safe activation boundary.

An implementation MAY use a verified immutable bind/reference rather than a physical copy only if it can guarantee the same immutability and upgrade isolation. A mutable unversioned reference into package-manager-owned files is not conforming.

# 9. State Model for System-Provided Plugins

A system-provided package introduces an **availability state outside the existing KUANG/11 lifecycle**.

The following distinction MUST remain clear:

```text
available from system source
        !=
INSTALLED
        !=
ENABLED
        !=
LOADED
```

The existence of:

```text
/usr/lib/ono-sendai/plugin-sources/io.github.godspeed-you.kubernetes/0.1.1/
```

means only:

> Kubernetes 0.1.1 is available as a local system-provided acquisition candidate.

It does not mean:

- publisher trusted;
- permissions granted;
- plugin installed in Ono state;
- plugin enabled;
- plugin loaded;
- mutation allowed.

# 10. Discovery and Resolution with System Sources

## 10.1 Discovery

`find plugin <query>` MUST include compatible system-provided packages in addition to the sources already required by the base specification.

Results SHOULD expose source kind in human-readable form, for example:

```text
NAME        VERSION  SOURCE          INSTALLED  TRUST
kubernetes  0.1.1    system package no         project-trusted
kubernetes  0.1.2    ono catalog    no         project-trusted
```

Detailed inspection SHOULD expose the outer distribution package when known:

```text
System package: ono-plugin-kubernetes
Package manager: apt/dpkg
Source root: /usr/lib/ono-sendai/plugin-sources/...
```

## 10.2 Resolution relationship to the base specification

This addendum extends the base resolution model by introducing `system-package` as a local acquisition source.

Resolution MUST first deduplicate candidates by:

- canonical package ID;
- publisher identity/key lineage;
- version;
- artifact digest.

If a catalog entry and system source describe the exact same canonical release and digest, they MAY be rendered as one release with multiple acquisition locations.

## 10.3 System-source preference

When a compatible system-provided package exists for a canonical package ID and the user has not explicitly selected another source, Ono SHOULD prefer the system-provided candidate over a network download.

Rationale:

- an administrator may intentionally pin a version through APT/DNF;
- offline installations should work naturally;
- enterprise repositories should not be bypassed by an implicit public-network fetch;
- host-level package policy should remain meaningful.

This preference MUST NOT resolve ambiguity between **different canonical package IDs** sharing the same short name. The ambiguity behavior from the base specification remains unchanged.

## 10.4 Explicit source override

Users and automation MUST be able to request an explicit source when policy permits it.

Conceptually equivalent interfaces MAY include:

```text
install plugin kubernetes --source system
install plugin kubernetes --source catalog
install plugin official/kubernetes
install plugin /path/to/package
```

Exact CLI spelling MAY follow the implementation conventions established by the base specification, but source choice MUST be inspectable and deterministic.

# 11. Trust Boundaries

## 11.1 Root installation is not publisher trust

The following implication is forbidden:

```text
installed by root => trusted KUANG/11 publisher
```

A package being owned by `root`, delivered by dpkg/rpm, or signed by a Linux repository key proves something about **system package provenance**. It does not automatically prove the KUANG/11 publisher identity intended by the plugin trust model.

Ono MUST still evaluate:

```text
system package provenance
KUANG package digest
KUANG package signature
publisher identity
publisher trust policy
runtime tier
permission plan
```

as distinct facts.

## 11.2 Outer package signature is additional evidence

APT repository signatures, `.deb` package provenance, RPM signatures and repository metadata MAY be shown as additional provenance information.

They MUST NOT silently substitute for the KUANG/11 package signature unless a future explicit trust-policy specification defines such delegation.

## 11.3 Package maintainer scripts

`preinst`, `postinst`, `prerm`, `postrm`, RPM scriptlets or equivalent package-manager hooks MUST NOT:

- grant KUANG/11 capabilities;
- persist permission decisions;
- mark a publisher trusted;
- enable a plugin in a user's Ono state;
- load plugin runtime code;
- bypass a required install or upgrade prompt;
- write consent on behalf of a user.

A maintainer script MAY perform passive system integration such as creating source directories or refreshing a non-executable system-source index, provided it does not activate plugin code or grant authority.

# 12. Permission Semantics

All permission behavior from the base specification applies identically to all acquisition sources.

Therefore:

```text
apt install ono-plugin-kubernetes
```

MUST NOT itself grant:

```text
network.connect
filesystem.read
secret.use
process.exec
provider.mutate
```

or any other user-facing permission/capability.

After system provisioning, normal activation still requires the same semantic decision process as any other source.

Example:

```text
$ apt install ono-plugin-kubernetes
...
Kubernetes plugin source installed.

$ ono
> install plugin kubernetes

Kubernetes 0.1.1
Source: system package (ono-plugin-kubernetes)
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

The output wording MAY vary, but source provenance MUST NOT obscure or replace the permission plan.

# 13. Non-Interactive and Fleet Deployment

System package distribution is expected to be useful in automated fleets.

A package manager MAY make the plugin payload available non-interactively, but activation MUST still follow the base non-interactive consent rules.

Therefore this is allowed:

```text
apt install -y ono-plugin-kubernetes
```

but it only provisions the acquisition source.

A later command such as:

```text
ono install plugin kubernetes --non-interactive ...
```

MUST succeed only if the required trust and permission decisions are already covered by explicit policy or machine-readable input defined by the base specification.

`--yes` or equivalent MUST NOT transform system package installation into implicit permission consent.

# 14. Upgrade Semantics

## 14.1 Package-manager upgrade does not silently upgrade active Ono state

Suppose Ono currently has Kubernetes 0.1.1 installed from a system-provided source and the administrator runs:

```text
apt upgrade
```

which provides Kubernetes 0.1.2 under the system source tree.

Ono MUST treat 0.1.2 as a **new available acquisition candidate**.

It MUST NOT silently replace the active installed payload merely because the package manager changed the source files.

The installed Ono copy/reference remains pinned to its verified version and digest until the normal KUANG/11 upgrade transaction occurs.

## 14.2 Ono-driven upgrade

A subsequent operation such as:

```text
update plugin kubernetes
```

SHOULD prefer the package's pinned acquisition lineage.

If the plugin was installed from a system source and a newer compatible system-provided release exists, that release SHOULD be the default upgrade candidate.

The normal base-spec upgrade rules remain mandatory:

- verify package identity and publisher lineage;
- verify digest/signature;
- validate compatibility;
- compute permission delta;
- preserve existing authority only where still applicable;
- require consent for broadened authority;
- prevent silent privilege escalation;
- commit atomically.

## 14.3 No silent mutation grant via package upgrade

If a new OS package version changes the plugin manifest from read-only/recommended permissions to request `provider.mutate`, that authority MUST remain denied until explicitly approved under the base specification.

Neither `apt upgrade` nor `dnf upgrade` constitutes consent.

## 14.4 Source disappearance

If the system package is removed after Ono has already transactionally installed its own verified copy, the installed plugin MAY continue to function.

Ono SHOULD report that its acquisition source is no longer currently available, for example:

```text
Installed: yes
Installed version: 0.1.1
Origin: system package
Origin currently available: no
```

A later upgrade from that source fails cleanly until another valid source is selected.

# 15. Removal Semantics

## 15.1 Ono removal does not own the OS package manager

Running:

```text
remove plugin kubernetes
```

MUST remove/deactivate Ono-managed installed state according to the base specification.

It MUST NOT automatically execute:

```text
apt remove ono-plugin-kubernetes
```

or:

```text
dnf remove ono-plugin-kubernetes
```

The system-provided acquisition source may therefore remain available after plugin removal.

Ono SHOULD communicate this distinction when relevant:

```text
Removed Kubernetes from Ono.
A system-provided source remains available from package ono-plugin-kubernetes.
```

## 15.2 OS package removal does not revoke user state by script

Conversely, removing the outer `.deb`/`.rpm` MUST NOT run maintainer scripts that mutate Ono user permission/trust state.

If Ono previously copied the verified payload into its installed store, that installed state is controlled by Ono, not by dpkg/rpm.

## 15.3 Full purge

Documentation MAY describe how to remove both layers explicitly:

```text
ono> remove plugin kubernetes
$ apt remove ono-plugin-kubernetes
```

or the corresponding DNF/RPM workflow.

# 16. Ownership and Filesystem Requirements

## 16.1 System source ownership

System-provided plugin-source files SHOULD be owned by the privileged package-management domain, normally `root` on Linux, and MUST NOT be writable by ordinary users.

Ono MUST treat unexpected user-writable system source roots as lower-trust input and SHOULD warn or reject them according to source policy.

## 16.2 Ono-installed store

The transactional installed package store remains controlled by Ono according to its existing per-user or system deployment model.

The store MUST preserve:

- canonical ID;
- version;
- verified digest;
- source lineage;
- publisher identity lineage;
- package verification result.

## 16.3 No execution from staging

Plugin runtime code MUST NOT execute directly from:

- a download temporary directory;
- an unverified local source;
- a package-manager staging area;
- a partially written system source path.

Execution begins only from verified installed state or an equivalently immutable verified reference allowed by section 8.3.

# 17. Distribution Package Requirements

A conforming DEB/RPM wrapper SHOULD:

1. install one or more versioned KUANG/11 package payloads into a configured system source root;
2. preserve the original KUANG/11 manifest and signature metadata unchanged;
3. depend on a compatible Ono version where distribution packaging can express a useful lower bound;
4. avoid automatic activation;
5. avoid permission/trust changes in maintainer scripts;
6. expose ordinary package metadata such as license, homepage, architecture and maintainer;
7. be safe to install in an offline image build.

Distribution dependencies are advisory/preflight assistance. The KUANG/11 compatibility declaration inside the plugin remains authoritative to Ono.

# 18. Source Lineage and Pinning

Installed plugin metadata MUST remember acquisition lineage sufficiently to answer:

- Was this release acquired from catalog, local path or system package?
- Which catalog/source entry was selected?
- Which canonical package ID and publisher lineage were verified?
- Which version and digest were installed?
- If system-provided, which source root and distribution package supplied it when known?

A later short-name collision MUST NOT redirect an installed plugin to a different canonical package or publisher lineage.

Switching from system-package lineage to catalog lineage, or vice versa, MAY be supported, but it MUST be an explicit or inspectable source change rather than an accidental consequence of source ordering.

# 19. Conflict Handling

## 19.1 Same canonical release, same digest

If catalog, local cache and system package all expose the same:

```text
canonical ID
publisher lineage
version
digest
```

they represent equivalent acquisition locations.

Ono MAY prefer the local/system copy to avoid unnecessary network access.

## 19.2 Same canonical ID and version, different digest

This is a supply-chain conflict and MUST NOT be silently resolved.

Ono MUST surface a structured conflict containing at least:

- canonical ID;
- version;
- each source;
- each digest;
- signature/publisher identity information.

The user/admin must select a source or policy resolution explicitly.

## 19.3 Same short name, different canonical ID

The ambiguity rules from the base specification remain unchanged.

System package preference MUST NOT cause Ono to silently choose a different canonical plugin merely because it is locally available.

# 20. Offline Operation

A fully offline installation MUST be possible using either:

```text
install plugin /local/path
```

or an OS-managed source such as:

```text
apt install ono-plugin-kubernetes
```

from an internal/offline repository.

In offline mode:

- package digest/signature verification still applies;
- cached publisher trust and local policy still apply;
- no public catalog refresh is required to activate a valid local/system-provided package;
- missing online revocation information MAY be reported as stale according to existing trust policy, but MUST NOT be silently fabricated;
- JIT permissions that require no network policy service continue to work normally.

# 21. User Experience

## 21.1 Ordinary catalog user

```text
> install plugin kubernetes
```

Ono resolves and acquires the package from the preferred configured source and presents the normal permission UX.

The user does not need to understand download URLs.

## 21.2 Enterprise/system-package user

```text
$ sudo apt install ono-plugin-kubernetes
$ ono
> install plugin kubernetes
```

Ono detects the local system-provided package and SHOULD avoid a public network fetch when that package is the selected compatible source.

## 21.3 Explicit local user

```text
> install plugin ./kubernetes.kuang
```

or the existing directory/path form.

## 21.4 Inspection

`inspect plugin kubernetes` SHOULD include acquisition information equivalent to:

```text
Acquisition source: system package
System package: ono-plugin-kubernetes
Source path: /usr/lib/ono-sendai/plugin-sources/io.github.godspeed-you.kubernetes/0.1.1
Installed version: 0.1.1
Installed digest: sha256:...
Origin available: yes
```

The default summary need not expose all details, but advanced inspection MUST make them available.

# 22. Kubernetes Reference Packaging

`ono-sendai-kubernetes` SHOULD become the first reference plugin demonstrating all three acquisition paths.

The project SHOULD produce or document artifacts equivalent to:

```text
KUANG/11 release payload
DEB wrapper
RPM wrapper
```

The `.deb` and `.rpm` MUST contain the same signed KUANG/11 payload semantics used by catalog installation.

A reference Linux package SHOULD be named:

```text
ono-plugin-kubernetes
```

subject to distribution naming requirements.

Installing that OS package MUST NOT grant `provider.mutate` or any other permission.

# 23. Security Invariants

Every conforming implementation MUST satisfy all of the following:

1. OS package installation is acquisition, not consent.
2. `root` ownership is not KUANG publisher trust.
3. DEB/RPM repository signatures do not silently replace KUANG package signatures.
4. Package-manager maintainer scripts cannot grant plugin permissions.
5. Package-manager upgrade cannot silently broaden active plugin authority.
6. System-provided payloads are versioned acquisition sources, not mutable active runtime directories.
7. Active installed state is pinned to a verified digest.
8. A source conflict with the same version but different digest fails closed.
9. Offline acquisition does not disable integrity verification.
10. A locally available package does not resolve a short-name collision between different canonical IDs.
11. Plugin runtime code never executes during source discovery.
12. Plugin runtime code never executes from unverified staging state.
13. Removal by one package-management layer does not mutate trust/permission state owned by another layer without explicit Ono action.

# 24. Required Implementation Components

The Ono core implementation of this addendum SHOULD introduce or extend components equivalent to:

```text
PluginSource
  kind: catalog-network | local-path | system-package
  location
  source_identity
  manager_metadata?

SystemPluginSourceScanner
  configured_roots[]
  enumerate_candidates()
  validate_source_permissions()

AcquisitionCandidate
  canonical_id
  short_name
  version
  digest
  publisher_metadata
  compatibility
  source

AcquisitionPlan
  candidate
  fetch_or_materialize_action
  verification_plan
  install_target

InstalledOriginRecord
  source_kind
  source_identity
  system_package_name?
  system_package_manager?
  source_path?
  catalog_lineage?
  artifact_digest
```

Exact data structures MAY differ, but equivalent state and observable behavior are required.

# 25. Tests and Acceptance Gates

The implementation is not complete until automated tests cover at least the following.

## 25.1 Catalog/network

1. Short-name catalog install can fetch a network artifact.
2. Downloaded artifacts are staged before commit.
3. Digest mismatch fails before install.
4. Invalid package signature fails according to trust policy.
5. Failed network acquisition leaves no installed state or new permissions.
6. Cached artifacts do not bypass verification or permission-delta checks.

## 25.2 Local source

7. Explicit local directory installation works.
8. Existing `path:` compatibility remains functional.
9. Local source does not imply publisher trust.
10. Local install uses the same permission plan as the same package from catalog.

## 25.3 System package source

11. A payload under the configured system source root is discoverable.
12. Merely creating/installing that payload does not create Ono `INSTALLED` state.
13. It does not grant permissions.
14. `install plugin <short-name>` can select the system-provided candidate.
15. The package is verified before becoming installed state.
16. Ono records system-source lineage.
17. Standard system source roots are searched without user configuration.
18. User-writable spoofed system roots are rejected or clearly downgraded according to policy.

## 25.4 Source selection

19. Same release/digest from system and catalog deduplicates safely.
20. System source is preferred over network acquisition for the same canonical plugin when no explicit override exists.
21. Same canonical ID/version with different digest fails as a supply-chain conflict.
22. Same short name with different canonical IDs remains ambiguous.
23. Explicit source selection overrides the default where policy allows it.

## 25.5 Upgrade

24. Updating the `.deb`/`.rpm` source does not mutate the currently installed Ono payload automatically.
25. `update plugin` can consume the newer system-provided candidate.
26. Permission escalation in the new package still requires consent.
27. `provider.mutate` cannot be obtained through `apt upgrade` or `dnf upgrade` alone.
28. Publisher-lineage changes are handled by existing trust/escalation policy.
29. Failed upgrade retains previous installed state where the base transaction model requires rollback.

## 25.6 Removal

30. `remove plugin` does not invoke the OS package manager.
31. Removing the OS package source does not mutate stored permission decisions through maintainer scripts.
32. An Ono-installed copied payload remains deterministic when its acquisition source disappears.
33. Reinstall after Ono removal does not inherit stale consent contrary to the base specification.

## 25.7 Offline

34. Local-path installation works with no network.
35. System-package installation works with no public catalog.
36. Integrity/signature checks remain active offline.

## 25.8 Security

37. No package-manager hook can activate plugin runtime code as part of package installation tests.
38. No package-manager hook writes KUANG permission/trust records.
39. Unverified staging files cannot be executed.
40. Installed state is pinned to canonical ID, publisher lineage, version and digest.

# 26. Documentation Requirements

The Ono documentation MUST clearly distinguish:

```text
Acquire / provision package
Verify / install into Ono
Grant permissions
Activate / lazy-load
```

The normal user documentation SHOULD present the catalog flow first:

```text
install plugin kubernetes
```

Linux/enterprise documentation SHOULD additionally show:

```text
apt install ono-plugin-kubernetes
# or
dnf install ono-plugin-kubernetes

ono
> install plugin kubernetes
```

Documentation MUST explicitly state that the OS package manager does not grant Ono permissions.

The Kubernetes repository SHOULD document its DEB/RPM packaging specifics but MUST reference the generic KUANG/11 acquisition contract rather than re-specifying core security behavior.

# 27. Migration and Compatibility

This addendum is intentionally additive.

Existing implementations of:

```text
install plugin path:...
```

remain valid.

The base catalog implementation remains valid.

Adding system package sources MUST NOT require users to migrate existing installed plugins.

Plugins already installed from a catalog or local path retain their source lineage. They MUST NOT silently switch to a newly discovered system package merely because one appears later.

A future explicit source migration command MAY allow an installed plugin to change acquisition lineage after verifying identity, publisher lineage and package compatibility.

# 28. Implementation Order

Because the base Plugin Installation, Resolution and Permission UX implementation may already be underway when this addendum is introduced, this addendum is designed for a **second, additive implementation pass**.

The recommended sequence is:

1. complete and stabilize the base plugin installation/permission specification;
2. retain all tests from that implementation unchanged unless this addendum requires additional source metadata;
3. introduce the acquisition-source abstraction around the existing fetch/read stage;
4. implement the system-source scanner;
5. add deterministic source resolution/preference;
6. add origin persistence and inspection;
7. add DEB/RPM reference packaging for Kubernetes;
8. add upgrade isolation tests;
9. add offline and enterprise deployment documentation;
10. perform a final gap analysis against both the base specification and this addendum.

The implementation MUST NOT regress the simple catalog UX in order to add system package support.

# 29. Final Product Contract

After both specifications are implemented, all of these workflows are valid:

```text
# simplest ordinary path
ono> install plugin kubernetes
```

```text
# local/offline package
ono> install plugin /mnt/usb/kubernetes.kuang
```

```text
# enterprise / distribution-managed acquisition
$ sudo apt install ono-plugin-kubernetes
$ ono
ono> install plugin kubernetes
```

```text
# Fedora/RHEL-family equivalent
$ sudo dnf install ono-plugin-kubernetes
$ ono
ono> install plugin kubernetes
```

In every case, the same KUANG/11 security model applies.

The package may arrive through the Internet, local storage or the operating-system package manager.

**Only Ono decides whether that package becomes trusted, which permissions it receives, and when it becomes active.**
