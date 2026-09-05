---
title: "ONO-SENDAI Specification v0.8"
subtitle: "Deck Workspace Composition & Terminal Ownership"
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
    \DefineVerbatimEnvironment{Highlighting}{Verbatim}{breaklines=true,breakanywhere=true,fontsize=\footnotesize,commandchars=\\\{\}}
    \setlength{\parskip}{0.45em}
    \setlength{\parindent}{0pt}
    ```
---

# ONO-SENDAI Specification v0.8
## Deck Workspace Composition & Terminal Ownership

**Status:** Product and architecture extension specification  
**Scope:** Persistent full-screen Deck workspace composed from the existing Ono view, history, context and rendering contracts; generic terminal ownership; bounded layout; focus and navigation; safe handoff to external terminal-owning processes; resilience and compatibility  
**Relationship:** Standalone extension to v0.2-v0.7; deliberately consolidating rather than replacing existing TUI/view semantics  
**Normative language:** MUST, MUST NOT, SHOULD, SHOULD NOT, MAY

> **The Deck is not a second application inside Ono. It is a persistent composition of views Ono already knows how to produce.**

---

# 0. Document Status and Relationship to Earlier Specifications

## 0.1 Standalone additive specification

This document defines Ono-Sendai v0.8. It does not replace, merge, rewrite or retrospectively edit the v0.2 baseline or the v0.3-v0.7 extension specifications.

The progression relevant to this release is:

```text
v0.2  Value / Stream<Value>, adaptive rendering, interactive selection,
      constrained view tree, view lifecycle, HistoryEntry, ResultRef,
      ContextStack, command metadata and KUANG/11 TUI lenses.

v0.3  Honest interoperability with external Unix programs.

v0.4  Spatial projection, full-screen map views and live topology.
      Shell and space are explicitly two projections of one system.

v0.5  Temporal truth: observations, events, evidence, coverage and gaps.

v0.6  Prospective change, impact, protection and recovery semantics.

v0.7  Presentation consolidation and production-quality Rich TTY.
      Existing render hints, profiles and view-tree concepts become one
      consistent human-presentation path.

v0.8  Deck workspace composition and terminal ownership.
      Existing views become persistently co-present without creating a
      second semantic model or a second plugin UI protocol.
```

All earlier concepts retain their earlier meaning unless this document explicitly narrows a presentation behavior required for Deck hosting.

## 0.2 Inheritance-first rule

v0.8 MUST reuse earlier concepts rather than create synonymous workspace concepts.

In particular, v0.8 inherits without replacement:

- `Value` and `Stream<T>`;
- `ValueRef`, `ObjectRef`, `ResultRef` and `HistoryEntry`;
- the v0.2 interactive-selection rule;
- the v0.2 constrained view tree and view lifecycle;
- command/schema/provider metadata;
- the `ContextStack` and active link/context semantics;
- KUANG/11 view contributions and capability boundaries;
- v0.4 spatial views, including full-screen and live map behavior;
- v0.5 temporal truth, freshness, coverage and gaps;
- v0.6 plan, impact, protection, recovery and verification semantics;
- v0.7 presentation resolution, Rich TTY capabilities and sanitization.

A v0.8 implementation MUST NOT add a parallel `WorkspaceValue`, `WorkspaceHistory`, `WorkspaceSelection`, `DeckResult`, `DeckContext`, `DeckStream`, `DeckObservation`, `DeckWidgetTree` or equivalent concept merely because a value is visible in the Deck.

## 0.3 What is genuinely new in v0.8

v0.8 introduces a small set of implementation-level capabilities that earlier releases did not fully generalize:

1. a **persistent Deck host** that composes multiple existing views around the shell editor;
2. a **generic terminal-ownership contract** shared by the Deck, existing full-screen Ono views and foreground external programs;
3. a **bounded layout policy** with one primary view, one optional auxiliary view and the shell command surface;
4. **workspace navigation state** that is explicitly ephemeral presentation state and never semantic shell context.

These capabilities are justified only because they remove duplicated full-screen hosting logic and allow existing Ono semantics to remain visible simultaneously.

## 0.4 No retrospective editing

Earlier specifications MUST NOT be changed merely to make the Deck easier to implement.

If v0.8 exposes a genuine contradiction between earlier full-screen/view contracts, the implementation MUST record an ADR. The ADR SHOULD prefer the older semantic concept and consolidate terminal/presentation mechanics around it.

## 0.5 Release thesis

v0.8 is built around four normative statements:

> **One model, multiple projections.**

> **The Deck composes views; it does not own domain truth.**

> **Full-screen terminal ownership is infrastructure, not a feature-specific hack.**

> **If the Deck is removed, Ono must still be a complete shell.**

## 0.6 Complexity budget

The release is explicitly constrained against feature-monster growth.

v0.8 MUST NOT require:

- a second semantic type system;
- a second history store;
- a second context model;
- a second stream/event model;
- a new KUANG/11 UI protocol;
- a general window manager;
- a dashboard builder;
- a terminal emulator;
- a public theme DSL;
- semantic object targeting beyond the selection/reference rules already defined;
- automatic command execution derived from UI state.

The preferred implementation is the one that achieves the Deck with the fewest new concepts while preserving correctness.

# 1. Product Thesis

## 1.1 Ono already contains the ingredients of a Deck

The baseline already states that Ono is a systems interface rather than only a command interpreter. It already supports interactive selection, constrained TUI views, timeline/history views, object pickers, status lines, live streams, spatial maps and plugin-provided interactive lenses.

The missing capability is not "a TUI framework".

The missing capability is **persistence of composition**.

In ordinary Rich TTY operation, each result is rendered into terminal flow and becomes scrollback. A Deck keeps a small number of views visible while the command editor remains available.

That difference is intentionally narrow:

```text
Rich TTY
command -> result -> scrollback -> command -> result

Deck
+--------------------------------------+-------------------+
| primary existing view                | auxiliary view    |
|                                      |                   |
+----------------------------------------------------------+
| command editor / prompt                                  |
+----------------------------------------------------------+
| existing StatusLine semantics                            |
+----------------------------------------------------------+
```

The values, commands, history records, references and contexts behind both presentations are the same.

## 1.2 The Deck is a presentation host, not a semantic mode

The Deck MAY be described as a "Deck Mode" in user-facing language because the terminal behaves differently, but implementations MUST NOT treat it as a different language or execution environment.

The following MUST resolve identically whether the shell is presented as Rich TTY or Deck:

```text
get process | where cpu > 20
@-1 | sort memory desc
enter service nginx
plan restart service nginx
impact @plan
watch process
map --live
```

Presentation state MAY differ. Semantic evaluation MUST NOT.

## 1.3 Persistence is the value

The Deck is justified when persistence reduces cognitive and interaction cost.

Examples:

- the command line remains visible while a large result is inspected;
- current execution context remains visible while history is browsed;
- a v0.6 plan remains inspectable while a related command is composed;
- an existing spatial map can occupy the primary view while shell input remains available;
- a live view already defined by earlier specs can remain mounted without consuming terminal scrollback;
- history can be browsed through `HistoryEntry`/`ResultRef` rather than screen scraping.

A feature that does not benefit from persistent composition SHOULD remain an ordinary Rich TTY interaction.

## 1.4 The Deck is optional in v0.8

v0.8 MUST NOT make the Deck the default startup presentation.

The release must earn that possibility through dogfooding rather than assume it.

The canonical default remains the v0.7 Rich TTY path. An explicit invocation such as:

```text
ono deck
```

MAY start the Deck presentation host.

The exact startup spelling may be finalized by CLI conventions, but v0.8 MUST provide an explicit, discoverable way to request the Deck without changing script/non-TTY behavior.

## 1.5 Emotional thesis

The Deck may feel more like an operator console, but the feeling must come from real system state.

Good:

- a real remote link is visible;
- a real result remains mounted;
- a real live stream updates;
- a real v0.6 recovery asset is visible;
- a real v0.5 gap is marked;
- a real spatial relation can be explored.

Bad:

- decorative panes containing invented telemetry;
- fake boot sequences;
- simulated scanning;
- permanent "activity" merely to make the screen look alive;
- cyberpunk labels that obscure ordinary system terminology.

The Neuromancer/cyberdeck identity may later be expressed through theming, but v0.8 MUST work as a restrained professional interface first.

# 2. Core Invariants

The following are release-level invariants.

1. **One semantic execution path.** Deck and Rich TTY MUST call the same parser, evaluator, pipeline, provider and mutation code.
2. **One value system.** Deck-visible data MUST remain ordinary Ono `Value`/`Stream<T>` data.
3. **One history.** Deck browsing MUST use existing `HistoryEntry` and `ResultRef` semantics.
4. **One context stack.** Focus or visual selection MUST NOT mutate the `ContextStack`.
5. **Selection remains ephemeral.** The baseline rule that selection does not change pipeline data remains authoritative.
6. **Explicit action remains required.** A highlighted row MUST NOT become an implicit target.
7. **Views remain constrained.** The existing view tree remains the UI contribution boundary; plugins do not receive terminal ownership.
8. **Terminal ownership is singular.** Exactly one component/process owns interactive terminal state at a time.
9. **Full-screen is reversible.** Leaving, suspending or crashing out of the Deck MUST attempt to restore a usable terminal.
10. **Scrollback loss is not hidden.** The Deck uses a full-screen presentation; the product MUST not pretend terminal scrollback behaves like Rich TTY.
11. **External programs remain real programs.** Ono MUST preserve direct TTY/PTY semantics rather than emulate arbitrary terminal applications inside a pane.
12. **Existing live semantics remain authoritative.** v0.8 hosts `Stream<T>`, `watch`, `map --live` and other earlier live views; it does not invent `Live<T>`.
13. **Temporal honesty persists.** Historical, stale, partial and proposed data MUST remain visibly distinct using earlier contracts.
14. **Safety is semantic, not chromatic.** A green/clean-looking Deck surface MUST never imply a v0.6 plan is safe unless the underlying plan semantics say so.
15. **No layout DSL.** The Deck is a bounded composition, not a terminal window manager.
16. **No essential mouse state.** Every required action MUST be keyboard-accessible.
17. **No hidden persistence.** Ephemeral workspace state MUST NOT silently become durable semantic state.
18. **No presentation-triggered mutation.** Rendering, focusing, scrolling, selecting or switching auxiliary views MUST be side-effect free.
19. **Failure degrades to shell.** A Deck-specific rendering failure SHOULD fall back to a usable shell rather than terminate the semantic session where practical.
20. **Subtraction test.** Any new v0.8 abstraction SHOULD remain useful for existing v0.4/full-screen views even if the Deck concept is later abandoned.

# 3. Terminology and Minimal Conceptual Model

## 3.1 Deck

**Deck** is the user-facing name for a persistent full-screen presentation of an Ono shell session.

It is not a different shell, not a different evaluator and not a different source of truth.

## 3.2 Deck host

The **Deck host** is the shell-owned presentation component that:

- acquires the terminal through the generic terminal-ownership contract;
- mounts existing Ono views;
- keeps the line editor/prompt available;
- computes the bounded layout;
- routes input according to focus;
- responds to resize/suspend/resume;
- hands the terminal to foreground external processes when required;
- restores the terminal on exit/failure.

The Deck host MUST NOT contain process, service, filesystem, spatial, temporal or change-plan domain logic.

## 3.3 Primary view

The **primary view** is the main mounted existing Ono view.

Typical content:

- the latest native result rendered through the v0.7 path;
- a historical `ResultRef` explicitly opened by the user;
- a v0.4 map;
- a v0.5 timeline;
- a v0.6 plan/impact/recovery view;
- an existing `watch`/stream view;
- a KUANG/11 interactive lens already permitted by v0.2.

`PrimaryView` SHOULD NOT become a semantic public type. It is a host slot containing a normal view handle.

## 3.4 Auxiliary view

The **auxiliary view** is one optional secondary slot.

It MAY display one of a bounded set of existing views such as:

- current context summary;
- history/timeline navigation;
- command help;
- active jobs/status;
- provenance/detail information;
- v0.6 protection/recovery detail.

Only one auxiliary view is required at a time. Wide terminals MAY show it beside the primary view. Narrow terminals MUST collapse it behind a tab/switch action rather than invent additional columns.

## 3.5 Command surface

The **command surface** is the existing Ono prompt/line editor from the normal interactive shell, embedded persistently at the bottom of the Deck.

It is not a view-plugin slot and MUST remain shell-owned.

## 3.6 Status line

The existing `StatusLine` view concept MAY be used for bounded persistent status:

- active link/host;
- active context;
- privilege indicator;
- background job count;
- currently mounted auxiliary kind;
- degraded terminal/render state.

The status line MUST NOT become an unbounded notification feed.

## 3.7 Focus

**Focus** is ephemeral presentation state indicating which host surface receives navigation/editing keystrokes.

Focus is not semantic context.

Changing focus MUST NOT:

- push/pop `ContextStack` frames;
- change cwd;
- change remote link;
- mutate selection in another view unless the view itself receives the action;
- create a `ValueRef`;
- execute a command.

## 3.8 Terminal lease

The **terminal lease** is a v0.8 implementation contract describing exclusive ownership of interactive terminal state.

It exists because Ono already has multiple legitimate full-screen/interactive actors:

- the Deck host;
- v0.4 full-screen map hosting outside the Deck;
- other existing full-screen Ono views;
- foreground external terminal applications;
- the normal Rich TTY shell.

The lease is infrastructure, not user data and not part of the Ono language type system.

## 3.9 No new selection type

v0.8 MUST NOT define `PresentationSelection`, `WorkspaceSelection` or `DeckSelection`.

The v0.2 rule remains sufficient:

> A rendered collection may expose an ephemeral selection cursor. Selection never changes pipeline data by itself; explicit input is required to act on it.

The Deck host stores only the view-private cursor/navigation state needed to restore the view.

# 4. Architecture: Composition Instead of Duplication

## 4.1 Required layering

The required architecture is conceptually:

```text
+-----------------------------------------------------------+
| Deck Host                                                 |
| layout, focus, terminal lease, input routing, redraw      |
+---------------------------+-------------------------------+
                            |
+---------------------------v-------------------------------+
| Existing Ono View Protocol / View Tree                    |
| Text Table Tree Graph KeyValue LogStream Tabs Split ...   |
+---------------------------+-------------------------------+
                            |
+---------------------------v-------------------------------+
| v0.7 Presentation Resolution                              |
| Value + metadata + terminal capability -> presentation   |
+---------------------------+-------------------------------+
                            |
+---------------------------v-------------------------------+
| Existing Semantic Core                                    |
| evaluator, Value, Stream, HistoryEntry, ResultRef,        |
| ContextStack, providers, v0.4/v0.5/v0.6 domain models     |
+-----------------------------------------------------------+
```

No command/provider may need `if deck_mode` logic to produce a different semantic result.

## 4.2 Reuse the v0.2 view tree

v0.8 MUST reuse the existing constrained view tree rather than introduce a second `RenderModel`, `WidgetTree`, DOM-like tree or plugin-specific pane format.

Existing conceptual nodes include:

```text
Text
Table
Tree
Graph
KeyValue
LogStream
Sparkline
Gauge
Tabs
Split
CommandPalette
ObjectPicker
StatusLine
```

v0.8 MAY add host-private layout wrappers if required by implementation, but they MUST NOT become a second public contribution API.

## 4.3 Reuse the v0.2 view lifecycle

The canonical lifecycle remains:

```text
created -> mounted -> focused <-> background -> closed
```

The Deck host MUST deliver resize, focus and cancellation events through the established view contract.

It MUST NOT define a parallel lifecycle with new states such as `loaded`, `visible`, `selected`, `pinned`, `suspended` or `detached` unless an existing view truly needs a distinct state and an ADR justifies it.

## 4.4 Generic full-screen host

The preferred implementation extracts/grows a generic full-screen host used by both:

- the persistent Deck composition;
- pre-existing full-screen Ono views such as the v0.4 map when invoked outside the Deck.

This avoids one terminal-state implementation for maps and another for the Deck.

Conceptually:

```text
FullScreenHost
  owns TerminalLease
  owns screen buffer / redraw
  mounts one root view tree
  routes focus/input

DeckComposition
  root view tree + persistent command editor

SpatialMapFullscreen
  root map view tree
```

Exact crate/type names are not normative. Shared responsibility is.

## 4.5 No domain cache in the host

The Deck host MAY cache rendered cells, layout calculations and view-private cursor positions.

It MUST NOT maintain authoritative copies of:

- process/service state;
- spatial topology;
- temporal observations;
- change-plan state;
- recovery assets;
- remote-link identity;
- history records.

Those remain owned by their existing systems.

## 4.6 One event authority for screen state

The full-screen host SHOULD serialize terminal/UI state transitions through one event authority. This may be an event loop, dispatcher or actor, but it MUST avoid concurrent writers issuing terminal control sequences independently.

Semantic command execution MAY be asynchronous. Screen mutation MUST remain coordinated.

# 5. Deck Entry, Exit and Availability

## 5.1 Explicit startup

v0.8 MUST provide explicit Deck startup. A recommended CLI shape is:

```text
ono deck
```

Equivalent option spelling MAY be chosen if repository CLI conventions require it.

The Deck MUST NOT activate merely because stdout is a capable TTY.

## 5.2 Non-TTY invocation

When the Deck is explicitly requested but required terminal capabilities are absent, Ono MUST:

1. emit a concise diagnostic;
2. avoid writing alternate-screen/raw-mode sequences;
3. either continue as normal Rich TTY/plain interactive shell when meaningful or exit with a documented nonzero code if no interactive terminal exists;
4. never alter command semantics to compensate.

Example:

```text
ono deck
error: Deck requires an interactive terminal with cursor addressing.
hint: starting normal interactive presentation instead.
```

Fallback behavior MUST be deterministic and configurable for tests.

## 5.3 Default remains Rich TTY

v0.8 MUST NOT make Deck startup the shipping default.

A user MAY configure Deck startup after installation. Product defaults should change only after sustained dogfooding demonstrates that the full-screen trade-offs are beneficial.

## 5.4 Exiting the Deck

v0.8 MUST distinguish:

- leaving the full-screen Deck presentation;
- exiting the shell process.

If in-process return to Rich TTY is implemented, it MUST preserve the same semantic session, history and context.

If the first v0.8 implementation only supports Deck for the lifetime of a shell invocation, ordinary shell-exit behavior MAY terminate the process. The implementation MUST NOT create complicated mode-switching machinery merely to satisfy a cosmetic toggle requirement.

## 5.5 In-process toggling is optional

A Rich-TTY-to-Deck toggle is explicitly NOT required for v0.8 Definition of Done.

This is deliberate scope control. `ono deck` is sufficient to validate the product concept.

## 5.6 Nested Deck requests

Starting a second Deck host inside an existing Deck session SHOULD be rejected or normalized to the current host.

Ono MUST NOT recursively acquire the same terminal lease.

# 6. Bounded Workspace Composition

## 6.1 Three required surfaces

The canonical Deck has only three persistent functional surfaces:

1. **Primary view** - main result/view;
2. **Auxiliary view** - optional secondary information;
3. **Command surface** - prompt/editor.

An existing `StatusLine` MAY be rendered as a thin fourth visual strip but is not a general pane.

This replaces the earlier draft's proliferation of separate Result, Context, Session and Activity regions.

## 6.2 Wide composition

Recommended wide layout:

```text
+-----------------------------------------+------------------+
| PRIMARY                                 | AUXILIARY        |
|                                         |                  |
|                                         |                  |
|                                         |                  |
+------------------------------------------------------------+
| COMMAND                                                    |
| local://~ > _                                              |
+------------------------------------------------------------+
| STATUS                                                     |
+------------------------------------------------------------+
```

The exact border glyphs and proportions are non-normative.

## 6.3 Medium composition

When width is insufficient for side-by-side views:

```text
+------------------------------------------------------------+
| PRIMARY                                                    |
|                                                            |
+------------------------------------------------------------+
| COMMAND                                                    |
+------------------------------------------------------------+
| STATUS   aux:context  (switch: history/help/jobs)          |
+------------------------------------------------------------+
```

The auxiliary view becomes hidden/collapsed and is opened on demand.

## 6.4 Narrow composition

At narrow but supported terminal sizes, the Deck SHOULD reduce to:

```text
+----------------------------------+
| PRIMARY                          |
+----------------------------------+
| COMMAND                          |
+----------------------------------+
| STATUS                           |
+----------------------------------+
```

Auxiliary content is reachable through an existing tab/palette/navigation action rather than by reducing the command line to unusable height.

## 6.5 Extremely small terminals

Below a documented minimum geometry, the Deck SHOULD refuse or leave full-screen presentation rather than create unreadable micro-panes.

A recommended initial floor is around 60 columns x 16 rows, subject to implementation testing. The exact threshold MAY vary, but behavior MUST be deterministic.

## 6.6 No user-created panes

v0.8 MUST NOT provide:

- arbitrary split creation;
- draggable borders;
- pane trees saved to config;
- free-form docking;
- pane spawning by plugins;
- a tiling-window-manager command language.

The existing view tree may internally use `Split` and `Tabs`. That does not make the Deck a window manager.

## 6.7 No persistent layout files

The exact sizes/positions of Deck surfaces MUST NOT become durable state in v0.8.

At most, simple preferences such as `auxiliary_visible = true|false` MAY persist.

# 7. Primary View Contract

## 7.1 Latest result behavior

When a command completes with a human-presentable native result, that result SHOULD become the primary view using the existing v0.7 presentation path.

No new `DeckResult` object is created.

The host stores the `ResultRef`/view handle necessary to display the result.

## 7.2 Historical results

A user MAY browse `HistoryEntry` records and explicitly open a retained `ResultRef` in the primary view.

The primary view MUST make historical age/context visible when confusion with current state is possible.

Opening a historical result MUST NOT re-execute the command.

## 7.3 New command replaces viewed historical result

To avoid a complex pin/latest dual-state model, the v0.8 default is simple:

> When the user executes a new foreground command that produces a primary result, that result becomes primary.

A historical result can be reopened from history afterward.

Persistent pinning of old results while new results arrive is not required in v0.8.

## 7.4 Existing selection semantics

If the mounted view exposes selection, it uses existing v0.2 selection semantics.

Selection may support explicit actions such as:

```text
inspect selected
open selected
enter selected
copy selected value/reference
use existing @ selection reference if/when canonicalized
```

Merely moving the cursor MUST NOT change command meaning.

## 7.5 Result expiration

If a historical `ResultRef` has expired under existing retention policy, the Deck MUST show that the result is unavailable.

It MUST NOT silently rerun the command to reconstruct it.

## 7.6 Large results

The host SHOULD use viewport-aware rendering and bounded caches.

It MUST NOT duplicate an entire large semantic collection merely to support scrolling if the existing result/view system can provide indexed/windowed access.

## 7.7 Raw and alternate views

Existing alternate view choices remain authoritative: table, list, tree, graph, JSON, YAML, raw and hex.

The Deck may make switching them easier, but it MUST NOT invent new values while switching presentation.

# 8. Auxiliary View Contract

## 8.1 One slot, multiple existing views

The auxiliary slot is a multiplexer, not a set of permanent panes.

Recommended built-in auxiliary choices:

```text
context
history
help
jobs
provenance
```

Additional choices MAY come from existing view contributions where appropriate, but v0.8 does not define a new plugin API for occupying the slot.

## 8.2 Context auxiliary

The context view projects existing state:

- active link/host;
- privilege;
- cwd/filesystem context;
- active `ContextStack` frame;
- v0.4 current place when relevant;
- v0.5 historical time context when active;
- v0.6 plan/change context when explicitly being inspected.

It MUST derive this information from authoritative state rather than cache a second "workspace context".

## 8.3 History auxiliary

The history view is a view over existing `HistoryEntry` records.

It MAY display:

```text
time
command text
duration
exit status
result availability
context/link summary
mutation summary
```

The view MUST use the same history retention/security rules as ordinary Ono history.

## 8.4 Help auxiliary

The help view consumes existing command/schema/metadata registries and v0.7 presentation logic.

It MUST NOT create a second command-help database.

## 8.5 Jobs auxiliary

The jobs view is a projection of existing job-control/background-stream state.

It MAY show status such as:

```text
#4 watch process       running
#5 cargo test          exited 0
#6 link prod-db        degraded
```

It MUST NOT become a generic notification/event bus.

## 8.6 Provenance auxiliary

When the primary result exposes provenance/evidence through existing schemas, the auxiliary may render those fields.

It must remain explicit about unknown, stale, partial or inferred evidence according to v0.4-v0.6 semantics.

## 8.7 Switching auxiliary view is side-effect free

Changing `context -> history -> help` is pure presentation navigation.

It MUST NOT execute commands, requery providers or change semantic context merely to populate a pane. If a view requires a query, that query must be explicit and visible as an Ono operation.

# 9. Command Surface

## 9.1 Reuse the existing line editor

The Deck MUST embed the same editor behavior used by the normal interactive shell.

It MUST NOT fork editing, completion, history substitution or parsing semantics into a Deck-specific editor.

## 9.2 Persistent visibility

The command surface SHOULD remain visible whenever the Deck is in its normal composed state.

A full-screen primary view MAY temporarily maximize when explicitly requested, but returning from it MUST restore the command surface without semantic loss.

## 9.3 Completion

Completion semantics remain inherited from v0.2/v0.7.

The Deck MAY render candidates in the auxiliary slot or an existing `CommandPalette`/transient view if that improves space use.

Candidate ranking and command meaning MUST remain identical to Rich TTY.

## 9.4 Multiline editing

The command surface MUST support the same multiline grammar and continuation behavior as the normal editor.

Layout must expand the editor only within a bounded maximum height. Extremely long drafts SHOULD scroll internally rather than consume the entire Deck.

## 9.5 Parse/runtime diagnostics

Diagnostics SHOULD be presented close to the command surface while the primary view remains intact where possible.

A diagnostic MUST not erase a useful previous result merely because the new command failed to parse.

## 9.6 Command execution and primary replacement

On successful foreground execution:

- semantic history is recorded through normal `HistoryEntry` behavior;
- `ResultRef` retention follows existing policy;
- the new result becomes primary when renderable;
- the command surface returns to editable state.

On error:

- structured error semantics remain authoritative;
- the error may become primary if substantial;
- otherwise a bounded diagnostic may appear without destroying the previous result.

# 10. Focus and Input Routing

## 10.1 Minimal focus set

The required focusable targets are:

```text
command
primary view
auxiliary view (when visible)
transient view (when present)
```

The status line is not required to be focusable.

## 10.2 Default focus

The command surface SHOULD receive focus after startup and after normal command completion.

This preserves the shell-first character of the product.

## 10.3 Semantic actions, not hard-coded UI verbs

The host SHOULD expose a small internal action registry such as:

```text
focus-command
focus-primary
focus-auxiliary
next-focus
previous-focus
open-aux-context
open-aux-history
open-aux-help
open-aux-jobs
close-transient
redraw
```

Exact default key bindings are intentionally not normative until PTY testing establishes a conflict-minimal set across readline-like editing, shells, tmux/screen and terminals.

## 10.4 Tab remains editor-owned when editing

When the command surface has focus and completion is applicable, `Tab` MUST retain completion behavior.

The Deck MUST NOT steal a universally expected completion key merely to cycle panes.

## 10.5 Escape hatch to command

There MUST be a reliable keyboard action that returns focus to the command surface from any normal Deck view without changing semantic state.

This action SHOULD be simple, discoverable and resistant to collision with embedded view controls.

## 10.6 Input dispatch order

A recommended dispatch order is:

1. emergency/fatal terminal recovery actions;
2. active transient view;
3. focused shell editor or focused existing view;
4. Deck host navigation actions not consumed by that surface;
5. unknown sequence handling.

Plugins MUST NOT intercept host-level recovery or terminal-lease actions.

## 10.7 Paste

Bracketed paste behavior SHOULD be shared with the normal line editor.

Pasted text into data views MUST NOT execute as shell input.

# 11. Navigation Is Not Semantic Context

## 11.1 View focus vs `enter`

Focusing a process row, map node or history entry MUST NOT perform `enter`.

The following are different operations:

```text
move selection to nginx.service    # UI state only
enter service nginx                # semantic ContextStack mutation
```

## 11.2 History browsing vs time context

Highlighting an old `HistoryEntry` does not establish v0.5 historical time context.

If Ono supports an explicit historical context operation, that operation must remain explicit.

## 11.3 Spatial navigation vs Deck navigation

v0.4 spatial navigation commands (`enter`, `follow`, `jump`, `back`, `up`, etc.) modify spatial/system context according to v0.4.

Deck navigation switches focus/views only.

Keys and labels MUST avoid making these two meanings ambiguous.

## 11.4 Selection references

If the baseline/canonical ADR provides a selection reference such as `@`, using that token is an explicit act by the user.

The Deck MUST NOT silently inject selected objects into commands.

## 11.5 No implicit target mode

v0.8 does not introduce a persistent `TARGET process:9132` semantic state.

A later release may evaluate whether existing `ValueRef`, selection and context already provide enough of that interaction. v0.8 MUST not pre-commit the model.

# 12. History and Result Reuse

## 12.1 Existing history is authoritative

The Deck MUST read the same `HistoryEntry` records as `history` in ordinary shell operation.

No `SessionEntry` or Deck-only history schema may be introduced.

## 12.2 Result references, not screenshots

Reopening a previous result uses `ResultRef` or another existing retained semantic reference.

The Deck MUST NOT persist terminal cell buffers as the authoritative history representation.

## 12.3 Re-execution is explicit

History may offer an explicit "put command text in editor" or "rerun" action.

Opening history MUST NOT rerun commands automatically.

## 12.4 Context visibility

When viewing an older entry, the UI SHOULD show its execution context sufficiently to avoid confusion:

```text
historical result
executed 2026-09-01 11:18:04
host prod-db-03
cwd /srv/app
result @42
```

This metadata comes from the existing history/context snapshot.

## 12.5 Sensitive result retention

The Deck MUST respect existing secret/sensitive-data retention rules.

It MUST NOT extend retention simply because reopening results is convenient.

## 12.6 Search/filter

A history view MAY support filtering over fields already present in `HistoryEntry`.

Search is presentation/query functionality over the existing store, not a new audit database.

# 13. Terminal Capability Preconditions

## 13.1 v0.7 capability model remains authoritative

v0.8 MUST build on the v0.7 terminal capability model rather than probe capabilities independently in every view.

## 13.2 Minimum Deck capabilities

A production Deck host normally requires:

- interactive stdin/stdout terminal;
- reliable cursor addressing;
- screen clearing/redraw;
- cursor show/hide;
- terminal-size detection and resize events;
- sufficiently correct Unicode width handling or ASCII fallback;
- a reversible raw/cbreak input mode;
- preferably alternate-screen support.

## 13.3 Alternate screen is the default requirement

The v0.8 Deck SHOULD use the terminal alternate screen.

If alternate-screen support is absent or known broken, the initial implementation SHOULD decline Deck startup and remain in Rich TTY rather than repeatedly repaint the normal scrollback buffer.

A future explicit no-alternate-screen implementation MAY be added after real demand, but it is not part of v0.8 Definition of Done.

## 13.4 `TERM=dumb`

`TERM=dumb` MUST disable Deck startup.

## 13.5 SSH, tmux, screen and mosh

The Deck MUST be tested in common remote/multiplexer environments.

Capability detection MUST use the visible terminal contract rather than assuming local hardware.

## 13.6 No terminal fingerprint theater

The host should detect only capabilities that affect correctness or usability.

It SHOULD NOT maintain a large terminal-brand-specific feature matrix unless a concrete compatibility issue requires it.

# 14. Generic Terminal Ownership Contract

## 14.1 Why a generic contract is required

Ono already has full-screen views and must coexist with arbitrary Unix terminal programs. Terminal state therefore cannot be a Deck-specific implementation detail.

Exactly one owner may control interactive terminal presentation at a time.

Conceptual owners:

```text
RichTtyShell
FullScreenOnoHost
ForegroundExternalProcess
Suspended/no owner
```

## 14.2 Terminal lease responsibilities

The lease owner is responsible for a bounded set of terminal state:

- foreground process group where applicable;
- input mode/termios changes;
- alternate-screen entry/exit;
- cursor visibility;
- bracketed-paste mode if enabled;
- mouse mode only if ever enabled by an existing view;
- terminal-title/hyperlink state only under established policy;
- redraw ownership.

## 14.3 Snapshot original state

Before acquiring full-screen control, Ono MUST capture the terminal state required to restore the user's environment.

Restoration MUST be best-effort and idempotent.

## 14.4 Lease state machine

A recommended internal state machine:

```text
UNOWNED
  -> ACQUIRING
  -> DECK_OWNED
  -> HANDING_OFF
  -> CHILD_OWNED
  -> REACQUIRING
  -> DECK_OWNED
  -> RELEASING
  -> UNOWNED
```

Error transitions MUST converge toward terminal restoration, not toward more control sequences.

## 14.5 One writer rule

While the Deck owns the lease, only the host renderer may issue terminal cursor/control sequences for Ono presentation.

Individual views/plugins submit view state; they do not write ANSI directly.

## 14.6 Reuse by other full-screen views

A v0.4 full-screen map launched outside the Deck SHOULD use the same full-screen host/lease infrastructure.

This is a key v0.8 consolidation outcome.

# 15. Alternate-Screen Lifecycle

## 15.1 Entry sequence

A robust entry sequence SHOULD conceptually be:

1. verify terminal preconditions;
2. snapshot restorable terminal state;
3. install scoped cleanup/fatal-signal hooks;
4. configure input mode;
5. enter alternate screen;
6. hide cursor only when rendering requires it;
7. mount root view composition;
8. perform full redraw;
9. place cursor at the command editor.

If any step fails, cleanup MUST unwind only the steps successfully applied.

## 15.2 Exit sequence

A normal release SHOULD conceptually:

1. stop accepting Deck navigation input;
2. cancel/close host-owned transient views;
3. flush pending renderer output;
4. show cursor;
5. disable host-enabled terminal modes;
6. leave alternate screen;
7. restore original termios/input state;
8. release foreground/ownership state;
9. remove cleanup hooks.

Cleanup MUST tolerate repeated calls.

## 15.3 Panic/error handling

A renderer or host panic MUST trigger a terminal-restoration path before process termination where the runtime permits it.

Panic handlers MUST avoid allocations/locking patterns likely to deadlock in a failure path.

## 15.4 Abrupt kill limitation

No userspace program can restore the terminal after uncatchable termination such as `SIGKILL` or power loss.

Documentation MUST be honest about this limitation and SHOULD provide a simple recovery hint (`reset`, `stty sane`) for rare corrupted-terminal cases.

## 15.5 No boot animation

Entering alternate screen MUST not be padded with a simulated boot sequence or artificial delay.

# 16. Foreground External Processes

## 16.1 Preserve Unix truth

Ono MUST NOT implement a partial terminal emulator merely to keep arbitrary external programs inside a Deck pane.

When an external foreground process genuinely needs the user's terminal, the Deck hands it the terminal.

## 16.2 When handoff is required

Handoff is required when execution semantics attach the external process directly to the interactive terminal, including common programs such as:

```text
vim
less
top
htop
ssh
python (interactive)
gdb/lldb
fzf when interactive
```

Unknown external commands with direct terminal stdio SHOULD be treated conservatively.

## 16.3 Handoff sequence

A recommended handoff:

1. stop Deck redraw/input dispatch;
2. flush host output;
3. leave Deck alternate-screen state as required;
4. restore normal cursor/input modes expected by a child;
5. transfer foreground process group / terminal control;
6. execute/wait using existing process/job-control semantics;
7. regain shell foreground control after child exit/stop;
8. reacquire Deck terminal modes;
9. re-enter alternate screen;
10. force a complete redraw from semantic/view state.

The sequence MUST be robust when the child itself uses alternate screen, changes termios or is stopped/resumed.

## 16.4 Direct external output is not magically retained

If an external process writes directly to the terminal during handoff, Ono MUST NOT claim to possess a structured/captured result it did not capture.

`HistoryEntry` may record command text, duration and exit status according to existing semantics.

Users who need structured/captured output should use normal pipelines/adapters/redirection where Ono actually owns the data.

## 16.5 Non-terminal external commands

External commands whose stdio is piped/captured by normal Ono pipeline semantics do not require terminal handoff merely because the Deck is active.

Their output crosses the v0.3 boundary exactly as outside the Deck.

## 16.6 Pagers

Ono SHOULD avoid implicitly launching a pager inside a Deck-owned result view. If an explicit external pager is executed, it receives normal terminal handoff.

## 16.7 SSH

`ssh` as an arbitrary external command remains an external terminal process and receives handoff.

Ono remote `link` semantics, when used, remain native context and may continue inside the Deck without spawning a separate terminal UI.

# 17. Job Control, Suspend and Resume

## 17.1 Existing job control remains authoritative

v0.8 does not redefine process groups, jobs, foreground/background execution or shell job control.

It only specifies how full-screen terminal ownership cooperates with those semantics.

## 17.2 Suspending Ono

When the user suspends the foreground Ono shell (for example through the terminal's suspend character), the Deck MUST attempt to:

1. stop redraw;
2. show the cursor;
3. leave alternate screen;
4. restore terminal modes needed by the parent shell;
5. release/normalize terminal ownership;
6. suspend according to normal job-control semantics.

## 17.3 Resume

On `SIGCONT`/foreground resume, Ono MUST:

1. re-evaluate terminal size/capabilities where necessary;
2. reacquire the terminal lease;
3. restore Deck input modes;
4. enter alternate screen;
5. perform a full redraw;
6. restore focus/view-private navigation when still valid.

## 17.4 Background output

Background jobs MUST NOT write uncontrolled terminal cursor sequences through the Deck host.

Existing job-control/output capture policy remains authoritative. The Deck may show a bounded status notification that output is available.

## 17.5 Stopped external child

If a handed-off child is stopped, terminal ownership MUST return to Ono according to existing job-control rules before the Deck redraws.

Resuming that child in foreground repeats the handoff safely.

# 18. Screen Model and Redraw

## 18.1 Logical screen belongs to the host

The full-screen host MAY maintain a logical cell buffer to compute efficient redraws.

This buffer is presentation cache only. It is not result history.

## 18.2 Full redraw correctness first

A correct complete redraw is required before damage-based optimization.

The host MUST be able to discard all cached cells and reconstruct the visible screen from mounted views and shell editor state.

## 18.3 Damage-based redraw

After correctness is established, the host SHOULD redraw only changed rows/cells where that materially reduces terminal traffic.

Optimization MUST NOT allow stale cells from a previous result/view to remain visible.

## 18.4 Cursor ownership

The host renderer owns physical cursor placement while the Deck owns the terminal.

Views may express logical focus/cursor information through the view protocol but MUST NOT emit raw cursor movement.

## 18.5 Atomic perception

Where terminals permit it, redraw SHOULD minimize visible tearing/flicker.

The implementation MUST NOT add artificial frame pacing or animation merely to create a graphical feel.

## 18.6 Untrusted text

All value/plugin/external captured text rendered by the Deck MUST pass the v0.7 terminal-control sanitization boundary.

User data MUST NOT be able to reposition the cursor, change title, write clipboard sequences or spoof host chrome unless an explicitly trusted raw-terminal mode exists outside normal Deck rendering.

# 19. Resize and Reflow

## 19.1 Resize is normal

`SIGWINCH`/terminal resize is a normal host event, not an error.

## 19.2 Preserve semantics, recompute presentation

On resize the host MUST preserve:

- mounted semantic result/reference;
- existing view identity;
- selection identity when still resolvable;
- command draft;
- focus when target remains visible.

It MAY recompute:

- view dimensions;
- line wrapping;
- visible columns;
- primary/auxiliary arrangement;
- scroll offsets when required to keep focus visible.

## 19.3 Collapse auxiliary before damaging primary/editor

When width shrinks, the auxiliary view SHOULD collapse before the command editor or primary content becomes unusable.

## 19.4 Resize storms

The host MAY coalesce rapid resize events for rendering efficiency, but final geometry MUST converge promptly.

## 19.5 Unicode width

Width calculation MUST use the same tested terminal-width policy as v0.7.

A resize must not introduce column drift because different components use different Unicode-width libraries.

# 20. Existing Live Views Inside the Deck

## 20.1 v0.8 does not invent live semantics

The Deck MUST be able to host existing live views already defined by earlier specifications, including examples such as:

```text
watch process
map --live
```

Their data remains `Stream<T>`/provider subscription/polling behavior defined earlier.

v0.8 MUST NOT introduce `Live<T>`, a new `Observation<T>` hierarchy, a second gap model or a second backpressure contract.

## 20.2 Host responsibility

The Deck host is responsible only for presentation integration:

- keeping the mounted live view visible;
- delivering redraw opportunities;
- preserving command input responsiveness;
- showing existing freshness/stale/partial indicators;
- cancelling/closing according to existing view/job lifecycle.

## 20.3 Existing backpressure remains authoritative

If a stream produces updates faster than the terminal can render, stream/backpressure semantics remain those of the existing core.

The host MAY coalesce purely visual redraws provided it does not falsify semantic stream delivery to downstream pipeline stages.

## 20.4 No dashboard composition

v0.8 does not require multiple simultaneous live charts, dashboard grids or arbitrary subscriptions in multiple panes.

One mounted primary live view plus bounded job/status information is sufficient.

## 20.5 v0.9 boundary

A later v0.9 MAY harden live-view integration around follow and pause behavior, navigation, multiple mounted live sources, redraw coalescing and long-running workspace ergonomics.

It MUST begin from existing `Stream<T>`, v0.4 live topology and v0.5 temporal semantics rather than creating another live data model.

# 21. v0.4 Spatial Integration

## 21.1 Spatial maps are existing views

A v0.4 map shown in the Deck is the same spatial projection defined by v0.4.

The Deck MUST NOT convert spatial objects into a separate workspace graph.

## 21.2 Full-screen map consolidation

Outside the Deck, an explicitly full-screen map SHOULD use the shared v0.8 full-screen host/terminal lease.

Inside the Deck, the map SHOULD mount as the primary view and use the same view-tree contract.

This is one of the strongest reasons for v0.8: terminal/full-screen mechanics become shared infrastructure rather than a map-specific feature.

## 21.3 Spatial focus remains spatial-view state

Moving the map cursor is selection/focus inside the spatial view.

Only explicit v0.4 navigation changes current place/context.

## 21.4 Live map

`map --live` retains v0.4 requirements:

- real provider events or explicit polling;
- visible freshness source;
- no fake animation;
- stale providers do not appear current;
- snapshot-diff inference remains labeled as such.

The Deck host only renders it persistently.

# 22. v0.5 Temporal and Causal Integration

## 22.1 Historical state must remain visibly historical

Persistent full-screen presentation increases the risk that an old value looks current.

The Deck MUST preserve v0.5 temporal distinctions and should expose timestamps/coverage/freshness prominently when a result is historical or stale.

## 22.2 Coverage and gaps

If a v0.5 view reports incomplete coverage or a gap, the Deck MUST render that uncertainty.

It MUST NOT fill empty visual space with interpolated "current" state unless the semantic model explicitly provides an interpolation.

## 22.3 Causal confidence/evidence

Relationship/causal views MUST preserve provenance and confidence semantics.

Visual emphasis cannot upgrade inferred evidence to fact.

## 22.4 History auxiliary is not a time machine

Browsing `HistoryEntry` records in the auxiliary view is not equivalent to entering a v0.5 historical/system-time context.

That semantic transition remains explicit.

# 23. v0.6 Change, Protection and Recovery Integration

## 23.1 Plans are ordinary semantic results

`ChangePlan`, `ImpactGraph`, `RecoveryPlan` and recovery assets are rendered using their existing semantics.

The Deck may keep them visible longer; it MUST NOT reinterpret them.

## 23.2 Persistent visibility can improve safety

A Deck may show a concise auxiliary summary while a v0.6 plan is primary:

```text
PLAN        plan:91ab
state       SEALED
risk        MODERATE
protection  PARTIAL
unknown     1 boundary
```

Every field must derive from the actual v0.6 object.

## 23.3 No UI approval state

Focusing a button-like label, selecting a plan or opening a recovery view MUST NOT constitute approval.

The v0.6 commitment point (`apply`) and its confirmation/policy semantics remain authoritative.

## 23.4 Recovery visibility

When a recovery asset exists, the Deck MAY make it easy to inspect, but it MUST preserve v0.6 distinctions such as:

- snapshot != backup;
- recovery != universal rollback;
- selective restore vs full dataset rollback;
- online vs reboot/offline recovery;
- residual risk and newer-state destruction.

## 23.5 Proposed state stays proposed

A `map --plan` or prospective result must remain visually distinct from current state exactly as required by v0.6.

Persistent side-by-side layout MUST NOT make the proposal look committed.

# 24. KUANG/11 Integration Without a New UI API

## 24.1 Existing view contributions remain valid

v0.2 already allows a KUANG/11 view plugin to submit a constrained view tree or use a stable UI protocol.

v0.8 MUST honor that contract.

A plugin view that could be mounted as a full-screen/interactive Ono view may also be mounted as the Deck primary view if its declared capabilities and size requirements are compatible.

## 24.2 Plugins do not own Deck chrome

KUANG/11 plugins MUST NOT directly control:

- terminal lease;
- alternate-screen lifecycle;
- physical cursor;
- shell command editor;
- host focus recovery;
- global key dispatch reserved for safety/recovery;
- arbitrary creation of Deck panes.

## 24.3 No new pane/plugin manifest fields

v0.8 SHOULD NOT add manifest fields such as:

```yaml
deck_regions:
  - right_panel
  - bottom_panel
workspace_widget: true
```

Existing view acceptance/type metadata should be sufficient for primary/auxiliary mounting where appropriate.

## 24.4 Existing plugin view lifecycle

Plugin views continue to receive the established mount, focus, background, resize and cancellation events.

The Deck host must not invent a plugin-specific second lifecycle.

## 24.5 Invalid plugin layout

A plugin may request a view tree that cannot fit. The host MUST reject/degrade that view without losing terminal control.

## 24.6 Security boundary

Untrusted plugin-rendered strings remain subject to host sanitization. A plugin cannot escape the constrained view protocol by embedding terminal controls in text.

# 25. Transient Views Without an Overlay Framework

## 25.1 Existing transient components

The baseline view vocabulary already contains interactive elements such as:

```text
CommandPalette
ObjectPicker
Tabs
```

v0.8 MAY host these transiently.

## 25.2 No general modal-window stack

The release MUST NOT build a desktop-style arbitrary overlay/window manager.

A simple host-private `transient_view: Option<ViewHandle>` or equivalent is sufficient for:

- completion palette where useful;
- command palette;
- object picker;
- bounded confirmation/help surface.

## 25.3 Focus

When a transient view is active, it receives focus according to the existing view contract. Closing it returns focus to the previous valid surface, normally the command editor.

## 25.4 Safety confirmations

If v0.6 or core mutation policy requires confirmation, the Deck may render it through a transient view.

The semantic confirmation token/state remains owned by the safety/mutation system, not the overlay.

## 25.5 Size fallback

If a transient view cannot fit, the host SHOULD fall back to a full-primary view or Rich TTY-style prompt interaction rather than clip critical information.

# 26. Status, Notifications and Activity

## 26.1 No Activity pane

The revised v0.8 deliberately removes the earlier separate Activity Region.

Transient activity does not justify a permanent pane.

## 26.2 Status line budget

The `StatusLine` may contain short, actionable state such as:

```text
prod-db-03 | service/nginx | root! | jobs +2 | aux history
```

It SHOULD remain one line in normal layouts.

## 26.3 Notifications

Short-lived notifications MAY appear in the status line or a bounded transient view:

```text
job #4 completed (exit 0)
link prod-db latency degraded
result @42 expired
```

Notifications MUST NOT obscure the command editor indefinitely.

## 26.4 No fake liveliness

The host MUST NOT animate status indicators in the absence of real state change.

## 26.5 Notification history

v0.8 does not require a separate notification database. Events that already belong in history/audit systems should be recorded there by those systems.

# 27. Staleness, Currentness and Visual Honesty

## 27.1 Persistence creates ambiguity

A normal shell result naturally looks old because it scrolls upward. A persistent view can remain visually prominent long after its data was observed.

Therefore the Deck MUST make currentness explicit when relevant.

## 27.2 Static query results

A static result such as `get process` MUST NOT silently refresh just because it remains visible.

If freshness matters, the view SHOULD show its observation/result time or a stale-age indicator according to existing metadata.

## 27.3 Live results

A live view uses existing stream/freshness semantics. It must distinguish event-driven, polled, cached, stale and partial state when those distinctions are provided by earlier specs.

## 27.4 Historical results

Historical `ResultRef` content SHOULD carry a clear historical marker.

## 27.5 Proposed results

v0.6 proposed state MUST remain labeled proposed/expected/unknown as appropriate.

## 27.6 No UI-invented freshness

The Deck MUST NOT infer "fresh" merely because a view was recently redrawn.

# 28. External/Opaque Output Boundary

## 28.1 Captured text remains text

When an external program's output enters Ono as text/bytes through v0.3 semantics, the Deck renders it honestly as text/bytes.

It MUST NOT infer table/object structure from terminal columns unless an explicit adapter/schema exists.

## 28.2 ANSI in captured output

Captured external ANSI/control sequences MUST be sanitized by default.

The host may support an explicitly trusted/render-ANSI presentation later, but v0.8 does not require it.

## 28.3 Terminal-owning external output

When a child owns the terminal directly, its terminal output is outside the Deck render tree during that lease.

After return, Ono redraws the Deck from its own semantic/view state.

## 28.4 Exit status

External command exit status continues to populate normal shell/history semantics.

## 28.5 No embedded terminal pane

An embedded xterm-compatible emulator is an explicit non-goal. It would make Ono responsible for terminal-emulation correctness, copy mode, alternate-screen nesting, OSC handling, graphics protocols and security far beyond the Deck's purpose.

# 29. Accessibility and Reduced Complexity

## 29.1 Keyboard first

All required Deck actions MUST be possible without a mouse.

## 29.2 No-color

The Deck MUST remain understandable with color disabled.

Focus, warning, historical/proposed state and errors require textual/symbol/border cues in addition to color.

## 29.3 ASCII fallback

Terminals with unreliable Unicode rendering MUST have an ASCII-safe border/symbol presentation.

## 29.4 Reduced motion

Any transition animation inherited from views MUST respect reduced-motion settings. v0.8 itself requires no animation.

## 29.5 Linear fallback

Every semantic result visible in the Deck must remain accessible through ordinary non-full-screen rendering/commands.

The Deck is not the only way to access information.

## 29.6 Focus visibility

The currently focused surface MUST be distinguishable without relying only on hue.

## 29.7 Information density

The host should favor readable compactness over permanent labels and decorative headers. Auxiliary detail should appear on demand.

# 30. Security

## 30.1 Terminal escape injection

Untrusted content MUST NOT gain terminal-control authority through Deck rendering.

The v0.7 sanitization boundary remains mandatory.

## 30.2 Focus spoofing

A plugin/view MUST NOT render content that is indistinguishable from trusted command/confirmation chrome when that could cause unsafe input.

Trusted mutation confirmation surfaces SHOULD have host-owned framing/identity markers.

## 30.3 Clipboard

The Deck MUST NOT write OSC clipboard data as a side effect of rendering.

Explicit copy actions MAY use a configured clipboard integration only under existing security policy.

## 30.4 Hyperlinks

OSC-8 or equivalent hyperlinks, if supported, must use sanitized destinations and must never be required to access the raw value.

## 30.5 Workspace persistence

Because v0.8 intentionally persists almost no cross-session workspace state, it SHOULD NOT create a new sensitive state file containing result data.

Simple preferences must use normal config permissions.

## 30.6 Plugin isolation

The Deck does not weaken KUANG/11 capability isolation. Mounting a view grants presentation space, not extra system capabilities.

# 31. Performance and Responsiveness

## 31.1 Command typing is the highest priority

The Deck fails as a shell if typing feels delayed because a large/live view is repainting.

The editor input path MUST remain isolated from expensive semantic queries and large layout recomputation where possible.

## 31.2 Initial budgets

Suggested p95 local targets on a modern development machine:

| Interaction | Target |
|---|---:|
| keypress -> visible editor update | <= 16 ms |
| focus switch | <= 25 ms |
| auxiliary view switch from cached data | <= 50 ms |
| resize -> useful redraw | <= 100 ms |
| full redraw typical 120x40 | <= 50 ms |
| Deck entry after shell initialization | <= 150 ms |
| Deck redraw after child return | <= 100 ms |

These are engineering targets, not protocol guarantees. Regressions SHOULD be measured continuously.

## 31.3 Slow provider separation

Opening context/help/history from already available state SHOULD not synchronously query slow remote providers.

Explicit queries may run asynchronously through normal command semantics.

## 31.4 Large tables

Selection/scrolling through a retained large table SHOULD be viewport-oriented and must not repeatedly reformat all rows on every keypress.

## 31.5 Redraw rate

Live views MAY request updates faster than a terminal can usefully redraw. The host SHOULD bound visual frame rate independently of semantic stream correctness.

A practical initial maximum such as 30-60 redraws/s is more than sufficient; lower defaults may be appropriate for remote links.

## 31.6 Remote terminals

The host SHOULD minimize bytes written on remote terminals through damage-based redraw once correctness is established.

# 32. Memory and Resource Management

## 32.1 No duplicate semantic retention

The Deck host MUST NOT copy every retained result into a second workspace cache.

It should hold references/view state and rely on the existing bounded result-retention system.

## 32.2 Cell buffers

At most a small number of screen-sized cell buffers are expected for diffing. These are cheap relative to semantic results and should be bounded by current geometry.

## 32.3 View-private state

Mounted views SHOULD retain only:

- semantic references;
- viewport/scroll position;
- ephemeral selection key;
- collapsed/expanded presentation state;
- small render caches.

They SHOULD NOT copy arbitrarily large streams.

## 32.4 Auxiliary view eviction

Non-visible auxiliary views MAY be unmounted/recreated rather than kept alive indefinitely.

## 32.5 Live view cancellation

Closing/replacing a live view MUST trigger the existing cancellation/job behavior and release subscriptions/resources as specified by the owning system.

## 32.6 Leak testing

Repeated Deck entry/exit, view switching and child handoff MUST be included in leak/resource tests.

# 33. Failure Containment and Recovery

## 33.1 Renderer failure

If one mounted view fails to render, the host SHOULD replace it with a structured error placeholder and keep the command surface usable where possible.

## 33.2 Host failure

If the full-screen host cannot safely continue, it SHOULD:

1. restore terminal state;
2. leave alternate screen;
3. report the error in ordinary terminal flow;
4. continue the semantic shell in Rich TTY if process state remains trustworthy.

## 33.3 Terminal desynchronization

The Deck MUST provide a full-redraw action that discards cached cells and reconstructs the screen.

## 33.4 Input decoder error

Unknown/invalid terminal input sequences MUST NOT panic the host. They should be ignored, surfaced in debug mode or passed to the focused surface only when safe.

## 33.5 Out-of-memory pressure

Under memory pressure the host SHOULD evict render caches and unmounted auxiliary state before semantic/session state.

It MUST NOT silently drop v0.6 safety facts from a visible plan while retaining decorative caches.

## 33.6 Recovery documentation

User documentation SHOULD include terminal recovery commands for the rare case of abnormal termination leaving modes altered.

# 34. Concurrency Model

## 34.1 Single UI authority

All physical screen updates while Deck-owned SHOULD flow through one host authority.

## 34.2 Async semantic execution

Commands/providers may execute asynchronously according to existing runtime design. Their results arrive as semantic events/results to the presentation host.

## 34.3 Generation guards

When a view is replaced while an asynchronous render/query completes, stale completion MUST NOT overwrite the new primary view.

The host SHOULD use view handles/generation IDs or cancellation tokens for presentation tasks.

This is host concurrency control, not a new semantic stream generation model.

## 34.4 Resize race

A resize that occurs during render must result in a final frame matching the latest known geometry.

## 34.5 Child handoff race

Once terminal handoff starts, no Deck writer may emit terminal-control output until ownership is reacquired.

## 34.6 Shutdown

Shutdown should cancel host-owned tasks, close mounted views and then restore terminal state in a deterministic order.

# 35. Configuration

## 35.1 Keep configuration small

v0.8 SHOULD introduce only configuration necessary to operate the Deck safely.

Reasonable examples:

```toml
[deck]
enabled = true              # only relevant when user opts into startup policy
auxiliary = "context"       # default auxiliary view
show_status_line = true
ascii = "auto"
```

Exact keys are non-normative.

## 35.2 No layout DSL

The following is intentionally outside v0.8:

```toml
[[deck.panes]]
position = "right"
width = "31%"
plugin = "..."
```

## 35.3 Key bindings

If Ono already has a keymap system, Deck host actions SHOULD integrate with it.

v0.8 SHOULD NOT create a separate Deck-only key configuration format.

## 35.4 Capability overrides

Diagnostic overrides for alternate-screen/Unicode behavior MAY exist for testing and terminal compatibility, but defaults should rely on the common capability model.

# 36. Observability and Debugging

## 36.1 Debug trace categories

A developer/debug mode SHOULD be able to record host events such as:

```text
deck.enter
terminal.acquire
view.mount
view.focus
layout.reflow
screen.full_redraw
screen.damage_redraw
child.handoff.begin
child.handoff.end
terminal.release
```

## 36.2 No sensitive value dump by default

Debug logs SHOULD record IDs/types/sizes rather than full user data by default.

## 36.3 Reproducible host-state dump

For bug reports, a diagnostic command MAY expose a sanitized host snapshot:

```text
terminal  120x40 xterm-256color
owner     deck
focus     command
primary   result_ref @42 view Table
aux       context
child     none
redraw    damage
```

This dump is presentation/debug metadata, not a second semantic state store.

## 36.4 Metrics

Development instrumentation MAY track redraw duration, cells changed, bytes written, input latency and view render time. Product telemetry MUST remain opt-in according to project policy.

# 37. Compatibility and Degradation Matrix

The implementation MUST test at least the following behaviors.

- **Local modern terminal:** full Deck.
- **tmux:** full Deck when exposed capabilities are valid.
- **GNU screen:** Deck with graceful capability limits.
- **SSH to a remote host:** full Deck based on the terminal capabilities and size visible on the remote side.
- **mosh:** Deck only when cursor and resize behavior are verified reliable.
- **`TERM=dumb`:** no Deck; Rich/plain fallback.
- **stdout redirected:** no Deck/full-screen control.
- **stdin non-TTY:** no Deck.
- **No color:** full semantic Deck without color dependency.
- **Narrow terminal:** collapsed auxiliary view or fallback.
- **Unsupported alternate screen:** Rich TTY fallback.

Degradation order SHOULD be:

```text
Deck full composition
  -> Deck without optional auxiliary/color features
  -> Rich TTY
  -> plain deterministic rendering
```

Semantic command behavior MUST remain unchanged across that degradation.

# 38. User Experience Examples

## 38.1 Startup

```text
$ ono deck

+-----------------------------------------+------------------+
| ONO                                     | CONTEXT          |
|                                         | host local       |
| No result yet.                          | cwd  ~           |
|                                         | user masl        |
|                                         | jobs 0           |
+------------------------------------------------------------+
| local://~ > _                                              |
+------------------------------------------------------------+
| local | ~ | aux context                                    |
+------------------------------------------------------------+
```

No fake boot text is required.

## 38.2 Native typed result

```text
+-----------------------------------------+------------------+
| PID   NAME       CPU   MEMORY           | CONTEXT          |
| 1821  postgres   44%   3.2 GiB          | host local       |
| 9291  node       31%   812 MiB          | cwd /srv/app     |
|                                         | result @17       |
+------------------------------------------------------------+
| local:///srv/app > _                                       |
+------------------------------------------------------------+
| 2 Process values | aux context                             |
+------------------------------------------------------------+
```

The table is a rendering of the same typed result available in Rich TTY.

## 38.3 Selection is weak by design

```text
> 1821  postgres   44%   3.2 GiB
  9291  node       31%   812 MiB
```

The marker means only that the table view has an ephemeral selected row.

No process operation occurs until the user explicitly inspects, references or acts.

## 38.4 History auxiliary

```text
+-----------------------------------------+------------------+
| PRIMARY: result @17                     | HISTORY          |
| ...                                     | 12:10 get proc   |
|                                         | 12:09 map        |
|                                         | 12:07 plan ...   |
+------------------------------------------------------------+
| local://~ > _                                              |
+------------------------------------------------------------+
```

Opening the 12:07 result uses its retained `ResultRef`; it does not rerun the command.

## 38.5 v0.6 plan

```text
+-----------------------------------------+------------------+
| CHANGE PLAN plan:91ab                   | PLAN             |
| state       SEALED                      | risk MODERATE    |
| actions     2                           | protect PARTIAL  |
| unknown     external upstream           | recovery ZFS     |
|                                         |                  |
| No mutation has occurred.               |                  |
+------------------------------------------------------------+
| prod-db://service/nginx > _                                |
+------------------------------------------------------------+
| proposed state | aux provenance                             |
+------------------------------------------------------------+
```

The Deck does not create the risk/protection facts; it exposes the v0.6 object.

## 38.6 Spatial map

A v0.4 map occupies the primary slot. Spatial navigation remains v0.4 navigation. The command line remains available below it.

## 38.7 External `vim`

```text
prod-db://~ > vim /etc/nginx/nginx.conf
```

Expected sequence:

```text
Deck releases terminal
vim runs with normal terminal ownership
vim exits
Deck reacquires terminal
full redraw
command line restored
```

The Deck MUST NOT attempt to emulate Vim inside a pane.

## 38.8 Narrow terminal

The auxiliary disappears before the primary result or command editor becomes unusable. The user can open context/history temporarily through an existing navigation action.

# 39. Acceptance Test Matrix

Each scenario must be automated where practical through PTY integration tests and supplemented by dogfooding.

## 39.1 Entry and exit

**Given:** a supported 120x40 terminal.  
**When:** `ono deck` starts and exits normally.  
**Then:** alternate screen/input modes are entered and restored; the parent terminal remains usable.

## 39.2 Semantic parity

Run the same native command in Rich TTY and Deck.

Assert:

- same semantic result type/values;
- same exit status;
- same provider execution;
- same history semantics;
- only presentation differs.

## 39.3 Typed result reuse

Execute a command producing a retained result, browse history, reopen the result.

Assert that no command re-execution occurs.

## 39.4 Selection

Move table selection across rows.

Assert:

- no provider call;
- no context change;
- no mutation;
- no history mutation entry from cursor movement.

Then explicitly inspect/reference selected value and assert normal semantic execution.

## 39.5 Auxiliary switching

Switch `context -> history -> help -> jobs` repeatedly.

Assert no semantic context mutation and bounded memory growth.

## 39.6 Resize

Resize from wide to medium to narrow and back.

Assert:

- auxiliary collapses/restores appropriately;
- command draft preserved;
- selected object preserved when identity still exists;
- no stale cells;
- no semantic requery merely from resize.

## 39.7 v0.4 map

Mount a spatial map inside Deck and outside Deck full-screen.

Assert both use shared terminal-host behavior and identical spatial semantics.

## 39.8 Existing live view

Run `watch process` or a deterministic test stream.

Assert the view updates while command input remains responsive and stream cancellation follows existing rules.

## 39.9 v0.5 stale/gap

Feed a deterministic historical result with a known gap/stale marker.

Assert the Deck does not render it as current/complete.

## 39.10 v0.6 plan

Open a sealed plan with partial protection and an unknown boundary.

Assert both facts remain visible and no UI action applies the plan without the normal explicit semantic path.

## 39.11 External full-screen child

Run `vim`, `less`, `top` (or deterministic fixture programs exercising equivalent termios/alternate-screen behavior).

Assert terminal ownership transfers and returns without corruption.

## 39.12 Suspend/resume

Suspend Ono, inspect parent terminal usability, resume.

Assert full redraw and retained command draft.

## 39.13 Crash-path restoration

Inject a renderer/host failure at multiple entry stages.

Assert best-effort terminal restoration and Rich-TTY fallback where process state permits.

## 39.14 No-color and ASCII

Run with no-color/ASCII settings.

Assert all critical state remains understandable.

## 39.15 Malicious control text

Render values containing CSI/OSC/escape sequences.

Assert they cannot move cursor, alter title, write clipboard or impersonate host chrome.

# 40. Performance Test Matrix

## 40.1 Keystroke latency under static load

Mount a table containing at least 100k logical rows through a virtualized fixture. Type continuously in the command editor.

Target: p95 visible edit latency <= 16 ms on reference hardware.

## 40.2 Keystroke latency under live load

Mount a synthetic existing `Stream<T>` producing more updates than the screen needs. Continue typing.

Assert visual redraw may coalesce but editor responsiveness remains within target and semantic stream behavior is not altered.

## 40.3 Scroll/navigation

Navigate a large retained table for 30 seconds.

Assert no O(n) full-table reformat on each keypress and stable memory.

## 40.4 Resize storm

Generate rapid width/height changes for several seconds.

Assert no panic, bounded queued work and convergence to final geometry.

## 40.5 Child roundtrip

Repeat external terminal handoff 100 times with a fixture that changes termios/alternate screen.

Assert terminal state matches expected state after each return.

## 40.6 Entry/exit leak test

Create/destroy Deck host repeatedly.

Assert no unbounded growth in threads, file descriptors, subscriptions, view handles or allocated screen buffers.

## 40.7 Remote low-bandwidth simulation

Throttle PTY output and measure bytes/redraw.

Damage-based redraw SHOULD materially reduce traffic after initial full frame.

# 41. Unit, Property and Integration Test Strategy

## 41.1 Unit tests

Required unit-test areas include:

- layout choice from terminal geometry;
- focus routing;
- auxiliary switching;
- terminal-lease transition validation;
- cleanup idempotence;
- cell-diff computation;
- Unicode width handling;
- stale async render rejection;
- host generation/view handle validity.

## 41.2 Property tests

Useful properties:

```text
layout never overlaps command surface
layout never produces negative dimensions
release(release(state)) is safe/idempotent
resize sequence ending at geometry G renders same semantic frame as direct G
selection movement never mutates semantic ContextStack
sanitized text cannot emit raw control sequences
```

## 41.3 Golden screen tests

Golden tests SHOULD cover a small canonical set of geometries and content types.

They should assert structure and visible semantics rather than brittle exact coloring where possible.

## 41.4 PTY integration tests

PTY tests are mandatory for:

- raw/cbreak mode;
- alternate screen;
- resize;
- suspend/resume;
- foreground process groups;
- child terminal handoff;
- tmux/screen compatibility fixtures where CI permits.

## 41.5 Fuzzing

Terminal input decoding and untrusted text sanitization SHOULD be fuzzed.

## 41.6 Fault injection

Inject failures at every terminal-acquire/release step and view render boundary to validate cleanup.

# 42. Recommended Internal Responsibility Boundaries

Exact crate/module names are non-normative, but responsibilities SHOULD remain separated approximately as follows:

```text
ono-terminal
  terminal capability abstraction inherited from v0.7
  TerminalLease
  termios/foreground process group
  alternate-screen acquire/release
  suspend/resume hooks

ono-view-host
  generic full-screen host
  view lifecycle mounting
  input/focus routing
  logical screen + redraw
  resize

ono-deck
  bounded Deck composition
  primary/auxiliary slot policy
  command-editor embedding
  status-line composition
  history/context view selection

existing modules
  evaluator / Value / Stream
  history / ResultRef
  context
  spatial
  temporal
  change/recovery
  KUANG/11
```

`ono-deck` MUST NOT become a dependency of the evaluator, provider layer or semantic types.

The generic full-screen host SHOULD be reusable by v0.4 maps and other existing interactive views.

# 43. Recommended Minimal Internal Structures

The following are illustrative implementation shapes, not public language/API contracts.

## 43.1 DeckHostState

```text
DeckHostState {
    terminal_lease
    geometry
    focus
    primary_view_handle
    auxiliary_view_handle?
    transient_view_handle?
    command_editor_handle
    status_line_state
    screen_cache
}
```

It intentionally does not contain duplicated semantic collections, history databases or context objects.

## 43.2 View slot

```text
ViewSlot {
    handle: ViewHandle
    viewport: ViewportState
}
```

`ViewportState` may include scroll/cursor state appropriate to the mounted existing view.

## 43.3 TerminalLease

```text
TerminalLease {
    owner
    original_termios
    foreground_pgrp
    alt_screen_active
    cursor_hidden
    bracketed_paste_active
    generation
}
```

Exact fields depend on platform/library design. The invariant is exclusive, reversible ownership.

## 43.4 No `WorkspaceSelection`

Selection stays inside the mounted view's existing view-private state and semantic references.

## 43.5 No `WorkspaceHistory`

History auxiliary queries existing history storage directly through its normal API.

# 44. Implementation Phases

The phases are intentionally ordered to validate consolidation before adding visual polish.

## 44.1 Phase A - Generic terminal lease

Deliver:

- terminal state snapshot/restore;
- alternate-screen acquire/release;
- cursor/input-mode ownership;
- idempotent cleanup;
- fault-injection tests;
- suspend/resume skeleton.

Gate:

> Existing full-screen fixtures can acquire/release the terminal repeatedly without corruption.

## 44.2 Phase B - Generic full-screen view host

Deliver:

- root existing-view-tree mount;
- view lifecycle delivery;
- full redraw;
- resize handling;
- focus/input routing;
- sanitization boundary.

Gate:

> A v0.2-style Table/Tree/CommandPalette fixture renders full-screen without a Deck-specific semantic type.

## 44.3 Phase C - Migrate/reuse v0.4 full-screen map host

Deliver integration of existing full-screen map behavior onto the generic host where practical.

Gate:

> Spatial map semantics unchanged; duplicate terminal lifecycle code removed/reduced.

## 44.4 Phase D - Deck bounded composition

Deliver:

- primary slot;
- command surface;
- optional auxiliary slot;
- status line;
- wide/medium/narrow layouts.

Gate:

> `ono deck` can execute ordinary native commands with semantic parity to Rich TTY.

## 44.5 Phase E - History/context auxiliary views

Deliver:

- context projection from existing context state;
- history browsing from `HistoryEntry`;
- retained-result open via `ResultRef`;
- explicit historical markers.

Gate:

> No new history/context storage or re-execution shortcut exists.

## 44.6 Phase F - Selection and transient existing views

Deliver:

- view-private selection navigation;
- CommandPalette/ObjectPicker hosting;
- explicit selection actions only;
- robust focus return.

Gate:

> Selection movement causes zero semantic mutations/provider calls.

## 44.7 Phase G - Foreground external child handoff

Deliver:

- process-group transfer;
- termios/alternate-screen restore;
- reacquire/redraw;
- stopped-child handling;
- `vim`/`less`/`top` fixtures.

Gate:

> Repeated child roundtrips leave terminal usable and shell state intact.

## 44.8 Phase H - Existing live/spatial/temporal/change views

Validate mounting:

- existing `watch` stream view;
- `map --live`;
- timeline/historical result;
- v0.6 plan/impact/recovery views.

Gate:

> No new live/temporal/change semantic layer was added to make these work.

## 44.9 Phase I - Hardening

Deliver:

- performance budgets;
- damage redraw;
- no-color/ASCII;
- terminal-injection tests;
- tmux/screen/SSH matrix;
- crash restoration;
- memory/leak tests.

## 44.10 Phase J - Dogfood gate

Use the Deck for real development/system administration for a sustained period.

Collect evidence on:

- whether persistent composition is actually faster/useful;
- terminal scrollback pain;
- external-program handoff friction;
- frequency of auxiliary use;
- whether users stay in Deck or revert to Rich TTY;
- whether layout complexity remains bounded.

No broader workspace roadmap should be approved before this evidence exists.

# 45. Definition of Done

v0.8 is complete only when all of the following are true:

1. `ono deck` (or final equivalent) explicitly starts the Deck on supported terminals.
2. Normal Ono startup remains Rich TTY by default.
3. Deck and Rich TTY share the same parser/evaluator/provider/value path.
4. The Deck uses the v0.7 presentation/view-tree path rather than a second renderer tree.
5. The Deck has one primary view, one optional auxiliary view and the existing command editor/status line.
6. There is no general pane/window/layout manager.
7. `HistoryEntry` and `ResultRef` power history/result reuse; no Deck history store exists.
8. Selection remains the v0.2 ephemeral selection and never implicitly targets/mutates.
9. `ContextStack` remains authoritative; focus does not alter it.
10. Existing KUANG/11 constrained views can be hosted without a new plugin UI protocol.
11. Existing v0.4 full-screen view hosting uses or is compatible with the generic terminal-lease infrastructure.
12. Existing `watch`/live views can run without introducing a second live type system.
13. v0.5 stale/gap/historical semantics remain visible.
14. v0.6 proposed/risk/protection/recovery semantics remain exact and explicit.
15. Terminal ownership is exclusive and recoverable.
16. `vim`, `less`, `top`-class external terminal programs receive proper handoff and return cleanly.
17. Suspend/resume restores terminal state and Deck content.
18. Resize collapses auxiliary content before making primary/editor unusable.
19. Renderer failures have a tested terminal-restoration/fallback path.
20. Untrusted text cannot inject terminal controls through host rendering.
21. No-color and ASCII modes remain usable.
22. Typing remains responsive under large/live-view load.
23. Repeated entry/exit/handoff shows no unbounded resource growth.
24. Non-TTY/script/redirection semantics are unchanged by the existence of v0.8.
25. Dogfooding confirms the Deck provides real value beyond visual novelty.

# 46. Explicit Non-Goals and Anti-Requirements

The following are not deferred implementation details. They are scope barriers for v0.8.

## 46.1 No second semantic mode

No command may execute differently because `deck_active == true` except for presentation/terminal attachment mechanics.

## 46.2 No new value/history/context/live ontology

Forbidden unless separately justified by an ADR unrelated to Deck presentation:

```text
DeckValue
WorkspaceResult
SessionEntry
WorkspaceContext
PresentationSelection
Live<T>
DeckObservation
DeckGap
```

## 46.3 No arbitrary window manager

No free splits, resizing handles, docking, workspace tabs or persistent layouts.

## 46.4 No embedded terminal emulator

External full-screen programs receive terminal handoff.

## 46.5 No dashboard builder

The Deck is not Grafana in a terminal.

## 46.6 No automatic refresh of static commands

`get process` remains a static result unless the user uses existing live/watch semantics.

## 46.7 No object-action framework

v0.8 does not derive a new action menu from object types. Existing explicit commands/selection references remain sufficient for this release.

## 46.8 No new plugin widget API

KUANG/11 uses the existing constrained view protocol.

## 46.9 No public theme system

v0.8 may use existing semantic style roles from presentation code, but does not standardize a theme DSL or Neuromancer theme.

## 46.10 No mouse-first design

Mouse support is not required for Definition of Done.

## 46.11 No persistent notification database

Status is bounded and derived from existing systems.

## 46.12 No browser-style navigation stack

Deck focus/view changes do not create a new semantic back/forward history beside v0.4 navigation and shell history.

## 46.13 No hidden reruns

Opening expired/historical results does not silently requery providers.

## 46.14 No fake cyberpunk behavior

No fake ICE, fake network scans, random telemetry, synthetic boot delay or decorative activity.

# 47. Architecture Review Checklist

Before merging a significant v0.8 subsystem, reviewers SHOULD ask:

1. Which earlier specification already owns this concept?
2. Is this code composing an existing view, or creating a parallel workspace type?
3. Does the evaluator/provider know the Deck exists? If yes, why?
4. Is this state semantic or merely ephemeral presentation state?
5. Could this use `HistoryEntry`, `ResultRef`, `ValueRef` or `ContextStack` instead of a new structure?
6. Is selection still the existing weak selection contract?
7. Does focus accidentally change context/target?
8. Is the existing view tree sufficient?
9. Are we adding a new KUANG/11 UI API unnecessarily?
10. Could the generic full-screen host also serve v0.4 maps?
11. Is terminal ownership exclusive at every transition?
12. Can cleanup run twice safely?
13. What happens on suspend, resize and child process stop?
14. What happens when the renderer fails midway through terminal acquisition?
15. Does an external program get real terminal semantics rather than partial emulation?
16. Are historical/stale/proposed values visually honest?
17. Does this add a permanent pane when an auxiliary/tab/transient view would suffice?
18. Does this require layout persistence or window management that the product does not need?
19. Would the ordinary Rich TTY shell still be complete if all Deck code were deleted?
20. Is this capability useful, or merely visually impressive?

A design that fails the consolidation questions SHOULD be revised before implementation.

# 48. End-to-End Interaction Scenarios

## 48.1 Fast expert workflow

1. User launches `ono deck`.
2. Command focus is active immediately.
3. User runs `get process | where cpu > 20`.
4. Typed result becomes primary.
5. User moves selection to a process.
6. No semantic state changes.
7. User explicitly invokes inspect/reference behavior.
8. Detail view replaces or transiently overlays primary according to existing view semantics.
9. User returns to command surface and continues typing without leaving the workspace.

Success criterion: fewer context switches than Rich TTY, no extra semantic concepts.

## 48.2 Investigative workflow

1. User opens a v0.4 spatial map as primary.
2. Context auxiliary shows current link/place from existing state.
3. User spatially navigates using v0.4 commands/actions.
4. User opens history auxiliary to compare an earlier result.
5. Opening history does not alter spatial context.
6. User explicitly restores/refers to an older result where needed.

Success criterion: workspace navigation and system navigation remain conceptually distinct.

## 48.3 Wrong-host prevention

1. User is connected to `prod-db-03` through native link semantics.
2. Status/context view shows that host and privilege.
3. User composes `plan restart service nginx`.
4. v0.6 plan result shows resolved target/context.
5. No visual focus trick can change the underlying target after plan sealing.
6. `apply` still performs v0.6 revalidation.

Success criterion: persistence improves visibility without becoming a second safety authority.

## 48.4 Historical incident investigation

1. User browses `HistoryEntry` records.
2. Opens retained result @42.
3. Deck clearly marks execution time/host.
4. User opens a v0.5 timeline related to the incident.
5. Coverage gap appears explicitly.
6. User runs a new command; new result becomes primary.
7. Old result remains reopenable if retention permits.

Success criterion: no screenshot history, no hidden reruns, no false currentness.

## 48.5 Recovery-aware workflow

1. User creates/seals a v0.6 change plan.
2. Plan occupies primary view.
3. Auxiliary shows protection/recovery facts from the plan.
4. User inspects a ZFS recovery asset.
5. Deck preserves the distinction between selective restore and full rollback.
6. User explicitly runs `apply @plan` through normal semantics.
7. Verification result becomes primary.

Success criterion: the Deck makes safety state harder to miss but never approves on behalf of the user.

## 48.6 External tool workflow

1. User types `vim file` in Deck.
2. Deck releases terminal lease.
3. Vim behaves exactly like a normal foreground terminal program.
4. User exits Vim.
5. Deck reacquires lease and redraws.
6. Shell history/context remain intact.

Success criterion: Ono remains a Unix shell, not an emulator shell.

## 48.7 Existing live view workflow

1. User runs `watch process`.
2. Existing live stream is mounted as primary.
3. Updates repaint without corrupting command input.
4. User focuses command surface and executes another command according to existing job/cancellation semantics.
5. No new `Live<T>` or Deck-specific observation state exists.

Success criterion: presentation integration, not semantic reinvention.

# 49. Future Boundary and Mandatory Reassessment

## 49.1 v0.9 should be Live View Integration, not Live Data reinvention

The next release MAY concentrate on long-running live-view ergonomics such as:

- follow vs manual navigation in a live view;
- view pause/resume semantics where already supported by streams/jobs;
- multiple concurrently mounted existing live views if truly needed;
- redraw/coalescing policy;
- long-running freshness/staleness UX;
- reconnect presentation for existing remote/live sources;
- resource budgets for hours-long Deck sessions.

It MUST reuse the existing `Stream<T>`, backpressure, v0.4 live topology and v0.5 temporal contracts.

## 49.2 Do not commit v0.10-v0.12 architecture yet

Before specifying later object-interaction, theme or extension releases, the project MUST diff the idea against v0.2-v0.8.

In particular, earlier specs already contain:

- interactive selection;
- `ValueRef`/`ObjectRef`/`ResultRef`;
- `ObjectPicker`/`CommandPalette`;
- context navigation;
- constrained KUANG/11 views;
- semantic style roles/themes as presentation concerns.

Later releases are justified only if they **remove friction by composing these concepts**, not if they rename/rebuild them.

## 49.3 Stop condition

After v0.9, the project SHOULD explicitly reassess whether the Deck roadmap should continue.

Evidence that argues for stopping includes:

- users frequently leave Deck to recover scrollback;
- terminal handoff makes common workflows jarring;
- auxiliary view rarely provides useful information;
- most value already exists in v0.7 Rich TTY;
- new roadmap items require parallel semantic systems.

Stopping would not make v0.8 wasted work if the generic full-screen host/terminal lease strengthens v0.4 and other existing views.

# 50. Release Rationale

## 50.1 Why v0.8 still deserves a release after consolidation

The critical review does not make the Deck unnecessary. It changes what the release is allowed to claim.

Ono already had TUI elements. v0.8 should not "add TUI".

Ono already had selection. v0.8 should not "add selection".

Ono already had history/result reuse. v0.8 should not "add session history".

Ono already had live streams and full-screen spatial views. v0.8 should not "add live data" or "add maps".

What v0.8 contributes is the missing structural capability:

```text
existing semantic/view capabilities
               |
               v
      generic full-screen host
               |
      +--------+--------+
      |                 |
existing map       bounded Deck
full-screen        composition
```

That is a real architectural improvement because it centralizes terminal ownership, failure recovery, resize, focus and child handoff instead of multiplying them per feature.

## 50.2 Why three surfaces are enough

A permanent Result + Context + Session + Activity + Command panel layout is attractive on paper but creates a focus/layout/state problem disproportionate to its value.

The revised design uses:

```text
primary + optional auxiliary + command + thin status
```

Context, history, help and jobs compete for the single auxiliary slot because the user rarely needs all of them simultaneously.

This is deliberate compression of UI concepts.

## 50.3 Why external program handoff is a feature, not a compromise

A shell that cannot transparently run `vim`, `less`, `ssh`, debuggers and arbitrary terminal programs has abandoned Unix interoperability.

The Deck should yield the terminal instead of pretending it can safely emulate every child application.

That makes the boundary visible and maintainable.

## 50.4 Why the Deck remains cyberpunk-compatible

A restrained implementation can still feel like a cyberdeck because real state persists spatially on screen: current system context, current result, live views, maps, plan/recovery facts.

The atmosphere emerges from capability and information density. Theming can later amplify it if the architecture proves valuable.

The Deck does not need fake terminology to justify its name.

\newpage

# 51. Closing Principle

The revised v0.8 should be judged less by how many panes it can show than by how little new machinery it needs to make Ono's existing semantics persistently visible.

The desired outcome is:

```text
fewer terminal-host implementations
fewer duplicated UI concepts
same semantic truth
more persistent situational awareness
```

The failure outcome is:

```text
new workspace state
new selection state
new history state
new live state
new plugin widgets
new layout language
new terminal emulator
```

The implementation must reject the second trajectory even if it produces a more impressive demo.

> **The Deck is successful when it feels like a richer place to operate Ono, while the architecture underneath becomes simpler rather than larger.**
