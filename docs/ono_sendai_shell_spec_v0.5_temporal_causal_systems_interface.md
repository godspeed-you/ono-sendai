---
title: "ONO-SENDAI"
subtitle: "Specification v0.5 - Temporal & Causal Systems Interface"
author: "Project Specification"
date: "2026-08-31"
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

This document is the **standalone ONO-SENDAI v0.5 Temporal & Causal Systems Interface specification**.

It is a new product and architecture increment. It does not rewrite, amend in place, regenerate, or otherwise modify the published ONO-SENDAI v0.2 base specification, the standalone v0.3 External Command Adaptation Layer specification, or the standalone v0.4 Spatial Systems Interface specification.

The relationship is:

```text
ONO-SENDAI v0.2
    base shell, typed values, object pipelines, providers,
    remote links, KUANG/11 and TUI foundations

        +

ONO-SENDAI v0.3
    external command adaptation,
    compatibility packs and structured Unix interoperability

        +

ONO-SENDAI v0.4
    system space, stable spatial identity,
    discovery, navigation, maps and live topology

        +

ONO-SENDAI v0.5
    time as a first-class coordinate,
    evidence-backed history, state reconstruction,
    changes, timelines and causal explanations

        =

candidate ONO-SENDAI v0.5 product contract
```

## 0.1 Normative scope

The keywords **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, **MAY** and **RECOMMENDED** are normative within this document.

This specification defines:

- the temporal coordinate of an Ono session;
- the difference between current state, observed past state and unknown past state;
- a canonical event model;
- an evidence ledger and provenance rules;
- state reconstruction from events, snapshots and provider-owned history;
- temporal coverage and explicit gaps;
- local recording and retention;
- timeline exploration;
- historical spatial navigation;
- changes between two points in time;
- causal explanations and the strict separation of causation, correlation and temporal ordering;
- causality for actions performed through Ono itself;
- remote and distributed time semantics;
- integration with v0.2 pipelines, v0.3 adapters, v0.4 spaces and KUANG/11;
- security, privacy, storage, performance and clock requirements;
- machine-readable contracts;
- a complete implementation sequence and acceptance definition.

This document deliberately leaves **no product-design questions open**. Implementation-specific details MAY be recorded in ADRs only when they preserve the semantics defined here. If implementation reality makes a normative requirement impossible, the implementation MUST record the deviation rather than silently reinterpret this document.

## 0.2 Current implementation baseline

v0.5 assumes the implementation state reached after the v0.4 tranche:

- typed values and schemas are stable concepts;
- providers expose facts without text scraping where native sources exist;
- external command adapters may expose structured output while retaining raw execution;
- stable spatial identity exists;
- system objects can be reconciled into places;
- `look`, `near`, `find place`, `enter`, `follow`, `jump`, `back`, `up`, `home`, `trail` and `map` exist;
- `map --live` can represent actual changes through the existing live/watch path;
- tombstones distinguish known-gone objects from objects that were never known;
- `ono-spatial-events` can merge live changes and snapshot diffs without owning providers, terminals or clocks;
- remote links and KUANG/11 spatial contributions exist as controlled extension points.

v0.5 MUST extend those foundations rather than duplicate them.

## 0.3 Intent

v0.4 gave Ono-Sendai a spatial dimension. The machine became something a user could enter, inspect, traverse and map.

But a map is not yet a world.

A real system is continually changing. Processes appear and disappear. Services restart. Filesystems mount and unmount. Connections form and vanish. Configuration changes precede actions. Commands cause observable effects. Remote systems move through their own state while the operator is elsewhere.

The central purpose of v0.5 is therefore:

> **Ono-Sendai must let the user move not only through the topology of a system, but through the supported history of that topology.**

The emotional thesis is:

> **Space makes the machine explorable. Time makes it alive.**

The technical thesis is:

> **A system is a topology changing through time. Ono-Sendai makes that changing topology queryable, navigable and explainable without inventing facts.**

The most important honesty rule in this entire document is:

> **Ono may reconstruct only what its evidence can support. It may correlate events that happened together, but it MUST NOT present correlation as causation.**

This rule is the temporal equivalent of v0.3 refusing to invent structure from arbitrary text and v0.4 refusing to invent topology for visual effect.

# 1. Product Thesis

## 1.1 From snapshot to world

Without v0.5, a spatial interaction can look like:

```text
local:// > enter service nginx
local/service/nginx:// > map
```

This answers:

```text
Where is nginx now?
What is connected to it now?
What state is visible now?
```

v0.5 adds:

```text
What happened here?
What changed?
What did this place look like before?
What evidence explains the current state?
What happened because of an action I performed?
What is known, what is only correlated, and what is unknown?
```

A user should be able to enter a service, filesystem, host, socket, container or other place and ask for its history without mentally switching to a separate log-analysis product.

## 1.2 Time is a coordinate, not a filter option

Ono MUST NOT implement temporal behavior merely by adding `--since` and `--until` to unrelated commands.

The session already has spatial position. v0.5 adds an orthogonal temporal coordinate:

```text
Context {
    space: local/service/nginx
    time: now
}
```

or:

```text
Context {
    space: local/service/nginx
    time: 2026-08-31T12:17:00+02:00
}
```

When the temporal coordinate changes, every Ono-native read operation that supports historical semantics MUST resolve against that coordinate consistently.

### Intent

The user should feel that they moved to a point in the machine's history, not that they remembered to repeat `--at 12:17` on six commands.

## 1.3 History is not omniscience

Ono MUST NOT pretend that historical truth exists simply because the user asks for it.

Historical information may come from:

- events observed during the current session;
- an Ono recorder;
- provider-owned historical sources such as a journal;
- retained snapshots;
- remote Ono agents or links;
- KUANG/11 extensions with historical capabilities.

If no source observed a fact, Ono does not know that fact.

If a source observed only part of a state, the reconstruction is partial.

If a provider cannot prove absence, missing data MUST NOT become a claim that an object did not exist.

## 1.4 Causality is evidence, not storytelling

A useful systems interface naturally invites questions such as:

```text
why service nginx
why event @e42
why field state
```

These questions are dangerous if implemented as narrative guesswork.

Ono MUST distinguish at least:

```text
CAUSED_BY
CORRELATED_WITH
PRECEDED_BY
UNKNOWN
```

A nearby configuration change is not automatically the cause of a service failure. A process disappearing shortly before a service failure may be relevant but is not automatically causal. A systemd job explicitly linking a requested restart to a state transition is much stronger evidence.

The interface must make those differences visible.

# 2. Core Temporal Invariants

An implementation conforming to v0.5 MUST obey all of the following invariants.

1. **Time is explicit.** Ono MUST always be able to report whether a query is evaluated at `now` or at a historical coordinate.
2. **The past is read-only.** Mutations MUST NOT execute while the active temporal coordinate is historical.
3. **History requires evidence.** No historical fact may be presented without provenance or a documented reconstruction rule.
4. **Absence is not default knowledge.** Failure to observe an object is not proof that the object did not exist.
5. **Coverage is first-class.** Historical queries MUST expose complete, partial, unavailable or uncertain coverage.
6. **Gaps remain gaps.** Ono MUST NOT interpolate arbitrary system states across unobserved intervals.
7. **Causation is stricter than correlation.** Temporal proximity alone MUST NEVER create a `caused_by` relation.
8. **Ordering is not causation.** `A happened before B` does not imply `A caused B`.
9. **Clock uncertainty is visible.** Distributed events MUST not be totally ordered beyond what clock and causal evidence support.
10. **Provenance survives reconstruction.** A reconstructed historical object MUST retain the evidence sources supporting its fields and relationships.
11. **Live and historical views share truth.** Pausing or rewinding a live map MUST use the same canonical event/state model as textual temporal queries.
12. **History and shell history are distinct.** System history is not merely a pretty renderer over command strings.
13. **Ono actions are traceable.** A mutation initiated through Ono MUST emit an action record that can later participate in causal explanation.
14. **External commands remain honest.** Ono MUST NOT claim to know side effects of arbitrary external tools unless an adapter/provider reports them.
15. **Retention is bounded.** v0.5 MUST NOT silently become an unlimited monitoring archive.
16. **Recording is opt-in.** Persistent local temporal recording MUST be disabled by default.
17. **Recording does not escalate privilege.** The recorder MUST NOT gain visibility the user does not already possess merely because it persists data.
18. **Secrets stay out of history.** Secret values, credentials and raw protected content MUST NOT enter the evidence ledger by default.
19. **Machine-readable semantics precede rendering.** Timeline and causal views MUST render typed values rather than become the data contract themselves.
20. **No fake rewind.** Interactive rewind MUST show only reconstructable states and MUST mark uncertainty rather than smoothing gaps into fiction.

# 3. Conceptual Model

## 3.1 The temporal system

v0.5 defines the temporal system from seven concepts:

```text
EVENTS
  +
EVIDENCE
  +
COVERAGE
  +
CHECKPOINTS
  +
RECONSTRUCTION
  +
CAUSAL LINKS
  +
TEMPORAL CONTEXT
  =
TEMPORAL SYSTEM INTERFACE
```

No single concept is sufficient.

Events without coverage cannot prove absence. Snapshots without event identity cannot explain transitions. Causal links without evidence become speculation. A timeline without temporal context is only a report.

## 3.2 Observation

An **Observation** is evidence that a source reported a fact at or about a point in time.

Examples:

```text
procfs snapshot says process 1842 exists at observed time
systemd D-Bus says unit nginx.service entered failed
netlink says route 10.0.0.0/8 was removed
journald entry records systemd job result
Ono ActionResult says restart service nginx was requested and accepted
```

Observation is not synonymous with truth. It always retains source identity and timing.

## 3.3 Event

An **Event** is a typed temporal record describing a meaningful change, observation or action.

An event MUST have stable identity independent from its rendered timeline row.

Canonical shape:

```text
TemporalEvent {
    event_id: EventId
    kind: EventKind
    scope: SpatialScope
    subject: SpatialId?
    related: List<SpatialId>

    source_time: Timestamp?
    observed_at: Timestamp
    ingested_at: Timestamp
    source_sequence: UInt?
    boot_id: String?

    before: Value?
    after: Value?
    changed_fields: List<FieldChange>

    evidence: List<EvidenceRef>
    causal_parents: List<CausalLinkRef>
    provenance: Provenance
}
```

`source_time` is when the source says the event happened.
`observed_at` is when the observing component received or detected it.
`ingested_at` is when it entered the Ono ledger.

These timestamps MUST NOT be silently collapsed into one field.

## 3.4 Evidence

Evidence answers:

> **Why does Ono believe this historical claim?**

Canonical shape:

```text
Evidence {
    evidence_id: EvidenceId
    source: EvidenceSource
    observed_at: Timestamp
    source_time: Timestamp?
    scope: SpatialScope
    subject: SpatialId?
    claim: EvidenceClaim
    strength: EvidenceStrength
    raw_ref: OpaqueReference?
    provenance: Provenance
}
```

Raw log content or secret-bearing payloads MUST NOT be copied into evidence merely for convenience. `raw_ref` MAY point to a provider-owned source that the user can explicitly inspect under normal permissions.

## 3.5 Coverage

Coverage represents what a source was capable of observing over an interval.

Canonical states:

```text
complete
partial
point_sample
unknown
unavailable
permission_denied
```

A source that took one process snapshot at 12:00 has `point_sample` coverage for process existence at 12:00. It does not have complete process-existence coverage from 11:50 to 12:10.

A recorder polling a provider every second MAY describe its coverage as partial with an explicit sampling interval unless the provider itself guarantees exhaustive event delivery.

## 3.6 Checkpoint

A **Checkpoint** is a bounded retained state projection used to reconstruct history efficiently.

A checkpoint MUST include:

```text
Checkpoint {
    checkpoint_id: Uuid
    scope: SpatialScope
    captured_at: Timestamp
    coverage: TemporalCoverage
    objects: List<ObjectStateRef>
    relations: List<RelationStateRef>
    provenance: Provenance
}
```

A checkpoint is not automatically complete just because it serializes many objects.

## 3.7 Reconstruction

A **Reconstruction** is the best evidence-supported state Ono can produce for a requested time.

Every reconstruction MUST report:

```text
as_of
coverage
sources
gaps
reconstruction_method
```

It MUST never imply exactness merely because a renderer can draw a complete-looking table or map.

## 3.8 Causal link

A causal link is a typed relationship between events.

Canonical relationship classes are defined in section 15.

## 3.9 Temporal context

A session temporal context is either:

```text
Present
```

or:

```text
Historical {
    requested: TimeSelector
    resolved_at: Timestamp
    coverage: TemporalCoverageSummary
}
```

The context is orthogonal to spatial place.

# 4. Temporal Coordinate and Session Semantics

## 4.1 Present context

A new interactive Ono session starts in:

```text
time = now
```

Current behavior from v0.2-v0.4 remains unchanged.

## 4.2 Entering historical context

The canonical command is:

```text
at <time-selector>
```

Examples:

```text
at -10m
at 12:17
at 2026-08-31T12:17:00+02:00
at event @e42
```

On success, Ono changes only the temporal coordinate. It MUST NOT change spatial place.

Example:

```text
local/service/nginx:// > at -10m

historical context
  requested    -10m
  resolved     2026-08-31 12:07:14 +02:00
  coverage     partial

local/service/nginx:// @12:07:14 [PAST?] >
```

The `?` marker indicates incomplete or uncertain coverage.

With complete supported coverage:

```text
local/service/nginx:// @12:07:14 [PAST] >
```

## 4.3 Returning to the present

The canonical command is:

```text
now
```

`now` MUST:

- restore present temporal context;
- retain current spatial place if it still exists;
- if the historical place has no live counterpart, return to its nearest live canonical parent and report the transition;
- never silently reinterpret a tombstone as a current live object.

## 4.4 Time selector grammar

v0.5 defines the following time selectors:

```text
absolute RFC3339       2026-08-31T12:17:00+02:00
local date-time        2026-08-31 12:17:00
local time today       12:17:00
relative past          -10m, -2h, -3d
event reference        event @e42
```

Relative future selectors are invalid for historical context.

Ambiguous local times during DST transitions MUST require disambiguation by offset when both instants exist.

## 4.5 Per-command `--at`

Read-only Ono-native commands that support historical evaluation SHOULD accept:

```text
--at <time-selector>
```

This evaluates the command at that time without changing session context.

Example:

```text
get service nginx --at -1h
```

`--at` MUST use the same reconstruction engine as `at` context. It MUST NOT implement a separate historical code path.

## 4.6 Historical prompt/HUD

Historical context MUST be visually obvious.

Minimum text prompt:

```text
local/service/nginx:// @12:07:14 [PAST] >
```

Partial coverage:

```text
local/service/nginx:// @12:07:14 [PAST?] >
```

The renderer MAY use color, but the distinction MUST remain visible in monochrome and plain text.

## 4.7 Past context is read-only

All Ono mutations MUST fail while historical context is active.

Example:

```text
local/service/nginx:// @12:07:14 [PAST] > restart service nginx

temporal.read_only
`restart service` cannot execute while observing historical state

return to the present:
  now
```

This rule applies to:

- native mutations;
- KUANG/11 mutation tools;
- remote mutations;
- shell-state-changing operations whose semantics would be confusing in a historical world.

## 4.8 External commands in historical context

Arbitrary external programs execute in the present and may have side effects. Therefore they MUST NOT run directly while historical context is active.

The explicit escape hatch is:

```text
present <external-command> [args...]
```

Example:

```text
local/service/nginx:// @12:07:14 [PAST] > present git status
```

This executes in the real current environment without changing the temporal context.

The HUD MUST make the present-bound execution visible at least once in the command result metadata.

A pure Ono transform over already reconstructed values remains allowed:

```text
get process | where cpu > 20 | select pid name
```

if `get process` can produce historical values for the active context.

### Intent

Historical context must be safe enough to explore freely. A user rewinding the machine should not accidentally mutate the current machine because their fingers remained in normal shell mode.

# 5. Temporal Identity and Lifetimes

## 5.1 Spatial identity remains authoritative

v0.5 MUST reuse v0.4 `SpatialId` and lifetime semantics. It MUST NOT create a parallel identity namespace for temporal history.

An event refers to the same logical object identity used by `enter`, `near`, `map` and pipelines.

## 5.2 Lifetime-bound objects

Processes, transient connections and other lifetime-bound objects require explicit lifetime identity.

A PID is not sufficient across time.

The process identity MUST include enough source information to distinguish PID reuse, using the v0.4 identity rules such as boot identity and process start identity.

Historical references MUST NOT accidentally resolve a dead process to a later process reusing the same PID.

## 5.3 Conceptual objects across lifetimes

A service such as `nginx.service` may remain one conceptual spatial identity across many process lifetimes.

Temporal history therefore distinguishes:

```text
service identity      stable conceptual object
process identity      specific lifetime object
```

A historical map may show one service place connected to different process identities at different times.

## 5.4 Tombstones and history

A v0.4 tombstone is evidence that a previously known place is gone in the present.

v0.5 extends this meaning:

- historical navigation MAY enter the object's known lifetime;
- `timeline` for a tombstone MUST remain available while retained evidence exists;
- `now` MUST NOT revive a tombstone;
- replacement candidates remain separate identities unless an existing conceptual identity explicitly spans them.

## 5.5 Identity merge across sources

Provider history, adapter history, recorder observations and KUANG/11 events MUST reconcile through existing canonical identity rules before becoming one object's history.

A log line mentioning PID 1842 MUST NOT independently mint a new process identity if the canonical process identity can be resolved.

If identity cannot be established, the event may remain an **unresolved temporal subject** and MUST be rendered as such rather than attached to a guessed object.

# 6. Canonical Event Model

## 6.1 Event kinds

v0.5 defines the following top-level event kinds:

```text
object.observed
object.appeared
object.changed
object.disappeared
relation.added
relation.removed
action.requested
action.authorized
action.executed
action.completed
action.failed
provider.event
coverage.started
coverage.ended
checkpoint.created
landmark.added
landmark.removed
```

Providers and plugins MAY define namespaced subtypes, but the top-level semantics MUST map to one of these classes.

## 6.2 Field changes

`object.changed` MUST carry typed field changes where known:

```text
FieldChange {
    field: String
    before: Value?
    after: Value?
    certainty: ChangeCertainty
}
```

`before == null` and `after == null` MUST NOT be overloaded to mean arbitrary unknown. The existing Ono absent/unknown/error vocabulary must be preserved.

## 6.3 Appearance and disappearance

`object.appeared` means evidence supports that the object became observable within the source's coverage semantics.

`object.disappeared` means evidence supports that a previously observed object ceased to exist or ceased to be represented according to an authoritative source.

A polling gap MUST NOT automatically emit disappearance unless the provider contract defines missing-from-complete-snapshot as meaningful absence.

## 6.4 Relation events

Spatial relationships gain temporal change records:

```text
relation.added
relation.removed
```

The relation event MUST identify:

```text
from SpatialId
to SpatialId
relation type
provider/source
confidence
```

A historical `map` reconstructs the edge set as of the requested time.

## 6.5 Provider event

A provider event may represent source-native information that does not yet map to a canonical object mutation.

It MUST still be typed and namespaced.

Provider events MAY later participate as evidence in a causal explanation without becoming canonical object state.

## 6.6 Action events

Actions initiated through Ono are defined in section 16. They are first-class events because the shell itself possesses unusually strong knowledge of operator intent and execution.

## 6.7 Event immutability

Persisted events MUST be append-only.

Corrections are represented by new events or evidence records referencing the prior event. Existing persisted events MUST NOT be silently rewritten except during explicit storage migration that preserves semantic identity.

## 6.8 Deduplication

The ledger MUST support deduplication where sources provide stable sequence IDs or event IDs.

Deduplication MUST NOT collapse two distinct source events merely because their rendered text is equal.

# 7. Evidence Model

## 7.1 Evidence sources

Canonical evidence source classes:

```text
ono.session
ono.recorder
linux.procfs
linux.netlink
linux.systemd-dbus
linux.journald
adapter:<adapter-id>
remote:<link-id>/<provider-id>
kuang:<package-id>/<provider-id>
```

Sources MUST have stable inspectable identity.

## 7.2 Evidence strength

Canonical evidence strengths are:

```text
authoritative
asserted
derived
correlated
observational
```

Definitions:

**authoritative**
: The source owns the relevant fact for the queried scope, such as a systemd unit state reported by systemd.

**asserted**
: A source explicitly reports a fact or relation but is not itself the sole authority.

**derived**
: Ono computed the claim through a deterministic documented rule from stronger evidence.

**correlated**
: The evidence establishes meaningful association but not causation.

**observational**
: Ono observed a sample or snapshot without exhaustive coverage guarantees.

Evidence strength MUST NOT be automatically upgraded by renderers, AI assistants or plugins.

## 7.3 Evidence chains

A derived claim MUST reference the evidence it derives from.

Example:

```text
claim: process 2741 appeared as member of nginx.service

derived from:
  systemd cgroup membership @14:03:13
  procfs process identity @14:03:13
```

`inspect --provenance` MUST make this chain navigable.

## 7.4 Negative evidence

Negative historical claims require special care.

Ono MAY claim:

```text
process X did not exist at time T
```

only if an authoritative or sufficiently complete source had coverage capable of proving that absence.

A sparse journal without a process start event MUST NOT be treated as proof that the process never existed.

## 7.5 Evidence gaps

A gap is itself a typed temporal object:

```text
TemporalGap {
    scope
    from
    to
    capability
    reason
    source
}
```

Reasons include:

```text
not_recorded
retention_expired
provider_unavailable
permission_denied
source_disconnected
clock_uncertain
corrupt_segment
unsupported
```

Gaps MUST be shown where they materially affect a query.

## 7.6 Raw evidence access

Where a provider owns raw historical material such as journal entries, Ono MAY expose an explicit route to inspect that material.

The temporal ledger SHOULD retain references and parsed structured facts, not duplicate unlimited raw logs.

### Intent

Evidence must remain inspectable enough that a skeptical operator can distinguish a fact Ono observed from a story Ono composed.

# 8. Temporal Coverage

## 8.1 Coverage dimensions

Coverage is described across:

```text
scope
object/relationship capability
interval
source
completeness
sampling period if applicable
permission state
```

Coverage MUST NOT be represented by one global boolean.

## 8.2 Complete coverage

`complete` means the source contract is capable of observing every relevant event or every object in the requested state class for the specified interval.

Examples may include:

- an authoritative event stream with sequence continuity;
- a complete checkpoint at a precise instant;
- a provider-owned historical database guaranteeing the queried class.

## 8.3 Partial coverage

`partial` means useful evidence exists but absence cannot be interpreted as proof of non-existence.

## 8.4 Point sample

A snapshot collected at one time is a `point_sample`.

It can support state at that point subject to provider semantics. It cannot by itself explain intermediate change.

## 8.5 Coverage composition

A reconstruction using multiple sources MUST compute the effective coverage per field/relation rather than simply selecting the strongest global label.

The renderer MAY summarize to:

```text
complete
partial
uncertain
```

but `inspect` MUST expose source-level detail.

## 8.6 Coverage in the prompt

Historical prompt indicators:

```text
[PAST]      supported coverage sufficient for current place summary
[PAST?]     current reconstruction is materially partial/uncertain
```

A prompt MUST NOT display `[PAST]` merely because at least one event exists near that time.

# 9. Historical State Reconstruction

## 9.1 Reconstruction algorithm

For a requested time `T`, Ono SHOULD reconstruct state in this order:

1. select the nearest trusted checkpoint at or before `T` for the relevant scope;
2. apply ordered compatible events from that checkpoint through `T`;
3. merge provider-owned historical queries that directly answer fields at `T`;
4. reconcile identity and relations through canonical v0.4 rules;
5. compute coverage and gaps;
6. return typed reconstructed objects with provenance.

If no checkpoint exists, Ono MAY reconstruct directly from events when coverage and event semantics support it.

## 9.2 No arbitrary interpolation

Between two observations:

```text
12:00 state=running
12:10 state=failed
```

Ono MUST NOT claim the state at 12:05 unless evidence supports it.

It may report:

```text
state: unknown in interval 12:00..12:10
last observed running at 12:00
next observed failed at 12:10
```

If an exhaustive event stream proves no transition occurred until 12:08, the stronger coverage may narrow the uncertainty.

## 9.3 State validity intervals

When evidence supports a continuous state interval, Ono MAY expose:

```text
valid_from
valid_until
```

These intervals MUST derive from source semantics, not from guessed midpoint interpolation.

## 9.4 Historical object schema

Reconstructed objects retain their canonical schema plus temporal metadata:

```text
_temporal {
    as_of: Timestamp
    coverage: TemporalCoverageSummary
    reconstructed: Bool
    sources: List<SourceRef>
    gaps: List<TemporalGapRef>
}
```

The temporal metadata MUST NOT collide with provider fields.

## 9.5 Historical relations

A relation exists at `T` only if reconstruction supports its existence at `T`.

Unknown relation state MUST be distinguishable from absent relation state.

## 9.6 Historical collections

`get process` in historical context MUST return the best supported process set for `T` and attach collection-level coverage.

If the source cannot prove complete enumeration, the result MUST NOT imply that the returned rows are the complete process list.

## 9.7 Historical current place

If the current v0.4 place did not yet exist at `T`, `look` MUST report:

```text
place not known at requested time
```

with coverage explaining whether this means known-absent or simply unknown.

It MUST NOT automatically jump elsewhere unless the user asks to navigate.

# 10. Recorder and Persistent Temporal Memory

## 10.1 Purpose

The recorder exists to retain enough local system history for Ono's temporal interface to remain useful across shell sessions.

It is **not** a metrics platform, log archive or general monitoring product.

## 10.2 Default state

Persistent recording MUST be disabled by default.

A normal Ono installation without recording MUST still support:

- current-session events;
- provider-owned history;
- historical sources exposed through adapters/plugins;
- temporal queries within whatever evidence exists.

## 10.3 Canonical commands

Recorder management follows Ono verb-target semantics:

```text
get recorder
start recorder
stop recorder
```

`get recorder` returns `ono.recorder-status/1`.

## 10.4 Default retention

When the user first starts the recorder without custom settings, defaults are:

```text
temporal.recording.enabled      true
temporal.retention.max_age      24h
temporal.retention.max_size     512MiB
temporal.checkpoint.interval    5m
temporal.flush.interval         2s
```

Retention removes the oldest eligible data when either age or size limit is exceeded.

## 10.5 Recorder privilege

The recorder MUST run with the user's privileges by default.

It MUST NOT:

- become setuid;
- automatically request sudo;
- run a privileged system daemon merely to increase visibility;
- read data the same user could not query through Ono providers.

A future separately configured privileged source may contribute evidence, but that is outside the default v0.5 recorder.

## 10.6 Collection policy

The recorder SHOULD collect:

- canonical provider events already available to Ono;
- snapshots required for configured checkpoint scopes;
- spatial relation changes;
- Ono action events;
- source coverage markers;
- landmark transitions where the landmark rule itself is deterministic and useful for timeline orientation.

It MUST NOT persist by default:

- arbitrary stdout/stderr bodies;
- complete file contents;
- shell environment dumps;
- secrets;
- command-line arguments known to contain secret values;
- raw network packet payloads;
- unlimited metrics samples.

## 10.7 Session-only temporal memory

Even without the recorder, the interactive session SHOULD maintain a bounded in-memory ledger for current-session temporal features.

Default session retention:

```text
max events     100000
max age        session lifetime
```

This in-memory ledger is discarded at session end unless recording is enabled.

## 10.8 Recorder lifecycle

`start recorder` MUST be idempotent.

`stop recorder` MUST flush the ledger and stop cleanly.

A crash MUST not leave the primary ledger unrecoverable; storage requirements are defined in section 31.

## 10.9 Background service model

The reference Linux implementation SHOULD support a user service named:

```text
ono-recorder.service
```

The command layer MAY start/stop this through the existing service mechanisms or a dedicated recorder controller, but user-facing semantics remain `start recorder` and `stop recorder`.

### Intent

Persistent history should be easy to enable but impossible to confuse with hidden surveillance. The user must know when Ono is retaining system history and how much it retains.

# 11. Timeline

## 11.1 Purpose

`timeline` is the primary chronological projection of temporal events.

Unlike v0.2 command history, it combines system events, actions, topology changes and relevant provider events into one typed stream.

## 11.2 Canonical syntax

```text
timeline [selector] [--since DURATION|TIME] [--until TIME] [--kind KIND] [--all]
```

Examples:

```text
timeline
timeline service nginx
timeline --since 30m
timeline process 1842 --since 10m
timeline --kind relation.added
timeline --all --since 5m
```

## 11.3 Default scope

Without a selector, `timeline` is scoped to the current spatial place and its directly relevant events.

At the root system place, it shows high-significance events and current-session actions rather than dumping every event from every object.

`--all` requests the full visible scope subject to retention and permissions.

## 11.4 Output type

`timeline` returns:

```text
Stream<TemporalEvent>
```

The timeline renderer is only a presentation.

This MUST work:

```text
timeline --since 1h
    | where kind == "object.changed"
    | where subject.type == "service"
```

## 11.5 Default text rendering

Example:

```text
12:17:51.203  config/nginx.conf       changed
12:18:02.011  nginx.service           reload requested        [ono]
12:18:02.044  nginx.service           activating              [systemd]
12:18:03.104  process/1827            disappeared
12:18:03.119  process/2741            appeared
12:18:03.401  nginx.service           active                  [systemd]
```

Source tags SHOULD be abbreviated but inspectable.

## 11.6 Event references

Rendered events MUST expose stable references usable in subsequent commands:

```text
@e42
@e43
```

Examples:

```text
inspect event @e42
at event @e42
why event @e42
```

## 11.7 Timeline gaps

If a visible interval contains a material coverage gap, the text renderer MUST show it:

```text
12:20:00        ---- coverage gap: recorder offline 4m12s ----
12:24:12
```

A gap MUST not be hidden simply because events exist on both sides.

## 11.8 Timeline and current temporal context

When historical context is active, default `timeline` centers around that time rather than `now`.

Default window:

```text
historical center +/- 15m
```

unless constrained by retention or user configuration.

# 12. `at` and `now` Behavior in Detail

## 12.1 `at` resolves before changing context

`at` MUST resolve the selector and coverage before committing the session transition.

If the selector is invalid, the existing temporal context remains unchanged.

## 12.2 `at event`

```text
at event @e42
```

sets the temporal coordinate to the event's primary resolved time.

The event remains available as the temporal anchor in session metadata.

## 12.3 `at` on unavailable history

If no usable evidence exists:

```text
local:// > at -3d

temporal.not_recorded
no source can reconstruct this scope at the requested time

available:
  session history      42m
  recorder history     disabled
  journald              2d for service events
```

The session remains at its previous temporal coordinate.

## 12.4 Temporal movement history

Changes to temporal coordinate MUST participate in a temporal trail separate from the spatial navigation trail.

The shell MAY expose this via:

```text
timeline context
```

but no new user-facing `time-back` command is required in v0.5.

`back` remains spatial navigation and MUST NOT become overloaded.

### Intent

Spatial and temporal navigation are orthogonal. Reusing `back` for both would make movement impossible to reason about.

# 13. Changes and State Comparison

## 13.1 Canonical command

The canonical command is:

```text
changes [selector] --since <time-selector> [--until <time-selector>]
```

If `--until` is omitted, it means the active temporal coordinate, normally `now`.

Examples:

```text
changes --since 10m
changes service nginx --since 1h
changes filesystem /data --since 08:00
changes --since 12:00 --until 12:30
```

## 13.2 Output type

`changes` returns:

```text
Stream<TemporalChange>
```

Canonical change classes:

```text
added
removed
changed
relation_added
relation_removed
```

## 13.3 Example

```text
local:// > changes --since 10m

ADDED
  process/7128          backup
  connection/98133      backup -> nas01:22

REMOVED
  process/6902          backup

CHANGED
  service/backup
    state               running -> failed

  filesystem/data
    used                 81.2% -> 94.1%
```

## 13.4 Unknown comparison

If one side lacks enough evidence, the field MUST be reported as unknown rather than fabricated:

```text
filesystem/data.used
  from      unknown
  to        94.1%
  coverage  partial before 12:00
```

## 13.5 Changes in spatial views

`look`'s v0.4 `changed` section SHOULD be backed by the same `TemporalChange` engine when v0.5 evidence exists.

It MUST NOT retain a separate ad-hoc snapshot comparison implementation once the temporal engine is available.

# 14. Historical Spatial Navigation

## 14.1 Spatial commands honor temporal context

The following v0.4 commands MUST evaluate against historical state where evidence permits:

```text
look
near
find place
map
enter
follow
jump
up
home
```

`back` continues to traverse the actual session spatial trail, but every visited place is rendered at the active temporal coordinate.

## 14.2 Example

```text
local:// > enter service nginx
local/service/nginx:// > at 12:17
local/service/nginx:// @12:17 [PAST] > map
```

The map shows the reconstructable topology at 12:17, not the current topology with an old timestamp label.

## 14.3 Historical exits

An exit shown by `look` or `near` at time `T` MUST correspond to a relation or hierarchy supported at `T`.

Current-only exits MUST not leak into a historical neighborhood.

## 14.4 Historical discovery

`find place` in historical context searches the historical index for the active time.

If the implementation uses present-day aliases to help resolve a historical object, it MUST distinguish **resolution aid** from **historical existence evidence**.

## 14.5 Historical storage paths

Filesystem paths are especially difficult because normal filesystems do not retain arbitrary historical directory trees.

Ono MUST therefore show only historical filesystem structure supported by:

- recorder checkpoints;
- filesystem-specific snapshot providers;
- explicit audit/inotify/FSEvents-like evidence sufficient for reconstruction;
- KUANG/11 providers.

No generic v0.5 implementation may pretend that current directory contents represent the past.

# 15. Causal Model

## 15.1 Causal relationship classes

v0.5 defines the following canonical temporal relationship classes:

```text
caused_by
triggered_by
resulted_in
correlated_with
preceded_by
```

Their inverse labels are:

```text
caused
triggered
result
correlated_with
followed_by
```

## 15.2 `caused_by`

`B caused_by A` is permitted only when evidence supports a direct causal statement under a registered rule or source contract.

Examples:

- Ono action ID directly leads to a systemd job;
- systemd job result explicitly identifies the unit transition;
- a parent process creation event from the kernel identifies its parent/child creation relationship;
- a provider explicitly exposes a causal transaction identifier.

Temporal proximity is insufficient.

## 15.3 `triggered_by`

`triggered_by` is used when A initiates a mechanism that may involve intermediate steps before B.

Example:

```text
service reload triggered_by operator action
```

The difference from `caused_by` is semantic and source-dependent. Registered rules MUST define which relationship they emit.

## 15.4 `resulted_in`

`A resulted_in B` is the forward-facing projection of a known effect chain, particularly useful for operator actions.

It MUST reference the same evidence chain as its inverse causal view.

## 15.5 `correlated_with`

Correlation means a registered correlation rule found meaningful temporal and/or structural association without sufficient causal evidence.

Examples:

- config file changed 9 seconds before a service failure;
- filesystem pressure and application errors overlapped;
- a remote endpoint disappeared near a local retry spike.

Correlation MUST be visually and structurally distinct from causation.

## 15.6 `preceded_by`

`preceded_by` states only temporal order according to the supported ordering model.

It MUST NOT be rendered with language such as:

```text
because
therefore
led to
caused
```

## 15.7 Unknown cause

Unknown cause is a valid outcome, not an error.

A causal query MUST be able to return:

```text
cause: unknown
```

while still listing evidence, correlated events and gaps.

## 15.8 Causal rule registry

Every built-in rule that emits `caused_by`, `triggered_by` or `resulted_in` MUST be machine-readable and inspectable.

A rule records:

```text
rule_id
input event kinds
required evidence strengths
identity constraints
time constraints if any
output relation
provider/source constraints
```

No renderer may create causal language outside this registry.

### Intent

Causal explanations should feel powerful precisely because they are conservative. "I do not know" is better than a confident fiction.

# 16. `why` - Causal Explanation

## 16.1 Purpose

`why` is the primary human-facing causal query.

It is not an LLM prompt and MUST NOT require an AI model.

The core implementation uses registered causal rules, event relationships, evidence and coverage.

## 16.2 Canonical forms

v0.5 defines exactly three forms:

```text
why <target> <selector>
why event <event-ref>
why field <field-name>
```

Examples:

```text
why service nginx
why process 2741
why event @e42
why field state
```

`why field` operates on the current spatial object and asks about the most recent supported change to that field at or before the active temporal coordinate.

## 16.3 Target explanation

For:

```text
why service nginx
```

Ono selects the most recent causally explainable notable state transition relevant to the service at the active temporal coordinate.

If multiple equally relevant transitions exist, Ono MUST refuse ambiguity and list event references rather than choose arbitrarily.

## 16.4 Typed output

`why` returns one `CausalExplanation` value:

```text
CausalExplanation {
    subject
    explained_event
    state_or_change
    cause: CausalNode?
    causal_chain: List<CausalStep>
    correlations: List<TemporalAssociation>
    preceding: List<TemporalAssociation>
    gaps: List<TemporalGapRef>
    coverage: TemporalCoverageSummary
    provenance: Provenance
}
```

## 16.5 Text rendering

Example with known cause:

```text
nginx.service
failed at 14:03:17.004

known cause
  process/1827 exited with code 1

chain
  14:03:16.812  process/1827 exited code=1
        |
        | caused_by evidence: systemd unit result + process membership
        v
  14:03:17.004  nginx.service entered failed

evidence
  systemd D-Bus       authoritative
  process identity    procfs observation
  unit result         journald/systemd asserted

coverage
  service events      complete for interval
  process details     partial
```

## 16.6 Text rendering with unknown cause

```text
nginx.service
failed at 14:03:17.004

cause
  unknown

correlated
  14:03:06  /etc/nginx/nginx.conf changed
              11s before failure
              correlation only

preceded by
  14:03:16  process/1827 disappeared

coverage gap
  process exit status unavailable
```

The renderer MUST NOT move the config change into the `known cause` section because it appears plausible.

## 16.7 Explanation depth

Default `why` SHOULD return at most three causal hops in text mode.

The user can request:

```text
why service nginx --depth 8
```

Interactive views MAY allow expansion.

The underlying typed graph may contain more steps than the default renderer shows.

## 16.8 Explaining current existence

For a process created through an Ono action chain:

```text
why process 2741
```

may answer:

```text
process/2741 exists because nginx.service started a new worker

chain
  restart service nginx             operator action
      -> systemd job 4821
      -> nginx.service activating
      -> process/2741 created
```

Only source-supported links may appear in the causal chain.

# 17. Ono Actions as Causal Anchors

## 17.1 Why shell actions matter

A shell has one source of causal information that external monitoring systems often lack: it knows exactly which actions the operator requested through the shell.

v0.5 MUST preserve this advantage.

## 17.2 Action lifecycle

Every Ono-native mutation MUST emit a causally linked action lifecycle:

```text
action.requested
    -> action.authorized
    -> action.executed
    -> action.completed | action.failed
```

Not every implementation requires all four as separate persisted rows, but the canonical event model MUST represent the semantics.

## 17.3 Action identity

Every mutation receives an `ActionId` before execution.

The ID MUST be propagated through internal provider calls where feasible.

If an external authority returns its own transaction/job ID, the event ledger MUST record the mapping.

Example:

```text
ActionId ono:a91f
    -> systemd job /org/freedesktop/systemd1/job/4821
```

## 17.4 Action record

Canonical public shape:

```text
ActionEvent {
    action_id: ActionId
    command: RedactedCommandSummary
    actor: UserIdentity
    session_id: SessionId
    requested_at: Timestamp
    target: SpatialId?
    operation: String
    authorization: AuthorizationSummary
    result: ActionResult?
    external_transaction: String?
    provenance: Provenance
}
```

## 17.5 Redaction

Command recording MUST use semantic redaction.

If a command includes a secret typed as `Secret`, the persisted summary records:

```text
<secret:redacted>
```

rather than the raw value.

Unknown arbitrary external command text is NOT persisted as an action body by the temporal recorder unless explicitly configured.

## 17.6 External command causality

Ono can safely claim:

```text
external process P was launched because command C executed
```

because the shell owns process creation.

Ono MUST NOT claim arbitrary downstream effects of P unless an adapter, provider or other evidence source reports them.

## 17.7 Example

```text
local/service/nginx:// > restart service nginx
```

Later:

```text
local/service/nginx:// > timeline --since 5m

14:03:11.002  @e91 action.executed    restart nginx.service
14:03:11.017  @e92 provider.event     systemd job 4821 created
14:03:11.401  @e93 object.changed     nginx.service active -> deactivating
14:03:12.108  @e94 object.disappeared process/1827
14:03:12.410  @e95 object.appeared    process/2741
14:03:13.002  @e96 object.changed     nginx.service activating -> active
14:03:13.010  @e97 action.completed   restart nginx.service
```

`why process 2741` may then have unusually strong causal evidence.

# 18. Live State, Pause and Rewind

## 18.1 One live/historical model

v0.4 `map --live` and v0.5 historical playback MUST converge on one event/state model.

There MUST NOT be:

```text
live diff model A
historical event model B
```

with subtly incompatible semantics.

`ono-spatial-events` MAY remain the live merge/diff component, but its canonical output MUST be ingestible into the temporal event path.

## 18.2 Pausing live view

In a full-screen live map:

```text
Space
```

pauses temporal advancement of the displayed view.

Pausing does NOT stop providers, the recorder or the real system. It freezes only the view's temporal cursor.

The HUD MUST show:

```text
PAUSED @14:03:12.410
```

## 18.3 Rewind keys

Canonical default bindings in the map temporal view:

```text
Space       pause/resume live cursor
[           previous significant event
]           next significant event
Shift-[     -30s
Shift-]     +30s
N           jump to now
T           open timeline at cursor
D           changes cursor -> now
Enter       enter focused object at cursor time
Esc         leave temporal controls / exit view per existing TUI contract
```

Keys MUST be configurable through the existing key-binding mechanism, but these defaults are normative.

## 18.4 Significant event stepping

`[` and `]` step through events relevant to the visible map horizon, not every raw provider event.

The relevance planner SHOULD prioritize:

1. node appearance/disappearance;
2. relation appearance/disappearance;
3. service/container state changes;
4. landmark changes;
5. operator actions;
6. other selected object changes.

## 18.5 No fake frames

Ono MUST NOT interpolate animated intermediate system states between events.

It MAY animate a transition between two evidence-supported states for rendering smoothness, but semantic state changes only at supported event positions.

## 18.6 Rewind beyond coverage

If the user steps into a gap, the view MUST display the gap explicitly.

Example:

```text
HISTORY GAP
12:40:18 - 12:44:30
recorder disconnected

last supported state shown at 12:40:18
```

The map MUST NOT continue showing the last state with a silently advancing timestamp.

## 18.7 Return to live

Pressing `N` or executing `now` returns the temporal cursor to present state.

The view SHOULD briefly summarize accumulated changes if the user spent meaningful time in the past:

```text
returned to now
  +3 processes
  -1 connection
  nginx.service active -> failed
```

This summary uses the canonical `changes` engine.

### Intent

The user should be able to stop a living map, move backward through real change, inspect an earlier topology, and return to the present. That is the core emotional payoff of v0.5.

# 19. Full-Screen Timeline View

## 19.1 Invocation

```text
timeline --view
```

or `T` from a temporal-capable map opens the full-screen timeline.

## 19.2 Layout

The canonical conceptual layout is:

```text
+-------------------- ONO / TIMELINE ------------------------------+
| local/service/nginx      13:55 ------------------------- 14:10   |
|                                                                  |
| 14:03:06  config changed                /etc/nginx/nginx.conf     |
| 14:03:11  action restart                nginx.service             |
| 14:03:12  process gone                  nginx/1827                 |
|>14:03:12  process appeared              nginx/2741                 |
| 14:03:13  service active                nginx.service             |
|                                                                  |
| evidence: procfs, systemd, recorder     coverage: complete*      |
+------------------------------------------------------------------+
| Enter inspect  W why  M map  A at  / filter  ? help  Esc exit   |
+------------------------------------------------------------------+
```

The exact border style may vary. The information architecture is normative.

## 19.3 Timeline interaction

Canonical keys:

```text
Up/Down or j/k    select event
Enter             inspect event
W                 why selected event
M                 map at event time
A                 set session temporal context to event
/                 filter/search
C                 show/hide correlations
G                 show coverage gaps
N                 jump to events near now
?                 help
Esc               exit
```

## 19.4 Event density

Large event sets MUST be semantically grouped rather than rendered as an unreadable firehose.

Allowed grouping dimensions:

- same object and same field in a short interval;
- repeated connection churn within a configured aggregation window;
- repeated provider samples that do not change canonical state;
- cluster-level events already aggregated by a provider.

Grouping MUST preserve hidden counts and time span.

## 19.5 Raw event expansion

A grouped event MUST be expandable to individual retained events where they exist.

# 20. Temporal Search and Discovery

## 20.1 The user must not know when something happened

Temporal discovery has the same principle as v0.4 spatial discovery:

> A user must not need to know the exact timestamp before being able to discover an event.

## 20.2 Timeline filters

The pipeline is the primary general query mechanism:

```text
timeline --since 2h
    | where subject.name contains "nginx"
```

The timeline command also provides ergonomic selectors for common cases.

## 20.3 `find event`

v0.5 adds a canonical native target:

```text
find event <predicate-or-query>
```

Examples:

```text
find event 'kind == "action.failed"'
find event 'subject.name contains "nginx" and source_time > now - 1h'
```

It returns `Stream<TemporalEvent>`.

This reuses the existing `find` verb and Ono expression semantics rather than inventing a new search language.

## 20.4 Completion

Completion for:

```text
at event @
why event @
inspect event @
```

SHOULD prioritize recent events relevant to the current place.

Completion MUST NOT enumerate hidden or unauthorized history.

# 21. Provider Temporal Capabilities

## 21.1 Capability advertisement

Every provider MAY advertise temporal capabilities:

```text
TemporalCapabilities {
    current_snapshot: Bool
    live_events: Bool
    historical_query: Bool
    exhaustive_events: Bool
    causal_tokens: Bool
    checkpointable: Bool
    retained_history: Duration?
}
```

Capabilities MUST be inspectable.

## 21.2 Current snapshot

Existing v0.2 providers already provide current state. v0.5 uses this capability for checkpoints and comparison.

## 21.3 Live events

A provider with `live_events` can emit canonical or mappable events.

Examples:

- netlink route/interface changes;
- systemd D-Bus state changes;
- provider-specific container events.

## 21.4 Historical query

A historical source can directly answer queries about past state or events.

Examples:

- journald for historical service events;
- filesystem snapshot provider;
- container runtime event database if available;
- KUANG/11 integration with an external observability store.

## 21.5 Exhaustive events

`exhaustive_events` is a strong contract. It means sequence continuity can support absence/change claims for the declared capability.

Providers MUST NOT advertise it merely because events usually arrive.

## 21.6 Causal tokens

A provider with `causal_tokens` can carry transaction/job/action identifiers supporting direct causal links.

## 21.7 Checkpointable

A provider is checkpointable if its current snapshot can be safely serialized into the temporal store with canonical identity and provenance.

## 21.8 Provider failure

A temporal provider failure MUST produce coverage loss, not silently freeze the last known state as current history.

# 22. Linux Temporal Sources

## 22.1 procfs

`procfs` is primarily a snapshot source.

The reference provider MUST NOT claim native historical process coverage.

The recorder MAY create process appearance/disappearance events by comparing complete-enough snapshots, but provenance MUST say `snapshot_diff` and coverage MUST reflect polling limitations.

## 22.2 systemd D-Bus

The systemd D-Bus provider SHOULD contribute live unit state transitions and job identity where available.

It is a strong source for service state and action/job correlation.

## 22.3 journald

Journald SHOULD be supported as a provider-owned historical event source for service/system events.

The implementation SHOULD query structured journal fields rather than scrape human-formatted `journalctl` text where a native API is practical.

If v0.3's `journalctl` adapter is used instead, provenance MUST identify the adapter path.

## 22.4 netlink

Netlink SHOULD provide live evidence for:

```text
interface changes
address changes
route changes
neighbor changes
```

Socket connection history is not automatically exhaustive merely because netlink is used elsewhere.

## 22.5 filesystem changes

v0.5 MUST NOT promise complete generic filesystem history.

Optional event sources MAY use inotify/fanotify or filesystem-specific facilities, but recursive coverage limitations must be explicit.

File content is outside the default recorder.

## 22.6 mounts and filesystems

Mount state MAY be checkpointed and changes derived from authoritative mount snapshots or source-native events.

Filesystem usage percentage is not a default high-frequency time series. It MAY be captured at checkpoints or landmark transition time.

## 22.7 containers

Container providers SHOULD expose runtime-native lifecycle events when the runtime supports them.

Container event IDs and runtime identities MUST reconcile with v0.4 container spatial identity.

## 22.8 optional eBPF

No root/eBPF requirement exists for core v0.5.

A KUANG/11 package MAY contribute higher-fidelity process/network/file causal events using eBPF under explicit capability and privilege policy.

### Intent

v0.5 must remain useful on a normal Linux account. High-fidelity privileged sources are extensions, not a hidden prerequisite.

# 23. External Command Adapters and v0.3

## 23.1 Adapter events

v0.3 adapters MAY contribute temporal evidence if their manifest declares temporal capabilities.

They MUST NOT infer temporal structure from arbitrary output simply because a command has been adapted for current structured output.

## 23.2 Historical adapters

An adapter may support a historical query plan when the underlying tool has explicit historical semantics.

Example:

```text
journalctl --since ... --until ... --output=json
```

A historical adapter plan MUST declare:

```text
historical_query: true
coverage semantics
source timestamp mapping
identity mapping
deduplication key if available
```

## 23.3 `ps`, `ss`, `ip`, `lsblk`

Adapters for current-state tools such as `ps`, ordinary `ss`, `ip address` or `lsblk` remain current observations unless their underlying tools expose history.

The temporal recorder MAY retain their structured results as checkpoints where appropriate.

## 23.4 Raw fallback

Raw command fallback remains raw. The temporal layer MUST NOT parse its text to manufacture events.

# 24. Remote and Distributed Time

## 24.1 Remote history is a separate evidence domain

A linked host may provide:

- current snapshots only;
- live events;
- persisted remote history;
- no temporal support.

Negotiation MUST report the available capabilities.

## 24.2 Timestamp fields

Remote events MUST preserve at least:

```text
source_time
source_clock_id or host identity
ingested_at local
clock_uncertainty if known
```

The local ledger MUST NOT overwrite remote source time with ingest time.

## 24.3 No false global total order

Events from two hosts cannot be strictly ordered solely because their wall clocks differ by a few milliseconds.

Ono MUST maintain a partial-order model where appropriate.

Strong ordering evidence includes:

- same-source monotonic sequence;
- explicit request/response relation;
- shared transaction ID;
- known causal message/link relation;
- recorder sequence continuity on one host.

Wall-clock timestamps provide presentation order with uncertainty, not universal causality.

## 24.4 Clock uncertainty rendering

Example:

```text
14:03:12.100 +/- 40ms  web01 connection opened
14:03:12.118 +/- 55ms  db01 connection accepted
```

Ono may display them in timestamp order but MUST NOT claim the first caused the second unless the connection identity or other evidence links them.

## 24.5 Remote `at`

When the user is on a remote spatial place and executes:

```text
at -10m
```

Ono queries/reconstructs the remote place using negotiated remote temporal capabilities.

If the local machine has only local history and the remote has none, it MUST say so.

## 24.6 Federated map history

A cross-host historical map may contain nodes with different coverage quality.

Coverage MUST be representable per host/cluster/node rather than forcing one global label.

# 25. Clock Model

## 25.1 Wall and monotonic time

Local events SHOULD record both:

```text
wall-clock UTC timestamp
monotonic timestamp or sequence where available
boot_id/session clock domain
```

Monotonic time is used for reliable local ordering within a clock domain. Wall time is used for human navigation and cross-source presentation.

## 25.2 UTC storage

Persistent timestamps MUST be stored in UTC with sufficient precision to preserve source data.

Original offset MAY be retained for display provenance.

## 25.3 Local display

Interactive display defaults to the user's current configured timezone.

`inspect event` MUST expose canonical UTC time and source time.

## 25.4 Clock jumps

NTP corrections or manual wall-clock jumps MUST NOT reorder events inside a source sequence.

The ledger MUST use sequence/monotonic evidence where available.

A detected backward wall-clock jump SHOULD create a diagnostic coverage annotation.

## 25.5 Boot boundaries

Monotonic clocks reset across boot.

`boot_id` or equivalent MUST separate clock domains.

## 25.6 DST ambiguity

As specified in section 4, ambiguous local wall times require explicit offset during input if they map to two instants.

# 26. Causal Ordering Across Systems

## 26.1 Happens-before

v0.5 defines `happens_before` as an internal ordering relation supported by stronger evidence than wall time.

Sources include:

- same sequence stream;
- action -> provider transaction;
- request -> response;
- process creation parent event;
- explicit message/connection transaction identity.

## 26.2 Lamport-style internal order

The implementation MAY maintain logical ordering counters internally, but these are not user-facing timestamps and MUST NOT replace source provenance.

## 26.3 Concurrent events

If neither A happens-before B nor B happens-before A, they are potentially concurrent.

The timeline renderer MAY still choose stable display order, but `inspect` MUST not claim semantic ordering.

## 26.4 Cross-host causal chains

A causal explanation can cross hosts only when the evidence chain actually crosses the boundary.

Example:

```text
web01 request @r42
    -> network connection identity
    -> db01 accepted connection @r42
```

Mere temporal proximity between events on different hosts is correlation at most.

# 27. Timeline Relevance and Landmarks

## 27.1 Temporal landmarks

v0.4 landmarks identify spatially significant state. v0.5 adds significant transitions that help orient the timeline.

Built-in temporal landmark candidates:

```text
service failure/recovery
restart loop transition
mount/unmount
filesystem read-only transition
interface up/down
route change
container start/stop/failure
operator action
permission/coverage boundary change
recorder gap
remote link loss/recovery
```

## 27.2 Not alerting

A temporal landmark is a navigation anchor, not an incident alert.

The shell MUST NOT claim operational severity beyond the underlying rule.

## 27.3 Landmark event refs

Landmark transitions MUST retain event references so the user can:

```text
why event @e42
at event @e42
map --at event @e42
```

# 28. Pipeline Integration

## 28.1 Events are values

Temporal events are ordinary Ono values.

Examples:

```text
timeline --since 1h | where kind == "object.changed"
```

```text
changes --since 30m | group subject.object_type
```

```text
find event 'kind == "action.failed"' | take 20
```

## 28.2 Historical objects are canonical objects

A historical `Process` remains a `Process` with temporal metadata, not a separate `HistoricalProcess` type.

This preserves pipeline compatibility.

## 28.3 Temporal functions in expressions

v0.5 adds typed expression helpers:

```text
now()
age(timestamp)
between(timestamp, from, to)
```

`now()` inside a command evaluated under historical context refers to real current time only in expression semantics; historical query time is exposed as:

```text
context.time
```

This distinction MUST be documented and testable.

## 28.4 Event references in values

Values MAY contain `EventRef` and `EvidenceRef` semantic types with inspectable rendering.

They MUST not degrade to magic strings in structured pipelines.

# 29. History vs Temporal History

## 29.1 Existing command history remains

v0.2 command history continues to record command/session semantics according to its existing contracts.

## 29.2 Temporal ledger is different

The temporal ledger records system evidence and events.

The two systems MAY reference one another using `SessionId`, `ActionId` and `EventId`, but they MUST NOT be merged into one unstructured table.

## 29.3 Unified timeline projection

The timeline renderer MAY show an operator command/action next to system events because they share temporal relationships.

The underlying command-history record and event record remain separate typed objects.

## 29.4 Ctrl-R remains command recall

Interactive command recall MUST NOT become a system-event browser.

`timeline` is the system-event browser.

### Intent

This preserves fast shell muscle memory while giving temporal system history a real data model rather than pretending shell history already solved the problem.

# 30. Privacy, Security and Data Policy

## 30.1 Principle

Temporal retention increases privacy risk because harmless current-state facts become a behavioral history when persisted.

The default must therefore be conservative.

## 30.2 File permissions

The local ledger directory MUST be user-private.

Reference Linux permissions:

```text
~/.local/share/ono/temporal/      0700
ledger.sqlite3                    0600
```

## 30.3 No secret values

Values typed as `Secret` MUST be redacted before persistence.

Environment variables are not bulk-recorded.

Command outputs are not bulk-recorded.

File contents are not recorded.

## 30.4 Process command lines

Process command lines may contain secrets.

The default recorder MUST retain process executable/name and identity fields required for useful topology, but SHOULD NOT persist full raw argv unless a provider has already produced a redacted safe representation.

A configuration option MAY enable argv persistence with a clear privacy warning, but it is disabled by default.

## 30.5 Network data

The default recorder MAY retain endpoint metadata needed for connection topology:

```text
protocol
local endpoint
remote endpoint
process identity where visible
```

It MUST NOT capture packet payloads.

## 30.6 Remote history

Remote temporal access must respect the same link authorization and capability model as current remote provider access.

A local client MUST NOT silently copy the complete remote ledger merely because it can query a remote host.

## 30.7 KUANG/11 access

Temporal access is capability-controlled.

Canonical capabilities:

```text
temporal.read.current
temporal.read.history
temporal.read.evidence
temporal.contribute.events
temporal.contribute.causality
temporal.recorder.manage
```

A plugin with current object read permission does not automatically receive historical access.

## 30.8 History deletion

The user MUST be able to clear local retained history explicitly:

```text
remove temporal-history
```

This operation is destructive and MUST use existing destructive-operation confirmation/policy semantics.

The target name `temporal-history` is canonical for v0.5.

## 30.9 Selective scope removal

v0.5 does not require selective event-by-event editing because it can destroy causal/evidence integrity.

Future selective retention policy may be added, but core v0.5 supports full local ledger removal plus retention expiry.

# 31. Persistent Storage Architecture

## 31.1 Reference store

The reference implementation MUST use SQLite in WAL mode for the local persistent temporal ledger.

Canonical path:

```text
~/.local/share/ono/temporal/ledger.sqlite3
```

This choice is normative for the reference implementation so tests and recovery behavior are deterministic. The logical temporal contracts remain independent from SQLite for future alternative backends.

## 31.2 Why SQLite

The store requires:

- transactional append;
- indexed time queries;
- bounded local deployment;
- crash recovery;
- schema migration;
- no separate daemon database dependency;
- portable inspection and repair tooling.

SQLite satisfies this without turning Ono into an observability database product.

## 31.3 Required logical tables

The physical schema MAY normalize differently, but MUST support equivalent logical sets:

```text
events
evidence
causal_links
coverage_intervals
checkpoints
checkpoint_objects
checkpoint_relations
source_sequences
actions
metadata
```

## 31.4 Event payload encoding

Typed payloads SHOULD use a versioned binary representation such as CBOR for compactness while indexed scalar metadata remains relational.

The exact encoding MUST carry a schema/version identifier.

Unknown future fields MUST be handled according to Ono schema evolution rules.

## 31.5 WAL and durability

The recorder MUST use WAL mode and transactions.

Default durability SHOULD tolerate process crashes without losing committed events.

A sudden power loss may lose the most recent unflushed interval but MUST not corrupt prior committed history.

## 31.6 Migration

Store schema migrations MUST be versioned and tested against fixtures from every shipped v0.5 store version.

Migration MUST preserve `EventId`, `EvidenceId`, `ActionId` and causal references.

## 31.7 Corruption

If corruption is detected, Ono MUST:

1. refuse to present affected history as valid;
2. identify the affected store/segment;
3. preserve current shell functionality;
4. offer diagnostic/repair guidance;
5. mark temporal coverage gaps resulting from discarded corrupt data.

The shell MUST remain usable even when the temporal store is unavailable.

## 31.8 Retention compaction

Retention cleanup MUST run in bounded background work.

Deleting expired events MUST also handle orphaned evidence/checkpoints/causal links without leaving invalid references.

## 31.9 Checkpoint cadence

Default checkpoint interval is 5 minutes, but providers MAY trigger additional checkpoints around high-significance topology changes if doing so is cheap and bounded.

Checkpoints MUST not block the interactive prompt.

# 32. Performance Requirements

## 32.1 Startup

With persistent recording disabled and no historical query executed, v0.5 MUST add less than **5 ms p95** to Ono interactive startup on the release reference environment.

Temporal storage initialization MUST be lazy when recording is disabled.

## 32.2 Current interaction

v0.5 MUST NOT materially regress the v0.4 current-state targets:

```text
interactive startup to usable prompt        < 150 ms target
basic look local cached                      < 50 ms target
near cached                                  < 50 ms target
map L0/L1 cached                             < 100 ms target
```

## 32.3 Temporal query targets

Reference targets on a ledger within default retention:

```text
timeline current place, 15m                  < 100 ms p95
changes current place, 1h                    < 150 ms p95
at recent checkpoint +/- events              < 150 ms p95
map historical L0/L1 cached                  < 150 ms p95
why with <=100 candidate events              < 200 ms p95
find event indexed predicate                 < 150 ms p95
```

Cold remote/provider-owned historical queries MAY exceed these targets but MUST remain cancellable and expose progress/state without fake animation.

## 32.4 Recorder overhead

On an ordinary idle Linux workstation with default providers, recorder CPU overhead SHOULD average below **1% of one CPU core** and memory below **100 MiB** excluding OS page cache.

These are release measurement targets, not promises for event-heavy production servers.

## 32.5 Write amplification

The recorder SHOULD batch commits within the default 2s flush interval while preserving bounded loss behavior.

High-frequency event sources MUST support aggregation/backpressure rather than unbounded queue growth.

## 32.6 Query cancellation

Long historical queries MUST be cancellable using normal Ono cancellation semantics.

Ctrl-C MUST not corrupt the ledger or leave locks held.

# 33. Configuration

v0.5 defines these canonical settings:

```text
temporal.recording.enabled = false
temporal.retention.max_age = 24h
temporal.retention.max_size = 512MiB
temporal.checkpoint.interval = 5m
temporal.flush.interval = 2s
temporal.session.max_events = 100000
temporal.timeline.default_window = 30m
temporal.timeline.default_depth = 3
temporal.why.max_candidates = 1000
temporal.remote.clock_uncertainty_warn = 100ms
temporal.record.process_argv = false
temporal.ui.show_source_tags = true
```

Settings MUST be typed, inspectable and included in machine-readable configuration metadata.

Changing retention MUST NOT retroactively resurrect expired data.

# 34. Structured Error Family

v0.5 reserves the `temporal` error family.

Canonical errors:

| Code | Name | Meaning |
|---|---|---|
| E1101 | `temporal.invalid_time` | Time selector cannot resolve unambiguously. |
| E1102 | `temporal.not_recorded` | No evidence source covers the requested time/scope. |
| E1103 | `temporal.out_of_retention` | Requested history is known to have expired. |
| E1104 | `temporal.read_only` | Mutation attempted in historical context. |
| E1105 | `temporal.present_only` | External/current-only operation attempted in historical context. |
| E1106 | `temporal.ambiguous_event` | A query resolves to multiple equally valid events. |
| E1107 | `temporal.store_unavailable` | Persistent temporal store cannot be accessed. |
| E1108 | `temporal.store_corrupt` | Ledger integrity failure detected. |
| E1109 | `temporal.permission_denied` | History/evidence exists but is not accessible. |
| E1110 | `temporal.unsupported_source` | Source cannot provide requested temporal capability. |
| E1111 | `temporal.coverage_gap` | Query requires an interval with a material unsupported gap. |
| E1112 | `temporal.clock_uncertain` | Requested strict ordering cannot be established. |
| E1113 | `temporal.recorder_not_running` | Operation requires a running recorder. |
| E1114 | `temporal.recorder_already_running` | Start requested while recorder already active where idempotency cannot absorb it. |

Unknown cause is deliberately **not** an error code.

Partial reconstruction is normally a successful result carrying partial coverage, not an error. A command may raise `temporal.coverage_gap` only when the requested operation explicitly requires completeness.

# 35. Canonical Public Schemas

v0.5 adds at least the following public schemas:

```text
ono.temporal-event/1
ono.temporal-context/1
ono.temporal-coverage/1
ono.temporal-gap/1
ono.temporal-change/1
ono.evidence/1
ono.causal-link/1
ono.causal-explanation/1
ono.action-event/1
ono.recorder-status/1
ono.temporal-source/1
```

## 35.1 `ono.temporal-event/1`

Required fields:

```text
event_id            EventId
kind                String
scope               SpatialScope
subject             SpatialRef?
related             List<SpatialRef>
source_time         Timestamp?
observed_at         Timestamp
ingested_at         Timestamp
source_sequence     UInt?
boot_id             String?
changed_fields      List<FieldChange>
evidence            List<EvidenceRef>
causal_parents      List<CausalLinkRef>
provenance          Provenance
```

## 35.2 `ono.temporal-context/1`

```text
mode                present|historical
requested           String?
resolved_at         Timestamp?
coverage            TemporalCoverageSummary?
anchor_event        EventRef?
```

## 35.3 `ono.temporal-coverage/1`

```text
scope
capability
from
until
completeness
sampling_interval
source
permission_state
```

## 35.4 `ono.temporal-change/1`

```text
change_id
kind
subject
from_time
to_time
field_changes
relation
coverage
provenance
```

## 35.5 `ono.causal-explanation/1`

Fields correspond to section 16.4 and MUST preserve causal vs correlated associations as separate arrays.

# 36. Machine-Readable Contract Set

v0.5 MUST add version-controlled registries under:

```text
docs/spec/temporal/
```

Required files:

```text
temporal.yaml
events.yaml
evidence.yaml
causality.yaml
sources.yaml
recorder.yaml
```

Commands SHOULD live in:

```text
docs/spec/commands/temporal.yaml
```

Errors remain in the shared error registry.

Schemas remain in the shared schema system.

## 36.1 `temporal.yaml`

Defines:

- context modes;
- time-selector grammar metadata;
- prompt markers;
- read-only historical policy;
- default windows;
- canonical capabilities.

## 36.2 `events.yaml`

Defines all canonical event kinds and required fields.

## 36.3 `causality.yaml`

Defines:

- causal relationship classes;
- inverse labels;
- built-in causal rules;
- required evidence strengths;
- renderer wording restrictions.

## 36.4 Drift checks

`xtask spec-check` MUST fail if:

- a stable temporal command is missing from registry;
- implementation emits an undocumented canonical event kind;
- a built-in causal rule is not registered;
- an error/schema referenced in temporal contracts does not exist;
- default configuration differs from registry;
- a provider advertises temporal capability absent from its contract metadata.

# 37. KUANG/11 Temporal Extensions

## 37.1 Purpose

KUANG/11 may extend history and causality, but Ono core retains authority over identity, evidence classes, causal labels, capability policy and rendering truth.

## 37.2 Plugin contributions

A package MAY contribute:

```text
temporal event sources
historical query providers
evidence providers
causal rules
correlation rules
temporal landmark rules
timeline views
```

## 37.3 Event contribution capability

Required capability:

```text
temporal.contribute.events
```

The host validates:

- schema;
- source identity;
- timestamps;
- scope visibility;
- referenced spatial identities;
- event size/rate limits.

A plugin cannot assert an object exists outside objects it can resolve through permitted providers.

## 37.4 Causal contribution capability

Required capability:

```text
temporal.contribute.causality
```

Third-party causal rules MUST be namespaced and MUST identify their source.

By default, plugin causal strength MUST NOT exceed `asserted` unless the host contract explicitly trusts that package/source as authoritative for a domain.

## 37.5 Historical query providers

A package may expose historical state from an external system such as:

```text
Prometheus-like metadata source
OpenTelemetry backend
container runtime archive
filesystem snapshot manager
security audit log
custom application event store
```

The provider MUST map data into canonical Ono objects/events and expose coverage/provenance.

## 37.6 Temporal views

Plugins MAY contribute alternate views but MUST consume canonical temporal schemas.

A view cannot create causality that is absent from its input.

# 38. AI and Model-Broker Integration

## 38.1 AI is a consumer, not temporal truth

v0.5 does not require AI for any core temporal feature.

A KUANG/11 assistant or future model broker MAY consume:

- temporal events;
- causal explanations;
- coverage;
- spatial context;
- evidence summaries.

## 38.2 Hypotheses

An AI may produce a hypothesis such as:

```text
The config change may have contributed to the failure.
```

This MUST be represented as:

```text
Inference {
    kind: hypothesis
    model
    inputs
    confidence
}
```

It MUST NOT become a canonical `caused_by` edge without independent registered evidence.

## 38.3 Prompt injection boundaries

Raw logs and external text used as model context are untrusted data.

The model broker MUST keep temporal evidence payloads separated from instructions and maintain the existing KUANG/11 tool/action policy.

## 38.4 Tool actions from historical context

An assistant operating while the session is historical MUST obey the same read-only policy. It cannot request mutations unless the user explicitly returns to present or uses a separately confirmed present-bound action flow.

### Intent

Structured temporal context is valuable precisely because it gives AI better evidence. AI must not be allowed to contaminate the evidence model in return.

# 39. Reference Crate Architecture

v0.5 MUST not turn `ono-cli` into the temporal engine.

The reference responsibility split is:

```text
ono-temporal-core
    event IDs and event model
    evidence
    coverage
    causal link types
    time selectors
    temporal context value types

ono-temporal-ledger
    SQLite persistence
    append/query
    retention
    migrations
    integrity
    source sequence tracking

ono-temporal-reconstruct
    checkpoints
    event application
    historical object/relation reconstruction
    coverage composition
    gap propagation

ono-temporal-query
    timeline planning
    changes
    event search
    why/causal graph planning
    relevance ranking

ono-temporal-render
    timeline text renderer
    causal explanation renderer
    full-screen timeline
    temporal HUD components

ono-recorder
    user-level collection service
    provider subscriptions
    checkpoint scheduling
    bounded buffering

ono-cli
    parses/dispatches
    owns active TemporalContext in Session
    integrates current spatial place with temporal query
    nothing more
```

## 39.1 Existing `ono-spatial-events`

`ono-spatial-events` remains responsible for live spatial change semantics and snapshot diffing.

v0.5 SHOULD add a small adapter seam from its canonical change output into `ono-temporal-core` events rather than moving provider/clock/store logic into it.

## 39.2 No clock in pure logic crates

Pure comparison/query logic SHOULD accept time as parameters rather than call the system clock directly. This preserves deterministic tests and continues the existing v0.4 design discipline.

## 39.3 No provider calls from renderer

Renderers MUST consume canonical query output and MUST NOT query providers, the ledger or the network directly.

## 39.4 No SQLite in core types

`ono-temporal-core` MUST not expose SQLite types or SQL semantics.

# 40. Temporal Source API

A conceptual provider-side interface is:

```rust
trait TemporalSource {
    fn identity(&self) -> TemporalSourceId;
    fn capabilities(&self) -> TemporalCapabilities;

    async fn historical_events(
        &self,
        scope: &SpatialScope,
        range: TimeRange,
        query: EventQuery,
    ) -> Result<EventStream>;

    async fn historical_snapshot(
        &self,
        scope: &SpatialScope,
        at: Timestamp,
    ) -> Result<Option<HistoricalSnapshot>>;

    fn coverage(&self, query: CoverageQuery) -> CoverageResult;
}
```

The exact Rust trait may differ, but these responsibilities are normative.

Live current events may continue using existing provider/watch APIs and be normalized into temporal events by integration code.

# 41. Causal Rule API

Conceptual rule interface:

```rust
trait CausalRule {
    fn id(&self) -> CausalRuleId;
    fn relation(&self) -> CausalRelationClass;
    fn required_evidence(&self) -> EvidenceRequirements;

    fn evaluate(
        &self,
        candidate: &EventSet,
        context: &CausalContext,
    ) -> Vec<CausalLink>;
}
```

Rules MUST be deterministic for the same canonical input.

AI-generated reasoning MUST NOT implement this trait in core v0.5.

# 42. Checkpoint Model

## 42.1 Scope

Checkpoints SHOULD be partitioned by host/scope so reconstructing one service does not require deserializing an entire federated environment.

## 42.2 Contents

A default local checkpoint SHOULD include enough canonical state to reconstruct:

- spatial identity index summaries;
- services;
- processes where visible;
- interfaces/routes;
- mounts/filesystems;
- containers;
- relevant relations;
- recorder/provider coverage metadata.

Huge unbounded sets such as all files under `/` MUST NOT be checkpointed by default.

## 42.3 Incremental storage

The implementation MAY use content-addressed or incremental encoding to reduce duplication, but logical checkpoint semantics remain immutable.

## 42.4 Checkpoint trust

Each checkpoint inherits the coverage quality of its sources. A checkpoint is not globally authoritative.

# 43. Backpressure and Event Storms

## 43.1 Bounded queues

Every event ingestion path MUST be bounded.

Unbounded channels are prohibited.

## 43.2 Overflow behavior

When an event source exceeds configured capacity, Ono MUST prefer an explicit coverage gap over pretending continuity.

Example:

```text
coverage gap
  source    linux.netlink
  reason    dropped events
  interval  14:03:12.100 .. 14:03:13.411
```

## 43.3 Aggregation

High-frequency metrics-like changes MAY be aggregated when they are not semantically relevant to exact topology.

Aggregation rules MUST be declared and must not hide object/relation lifecycle changes.

## 43.4 Recorder health landmark

A persistent recorder that is falling behind SHOULD become a temporal/system landmark visible at root `look`.

# 44. Recovery and Restart Semantics

## 44.1 Recorder restart

On restart, the recorder MUST:

1. validate store metadata;
2. restore source sequence checkpoints;
3. mark any unobserved downtime as a coverage gap;
4. take a fresh checkpoint as appropriate;
5. continue without pretending the gap was covered.

## 44.2 System reboot

A host reboot creates a new boot clock domain.

The ledger remains continuous at the wall-time layer, but monotonic/sequence semantics do not cross the boot boundary unless a provider supplies explicit continuity.

## 44.3 Ono upgrade

An Ono upgrade MUST migrate the ledger before writing new-version events.

If migration cannot complete safely, current shell operation continues with temporal persistent functionality disabled and an explicit diagnostic.

# 45. UX Details and Earned Coolness

## 45.1 Temporal state should feel physical without fake physics

The temporal interface should produce a sensation of moving through a living machine by making real state transitions manipulable.

Allowed:

```text
pause a live map
step to the previous connection event
enter a process that existed then
inspect why it appeared
return to now and see what changed
```

Forbidden:

```text
rewind sound effects by default
fake tape-scrubbing animation
random visual noise in historical mode
invented frames between unsupported states
"DECRYPTING PAST..."
```

## 45.2 Visual language

Historical state SHOULD use subtle visual distinction from present:

- timestamp always visible;
- `[PAST]`/`[PAST?]` textual marker;
- optional desaturation/dimness where color is available;
- gap boundaries rendered clearly;
- causal edges visually distinct from correlated edges.

Color MUST not be the sole carrier of meaning.

## 45.3 Causal graph rendering

Text example:

```text
restart nginx.service  @a91
        |
        | triggered
        v
systemd job 4821
        |
        | caused
        v
nginx.service activating
        |
        +---------> process/1827 gone
        |
        +---------> process/2741 appeared
        |
        v
nginx.service active
```

Correlation should use a distinct non-causal connector:

```text
nginx.conf changed  .... correlated ....  nginx.service failed
```

A renderer MUST never use the same edge style/label for both.

## 45.4 The wow interaction

A release-quality v0.5 SHOULD make this sequence feel immediate:

```text
map --live
Space
[
[
Enter
W
N
```

Meaning:

```text
watch the machine
pause it
step backward twice
enter the object at that time
ask why
return to now
```

If this requires the user to understand internal temporal storage concepts first, the UX has failed.

# 46. Documentation Requirements

v0.5 documentation MUST include:

- a conceptual introduction to evidence-backed time;
- recorder privacy/retention explanation;
- `at`/`now` quickstart;
- timeline guide;
- historical map guide;
- causal explanation guide;
- causation vs correlation examples;
- provider temporal capability matrix;
- remote clock uncertainty explanation;
- storage/retention management;
- troubleshooting for gaps and corrupted history;
- KUANG/11 temporal extension guide.

Generated command help SHOULD derive from machine-readable contracts.

# 47. Test Strategy

## 47.1 Unit tests

Required unit coverage includes:

- time selector parsing;
- DST ambiguity;
- event identity;
- field changes;
- evidence strength ordering;
- coverage composition;
- gap propagation;
- checkpoint reconstruction;
- causal rule determinism;
- correlation never becoming causation;
- retention boundaries;
- redaction;
- prompt temporal markers.

## 47.2 Property tests

Property tests SHOULD verify:

- applying an event sequence to a checkpoint is deterministic;
- replaying persisted events yields the same reconstruction as before restart;
- retention never leaves dangling causal/evidence references;
- event deduplication is idempotent;
- ordering is stable under wall-clock jumps when source sequence exists;
- serialization round-trips canonical schemas.

## 47.3 Fuzzing

Fuzz:

- temporal store decoders;
- event payload decoders;
- time selectors;
- historical adapter event input;
- KUANG/11 temporal contributions;
- causal rule candidate input;
- corrupted ledger segments/migration boundaries.

## 47.4 Integration tests

Integration tests MUST use real provider behavior where practical:

- systemd user service transition;
- process start/exit;
- network interface/route fixture namespace;
- recorder stop/start gap;
- Ono action -> systemd job -> service transition;
- remote link with controlled clock skew.

## 47.5 PTY tests

Historical context safety MUST be tested under a real PTY:

- prompt changes;
- Ctrl-C cancellation;
- entering/exiting timeline view;
- pause/rewind keys;
- terminal restoration;
- mutation refusal in historical context.

## 47.6 Container acceptance

The acceptance container MUST enable the recorder for temporal cases in a private writable home and run as an unprivileged user.

No acceptance case may rely on external Internet access.

# 48. Acceptance Scenarios

A release-quality v0.5 MUST automate at least the following scenarios.

## 48.1 Temporal context

1. `at -1m` enters historical context and visibly changes prompt.
2. `now` returns to present without changing spatial place when still valid.
3. invalid/ambiguous time does not change context.
4. a native mutation in past context returns `temporal.read_only`.
5. an arbitrary external command in past context returns `temporal.present_only`.
6. `present printf ok` executes while preserving historical context.

## 48.2 Timeline and events

7. a created process appears as a typed event.
8. the process exit appears as disappearance with the same lifetime identity.
9. timeline at a service place excludes irrelevant firehose events by default.
10. `timeline --all` exposes the wider visible scope.
11. an event reference can be inspected and used by `at event`.
12. a recorder downtime interval is rendered as a gap.

## 48.3 Reconstruction

13. a checkpoint plus later change reconstructs the correct earlier service state.
14. a PID reused later does not resolve as the historical process.
15. a relation visible now but added after `T` does not appear in `map --at T`.
16. absence is reported only when coverage can support it.
17. sparse evidence produces partial/unknown state rather than fake completeness.
18. history outside retention returns the correct diagnostic.

## 48.4 Changes

19. `changes --since` reports added/removed/changed objects with typed values.
20. a partial before-state is shown as unknown, not zero/empty.
21. v0.4 `look` recent changes uses the canonical temporal engine.

## 48.5 Causality

22. an Ono `restart service` action creates an ActionId and linked systemd job evidence.
23. `why` on the resulting service transition returns a causal chain.
24. a config file change shortly before failure appears only as correlation unless a rule/source proves causality.
25. a preceding unrelated event never appears under `known cause`.
26. unknown cause is a successful typed explanation with `cause: null`.
27. every causal edge names its rule/source/evidence.

## 48.6 Rewind UX

28. `map --live` can be paused without stopping real event ingestion.
29. stepping backward changes the visible topology to the previous significant event.
30. stepping into a coverage gap shows the gap instead of a fabricated map.
31. Enter on a past map node enters that historical place.
32. `N` returns to present and projects current topology.
33. terminal resize during temporal TUI does not change semantic time/place.

## 48.7 Recorder

34. recorder is disabled by default.
35. `start recorder` creates a private ledger with correct permissions.
36. recorder restart marks its downtime as a gap.
37. retention expires old data without dangling references.
38. Secret values do not appear in ledger bytes/queries.
39. corrupted ledger disables affected temporal persistence without breaking the shell.

## 48.8 Remote/distributed

40. remote source time and local ingest time remain distinct.
41. clock skew alone never creates a causal edge.
42. explicit cross-host transaction evidence may create a causal chain.
43. a remote host with no history reports no history rather than using current state as past state.

## 48.9 KUANG/11

44. a plugin without `temporal.read.history` cannot query retained history.
45. a plugin without `temporal.contribute.causality` cannot add causal links.
46. a plugin correlation remains correlation in core rendering.
47. contributed events outside permitted spatial scope are rejected.

# 49. Performance Acceptance

Release evidence MUST include measurements for:

```text
startup with temporal disabled
recorder idle overhead
timeline 15m query
changes 1h query
recent reconstruction
historical map L1
why query
retention cleanup under load
```

Performance tests SHOULD run against a deterministic fixture ledger with at least:

```text
1,000,000 events
100,000 objects/lifetimes
500,000 relation changes
10,000 action records
```

The purpose is to catch architectural scaling failures, not to claim production observability scale.

# 50. Implementation Sequence

The sequence below targets the complete v0.5 contract. It is not an MVP sequence.

## Phase T1 - Temporal contracts

Deliver:

- temporal error family;
- time selector types;
- EventId/EvidenceId/ActionId;
- event/evidence/coverage schemas;
- causal relationship registry;
- machine-readable temporal registries;
- configuration metadata.

Gate:

```text
the full temporal vocabulary exists as machine-readable contracts
with spec-check drift enforcement before persistence or UI code
```

## Phase T2 - In-memory ledger and session context

Deliver:

- bounded in-memory event ledger;
- active TemporalContext;
- `at` and `now`;
- historical prompt/HUD;
- read-only policy;
- `present` escape for external commands.

Gate:

```text
a session can move to a supported in-memory past state and cannot mutate the present accidentally
```

## Phase T3 - Canonical event bridge

Deliver:

- normalize v0.4 spatial live changes into temporal events;
- provider event mapping;
- action event lifecycle;
- source sequence/coverage markers;
- event references.

Gate:

```text
real current system changes and Ono actions enter one typed event model
```

## Phase T4 - Persistent ledger and recorder

Deliver:

- SQLite WAL store;
- migrations;
- retention;
- private permissions;
- recorder user service;
- start/stop/status commands;
- crash recovery;
- gap creation on downtime.

Gate:

```text
events survive shell restart under bounded retention without hidden privilege or secret leakage
```

## Phase T5 - Checkpoints and reconstruction

Deliver:

- checkpoint scheduler;
- canonical checkpoint projections;
- reconstruction engine;
- coverage composition;
- historical object metadata;
- lifetime-safe identity replay.

Gate:

```text
get/look/near can answer a recent historical point using evidence-backed state and explicit coverage
```

## Phase T6 - Timeline and changes

Deliver:

- `timeline`;
- `find event`;
- `changes`;
- timeline renderer;
- gaps;
- event filtering;
- v0.4 `look` recent-change integration.

Gate:

```text
the user can discover what happened without already knowing exact timestamps or event IDs
```

## Phase T7 - Historical spatial world

Deliver:

- historical `map`;
- historical exits/relations;
- historical `find place`;
- entering objects at past time;
- tombstone history integration;
- current/past index separation.

Gate:

```text
the user can move through a supported past topology without current-state leakage
```

## Phase T8 - Causal engine

Deliver:

- causal rule runtime;
- action/provider causal token bridge;
- `why`;
- correlation/precedes separation;
- causal graph renderer;
- evidence inspection.

Gate:

```text
known causal chains can be explained and plausible-but-unproven events remain visibly non-causal
```

## Phase T9 - Temporal TUI and rewind

Deliver:

- pause/rewind in live map;
- event stepping;
- full-screen timeline;
- map at cursor;
- return-to-now changes summary;
- TUI liveness/resize tests.

Gate:

```text
a user can pause a live machine, step backward through real events, inspect the past and return to now
```

## Phase T10 - Linux history sources

Deliver:

- systemd D-Bus event fidelity;
- journald historical provider/adapter path;
- netlink temporal mapping;
- process snapshot-diff coverage;
- mount/container event integration;
- source capability matrix.

Gate:

```text
temporal behavior on a normal Linux host uses the strongest available native sources and states their limitations
```

## Phase T11 - Remote and KUANG/11

Deliver:

- remote temporal capability negotiation;
- source/ingest/uncertainty clocks;
- partial-order handling;
- KUANG temporal capabilities;
- plugin event/causal validation;
- remote historical maps.

Gate:

```text
history can cross extension and host boundaries without inventing ordering, authority or access
```

## Phase T12 - Hardening and release proof

Deliver:

- million-event performance fixtures;
- fuzzing;
- privacy review;
- migration fixtures;
- corruption recovery;
- documentation;
- complete container acceptance;
- dogfooding fixes;
- release check.

Gate:

```text
release-check passes and all temporal acceptance claims have named automated proof
```

# 51. Spec-Driven Work Packages

A coding agent SHOULD be able to derive work items such as:

```text
TEMP-001  TimeSelector parser and DST ambiguity
TEMP-002  TemporalContext and prompt state
TEMP-003  Historical mutation guard
TEMP-004  present external execution escape
TEMP-005  EventId/EvidenceId/ActionId semantic types
TEMP-006  TemporalEvent schema and registry
TEMP-007  Coverage model and composition
TEMP-008  TemporalGap model
TEMP-009  v0.4 spatial-change -> event bridge
TEMP-010  action lifecycle events

LEDG-001  SQLite schema v1
LEDG-002  WAL initialization and private permissions
LEDG-003  append transaction
LEDG-004  indexed range query
LEDG-005  event deduplication
LEDG-006  retention age policy
LEDG-007  retention size policy
LEDG-008  migration harness
LEDG-009  integrity/corruption handling
LEDG-010  source sequence continuity

REC-001   recorder process/service
REC-002   bounded ingestion queue
REC-003   provider subscription bridge
REC-004   checkpoint scheduler
REC-005   downtime gap
REC-006   start recorder
REC-007   stop recorder
REC-008   get recorder
REC-009   redaction policy

RECON-001 checkpoint format
RECON-002 checkpoint nearest-before query
RECON-003 event replay
RECON-004 field coverage propagation
RECON-005 relation reconstruction
RECON-006 lifetime identity replay
RECON-007 unknown interval semantics
RECON-008 historical collection completeness

QUERY-001 timeline planner
QUERY-002 timeline current-place relevance
QUERY-003 event references
QUERY-004 find event
QUERY-005 changes engine
QUERY-006 historical provider merge
QUERY-007 historical map projection
QUERY-008 historical find place

CAUSE-001 causal relation registry
CAUSE-002 causal rule interface
CAUSE-003 Ono action -> provider transaction rule
CAUSE-004 systemd job causality
CAUSE-005 process creation causality
CAUSE-006 correlation rule representation
CAUSE-007 why target
CAUSE-008 why event
CAUSE-009 why field
CAUSE-010 causal explanation renderer

TUI-001   temporal HUD
TUI-002   pause live map cursor
TUI-003   significant event stepper
TUI-004   coverage gap frame
TUI-005   full-screen timeline
TUI-006   map at selected event
TUI-007   return-to-now summary

LINUX-001 journald historical source
LINUX-002 systemd live event mapping
LINUX-003 netlink temporal event mapping
LINUX-004 procfs snapshot-diff policy
LINUX-005 mount checkpoint mapping
LINUX-006 container event source mapping

DIST-001  remote temporal capability negotiation
DIST-002  source vs ingest timestamp protocol
DIST-003  clock uncertainty
DIST-004  partial-order model
DIST-005  cross-host causal evidence

K11T-001  temporal capability IDs
K11T-002  event contribution API
K11T-003  causal rule contribution API
K11T-004  history provider API
K11T-005  testhost fixtures
K11T-006  scope/capability enforcement

TEST-001  temporal contract fixture generation
TEST-002  deterministic clock harness
TEST-003  million-event fixture generator
TEST-004  migration matrix
TEST-005  PTY past-mode safety
TEST-006  causality falsification suite
TEST-007  privacy byte-scan fixture
```

Every work package MUST link back to normative section IDs or machine-readable registry entries.

# 52. Dogfooding Scenarios

Automated tests are necessary but not sufficient for a shell whose value is experiential.

Before v0.5 is considered product-complete, a human dogfooding pass SHOULD include these scenarios.

## 52.1 Service restart exploration

1. start recorder;
2. enter a real user service;
3. restart it through Ono;
4. observe live map changes;
5. wait several minutes;
6. open timeline;
7. rewind to before restart;
8. enter old process identity;
9. ask `why` about new process;
10. return to now.

Success is not merely correctness. The flow should feel coherent and require no mental jump into a separate logging model.

## 52.2 Unknown-cause honesty

Create a failure where evidence is deliberately incomplete.

`why` must resist producing a plausible fake cause.

The operator should be able to understand what evidence is missing.

## 52.3 Recorder gap

Stop recorder, change the system, restart recorder.

The historical experience must make the missing interval obvious and must never imply exact reconstruction across it.

## 52.4 Remote skew

Use two linked test hosts with artificial wall-clock skew.

A human should be able to understand that event order is uncertain without needing distributed-systems expertise.

# 53. Resolved Design Decisions

The following decisions are resolved by this specification and MUST NOT be reopened by implementation agents as product questions.

| Question | Decision |
|---|---|
| Is time merely a set of `--since` flags? | No. Time is a first-class session coordinate. |
| What command enters the past? | `at <time-selector>`. |
| What command returns to present? | `now`. |
| Can the past mutate the present? | No. Historical context is read-only. |
| Can external programs run while in the past? | Only explicitly through `present <command...>`. |
| Is persistent recording automatic? | No. It is opt-in and disabled by default. |
| Default retained history after enabling recorder? | 24h or 512 MiB, whichever bound removes data first. |
| Persistent store? | SQLite WAL at the canonical user-private path. |
| Are command outputs stored? | No, not by default. |
| Are raw file contents stored? | No. |
| Are packet payloads stored? | No. |
| Does missing evidence mean object absent? | No. |
| Can a snapshot prove an interval? | No; it is a point sample unless source semantics are stronger. |
| Does temporal proximity imply cause? | Never. |
| Is unknown cause an error? | No; it is a valid explanation result. |
| Can AI create canonical causal edges? | No, not without independent registered evidence. |
| Are historical objects new object types? | No. Canonical objects carry temporal metadata. |
| Does `back` travel backward in time? | No. `back` remains spatial trail navigation. |
| Can current v0.4 maps be reused for history? | Yes, but only through reconstructed historical input; no current-state leakage. |
| Does v0.5 require eBPF/root? | No. |
| Can plugins contribute temporal data? | Yes, through explicit KUANG/11 temporal capabilities. |
| Can remote wall clocks establish causality? | No. |
| What makes rewind meaningful? | Significant evidence-backed events, not generated animation frames. |
| Is Ono becoming a monitoring database? | No. Bounded event/state history only. |
| Is the old v0.2 timeline MAY enough? | No. v0.5 defines a new typed system-event timeline with independent semantics. |

# 54. Explicit Non-Goals

v0.5 MUST NOT attempt to become:

- a general-purpose time-series metrics database;
- a replacement for Prometheus, Loki, Elasticsearch, Splunk or a SIEM;
- an unlimited audit archive;
- a packet capture platform;
- a filesystem version-control system;
- a deterministic replay system for arbitrary Unix processes;
- a virtual machine snapshot/restore product;
- a debugger capable of rewinding program execution;
- an omniscient incident root-cause engine;
- an AI-generated explanation system pretending hypotheses are facts;
- a distributed tracing backend by itself;
- a reason to require privileged eBPF on normal installations;
- a reason to hide gaps or weak evidence for visual continuity.

v0.5 also MUST NOT change the core truth that Ono remains a shell. Full-screen timeline and rewind are deliberate views that the user can exit cleanly.

# 55. Failure Modes to Avoid

## 55.1 Pretty log viewer

If `timeline` is just `journalctl` with nicer colors, v0.5 has failed.

The event model must unify real typed system changes, actions, topology and evidence across sources.

## 55.2 Fake historical map

Rendering today's graph with an old timestamp is prohibited.

## 55.3 Causal storytelling

A renderer or AI that says "the config change caused the outage" because it happened first violates the core contract.

## 55.4 Recording everything

Persisting all stdout, logs, environment and filesystem content to make history easier violates the product boundary and privacy model.

## 55.5 Silent gaps

A recorder restart or dropped event stream that is not represented as a coverage gap destroys operator trust.

## 55.6 Global wall-clock ordering

Sorting remote events by timestamp and then treating that order as causal truth is prohibited.

## 55.7 Temporal logic in `ono-cli`

Building the ledger, reconstruction and causal engine directly into the CLI integration crate is prohibited by the architecture split.

## 55.8 AI required for `why`

If `why` cannot explain known systemd/Ono action causality without a model configured, the core design has failed.

## 55.9 Past mode that is only cosmetic

If `at -10m` changes the prompt but `look`/`map` still show present objects, the feature is invalid.

# 56. Release Definition

v0.5 is release-ready only when all of the following are true:

1. every normative temporal command is machine-registered;
2. `at` and `now` work in interactive and non-interactive tests;
3. historical mode is reliably read-only;
4. current shell behavior remains functional when temporal persistence is disabled;
5. recorder is opt-in, bounded and private;
6. retained events survive restart and migration tests;
7. gaps and partial coverage remain visible;
8. historical reconstruction is identity-safe;
9. `timeline` is typed and pipeline-compatible;
10. `changes` is typed and coverage-aware;
11. v0.4 historical maps contain no known present-state leakage;
12. causal relationships are registered and evidence-backed;
13. correlation is never rendered as causation;
14. `why` can return unknown honestly;
15. Ono action causality is retained and inspectable;
16. rewind works over real events and handles gaps;
17. remote clock uncertainty is preserved;
18. KUANG/11 temporal permissions are enforced;
19. secret/privacy acceptance cases pass;
20. million-event fixture performance is within documented targets or explicit release budgets;
21. corruption and store-unavailable behavior do not break current shell use;
22. all release acceptance boxes have named automated proof;
23. `scripts/release-check.sh` is green.

# 57. End-to-End Reference Interaction

The following interaction is illustrative of the complete intended product behavior. It is not merely marketing copy; each transition corresponds to normative semantics defined above.

```text
local:// > start recorder

recorder
  state       running
  retention   24h / 512 MiB
  store       ~/.local/share/ono/temporal/ledger.sqlite3

local:// > enter compute
local/compute:// > map --live
```

The live map shows real current topology. nginx restarts and its worker changes.

The user presses `Space`.

```text
PAUSED @14:03:13.002
```

Presses `[` twice.

```text
@14:03:12.108
nginx worker/1827 disappeared
```

Presses `[` again.

```text
@14:03:11.401
nginx.service active -> deactivating
```

The map now reflects that historical topology.

The user focuses nginx and presses Enter.

```text
local/compute/service/nginx:// @14:03:11.401 [PAST] >
```

Then:

```text
> timeline --since 30s

14:03:06.100  @e88  /etc/nginx/nginx.conf changed
14:03:11.002  @e91  restart nginx.service requested     [ono]
14:03:11.017  @e92  systemd job 4821 created            [systemd]
14:03:11.401  @e93  nginx.service deactivating          [systemd]
14:03:12.108  @e94  process/1827 disappeared
14:03:12.410  @e95  process/2741 appeared
14:03:13.002  @e96  nginx.service active                [systemd]
```

The user asks:

```text
> why event @e95

process/2741 appeared at 14:03:12.410

known cause
  nginx.service started a replacement worker

chain
  action @a91: restart nginx.service
        |
        | triggered
        v
  systemd job 4821
        |
        | caused
        v
  nginx.service activating
        |
        | caused
        v
  process/2741 appeared

correlated
  /etc/nginx/nginx.conf changed 6.3s before restart
  correlation only - no evidence that this change caused the restart

evidence
  Ono ActionResult      authoritative for operator action
  systemd job identity authoritative for unit transaction
  process membership   derived from systemd/procfs

coverage
  service lifecycle    complete for interval
  process lifecycle    partial (snapshot/event combination)
```

The user then types:

```text
> restart service nginx
```

Ono refuses:

```text
temporal.read_only
`restart service` cannot execute while observing historical state

return to the present:
  now
```

The user returns:

```text
> now

returned to now
  process/2741 still running
  nginx.service active
  +7 connections since historical cursor

local/compute/service/nginx:// >
```

This is the intended v0.5 experience:

> **The user does not read a post-mortem about the machine. The user moves through the machine's supported history, inspects the evidence, and returns to the live system.**

# 58. Final Product Principle

The v0.5 implementation should be judged against one final question:

> **Does time become as tangible and navigable as space became in v0.4, without sacrificing Ono's honesty about what the machine actually revealed?**

If the answer is yes, Ono-Sendai is no longer merely a shell that can describe the current system.

It is a systems interface in which:

```text
objects have structure,
Unix tools can gain structure,
structure has topology,
topology changes through time,
and change can be explained only as far as evidence permits.
```

That is the complete intent of ONO-SENDAI v0.5.
