---
title: "ONO-SENDAI"
subtitle: "Specification v0.3 - External Command Adaptation Layer"
author: "Project Specification"
date: "2026-08-26"
geometry: "margin=18mm"
fontsize: 10pt
colorlinks: true
linkcolor: blue
urlcolor: blue
toc: true
toc-depth: 2
numbersections: false
---

# 0. Document Status and Relationship to v0.2

This document is the **standalone ONO-SENDAI v0.3 extension specification** for the External Command Adaptation Layer.

It is intentionally a separate document. It does **not** rewrite, regenerate, amend in place, or otherwise modify the immutable ONO-SENDAI v0.2 baseline specification.

The relationship is:

```text
ONO-SENDAI v0.2
    base product, language, object model, providers,
    Unix interoperability, remote links and KUANG/11

        +

ONO-SENDAI v0.3 Extension Specification
    external-command adaptation, compatibility packs,
    output-demand negotiation and adapter SDK/contracts

        =

candidate ONO-SENDAI v0.3 product contract
```

For the original autonomous-implementation experiment, the v0.2 specification remains the immutable initial input. This v0.3 document is a **new product input for a later revision** and must not be back-merged into the frozen v0.2 file.

## 0.1 Normative scope

The keywords **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, **MAY** and **RECOMMENDED** are normative within this document.

This specification defines:

- a new architectural layer for selected external Unix commands;
- demand-driven structured adaptation without replacing the underlying tool;
- canonical schema normalization between native Ono providers and adapted tools;
- safe fallback to classic text/byte Unix behavior;
- adapter discovery, negotiation, provenance and diagnostics;
- a KUANG/11 distribution and permission model for adapter packs;
- a curated Linux compatibility baseline and operator tool set;
- tool-specific design guidance for high-value utilities;
- SDK, manifest, versioning, testing and release requirements;
- the work-package model needed to derive implementation tasks automatically.

This specification does **not** require Ono to reimplement the utilities it adapts.

## 0.2 Base contracts inherited from v0.2

This document assumes the v0.2 concepts and contracts for:

- typed Ono values and canonical schemas;
- `Stream<T>` pipelines;
- external process execution, PTYs, signals and job control;
- command resolution and explicit raw/external escape hatches;
- native system providers;
- provenance and structured errors;
- remote links;
- KUANG/11 package lifecycle, capabilities, isolation and SDK conventions;
- spec-driven registries, tests and generated documentation.

Where this document refers to a type such as `Process`, `Socket`, `Interface`, `Service`, `Value`, `Stream<T>` or `Provenance`, the v0.2 base definition is inherited unless this document explicitly adds adapter-specific fields or constraints.

## 0.3 Product delta

v0.2 established two important truths simultaneously:

1. native Ono commands should produce typed values;
2. arbitrary Unix programs must remain first-class citizens and normally produce text or bytes.

v0.3 adds the missing middle path:

> **Selected external Unix programs can participate in Ono's typed object world without being replaced, emulated or heuristically scraped.**

The concise design principle introduced by this specification is:

> **Adapt before replacing. Normalize concepts, not commands. Fall back honestly.**

## 0.4 Required integration points

An implementation that adopts this v0.3 specification must extend the v0.2 architecture at the following boundaries:

```text
command resolution
    -> external executable resolved
    -> adapter registry consulted
    -> output demand negotiated
    -> raw or structured execution plan selected

process subsystem
    -> remains responsible for spawn/PTY/signals/job control
    -> executes adapter plans without surrendering process ownership

value/schema system
    -> receives canonical typed records from trusted adapter decoders
    -> preserves adapter provenance

KUANG/11
    -> distributes concrete adapter packages
    -> grants only declared capabilities
    -> isolates executable-specific knowledge from Ono core

remote links
    -> negotiate adapter availability/version remotely
    -> never assume local and remote compatibility are identical
```

## 0.5 Versioning intent

The document version is **v0.3** because it specifies a product increment after the frozen v0.2 baseline. It is not a patch to the original file. A future v0.4 or v1.0 may either remain a set of composable extension specifications or consolidate them into a new baseline only as a deliberate release-management decision.

---

# 1. External Command Adaptation Layer

The Unix ecosystem is too valuable to replace and too semantically rich to treat only as anonymous text forever. Ono-Sendai therefore defines a fourth architectural layer between native Ono commands and arbitrary raw executables: **External Command Adapters**.

An adapter teaches Ono how to invoke, recognize, decode and normalize selected invocations of an existing external tool while the real tool remains the implementation of record. The adapter does not fork the utility, does not emulate it, and does not claim that every invocation is structured. It provides a typed bridge where the external program exposes enough stable semantics to justify one.

The product goal is simple:

> **Keep your Unix commands. Ono makes the ones it understands participate in the object world.**

A user who already knows `ps`, `ss`, `ip`, `lsblk`, `systemctl`, `journalctl`, `find`, `lsof`, `git` or `curl` should not have to abandon that vocabulary merely to gain structured pipelines. When an adapter can prove that an invocation is safe to structure, the output SHOULD become the same canonical Ono object types that native providers use. When Ono cannot prove that, the command remains ordinary Unix text or bytes.

This is not automatic schema inference. It is explicit, versioned, testable knowledge.

## 1.1 The four-layer command model

Ono-Sendai conceptually exposes four command layers:

```text
                         ONO-SENDAI

+---------------------------------------------------------------+
| Layer 1 - Shell semantics                                     |
| cd, cwd, env, jobs, fg/bg, history, source, exec, functions   |
+---------------------------------------------------------------+
| Layer 2 - Ono-native systems interface                        |
| get process, get socket, get service, trace, watch, ...       |
| output: native typed values                                   |
+---------------------------------------------------------------+
| Layer 3 - Adapted Unix ecosystem                              |
| ps, ss, ip, lsblk, systemctl, journalctl, curl, git, ...      |
| implementation: external executable                           |
| output: typed values only for declared supported invocations  |
+---------------------------------------------------------------+
| Layer 4 - Arbitrary Unix ecosystem                            |
| anything executable on PATH                                   |
| output: TextStream or ByteStream                              |
+---------------------------------------------------------------+
```

Layers 2 and 3 SHOULD converge on canonical schemas whenever they describe the same real-world concept.

Examples:

```text
get process                         -> Stream<Process>
ps -eo pid,ppid,user,comm           -> Stream<Process>

get socket                          -> Stream<Socket>
ss -tunap                           -> Stream<Socket>

get interface                       -> Stream<Interface>
ip address                          -> Stream<InterfaceAddress>
```

The implementation path differs. The object contract should not.

## 1.2 Design invariants

The adapter layer MUST obey the following invariants:

1. **The external executable remains real.** An adapter wraps or rewrites an invocation; it does not impersonate the tool with an Ono reimplementation.
2. **No guessed structure.** Ono MUST NOT infer schemas from arbitrary stdout merely because it resembles a table.
3. **Unsupported means raw.** An invocation outside an adapter's declared support surface falls back to normal external execution unless the user explicitly requires structured output.
4. **Canonical schemas win.** Adapters SHOULD emit existing Ono schemas rather than tool-specific duplicates when semantics match.
5. **Raw Unix remains reachable.** Users MUST be able to bypass an adapter and execute the external program with unmodified byte semantics.
6. **Adaptation is inspectable.** Users MUST be able to discover which adapter matched, why it matched, how the invocation was rewritten, which parser was used and which schema was emitted.
7. **Machine formats first.** Adapters SHOULD prefer JSON, Protobuf, null-delimited records, stable field selectors or other machine-oriented outputs over human-output parsing.
8. **Version claims are explicit.** An adapter MUST declare the executable/version surface for which its structured interpretation is valid.
9. **TTY behavior is sacred.** Interactive programs and invocations that own the terminal MUST NOT be trapped inside an Ono renderer merely because an adapter exists.
10. **Unix composition is not silently broken.** Redirection and pipelines into arbitrary external tools MUST preserve expected byte semantics unless the user or downstream Ono operation requests structured values.
11. **Provenance survives normalization.** A `Socket` produced through `ss` must reveal that `ss` produced it.
12. **Failure is honest.** If structured decoding fails, Ono MUST report the failure and either fall back safely or stop the structured pipeline; it MUST NOT fabricate partial records without marking them partial.

## 1.3 Adapter versus replacement

An adapter is intentionally narrower than a provider.

A native provider might read netlink directly and expose `Socket` objects through `get socket`. An `ss` adapter instead executes `/usr/bin/ss` with a supported argument strategy and decodes the result.

Both are useful:

```text
native provider
  Ono -> netlink/procfs -> Socket

external adapter
  Ono -> /usr/bin/ss -> declared output protocol -> Socket
```

The native provider can offer deeper control, stable object identity and lower overhead. The adapter preserves familiar Unix vocabulary and can gain capabilities faster by leveraging a mature tool.

Adapters SHOULD therefore be treated as compatibility and ecosystem acceleration, not as excuses to avoid high-value native providers.

## 1.4 Demand-driven adaptation

The most important compatibility rule is that adaptation SHOULD be **demand-driven** rather than blindly applied to every external invocation.

The execution planner knows what kind of downstream consumer is attached to stdout.

### Structured consumer

```text
ss -tunap | where state == established
```

`where` requires structured values. If an `ss` adapter supports the invocation, Ono MAY rewrite `ss` into a machine-oriented form, decode it and expose `Stream<Socket>`.

### Interactive terminal renderer

```text
ss -tunap
```

In an interactive Ono session, a high-confidence adapter MAY produce structured values and let the Ono renderer display them. This makes the command feel native while retaining `raw ss -tunap` as an exact-output escape hatch.

### External byte consumer

```text
ss -tunap | grep ':443'
```

The downstream process expects bytes. Ono SHOULD preserve classic external-pipeline semantics and execute the normal `ss` invocation unless the user explicitly requests adaptation plus serialization.

### File redirection

```text
ss -tunap > sockets.txt
```

Redirection SHOULD preserve raw external output by default. A command that wants structured serialization should say so:

```text
ss -tunap | to json > sockets.json
```

This rule avoids one of the worst possible failure modes: a shell that makes commands prettier interactively but silently changes shell scripts, redirections and existing pipelines.

## 1.5 Output demand model

The planner SHOULD model stdout demand explicitly:

```text
enum OutputDemand {
    RawBytes,
    Text,
    Structured(SchemaConstraint?),
    Interactive,
    Discard,
}
```

An external adapter receives the invocation plus the demand. It can then return an execution strategy.

Conceptually:

```text
Invocation + OutputDemand
          |
          v
   adapter negotiation
     /     |      \
 structured raw   unsupported
    |       |         |
 rewrite  execute   execute raw
 +parse    normal    or error if
                     structure required
```

The demand model MUST be part of execution planning, not an after-the-fact renderer trick.

## 1.6 Adapter negotiation states

An adapter SHOULD return one of a small set of explicit negotiation states:

```text
NotApplicable
RawPreferred(reason)
StructuredSupported(plan)
StructuredSupportedWithLimits(plan, limits)
UnsupportedInvocation(reason)
IncompatibleVersion(found, supported)
```

`NotApplicable` means another adapter may try.

`RawPreferred` means the adapter recognizes the tool but the current context should retain raw semantics, for example because stdout flows to an external program.

`StructuredSupported` means the adapter can provide its advertised schema with high confidence.

`StructuredSupportedWithLimits` means structure is valid but some fields or semantics are unavailable. Limits MUST be visible in provenance/schema metadata.

`UnsupportedInvocation` means the adapter recognizes the executable but not the supplied option combination.

`IncompatibleVersion` means the executable version falls outside tested compatibility constraints.

## 1.7 Execution plan contract

Adapters SHOULD compile an invocation into a declarative execution plan rather than directly spawning processes wherever possible.

Conceptual type:

```text
ExternalAdapterPlan {
    adapter_id: AdapterId
    executable: Path
    argv: List<String>
    environment: Map<String, StringDelta>
    stdin_mode: Inherit | Bytes | Null
    stdout_mode: CaptureBytes | CaptureText | Protocol
    stderr_mode: Inherit | Capture | Merge
    decoder: DecoderId?
    output_schema: SchemaId?
    provenance: ProvenanceTemplate
    version_assertion: VersionConstraint?
    tty: Forbidden | Optional | Required
    fallback: FallbackPolicy
}
```

The Ono process subsystem remains responsible for spawning, process groups, signals, PTYs and resource cleanup. Adapters describe semantics; they SHOULD NOT reinvent process execution.

## 1.8 Provenance requirements

Every adapted value MUST expose provenance sufficient to answer:

- which external executable produced it;
- executable version when known;
- original user invocation;
- actual rewritten invocation;
- adapter package and adapter version;
- decoder/parser strategy;
- observation timestamp;
- host/link context;
- whether fields are exact, normalized, inferred or omitted;
- whether output came from a stable machine format or a version-constrained human parser.

Example:

```text
inspect @socket-17 --provenance

schema            ono.socket/1
provider          adapter:org.ono.compat.iproute2.ss
adapter_version   1.4.0
executable        /usr/bin/ss
executable_ver    iproute2-6.15
user_invocation   ss -tunap
actual_invocation ss -H -O -t -u -n -a -p
parser            ss-text-v6
confidence        exact/version-constrained
observed_at       2026-08-26T13:14:22Z
```

## 1.9 Structured-output strategy hierarchy

Adapters MUST choose the strongest available source strategy. Preferred order:

### Tier A - Native machine-readable protocol

Examples include JSON, XML, Protobuf, stable binary formats or a documented API mode.

```text
ip -j address
journalctl -o json
kubectl ... -o json
```

This is the preferred strategy.

### Tier B - Stable explicit field protocol

The tool exposes deterministic field selection, separators or null-delimited output.

Examples conceptually include:

```text
ps -eo pid=,ppid=,user=,comm=
find ... -printf ...\0
lsof -F ...
git status --porcelain=v2 -z
```

The adapter owns the exact invocation and decoder contract.

### Tier C - Version-constrained human-output parser

The adapter parses human-facing output only when no better mode exists and the value is high enough to justify the maintenance burden.

Requirements:

- executable family and version constraints;
- fixture corpus across supported versions/distros;
- locale control where possible (`LC_ALL=C` or equivalent);
- explicit brittle-parser metadata;
- fast fallback on unknown shapes;
- no silent best-effort field shifting.

`ss` may require this strategy for some useful views unless a more stable machine interface becomes available.

### Tier D - No structured adapter

The correct adapter for many programs is no adapter at all.

`cat`, `grep`, `sed`, `head`, `tail`, `less` and similar text-oriented tools frequently already produce the semantically correct type: text or bytes.

## 1.10 Machine-readable output is not automatically trustworthy

JSON alone does not make an adapter stable.

Adapters MUST still define:

- supported executable versions;
- expected fields and optionality;
- field type coercion;
- unknown-field behavior;
- schema evolution;
- locale/timezone assumptions;
- numeric units;
- error payload handling;
- whether the machine format itself is documented as stable.

A tool's undocumented `--json` output is weaker than a documented stable line protocol.

## 1.11 Canonical schema normalization

The adapter layer exists to normalize system concepts, not to expose one schema per command.

Bad:

```text
SsSocket
IpAddrRow
PsProcessRow
SystemctlUnitLine
```

Preferred:

```text
Socket
InterfaceAddress
Process
Service
```

Tool-specific fields MAY live in a namespaced extension map or schema extension when valuable:

```text
Socket {
    ... canonical fields ...
    extensions: {
        "iproute2.ss": {
            timer: ...
            congestion: ...
        }
    }
}
```

Canonical fields MUST preserve common semantics. Tool-specific richness SHOULD NOT force the core schema to absorb every output flag of every utility.

## 1.12 Native and adapted object equivalence

Where a native provider and external adapter expose the same schema, Ono SHOULD allow normal composition without conversion.

```text
let native = get socket
let legacy = ss -tunap

$native | diff $legacy --key identity
```

This also creates a useful conformance mechanism: adapters can be cross-checked against native providers on systems where both exist.

Object provenance must remain distinct even if schemas match.

## 1.13 Identity and deduplication

Adapters MUST NOT invent stable object identity when the external tool does not provide enough information.

For example, a process can often be keyed by `(host, pid, start_time)` rather than PID alone. A socket may need a tuple plus observation epoch. A service may have a stable manager/unit identity.

When identity is uncertain, the value SHOULD be marked snapshot-only.

This matters for:

- `diff`;
- `watch`;
- history reuse;
- graph relations;
- deduplication across native and adapted sources.

## 1.14 Invocation ownership

An adapter MAY rewrite an invocation only inside its declared semantic surface.

Example:

```text
user:   ip address
actual: ip -j address
```

This rewrite is reasonable when the resulting behavior is equivalent for the requested structured output.

An adapter MUST NOT silently remove, reinterpret or approximate arbitrary flags.

If the user writes:

```text
ip -details -statistics address
```

and the adapter only models basic addresses, it should either:

- use a machine mode that preserves the requested semantics;
- expose a limited structured result with explicit limitations; or
- fall back to raw execution.

## 1.15 Argument parsing and option models

Adapters SHOULD describe recognized options declaratively when practical.

Conceptual descriptor:

```yaml
invocations:
  - id: address-list
    match:
      argv:
        command: ["address", "addr", "a"]
        flags:
          allow: ["-brief", "-details", "-statistics", "dev", "scope", "to", "up"]
    structured_plan:
      argv: ["ip", "-j", "address", "show"]
      decoder: json
      schema: ono.interface-address/1
```

Complex tools MAY require parser code rather than a pure descriptor, but the supported surface must still be machine-readable enough for help, tests and diagnostics.

## 1.16 Unsupported options and partial support

A supported executable is not the same as a supported invocation.

Example:

```text
ss --some-new-mode
```

If the installed adapter does not understand the mode:

```text
local://~ > ss --some-new-mode | where state == established

error ADAPTER_UNSUPPORTED_INVOCATION
adapter org.ono.compat.iproute2.ss recognizes `ss`
but cannot guarantee structured semantics for:
  --some-new-mode

suggestions:
  raw ss --some-new-mode
  ss --some-new-mode | from <format>
```

If there is no structured downstream demand, Ono MAY simply execute raw and optionally expose a subtle diagnostic in `explain` rather than interrupt normal use.

## 1.17 Raw execution escape hatches

The adapter system MUST preserve explicit bypass paths.

Canonical options SHOULD include a form equivalent to:

```text
raw ss -tunap
exec:ss -tunap
```

The final syntax should follow the command-resolution rules established elsewhere, but at least one concise raw path MUST exist.

The bypass means:

- no argv rewrite;
- no decoder;
- no Ono table renderer;
- stdout/stderr behave as ordinary external streams;
- normal external exit status applies.

## 1.18 Explicit adaptation

Users SHOULD also be able to force adaptation when ambiguity exists.

Potential forms:

```text
adapt ss -tunap
ono:ss -tunap
```

Exact syntax remains subject to language review. The semantic requirement is that scripts can state whether structured adaptation is required instead of relying on interactive heuristics.

A forced structured invocation MUST fail rather than silently downgrade to raw text when no compatible adapter is available.

## 1.19 TTY and interactive programs

Adapters MUST declare whether they apply to interactive invocations.

Examples:

- `git status` can be adapted.
- `git log` without a pager may be adapted under a declared mode.
- `git add -p` is interactive and SHOULD bypass structured adaptation.
- `ssh host` owns a TTY and SHOULD remain raw/PTY-driven.
- `top` is interactive and SHOULD remain raw unless a dedicated Ono view intentionally replaces that interaction through a separate native/plugin command.

No adapter should accidentally break terminal feature detection, colors, alternate-screen behavior or input handling.

## 1.20 Exit status and stderr

Structured output MUST NOT erase normal process semantics.

An adapted execution preserves:

- child exit status;
- termination signal;
- stderr bytes unless the adapter explicitly declares a machine error protocol;
- timing and resource metadata where available.

Decoder success does not turn a failing child into success.

If a tool exits non-zero but emits valid partial structured data, the adapter MAY expose both the partial values and a structured external error, using Ono's partial-failure model.

## 1.21 Locale, encoding and terminal width

Human-output parsers are especially vulnerable to environment-dependent rendering.

Adapters that parse text SHOULD control relevant environment variables where safe:

```text
LC_ALL=C
LANG=C
NO_COLOR=1
TERM=dumb
COLUMNS=<known>
```

Only variables necessary to stabilize output should be overridden. The actual environment delta MUST be visible in the execution plan/provenance.

Adapters MUST correctly handle filenames and values that are not valid UTF-8. Null-delimited byte protocols are preferable to line parsing for filesystem paths.

## 1.22 Security and capability boundaries

An adapter is executable knowledge and therefore a security boundary.

A KUANG/11 adapter package that executes external tools MUST request a scoped capability such as:

```text
process.exec:
  executables:
    - /usr/bin/ss
    - /usr/sbin/ss
  argv_policy: declared-invocations-only
```

An adapter MUST NOT turn a declaration for `ss` into general shell execution.

Further rules:

- executable paths SHOULD be resolved and pinned for a plan;
- PATH substitution after approval SHOULD NOT change the binary silently;
- adapters MUST avoid invoking through `sh -c` unless explicitly required and separately authorized;
- user arguments MUST remain structured argv elements rather than concatenated shell strings;
- environment injection must be explicit;
- output parsers must treat tool output as untrusted input;
- terminal escape sequences from adapted output must never gain renderer authority.

First-party bundled compatibility packs may receive predeclared low-risk capabilities, but the capability model still applies.

## 1.23 Discovery and introspection

Users should be able to ask what Ono knows about an external command.

Potential interactions:

```text
get command ss
```

```text
COMMAND ss
resolution       /usr/bin/ss
kind             external
adapter          org.ono.compat.iproute2.ss/1.4.0
structured       conditional
schemas          Socket, SocketSummary
versions         iproute2 >= 6.1 < 7
raw bypass       raw ss ...
```

And:

```text
explain ss -tunap | where state == established
```

could show:

```text
1. resolve external /usr/bin/ss
2. structured downstream requires fields [state]
3. select adapter org.ono.compat.iproute2.ss
4. execute stabilized ss invocation
5. decode using ss-text-v6
6. normalize to ono.socket/1
7. apply typed predicate Socket.state == established
8. render table
```

The adapter layer SHOULD make hidden magic inspectable enough that an expert can reason about it.

## 1.24 Adapter registry

Ono SHOULD maintain a registry of installed adapters separate from command resolution metadata.

Conceptual record:

```text
ExternalAdapter {
    id: AdapterId
    package: PackageId
    version: SemVer
    executable_names: List<String>
    executable_constraints: List<ExecutableConstraint>
    invocation_contracts: List<InvocationContractId>
    output_schemas: List<SchemaId>
    trust: TrustState
    enabled: Bool
    priority: Int
}
```

The registry MUST support multiple adapters for one executable while producing deterministic conflict resolution.

## 1.25 Adapter conflict resolution

If multiple adapters match one invocation, resolution SHOULD consider:

1. explicit user namespace/selection;
2. exact executable identity/path match;
3. exact version match;
4. invocation specificity;
5. trust/publisher policy;
6. configured priority;
7. deterministic lexical tie-break only as a final fallback.

Ono MUST NOT nondeterministically choose an adapter based on plugin load order.

`explain` SHOULD display candidates and the selection reason.

## 1.26 KUANG/11 as the distribution mechanism

The adapter API belongs to Ono's stable extension boundary. The majority of concrete adapters SHOULD be distributed as KUANG/11 packages rather than compiled permanently into the shell core.

This keeps Ono core small while allowing rapid ecosystem coverage.

Suggested first-party package structure:

```text
KUANG/11 compatibility packs

org.ono.compat.linux-base
+-- procps
|   +-- ps
|   +-- free
|   +-- uptime
+-- util-linux
|   +-- lsblk
|   +-- findmnt
|   +-- lsns
|   +-- mount-query
+-- core-system
    +-- stat
    +-- df
    +-- du
    +-- find

org.ono.compat.linux-network
+-- iproute2
|   +-- ip
|   +-- ss
+-- lsof
+-- dns tools

org.ono.compat.systemd
+-- systemctl
+-- journalctl
+-- loginctl

org.ono.compat.operator
+-- curl
+-- wget (limited)
+-- openssh (inspection/non-interactive surfaces)
+-- rsync (limited)

org.ono.compat.developer
+-- git
+-- cargo (selected metadata surfaces)
+-- docker/podman (optional)
+-- kubectl (optional)
```

Packaging MAY be consolidated differently. The important separation is architectural: first-party adapters can evolve independently of the Ono binary while still using signed/trusted KUANG/11 distribution.

## 1.27 Bundled, recommended and community adapters

Adapters SHOULD have support classes:

### Bundled baseline

Installed with normal Ono packages where licensing/distribution permits. High-value, low-risk, broadly available Linux utilities.

### Recommended first-party

Officially maintained but installed on demand because the corresponding tool is not universally present.

### Community

Third-party KUANG/11 packages. Same capability model, weaker trust by default.

### Experimental

Adapters relying on unstable human output or narrow versions. Clearly marked and not used silently for durable scripts unless explicitly enabled.

## 1.28 Distribution baseline methodology

The baseline SHOULD be informed by the common administrative surface of Debian, Ubuntu, Fedora and RHEL rather than by one exact image definition.

"Default installation" is intentionally not a normative package list because distributions provide multiple forms:

- minimal/container image;
- server installation;
- cloud image;
- desktop installation;
- custom package groups.

Ono SHOULD instead maintain a curated **Linux Base Compatibility Set** selected using:

- presence across major Debian- and RPM-family distributions;
- likelihood of being installed on real operator/developer systems;
- semantic richness of output;
- usefulness in object pipelines;
- availability of stable machine-oriented output;
- maintenance cost;
- security implications;
- overlap with existing native Ono providers.

The set should be reviewed per release rather than pretending distribution defaults are static.

## 1.29 Adapter priority scoring

Candidate adapters MAY be scored with a simple review model:

```text
value = prevalence
      * semantic_richness
      * pipeline_value
      * machine_format_quality
      * ecosystem_relevance
      / maintenance_risk
```

This is not intended as literal arithmetic in code. It is a product-review heuristic.

Examples:

- `ps`: extremely high prevalence, rich semantics, high pipeline value -> very high priority.
- `ss`: high operator value, rich semantics, parser maintenance risk -> high priority.
- `lsblk`: rich system object data and machine output -> very high priority.
- `cat`: ubiquitous but already correctly emits text -> almost no adapter value.
- `printf`: no meaningful structured domain -> no adapter.

## 1.30 Initial Linux Base Compatibility Set

The following matrix is a proposed v0.3 target, not a claim that every command is installed on every distribution image.

| Command/family | Canonical Ono output | Preferred strategy | Priority | Notes |
|---|---|---|---|---|
| `ps` | `Process` / `ProcessSummary` | explicit fields | P0 | Familiar process vocabulary; compare with native provider. |
| `free` | `MemorySummary` | stable explicit output/parser | P1 | Useful one-shot host summary. |
| `uptime` | `HostLoad` | stable machine source or constrained parser | P2 | Native provider may supersede. |
| `lsblk` | `BlockDevice` | JSON/explicit output | P0 | Excellent structured candidate. |
| `findmnt` | `Mount` | JSON/explicit output | P0 | Should normalize with native Mount schema. |
| `lsns` | `Namespace` | JSON/explicit output | P1 | High systems value. |
| `stat` | `File` / `FileStat` | explicit format | P0 | Deterministic fields. |
| `df` | `FilesystemUsage` | explicit columns | P1 | Avoid human-size parsing. |
| `du` | `FileUsage` | byte/null-safe modes | P1 | Recursive volume can be large; stream. |
| `find` | `File` / `PathResult` | `-printf` + NUL | P0 | Must preserve unusual filenames. |
| `ip address` | `InterfaceAddress` | JSON | P0 | Strong candidate. |
| `ip link` | `Interface` | JSON | P0 | Normalize with netlink provider. |
| `ip route` | `Route` | JSON | P0 | Normalize with native route schema. |
| `ip neigh` | `Neighbor` | JSON | P1 | Useful network object. |
| `ss` | `Socket` | stable fields or versioned parser | P0 | High value despite parser risk. |
| `systemctl list-units` | `Service` | explicit/show properties | P0 | Normalize with systemd provider. |
| `systemctl show` | `ServiceDetail` | key-value machine output | P0 | Strong data surface. |
| `journalctl` | `JournalEvent` | JSON | P0 | Natural live/event stream. |
| `loginctl` | `LoginSession` / `UserSession` | show properties | P1 | Useful host context. |
| `lsof` | `OpenFile` / `SocketRef` | field mode | P1 | Useful relation enrichment. |
| `id` | `Identity` | explicit constrained parser/native | P2 | Native identity provider likely simpler. |
| `getent` | typed records by database | database-specific parsers | P2 | Useful but many schemas. |

Commands such as `grep`, `sed`, `awk`, `cut`, `tr`, `head`, `tail`, `sort`, `uniq`, `wc`, `tee` and `cat` remain essential Unix citizens but generally SHOULD NOT receive object adapters merely for symmetry. Their text/byte behavior is already their semantic purpose.

## 1.31 Common Operator Compatibility Set

A second curated set SHOULD target tools frequently installed by developers and operators.

| Command/family | Potential Ono output | Strategy | Priority | Scope |
|---|---|---|---|---|
| `curl` | `HttpExchange`, `HttpResponseMeta`, `TransferMetric` | write-out/header machine modes + raw body | P0 | High value, careful body semantics. |
| `wget` | `DownloadResult` | limited/versioned | P2 | Less machine-oriented than curl; avoid overreach. |
| `git status` | `GitStatusEntry` | porcelain v2 + NUL | P0 | Excellent stable protocol surface. |
| `git log` | `Commit` | explicit format + NUL | P0 | Structured history without replacing git. |
| `git for-each-ref` | `GitRef` | explicit format | P0 | Branch/tag/ref discovery. |
| `git diff --numstat` | `GitDiffStat` | stable explicit output | P1 | Full diff remains text/patch. |
| `ssh -G` | `SshConfig` | config dump parser | P2 | Interactive `ssh host` remains raw PTY. |
| `rsync --itemize-changes` | `FileTransferEvent` | constrained parser | P2 | Useful but version/locale care. |
| `strace` | `SyscallEvent` | version-constrained | P2/experimental | Strong cyberpunk value, high parser cost. |
| `tcpdump` | `PacketSummary` | prefer pcap/protocol path | P2/plugin | Dedicated provider may be better. |
| DNS tools | `DnsAnswer` | explicit machine modes where available | P1 | Package names vary by distro. |
| `nmap` | `HostDiscovery`, `PortObservation` | XML output | P2/community | Strong showcase. |

Optional ecosystem packs MAY later include Docker, Podman, Kubernetes, ZFS, LVM-specific tools, cloud CLIs and database clients.

## 1.32 Tool design: `ss`

`ss` is the archetypal high-value adapter because its output represents real objects but is traditionally consumed as text.

User interaction:

```text
local://~ > ss -tunap | where state == established | select local remote process

LOCAL                  REMOTE                 PROCESS
192.168.1.10:443       192.168.1.27:51742     nginx/4812
10.0.0.8:5432          10.0.0.19:44118        postgres/812
```

Pipeline type:

```text
Stream<Socket>
```

The adapter SHOULD map:

- protocol;
- state;
- local address/port;
- remote address/port;
- receive/send queue;
- process identity where permissions permit;
- socket inode/cookie where available;
- namespace/link context;
- optional transport-specific extension fields.

A version-constrained parser SHOULD avoid relying on column widths. If process details are unavailable because of permissions, the `process` field is null/absent with provenance rather than a parse failure.

`ss | grep` remains raw by demand-driven rules.

## 1.33 Tool design: `ip`

The `ip` family should be among the cleanest adapters because many subcommands can expose machine-oriented output.

Examples:

```text
ip address | where family == inet6
ip link | where state == up
ip route | where protocol == static
ip neigh | where state == reachable
```

Potential output types:

```text
Stream<InterfaceAddress>
Stream<Interface>
Stream<Route>
Stream<Neighbor>
```

The adapter SHOULD preserve user selectors such as device, scope and table by translating them into semantically equivalent machine-format invocations.

Native netlink providers and `ip` adapters SHOULD share conformance fixtures for canonical schema meaning.

## 1.34 Tool design: `ps`

A `ps` adapter provides familiarity while proving that Ono's object model is not a replacement-only strategy.

```text
ps aux | where cpu > 20 | sort memory desc
```

The adapter SHOULD NOT parse the visual `ps aux` table if an explicit field invocation can reproduce its semantic data. It can translate a recognized form into a controlled field set and normalize the result.

Canonical fields may include:

```text
pid
ppid
user
uid
state
cpu
memory
rss
vsz
command
args
started_at
terminal
```

If `ps` cannot expose a field required by canonical `Process`, that field remains optional or is enriched through the native provider only when the provenance model makes the merge explicit.

## 1.35 Tool design: `lsblk`, `findmnt` and `lsns`

These tools are especially attractive because they expose system topology that maps naturally to Ono objects.

Examples:

```text
lsblk | where type == disk | select name size model
findmnt | where filesystem == ext4
lsns | group type
```

Potential schemas:

```text
BlockDevice
Mount
Namespace
```

Tree relationships SHOULD be represented structurally rather than encoded only through indentation. A renderer may still display a tree.

For `lsblk`, parent/child device relationships can contribute to `trace block-device` later.

## 1.36 Tool design: `systemctl`

`systemctl` is both a command surface and a source of structured system state.

High-value adapted forms include:

```text
systemctl list-units
systemctl list-unit-files
systemctl show nginx.service
systemctl is-active nginx.service
systemctl is-failed nginx.service
```

Read operations can normalize to `Service` / `ServiceDetail`.

Mutating operations such as:

```text
systemctl restart nginx
```

SHOULD remain ordinary external actions unless a dedicated adapter contract explicitly maps them to Ono `ActionResult` semantics and preserves authorization/error behavior. Native Ono commands such as `restart service nginx` remain the preferred typed mutation API.

The adapter layer should be conservative about pretending imperative external commands are equivalent to native actions.

## 1.37 Tool design: `journalctl`

`journalctl` is a natural structured event source.

```text
journalctl -u nginx --since today | where priority <= 3
```

may become:

```text
Stream<JournalEvent>
```

Fields SHOULD retain raw journal keys in a namespaced extension while normalizing common concepts:

```text
timestamp
priority
message
unit
pid
uid
boot_id
host
cursor
```

`journalctl -f` can map to a live stream and SHOULD integrate with Ono cancellation/backpressure semantics.

The journal cursor is useful as a stable continuation/provenance token.

## 1.38 Tool design: `find`

`find` is valuable but dangerous to parse as lines because Unix filenames may contain newlines.

A structured adapter SHOULD translate supported discovery invocations into a null-delimited explicit field protocol.

Example:

```text
find . -type f -mtime +30 | where size > 100MiB
```

The adapter may preserve `find`'s own filtering while emitting `File` or `PathResult` objects.

Rules:

- path boundaries MUST be byte-safe;
- unsupported actions such as complex `-exec` combinations MAY force raw mode;
- ordering should match the underlying tool unless documented otherwise;
- adapter-side enrichment (`stat`) should be opt-in or cost-aware to avoid turning cheap path enumeration into expensive metadata storms.

## 1.39 Tool design: `stat`, `df` and `du`

These tools expose structured filesystem facts and are straightforward adapter candidates when stable explicit formats are used.

Examples:

```text
stat README.md | select path size modified mode owner

df | where available < 10GiB

du -a /var | sort bytes desc | take 20
```

`du` MUST stream rather than buffer complete trees. Units MUST be normalized to typed byte values independent of human-readable flags.

If the user's invocation explicitly asks for a particular human representation for an external consumer, demand-driven raw mode should preserve it.

## 1.40 Tool design: `lsof`

`lsof` can enrich relations between processes, files and sockets.

Potential output:

```text
Stream<OpenFile>
```

with references to canonical objects:

```text
OpenFile {
    process: ProcessRef
    fd: String
    kind: OpenFileKind
    path: Path?
    socket: SocketRef?
    device: ...
}
```

This adapter can contribute edges to Ono's relationship graph without making `lsof` itself a native provider.

Because permissions significantly affect visibility, provenance MUST state observation limitations.

## 1.41 Tool design: `curl`

`curl` is a special case because stdout often **is the response body**. Ono must not replace body bytes with a metadata table merely because an adapter exists.

The adapter should model an HTTP exchange as a compound result with explicit body semantics.

Conceptually:

```text
HttpExchange {
    request: HttpRequestSummary
    response: HttpResponseMeta
    transfer: TransferMetric
    body: ByteStream | TextStream | StructuredValue?
}
```

Potential interactions:

```text
curl https://example.com/api/users | from json
```

continues to work as classic bytes/text.

A specifically adapted form could expose metadata:

```text
adapt curl https://example.com | inspect
```

or a future Ono-native `get url` command could use the same schema.

The adapter MAY use stable curl write-out/header facilities to capture status, timing, remote endpoint, content type and transfer metrics while keeping the body stream separate.

Rules:

- do not buffer arbitrary response bodies merely to create an object;
- preserve streaming downloads;
- never expose credentials/secrets in provenance/history;
- redirects and TLS metadata require explicit schema semantics;
- interactive progress meters should not corrupt structured output.

## 1.42 Tool design: `git`

Git is an ideal demonstration that adapters can improve a complex tool without replacing it.

Ono SHOULD focus on stable machine-oriented Git surfaces rather than trying to type every Git subcommand.

High-value examples:

```text
git status | where state != unmodified

git log | where author.email == "me@example.org" | take 20

git branch | where upstream.ahead > 0
```

Candidate mappings:

```text
git status --porcelain=v2 -z       -> Stream<GitStatusEntry>
git log --format=... -z            -> Stream<Commit>
git for-each-ref --format=...      -> Stream<GitRef>
git diff --numstat -z              -> Stream<GitDiffStat>
```

Human patch output remains text:

```text
git diff | less
```

Interactive commands remain Git-native:

```text
git add -p
git rebase -i
```

The adapter MUST respect repository discovery, worktree context, submodules and unusual filenames.

## 1.43 Tool design: SSH and other terminal-owning tools

The existence of an adapter does not imply that the main command should be adapted.

`ssh host` should remain an external PTY session. Useful narrow surfaces may still be structured:

```text
ssh -G host
```

could expose resolved SSH configuration as `SshConfig`.

Similarly:

- `top` remains interactive;
- `less` remains interactive/textual;
- editors remain untouched;
- REPLs remain PTY-driven;
- `watch` as an external command remains external even though Ono has a native `watch` verb.

The adapter system must know when *not* to be clever.

## 1.44 Adapter manifest

KUANG/11 SHOULD define a declarative adapter contract for the majority of simple integrations.

Example:

```yaml
package:
  id: org.ono.compat.iproute2
  version: 1.4.0
  kuang_api: ">=11.1 <12"

roles: [adapter]

capabilities:
  - process.exec:
      executables: ["/usr/bin/ip", "/usr/sbin/ip", "/usr/bin/ss", "/usr/sbin/ss"]
      argv_policy: declared-only

adapters:
  - id: ip-address
    executable:
      names: [ip]
      versions: ">=6.1 <7"
    invocation:
      aliases:
        - [address]
        - [addr]
        - [a]
      mode: read-only
    output_demand: [structured, interactive]
    rewrite:
      insert: ["-j"]
    decoder:
      kind: json
      contract: iproute2.address.v1
    schema: ono.interface-address/1
    fallback: raw

  - id: ss-sockets
    executable:
      names: [ss]
      versions: ">=6.1 <7"
    invocation:
      matcher: builtin:ss-v1
    output_demand: [structured, interactive]
    rewrite:
      planner: builtin:ss-stable-fields-v1
    decoder:
      kind: parser
      id: ss-text-v6
      stability: version-constrained
    schema: ono.socket/1
    fallback: raw
```

The actual manifest schema MUST be versioned and machine-validated.

## 1.45 Adapter SDK

Simple adapters SHOULD be possible without writing a full plugin runtime component.

The declarative SDK should support:

- executable matching;
- version detection;
- option/invocation matching;
- argv rewrite templates;
- environment stabilization;
- decoder selection;
- schema mapping;
- field coercion;
- provenance templates;
- fallback policy;
- fixtures.

Complex adapters MAY use the component SDK.

Conceptual Rust API:

```rust
trait ExternalCommandAdapter {
    fn descriptor(&self) -> &AdapterDescriptor;

    fn negotiate(
        &self,
        executable: &ResolvedExecutable,
        invocation: &ExternalInvocation,
        demand: &OutputDemand,
    ) -> AdapterNegotiation;

    fn plan(
        &self,
        ctx: &AdapterContext,
        invocation: &ExternalInvocation,
    ) -> Result<ExternalAdapterPlan>;

    fn decode(
        &self,
        ctx: &DecodeContext,
        stdout: ByteStream,
    ) -> Result<ValueStream>;
}
```

Adapters SHOULD be deterministic for the same executable identity/version, invocation and environment contract.

## 1.46 Version detection

Adapter matching SHOULD avoid executing arbitrary version probes repeatedly.

The registry MAY cache executable fingerprints keyed by:

```text
path
inode/device or platform equivalent
mtime
size
optional content hash
```

Version probes should be declared and bounded:

```yaml
version_probe:
  argv: ["--version"]
  parser: regex:...
  cache: executable-identity
```

If version detection fails, a high-risk structured parser SHOULD refuse to adapt rather than assuming compatibility.

Machine protocols that are genuinely version-independent may declare broader compatibility.

## 1.47 Fixture and conformance requirements

Every first-party adapter MUST ship fixtures for its supported surface.

Fixture dimensions SHOULD include:

- multiple supported tool versions;
- Debian-family and RPM-family outputs where output differs;
- empty results;
- permission-limited results;
- unusual Unicode and non-UTF-8 path data where applicable;
- very long values;
- IPv4/IPv6;
- namespaces/containers where relevant;
- locale variations for any parser that cannot force locale;
- error output;
- unknown fields added by newer versions;
- malformed/truncated output.

CI SHOULD test:

```text
fixture bytes
    -> decoder
    -> canonical value
    -> schema conformance
    -> provenance assertions
```

Adapters with command rewrite logic SHOULD additionally test generated argv exactly.

## 1.48 Live executable conformance tests

Where licenses and CI environments permit, first-party adapter tests SHOULD run the real tools in container images representing supported distribution families.

Example matrix concept:

```text
Debian stable
Ubuntu LTS
Fedora current
RHEL-compatible current
```

The purpose is not to guarantee every distribution image but to detect output/protocol drift across the major Linux families targeted by Ono.

Container tests SHOULD verify both adapted and raw paths.

## 1.49 Cross-provider conformance

When Ono has a native provider for the same canonical schema, acceptance tests SHOULD compare semantic results.

Example:

```text
native:  get process 1
adapted: ps ... | where pid == 1
```

Fields known to both paths should agree within documented timing/race constraints.

This is especially useful for:

- Process;
- Socket;
- Interface;
- Route;
- Mount;
- Service.

Differences become either bugs, provenance limitations or schema-design feedback.

## 1.50 Performance requirements

Adapters must not make familiar external commands feel slow.

Requirements SHOULD include:

- adapter lookup near-constant and cheap;
- executable version probes cached;
- streaming decoders where records can be emitted incrementally;
- bounded buffering;
- no whole-output parse for unbounded commands when a streaming protocol exists;
- no hidden N+1 enrichment by default;
- no network calls merely to resolve an adapter;
- startup must not load every adapter runtime eagerly.

A first-party adapter pack should mostly contribute metadata until a matching executable is invoked.

## 1.51 Enrichment policy

Adapters MAY enrich external output using Ono-native providers, but only under explicit cost/provenance rules.

Example: `ps` provides PID and command, while the native provider can add cgroup/service identity.

Possible policy:

```text
adapt.enrichment = off | cheap | full
```

Default SHOULD be `cheap` or equivalent, with no surprising network or privileged operations.

Every enriched field must record its own source if it did not originate from the external command.

## 1.52 Caching

Adapters MAY cache:

- executable/version identity;
- parsed static capability metadata;
- schema mapping tables.

They SHOULD NOT cache live command results unless the user/provider semantics explicitly allow it.

A command typed at the prompt should normally observe current system state.

## 1.53 Scripts and reproducibility

Interactive auto-adaptation is convenient. Scripts require stronger determinism.

Ono scripts SHOULD be able to require:

```text
adapter org.ono.compat.iproute2.ss >= 1.4
```

or an equivalent module/capability declaration when their correctness depends on adapted external output.

A durable script SHOULD NOT depend on an adapter that may silently disappear and turn a typed pipeline into text.

Therefore:

- structured demand in scripts SHOULD fail if the required adapter is unavailable;
- scripts MAY pin adapter/schema versions;
- raw external commands remain portable without adapter dependencies;
- script metadata SHOULD expose external executable dependencies separately from adapter dependencies.

## 1.54 Remote links

Adapter resolution on remote hosts requires explicit locality semantics.

If a command executes on `prod-db`, the compatible adapter and executable version must be evaluated for `prod-db`, not only for the local deck.

Agent mode can negotiate:

```text
remote capabilities
remote executable inventory/fingerprints
remote adapter availability
schemas supported
```

Possible strategies:

1. remote Ono agent runs the adapter locally and streams canonical values;
2. local Ono invokes a remote raw command and decodes locally if the adapter contract allows it;
3. raw SSH fallback with no adaptation.

Capability and provenance records MUST identify where decoding occurred.

## 1.55 KUANG/11 marketplace implications

External adapters are an ideal low-barrier KUANG/11 ecosystem contribution because they can be narrow, useful and independently testable.

A community package might provide:

```text
org.example.compat.nmap
org.example.compat.smartctl
org.example.compat.zfs
org.example.compat.virsh
org.example.compat.nvidia-smi
```

A good adapter marketplace entry should publish:

- supported tool versions;
- supported invocation matrix;
- emitted schemas;
- capabilities requested;
- parser stability class;
- fixture coverage;
- target OS/distributions;
- last conformance run.

Ono SHOULD make these properties visible before installation.

## 1.56 Trust and signing

First-party adapters may rewrite executable arguments. That makes supply-chain integrity important.

Signed packages SHOULD bind:

- manifest;
- decoder/component bytes;
- schemas;
- fixtures or fixture hashes where practical;
- publisher identity;
- version.

An update that expands executable capabilities or supported mutation surfaces SHOULD be surfaced as a permission-relevant change.

## 1.57 Adapter diagnostics

The runtime SHOULD expose concise diagnostics without spamming normal shell use.

Potential diagnostic states:

```text
adapted
raw (downstream bytes)
raw (unsupported invocation)
raw (adapter disabled)
raw (version incompatible)
degraded (partial fields)
failed (decode error)
```

`explain`, verbose mode and structured history should record these states.

A decode failure in interactive mode could render:

```text
adapter org.ono.compat.iproute2.ss failed to decode ss output
expected iproute2 >=6.1 <7, found 6.15
fixture signature mismatch near record 4

falling back to raw output
```

Fallback is appropriate only when doing so cannot make a structured downstream pipeline silently incorrect.

## 1.58 History integration

History SHOULD remember that a command was adapted.

```text
HistoryEntry {
    ...
    resolution: External("/usr/bin/ss")
    adapter: AdapterExecution?
    result_ref: ResultRef?
}
```

This allows later inspection:

```text
history --explain 1842
```

and prevents confusion when the same command behaves raw on another machine lacking the adapter.

## 1.59 Completion integration

Adapters MAY contribute semantic completion for external tools, but MUST NOT attempt to replace mature tool-specific completion blindly.

High-value additions include:

- known schema fields after an adapted pipeline;
- adapter-supported invocation hints;
- host objects for selectors that the external tool accepts;
- warnings when a chosen flag leaves the structured support surface.

Example:

```text
ss -tuna<TAB>
```

can still use normal external completion.

After the pipe:

```text
ss -tunap | where <TAB>
```

completion knows `Socket` fields.

That transition itself is part of the Ono "wow" factor.

## 1.60 Rendering policy

When an adapted command terminates at the interactive Ono renderer, canonical Ono rendering applies by default.

This means:

```text
ss -tunap
```

may look different from raw `ss -tunap`, because Ono is rendering `Socket` objects rather than replaying stdout.

The renderer SHOULD preserve useful domain conventions where practical, but exact visual compatibility is not required.

Users who need exact output use raw execution.

Adapted commands redirected to an external byte sink SHOULD normally preserve original output as defined by demand-driven adaptation.

## 1.61 Adapter-aware `type` and `inspect`

The shell's introspection commands SHOULD make layer membership visible.

```text
type ss
```

could return:

```text
external executable
path      /usr/bin/ss
adapter   org.ono.compat.iproute2.ss
```

while:

```text
inspect command ss
```

returns structured metadata including supported schemas and invocation contracts.

This avoids creating an invisible second command-resolution system.

## 1.62 No adapter monopoly

An adapter MUST NOT prevent users from invoking alternative implementations.

If `ss` resolves differently under PATH, containers, Nix profiles or aliases, adapter matching must follow executable identity constraints rather than blindly matching the command token.

For example, a binary named `ss` that is not iproute2 MUST NOT receive the iproute2 decoder solely because of its basename.

## 1.63 Distro packaging

Distribution packages MAY choose to install a curated set of first-party adapter packs alongside Ono.

Suggested policy:

```text
ono
  hard dependency: core shell/runtime only

ono-compat-linux-base
  recommended dependency on common Linux packages where appropriate

ono-compat-network
ono-compat-systemd
ono-compat-operator
ono-compat-developer
```

Exact package names are packaging-level decisions.

Ono MUST remain fully functional when no compatibility pack is installed. The result is simply more raw TextStream/ByteStream behavior.

## 1.64 Licensing boundary

Adapters execute third-party/system utilities but generally should not copy their implementation.

The project MUST still review:

- redistribution of fixtures derived from tool output;
- bundled schemas/protocol descriptions;
- whether first-party packaging pulls tools as dependencies;
- trademarks/names in adapter metadata;
- licenses of optional parser libraries.

Keeping the executable external usually preserves a cleaner separation than embedding its implementation.

## 1.65 Error taxonomy additions

The adapter layer SHOULD introduce stable structured errors such as:

```text
ADAPTER_NOT_AVAILABLE
ADAPTER_DISABLED
ADAPTER_UNSUPPORTED_INVOCATION
ADAPTER_VERSION_INCOMPATIBLE
ADAPTER_EXECUTABLE_MISMATCH
ADAPTER_REWRITE_FAILED
ADAPTER_DECODE_FAILED
ADAPTER_SCHEMA_VIOLATION
ADAPTER_CAPABILITY_DENIED
ADAPTER_CONFLICT
ADAPTER_REQUIRED_FOR_STRUCTURED_PIPELINE
```

These errors should carry:

- adapter ID/version;
- executable identity/version;
- original invocation;
- whether raw fallback is safe;
- suggested recovery.

## 1.66 Spec-driven adapter derivation

The adapter framework SHOULD participate in the same derivation pipeline as commands and schemas.

A machine-readable adapter contract can generate:

- adapter reference pages;
- supported invocation matrices;
- completion warnings;
- executable/version probes;
- capability declarations;
- fixture harnesses;
- schema conformance tests;
- packaging metadata;
- compatibility dashboards.

Suggested registry tree:

```text
spec/
+-- adapters/
|   +-- schema.yaml
|   +-- first-party/
|       +-- iproute2.yaml
|       +-- procps.yaml
|       +-- util-linux.yaml
|       +-- systemd.yaml
|       +-- git.yaml
|       +-- curl.yaml
+-- schemas/
+-- commands.yaml
+-- errors.yaml
```

KUANG/11 packages MAY ship adapter descriptors externally, but first-party bundled contracts should also be testable from the main source tree.

## 1.67 Work package derivation

A code-generating agent could derive work claims such as:

```text
ADAPT-001  OutputDemand execution contract
ADAPT-002  adapter registry and deterministic resolution
ADAPT-003  raw bypass semantics
ADAPT-004  adapter execution-plan integration
ADAPT-005  decoder streaming API
ADAPT-006  version probe cache
ADAPT-007  provenance model
ADAPT-008  capability mapping into KUANG/11
ADAPT-009  declarative manifest schema
ADAPT-010  fixture conformance harness
ADAPT-011  remote adapter negotiation

COMPAT-PS-001       procps ps explicit-field adapter
COMPAT-IP-001       ip address JSON adapter
COMPAT-IP-002       ip link JSON adapter
COMPAT-IP-003       ip route JSON adapter
COMPAT-SS-001       ss invocation matcher
COMPAT-SS-002       ss versioned decoder
COMPAT-LSBLK-001    lsblk adapter
COMPAT-FINDMNT-001  findmnt adapter
COMPAT-SYSTEMD-001  systemctl read adapter
COMPAT-JOURNAL-001  journalctl JSON event adapter
COMPAT-FIND-001     find NUL-safe path adapter
COMPAT-GIT-001      git status porcelain-v2 adapter
COMPAT-GIT-002      git log explicit-format adapter
COMPAT-CURL-001     curl exchange metadata adapter
```

This level of decomposition is intentionally compatible with the project's broader spec-driven implementation model.

## 1.68 Release quality bar for first-party adapters

A first-party adapter SHOULD NOT be marked stable until:

1. its supported invocation surface is machine-readable;
2. its executable/version constraints are explicit;
3. raw fallback behavior is defined;
4. fixtures cover all supported output families;
5. malformed output cannot panic the shell;
6. canonical schema conformance passes;
7. provenance is complete;
8. capability requirements are minimal and reviewed;
9. cross-distro live tests pass where applicable;
10. structured and raw pipeline behavior are both acceptance-tested;
11. TTY and redirection regressions are tested;
12. unsupported flags demonstrably fail/fallback safely;
13. performance overhead is measured;
14. documentation states limitations;
15. `explain` can show the selected adaptation plan.

## 1.69 Initial recommended implementation order

The first adapter ecosystem SHOULD prioritize maximum semantic value with minimum parser risk.

Suggested order:

```text
1. adapter framework + OutputDemand
2. lsblk / findmnt / lsns
3. ip address/link/route/neigh
4. journalctl JSON
5. systemctl show/list read surfaces
6. ps explicit fields
7. stat / df / find
8. git status/log/ref machine protocols
9. lsof field mode
10. ss version-constrained adapter
11. curl metadata/body split
12. broader operator/community packs
```

This order intentionally begins with machine-oriented protocols before tackling brittle human-output decoders.

## 1.70 What should deliberately remain raw

The existence of a compatibility program must not become a quota requiring adapters for every common executable.

Likely raw-by-design families include:

```text
cat
less
more
grep
sed
awk
cut
tr
head
tail
sort
uniq
wc
tee
printf
editors
REPLs
interactive TUIs
compilers whose primary output is diagnostics/artifacts
```

Some may later gain narrow metadata adapters, but their ordinary output remains text/bytes.

A mature Ono ecosystem should be proud of leaving the correct tools raw.

## 1.71 Product experience

The adapter layer should create a very specific user reaction: familiar commands suddenly become composable with Ono semantics without losing their Unix identity.

```text
local://~ > ps aux | where cpu > 20

PID    USER      CPU     MEM      COMMAND
4419   masl      96.1%   3.8GiB   rustc
812    postgres  24.8%   1.2GiB   postgres
```

Then:

```text
local://~ > ss -tunap | where process.pid in @-1.pid

PROTO  STATE        LOCAL            REMOTE           PROCESS
TCP    established  10.0.0.8:5432    10.0.0.19:44118  postgres/812
```

Then:

```text
local://~ > ip route | where dev == "wg0"
```

No special "legacy integration mode" is visible in ordinary use. The commands remain recognizable Unix commands. The object behavior appears exactly where it becomes useful.

Yet an expert can always ask:

```text
explain ss -tunap | where state == established
```

or escape:

```text
raw ss -tunap
```

That combination - familiar vocabulary, richer semantics, inspectable magic and an immediate escape hatch - is the intended Ono feel.

## 1.72 Relationship to native Ono commands

The adapter layer does not deprecate native Ono vocabulary.

Both of these are first-class:

```text
get socket | where state == established
ss -tunap | where state == established
```

They serve different user instincts.

Native commands offer:

- predictable Ono grammar;
- stable cross-provider contracts;
- deeper object identity;
- direct provider capabilities;
- consistent help/discovery.

Adapted commands offer:

- familiarity;
- migration comfort;
- ecosystem reach;
- leverage of mature existing utilities;
- an incremental path from Unix text to Ono objects.

A user can gradually learn Ono rather than crossing a hard language boundary on day one.

## 1.73 Strategic consequence

This layer changes the migration story substantially.

Without adapters, Ono says:

> Learn our native commands when you want objects; old Unix commands still work as text.

With adapters, Ono can say:

> Your existing Unix muscle memory already participates in Ono where we can do so honestly.

That is a stronger proposition for the intended expert audience because it respects accumulated knowledge instead of treating it as legacy baggage.

It also gives KUANG/11 a concrete ecosystem role before AI assistants or exotic analysis plugins are required: **teach the deck to understand the tools operators already carry.**

## 1.74 Non-goals

The adapter layer MUST NOT become:

- automatic table scraping for arbitrary stdout;
- a promise that every flag of every supported tool is typed;
- a compatibility emulator that replaces the executable;
- a hidden shell-script behavior changer;
- a reason to duplicate native providers indefinitely;
- a way around KUANG/11 capability controls;
- a central repository of fragile regexes without version contracts;
- an excuse to claim semantic equivalence where only presentation is understood;
- a requirement that users install every supported external program;
- a mechanism that prevents raw Unix execution.

## 1.75 Design summary

The external adapter layer is successful when the following statements are simultaneously true:

1. `ss`, `ps`, `ip`, `lsblk`, `systemctl`, `journalctl`, selected `git` commands and other curated tools can participate in typed Ono pipelines.
2. The tools remain external programs maintained by their original projects.
3. Native Ono commands and adapted commands converge on shared object schemas.
4. Classic external pipelines and redirections retain expected byte semantics.
5. Unsupported invocations fall back rather than being guessed.
6. Every adaptation is inspectable through provenance and `explain`.
7. KUANG/11 can deliver first-party and community adapters without bloating Ono core.
8. The initial compatibility set reflects real Linux operator environments across Debian-, Ubuntu-, Fedora- and RHEL-oriented systems without pretending their package defaults are identical.
9. Text-oriented tools remain text-oriented where that is the correct abstraction.
10. The user experiences the result not as a compatibility framework, but as Unix becoming structurally aware.

The concise product principle is:

> **Adapt before replacing. Normalize concepts, not commands. Fall back honestly.**

# 2. Integration Checklist for a v0.3 Implementation

The following checklist summarizes the minimum architectural consequences of adopting this document. It is intentionally redundant with the detailed normative text so an implementation agent can use it as a final cross-check.

## 2.1 Core runtime

- [ ] Add an `OutputDemand` concept to external execution planning.
- [ ] Add a stable External Command Adapter API.
- [ ] Add a deterministic adapter registry and conflict-resolution mechanism.
- [ ] Keep process creation, PTY ownership, signals and job control in Ono's process subsystem.
- [ ] Preserve a guaranteed raw execution path.
- [ ] Preserve byte semantics for arbitrary external pipelines and redirections unless structured demand is explicit or safely interactive.
- [ ] Attach adapter provenance to every adapted value.
- [ ] Reject or safely fall back on unsupported invocations and incompatible versions.

## 2.2 Schema and object system

- [ ] Reuse canonical Ono schemas whenever adapted output describes an existing Ono concept.
- [ ] Define adapter-specific schemas only when no canonical semantic equivalent exists.
- [ ] Keep raw source/provenance inspectable.
- [ ] Define partial/limited structured-output semantics explicitly.

## 2.3 KUANG/11

- [ ] Add adapter roles/capabilities to the KUANG/11 manifest model.
- [ ] Add adapter SDK interfaces and deterministic test-host support.
- [ ] Permit first-party, recommended, community and experimental adapter packs.
- [ ] Require signing/trust policy appropriate to executable invocation and decoding privileges.
- [ ] Prevent adapters from bypassing KUANG/11 capability controls.

## 2.4 User experience

- [ ] Make adaptation mostly invisible in successful ordinary use.
- [ ] Make adaptation fully inspectable through `type`, `inspect` and/or `explain` semantics.
- [ ] Provide an immediate raw escape hatch.
- [ ] Integrate adapter-aware completion without inventing options unsupported by the underlying executable.
- [ ] Preserve familiar Unix muscle memory.

## 2.5 Initial compatibility program

- [ ] Start with machine-readable or explicit-field protocols before fragile human-output parsers.
- [ ] Prioritize `lsblk`/`findmnt`/`lsns`, `ip`, `journalctl`, `systemctl`, `ps`, `stat`, `df`, `find`, selected `git`, `lsof`, `ss` and `curl` surfaces as described in section 1.
- [ ] Maintain a curated cross-distribution baseline rather than claiming one universal "default installation".
- [ ] Deliberately leave text-oriented tools raw where text is the correct abstraction.

## 2.6 Release evidence

- [ ] Every stable adapter has machine-readable support claims.
- [ ] Every stable adapter has executable/version constraints.
- [ ] Every stable adapter has fixtures covering its supported output families.
- [ ] Cross-distro live conformance exists where applicable.
- [ ] Raw and structured behavior are both acceptance-tested.
- [ ] Unsupported flags and incompatible versions demonstrably fail or fall back safely.
- [ ] Parser/decoder failures cannot crash Ono.
- [ ] Adapter overhead is measured.
- [ ] Documentation and compatibility matrices are generated from contracts where possible.

# 3. Closing Product Statement

The v0.3 adapter layer is not intended to make Ono "understand every command". Its purpose is narrower and more powerful: preserve the Unix ecosystem while allowing high-value, semantically rich tools to cross the boundary into Ono's typed systems interface when that can be done explicitly, versionably and honestly.

The desired user experience is:

```text
old knowledge still works
        +
Ono-native structure appears when useful
        +
raw Unix is always one escape hatch away
```

That is the migration strategy as much as it is an implementation strategy.

> **Keep your Unix commands. Ono makes the ones it understands participate in the object world.**
