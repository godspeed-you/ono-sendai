---
title: "ONO-SENDAI Specification v0.7"
subtitle: "Presentation Consolidation & Rich TTY Interface"
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

# ONO-SENDAI Specification v0.7
## Presentation Consolidation & Rich TTY Interface

**Status:** Product and architecture extension specification  
**Scope:** Consolidation of the existing v0.2 presentation contracts, rich TTY projection, capability-aware terminal rendering, prompt-adjacent guidance, context visibility, HistoryEntry/ResultRef integration, accessibility and terminal hardening  
**Relationship:** Standalone extension to the published Ono-Sendai baseline and the v0.3-v0.6 extension specifications  
**Normative language:** MUST, MUST NOT, SHOULD, SHOULD NOT, MAY

> **Ono should not merely know more about the system. It should become better at showing what it knows without turning presentation into a second source of truth.**

---

# 0. Document Status and Relationship to Earlier Specifications

## 0.1 Standalone additive specification

This document is the independent specification for Ono-Sendai v0.7. It does not replace, merge, rewrite, regenerate or retrospectively modify the v0.2 baseline or the v0.3-v0.6 extension specifications.

v0.7 is deliberately a **consolidation release**. Earlier specifications already define most of the semantic ingredients needed for rich terminal interaction. v0.7 therefore MUST begin from inheritance, not invention.

The intended progression is:

```text
v0.2  Structure + interaction foundations
      Value / Stream<Value>, rendering separated from data,
      renderer hints, presentation profiles, adaptive TTY output,
      selection, constrained view trees, HistoryEntry, ResultRef,
      metadata-driven completion and KUANG/11 interactive lenses.

v0.3  Interoperability
      External Unix tools can cross typed/text boundaries honestly.

v0.4  Space
      Existing objects become a discoverable, navigable topology;
      shell and spatial views remain projections of one system.

v0.5  Time and causality
      Observations, events, evidence, coverage and gaps add temporal truth.

v0.6  Prospective change, protection and recovery
      Proposed effects, risk and recovery become inspectable semantics.

v0.7  Presentation consolidation
      Existing presentation and interaction contracts are unified into a
      production-quality rich TTY path without a second UI ontology.
```

Earlier specifications remain authoritative for the concepts they define. Where this document uses `Value`, `Stream<T>`, `HistoryEntry`, `ResultRef`, `ValueRef`, render hints, presentation profiles, the constrained view tree, command registries, schema registries, context stacks, observations, spatial objects, change plans or KUANG/11 lenses, those names retain their earlier meanings.

## 0.2 Inheritance rule

v0.7 MUST NOT create a new type, state machine, metadata registry or user-visible mode when an earlier specification already provides the required concept.

A new v0.7 abstraction is justified only when all of the following are true:

1. no earlier contract expresses the same concept adequately;
2. the abstraction removes duplicated implementation policy rather than adding another layer beside it;
3. its relationship to inherited types is explicit;
4. deleting future Deck Mode work would still leave the abstraction useful;
5. the abstraction does not require command providers to know which human renderer is active.

If these conditions are not met, the implementation MUST reuse or extend the inherited contract.

## 0.3 No retrospective editing

Earlier specifications MUST NOT be edited merely to simplify implementation.

If v0.7 exposes genuine ambiguity or collision between existing contracts, an ADR MUST resolve that ambiguity. The ADR MUST prefer consolidation over synonym creation and MUST record which earlier contract remains canonical.

## 0.4 Product intent

The purpose of v0.7 is not to add a decorative TUI around Ono and not to introduce a third interaction model beside shell and spatial navigation.

The purpose is narrower:

> **Make the ordinary interactive TTY a better projection of the semantic system Ono already has.**

The release therefore answers:

> How can Ono expose type, identity, context, uncertainty, risk, history and discoverability more effectively in the normal terminal flow while preserving ordinary shell behavior?

The answer MUST remain compatible with pipes, redirection, scripts, external programs, terminal scrollback and the interaction contracts already defined by v0.2-v0.6.

## 0.5 Why v0.7 precedes Deck Mode

The baseline already permits interactive TTY tables, selection, drill-down and constrained views. What it does not yet provide as a release-quality implementation contract is one consolidated policy for:

- resolving renderer hints and presentation profiles;
- adapting ordinary TTY output to capabilities and dimensions;
- presenting prompt-adjacent guidance without owning the whole screen;
- projecting v0.4-v0.6 context and safety state consistently;
- connecting existing `HistoryEntry` and `ResultRef` semantics to rich interaction;
- sanitizing, testing and degrading terminal presentation predictably.

v0.7 establishes those policies first. A later Deck workspace may compose the same values and views in a persistent full-screen layout, but v0.7 MUST remain valuable if that later workspace is never implemented.

## 0.6 Release thesis

v0.7 is built around two statements:

> **The UI is a projection of Ono's model, never a competing model.**

> **Consolidate before extending.**

Both statements are normative.

# 1. Product Thesis

## 1.1 Ono already has a presentation model

v0.2 already establishes that native commands produce `Value` or `Stream<Value>`, that renderers consume those values only for human/text sinks, that objects may carry renderer hints and presentation profiles, and that a capable TTY may progressively enhance output with rich tables, selection and drill-down.

v0.7 MUST treat those contracts as the starting point rather than re-specifying them under new names.

The problem solved by v0.7 is therefore not "introduce presentation". It is:

- remove presentation decisions that are scattered through providers or command implementations;
- make resolution of existing render metadata deterministic;
- make the ordinary TTY experience consistently rich without creating a mode-dependent language;
- make context from v0.4-v0.6 visible with one coherent policy;
- make the inherited interactive contracts production-quality across terminal capabilities.

## 1.2 Rich does not mean graphical

The target is not a GUI transplanted into a terminal.

The target is a normal shell session that can make disciplined use of:

- adaptive columns and record layouts;
- semantic emphasis;
- interactive selection where v0.2 already permits it;
- compact context near the prompt;
- registry-derived completion and guidance;
- structured diagnostics;
- existing result references;
- bounded transient terminal regions that disappear cleanly after input is committed.

Committed command output remains ordinary terminal history unless an existing or later specialized view explicitly owns a different lifecycle.

## 1.3 Rich TTY is a presentation profile, not a shell mode

v0.7 MUST NOT introduce a semantic `Console Mode` state.

Interactive Ono already has one shell language and one execution model. When stdin/stdout are attached to a capable TTY, the renderer MAY select a richer **TTY presentation profile**. When capabilities, user preference or output destination do not permit that profile, it falls back to a simpler projection.

Conceptually:

```text
same command + same semantic result
              |
              +-- capable TTY  -> rich TTY projection
              +-- simple TTY   -> plain TTY projection
              +-- redirect     -> deterministic text/serializer policy
              +-- pipe         -> typed or explicit interop boundary
              +-- script       -> no hidden interactive behavior
```

There is no `if rich_ui { different command semantics }` branch.

## 1.4 The emotional thesis

The desired effect is **orientation without ceremony**.

A user should feel:

> I know where I am.  
> I know what Ono thinks this value is.  
> I know what is valid next.  
> I can see uncertainty and danger without opening a manual.  
> I can still type a command and move on immediately.

The user MUST NOT feel trapped in a menu system or forced to learn a parallel UI vocabulary.

## 1.5 The v0.7 truth rule

The cumulative truth rules become:

```text
v0.3  Do not invent structure.
v0.4  Do not invent topology.
v0.5  Do not invent history or causality.
v0.6  Do not invent the future or recoverability.
v0.7  Do not invent semantics or duplicate existing concepts in presentation.
```

A renderer may omit detail for readability. It MUST NOT fabricate detail.

A renderer may summarize confidence, history, protection or risk. It MUST preserve access to the inherited semantic state from which that summary was derived.

# 2. Core Invariants

The following invariants are non-negotiable.

1. **Existing names remain canonical.** `Value`, `Stream<T>`, `HistoryEntry`, `ResultRef`, `ValueRef`, context frames and earlier observation/safety types MUST NOT receive v0.7 synonyms.
2. **Values precede views.** Native commands produce typed values independent of renderer choice.
3. **No semantic UI mode.** Rich TTY is a presentation profile selected at the human-output boundary, not a different command-execution mode.
4. **Commands do not branch on presentation profile.** A native command MUST NOT change semantic output because rich TTY rendering is active.
5. **Presentation is side-effect free.** Rendering a value MUST NOT mutate the target system or the value being rendered.
6. **No hidden semantic state in views.** Information required to reproduce meaning MUST exist outside transient presentation state.
7. **Progressive enhancement remains the model.** TTY richness MUST degrade toward plain output without changing command meaning.
8. **Terminal scrollback remains useful.** Ordinary interactive output MUST NOT require permanent alternate-screen ownership.
9. **Pipes, redirection and scripts remain first-class.** Rich presentation MUST NOT leak control sequences or hidden interaction into non-interactive paths.
10. **External Unix behavior remains honest.** Foreign command bytes are not silently reinterpreted merely to obtain prettier output.
11. **Unknown remains visible.** `null`, missing evidence, unknown provenance, gaps, unknown risk and uncertain future state MUST NOT be normalized into plausible-looking facts.
12. **Color is never the only signal.** Meaning MUST remain available without color.
13. **Width loss reduces decoration before meaning.** Narrow terminals remove secondary presentation before obscuring identity, uncertainty or safety-critical state.
14. **History stays `HistoryEntry`.** v0.7 enriches or renders the inherited history contract; it MUST NOT create a parallel session-history ontology.
15. **Prior results stay `ResultRef`.** v0.7 MAY expose them more effectively but MUST NOT invent a competing result-reference type.
16. **Selection stays ephemeral until explicitly consumed.** Existing v0.2 selection semantics remain authoritative; visual selection alone MUST NOT mutate context or target state.
17. **Existing live semantics remain valid.** `Stream<T>`, `watch`, backpressure, cancellation and v0.5 observation/coverage/gap semantics are inherited. v0.7 MUST NOT redefine them or pretend they do not exist.
18. **Existing KUANG/11 view contracts remain valid.** v0.7 MUST NOT revoke the constrained view-tree/lens capability already defined by v0.2, and MUST NOT expand it into an arbitrary terminal-ownership API.
19. **Accessibility is architectural.** No-color, ASCII-safe, reduced-motion and low-capability operation are core renderer requirements.
20. **Output remains copyable and inspectable.** Any rich projection MUST have a stable textual equivalent or underlying typed representation.
21. **Presentation failures degrade, not corrupt.** Renderer failure MUST fall back to simpler presentation without rewriting semantic command outcome.
22. **Future Deck composition must reuse these contracts.** v0.7 MUST NOT freeze pane, focus or workspace semantics merely to anticipate v0.8.
23. **No speculative framework.** v0.7 MUST NOT add a new reactive system, new plugin UI ABI, new theme language or new history subsystem simply because a future release might use one.
24. **Deletion test.** If the future Deck were cancelled, every v0.7 architectural addition MUST still justify itself in the ordinary shell.

# 3. Terminology and Conceptual Model

## 3.1 Inherited semantic vocabulary

v0.7 uses the following earlier concepts without renaming them:

| Concept | Canonical owner | v0.7 use |
|---|---|---|
| `Value` / typed records | v0.2 | input to human presentation |
| `Stream<T>` | v0.2 | finite or unbounded semantic stream |
| renderer hints | v0.2 | type/schema-supplied presentation advice |
| presentation profiles | v0.2 | policy for human projection |
| constrained view tree | v0.2 | host-owned semantic view description |
| `ValueRef` | v0.2 | stable reference used by context/graph semantics |
| `HistoryEntry` | v0.2 | semantic command-history record |
| `ResultRef` | v0.2 | bounded reference to a prior structured result |
| command/schema registries | v0.2 | source for help/completion/render defaults |
| context stack | v0.2 | execution/object context |
| spatial place/trail/relationships | v0.4 | orientation and topology |
| observations/events/coverage/gaps | v0.5 | temporal truth and uncertainty |
| change plan/risk/protection/recovery | v0.6 | prospective and safety truth |

When implementation types differ from the conceptual spelling above, the existing repository contract remains authoritative.

## 3.2 PresentationDescriptor

`PresentationDescriptor` is the principal v0.7 consolidation abstraction.

It is **not a new source of semantic metadata**. It is a normalized, renderer-facing compilation of presentation information that already exists in schema defaults, renderer hints, presentation profiles, explicit user requests and inherited semantic annotations.

Conceptually:

```text
schema default_view / renderer hints
              +
command result hint where already supported
              +
explicit user projection (`select`, explicit view request)
              +
semantic annotations from v0.4-v0.6
              |
              v
      PresentationDescriptor
              |
              v
      host-owned view/render tree
```

A `PresentationDescriptor` MUST be reproducible from authoritative inputs. It MUST NOT become a second schema registry.

## 3.3 Existing view tree

v0.2 already defines a constrained host-owned view tree with conceptual components including `Text`, `Table`, `Tree`, `Graph`, `KeyValue`, `LogStream`, `Sparkline`, `Gauge`, `Tabs`, `Split`, `CommandPalette`, `ObjectPicker` and `StatusLine`.

v0.7 does not replace that model with a new `RenderModel` ontology.

For ordinary rich TTY projection, v0.7 standardizes a **core linear subset** and a few host-owned utility forms needed by the shell renderer. Implementation MAY represent these as existing view-tree nodes or compatible internal nodes, but there MUST NOT be two independent trees requiring semantic translation between them.

## 3.4 Renderer

A renderer converts the resolved host-owned view description into bytes appropriate for an output sink.

v0.7 distinguishes renderer implementations such as:

- plain text renderer;
- rich TTY renderer;
- existing machine serializers, which operate on semantic values rather than on human view nodes.

## 3.5 TTY presentation profile

A TTY presentation profile is a set of human-presentation policy choices such as:

- rich vs plain styling;
- available terminal capabilities;
- width/density budget;
- whether prompt-adjacent transient guidance is allowed;
- ASCII/Unicode policy;
- color policy.

It MUST NOT change command semantics or pipeline types.

## 3.6 Prompt surface

The **Prompt Surface** is the ordinary command-entry boundary already implied by Ono's prompt/editor model. v0.7 makes its presentation responsibilities explicit.

It includes the editable input line and MAY include a compact adjacent context/status line. It is not a pane system and is not persistent alternate-screen UI.

## 3.7 Guidance surface

The **Guidance Surface** is a bounded transient TTY projection used while editing a command.

It may show registry- and parser-derived information such as:

- completion candidates;
- required arguments/options;
- expected input/output types;
- command synopsis;
- parse/type diagnostics;
- mutation/risk facts already defined by command metadata or v0.6.

The Guidance Surface is presentation state only. It MUST disappear or redraw cleanly when the command is committed.

## 3.8 Context summary is a projection, not a type

v0.7 MUST NOT introduce a new canonical `ContextSummary` semantic record merely for the prompt.

A compact context line is derived from existing context sources, for example:

- active link/host and identity from the context stack;
- spatial place/trail from v0.4;
- historical/present state from v0.5;
- proposed/change-plan context from v0.6;
- job count from existing shell job control.

If a structured `context` inspection command exists, that command returns the inherited semantic context representation, not the formatted prompt summary.

## 3.9 HistoryEntry

`HistoryEntry` remains the canonical semantic history record from v0.2.

v0.7 MAY specify additional optional/versioned fields required for richer presentation, but implementations MUST evolve the existing record rather than introduce `SessionEntry` as a parallel concept.

## 3.10 ResultRef

`ResultRef` remains the canonical bounded reference to a prior structured result from v0.2.

v0.7 defines richer display, retention diagnostics and interactive reuse around `ResultRef`; it does not create `ResultReference` or another competing handle.

## 3.11 Selection

Selection is inherited ephemeral view state from v0.2.

A selected row/value does nothing by itself. It becomes semantic input only through an explicit operation already defined by the shell/view contract, such as `inspect`, `enter`, an explicit picker confirmation or a language-level reference token once that syntax is stabilized.

Selection MUST NOT silently become target, context, mutation scope or history state.

# 4. Output Contexts and Progressive Enhancement

## 4.1 No new execution-mode enum

v0.7 MUST NOT require a new semantic execution-context enum such as `PLAIN_INTERACTIVE`, `CONSOLE_INTERACTIVE` or `MACHINE_OUTPUT` merely to choose presentation.

The implementation already knows facts that matter independently:

```text
stdin_is_tty
stdout_is_tty
stderr_is_tty
interactive_shell
output_sink_kind
explicit_serializer
terminal_capabilities
user presentation preferences
```

Renderer selection SHOULD be derived from these facts.

## 4.2 Canonical behavior inherited from v0.2

The baseline progressive-enhancement rule remains authoritative:

```text
TTY:      human presentation; rich when capable, plain when not
pipe:     semantic stream inside Ono; explicit conversion at external boundary
redirect: deterministic serialization or explicit text rendering
script:   no hidden terminal interaction
```

v0.7 standardizes the rich/plain TTY branch. It does not replace the other branches.

## 4.3 Non-interactive execution

Examples:

```bash
ono -c 'get process | where cpu > 20'
ono script.ono
printf '...' | ono -c '...'
```

Non-interactive execution MUST NOT depend on cursor addressing, prompt-adjacent guidance or terminal focus state.

It MUST NOT emit cursor-motion control sequences unless an explicit human-rendering command intentionally targets a terminal sink.

## 4.4 Plain TTY presentation

Plain TTY presentation is the same interactive shell with conservative human formatting.

It MUST work in low-capability terminals and when explicitly requested.

Example:

```text
local://~ > get process | take 2
PID   NAME      CPU
1     systemd   0.1%
642   sshd      0.0%
local://~ >
```

## 4.5 Rich TTY presentation

When a supported TTY and user policy allow it, Ono MAY use:

- ANSI semantic styling;
- width-aware adaptive layouts;
- the inherited ephemeral selection cursor;
- cursor-aware line editing;
- bounded transient completion/guidance;
- transient progress for operations that already expose progress state.

Committed command text and ordinary final results MUST become stable scrollback content.

## 4.6 Explicit controls

Users MUST be able to force a conservative presentation path.

At minimum, stable equivalents of the following are required:

```text
--plain
--no-color
--color=auto|always|never
```

An explicit `--rich` diagnostic/override switch MAY exist, but v0.7 MUST NOT require a `--console` mode because rich TTY is not a semantic shell mode.

## 4.7 Capability selection

Rich TTY presentation MAY be selected when all of the following are true:

- interactive shell behavior is active where required;
- stdout is a terminal;
- required terminal capabilities are available;
- dimensions are sufficient for the chosen rich behavior;
- the user has not forced plain/no-interaction presentation;
- accessibility preferences permit the specific effects used.

Selection MUST be conservative and deterministic from inspectable inputs.

## 4.8 `TERM=dumb`

For `TERM=dumb`, Ono MUST use a plain, line-oriented presentation and MUST NOT emit color or cursor movement.

## 4.9 Redirection and external process boundaries

Redirection and typed-to-byte interop remain governed by v0.2/v0.3.

Rich TTY presentation MUST NOT alter adapter negotiation or silently turn a human table into an external-process API.

## 4.10 SSH, tmux, screen and mosh

Rich TTY presentation SHOULD operate correctly through common multiplexers and remote transports.

Capability detection MUST describe the effective terminal contract, not maintain a collection of brand-specific product assumptions.

# 5. Architecture

## 5.1 Required separation

The architecture MUST preserve the baseline separation and make only one additional consolidation step explicit:

```text
command / provider / pipeline
            |
            v
      Value / Stream<T>
            |
            +-------------------------------> machine serializer / typed sink
            |
            v
existing render hints + schema default_view + presentation profile
            |
            v
   resolve PresentationDescriptor
            |
            v
 existing host-owned view tree / linear render description
            |
            +--------------------+
            |                    |
            v                    v
     plain renderer       rich TTY renderer
            |                    |
            v                    v
        text bytes          terminal bytes
```

The `PresentationDescriptor` resolution step consolidates existing metadata. It does not insert a new semantic model between `Value` and the shell.

## 5.2 Forbidden architecture

The following patterns are forbidden:

```text
command
  +-- if rich_tty: return UiProcessRow
  +-- else:        return Process
```

```text
provider -> ANSI table bytes -> parser -> rich view
```

```text
Value -> NewSemanticValue -> RenderModel -> ExistingViewTree
```

The third example is forbidden because it creates translation layers between two presentation ontologies without adding capability.

## 5.3 Presentation resolution service

A core presentation service or equivalent cohesive subsystem SHOULD own:

- collection and precedence of existing render hints/presentation profiles;
- compilation to `PresentationDescriptor`;
- mapping into the existing host-owned view representation;
- width/density budgeting;
- terminal capability normalization;
- renderer invocation and fallback;
- sanitization policy hooks;
- presentation diagnostics.

Exact crate names are implementation-defined.

## 5.4 Authoritative metadata sources

Descriptor inputs MAY originate from:

1. explicit user presentation/projection requests;
2. existing command-result renderer hints;
3. schema `default_view` and schema metadata from v0.2;
4. existing presentation profiles;
5. provider-declared non-executable metadata allowed by existing contracts;
6. v0.4-v0.6 semantic annotations that already exist on values/context.

v0.7 MUST NOT add a second hand-maintained presentation registry containing copies of schema fields or command semantics.

## 5.5 Descriptor precedence

Precedence SHOULD be deterministic. A recommended order is:

```text
explicit user request
> command/result hint already present in the execution contract
> schema/type default_view and renderer hints
> generic type-based fallback
```

v0.4-v0.6 safety/uncertainty markers are not decorative overrides; when semantically applicable they MUST survive whatever view family is selected.

## 5.6 Renderer isolation and fallback

A renderer panic or recoverable presentation error MUST NOT invalidate an otherwise successful command if a simpler safe projection can be produced.

Recommended fallback:

```text
RichTtyRenderer
      |
      v on presentation failure
PlainTextRenderer
      |
      v on formatting failure
safe diagnostic fallback
```

The semantic command exit/result state remains authoritative.

## 5.7 Determinism

Given the same semantic value, resolved descriptor, terminal dimensions/capabilities and locale/unit policy, stable renderers SHOULD produce deterministic output except for fields that are explicitly dynamic.

## 5.8 Presentation diagnostics

A diagnostic mode SHOULD expose:

- effective TTY/plain policy;
- detected terminal capabilities;
- descriptor inputs and winning precedence source;
- selected view family;
- width-budget decisions;
- fallback decisions.

These diagnostics MUST NOT corrupt machine output or become required runtime state.

# 6. Presentation Metadata Consolidation Contract

## 6.1 Goal

v0.7 MUST turn the presentation information already distributed across v0.2 schemas, renderer hints and presentation profiles into one deterministic **resolved descriptor** at render time.

The resolved descriptor answers presentation questions such as:

- Which fields form the recognizable identity of a compact row?
- Which fields are useful in the default human view?
- Which fields are secondary under width pressure?
- Which view family is appropriate for this value?
- Which semantic annotations require visible treatment?

It MUST NOT answer domain questions such as whether a process is safe to stop or whether a causal relation is proven. Those answers come from earlier semantic models.

## 6.2 Canonical resolved shape

A reference internal shape is:

```text
PresentationDescriptor {
    source_type: TypeId
    preferred_view: ViewFamily?
    identity_fields: List<FieldId>
    summary_fields: List<FieldId>
    field_order: List<FieldId>
    fields: Map<FieldId, FieldPresentation>
    density_hint: DensityHint?
}

FieldPresentation {
    label: String?
    importance: PRIMARY | SECONDARY | DETAIL
    align: AUTO | LEFT | RIGHT | CENTER
    width: WidthPolicy
    truncate: TruncationPolicy
    unit_style: UnitStyle?
}
```

This shape is an implementation reference, not a new public schema format. If existing schema descriptors can represent these fields directly, implementations SHOULD extend/reuse them rather than serialize a duplicate descriptor database.

## 6.3 Compilation, not duplication

For a `Process`, the baseline schema may already contain:

```yaml
identity: [pid, started]
default_view:
  columns: [pid, name, cpu, memory, user]
```

v0.7 MAY derive display importance, width policy and labels from this information plus renderer hints. It MUST NOT copy the full `Process` field catalog into a separate presentation registry.

CI SHOULD be able to detect presentation metadata that references nonexistent fields.

## 6.4 Identity

Presentation identity hints help keep an object recognizable when width decreases.

They MUST NOT replace `ObjectRef`, `ValueRef`, spatial identity or the canonical type identity defined elsewhere.

## 6.5 Semantic annotations

Sensitive, uncertain, historical, proposed, risk and protection state MUST be sourced from the established semantic contracts.

A field-presentation hint may control visibility or label choice; it MUST NOT be the only security or safety boundary.

## 6.6 User projection wins

If a user explicitly selects fields using ordinary Ono operators such as `select`, that semantic projection controls the available result fields.

The renderer MUST NOT silently reconstruct omitted value fields merely because a default view would have included them.

Presentation-only metadata such as a result count, type label or safety notice MAY be shown separately and MUST not masquerade as pipeline columns.

## 6.7 Versioning discipline

If descriptor serialization is needed for caches or test fixtures, it MUST be versioned.

It SHOULD NOT become a public plugin ABI in v0.7. Existing KUANG/11 schema/view contracts remain the public extension boundary.

# 7. Core View-Tree Use for Ordinary TTY Rendering

## 7.1 Reuse the v0.2 view tree

v0.2 already establishes a constrained host-owned view tree for specialized views and KUANG/11 lenses. v0.7 MUST reuse that conceptual model rather than invent a parallel `RenderModel` tree.

For ordinary shell output, only a linear/scrollback-friendly subset is required.

## 7.2 Required ordinary-TTY node capabilities

The host renderer MUST be able to express at least the information content of:

```text
Text
Table
KeyValue / Record
List
Tree
StatusLine or equivalent compact status
Diagnostic / Notice
Progress summary where an existing operation exposes progress
```

Existing v0.2 components such as `Graph`, `LogStream`, `Sparkline`, `Gauge`, `Tabs`, `Split`, `CommandPalette` and `ObjectPicker` remain valid baseline concepts. v0.7 neither removes them nor requires all of them for ordinary scrollback rendering.

## 7.3 No second tree

An implementation MAY introduce internal structs optimized for layout, but they MUST be an implementation detail of the existing host view/render system.

The project MUST NOT maintain two public conceptual trees where plugins/providers emit one tree and the core renderer requires translation into an unrelated second tree.

## 7.4 Text and semantic roles

Styled text uses semantic roles, not literal colors in domain code.

Representative roles include:

```text
NORMAL
MUTED
EMPHASIS
IDENTITY
SUCCESS
WARNING
ERROR
RISK
UNKNOWN
PROVENANCE
COMMAND
CODE
```

If equivalent semantic theme tokens already exist in the implementation, v0.7 MUST reuse them.

## 7.5 Table

A table is a projection of homogeneous values, not the pipeline value itself.

Row identity MAY reference inherited object/value references so the existing selection/drill-down behavior can act on the underlying value. The rendered cells MUST NOT become the only copy of that value.

## 7.6 Record / KeyValue

Single structured objects SHOULD be projectable as labeled fields using the existing `KeyValue` capability or an equivalent host-owned record node.

## 7.7 Tree

Tree indentation represents hierarchy only when hierarchy exists semantically. It MUST NOT fabricate v0.4 topology from visual grouping.

## 7.8 Diagnostics and notices

Diagnostic/notice presentation MUST retain text-level severity and stable diagnostic identity where available. Color or symbols only supplement the textual meaning.

## 7.9 Progress

Progress presentation is permitted only for execution/progress information already emitted by the relevant existing command/job contracts.

v0.7 MUST NOT create a new generic observable/live-value type to support a spinner or progress row.

## 7.10 No hidden executable callbacks

Ordinary presentation nodes MUST NOT carry arbitrary executable callbacks.

Inherited constrained lens/action semantics remain governed by v0.2/KUANG/11. v0.7 does not widen that boundary.

# 8. Plain Text Renderer

## 8.1 Plain text is a product surface

Plain rendering is not a debugging fallback. It is the canonical readable representation for:

- redirected output;
- low-capability terminals;
- logs;
- copied examples;
- no-color operation;
- test fixtures.

## 8.2 Stability

Plain output SHOULD be stable enough for humans and documentation, but users MUST NOT be encouraged to parse native human tables when a structured serializer or typed Ono pipeline is available.

## 8.3 No ANSI

Plain output MUST contain no ANSI color, cursor movement or terminal OSC sequences.

## 8.4 Table rendering

Tables SHOULD use readable column alignment when terminal width is known.

When width is unknown or insufficient, the renderer MAY switch to a record-per-item representation.

Example:

```text
PID   NAME      MEMORY
120   postgres  651.3 MiB
```

may become:

```text
pid: 120
name: postgres
memory: 651.3 MiB
```

rather than truncating every column beyond recognition.

---

# 9. Rich TTY Renderer

## 9.1 Goals

The Rich TTY Renderer is the capable-terminal implementation of the inherited TTY presentation profile. It SHOULD optimize for scanability, compactness, type/identity recognition, uncertainty visibility and low interaction overhead.

It MUST remain a renderer. It is not an execution mode and MUST NOT become a source of domain state.

## 9.2 Styling policy

Rich TTY styling MAY use color, bold/dim intensity, underline, reliable Unicode box drawing and compact symbols.

The renderer MUST have a no-color and ASCII-safe path. Semantic roles MUST map through the existing theme/presentation-token mechanism rather than hard-coded domain colors.

## 9.3 Existing theme contract

v0.2 already anticipates semantic presentation tokens and theme configuration. v0.7 MAY harden and centralize their use, but MUST NOT introduce a second theme system.

A later release may ship richer bundled themes, including a Neuromancer/Ono-Sendai theme, without changing semantic render nodes.

## 9.4 Width behavior

There is no normative width at which Ono changes "mode". Instead, individual rich features have minimum capability/space requirements.

A recommended implementation policy is:

```text
>= 80 columns   full default field set where appropriate
60-79 columns   compact field set / tighter guidance
< 60 columns    aggressively compact or stacked plain-like projection
```

Exact thresholds MAY change after testing but MUST be centralized and deterministic.

## 9.5 Height behavior

Prompt-adjacent guidance MUST be bounded by terminal height. A completion list MUST NOT consume the entire visible terminal by default.

## 9.6 Resizing

Resize events MAY reflow transient prompt/guidance state and future uncommitted view state.

Already committed ordinary scrollback output MUST NOT be retrospectively rewritten.

# 10. Rich TTY Interaction Model

## 10.1 The command line stays primary

The ordinary Ono command line remains the primary interaction mechanism. Rich TTY presentation MUST never add ceremony to users who already know the command they want to execute.

## 10.2 Prompt composition

The prompt MAY summarize inherited context:

```text
[link/place] [exceptional temporal/change state] [jobs] >
```

Example:

```text
prod-db-03:/srv/app [PAST] [2 jobs] >
```

The exact visual syntax is non-normative.

## 10.3 Prompt information budget

Prompt metadata MUST be aggressively bounded. It SHOULD prioritize:

1. active host/link identity when not obvious;
2. current place/path;
3. exceptional state such as historical or proposed context;
4. exceptional job/error state;
5. optional low-priority decoration.

The prompt MUST NOT become a dashboard.

## 10.4 Editing

Rich presentation MUST build on the existing editor/parser contract rather than create a second input editor solely for visual features.

Commands longer than terminal width and multiline commands MUST render without corrupting prior scrollback.

## 10.5 Commit boundary

When Enter commits a syntactically complete command:

1. transient guidance is cleared;
2. the command follows the existing `HistoryEntry` recording path;
3. committed command text becomes stable scrollback;
4. execution follows the normal shell path;
5. inherited progress state MAY be shown transiently;
6. final human output is rendered from the semantic result;
7. the next prompt is drawn.

No new session-entry state machine is introduced.

## 10.6 Incomplete syntax and parse errors

If the parser reports incomplete syntax, Enter MAY continue multiline entry according to existing grammar/editor rules.

Parse diagnostics SHOULD render close to the relevant source range and MUST have a plain textual form.

Example:

```text
get process | where cpu >
                        ^ expected expression
```

Caret placement MUST use the same Unicode display-width implementation as the renderer.

# 11. Context Surface

## 11.1 Purpose

Context should be visible enough that the user does not accidentally operate in the wrong semantic world.

This is especially important after v0.4-v0.6 because context can now include:

- remote link identity;
- spatial place;
- historical time;
- proposed-plan context;
- elevated privilege state.

## 11.2 Exceptional contexts are prominent

Historical or proposed-state context, degraded link state and elevated destructive context MUST receive stronger presentation than routine local present-state operation. v0.7 MUST reuse the canonical context/status vocabulary established by the specification that owns that semantic state rather than inventing new prompt tokens.

## 11.3 Historical context

If the shell is in a v0.5 historical context, Rich TTY presentation MUST preserve the v0.5 distinction between sufficiently covered historical context (`[PAST]`) and materially partial or uncertain historical context (`[PAST?]`). The marker MUST remain visible near the prompt or equivalent immediate context surface for as long as that historical context is active.

Color alone is insufficient, and v0.7 MUST NOT collapse `[PAST?]` into `[PAST]` merely to simplify presentation.

## 11.4 Proposed/change context

If an operation is scoped to a v0.6 plan or proposed-state context, the prompt or immediate context line SHOULD identify it.

## 11.5 Privilege context

When Ono knows it is operating with materially elevated privileges, the prompt SHOULD make that visible without resorting to alarm fatigue.

## 11.6 Remote links

The active remote link MUST be distinguishable from local operation.

A remote target MAY be summarized by stable short identity, but the full identity MUST be available through normal inspection.

---

# 12. Completion Presentation Integration

## 12.1 Completion semantics are inherited

Schema-aware, provider-aware and context-aware completion is already a v0.2 contract. v0.7 MUST NOT claim a second completion engine or duplicate the command/type taxonomy.

Its responsibility is to make existing completion information render consistently in the ordinary TTY.

## 12.2 Authoritative sources

Candidate generation MUST continue to derive from the same grammar, command registry, schema registry, provider metadata and context information used by execution and introspection.

A presentation-only cache or index MAY accelerate lookup, but it MUST be derivable from those sources and MUST NOT become independently authoritative.

## 12.3 Transient candidate representation

The editor/render boundary MAY use an internal candidate structure carrying information such as:

```text
CompletionCandidate {
    insert_text: String
    display_text: String
    kind: CompletionKind
    description: String?
    expected_type: TypeId?
    result_type: TypeId?
    source: RegistryRef
}
```

This is an implementation data-transfer shape, not a new public semantic type or persistent registry.

## 12.4 Ranking and type awareness

Existing completion behavior SHOULD prioritize syntactic validity and semantic/type compatibility before historical relevance.

Examples already implied by v0.2 remain expected:

```text
get process | where <TAB>   -> Process fields
get process | sort <TAB>    -> sortable Process fields
memory > 5<TAB>             -> compatible units
```

v0.7's contribution is the bounded, readable presentation of these candidates.

## 12.5 Side-effect and latency discipline

Completion MUST NOT mutate target systems.

Remote/provider-backed discovery MUST obey existing provider side-effect rules and SHOULD be cancellable, cached or latency-bounded so editing remains responsive.

## 12.6 Large candidate sets

Large candidate sets SHOULD be filtered/paged within transient guidance rather than dumped into terminal scrollback.

Dismissal or staleness of the candidate view MUST NOT change the editor buffer unless the user explicitly accepts a candidate.

# 13. Command Guidance Surface

## 13.1 Purpose

The Guidance Surface is a **presentation of existing parser, registry, type and safety metadata** while a command is being edited.

It SHOULD help answer:

- What syntactic construct am I writing?
- Which continuations are valid?
- What input/output type is expected?
- Which required argument or option is missing?
- Does existing command/v0.6 metadata identify this as mutating or risky?

## 13.2 No new guidance ontology

v0.7 MUST NOT create a separate command-description database for guidance.

The same metadata that powers `help`, completion, parser/type validation and `explain` SHOULD supply the guidance projection.

## 13.3 Bounded assistance

Guidance MUST remain compact and dismissible. It is not a permanent pane and not an inline replacement for full documentation.

Example:

```text
stop process
input:     Stream<Process>
mutation:  yes
```

If v0.6 has established a relevant risk/protection fact, that fact MAY be added. The renderer MUST NOT invent it from the verb name alone.

## 13.4 Opaque external commands

For external commands without a trusted adapter/metadata contract, guidance MUST remain honest about the opaque boundary. Ono MAY still provide generic executable/path completion but MUST NOT fabricate argument schemas or result types.

## 13.5 Help escalation

The Guidance Surface SHOULD provide a clear route to existing `help`, `explain`, `inspect` or equivalent commands where deeper information is required.

# 14. Rich Native Result Rendering

## 14.1 Default table rendering

Homogeneous lists of record-like native objects SHOULD render as tables when width allows.

Example:

```text
PID    NAME            CPU    MEMORY
3647   claude-desktop  18%    3.21 GiB
1719   postgres        4%     651 MiB
```

The table is not the pipeline value.

## 14.2 Single-object rendering

Single structured objects SHOULD render as a compact record where that better exposes field semantics.

## 14.3 Null and unknown

`null` MUST render distinctly from zero, empty string and false.

Suggested plain spelling:

```text
null
```

An unknown or unobserved semantic state MAY use a distinct marker, but the spelling MUST be documented and consistent.

## 14.4 Quantities

Quantities SHOULD preserve unit-aware formatting.

Human-readable formatting such as `3.21 GiB` is acceptable because the underlying value remains typed.

## 14.5 Provenance

Provenance MAY be collapsed in normal table views but MUST be available through normal inspection.

Where provenance affects trust materially, the renderer SHOULD expose a compact indicator.

## 14.6 Risk

Risk-related values from v0.6 SHOULD use semantic emphasis plus text labels.

Example:

```text
risk: HIGH
protection: PARTIALLY_PROTECTED
```

rather than relying on red/yellow alone.

## 14.7 Temporal state

Historical values SHOULD visibly indicate their historical timestamp/context when there is a realistic chance they could be mistaken for present state.

## 14.8 Proposed values

Proposed values MUST be marked as proposed and MUST preserve the v0.6 confidence classes `GUARANTEED`, `EXPECTED`, `POSSIBLE` and `UNKNOWN` where applicable.

---

# 15. Width Adaptation and Density

## 15.1 Principle

Narrowness is a presentation problem, not permission to lose semantic honesty.

## 15.2 Column budgeting

Table layout SHOULD use a deterministic width budgeting algorithm based on:

- terminal width;
- field importance;
- minimum width;
- preferred width;
- truncation policy;
- identity preservation.

## 15.3 Truncation markers

Truncation MUST be visibly marked.

A truncated value MUST NOT be visually indistinguishable from the full value.

## 15.4 Horizontal scrolling

v0.7 SHOULD NOT make horizontal scrolling a primary result-navigation mechanism because stable scrollback is a core requirement.

For very wide records, column reduction or vertical record layout is preferred.

## 15.5 Row limits

Large native results SHOULD obey an explicit display policy.

The renderer MAY show a bounded preview with a message such as:

```text
showing 50 of 8,241 values
```

but it MUST make clear whether values were omitted only from presentation or from pipeline execution.

## 15.6 Presentation truncation vs pipeline truncation

This distinction is critical:

```text
get process | take 50
```

changes the semantic result.

A renderer showing only the first 50 of 8,241 values changes only presentation.

The UI MUST distinguish these cases.

---

# 16. HistoryEntry Consolidation

## 16.1 v0.2 remains canonical

v0.2 already defines semantic command history as `HistoryEntry` with command text, context, exit status, duration, `ResultRef` and mutation summaries.

v0.7 MUST implement and present that contract rather than introduce a second `SessionEntry` abstraction.

## 16.2 Additive fields only where justified

If richer interaction requires metadata not expressible by the current versioned `HistoryEntry`, the existing format MAY gain optional fields such as:

```text
result_summary: ResultSummary?
diagnostics: List<DiagnosticRef>?
execution_id: ExecutionId?
```

Such fields MUST be additive/versioned and MUST NOT duplicate information already reachable through another stable reference without a demonstrated performance or durability reason.

## 16.3 Result summary

A `ResultSummary` is optional presentation-oriented metadata. It MAY contain result type, item count where known, byte count where meaningful, success/failure summary and presentation truncation state.

It MUST NOT pretend to be the result and MUST NOT include renderer escape sequences.

## 16.4 Conventional history behavior

Previous/next recall, reverse search, persistent command history and sensitive-command policy remain available according to existing shell behavior.

Rich presentation may expose more context but MUST NOT make conventional history navigation dependent on a TUI.

## 16.5 Persistence discipline

v0.7 MUST NOT silently persist arbitrary full structured results merely because `HistoryEntry` can reference them.

Full result retention remains bounded and policy-controlled. History metadata and result payload retention are separate concerns.

## 16.6 Re-execution

A rendered history entry MAY offer a discoverable route to re-submit its command text, but re-execution means executing again under explicitly understood context semantics. It MUST NOT silently replay cached mutations or treat a stale result as current state.

# 17. ResultRef Integration and Reuse

## 17.1 Reuse the baseline concept

`ResultRef` from v0.2 is the canonical reference to a retained prior structured result.

v0.7 improves how `ResultRef` is shown, inspected and reused. It MUST NOT introduce `ResultReference`, `SessionResult`, or another synonym.

## 17.2 Required behavior

A retained `ResultRef` MUST allow Ono to determine at least:

- which prior result it refers to;
- the result type or known schema identity;
- whether the value is still materialized/available;
- whether the reference is expired or unavailable;
- applicable secrecy/retention restrictions.

Exact storage layout is implementation-defined.

## 17.3 Expiration

Expired references MUST fail explicitly. They MUST NOT silently rerun the producing command.

## 17.4 Sensitive and large values

Sensitive values MAY be excluded from reusable result retention. Large values and streams MUST obey existing bounded-retention/replay semantics.

v0.7 MUST NOT solve stream replay by inventing a second materialization model.

## 17.5 Pipeline reuse

Where result-reference syntax is available, the referred typed value SHOULD re-enter an ordinary Ono pipeline without parsing terminal text.

Conceptually:

```text
@-1 | where state == failed
```

Exact shorthand remains governed by the language decision already left open in v0.2.

# 18. Foreground Progress, Jobs and Existing Live Streams

## 18.1 No new live semantics

v0.2 already defines finite/unbounded `Stream<T>`, `watch`, backpressure, cancellation, provider subscription/polling behavior and live TTY updates. v0.4/v0.5 add live topology and temporal observation semantics.

v0.7 MUST NOT redefine any of those concepts and MUST NOT describe live data as absent from Ono.

## 18.2 What v0.7 adds

v0.7 only standardizes how the ordinary prompt/TTY presentation coexists with:

- foreground execution progress already exposed by an operation;
- existing shell jobs;
- inherited `watch`/live views when they are used outside a future Deck workspace.

## 18.3 Progress

If an operation exposes progress, the TTY renderer MAY update a bounded transient progress line/region. Progress MUST remain subordinate to the operation's semantic result and cancellation contract.

The renderer MUST NOT synthesize a fake percentage when no total/progress fact exists.

## 18.4 Existing watch behavior

Existing `watch` or live-view commands remain valid. v0.7's terminal hardening MUST not regress them.

Where the baseline requires in-place TTY updates, those updates SHOULD use the host-owned view/render path and terminal capability service rather than bespoke ANSI code in providers.

## 18.5 Non-TTY behavior

Cursor-rewriting progress/live presentation MUST NOT leak into redirected files. Existing non-interactive stream semantics remain authoritative.

## 18.6 Background jobs

The prompt MAY show a compact active/failed job summary derived from existing job-control state. Detailed job lifecycle and signal behavior remain governed by the shell core.

# 19. External Commands

## 19.1 Default rule

Foreign commands remain foreign.

Their stdout/stderr bytes pass through according to existing Ono external-command semantics.

## 19.2 No forced wrapping

Rich TTY presentation MUST NOT wrap every external command inside a rich result container merely for visual consistency.

## 19.3 Adapted external commands

Where v0.3 provides a trustworthy typed adapter, its typed results may use normal native presentation.

## 19.4 Interactive external programs

Interactive programs such as editors, pagers and TUIs MUST retain terminal ownership according to existing job/TTY control rules.

Rich TTY transient surfaces MUST be suspended before terminal control is handed to such a program and restored safely afterwards.

## 19.5 Alternate screen children

If a child application uses the alternate screen, Ono MUST not assume its contents are available for structured history.

---

# 20. Terminal Capability Model

## 20.1 Normalized capability set

Ono SHOULD normalize terminal capabilities into a small internal model rather than scatter environment checks through the codebase.

A reference set includes:

```text
color_depth
unicode_level
cursor_addressing
clear_region
bracketed_paste
focus_events
hyperlinks
width
height
```

Not all capabilities are required for v0.7 use.

## 20.2 Capability truthfulness

If a capability is not known, Ono SHOULD choose the safer fallback.

## 20.3 Unicode width

Rendering MUST use a consistent implementation for grapheme and display width.

Tests MUST cover:

- ASCII;
- accented Latin text;
- CJK wide characters;
- combining marks;
- emoji where supported;
- ambiguous-width cases.

## 20.4 Hyperlinks

OSC 8 hyperlinks MAY be supported when detected and enabled.

No essential navigation MAY depend on them.

## 20.5 Bracketed paste

Rich TTY presentation SHOULD use bracketed paste when available to distinguish paste from typed input and avoid accidental immediate execution patterns.

Pasted multiline text MUST remain subject to normal parsing and commit rules.

---

# 21. Keyboard Interaction and Editor Compatibility

## 21.1 Do not re-specify the editor

v0.2 already requires a production-quality interactive shell/editor foundation. v0.7 MUST NOT create a second keyboard model simply because prompt-adjacent guidance exists.

Existing behavior for cursor movement, history navigation, completion, Ctrl-C, Ctrl-D, multiline editing and job control remains authoritative.

## 21.2 Guidance-specific requirements

v0.7 adds only the interaction necessary for transient guidance:

- candidate navigation MUST not prevent ordinary shell syntax entry;
- accepting a candidate MUST be explicit;
- dismissing guidance MUST leave the buffer unchanged;
- Escape SHOULD dismiss transient guidance before changing persistent shell state;
- Ctrl-C during line editing MUST retain its existing cancel-input semantics;
- highlighted guidance MUST never execute merely because an interrupt/dismiss key is pressed.

## 21.3 Keymap integration

Guidance bindings MUST integrate with the existing configurable keymap where one exists. v0.7 MUST NOT introduce a separate hard-coded TUI keymap for the ordinary prompt.

# 22. Discoverability Without a Parallel Menu System

## 22.1 Principle

v0.2 already treats completion/help as system exploration. v0.7 improves its presentation; it does not introduce a second discovery hierarchy.

## 22.2 Verb-target and type-aware discovery

While editing, the rich TTY MAY show valid continuations and type-compatible transforms derived from existing registries.

Example:

```text
get process | <TAB>
```

may visually prioritize commands/transforms valid for `Stream<Process>`.

## 22.3 Command-derived suggestions

The renderer MAY show a compact list of **commands** known to accept the current value/type, for example:

```text
for Process:
  inspect
  stop process
  trace
```

This is a discoverability projection only. It MUST NOT establish a new object-action registry if the same relationship can be derived from existing command metadata.

## 22.4 Equivalent command principle

Any later clickable/selectable control that causes an Ono operation SHOULD be explainable as an equivalent textual command or explicit existing semantic operation.

v0.7 records this as a design constraint for future Deck work; it does not need a new action-execution API.

# 23. Safety and v0.6 Integration

## 23.1 Safety information is semantic

Risk, protection and recovery status come from v0.6 data models.

v0.7 only chooses how to present them.

## 23.2 Mutating commands

Guidance SHOULD distinguish mutating from non-mutating commands where command metadata knows the distinction.

## 23.3 Dangerous contexts

Potentially destructive operations SHOULD receive visible text-level warning roles.

Example:

```text
MUTATION  remove file
RISK      HIGH
PROTECT   available through ZFS snapshot
```

Only information established by v0.6 may appear.

## 23.4 No safety theater

The rich TTY presentation MUST NOT add generic dramatic confirmations to every mutating command.

Safety behavior remains owned by semantic policy.

The UI exists to improve visibility, not to create ritual.

## 23.5 Recovery visibility

When a v0.6 operation has a validated recovery asset, Rich TTY rendering SHOULD make that fact compactly visible.

When only partial protection exists, the display MUST say partial.

---

# 24. v0.4 Spatial Integration

## 24.1 Contextual place

Current place may appear in the prompt or context line.

## 24.2 Relationship results

Relationship graphs MAY be rendered as trees or indented edge lists only when such a projection is semantically valid.

A general graph visualization is outside v0.7.

## 24.3 Navigation trail

A compact trail MAY be surfaced near the prompt if it remains bounded.

The terminal prompt MUST not become a breadcrumb wall.

## 24.4 Local vs remote

Spatial identity MUST preserve host/link scope in presentation when ambiguity exists.

---

# 25. v0.5 Temporal and Causal Integration

## 25.1 Time is visible

Historical context markers are mandatory as defined above.

## 25.2 Timeline presentation

Timeline values MAY use a specialized line-oriented or tabular existing view-tree representation.

## 25.3 Causal claims

Causal confidence and evidence MUST remain visible through textual labels or inspection.

Styling MUST NOT make a weak correlation look like a proven causal edge.

---

# 26. Diagnostics and Errors

## 26.1 Existing diagnostics remain semantic

Parser, type, provider and execution errors are already structured shell semantics. v0.7 standardizes their human presentation; it MUST NOT define a second error taxonomy.

## 26.2 Required presentation information

Where the underlying diagnostic provides them, rich and plain renderers SHOULD preserve:

- severity;
- stable code/identity;
- message;
- source span;
- causes;
- actionable hints;
- related object/value references.

Missing fields MUST remain missing rather than be synthesized for visual consistency.

## 26.3 Error codes and source spans

Stable diagnostic codes SHOULD remain visible or readily inspectable. Source-span highlighting MUST have a plain textual/caret fallback and MUST use correct display-width calculation.

## 26.4 Hints

Hints MUST be derived from known alternatives, parser/type information or established provider/safety metadata. Presentation MUST NOT fabricate speculative fixes.

## 26.5 Multiple diagnostics

Multiple diagnostics SHOULD render in a stable order that preserves the underlying diagnostic sequence/priority rules.

# 27. Machine Serialization Boundary

## 27.1 Human presentation is not serialization

Machine serializers operate on `Value` / typed semantic data, not on rich TTY tables or view-tree decoration.

## 27.2 JSON/YAML-style serializers

Where machine serialization exists, it MUST preserve the documented encoding, types and null semantics established by the relevant serializer contract.

## 27.3 Explicitness

Non-human output paths MUST omit prompt text, transient guidance, cursor-rewritten progress and decorative status material unless the user explicitly requests a human renderer.

## 27.4 Future front ends

The existing constrained view tree and v0.7 descriptor resolution are not automatically a remote GUI wire protocol.

v0.7 MUST NOT freeze internal layout structures as a network API merely because v0.8 or another front end could consume similar concepts.

# 28. Configuration

## 28.1 Extend existing render/theme configuration

v0.2 already defines `prompt`, `theme`, `render`, `completion`, `history`, `keymap` and related configuration domains. v0.7 MUST extend those domains rather than introduce a parallel `console.*` configuration namespace solely for rich TTY behavior.

## 28.2 Required controls

The existing configuration model SHOULD be able to express equivalents of:

```text
render.tty = auto | rich | plain
render.color = auto | always | never
render.unicode = auto | on | off
render.density = compact | normal
render.max_preview_rows = <integer>
completion.guidance = on | off
render.progress = auto | on | off
```

Exact syntax MUST follow Ono's established configuration format.

## 28.3 Safe startup

Presentation settings MUST remain declarative and MUST NOT create a new route for arbitrary command execution during startup.

## 28.4 Precedence

CLI overrides, environment/debug overrides and configuration precedence MUST be deterministic and inspectable.

# 29. Accessibility

## 29.1 No-color operation

All Rich TTY semantics MUST remain understandable with `--no-color`.

## 29.2 Contrast

Built-in rich TTY styling SHOULD avoid low-contrast combinations and excessive use of dim text for important information.

## 29.3 Motion

v0.7 SHOULD avoid non-essential animation.

Spinners MAY be used for progress but MUST have a reduced-motion or static alternative.

## 29.4 Symbols

Symbols such as checkmarks MAY supplement words.

They MUST NOT be the sole carrier of success/failure meaning.

## 29.5 Screen readers and linearity

Plain TTY presentation MUST remain a high-quality linear representation suitable for assistive terminal workflows.

Rich TTY presentation SHOULD avoid excessive cursor rewriting that makes linear terminal consumption unusable.

---

# 30. Performance and Latency Budgets

## 30.1 Interactive latency matters

Rich TTY features that add noticeable delay to every keystroke will make Ono worse than a simpler shell.

## 30.2 Keystroke path

Local syntax-aware editing and cached completion SHOULD target effectively immediate response.

A reference engineering budget is:

```text
median local edit/guidance update    < 16 ms
95th percentile local update         < 50 ms
```

These are engineering targets, not protocol guarantees.

## 30.3 Remote completion

Remote completion must be asynchronous or bounded so a slow provider does not freeze line editing.

v0.7 MUST NOT require a general reactive UI framework to accomplish this.

## 30.4 Rendering large results

The Rich TTY Renderer SHOULD stream or incrementally format large result sets where compatible with the semantic execution model, while obeying display preview limits.

It MUST avoid building unnecessarily huge terminal strings in memory solely for alignment.

## 30.5 Startup

Rich TTY initialization SHOULD not materially regress shell startup time.

Capability detection must be cheap and bounded.

---

# 31. Memory and Resource Management

## 31.1 Result retention

In-memory Result References require explicit retention limits.

Limits SHOULD consider:

- object count;
- approximate memory size;
- result sensitivity;
- age;
- active references.

## 31.2 Eviction

Eviction MUST produce an expired reference state, never stale unrelated data under the same identifier.

## 31.3 History metadata

Structured Session Entry metadata SHOULD remain small even when the underlying command result is enormous.

## 31.4 Terminal buffers

Ono MUST not assume terminal scrollback size or persistence.

Structured history and terminal history are complementary.

---

# 32. Security

## 32.1 Escape injection

Untrusted values rendered in a terminal MUST be sanitized so embedded control sequences cannot arbitrarily manipulate the terminal.

This applies especially to:

- filenames;
- process names;
- container labels;
- remote metadata;
- external command-derived adapted fields.

## 32.2 OSC and hyperlinks

Only renderer-generated OSC sequences may be emitted by rich rendering policy.

Value text MUST not be passed through as raw terminal control bytes.

## 32.3 Clipboard control sequences

v0.7 MUST NOT emit clipboard-setting control sequences from ordinary result rendering.

## 32.4 Secret leakage

Rich summaries MUST NOT accidentally surface fields that plain views correctly redact.

Security filtering occurs before or during view-description construction according to the existing secrecy contract.

## 32.5 History secrets

Structured history MUST apply the same or stronger secret-handling rules as legacy command history.

---

# 33. Testability

## 33.1 View/render tests

View-description construction SHOULD be testable independently from terminal escape generation.

## 33.2 Golden tests

Stable golden tests SHOULD cover:

- plain tables;
- rich TTY tables with semantic style roles normalized;
- narrow terminal fallback;
- no-color mode;
- Unicode width;
- null/unknown rendering;
- v0.6 risk/protection summaries;
- historical-context prompt markers.

## 33.3 Escape-sequence tests

ANSI output tests MUST verify cursor safety and cleanup, not merely string snapshots.

## 33.4 Pseudo-terminal tests

Integration tests SHOULD run Ono under a PTY and verify:

- editing;
- completion;
- resize;
- Ctrl-C;
- external interactive child handoff;
- restoration after child exit.

## 33.5 Redirection tests

CI MUST assert that redirected output does not contain unexpected ANSI controls.

## 33.6 Property tests

Property/fuzz tests SHOULD cover render width calculations and hostile control characters in values.

---

# 34. Observability of Presentation Itself

## 34.1 Debug tracing

Developers SHOULD be able to inspect rich TTY presentation decisions without attaching a debugger.

## 34.2 Useful events

Internal tracing MAY include:

```text
presentation.profile_selected
presentation.capabilities_detected
presentation.descriptor_resolved
presentation.view_selected
presentation.render_fallback
presentation.guidance_request
presentation.completion_request
presentation.resize
history.result_ref_evicted
```

Event names are non-normative but SHOULD reflect presentation rather than a fictitious semantic Console mode.

## 34.3 Privacy

Tracing MUST NOT record full sensitive command results by default.

# 35. KUANG/11 Boundary

## 35.1 Preserve the v0.2 lens contract

v0.2 already allows KUANG/11 view plugins to submit a constrained view tree or use a stable UI protocol; the host renderer owns terminal escapes, sizing, focus, accessibility and recovery.

That capability remains valid. v0.7 MUST NOT accidentally revoke it by claiming that plugins may only contribute field labels.

## 35.2 What v0.7 changes

v0.7 MAY require inherited plugin views and schema render hints to flow through centralized terminal capability, sanitization and semantic styling policy when rendered by the host.

It MAY clarify which existing schema/default-view metadata is usable by the ordinary rich TTY renderer.

## 35.3 What v0.7 does not add

v0.7 MUST NOT add a second public plugin UI API for:

- raw terminal ownership;
- arbitrary ANSI emission;
- custom terminal event loops;
- unrestricted executable render callbacks;
- a new widget tree that duplicates the v0.2 view protocol.

Existing KUANG/11 view lifecycle and constrained components remain governed by the baseline.

## 35.4 Forward compatibility

Future Deck integration SHOULD consume the same constrained host-owned view semantics where practical. If v0.8 needs workspace-specific lifecycle metadata, it MUST add only what persistent workspace composition actually requires.

# 36. Non-Goals

## 36.1 No full-screen Deck workspace

v0.7 does not introduce permanent full-screen shell ownership, persistent panes, workspace focus management or alternate-screen lifecycle for the shell itself.

Specialized inherited TUI views and child TUIs remain valid; this non-goal applies to the ordinary shell workspace.

## 36.2 No second presentation ontology

v0.7 MUST NOT create a new render tree that competes with the constrained host-owned view tree already established by v0.2.

## 36.3 No new live-data type system

v0.7 does not introduce `Live<T>`, `Observable<T>` or replacement observation/backpressure semantics.

Existing `Stream<T>`, `watch`, live topology, temporal observations, coverage and gaps remain valid and MUST continue to work.

## 36.4 No new dashboard framework

v0.7 does not add arbitrary persistent dashboard composition.

## 36.5 No new forms framework

Registry-derived guidance for missing/invalid arguments is allowed; a generic form-application model is not required.

## 36.6 No mouse dependency

All v0.7 behavior MUST remain keyboard-operable. Existing optional terminal mouse behavior, if any, is not expanded by this spec.

## 36.7 No second theme language

v0.7 reuses/hardens semantic theme/presentation tokens already anticipated by v0.2. It does not create another theme DSL.

## 36.8 No new arbitrary plugin UI ABI

The v0.2 constrained KUANG/11 view protocol remains authoritative. v0.7 adds no competing public widget ABI.

## 36.9 No terminal emulator

Ono does not render arbitrary child-process terminal applications itself.

## 36.10 No replacement for ordinary scrollback

The user's terminal remains responsible for normal committed shell scrollback in v0.7.

# 37. Future Release Boundaries

## 37.1 v0.8 - Workspace / Deck Mode

v0.8 may compose the same semantic values, existing view tree, `HistoryEntry`, `ResultRef`, context and terminal capability services into a persistent full-screen workspace.

It may add only the state needed for workspace composition: region ownership, focus, resize/layout lifecycle, alternate-screen entry/exit, child-TTY handoff and workspace navigation.

v0.7 MUST NOT predefine those pane/focus contracts.

## 37.2 v0.9 - Live View Integration

v0.9 SHOULD NOT invent a new live-data subsystem.

Its expected purpose is to integrate the already-existing v0.2 streams/`watch`/backpressure model and v0.5 observation/coverage/gap semantics with the v0.8 persistent workspace: stable live rows, follow/pause behavior, stale/gap presentation, cancellation, suspend/resume and bounded rendering.

## 37.3 v0.10 - Reassessment before new object interaction

Object selection, `ValueRef`, `ResultRef`, object context, pickers, `enter` and command-derived interactions already exist in earlier specifications to varying degrees.

Before v0.10 is specified, those contracts MUST be diffed against the intended Deck interaction. v0.10 is justified only for missing semantics, not for renaming existing selection/action concepts.

## 37.4 v0.11 - Theme refinement / bundled Neuromancer theme

Because v0.2 already anticipates themes and semantic presentation tokens, a later theme release SHOULD primarily stabilize/extensively test theme roles and ship bundled themes rather than invent a brand-new styling architecture.

## 37.5 v0.12 - Extension-surface reassessment

KUANG/11 already has a constrained view protocol. Any v0.12 work MUST begin by determining what persistent Deck integration genuinely lacks. It MUST NOT assume that a new widget API is necessary.

# 38. User Experience Examples

## 38.1 Native typed result

Input:

```text
get process | sort memory desc | take 3
```

Rich TTY output may be:

```text
PID      NAME             CPU     MEMORY
3647     claude-desktop   18.1%   3.21 GiB
2573964  rustc            82.0%   815.5 MiB
1719281  postgres          4.2%   651.3 MiB

3 Process values
```

The final line is optional presentation metadata. The semantic result is still the three `Process` values.

## 38.2 Narrow terminal

At 58 columns the same result may become:

```text
PID      NAME             MEMORY
3647     claude-desktop   3.21 GiB
2573964  rustc            815.5 MiB
1719281  postgres         651.3 MiB
```

CPU is removed because it is less important than identity and memory in the selected presentation descriptor.

## 38.3 Historical context

```text
prod-db-03:/srv @2026-08-31 12:17 [PAST] > get service nginx
```

The exact prompt layout remains governed by the inherited prompt/context contracts. The important v0.7 requirement is that the canonical v0.5 marker remains textual and persistent; when coverage is materially partial or uncertain, the corresponding `[PAST?]` marker MUST be preserved instead.

## 38.4 Completion

Editing:

```text
get process | where c<TAB>
```

Guidance may show:

```text
cpu       Quantity<Percent>
command   String
cwd       Path?
```

## 38.5 v0.6 plan result

```text
plan restart service nginx
```

may render:

```text
CHANGE PLAN  plan:7f31
state        SEALED
risk         MODERATE
protection   PARTIALLY_PROTECTED
impact       1 direct target, 4 known dependents
unknown      1 external boundary

No mutation has occurred.
```

Every label above must derive from the `ChangePlan` and related semantic models.

## 38.6 External command

```text
git log --oneline
```

remains the child command's textual output unless a trusted adapter is explicitly active.

## 38.7 Progress

```text
apply @plan
```

may transiently show:

```text
Preparing protection  2/3  00:01.8
```

and then commit the final stable result to scrollback.

---

# 39. CLI and Introspection Surface

## 39.1 Required controls

The product MUST provide a stable way to request conservative output, including equivalents of:

```text
ono --plain
ono --no-color
```

A `--rich` override MAY exist for diagnostics or explicit preference. A dedicated `--console` execution mode is not required and SHOULD NOT be introduced merely for v0.7.

## 39.2 Presentation introspection

Ono SHOULD expose presentation introspection through the existing command vocabulary, showing information such as:

- selected presentation profile;
- terminal size and relevant capabilities;
- color/Unicode policy;
- guidance policy;
- effective descriptor/view choice for a value when requested.

Exact verb-target spelling MUST follow the established command registry rather than add an isolated `console` namespace.

## 39.3 Explicit view selection

v0.2 already defines alternate views and rendering concepts. v0.7 SHOULD reuse the established `render`/view grammar if implemented.

An ADR is required before adding new language syntax solely to express presentation that existing `select`, `inspect`, serializer or view operations can already represent.

# 40. Internal Rust Architecture Guidance

## 40.1 Evolve existing boundaries

v0.2 already suggests `ono-render`, `ono-editor`, `ono-history` and related crate boundaries. v0.7 SHOULD evolve those existing modules rather than create new crates solely to mirror terminology in this document.

A likely responsibility split is:

```text
ono-value / schemas        semantic values and type metadata
ono-render                 existing view tree, descriptor resolution, renderers
ono-editor                 input editing and prompt-adjacent guidance integration
ono-history                HistoryEntry / ResultRef retention
ono-cli                    interactive orchestration and terminal handoff
```

Actual repository structure remains authoritative.

## 40.2 Domain isolation

Provider crates MUST NOT depend on terminal ANSI backends. Provider/schema render hints are declarative metadata.

## 40.3 Terminal library

Choice of crossterm, termion, reedline, rustyline or equivalents is non-normative. Third-party APIs MUST NOT define Ono's public semantic contract.

## 40.4 Interactive event handling

The ordinary interactive loop must already handle key input, resize, completion, foreground execution, signals and child-process handoff. v0.7 MAY extend that loop with transient guidance/redraw events.

It SHOULD NOT create a second event loop called "Console" if the existing editor/session loop can own these responsibilities coherently.

## 40.5 Cancellation

Asynchronous completion or provider-backed suggestions MUST be cancellable or discardable when the input buffer changes, so stale guidance is not shown as applicable to newer text.

# 41. Interaction State Machines

## 41.1 Reuse the shell session lifecycle

v0.7 MUST not define a parallel session lifecycle merely because rendering is richer.

Conceptually, the existing interactive loop still moves through familiar states:

```text
EDITING -> EXECUTING -> EDITING
              |
              +-> CHILD_TTY_OWNERSHIP -> return
              +-> CANCELLED -> return
```

Rich guidance is subordinate transient state while `EDITING`; rich result formatting is a projection step around ordinary result delivery.

## 41.2 Guidance state

```text
HIDDEN
  |
  +-- valid parser/completion information --> VISIBLE
  |
  +<-- dismiss / commit / stale ------------+
```

Guidance state MUST NOT carry command semantics unavailable elsewhere.

## 41.3 Child terminal ownership

Before an interactive child owns the terminal, Ono MUST clear/suspend transient prompt decorations and restore the terminal state expected by the child.

On return, Ono MUST restore its editor/presentation terminal state safely. This requirement is compatible with the stronger full-screen handoff lifecycle to be defined by v0.8.

# 42. Persistence

## 42.1 What persists

At minimum, command history may persist according to existing policy.

Structured metadata persistence MAY include:

- timestamp;
- context identity;
- exit status;
- duration;
- result type summary.

## 42.2 What does not persist by default

The following SHOULD NOT be persisted by default solely for v0.7:

- full native result payloads;
- full adapted external outputs;
- transient completion candidates;
- terminal escape sequences;
- prompt frames.

## 42.3 Format versioning

Any additive extension to the existing persistent `HistoryEntry` format required by v0.7 MUST be versioned and forward-migratable. v0.7 MUST NOT introduce a second persistent history format solely for presentation metadata.

A corrupt optional history entry MUST NOT prevent Ono from starting.

---

# 43. Compatibility

## 43.1 Script compatibility

Scripts MUST not observe rich-vs-plain TTY presentation as a change in command semantics. If presentation environment state is explicitly queried, that query is metadata only.

## 43.2 Exit codes

Presentation success/failure MUST NOT overwrite the semantic exit code of command execution except when the user explicitly invokes a presentation command whose own failure is the command result.

## 43.3 Signals

Signal semantics remain those of the shell/job-control system.

## 43.4 Environment

Child processes SHOULD receive conventional terminal-related environment variables consistent with actual terminal state.

Ono MUST NOT claim a terminal capability to children that it has not actually made available.

## 43.5 Copy and paste

Committed output SHOULD remain suitable for terminal selection/copying.

Decorative borders SHOULD be used sparingly in ordinary rich TTY output because they increase copy noise.

---

# 44. UX Guardrails

## 44.1 No information carnival

The rich TTY presentation MUST NOT attempt to show every available contextual fact at once.

## 44.2 Quiet by default

Routine successful commands SHOULD remain visually quiet.

Strong emphasis is reserved for:

- risk;
- error;
- unknown state;
- historical/proposed context;
- broken/degraded links;
- user-requested focus.

## 44.3 No modal dependence

v0.7 SHOULD avoid modal popups or interaction states that block normal typing.

## 44.4 No fake sci-fi language

The core terminal terminology MUST remain technically honest.

Cyberpunk/Neuromancer vocabulary may appear later as optional theming or branding, not as replacement names for fundamental system concepts in v0.7.

## 44.5 Power users are not punished

A user who already knows the command MUST be able to type and execute it with no additional ceremony compared with plain TTY presentation.

---

# 45. Failure Modes and Required Behavior

## 45.1 Terminal capability failure

Fall back to a simpler profile.

## 45.2 Renderer bug

Attempt plain rendering; emit a presentation diagnostic; preserve command result/exit semantics.

## 45.3 Completion provider timeout

Keep editing responsive; discard or mark delayed candidates; do not freeze the shell.

## 45.4 Resize storm

Coalesce redundant redraws where practical.

## 45.5 Unicode width mismatch

Prefer ASCII-safe fallback when capability confidence is low.

## 45.6 History persistence failure

Warn non-fatally unless user policy requires strict history durability.

## 45.7 ResultRef eviction

Return an explicit existing/result-ref-specific expiry error; expiration MUST NOT trigger implicit re-execution.

## 45.8 Child program corrupts terminal state

Ono SHOULD restore known terminal modes on return and provide a documented recovery command if restoration cannot be automatic.

---

# 46. Acceptance Criteria

v0.7 is complete only when all acceptance criteria below are met.

## 46.1 Consolidation acceptance

- No new synonym has replaced `Value`, `Stream<T>`, `HistoryEntry`, `ResultRef` or `ValueRef`.
- Existing schema `default_view`/renderer hints/presentation profiles feed one deterministic resolution path.
- Ordinary human output uses the existing host-owned view/render concepts rather than a second public tree.
- Native command/provider semantics remain renderer-independent.
- Machine serialization remains value-based and separate from human presentation.

## 46.2 Rich TTY acceptance

- A capable TTY receives materially better adaptive human output without entering a different semantic mode.
- A low-capability TTY degrades cleanly to plain output.
- Ordinary committed output preserves terminal scrollback.
- Prompt-adjacent guidance clears safely on commit/cancel.
- Resize does not corrupt the prompt or committed output.
- Existing v0.2 interactive selection behavior remains semantically unchanged.

## 46.3 Compatibility acceptance

- redirected `ono -c` output is ANSI-clean by default;
- external interactive editors/TUIs retain expected terminal ownership;
- tmux/screen/SSH basic scenarios pass integration tests;
- `TERM=dumb` produces usable output;
- Ctrl-C/Ctrl-D behavior is not changed by presentation;
- existing `watch`/live-view behavior is not regressed.

## 46.4 Semantic-context acceptance

- v0.4 place/remote boundaries are projected without inventing a second context record;
- v0.5 historical/gap/uncertainty state is textually distinguishable when relevant;
- v0.6 proposed/protection/risk states are displayed only from semantic facts;
- color is not required to understand important state.

## 46.5 History/result acceptance

- implementation uses/evolves `HistoryEntry`, not `SessionEntry`;
- implementation uses `ResultRef`, not a new reference type;
- full result persistence is not silently enabled;
- expired result refs fail explicitly;
- conventional history navigation remains available.

## 46.6 Security/accessibility acceptance

- terminal control injection from value text is prevented;
- no-color and ASCII-safe paths are usable;
- important meaning is not encoded solely by symbol/color;
- reduced-motion/static progress is possible.

## 46.7 Performance acceptance

- editing and cached local guidance remain responsive;
- slow provider-backed completion does not freeze input;
- large human results obey preview/width policies without excessive allocation;
- rich TTY initialization does not materially regress startup.

# 47. Required Test Matrix

The implementation MUST include automated coverage for at least the following matrix.

| Area | Cases |
|---|---|
| Human sink | rich TTY, plain TTY, `TERM=dumb` |
| Non-human sink | redirect, script/`-c`, typed pipeline, explicit serializer |
| Width | 40, 59, 60, 79, 80, 120, 200 columns |
| Height | 10, 24, 50+ rows |
| Color | none, 16, 256, truecolor where supported |
| Unicode | ASCII, Latin accents, CJK, combining marks, emoji |
| Context | local, remote, spatial, historical, proposed/change |
| Results | scalar, record, list, empty, huge list, null/unknown-heavy |
| History | `HistoryEntry`, `ResultRef`, expiry, sensitive exclusion |
| Live regression | `watch`, cancellation, in-place TTY update, redirect behavior |
| External programs | stdout/stderr, pager/editor/TUI, child terminal restoration |
| Interaction | completion, inherited selection, cancel, resize, paste, history search |
| Safety | risk, partial protection, unknown effect, recovery asset |
| Security | embedded CSI/OSC/control chars, hostile labels/filenames |
| Plugin regression | existing constrained KUANG/11 lens still host-rendered safely |

Golden tests MUST normalize environment-dependent data and MUST distinguish presentation snapshots from semantic data contracts.

# 48. Implementation Sequence

## 48.1 Phase A - Inheritance and duplication audit

Before adding visible behavior:

1. inventory current v0.2 rendering, view tree, history, result-ref, completion and theme-token implementations;
2. inventory v0.4-v0.6 context/safety presentation hooks;
3. identify direct printing/ANSI code inside native providers;
4. identify duplicate or near-duplicate internal types already introduced during implementation;
5. document a canonical-type crosswalk;
6. create ADRs only for true contract ambiguity.

Exit criterion: the team can name one canonical owner for every concept v0.7 touches.

## 48.2 Phase B - Presentation metadata consolidation

Implement or harden:

- resolution of schema `default_view`, existing renderer hints and presentation profiles;
- `PresentationDescriptor` as resolved/internal compilation output;
- validation that referenced fields/types exist;
- generic fallback from `Value` shape;
- semantic style roles using existing theme tokens.

Exit criterion: providers no longer need ad hoc table-layout policy for native values.

## 48.3 Phase C - Existing view tree and plain renderer hardening

Ensure ordinary scalar/record/list/table/tree/diagnostic projections can use the host-owned view/render model already implied by v0.2.

Harden plain rendering and snapshot tests first.

Exit criterion: the same semantic value can be projected cleanly without ANSI or rich-terminal capabilities.

## 48.4 Phase D - Terminal capability and sanitization layer

Centralize effective width/height, color, Unicode, cursor-addressing, bracketed-paste and sanitization policy.

Exit criterion: terminal capability decisions are inspectable and not scattered through providers/editor code.

## 48.5 Phase E - Rich TTY projection

Implement adaptive styling/layout on top of the same value/view path, preserving scrollback and inherited selection semantics.

Exit criterion: capable terminals improve presentation without a new shell mode.

## 48.6 Phase F - Completion/guidance projection

Use existing grammar, command/schema registries and completion metadata to add bounded prompt-adjacent guidance. Do not duplicate the command taxonomy.

Exit criterion: guidance is useful and removable without changing completion semantics.

## 48.7 Phase G - HistoryEntry / ResultRef integration

Implement the baseline semantic-history and retained-result contracts completely; add only demonstrably necessary optional metadata.

Exit criterion: rich history/result display needs no parallel session model.

## 48.8 Phase H - Existing progress/live regression integration

Route existing progress and `watch`/live TTY presentation through centralized terminal/render policy where feasible without changing stream/observation semantics.

Exit criterion: v0.7 hardening does not break capabilities Ono already promised.

## 48.9 Phase I - Hardening

Run PTY matrices, hostile-terminal-value fuzzing, Unicode-width property tests, tmux/SSH/manual tests, performance profiling, accessibility/no-color review and KUANG/11 lens regression tests.

# 49. Definition of Done

v0.7 is not done when Ono merely looks better.

It is done when:

1. earlier presentation contracts have one coherent implementation path;
2. no parallel `SessionEntry`, `ResultReference`, semantic UI-mode enum or second view ontology was introduced;
3. schema defaults/render hints/profiles resolve deterministically;
4. rich TTY behavior is progressive enhancement of the normal shell;
5. plain, scripted, redirected and external-command behavior remain excellent;
6. v0.4-v0.6 context, uncertainty and safety are visible without being re-modeled;
7. inherited `HistoryEntry`, `ResultRef`, selection, live streams and KUANG/11 lenses remain valid;
8. terminal differences degrade gracefully and safely;
9. future Deck Mode can compose these existing contracts rather than forcing a semantic migration;
10. deleting every future Deck-related release would still leave v0.7 as a worthwhile simplification and quality improvement.

# 50. Explicit Anti-Requirements

The implementation MUST reject designs that require any of the following to complete v0.7:

- a semantic `Console Mode` branch in command execution;
- `SessionEntry` beside existing `HistoryEntry`;
- `ResultReference` beside existing `ResultRef`;
- a new semantic-value wrapper around existing `Value` solely for UI;
- a new public render/widget tree beside the v0.2 constrained view tree;
- duplicated schema/command registries for presentation;
- changing command output types depending on terminal capabilities;
- storing the only semantic copy of selection/context inside a visual component;
- converting arbitrary external output into fake native structure;
- permanent alternate-screen ownership for the ordinary shell;
- a general-purpose pane/window manager;
- a new `Live<T>`/`Observable<T>` type system or replacement backpressure model;
- redefinition of v0.5 observations/gaps/coverage;
- a second plugin widget ABI beside KUANG/11's inherited view protocol;
- a second theme DSL;
- a dashboard language;
- mouse dependence;
- safety state inferred from presentation heuristics;
- sacrificing redirected/script output quality for interactive cosmetics.

If an implementation proposal needs one of these, it belongs in another release, is already covered by an earlier contract, or requires an explicit scope-changing ADR.

# 51. Architecture Decision Checklist

Before merging a significant v0.7 subsystem, reviewers SHOULD ask:

1. Which earlier specification owns this concept today?
2. Am I extending that canonical concept or creating a synonym?
3. Does this code operate on `Value`/existing semantic references or on formatted strings?
4. Is `PresentationDescriptor` being compiled from authoritative metadata, or becoming a duplicate registry?
5. Can the same result be rendered plainly without changing command execution?
6. Is rich TTY merely presentation, or did a new semantic mode leak into dispatch/state?
7. Does this preserve `HistoryEntry`, `ResultRef`, selection and live-stream semantics from v0.2?
8. Does it preserve v0.4-v0.6 truth/uncertainty/safety semantics without copying them into UI types?
9. Does narrow/no-color/redirected behavior preserve identity and honesty?
10. Can malicious value text inject terminal controls?
11. Does provider/remote latency block line editing?
12. Does this expand KUANG/11 beyond its existing constrained view contract without a proven need?
13. Would this still be useful if v0.8-v0.12 were cancelled?
14. Are we solving a presentation problem, or accidentally designing another product?

Any answer indicating duplication or mode-dependent semantics requires design review before implementation.

# 52. Example End-to-End Session

The following illustrates the intended feeling. Visual details are non-normative; the semantic behavior is normative.

```text
prod-eu-1:/srv/app > get process | where cpu > 20

PID    NAME       CPU    MEMORY
1821   postgres   44%    3.2 GiB
9291   node       31%    812 MiB

2 Process values  result @17

prod-eu-1:/srv/app > @17 | sort memory desc

PID    NAME       CPU    MEMORY
1821   postgres   44%    3.2 GiB
9291   node       31%    812 MiB

prod-eu-1:/srv/app > plan restart service nginx

CHANGE PLAN  plan:91ab
state        SEALED
risk         MODERATE
protection   PARTIALLY_PROTECTED
unknown      1 external boundary

No mutation has occurred.  result @18

prod-eu-1:/srv/app > impact @18

DIRECT TARGET
  service nginx

KNOWN DEPENDENTS
  process nginx[4421]
  socket :80
  socket :443

UNKNOWN BOUNDARY
  external upstream dependency

prod-eu-1:/srv/app >
```

The numeric `@17` / `@18` notation in this example is illustrative shorthand for an existing `ResultRef`; v0.7 does not standardize a new result-reference syntax. Exact reference syntax remains governed by v0.2 and any later authoritative ADR/specification.

Important observations:

- the result references are semantic conveniences, not screenshots;
- risk and protection are v0.6 facts;
- the prompt remains the primary surface;
- outputs remain stable terminal scrollback;
- there are no persistent panels;
- nothing requires a mouse;
- nothing prevents `ono -c` from remaining plain and composable.

---

# 53. Rationale: Why This Is Worth a Release

v0.7 intentionally looks smaller after consolidation than the first draft. That is a success criterion, not a loss of ambition.

Ono already promised typed values, adaptive rendering, interactive selection, TUI lenses, semantic completion, history/result reuse, live streams, spatial context, temporal evidence and change/recovery semantics. Re-implementing those ideas under new v0.7 names would increase architecture without increasing capability.

The release is worthwhile because it removes accidental fragmentation at the human-output boundary:

```text
existing semantic system
        |
        v
existing render hints / profiles / view tree
        |
        v
v0.7 deterministic consolidation + terminal hardening
        |
        +-- plain TTY
        +-- rich TTY
        +-- future Deck composition
```

That gives Ono a stronger ordinary shell experience and a cleaner base for v0.8 while reducing, rather than increasing, the number of conceptual moving parts.

The key architectural gain is therefore not a new UI framework. It is the elimination of reasons for providers, commands, history, completion and future workspace code to invent their own presentation policies.

\newpage

# 54. Closing Principle

Ono-Sendai's identity is not that it looks like a cyberdeck.

Its identity is that it treats the machine as a structured system rather than a pile of formatted strings.

v0.7 makes the human interface finally reflect that fact while preserving the speed, composability and honesty of a shell.

The implementation should therefore be judged by a simple test:

> **Did v0.7 make Ono's existing truth easier to see with fewer concepts, or did it add another vocabulary for things Ono already knew?**

Only the first outcome is acceptable for v0.7.
