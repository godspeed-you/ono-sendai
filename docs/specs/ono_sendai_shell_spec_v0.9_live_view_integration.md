---
title: "ONO-SENDAI Specification v0.9"
subtitle: "Live View Integration & Long-Running Workspace Ergonomics"
author: "Project Specification"
date: "2026-09-01"
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
header-includes:
  - |
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

# ONO-SENDAI Specification v0.9
## Live View Integration & Long-Running Workspace Ergonomics

**Status:** Product and architecture extension specification  
**Scope:** Integration of existing `Stream<T>`, `watch`, job-control, spatial-live and temporal semantics into the v0.8 Deck; follow/browse ergonomics; bounded view retention; redraw scheduling; continuity presentation; long-running resource and failure hardening  
**Relationship:** Standalone extension to the published Ono-Sendai baseline and the v0.3-v0.8 extension specifications  
**Normative language:** MUST, MUST NOT, SHOULD, SHOULD NOT, MAY

> **v0.9 does not teach Ono how to be live. Ono is already live. v0.9 teaches the Deck how to remain useful, honest and responsive while existing live semantics continue for minutes or hours.**

---

# 0. Document Status and Relationship to Earlier Specifications

## 0.1 Standalone additive specification

This document is the independent specification for Ono-Sendai v0.9.

It does not replace, merge, rewrite, regenerate or retrospectively modify the v0.2 baseline or the v0.3-v0.8 extension specifications.

The cumulative progression relevant to this release is:

```text
v0.2  Values, Stream<T>, pipelines, backpressure, cancellation,
      watch, foreground/background jobs, adaptive live TTY rendering,
      constrained views and view lifecycle.

v0.4  Spatial identity and topology, including map --live through the
      existing live/watch path.

v0.5  Observations, TemporalEvent, evidence, coverage, gaps, temporal
      cursor, pause/rewind and one live/historical truth model.

v0.6  Prospective change, protection and recovery semantics.

v0.7  Presentation consolidation and Rich TTY hardening without a
      second UI ontology.

v0.8  Deck workspace composition and generic terminal ownership while
      reusing existing views, values, history and live streams.

v0.9  Long-running live-view integration inside that workspace without
      a second live-data model.
```

Earlier specifications remain authoritative for the concepts they define.

In particular, v0.9 inherits without replacement:

- `Value` and `Stream<T>`;
- finite vs unbounded stream behavior;
- pipeline backpressure and cancellation;
- `watch` command semantics;
- provider subscription and explicit polling metadata;
- shell job control and background jobs;
- `ValueRef`, `ObjectRef`, `ResultRef` and `HistoryEntry`;
- v0.2 constrained view trees and view lifecycle;
- v0.4 stable spatial identity and `map --live`;
- v0.5 `Observation`, `TemporalEvent`, evidence, temporal coverage, gaps, recorder and temporal cursor;
- v0.5 live-map pause/rewind behavior;
- v0.7 presentation/capability resolution;
- v0.8 Deck host, primary/auxiliary composition and terminal lease;
- KUANG/11 stream, view and quota contracts already present in v0.2.

## 0.2 This specification replaces the abandoned v0.9 design direction, not an earlier published contract

A previous internal design direction for v0.9 considered introducing concepts such as:

```text
Live<T>
StateObservation<T>
EventObservation<T>
StreamGeneration
Watermark
LiveExecution lifecycle
new gap/backpressure contracts
```

That direction is rejected by this specification.

The reason is architectural, not cosmetic: v0.2 and v0.5 already define the semantic foundations those concepts attempted to cover.

The implementation MUST NOT introduce those abandoned concepts merely because an earlier draft or prototype used them.

If code from such a prototype exists, it MUST be reviewed as migration/deletion work rather than treated as an implicit contract.

## 0.3 Inheritance-first rule

v0.9 MUST NOT create a new semantic type, lifecycle, temporal envelope or queue policy when an earlier specification already owns the concept.

A new v0.9 abstraction is justified only if it is all of the following:

1. presentation-local or Deck-host-local rather than domain semantic;
2. necessary to keep an existing live view usable over time;
3. impossible to express cleanly through the existing view lifecycle or job/stream contracts;
4. explicitly bounded in lifetime and memory;
5. removable without changing command/provider/pipeline semantics.

The preferred implementation shape is therefore:

```text
existing semantic stream/job/view
              |
              v
small presentation-local binding/policy
              |
              v
existing v0.8 Deck host
```

not:

```text
existing stream
     |
     v
new live runtime
     |
     v
new observation model
     |
     v
new live renderer
```

## 0.4 What is genuinely new in v0.9

v0.9 introduces only the missing long-running **presentation integration** around live views.

The release may add internal host concepts equivalent to:

- a binding between an existing executing stream/job and an existing mounted view;
- a view-local distinction between following newest display state and browsing retained display history;
- a bounded display-retention policy;
- a redraw scheduler that decouples semantic update rate from terminal frame rate;
- continuity/currentness presentation derived from existing source/job/temporal facts;
- resource accounting for long-lived mounted views.

These are presentation/runtime integration mechanisms.

They MUST NOT become new public Ono data types merely because implementation structs need names.

## 0.5 What v0.9 explicitly does not add

v0.9 does **not** add:

- `Live<T>` or `Observable<T>`;
- a new state/event ontology;
- a new Observation envelope;
- a new gap type;
- a new timestamp hierarchy;
- a new backpressure algorithm for the semantic pipeline;
- a new reconnect protocol;
- a new provider subscription API if the existing provider/KUANG contracts suffice;
- a trigger/automation language;
- reactive side-effect semantics;
- a dashboard builder;
- arbitrary user-created panes;
- a time-series database;
- persistent metrics retention;
- a second recorder;
- a second job-control lifecycle;
- a plugin-specific live-widget API;
- a generic charting subsystem as a release requirement;
- implicit monitoring/alerting behavior.

## 0.6 No retrospective editing

Earlier specifications MUST NOT be edited merely to make v0.9 easier to implement.

If v0.9 exposes an ambiguity between v0.2 and v0.5 - for example, an old generic `ObjectEvent` envelope versus the later canonical temporal event model - an ADR MUST identify the authoritative concept for the relevant path.

The ADR SHOULD prefer the newer, more semantically complete contract where the earlier one was explicitly exploratory, but it MUST NOT silently rename both into a third concept.

## 0.7 Complexity budget

The complexity budget for v0.9 is intentionally strict.

At architecture review, every new persistent state field SHOULD be classifiable as one of:

```text
view cursor / follow state
bounded display buffer metadata
render scheduling metadata
existing job/stream/view reference
existing semantic continuity/coverage reference
```

A design that requires a new domain cache, event ledger, subscription graph, semantic clock model or action engine is outside v0.9.

The release passes the **subtraction test** only if the old v0.9 `Live<T>`-style machinery is unnecessary after implementation.

## 0.8 Release thesis

v0.9 is built around three statements:

> **Integrate duration, do not reinvent liveness.**

> **The view may fall behind the screen; semantic truth must not fall behind the view.**

> **Presentation can be lossy in frames, never dishonest in meaning.**

All three are normative.

# 1. Product Thesis

## 1.1 Ono already has live semantics

The baseline already defines `watch` as a stream producer and requires native pipelines to implement backpressure and cancellation.

It already permits a live query to become a background job.

It already states that TTY renderers SHOULD update stable objects in place.

v0.4 already makes topology live.

v0.5 already connects live and historical behavior into one temporal model and explicitly defines pause/rewind for the live map.

Therefore the product problem remaining in v0.9 is not:

> How should live data work?

It is:

> How should an interactive workspace host an existing live stream for a long time without becoming visually unstable, semantically misleading, resource-hungry or difficult to navigate?

## 1.2 Duration changes UX even when semantics do not change

A static table exists for a moment.

A live view may exist for hours.

That introduces presentation concerns that ordinary one-shot output does not have:

- the user may scroll away from the newest values;
- new data may continue while the user inspects older visible content;
- the screen may not be able to repaint as quickly as data arrives;
- rows may appear, disappear and reorder;
- terminal resize may happen many times;
- a remote link may degrade and recover;
- the Deck may temporarily hand the terminal to `vim`, `less`, `ssh` or another child;
- the user may suspend and resume Ono;
- retained view history must remain bounded;
- a view can look current even when its source stopped updating;
- a long-running view must not quietly become a hidden recorder.

v0.9 solves those problems at the presentation boundary.

## 1.3 The Deck is not a monitoring product

The Deck may display live CPU values, logs, process tables, network state and topology.

That does not make it a monitoring platform.

v0.9 MUST resist features whose primary purpose is unattended monitoring rather than interactive operation.

Out of scope examples:

```text
alert rules
notification routing
metric retention for days
SLO evaluation
multi-host dashboard grids
scheduled reports
persistent graph panels
long-term downsampling
```

If a future product direction wants those capabilities, it requires a separate product-level decision.

## 1.4 The Deck is not a terminal Grafana

A live workspace is tempting to turn into a free-form dashboard.

v0.9 MUST not do that.

The canonical composition remains v0.8:

```text
primary view
optional auxiliary
command surface
status line
```

v0.9 MAY make the primary view excellent at being live.

It MUST NOT turn every command into a tile or add pane persistence solely to keep multiple graphs visible.

## 1.5 Long-running interaction must remain shell-like

The user should still think in commands and values:

```text
watch process --every 1s
watch service nginx &
map --live
get job
```

The Deck can make those operations easier to observe and revisit, but essential semantics MUST remain expressible in plain command form.

No live view may require an invisible mouse-only action to continue, cancel, serialize or reproduce the underlying operation.

## 1.6 Pause is a view concept unless an earlier semantic contract says otherwise

For a generic live table or append view, pausing/browsing the screen MUST NOT silently pause the provider, pipeline or recorder.

The default generic behavior is:

```text
source continues
pipeline continues
view state continues to ingest according to existing semantics
screen cursor may stop following newest content
```

For specialized views where an earlier specification already defines pause semantics, that earlier behavior remains authoritative.

Most importantly, v0.5 `map --live` pause freezes the temporal cursor of the displayed view while the real system, providers and recorder continue.

v0.9 MUST reuse that rule rather than replace it with a generic pause implementation.

## 1.7 A slow screen is not a slow machine

Terminal rendering may be limited to tens of frames per second while a provider produces thousands of updates per second.

The implementation MUST separate:

```text
semantic update processing
        from
terminal repaint frequency
```

Skipping redundant terminal frames is allowed.

Skipping semantic events is allowed only when the inherited stream/provider policy explicitly permits coalescing or loss.

A renderer MUST NOT silently decide that correctness-sensitive events are disposable merely because they arrived faster than the terminal could repaint.

## 1.8 Emotional thesis

The intended feeling is **a machine that stays alive under your hands without demanding your attention**.

A good v0.9 session should feel:

- stable rather than frantic;
- current without pretending certainty;
- easy to leave and return to;
- fast while updates are flowing;
- bounded rather than memory-hungry;
- inspectable rather than magical.

The visual experience may be impressive because real state changes continuously.

Fake activity, synthetic pulses and decorative update animation remain anti-Ono.

# 2. Core Invariants

The following invariants are non-negotiable.

1. **`Stream<T>` remains the live pipeline abstraction.** v0.9 MUST NOT add `Live<T>`.
2. **Existing backpressure remains authoritative.** The semantic pipeline owns flow control; the renderer does not invent a parallel queue discipline.
3. **Existing cancellation remains authoritative.** Closing/unmounting a view MUST map to existing job/stream/view cancellation rules rather than a new cancellation state machine.
4. **Existing job control remains authoritative.** Foreground/background semantics do not change merely because a live view is mounted in Deck.
5. **Existing temporal truth remains authoritative.** `Observation`, `TemporalEvent`, coverage, evidence and gaps come from v0.5.
6. **Existing spatial truth remains authoritative.** `map --live` uses v0.4/v0.5 identity/event semantics.
7. **A view cursor is not a temporal cursor unless the specialized view says so.** Generic browsing MUST NOT change session time.
8. **Following is presentation state.** Entering/leaving follow mode MUST NOT rerun the underlying command.
9. **Browsing does not stop ingestion.** Moving away from newest visible content MUST not silently pause providers or the pipeline.
10. **Display retention is not history.** A bounded view buffer MUST NOT be exposed as if it were the v0.5 recorder, command history or durable provider history.
11. **Display truncation is not a temporal gap.** Evicting old screen rows MUST be labeled as view truncation, not `TemporalGap`, unless a real semantic gap exists.
12. **Frame coalescing is presentation-only.** It may reduce terminal writes; it MUST NOT falsify the semantic state processed by the view.
13. **Static values remain static.** v0.9 MUST NOT auto-refresh `get process` merely because it is mounted for a long time.
14. **No hidden subscriptions.** A view MUST NOT start additional provider subscriptions merely to decorate itself unless an inherited view contract explicitly requires them.
15. **No hidden persistent recording.** Mounted live views MUST NOT persist their feed across sessions by default.
16. **No implicit dashboard.** v0.9 does not add arbitrary concurrent live panels.
17. **Command semantics stay renderer-independent.** Providers and pipeline stages MUST NOT branch on Deck follow/browse state.
18. **Selection remains weak.** Updating/disappearing rows MUST NOT turn selection into an implicit target.
19. **Source continuity must be honest.** If continuity is unknown after disconnect/resume, the UI MUST say so or defer to existing coverage semantics; it MUST NOT draw an uninterrupted timeline.
20. **Foreground external tools still receive the terminal.** Live redraw work MUST stop while terminal ownership is handed off.
21. **Typing outranks repaint.** Command editor responsiveness has priority over live terminal frames.
22. **Memory is bounded.** Every view-local retention buffer MUST have explicit bounds.
23. **Long-running sessions are test targets.** Leak behavior that appears only after hours is a release blocker.
24. **Plugins keep existing contracts.** KUANG/11 does not gain a new live-widget API in v0.9.
25. **One truth, multiple projections.** Rich TTY, Deck and non-interactive sinks may present the same stream differently without changing its semantic values.

# 3. Canonical Ownership Matrix

v0.9 implementation and review MUST use the following ownership model.

| Concern | Canonical owner | v0.9 responsibility |
|---|---|---|
| typed values | v0.2 `Value` | display only |
| live pipeline | v0.2 `Stream<T>` | host existing execution |
| stream boundedness | v0.2 pipeline | respect and expose errors |
| backpressure | v0.2 pipeline / KUANG host policy | never replace |
| cancellation | v0.2 pipeline/job/view lifecycle | route user intent |
| foreground/background | v0.2 job control | present status |
| provider subscribe/poll | v0.2 provider/KUANG contracts | expose metadata if useful |
| stable object identity | v0.2/v0.4 schemas/spatial identity | preserve row/view identity |
| live spatial topology | v0.4 | host view |
| temporal event | v0.5 `TemporalEvent` | render faithfully |
| temporal coverage/gap | v0.5 | render faithfully |
| temporal pause/rewind | v0.5 specialized view | do not redefine |
| rich TTY profile | v0.7 | degrade consistently |
| Deck composition | v0.8 | integrate live view |
| terminal ownership | v0.8 `TerminalLease` concept | stop/restart redraw safely |
| view lifecycle | v0.2 | reuse |
| plugin views | v0.2 KUANG/11 | host existing contribution |
| display follow/browse | **v0.9 presentation-local** | define |
| bounded display retention | **v0.9 presentation-local** | define |
| render scheduling/coalescing | **v0.9 presentation-local** | define |
| long-running view resource budget | **v0.9 presentation-local** | define |

If an implementation proposal moves a concern from an earlier owner into a v0.9-specific subsystem, reviewers SHOULD reject it unless an ADR demonstrates why consolidation is impossible.

# 4. Terminology and Minimal New Conceptual Model

## 4.1 Live view

A **live view** is an existing Ono view whose underlying execution continues to produce or update semantic values over time.

Examples include:

```text
watch process
watch service nginx
map --live
provider/KUANG view backed by subscribe/watch
append-oriented log/timeline stream where command semantics are live
```

`LiveView` SHOULD NOT become a new public semantic value type.

It is a description of a view/execution relationship.

## 4.2 Live-view binding

The implementation MAY use an internal object conceptually equivalent to:

```text
LiveViewBinding {
    view: ViewHandle
    execution: JobRef | StreamExecutionRef
    mounted_at: Instant
    follow: FollowState
    display_retention: DisplayRetentionState
    render_schedule: RenderScheduleState
}
```

This structure is illustrative, not a public schema.

It MUST contain references to existing execution/view state rather than duplicate semantic stream contents without bounds.

It MUST NOT become serializable user data by default.

## 4.3 Follow state

Generic live views MAY have two presentation-local states:

```text
FOLLOWING
BROWSING
```

**FOLLOWING** means the viewport tracks the newest relevant display position.

**BROWSING** means the viewport is intentionally anchored away from the newest display position while new semantic updates may continue to arrive.

No third state such as `PAUSED_STREAM` is introduced by v0.9.

## 4.4 View cursor

A **view cursor** is the presentation anchor used while browsing a live view.

It may represent:

- a selected stable object identity in a table;
- an event/log row identity;
- a scroll offset relative to a bounded retained view history;
- a specialized cursor owned by an inherited view.

A generic view cursor MUST NOT alter the Ono `ContextStack` or v0.5 temporal session context.

## 4.5 Latest position

The **latest position** is the newest display-relevant state known to the mounted view.

For an append view, it is normally the newest retained record.

For a state table, it is the current materialized display projection after all semantic updates processed so far.

For a specialized map/timeline, the earlier specification defines what "now" means.

## 4.6 Unseen update count

While a generic view is `BROWSING`, the host MAY maintain a presentation-only count such as:

```text
+37 updates
```

This counter is for orientation only.

It MUST NOT claim the number of semantic source events if the underlying view semantics coalesce or summarize updates.

Where exact event count is not known, the label SHOULD be generic (`updates`) rather than (`events`).

## 4.7 Display retention

**Display retention** is bounded in-memory state kept solely to let a user inspect recent content already consumed by a mounted view.

It is not:

- a provider history API;
- the v0.5 recorder;
- command `HistoryEntry` retention;
- a `ResultRef` guarantee;
- a monitoring database.

## 4.8 View truncation

**View truncation** occurs when display-retention limits evict older presentation data.

The UI MUST distinguish it from source loss.

Example:

```text
--- older rows no longer retained by this view ---
```

This message means only that the Deck cannot browse further back in its local display buffer.

It MUST NOT be rendered as:

```text
coverage gap
```

unless v0.5 semantic evidence actually reports one.

## 4.9 Render coalescing

**Render coalescing** combines multiple already-processed display invalidations into fewer terminal frames.

For example:

```text
semantic updates received: 120 in 100ms
view state transitions:     applied according to canonical semantics
terminal frames written:    3
```

This is allowed.

Dropping 117 correctness-sensitive semantic events before the view sees them merely to obtain three frames is not render coalescing; that is stream loss and remains governed by the inherited stream/provider policy.

## 4.10 Currentness indicator

A **currentness indicator** is presentation derived from existing facts such as:

- execution is running/completed/failed;
- timestamp of last processed update;
- provider/link state;
- v0.5 coverage/gap information;
- explicit provider freshness metadata if such metadata exists.

v0.9 MUST NOT introduce a universal semantic `stale: Bool` field.

## 4.11 Specialized live view

A **specialized live view** is one whose earlier specification defines domain-specific cursor, pause, rewind or continuity semantics.

Examples:

- v0.5 live temporal map;
- v0.5 timeline/rewind experiences;
- a KUANG/11 view with an already-valid constrained view contract.

Generic v0.9 follow/browse behavior MUST yield to the specialized contract where the two overlap.

# 5. Architecture: Bind Existing Execution to Existing Views

## 5.1 Required layering

The reference architecture SHOULD resemble:

```text
provider / native command / KUANG command
                  |
                  v
          existing Stream<T>
                  |
                  v
       existing pipeline + job control
                  |
                  v
           existing ViewHandle
                  |
                  +-------------------+
                  |                   |
                  v                   v
            Rich TTY host        v0.8 Deck host
                                      |
                                      v
                              v0.9 live-view binding
                                      |
                                      v
                         bounded display + redraw policy
```

The v0.9 layer sits after semantic execution.

## 5.2 No provider awareness of follow state

Provider APIs MUST NOT receive parameters such as:

```text
ui_following=true
user_scrolled_up=true
deck_frame_rate=20
```

unless an earlier semantic command already exposes a user-requested provider behavior such as `--every` or subscription policy.

Viewport navigation is not provider semantics.

## 5.3 No evaluator awareness of Deck frame rate

The evaluator and pipeline MUST NOT change transformation semantics to satisfy terminal repaint frequency.

A `where`, `select`, `take`, windowed transform or provider source behaves the same whether its output is:

- serialized;
- rendered in Rich TTY;
- mounted in Deck;
- consumed by KUANG/11.

## 5.4 View updates happen after semantic processing

A view may maintain bounded presentation state after values leave the pipeline stage that feeds it.

The conceptual order is:

```text
source value/event
      |
pipeline semantics
      |
view semantic update
      |
display model invalidation
      |
render scheduler
      |
terminal frame
```

This ordering prevents terminal frame skipping from becoming semantic event skipping.

## 5.5 One source, one execution owner

Mounting the same live execution in Deck MUST NOT automatically create a second provider subscription.

If the user explicitly starts two `watch` commands, there may be two executions according to normal semantics.

But one execution appearing in jobs/history/primary view should reference the same underlying execution where the view lifecycle permits.

## 5.6 No view-owned domain truth

A live table may materialize enough state to render stable rows.

That materialization MUST remain view-private presentation state.

It MUST NOT become the canonical process/service/network cache used by unrelated commands.

`get process` must still ask the normal provider path; it MUST NOT read whichever rows happen to be visible in `watch process`.

## 5.7 No hidden fan-out

If multiple display components need information about the same execution, the host SHOULD share bounded presentation metadata rather than clone the semantic stream.

Examples:

```text
primary view  <- same binding -> status line
jobs aux      <- existing job registry, not copied stream
```

## 5.8 Existing view lifecycle remains authoritative

The v0.2 lifecycle remains:

```text
created -> mounted -> focused <-> background -> closed
```

v0.9 adds no parallel `CONNECTING/LIVE/DEGRADED/...` view lifecycle.

Source/job status may of course expose those facts if an existing provider/link/runtime does so; the view renders them without adopting them as its own lifecycle states.

# 6. Live View Eligibility and Resolution

## 6.1 A live view must come from real live semantics

The host may treat output as live only when the underlying command/view/execution contract indicates continued updates.

Examples:

```text
watch process                 yes
map --live                    yes
watch service nginx &         yes
tail/follow command if defined as live stream  yes
get process                   no
get process | format table    no
```

The Deck MUST NOT infer liveness merely because a displayed object contains timestamps or rapidly changing fields.

## 6.2 Static command auto-refresh is forbidden

This is invalid v0.9 behavior:

```text
user runs: get process
Deck secretly repeats get process every second
```

If the user wants continued observation, the user uses existing live semantics such as `watch` or an explicitly live provider view.

## 6.3 Presentation resolution

The v0.7 `PresentationDescriptor`/existing render-hint path remains authoritative for choosing an appropriate view.

v0.9 MAY add internal live-host policy based on the resolved view class, but it MUST NOT add a parallel provider-level `live_renderer` registry unless an ADR proves the existing view registry insufficient.

## 6.4 View classes relevant to v0.9

For presentation policy only, mounted live views can be reasoned about as three broad classes:

```text
state projection
append projection
specialized projection
```

This is not a new semantic type hierarchy.

### State projection

A view showing the latest known state of identifiable items, such as a process/service table.

### Append projection

A view showing an ordered sequence of records, such as logs or events.

### Specialized projection

A domain view with its own established interaction model, such as `map --live`.

## 6.5 Unknown/custom view behavior

A KUANG/11 view that does not fit a built-in host policy MAY handle its own view-private update state through the existing constrained view lifecycle.

It remains subject to:

- host memory/resource quotas;
- terminal ownership rules;
- cancellation;
- accessibility;
- no raw terminal escape ownership;
- no hidden persistence.

v0.9 MUST NOT require every plugin to declare a new public `live_view_kind` manifest field.

# 7. State-Projection Live Views

## 7.1 Purpose

State projections make continuously changing collections readable without printing a new full table for every update.

Canonical example:

```text
watch process
```

## 7.2 Stable identity is required for stable rows

The renderer may update a row in place only when the inherited schema/provider semantics provide a stable identity for the represented lifetime.

For processes, PID alone may be insufficient across PID reuse; the provider identity contract SHOULD use start-time/lifetime identity as already required by earlier architecture.

If stable identity is unavailable, the renderer MUST choose a more conservative representation rather than guess correspondence.

## 7.3 Row update

When a semantic update identifies an existing object lifetime, the view may replace the displayed field values for that object.

This replacement is presentation materialization.

It does not mutate prior `HistoryEntry` results or fabricate temporal history.

## 7.4 Row appearance

A newly observed stable identity may appear as a new row according to the underlying `watch` semantics.

The view MAY briefly emphasize the row using semantic presentation roles, subject to reduced-motion/accessibility settings.

Emphasis MUST NOT imply severity or importance unless the data says so.

## 7.5 Row disappearance

When the live semantic source reports that an object is gone, the row may be removed.

The renderer MAY retain a short presentation-only tombstone/fade marker if the inherited view semantics permit, but it MUST NOT fabricate a v0.4 tombstone object unless v0.4 identity semantics produced one.

## 7.6 Sorting

A state table may be sorted by a displayed field.

Continuous changes can otherwise cause rows to jump on every update.

Default policy SHOULD optimize readability:

- stable sort key remains active;
- rows may move when the key materially changes;
- the selected stable object remains selected when possible;
- viewport anchoring SHOULD prefer selected identity over numeric row offset;
- excessive reorder animation is forbidden.

The exact debounce/coalescing of visual reordering is presentation policy, not semantic ordering.

## 7.7 Selection stability

If a selected object updates, selection SHOULD remain on the same stable identity.

If the object disappears:

- selection MUST NOT silently transfer semantic intent to the row now occupying the same screen position;
- the view MAY move visual cursor to a neighboring row;
- any subsequent explicit action must resolve the new selection normally;
- a pending v0.6 plan or already-resolved target is unaffected by screen selection movement.

## 7.8 Filtering

Pipeline filtering occurs before the view and remains semantic.

Example:

```text
watch process | where cpu > 20
```

An object may enter or leave the visible set as its CPU changes.

The view MUST treat that as membership change in the already-filtered stream, not as a reason to query all processes independently.

## 7.9 View-local search

If the existing view supports local text/object search, that search may hide rows for navigation.

Local search MUST be visibly distinguishable from pipeline `where` semantics if ambiguity would matter.

Closing local search MUST restore the view of the same underlying stream; it MUST NOT re-run the provider.

## 7.10 Large state sets

State views MUST virtualize large collections where necessary.

Only visible/near-visible rows need formatted cell strings.

The view MAY retain compact semantic/display references for off-screen rows according to memory budgets, but MUST NOT eagerly format every row on every update.

# 8. Append-Projection Live Views

## 8.1 Purpose

Append projections are appropriate for ordered record streams such as:

- logs;
- audit events;
- provider events;
- timeline-like live tails where the command explicitly produces a live stream.

## 8.2 Follow latest by default

An append view SHOULD start in `FOLLOWING` unless the specialized command/view contract says otherwise.

New records appear at the latest edge of the viewport.

## 8.3 User scrolling enters browsing

When the user scrolls away from the latest edge through a normal navigation action, the view enters `BROWSING`.

The source and pipeline continue.

The viewport remains stable enough to read the selected/anchored content.

## 8.4 New updates while browsing

While browsing, the host SHOULD indicate that newer display updates exist.

Example:

```text
BROWSING  +128 updates   End: latest
```

The exact key binding is configurable through normal view keymap policy.

The indicator MUST NOT push rows around or repeatedly steal focus.

## 8.5 Returning to latest

A semantic view action such as `view.follow-latest` returns the viewport to the latest retained position.

This action:

- does not restart the stream;
- does not query the provider;
- does not alter shell context;
- does not alter temporal session context unless the specialized view defines that behavior;
- clears/updates the unseen display indicator.

## 8.6 Display buffer bounds

Append views MUST use bounded display retention.

A recommended default budget is the lower of:

```text
10,000 retained display records
16 MiB estimated retained display payload
```

The exact defaults MAY be tuned after measurement, but bounds MUST exist and be user-inspectable.

These limits apply to presentation retention, not to the underlying provider/recorder.

## 8.7 Eviction

When older display records are evicted while the user is following latest, no interruption is required.

When the user is browsing near an eviction boundary, the view MUST preserve orientation where possible and show a truncation landmark if the requested older content is no longer retained.

Example:

```text
--- view history truncated; older rows were not retained ---
```

## 8.8 Eviction is not source loss

The truncation landmark MUST NOT use v0.5 `coverage gap` language unless a real temporal gap exists.

If both conditions exist, both should be visible:

```text
--- view history truncated before 14:00:00 ---
14:02:11  ---- coverage gap: source disconnected 4.2s ----
```

## 8.9 Exact event identity

When records have stable event references such as v0.5 `@e42`, selection/navigation SHOULD retain those references even if screen line numbers move.

A copied reference must remain meaningful independently of the view buffer where retention permits.

## 8.10 High-rate append sources

A high-rate source may exceed a human's ability to read it.

The view MAY:

- reduce terminal frame rate;
- batch visual insertion into frames;
- show rate/count summaries in status;
- virtualize formatting;
- retain only a bounded presentation window.

It MUST NOT silently discard semantic records before required pipeline/view processing unless the canonical upstream overflow policy allows loss/coalescing.

# 9. Specialized Live Views

## 9.1 Earlier semantics win

A specialized view may already define behavior more precise than generic follow/browse.

v0.9 MUST not flatten that behavior into the generic policy.

## 9.2 `map --live`

v0.4/v0.5 remain authoritative.

In particular:

- live topology is derived from real spatial/event semantics;
- pausing the live map freezes the view temporal cursor, not the real system;
- providers and recorder continue;
- rewind keys operate on v0.5 temporal evidence;
- coverage gaps remain explicit;
- returning to `now` uses the v0.5 model.

The v0.9 Deck host contributes only:

- stable terminal ownership;
- mounting inside the primary view;
- redraw scheduling;
- command-surface coexistence according to job semantics;
- resource accounting;
- suspend/child-handoff restoration.

## 9.3 Timeline live tails

If a timeline command/view is explicitly live, v0.5 event identity, coverage and gap semantics remain authoritative.

Generic append-view retention may be used for the visible tail, but must not alter event ordering or evidence semantics.

## 9.4 KUANG/11 specialized views

Existing KUANG/11 constrained views may receive live stream updates through their existing host contracts.

They do not gain raw terminal access.

They MUST respect host quotas and cancellation.

If a plugin needs durable history, it must use the existing explicit history/state capability model; a mounted view does not grant persistence by implication.

# 10. Follow, Browse and Pause Semantics

## 10.1 Generic state machine

For generic live views, the presentation-only state machine is intentionally tiny:

```text
              user navigates away
FOLLOWING  ------------------------>  BROWSING
    ^                                     |
    |                                     |
    +---------- follow latest ------------+
```

Closing/unmounting is handled by the existing view lifecycle.

Execution running/stopped/failed is handled by existing job/stream semantics.

## 10.2 No generic stream pause

v0.9 MUST NOT introduce a host-level key that universally suspends a semantic stream while leaving the view mounted.

Why:

- some providers cannot be paused safely;
- pausing a consumer can trigger backpressure and change source behavior;
- event sources may lose continuity;
- v0.5 already defines a different and safer view-temporal pause model for live maps;
- shell job control already provides explicit process/job stop semantics where applicable.

If the user intentionally wants to stop/suspend a job, existing job-control commands/signals apply.

## 10.3 Browsing state survives updates

While `BROWSING`, new updates MUST NOT yank the viewport to latest.

For state tables, selected stable identity SHOULD remain anchored where practical.

For append views, the top/selected retained record SHOULD remain anchored while newer records accumulate.

## 10.4 Browse state and resize

Resize SHOULD preserve semantic anchor rather than raw row number.

Examples:

- selected `ObjectRef` in a table;
- selected `EventId` in a log/timeline;
- nearest retained record if the exact presentation row was a wrapped continuation.

## 10.5 Browse state and data eviction

If the anchor is evicted due to bounded display retention, the view MUST not pretend it is still showing the same item.

It SHOULD:

1. move to the nearest retained boundary;
2. show a view-truncation indicator;
3. preserve an independent stable reference if one exists and is still resolvable elsewhere.

## 10.6 Follow state and sort churn

A state table can be `FOLLOWING` while still preserving selection.

Following means "track current display state", not "force cursor to row 1".

## 10.7 Status vocabulary

Generic view status SHOULD use literal terms:

```text
LIVE
BROWSING
ENDED
FAILED
+N updates
last update 2.1s ago
```

But `ENDED`/`FAILED` MUST be derived from existing execution status, not a new v0.9 lifecycle.

Cyberpunk theme vocabulary may later restyle these labels, but the semantic content must remain obvious.

# 11. Foreground, Background and Deck Interaction

## 11.1 Existing shell semantics stay authoritative

A command written as:

```text
watch process
```

and a command written as:

```text
watch process &
```

are not made equivalent by Deck.

The first follows normal foreground stream/job semantics.

The second follows normal background job semantics.

## 11.2 Deck MUST NOT auto-background foreground live commands

Automatically converting `watch process` into a background job because a command surface is visible would change shell semantics.

v0.9 forbids that behavior.

## 11.3 Foreground live command UX

While a foreground live execution owns the shell's foreground semantic slot, Deck MAY continue to render the command surface for orientation/editor preservation, but it MUST NOT execute another foreground command unless existing job-control semantics permit it.

The UI SHOULD make the state explicit, for example:

```text
foreground: watch process
Ctrl-C cancel   Ctrl-Z suspend
```

Exact key bindings follow existing shell/job control.

## 11.4 Background live job UX

A background live job MAY remain visible as the primary live view while the shell accepts new commands.

Example:

```text
watch process &
```

The job registry is authoritative for lifecycle/status.

The primary view is merely attached to that job's existing view/execution.

## 11.5 New command while a background live view is primary

v0.8's normal primary-result replacement behavior remains the default.

When a new command produces a normal primary result:

- the background live job continues;
- its mounted view may move to background/close according to existing view lifecycle;
- the new result becomes primary;
- the live job remains discoverable through the existing jobs surface;
- reopening the job/view MUST NOT start a duplicate subscription.

## 11.6 No pinning system in v0.9

v0.9 MUST NOT add persistent pins, tiles or "keep this live panel forever" layout state.

The user can run multiple background watches as jobs, but the Deck remains a bounded workspace rather than a dashboard.

## 11.7 Returning to a live job

The jobs auxiliary or existing command/view navigation SHOULD make it fast to reopen a running live job.

Reopening:

- reuses the execution where possible;
- does not rerun the command;
- may create a fresh presentation binding if the previous view was closed;
- begins from currently available semantic/view state;
- MUST state when prior local display history is no longer available.

## 11.8 Completed background job

If a live job ends while not mounted, reopening it shows its final retained result/state according to existing job/result retention policy.

v0.9 MUST NOT promise indefinite retention merely because the job was once a live view.

# 12. Render Scheduling and Terminal Frame Coalescing

## 12.1 Problem

Semantic update rates and useful terminal frame rates differ.

A process source can change hundreds of fields per second while a human does not benefit from hundreds of complete terminal repaints.

## 12.2 Rendering cadence is not collection cadence

The provider's sampling/subscription cadence remains semantic/provider policy.

The render scheduler only determines when the already-updated view is painted to the terminal.

Example:

```text
watch process --every 100ms
```

may collect/process at 10 Hz while the screen paints at 10 Hz or less depending on changes/load.

A native event source may process thousands of updates per second while the screen paints at a bounded rate.

## 12.3 Default frame budget

The Deck SHOULD target a maximum live repaint rate of approximately:

```text
30 frames/s normal ceiling
60 frames/s allowed for specialized high-value motion if measured useful
```

It SHOULD paint more slowly when:

- there are no visible changes;
- terminal bandwidth is constrained;
- the user is actively typing;
- formatting work is expensive;
- the view is unfocused/backgrounded.

These are presentation budgets, not semantic throttles.

## 12.4 Typing priority

When command input and live repaint compete for CPU/terminal writes, editor latency wins.

The scheduler SHOULD defer non-critical live frames long enough to keep visible keystroke latency within v0.8 targets.

It MUST NOT delay semantic cancellation or safety prompts merely to preserve frame cadence.

## 12.5 Damage-based redraw

The v0.8 screen/damage model remains authoritative.

v0.9 SHOULD avoid full-screen repaint when only a small region changed.

However, correctness takes priority over micro-optimization; a full redraw is preferable to stale/corrupted cells.

## 12.6 Frame invalidation coalescing

Multiple view invalidations arriving before the next scheduled frame MAY collapse into one paint.

Example:

```text
row 17 cpu 21 -> 22
row 17 cpu 22 -> 23
row 9 memory 800 -> 805
```

If all semantic updates were applied, the next frame may render only final current values.

For append views, the next frame may draw all newly retained records or an appropriate visible subset without losing their semantic presence in the view state.

## 12.7 No animation queue

The host MUST NOT queue one animation/frame per semantic update.

That design creates unbounded latency under burst load.

Real state is more important than replaying every intermediate visual transition.

## 12.8 Idle behavior

A live view with no visible changes SHOULD emit no terminal traffic except deliberate low-frequency status updates that convey real source state.

A blinking "LIVE" indicator is unnecessary.

## 12.9 Low-bandwidth remote terminals

When terminal output is transmitted over a slow/high-latency path, the scheduler SHOULD reduce byte volume through:

- damage redraw;
- lower frame ceiling;
- avoiding repetitive status churn;
- formatting only visible rows;
- preserving stable column widths where practical.

It MUST NOT reduce semantic source fidelity solely because terminal bandwidth is low.

# 13. Relationship to Semantic Backpressure and Overflow

## 13.1 v0.2 remains authoritative

Native pipelines MUST continue to use the v0.2 backpressure contract.

A slow semantic consumer must not permit an unbounded producer to exhaust memory.

v0.9 MUST NOT insert an unbounded "UI queue" after the pipeline and thereby bypass that guarantee.

## 13.2 Renderer queue must be bounded

The path from view invalidation to terminal paint MUST use bounded/coalescing scheduling state.

A valid implementation can often represent pending work as:

```text
frame_pending: Bool
next_deadline: Instant?
dirty_regions: bounded set/bitmap
```

rather than a queue containing one item per semantic update.

## 13.3 Semantic overflow vs display overflow

Two distinct problems MUST remain distinguishable.

### Semantic/source overflow

Values/events were not processed or delivered according to the canonical source/pipeline policy.

This may create:

- a structured stream error;
- provider-specific loss metadata;
- a v0.5 coverage gap;
- KUANG/11 overflow diagnostics;
- job failure.

### Display overflow

The view successfully processed semantic updates but cannot retain arbitrary old presentation rows forever.

This creates only view truncation.

The UI MUST NOT merge these conditions.

## 13.4 View state may intentionally collapse replaceable current state

For a state projection, presentation state typically stores only the current visible state for each stable object.

If an object CPU value changes:

```text
12.1 -> 12.2 -> 12.4 -> 12.3
```

between frames, the next frame may show `12.3`.

This is not event loss if the underlying command semantics promise current state snapshots/updates rather than a lossless event history.

The view MUST NOT claim that all intermediate values are retained.

## 13.5 Event projections need stronger retention honesty

For an append/event projection, the user may reasonably expect received records to remain individually navigable until presentation-retention eviction.

The view therefore SHOULD append every semantic record it receives after upstream overflow/coalescing policy has been applied.

If upstream reports dropped events, that fact remains visible according to existing semantics.

## 13.6 KUANG/11 overflow remains KUANG/11 policy

v0.2 already defines plugin overflow choices such as blocking, drop-oldest, coalesce and fail-stream under host policy.

v0.9 does not redefine them.

A plugin view may present the resulting facts, but the Deck does not silently override plugin stream correctness policy merely to improve frame rate.

## 13.7 Blocking renderer is a bug

Terminal rendering MUST NOT hold semantic pipeline locks or provider callbacks while performing potentially slow terminal I/O.

The architecture SHOULD hand off view invalidation through bounded non-blocking/coalescing state so that a slow terminal cannot deadlock provider execution.

# 14. Currentness, Age and Continuity Presentation

## 14.1 Persistence creates a currentness problem

A static or live view can remain visible after its semantic source stops changing.

The longer a Deck result stays mounted, the more dangerous it becomes to equate "visible" with "current".

v0.9 therefore requires explicit currentness presentation for long-running live views.

## 14.2 Do not invent universal freshness semantics

Not every source has a meaningful freshness SLA.

A systemd event stream may legitimately be silent for hours.

A one-second process watch normally changes frequently.

A network link may be idle while still healthy.

Therefore v0.9 MUST NOT define:

```text
stale if no update for 5s
```

as a universal rule.

## 14.3 Always available facts

The view MAY safely display factual metadata such as:

```text
running
last update 14:03:12.441
age 2.4s
background job #7
source linux.procfs / polling 1s
```

when those facts are available from existing execution/provider metadata.

## 14.4 Freshness thresholds are source/policy facts

If an inherited provider/command contract declares an expected cadence or freshness threshold, the presentation MAY derive a warning from that contract.

Example:

```text
expected every 1s
last update 6.4s ago
update delayed
```

The renderer MUST identify this as a timing/source warning, not fabricate a semantic conclusion such as "service unhealthy".

## 14.5 Silent event stream

A live event stream with no recent events is not automatically stale.

If the connection/job is healthy and the event source is event-driven, the correct presentation may simply be:

```text
LIVE  no new events for 2h17m
```

## 14.6 Ended stream

When the inherited execution reports normal completion, a mounted live view MUST stop claiming it is live.

It SHOULD show:

```text
ENDED 14:08:11
```

plus whatever final result/records remain retained.

## 14.7 Failed stream

When the inherited execution fails, the view MUST expose the structured error through normal error presentation.

It MUST NOT keep animating or displaying an ambiguous `LIVE` label.

## 14.8 Cancelled stream

Cancellation is displayed as cancellation, not failure, where the existing error/job model distinguishes them.

## 14.9 Remote link continuity

If a remote link disconnects and reconnects, v0.9 only renders the continuity facts that the remote/link/provider layer can prove.

Possible truthful outcomes include:

```text
reconnected; source resumed with proven sequence continuity
```

or:

```text
reconnected; continuity unknown during 4.2s disconnect
```

or a v0.5 coverage gap when temporal semantics provide one.

The Deck MUST NOT infer uninterrupted continuity simply because the final connection succeeds.

## 14.10 Static results remain timestamped snapshots

A static result that remains visible beside a live job is still static.

The Deck SHOULD preserve execution/result time context so the user can distinguish:

```text
result @42  captured 14:01:03
```

from:

```text
job #7 watch process  running  latest 14:03:19
```

# 15. Temporal Coverage, Gaps and Display Truncation

## 15.1 v0.5 is the only temporal gap authority

Where v0.5 applies, `TemporalCoverage` and gap semantics remain canonical.

v0.9 may render those facts prominently in a persistent view but MUST NOT invent alternatives.

## 15.2 Material source gap

If a live temporal source reports a real gap, the view MUST keep it visible at the appropriate boundary.

Example:

```text
14:02:18  service nginx active
14:02:21  ---- coverage gap: recorder/source unavailable 4.2s ----
14:02:25  service nginx failed
```

A subsequent live update does not erase the fact that the interval was unsupported.

## 15.3 View truncation landmark

Display retention may independently show:

```text
--- Deck retained view starts here; older rows evicted ---
```

The landmark SHOULD use different wording/style from a semantic gap.

## 15.4 Both can coexist

The following is legitimate:

```text
--- Deck view history starts at 14:00:00 ---
14:02:21  ---- coverage gap: source disconnected 4.2s ----
```

The first is local presentation retention.

The second is evidence about the observed world.

## 15.5 Historical context remains historical

If the session is in v0.5 historical context, a newly mounted ordinary live/current view MUST NOT silently switch the session to present.

Commands follow existing temporal-context rules.

Where a command is invalid in historical context, it should fail according to v0.5 rather than the Deck guessing that `watch` means `now`.

## 15.6 Return-to-now remains v0.5

The generic `view.follow-latest` action MUST NOT be treated as the same operation as v0.5 `now`.

A generic append view can follow the newest records while the session's semantic temporal context remains governed by v0.5.

Specialized temporal views may map their own follow action to `now` if their earlier contract explicitly defines that semantics.

# 16. Logs and Append-Oriented Operational Streams

## 16.1 Logs are ordinary typed records when Ono owns the semantics

A native/provider log command SHOULD preserve structured fields such as:

```text
timestamp
severity
source/unit
message
pid
fields/provenance
```

where available.

v0.9 does not create a new log data model.

## 16.2 No string-only live API

A live log view consumes the canonical output of the existing command/provider.

The view MUST NOT require providers to pre-render ANSI log lines.

## 16.3 Follow behavior

A live log view uses the generic append policy unless an existing command defines something stronger:

- starts following latest;
- user scroll enters browsing;
- new records continue to ingest;
- unseen update indicator appears;
- return-to-latest does not rerun.

## 16.4 Wrapped messages

Wrapping is presentation-only.

Selection/reference should anchor to the underlying record rather than a wrapped terminal line.

Resize MUST NOT create duplicate semantic rows merely because one message wraps differently.

## 16.5 Filtering belongs in the pipeline

This remains the preferred semantic form:

```text
watch log --service nginx | where level >= error
```

View-local search/filter MAY exist for temporary navigation, but must not masquerade as the pipeline expression.

## 16.6 High-rate logs

At high rate, the Deck may stop auto-scrolling smoothly enough for humans to perceive every insertion.

It SHOULD prioritize:

1. consuming according to canonical stream policy;
2. bounded retention;
3. responsive input;
4. truthful rate/currentness status;
5. fewer terminal frames.

It MUST NOT prioritize decorative smoothness over correctness.

## 16.7 Copying while live

Text selection/copy behavior MUST not be repeatedly disrupted by live redraw where terminal/application mechanics permit.

When the user is in `BROWSING`, the view should avoid moving already-visible rows unnecessarily.

## 16.8 Secret/redaction behavior

Existing secret/redaction semantics remain authoritative.

Display retention MUST NOT retain an unredacted value merely because a later render masks it.

# 17. State Tables and Rapidly Changing Metrics-Like Fields

## 17.1 Current values, not implicit metric history

A process table may show CPU changing over time.

That does not mean the Deck should retain a time series of CPU samples.

v0.9 default state projection stores only enough data to render current state plus bounded interaction state.

## 17.2 Sparklines are not a release requirement

v0.2 already mentions a `Sparkline` view primitive.

v0.9 does not need to remove that capability, but it MUST NOT add automatic historical sampling merely to draw sparklines.

A sparkline may be used only when its underlying view/provider already supplies legitimate bounded series data.

## 17.3 No implicit metric recorder

The following is forbidden as a hidden side effect:

```text
watch process
-> Deck begins storing 60 minutes of CPU history for every process
```

If such history is a future explicit feature, it needs a semantic storage/retention contract.

## 17.4 Rate fields

If a provider/pipeline already computes a rate such as bytes/s, the view renders it.

The renderer SHOULD NOT independently compute semantic rates from irregular frame timestamps unless the command/view contract explicitly assigns that responsibility.

## 17.5 Stable columns

For live tables, column width jitter is especially distracting.

The renderer SHOULD use stable width policy within a mounted session, subject to terminal resize and materially larger required values.

It MAY reserve reasonable width based on schema/initial samples rather than recomputing every column width for every frame.

## 17.6 Numeric alignment

Numeric fields SHOULD remain aligned using v0.7 semantic formatting.

Repeated updates MUST not introduce layout drift due to inconsistent unit formatting.

# 18. Terminal Ownership, Child Handoff and Live Views

## 18.1 v0.8 terminal lease remains authoritative

When Deck owns the terminal, live views may repaint.

When Deck releases the terminal to a foreground child, live views MUST stop terminal writes immediately.

## 18.2 Semantic execution may continue during child handoff

Background native live jobs MAY continue while `vim`, `less`, `ssh` or another foreground child owns the terminal, according to ordinary process/job semantics.

The Deck MUST NOT attempt to repaint behind the child.

## 18.3 Return from child

After Deck reacquires terminal ownership:

1. restore terminal input modes according to v0.8;
2. query current terminal dimensions;
3. reflow the mounted view;
4. paint one coherent current frame;
5. do not replay every frame that would have occurred during child ownership.

## 18.4 Events accumulated during handoff

If a live append view retained semantic records while the child owned the terminal, the returned frame may show the newest retained state plus an update count.

Any retention eviction during handoff is view truncation and must be labeled if the user later browses to the boundary.

Upstream semantic loss remains governed by source/pipeline policy.

## 18.5 Foreground child started from live foreground job

Existing job-control constraints remain authoritative.

v0.9 MUST NOT create an unsupported nested foreground semantic execution merely because the Deck layout could display both.

## 18.6 Handoff stress

Repeated child handoff while multiple background live jobs execute MUST be part of the release test matrix.

# 19. Suspend, Resume and Process Stop

## 19.1 Suspend Ono

When Ono itself is suspended by shell/job-control semantics, the Deck loses the ability to process/render normally.

On resume, v0.8 terminal restoration/reacquisition remains authoritative.

## 19.2 No invented continuity after suspend

After resume, the live view may have one of several truthful situations:

- upstream was blocked and resumed with continuity;
- remote/provider buffered and resumed with continuity;
- source disconnected and reconnected;
- events were lost;
- continuity is unknown.

v0.9 MUST render whichever facts the inherited source/runtime can prove.

## 19.3 Full redraw on resume

The first Deck frame after resume SHOULD be a full redraw from current view state.

Incremental damage assumptions from before suspension MUST be discarded.

## 19.4 Follow state preservation

Generic `FOLLOWING`/`BROWSING` state SHOULD survive suspend/resume when its anchor is still retained.

If the browse anchor was evicted while execution continued elsewhere, the view must move to a valid retained position and show truncation.

## 19.5 v0.5 temporal pause

A v0.5 paused live map remains a specialized temporal cursor state.

Suspend/resume of the process MUST NOT silently convert that cursor to `now`.

# 20. Reconnect and Remote Live Sources

## 20.1 Reconnect is not defined here

v0.9 does not invent a reconnect protocol or lifecycle.

Remote link/provider/KUANG contracts own connection and retry behavior.

## 20.2 Presentation requirements

When existing execution metadata indicates remote degradation/reconnect, the mounted view SHOULD expose:

- source/link identity;
- running/retrying/failed fact where available;
- time since last processed update;
- proven or unknown continuity;
- structured error/help path.

## 20.3 Do not hide link changes

A live view started on `prod-db-03` MUST remain visibly associated with that resolved execution/link context even if the shell later changes context.

The view MUST NOT visually adopt a newly active host merely because prompt context changed.

## 20.4 Reconnect does not retarget

If the shell changes to another link while a background live job reconnects, the job continues to refer to its original resolved target unless the underlying semantic command explicitly says otherwise.

## 20.5 Authentication failure

Authentication/trust failure during a remote live operation is an execution/provider error.

The Deck renders it through normal structured diagnostics.

It MUST NOT pop up repeated custom password dialogs that bypass existing link/security policy.

# 21. Multiple Live Jobs Without Dashboard Creep

## 21.1 Multiple jobs already exist

v0.2 explicitly permits multiple background watches:

```text
watch service nginx &
watch process --service nginx &
get job
```

v0.9 does not need a dashboard to support that capability.

## 21.2 One primary update-bearing view

The v0.9 Deck SHOULD support at most one independent high-frequency/update-bearing live view mounted as the primary view by default.

The auxiliary slot remains primarily for context, history, help, jobs and provenance as defined by v0.8.

This constraint is intentional.

## 21.3 No live grid

The release MUST NOT add:

```text
2x2 live tiles
user-created monitoring layouts
persistent pane pinning
multi-chart dashboard templates
```

## 21.4 Jobs auxiliary is the multiplexor

Multiple background live jobs are managed/discovered through the existing jobs model.

The user can move between them by opening the desired existing job/view.

This keeps multiplicity in semantic job control rather than multiplying visual panes.

## 21.5 Low-frequency status summaries

The status line/jobs auxiliary MAY show bounded summaries for several jobs:

```text
#4 watch process  running
#5 watch service  running
#6 map --live     stopped
```

It MUST NOT subscribe to every job's full stream merely to generate decorative mini-widgets.

## 21.6 Reassessment trigger

If dogfooding shows that one mounted live view is insufficient for a common operator workflow, the project MAY reconsider a second update-bearing slot in a later release.

That decision MUST be evidence-driven and must still avoid a free-form layout system.

# 22. Display Retention and Memory Policy

## 22.1 Every retained collection is bounded

v0.9 code MUST make display-retention limits explicit in configuration or constants with documented rationale.

No `Vec`/queue collecting an unbounded live history is acceptable.

## 22.2 Recommended initial defaults

Recommended baseline limits:

```text
append view records      10,000
append view bytes        16 MiB estimated payload
state view objects       provider/result dependent, virtualized
render invalidations     coalesced, not queued per update
unseen update counter    saturating integer
```

Implementation may tune values after profiling.

## 22.3 State view memory

A state view may need one current row state per object in the live set.

This is inherently proportional to current result cardinality.

It MUST NOT additionally retain every prior version of each row by default.

## 22.4 Large object fields

Views SHOULD retain stable references/compact summaries where possible rather than clone arbitrarily large nested values for display.

Inspecting a large object can use the existing value/result/provider path.

## 22.5 Eviction diagnostics

A debug/inspect path SHOULD expose:

```text
view rows retained       10000 / 10000
estimated bytes          15.2 MiB / 16 MiB
oldest retained          14:01:22
truncations              3
```

This is diagnostic metadata, not product telemetry by default.

## 22.6 Background/unmounted view retention

When a live execution remains a background job but its view is unmounted, the host SHOULD release expensive presentation buffers unless an inherited view contract requires bounded background state.

Reopening may therefore start with current/latest available state rather than the exact prior scrollback.

The UI must not promise otherwise.

## 22.7 No persistence by default

Display buffers MUST disappear when their view/session ends unless explicit earlier persistence semantics apply.

They MUST NOT be written into the v0.5 recorder automatically.

# 23. History, ResultRef and Live Executions

## 23.1 Command history remains `HistoryEntry`

Starting a live command creates normal semantic history according to existing execution/history policy.

v0.9 MUST NOT create a second `LiveSessionHistory` database.

## 23.2 ResultRef semantics remain bounded

A live execution may have a retained result/reference according to existing result policy.

v0.9 does not require every intermediate update to become a `ResultRef`.

That would effectively create a hidden history database.

## 23.3 Opening live job from history

History may identify that a command was/ is associated with a job/result.

Opening a still-running job should attach to the existing execution where possible rather than rerun the command.

Opening an ended/expired job follows existing retention semantics.

## 23.4 Reproducibility remains textual

The underlying command remains copyable:

```text
watch process --every 1s | where cpu > 20
```

The user does not need to serialize Deck follow state to reproduce the semantic operation.

## 23.5 Presentation state is intentionally not command history

The following need not be preserved in `HistoryEntry`:

```text
user had scrolled up 37 rows
selected visual row was #18
last frame painted at 27 Hz
```

These are ephemeral view facts.

# 24. Selection, Identity and Disappearing Objects

## 24.1 Selection remains v0.2 selection

A live view does not gain stronger target semantics.

Selection is ephemeral until explicitly consumed.

## 24.2 Object updates retain selection by identity

When possible, selection follows stable object identity through row reordering/updates.

## 24.3 Object disappearance invalidates selection target

If a selected process disappears, the view must not allow an explicit action to accidentally target the new process displayed at the same row coordinate.

Any selected reference must be re-resolved according to existing identity rules.

## 24.4 PID reuse case

A test MUST cover:

1. process PID 1842 is selected;
2. process exits;
3. PID 1842 is reused by a new process;
4. visual row appears in a similar position.

The new lifetime MUST NOT inherit the old selection/reference merely because PID is equal.

## 24.5 Historical event selection

v0.5 event IDs remain stable references independently of view scrolling.

# 25. Safety and Mutations Around Live Views

## 25.1 v0.9 does not redefine pipeline mutation semantics

Whether an unbounded stream may feed a mutating consumer is a language/safety question larger than presentation integration.

The earlier specifications remain authoritative until a dedicated language/safety clarification explicitly changes them.

v0.9 MUST NOT silently add new execution rejection rules solely in the Deck renderer.

## 25.2 Renderer never triggers mutation

A live row appearing, changing, becoming selected or crossing a visual threshold MUST NEVER automatically execute a mutation.

The renderer does not turn:

```text
cpu > 90
```

into:

```text
kill process
```

## 25.3 No alert-action shortcuts

v0.9 does not add:

```text
when value changes -> command
when threshold crossed -> restart
on event -> apply plan
```

Those would be trigger/automation semantics requiring an explicit future design.

## 25.4 Explicit user action remains explicit

The user may inspect/reference an object visible in a live view and then execute an ordinary command.

That command resolves target/context using existing rules at execution time.

## 25.5 v0.6 plan safety

If a v0.6 plan was created from an object while a live view later changes, the plan's sealed target/effect/revalidation semantics remain authoritative.

The Deck MUST NOT silently retarget the plan to whatever row is currently highlighted.

## 25.6 Currentness warning before action

If the user attempts an action using a reference known to be no longer resolvable/current, existing identity/revalidation errors should stop or re-resolve according to semantic policy.

The UI may explain the situation but is not itself the authority.

# 26. KUANG/11 Integration

## 26.1 Existing stream host API remains valid

KUANG/11 already defines stream subscribe/produce/cancel/backpressure capabilities and quotas.

v0.9 MUST use those contracts rather than introduce `ui.live_stream` or equivalent solely for Deck.

## 26.2 Existing view API remains valid

Plugins continue to submit constrained view trees or use the stable view protocol.

The host owns:

- terminal rendering;
- focus;
- sizing;
- accessibility;
- cancellation routing;
- display-retention/resource ceilings.

## 26.3 Plugin does not own frame scheduler

A plugin may invalidate/update its view through the existing protocol.

It MUST NOT demand one terminal repaint per update or write ANSI directly.

The host may coalesce display frames.

## 26.4 Plugin view-private state is bounded

v0.2 already requires views not to retain arbitrary large stream copies.

v0.9 strengthens this operationally for long-running Deck sessions.

Host quotas SHOULD include view-private memory accounting.

## 26.5 Plugin continuity claims require evidence

A plugin view MUST NOT label a reconnect gap as complete continuity unless its source contract can prove it.

If it participates in v0.5 temporal semantics, it should emit/use canonical coverage/evidence.

## 26.6 No new manifest fields unless proven necessary

The project SHOULD first attempt to derive long-running policy from:

- existing view kind/tree;
- existing stream metadata;
- existing provider/KUANG capability metadata;
- host defaults.

A new manifest field is justified only through an ADR showing real ambiguity.

# 27. Accessibility and Human Factors

## 27.1 Motion is optional

Rapid live change can be cognitively exhausting.

Reduced-motion mode MUST disable non-essential transition emphasis.

Current values may still update; the host simply avoids animations/flashes.

## 27.2 Color is not currentness

`LIVE`, `BROWSING`, gap, failed and truncation states MUST remain understandable without color.

## 27.3 Focus remains visible

A user must be able to tell whether keystrokes control:

- command editor;
- primary live view navigation;
- auxiliary view.

Live redraw MUST NOT obscure the focus indicator.

## 27.4 Screen-reader/linear fallback

Where the full Deck is unsuitable, Rich TTY/plain output remains the degradation path.

v0.9 MUST preserve deterministic non-interactive stream behavior.

## 27.5 Avoid change blindness

State tables MAY indicate recently changed fields/rows briefly when useful, but:

- indication must be bounded;
- no flashing storms;
- no meaning carried only by animation;
- reduced-motion disables it;
- unchanged values should remain visually stable.

## 27.6 Browse mode must be calm

When the user deliberately browses older retained content, new incoming updates MUST not repeatedly shift the viewport, steal focus or open notifications.

# 28. Security

## 28.1 Terminal escape injection

Live values may contain hostile filenames, log messages or remote text.

All v0.7/v0.8 sanitization rules remain mandatory on every update.

A high-rate stream MUST NOT create a bypass because sanitization is considered too expensive.

## 28.2 OSC/clipboard attacks

Untrusted live content MUST NOT emit OSC 52 or other terminal control sequences through the host renderer.

## 28.3 Notification spoofing

A live record whose text contains strings such as:

```text
LIVE
ERROR
prod-db://root
```

must remain visually distinguishable as content from host chrome/status.

## 28.4 Secret retention

Display buffers must retain already-redacted semantic values or protected representations according to existing secret policy.

Evicted buffers should release memory promptly where practical.

## 28.5 Remote source identity

The host/link identity associated with a live execution MUST remain visible enough to prevent content from making a remote stream look local.

## 28.6 Plugin quota abuse

A plugin cannot request unlimited display retention under `ui.view`.

Host policy remains authoritative.

# 29. Resource and Performance Budgets

## 29.1 Reference priority order

Under load, v0.9 prioritizes:

```text
1 semantic correctness/cancellation
2 command input responsiveness
3 terminal integrity
4 current visible state
5 navigation continuity
6 visual smoothness
```

## 29.2 Keystroke latency

While one live view is updating heavily, visible command-editor input latency SHOULD remain:

```text
p95 <= 16 ms on reference local hardware
p99 <= 33 ms under synthetic high live load
```

where terminal transport itself is not the bottleneck.

## 29.3 Frame cost

Typical live frames SHOULD complete within the v0.8 render budget.

The implementation SHOULD instrument:

- time to apply semantic update to view state;
- formatting time;
- diff/damage computation;
- bytes written;
- frame duration.

## 29.4 CPU budget

An idle/silent live view SHOULD consume negligible CPU beyond its actual source cadence.

There must be no busy-loop animation or polling solely for UI status.

## 29.5 Memory budget

Presentation-only retention for a typical live view SHOULD stay within tens of MiB, not hundreds, under defaults.

Large semantic result sets may exceed this based on actual object count, but historical versions must not multiply usage without explicit semantics.

## 29.6 Hours-long session test

A release candidate MUST survive a minimum multi-hour synthetic/live-view soak without unbounded growth in:

- heap;
- file descriptors;
- tasks/threads;
- view handles;
- subscriptions;
- render queues;
- retained formatted strings;
- job registry artifacts.

## 29.7 Burst handling

Tests MUST inject bursts significantly above normal terminal repaint rates.

Success means:

- semantic policy remains correct;
- command input remains usable;
- memory remains bounded;
- frames converge quickly to current state;
- real gaps/loss remain visible.

## 29.8 Large table target

A live state table with at least 100k logical rows in a synthetic fixture must remain navigable through virtualization.

The implementation MUST NOT format all rows on every semantic update.

# 30. Concurrency Model

## 30.1 Single terminal UI authority

v0.8 remains authoritative: one component owns terminal drawing/input routing.

Background live producers never write to terminal directly.

## 30.2 Semantic updates arrive asynchronously

Live view state updates may be delivered asynchronously from provider/pipeline tasks.

They SHOULD enter a host-controlled bounded update path that preserves required ordering/identity semantics.

## 30.3 Frame scheduling is separate

The UI loop may mark dirty state and paint at its next allowed frame rather than immediately from the producer task.

## 30.4 No lock inversion with terminal writes

Producer callbacks MUST NOT hold domain/provider locks while waiting for terminal I/O.

## 30.5 Close/update race

If a view closes while an update is in flight:

- update must not resurrect the closed view;
- cancellation/handle generation guards SHOULD make late updates no-ops;
- semantic execution cancellation follows its own existing lifecycle.

## 30.6 Replace-primary race

If a live primary is replaced by a new command result while updates arrive:

- the old view handle becomes background/closed deterministically;
- late frames from old view MUST NOT overwrite the new primary;
- background job may continue if semantic job state says so.

## 30.7 Resize/update race

Resize and live update may occur concurrently.

The host SHOULD serialize presentation-state application sufficiently to produce a coherent final layout.

Semantic requery is not required solely for resize.

## 30.8 Child handoff/update race

Once terminal lease handoff begins, no live frame may be written until the Deck owns the lease again.

Updates can continue to affect bounded view state off-screen.

# 31. Failure Containment

## 31.1 Renderer failure does not become stream truth

If rendering one frame fails, the semantic stream/job should not automatically be marked failed unless the failure is part of the semantic consumer contract.

The host SHOULD attempt:

1. record diagnostic;
2. reset/rebuild presentation state as needed;
3. full redraw;
4. degrade to simpler rendering if necessary;
5. cancel only if the view cannot continue safely.

## 31.2 Source failure is not renderer failure

A provider/job error must be shown as source/execution failure, not generic UI failure.

## 31.3 Display-buffer allocation failure

Under memory pressure, the view SHOULD reduce/evict presentation retention before compromising shell stability.

If browsing history is lost, show view truncation.

Do not discard correctness-sensitive semantic events without canonical policy.

## 31.4 Terminal write failure

Terminal write failure follows v0.8 host recovery/degradation.

The renderer MUST not spin retrying at high rate.

## 31.5 Plugin view failure

A plugin view crash/protocol violation degrades/closes the plugin view according to KUANG isolation.

The shell/Deck survives.

## 31.6 Corrupt update

Schema/protocol violations from provider/plugin are structured errors.

The view MUST NOT "best effort" reinterpret malformed values into plausible rows.

# 32. Configuration

## 32.1 Keep configuration small

v0.9 SHOULD add only settings justified by real long-running UX/resource needs.

Potential settings:

```text
live_view.max_fps
live_view.append.max_records
live_view.append.max_bytes
live_view.background_fps
live_view.change_emphasis_duration
```

Names are illustrative.

## 32.2 No per-command dashboard config

v0.9 MUST NOT introduce configuration such as:

```yaml
panels:
  - command: watch process
    x: 0
    y: 0
    width: 50
  - command: watch network
    x: 50
```

## 32.3 Frame-rate override

A user may reduce max repaint frequency for battery/remote/low-power use.

Increasing the limit must remain subject to safe host ceilings.

## 32.4 Retention override

Users MAY configure bounded display retention.

The implementation SHOULD apply a hard safety ceiling or warn when configuration can consume excessive memory.

Changing display retention MUST NOT change provider sampling or recorder retention.

## 32.5 Key bindings

Follow/browse actions integrate with existing keymap/view actions.

The release SHOULD avoid hard-coding a second keybinding subsystem.

# 33. Observability and Debugging

## 33.1 Debug categories

Useful trace categories MAY include:

```text
live.bind
live.follow
live.retention
live.render
live.source-status
live.handoff
live.truncation
```

These are implementation diagnostics, not public semantic objects.

## 33.2 Binding inspection

A sanitized debug dump MAY show:

```text
primary view        table
execution           job #7
command             watch process --every 1s
follow              browsing
anchor              process lifetime ref ...
retained rows       231 / 10000
retained bytes      1.7 MiB / 16 MiB
last semantic update 14:03:18.112
last frame          14:03:18.145
pending frame       false
```

## 33.3 Do not dump sensitive values

Debug traces SHOULD record IDs/counts/timing rather than full row payloads by default.

## 33.4 Frame counters

Development metrics MAY track:

```text
semantic updates processed
view mutations
frames requested
frames painted
frames coalesced
bytes written
input latency
retention evictions
```

## 33.5 Coalescing ratio is not data-loss ratio

Diagnostics MUST name this clearly.

A line such as:

```text
frames coalesced 1204
```

must not be interpretable as "1204 events dropped".

# 34. Compatibility and Degradation Matrix

The implementation MUST test at least:

- **Local modern terminal:** full live Deck integration.
- **tmux:** live view, browse/follow, resize and input responsiveness.
- **GNU screen:** same with capability limits.
- **SSH:** live rendering with latency/bandwidth variation.
- **mosh:** only where redraw semantics are verified.
- **`TERM=dumb`:** no Deck; existing stream/plain behavior.
- **stdout redirected:** no Deck view; ordinary stream serialization/rendering.
- **stdin non-TTY:** no hidden interaction.
- **No color:** all live/currentness/gap/truncation states textual.
- **ASCII mode:** usable status markers.
- **Narrow terminal:** live view remains readable or degrades to Rich TTY.
- **Slow terminal transport:** frame rate drops without semantic change.
- **Child TUI:** terminal handoff suppresses live frames.

Degradation remains:

```text
Deck live view
  -> lower-rate/simpler Deck view
  -> Rich TTY existing live rendering
  -> plain deterministic stream output
```

# 35. User Experience Reference Interactions

## 35.1 Live process table

```text
$ ono deck
local://~ > watch process --every 1s &
```

Primary:

```text
+--------------------------------------------------+
| LIVE  job #4  watch process      last 0.3s      |
|                                                  |
| PID   NAME        CPU    MEMORY   STATE           |
| 1821  postgres    44.1%  3.2GiB   sleeping        |
| 9291  node        31.0%  812MiB   running         |
| 4419  rustc       18.2%  2.1GiB   running         |
|                                                  |
+--------------------------------------------------+
| local://~ > _                                    |
+--------------------------------------------------+
```

The view uses the existing background job.

## 35.2 Browse live process table

The user moves selection and scrolls away from the current sorted region.

Status may become:

```text
BROWSING  +24 updates   follow latest
```

Processes continue to update.

The selected process remains anchored by stable lifetime identity.

## 35.3 Selected process disappears

```text
process/1821 postgres exited
```

The view does not silently retarget selection to the new row at the same coordinates.

An explicit `inspect @selection` must resolve whatever is currently actually selected.

## 35.4 Live log following

```text
watch log --service nginx &
```

The newest log rows appear.

The user pages upward.

The viewport stops following while records continue to arrive.

```text
BROWSING  +318 updates   End: latest
```

Returning to latest does not restart the command.

## 35.5 View truncation

After enough high-rate log records:

```text
--- Deck view history truncated; older rows were not retained ---
14:03:11 info  ...
```

This is not called a coverage gap.

## 35.6 Real coverage gap

If v0.5 evidence reports a source gap:

```text
14:03:17 warn  ...
14:03:18 ---- coverage gap: source disconnected 4.2s ----
14:03:22 info  ...
```

The gap remains visible even after follow resumes.

## 35.7 External editor while watch runs

```text
watch process &
vim /etc/nginx/nginx.conf
```

Expected:

```text
Deck stops terminal writes
vim owns terminal normally
background watch continues according to job semantics
vim exits
Deck reacquires terminal
one current frame is redrawn
no replay storm of missed frames
```

## 35.8 New command replaces live primary

While `watch process &` is primary:

```text
get service nginx
```

The service result becomes primary.

The watch continues as job #4 and is reopenable through jobs.

No duplicate provider subscription is started.

## 35.9 Live map

`map --live` uses the v0.5 temporal controls.

Pressing Space pauses its temporal cursor, not the pipeline/provider.

Generic v0.9 follow/browse does not override those semantics.

# 36. Acceptance Test Matrix

## 36.1 No new live semantic types

Static/code inspection MUST verify no public v0.9 semantic schema equivalent to:

```text
Live<T>
StateObservation<T>
EventObservation<T>
StreamGeneration
Watermark
```

is introduced without a separately approved ADR outside this release intent.

## 36.2 Existing `watch` parity

Run the same deterministic `watch` fixture in:

- Rich TTY;
- Deck;
- serialized/piped mode.

Assert identical semantic values/order according to source contract.

Only presentation differs.

## 36.3 Follow/browse

Start append fixture.

Assert:

1. starts following;
2. scroll away enters browsing;
3. incoming records do not move viewport to latest;
4. unseen indicator grows;
5. follow-latest returns without source restart;
6. no duplicate subscription occurs.

## 36.4 State selection stability

Update/reorder selected row repeatedly.

Assert selection tracks stable identity, not numeric row.

## 36.5 PID reuse

Use deterministic lifetime fixture.

Assert selection/reference does not cross lifetimes.

## 36.6 Display truncation

Set tiny append retention limit.

Generate more rows than fit.

Assert older display rows evict and view shows truncation marker when relevant.

Assert no v0.5 gap object is fabricated.

## 36.7 Real gap

Feed canonical v0.5 gap fixture.

Assert gap is shown distinctly from display truncation.

## 36.8 Frame coalescing

Generate 10,000 semantic updates in a short burst.

Assert:

- canonical semantic/view state processes required updates;
- terminal frames are far fewer;
- final visible state matches canonical state;
- diagnostics call this frame coalescing, not event dropping.

## 36.9 Input latency

Type continuously during high live load.

Assert p95/p99 target on reference hardware.

## 36.10 Child handoff

Run a background live fixture and repeatedly invoke a terminal-owning child fixture.

Assert no Deck bytes are written while child owns terminal and first return frame is coherent.

## 36.11 Suspend/resume

Suspend/resume Ono during a live job.

Assert terminal restoration, full redraw and truthful continuity status.

## 36.12 Replace-primary race

Replace live primary with static result while updates arrive.

Assert old late frames never overwrite new result.

## 36.13 Background reopen

Background live job, replace/close its view, then reopen it.

Assert same semantic execution is reused rather than duplicated.

## 36.14 Static result remains static

Mount `get process`, wait and change system process state.

Assert result does not silently refresh.

## 36.15 Remote reconnect

Use deterministic remote fixture supporting proven and unknown continuity cases.

Assert presentation distinguishes them.

## 36.16 No-color/ASCII

Assert `LIVE`, `BROWSING`, failure, gap and truncation remain legible without color/Unicode decoration.

# 37. Performance and Soak Test Matrix

## 37.1 State-table burst

Fixture:

```text
100k logical objects
10k object updates/s burst
120x40 terminal
```

Assert virtualized formatting, bounded pending render work and responsive input.

## 37.2 Append burst

Fixture emits event records faster than terminal frames.

Assert bounded presentation retention and no unbounded formatting queue.

## 37.3 Eight-hour soak

Run at least one representative state watch and one append-source test (sequentially or in controlled background jobs) for eight hours in CI/nightly infrastructure where feasible.

Track heap, tasks, FDs, retained buffers and frame counters.

## 37.4 Resize storm under load

Resize terminal continuously while live state updates.

Assert no panic, no stale cell corruption and convergence after storm.

## 37.5 Slow SSH simulation

Throttle terminal output bandwidth/latency.

Assert frame rate/bytes decrease while semantic source behavior remains unchanged.

## 37.6 View open/close churn

Open/close/reopen a running live job view thousands of times in fixture.

Assert no duplicate subscriptions or handle leaks.

# 38. Unit, Property and Integration Test Strategy

## 38.1 Unit tests

Required areas:

- follow-state transitions;
- unseen update counter behavior;
- stable anchor selection;
- view truncation marker logic;
- display-retention eviction;
- frame-deadline/coalescing scheduler;
- currentness age formatting;
- no-color/ASCII status rendering;
- late-update generation guards;
- background/unmounted buffer release.

## 38.2 Property tests

Useful properties:

- follow -> browse -> follow never changes underlying execution ID;
- viewport navigation never changes `ContextStack`;
- display eviction never constructs a `TemporalGap`;
- frame coalescing never changes final state projection for the same ordered semantic updates;
- selected stable identity never changes solely because sort order changes;
- a closed view cannot be resurrected by late updates;
- bounded append retention never exceeds configured record/byte ceiling beyond documented accounting slack.

## 38.3 PTY tests

PTY tests MUST cover:

- live redraw + typing;
- resize;
- copy/browse behavior where testable;
- Ctrl-C/Ctrl-Z existing semantics;
- child alternate-screen handoff;
- suspend/resume;
- no control-sequence injection from content.

## 38.4 Provider fixtures

Deterministic fixtures should model:

- snapshot polling;
- event subscription;
- bursty updates;
- stable identity;
- object disappearance/reappearance;
- disconnect/reconnect with continuity;
- disconnect/reconnect without continuity;
- explicit source loss/gap.

## 38.5 v0.5 integration fixtures

Tests MUST use canonical temporal event/gap structures rather than v0.9-local stand-ins.

# 39. Documentation Requirements

Documentation for v0.9 MUST explain:

- that Ono already uses `Stream<T>`/`watch` for live data;
- foreground vs background live commands;
- how Deck follow/browse works;
- that browsing does not pause the source;
- how to return to latest;
- difference between view truncation and real temporal/source gaps;
- why a static result does not auto-refresh;
- what happens when a live job continues while another command becomes primary;
- how to reopen background live jobs;
- resource/retention defaults;
- v0.5 `map --live` specialized pause/rewind behavior;
- terminal handoff behavior with interactive external programs.

Troubleshooting MUST include:

```text
view seems frozen but says BROWSING
live source ended/failed
updates delayed
view history truncated
remote continuity unknown
high CPU from provider vs high CPU from rendering
background watch still running
```

# 40. Implementation Phases

The phases below are work packages, not permission to create new semantics.

## Phase L1 - Inventory and deletion/consolidation audit

Deliver:

- map every existing live path in v0.2-v0.8;
- identify any prototype `Live<T>`/duplicate observation/backpressure code;
- produce deletion/migration list;
- verify provider and KUANG stream APIs already satisfy required inputs.

Gate:

```text
team can explain one canonical path from provider -> Stream<T> -> view -> Deck
without a second live runtime
```

## Phase L2 - Live-view binding

Deliver:

- internal binding between existing execution/job and view handle;
- ownership/cancellation rules;
- no duplicate subscriptions;
- close/reopen behavior.

Gate:

```text
one background watch can be mounted, replaced, reopened and cancelled
without semantic re-execution or leaks
```

## Phase L3 - Follow/browse

Deliver:

- generic two-state follow model;
- stable view anchors;
- unseen update indicator;
- return-to-latest action;
- resize preservation.

Gate:

```text
user can read older live content while source continues and return to latest
without rerunning or retargeting anything
```

## Phase L4 - Bounded display retention

Deliver:

- append record/byte budgets;
- eviction;
- view truncation landmark;
- background-view buffer release;
- debug accounting.

Gate:

```text
arbitrarily long append stream cannot create arbitrarily large Deck history
and truncation is never called a source gap
```

## Phase L5 - State-table stability

Deliver:

- identity-based row materialization;
- selection stability;
- reorder policy;
- disappearance/PID-reuse correctness;
- virtualization.

Gate:

```text
rapidly changing large state table remains readable and never transfers
selection across object lifetimes
```

## Phase L6 - Render scheduler

Deliver:

- frame ceiling;
- invalidation coalescing;
- typing priority;
- background frame reduction;
- low-bandwidth behavior;
- instrumentation.

Gate:

```text
semantic burst rates can greatly exceed terminal frame rate without
unbounded UI queues or changed semantic results
```

## Phase L7 - Temporal/continuity integration

Deliver:

- canonical v0.5 gap rendering in live Deck;
- separation from display truncation;
- currentness/age facts;
- reconnect continuity presentation;
- `map --live` integration tests.

Gate:

```text
Deck never paints a continuous/current-looking story where canonical
temporal/source evidence says continuity is partial, absent or unknown
```

## Phase L8 - Terminal lifecycle hardening

Deliver:

- child handoff under live load;
- suspend/resume;
- resize/update races;
- terminal reacquire full redraw;
- no background ANSI writes.

Gate:

```text
live jobs coexist with normal Unix terminal software without corrupting
or fighting for terminal ownership
```

## Phase L9 - KUANG/11 and security hardening

Deliver:

- plugin live-view quota tests;
- host frame control;
- stream/view cancellation conformance;
- malicious live content sanitization;
- secret retention tests.

Gate:

```text
third-party live views remain bounded and isolated without a new plugin UI API
```

## Phase L10 - Soak, documentation and release proof

Deliver:

- long-running soak tests;
- performance baselines;
- user documentation;
- migration/deletion confirmation;
- architecture review against anti-requirements.

Gate:

```text
v0.9 makes existing live behavior better for hours-long use while the
semantic architecture is smaller than the abandoned Live<T> direction
```

# 41. Deliverables

A release-quality v0.9 implementation MUST deliver:

- live-view binding using existing execution/job/view identities;
- generic follow/browse behavior;
- bounded append-view retention;
- explicit view-truncation presentation;
- identity-stable live state tables;
- redraw/frame coalescing separate from semantic backpressure;
- input-priority scheduler behavior;
- currentness/source-status presentation from existing facts;
- canonical v0.5 gap/coverage integration;
- `map --live` compatibility inside Deck;
- safe child terminal handoff under background live jobs;
- suspend/resume hardening;
- remote continuity presentation;
- bounded KUANG live-view resource policy;
- soak/performance/security test coverage;
- documentation explaining semantic vs presentation boundaries.

# 42. Explicit Non-Goals / Anti-Requirements

## 42.1 No `Live<T>`

Do not add a second type constructor for live values.

## 42.2 No second Observation model

Do not add `Observation<T>`, `StateObservation` or `EventObservation` beside v0.5 semantics.

## 42.3 No second gap model

Display truncation is not a temporal gap.

## 42.4 No Watermark/Generation ontology for UI convenience

If a transport/provider already exposes sequences/resume tokens, retain them in their canonical subsystem.

Do not promote generic stream-processing concepts into Ono's public model merely because the Deck needs redraw safety.

## 42.5 No new semantic backpressure policy

Frame coalescing is not pipeline flow control.

## 42.6 No automatic static refresh

`get` does not become `watch` because it is visible in Deck.

## 42.7 No hidden recording

Live views do not create durable time-series/log history.

## 42.8 No dashboard grid

One strong live primary beats a pane manager.

## 42.9 No pinning/layout persistence

Not in v0.9.

## 42.10 No universal pause-stream key

Generic browsing pauses viewport following, not semantic execution.

## 42.11 No new reconnect protocol

Presentation uses existing source/link facts.

## 42.12 No renderer-triggered automation

No threshold/actions/alerts.

## 42.13 No second jobs model

A live view does not have its own running/stopped lifecycle beside `Job`/view lifecycle.

## 42.14 No plugin raw terminal ownership

Existing constrained view contract remains.

## 42.15 No fake liveness

No blinking heartbeat if there is no meaningful heartbeat fact.

## 42.16 No fake continuity

A successful redraw after disconnect does not prove nothing was missed.

## 42.17 No infinite scroll promise

Bounded display history is honest and finite.

## 42.18 No semantic use of screen coordinates

Rows move; object identity does not derive from row number.

## 42.19 No cyberpunk terminology that hides operational truth

Themes may later restyle presentation, but v0.9 core labels must remain understandable.

# 43. Architecture Review Checklist

Before merging a significant v0.9 subsystem, reviewers SHOULD ask:

1. Which earlier specification owns the semantic concept?
2. Is this truly presentation-local?
3. Why is `Stream<T>` insufficient here?
4. Are we accidentally rebuilding `Observation`/`TemporalEvent`?
5. Are we accidentally creating a second gap type?
6. Does this change provider behavior based on viewport state?
7. Does scrolling pause or backpressure the source unexpectedly?
8. Is render coalescing being confused with event dropping?
9. Is any queue unbounded?
10. Is any view buffer becoming hidden history?
11. Could display eviction be mistaken for source loss?
12. Does reopening a live job duplicate the subscription?
13. Does a static result remain static?
14. Can selection transfer across object lifetime/PID reuse?
15. What happens while the user is typing under update burst?
16. What happens while a child owns the terminal?
17. What happens on suspend/resume?
18. What continuity can actually be proven after reconnect?
19. Does v0.5 `map --live` still behave exactly as specified?
20. Does the plugin path reuse existing stream/view APIs?
21. Is a new manifest field really necessary?
22. Does this make Deck more dashboard-like without strong evidence?
23. Could the same benefit be achieved with one primary live view plus jobs?
24. Would deleting this new abstraction change semantic command results? If yes, it may not be presentation-local.
25. Does the implementation delete/reject more duplicate live machinery than it adds?

# 44. End-to-End Reference Scenarios

## 44.1 Observe and investigate without dashboarding

1. User runs `watch process --service nginx &`.
2. Watch becomes background job #4 and its state table is primary.
3. User browses a busy process while updates continue.
4. Status shows `BROWSING +N updates`.
5. User returns to latest.
6. User runs `get service nginx`.
7. Static service result replaces primary.
8. Job #4 continues.
9. User opens jobs auxiliary and reopens #4.
10. Existing execution is reused.

Success criterion: continuous investigation using one workspace, no live pane grid and no duplicate provider subscription.

## 44.2 High-rate log incident

1. `watch log --service nginx &`.
2. Error burst produces thousands of records.
3. Semantic source/pipeline applies canonical policy.
4. View retains bounded records.
5. Terminal paints fewer frames than records.
6. User scrolls up; browsing remains stable.
7. Retention boundary eventually evicts older display rows.
8. Deck shows **view history truncated**.
9. A real source disconnect later produces canonical v0.5 gap evidence.
10. Gap is shown with different wording.

Success criterion: user can distinguish "Deck did not retain old screen rows" from "Ono did not observe part of reality".

## 44.3 Edit configuration while live watch continues

1. Start `watch service nginx &`.
2. Run `vim /etc/nginx/nginx.conf`.
3. Deck releases terminal fully.
4. Background watch continues according to job semantics.
5. Vim exits.
6. Deck reacquires and renders current service state once.
7. No frame backlog replays.

Success criterion: Ono remains a normal Unix shell even while live features are active.

## 44.4 Remote disconnect

1. User links to `prod-web-3`.
2. Starts background watch.
3. Network disconnects for five seconds.
4. Link/provider reconnects.
5. If sequence continuity is proven, Deck says so.
6. If not, Deck shows continuity unknown or canonical coverage gap.
7. It never simply resumes an uninterrupted-looking timeline without evidence.

Success criterion: visual continuity never outruns semantic evidence.

## 44.5 Spatial temporal investigation

1. User opens `map --live`.
2. v0.5 temporal map is mounted through the generic Deck host.
3. Space pauses its temporal cursor.
4. Providers/recorder continue.
5. User rewinds through real events.
6. Gap appears where evidence is incomplete.
7. User returns to now.

Success criterion: v0.9 contributes long-running hosting/render robustness, not a competing pause/time model.

## 44.6 Selected process PID reuse

1. Select process lifetime A at PID 812.
2. A exits.
3. PID 812 is reused by process lifetime B.
4. View update places B where A was.
5. Selection/reference does not silently carry over.
6. Explicit action requires normal identity resolution.

Success criterion: screen position never becomes semantic identity.

# 45. Release Acceptance Criteria

v0.9 is complete only when all of the following are true:

1. No new `Live<T>`/parallel observation/gap type is introduced.
2. Existing `watch` semantics remain unchanged outside presentation.
3. Live Deck views use existing stream/job/view identities.
4. A user can browse a live append/state view without stopping its source.
5. Returning to latest does not rerun the command.
6. Display retention is explicitly bounded.
7. View truncation is distinguishable from source/temporal gaps.
8. State-table selection survives reorder by stable identity and does not survive lifetime replacement incorrectly.
9. Terminal frame rate can be far lower than semantic update rate without unbounded frame queues.
10. Command editor responsiveness remains within target under load.
11. Static results never auto-refresh.
12. Background live jobs can survive primary replacement and be reopened without duplicate subscriptions.
13. `map --live` retains v0.5 pause/rewind/coverage semantics.
14. Child terminal handoff under live jobs is correct.
15. Suspend/resume produces a coherent full redraw and honest continuity status.
16. Remote reconnect does not imply unproven continuity.
17. KUANG/11 live views remain bounded through existing host/view contracts.
18. Long-running soak tests show no unbounded resource growth.
19. No dashboard/layout/pinning subsystem is introduced.
20. The final architecture is smaller conceptually than the abandoned v0.9 live-data reinvention.

# 46. Mandatory Post-v0.9 Reassessment

## 46.1 Stop before automatically writing v0.10

v0.9 completes the three-release Deck foundation sequence:

```text
v0.7  consolidate presentation
v0.8  compose persistent workspace
v0.9  integrate long-running existing live views
```

The project MUST NOT assume that v0.10-v0.12 are still necessary in their previously imagined form.

## 46.2 Questions to answer after dogfooding

Before approving another Deck-oriented release, evaluate:

- Is Rich TTY still used more often than Deck?
- Does Deck improve real workflows enough to justify terminal lifecycle complexity?
- Is one primary + auxiliary enough?
- Do users actually browse live views or mostly glance at latest?
- Does background-job reopening feel natural?
- Are follow/browse concepts intuitive without documentation?
- Does persistent currentness reduce mistakes?
- Does lost terminal scrollback remain a significant cost?
- Are v0.4/v0.5 full-screen views materially simpler because of shared host infrastructure?
- Have v0.7-v0.9 removed more duplication than they introduced?

## 46.3 Reassess the proposed object-interaction release

Earlier planning considered a v0.10 focused on targets/actions.

Before specifying it, diff the idea against existing:

- v0.2 selection and `@selection`/`ValueRef` concepts;
- `inspect`, `enter`, `trace`, mutation commands;
- v0.4 spatial navigation;
- v0.6 sealed plans/revalidation;
- existing `ObjectPicker`/`CommandPalette` view primitives.

A v0.10 is justified only if it **consolidates** those interactions rather than introducing a target state beside them.

## 46.4 Reassess theming separately

The Neuromancer/cyberdeck theme remains attractive, but theming should be small and semantic-token-based.

It must not become the justification for further architectural UI expansion.

# 47. Release Rationale

## 47.1 Why v0.9 still deserves a release after removing most of the old design

The rejected design made v0.9 look architecturally large because it introduced a complete live-data model.

The revised release is smaller in ontology but still solves a real product problem.

Ono already knew how to produce an unbounded stream.

What it did not yet specify rigorously was how a persistent Deck should behave when that stream remains visible for a long time.

The missing capabilities are interaction-level:

```text
follow latest
browse without stopping source
retain only bounded recent display history
keep identity stable while rows move
paint fewer frames than updates
survive child tools/suspend/reconnect
show real gaps differently from local truncation
```

Those concerns are substantial enough to warrant implementation and release hardening, but they do not need a second data architecture.

## 47.2 Why frame loss is acceptable but semantic loss is not

A terminal is a presentation device.

If process CPU changed three times between two frames, the user usually needs the correct latest CPU value, not three animated intermediate states.

By contrast, if a correctness-sensitive event stream loses an event, the user may lose evidence about the system.

Therefore v0.9 draws a hard boundary:

```text
semantic stream policy    protects meaning
view state                preserves required current/retained meaning
frame scheduler           protects responsiveness
terminal                  shows selected frames
```

## 47.3 Why bounded display history is healthier than hidden recording

Infinite scroll is seductive because it feels convenient.

But an infinite live view quietly becomes:

- a storage subsystem;
- a retention policy;
- a secret-handling problem;
- a persistence expectation;
- a performance liability.

Ono already has explicit temporal-recording semantics in v0.5.

The Deck should not create another recorder by accident.

## 47.4 Why one live primary is enough for this release

A dashboard grid is the easiest path from "interesting shell" to "feature monster".

Ono's actual composition primitive is already the shell job model:

```text
many running jobs
one primary thing being inspected
optional contextual auxiliary
```

That is enough to test whether persistent live operation is genuinely valuable.

If users later prove they repeatedly need two simultaneous update-bearing views, the architecture can be reconsidered with evidence.

## 47.5 Why this still fits the cyberdeck idea

A cyberdeck does not need twelve animated charts.

A single persistent live view that remains responsive while the operator navigates, inspects, edits and returns to current state can feel far more like a serious deck than a decorative dashboard.

The Neuromancer influence is strongest when the machine feels present and manipulable because real state persists - not when the screen is crowded.

\newpage

# 48. Closing Principle

The revised v0.9 should be judged by how **little new semantic architecture** is required to make Ono's existing live capabilities excellent inside the Deck.

The desired outcome is:

```text
same Stream<T>
same providers
same watch
same jobs
same temporal evidence
same views

+ bounded long-running presentation
+ stable follow/browse interaction
+ responsive redraw scheduling
+ honest continuity/truncation UX
```

The failure outcome is:

```text
new Live<T>
new observation envelope
new gap model
new backpressure runtime
new reconnect lifecycle
new dashboard system
new hidden recorder
```

The implementation MUST reject the second trajectory even if it produces a more impressive architecture diagram.

> **v0.9 succeeds when Ono can stay live for hours without becoming a monitoring suite - and when the code underneath becomes more consolidated, not more elaborate.**
