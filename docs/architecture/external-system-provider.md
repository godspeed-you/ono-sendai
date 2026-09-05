---
title: "ONO-SENDAI External System Provider Specification"
subtitle: "KUANG/11 Contract for Typed, Navigable External Systems"
author: "Architecture Specification"
date: "2026-09-03"
geometry: "paperwidth=157mm,paperheight=210mm,left=13mm,right=13mm,top=14mm,bottom=15mm"
fontsize: 11pt
mainfont: "DejaVu Sans"
monofont: "DejaVu Sans Mono"
colorlinks: true
linkcolor: blue
urlcolor: blue
toc: true
toc-depth: 3
numbersections: false
header-includes: |
  ```{=latex}
  \usepackage{microtype}
  \usepackage{enumitem}
  \setlist{nosep,leftmargin=*}
  \usepackage{fvextra}
  \DefineVerbatimEnvironment{Highlighting}{Verbatim}{breaklines=true,breakanywhere=true,fontsize=\small,commandchars=\\\{\}}
  \setlength{\parskip}{0.45em}
  \setlength{\parindent}{0pt}
  ```
---

# ONO-SENDAI External System Provider Specification

## KUANG/11 Contract for Typed, Navigable External Systems

**Status:** Additive architecture specification; not tied to a numbered release  
**Scope:** Generic contract by which first-party and KUANG/11 providers expose external systems such as Kubernetes, AWS, Azure and GCP as typed Ono resources, relationships, places, observations and bounded actions  
**Relationship:** Builds on the existing Ono-Sendai provider, extension, spatial, temporal, prospective-change, presentation and remote-link contracts; does not replace them  
**Normative language:** MUST, MUST NOT, SHOULD, SHOULD NOT, MAY

> A provider does not add a new command universe. It teaches Ono about another system.

> Provider integrations preserve the system's native truth, then project it through Ono's existing language.

---

# 0. Document Status and Relationship to Earlier Specifications

## 0.1 Standalone additive architecture specification

This document defines the generic external-system provider contract for Ono-Sendai.

It is intentionally not named v0.10, v0.11, v0.12, or any later release. Existing release specifications retain their own scopes and ordering.

This specification may be implemented incrementally across releases and may be consumed by later provider-specific specifications such as:

```text
Kubernetes Provider Specification
AWS Provider Specification
Azure Provider Specification
GCP Provider Specification
Cross-System Relationships Specification
```

Provider-specific documents MUST conform to this contract unless an explicit ADR records a justified deviation.

## 0.2 Inheritance rule

Earlier Ono-Sendai specifications remain authoritative for concepts they already define.

This document MUST NOT create parallel replacements for inherited concepts including, where already present in the repository contract:

- `Value` and typed records;
- `Stream<T>`;
- schema and command registries;
- provider provenance;
- `ValueRef` or canonical value identity references;
- places, exits, trails and graph relationships;
- observations, events, evidence, coverage and gaps;
- prospective changes, plans, risk, protection and recovery;
- presentation descriptors or renderer hints;
- KUANG/11 manifest, sandbox, capability and audit semantics;
- remote-link and context-stack semantics.

Where this specification uses a conceptual name that overlaps an existing canonical repository type, the implementation MUST reuse or evolve the existing type rather than introduce a synonym merely to match this document's prose.

## 0.3 No retrospective rewriting

Earlier immutable narrative specifications MUST NOT be edited to make the external-system implementation easier.

If this specification exposes ambiguity in an earlier contract, an ADR MUST resolve the ambiguity. The ADR SHOULD prefer extension and consolidation over a second competing abstraction.

## 0.4 Product intent

The purpose of the external-system provider contract is not:

- to wrap provider CLIs;
- to create a generic REST client plugin API;
- to expose arbitrary JSON as Ono objects;
- to introduce a cloud-specific shell mode;
- to invent a lowest-common-denominator multi-cloud schema.

The purpose is:

> **Allow an external system to become a first-class part of Ono's typed, navigable, provenance-aware systems model without teaching Ono core the provider's domain.**

## 0.5 Architectural thesis

The contract is built around five statements:

> **Providers contribute systems, not command dialects.**

> **Native provider identity and semantics remain visible.**

> **Relationships are data with provenance, not UI decoration.**

> **Partial knowledge, denied access and stale data remain explicit.**

> **The host owns Ono semantics; the provider owns domain truth.**

All five statements are normative.

---

# 1. Goals

The external-system provider architecture MUST make the following possible.

## 1.1 External resources as typed values

A provider can expose resources such as:

```text
Kubernetes Pod
AWS EC2 Instance
Azure Virtual Machine
GCP Compute Instance
```

as typed Ono values with stable schemas, native identities and provenance.

## 1.2 Native relationships

A provider can assert domain relationships such as:

```text
Pod -> scheduled-on -> Node
Service -> selects -> Pod
Instance -> attached-to -> NetworkInterface
NetworkInterface -> protected-by -> SecurityGroup
```

without flattening them into text or requiring command-specific renderers.

## 1.3 Spatial integration

External resources can participate in the existing spatial systems interface so that users can:

```text
look
near
enter
follow
trace
find place
back
up
home
trail
```

without entering a provider-specific mini-shell.

## 1.4 Live and temporal integration

Providers can expose current observations, watches, events, freshness, coverage and gaps to the inherited live and temporal models.

## 1.5 Bounded safe actions

Providers can expose actions when they can describe targets, required capabilities, prospective effects and verification honestly.

## 1.6 Community implementation

A contributor can implement a useful external provider without modifying parser internals, shell job control, terminal rendering or unrelated core code.

## 1.7 Provider-independent core

Ono core remains free of provider-specific types such as `AwsAccount`, `KubernetesPod` or `AzureSubscription` except in isolated first-party provider crates/packages that implement this generic contract.

---

# 2. Non-Goals

## 2.1 Full API parity

The contract MUST NOT require a provider to expose every API operation of an external system.

## 2.2 One universal cloud schema

The contract MUST NOT require provider-native types to be converted into one common multi-cloud resource type.

## 2.3 Arbitrary remote code execution

A provider MUST NOT receive unrestricted host access merely because it needs to talk to a remote API.

## 2.4 Hidden background inventory service

The contract MUST NOT require Ono to run a permanent daemon that continuously inventories all configured systems.

A provider MAY support background or persistent observation only under explicit host-controlled capability, lifecycle and resource policies.

## 2.5 Monitoring backend

The contract does not turn Ono into a long-term telemetry database.

## 2.6 IaC engine

The contract does not make provider packages into declarative infrastructure state managers.

## 2.7 Provider-specific presentation ontology

A provider MAY supply presentation hints. It MUST NOT own arbitrary terminal rendering or create a second view model.

---

# 3. Core Invariants

The following invariants are non-negotiable.

1. **Host language remains canonical.** Providers contribute targets, schemas, relationships, actions and metadata; they MUST NOT redefine pipeline, parsing or shell grammar semantics.
2. **Native identity survives.** Provider-native resource identity MUST remain inspectable even when Ono adds semantic roles or aliases.
3. **No invented structure.** Providers MUST NOT infer typed fields from human-formatted text when a machine-readable API or documented schema exists.
4. **No invented topology.** A relationship MUST carry origin and evidence; inferred edges MUST NOT be presented as provider-declared edges.
5. **No invented availability.** Permission denial, API failure, scope exclusion and pagination failure MUST NOT be rendered as an empty successful result.
6. **No invented freshness.** Cached or delayed data MUST carry freshness semantics; it MUST NOT masquerade as a live read.
7. **No secret leakage.** Credential material MUST NOT become ordinary Ono values, logs, diagnostics, history records or provider audit payloads.
8. **No silent scope widening.** A provider MUST NOT query additional accounts, clusters, subscriptions, projects, regions or tenants beyond declared/requested scope merely to produce a more complete-looking result.
9. **No hidden mutation.** Discovery, rendering, relationship resolution and inspection MUST be side-effect free unless the provider explicitly declares an unavoidable read-side effect and the host permits it.
10. **Actions are explicit.** A provider mutation MUST enter through an action/mutation contract and MUST NOT be triggered by navigation or rendering.
11. **Partial results remain partial.** If a multi-page or multi-scope query fails after producing some values, the stream MAY retain valid values but MUST also expose incomplete coverage.
12. **Backpressure is host-compatible.** Providers MUST honor cancellation and bounded buffering for streams and watches.
13. **Provider failure is isolated.** A crashing or misbehaving KUANG/11 provider MUST NOT corrupt the shell process or unrelated providers.
14. **Capabilities precede privilege.** Network, credential, filesystem, process and mutation privileges MUST follow the existing KUANG/11 capability model.
15. **Provider code cannot forge host provenance.** The host MUST distinguish provider-issued evidence from host-derived metadata and from user assertions.
16. **Provider-native semantics remain reachable.** Cross-provider roles or normalized fields MUST NOT remove access to native fields.
17. **Unknown is representable.** The provider contract MUST permit missing, unknown, unsupported and inaccessible facts without plausible defaults.
18. **Context remains visible.** Security-relevant provider scope MUST participate in Ono context projection.
19. **Generated docs come from contracts where possible.** Provider capability and schema documentation SHOULD be derived from the same metadata the host dispatches against.
20. **A read-only provider can be complete.** Mutation support is never required merely to call a provider production-ready for its declared scope.

---

# 4. Conceptual Model

## 4.1 ExternalSystemProvider

`ExternalSystemProvider` is the conceptual contract implemented by a first-party provider or a KUANG/11 package that teaches Ono how to observe and optionally operate an external system.

Examples include:

```text
kubernetes
aws
azure
gcp
openstack
proxmox
```

The concrete implementation MAY use a differently named existing provider trait if the repository already defines one. This specification defines behavior, not a mandatory Rust identifier.

## 4.2 Provider Package

A Provider Package is the distributable implementation unit.

A package may contain:

- one provider driver;
- one or more schema catalogs;
- relationship resolvers;
- optional watch/event adapters;
- optional action implementations;
- optional lenses or analysis helpers;
- fixtures and conformance tests;
- documentation metadata.

A package MUST declare which of these capabilities it implements.

## 4.3 Provider Instance

A Provider Instance is one configured logical use of a provider.

Examples:

```text
kubernetes provider + kube context prod-eu
aws provider + credential profile production + account scope 1234
azure provider + tenant T + subscription S
gcp provider + project P
```

A provider package can therefore have multiple simultaneous instances.

Provider instances MUST have stable local identities distinct from the remote resource identities they expose.

## 4.4 Provider Session

A Provider Session is a bounded runtime interaction context created by the host for a provider instance.

It may contain:

- resolved authentication handles;
- selected scope;
- capability grants;
- rate-limit state;
- cache handles;
- cancellation token;
- audit context;
- host API handle.

Provider Session lifetime MUST be host-controlled.

A session MUST NOT be assumed to be permanent.

## 4.5 System Scope

A System Scope describes the provider-native administrative or query boundary relevant to an operation.

Examples:

```text
Kubernetes cluster / namespace
AWS organization / account / region
Azure tenant / subscription / resource group
GCP organization / folder / project / region
```

Scopes are provider-native. Ono MAY assign generic roles to them but MUST NOT assert equivalence where semantics differ.

## 4.6 Resource

A Resource is a typed external-system entity with provider-native identity.

A Resource MUST expose, directly or through inherited value metadata:

- resource type identity;
- provider instance identity;
- provider-native resource identity;
- observed properties;
- observation/freshness metadata;
- provenance;
- optional spatial metadata;
- optional semantic roles.

## 4.7 Resource Identity

A Resource Identity identifies a resource independently of its current rendering.

It MUST be sufficient to distinguish resources that share a human name.

It SHOULD be stable across ordinary refreshes and property changes.

It MUST NOT rely on a display name alone.

## 4.8 Semantic Role

A Semantic Role is an optional cross-provider classification such as:

```text
compute
network
identity
storage
workload
cluster
database
load-balancer
```

Roles are overlays. They do not replace native types.

## 4.9 Provider Fact

A Provider Fact is a value or property assertion sourced from the external system.

Facts MUST be attributable to:

- provider instance;
- source operation or evidence class;
- observation time where meaningful;
- freshness/coverage state where meaningful.

## 4.10 Relationship Edge

A Relationship Edge connects two identities through a typed relation.

It is semantic data, not just navigation metadata.

## 4.11 Coverage

Coverage describes what a provider operation successfully observed and what it did not.

Coverage may express:

- complete for requested scope;
- partial due permission;
- partial due paging failure;
- partial due timeout;
- intentionally sampled;
- cached/stale;
- unsupported scope;
- unknown completeness.

The implementation SHOULD reuse the canonical temporal coverage/gap model where available.

## 4.12 Provider Capability Set

The Provider Capability Set declares which operations a provider instance can currently support under its installed package, configuration, remote API version and granted permissions.

Capability availability may vary per session and scope.

---

# 5. Provider Maturity Levels

Provider maturity is capability-based, not marketing-based.

## 5.1 Level 0 - Identity and health

A Level 0 provider can:

- load safely;
- report its version and compatibility;
- validate configuration;
- establish or reject authentication;
- identify its current remote scope;
- expose provider health diagnostics.

## 5.2 Level 1 - Resource discovery

A Level 1 provider additionally exposes read-only typed resources.

It MUST support:

- native identity;
- schema registration;
- enumeration or lookup;
- provenance;
- coverage/failure semantics.

## 5.3 Level 2 - Relationships and navigation

A Level 2 provider additionally exposes meaningful relationships and spatial integration.

This SHOULD be considered the minimum maturity for a provider that claims to express Ono's systems-interface value.

## 5.4 Level 3 - Live observation

A Level 3 provider additionally supports watches, events or efficient refresh semantics.

## 5.5 Level 4 - Bounded actions

A Level 4 provider exposes explicit safe actions with target resolution, permission metadata, action descriptions and verification hooks.

## 5.6 Level 5 - Temporal and prospective integration

A Level 5 provider participates deeply in system history, change planning, risk, recoverability and post-action verification.

## 5.7 Declared maturity

A provider MUST NOT declare a maturity level higher than the lowest required capability it actually implements for the advertised resource domain.

A package MAY have different maturity levels by resource family.

Example:

```text
kubernetes/core-workloads: level 3
kubernetes/rbac:           level 2
kubernetes/mutation:       level 0 / unsupported
```

This granularity is preferable to a single misleading "supports Kubernetes" flag.

---

# 6. Provider Registration and Discovery

## 6.1 Registration is metadata-first

When a provider loads, the host MUST be able to obtain a bounded description of its declared contracts without first performing arbitrary remote discovery.

At minimum registration includes:

- provider ID;
- provider version;
- compatible host API range;
- resource type catalog or discovery mechanism;
- declared semantic roles;
- relationship types;
- required KUANG/11 capabilities;
- authentication mechanisms;
- optional action catalog;
- optional watch support;
- documentation metadata;
- conformance level.

## 6.2 Package discovery does not imply activation

Installing a provider package MUST NOT automatically:

- access credentials;
- contact remote APIs;
- enumerate resources;
- create background tasks;
- mutate configuration.

Activation requires an explicit configured provider instance or an explicit user operation according to host policy.

## 6.3 Multiple providers may expose overlapping resource domains

For example, both a Kubernetes provider and a cloud-specific EKS integration may expose facts related to a cluster.

The host MUST preserve provenance rather than selecting one provider as globally authoritative without an explicit rule.

Provider-specific specs MAY define authority precedence for particular facts if that precedence is evidence-based and documented.

---

# 7. Configuration and Provider Instances

## 7.1 Configuration must separate package from instance

A user may install one AWS provider package and configure multiple instances:

```text
aws:personal
aws:staging
aws:production
```

The package identity and configured instance identity MUST remain distinct.

## 7.2 Configuration should be declarative and inert

Following Ono's restricted startup philosophy, provider configuration SHOULD describe:

- provider package;
- instance name;
- credential reference;
- default scope;
- optional region/namespace filters;
- cache policy;
- explicit capability grants.

Loading configuration SHOULD NOT execute remote calls merely because the shell started unless the user explicitly enabled bounded eager initialization.

## 7.3 Secret values are references, not configuration payload

Configuration SHOULD store credential references such as:

```text
credential = "aws-profile:production"
credential = "kubeconfig-context:prod"
credential = "oidc-login:corp"
```

rather than raw tokens, private keys or passwords.

The host credential broker SHOULD resolve the reference at session time.

## 7.4 Environment-derived configuration

Providers MAY support provider-standard environment configuration, but resolution order MUST be deterministic and inspectable.

`explain` or a provider diagnostic command SHOULD show which configuration source won without revealing secret material.

---

# 8. Authentication and Credential Brokerage

## 8.1 Host-owned credential boundary

The preferred architecture is for the host to broker credentials rather than giving a provider unrestricted access to credential files and process environment.

Where feasible, the provider receives:

- a scoped credential handle;
- a short-lived token;
- signed-request capability;
- or another minimal credential interface.

## 8.2 Provider-native authentication remains supported

Some ecosystems require provider-native helpers or SDK chains.

The architecture MAY support these through explicit capabilities, for example:

```text
credential.read:~/.aws/config
credential.exec:aws-sso-helper
network.auth:oidc.example.com
```

The exact capability syntax is inherited from KUANG/11 and MUST NOT be duplicated here.

## 8.3 No secret serialization

Credential values MUST NOT appear in:

- `Value` output;
- ordinary `inspect` output;
- command history;
- provider audit logs;
- crash reports;
- test snapshots;
- generated docs;
- relationship evidence.

Diagnostics MAY state credential source identity and expiry metadata when safe.

## 8.4 Expiry and refresh

If credentials expire during a session, the provider MUST distinguish:

```text
authentication expired
authorization denied
remote API unavailable
resource absent
```

The provider MAY request a refresh through the host credential broker.

Interactive authentication MUST NOT occur unexpectedly inside a non-interactive script unless explicitly permitted by invocation policy.

## 8.5 Identity is inspectable

The provider SHOULD expose non-secret authenticated-principal information so the user can answer:

> Who am I to this system right now?

Examples include account principal, tenant identity, project identity or Kubernetes user/subject information where the remote system exposes it.

---

# 9. Scope Model

## 9.1 Scope is part of operational truth

A resource result without security and administrative scope can be dangerously ambiguous.

Every provider resource identity MUST be interpretable within the provider's native scope model.

## 9.2 Scope dimensions are explicit

Provider-specific specs MUST define their scope dimensions.

Examples:

```text
Kubernetes:
  cluster
  namespace?  # absent for cluster-scoped resource

AWS:
  partition
  account
  region?      # absent for global resources

Azure:
  tenant
  subscription
  resource-group?

GCP:
  organization/folder/project as applicable
  location as applicable
```

## 9.3 No silent fan-out

A query against one scope MUST NOT silently fan out to all configured scopes merely because the provider can.

Cross-scope search MAY be explicit and MAY be supported by `find` or another host-level operation.

The output MUST preserve each resource's scope.

## 9.4 Scope transitions are visible

Entering a resource in another account, project, cluster or equivalent security boundary SHOULD visibly update prompt/context projection.

A navigation operation MUST NOT silently change the credential identity used for subsequent mutations unless the user explicitly enters a provider instance whose authentication context is already known.

---

# 10. Resource Type and Schema Contract

## 10.1 Provider-native type identity

Every resource type MUST have a globally unambiguous Ono schema identity.

A recommended conceptual shape is:

```text
<provider>.<service-or-api-group>.<type>
```

Examples:

```text
k8s.core.v1.Pod
k8s.apps.v1.Deployment
aws.ec2.Instance
azure.compute.VirtualMachine
gcp.compute.Instance
```

Provider-specific specs MAY choose exact canonical spelling consistent with existing Ono schema conventions.

## 10.2 Native and normalized fields

Provider resources may expose:

1. **native fields** - direct semantically faithful representation of provider data;
2. **canonical Ono metadata** - identity, provenance, observed time, context;
3. **semantic role fields** - carefully normalized fields used for cross-provider operations.

Normalized fields MUST identify their derivation where they are not direct native facts.

## 10.3 Schema stability

Providers MUST version schemas according to Ono's existing schema-evolution rules.

A provider MUST NOT change the meaning or type of an existing field silently because the upstream API changed.

If an upstream API introduces incompatible behavior, the provider MUST:

- adapt it compatibly;
- add a versioned schema;
- or reject unsupported upstream versions visibly.

## 10.4 Dynamic schemas

Some systems expose runtime-discovered types, especially Kubernetes CRDs.

The provider contract MUST permit dynamic schema registration under host control.

Dynamic schemas MUST:

- be namespaced to the provider instance/type identity;
- preserve upstream version identity;
- not shadow built-in host schemas;
- be bounded by host resource limits;
- be removable when the provider session ends without corrupting values already materialized.

## 10.5 Unknown fields

Providers MUST preserve unknown provider data when doing so is operationally useful and safe, but MUST NOT force every opaque provider payload into the core schema.

A provider-specific raw/native subtree MAY be exposed if:

- its type is explicit;
- it does not contain secrets by default;
- it does not become the only source of important normalized identity;
- it remains inspectable and serializable under host policy.

## 10.6 Human formatting is not schema

Display strings such as:

```text
"3 nodes ready"
"running (healthy)"
"10.2.3.4:443"
```

MUST NOT replace typed fields when the source exposes the underlying values separately.

---

# 11. Resource Identity

## 11.1 Identity must survive renaming when the provider allows it

A resource name is not necessarily an identity.

Provider-specific specs MUST identify the strongest provider-native identifier available.

Examples may include:

- Kubernetes UID plus cluster identity;
- AWS ARN or service-specific immutable ID plus account/partition;
- Azure resource ID;
- GCP full resource name/self link or stable project/resource identity.

## 11.2 Identity composition

A resource identity MUST include enough provider-instance/native-scope context to avoid collision.

Conceptually:

```text
ResourceIdentity {
  provider_instance
  native_type
  native_id
  scope
}
```

The implementation SHOULD reuse existing `ValueRef`/place identity machinery rather than create an unrelated handle.

## 11.3 Tombstones and disappearing resources

External resources are often ephemeral.

When a previously visited resource disappears, Ono SHOULD preserve the inherited tombstone semantics where possible:

```text
known identity
last observed properties
last observation time
now absent / deleted / unreachable / unknown
```

The provider MUST NOT bind the old place automatically to a newly created resource that reuses the same human name.

## 11.4 Identity aliasing

Providers MAY expose aliases such as names, labels or short IDs for discoverability.

Aliases MUST NOT become equality semantics.

---

# 12. Discovery, Enumeration and Lookup

## 12.1 Discovery operations

A provider SHOULD support three conceptual access patterns where the upstream system allows them:

```text
enumerate by type/scope
lookup by stable identity
search/filter using provider-native capabilities
```

The host may map these into existing `get`, `find`, `where` and spatial operations.

## 12.2 Server-side versus client-side filtering

Providers SHOULD push filtering server-side when:

- semantics are equivalent;
- it reduces cost or rate-limit pressure;
- it does not hide unsupported filter behavior.

If a user expression cannot be translated exactly, the provider MUST NOT silently approximate it.

The host MAY fetch a broader set and apply Ono filtering locally.

`explain` SHOULD make pushdown visible when operationally relevant.

## 12.3 Pagination

Pagination is provider responsibility.

A provider MUST:

- follow continuation tokens correctly;
- preserve cancellation;
- avoid unbounded page buffering;
- surface partial coverage if a later page fails;
- prevent duplicate emission where provider pagination semantics permit stable deduplication.

## 12.4 Streaming enumeration

Providers SHOULD emit resources as pages arrive rather than waiting for full enumeration, unless the upstream API or stable ordering requirement prevents it.

The result MUST remain compatible with `Stream<T>` and host backpressure.

## 12.5 Ordering

Provider enumeration order MUST NOT be treated as semantic unless the provider explicitly guarantees it.

If deterministic output is required for tests or presentation, the host or provider MAY sort on explicit fields.

## 12.6 Expensive discovery

A provider MUST declare operations that may fan out across many scopes, regions or APIs.

The host MAY require explicit user confirmation, a query budget, or an opt-in expensive capability for unusually costly discovery.

---

# 13. Provenance and Observation Metadata

## 13.1 Every external fact has a source

A typed external value MUST retain enough provenance to answer:

```text
Which provider produced this?
Which configured instance?
Which remote scope?
When was it observed?
Was it live or cached?
Was the operation complete?
```

## 13.2 Provider claims versus host derivations

The host MUST distinguish at least:

- provider-native fact;
- provider-derived normalized fact;
- host-derived fact;
- relationship resolver inference;
- user-supplied annotation.

A provider MUST NOT mark its own inference as host-verified truth.

## 13.3 Observation time

An observation SHOULD carry both where meaningful:

- local acquisition time;
- remote/provider event or resource timestamp.

These times have different meanings and MUST NOT be conflated.

## 13.4 Clock uncertainty

Provider-specific specs SHOULD define how clock skew or provider timestamps affect temporal reasoning.

Where exact ordering is not defensible, Ono MUST preserve uncertainty rather than invent total order.

---

# 14. Relationship Contract

## 14.1 Relationships are typed values

Relationship output MUST be pipeable and inspectable.

A conceptual edge contains:

```text
source
target
relation_type
direction
provenance
observation/freshness
confidence/evidence class
optional provider metadata
```

## 14.2 Provider-declared relationships

A provider-declared edge is one that can be established directly from authoritative provider semantics.

Examples:

```text
Kubernetes ownerReference
Pod spec.nodeName / bound node identity
AWS ENI attachment instance ID
Azure resource parent ID
```

The provider MUST document the evidence source.

## 14.3 Derived relationships

A provider MAY derive an edge from provider data when the derivation is deterministic and documented.

Example:

```text
Service selector + Pod labels -> selects
```

The edge MUST indicate that it is derived rather than a literal upstream edge.

## 14.4 Inferred relationships

Heuristic or cross-system correlation MUST be marked as inference and SHOULD be implemented through a resolver contract rather than disguised as provider-native topology.

Detailed cross-system confidence semantics belong to the later Cross-System Relationships Specification.

## 14.5 Missing target

A provider may know a relationship target by identity even if it cannot currently read the target resource.

In that case the edge SHOULD survive with an unresolved or inaccessible target descriptor.

Examples:

```text
target known but permission denied
target known but provider not installed
target known but scope not active
target deleted
target unknown
```

These states MUST remain distinguishable.

## 14.6 Relationship direction

Relation types MUST define direction semantically.

The host MAY expose reverse traversal, but reverse traversal MUST NOT imply a distinct provider assertion unless the relation contract says so.

## 14.7 Relationship cardinality

Provider metadata SHOULD describe expected cardinality where stable enough to aid validation and UX, for example:

```text
Pod --scheduled-on--> Node      0..1
Deployment --owns--> ReplicaSet 0..N
```

Cardinality metadata is descriptive, not proof that missing edges are errors.

---

# 15. Spatial Systems Integration

## 15.1 External systems are places in the existing world

A provider MUST integrate through the existing spatial systems interface rather than creating a separate navigation stack.

## 15.2 Hierarchy is not graph

Providers MUST define which containment relation, if any, drives `up`/place hierarchy separately from general graph relationships.

Examples:

```text
Kubernetes:
cluster -> namespace -> resource

AWS:
provider instance -> account -> region -> resource
```

These are navigation projections, not claims that all resources form a strict containment tree.

## 15.3 `near`

A provider SHOULD expose a bounded set of operationally meaningful nearby objects for entered resources.

`near` MUST NOT simply dump every graph neighbor when that would produce hundreds of low-value edges.

Provider-specific specs SHOULD define prioritization rules.

## 15.4 `follow`

`follow` traverses a declared relationship and MUST preserve edge provenance.

If multiple targets match, the result should remain a typed collection or require explicit selection according to inherited navigation semantics.

## 15.5 `find place`

External resources SHOULD participate in place search using:

- human name;
- native ID;
- aliases/tags/labels where indexed;
- semantic role;
- selected well-defined properties.

Search MUST preserve provider instance and scope in results.

## 15.6 Place URI

Provider-specific specs SHOULD define stable human-readable place URI schemes consistent with Ono's spatial model.

Conceptually:

```text
k8s://prod/shop/pod/checkout-7f9d
aws://prod/eu-central-1/ec2/i-0abc123
```

A display URI MUST NOT be the sole internal identity.

---

# 16. Freshness, Caching and Consistency

## 16.1 External data is not automatically current

Provider APIs may be eventually consistent, cached, watch-delayed or rate-limited.

Every provider MUST define freshness behavior.

## 16.2 Cache policy is explicit

A provider MAY use host-managed or provider-local caches for:

- resource schemas;
- discovery metadata;
- resource snapshots;
- relationship indexes;
- auth metadata;
- expensive list results.

The cache MUST have explicit invalidation/expiry semantics.

## 16.3 Cached data remains marked

A cached value MUST retain:

```text
observed_at
cache_age / freshness class
source
coverage
```

where those facts are applicable.

## 16.4 Eventual consistency

A provider-specific spec MUST document known consistency behavior for important operations where upstream semantics are material to user safety.

For example, immediately after mutation:

```text
GET may lag mutation result
list may lag point lookup
relationship indexes may lag resource creation
```

Ono SHOULD use these semantics in verification windows rather than declaring failure too early.

## 16.5 Cache and mutation interaction

After a successful mutation, the provider MUST invalidate or mark potentially affected cached facts as stale according to declared impact.

It MUST NOT show known-stale pre-change state as if freshly confirmed.

---

# 17. Watches, Events and Long-Running Observation

## 17.1 Watch capability is optional

A provider may be useful without watches.

If it implements watch capability, it MUST integrate with inherited stream cancellation, backpressure and live-view semantics.

## 17.2 Watch semantics

A provider MUST document whether a watch yields:

- full resource snapshots;
- deltas;
- provider events;
- state transitions;
- resync markers;
- bookmarks/checkpoints.

The host MUST know enough to avoid interpreting an event delta as a complete resource state.

## 17.3 Reconnect

Providers SHOULD reconnect watches when safe.

They MUST surface gaps when continuity cannot be proven.

A reconnect that may have missed events MUST NOT silently continue as gap-free history.

## 17.4 Resource versions and checkpoints

Where the upstream API provides resource versions or continuation checkpoints, the provider SHOULD preserve them as evidence metadata.

## 17.5 Watch cost

A provider SHOULD expose resource-cost hints for watches that create significant API or network load.

The host MAY limit concurrent watches per provider instance.

---

# 18. Permissions and Authorization Semantics

## 18.1 Denied is data

Authorization failure is not equivalent to resource absence.

A provider MUST expose permission-denied outcomes in a structured way compatible with Ono's truth model.

## 18.2 Capability discovery

Where the upstream system supports permission introspection, a provider MAY discover whether an action is authorized before attempting it.

Permission introspection is advisory unless the provider can prove it is authoritative at execution time.

## 18.3 Read partiality

A list operation that omits resources due to permission boundaries MUST NOT claim complete coverage.

## 18.4 Mutation authorization

An action MAY pass preflight authorization and later fail due to policy changes, race conditions or conditional permissions.

The host MUST treat execution-time provider response as authoritative for that attempt.

## 18.5 Least privilege

Provider documentation SHOULD state minimum permission sets for each declared capability group.

A provider SHOULD not require broad administrative access for read-only operation when upstream APIs allow narrower permissions.

---

# 19. Errors and Partial Failure

## 19.1 Error taxonomy

Providers MUST map remote failures into structured Ono errors without destroying provider-native diagnostics.

At minimum providers SHOULD distinguish:

```text
configuration error
authentication error
authorization error
scope error
not found
conflict
rate limited
timeout
transport error
remote service error
schema/version incompatibility
partial result
provider internal error
cancelled
```

## 19.2 Provider-native details

Structured errors MAY preserve provider-specific codes, request IDs and safe diagnostic fields.

Secrets and sensitive payloads MUST be redacted.

## 19.3 Partial success

A multi-scope operation may yield valid values before one scope fails.

The provider MAY emit the valid values and an explicit gap/partial coverage marker rather than discarding everything.

The user MUST be able to tell that the result is incomplete.

## 19.4 Retryability

Errors SHOULD declare retryability when known.

The provider MUST NOT blindly retry non-idempotent mutation operations unless the upstream API provides a safe idempotency mechanism.

---

# 20. Rate Limits, Retries and Cost

## 20.1 Rate limits are first-class operational constraints

Providers MUST honor upstream rate limits.

## 20.2 Retry policy

Read operations MAY use bounded retries with provider-appropriate backoff.

Retries MUST:

- honor cancellation;
- respect retry-after semantics where provided;
- be bounded;
- avoid synchronized retry storms;
- expose material delay through progress/diagnostic channels where appropriate.

## 20.3 Mutation retries

Mutation retries require stronger safety.

A provider MUST know whether the operation is idempotent or protected by an idempotency token before automatic retry.

Unknown idempotency means no automatic mutation retry.

## 20.4 Query budgets

The host MAY provide a query budget abstraction limiting:

- requests;
- scopes;
- pages;
- elapsed time;
- transferred bytes;
- concurrent requests.

Providers SHOULD cooperate with host budgets rather than implementing unbounded fan-out.

## 20.5 Cost-bearing APIs

Where an external API has direct monetary or material operational cost, the provider-specific spec SHOULD mark such operations and require explicit user intent where reasonable.

---

# 21. Actions and Mutations

## 21.1 Actions are not arbitrary API calls

A provider action is a typed operation with declared semantics.

It MUST specify:

- action identity;
- accepted target type(s);
- parameter schema;
- required provider capabilities;
- required KUANG/11 privileges;
- whether it mutates state;
- known idempotency semantics;
- expected result schema;
- verification hook or explicit lack thereof;
- prospective-change metadata where supported.

## 21.2 No mutation through getters

`get`, discovery, inspection, relationship traversal, rendering and help paths MUST NOT mutate remote state.

## 21.3 Target resolution

Before mutation, the provider MUST resolve the target to stable identity in the current provider instance and scope.

A human name alone is insufficient if ambiguous.

## 21.4 Dry-run versus explain

A provider-native dry-run MAY be valuable, but it is not equivalent to Ono `explain` or prospective modeling.

The provider MUST label whether a prediction comes from:

- provider-native dry-run;
- static provider metadata;
- Ono impact analysis;
- heuristic inference.

## 21.5 Confirmation policy

Confirmation belongs to host safety policy, not provider-specific ad-hoc prompts.

A provider MUST return structured risk/action information so the host can apply consistent confirmation rules.

## 21.6 Result is not verification

A successful mutation response means the provider accepted/performed the API operation according to its semantics.

It does not necessarily mean the intended system outcome occurred.

Providers SHOULD expose verification strategies where possible.

---

# 22. Prospective Change, Risk and Recovery Integration

## 22.1 Reuse v0.6 semantics

External providers MUST reuse the canonical prospective-change, protection and recovery model rather than invent a cloud-specific plan format.

## 22.2 Effect description

For supported actions, a provider SHOULD be able to describe:

```text
direct target changes
known dependent resources
security-boundary changes
expected asynchronous reconciliation
verification conditions
reversibility
known irreversible side effects
```

## 22.3 Inverse operation is not full recovery

A provider MUST NOT report `recoverable=true` merely because an inverse API call exists.

Recovery semantics SHOULD distinguish:

- configuration reversibility;
- data reversibility;
- control-plane reversibility;
- traffic/external side effects;
- provider-retained previous versions;
- snapshot/backup protection;
- unknown effects.

## 22.4 Safety uncertainty

If the provider cannot determine impact reliably, the plan MUST say so.

Unknown risk MUST NOT be converted to low risk.

---

# 23. Temporal Integration

## 23.1 Provider events become observations with provenance

Provider-specific audit or event feeds MAY enter the inherited temporal model.

Examples:

```text
Kubernetes Event
cloud activity log
audit API record
resource state transition
controller condition change
```

## 23.2 Event identity

Providers SHOULD preserve upstream event IDs, sequence numbers or resource versions where available.

## 23.3 Coverage windows

A provider MUST state the observed time window and known gaps for temporal queries.

## 23.4 Causal discipline

Providers MAY expose provider-native causality when the external system actually asserts it, for example controller owner references or explicit audit request linkage.

They MUST NOT infer causality solely from timestamp proximity.

---

# 24. Cross-System Relationship Hooks

## 24.1 Purpose

This specification defines only the generic hook required for later cross-system work.

It does not define the final confidence taxonomy or resolver precedence.

## 24.2 Exportable identity evidence

A provider SHOULD be able to expose safe identity evidence useful for cross-system correlation.

Examples:

```text
provider IDs
instance IDs
node providerID
resource ARNs/IDs
network interface IDs
IP addresses with observation times
machine identity handles
cluster identifiers
workload identity bindings
```

## 24.3 Resolver separation

Cross-system heuristics SHOULD live in dedicated resolver components that consume provider facts rather than being hidden inside one provider.

This prevents an AWS provider from becoming the implicit owner of Kubernetes semantics, or vice versa.

## 24.4 Provider sovereignty

A resolver MUST NOT alter the provider-native identity of either endpoint.

It adds an evidenced edge between existing identities.

---

# 25. Semantic Roles and Cross-Provider Querying

## 25.1 Roles are opt-in mappings

A provider MAY register native resource types under semantic roles.

Example:

```text
aws.ec2.Instance                -> compute
azure.compute.VirtualMachine    -> compute
gcp.compute.Instance            -> compute
k8s.apps.Deployment             -> workload
```

## 25.2 Role schemas are intentionally small

A semantic role SHOULD expose only fields with defensible shared meaning.

For `compute`, examples might include:

```text
name
provider
scope
state
location
publicly_addressable?
```

Provider-specific detail remains on the native value.

## 25.3 Null does not mean false

If a normalized property cannot be known, it MUST be `null`/unknown according to Ono's type model rather than a plausible boolean default.

## 25.4 Role mapping provenance

Derived role properties SHOULD state which native fields or rules produced them when useful for inspection.

---

# 26. Provider Capability Negotiation

## 26.1 Static and dynamic capabilities

Capabilities exist at two layers:

1. package-declared support;
2. runtime-available support under current configuration, API version, scope and permission.

The host MUST distinguish them.

## 26.2 Capability examples

A provider may declare capabilities such as:

```text
resource.list
resource.get
relationship.list
watch.resource
event.query
action.mutate
action.dry-run
history.query
recovery.describe
```

Exact identifiers SHOULD align with existing registry conventions.

## 26.3 Unsupported is explicit

If a provider package does not implement mutation, a user asking for a mutation MUST receive a structured unsupported-capability result, not a vague command-not-found path that hides the provider boundary.

## 26.4 Permission-dependent capability

A provider MAY support mutation generally while the current principal cannot perform it.

The host should present:

```text
provider supports action
current session not authorized
```

rather than `unsupported`.

---

# 27. KUANG/11 Security and Isolation

## 27.1 Existing sandbox model remains authoritative

External providers operate under the established KUANG/11 manifest, capability, isolation and audit model.

This specification adds provider-specific requirements but does not weaken sandboxing.

## 27.2 Network capability

Remote API access requires explicit network capability.

A provider SHOULD declare the expected endpoint classes/domains when the existing capability model supports sufficiently precise declarations.

Dynamic provider endpoints MAY require a broader but still bounded grant.

## 27.3 Filesystem capability

Providers MUST NOT receive unrestricted filesystem access merely to read conventional provider configuration.

Credential/config access SHOULD be brokered or path-scoped.

## 27.4 Process execution

Executing helper programs such as authentication helpers requires explicit process capability.

The provider MUST NOT execute arbitrary binaries from remote data.

## 27.5 Host API minimization

The host/provider ABI SHOULD expose only functions required for:

- typed value construction;
- schema registration;
- stream emission;
- relationship emission;
- credential brokerage;
- controlled network/API access where architected;
- cancellation;
- audit/diagnostics;
- action registration;
- host cache interfaces where provided.

## 27.6 Audit

Security-sensitive provider operations SHOULD emit audit records including:

- provider package and version;
- provider instance;
- action category;
- scope;
- capability used;
- target identity where safe;
- result category;
- timestamp.

Secrets MUST be excluded.

---

# 28. Provider Lifecycle

## 28.1 Lifecycle states

A provider instance SHOULD have an explicit host-visible lifecycle equivalent to:

```text
configured
loading
ready
degraded
auth-required
incompatible
failed
stopping
stopped
```

Exact enum spelling may reuse existing host lifecycle types.

## 28.2 Load failure isolation

One provider failing to load MUST NOT prevent Ono from starting unless the user explicitly configured that provider as required for the invocation.

## 28.3 Lazy connection

Providers SHOULD avoid remote connection until an operation requires it, unless eager health checking was explicitly configured.

## 28.4 Shutdown

On shutdown or unload, providers MUST:

- cancel watches;
- stop background tasks;
- release session credentials;
- flush bounded audit state if required;
- avoid leaving child processes behind;
- return within host-defined shutdown timeout.

## 28.5 Hot reload

Hot reload MAY be supported by KUANG/11, but the provider contract MUST NOT require it.

If supported, active values and place identities MUST remain safely interpretable after reload or become explicit tombstones/incompatible references.

---

# 29. Versioning and Compatibility

## 29.1 Three compatibility axes

External providers face at least three versions:

```text
Ono host/provider ABI
provider package version
remote API/platform version
```

All three MUST be diagnosable.

## 29.2 Host ABI range

A provider package MUST declare a compatible host ABI/API range.

The host MUST reject incompatible packages before executing provider code where possible.

## 29.3 Remote API compatibility

A provider SHOULD declare supported upstream version ranges or capability-detect dynamically.

Unsupported remote versions MUST fail visibly rather than returning partially misparsed resources as valid.

## 29.4 Graceful feature negotiation

If an upstream API lacks one optional capability, the provider SHOULD degrade that capability rather than rejecting the whole provider when safe.

Example:

```text
resource reads supported
watch unsupported on this endpoint
```

## 29.5 Schema compatibility tests

Provider CI SHOULD include fixtures from the oldest and newest supported upstream API versions for critical resource types.

---

# 30. Performance and Resource Management

## 30.1 Shell responsiveness is a requirement

A slow provider MUST NOT freeze line editing, job control or unrelated local shell operations.

External provider work MUST run through asynchronous/bounded execution appropriate to Ono's runtime architecture.

## 30.2 Bounded concurrency

Providers MUST use bounded concurrency for fan-out operations.

Provider-specific defaults MAY vary, but unbounded task creation is prohibited.

## 30.3 Cancellation

Every potentially long provider operation MUST observe host cancellation.

Ctrl-C or pipeline cancellation SHOULD terminate pending remote requests where the underlying SDK permits it.

## 30.4 Memory bounds

Enumeration and watch implementations MUST avoid retaining entire remote inventories when streaming semantics suffice.

## 30.5 Relationship indexes

Providers MAY maintain indexes for relationship traversal, but index size and invalidation MUST be bounded and observable.

## 30.6 Expensive field loading

Providers MAY implement lazy or staged property loading for resource details that require additional API calls.

A resource MUST indicate when a field is:

```text
not loaded
unknown
unsupported
denied
```

These states MUST NOT collapse into the same value.

---

# 31. Presentation and Discoverability

## 31.1 Values precede views

Providers return typed values and presentation hints. They MUST NOT print their own tables to stdout for native Ono operations.

## 31.2 Default views

Provider schemas MAY define compact default fields useful for ordinary TTY display.

Example EC2-like default:

```text
NAME  ID  STATE  REGION  PRIVATE_IP
```

The full native resource remains available through `inspect`/structured access.

## 31.3 Identity and scope visibility

Default views SHOULD include enough context to avoid ambiguous results when a stream crosses scopes.

## 31.4 Help generation

The host SHOULD generate provider help from registered metadata:

- resource types;
- semantic roles;
- relationships;
- supported actions;
- required permissions;
- current capabilities;
- provider version;
- known upstream compatibility.

## 31.5 `explain`

`explain` SHOULD be able to show provider routing decisions, including:

```text
provider selected
scope selected
server-side filter pushdown
cache use
required capability
action plan / mutation boundary
```

without leaking credentials.

---

# 32. Provider Manifest Requirements

The concrete KUANG/11 manifest schema remains governed by the existing extension specification. An external-system provider package MUST be able to declare equivalent information to the following conceptual example:

```yaml
package:
  id: org.onosendai.provider.aws
  version: 0.1.0
  kind: provider

host:
  api: ">=1.0,<2.0"

provider:
  id: aws
  display_name: Amazon Web Services
  maturity:
    identity: 0
    resources: 1
    relationships: 2
    watch: 0
    actions: 0

capabilities:
  - network.remote
  - credential.broker

resource_types:
  - aws.ec2.Instance
  - aws.ec2.NetworkInterface
  - aws.ec2.SecurityGroup

relationships:
  - attached-to
  - protected-by
  - member-of

semantic_roles:
  aws.ec2.Instance:
    - compute
```

This example is illustrative. The implementation MUST use the canonical manifest shape from KUANG/11 rather than introducing a second file format solely for providers.

---

# 33. Provider SDK Requirements

## 33.1 SDK goal

The provider SDK should make the safe path the easy path.

A provider author SHOULD receive high-level host APIs for common operations rather than constructing internal Ono values manually.

## 33.2 Minimum SDK affordances

The SDK SHOULD provide stable helpers for:

- resource type registration;
- typed record creation;
- native identity attachment;
- provenance attachment;
- coverage markers;
- relationship creation;
- error mapping;
- stream/page emission;
- cancellation;
- capability checks;
- action registration;
- test-host fixtures.

## 33.3 Preventable mistakes should be impossible or loud

The SDK SHOULD make it difficult to:

- emit a resource without provider instance identity;
- emit a relationship without provenance;
- mark cached data as live accidentally;
- expose secret-tagged values through ordinary serialization;
- execute a mutation from a read callback;
- ignore cancellation accidentally;
- return partial pages as complete coverage.

## 33.4 Language boundary

KUANG/11 may support one or more implementation languages/runtimes. This specification does not mandate Rust, WASM or another execution technology.

The semantic contract must remain stable across runtime implementations.

---

# 34. Deterministic Test Host

## 34.1 Every provider must be testable without production credentials

The existing KUANG/11 deterministic test-host idea is mandatory for external-system providers.

A provider package MUST have a test path that does not require a real production account.

## 34.2 Fixture transport

The test host SHOULD be able to emulate:

- HTTP/API responses;
- pagination;
- authentication expiry;
- permission denial;
- rate limiting;
- timeouts;
- watch streams;
- connection reset;
- eventual consistency;
- remote version changes.

## 34.3 Deterministic time

Tests involving freshness, retries, token expiry or event ordering SHOULD use a host-controlled clock.

## 34.4 No live-network default

Provider unit and conformance tests MUST NOT contact live provider APIs by default.

Live integration tests MAY exist behind explicit credentials and CI gates.

---

# 35. Conformance Suite

## 35.1 Purpose

Provider conformance tests are not optional polish. They are the trust boundary that makes a third-party provider credible.

## 35.2 Level 0 conformance

A Level 0 provider MUST prove:

- manifest validation;
- ABI compatibility rejection;
- clean load/unload;
- configuration errors are structured;
- credentials do not leak;
- cancellation/shutdown works;
- denied network/process capabilities fail safely.

## 35.3 Level 1 conformance

A Level 1 provider additionally MUST prove:

- stable resource identity;
- schema validity;
- native fields preserve types;
- pagination completeness;
- partial failure exposes incomplete coverage;
- not-found differs from denied;
- cached data is marked;
- no mutation occurs on read paths.

## 35.4 Level 2 conformance

A Level 2 provider additionally MUST prove:

- relationship source/target identities are valid;
- provenance exists on every edge;
- derived/inferred edges are labeled correctly;
- spatial entry/traversal does not alter resource identity;
- missing/inaccessible targets remain distinguishable.

## 35.5 Level 3 conformance

A Level 3 provider additionally MUST prove:

- watch cancellation;
- reconnect semantics;
- gap emission after unprovable continuity;
- bounded buffering;
- resource-version/checkpoint handling where supported.

## 35.6 Level 4 conformance

A Level 4 provider additionally MUST prove:

- read paths cannot invoke mutations;
- action target identity is stable;
- permission failures are structured;
- idempotency/retry policy is correct;
- action metadata exists;
- confirmation remains host-owned;
- verification result differs from transport success.

## 35.7 Level 5 conformance

A Level 5 provider additionally MUST prove:

- temporal evidence preserves source and coverage;
- prospective effects preserve uncertainty;
- recovery claims are scoped;
- inverse API calls are not mislabeled as total rollback;
- causal output does not overclaim evidence.

---

# 36. Provider-Specific Specification Contract

Every concrete provider specification MUST contain at least the following sections.

## 36.1 Provider thesis

What operational problem does the provider make easier in Ono?

## 36.2 Upstream systems and supported versions

Which APIs/platform versions are supported?

## 36.3 Authentication

Which auth mechanisms are supported and how are secrets brokered?

## 36.4 Scope model

What are the administrative/security/query scopes?

## 36.5 Resource inventory

Which resource types are supported, grouped into explicit implementation tiers?

## 36.6 Identity rules

What makes each resource type stable and unique?

## 36.7 Relationships

Which relationships exist, which are provider-declared, derived or inferred, and what is their evidence?

## 36.8 Spatial mapping

How are resources organized for `up`, `near`, `enter`, `follow` and place URIs?

## 36.9 Read operations

How are lookup, enumeration, pagination, filtering and rate limits handled?

## 36.10 Watch/event model

Which event sources and continuity semantics exist?

## 36.11 Freshness and consistency

What data can be cached, delayed or eventually consistent?

## 36.12 Actions

Which bounded mutations, if any, are exposed?

## 36.13 Risk and recovery

How do provider actions integrate with prospective-change semantics?

## 36.14 Permission model

What minimum permissions are needed by capability group?

## 36.15 Failure semantics

How are upstream errors mapped?

## 36.16 Test plan

Which deterministic fixtures and live integration tests prove the contract?

## 36.17 Non-goals

Which upstream APIs are intentionally not covered?

A provider-specific specification that omits these questions is incomplete.

---

# 37. Reference Provider: Kubernetes Expectations

This section does not replace the later Kubernetes Provider Specification. It defines why Kubernetes is the reference stress test.

## 37.1 Dynamic resource discovery

The generic contract MUST be capable of representing built-in resources and dynamically discovered CRDs without adding Kubernetes concepts to Ono core.

## 37.2 Namespaced and cluster-scoped resources

The scope model MUST represent both cleanly.

## 37.3 Owner references and selectors

The relationship model MUST support direct and derived relationships distinctly.

## 37.4 Watch continuity

The watch model MUST support resource versions, reconnection and explicit gaps.

## 37.5 Desired and observed state

Schemas MUST preserve both where Kubernetes exposes them rather than flattening them into one synthetic `status` string.

If Kubernetes cannot fit this external-provider contract cleanly, the generic contract should be revised before additional providers are built.

---

# 38. Reference Provider: AWS Expectations

This section does not replace the later AWS Provider Specification.

AWS is the second architecture stress test because it introduces different pressures:

- huge API breadth;
- regional and global resources;
- account/organization scope;
- service-specific identifiers;
- IAM and conditional permissions;
- eventual consistency;
- pagination diversity;
- API rate limits;
- many meaningful infrastructure relationships.

The generic contract MUST support a useful AWS provider without hard-coding AWS-specific concepts into Ono core.

If the AWS implementation requires repeated generic-core exceptions, the abstraction should be reconsidered rather than normalized through ad-hoc special cases.

---

# 39. Implementation Architecture Guidance

This section is normative in behavior but intentionally non-prescriptive about exact Rust module names.

## 39.1 Host/provider separation

A recommended layering is:

```text
Ono parser / pipeline / context / spatial / temporal core
                         |
               Provider Host API
                         |
        +----------------+----------------+
        |                |                |
   first-party      KUANG/11         test-host
    provider         runtime           adapter
        |                |
        +-------- External provider package
```

## 39.2 Domain logic stays outside core

Provider-specific API clients, resource mappings and relationship rules MUST live outside the generic core contract.

## 39.3 Host owns cross-cutting policy

The host owns:

- parsing and grammar;
- generic verbs;
- pipeline types;
- cancellation;
- confirmation policy;
- terminal rendering;
- history;
- context stack;
- core safety gates;
- KUANG/11 capability enforcement.

Providers own:

- upstream API semantics;
- resource schema mappings;
- provider-native identity;
- provider-local relationships;
- upstream error mapping;
- provider-specific consistency behavior;
- action execution.

## 39.4 Shared SDK logic is allowed

Providers MAY share libraries for common cloud SDK concerns, credential brokerage, HTTP instrumentation or test fixtures.

Shared libraries MUST NOT become a hidden universal cloud ontology.

---

# 40. Acceptance Gates for the Generic Architecture

The generic provider architecture is not considered proven merely because one provider loads.

## 40.1 Gate A - No-core-domain test

A minimal external provider for a synthetic system can register typed resources and relationships without adding domain-specific code to Ono core.

## 40.2 Gate B - Kubernetes test

A Kubernetes provider can expose at least:

```text
Namespace
Deployment
ReplicaSet
Pod
Service
EndpointSlice
Node
```

with stable identity, relationships, spatial navigation and watch semantics using only generic provider contracts.

## 40.3 Gate C - AWS test

An AWS provider can expose at least:

```text
Account/region scope
EC2 Instance
NetworkInterface
SecurityGroup
Subnet
LoadBalancer/Target relationship subset
```

with pagination, permissions, rate-limit handling and relationship traversal using the same generic contract.

## 40.4 Gate D - Multi-instance test

Two instances of the same provider can coexist without identity collision or credential leakage.

Example:

```text
aws:staging
aws:production
```

## 40.5 Gate E - Partial-failure truth test

A multi-page query that fails on page N returns valid earlier values plus explicit incomplete coverage; it does not return a clean complete stream.

## 40.6 Gate F - Permission truth test

The same lookup fixture can distinguish:

```text
absent
denied
scope not queried
provider unavailable
```

## 40.7 Gate G - Freshness test

A cached resource cannot be mistaken for a live read in `inspect`, provenance or temporal output.

## 40.8 Gate H - Relationship evidence test

Every edge can be inspected to reveal its origin and evidence class.

## 40.9 Gate I - Mutation isolation test

Read, render, search and navigation paths are mechanically unable to invoke provider mutations.

## 40.10 Gate J - Sandbox test

A provider denied network, credential or process capability cannot escape through another provider-host API.

## 40.11 Gate K - Deterministic test-host test

All required conformance gates can run without live cloud credentials.

## 40.12 Gate L - Cancellation test

Cancelling a large enumeration and a watch terminates remote work within a bounded host-defined period and leaves the provider usable afterward.

---

# 41. Anti-Patterns

The following designs violate the intent of this specification.

## 41.1 CLI subprocess wrapper as provider

```text
provider calls `aws ... --output json`
parses stdout
calls that native system integration
```

A provider MAY use an external CLI only where no suitable API exists and the limitation is explicit. It SHOULD prefer stable machine APIs/SDKs.

The generic provider abstraction is not an excuse to reconstruct structure from CLI text.

## 41.2 Generic JSON resource

```text
ExternalResource { json: Value }
```

as the only schema for all provider resources defeats typed pipelines and discoverability.

Opaque provider-native data may exist as a supplementary field, not as the entire model.

## 41.3 Hidden provider shell

Commands such as:

```text
aws> ec2 describe ...
k8s> get ...
```

that introduce a second grammar and mode violate the one-language goal unless they are explicit foreign CLI execution.

## 41.4 False multi-cloud normalization

Mapping unlike concepts to one type and discarding native semantics is prohibited.

## 41.5 Silent best-effort completeness

Returning whatever scopes happened to succeed while displaying a normal complete table is prohibited.

## 41.6 Relationship by matching names

Two resources with the same human name are not related evidence.

A heuristic name match must remain an inference at best.

## 41.7 Mutation in `enter`

Entering a resource must never perform provider state changes.

## 41.8 Provider-specific confirmation prompt

A provider printing `Are you sure? y/N` directly bypasses Ono safety policy and non-interactive behavior. It is prohibited for native provider actions.

---

# 42. Open Questions Reserved for Later Specifications

The following are intentionally not finalized here.

## 42.1 Exact cross-system confidence taxonomy

The later Cross-System Relationships Specification will define canonical confidence/evidence classes and contradiction handling.

## 42.2 Exact provider URI grammar

Provider-specific specs will define canonical place URI components consistent with the spatial interface.

## 42.3 Exact semantic role registry

Initial roles should emerge from Kubernetes + AWS implementation experience rather than speculative universal modeling.

## 42.4 Provider distribution and signing policy

KUANG/11 package distribution, trust roots, signing and registry policy remain governed by the extension ecosystem and may need a dedicated supply-chain specification.

## 42.5 Long-lived background provider services

The need for durable watchers/inventory processes should be proven through dogfooding before adding a general daemon framework.

## 42.6 Federated query planner

A future optimization may push one query across several provider instances. This specification deliberately does not introduce a distributed query planner.

---

# 43. Implementation Sequence

A disciplined implementation order is recommended.

## Phase 1 - Generic contract and synthetic provider

Build:

- provider registration;
- provider instance identity;
- resource schema registration;
- read-only enumeration;
- provenance;
- coverage;
- deterministic test host.

Prove it with a tiny synthetic provider.

## Phase 2 - Kubernetes read model

Implement Kubernetes Level 1 and Level 2 behavior:

- resource identity;
- core schemas;
- owner relationships;
- selectors;
- spatial integration;
- permission and partial-result behavior.

Do not add mutation yet.

## Phase 3 - Kubernetes watch model

Add Level 3:

- resource versions;
- watch streams;
- reconnect;
- gaps;
- live views.

## Phase 4 - AWS read model

Implement a narrow connected AWS graph:

```text
Instance
NetworkInterface
SecurityGroup
Subnet
VPC
LoadBalancer/Target subset
```

Use it to stress pagination, region scope, rate limits and permissions.

## Phase 5 - Refine generic contract

Only after Kubernetes and AWS both exist should the project freeze the first stable provider ABI.

Any abstraction used only by one provider should be challenged before entering the stable ABI.

## Phase 6 - Cross-system resolver foundation

Expose identity evidence hooks and implement the first verified Kubernetes Node -> cloud instance relationship.

## Phase 7 - Safe bounded actions

Add provider mutations only after read, identity, relationship and test contracts are mature.

---

# 44. Provider Author Checklist

Before a provider is merged or published, the author should be able to answer yes to the relevant items below.

## Identity

- [ ] Every resource has a provider-native stable identity.
- [ ] Human names are aliases, not equality.
- [ ] Scope is part of identity where required.
- [ ] Deleted resources cannot silently rebind to new resources with the same name.

## Types

- [ ] Important fields are typed.
- [ ] Human formatting is not used as schema.
- [ ] Unknown/unsupported/denied fields remain distinguishable.
- [ ] Provider-native fields remain reachable.

## Provenance

- [ ] Every resource identifies its provider instance.
- [ ] Observation time is preserved where meaningful.
- [ ] Cache/freshness is visible.
- [ ] Partial coverage is visible.

## Relationships

- [ ] Every edge has source, target, type and provenance.
- [ ] Derived edges are distinguishable from direct provider assertions.
- [ ] Inference is never displayed as proof.
- [ ] Inaccessible targets do not disappear silently.

## Security

- [ ] Credentials are brokered or minimally scoped.
- [ ] Secrets never enter normal values/logs/history.
- [ ] Network/process/filesystem capabilities are explicit.
- [ ] Read paths cannot mutate.

## Runtime

- [ ] Pagination is correct.
- [ ] Rate limits are handled.
- [ ] Cancellation works.
- [ ] Concurrency and memory are bounded.
- [ ] Shutdown leaves no background work.

## Tests

- [ ] Provider works against deterministic fixtures.
- [ ] Auth expiry is tested.
- [ ] Permission denial is tested.
- [ ] Partial paging failure is tested.
- [ ] Rate limiting is tested.
- [ ] Stale cache behavior is tested.
- [ ] Relationship provenance is tested.

---

# 45. Final Architecture Rule

The external-system provider layer exists to let Ono expand into new systems **without expanding Ono into a pile of provider-specific shells**.

The boundary is healthy when a new domain can say:

```text
Here are the things that exist.
Here is how they are identified.
Here are the facts I can observe.
Here is how the things relate.
Here is what I am allowed to do.
Here is what I cannot know.
```

and Ono can immediately give those facts its existing language:

```text
get
where
inspect
enter
follow
trace
watch
past
explain
plan
```

The provider should teach Ono the **world**.

Ono should continue to provide the **language**.

That division of responsibility is the central contract of this specification.

---

# Appendix A. Conceptual Host/Provider Exchange

The following pseudo-interface is illustrative only. It is not a mandatory Rust ABI.

```text
provider.register() -> ProviderDescriptor

host.open_instance(instance_config) -> ProviderSession

provider.list_resources(
    session,
    resource_type,
    scope,
    query,
    page_budget,
    cancellation
) -> Stream<ResourceObservation>

provider.get_resource(
    session,
    identity,
    freshness_policy,
    cancellation
) -> ResourceObservation | StructuredError

provider.relationships(
    session,
    identity,
    relation_filter,
    cancellation
) -> Stream<RelationshipObservation>

provider.watch(
    session,
    watch_target,
    checkpoint?,
    cancellation
) -> Stream<ObservationOrGap>

provider.describe_action(
    session,
    action,
    target,
    parameters
) -> ActionDescriptor

provider.execute_action(
    session,
    prepared_action,
    idempotency_context,
    cancellation
) -> ActionResult

provider.verify_action(
    session,
    action_result,
    verification_policy,
    cancellation
) -> VerificationResult
```

The important properties are:

- typed input/output;
- explicit session and scope;
- cancellation everywhere;
- observation/freshness metadata;
- separate action description and execution;
- no implicit rendering;
- no implicit credentials.

# Appendix B. Conceptual Resource Observation

```text
ResourceObservation {
  identity
  schema
  value
  semantic_roles[]
  provider_instance
  scope
  observed_at
  remote_timestamp?
  freshness
  coverage
  provenance
}
```

This conceptual record SHOULD compile into or reuse existing Ono value/provenance/observation types rather than exist as a parallel ontology if equivalent canonical types already exist.

# Appendix C. Conceptual Relationship Observation

```text
RelationshipObservation {
  source_identity
  relation_type
  target_identity | unresolved_target
  direction
  evidence_kind
  provenance
  observed_at
  freshness
  metadata?
}
```

Again, implementation MUST reuse inherited graph/observation types where available.

# Appendix D. Example: Narrow AWS Provider Surface

A useful first AWS provider does not need hundreds of services.

A coherent first graph could be:

```text
Account
  |
  +-- Region
       |
       +-- VPC
            |
            +-- Subnet
            |    |
            |    +-- Instance
            |         |
            |         +-- NetworkInterface
            |              |
            |              +-- SecurityGroup
            |
            +-- LoadBalancer
                 |
                 +-- TargetGroup
                      |
                      +-- Instance
```

This surface already enables valuable questions:

```text
Which security groups protect this instance?
Which load balancers target it?
Which subnet/VPC contains it?
Which instances are reachable through this target group?
Which resources changed recently?
```

A connected surface is more aligned with Ono than broad disconnected service enumeration.

# Appendix E. Example: Kubernetes Relationship Evidence

```text
Deployment checkout
  --owns--> ReplicaSet checkout-7f9d
    evidence: metadata.ownerReferences

ReplicaSet checkout-7f9d
  --owns--> Pod checkout-7f9d-abc12
    evidence: metadata.ownerReferences

Service checkout
  --selects--> Pod checkout-7f9d-abc12
    evidence:
      service.spec.selector
      pod.metadata.labels
    class: derived

Pod checkout-7f9d-abc12
  --scheduled-on--> Node ip-10-42-2-19
    evidence: pod.spec.nodeName + resolved Node identity
```

This example demonstrates why relationship evidence classification is part of the provider contract rather than a rendering concern.

# Appendix F. Deletion Test

Before adding any new generic provider abstraction, ask:

> If Kubernetes, AWS, Azure and GCP were all removed tomorrow, would this abstraction still describe a coherent external-system integration need?

And:

> Can at least two materially different providers use it without one of them pretending to be the other?

If both answers are no, the concept probably belongs in a provider-specific specification rather than the generic host ABI.
