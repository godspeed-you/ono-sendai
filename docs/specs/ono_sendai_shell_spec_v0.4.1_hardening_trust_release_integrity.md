---
title: "ONO-SENDAI"
subtitle: "Specification v0.4.1 - Hardening, Trust & Release Integrity"
author: "Project Specification"
date: "2026-09-01"
geometry: "paperwidth=157.5mm,paperheight=210mm,top=13mm,bottom=15mm,left=13mm,right=13mm,headsep=4mm,footskip=8mm"
fontsize: 11pt
linestretch: 1.08
colorlinks: true
linkcolor: blue
urlcolor: blue
toc: true
toc-depth: 3
numbersections: false
---

# ONO-SENDAI Specification v0.4.1

## Hardening, Trust & Release Integrity

**Status:** Maintenance, security and release-hardening specification  
**Scope:** Security boundaries, remote trust, KUANG/11 confinement, resource boundedness, streaming correctness, performance stability, test truthfulness, maintainability boundaries, CI/CD and release provenance  
**Relationship:** Standalone maintenance extension to the published Ono-Sendai v0.2 baseline and v0.3-v0.4 feature specifications; prerequisite hardening layer for later implementation of the already specified v0.5 and v0.6 feature releases  
**Normative language:** MUST, MUST NOT, SHOULD, SHOULD NOT, MAY and RECOMMENDED are used in the RFC sense  
**Reference implementation baseline:** Public `v0.4.0` release plus the active `implementation` branch as reviewed on 2026-09-01

> **If Ono says a boundary is authenticated, a limit is bounded, a stream is streaming, a test passed, or a release is reproducible, that statement must itself be true.**

---

# 0. Document Status and Relationship to Earlier Specifications

## 0.1 Standalone maintenance specification

This document is the standalone ONO-SENDAI v0.4.1 Hardening, Trust & Release Integrity specification.

It does not replace, rewrite, regenerate or retrospectively modify the v0.2 base specification, the v0.3 External Command Adaptation Layer specification, the v0.4 Spatial Systems Interface specification, the v0.5 Temporal & Causal Systems Interface specification, or the v0.6 Prospective Change, Protection & Recovery Interface specification.

The intended relationship is:

```text
ONO-SENDAI v0.2
    base shell, typed values, providers, object pipelines,
    remote links, KUANG/11 and TUI foundations

        +

ONO-SENDAI v0.3
    external command adaptation and structured Unix interoperability

        +

ONO-SENDAI v0.4
    spatial systems interface and navigable system topology

        +

ONO-SENDAI v0.4.1
    hardening of the already implemented substrate:
    trust, bounds, streaming, performance, tests and release integrity

        ->

stable implementation base for the future v0.5 and v0.6 feature tranches
```

v0.4.1 is deliberately a maintenance release rather than a new conceptual product dimension. It exists because the v0.4 codebase is now large and capable enough that the quality of its boundaries matters more than the addition of another major feature.

## 0.2 Why this is v0.4.1 rather than v0.7

v0.5 and v0.6 already have standalone feature specifications and remain future product increments. This document MUST NOT consume a new feature-version number merely because it is extensive.

The release version `0.4.1` communicates the correct intent:

- the user-facing v0.4 product model remains the same;
- the implementation contract becomes stricter;
- unsafe or ambiguous defaults may become safer even when that changes edge-case behavior;
- later feature work MUST inherit these hardened guarantees rather than re-solve them.

A security or correctness fix MAY intentionally refuse behavior that v0.4.0 previously accepted. Such a refusal is not considered a forbidden compatibility break when the accepted behavior violated an existing safety claim or had no trustworthy security semantics.

## 0.3 No retrospective editing

Earlier narrative specifications remain immutable historical product inputs.

If v0.4.1 reveals that an earlier statement such as "authenticated transport" was implemented too narrowly, the earlier document MUST NOT be edited to disguise the gap. v0.4.1 MUST state the strengthened interpretation explicitly and an ADR MUST document the implementation decision where architecture changes.

## 0.4 Normative scope

This specification defines:

- a bidirectional trust model for directly listening Ono agents;
- authenticated peer identity independent from self-reported user metadata;
- authorization of remote operations after authentication;
- listener defaults, connection limits and denial behavior;
- key generation, storage, rotation and migration;
- KUANG/11 native execution trust levels and fail-closed confinement;
- terminology for "sandbox", "confinement" and "trusted native plugin";
- resource budgets for captures, materialization and retained results;
- true streaming semantics for `each` and other non-global operations;
- ordering, cancellation and backpressure contracts for pipeline events;
- performance measurement at realistic cardinalities;
- required remediation for pathological spatial/live-query behavior;
- truthful test outcomes and explicit skip semantics;
- coverage-guided fuzzing and memory/undefined-behavior verification tiers;
- CI supply-chain hardening;
- reproducible build requirements;
- release checksums, signatures and build provenance;
- structural refactoring boundaries for the parser, evaluator and session state;
- generated repository metrics and documentation-truth rules;
- a complete implementation sequence, work-package model and release definition.

This document deliberately leaves no product-design questions open for the v0.4.1 scope. Implementation details MAY vary only where they preserve the semantics and invariants defined here.

## 0.5 Current implementation facts that motivate this specification

The review that produced this document identified a set of important implementation facts. They are recorded here because v0.4.1 exists to close specific gaps, not to create abstract process work.

The reviewed codebase already has substantial strengths:

- typed values and schemas are consistent across commands and providers;
- pipeline channels are bounded and cancellation-aware;
- the provider API is a real domain boundary rather than a formatting wrapper;
- unsafe Unix code is concentrated in a narrow process crate;
- the remote wire format already has size, depth and credit constraints;
- TLS 1.3 server identity pinning already exists for direct TCP links;
- KUANG/11 already has capability, audit and resource-control concepts;
- CI already includes format, lint, tests, fuzz targets, acceptance containers and package installation tests;
- the project already treats specifications and ADRs as executable architecture inputs.

The reviewed implementation also has concrete gaps:

1. A directly listening TLS agent authenticates itself to the client, but the server side currently accepts TLS clients without a client certificate. The protocol-level `Identity` is self-reported metadata and is not a cryptographic credential.
2. The agent may expose query, subscription, adapter and action functionality after such a connection without an authenticated client principal.
3. Native KUANG/11 process setup invokes security-relevant system calls whose failures are not all treated as fatal.
4. Native KUANG/11 execution is not filesystem or network isolation, even where high-level prose may use the broad word "sandbox".
5. Some evaluator paths materialize streams that are conceptually non-global operations, most notably block-based `each` execution.
6. Count-based limits do not consistently impose byte-based memory limits.
7. High-cardinality spatial operations have demonstrated time-to-first-result failures that are not visible in small fixtures.
8. Earlier test helpers allowed environment-dependent tests to return without assertions while still appearing as successful test cases.
9. CI and release workflows rely on mutable third-party action tags and mutable container tags in places where immutable references are possible.
10. The parser, evaluator and session modules have reached a size where structural decomposition is required to preserve maintainability without changing behavior.

Every work package in this specification MUST trace back to one or more of those facts or to a system-wide invariant needed to prevent the same class of defect from reappearing.

---

# 1. Product Thesis

## 1.1 Hardening is a product property

Ono-Sendai presents itself as a systems interface. A systems interface is trusted differently from an ordinary application.

Users may run it as a login shell. They may inspect credentials, processes, sockets, filesystems and services through it. They may connect it to remote systems. They may load plugins. They may eventually ask it to prepare and execute destructive changes.

For such a product, security, boundedness and truthfulness are not internal engineering preferences. They are part of the user-visible product contract.

v0.4.1 therefore adopts the thesis:

> **A trustworthy systems interface must make its guarantees at the same layer where the risk exists.**

A UI label saying "authenticated" cannot compensate for an unauthenticated transport peer. A `Vec` limit cannot compensate for an unlimited payload size. A test name cannot compensate for a test body that did not execute. A "reproducible" build cannot depend on a mutable base image.

## 1.2 The release has no feature-count objective

v0.4.1 MUST NOT be judged by the number of new commands, views or providers it introduces.

The desired user-visible effect is that existing functionality becomes more boring in the best possible way:

- remote links refuse the wrong peer;
- plugins fail before execution when protection cannot be installed;
- long-running commands begin producing output when their semantics allow it;
- large systems degrade predictably rather than catastrophically;
- memory growth stops at declared limits;
- CI failures mean something;
- published binaries can be traced back to a specific source and build environment.

## 1.3 Hardening must preserve Ono's character

Hardening MUST NOT turn Ono into a ceremony-heavy security product.

The shell remains terse, discoverable and local-first. Secure defaults SHOULD remove decisions from common flows rather than add prompts.

The preferred interaction pattern is:

```text
safe default
    -> clear refusal when more authority is needed
    -> explicit configuration or command to grant that authority
    -> no "continue anyway" escape inside the risky operation
```

This mirrors existing host-key behavior and MUST be reused throughout v0.4.1.

---

# 2. Core Invariants

The following invariants are normative across the entire v0.4.1 release.

## 2.1 Authenticated means cryptographically authenticated

When documentation, UI, diagnostics, APIs or code comments call a remote peer "authenticated", Ono MUST possess cryptographic evidence that the peer holds the private key corresponding to the identity accepted by policy.

Self-reported fields such as user name, UID, operating system, architecture or elevation status MUST NOT satisfy this invariant.

## 2.2 Authentication precedes authorization

No network peer MAY reach provider query, subscription, adapter execution or action dispatch until its transport identity has been authenticated and authorized.

The sequence MUST be:

```text
transport connection
    -> cryptographic peer proof
    -> peer trust decision
    -> authorization policy resolution
    -> protocol negotiation constrained by authorization
    -> provider or adapter operation
```

## 2.3 Safety mechanisms fail closed

If Ono claims that a safety control is applied before an operation, failure to apply that control MUST prevent the operation from starting.

This applies to, at minimum:

- `PR_SET_NO_NEW_PRIVS`;
- resource limits;
- process-session isolation when required by the execution tier;
- privilege dropping where configured;
- remote peer verification;
- authorization-policy loading;
- release-signature verification in future consumers that advertise it.

## 2.4 Bounded means bounded in relevant dimensions

A count limit alone is insufficient when each element can have arbitrary size.

Every retained, queued or materialized collection with user- or provider-controlled element size MUST have both:

- an item-count bound where item count is meaningful; and
- an approximate byte bound.

If an exact memory measurement is impractical, Ono MUST use a deterministic conservative estimator rather than omit the byte bound.

## 2.5 Streaming means incremental production

A command is "streaming" only if it can consume and produce incrementally without first waiting for the complete upstream stream, except where its semantics inherently require global knowledge.

`each`, filtering, projection and one-to-one transformations MUST be streaming.

Sorting, global aggregation and operations whose contract explicitly requires a complete finite set MAY materialize.

## 2.6 Unknown remains unknown

Hardening MUST NOT invent certainty to simplify policy or performance behavior.

If Ono cannot determine whether a plugin control was installed, whether a remote capability is authorized, whether a query completed, or whether a historical test prerequisite was satisfied, it MUST report an explicit unknown/refusal state rather than claim success.

## 2.7 Tests report execution truth

A test that did not execute its intended assertion path MUST NOT be reported as an ordinary pass.

## 2.8 Reproducible means immutable inputs

A build MAY be called reproducible only when all inputs that can materially alter the artifact are pinned or recorded sufficiently to recreate the environment.

Mutable GitHub Action tags, `latest` container tags or unrecorded tool versions violate this invariant for the affected step.

## 2.9 Security defaults are non-interactive

No security decision in this specification MAY be implemented as an interactive "trust this once?" or "continue anyway?" prompt inside a connection, plugin spawn or release verification path.

Trust changes are separate deliberate operations.

---

# 3. Scope Classification and Priorities

## 3.1 Priority classes

Every v0.4.1 work package MUST be classified as one of:

- **P0 - release blocker:** an existing boundary can claim safety without actually enforcing it;
- **P1 - correctness or stability blocker:** realistic use can hang, grow without useful bound, or violate a major execution invariant;
- **P2 - maintainability or verification hardening:** reduces the chance of regression and makes evidence trustworthy;
- **P3 - hygiene:** improves automation, documentation or release ergonomics without changing runtime safety.

## 3.2 Mandatory P0 scope

The release MUST NOT ship without:

1. authenticated clients for direct TCP listening-agent mode;
2. server-side authorization derived from the authenticated client identity;
3. fail-closed handling of mandatory KUANG/11 native confinement setup;
4. documentation and UI terminology that no longer overstates native process isolation.

## 3.3 Mandatory P1 scope

The release MUST NOT ship without:

- bounded materialization and capture memory;
- incremental `each` block execution;
- deterministic behavior for unbounded input to operations requiring finiteness;
- realistic-cardinality spatial performance gates;
- resolution of known "no first result" spatial/live-query pathologies or a bounded, explicit refusal when a query exceeds supported cost;
- connection and per-peer resource limits for listening agents.

## 3.4 P2 and P3 completion

P2 and P3 items are part of the complete v0.4.1 product contract, not optional backlog.

A release candidate MAY be cut while some P2/P3 work is still in progress, but the final v0.4.1 release MUST satisfy the release definition in section 66.

---

# 4. Compatibility and Versioning Contract

## 4.1 Language and public schemas

v0.4.1 MUST NOT intentionally change the syntax or semantics of existing v0.4 language constructs except where required to correct a contradiction with an earlier documented guarantee.

Existing typed schemas MUST remain compatible unless a security-relevant field needs an additive extension.

Additive fields MUST follow existing schema evolution rules.

## 4.2 Direct TCP remote compatibility

Direct TCP links between v0.4.0 and v0.4.1 MUST fail safely rather than silently downgrade authentication.

A v0.4.1 listening agent MUST NOT accept a v0.4.0 direct-TCP client that cannot present an authenticated client identity.

A v0.4.1 client MAY connect to a v0.4.0 server only if the user explicitly selected a legacy compatibility mode outside the normal `link` path. The canonical v0.4.1 link path MUST NOT downgrade automatically.

The preferred implementation is to bump the direct-link trust/protocol capability such that a missing mutual-authentication capability produces `remote.protocol_mismatch` or a new stable authentication-specific error before any provider operation.

## 4.3 SSH-carried stdio agent compatibility

`ono --agent` over stdin/stdout, when carried by an already authenticated SSH channel, MAY retain the v0.4 transport model because the network peer is authenticated by OpenSSH before Ono receives the stream.

However, Ono MUST describe that trust source accurately:

```text
transport trust: ssh
peer key visible to ono: no
remote process identity: inherited from ssh execution context
```

The protocol-level `Identity` remains descriptive metadata and MUST NOT be reclassified as a cryptographic credential.

## 4.4 Plugin compatibility

Plugins that previously happened to run even when a mandatory confinement syscall failed MAY stop launching under v0.4.1.

This is an intended compatibility change.

A plugin MUST NOT be granted execution merely to preserve historical success when the configured execution tier cannot be established.

## 4.5 Configuration migration

Existing configuration files MUST continue to parse.

New security stores or fields MUST be additive. If a file format changes, migration MUST be deterministic, testable and lossless, with the old file preserved until the new file has been fsynced successfully.

---

# 5. Threat Model

## 5.1 Protected assets

v0.4.1 treats the following as protected assets:

- confidentiality of system data exposed by providers;
- integrity of provider actions and remote mutations;
- the identity of remote systems and remote clients;
- local files readable by the Ono process;
- network access available to the Ono process;
- user credentials present in environment, files or process metadata;
- availability of the shell under untrusted or pathological input;
- integrity of published release artifacts;
- trustworthiness of test and release claims.

## 5.2 Attacker classes

The implementation MUST consider at least:

1. an unauthenticated host that can reach a listening agent TCP port;
2. a network attacker able to intercept or redirect first-contact traffic;
3. a previously trusted peer whose key has changed;
4. a valid but minimally authorized remote client attempting broader capabilities;
5. a malicious or buggy native KUANG/11 plugin;
6. a provider returning extremely large or adversarial values;
7. an external command or adapter producing malformed machine-readable output;
8. a pathological local system with very high process/socket/topology cardinality;
9. a compromised or replaced third-party CI action/tag;
10. a mutable build image that changes after release source is tagged.

## 5.3 Explicitly out-of-scope attacker

v0.4.1 does not claim to defend an unprivileged Ono process against a fully compromised kernel, a root attacker on the same host, or malicious hardware/firmware.

A native KUANG/11 plugin running under the same Unix account is also not treated as fully isolated from that account unless a future stronger execution tier explicitly provides kernel-enforced filesystem/network isolation.

The product MUST say this plainly.

---

# 6. Security Boundary Map

## 6.1 Required boundary inventory

The repository MUST contain a machine-readable or generated boundary inventory that names at minimum:

```text
Boundary ID                    Input trust          Required enforcement
remote.tcp.transport           network             TLS 1.3 + mutual peer proof
remote.tcp.authorization       authenticated peer  explicit server policy
remote.ssh.transport           ssh channel          external SSH authentication
protocol.frame                 peer bytes           size/depth/version limits
provider.query                 authorized request   provider capability contract
provider.act                   authorized request   capability + risk/elevation checks
kuang.native.spawn             package process      manifest + fail-closed confinement
kuang.protocol                 plugin bytes         frame/credit/schema limits
external.adapter               process output       adapter decoder and schema validation
pipeline.materialization       value stream         count + byte budget
release.build                  CI inputs            immutable refs + locked dependencies
release.publish                artifacts            checksum + signature + provenance
```

This inventory MUST be derivable into documentation and MUST be referenced by security tests.

## 6.2 Boundary ownership

Each boundary MUST have one owning crate/module responsible for enforcing the primary guarantee.

Higher layers MAY add policy but MUST NOT be the only place a lower-level safety property is enforced.

Examples:

- TLS peer proof belongs in `ono-remote`, not in CLI rendering;
- wire size limits belong in `ono-protocol`, not in command handlers;
- native process setup errors belong in `ono-kuang-supervisor`/process code, not in a later audit view;
- byte-budget enforcement belongs in the materialization primitive, not in each caller independently.

---

# 7. Direct Remote Agent Trust Model

## 7.1 Symmetric transport identity

The direct TCP transport introduced before v0.4.1 already lets a client authenticate the server certificate as a pinned host key. v0.4.1 MUST make the transport symmetric.

Both endpoints MUST present a certificate and prove possession of the corresponding private key during TLS 1.3 negotiation.

The certificate MAY remain self-signed. Ono's trust model is explicit key/fingerprint trust, not a public certificate-authority hierarchy.

## 7.2 Peer identity object

The implementation SHOULD generalize the current host-only identity abstraction into a transport-neutral `PeerIdentity` or equivalent concept.

A peer identity contains:

```text
algorithm
certificate/public material
private key
fingerprint
storage location
creation metadata where available
```

The public contract is the fingerprint. The private key MUST never be serialized into ordinary structured pipeline output, logs, diagnostics or crash messages.

## 7.3 Cryptographic identity versus runtime identity

The authenticated transport identity and the runtime `Identity { user, uid, elevated }` MUST remain separate fields.

A negotiated peer view SHOULD conceptually resemble:

```text
Peer {
    transport_fingerprint: sha256:...
    transport_trust: pinned | authorized
    runtime_user: "alice"
    runtime_uid: 1000
    runtime_elevated: false
    agent: "ono/0.4.1"
    os: "linux"
    arch: "x86_64"
}
```

The runtime identity is useful context but MUST NOT grant authority.

## 7.4 No unauthenticated network mode in the canonical agent

The normal direct listening-agent mode MUST NOT provide a flag that disables client authentication.

If an unauthenticated transport remains necessary for tests or in-process duplexes, it MUST be inaccessible from ordinary network CLI configuration and clearly named `Unauthenticated` in internal APIs.

---

# 8. Client Identity Lifecycle

## 8.1 Canonical identity file

A direct-link client MUST have a persistent peer identity so the server can authorize it consistently.

The reference implementation MUST store the default identity under the Ono configuration directory.

Canonical path:

```text
~/.config/ono/link_identity.pem
```

If `ONO_CONFIG_DIR` or XDG configuration rules relocate the Ono configuration directory, this path follows the existing configuration-directory resolution logic.

## 8.2 Migration from `host_key.pem`

A v0.4.1 installation that already has `host_key.pem` from listening-agent use MUST reuse that identity rather than silently generate a second unrelated identity.

Migration rules:

1. if `link_identity.pem` exists, use it;
2. else if legacy `host_key.pem` exists and parses, atomically copy/migrate it to `link_identity.pem` while preserving mode `0600`;
3. else generate `link_identity.pem`;
4. never delete the legacy file automatically in v0.4.1;
5. both files MUST NOT diverge silently if both are explicitly configured for the same process role.

An ADR MUST document whether the implementation keeps one canonical file plus a compatibility symlink/read fallback or performs a one-time copy.

## 8.3 File permissions

Private identity files MUST be created with owner-read/write permissions only (`0600`) and the containing directory SHOULD be owner-only where Ono owns it.

If an existing identity file is group/world writable, Ono MUST refuse to use it and report a security error.

If it is group/world readable, Ono MUST refuse by default because the private key is exposed.

## 8.4 Identity generation

Identity generation MUST use a cryptographically secure RNG through the selected TLS/key library.

Failure to generate or persist the key MUST prevent direct-link establishment.

## 8.5 Key fingerprint display

The shell MUST provide a non-secret way to print the local peer fingerprint.

Canonical global invocation:

```text
ono --print-peer-key
```

Existing:

```text
ono --agent --print-host-key
```

MUST remain accepted in v0.4.1 and MUST print the same identity fingerprint when the default identity path is used.

The help text SHOULD direct new users to `--print-peer-key`.

## 8.6 Rotation

Key rotation MUST be explicit. Replacing or deleting the identity file is considered an administrative action that will cause previously authorized peers to reject the identity until reauthorized.

Ono MUST NOT auto-rotate this key based solely on age in v0.4.1.

---

# 9. Server-Side Client Authorization

## 9.1 Authentication is not sufficient

A valid client certificate proves only that the connecting process holds a private key. It does not prove that the agent operator wants to expose system data or actions to that key.

A listening agent MUST therefore maintain an explicit authorization store.

## 9.2 Canonical authorization store

Reference path:

```text
~/.config/ono/authorized_clients
```

The file MUST be human-readable, line-oriented and parse strictly.

Malformed non-comment lines MUST fail loading of the store. A malformed authorization store MUST NOT be treated as empty and MUST NOT cause the agent to fall back to permissive access.

## 9.3 Authorization entry model

Each entry MUST represent at least:

```text
fingerprint
label
read_policy
allowed_actions
```

The logical model is:

```text
AuthorizedClient {
    fingerprint: Fingerprint,
    label: String?,
    observe: bool,
    actions: Set<CapabilityId>
}
```

Unknown fields in a future version MUST be rejected unless the file has an explicit version and schema-evolution rule.

## 9.4 Default grant

Adding a client without further options MUST grant **observe-only** access:

- provider snapshot/query where the relevant capability risk is `read` or `observe`;
- provider subscription where the relevant capability risk is `observe`;
- no `Act` request;
- no elevated action;
- no destructive action.

A client not present in the authorization store MUST be refused before protocol negotiation exposes providers.

## 9.5 Explicit action grants

Actions MUST be authorized by exact capability ID.

Example logical grants:

```text
process.signal
service.restart
network.route.change
```

Wildcards MUST NOT be the storage default.

If the implementation offers an explicit convenience operation equivalent to "all current actions", it MUST expand to the exact capability IDs known at grant time and persist that expanded list. Newly introduced future capabilities MUST therefore remain denied until explicitly authorized.

## 9.6 Elevation and destructive actions

Capabilities marked as requiring elevation or `destructive` risk MUST require exact explicit grant even if a future policy profile otherwise allows mutations.

There MUST be no implicit `admin` profile in v0.4.1 that automatically grows when new capabilities are added.

## 9.7 Authorization commands

The reference implementation MUST expose authorization management through ordinary Ono verb-target semantics.

Canonical targets:

```text
get client-key
add client-key
set client-key
remove client-key
```

Required behavior:

```text
get client-key
    lists fingerprint, label, observe permission, allowed action IDs and store path

add client-key <fingerprint> [--label <name>]
    adds an observe-only authorized client

set client-key <fingerprint> --allow <capability>...
    replaces/sets the exact action allowlist while preserving observe state

set client-key <fingerprint> --observe true|false
    changes query/subscription permission

remove client-key <fingerprint>
    revokes the client
```

A different internal target name MAY be used only if an ADR demonstrates that it fits the existing command registry better without reducing clarity. The user-facing concept MUST remain "authorized client key", not a vague ACL blob.

## 9.8 Atomic updates

Authorization-store changes MUST be written atomically using write-to-temporary, fsync, rename and directory sync where supported by the existing persistence conventions.

A failed update MUST leave the previous valid store intact.

---

# 10. Authorization-Constrained Negotiation

## 10.1 Server offer is policy-filtered

The protocol handshake MUST NOT advertise the full server capability set and then rely only on later dispatch checks.

The `Offer` used to negotiate a direct link MUST first be intersected with the authenticated client's authorization.

This means unauthorized capabilities are absent from the accepted link contract.

## 10.2 Defense in depth at dispatch

Negotiation filtering is not sufficient by itself.

Every provider/adapter/action dispatch path MUST also validate that the operation is permitted by the established peer authorization context.

A malicious peer that sends a validly encoded request for a capability omitted from negotiation MUST receive a stable authorization refusal and the operation MUST NOT execute.

## 10.3 Authorization context

The server-side connection state MUST carry an immutable authorization context created immediately after TLS peer verification.

It SHOULD contain:

```text
peer_fingerprint
client_label
observe_allowed
allowed_action_capabilities
connection_id
connected_at
```

Request handlers MUST receive this context explicitly or through a connection service object. They MUST NOT re-read a mutable authorization file on each individual request.

Changes to authorization affect new connections. Revocation MAY additionally terminate matching live connections; see section 12.5.

## 10.4 Stable refusal

Unauthorized operations MUST return a stable error family such as:

```text
remote.unauthenticated
remote.unauthorized
remote.capability_denied
```

The exact numeric codes MUST be allocated through the existing error registry and checked by contract tests.

The error SHOULD include:

- authenticated client fingerprint;
- requested capability ID;
- whether the request was denied because observe access is off or because the action capability is absent;
- non-secret remediation guidance.

---

# 11. Listening-Agent Defaults

## 11.1 Explicit network exposure

`ono --agent` with no `--listen` MUST continue to use stdin/stdout and MUST NOT open a network socket.

A direct TCP socket is opened only with explicit `--listen`.

## 11.2 Bind address

Because `--listen` is already explicit, v0.4.1 MAY accept a non-loopback address directly. However, the process MUST print a clear startup summary including:

```text
bound address
server peer fingerprint
authorization store path
authorized client count
maximum concurrent connections
```

If the authorization store contains zero clients, the agent MAY listen but MUST refuse all connections after cryptographic handshake. It MUST NOT infer authorization from network locality.

## 11.3 No reachability trust

Loopback, RFC1918/private address space, Unix user identity inferred from source port, source IP allowlists, or "same LAN" MUST NOT substitute for cryptographic client authentication.

Network ACLs MAY be used as additional defense but never as the canonical Ono trust decision.

---

# 12. Connection and Resource Limits

## 12.1 Global connection limit

A listening agent MUST have a hard limit on concurrent accepted authenticated connections.

Default:

```text
max_connections = 32
```

The limit MUST include connections that completed TCP accept but are still in TLS/protocol handshake state, using a separate handshake semaphore if required to prevent handshake exhaustion.

## 12.2 Handshake limit and timeout

Defaults:

```text
max_pending_handshakes = 16
handshake_timeout = 10 seconds
```

A peer that does not complete TLS plus Ono protocol negotiation within the timeout MUST be disconnected.

## 12.3 Per-client connection limit

Default:

```text
max_connections_per_client = 4
```

The limit is keyed by authenticated client fingerprint, not source IP.

## 12.4 Stream and credit limits

Existing protocol limits for frame size, value depth, stream count and credit windows MUST remain enforced.

Their defaults MUST be centralized in one `Limits` contract and MUST be printed by a diagnostic command or test fixture.

No code path may construct an effectively unlimited `Limits` instance for a network listener in production.

## 12.5 Revocation behavior

Removing an authorized client MUST prevent all new connections immediately.

The reference implementation SHOULD also close existing direct-TCP connections for that fingerprint within 5 seconds. If live revocation is not implemented, `remove client-key` MUST say clearly that existing connections remain valid until disconnect and an ADR MUST record the limitation.

For v0.4.1 release quality, live revocation is RECOMMENDED but not a P0 blocker if new requests on existing connections remain constrained by the immutable grant that existed at connection time.

## 12.6 Failure isolation

One malformed, unauthorized or slow client MUST NOT terminate the listener or consume unbounded tasks.

Accept-loop errors are reported and the listener continues unless the listening socket itself becomes unusable.

---

# 13. Handshake and Downgrade Resistance

## 13.1 Authentication before Ono `Hello`

For direct TCP, mutual TLS MUST complete before an Ono `Hello` frame is accepted.

This prevents self-reported protocol identity from being processed as if it established trust.

## 13.2 Protocol-version binding

The negotiated Ono protocol version MUST remain within the TLS-protected transcript/channel.

A peer MUST NOT be able to request a legacy unauthenticated protocol mode after mutual TLS has established a v0.4.1 connection.

## 13.3 No automatic fallback

If a v0.4.1 direct client encounters a server that does not support mutual client authentication, it MUST fail.

It MUST NOT retry with no client certificate.

If a legacy diagnostic mode exists, it MUST require an explicit different command path or flag containing the word `legacy` or `unauthenticated`, and MUST print a high-visibility warning. The normal `link` command MUST never select it automatically.

## 13.4 ALPN

The direct transport SHOULD advance from the existing `ono/1` ALPN token to a token that unambiguously represents the mutual-authentication contract, for example:

```text
ono/2
```

If the existing protocol version number is bumped instead and ALPN remains `ono/1`, an ADR MUST demonstrate that downgrade cannot occur before client authentication.

---

# 14. Remote Audit and Diagnostics

## 14.1 Connection events

The listening agent MUST produce structured audit events for:

- successful authenticated connection;
- unknown/unapproved client refusal;
- client-certificate verification failure;
- authorization denial;
- connection-limit denial;
- protocol mismatch;
- client disconnect;
- action execution request and result for authorized actions.

## 14.2 Audit data

Events SHOULD include:

```text
connection_id
peer_fingerprint
peer_label when known
source_address
protocol_version
requested_capability
result
error_code
timestamp
```

They MUST NOT include private keys, full secret environment values or unredacted credentials from provider payloads.

## 14.3 User-facing inspection

`get link` / equivalent link inspection SHOULD show the authenticated fingerprint and authorization state for direct connections.

The words `authenticated`, `authorized`, `pinned` and `self-reported identity` MUST not be conflated.

---

# 15. KUANG/11 Trust Tiers

## 15.1 Required terminology

v0.4.1 distinguishes three concepts:

- **capability mediation:** Ono decides which operations the plugin protocol may ask Ono to perform;
- **process confinement:** Ono installs process-level restrictions such as rlimits, no-new-privileges, session separation and environment/fd hygiene;
- **kernel isolation:** the plugin is prevented by kernel policy from directly accessing filesystem/network resources outside an explicit allowlist.

The existing native process tier provides the first two to varying degrees. It does not, by default, provide the third.

## 15.2 Native trust statement

Documentation MUST state:

> A native KUANG/11 plugin executes as a process of the Ono user. Ono limits its brokered capabilities and applies process confinement, but native execution in v0.4.1 is not a complete filesystem or network sandbox. Install native plugins only from sources you are willing to run as your user account.

Equivalent wording MAY be used, but the security meaning MUST remain.

## 15.3 No accidental trust upgrade

A manifest capability declaration MUST NOT be interpreted as preventing the native process from making equivalent direct syscalls to resources the Unix account can access.

The audit UI MUST distinguish:

```text
brokered capability: denied
native direct OS access: not isolated by this execution tier
```

where relevant to documentation and diagnostics.

---

# 16. Native Process Confinement

## 16.1 Mandatory controls

Before executing a native plugin, the supervisor MUST attempt all controls required by the selected native tier.

The default native tier MUST include at least:

- close-on-exec / fd inheritance hygiene;
- controlled stdin/stdout/stderr protocol descriptors;
- process/session separation as defined by the implementation;
- `PR_SET_NO_NEW_PRIVS` on Linux;
- configured resource limits;
- sanitized environment according to existing KUANG rules;
- working-directory policy;
- process lifetime ownership by the supervisor.

## 16.2 Return-value checking

Every syscall used to establish a mandatory security or resource control MUST have its return value checked.

A wrapper such as `set_limit()` MUST return `Result`, not discard the underlying `setrlimit` result.

The same rule applies to `prctl`, `setsid`, `setpriority` where priority is a promised control, privilege-drop calls and future seccomp/Landlock setup.

## 16.3 Child pre-exec error propagation

Failures that occur in a pre-exec child setup context MUST be propagated to the parent in a way that prevents `exec` of the plugin.

The caller MUST receive a structured error identifying which control could not be installed.

Example error family:

```text
plugin.confinement_failed
plugin.resource_limit_failed
plugin.no_new_privs_failed
```

## 16.4 Mandatory versus best-effort controls

Each control MUST be declared either `mandatory` or `best_effort` in one central table.

For v0.4.1:

```text
no_new_privs              mandatory
rlimit address-space      mandatory when configured by tier
rlimit cpu                mandatory when configured by tier
rlimit open-files         mandatory when configured by tier
rlimit processes          mandatory when configured by tier
session separation        mandatory for the native supervised tier
nice/setpriority          best_effort unless policy explicitly requires it
```

A best-effort failure MUST still be observable in diagnostics but does not prevent spawn.

## 16.5 Confinement report

The supervisor SHOULD build a `ConfinementReport` for every spawn:

```text
control
required
attempted
result
platform_detail
```

A successful plugin spawn MUST imply every `required=true` control has `result=applied`.

This report MAY feed audit and debug output, but MUST not expose secrets.

---

# 17. Optional Stronger Isolation

## 17.1 Not required for v0.4.1

v0.4.1 does not require implementing Landlock, seccomp filtering, user namespaces, network namespaces, containers or WASM solely to claim completion.

Adding those mechanisms without a mature policy model would create new complexity and compatibility risk during a hardening release.

## 17.2 Architectural preparation

The execution-tier model SHOULD nevertheless make future stronger isolation possible without changing plugin protocol semantics.

A future tier MAY be:

```text
native-confined
native-isolated
wasm
```

The v0.4.1 code SHOULD avoid boolean names such as `sandboxed: true` that cannot represent these distinctions.

## 17.3 No security marketing ahead of implementation

Until kernel isolation exists, no native tier may be described simply as "sandboxed" without an immediate qualifier explaining the boundary.

---

# 18. Plugin Failure and Quarantine Semantics

## 18.1 Pre-exec failure

A plugin whose required confinement cannot be installed MUST not enter quarantine, because it never safely started. It receives a launch failure.

## 18.2 Protocol violation

A plugin that starts correctly but sends malformed, oversized or credit-violating protocol frames MAY enter the existing quarantine path.

## 18.3 Resource-limit termination

If the kernel terminates a plugin because of a configured resource limit, Ono MUST classify the exit distinctly from a protocol crash where the platform permits determination.

The error SHOULD identify the enforced resource class, not merely "plugin exited".

## 18.4 Crash containment

Plugin failure MUST not corrupt the shell's provider registry or leave partially registered capabilities visible as healthy.

Any runtime registration contributed by a dead plugin MUST transition to unavailable/failed according to existing provider semantics.

---

# 19. Documentation Terminology Contract

## 19.1 Canonical terms

The following definitions MUST be used consistently in README, Wiki, `help`, generated reference and architecture documentation:

| Term | Meaning |
|---|---|
| authenticated | cryptographic peer proof was verified |
| authorized | authenticated principal is permitted by policy |
| pinned | fingerprint matches a recorded trust decision |
| confined | process-level restrictions were successfully installed |
| isolated | kernel policy prevents direct access outside a defined boundary |
| sandboxed | MAY be used only when the specific isolation boundary is stated |
| bounded | a hard enforceable limit exists for the relevant resource |
| streaming | output may progress before complete upstream exhaustion unless semantics require global state |

## 19.2 Generated documentation

Where command contracts or capability tables already generate reference documentation, the security terms SHOULD be generated from the same registries rather than duplicated in prose.

---

# 20. Security Acceptance Principle

A security control is accepted only when there is an automated negative test proving the forbidden behavior is refused.

Examples:

```text
unknown TLS client           -> cannot negotiate providers
known client, query access   -> read succeeds
known client, no action      -> Act refused
known client, exact action   -> only granted capability succeeds
changed client key           -> refused
failed no_new_privs setup    -> plugin never execs
malformed auth store         -> agent refuses to start/listen permissively
```

Positive tests alone are insufficient for every P0 boundary.

---

# 21. Resource Budget Model

## 21.1 One shared budget abstraction

v0.4.1 MUST introduce a shared budget abstraction for operations that retain or materialize values.

The logical model is:

```text
Budget {
    max_items: Option<u64>,
    max_bytes: Option<u64>,
    consumed_items: u64,
    consumed_bytes: u64,
}
```

A limit of `None` is permitted only for internal/test contexts where unboundedness is explicit in the type or constructor name. Production interactive paths MUST NOT accidentally obtain an unlimited budget through a default constructor.

## 21.2 Deterministic value-size estimation

`Value` MUST expose, directly or through a utility, an approximate retained-size estimator.

The estimator MUST:

- include string/byte payload lengths;
- recursively account for list/map/object contents;
- avoid double-counting shared `Arc` data within one estimation traversal where practical;
- cap recursion using the same or stricter depth rules used for serialization;
- be deterministic for the same value;
- never intentionally undercount known payload bytes.

The result need not equal allocator RSS. Its purpose is to enforce predictable logical payload limits.

## 21.3 Budget-exceeded semantics

When a budget is exceeded, Ono MUST not continue collecting while merely warning.

The operation MUST either:

1. stop and return a structured resource-limit error; or
2. for a defined cache/history use case, evict older entries according to a documented policy.

The two behaviors MUST not be mixed implicitly.

## 21.4 Stable errors

Required error families:

```text
resource.item_limit
resource.byte_limit
resource.materialization_limit
```

Errors SHOULD include the configured limit and observed/estimated consumption without dumping the retained values themselves.

---

# 22. Materialization Contract

## 22.1 Global operations may materialize

Operations whose semantics require global knowledge MAY materialize a finite upstream stream.

Examples include:

- full sort;
- exact global median/percentile where no streaming algorithm is selected;
- global group operations whose output depends on complete membership;
- explicit collection into a list;
- operations documented as finite-set transforms.

## 22.2 Default materialization limits

The reference implementation defaults are:

```text
materialize.max_items = 100000
materialize.max_bytes = 134217728   # 128 MiB
```

Both limits apply; the first reached wins.

These values MAY be overridden by configuration, but the configuration surface MUST preserve a finite default and MUST validate ranges.

A value of zero means "no values permitted", not unlimited.

An explicit keyword such as `unlimited` MAY exist only for expert configuration and MUST NOT be accepted in security-sensitive remote agent mode without a separate explicit override.

## 22.3 Finite-stream requirement

A command that must see the entire stream MUST know or establish that the upstream is finite.

If the stream is marked `Unbounded`, the operation MUST refuse immediately with an error explaining that it requires finite input.

It MUST NOT wait forever to discover that an unbounded stream never ends.

## 22.4 Explain visibility

`explain` MUST expose materialization when it affects execution semantics.

Example conceptual output:

```text
sort memory desc
  execution: global materialization
  requires: finite input
  budget: 100000 values / 128 MiB
```

This is a product feature of honesty, not merely debug output.

---

# 23. Capture Buffers

## 23.1 Capture is not an invisible unlimited vector

Any evaluator mechanism that captures pipeline output for later use MUST use the shared materialization budget.

No new direct `Vec<Value>` capture path may be added without an explicit budget wrapper.

## 23.2 Capture defaults

Unless a narrower operation-specific bound is defined, captures use the global materialization defaults from section 22.2.

## 23.3 Cancellation

Cancellation while capturing MUST stop upstream consumption promptly and release retained values as soon as the owning operation unwinds.

Target cancellation latency under a responsive provider is:

```text
p95 < 100 ms
p99 < 250 ms
```

The benchmark MUST measure from cancellation signal to cessation of additional captured-value growth.

## 23.4 Capture nesting

Nested captures MUST not each independently consume the full global allowance without accounting.

The evaluator SHOULD use hierarchical budgets so child captures borrow from a parent execution budget.

At minimum, a single shell command MUST have a documented upper bound on the total bytes retained by simultaneous evaluator captures.

Reference command-level default:

```text
command.capture.max_bytes = 256 MiB
```

This is a ceiling across nested capture contexts, not an invitation for each capture to allocate 256 MiB.

---

# 24. Retained Result History

## 24.1 Existing behavior preserved with a byte ceiling

The session MAY continue retaining recent structured results for interactive inspection.

The reference implementation keeps the existing conceptual limits:

```text
history.max_results = 16
history.max_items_per_result = 10000
```

v0.4.1 adds:

```text
history.max_bytes_per_result = 16 MiB
history.max_bytes_total = 64 MiB
```

## 24.2 Eviction policy

Result history is a cache, not a correctness requirement. It therefore uses eviction rather than failing the user's command.

Rules:

1. the live pipeline result is never truncated merely to fit history;
2. history retention for a result stops when its per-result item or byte cap is reached;
3. the stored result is marked `truncated_for_history=true`;
4. oldest history entries are evicted until the total byte budget is satisfied;
5. a single value larger than the per-result history byte limit is not retained, but it still flows through the pipeline normally.

## 24.3 User visibility

If the user inspects a history entry that was truncated for retention, Ono MUST say so.

It MUST NOT present the retained subset as though it were the complete original output.

---

# 25. `each` Streaming Semantics

## 25.1 Canonical behavior

Block-based `each` MUST process values incrementally.

Given:

```text
source | each { transform $it } | downstream
```

Ono MUST be able to begin executing `transform` for the first value before `source` has completed, provided `source` has produced that value.

## 25.2 No full-input capture

The normal `each` implementation MUST NOT capture the complete upstream stream into a `Vec<Value>` before block execution.

## 25.3 Ordered output

The default `each` behavior remains serial and preserves input order unless an existing specification explicitly says otherwise.

v0.4.1 does not introduce parallel `each` as a feature.

## 25.4 Block output

If a block emits zero, one or many values for one input item, those values MUST be forwarded before the next input item is required, subject to downstream backpressure.

The implementation SHOULD model each block invocation as a small streaming/capture scope only where the block semantics require knowing the block's complete result for that individual item.

It MUST NOT collect results for all upstream items merely because a later pipeline stage exists.

## 25.5 Break, continue, return and error

Control-flow semantics MUST remain exact:

- `continue` skips the remainder of the current item;
- `break` stops consuming upstream and cancels the remaining source where possible;
- `return` exits the containing function according to existing language semantics;
- an unhandled error follows existing error propagation and MUST stop or continue exactly as the language contract defines.

The streaming rewrite MUST have dedicated regression tests for every control-flow path.

## 25.6 Unbounded sources

`each` MUST accept an unbounded stream because its semantics are incremental.

This is an acceptance criterion that distinguishes a real streaming implementation from the previous capture-based path.

## 25.7 Time to first output

For a synthetic source that emits the first item immediately and then waits, an identity `each` pipeline MUST emit its first result before the source closes.

The acceptance test MUST fail if output appears only after upstream completion.

---

# 26. Function and Block Pipeline Streaming

## 26.1 General rule

The fix for `each` MUST not remain a special-case island if the evaluator contains the same materialize-then-forward pattern for functions or blocks whose semantics do not require global results.

The implementation MUST inventory every `Vec<Value>` or equivalent capture in evaluator execution paths and classify it as:

```text
semantic materialization
implementation convenience
history/cache
```

All `implementation convenience` captures on pipeline data MUST be removed or bounded and justified by ADR.

## 26.2 Function invocation

A function used as a pipeline stage SHOULD be able to stream values to downstream stages when the function body itself streams.

If function semantics currently require a complete function result before continuation, that limitation MUST be explicit in `explain` and MUST have a finite-input/budget guard.

The preferred v0.4.1 outcome is streaming continuation rather than preservation of an accidental capture architecture.

## 26.3 Scope lifetime

Streaming a block/function MUST NOT let lexical scope references outlive their owning scope unsafely.

The evaluator MAY introduce an execution frame object whose lifetime covers the asynchronous stream producer.

The refactor MUST preserve deterministic variable binding and mutation semantics.

---

# 27. Pipeline Event Ordering

## 27.1 Per-channel ordering

The existing guarantee that values preserve their value-channel order and errors preserve their error-channel order MUST remain.

## 27.2 Cross-kind ordering

v0.4.1 MUST make the cross-kind ordering contract explicit.

The reference contract is:

> `StreamEvent` does not promise a total temporal ordering between independently produced value and partial-error channels unless a producer explicitly serializes them through one event source.

Consumers MUST NOT infer causality from the relative observation order of a value and an asynchronously reported partial error.

## 27.3 When total order is required

A provider or operation that needs to express "error occurred between value A and value B" as part of its semantic contract MUST emit an ordered event stream through one sequence-bearing path rather than rely on Tokio scheduling between two channels.

## 27.4 Documentation and tests

The stream module documentation MUST state this rule, and concurrency tests MUST prove only the guarantees actually promised.

Tests MUST NOT accidentally hard-code a stronger cross-channel order that the implementation does not guarantee.

---

# 28. Backpressure and Cancellation

## 28.1 Bounded channels remain mandatory

The default pipeline data path MUST continue to use bounded channels.

The reference capacity remains:

```text
pipeline.channel_capacity = 64
```

Changing this number for tuning MAY occur through an ADR and benchmark evidence, but replacing bounded flow with unbounded channels is forbidden.

## 28.2 Backpressure across new streaming paths

The `each`/function streaming changes MUST propagate downstream backpressure upstream. They MUST NOT solve materialization by inserting an unbounded task queue.

## 28.3 Cancellation wins

When cancellation and capacity availability race, cancellation SHOULD win such that a cancelled producer does not continue to enqueue a large tail of values.

Existing cancellation semantics MUST be preserved and regression tested around the refactored evaluator paths.

## 28.4 Child process cancellation

Where a streaming pipeline stage owns an external process, cancellation MUST close or signal it using the existing process/job-control policy rather than leaving an orphan merely because downstream stopped reading.

---

# 29. Parser Structural Refactoring

## 29.1 Objective

The parser is functionally strong but has reached a module size where local changes carry excessive cognitive scope.

v0.4.1 MUST perform a structural decomposition without changing grammar semantics.

## 29.2 Required module boundaries

The reference structure SHOULD approach:

```text
ono-parser/src/parser/
    mod.rs
    state.rs
    statements.rs
    expressions.rs
    pipelines.rs
    blocks.rs
    literals.rs
    recovery.rs
    diagnostics.rs
```

Exact filenames MAY differ, but the following responsibilities MUST become separately navigable:

- parser state/token access;
- statement parsing;
- expression parsing and precedence;
- pipelines/commands;
- blocks/functions/control constructs;
- recovery/incomplete-input logic;
- diagnostic construction.

## 29.3 No rewrite

This work is explicitly not a parser rewrite.

The recursive-descent strategy, recovery behavior, incomplete-input semantics, recursion-depth guard and AST contracts MUST remain unless an independent bug fix has a failing test first.

## 29.4 Diff discipline

Parser decomposition SHOULD be performed in moves/extractions with minimal behavioral edits.

Each extraction commit SHOULD pass the existing parser tests before further semantic work.

---

# 30. Evaluator Structural Refactoring

## 30.1 Objective

`ono-cli` may remain the composition root, but evaluator orchestration MUST no longer concentrate statements, expressions, pipelines, functions, blocks and native execution in one large module.

## 30.2 Required responsibility split

The target conceptual structure is:

```text
eval/
    mod.rs
    statement.rs
    expression.rs
    pipeline.rs
    block.rs
    function.rs
    control.rs
    native.rs
    materialize.rs
```

The `materialize` module SHOULD own budget-aware finite collection helpers so no caller recreates them ad hoc.

## 30.3 `Flow` remains explicit

The existing explicit control-flow representation for normal status, `break`, `continue`, `return` and `exit` is a strength and MUST be preserved or strengthened.

The refactor MUST NOT replace it with magic error strings, panics or implicit flags.

## 30.4 No architecture inversion

The refactor MUST not move domain logic from lower-level crates into `ono-cli` merely to reduce file size.

File decomposition is subordinate to correct crate boundaries.

---

# 31. Session State Segmentation

## 31.1 Session remains a first-class shell concept

v0.4.1 does not attempt to make the shell stateless. A shell session legitimately owns mutable state.

The goal is to make categories of state explicit.

## 31.2 Internal state groups

`Session` SHOULD internally compose state groups equivalent to:

```text
EnvironmentState
ScopeState
ExecutionState
NavigationState
ResultHistoryState
JobState
ProviderState
PresentationState
```

The public API MAY continue to expose convenient methods on `Session` so callers do not need to know the internal split.

## 31.3 Mutation locality

Each state group SHOULD own the invariants for its data.

For example, result-history byte-budget enforcement belongs in `ResultHistoryState`, not scattered across evaluator call sites.

## 31.4 Serialization

No new requirement is introduced to serialize a complete `Session`. Internal segmentation MUST not accidentally turn ephemeral handles, runtimes or jobs into serializable state.

---

# 32. Spatial Performance Contract

## 32.1 Performance is a curve

v0.4.1 MUST stop treating one small fixture passing a latency budget as sufficient proof that a spatial operation is performant.

Every performance-sensitive spatial path MUST be measured across increasing cardinalities.

## 32.2 Reference cardinality profiles

The automated performance suite MUST include at least:

```text
Profile S   100 processes,     500 graph nodes,     2,000 edges
Profile M   1,000 processes,   5,000 graph nodes,  25,000 edges
Profile L   10,000 processes, 50,000 graph nodes, 250,000 edges
```

Fixtures MAY synthesize entities where real host creation is impractical, but provider/planner code exercised by the benchmark MUST match production logic.

Socket-specific profiles MUST include at least:

```text
1,000
10,000
100,000 sockets
```

## 32.3 Required metrics

Each benchmark MUST record:

```text
time to first value
time to completion
peak or sampled RSS where practical
values per second
allocated/estimated bytes where available
cancellation latency
```

A single total runtime number is insufficient for streaming operations.

## 32.4 Regression baselines

Performance results MUST be stored in a machine-readable baseline file tied to the reference environment.

CI MAY use percentage thresholds rather than exact wall-clock values on shared runners, but release qualification MUST run on a named reference environment with stable absolute targets.

---

# 33. Time-to-First-Result Targets

## 33.1 Interactive priority

For interactive system exploration, time to first useful result is a first-class target.

The user should see progress before a complete high-cardinality world has been constructed when the semantics permit partial output.

## 33.2 Reference targets

On the release reference environment:

```text
basic cached look/near first result            < 50 ms p95
spatial query Profile M first result           < 150 ms p95
map live Profile M initial visible frame       < 500 ms p95
map live Profile L initial progress/summary    < 1.5 s p95
```

For operations that cannot produce the final representation incrementally, they MUST at least emit progress metadata or a deterministic cost/refusal message before 1.5 seconds on Profile L.

## 33.3 No silent 30-second blank state

A supported interactive operation MUST NOT spend 30 seconds producing neither output nor progress on the reference Profile M/L fixtures.

If the planner predicts cost beyond the supported interactive budget, Ono MUST refuse or switch to a bounded lower-detail strategy rather than silently appear hung.

---

# 34. Spatial Query Cost Model

## 34.1 Cost estimation

The spatial planner SHOULD compute a coarse cost estimate before expanding expensive relationships.

The estimate MAY use:

- candidate node count;
- expected edge fan-out;
- selector selectivity;
- relationship acquisition cost class;
- cache state;
- requested depth/detail.

It need not be mathematically exact. It MUST be conservative enough to avoid obviously explosive work.

## 34.2 Cost classes

Relationship/provider acquisition SHOULD classify cost as:

```text
cheap
moderate
expensive
external
```

The class MUST be machine-readable.

## 34.3 Expensive relation behavior

If a relationship is described as "available on request", there MUST actually be a request path.

v0.4.1 MUST resolve any state where `follow <relation>` refuses because the relation is expensive while another unrelated command can acquire it but the user has no way to request it through `follow`.

Canonical approaches are:

```text
follow owner --resolve
```

or a globally consistent equivalent such as:

```text
follow owner --include-expensive
```

The exact flag MUST follow existing command option conventions and be recorded in the command contract.

## 34.4 No hidden global graph build for local questions

A local neighborhood query SHOULD NOT require construction of the complete system graph when provider APIs can answer the neighborhood incrementally.

Any unavoidable global build MUST be visible in `explain` and covered by materialization/performance budgets.

---

# 35. `map --live` Stabilization

## 35.1 Release blocker behavior

The known class of behavior where `map --live` produces no bytes for tens of seconds on a realistic host MUST be eliminated for v0.4.1.

## 35.2 Bounded initial projection

`map --live` MUST construct an initial projection with a bounded work budget.

The UI MAY progressively refine after the first frame.

A first frame does not need every edge if the chosen semantic zoom level intentionally aggregates detail, but it MUST be truthful about omitted/pending detail.

## 35.3 Incremental updates

After the initial map, live changes SHOULD be applied as deltas rather than full graph recomputation when the provider event model supplies sufficient identity.

## 35.4 Backpressure

Live map update streams MUST use bounded queues. If updates arrive faster than rendering, the system MAY coalesce state updates where only latest state matters, but MUST not coalesce semantically distinct events in a way that produces false topology.

## 35.5 Cancellation

Ctrl-C MUST end a live map promptly without waiting for a complete expensive recomputation.

---

# 36. Selector and Completion Performance

## 36.1 Selector misses

A selector miss MUST not be substantially more expensive than a hit solely because the system scans an unnecessarily complete global candidate set.

Profile M target:

```text
selector miss p95 < 250 ms
```

Profile L target:

```text
selector miss p95 < 1 s
```

If the system cannot meet Profile L without indexing, v0.4.1 MUST add the necessary index or a bounded candidate strategy for canonical selectors.

## 36.2 Completion budget

Interactive completion MUST have a hard wall-clock work budget.

Default:

```text
completion.soft_budget = 50 ms
completion.hard_budget = 150 ms
```

At the soft budget, completion MAY return a partial set marked incomplete. At the hard budget it MUST stop additional discovery work and return what it has.

Completion MUST never block indefinitely on remote or expensive spatial acquisition.

---

# 37. Performance Benchmark Infrastructure

## 37.1 Dedicated benchmark command

The repository SHOULD expose performance fixtures through `xtask`, for example:

```text
cargo xtask perf
cargo xtask perf --profile M
cargo xtask perf --compare baseline.json
```

Exact syntax MAY differ, but benchmark execution must be discoverable and reproducible.

## 37.2 Release reference environment

The release documentation MUST name the hardware/software environment used for absolute performance gates:

```text
CPU model / core count
RAM
kernel version
distribution/container image
Rust toolchain
release build flags
```

## 37.3 Warm and cold measurements

Benchmarks MUST distinguish:

- cold startup / uncached query;
- warm process with provider initialized;
- cache-hit behavior where caches are part of product semantics.

A warm-cache number MUST not be advertised as cold performance.

## 37.4 Statistical rule

Performance acceptance SHOULD use at least 20 iterations for short benchmarks and report median plus p95.

Single-run best-case timings MUST NOT define release success.

---

# 38. Test Outcome Truthfulness

## 38.1 Three visible outcomes

Environment-dependent tests MUST distinguish:

```text
PASS
FAIL
SKIP(reason)
```

A skip MUST not be encoded as a function returning early while the test harness reports success without any skip marker.

## 38.2 Canonical CI expectation

For the repository's canonical CI environment, the expected skip set MUST be declared.

The preferred expected count is zero for tests whose prerequisites can reasonably be supplied by CI.

If non-zero skips are intentional, their IDs and reasons MUST be listed in a machine-readable file.

## 38.3 Unexpected skip is failure

A test that becomes skipped when it was expected to run MUST fail the CI gate or an explicit skip-verification step.

## 38.4 Skip reason taxonomy

Reasons SHOULD be stable categories such as:

```text
missing_kernel_feature
missing_privilege
unsupported_arch
unsupported_distribution
external_tool_unavailable
fixture_not_applicable
```

Free-form text MAY accompany the category.

---

# 39. Test Helper Architecture

## 39.1 Shared helpers are production-quality test infrastructure

Common integration-test helpers such as invoking `ono`, decoding rows and constructing fixtures MUST live in shared modules rather than dozens of near-identical local variants.

## 39.2 No helper divergence

The gate MUST include a lightweight structural check preventing reintroduction of known duplicated helper definitions where a canonical helper exists.

This check SHOULD target semantics/signatures rather than fragile exact source strings.

## 39.3 Helper contracts

Shared helpers MUST document:

- whether they use debug or release binaries;
- environment isolation;
- timeout behavior;
- stdout/stderr capture semantics;
- locale and terminal assumptions;
- how skip prerequisites are reported.

## 39.4 Test code review

A production-code change that adds a new test helper duplicating an existing capability SHOULD fail review/gate policy unless an ADR or test-infrastructure note justifies the difference.

---

# 40. Acceptance Tests

## 40.1 Real binary remains mandatory

Container acceptance cases MUST continue to execute the real `ono` binary, not mock command dispatch at a lower layer.

## 40.2 Network default

Acceptance containers SHOULD continue to run without network access unless the case explicitly tests network behavior.

Remote direct-link tests MUST use an isolated local/container network created for the case and MUST not depend on public internet availability.

## 40.3 New v0.4.1 acceptance families

The acceptance suite MUST add named cases for:

- direct mutual TLS client/server authentication;
- unknown client refusal;
- authorization-constrained capability negotiation;
- unauthorized action refusal;
- authorized exact action success;
- changed client key refusal;
- malformed authorization-store fail-closed startup;
- KUANG mandatory confinement setup failure;
- `each` streaming from an unbounded/delayed source;
- materialization item and byte limit refusal;
- result-history truncation visibility;
- Profile M spatial first-result target;
- live map cancellation under load;
- package signature/checksum/provenance generation.

## 40.4 Timeouts

Every acceptance case MUST have a finite timeout.

A timeout is a failure unless the case explicitly asserts timeout behavior as its product result.

---

# 41. Fuzzing Strategy

## 41.1 Fast deterministic gate retained

The existing lightweight/deterministic fuzz targets remain valuable and MUST stay in the normal gate where they are fast enough.

## 41.2 Coverage-guided tier

v0.4.1 MUST add a scheduled coverage-guided fuzzing tier using `cargo-fuzz`/libFuzzer or an equivalent Rust-compatible engine.

At minimum, targets MUST cover:

```text
parser/lexer entry points
Value deserialization
remote frame decoder
remote handshake decoder
KUANG frame decoder
procfs/netlink or equivalent structured system-data decoders
adapter machine-readable decoders with attacker-controlled bytes
```

## 41.3 Schedule

The scheduled fuzz job MUST run at least daily on the default branch or for a minimum aggregate time of 30 minutes per day across the critical targets.

Release qualification SHOULD include a longer run of at least 2 CPU-hours aggregate.

## 41.4 Corpus persistence

Interesting corpus inputs and every minimized crash reproducer MUST be committed or stored as durable CI artifacts and promoted into regression tests when they represent a fixed bug.

## 41.5 Hangs

Coverage-guided fuzz targets MUST enforce per-input timeouts where supported so pathological hangs become findings, not merely long CI jobs.

---

# 42. Miri and Sanitizer Verification

## 42.1 Unsafe boundary focus

Because unsafe code is intentionally concentrated, v0.4.1 MUST exploit that architecture with targeted verification.

## 42.2 Miri

A scheduled Miri job SHOULD cover safe testable subsets of crates around:

- value ownership/sharing;
- parser data structures;
- protocol serialization;
- code that can run under Miri without unsupported OS syscalls.

Miri is not required to execute the real process/job-control layer.

## 42.3 Sanitizers

Linux scheduled CI SHOULD run AddressSanitizer and UndefinedBehaviorSanitizer on selected integration tests where Rust/toolchain support permits.

The unsafe process crate and FFI/syscall wrappers are priority targets.

## 42.4 Failure handling

A reproducible sanitizer or Miri finding is a release blocker until fixed or proven false-positive and documented.

---

# 43. CI Workflow Hardening

## 43.1 Immutable GitHub Action references

Every third-party GitHub Action used by release or required CI workflows MUST be pinned to a full commit SHA.

Example:

```yaml
uses: actions/checkout@<40-hex-commit>
```

A trailing comment MAY record the human-readable release tag:

```yaml
uses: actions/checkout@<sha> # v4.x.y
```

Major tags such as `@v4` or `@v2` are not sufficient for the hardened workflows.

## 43.2 First-party scripts preferred for critical logic

Critical release logic SHOULD live in repository-owned scripts where practical, with GitHub Actions orchestrating them.

An external action MUST not be the only implementation of artifact signing, checksum generation or package validation if a small auditable repository script can perform the task.

## 43.3 Permissions

Workflow permissions MUST use least privilege.

`contents: write` MUST be granted only to the publishing job/step that requires it.

Build/test jobs SHOULD use `contents: read` and no unnecessary token scopes.

## 43.4 Pull-request trust

Workflows triggered by untrusted pull requests MUST NOT expose release signing credentials, write tokens or other privileged secrets.

`pull_request_target` MUST NOT be used to execute untrusted repository code with elevated permissions.

## 43.5 Concurrency

Release publication MUST use a concurrency guard or tag uniqueness rule preventing two jobs from publishing conflicting artifacts for the same version/tag.

---

# 44. Build Image and Tool Pinning

## 44.1 Container digests

Container images used for release-critical builds or package validation MUST be pinned by digest.

Human-readable tags MAY remain as comments or variables, but the actual pull reference MUST include `@sha256:...`.

Examples of mutable references that MUST be removed from release-critical paths include:

```text
fedora:latest
rust:<version>-slim-bookworm   # without digest
```

## 44.2 Tool versions

Tools installed during CI or packaging MUST use exact versions where they can affect artifacts or verification semantics.

This includes packaging tools, signing tools, cargo subcommands and code generators.

## 44.3 Rust toolchain

The existing pinned Rust toolchain file remains authoritative.

Release builds MUST use that exact toolchain and `cargo build --locked` or the equivalent locked workspace command.

## 44.4 Dependency fetch reproducibility

`Cargo.lock` MUST be committed and used for release builds.

A release build MUST fail if lockfile resolution would change.

---

# 45. Dependency and Supply-Chain Policy

## 45.1 Advisory scanning

The gate MUST include a Rust dependency advisory check using a maintained advisory source/tool.

A known vulnerability affecting compiled production code is a release blocker unless an ADR/security note documents why the vulnerable path is unreachable and defines a removal deadline.

## 45.2 License/source policy

The repository SHOULD use `cargo-deny` or an equivalent policy tool to validate:

- allowed licenses;
- banned/duplicate dependency policy where relevant;
- advisories;
- source registries/git dependencies.

## 45.3 Git dependencies

Production release dependencies sourced directly from Git MUST be pinned to immutable revisions.

Floating branches/tags are forbidden.

## 45.4 New cryptographic dependencies

Dependencies involved in TLS, signing, hashing or key handling require explicit ADR/security review before introduction or major implementation change.

---

# 46. Reproducible Build Contract

## 46.1 Definition

A v0.4.1 release artifact is reproducible when rebuilding the same source commit with the documented release environment and inputs yields an artifact that is byte-for-byte identical, or when an explicitly documented unavoidable container/archive metadata field is normalized by the packaging process such that the final distributable is identical.

The target is byte-for-byte identity for:

```text
.deb
.rpm
standalone binary archives where published
checksum manifests
```

## 46.2 Source date

The packaging pipeline MUST continue to derive `SOURCE_DATE_EPOCH` from the release commit/tag timestamp or another deterministic source.

No wall-clock build time may leak into an artifact field when a reproducible equivalent exists.

## 46.3 Locale and timezone

Release builds MUST set deterministic locale and timezone environment values where build/package tools may embed them.

Reference values:

```text
LC_ALL=C.UTF-8
TZ=UTC
```

## 46.4 File ordering and ownership

Archive/package file ordering, ownership, group, modes and mtimes MUST be deterministic.

The packaging scripts MUST not inherit arbitrary developer workstation UID/GID metadata.

## 46.5 Rebuild verification

Release qualification MUST build every release artifact twice in fresh clean environments and compare hashes.

A mismatch MUST fail the release check and produce a diagnostic identifying which files/archive members differ where tooling permits.

## 46.6 Cross-architecture builds

Each supported release architecture MUST satisfy reproducibility independently.

Reproducibility is not proven merely because x86_64 is stable if aarch64 artifacts differ between clean builds.

---

# 47. Checksums, Signatures and Provenance

## 47.1 Required release files

Every GitHub release MUST publish, in addition to packages, at least:

```text
SHA256SUMS
SHA256SUMS.sig or equivalent verifiable signature
build-provenance.json / .intoto.jsonl or equivalent signed provenance
```

## 47.2 Checksum manifest

`SHA256SUMS` MUST contain the SHA-256 digest of every downloadable executable/package artifact in the release.

The manifest ordering MUST be deterministic.

## 47.3 Signing model

The reference implementation SHOULD use keyless OIDC-backed signing through Sigstore/Cosign or an equivalent system that does not require a long-lived private signing key stored as a repository secret.

If another signing model is selected, an ADR MUST define key custody, rotation, revocation and offline verification.

## 47.4 Provenance

Provenance MUST bind at least:

```text
repository
source commit
release tag
workflow identity
builder/toolchain version
artifact digest
build timestamp
```

The provenance MUST be generated by the trusted release workflow rather than supplied by untrusted build output.

## 47.5 User documentation

The Wiki/install documentation MUST show how to verify checksums and signatures before package installation.

Verification instructions SHOULD fit in a short copyable sequence and MUST not require a proprietary service.

---

# 48. Packaging Validation

## 48.1 Existing real-install checks remain

The current pattern of installing generated `.deb` and `.rpm` packages in clean distribution environments MUST remain part of release qualification.

## 48.2 New validation

Package checks MUST also verify:

- binary version equals release version;
- expected path `/usr/bin/ono` or defined package path exists;
- file ownership/mode are correct;
- no private build paths are embedded where avoidable;
- package metadata version/architecture match artifact filename;
- uninstall removes package-owned files without deleting user configuration;
- reinstall works;
- login-shell smoke behavior remains valid;
- checksum manifest matches the uploaded file.

## 48.3 Dependency floor

Package validation MUST run on the oldest supported glibc/distribution baseline as well as one current representative distribution.

The oldest supported baseline is the binding compatibility proof.

## 48.4 Artifact identity

The artifact tested by package validation MUST be the same bytes later uploaded to the release.

The workflow MUST NOT rebuild packages after tests and then upload the untested rebuild.

---

# 49. Release Workflow Architecture

## 49.1 Build once, promote after proof

The preferred release pipeline is:

```text
tag/source commit
    -> immutable build environment
    -> build artifacts
    -> hash artifacts
    -> acceptance/package tests against those artifacts
    -> reproducibility rebuild comparison
    -> sign hashes/artifacts + generate provenance
    -> publish the already-tested bytes
```

## 49.2 No hidden local release step

A public release MUST be reproducible from repository automation. It MUST NOT require a maintainer to run an undocumented local packaging/signing command that changes artifact content.

## 49.3 Release candidate versus final

Release candidates MAY use prerelease tags. Final publication MUST rerun the complete release check on the final source commit/tag even when an earlier RC passed.

## 49.4 Failure atomicity

A failed publishing step SHOULD avoid leaving a partially populated final GitHub release that appears complete.

The workflow MAY create a draft release, upload everything, verify asset inventory and only then publish it.

---

# 50. Generated Repository Metrics

## 50.1 No manually maintained volatile counts

Counts such as:

```text
number of crates
number of unit/integration tests
number of acceptance cases
number of ADRs
number of generated command contracts
```

MUST NOT be manually duplicated across README/docs without automated verification.

## 50.2 Canonical generator

`xtask` or an equivalent repository-owned tool MUST compute the current metrics.

Example output:

```text
crates=30
tests=...
acceptance_cases=...
adrs=...
```

## 50.3 README integration

The README MAY contain the numbers because they are useful evidence, but the gate MUST fail if the numbers do not match generated truth.

Alternatively the relevant block MAY be generated automatically. The original narrative text around it remains human-owned.

## 50.4 No vanity metric substitution

Generated test counts MUST distinguish executed tests from skips where the harness exposes that distinction.

A README statement such as "N tests pass" MUST not count skipped cases as proof of execution.

---

# 51. Documentation Truth and Security Claims

## 51.1 Security documentation gate

The repository MUST include a check for forbidden or qualified terms where they could overstate implementation.

Examples requiring review:

```text
sandboxed execution
fully isolated
unlimited
authenticated
reproducible
```

The goal is not to ban these words. The goal is to ensure they refer to a defined contract.

## 51.2 README correction

The KUANG/11 README description MUST be changed so native execution is not presented as an unspecified complete sandbox.

## 51.3 Remote documentation

Remote-link documentation MUST distinguish:

- SSH-carried stdio transport;
- direct mutual-TLS transport;
- server host pinning;
- client authorization;
- runtime user/UID metadata;
- capability negotiation.

## 51.4 Security page

The project SHOULD add or strengthen a `SECURITY.md` explaining:

- supported versions;
- how to report vulnerabilities privately;
- high-level trust boundaries;
- that public issues are inappropriate for an unpatched exploitable vulnerability.

This file does not replace the architectural security specification.

---

# 52. Machine-Readable Hardening Contracts

## 52.1 New contract domains

Where practical, v0.4.1 MUST encode its policy data in machine-readable registries rather than prose-only constants.

Required or strongly recommended registries include:

```text
security_boundaries
remote_limits
materialization_limits
kuang_confinement_controls
performance_profiles
expected_test_skips
release_inputs
```

## 52.2 Single source of truth

Runtime defaults, generated help/reference and tests SHOULD consume the same source of truth.

A number such as `max_connections = 32` MUST not be independently typed into five files if one contract can generate the others.

## 52.3 Contract validation

`scripts/gate.sh` MUST validate every machine-readable contract for schema correctness and cross-reference integrity.

Unknown capability IDs in an authorization fixture or unknown control IDs in a KUANG tier definition MUST fail the gate.

---

# 53. Error Model Extensions

## 53.1 Stable errors are part of automation

v0.4.1 MUST add all required new failures through the canonical error registry.

At minimum the implementation needs stable distinctions equivalent to:

```text
remote.unauthenticated
remote.unauthorized
remote.capability_denied
remote.connection_limit
remote.handshake_timeout
plugin.confinement_failed
plugin.resource_limit_failed
resource.item_limit
resource.byte_limit
resource.materialization_limit
release.verification_failed   # if exposed by repository tooling/runtime
```

Exact names may be reconciled with existing naming conventions, but the failure classes MUST remain distinct.

## 53.2 No string matching for policy

Internal callers MUST match error codes/types, not human-readable messages, to determine whether an authentication, authorization or resource failure occurred.

## 53.3 Error details

Error detail fields MAY include limits, fingerprints and capability IDs. They MUST avoid secrets.

A fingerprint is public identity material and MAY be shown in full.

---

# 54. Observability of the Hardening Layer

## 54.1 Explainable refusal

A refusal should tell the user which boundary made the decision.

Examples:

```text
remote client sha256:... is authenticated but not authorized for service.restart

plugin foo did not start: mandatory control no_new_privs could not be installed

sort requires finite input; upstream is declared unbounded

result history kept 10,000 of 84,212 values because the 16 MiB history budget was reached
```

## 54.2 No debug-only dependence

Important refusal explanations MUST appear in normal structured errors. Users must not need `RUST_LOG=debug` to understand why a security policy denied them.

## 54.3 Optional diagnostics command

A diagnostic surface such as `inspect limits`, `get limit`, or existing equivalent SHOULD expose effective non-secret runtime limits.

The exact target MUST fit the command registry; this is secondary to having the values accessible to tests and `explain`.

---

# 55. Configuration Surface

## 55.1 Hardening configuration is finite by default

The reference implementation SHOULD expose configurable resource limits through the normal declarative config layer.

Canonical logical keys:

```text
limits.materialize_items = 100000
limits.materialize_bytes = 134217728
limits.history_results = 16
limits.history_items_per_result = 10000
limits.history_bytes_per_result = 16777216
limits.history_bytes_total = 67108864
limits.remote_connections = 32
limits.remote_pending_handshakes = 16
limits.remote_connections_per_client = 4
limits.remote_handshake_timeout_ms = 10000
limits.completion_soft_ms = 50
limits.completion_hard_ms = 150
```

The exact config syntax follows existing Ono config semantics.

## 55.2 Validation

Numeric limits MUST be range-checked at configuration load time.

Invalid limits cause a config diagnostic and fall back only according to existing config-layer error rules. A security-sensitive agent limit MUST NOT silently become unlimited because a value failed to parse.

## 55.3 Agent authorization path

The authorization-store path MAY be configurable by an explicit agent CLI option and/or declarative configuration, but the default path remains as defined in section 9.2.

## 55.4 Environment overrides

New security-sensitive environment variables SHOULD be avoided unless they are necessary for container/service deployment. If added, their precedence MUST follow the existing configuration architecture and be documented.

---

# 56. Reference Crate Architecture Changes

## 56.1 `ono-remote`

Expected responsibilities after v0.4.1:

- symmetric TLS peer identity;
- client/server certificate proof;
- connection listener limits/handshake timeout primitives where transport-specific;
- authenticated fingerprint exposure to protocol/server layer;
- no authorization policy semantics beyond transporting authenticated identity.

## 56.2 `ono-protocol`

Expected responsibilities:

- wire limits;
- protocol negotiation;
- trust-decision primitives;
- authenticated peer metadata carried into `Negotiated`/server connection context;
- protocol-level rejection codes;
- no filesystem path policy.

## 56.3 `ono-cli`

Expected responsibilities:

- locating identity/trust/authorization files;
- user commands to manage authorized client keys;
- building the agent's effective authorization policy;
- composition of provider registry and listener;
- user-facing errors/help.

## 56.4 `ono-provider-api`

Provider capabilities remain the canonical units used for authorization.

v0.4.1 SHOULD NOT introduce a duplicate remote-only action taxonomy.

## 56.5 `ono-kuang-supervisor`

Expected responsibilities:

- mandatory/best-effort control table;
- fail-closed pre-exec setup;
- confinement report;
- resource-limit outcome classification;
- plugin lifetime ownership.

## 56.6 `ono-value`

Expected addition:

- deterministic approximate retained-size calculation or a closely associated utility crate if architectural layering makes it inappropriate directly in `ono-value`.

## 56.7 `ono-pipeline`

Expected responsibilities remain bounded streaming, cancellation and stream boundedness metadata.

The crate SHOULD expose reusable primitives required to forward evaluator-generated streams without full collection.

## 56.8 `ono-parser` and `ono-cli/eval`

Structural decomposition is required as specified, but crate ownership remains unchanged.

---

# 57. Implementation Sequence

The implementation MUST be staged so safety-critical work lands before broad refactoring. The following order is normative unless an ADR documents a dependency requiring a local swap.

## Phase H0 - Baseline and guardrails

Deliverables:

- freeze a v0.4.1 baseline test/performance snapshot;
- add failing regression tests reproducing the direct-agent unauthenticated-client problem;
- add failing tests for ignored KUANG control failures;
- add failing streaming test for `each` delayed/unbounded input;
- add reproducible high-cardinality spatial fixture reproducing the known first-output pathology;
- record current release artifact hashes and workflow inputs.

No production fix lands before the corresponding failure proof where practical.

## Phase H1 - Direct remote mutual authentication

Deliverables:

- peer identity generalization;
- persistent client identity;
- TLS client certificate presentation;
- server certificate verification of the client;
- explicit fingerprint available at accept;
- no-client-cert refusal;
- legacy direct protocol downgrade prevention.

## Phase H2 - Remote authorization

Deliverables:

- `authorized_clients` store;
- strict parser/atomic writer;
- client-key management commands;
- observe-only default;
- exact action capability grants;
- authorization-filtered negotiation;
- dispatch defense in depth;
- audit events.

## Phase H3 - Remote resource limits

Deliverables:

- connection semaphore;
- pending-handshake semaphore;
- handshake timeout;
- per-fingerprint connection limit;
- refusal errors/tests;
- startup diagnostics.

## Phase H4 - KUANG fail-closed confinement

Deliverables:

- classify controls mandatory/best-effort;
- check every syscall result;
- prevent exec on mandatory failure;
- structured confinement report;
- tests using injected syscall/control failures;
- documentation terminology correction.

## Phase H5 - Resource-budget primitives

Deliverables:

- value-size estimator;
- shared budget type;
- budget-aware materialization helper;
- result-history byte limits and truncation marker;
- configuration defaults and generated docs.

## Phase H6 - Streaming evaluator repair

Deliverables:

- incremental `each`;
- unbounded-source acceptance;
- downstream backpressure;
- break/continue/return/error regression suite;
- inventory and remediation of convenience captures;
- `explain` materialization visibility.

## Phase H7 - Spatial performance stabilization

Deliverables:

- scalable fixture profiles;
- time-to-first-result metrics;
- `map --live` bounded initial projection;
- selector miss optimization;
- expensive-relation request path;
- completion budget;
- cancellation/load tests.

## Phase H8 - Test truthfulness and fuzzing

Deliverables:

- explicit skip markers/categories;
- expected-skip verification;
- canonical helpers;
- scheduled coverage-guided fuzzing;
- targeted Miri/sanitizer workflows.

## Phase H9 - Structural maintainability refactor

Deliverables:

- parser module decomposition;
- evaluator module decomposition;
- session state segmentation;
- no semantic changes except separately tested fixes;
- architecture docs updated.

## Phase H10 - CI and supply-chain hardening

Deliverables:

- Action SHA pinning;
- image digest pinning;
- least-privilege workflow permissions;
- advisory/license/source checks;
- exact tool versions;
- immutable build inputs report.

## Phase H11 - Reproducible and attestable release

Deliverables:

- build-twice verification;
- checksums;
- signatures;
- provenance;
- draft/promote release flow;
- verification documentation.

## Phase H12 - Documentation and release proof

Deliverables:

- generated metrics;
- README/Wiki security wording;
- v0.4.1 acceptance matrix;
- complete package/install tests;
- no unexpected skips;
- final release candidate dogfooding.

---

# 58. Spec-Driven Work Packages

Each phase MUST be decomposed into work packages small enough for an implementation agent to complete and verify independently.

Every work package MUST contain:

```text
ID
intent
normative source sections
owned crates/files
preconditions
tests that must fail first
implementation requirements
negative tests
performance/security consequences
documentation consequences
acceptance proof
done definition
```

## 58.1 Example work package: H1-WP2

```text
ID: H1-WP2
Title: Require authenticated client certificate on TlsListener
Intent:
  A TCP peer must prove possession of a persistent Ono peer key before
  the agent accepts protocol frames.

Normative source:
  2.1, 2.2, 7, 13

Owned code:
  ono-remote TLS listener/transport
  protocol transport peer-key exposure

Failing proof first:
  connect with TLS client configured with no client certificate;
  current behavior reaches protocol handshake;
  desired behavior fails TLS handshake.

Implementation:
  configure rustls server-side client certificate verification against
  Ono's peer-proof model; return authenticated peer certificate as HostKey/
  PeerKey material.

Negative tests:
  no certificate
  malformed certificate
  certificate signature proof failure
  wrong ALPN

Done:
  no protocol byte from an unauthenticated TCP client reaches agent_main;
  TlsTransport::peer_key() is Some for accepted network clients.
```

## 58.2 Example work package: H6-WP1

```text
ID: H6-WP1
Title: Stream each block input incrementally
Intent:
  each is an item transform, not an accidental global collector.

Failing proof first:
  source emits one value, waits on a barrier, then would emit forever;
  `each { $it } | take 1` must complete before barrier release.

Implementation:
  move block execution into incremental stream forwarding;
  preserve ordered serial semantics and Flow behavior.

Done:
  unbounded-source acceptance passes;
  no complete-input Vec capture remains in the each path;
  memory stays within bounded channel + per-item frame overhead.
```

---

# 59. Security Acceptance Scenarios

## 59.1 Unknown direct client

Given an agent with one authorized client key A, when client B connects with a valid but unknown certificate, TLS/key proof may complete but Ono authorization MUST refuse the session before provider negotiation.

No process list, schema list or capability inventory beyond minimal rejection protocol data may be disclosed.

## 59.2 Authorized observer

Client A is authorized with `observe=true` and no actions.

It MUST be able to execute representative read and observe operations.

An `Act` request for `service.restart` MUST be refused even if the provider offers that capability.

## 59.3 Exact action grant

After the operator grants `service.restart` to client A, that action MAY succeed according to provider/elevation rules.

`process.signal` remains refused unless separately granted.

## 59.4 Changed client key

If A's certificate/key changes, the old fingerprint authorization MUST NOT authorize the new key.

The server refuses until the new fingerprint is explicitly added/set.

## 59.5 Corrupt authorization store

A malformed line in `authorized_clients` MUST cause deterministic startup/configuration failure.

The agent MUST NOT treat it as zero restrictions or ignore only the malformed entry.

## 59.6 Private key permissions

A world-readable `link_identity.pem` MUST be refused.

The diagnostic identifies the path and required permissions without printing key material.

## 59.7 KUANG no-new-privileges failure

Using an injectable platform layer/test hook that makes `PR_SET_NO_NEW_PRIVS` fail, a native plugin spawn MUST fail before plugin code executes.

A marker the plugin would create on startup MUST remain absent.

## 59.8 Resource limit failure

When mandatory `setrlimit` installation fails, plugin execution MUST not proceed.

## 59.9 No security prompt

Every trust failure above MUST be non-interactive and deterministic in scripts.

---

# 60. Streaming and Resource Acceptance Scenarios

## 60.1 `each` before source completion

A test source emits value `1`, waits indefinitely and is marked unbounded.

```text
source | each { $it } | take 1
```

MUST return `1` and complete without waiting for source closure.

## 60.2 Backpressure

A fast synthetic source feeding a slow `each`/downstream MUST not cause retained queue length to grow beyond configured bounded channels plus documented in-flight values.

## 60.3 Break cancellation

```text
source-unbounded | each { if condition { break } ... }
```

MUST stop upstream consumption promptly after `break`.

## 60.4 Materialization item limit

A finite source with 100001 small values sent to a default global materializer MUST fail with `resource.materialization_limit`/item-limit semantics before storing unbounded additional data.

## 60.5 Materialization byte limit

A small number of individually large values whose estimated total exceeds 128 MiB MUST hit the byte limit even though the item count remains far below 100000.

## 60.6 History does not alter output

A pipeline producing more than history limits MUST still emit its complete result to the user/downstream. Only retained history is truncated/evicted.

---

# 61. Performance Acceptance Scenarios

## 61.1 Profile M interactive spatial navigation

Canonical `look`, `near` and selector operations MUST satisfy the Profile M p95 targets on the reference environment.

## 61.2 Live map first frame

Profile M `map --live` MUST render an initial visible frame within 500 ms p95.

Profile L MUST produce a frame or truthful progress/cost response within 1.5 seconds p95.

## 61.3 No blank hang

A watchdog acceptance test MUST fail any interactive spatial command that produces neither first result nor progress/refusal within the declared hard interactive budget.

## 61.4 Completion

A completion request that triggers expensive discovery MUST return partial results or stop by the 150 ms hard budget.

## 61.5 Cancellation

Cancelling the heaviest Profile L spatial/live benchmark MUST release the main query task promptly and stop measurable result growth.

---

# 62. CI and Release Acceptance Scenarios

## 62.1 Action pin scanner

The gate MUST fail if a required workflow contains a third-party `uses:` reference that is not a full commit SHA, except repository-local actions.

## 62.2 Mutable image scanner

Release-critical scripts/workflows MUST fail policy if a container reference lacks a digest.

Test-only developer convenience images MAY be exempt only through an explicit allowlist.

## 62.3 Dependency audit

A fixture or controlled test MUST prove the dependency policy command fails the gate on a denied advisory/license/source condition.

## 62.4 Build twice

Two clean release builds from the same commit MUST produce identical hashes for every published package.

## 62.5 Provenance verification

The release-check job MUST verify that each artifact digest is present in checksum and provenance output before publication.

## 62.6 Package identity

The exact artifact installed in package smoke tests MUST hash identically to the later published asset.

---

# 63. Migration Strategy

## 63.1 Existing users

No user action is required for ordinary local shell use, adapters, spatial navigation or SSH-carried remote links.

## 63.2 Existing direct listening-agent users

Direct TCP users MUST perform a one-time client authorization step because v0.4.1 intentionally stops accepting anonymous TLS clients.

Recommended migration:

```text
# On the client
ono --print-peer-key

# On the agent host, after verifying the fingerprint through a trusted channel
ono -c 'add client-key sha256:... --label my-laptop'
```

The exact quoting may follow final command grammar, but the workflow is normative.

## 63.3 Existing host identity

A pre-existing direct-agent host key MUST continue to identify that host after migration according to section 8.2, so clients do not receive an unnecessary host-key-change refusal merely because v0.4.1 generalized identity storage.

## 63.4 Existing KUANG plugins

Trusted native plugins require no manifest migration solely because confinement failures become fatal.

If a plugin only ran because an invalid/unavailable resource limit was silently ignored, it will now fail with a clear diagnostic. The remedy is to fix the policy/platform incompatibility, not to disable error checking.

## 63.5 Existing test infrastructure

Tests formerly relying on silent early returns MUST be converted to explicit skips or made runnable in the canonical environment.

Historical test names MAY be renamed when the old name encodes no longer valid semantics, but test intent and coverage MUST be preserved.

---

# 64. Explicit Non-Goals

v0.4.1 MUST NOT expand into the following projects:

1. **No new product dimension.** Time/causality remains v0.5; prospective change/recovery remains v0.6.
2. **No complete KUANG kernel sandbox requirement.** Landlock/seccomp/WASM may come later; v0.4.1 makes current guarantees truthful and fail-closed.
3. **No public PKI requirement.** Direct links continue to use explicit Ono key trust rather than WebPKI/DNS identity.
4. **No enterprise RBAC system.** Client authorization is local, explicit and capability-oriented, not LDAP/OIDC/organization policy management.
5. **No multi-user daemon architecture.** The listening agent remains a process serving providers under its Unix execution identity.
6. **No parallel `each`.** Streaming correctness is the goal; concurrency semantics are a separate feature.
7. **No wholesale parser/evaluator rewrite.** Structural decomposition only.
8. **No Windows/macOS port.** Linux remains the target platform.
9. **No performance theater.** The release does not chase synthetic throughput at the cost of first-result latency, boundedness or correctness.
10. **No feature-count target.** A release with zero flashy new features is acceptable if all hardening contracts are met.

---

# 65. Failure Modes to Avoid

The implementation MUST explicitly avoid the following anti-patterns.

## 65.1 "TLS means authenticated"

Using encryption while accepting any client certificate/no certificate and then calling the session authenticated is forbidden.

## 65.2 Self-reported authorization

Using `Hello.identity.user`, UID, elevation or source IP to grant capabilities is forbidden.

## 65.3 Negotiation-only authorization

Hiding a capability in `Accept` but still executing a forged request for it is forbidden.

## 65.4 Fail-open plugin setup

Calling a confinement syscall, discarding its result and executing the plugin anyway is forbidden.

## 65.5 "Sandbox" as marketing shorthand

Calling native plugins sandboxed without stating the missing filesystem/network isolation is forbidden.

## 65.6 Byte-unbounded count limits

Allowing `N` values while each may contain arbitrarily large payloads, with no byte budget, is forbidden for retained/materialized collections.

## 65.7 Streaming via background collection

Replacing a foreground `Vec` with an unbounded background queue is not a streaming fix and is forbidden.

## 65.8 Unbounded input to global operation

Waiting forever for an explicitly unbounded stream to finish is forbidden. Refuse early.

## 65.9 Progress-free interactive computation

An interactive command performing long expensive work must produce results, progress or a bounded refusal; a silent blank terminal is not acceptable behavior.

## 65.10 Skip-as-pass

A test returning before its assertion path without an explicit skip outcome is forbidden.

## 65.11 Mutable release inputs

Claiming reproducibility while using mutable action/image/tool references is forbidden.

## 65.12 Refactor plus semantic redesign

Combining parser/evaluator file decomposition with broad language redesign in the same work package is forbidden. It destroys the ability of tests to prove behavior preservation.

---

# 66. Release Definition

A candidate is ONO-SENDAI v0.4.1 only when all criteria below are satisfied.

## 66.1 Security

- Direct TCP server and client cryptographically authenticate each other.
- Unknown clients cannot reach provider negotiation/data.
- Authorized clients receive only policy-allowed operations.
- `Act` requires exact granted capability.
- Direct-link downgrade is not automatic.
- Key files and authorization stores have secure/strict handling.
- Mandatory KUANG native confinement failures prevent plugin execution.
- Documentation accurately describes native trust/isolation boundaries.

## 66.2 Resource correctness

- Materialization has item and byte limits.
- Captures use shared budgets.
- Recent result history has total/per-result byte ceilings and truthful truncation markers.
- Finite-required operations refuse unbounded input before waiting indefinitely.

## 66.3 Streaming

- `each` consumes and emits incrementally.
- `each` works with unbounded sources.
- backpressure and cancellation remain bounded.
- implementation-convenience captures have been removed or explicitly justified/bounded.
- cross-kind stream ordering semantics are documented.

## 66.4 Performance

- Profile S/M/L fixtures exist.
- time-to-first-result is measured.
- `map --live` no longer exhibits the reproduced long blank hang on supported profiles.
- selector/completion targets are met or bounded refusal behavior is implemented.
- cancellation under load is verified.

## 66.5 Verification

- no silent test skips remain in covered patterns;
- expected skips are machine-readable and checked;
- shared test helpers are consolidated;
- normal gate fuzzing passes;
- scheduled coverage-guided fuzzing exists;
- targeted Miri/sanitizer jobs exist and are green for the release commit.

## 66.6 Maintainability

- parser responsibilities are decomposed without grammar regression;
- evaluator responsibilities are decomposed without execution regression;
- session state is segmented enough that history/resource invariants have a clear owner;
- no new cross-crate dependency inversion was introduced.

## 66.7 Supply chain and release

- required GitHub Actions are pinned by commit SHA;
- release-critical container images are pinned by digest;
- Rust/tool dependencies are locked/pinned;
- dependency advisory/policy checks pass;
- release packages rebuild identically in two clean runs;
- checksum manifest exists;
- signatures exist and verify;
- provenance exists and binds all published artifacts;
- the exact tested bytes are the published bytes.

## 66.8 Documentation

- README/Wiki/help use the security terminology contract;
- generated repository metrics are current;
- remote client authorization migration is documented;
- release verification instructions are documented;
- `docs/STATE.md`, acceptance documentation and release notes agree on status.

## 66.9 Zero unresolved P0/P1 findings

There MUST be no known unresolved P0 or P1 issue in the v0.4.1 scope at final release.

A P2/P3 issue MAY remain only if it is explicitly excluded from this specification through an ADR made before release candidate freeze; such an ADR cannot waive a release criterion listed above.

---

# 67. End-to-End Reference Interactions

## 67.1 Secure direct link bootstrap

```text
client$ ono --print-peer-key
sha256:8b6c...e21a

server$ ono -c 'add client-key sha256:8b6c...e21a --label laptop'
authorized laptop for observation; actions: none

server$ ono --agent --listen 0.0.0.0:7734
ono: listening on 0.0.0.0:7734
ono: peer key sha256:0f31...94ac
ono: authorized clients 1
ono: direct transport requires mutual authentication

client$ ono
local://~ > add host-key server tls-x509 sha256:0f31...94ac
local://~ > link host server --transport tls
linked server (tls, mutually authenticated): process file dir user group env mount interface route socket service

local://~ > enter link server
server://~ > get process | take 3
...
```

The important experience is not certificate ceremony. It is that the operator makes two explicit durable trust decisions and normal use becomes simple afterwards.

## 67.2 Unauthorized mutation

```text
server://~ > get service nginx | restart service
error remote.capability_denied:
  client laptop is authenticated but is not authorized for service.restart
help:
  authorize that exact action on the agent host; there is no continue-anyway override
```

After explicit server-side grant:

```text
server$ ono -c 'set client-key sha256:8b6c...e21a --allow service.restart'

# reconnect
server://~ > get service nginx | restart service
...
```

Granting `service.restart` MUST NOT implicitly grant `process.signal`, package mutation or future capabilities.

## 67.3 Plugin confinement failure

```text
local://~ > run plugin example
error plugin.confinement_failed:
  example was not started because no_new_privs could not be installed
  required control: no_new_privs
  execution tier: native-confined
```

No plugin code executes after that failure.

## 67.4 Streaming `each`

```text
local://~ > watch process | each { select pid name } | take 1
PID   NAME
1     systemd
```

The command returns the first value without waiting for `watch process` to end.

## 67.5 Materialization refusal

```text
local://~ > unbounded-source | sort timestamp
error resource.materialization_limit:
  sort requires finite input, but the upstream stream is unbounded
```

For a large finite stream:

```text
error resource.byte_limit:
  sort reached its 128 MiB materialization budget
  consumed: 128.0 MiB
  values: 48217
help:
  narrow the input or raise limits.materialize_bytes deliberately
```

## 67.6 Truthful history

```text
local://~ > huge-query
... complete output ...

local://~ > inspect result -1
note: this history entry is partial; retention stopped at 16 MiB
```

The original command was not truncated.

## 67.7 Release verification

```text
$ sha256sum -c SHA256SUMS
ono_0.4.1_amd64.deb: OK
...

$ cosign verify-blob ...
Verified OK
```

The project documentation provides the exact supported command for the chosen signature mechanism.

---

# 68. Final Product Principle

v0.2 established structure.  
v0.3 made foreign Unix tools participate without pretending they were native.  
v0.4 made the machine navigable as space.  
v0.5 and v0.6 will add time, causality, intent, protection and recovery.

v0.4.1 is the release that makes the substrate worthy of those later ideas.

The principle is:

> **Ono must never obtain trust by wording. Trust is earned by enforced boundaries, explicit uncertainty, bounded work and reproducible evidence.**

Or, operationally:

```text
Authenticate before trusting.
Authorize before acting.
Fail closed before executing.
Bound before retaining.
Stream before collecting.
Measure before claiming performance.
Execute before calling a test passed.
Pin before calling a build reproducible.
Verify before publishing.
```

That is the complete v0.4.1 contract.

---

# Appendix A. Default Hardening Limits

The following defaults are normative for the v0.4.1 reference implementation unless an ADR changes a value with benchmark/security evidence.

| Limit | Default | Semantics |
|---|---:|---|
| Pipeline channel capacity | 64 events | bounded backpressure queue |
| Materialization values | 100,000 | hard per materializer |
| Materialization bytes | 128 MiB | hard per materializer |
| Nested command capture bytes | 256 MiB | hard aggregate command ceiling |
| Result-history entries | 16 | cache slots |
| History values/result | 10,000 | retention only |
| History bytes/result | 16 MiB | retention only |
| History bytes total | 64 MiB | oldest-first eviction |
| Direct agent connections | 32 | concurrent authenticated/active ceiling |
| Pending handshakes | 16 | pre-negotiation ceiling |
| Connections/client | 4 | authenticated fingerprint key |
| Handshake timeout | 10 s | TLS + protocol negotiation |
| Completion soft budget | 50 ms | partial response may begin |
| Completion hard budget | 150 ms | discovery stops |
| Cancellation p95 target | 100 ms | responsive provider |
| Cancellation p99 target | 250 ms | responsive provider |

Limits MUST be expressed internally in integer base units and rendered in human-readable units separately.

---

# Appendix B. Remote Trust State Machine

```text
                  TCP accepted
                       |
                       v
                TLS handshake
                 /          \
              fail          peer cert proved
               |                  |
          disconnect              v
                         fingerprint extracted
                                  |
                        authorization lookup
                          /             \
                      absent          present
                        |                 |
                     reject              v
                                  policy context
                                       |
                                  Ono Hello
                                       |
                                  negotiate
                                       |
                           policy-filtered Accept
                                       |
                               request dispatch
                              /                \
                         permitted            denied
                            |                    |
                         execute          stable refusal
```

At no state before `policy context` may provider data be served.

---

# Appendix C. Authorization Decision Matrix

| Request | Unlisted client | Observe client | Observe + exact action | Result |
|---|---|---|---|---|
| Query read capability | deny | allow | allow | policy enforced before dispatch |
| Subscribe observe capability | deny | allow | allow | policy enforced before dispatch |
| Act ungranted mutate | deny | deny | deny | exact ID required |
| Act granted mutate | deny | deny | allow | provider/elevation rules still apply |
| Act destructive not granted | deny | deny | deny | no risk-class wildcard |
| New future capability | deny | read/observe only if covered by observe semantics; action denied | action denied until explicitly added | fail conservative |

For action authorization, an unknown capability ID is always denied.

---

# Appendix D. KUANG/11 Confinement Matrix

| Control | Native v0.4.1 | Failure | Security meaning |
|---|---|---|---|
| Capability broker | required | refuse brokered operation | Ono-mediated API control |
| Protocol size/credit limits | required | quarantine/terminate | memory/protocol containment |
| FD inheritance hygiene | required | spawn fails | prevents accidental descriptor authority |
| `no_new_privs` | required Linux | spawn fails | blocks privilege-gaining exec transitions |
| CPU/resource rlimits | required when configured | spawn fails | resource confinement |
| Session/process separation | required | spawn fails | lifecycle/job containment |
| Environment sanitization | required | spawn fails | reduces inherited secrets/behavior |
| Filesystem isolation | not provided by native tier | n/a | plugin can access files user can access |
| Network isolation | not provided by native tier | n/a | plugin can use network available to user |
| seccomp syscall allowlist | not required v0.4.1 | n/a | future stronger tier |
| Landlock path allowlist | not required v0.4.1 | n/a | future stronger tier |

The UI/documentation MUST never infer the last four rows from the first rows.

---

# Appendix E. Streaming Classification Matrix

Every pipeline operation SHOULD be classifiable as one of:

| Class | Examples | May require finite input? | May materialize? |
|---|---|---:|---:|
| Item transform | `each`, projection | no | no global materialization |
| Predicate | `where` | no | no |
| Prefix | `take` | no | no |
| Incremental aggregate | count where defined streaming | no | constant/bounded state only |
| Global reorder | `sort` | yes | yes within budget |
| Global grouping | full group | yes | yes within budget |
| Explicit collect | list/collect | yes | yes within budget |
| Live view | `watch`, `map --live` | no | bounded current-state model only |

If a command cannot be placed in this matrix, its execution semantics are underspecified and MUST be resolved before release.

---

# Appendix F. Performance Fixture Matrix

## F.1 Process/topology fixtures

```text
S:
  processes          100
  graph nodes        500
  edges             2,000

M:
  processes        1,000
  graph nodes      5,000
  edges            25,000

L:
  processes       10,000
  graph nodes     50,000
  edges           250,000
```

## F.2 Socket fixtures

```text
S  1,000 sockets
M 10,000 sockets
L 100,000 sockets
```

## F.3 Payload fixtures

Materialization benchmarks MUST include values averaging approximately:

```text
100 B
10 KiB
1 MiB
```

The 1 MiB case may use fewer elements to stay within practical test-run memory while still proving the byte budget triggers before the item budget.

## F.4 Measurements

Each result record SHOULD contain:

```json
{
  "benchmark": "spatial.map_live",
  "profile": "M",
  "commit": "...",
  "time_to_first_ms": 0,
  "time_to_complete_ms": 0,
  "p95_ms": 0,
  "peak_rss_bytes": 0,
  "values": 0,
  "cancel_ms": 0
}
```

Field names MAY differ, but the information content is required.

---

# Appendix G. Test Result Contract

A host-dependent test helper SHOULD expose an API equivalent to:

```text
require(condition, reason_category, detail) -> TestPrerequisite
```

When unmet, the test harness/integration layer records:

```text
SKIP missing_kernel_feature: pidfd unavailable on kernel ...
```

The canonical CI skip verifier compares observed skip IDs/categories to the checked-in expectation.

A raw pattern such as:

```rust
if prerequisite_missing() {
    return;
}
```

inside an integration test is prohibited unless the return path has already emitted the canonical explicit skip signal that the gate recognizes.

---

# Appendix H. Release Input Manifest

The release workflow MUST generate an input manifest containing at least:

```text
source commit
source tag
Rust toolchain
Cargo.lock hash
build container digest(s)
package-test container digest(s)
GitHub Action SHAs
packaging tool versions
SOURCE_DATE_EPOCH
workflow run identity
```

This manifest MAY be embedded in or referenced by provenance.

Its purpose is to answer a future maintainer's question:

> What exactly did we trust to produce these bytes?

---

# Appendix I. Refactoring Guardrails

## I.1 Parser

Before extraction:

- capture parser test count and corpus;
- run fuzz seeds;
- record public AST snapshot tests where available.

After each module extraction:

- same tests must pass;
- no new grammar behavior without a separate failing test/commit.

## I.2 Evaluator

Before extraction:

- capture behavior for status, errors, `break`, `continue`, `return`, `exit`;
- capture pipeline cancellation/backpressure tests;
- capture native process/job tests.

Streaming fixes MAY change timing/memory semantics intentionally, but value/control-flow semantics remain.

## I.3 Session

State segmentation MUST not cause:

- different config precedence;
- changed environment mutation semantics;
- lost job reaping;
- changed navigation trail semantics;
- changed result-history identifiers.

---

# Appendix J. Design Review Checklist

A reviewer of v0.4.1 SHOULD be able to answer **yes** to every item.

## Remote trust

- Does every accepted direct TCP connection have an authenticated client fingerprint?
- Is that fingerprint checked against an explicit authorization store?
- Can an unauthorized peer learn provider data before refusal?
- Are action capabilities exact grants?
- Can a v0.4.1 client/server silently fall back to unauthenticated direct mode?
- Are runtime user/UID fields clearly separated from transport identity?

## KUANG/11

- Does every mandatory confinement syscall propagate failure?
- Can a test prove plugin code did not execute after setup failure?
- Does documentation clearly say native filesystem/network isolation is not provided?

## Bounds and streaming

- Does every materializer have item and byte limits?
- Does `each` produce before upstream completion?
- Does `each` accept an unbounded stream?
- Can downstream slowness create an unbounded queue?
- Do history limits affect only retention, never live output?

## Performance

- Are Profile S/M/L fixtures real and reproducible?
- Is time to first result measured?
- Can `map --live` remain blank for tens of seconds?
- Does completion have a hard time budget?
- Are expensive relationships actually requestable when advertised as such?

## Tests

- Can a skipped test appear as a pass without a skip marker?
- Does CI fail on unexpected skips?
- Are shared helpers canonical?
- Is coverage-guided fuzzing scheduled?

## Release

- Are Actions pinned by SHA?
- Are build images pinned by digest?
- Are dependencies locked and audited?
- Do two clean builds produce identical packages?
- Are checksums, signature and provenance published?
- Are the tested package bytes exactly the published package bytes?

## Maintainability

- Is parser logic navigable by responsibility?
- Is evaluator logic navigable by responsibility?
- Does session state have clear internal owners?
- Were these refactors separated from unrelated feature work?

If any P0/P1 answer is "no", the release is not v0.4.1.
