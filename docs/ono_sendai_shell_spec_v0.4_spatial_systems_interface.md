---
title: "ONO-SENDAI"
subtitle: "Specification v0.4 - Spatial Systems Interface"
author: "Project Specification"
date: "2026-08-27"
geometry: "margin=18mm"
fontsize: 10pt
colorlinks: true
linkcolor: blue
urlcolor: blue
toc: true
toc-depth: 3
numbersections: false
---

# 0. Document Status and Relationship to Earlier Specifications

This document is the **standalone ONO-SENDAI v0.4 Spatial Systems Interface specification**.

It is a new product and architecture increment. It does not rewrite, amend in place, regenerate, or otherwise modify the published ONO-SENDAI v0.2 baseline specification or the standalone v0.3 External Command Adaptation Layer specification.

The relationship is:

```text
ONO-SENDAI v0.2
    base shell, language, typed values, providers,
    object pipelines, contextual navigation, remote links,
    KUANG/11, TUI foundations

        +

ONO-SENDAI v0.3
    external command adaptation layer,
    compatibility packs, adapter negotiation

        +

ONO-SENDAI v0.4
    spatial systems interface,
    discoverable system topology,
    navigable object spaces,
    maps, neighborhoods and live spatial state

        =

candidate ONO-SENDAI v0.4 product contract
```

## 0.1 Normative scope

The keywords **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, **MAY** and **RECOMMENDED** are normative within this document.

This specification defines:

- what the word **space** means in Ono-Sendai;
- how real system objects become places without inventing fictional geometry;
- how users discover objects before knowing their names;
- how hierarchy and graph relationships coexist;
- stable spatial identity and navigation history;
- the root system space and its canonical domains;
- spatial behavior for processes, services, networking, storage, containers, identities, devices and remote systems;
- commands such as `look`, `map`, `near`, `enter`, `follow`, `jump`, `back`, `up` and `home`;
- interactive map views and non-interactive textual projections;
- semantic zoom and clustering;
- live topology and change visualization;
- prompt/HUD behavior while moving through spaces;
- integration with typed pipelines, providers, external-command adapters and KUANG/11;
- security, permission, performance and caching rules;
- machine-readable registries required to derive implementation, tests, help and completion;
- exact acceptance criteria for a release-quality implementation.

This document deliberately leaves **no product-design questions open**. Implementation-specific details MAY be recorded in ADRs only when they do not alter the semantics defined here. If implementation reality makes a normative requirement impossible, the implementation MUST document the deviation rather than silently reinterpret this document.

## 0.2 Intent

The central purpose of v0.4 is to solve a specific deficiency:

> Ono-Sendai can already expose structured objects, but a structured object shell is not automatically a spatial systems interface.

A user who must already know that `nginx` exists before typing `enter nginx` has not discovered a space. The user has used a command-line shortcut. A spatial interface must answer three questions continuously and without prior object names:

1. **Where am I?**
2. **What is around me?**
3. **Where can I go from here?**

The intended emotional result is not decorative cyberpunk. It is orientation inside a real machine.

The intended technical result is:

> **Ono-Sendai turns real Unix system topology into a discoverable, navigable, live object space.**

The space MUST be earned by capability. No object, edge, location, movement or animation may exist merely to support a theme.

## 0.3 Base contracts inherited from v0.2 and v0.3

This document assumes the existing contracts for:

- typed values and canonical schemas;
- streams and backpressure;
- native system providers;
- provenance and confidence;
- context stacks;
- `trace` and relationship graphs;
- remote links;
- terminal rendering and interactive views;
- command resolution;
- KUANG/11 extension packages and capability controls;
- external command adapters from v0.3;
- structured errors and machine-readable registries.

Where this document references `Process`, `Service`, `Socket`, `File`, `Filesystem`, `Mount`, `Interface`, `Connection`, `User`, `Container`, `Device`, `Graph`, `Provenance` or `Stream<T>`, the existing canonical definition is inherited unless this document adds spatial metadata.

# 1. Product Thesis

## 1.1 The missing "space" in cyberspace

A command shell naturally encourages a request-response model:

```text
prompt -> command -> output -> prompt -> command -> output
```

Even when commands return structured objects, the user's mental position does not necessarily change. The user remains outside the system and asks it questions.

Ono-Sendai v0.4 introduces a second interaction model:

```text
place -> surroundings -> movement -> new place -> changed surroundings
```

The user can still use ordinary commands and pipelines at all times. Spatial interaction is an additional projection of the same object model, not a separate application mode with invented semantics.

### Intent

The purpose is to create a qualitative difference from Bash and PowerShell. The user should not merely think "this is a nicer shell". During exploration, the user should develop a persistent mental map of the machine and be able to move through that map.

## 1.2 Space is topology, not scenery

Ono MUST NOT map every technical object to a physical-world metaphor.

The following approaches are explicitly rejected as core semantics:

- filesystems as filing cabinets;
- processes as rooms;
- sockets as literal cables;
- services as buildings;
- remote hosts as planets;
- containers as boxes because they are called containers;
- arbitrary X/Y/Z coordinates assigned for visual effect.

Such metaphors may occasionally appear in documentation analogies, but they MUST NOT determine data structures, navigation behavior, command grammar or topology.

Instead, Ono defines space from six real properties:

```text
OBJECT IDENTITY
      +
HIERARCHY
      +
RELATIONSHIPS
      +
NEIGHBORHOOD
      +
NAVIGATION HISTORY
      +
LIVE STATE
      =
SYSTEM SPACE
```

If a relationship cannot be justified by system data, provider knowledge, configuration, user declaration or explicitly identified inference, it MUST NOT become a spatial edge.

## 1.3 Shell and space are two projections of one system

Ono MUST NOT have a "normal mode" and a "cyberpunk mode" with different truths.

The following commands may refer to the same underlying `Process` objects:

```text
get process | where cpu > 20
```

and:

```text
enter compute
map
```

The first is a data/pipeline projection. The second is a spatial projection.

A process selected on a map MUST be usable in a pipeline. A process returned by a pipeline MUST be enterable if it has spatial identity. The representations differ; the object does not.

### Intent

This prevents visual novelty from becoming a maintenance burden or a second-class interface. Every spatial feature must strengthen the object model already valuable to scripting and automation.

# 2. Core Spatial Invariants

An implementation conforming to v0.4 MUST obey all of the following invariants.

1. **Discovery before naming.** A user MUST be able to discover an object without already knowing its name.
2. **Location is explicit.** Ono MUST always be able to explain the current spatial context.
3. **Movement changes context.** `enter`, `follow` and `jump` MUST produce a new spatial context, not merely print an object.
4. **Every movement is reversible.** `back` MUST return through the actual navigation trail where the previous location still exists.
5. **Every edge is explainable.** `inspect relation` or equivalent MUST expose why two objects are considered related.
6. **Hierarchy and graph are separate concepts.** Parent/child spatial grouping MUST NOT be confused with arbitrary relationships.
7. **No fabricated geometry.** Screen layout may choose positions, but those positions MUST NOT become semantic coordinates.
8. **Stable identity beats transient identifiers.** A PID is an attribute, not necessarily the lifetime identity of a conceptual service or workload.
9. **The horizon is bounded.** Ono MUST show a manageable neighborhood rather than dumping the complete system graph.
10. **Zoom is semantic.** Higher-level views MUST aggregate real concepts, not merely hide random rows.
11. **Landmarks reflect significance.** Highlighting MUST be driven by real state, change, importance or user pinning.
12. **Live views reflect real change.** Motion and visual updates MUST correspond to actual topology or metric changes.
13. **Text remains sufficient.** Essential spatial operations MUST remain usable without a full-screen TUI.
14. **TTY richness is optional presentation.** Scripts and non-interactive commands MUST retain deterministic machine-readable semantics.
15. **Unix remains underneath.** Spatial navigation MUST NOT prevent ordinary external command execution.
16. **Providers own facts.** Ono's spatial layer composes provider data; it MUST NOT become an undocumented source of system truth.
17. **Unknown is visible.** Missing permission, unsupported provider data and uncertainty MUST not be rendered as absence.
18. **Remote boundaries are visible.** Crossing a host, namespace, container or mount boundary MUST be apparent.
19. **The user's place survives rendering changes.** Terminal resize or renderer selection MUST NOT change semantic location.
20. **Spatial state is inspectable and scriptable.** The user MUST be able to query the current space and neighborhood as structured values.

# 3. Terminology and Data Model

## 3.1 SpatialObject

Any Ono value that can participate in spatial navigation MUST have or be projectable to a `SpatialObject` identity.

Conceptual contract:

```text
SpatialObject {
    spatial_id: SpatialId
    object_type: TypeId
    canonical_ref: ObjectRef
    display_name: String
    scope: SpatialScope
    lifetime: LifetimeDescriptor
    provenance: Provenance
    capabilities: Set<SpatialCapability>
}
```

`SpatialId` MUST be opaque to users and stable for as long as the implementation can truthfully identify the same conceptual object.

The display name is not identity.

## 3.2 SpatialScope

A scope defines the execution and discovery boundary to which an object belongs.

Minimum scope kinds:

```text
HostScope
RemoteHostScope
ContainerScope
NamespaceScope
FilesystemScope
UserScope
PluginScope
```

Scopes MAY nest.

Example:

```text
host:web01
  -> container:payments-api
      -> namespace:net:[4026533331]
```

Crossing a scope boundary MUST be observable in the navigation trail and prompt/HUD.

## 3.3 Place

A `Place` is the current spatial interpretation of a `SpatialObject` or a canonical aggregate space.

Canonical aggregate places defined by v0.4 include:

```text
System
Compute
Network
Storage
Containers
Identity
Devices
```

A specific `Service`, `Process`, `Socket`, `Filesystem`, `Directory`, `Container`, `User`, `Interface` or other spatial object may also be a place.

## 3.4 HierarchicalEdge

A hierarchical edge represents containment or canonical spatial grouping.

Examples:

```text
System -> Compute
Compute -> Services
Services -> nginx.service
Storage -> Filesystems
Filesystem -> Mount
Directory -> child Directory
```

Hierarchy exists primarily to support orientation and zoom.

Hierarchy MUST NOT assert operational dependency unless such a dependency is separately represented as a relationship edge.

## 3.5 RelationshipEdge

A relationship edge represents a real connection between objects.

Minimum fields:

```text
RelationshipEdge {
    edge_id: EdgeId
    source: SpatialId
    target: SpatialId
    relation: RelationType
    direction: Direction
    confidence: Confidence
    provenance: Provenance
    observed_at: Timestamp
    validity: ValidityWindow?
    attributes: Record
}
```

Examples include:

```text
process --parent-of--> process
process --owns--> socket
process --opened--> file
service --controls--> process
socket --connected-to--> socket/endpoint
mount --backs--> directory
container --contains--> process
user --owns--> process
process --member-of--> cgroup
service --depends-on--> service
host --linked-to--> host
```

## 3.6 Neighborhood

A neighborhood is a bounded, ranked projection of objects and relationships around the current place.

It is not simply "all adjacent nodes".

Conceptual contract:

```text
Neighborhood {
    center: SpatialId
    groups: List<NeighborhoodGroup>
    landmarks: List<Landmark>
    hidden_count: Int
    generated_at: Timestamp
    completeness: Completeness
}
```

Neighborhood generation MUST consider:

- relationship relevance;
- object importance;
- recent change;
- current view purpose;
- user filters;
- terminal size;
- security and permission boundaries.

## 3.7 Landmark

A landmark is an object or condition promoted because it helps orientation or deserves attention.

Built-in landmark reasons MUST include:

```text
high_cpu
high_memory
failed
restarting
recently_changed
public_listener
privileged
storage_pressure
connection_spike
new_object
removed_object
security_boundary
remote_boundary
user_pinned
```

A landmark MUST always expose its reason.

# 4. Canonical System Topology

Every local host MUST expose a canonical root space even if some providers are unavailable.

```text
SYSTEM
|
+-- COMPUTE
|
+-- NETWORK
|
+-- STORAGE
|
+-- CONTAINERS
|
+-- IDENTITY
|
+-- DEVICES
```

Unavailable domains remain visible but carry an `unavailable`, `unsupported` or `permission_denied` state rather than disappearing silently.

## 4.1 Why canonical domains exist

The system graph itself is not hierarchical. A process may simultaneously belong to a service, user, cgroup, namespace and container and may open files and sockets.

The canonical root hierarchy therefore does not claim to represent the entire system ontology. It provides **orientation anchors**.

The user should be able to learn six stable directions in Ono and then encounter graph relationships underneath them.

### Intent

A map with hundreds of peers at the root does not feel spatial; it feels like an unfiltered graph database. Canonical domains provide a persistent geography without lying about underlying relationships.

# 5. Entry Experience and the Spatial Horizon

Starting an interactive Ono session MUST provide enough information to establish place and nearby possibilities without requiring an explicit discovery command.

The default interactive startup SHOULD render a compact root horizon similar in information content to:

```text
LOCAL / workstation

 compute      312 processes      14 services
 network      3 interfaces       47 connections
 storage      6 filesystems      1.8 TiB used
 containers   8 running
 identity     3 active users
 devices      29 visible

 notable
   rustc        cpu 87%
   postgres     mem 4.2 GiB
   /data        91% full
   nginx        restarted 18s ago

local:// >
```

The exact visual layout MAY vary with terminal size, but the semantics MUST include:

- current host/context identity;
- available canonical domains;
- compact counts or summaries;
- a bounded set of current landmarks;
- a prompt showing current spatial scope.

The startup horizon MUST NOT block startup on expensive global scans. Providers SHOULD populate expensive counts asynchronously and update the horizon when available.

### Intent

The first screen must communicate that the user is **somewhere**, not merely at an empty prompt. It must also teach discovery without documentation: visible domain names become possible destinations.

# 6. Spatial Command Language

The spatial language is intentionally small. It MUST remain distinct from the data manipulation language while composing with it.

Core spatial verbs are fixed by v0.4:

| Command | Purpose |
|---|---|
| `look` | Describe the current place and its immediate horizon. |
| `map` | Render or return a structured topology projection. |
| `near` | Return ranked neighboring objects around the current place. |
| `enter` | Move into a hierarchical child or explicitly selected object. |
| `follow` | Traverse a real relationship edge from the current place. |
| `jump` | Move directly to a resolved place outside the immediate neighborhood. |
| `back` | Return to the previous place in actual navigation history. |
| `up` | Move to the canonical hierarchical parent. |
| `home` | Return to the root system place of the active host/link. |
| `trail` | Show the navigation trail. |
| `find` | Discover and rank places by name, type, attributes or relationship. |
| `pin` | Mark an object as a user landmark. |
| `unpin` | Remove a user landmark. |

These names are normative for v0.4.

Aliases MAY exist, but documentation, completion and generated examples MUST teach the canonical commands above.

## 6.1 `look`

Syntax:

```text
look
look --json
look --all
look --changes [duration]
```

`look` MUST describe the current place, grouped exits/relationships, important attributes and landmarks.

`look` with no arguments MUST never require prior knowledge of object names.

Structured form:

```text
look --json -> PlaceView
```

A generic process view might contain:

```text
PROCESS / nginx / pid 1842

 state       running
 service     nginx.service
 user        www-data
 uptime      17d 04h

 nearby
   parent       systemd/1
   children     4
   sockets      2
   files        23
   namespaces   3
   cgroup       system.slice/nginx.service

 connected
   tcp :80
   tcp :443
   10.4.1.17:5432

 changed
   worker/1871 cpu +41%
```

The headings are presentation. The underlying object MUST remain structured.

## 6.2 `near`

Syntax:

```text
near
near <relation>
near --type <type>
near --changed [duration]
near --limit <n>
near --all
```

`near` returns a `Stream<SpatialNeighbor>`.

Default behavior MUST rank and bound results. `--all` requests the complete currently known one-hop neighborhood and MAY be expensive.

Example:

```text
process/nginx:// > near

RELATION        OBJECT                   STATE
service         nginx.service            running
parent          systemd/1                running
socket          tcp/:80                  listening
socket          tcp/:443                 listening
file            /etc/nginx/nginx.conf    open
connection      postgres:5432            established
```

## 6.3 `enter`

Syntax:

```text
enter <selector>
enter @<result-ref>
enter .
```

`enter` MUST resolve one place and push the previous place onto the navigation trail.

If the selector resolves to more than one place interactively, Ono MUST open a deterministic picker. In non-interactive mode, ambiguity MUST be an error.

`enter` SHOULD prefer hierarchical children and currently visible neighborhood objects over global search results.

Examples:

```text
enter compute
enter services
enter nginx
enter socket:443
```

## 6.4 `follow`

Syntax:

```text
follow <relation>
follow <relation> <selector>
follow @<edge-ref>
```

`follow` MUST traverse a relationship edge, not a canonical hierarchy edge.

Examples:

```text
follow service
follow socket :443
follow connection postgres
follow owner
follow parent
```

If multiple edges match, interactive selection is required.

The relation traversed MUST be recorded in the navigation trail.

## 6.5 `jump`

Syntax:

```text
jump <place-selector>
jump <link>/<place-selector>
jump @<bookmark>
```

`jump` performs global or cross-scope place resolution without requiring adjacency.

It is conceptually equivalent to teleportation in the spatial model and therefore MUST visibly record the source and destination in the trail.

Examples:

```text
jump service/postgresql
jump storage:/data
jump prod/web-01
```

## 6.6 `back`, `up`, `home`

`back` follows navigation history.

`up` follows canonical hierarchy.

These are deliberately different.

Example trail:

```text
SYSTEM
 -> COMPUTE
 -> SERVICES
 -> nginx.service
 --follow owns--> process/1842
 --follow socket--> tcp/:443
```

At `tcp/:443`:

```text
back
```

returns to `process/1842`.

```text
up
```

returns to the canonical parent of the socket in the currently active map projection, normally `NETWORK/SOCKETS`, not necessarily to the process.

`home` returns to the root `SYSTEM` place for the current host.

## 6.7 `trail`

Syntax:

```text
trail
trail --json
trail --compact
```

The trail MUST preserve:

- source place;
- destination place;
- movement kind;
- relationship where applicable;
- scope boundary crossings;
- timestamp.

The trail is session state, not command history.

## 6.8 `find`

Syntax:

```text
find <query>
find <type> <query>
find --where <expression>
find --near <place-selector> <query>
```

`find` MUST search the spatial index and provider registries rather than blindly grep rendered text.

Examples:

```text
find nginx
find service post
find process --where cpu > 50
find --near network postgres
```

Results MUST include enough path/scope information to disambiguate identical names.

## 6.9 `map`

Syntax:

```text
map
map <selector>
map --depth <n>
map --live
map --json
map --relations <list>
map --type <list>
map --changes [duration]
map --all
```

The default map MUST show the current place, canonical children, significant direct relationships and landmarks within a bounded semantic horizon.

`map --json` returns `SpatialMap` and MUST not depend on terminal rendering.

`map --live` subscribes to change events and updates topology in place on a TTY.

### Intent

These commands form an exploration vocabulary. The user can always return to `get`, `where`, `select`, external commands or scripts. Spatial verbs exist specifically to answer orientation and movement questions, not to replace data manipulation.

# 7. Root Space and Canonical Domains

## 7.1 Root `SYSTEM`

`home` MUST resolve to a `SystemPlace` representing the active host or remote host scope.

Conceptual schema:

```text
SystemPlace {
    host: HostRef
    hostname: String
    os: String
    kernel: String
    uptime: Duration
    domains: List<DomainSummary>
    landmarks: List<Landmark>
    links: List<LinkSummary>
    generated_at: Timestamp
}
```

The root MUST never be a flat list of every object known to Ono.

### Intent

The root is an orientation anchor. Its purpose is to let a user begin exploration before knowing a process name, service name, mount path, interface name or container identifier.

## 7.2 `COMPUTE`

`COMPUTE` MUST provide access to:

```text
processes
services
jobs
workloads
cgroups
```

`workloads` is a spatial aggregate, not a new mandatory provider type. Ono MAY use workload grouping when reliable evidence connects processes/services/containers into a meaningful unit.

Default `look` in `COMPUTE` SHOULD show:

- process count;
- service state summary;
- native shell jobs;
- high CPU and high memory landmarks;
- failed/restarting services;
- newly created or recently exited workloads.

Example:

```text
COMPUTE / web01

 processes     312
 services       46   42 running, 2 failed, 2 inactive
 jobs            3
 containers      8   linked through CONTAINERS

 active
   rustc/4419              cpu 87%
   postgres/812            mem 4.2 GiB
   nginx.service           18 workers

 changed
   backup.service          failed 2m ago
   nginx.service           restarted 18s ago

compute:// >
```

## 7.3 `NETWORK`

`NETWORK` MUST provide access to:

```text
interfaces
addresses
routes
neighbors
listeners
connections
namespaces
```

Default landmarks SHOULD include:

- public listeners;
- interfaces down or flapping;
- route changes;
- connection spikes;
- high traffic interfaces;
- denied/unknown network namespace visibility.

## 7.4 `STORAGE`

`STORAGE` MUST provide access to:

```text
filesystems
mounts
volumes/devices where known
directory roots
storage pressure landmarks
```

The spatial model MUST preserve normal Unix path semantics. Ono does not replace the filesystem tree with an alternative metaphor.

## 7.5 `CONTAINERS`

`CONTAINERS` MUST provide access to container-like scopes available through installed providers.

The domain MUST remain provider-neutral. Docker, Podman, containerd, Kubernetes or systemd-nspawn MAY contribute objects, but Ono's canonical spatial model SHOULD expose common concepts where semantics match.

## 7.6 `IDENTITY`

`IDENTITY` MUST provide access to:

```text
users
groups
active sessions
ownership relationships
privilege landmarks
```

## 7.7 `DEVICES`

`DEVICES` SHOULD expose hardware and kernel-visible devices for which providers can supply meaningful structure.

Minimum useful categories MAY include:

```text
block
network
input
serial
pci
usb
gpu
```

Devices MUST NOT be invented from arbitrary `/dev` filenames without provider semantics.

# 8. Semantic Zoom

A system graph can contain millions of objects. Spatial usability therefore depends on **semantic zoom**.

Zoom is not merely a graphical scale. It changes the level of conceptual aggregation while preserving drill-down paths.

## 8.1 Canonical zoom levels

Ono v0.4 defines five semantic levels:

```text
L0 - SYSTEM
     canonical domains and global landmarks

L1 - DOMAIN
     compute, network, storage, containers, identity, devices

L2 - COLLECTION
     services, processes, filesystems, interfaces, containers, users...

L3 - ENTITY
     nginx.service, process/1842, eth0, /data, container/payments...

L4 - DETAIL/RELATION
     socket :443, specific connection, open file, namespace, cgroup edge...
```

Providers MAY expose deeper detail levels, but the L0-L4 vocabulary is normative for renderer behavior and tests.

## 8.2 Clustering

When the visible object count exceeds the view budget, Ono MUST cluster rather than truncate arbitrarily.

Allowed clustering dimensions include:

- canonical collection;
- service;
- container;
- user;
- cgroup;
- network endpoint group;
- directory subtree;
- filesystem/mount;
- provider-defined workload group where provenance is explicit.

A cluster MUST report the number of hidden objects.

Example:

```text
SERVICES

 nginx.service
 postgresql.service
 ssh.service
 docker.service
 + 41 more services
```

or graphically:

```text
                 COMPUTE
                    |
        +-----------+-----------+
        |           |           |
     services    processes   containers
      46 nodes    312 nodes     8 nodes
```

## 8.3 Expansion

An interactive cluster MUST be expandable without changing the underlying current place unless the user explicitly enters a child.

Expansion is a view action.

`enter` is navigation.

This distinction MUST remain consistent.

## 8.4 Intent

Spatial orientation fails when every object has equal visual weight. Semantic zoom ensures users see concepts first and details when they choose to approach them.

# 9. Discovery Without Prior Names

The spatial interface MUST provide multiple discovery paths.

## 9.1 Passive discovery

Passive discovery happens without a command:

- startup horizon;
- current-place header;
- visible exits/groups;
- landmarks;
- completion suggestions based on current neighborhood.

## 9.2 Active local discovery

Commands:

```text
look
near
map
```

These MUST work without an object name.

## 9.3 Active global discovery

Commands:

```text
find nginx
find service
find --where state == failed
```

## 9.4 Completion as spatial discovery

At:

```text
compute/services:// > enter <TAB>
```

completion MUST prioritize services visible in the current neighborhood and then offer broader matches.

At:

```text
process/nginx:// > follow <TAB>
```

completion MUST show actual available relation types, for example:

```text
service
parent
child
socket
file
cgroup
namespace
user
```

A completion entry MAY show a compact count or state:

```text
socket       2
file        23
child        4
service      nginx.service
```

### Intent

The user should learn the system by moving through it. Completion is therefore not merely token completion; it is a lightweight local map.

# 10. Stable Identity and Lifetime

Spatial navigation requires objects to feel persistent across time.

## 10.1 Identity tiers

Ono MUST distinguish at least three identity tiers:

### Tier A - Stable conceptual identity

Examples:

```text
service nginx.service
filesystem UUID=...
network interface identified by kernel identity
container stable runtime ID
host identity
user uid=1000
```

### Tier B - Lifetime identity

Examples:

```text
process start-time + pid + boot identity
socket inode + namespace + lifetime
connection tuple + creation/observation epoch
```

### Tier C - Observation identity

Used only when a provider cannot guarantee stronger identity.

The renderer MUST NOT imply stable persistence for Tier C objects.

## 10.2 Process identity

PID alone MUST NOT be treated as a persistent spatial identity.

A local Linux process identity SHOULD include:

```text
host boot identity
pid
process start time
pid namespace identity
```

Where a process is entered through a stable service context and later restarts, Ono MAY offer a continuity relation:

```text
nginx.service
  previous process/1842 [exited]
  current  process/2198 [running]
```

The process itself has changed. The service place remains stable.

## 10.3 Tombstones

Recently removed objects MAY remain as short-lived tombstones in navigation history and live maps.

Example:

```text
process/1842
state: exited 12s ago
replacement: process/2198
```

A tombstone MUST be visually distinct and MUST NOT accept actions that require a live object.

### Intent

A world where places silently vanish or PIDs silently refer to different processes is disorienting and unsafe. Identity rules preserve trust in movement and history.

# 11. Hierarchy Versus Graph Relationships

## 11.1 Canonical hierarchy

Hierarchy provides a stable path such as:

```text
SYSTEM
  -> COMPUTE
     -> SERVICES
        -> nginx.service
```

This path is for orientation.

## 11.2 Relationship graph

Real operational topology may instead be:

```text
nginx.service
    --controls--> process/1842
process/1842
    --owns--> socket/:443
process/1842
    --opens--> /etc/nginx/nginx.conf
socket/:443
    --accepts--> connection/client-A
process/1842
    --connects-to--> postgres:5432
```

## 11.3 No forced single parent

A spatial object MAY have one canonical parent for `up` while participating in many relationships.

The canonical parent MUST be deterministic for a given view profile.

The canonical parent does not claim that other relationships are less real.

## 11.4 Relationship explainability

Every displayed relationship MUST support inspection:

```text
inspect relation @edge-17
```

or equivalent structured selection.

The result MUST include:

```text
relation
source
target
direction
provider
provenance
confidence
observed_at
raw evidence/reference where safe
```

## 11.5 Confidence

Relations MUST use a confidence model compatible with v0.2 provenance.

Recommended values:

```text
exact
strong
inferred
user_declared
unknown
```

Maps SHOULD visually distinguish `inferred` edges from `exact` edges.

# 12. Process Spaces

A process place MUST make operationally meaningful relationships discoverable.

Minimum `look` groups:

```text
identity
state
parent
children
service
user
cgroup
namespaces
files
sockets/connections
container if known
recent changes
```

Example:

```text
PROCESS / nginx / 1842

 state       running
 user        www-data
 service     nginx.service
 cpu         2.1%
 memory      83 MiB
 uptime      17d 04h

 exits
   parent        systemd/1
   children      4
   sockets       2
   files         23
   namespaces    3
   cgroup        system.slice/nginx.service
   service       nginx.service

 landmarks
   socket :443   public listener
```

`enter children` MAY enter a collection place rather than choosing a child automatically.

`follow service` MUST traverse directly to the service if unique.

`follow socket :443` MUST traverse to the matching socket.

### Intent

A process should feel like a location with observable exits, not a row in a process table.

# 13. Service Spaces

A service place represents the stable service-manager concept rather than a single process lifetime.

Minimum groups:

```text
state
unit/provider identity
processes
listeners
logs/config where provider knows them
dependencies
dependents
restart history/recent state changes
resource/cgroup
```

Example:

```text
SERVICE / nginx.service

 state        running
 since        2026-08-10 06:12
 processes    5
 listeners    :80, :443

 exits
   processes      5
   sockets        2
   dependencies   network-online.target
   dependents     web-stack.target
   cgroup         /system.slice/nginx.service

 changed
   restarted      18s ago
```

A service provider MUST NOT claim config/log relationships unless it can justify them.

# 14. Network Spaces

Networking is naturally graph-oriented and is a primary showcase for spatial navigation.

## 14.1 Canonical network hierarchy

```text
NETWORK
  -> INTERFACES
  -> ROUTES
  -> NEIGHBORS
  -> LISTENERS
  -> CONNECTIONS
  -> NAMESPACES
```

## 14.2 Interface place

Minimum groups:

```text
addresses
routes
neighbors
traffic
namespace
link state
related listeners/connections when known
```

## 14.3 Listener/socket place

Minimum groups:

```text
protocol
local endpoint
owner process/service
interface/namespace
accepted/current connections
security/public exposure landmark
```

## 14.4 Connection place

Minimum groups:

```text
local endpoint
remote endpoint
owner
namespace
route/interface where derivable
remote host link where Ono can resolve a known linked host
metrics/state where available
```

## 14.5 Cross-host following

If a connection's remote endpoint is confidently mapped to a host available through an Ono remote link, the map MAY expose:

```text
web01/socket/:443
    --connected-to--> app01/socket/51722
```

Following this edge MUST visibly cross a remote boundary and MUST obey remote-link permissions.

It MUST NOT infer remote identity from IP coincidence alone when ambiguity exists.

### Intent

Network topology is one of the places where "space" can emerge most naturally. The implementation must preserve that power without pretending that every IP address is a known place.

# 15. Storage and Filesystem Spaces

## 15.1 Filesystem hierarchy remains Unix

Ono MUST preserve canonical Unix filesystem paths and directory semantics.

The filesystem is already a spatial tree:

```text
/
+-- etc
+-- home
+-- var
+-- data
```

Ono should integrate that tree into the wider system topology rather than replacing it.

## 15.2 Storage hierarchy

```text
STORAGE
  -> FILESYSTEMS
  -> MOUNTS
  -> VOLUMES/DEVICES when known
  -> DIRECTORY ROOTS
```

## 15.3 Mount boundaries

Crossing a mount boundary MUST be discoverable.

Example:

```text
storage:// > enter /mnt/backup

boundary
  local path     /mnt/backup
  filesystem     nfs
  source         nas01:/exports/backup
  remote         yes

storage:/mnt/backup:// >
```

If `nas01` is also an Ono-linked host and the remote export can be resolved safely, Ono MAY expose a cross-host storage relation.

## 15.4 Directory place

A directory place MUST support normal path navigation and MAY also expose semantic neighbors:

```text
children
mount boundary
open-by processes
owned-by users
changed recently
filesystem
```

The spatial renderer MUST NOT enumerate huge directories by default. It SHOULD cluster or summarize when entry counts exceed the view budget.

## 15.5 File place

A file place MAY expose:

```text
path
filesystem
metadata
owner
open-by processes
referenced-by service/config provider
recent changes
```

File content is not automatically part of the spatial horizon.

### Intent

The storage model intentionally avoids the "filing cabinet" metaphor. Unix paths already provide a better native spatial structure. Ono adds relationships from files back into processes, services, mounts and users.

# 16. Container, Namespace and Cgroup Spaces

## 16.1 Containers

A container place SHOULD expose:

```text
runtime/provider
state
image
processes
namespaces
cgroups
mounts
network endpoints
ports
related service/workload where known
```

## 16.2 Namespace boundary

Entering a namespace MUST show the boundary explicitly.

Example:

```text
host/process/nginx:// > follow namespace net

crossing scope
  host namespace       net:[4026531840]
  target namespace     net:[4026533331]

namespace/net:4026533331:// >
```

## 16.3 Cgroups

Cgroups SHOULD be spatially navigable through hierarchy where the kernel hierarchy is meaningful.

```text
cgroup:/system.slice
  -> nginx.service
  -> ssh.service
```

Cgroup hierarchy MAY be different from service hierarchy and MUST not be silently conflated.

# 17. Identity Spaces

A user place SHOULD expose:

```text
uid/gid
sessions
processes
owned files where queryable/appropriate
groups
privilege information
recent login/session changes
```

A group place SHOULD expose members and permission-relevant relationships.

Security-sensitive enumeration MUST respect platform policy and permission boundaries.

Identity spaces MUST NOT reveal secrets, credentials, environment contents or private files merely because they are related to a user.

# 18. Device Spaces

Devices are optional spatial objects unless a provider supplies stable identity and useful relationships.

Examples of useful relationships:

```text
block device --backs--> filesystem
network device --provides--> interface
pci device --hosts--> network device
gpu device --used-by--> process
usb device --backs--> tty
```

`/dev/*` path existence alone is insufficient to create a rich device place.

# 19. Remote Systems as Space

Remote links are not merely SSH subprocesses in v0.4. They are reachable system roots.

## 19.1 Link map

At local `SYSTEM`, `look` SHOULD expose available links:

```text
links
  prod/web01       connected     12ms
  prod/db01        connected     13ms
  home/nas01       disconnected  last seen 3h ago
```

## 19.2 `jump` across links

```text
jump prod/web01
```

MUST produce a new `SystemPlace` for the remote host.

The prompt/HUD MUST clearly indicate the remote scope.

## 19.3 Federated map

A map MAY show multiple linked hosts when explicitly requested:

```text
map links
```

Example:

```text
LOCAL/workstation
       |
       +------ prod/web01 ----- prod/db01
       |
       +------ home/nas01
```

The default root map SHOULD NOT automatically expand all remote graphs.

## 19.4 Cross-host relationships

Cross-host edges MUST be based on explicit remote evidence or strong multi-sided correlation.

One-sided observations MAY be displayed but MUST carry the correct confidence.

### Intent

Remote hosts are where Ono's spatial model most clearly differs from ordinary shell nesting. The user should perceive a change of place inside one systems interface, not "a shell inside another shell".

# 20. Navigation Trail, History and Orientation

## 20.1 Spatial trail

The spatial trail is separate from command history.

Schema:

```text
NavigationStep {
    timestamp: Timestamp
    from: SpatialId
    to: SpatialId
    movement: enter | follow | jump | back | up | home
    relation: RelationType?
    scope_crossing: ScopeBoundary?
}
```

## 20.2 Breadcrumbs

Interactive rendering SHOULD expose a compact breadcrumb when depth is greater than one.

Example:

```text
web01 > compute > services > nginx.service > process/1842
```

The prompt itself SHOULD remain concise; full breadcrumbs MAY occupy a status line or be shown by `trail`.

## 20.3 Dead destinations

If `back` points to an object that no longer exists, Ono MUST:

1. resolve a tombstone if available;
2. otherwise skip to the nearest valid previous place only after informing the user;
3. retain the original trail record.

## 20.4 Bookmarks and pins

`pin` marks a place as a persistent user landmark.

Examples:

```text
pin
pin --name edge-proxy
jump @edge-proxy
```

Pins MUST store a resilient selector and identity metadata rather than only a rendered path.

If the target cannot be resolved later, the pin remains but reports unresolved state.

# 21. Prompt and HUD Semantics

The prompt communicates current execution scope and current spatial place. It MUST remain useful even when rich rendering is disabled.

## 21.1 Canonical prompt form

Recommended canonical forms:

```text
local:// >
local/compute:// >
local/process/nginx:// >
prod/web01:// >
prod/web01/service/nginx:// >
```

The exact separator characters MAY vary by theme, but the semantic components MUST be available to the renderer:

```text
link/host
canonical place path or concise place identity
privilege state
context warnings
```

## 21.2 Prompt compression

Deep graph traversal can create excessively long paths. Ono MUST NOT blindly render the entire navigation trail in the prompt.

Instead, the prompt SHOULD show:

```text
<host>/<current-place-kind>/<display-name>
```

while `trail` and the optional HUD show full movement history.

## 21.3 Security boundary markers

Privilege, remote and namespace changes MUST be visually recognizable even in minimal colorless terminals.

Examples:

```text
prod/web01 [root] /service/nginx:// >
container:api /process/441:// >
ns:net/4026533331 /socket/443:// >
```

Color MAY reinforce these states but MUST NOT be the sole indicator.

## 21.4 HUD

An optional one-line HUD MAY display:

- current host/link;
- current place;
- parent place;
- count of nearby objects;
- landmark count;
- live/watch state;
- privilege/scope boundary;
- pending background jobs.

The HUD MUST not consume excessive vertical space or become a dashboard that competes with command output.

### Intent

The prompt should create a sense of place through truthful context, not through decorative labels. A user returning from a long command must immediately know where future relative spatial commands will operate.

# 22. Map Data Contract

`map --json` MUST return a renderer-independent `SpatialMap`.

Recommended contract:

```text
SpatialMap {
    map_id: Uuid
    center: SpatialId
    scope: SpatialScope
    zoom_level: Int
    nodes: List<MapNode>
    edges: List<MapEdge>
    clusters: List<MapCluster>
    landmarks: List<Landmark>
    hidden: HiddenSummary
    generated_at: Timestamp
    completeness: Completeness
    live_capable: Bool
}

MapNode {
    id: SpatialId
    object_ref: ObjectRef
    label: String
    type: TypeId
    state: StateSummary?
    canonical_parent: SpatialId?
    landmark_reasons: List<LandmarkReason>
}

MapEdge {
    id: EdgeId
    source: SpatialId
    target: SpatialId
    relation: RelationType
    confidence: Confidence
    direction: Direction
    changed: ChangeState?
}

MapCluster {
    id: ClusterId
    label: String
    members: Int
    grouping: String
    expandable: Bool
}
```

Screen coordinates MUST NOT appear in the semantic `SpatialMap` contract. Layout coordinates belong to the renderer.

# 23. Map Rendering

## 23.1 Renderer goals

The map renderer exists to make topology legible and navigable.

It is not required to draw every edge or node.

Priority order:

1. current place;
2. canonical exits;
3. landmarks;
4. strongest operational relationships;
5. user-selected relation filters;
6. lower-priority context if space remains.

## 23.2 Default textual map

Every terminal MUST have a non-fullscreen textual map representation.

Example:

```text
                         [ nginx.service ]
                                |
                            controls
                                v
                          [ nginx/1842 ]
                           /     |      \
                      owns    opens    connects
                       /         |          \
                  [:443]   [nginx.conf]  [postgres:5432]
```

The exact ASCII/Unicode line characters are presentation details. ASCII fallback MUST exist.

## 23.3 Interactive full-screen map

On an interactive TTY, `map` MAY open a full-screen navigable view when terminal capability and user configuration permit.

Required controls:

```text
Arrow keys / hjkl   move focus among visible nodes
Tab                 next logical node
Shift-Tab           previous logical node
Enter               enter focused node
f                   follow selected relation when unambiguous
b / Backspace       back
u                   up
h or Home command   home (key binding may differ from vi-h)
/                   search visible/global map
z / + / -           semantic zoom controls
r                   refresh
w                   toggle live map
i                   inspect focused object/relation
p                   pin/unpin
Esc                 close map view, preserving current place
?                   view help
```

Key bindings MUST be configurable. Semantic actions are normative; exact single-key choices MAY be remapped.

## 23.4 Focus versus current place

Moving focus inside a map MUST NOT change the shell's current place.

Only `Enter` or explicit navigation action changes place.

This distinction prevents accidental semantic movement while browsing.

## 23.5 Edge rendering

Edges SHOULD expose direction and relation when ambiguity would otherwise arise.

Common relations MAY use compact labels or legends.

Inferred edges MUST be visually distinguishable from exact edges.

## 23.6 Large maps

A renderer MUST NOT attempt to draw an unreadable all-node graph.

When the requested set cannot fit:

- cluster;
- rank;
- paginate/scroll spatially;
- increase semantic aggregation;
- or require a narrower filter.

It MUST NOT silently drop significant landmarks without indicating hidden counts.

### Intent

The map is not an illustration of Ono. It is a manipulable projection of the same object graph used by commands. The user's confidence depends on knowing that every visible node and edge corresponds to inspectable data.

# 24. `look` Rendering Rules

`look` is the primary low-friction spatial command.

## 24.1 Information budget

Default `look` SHOULD fit in approximately one terminal screen at common heights when possible.

It MUST prioritize:

1. identity and state;
2. direct exits;
3. landmarks;
4. recent relevant changes;
5. summary counts.

It MUST NOT default to dumping all properties of the underlying object.

`inspect` remains the exhaustive property view.

## 24.2 Groups as exits

When `look` displays:

```text
children    14
sockets      3
files       82
```

those group labels MUST be valid navigation or query targets where practical:

```text
enter children
enter sockets
get file
```

If a displayed group is not navigable, the renderer MUST not visually imply that it is an exit.

## 24.3 Change section

The `changed` group SHOULD show only changes relevant to the current place and recent configurable time horizon.

No fake change summary may be generated when no event source or comparison snapshot exists.

# 25. Live Spatial State

A space should feel alive only when the system is actually changing.

## 25.1 Live map

`map --live` MUST subscribe to available provider events and/or explicit polling sources.

It may visualize:

- node appearance/removal;
- state transitions;
- edge appearance/removal;
- landmark appearance/removal;
- metric changes when relevant to landmark status;
- replacement of lifetime objects such as restarted processes.

## 25.2 Animation policy

Animation MAY smooth actual transitions, but MUST NOT add artificial delay or activity.

Forbidden examples:

```text
fake scan progress
random pulsing nodes
artificial "connection established" delays
glitch effects unrelated to state
continuous motion simply to appear cyberpunk
```

Allowed examples:

```text
new edge fades in because a connection appeared
failed service changes state immediately
a process tombstone remains briefly before cluster collapse
traffic intensity changes based on real measurement
```

## 25.3 Event freshness

Live views MUST expose whether updates are:

```text
event-driven
polled
cached
stale
partial
```

A stale provider MUST not continue visually animating old data as if current.

## 25.4 Snapshot comparison

Where event streams are unavailable, Ono MAY build live changes by comparing successive snapshots.

The provenance must identify that the change was inferred from snapshots.

### Intent

"Alive" should mean responsive to the machine, not busy on the screen. This is central to earned coolness.

# 26. Landmark Engine

## 26.1 Purpose

Landmarks solve two problems:

- users need orientation anchors;
- huge systems require relevance ranking.

## 26.2 Built-in landmark rules

The core SHOULD provide conservative rules for:

### Compute

```text
high CPU relative to configurable threshold
high memory relative to host/cgroup budget
failed service
restart loop
unexpected exit/recent start
privileged process when context makes it relevant
```

### Network

```text
public listener
interface down
route change
connection spike
unusually high traffic
new remote peer
```

### Storage

```text
filesystem pressure
read-only transition
mount failure
new/remounted filesystem
I/O pressure where provider exists
```

### Security/scope

```text
root context
cross-host boundary
namespace boundary
permission-limited visibility
```

## 26.3 Thresholds

Thresholds MUST be inspectable and configurable.

Default thresholds SHOULD be conservative to avoid turning every busy system into an alert board.

Landmarks are not an observability alerting subsystem. Ono MUST avoid pretending that a local heuristic is an incident.

## 26.4 User pins

User-pinned objects are always landmarks within relevant maps unless filtered explicitly.

## 26.5 Plugin landmarks

KUANG/11 plugins MAY contribute landmark reasons under capability control.

Plugin-contributed landmarks MUST identify their source.

# 27. Spatial Search and Resolution

## 27.1 Selector resolution order

Relative spatial selectors SHOULD resolve in this order:

1. exact visible child/group;
2. exact visible neighbor;
3. exact canonical identifier in current scope;
4. fuzzy visible match;
5. current-host spatial index;
6. linked-host index only when explicitly requested or configured.

This prioritizes local orientation over surprising global jumps.

## 27.2 Ambiguity

Interactive ambiguity opens a picker.

The picker MUST show disambiguating context:

```text
nginx.service             service   local/compute/services
nginx/1842                process   local/compute/processes
/etc/nginx                directory local/storage/fs-root/etc
container/nginx-proxy     container local/containers
```

Non-interactive ambiguity returns a structured `spatial.ambiguous_selector` error.

## 27.3 Fuzzy matching

Fuzzy matching MUST NOT execute destructive operations automatically.

A fuzzy selector may be used for navigation after interactive confirmation/picking.

Actions on system objects must follow existing v0.2 action safety rules.

## 27.4 Index freshness

Search results MUST include freshness/provenance when they may come from cached indexes.

# 28. Spatial Selection and Typed Pipelines

Spatial UI and object pipelines MUST interoperate.

## 28.1 Map selection to pipeline

An interactive map SHOULD allow selection of one or more nodes.

Selected objects MUST be exposable as typed values, for example through the existing selection/reference mechanism:

```text
@selection | select name state
```

or the canonical mechanism established by the v0.2 implementation.

## 28.2 Pipeline result to space

A structured pipeline result containing spatially identifiable objects MUST be enterable.

Examples:

```text
get process | where cpu > 80
```

If one result:

```text
enter @-1
```

If multiple results, entering opens a picker or collection space.

## 28.3 Temporary collection spaces

A set of objects MAY become an ephemeral collection place:

```text
get process | where memory > 1GiB | enter
```

Conceptual result:

```text
COLLECTION / recent-result-17

  postgres/812
  java/1192
  firefox/4122
```

Ephemeral collection places MUST preserve references to the originating result and MUST disappear according to result-retention policy unless pinned.

### Intent

The spatial system must strengthen typed pipelines rather than create a parallel silo. Moving from data analysis to exploration should be one operation.

# 29. Non-Interactive and Scripting Semantics

Spatial semantics MUST remain scriptable.

## 29.1 No hidden TUI dependency

The following MUST work in non-interactive mode:

```text
look --json
map --json
near
find
trail --json
```

## 29.2 Navigation inside scripts

Native Ono scripts MAY use spatial navigation, but the current place is script-local unless explicitly operating in the interactive shell session.

A script MUST NOT silently change the caller's interactive spatial context.

## 29.3 Deterministic ambiguity

Scripts MUST never open interactive pickers. Ambiguity is an error unless the script explicitly selects first/unique or uses an exact ID.

## 29.4 Streaming

`near` and `find` return normal structured streams and can participate in object pipelines.

`map --json` returns a bounded graph value.

# 30. Filesystem `cd` and Spatial Navigation

Ono MUST maintain a clear distinction between filesystem working directory and spatial place.

## 30.1 `cd`

`cd` changes the process working directory used by external commands and filesystem-relative operations.

## 30.2 `enter`

`enter` changes the spatial place.

Entering a directory MAY optionally synchronize cwd according to configuration, but the default behavior defined by v0.4 is:

> **Entering a directory place changes both spatial place and cwd to that directory.**

This decision aligns spatial navigation with the deeply established Unix path model.

Entering non-filesystem places MUST NOT change cwd.

## 30.3 `cd` updates spatial storage context

When the user executes:

```text
cd /var/log
```

Ono SHOULD update the current spatial place to the corresponding directory place **only if the current place is within the filesystem/storage navigation family or the user has enabled `spatial.follow_cwd=always`**.

Default setting:

```text
spatial.follow_cwd = storage-only
```

This avoids surprising jumps out of a process/service investigation because an external command changed cwd semantics.

## 30.4 Environment variable `PWD`

Spatial place MUST NOT be encoded into `PWD`. `PWD` remains the filesystem working directory.

### Intent

Ono extends Unix rather than redefining basic process semantics. Filesystem space and object space meet, but they are not the same state variable.

# 31. Relationship Discovery and `trace`

`trace` remains the explicit relationship exploration command from earlier specifications.

v0.4 defines its relationship to spatial navigation.

## 31.1 `trace` creates a graph projection

```text
trace nginx
```

returns or renders a graph centered on the resolved object.

It MUST NOT automatically change current place.

## 31.2 Entering a trace result

In an interactive trace view, `Enter` on a node performs normal spatial navigation.

## 31.3 `map` versus `trace`

`map` answers:

> What is the useful spatial neighborhood around this place?

`trace` answers:

> What relationships of specified types/depth connect this object to others?

`map` is relevance-ranked and orientation-driven.

`trace` is relationship-query-driven.

The underlying graph contracts SHOULD be shared.

# 32. Relationship Provider Requirements

A provider contributing spatial relationships MUST declare:

```text
relation types
source/target schemas
directionality
confidence semantics
freshness model
required privileges
cost class
event support
```

## 32.1 Cost classes

Recommended cost classes:

```text
cheap       already available / O(1) or small local lookup
normal      bounded system query
expensive   broad scan or cross-provider correlation
privileged  requires elevated permission
remote      requires remote operation
```

Default `look` and `map` MUST avoid expensive relationships unless cached or already available.

## 32.2 Lazy expansion

Expensive relationship groups SHOULD appear as discoverable but unloaded exits:

```text
files        23 known
reverse refs available on request
```

The user can then `enter` or `near` that relation explicitly.

# 33. Spatial Cache and Index

A spatial interface requires enough indexing for fast discovery without inventing stale truth.

## 33.1 Spatial index

Ono SHOULD maintain an in-memory spatial index containing:

```text
SpatialId
canonical object reference
display names and aliases
scope
object type
canonical parent
known relationship summary
freshness
landmark state
```

## 33.2 Source of truth

The index is a cache. Providers remain authoritative.

Actions MUST resolve/revalidate live objects before mutation.

## 33.3 TTL

Different object classes require different freshness policies.

Recommended starting points:

```text
services         event-driven / 5s fallback
processes        1s active view, 5s passive
connections      1s active, 5s passive
interfaces       event-driven / 10s fallback
mounts           event-driven / 10s fallback
files/directories query-driven
users/groups     30s or NSS/provider policy
remote objects   remote-advertised freshness
```

These are implementation defaults and MAY be tuned without changing semantics.

## 33.4 Cache visibility

`inspect` MUST reveal source freshness when relevant.

# 34. Performance Budgets

Spatial features MUST not make Ono feel slower than a shell.

Normative performance goals for a typical modern Linux workstation under warm-cache conditions:

```text
interactive startup to usable prompt        < 150 ms target
basic `look` local cached                    < 50 ms target
`near` cached                                < 50 ms target
map L0/L1 cached                             < 100 ms target
map L2 ordinary host                         < 250 ms target
focus/navigation inside rendered map         < 16 ms frame target
search common indexed objects                < 100 ms target
```

Cold provider discovery MAY exceed these targets, but the shell MUST remain interactive and progressively update rather than block unnecessarily.

## 34.1 Background discovery

Expensive discovery SHOULD occur asynchronously after prompt availability.

## 34.2 View budgets

Default visible-node budget SHOULD be approximately:

```text
text map        30 nodes
interactive map 100 nodes before mandatory clustering
```

Renderer-specific budgets MAY vary, but unbounded graph rendering is prohibited.

# 35. Security and Permission Boundaries

Spatial discoverability must not become unauthorized enumeration.

## 35.1 Principle

> **If the current user could not legitimately query the underlying information through the provider, the spatial layer must not reveal it.**

## 35.2 Permission states

A neighborhood group may be:

```text
available
empty
unknown
permission_denied
unsupported
stale
```

These states MUST remain distinct.

Example:

```text
files       permission denied for 14 process FDs
```

is preferable to:

```text
files       0
```

## 35.3 Privilege escalation

Spatial navigation itself SHOULD NOT trigger privilege escalation.

An action or explicit privileged inspection may request escalation using the existing Ono security model.

## 35.4 Remote boundaries

Remote traversal MUST honor link capabilities and authentication. `jump` MUST NOT silently establish arbitrary new network connections merely because a hostname resembles a known place.

## 35.5 Plugin contributions

KUANG/11 plugins cannot use the map as a side channel to expose information outside granted capabilities.

The spatial host MUST filter plugin nodes/edges according to capability scope before merging them into maps.

# 36. KUANG/11 Spatial Extensions

KUANG/11 MAY extend the spatial world, but Ono core retains control of identity, security and rendering contracts.

## 36.1 Contribution types

Plugins MAY contribute:

```text
object schemas that implement SpatialObject
relationship providers
landmark detectors
map overlays
custom inspect panels
search aliases
collection spaces
navigation actions
```

## 36.2 Forbidden plugin behavior

A plugin MUST NOT:

- create uninspectable phantom edges;
- replace core canonical domains;
- silently change core object identity;
- capture global navigation keys outside its active view;
- expose data outside capabilities;
- make AI-generated relationships appear exact without provenance.

## 36.3 AI assistant spatial access

An AI assistant loaded through KUANG/11 MAY receive a structured map or neighborhood through the context broker only within granted scopes.

It MAY propose navigation:

```text
"The failed service is connected to postgres. Open that relation?"
```

But model text MUST NOT directly mutate current place. Navigation happens through typed host actions.

## 36.4 Plugin-defined spaces

A plugin-defined aggregate space MUST declare:

```text
id
label
parent domain
membership query
supported relations
cost/freshness
permissions
```

Example:

```text
Kubernetes plugin:
COMPUTE -> WORKLOADS -> namespace -> deployment -> pod -> process
```

The plugin MAY provide this richer hierarchy without replacing core host-level topology.

### Intent

KUANG/11 can make Ono's world much larger, including cloud, observability and application topology. The host must still guarantee that every extension behaves like part of one navigable system rather than a collection of unrelated mini-TUIs.

# 37. Integration with v0.3 External Command Adapters

Adapted external tools may contribute typed objects to the spatial model when their output maps to canonical spatial schemas.

Examples:

```text
ss -tunap       -> Stream<Socket>      -> spatial sockets
ps ...          -> Stream<Process>     -> spatial processes
ip address      -> InterfaceAddress    -> network space
lsblk           -> BlockDevice         -> storage/device space
systemctl       -> Service             -> compute/services
```

## 37.1 Identity merge

Objects from adapters MUST be reconciled with canonical provider identities before appearing as duplicate map nodes.

If identity cannot be safely reconciled, both objects may appear with provenance and an unresolved-equivalence relation.

## 37.2 No raw-text spatial inference

Raw external command output MUST NOT become spatial nodes through generic table heuristics.

Only canonical typed adapter output or explicit plugin schemas may enter the spatial index.

# 38. Help and Discoverability

Spatial commands MUST teach themselves.

## 38.1 `help spatial`

Ono MUST provide a concise overview explaining:

```text
look     see where you are and what is nearby
map      see topology
enter    move into a visible child/object
follow   traverse a relationship
jump     move directly to another known place
back     return along your trail
up       go to canonical parent
home     return to system root
near     query neighboring objects
find     search known places
trail    inspect where you moved
```

## 38.2 Context-sensitive help

At any place:

```text
help here
```

SHOULD show spatial operations supported by that place.

## 38.3 First-run teaching

On first interactive run only, Ono MAY show a single subtle hint:

```text
hint: `look` shows where you are; `map` shows where you can go
```

Persistent tutorial banners are prohibited by default.

# 39. Accessibility and Terminal Capability

The spatial system MUST work across terminal capabilities.

## 39.1 Color

Color MUST NOT be required to distinguish:

- current node;
- inferred edge;
- failed state;
- remote boundary;
- root privilege;
- focused item.

## 39.2 Unicode

Unicode box drawing MAY be used when supported. ASCII fallback MUST exist.

## 39.3 Small terminals

At narrow widths, maps MAY collapse into ranked tree/list projections rather than drawing graphs.

Spatial semantics remain identical.

## 39.4 Reduced motion

A `reduced_motion` setting MUST disable transition animation while preserving live updates.

# 40. Error Model

Spatial operations MUST emit structured errors.

Required error codes include:

```text
spatial.not_found
spatial.ambiguous_selector
spatial.not_enterable
spatial.no_relation
spatial.no_parent
spatial.history_empty
spatial.destination_gone
spatial.permission_denied
spatial.unsupported
spatial.stale
spatial.remote_unavailable
spatial.scope_violation
spatial.map_too_large
spatial.identity_conflict
```

Errors SHOULD include actionable next steps where deterministic.

Examples:

```text
cannot follow `socket`
process nginx has 3 matching sockets
use `near socket` or `follow socket :443`
```

and:

```text
destination no longer exists
process/1842 exited 12s ago
replacement candidate: process/2198 via nginx.service
```

# 41. Machine-Readable Spatial Registry

v0.4 requires machine-readable contracts sufficient to generate help, completion, tests and SDK bindings.

Recommended files:

```text
docs/spec/spatial.yaml
docs/spec/relations.yaml
docs/spec/spaces.yaml
docs/spec/landmarks.yaml
docs/spec/spatial-errors.yaml
```

## 41.1 `spaces.yaml`

Each canonical place/collection MUST define:

```yaml
id: compute.services
label: services
parent: compute
object_type: Service
enterable: true
commands:
  - look
  - map
  - near
  - enter
  - find
summary_fields:
  - state
  - process_count
```

## 41.2 `relations.yaml`

Each relation MUST define:

```yaml
id: process.owns_socket
source: Process
target: Socket
direction: outbound
canonical_label: socket
inverse_label: owner
confidence: exact_or_provider_declared
```

## 41.3 Generation

The registries SHOULD generate or validate:

- completion relation names;
- `help spatial` content;
- parser fixtures;
- relation compatibility checks;
- map legends;
- SDK enums;
- conformance tests;
- documentation tables.

### Intent

The spatial interface has many cross-cutting semantics. Machine contracts prevent renderer, provider, parser and documentation from drifting into different definitions of the world.

# 42. Provider Conformance for Spatial Objects

A provider that exposes objects to spatial navigation MUST pass additional conformance tests beyond ordinary schema validity.

Required provider claims:

```text
spatial identity strategy
canonical parent strategy
supported relationships
freshness strategy
event support
permission behavior
cost class
landmark-relevant metrics/states
```

## 42.1 Identity test

Repeated observations of the same live object MUST resolve to the same `SpatialId` within the provider's advertised identity tier.

## 42.2 Reuse safety test

For lifetime identities such as PIDs, the provider MUST prove that identifier reuse cannot silently resolve a tombstoned place to a different object.

## 42.3 Relation integrity

Every edge target MUST resolve to either:

- a known spatial object;
- an explicit unresolved endpoint object;
- or a remote/opaque reference with correct type.

Dangling internal IDs are invalid.

## 42.4 Permission test

Denied information must produce `permission_denied` or `unknown`, never false empty collections.

# 43. Test Strategy

Spatial behavior must be tested at data, interaction and product levels.

## 43.1 Unit tests

Required unit coverage includes:

- `SpatialId` stability rules;
- canonical parent selection;
- selector precedence;
- ambiguity detection;
- neighborhood ranking;
- clustering;
- landmark thresholds;
- trail operations;
- tombstone resolution;
- relation inverse handling;
- scope boundary detection;
- map node/edge filtering;
- permission-state preservation.

## 43.2 Property tests

Recommended properties:

```text
back(enter(x)) returns prior place when both remain valid
up never traverses arbitrary graph edges
map coordinates never affect semantic identity
filtering cannot create unknown edges
same stable provider identity -> same SpatialId
PID reuse -> different lifetime SpatialId
all rendered edges reference existing rendered nodes or explicit off-map endpoints
```

## 43.3 Integration fixtures

A deterministic Linux fixture MUST create at least:

```text
2 services
one service with multiple processes
one process opening a known file
one TCP listener
one client/server connection
one filesystem mount boundary
one namespace/container boundary where environment permits
multiple users/ownership relations
one failed/restarting service fixture
```

Tests MUST verify that the expected places and relationships can be discovered **without using their exact names as initial input** where practical.

Example acceptance path:

```text
home
enter compute
enter services
find --where state == running
select fixture service
enter
follow process
follow socket
```

## 43.4 PTY interaction tests

PTY tests MUST verify:

- `look` startup horizon;
- interactive ambiguity picker;
- map opening/closing;
- map focus without place change;
- Enter changes place;
- Backspace/back returns;
- terminal resize preserves current place and focus where possible;
- Ctrl-C exits live map without killing the shell;
- external interactive programs still work after map exit.

## 43.5 Snapshot tests

Renderer snapshots MAY verify layout at representative widths:

```text
40 columns
80 columns
120 columns
200 columns
```

Snapshots are presentation tests and MUST NOT become semantic contracts.

## 43.6 Live tests

Tests MUST create real changes and verify updates:

- start/exit process;
- open/close connection;
- service state transition;
- mount/unmount where safe;
- landmark threshold crossing.

No test may pass based only on timer animation.

## 43.7 Remote tests

Remote integration tests SHOULD use isolated containers/VMs to verify:

- remote root place;
- `jump` boundary;
- cross-host trail;
- stale/disconnected link state;
- remote relation confidence;
- no accidental local/remote identity merge.

# 44. Acceptance Scenarios

A release conforming to v0.4 MUST pass all scenarios below.

## 44.1 Cold-start discovery

**Given** a user who does not know service or process names,
**when** Ono starts,
**then** the user can discover canonical domains and at least one meaningful object using only visible hints, `look`, `map`, `near`, completion or `find`.

## 44.2 Unknown nginx scenario

A fixture contains nginx-like service `fixture-web.service`, but the test operator is not given its name.

The operator must be able to:

```text
home
enter compute
enter services
look/map
select the running web service by visible metadata
enter it
follow one of its processes
follow its listening socket
```

The test proves that spatial navigation does not require prior names.

## 44.3 Storage discovery

Without prior mount names, the user must be able to:

```text
home -> storage -> filesystems/mounts
```

identify a secondary mount, enter it, see the mount boundary/source, and traverse into the mounted directory.

## 44.4 Process-to-file-to-process path

Given a process that opens a known file:

```text
process -> follow file -> file -> near process
```

must expose the relationship in both directions where provider data supports it.

## 44.5 Network path

Given a listening service and active connection:

```text
service -> process -> socket -> connection
```

must be navigable using relationship discovery.

## 44.6 Back versus up

After:

```text
SYSTEM -> COMPUTE -> SERVICES -> service ->follow process ->follow socket
```

`back` must return to the process.

`up` from the socket must return to its canonical network hierarchy parent, demonstrating the distinction.

## 44.7 Identity replacement

A service process is entered, restarted and receives a new PID.

The old process place becomes a tombstone; the service place remains stable and shows the replacement process. `back`/history must not confuse old and new process identities.

## 44.8 Permission honesty

A non-root user investigates a process with restricted file descriptors.

Ono must show unknown/permission-denied state rather than zero files.

## 44.9 Live map

A connection is opened while `map --live` watches the relevant network/service neighborhood.

A real edge must appear without fake animation and disappear or tombstone when the connection closes.

## 44.10 Raw shell continuity

After extensive spatial navigation and full-screen map use, the user runs:

```text
vim
ssh
less
cargo test
```

Interactive process control, terminal state and cwd must remain correct.

# 45. Implementation Architecture

The implementation SHOULD introduce a dedicated spatial subsystem rather than continue concentrating behavior in the CLI evaluator.

Recommended Rust workspace additions:

```text
crates/
  ono-spatial-core/       identities, places, relations, trail
  ono-spatial-index/      discovery index, cache, reconciliation
  ono-spatial-query/      look/near/find/map planning
  ono-spatial-render/     textual/full-screen map renderers
  ono-spatial-events/     live topology aggregation
```

Exact crate names MAY differ, but equivalent responsibility boundaries are normative.

## 45.1 `ono-spatial-core`

Responsibilities:

- `SpatialId`;
- `SpatialObject` projection;
- place types;
- canonical hierarchy;
- relation contracts;
- confidence/provenance bridge;
- navigation trail;
- tombstones.

It MUST NOT depend on terminal rendering.

## 45.2 `ono-spatial-index`

Responsibilities:

- object registration/reconciliation;
- alias/search index;
- freshness state;
- canonical parent lookup;
- bounded relation summaries;
- pin resolution.

It MUST treat providers as truth and revalidate mutation targets.

## 45.3 `ono-spatial-query`

Responsibilities:

- `look` plans;
- neighborhood ranking;
- semantic zoom;
- map graph selection;
- `find` resolution;
- cluster construction;
- cost-aware lazy queries.

## 45.4 `ono-spatial-render`

Responsibilities:

- compact textual place view;
- ASCII/Unicode map layout;
- interactive full-screen map;
- focus and keyboard handling;
- reduced-motion support;
- width adaptation.

It MUST NOT invent semantic nodes/edges.

## 45.5 `ono-spatial-events`

Responsibilities:

- merge provider event streams;
- snapshot diffing where necessary;
- maintain change state;
- landmark recalculation triggers;
- live map update messages.

## 45.6 CLI integration

`ono-cli` should parse/dispatch spatial commands and own session current-place state, but SHOULD NOT implement graph selection, identity reconciliation or map layout directly.

### Intent

v0.4 deliberately introduces enough complexity that putting it all into `eval.rs` or `native.rs` would create architectural collapse. Spatial semantics deserve their own subsystem.

# 46. Spatial Session State

The interactive session MUST maintain:

```text
SpatialSessionState {
    current_place: SpatialId
    trail: NavigationTrail
    pins: PinRegistry
    view_preferences: SpatialViewPreferences
    current_focus: SpatialId?      // only while interactive view active
    live_subscription: LiveViewId?
}
```

`current_focus` is transient UI state.

`current_place` is semantic shell state.

## 46.1 Persistence

The current place MAY persist across shell restarts if configured, but the default v0.4 behavior is:

```text
start at local SYSTEM root
```

Pins MAY persist.

Trail persistence across sessions is disabled by default for privacy and stale-identity reasons.

# 47. Configuration

Required configuration keys:

```text
spatial.enabled = true
spatial.startup_horizon = true
spatial.follow_cwd = "storage-only"
spatial.map.mode = "auto"          # auto | text | fullscreen
spatial.map.live = false
spatial.map.node_budget = 100
spatial.look.change_window = "5m"
spatial.landmarks.enabled = true
spatial.reduced_motion = false
spatial.remote_search = "explicit"
spatial.trail.persist = false
```

The exact configuration file syntax follows Ono's base configuration system.

Disabling `spatial.enabled` MUST leave the typed shell and ordinary commands functional.

# 48. Concrete User Journeys

This section is normative in behavior even where presentation is illustrative.

## 48.1 First contact: discover a web service

The user starts Ono and knows nothing about the host.

```text
LOCAL / wintermute

 compute      221 processes     38 services
 network      2 interfaces      31 connections
 storage      4 filesystems
 containers   3 running
 identity     2 active users
 devices      24 visible

 notable
   /data        89% full
   web.service  public listener :443

local:// >
```

The visible landmark teaches that `web.service` exists, but even without the landmark:

```text
local:// > enter compute
```

```text
COMPUTE / wintermute

 processes   221
 services     38
 jobs          1
 containers    3

 groups
   services
   processes
   workloads
   cgroups

compute:// > enter services
```

```text
SERVICES / wintermute

 running 35    failed 1    inactive 2

 notable
   web.service       running   listener :443
   backup.service    failed

services:// > enter web.service
```

```text
SERVICE / web.service

 state        running
 processes    4
 listeners    :80, :443

 exits
   processes      4
   sockets        2
   dependencies   3
   cgroup         /system.slice/web.service

service/web:// > follow socket :443
```

```text
SOCKET / tcp :443

 state        listen
 owner        web/1842
 interface    eth0
 namespace    host
 exposure     public

 connections  14

socket/443:// > map
```

At no point did the user need to know a process PID or memorize `ss`, `lsof`, `systemctl` or `/proc` paths.

### Intent

This is the core v0.4 success case: exploration is discoverable and relationship-driven while still exposing real objects.

## 48.2 Investigate high CPU

```text
local:// > look
```

shows:

```text
notable
   rustc/4419   cpu 96%
```

The user may:

```text
enter rustc/4419
```

or use object syntax:

```text
get process | where cpu > 80
```

Both reach the same `Process` identity.

Inside:

```text
PROCESS / rustc / 4419

 parent        cargo/4381
 children      8
 files         127
 sockets       0
 cgroup        user.slice/...

 changed
   cpu +31% in 8s
```

`follow parent` reaches cargo.

`up` reaches the canonical process collection.

`back` returns to rustc.

## 48.3 Discover storage without `lsblk`

```text
home
enter storage
```

```text
STORAGE / wintermute

 filesystems  4
 mounts        7

 notable
   /data       ext4   89% full

storage:// > enter filesystems
```

```text
FILESYSTEMS

 rootfs        ext4    /
 data          ext4    /data
 backup        nfs     /mnt/backup   remote
```

The user can enter `backup`, inspect its source, then enter the mount path.

The ordinary command:

```text
lsblk
```

remains available, and v0.3 may adapt it into the same `BlockDevice` objects.

## 48.4 Cross-host connection

On `web01`:

```text
service/web:// > follow socket :443
socket/443:// > near connection
```

A backend connection appears:

```text
postgres:5432   established   known host prod/db01
```

```text
follow connection postgres:5432
```

If both sides are confidently correlated and the remote link is authorized:

```text
crossing link
  from prod/web01
  to   prod/db01
  via  tcp connection

prod/db01/connection/5432:// >
```

The user has moved through actual system topology rather than manually opening SSH based on an IP address.

## 48.5 File relationship

```text
service/web:// > follow process
process/1842:// > near file | where path contains "nginx.conf"
```

Then:

```text
follow file /etc/nginx/nginx.conf
```

The directory/file place integrates with Unix cwd if entered:

```text
file/nginx.conf:// > up
```

moves to the canonical directory parent.

`back` returns to the process because that was the actual navigation origin.

# 49. Anti-Patterns and Explicit Non-Goals

v0.4 MUST NOT devolve into any of the following.

## 49.1 Cyberpunk theater

Forbidden as core/default product behavior:

```text
"BREACHING ICE..."
random matrix rain
fake network scanning
fake progress bars
artificial typing delays
glitch transitions
meaningless neon animation
```

## 49.2 Everything-is-a-room metaphors

Do not invent physical analogies merely to make text sound spatial.

## 49.3 Unbounded force-directed graph

Dumping the entire process/socket/file graph into a force-directed renderer does not satisfy this specification.

## 49.4 Name-first navigation

A feature set where `enter nginx` works but the user has no generic way to discover nginx does not satisfy v0.4.

## 49.5 Renderer-owned truth

A relationship that only exists because the map placed two nodes near each other is invalid.

## 49.6 Hidden remote execution

Crossing to another machine must never be visually subtle or implicit.

## 49.7 Spatial shell replaces pipelines

Users must not be forced to navigate interactively for tasks better expressed as:

```text
get process | where cpu > 20 | stop process
```

## 49.8 Permanent full-screen shell

Ono remains a shell, not a terminal dashboard. Full-screen spatial views are entered deliberately and exited cleanly.

# 50. Implementation Sequence

This sequence is dependency-driven and targets the complete v0.4 feature set rather than an MVP.

## Phase S1 - Spatial core contracts

Deliver:

- `SpatialId`;
- spatial object projection;
- canonical place/domain definitions;
- relation registry;
- hierarchy model;
- navigation trail;
- structured spatial errors;
- machine-readable registries.

Gate:

```text
spatial core can represent local SYSTEM -> domain -> collection -> entity
without rendering or provider-specific hacks
```

## Phase S2 - Provider identity and relation bridge

Deliver:

- spatial identity for Process, Service, Socket, File, Filesystem, Mount, Interface, User and Container where available;
- canonical parents;
- core exact relations;
- permission-state propagation;
- conformance tests.

Gate:

```text
provider objects can be reconciled into one graph without duplicate identity for known-equal objects
```

## Phase S3 - Spatial index and discovery

Deliver:

- index;
- aliases;
- selector resolution;
- `find`;
- local neighborhood query;
- pins;
- freshness tracking.

Gate:

```text
objects can be discovered without prior exact names
```

## Phase S4 - Navigation commands

Deliver:

```text
look
near
enter
follow
jump
back
up
home
trail
pin/unpin
```

Gate:

```text
full navigation scenarios work in non-interactive structured/text mode
```

## Phase S5 - Semantic maps

Deliver:

- `SpatialMap`;
- ranking;
- clustering;
- semantic zoom;
- textual map renderer;
- relation inspection.

Gate:

```text
maps remain legible on fixture systems with hundreds of processes and connections
```

## Phase S6 - Interactive map

Deliver:

- full-screen view;
- focus;
- keyboard navigation;
- Enter/back/up/home;
- inspect/search;
- terminal resize;
- reduced motion;
- PTY tests.

## Phase S7 - Live topology

Deliver:

- event aggregator;
- snapshot diff fallback;
- live map;
- tombstones;
- landmark updates;
- freshness display.

## Phase S8 - Remote spatial federation

Deliver:

- remote root places;
- cross-link jumps;
- remote identity namespace;
- federated maps;
- cross-host edge confidence;
- remote tests.

## Phase S9 - KUANG/11 spatial SDK

Deliver:

- plugin spatial schemas;
- relation provider API;
- landmark API;
- overlay/view contributions;
- capability filtering;
- deterministic plugin test host.

## Phase S10 - v0.3 adapter reconciliation

Deliver:

- adapter object identity merge;
- adapted objects in spatial index;
- provenance preservation;
- duplicate-resolution tests.

## Phase S11 - Release hardening

Deliver:

- full acceptance suite;
- performance budgets;
- security review;
- terminal compatibility matrix;
- stress tests;
- fuzzing;
- dogfood fixes;
- generated docs/help completion consistency.

# 51. Spec-Driven Work Packages

A code-generating agent SHOULD be able to derive work packages similar to:

```text
SPC-001  SpatialId representation
SPC-002  SpatialObject trait/projection
SPC-003  SpatialScope model
SPC-004  canonical domain registry
SPC-005  canonical parent resolver
SPC-006  relation registry loader
SPC-007  navigation trail
SPC-008  tombstone model

IDX-001  spatial object index
IDX-002  alias/name index
IDX-003  freshness metadata
IDX-004  identity reconciliation
IDX-005  pin persistence

QRY-001  neighborhood ranking
QRY-002  semantic cluster planner
QRY-003  map projection
QRY-004  selector precedence
QRY-005  fuzzy picker model
QRY-006  find query execution

CMD-001  look command
CMD-002  near command
CMD-003  enter command
CMD-004  follow command
CMD-005  jump command
CMD-006  back command
CMD-007  up command
CMD-008  home command
CMD-009  trail command
CMD-010  pin/unpin
CMD-011  map command

SPP-001  Process spatial identity
SPP-002  Process parent/child relations
SPP-003  Process socket relation
SPP-004  Process file relation
SPS-001  Service spatial identity
SPS-002  Service process relation
SPS-003  Service dependency relation
SPN-001  Network interface space
SPN-002  Socket identity
SPN-003  Connection relations
SPF-001  Filesystem/mount space
SPF-002  Directory/file spatial projection
SPCtn-001 Container space
SPI-001  Identity/user space
SPD-001  Device spatial projection

RND-001  PlaceView text renderer
RND-002  text map layout
RND-003  map clustering rendering
RND-004  full-screen spatial view
RND-005  keyboard focus/navigation
RND-006  resize handling
RND-007  reduced-motion mode

EVT-001  spatial event envelope
EVT-002  provider event merge
EVT-003  snapshot diff
EVT-004  tombstone lifecycle
EVT-005  landmark recalculation
EVT-006  live map subscription

REM-001  remote SpatialId namespace
REM-002  remote root place
REM-003  jump across link
REM-004  federated map
REM-005  cross-host relation evidence

K11S-001 spatial object contribution API
K11S-002 relationship contribution API
K11S-003 landmark contribution API
K11S-004 capability-filtered map merge
K11S-005 deterministic spatial plugin testkit

ADP-S01 adapter object reconciliation
ADP-S02 canonical schema identity merge

TEST-S01 unknown-name discovery fixture
TEST-S02 back-vs-up acceptance
TEST-S03 PID reuse identity
TEST-S04 permission honesty
TEST-S05 live connection edge
TEST-S06 remote scope crossing
TEST-S07 PTY map interaction
PERF-S01 startup horizon benchmark
PERF-S02 map planning benchmark
SEC-S01 spatial enumeration review
```

Each work package SHOULD reference exact registry IDs and normative sections.

# 52. Release Criteria

A v0.4 implementation is complete only when all of the following are true.

## 52.1 Functional

- root `SYSTEM` and canonical domains exist;
- users can discover objects without prior names;
- all core spatial commands are implemented;
- hierarchy and graph traversal are distinct;
- typed pipeline and spatial selection interoperate;
- storage paths integrate with cwd according to this spec;
- remote host roots can be entered/jumped when links exist;
- map text rendering works without full-screen TUI;
- full-screen map works on supported interactive terminals;
- live map reflects real changes;
- tombstones and lifetime identity prevent PID/object reuse confusion;
- permissions remain honest;
- v0.3 adapted canonical objects can participate where available;
- KUANG/11 can extend spatial relationships under capabilities.

## 52.2 Quality

- all spatial registries validate;
- unit/property/integration/PTY tests pass;
- acceptance scenarios pass;
- no release-blocking known defects remain;
- performance targets are measured and major violations are resolved/documented;
- security review completed;
- renderer works with color disabled and ASCII fallback;
- terminal state survives entering/exiting full-screen views;
- provider conformance proves identity and permission semantics.

## 52.3 Product experience

The release MUST satisfy this qualitative acceptance statement through concrete test scenarios and dogfooding:

> A technically experienced user can start on an unfamiliar Linux host, understand the major areas of the system, discover significant services/processes/filesystems/network endpoints, move through real relationships, return along the path taken, and observe live change without needing to know the object names or the traditional Unix inspection commands in advance.

This is not optional polish. It is the reason v0.4 exists.

# 53. Resolved Design Decisions

This section exists explicitly so implementation agents do not reopen settled product questions.

| Topic | Decision |
|---|---|
| What is "space"? | Real topology formed from identity, hierarchy, relationships, neighborhood, trail and live state. |
| Physical metaphors? | Not part of the semantic model. |
| Root geography? | Six canonical domains: Compute, Network, Storage, Containers, Identity, Devices. |
| Must users know names first? | No. Discovery-before-naming is mandatory. |
| Hierarchy or graph? | Both; hierarchy for orientation, graph for real relationships. |
| `back` vs `up`? | `back` follows history; `up` follows canonical hierarchy. |
| `enter` vs `follow`? | `enter` navigates hierarchy/selected objects; `follow` traverses relationship edges. |
| `jump`? | Direct global/cross-scope move to a resolved known place. |
| Map default? | Bounded, relevance-ranked, semantically clustered. Never entire graph by default. |
| Full-screen required? | Supported for interactive use; essential semantics also available as text/structured output. |
| Does focus move the shell? | No. Only explicit navigation changes current place. |
| Stable PIDs? | No. PID alone is insufficient identity. |
| Restarted service process? | Old process tombstones; stable service remains; new process has new identity. |
| Live animation? | Only real state changes; no decorative motion. |
| Filesystem metaphor? | Preserve Unix path tree; integrate relations rather than replace it. |
| `cd` vs spatial place? | Separate state. Entering a directory changes cwd; entering other object types does not. |
| Does `cd` always change spatial place? | No. Default `storage-only` synchronization. |
| Remote host handling? | Remote roots are explicit spaces; boundary crossing always visible. |
| Unknown/denied data? | Distinct from empty. |
| Plugins? | May extend objects/relations/landmarks under KUANG/11 capabilities; cannot create untraceable truth. |
| AI relationships? | Must carry source/provenance/confidence; never silently exact. |
| External adapters? | Typed canonical adapter objects may enter spatial index after identity reconciliation. Raw text never does. |
| Spatial subsystem location? | Dedicated subsystem/crates; not implemented as CLI renderer hacks. |
| Startup experience? | Compact spatial horizon enabled by default interactively. |
| Default trail persistence? | Session-only; pins may persist. |
| Open product questions in v0.4? | None. ADRs may resolve implementation details but not change these semantics silently. |

# 54. Design Review Checklist

Before accepting any v0.4 feature, reviewers MUST ask:

- Can the user discover it without knowing an exact name?
- Does it represent a real object or relationship?
- Can Ono explain provenance/confidence?
- Is it hierarchy or relationship, and is that distinction correct?
- Does navigation change semantic place only when intended?
- Can the user get back?
- Is `up` deterministic?
- Does the view remain useful with many objects?
- Does semantic zoom aggregate meaningfully?
- Are hidden objects/counts disclosed?
- Are permission-denied and unknown distinct from empty?
- Is object identity safe across reuse/restart?
- Does text/non-TTY behavior remain valid?
- Does ordinary Unix execution still work?
- Is live motion driven by real state?
- Are remote/scope crossings obvious?
- Are plugins capability-filtered?
- Can structured pipelines consume selected objects?
- Can pipeline results become spatial places?
- Is the feature implemented outside CLI glue when it belongs to spatial core?
- Does it strengthen the sense of orientation rather than merely add graphical novelty?

If the last answer is no, the feature is probably not part of the Spatial Systems Interface.

# 55. Closing Product Contract

The v0.4 product intent can be summarized without metaphor:

> **The machine is a graph of real objects. Ono gives that graph stable places, discoverable neighborhoods, reversible movement and live state.**

The user should never have to begin an investigation by already knowing `nginx`, PID 1842, `/dev/nvme0n1p2`, `eth0`, a mount name or a remote host's process list.

Ono must expose the immediate shape of the system first.

From there the user can move:

```text
SYSTEM
  -> COMPUTE
     -> SERVICES
        -> web.service
           --controls--> process/1842
              --owns--> socket/:443
                 --connected-to--> remote peer
```

or:

```text
SYSTEM
  -> STORAGE
     -> FILESYSTEMS
        -> backup mount
           -> /mnt/backup
```

or use the same objects directly as data:

```text
get process | where cpu > 80
get socket | where state == established
```

These are not competing interaction models. They are two projections of the same system truth.

That is the defining requirement of Ono-Sendai v0.4:

> **Do not decorate the shell as cyberspace. Make the system itself navigable enough that the user experiences space.**

