---
title: "ONO-SENDAI Cloud-Native Vision"
subtitle: "A Systems Shell for Cloud-Native Infrastructure"
author: "Project Strategy Document"
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

# ONO-SENDAI Cloud-Native Vision

## A Systems Shell for Cloud-Native Infrastructure

**Status:** Strategic product and ecosystem vision; non-release, non-implementation specification  
**Scope:** Product identity, cloud-native problem definition, strategic role of KUANG/11 providers, Kubernetes and cloud-provider direction, cross-system operating model, contributor strategy, CNCF alignment, boundaries and non-goals  
**Relationship:** Complements the immutable Ono-Sendai baseline and the v0.3+ extension specifications without replacing or renumbering them  
**Normative language:** This is primarily a strategy document. `MUST`, `MUST NOT`, `SHOULD`, `SHOULD NOT`, and `MAY` are used only where the vision establishes durable product guardrails rather than implementation details.

> Cloud infrastructure became structured and relational. The shell never did.

> Ono-Sendai should become the shell in which the system is not merely commanded, but understood as a system.

---

# 0. Document Status and Strategic Role

## 0.1 This is not a release specification

This document does not define Ono-Sendai v0.10, v0.11, v0.12, or any later numbered release. Existing and already-reserved release specifications remain independent.

The cloud-native direction is deliberately maintained as a **parallel product and ecosystem track**. Capabilities described here may enter Ono-Sendai across multiple releases, through KUANG/11 packages, through first-party providers, or through later architecture specifications.

A release number answers:

> What is delivered in version X?

This document answers:

> What real-world problem should Ono-Sendai become unusually good at solving, and why does that direction justify the project?

Those questions MUST remain separate.

## 0.2 Relationship to the existing Ono-Sendai thesis

The current project already establishes several durable principles:

- structured values survive the pipeline;
- Unix remains first-class rather than being hidden behind a replacement universe;
- one small language operates across domains;
- relationships are first-class and carry provenance;
- the machine has a navigable geography;
- remote systems are places in the same mental model;
- uncertainty remains visible;
- danger should become visible before damage;
- KUANG/11 extensions contribute real objects and relationships rather than cosmetic command wrappers.

The cloud-native direction does not replace those ideas. It is the environment in which they become most valuable.

The long-term product identity remains broader than cloud:

> **Ono-Sendai is a systems interface.**

The first sharply differentiated operating domain should be:

> **Cloud-native infrastructure.**

## 0.3 Strategic intent

The goal is not to make Ono-Sendai interesting because it is a novel shell implementation.

The goal is to make operators miss Ono-Sendai when they return to another shell.

A successful outcome is not:

> "Ono has nicer syntax for AWS."

It is:

> "With Ono I can see how the things I am operating relate, move through those relationships, understand what changed, and act without rebuilding the system model in my head every time."

## 0.4 The maturation period is part of the strategy

Ono-Sendai already has an unusually broad specification backlog. The project therefore does not need continuous semantic expansion merely to appear active.

A deliberate maturation phase is strategically useful for four reasons:

1. **Stability creates trust.** A login shell must be boring in the places where shells are expected to be boring.
2. **Dogfooding exposes real friction.** A systems shell must be tested against ordinary work, not only against acceptance fixtures.
3. **Provider work creates contribution surfaces.** External systems are a natural place for domain experts to contribute without redesigning the core.
4. **Community maturity matters.** A CNCF-oriented project benefits from visible time, independent contribution, governance discipline, and repeatable releases.

Time is therefore not an absence of progress. It is a product input.

---

# 1. Executive Thesis

## 1.1 The category problem

Shells traditionally expose commands, streams, text, files, jobs, and processes.

Cloud-native infrastructure exposes something different:

- typed resources;
- declarative and observed state;
- identity across APIs;
- explicit and implicit relationships;
- dynamic topology;
- event streams;
- policy and permission boundaries;
- partial failure;
- eventual consistency;
- resources that span machines and providers;
- changes whose effect is larger than the command that caused them.

The dominant interaction model is still a collection of provider-specific executables:

```text
kubectl
helm
aws
az
gcloud
terraform
ssh
systemctl
journalctl
jq
yq
grep
```

Each tool can be excellent inside its own boundary. The operator is still responsible for stitching the boundaries together.

## 1.2 The central problem

Cloud-native systems are graphs. The operational interface is fragmented into calls.

When a user asks:

> Why is this application unreachable?

The answer may cross:

```text
Ingress
  -> Service
    -> EndpointSlice
      -> Pod
        -> Node
          -> cloud VM
            -> NIC
              -> security policy
                -> subnet
                  -> route
```

The next hop may continue into the node:

```text
Pod
  -> container
    -> process
      -> socket
```

Today the human usually performs that graph traversal manually. They switch tools, translate identifiers, compare timestamps, remember provider semantics, and maintain an internal model of what each output means.

The burden is not primarily command syntax.

The burden is **system reconstruction**.

## 1.3 Ono-Sendai's opportunity

Ono-Sendai should make system reconstruction an explicit function of the shell.

The shell should preserve and expose:

```text
identity
structure
relationships
provenance
space
time
change
uncertainty
risk
recovery
```

The operator should be able to ask the same small set of conceptual questions across local Linux, Kubernetes, and cloud infrastructure:

1. **What is here?**
2. **How is it related?**
3. **What is happening?**
4. **What changed?**
5. **Why might it be happening?**
6. **What would happen if I change this?**
7. **How safely can I act?**

The interface vocabulary should remain recognizably Ono:

```text
get
where
select
inspect
look
near
enter
follow
trace
find place
watch
past
diff
why
explain
plan
apply
recover
```

Cloud-native capability should deepen the existing language, not create a second cloud language beside it.

## 1.4 Product statement

The recommended product statement is:

> **Ono-Sendai is a systems shell for cloud-native infrastructure.**

A longer form:

> **Ono-Sendai preserves the structure and relationships of the systems operators work with, so Kubernetes, cloud resources, hosts, services and processes can be navigated, queried and operated through one consistent systems model.**

The long-term identity remains:

> **A shell that understands the systems you are operating.**

## 1.5 The strategic line

A useful public line is:

> **Cloud infrastructure became structured and relational. The shell never did.**

This sentence is useful because it identifies the problem rather than advertising a feature.

---

# 2. The Problem Ono Must Solve

## 2.1 Too many grammars are a symptom, not the disease

One obvious cloud-operations problem is command sprawl:

```text
kubectl get pods -A
aws ec2 describe-instances
az vm list
gcloud compute instances list
```

A smaller grammar helps, but syntax normalization alone does not justify a new shell.

If Ono merely maps these commands to:

```text
get pod
get ec2-instance
get azure-vm
get gcp-instance
```

then Ono has created a nicer dispatcher, not a new operating model.

The deeper problem is that the outputs are isolated views of one operational reality.

## 2.2 Structure is repeatedly destroyed and reconstructed

Cloud APIs already know that a resource is a resource with fields and identity. Kubernetes knows a Pod is not a line of text. AWS knows an EC2 instance has interfaces, security groups, tags, state and an instance identity. Azure and GCP expose equally structured APIs.

Yet many workflows still become:

```text
structured API
   -> CLI rendering / JSON
      -> jq / yq / grep
         -> human interpretation
            -> next provider-specific query
```

Ono-Sendai's original structured-pipeline thesis generalizes naturally:

> Do not flatten information that the source already knows.

For cloud-native operation, the extension is:

> Do not destroy relationships that the source can prove.

## 2.3 The operator is the integration layer

A production incident frequently requires the operator to know facts such as:

- which Kubernetes Node corresponds to which cloud instance;
- which Service selects which Pods;
- which Pod is owned by which workload;
- which load balancer points to which target group or backend;
- which security controls apply to which interface;
- which identity a workload assumes;
- which host process owns a listening socket;
- which recent change preceded a failure;
- which evidence comes from which system and how current it is.

No single command line contains that whole answer.

The human therefore becomes the integration layer between tools.

Ono should move part of that integration burden into the system model while keeping evidence visible.

## 2.4 Context switching creates operational risk

Every tool boundary introduces opportunities for mistakes:

- wrong cluster;
- wrong namespace;
- wrong cloud account;
- wrong subscription or project;
- wrong region;
- stale credentials;
- ambiguous resource names;
- copied identifiers from the wrong environment;
- assumptions that two similarly named resources are related;
- permission failure misread as absence;
- stale data misread as current state.

Ono already has context, place, provenance and uncertainty concepts. Cloud-native operation should use them to make context visible rather than implicit.

## 2.5 Time is fragmented

Cloud-native systems contain many histories:

- Kubernetes Events;
- controller transitions;
- audit logs;
- provider activity logs;
- CloudTrail-like audit streams;
- metrics and health transitions;
- deployment histories;
- local journald events;
- filesystem changes;
- operator command history.

Traditional shells remember commands. They do not inherently provide a system history.

Ono's temporal model creates a stronger possibility:

> Command history answers what the operator typed. System history answers what the system was observed to do.

Cloud-native operation needs the latter.

## 2.6 Change is larger than invocation

A single API call can trigger asynchronous consequences:

```text
change desired replicas
  -> controller reconciles
    -> pods created
      -> scheduler places them
        -> CNI configures networking
          -> load balancer health changes
```

A shell that only reports "command succeeded" is reporting transport success, not system success.

Ono's prospective-change and verification direction is therefore especially relevant to cloud-native systems.

---

# 3. Why Cloud-Native Is the Right First Differentiated Domain

## 3.1 It magnifies Ono's existing strengths

Cloud-native systems make each existing Ono concept more useful:

| Existing Ono concept | Cloud-native amplification |
|---|---|
| Typed values | Provider APIs already expose structured data |
| Relationships | Infrastructure is inherently relational |
| Places and navigation | Resources form natural navigable spaces |
| Remote systems | Remote is the default case |
| Provenance | Multiple APIs may disagree or cover different facts |
| Time and causality | Events and control loops create rich temporal evidence |
| Prospective change | API mutations often have wide asynchronous effects |
| Risk visibility | Wrong scope or identity can damage production quickly |
| KUANG/11 | Provider ecosystems are natural extension boundaries |

The domain therefore does not require Ono to invent a new personality. It gives the existing personality a real operational target.

## 3.2 It creates obvious demonstrations

A differentiated open-source project needs demonstrations that are understandable in minutes.

The following demo is immediately legible:

```text
enter service checkout
trace
follow selected-by
follow scheduled-on
follow hosted-by
```

and produces a path from Kubernetes to the cloud instance underneath.

A stronger demonstration continues:

```text
follow runs
follow listens-on
```

and reaches a local process or socket.

The value can be seen without a long explanation of shell-language design.

## 3.3 It creates bounded contribution areas

A monolithic shell core is difficult for new contributors to enter.

Provider ecosystems create smaller ownership domains:

- Kubernetes provider;
- AWS provider;
- Azure provider;
- GCP provider;
- OpenStack provider;
- Proxmox provider;
- Docker/Podman provider;
- service-specific providers;
- relationship resolvers;
- event-source integrations;
- provider conformance tests.

A contributor can know a system deeply without first understanding every part of Ono's parser, job control or renderer.

## 3.4 It aligns with CNCF participation without reducing Ono to Kubernetes

The project goal of eventually entering the CNCF ecosystem should influence project maturity and community strategy, but should not distort the technical design into "Kubernetes shell only".

Kubernetes is strategically important because it is:

- central to cloud-native operations;
- highly structured;
- relationship-heavy;
- extensible through CRDs;
- eventful and dynamic;
- a strong test of provider abstractions.

But Ono's value grows when Kubernetes is connected to the systems around it.

The strategic shape is:

```text
                 Ono-Sendai
                     |
        cloud-native systems model
                     |
      +--------------+--------------+
      |              |              |
  Kubernetes        AWS           Azure
      |              |              |
      +------------- GCP ------------+
                     |
                 local Linux
```

Kubernetes should be the reference external-system provider, not the edge of the product.

---

# 4. Product Identity and Positioning

## 4.1 What Ono is

Ono is a shell whose native abstraction is increasingly **the system** rather than the command output.

A useful category stack is:

```text
Bash / zsh
  command interpreters with Unix process and text composition

PowerShell / Nushell
  structured shells with object/data pipelines

Ono-Sendai
  systems interface built on a structured shell
```

This distinction MUST be earned through functionality. Marketing language alone is not sufficient.

## 4.2 What "cloud-native shell" should mean

The phrase MUST NOT mean merely:

- runs in a container;
- bundles `kubectl`;
- has cloud completions;
- can execute cloud CLIs;
- supports YAML well;
- has shortcuts for provider APIs.

For Ono, "cloud-native shell" should mean:

1. cloud and orchestration resources are native typed objects;
2. provider-asserted relationships survive as first-class graph edges;
3. resource identity is stable and explicit;
4. scopes such as cluster, account, subscription, project, region and namespace are visible context;
5. provenance is attached to facts and relationships;
6. watches and eventual consistency are modeled rather than hidden;
7. resources can be navigated as places;
8. cross-system relationships can connect providers without pretending they are identical;
9. mutations participate in Ono's explain/plan/risk/verification model;
10. the same core language works across systems.

## 4.3 Positioning against provider CLIs

Provider CLIs remain valuable and MUST remain usable from Ono as ordinary Unix programs.

Ono should not attempt to win by reproducing every command and option of:

```text
kubectl
aws
az
gcloud
```

That is an infinite parity race with weak differentiation.

Instead, Ono should win workflows where **relationships, context and multi-system reasoning matter**.

Provider CLIs answer:

> Call this API.

Ono should answer:

> Show me this system, where this thing sits in it, and what is connected to it.

## 4.4 Positioning against IaC

Ono is not Terraform, OpenTofu, Pulumi, Crossplane, Helm, or a GitOps controller.

Those systems define, reconcile or apply desired infrastructure state.

Ono should complement them by helping a human understand the **observed operational system** and safely perform bounded actions.

Ono MAY inspect IaC provenance and MAY link observed resources back to definitions, states, revisions or controllers. It MUST NOT require Ono to become the source of truth for infrastructure declarations.

## 4.5 Positioning against observability platforms

Ono is not a monitoring database, APM backend, metrics lake or long-term log platform.

It MAY consume evidence from those systems and integrate their observations into an operator workflow.

The distinction is:

> Observability systems collect and retain evidence at scale.

> Ono provides an interactive systems interface that can traverse and reason over evidence available to the current operation.

## 4.6 Positioning against AI shells

AI assistance can be valuable, but AI is not the differentiator.

A language model can suggest:

```text
kubectl describe pod ...
```

Ono's stronger value is that it can know, from registered providers and evidence:

- which Pod is in context;
- what owns it;
- where it is scheduled;
- which cloud resource hosts it;
- which facts are verified;
- which facts are stale;
- which permissions are missing;
- which recent observed changes are relevant.

AI assistants in KUANG/11 SHOULD consume the same system model and provenance that a human sees. They MUST NOT become a parallel source of fabricated system truth.

---

# 5. The Cloud-Native Systems Model

## 5.1 Resources are not commands

A provider integration starts with system concepts, not CLI verbs.

For example, Kubernetes contributes concepts such as:

```text
Namespace
Deployment
ReplicaSet
Pod
Service
EndpointSlice
Ingress
Node
PersistentVolumeClaim
```

AWS contributes concepts such as:

```text
Account
Region
VPC
Subnet
EC2Instance
NetworkInterface
SecurityGroup
LoadBalancer
TargetGroup
IAMRole
```

The provider maps those concepts into Ono values, identity and relationships.

The user-facing verbs remain mostly Ono verbs.

## 5.2 Provider-native identity remains canonical

Ono MUST NOT erase provider identity in the name of multi-cloud convenience.

Examples:

```text
k8s.core.v1.Pod
aws.ec2.Instance
azure.compute.VirtualMachine
gcp.compute.Instance
```

A semantic role such as `compute` may provide cross-provider discovery, but it MUST NOT replace provider-native types.

This prevents a lowest-common-denominator abstraction from destroying important semantics.

## 5.3 Semantic roles are overlays, not replacements

Cross-provider roles MAY classify resources into broad operational concepts:

```text
compute
network
identity
storage
database
load-balancer
queue
function
secret
cluster
workload
```

This enables queries such as:

```text
find resource --role compute
find resource --role database --where encrypted == false
```

The role layer MUST preserve access to the native schema and provider-specific fields.

## 5.4 Relationships are first-class

The graph is central to the value proposition.

Relationships may include:

```text
owns
selected-by
selects
scheduled-on
hosted-by
attached-to
member-of
protected-by
routes-to
mounts
uses
assumes
exposes
listens-on
runs
```

A relationship MUST have:

- source identity;
- target identity or unresolved target descriptor;
- relation type;
- provider or resolver origin;
- provenance;
- observation time or validity metadata;
- confidence/evidence semantics where the edge is inferred rather than provider-declared.

The system MUST distinguish a relationship the provider explicitly asserts from a relationship Ono infers from matching identifiers.

## 5.5 Places provide an operational projection of the graph

Not every graph edge is a navigation hierarchy.

Ono's earlier distinction remains important:

- `up` answers where an object belongs;
- `back` answers where the user has been;
- `follow` traverses a relationship;
- `near` exposes nearby meaningful entities;
- `trace` returns graph paths and evidence.

Cloud-native providers SHOULD map resources into spatial navigation without collapsing graph semantics into a tree.

## 5.6 Context must be impossible to ignore accidentally

Cloud operation requires explicit context dimensions.

Examples include:

```text
provider
credential identity
organization / tenant
account / subscription / project
cluster
region
zone
namespace
resource
```

The active context SHOULD be visible in the prompt or rich TTY context projection when relevant.

Commands MUST NOT silently change cloud account, subscription, project, cluster or equivalent security context as a side effect of unrelated navigation.

## 5.7 Unknown and denied are different from empty

A provider query may return no objects because:

- none exist;
- the caller lacks permission;
- the API is unavailable;
- a region was not queried;
- the provider deliberately limited scope;
- cached data is stale;
- a page failed;
- the object disappeared during enumeration.

Ono MUST NOT normalize these cases into an empty collection without metadata.

The truth rule remains:

> Absence of evidence is not evidence of absence.

---

# 6. The Core User Experience

## 6.1 Connection should establish a world, not a mode

A conceptual interaction may look like:

```text
local://~ > link cloud prod
linked prod: kubernetes aws

local://~ > enter link prod
prod:// > look
```

The exact command syntax belongs to later provider and architecture specifications. The important property is that the user enters an external system as another place in Ono's world rather than launching a separate mini-shell.

## 6.2 Kubernetes example

A user investigating checkout traffic might begin:

```text
prod:// > find place --where name == "checkout"
```

Ono may return several typed matches:

```text
TYPE          NAME       SCOPE
Deployment    checkout   k8s/prod/shop
Service       checkout   k8s/prod/shop
Pod           checkout-7f9d...  k8s/prod/shop
```

The user chooses the Service:

```text
prod:// > enter service checkout
k8s://prod/shop/service/checkout > near
```

A useful projection may show:

```text
Service checkout
|
+-- selected-by/selector -> Pod checkout-7f9d...
+-- selected-by/selector -> Pod checkout-6ac1...
+-- routed-from          -> Ingress public
+-- exposes              -> 10.42.7.12:8080
```

The value is not the pretty tree. The value is that these are typed relationships with provenance that can be piped, inspected and traversed.

## 6.3 Cross-system example

From a Pod:

```text
k8s://.../pod/checkout-7f9d > follow scheduled-on
k8s://.../node/ip-10-42-2-19 > follow hosted-by
aws://prod/eu-central-1/ec2/i-0abc... >
```

From the cloud instance:

```text
> follow protected-by
> follow member-of
```

The user can now move through the infrastructure without copying identifiers between tools.

## 6.4 Local-system continuation

Where a trusted relationship exists between a cloud instance and an Ono remote host link, navigation may continue:

```text
aws://.../ec2/i-0abc > follow observed-as
prod-node-7:// > get service
```

This is a strategic differentiator:

```text
Kubernetes workload
   -> Kubernetes node
      -> cloud instance
         -> Linux host
            -> systemd service
               -> process
                  -> socket
```

The shell becomes a continuous operational space.

## 6.5 Troubleshooting is the primary proving workflow

The strongest early use case is troubleshooting, because troubleshooting naturally demands graph traversal and context integration.

Representative questions include:

```text
What serves this endpoint?
Where is this workload running?
What security policy governs this path?
What changed before this target became unhealthy?
Which resources depend on this object?
Which layer currently reports failure?
Which evidence is fresh and which is stale?
```

A cloud-native provider is successful when Ono reduces the amount of manual system reconstruction required to answer these questions.

## 6.6 Mutation should follow understanding

Cloud write operations MUST NOT become a broad imperative CLI clone.

Ono should prioritize mutations that can participate meaningfully in:

- `explain`;
- target resolution;
- permission disclosure;
- prospective effects;
- risk classification;
- protection or reversibility statements;
- verification.

The ideal flow is:

```text
inspect
trace
plan
explain
apply
verify
```

not:

```text
memorize-provider-verb --many --flags
```

---

# 7. KUANG/11 as the Provider Ecosystem

## 7.1 Strategic definition

The recommended ecosystem definition is:

> **KUANG/11 is the framework through which Ono learns about systems.**

This is stronger than defining KUANG/11 primarily as a plugin mechanism for adding commands.

A provider package teaches Ono:

```text
what kinds of things exist
how they are identified
how they are discovered
what properties they have
how they relate
what can be observed
what can change
what the provider can prove
what it cannot know
```

## 7.2 Provider packages should contribute semantic capability

A provider MAY contribute commands when a domain genuinely requires them, but the default contribution model should be:

- schemas;
- resource discovery;
- relationships;
- spatial integration;
- observations and watches;
- actions;
- help and discoverability metadata;
- provider-specific render hints where justified;
- tests and conformance fixtures.

The host remains responsible for Ono language semantics, pipeline behavior, trust boundaries and general rendering.

## 7.3 The ecosystem should be contributable in slices

A provider need not implement every possible feature on day one.

A useful maturity ladder is:

```text
Level 0  Identity and health
Level 1  Read-only resource discovery
Level 2  Relationships and navigation
Level 3  Watch/event integration
Level 4  Safe bounded actions
Level 5  Temporal and prospective integration
```

A community provider can be useful at Level 2 without pretending to support safe mutation.

Capability negotiation MUST make missing levels explicit.

## 7.4 Provider quality is product quality

A broken cloud provider can cause incorrect operational conclusions even if Ono core is flawless.

Provider quality therefore requires:

- deterministic schema contracts;
- explicit compatibility ranges;
- permission-aware errors;
- pagination correctness;
- rate-limit behavior;
- stale-data semantics;
- identity stability tests;
- relationship provenance tests;
- destructive-action gates;
- integration fixtures against realistic API behavior.

The provider conformance suite should become one of the easiest ways for external contributors to know whether their integration is trustworthy.

---

# 8. Kubernetes as the Reference External-System Provider

## 8.1 Why Kubernetes first

Kubernetes is the ideal reference implementation because it stresses nearly every dimension required by the provider model:

- many resource kinds;
- discoverable APIs;
- namespaced and cluster-scoped identity;
- selectors and owner references;
- desired versus observed state;
- conditions;
- watches;
- rapid object churn;
- RBAC;
- CRDs;
- version skew;
- controller-driven asynchronous behavior.

If the generic provider model handles Kubernetes cleanly without Kubernetes-specific leakage into Ono core, the architecture is likely on a sound path.

## 8.2 Kubernetes must not become a special language

Ono should resist creating a parallel vocabulary such as:

```text
kubectl-like verbs in Ono grammar
```

where existing Ono verbs already express the operation.

Examples:

```text
get pod
where namespace == "shop"
enter pod checkout-...
follow owned-by
watch pod
inspect
```

Kubernetes-specific operations may exist where the domain genuinely has unique semantics, but they should be exceptions.

## 8.3 CRDs are a core requirement, not an afterthought

A provider that understands only built-in Kubernetes objects does not understand real Kubernetes environments.

The Kubernetes provider must eventually support unknown and dynamically discovered resource kinds while preserving:

- Group/Version/Kind;
- schema where available;
- identity;
- metadata;
- owner references;
- labels and selectors where meaningful;
- generic conditions/events where exposed;
- extension-defined relationships when a plugin or resolver knows them.

The system must degrade to honest generic resource handling rather than silently ignoring unfamiliar kinds.

---

# 9. AWS, Azure and GCP as Architecture Proofs

## 9.1 The purpose of cloud-provider integrations

AWS, Azure and GCP are not valuable merely because they are popular APIs.

Together they test whether Ono's systems model is truly provider-independent.

Each has different notions of:

- hierarchy;
- scope;
- identity;
- region and zone;
- IAM;
- API consistency;
- tagging;
- network topology;
- event/audit sources;
- resource lifecycle.

If all three can fit the same external-system contract without flattening provider semantics, that is strong evidence that the abstraction is correct.

## 9.2 No forced uniformity

Ono MUST NOT pretend that:

```text
AWS account == Azure subscription == GCP project
```

in every semantic sense.

They may all play a broad "administrative scope" role for some queries, but their native types, permissions and lifecycle semantics remain distinct.

Similarly:

```text
EC2 instance
Azure VM
GCE instance
```

may all have role `compute`, while retaining provider-native schemas.

## 9.3 Breadth should follow operational value

Cloud providers have enormous APIs. Full service parity is not an appropriate early goal.

Provider specs should prioritize resource domains that produce a coherent operational graph:

1. identity and scope;
2. compute;
3. networking;
4. load balancing;
5. storage attachment;
6. cluster integration;
7. common managed data services;
8. audit/change evidence.

A small connected graph is more valuable than a large disconnected catalog.

---

# 10. Cross-System Relationships: The Differentiator

## 10.1 Single-provider support is necessary but insufficient

Many tools can inventory or query a single provider.

Ono's highest-value capability appears when a user can move across provider boundaries while preserving identity and evidence.

The canonical example is:

```text
Ingress
  -> Service
    -> Pod
      -> Node
        -> cloud instance
          -> Linux host
            -> process
```

## 10.2 Cross-system edges need stronger evidence rules

Provider-local relationships may be directly asserted by an API.

Cross-system relationships are often discovered through correlating identifiers:

- Kubernetes `spec.providerID`;
- instance metadata;
- cloud instance IDs;
- node addresses;
- machine identity;
- container runtime metadata;
- workload identity bindings;
- load-balancer annotations;
- CNI metadata.

Ono MUST distinguish evidence classes.

A conceptual confidence vocabulary may include:

```text
VERIFIED
STRONG
INFERRED
AMBIGUOUS
UNKNOWN
```

The exact canonical type belongs in the cross-system relationship specification, not this vision document.

The invariant is simple:

> A guessed relationship must never render as a provider-proven relationship.

## 10.3 Contradictions are first-class

Different providers or evidence sources may disagree.

For example:

- Kubernetes reports a Node address that no longer matches a cloud interface;
- a cloud instance is terminated while a stale Kubernetes Node still exists;
- an object name matches but identity does not;
- cached inventory conflicts with a live query.

Ono should expose contradictions rather than choose a plausible answer silently.

This is central to the project's truth-first philosophy.

---

# 11. Temporal and Causal Operation in the Cloud

## 11.1 Cloud-native systems are reconciliation systems

A command often changes desired state and triggers controllers rather than causing one immediate final state.

The temporal model should therefore distinguish:

```text
request accepted
state mutation observed
controller reaction
resource creation/deletion
health transition
steady state / failure
```

A successful API response is not proof that the intended system effect occurred.

## 11.2 Evidence may come from many sources

A useful incident timeline may combine:

```text
Ono command history
Kubernetes Events
Kubernetes object revisions
cloud audit/activity logs
load-balancer health
provider resource state
local journald
process lifecycle
```

Every event or observation must preserve provenance and coverage semantics.

## 11.3 `why` must remain evidence-disciplined

The cloud-native direction makes causal overclaiming especially dangerous.

A timeline such as:

```text
14:21 security group changed
14:22 health checks begin failing
14:23 target marked unhealthy
```

supports temporal and dependency reasoning. It does not automatically prove causation.

Ono should be valuable precisely because it can say:

```text
PRECEDED_BY
CORRELATED_WITH
DEPENDENCY_PATH_EXISTS
CAUSALITY_NOT_PROVEN
```

rather than manufacturing certainty.

---

# 12. Safe Change in Cloud-Native Systems

## 12.1 The safety model must survive asynchronous systems

Cloud actions frequently have delayed effects and incomplete rollback semantics.

A safe-change model must therefore describe separately:

- API reversibility;
- data reversibility;
- traffic effects;
- identity and permission effects;
- resources created indirectly by controllers;
- irreversible external side effects;
- verification window;
- uncertainty.

## 12.2 "Rollback available" is usually too weak

An inverse API call is not equivalent to returning the system to a previous state.

For example, temporarily allowing traffic may create external requests whose effects cannot be undone by removing the rule later.

Ono should report:

```text
REVERSIBLE CONFIGURATION
IRREVERSIBLE EXTERNAL EFFECTS POSSIBLE
VERIFICATION AVAILABLE
RECOVERY SCOPE: policy object only
```

rather than a misleading boolean.

## 12.3 Read-only usefulness comes first

A provider does not need mutation to justify itself.

The project should prefer excellent read-only understanding over broad unsafe write parity.

The order of trust should be:

```text
see correctly
understand relationships
observe change
explain action
plan safely
mutate narrowly
verify
```

---

# 13. Contributor and Maintainer Strategy

## 13.1 The project needs domains people can own

A contributor is more likely to become a maintainer when there is a bounded subsystem they can understand, improve and feel responsible for.

Cloud-native providers offer exactly that.

Possible ownership areas include:

- provider runtime/conformance;
- Kubernetes core provider;
- Kubernetes CRD generic handling;
- AWS network graph;
- Azure identity integration;
- GCP inventory;
- cross-system relationship resolvers;
- test fixtures;
- provider documentation;
- examples and demo environments.

## 13.2 Contribution must not require core-shell expertise

A strong provider SDK should allow a contributor to create useful integration without touching:

- parser internals;
- job control;
- terminal rendering;
- shell startup;
- external command adaptation.

The extension boundary is successful when domain expertise is enough to begin.

## 13.3 Small complete providers beat giant unfinished ones

A contribution should be able to say:

> This provider supports these resource types, these relationships, these watch capabilities and no mutations.

That is healthier than claiming generic cloud support with unpredictable gaps.

Capability declarations and generated documentation should make support boundaries visible.

---

# 14. CNCF Direction

## 14.1 CNCF is an ecosystem goal, not a feature

The project should pursue CNCF participation because it can provide:

- visibility among cloud-native practitioners;
- a credible community home;
- contributor discovery;
- governance pressure that reduces single-maintainer risk;
- technical feedback from adjacent projects.

It must not become a reason to add superficial Kubernetes features.

## 14.2 The best CNCF story is a real operational need

The strongest eventual project narrative is not:

> "Ono is a Rust shell that supports Kubernetes."

It is:

> **Cloud-native operators work with strongly related resources through disconnected command-line tools. Ono-Sendai provides a typed, relationship-aware systems interface across Kubernetes, cloud infrastructure and the hosts underneath.**

That positions Ono as a novel interface layer rather than another provider CLI.

## 14.3 Readiness must be built before application

A later dedicated CNCF-readiness document should track:

- governance;
- maintainer diversity;
- contributor activity;
- security process;
- release maturity;
- project documentation;
- adopter evidence;
- public roadmap;
- community interaction;
- CNCF application readiness.

This vision document intentionally does not duplicate those operational criteria.

---

# 15. Non-Goals and Guardrails

## 15.1 Ono is not a CLI parity project

The project MUST NOT measure cloud success by percentage coverage of provider CLI commands.

A 10% API surface that forms a useful connected system may be more valuable than 80% disconnected API wrappers.

## 15.2 Ono is not a universal cloud abstraction

The project MUST NOT erase native provider semantics to produce an artificially uniform cloud API.

Common roles are acceptable. False equivalence is not.

## 15.3 Ono is not an IaC source of truth

Ono MUST NOT evolve into a competing declarative infrastructure language merely because cloud actions exist.

## 15.4 Ono is not a monitoring backend

Ono MUST NOT require long-term ingestion and storage of all telemetry in order to be useful.

It may integrate with systems that do.

## 15.5 Ono is not an arbitrary graphical cloud console

Rich TTY and Deck capabilities may project the systems model visually. They MUST remain projections of the same model rather than a separate dashboard ontology.

## 15.6 Ono is not an AI agent that happens to have a shell

AI assistants may accelerate reasoning and discovery. The authoritative model remains provider evidence, schemas, relationships and explicit uncertainty.

## 15.7 The core should not absorb provider churn

Provider-specific APIs change rapidly. The core shell should expose stable extension contracts and keep provider churn at the edge whenever possible.

---

# 16. Success Criteria

## 16.1 User-success criteria

The cloud-native strategy is succeeding when users can answer common operational questions with less tool switching and less manual identifier translation.

Representative tests:

- Can a user begin with an application-facing Kubernetes object and reach the cloud instance underneath?
- Can they see which evidence establishes each relationship?
- Can they distinguish unavailable data from empty data?
- Can they move across cluster/account/region context without losing orientation?
- Can they inspect recent system changes without reconstructing a timeline from several CLIs manually?
- Can they understand the likely scope of a change before execution?

## 16.2 Product-differentiation criteria

The strategy is working when a demonstration cannot be reproduced merely by changing command syntax in another shell.

If the main value can be described as:

> shorter alias for `kubectl` / `aws`

then the implementation has missed the vision.

If the value is:

> one typed, navigable, provenance-aware graph across systems

then the project is on target.

## 16.3 Community criteria

The provider ecosystem is healthy when:

- external contributors can build providers without deep core changes;
- providers have clear conformance tests;
- maintainership can be delegated by domain;
- documentation is generated from provider contracts where possible;
- unsupported capability is explicit;
- provider packages can mature independently.

## 16.4 Architecture criteria

The provider model is healthy when Kubernetes, AWS, Azure and GCP can all fit it without any one provider's ontology becoming the core ontology.

A warning sign is a growing list of core exceptions named after specific providers.

---

# 17. Recommended Document and Implementation Sequence

The cloud-native track should proceed through independent documents rather than one monolithic specification.

Recommended sequence:

```text
1. Cloud-Native Vision
       |
       v
2. External System Provider Specification
       |
       v
3. Kubernetes Provider Specification
       |
       +--> first reference implementation
       |
       v
4. AWS Provider Specification
       |
       v
5. Cross-System Relationships Specification
       |
       +--> Kubernetes <-> AWS <-> Linux proof
       |
       v
6. Azure Provider Specification
       |
       v
7. GCP Provider Specification
       |
       v
8. CNCF Readiness Plan
```

The sequence is architectural, not necessarily release-chronological.

Kubernetes and AWS together are the first major abstraction test:

- Kubernetes stresses dynamic schemas, watches, controllers and CRDs;
- AWS stresses scale of API surface, account/region scope, identity, consistency and infrastructure relationships.

Cross-system work should begin once both sides expose enough stable identity to form defensible links.

---

# 18. Decision Filter for Future Features

Every proposed cloud-native feature should be evaluated against the following questions.

## 18.1 Does it improve system understanding?

Does the feature help answer:

```text
What is here?
How is it related?
What is happening?
What changed?
Why might it be happening?
What would happen if I act?
```

If not, it needs unusually strong justification.

## 18.2 Does it preserve truth?

Does the feature make provenance, uncertainty, denied scope, stale data and inference visible?

If it requires pretending uncertain facts are certain, reject it.

## 18.3 Does it reuse Ono grammar?

If a provider needs dozens of new verbs merely to mirror its CLI, the abstraction should be challenged.

## 18.4 Does it belong in core?

If the capability is provider-specific and can live behind KUANG/11, it SHOULD stay out of core.

## 18.5 Does it create a maintainable contribution surface?

A feature that can be owned and tested independently is preferable to a cross-cutting framework expansion.

## 18.6 Would Ono still be Ono without it?

The project must remain willing to say no.

The strongest product profile is not produced by adding everything cloud operators use. It is produced by doing a small set of system-level interactions unusually well.

---

# 19. Canonical Strategic Examples

These examples are illustrative, not syntax commitments. Later architecture and provider specifications remain authoritative for exact commands and types.

## 19.1 Find without knowing the provider-specific name

```text
> find place --where public.endpoint == "shop.example.com"

Ingress public
  k8s://prod/shop/ingress/public
```

## 19.2 Traverse application to infrastructure

```text
> enter ingress public
> follow routes-to
Service checkout

> follow selects
Pod checkout-7f9d

> follow scheduled-on
Node ip-10-42-2-19

> follow hosted-by
EC2 i-0abc123
```

## 19.3 Inspect the evidence behind a cross-system edge

```text
> trace hosted-by | inspect

RELATION      hosted-by
SOURCE        k8s://prod/node/ip-10-42-2-19
TARGET        aws://prod/eu-central-1/ec2/i-0abc123
CONFIDENCE    verified
EVIDENCE      spec.providerID
OBSERVED      2026-09-03T15:42:11Z
PROVIDER      kubernetes
```

## 19.4 Ask what changed

```text
> past 30m | where related_to current
```

Possible projection:

```text
15:21:04  SecurityGroup ingress changed
15:22:51  target health changed -> unhealthy
15:23:17  Pod readiness changed -> false
```

## 19.5 Ask for disciplined causality

```text
> why current.health == "unhealthy"
```

Possible projection:

```text
OBSERVED
  target became unhealthy at 15:22:51

PRECEDED_BY
  security-group change at 15:21:04

DEPENDENCY
  health-check path traverses the changed policy

CAUSALITY
  not proven
```

## 19.6 Plan a bounded change

```text
> plan set security-policy sg-web allow tcp/443 from alb-production
```

Possible projection:

```text
PROPOSED
  add ingress rule

AFFECTS
  3 instances
  1 target group

PUBLIC EXPOSURE
  no new internet source introduced

REVERSIBILITY
  configuration inverse available

NOT RECOVERABLE
  effects of traffic admitted while rule is active
```

The user should recognize the same Ono concepts regardless of provider.

---

# 20. Final Thesis

Ono-Sendai does not need another dozen unrelated shell features to justify its existence.

It needs a problem where its existing ideas become indispensable.

Cloud-native operation supplies that problem:

- the systems are already structured;
- the systems are already relational;
- the systems are remote by default;
- identity and scope matter constantly;
- events and reconciliation make time important;
- changes have broad asynchronous effects;
- operators still reconstruct the system manually across tools.

Ono's opportunity is to preserve that reality instead of flattening it.

The intended progression is:

```text
commands
  -> typed values
    -> system objects
      -> relationships
        -> navigable space
          -> observed time
            -> prospective change
              -> cloud-native systems interface
```

The cloud-native strategy is successful when users stop thinking of Ono as:

> a shell with cloud plugins

and begin thinking of it as:

> **the place where the cloud-native system becomes understandable as one system.**

The project should keep the larger identity intact:

> **Bash is a command interpreter. PowerShell is an object shell. Ono-Sendai is a systems interface.**

Cloud-native infrastructure is where that claim should first become impossible to dismiss.

---

# Appendix A. Relationship to Existing Project Documents

This strategy relies on, but does not rewrite, the existing project contracts and philosophy, particularly:

- [`README.md`](../../README.md) - current public product thesis and KUANG/11 description;
- [`PHILOSOPHY.md`](../../PHILOSOPHY.md) - truth, structure, uncertainty and safety principles;
- [`docs/architecture/external-system-provider.md`](../architecture/external-system-provider.md) - the generic KUANG/11 contract this strategy's provider ecosystem is built on;
- [`docs/strategy/cncf-readiness.md`](cncf-readiness.md) - the community, governance and ecosystem conditions under which CNCF participation becomes appropriate;
- [`docs/specs/ono_sendai_shell_spec_v0.2.md`](../specs/ono_sendai_shell_spec_v0.2.md) - typed pipeline, providers, registries and extension foundations;
- [`docs/specs/ono_sendai_shell_spec_v0.3_external_command_adapters.md`](../specs/ono_sendai_shell_spec_v0.3_external_command_adapters.md) - honest structured/text interoperability;
- [`docs/specs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md`](../specs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md) - places, navigation, identity and relationships;
- [`docs/specs/ono_sendai_shell_spec_v0.5_temporal_causal_systems_interface.md`](../specs/ono_sendai_shell_spec_v0.5_temporal_causal_systems_interface.md) - observations, evidence, time and causal discipline;
- [`docs/specs/ono_sendai_shell_spec_v0.6_prospective_change_protection_recovery.md`](../specs/ono_sendai_shell_spec_v0.6_prospective_change_protection_recovery.md) - proposed state, protection, risk and recovery;
- [`docs/specs/ono_sendai_shell_spec_v0.7_presentation_consolidation_rich_tty.md`](../specs/ono_sendai_shell_spec_v0.7_presentation_consolidation_rich_tty.md) - presentation as projection rather than second ontology;
- later release specifications - retained independently and not renumbered by this strategy.

The Kubernetes Provider Specification named in section 17 is deliberately not part of this repository. The Kubernetes provider is maintained in a dedicated repository, [ono-sendai-kubernetes](https://github.com/godspeed-you/ono-sendai-kubernetes), which holds the single canonical copy of its provider-specific specification at `docs/architecture/kubernetes-provider.md`. A separate repository is not a separate CNCF project; whether provider repositories would ever be subprojects is a later governance question.

Where this document conflicts with a normative released specification, the normative specification wins until a deliberate additive architecture specification or ADR changes the contract.

# Appendix B. One-Page Product Test

Before adding a cloud-native capability, ask:

```text
Does this help Ono understand a system,
not merely call another API?

Does it preserve native structure?
Does it preserve identity?
Does it expose relationships?
Does it expose provenance?
Does it keep uncertainty visible?
Does it reuse Ono's language?
Does it work without AI guessing?
Does it avoid provider-specific leakage into core?
Can a contributor own and test it independently?
```

If most answers are no, the feature probably does not belong in this strategic track.
