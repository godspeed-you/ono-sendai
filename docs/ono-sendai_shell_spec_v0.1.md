
---
title: "KUANG"
subtitle: "Product, Language and Architecture Specification v0.1"
author: "Working design specification"
date: "2026-08-26"
lang: de-DE
toc: true
toc-depth: 3
numbersections: true
geometry: margin=22mm
fontsize: 10pt
papersize: a4
header-includes:
  - |
    \usepackage{longtable}
    \usepackage{booktabs}
    \usepackage{array}
    \usepackage{microtype}
    \usepackage{fvextra}
    \DefineVerbatimEnvironment{Highlighting}{Verbatim}{breaklines,breakanywhere,commandchars=\\\{\}}
---

> **Status:** Explorative but implementation-oriented specification. `KUANG` is a working name.
>
> **Intent:** This document is deliberately more detailed than a conventional pitch. It is meant to be usable as a source-of-truth from which product requirements, parser rules, command metadata, object schemas, documentation, completion metadata, test fixtures, milestones and implementation work packages can be derived.
>
> **Normative language:** `MUST`, `MUST NOT`, `SHOULD`, `SHOULD NOT` and `MAY` are used in the RFC sense. They indicate implementation requirements, not rhetorical emphasis.

# 0. Executive Pitch

**KUANG is a Unix shell designed around two ideas that traditional shells never had the opportunity to make fundamental: a predictable language and structured, typed pipelines.**

Bash is extraordinarily capable, portable and culturally foundational, but it exposes decades of Unix history rather than a coherent interaction model. Every command has its own grammar. `ps`, `find`, `tar`, `dd`, `ip`, `systemctl`, `git`, `awk` and `grep` are individually useful but collectively form a vocabulary a user memorizes rather than a language a user understands. Their outputs are mostly text, which means the structure the operating system already knows is flattened into characters and later reconstructed with regexes, field positions, `awk`, `cut`, `sed`, `jq` or tool-specific parsers.

PowerShell demonstrated that object pipelines are a profound improvement: processes can remain processes, files can remain files, and services can remain services while travelling through a pipeline. It also showed the value of a consistent `Verb-Noun` vocabulary. KUANG takes those ideas seriously, but reinterprets them for Unix culture: terse enough for a terminal, composable like a Unix shell, deeply interoperable with existing programs, and intentionally designed to feel like a systems interface rather than an administration framework.

The product thesis is:

> **Bash is a command interpreter. PowerShell is an object shell. KUANG is a systems interface.**

The emotional thesis is equally important:

> **KUANG should feel like a serious systems tool from a slightly different timeline.**

The shell is intended first for people who enjoy terminals, operating systems, observability, networks and programming for their own sake. Coolness is therefore not decoration. It is a product requirement. A feature is "Kuang-like" when its technical power and its interaction design reinforce each other. A live object stream is useful and looks alive. A dependency trace is useful and feels like entering a machine. A context-aware prompt is useful and creates a sense of place. Fake "mainframe hacking" animations are not Kuang-like because they add theatre without capability.

At the prompt, KUANG should be immediately approachable:

```text
local://~ > get process | where cpu > 20 | sort cpu desc

PID    PROCESS        CPU     MEM      USER
4419   rustc          96.1%   3.8 GiB  masl
812    postgres       24.8%   1.2 GiB  postgres

local://~ >
```

But the table is only a representation. The pipeline contains typed records:

```text
Stream<Process>
```

The next operation therefore does not parse columns:

```text
local://~ > get process | where name == "postgres" | stop process
```

Existing Unix programs remain first-class citizens:

```text
local://~ > git log --oneline | grep fix
```

Their output enters KUANG as text or bytes. KUANG does not attempt to magically infer schemas from arbitrary stdout. Structured data is native where KUANG owns the command or where an external program explicitly speaks a structured protocol; otherwise it remains honest about receiving text.

A complete KUANG experience extends beyond pipelines. Results can be inspected, filtered and reused. Commands are discoverable. The prompt expresses the current machine and context. Long-running streams can be watched without losing the shell. Remote hosts can become links rather than one-off SSH subprocesses. Selected resources can become a temporary context. System relationships can be traced when providers know how to expose them.

A plausible future interaction:

```text
local://~ > link prod-db
link prod-db established  12 ms  linux/amd64

prod-db://~ > get service postgres

SERVICE postgres
state       running
pid         1821
since       6d 03h
ports       [5432]
unit        postgresql.service

prod-db://~ > enter service postgres
prod-db://service/postgres > get socket

PROTO  LOCAL        REMOTE            STATE
unix   .s.PGSQL...  -                 listen
tcp    :5432        10.4.3.17:55128   established
tcp    :5432        10.4.3.21:49312   established

prod-db://service/postgres > trace

postgres
+-- listens tcp/:5432
+-- reads /etc/postgresql/...
+-- writes /var/lib/postgresql/...
+-- clients
    +-- api-1  10.4.3.17
    +-- api-2  10.4.3.21
```

This is not required for the first implementation phase, but it illustrates the direction: **the terminal stops being only a place where commands are typed and starts becoming a view into a connected system model.**

KUANG succeeds if expert users keep it installed because it is genuinely useful, but show it to other people because it is unusually satisfying to use.

# 1. Problem Definition

### 1.1 Bash is excellent at what it inherited

Bash should not be treated as a failed design. It is the product of a Unix tradition in which small programs communicate through byte streams, where text is universal, debuggable and language-neutral. That model created an ecosystem of extraordinary durability. KUANG should preserve its strongest property: arbitrary programs can be composed without needing to know about the shell's internal type system.

The problem is not that text pipes are bad. The problem is that **text pipes are the only universal abstraction**.

A process is structured data before `ps` prints it. A socket is structured data before `ss` formats it. A mount has a filesystem type, source, target and options before `mount` renders a line. A service manager has state, dependencies, timestamps and identifiers before `systemctl` generates human-oriented text. Flattening those values into text is useful for humans, but lossy for composition.

### 1.2 Three structural weaknesses KUANG addresses

**A. Command vocabulary is historically inconsistent.**

The following operations all belong to one conceptual domain - inspecting or manipulating system state - but their naming and syntax are unrelated:

```text
ps aux
ss -tulpn
ip addr show
df -h
find . -name '*.rs'
systemctl restart nginx
kill -TERM 1234
journalctl -u nginx -f
```

The burden is placed on human memory.

**B. Output structure is routinely destroyed.**

A classic shell pipeline often uses formatting as an accidental API:

```bash
ps aux | grep postgres | awk '{print $2}' | xargs kill
```

Field positions, delimiters and escaping rules become part of program semantics even when they were intended only for display.

**C. Argument syntax has no common grammar.**

Unix programs legitimately evolved independently. The result is that options can look like `-x`, `--long`, `key=value`, positional modes, subcommands or tool-specific expressions. A shell cannot make external programs consistent, but native commands can form a language instead of another pile of utilities.

### 1.3 Secondary problems

KUANG should also improve:

- discoverability without abandoning keyboard speed;
- safe manipulation of destructive operations;
- predictable filtering, sorting, projection and grouping;
- machine-readable error values;
- rich units such as bytes, durations and timestamps;
- consistent live-stream semantics;
- remote system interaction;
- documentation generation from command metadata;
- scripting that uses the same object semantics as the interactive shell;
- strong interop boundaries between objects, text and bytes.

### 1.4 What KUANG is not trying to fix

KUANG is not an attempt to remove Unix, replace POSIX, hide the filesystem, redefine every command, or force the existing ecosystem into a new protocol. A shell that cannot comfortably run `git`, `cargo`, `ssh`, `vim`, `ffmpeg`, `kubectl`, `docker`, `grep` and thousands of arbitrary binaries would fail its target users immediately.

Therefore, native KUANG semantics MUST coexist with normal process execution.

# 2. Audience, Positioning and Product Personality

### 2.1 Primary audience

The initial audience is intentionally narrow:

- Unix/Linux power users;
- developers who live in a terminal;
- SREs, infrastructure engineers and observability engineers;
- security-minded systems engineers;
- Rust, Go, C/C++, Python and systems-programming enthusiasts;
- homelab users who enjoy understanding machines rather than merely operating them;
- people for whom trying a new shell is recreation rather than migration work.

This audience is a feature, not a limitation. Early KUANG does not need to win users who are satisfied with the default shell. It needs to win the people who install a new terminal emulator because its rendering pipeline is interesting.

### 2.2 Emotional positioning

KUANG SHOULD evoke:

- precision;
- speed;
- proximity to the machine;
- a sense that state is live, not static;
- a small amount of danger when operating privileged or remote contexts;
- confidence that every visible effect corresponds to real system information.

KUANG MUST NOT evoke:

- fake hacker theatre;
- gamified "breach" language;
- decorative animation that delays work;
- novelty fonts as a functional dependency;
- hidden magic that guesses what a user meant after destructive input;
- a GUI application pretending to be a terminal.

### 2.3 The "show another nerd" test

A core product acceptance criterion:

> At least one interaction in every major capability area SHOULD be sufficiently elegant, direct or visually informative that a technically sophisticated user would plausibly show it to another technically sophisticated user.

Examples:

- process filtering without parsing;
- `watch` transforming a static object query into a live table;
- semantic completion that knows property names and units;
- `inspect` exposing object fields and provenance;
- a remote prompt that clearly communicates where commands execute;
- `trace` showing real relationships between processes, sockets, files and services;
- pipeline history that can reuse the previous structured result.

This criterion is not satisfied by adding color to a conventional command.

# 3. Product Principles

1. **Structured by default, text by choice.** Native commands MUST emit typed values. Rendering is separate from data.
2. **Unix remains underneath.** Arbitrary executables MUST remain trivial to invoke.
3. **Predictable language beats clever abbreviation.** Native commands SHOULD follow a small grammar and a controlled verb registry.
4. **Discoverability is part of speed.** Completion and help SHOULD expose the language rather than merely finish filenames.
5. **No fake intelligence.** The shell MUST NOT infer destructive intent from vague text.
6. **Coolness must be earned by capability.** Motion, color and layout SHOULD reflect actual state changes or structure.
7. **Objects are transparent.** Users MUST be able to inspect exact field names, types, origin and raw values.
8. **Rendering is adaptive.** A value SHOULD render for the available terminal width without changing its semantics.
9. **Pipelines are deterministic.** Transformations SHOULD have explicit and testable rules.
10. **Errors are values with human renderings.** Native command errors MUST be structured.
11. **Interop boundaries are explicit.** Text, bytes and objects MUST not be silently confused.
12. **Local and remote contexts use the same mental model.** The active execution context MUST always be visible.
13. **Danger is visible before damage.** Privilege, remote targets and destructive scope SHOULD be obvious in the prompt and operation preview.
14. **The shell itself is scriptable using its own language.** Interactive and scripted semantics SHOULD not fork unnecessarily.
15. **Metadata should generate tooling.** Command docs, completion, argument validation and test matrices SHOULD derive from a common registry.

# 4. Experience Design: What KUANG Should Feel Like

### 4.1 Startup

Default startup SHOULD be nearly silent. No ASCII logo, no splash sequence, no simulated boot. Optional first-run onboarding MAY exist, but a configured shell should reach the prompt immediately.

Example:

```text
KUANG/11  local  linux/amd64
local://~ >
```

The one-line identifier MAY be disabled. A more austere configuration can start directly at the prompt.

### 4.2 Prompt as a HUD

The prompt MUST communicate execution location and MAY communicate privilege, object context, environment, VCS state and asynchronous activity.

Examples:

```text
local://~ >
prod-db://~ >
prod-db://root:/etc >
prod-db://container/postgres:/var/lib/postgresql >
prod-db://service/postgres >
```

The prompt SHOULD remain short. Information that is not actionable SHOULD not be shown permanently.

Suggested semantic prompt segments:

| Segment | Example | Meaning |
|---|---|---|
| link | `prod-db` | machine or session that will execute native operations |
| privilege | `root` or `!` | elevated identity/capability |
| context | `service/postgres` | selected system object |
| path | `/etc` | filesystem location when meaningful |
| vcs | `git:main*` | optional source-control state |
| jobs | `+2` | background/live jobs |

### 4.3 Color

Default palette SHOULD be mostly neutral. Color is semantic, not decorative.

Recommended roles:

- foreground: normal values and prose;
- dim: metadata, provenance, secondary columns;
- accent: selected object, active link, relationship edges;
- warning: degraded or risky state;
- danger: destructive action, privilege escalation, critical errors;
- success: completed state change when confirmation is useful.

Themes MAY be highly stylized, but the default theme should remain restrained.

### 4.4 Motion

Animation MUST NOT block input. Motion SHOULD only represent:

- values updating;
- relationships appearing/disappearing;
- asynchronous tasks completing;
- a context transition;
- progress that genuinely exists.

Artificial delays are forbidden by design.

### 4.5 Dense, keyboard-first output

Tables SHOULD be compact. Headers SHOULD remain stable. Users SHOULD be able to move a selection through structured output with the keyboard when interactive mode is enabled, while redirected/non-interactive output MUST remain deterministic.

### 4.6 CLI and TUI should blur carefully

KUANG is still a shell. However, output MAY become interactive when stdout is attached to a capable terminal. This must be progressive enhancement:

```text
TTY:      rich table + selection + drill-down
pipe:     stream of structured values
redirect: deterministic serialization or explicit text rendering
script:   no hidden terminal interaction
```

An operation MUST NOT behave differently semantically merely because a table is interactive.

# 5. Core Mental Model

KUANG consists of five conceptual layers:

```text
+-----------------------------------------------------------+
| USER INTERACTION                                          |
| prompt, editor, completion, selection, help, TUI views    |
+------------------------------+----------------------------+
                               |
+------------------------------v----------------------------+
| LANGUAGE                                                   |
| parse -> AST -> evaluate -> pipeline -> control flow      |
+------------------------------+----------------------------+
                               |
+------------------------------v----------------------------+
| VALUE SYSTEM                                                |
| scalar, record, list, stream, table view, error           |
+------------------------------+----------------------------+
                               |
+------------------------------v----------------------------+
| PROVIDERS / NATIVE COMMANDS                                |
| process, file, service, network, user, mount, package ... |
+------------------------------+----------------------------+
                               |
+------------------------------v----------------------------+
| UNIX / EXTERNAL WORLD                                      |
| exec, stdin/stdout/stderr, PTY, signals, filesystem, OS   |
+-----------------------------------------------------------+
```

The shell language MUST not depend on any specific renderer. Providers MUST not format human tables themselves. External programs remain external processes and are connected through explicit adapters.

A native command produces `Value` or `Stream<Value>`. A renderer consumes values only when a human or a text sink requires presentation.

# 6. Language Design Overview

### 6.1 Canonical command shape

The primary KUANG pattern is:

```text
<verb> <target> [selector] [arguments] [options]
```

Examples:

```text
get process
get process 4419
get service nginx
start service nginx
stop service nginx
restart service nginx
get file ./src
remove file ./build.tmp
connect host prod-db
```

The grammar SHOULD prefer words over punctuation when operating on system objects, but MUST preserve concise operators for data expressions.

### 6.2 Pipelines

```text
producer | transform | transform | consumer
```

Examples:

```text
get process | where cpu > 20 | sort cpu desc | take 10
get file ./src --recursive | where size > 1MiB | select path size modified
get socket | where state == established | group remote.host
```

### 6.3 Expressions

Expressions SHOULD support:

- numeric arithmetic;
- string operations;
- typed units;
- booleans;
- comparisons;
- nullability;
- property access;
- list membership;
- regex as an explicit operator/type;
- dates and durations.

Examples:

```text
cpu > 20
memory >= 512MiB
name ~= /postgres|redis/
user in ["root", "postgres"]
modified < now() - 7d
remote.port == 443
```

### 6.4 Selection shorthand

Interactive convenience MAY allow a selected row/object to be referenced via a stable token such as `@selection`. Previous structured results SHOULD be referable without relying on screen scraping.

Potential syntax:

```text
@               current selected value
@-1             previous pipeline result
@3              item 3 of current displayed result
```

This syntax is intentionally marked **open for validation**. The semantics matter more than the exact token.

### 6.5 Native vs external command resolution

Resolution order SHOULD be explicit and inspectable. A suggested default:

1. language keyword / control form;
2. user function or alias;
3. native KUANG command;
4. external executable on `PATH`;
5. "command not found" with discovery suggestions.

Users MUST be able to force a namespace:

```text
kuang:get process
exec:get process
```

Exact spelling may change, but ambiguity MUST be resolvable.

# 7. Verb Registry

KUANG SHOULD maintain a deliberately small, curated verb registry. Third-party modules SHOULD reuse existing verbs whenever semantics match. New verbs MAY be registered, but tooling SHOULD warn about near-duplicates.

### 7.1 Core verbs

| Verb | Semantics | Typical targets | Pipeline role |
|---|---|---|---|
| `get` | obtain current objects/state | process, file, service, socket, user | producer |
| `find` | discover by search criteria/location | file, host, command, package | producer |
| `inspect` | expose detailed representation/provenance | any object | transform/terminal |
| `watch` | repeatedly/live observe | query, process, file, service | stream producer |
| `trace` | derive relationships/path/provenance | process, service, host, packet | producer/view |
| `start` | transition into running/active state | service, job, container | consumer |
| `stop` | transition into stopped/inactive state | service, process, job | consumer |
| `restart` | stop + start with target semantics | service, container | consumer |
| `kill` | deliver a termination signal/forceful stop | process, job | consumer |
| `open` | open resource in associated handler/context | file, URL, device | consumer |
| `enter` | make a resource the active interaction context | dir, container, service | context |
| `leave` | pop active context | context | context |
| `connect` | create a protocol connection | host, socket, database | consumer/context |
| `link` | create/persist a KUANG remote execution link | host/session | context |
| `detach` | leave live or remote attachment without stopping target | link, job, stream | context |
| `add` | create membership/association | user, route, group, repo | mutation |
| `remove` | delete resource/membership | file, route, package | mutation |
| `set` | modify properties/configuration | env, service, file metadata | mutation |
| `move` | relocate object | file, directory | mutation |
| `copy` | duplicate object | file, directory | mutation |
| `rename` | change identity label/name | file, link | mutation |
| `mount` | attach filesystem/resource | filesystem | mutation |
| `unmount` | detach filesystem/resource | filesystem | mutation |
| `send` | emit data/signal/message | signal, packet, notification | consumer |
| `read` | consume content rather than metadata | file, stream, secret | producer |
| `write` | create/replace content | file, stream | consumer |
| `tail` | follow append-oriented content | file, journal | stream |
| `test` | evaluate condition/capability | port, path, permission | producer |
| `resolve` | map name/identifier | dns, user, path, command | producer |
| `format` | explicit human rendering | any value | transform |
| `to` | serialization/conversion | json, yaml, csv, text, bytes | transform |
| `from` | parse explicit representation | json, yaml, csv | transform |
| `select` | project fields | records | transform |
| `where` | filter values | stream/list | transform |
| `sort` | order values | stream/list | transform |
| `group` | group by expression | stream/list | transform |
| `take` | first N | stream/list | transform |
| `skip` | skip first N | stream/list | transform |
| `each` | execute expression/block per value | stream/list | transform |
| `reduce` | fold stream into value | stream/list | transform |
| `count` | count values | stream/list | terminal/transform |
| `measure` | calculate statistics | numeric stream | transform |
| `join` | relational join | records/tables | transform |
| `diff` | compare values/state | file, record, query | producer/view |
| `help` | discover documentation | command, target, topic | meta |
| `explain` | show resolution/plan without execution | command/expression | meta |
| `type` | show type/schema | value/command output | meta |

# 8. Target Registry and Native Command Families

Targets provide semantic nouns/resources. The registry SHOULD be hierarchical where helpful but remain easy to type.

### 8.1 System targets

```text
process
service
job
thread
file
dir
mount
filesystem
device
socket
connection
interface
route
neighbor
dns
host
user
group
session
env
command
package
kernel
module
container
namespace
cgroup
journal
log
port
signal
```

### 8.2 Development targets

Native integration MAY expand to:

```text
repo
branch
commit
worktree
build
test
artifact
project
```

These SHOULD only be native when KUANG can expose meaningful structured semantics without becoming a replacement for specialized tools.

### 8.3 Infrastructure targets

Provider modules MAY add:

```text
pod
node
namespace        # Kubernetes namespace is provider-qualified if ambiguous
vm
image
volume
network
secret
endpoint
database
queue
```

Provider qualification MUST resolve collisions, for example:

```text
get k8s:namespace
get linux:namespace
```

### 8.4 Target design rule

A target SHOULD exist when:

1. the underlying system exposes durable structure;
2. users commonly need to filter or compose that structure;
3. text parsing is currently common or fragile;
4. object identity can be represented clearly;
5. a provider can define stable schemas and capabilities.

# 9. Native Command Catalog

### 9.1 Command family catalog
#### Process
| Command | Semantics | Output |
|---|---|---|
| `get process [selector]` | Enumerate or resolve processes. | `Stream<Process>` |
| `inspect process <pid|object>` | Return detailed process properties and provenance. | `ProcessDetail` |
| `watch process [selector]` | Emit process snapshots/updates as a live stream. | `Stream<ProcessEvent>` |
| `stop process <selector>` | Request graceful termination using default TERM semantics. | `ActionResult` |
| `kill process <selector> [--signal SIGKILL]` | Send explicit signal; forceful semantics must be visible. | `ActionResult` |
| `trace process <selector>` | Show known process relationships: parent/child, files, sockets, cgroup, service. | `Graph` |

#### Service
| Command | Semantics | Output |
|---|---|---|
| `get service [name]` | List or resolve service-manager units through the active provider. | `Stream<Service>` |
| `start service <name>` | Start a service. | `ActionResult` |
| `stop service <name>` | Stop a service. | `ActionResult` |
| `restart service <name>` | Restart a service. | `ActionResult` |
| `watch service [name]` | Emit state transitions. | `Stream<ServiceEvent>` |
| `trace service <name>` | Show dependency/process/socket/file relationships. | `Graph` |
| `enter service <name>` | Push service object into the context stack. | `Context` |

#### Filesystem
| Command | Semantics | Output |
|---|---|---|
| `get file <path/glob>` | Return file metadata records. | `Stream<File>` |
| `find file <root> [predicate]` | Recursive/provider-backed file discovery. | `Stream<File>` |
| `read file <path>` | Return file content as bytes/text according to explicit detection policy. | `Bytes|Text` |
| `write file <path>` | Consume pipeline content and write with explicit overwrite policy. | `ActionResult` |
| `copy file <src> <dst>` | Copy file resources. | `ActionResult` |
| `move file <src> <dst>` | Move resource. | `ActionResult` |
| `remove file <path>` | Delete resource; destructive preview for broad selections. | `ActionResult` |
| `get dir <path>` | List directory entries as File records. | `Stream<File>` |
| `enter dir <path>` | Change filesystem context/cwd. | `Context` |

#### Network
| Command | Semantics | Output |
|---|---|---|
| `get socket` | Enumerate sockets as structured records. | `Stream<Socket>` |
| `get connection` | Enumerate connection-oriented socket views. | `Stream<Connection>` |
| `get interface` | Enumerate interfaces, addresses and state. | `Stream<Interface>` |
| `get route` | Enumerate routes. | `Stream<Route>` |
| `get neighbor` | Enumerate ARP/NDP neighbors. | `Stream<Neighbor>` |
| `resolve dns <name|address>` | Perform explicit DNS resolution. | `Stream<DnsRecord>` |
| `test port <host> <port>` | Probe reachability with timing and error detail. | `ProbeResult` |
| `trace connection <selector>` | Resolve process/service/remote endpoint relationships where possible. | `Graph` |

#### Identity
| Command | Semantics | Output |
|---|---|---|
| `get user [name|uid]` | Enumerate users. | `Stream<User>` |
| `get group [name|gid]` | Enumerate groups. | `Stream<Group>` |
| `get session` | Enumerate local/login/session objects. | `Stream<Session>` |
| `get env [name]` | Return environment values as records. | `Stream<EnvVar>` |
| `set env <name> <value>` | Set environment in current scope. | `ActionResult` |

#### Storage
| Command | Semantics | Output |
|---|---|---|
| `get mount` | Enumerate mounted filesystems. | `Stream<Mount>` |
| `get filesystem` | Enumerate filesystems/devices where provider supports it. | `Stream<Filesystem>` |
| `get device` | Enumerate block/character devices. | `Stream<Device>` |
| `mount filesystem <source> <target>` | Mount with validated options. | `ActionResult` |
| `unmount filesystem <target>` | Unmount/detach. | `ActionResult` |

#### Package
| Command | Semantics | Output |
|---|---|---|
| `get package [name]` | Enumerate installed/known packages via provider. | `Stream<Package>` |
| `find package <query>` | Search provider repositories. | `Stream<Package>` |
| `add package <name>` | Install package. | `ActionResult` |
| `remove package <name>` | Remove package. | `ActionResult` |
| `set package <name> --version <v>` | Request version transition where provider supports it. | `ActionResult` |

#### Container
| Command | Semantics | Output |
|---|---|---|
| `get container` | Enumerate containers through installed provider(s). | `Stream<Container>` |
| `start container <id>` | Start. | `ActionResult` |
| `stop container <id>` | Stop. | `ActionResult` |
| `enter container <id>` | Push execution/container context. | `Context` |
| `get image` | Enumerate images. | `Stream<Image>` |
| `trace container <id>` | Show namespaces, cgroups, mounts, processes, sockets and image relation. | `Graph` |

#### Remote
| Command | Semantics | Output |
|---|---|---|
| `get host` | Enumerate known hosts from configured providers/sources. | `Stream<Host>` |
| `link host <name>` | Create reusable remote KUANG link. | `Link` |
| `get link` | List active links. | `Stream<Link>` |
| `detach link <name>` | Detach active link/context. | `ActionResult` |
| `enter link <name>` | Switch active execution context. | `Context` |
| `test host <name>` | Check reachability/capabilities. | `ProbeResult` |

#### Data
| Command | Semantics | Output |
|---|---|---|
| `where <expr>` | Filter stream values. | `Stream<T>` |
| `select <fields...>` | Project records. | `Stream<Record>` |
| `sort <expr> [asc|desc]` | Order finite input. | `Stream<T>` |
| `group <expr>` | Group finite or windowed stream. | `Stream<Group<T>>` |
| `take <n>` | Take N values. | `Stream<T>` |
| `skip <n>` | Skip N values. | `Stream<T>` |
| `each <block>` | Map values through expression/block. | `Stream<U>` |
| `count` | Count finite input. | `Int` |
| `measure <expr>` | Compute statistics. | `Measure` |
| `to json|yaml|csv|text|bytes` | Explicit serialization. | `Text|Bytes` |
| `from json|yaml|csv` | Explicit parsing. | `Value|Stream<Record>` |

#### Meta
| Command | Semantics | Output |
|---|---|---|
| `help [topic]` | Structured help/discovery. | `HelpPage` |
| `type <expr|value>` | Show type/schema. | `TypeInfo` |
| `inspect <value>` | Detailed field/value/provenance view. | `Inspection` |
| `explain <command>` | Show resolution, provider, coercions, privilege and planned effects. | `ExecutionPlan` |
| `get command [name]` | Discover native/external commands and metadata. | `Stream<Command>` |

# 10. Value and Type System

### 10.1 Goals

The type system must be rich enough to avoid text parsing but small enough to remain understandable at a prompt. It SHOULD favor structural records over an inheritance-heavy class hierarchy.

### 10.2 Core values

```text
Null
Bool
Int
Float
Decimal          optional, for exact decimal arithmetic
String
Bytes
Path
Timestamp
Duration
ByteSize
Regex
Uuid            optional built-in semantic scalar
IpAddress
IpNetwork
Port
List<T>
Map<K,V>
Record
Stream<T>
Error
```

Semantic scalar types are important because `512MiB`, `10s`, `192.0.2.4` and `/etc/passwd` should not be indistinguishable strings if a provider can preserve meaning.

### 10.3 Records

A record is a map-like value with a stable schema and optional semantic type name.

```text
Process {
    pid: Int
    ppid: Int?
    name: String
    executable: Path?
    user: UserRef?
    cpu: Float?
    memory: ByteSize?
    started: Timestamp?
    state: ProcessState
}
```

Records MUST support field access:

```text
process.pid
process.user.name
```

Within pipeline predicates, the current record MAY expose fields directly:

```text
get process | where cpu > 20
```

This is syntactic sugar for a current-value binding, not implicit global variables.

### 10.4 Schema evolution

Provider schemas require compatibility rules. A proposed contract:

- adding an optional field: backward compatible;
- adding a required field: versioned schema change;
- renaming/removing field: breaking change;
- widening numeric representation: usually compatible if lossless;
- changing units/meaning: breaking change;
- provider-specific extensions SHOULD live under a namespaced `extra` record or schema extension mechanism.

### 10.5 Null and absence

KUANG MUST distinguish:

- field absent from schema;
- field present but value unknown/unavailable (`null`);
- field access failed due to permission/provider error.

Silently converting all three into empty strings would recreate the ambiguity KUANG is designed to remove.

### 10.6 Units

Literals SHOULD be first-class:

```text
128B
64KiB
1MiB
3.5GiB
250ms
10s
5m
2h
7d
80port        # probably not this exact syntax; Port may parse from integer context
```

Comparisons convert compatible units automatically and reject incompatible dimensions.

### 10.7 Tables are views, not values

A table is a rendering strategy over a list/stream of records. KUANG MUST NOT make "table" the canonical storage format. This prevents column width, ordering and truncation from leaking into pipeline semantics.

# 11. Pipeline Semantics

### 11.1 Streaming first

Pipelines SHOULD stream values where possible. The engine MUST distinguish operations that can remain streaming from operations requiring bounded input.

Streaming transforms:

```text
where
select
take
skip
each
```

Potentially blocking/finite transforms:

```text
sort
group
join
measure median
```

For unbounded streams, blocking transforms MUST either require a window or reject the operation with a structured error.

### 11.2 Backpressure

Native pipelines MUST implement backpressure. A slow consumer should not allow an infinite producer to exhaust memory. The execution engine SHOULD use bounded asynchronous channels.

### 11.3 Pipeline typing

Where schemas are known, the shell SHOULD type-check field names before execution when possible:

```text
get process | where cpy > 20
```

could produce:

```text
unknown field `cpy` on Process
perhaps: cpu
```

This can happen before process enumeration begins because `get process` advertises `Stream<Process>`.

### 11.4 Heterogeneous streams

Heterogeneous values are allowed but SHOULD be explicit. Operations against fields that do not exist require defined semantics. Default proposal:

- `where field == x`: missing field evaluates to `false` only when explicitly using a safe field lookup; otherwise type/schema error for known homogeneous streams;
- dynamic/heterogeneous streams expose optional access such as `?.`;
- scripts can pattern-match on type.

### 11.5 Consumer acknowledgement

Mutating consumers SHOULD return structured `ActionResult` values rather than only exit codes.

```text
ActionResult {
    target: ValueRef
    operation: String
    status: success | skipped | failed
    changed: Bool
    message: String?
    error: Error?
    duration: Duration
}
```

This enables:

```text
get process | where name == "foo" | stop process | where status == failed
```

### 11.6 Destructive fan-out

When a mutating command receives many objects, KUANG SHOULD calculate scope before execution when input is finite or cheaply enumerable.

Example:

```text
get file /tmp --recursive | where modified < now()-30d | remove file
```

Interactive mode might render:

```text
remove file: 18,421 targets, 3.8 GiB
scope: /tmp
[enter] execute   [p] preview   [esc] cancel
```

Confirmation thresholds MUST be configurable, and scripts MUST have non-interactive semantics that never wait for a TTY prompt unexpectedly.

# 12. Interoperability with Unix Text and Bytes

### 12.1 Principle

KUANG must be a good Unix citizen. External processes receive byte-oriented stdin and produce byte-oriented stdout/stderr. The shell MUST not pretend otherwise.

### 12.2 External command output

Default external stdout enters the value system as `Bytes` or `TextStream` depending on decoding policy. A safe design is:

- retain raw bytes internally;
- expose decoded text when valid under configured encoding;
- never lose undecodable bytes;
- line-oriented transforms MAY operate on `TextStream`.

### 12.3 Object-to-external conversion

An object pipeline cannot be silently sent to an arbitrary process. The user must choose or a command metadata adapter must define the serialization.

Preferred explicit form:

```text
get process | to json | external-tool
```

Potential convenience rule:

- if an external process is the next pipeline stage and input is `Text`/`Bytes`, pass it directly;
- if input is structured, emit an error suggesting `to json`, `to csv`, `format table`, or a registered adapter.

This prevents hidden formatting from becoming API behavior.

### 12.4 External-to-object conversion

Explicit parsing:

```text
curl -s https://example/api | from json | where status == "open"
```

Tools that support a structured output mode can have adapters:

```text
adapter register kubectl --format json ...
```

But adapters MUST be visible through `explain` and MUST not rewrite user commands opaquely.

### 12.5 POSIX pipes and stderr

KUANG MUST retain separate stdout and stderr concepts for external processes. Native commands SHOULD expose errors as structured error streams or result values, but the language must provide bridges for process-oriented semantics.

Potential redirection syntax can remain familiar:

```text
cmd > file
cmd >> file
cmd 2> error.log
```

Structured redirection MUST require a serializer unless the destination is a KUANG-aware sink.

# 13. Rendering and Presentation Engine

### 13.1 Separation of value and presentation

Every native object has:

1. semantic type/schema;
2. raw value;
3. zero or more renderer hints;
4. one or more presentation profiles.

A provider MAY suggest preferred columns, but the renderer owns terminal formatting.

### 13.2 Default table selection

For `Stream<Record>`, the renderer SHOULD choose a compact table when records are homogeneous and terminal width permits. It MAY switch to stacked records for very narrow terminals.

Example wide:

```text
PID   NAME       CPU    MEM      USER
812   postgres   24.8%  1.2 GiB  postgres
```

Example narrow:

```text
PROCESS 812 postgres
cpu      24.8%
mem      1.2 GiB
user     postgres
```

The underlying result is identical.

### 13.3 Truncation

Truncation MUST be visible. Strings may render with an ellipsis-like marker, but copy/export/serialization must retain the full value. Because this spec favors ASCII-safe output, a default textual marker MAY be `...`.

### 13.4 Human formatting

Semantic scalars SHOULD render naturally:

- `ByteSize(1288490188)` -> `1.20 GiB`;
- timestamp -> context-sensitive local time with full value available on inspect;
- duration -> `4d 03h` or `843ms`;
- IPs remain canonical textual representations.

### 13.5 Interactive selection

When stdout is a supported terminal, a rendered collection MAY expose an ephemeral selection cursor. Selection MUST never change pipeline data by itself. Actions based on selection require explicit input such as `enter`, `inspect`, or `@` reference.

### 13.6 Alternate views

Built-in views MAY include:

```text
table
list
tree
graph
json
yaml
raw
hex
```

`graph` is only available for graph/relationship data and MUST not fabricate relationships from visual grouping.

# 14. Context Stack and Navigable Objects

Context is the feature that can make KUANG feel distinct without requiring the entire operating system to become a graph database.

### 14.1 Context stack

The shell maintains a stack of context frames:

```text
Frame {
    kind: local | link | filesystem | object | container | provider
    identity: ValueRef
    capabilities: Set<Capability>
    env_overlay: Map<String,Value>
}
```

`enter` pushes a frame. `leave` pops it.

### 14.2 Filesystem context

`enter dir /etc` is equivalent in effect to changing cwd but uses the same mental model as other contexts.

### 14.3 Object context

```text
local://~ > enter service nginx
local://service/nginx > get process
```

The `service` context provides an implicit selector. `get process` asks the active provider for processes belonging to that service.

If a command is unsupported in a context, KUANG MUST say why rather than silently falling back to global scope.

### 14.4 Remote context

```text
local://~ > link host prod-db
prod-db://~ > get process
```

The active link frame determines where provider calls and external processes execute. The prompt MUST make this unambiguous.

### 14.5 Context is optional

All operations SHOULD remain expressible without entering context:

```text
get process --service nginx
```

Context is an ergonomic tool, not hidden global magic.

# 15. Discovery, Completion and Help

### 15.1 Completion is system exploration

Completion SHOULD be schema-aware, provider-aware and context-aware.

Typing:

```text
get <tab>
```

could show:

```text
process   service   file   socket   interface   route   user   mount ...
```

Typing:

```text
get process | where <tab>
```

should show Process fields:

```text
pid  ppid  name  user  cpu  memory  state  started ...
```

Typing:

```text
memory > 5<tab>
```

MAY suggest byte units rather than unrelated filenames.

### 15.2 Help derives from metadata

`help get process` SHOULD be generated from the command registry and provider metadata. At minimum it contains:

- synopsis;
- description;
- accepted selectors;
- options;
- input type;
- output type;
- privileges/capabilities;
- examples;
- related commands;
- provider source;
- stability level.

### 15.3 Explain mode

`explain` is a signature KUANG capability.

```text
explain get process | where cpu > 20 | stop process
```

Potential output:

```text
1 get process
  provider: linux.procfs
  output: Stream<Process>

2 where cpu > 20
  field: Process.cpu Float
  streaming: yes

3 stop process
  action: signal TERM
  privilege: current user or CAP_KILL
  destructive: yes
  fan-out: dynamic
```

This is valuable for learning, debugging and trust.

### 15.4 Fuzzy command discovery

Unknown native-like commands SHOULD suggest language-level alternatives:

```text
local://~ > list processes
unknown command `list`
Did you mean: get process
```

Suggestions MUST NOT execute automatically.

# 16. Error Model

### 16.1 Errors are structured

```text
Error {
    code: ErrorCode
    message: String
    kind: resolution | permission | io | parse | type | provider | external | conflict | timeout | cancelled
    target: ValueRef?
    source: Error?
    help: String?
    retryable: Bool?
    metadata: Record
}
```

### 16.2 Human rendering

Errors should be terse by default, rich on demand.

```text
access denied: /etc/shadow
requires root or read capability
```

`inspect @error` can reveal errno, provider, operation and causal chain.

### 16.3 Parse errors

Parse errors MUST point at the relevant span and SHOULD offer a concrete correction only when confidence is high.

### 16.4 External exit codes

External process exit status remains available as process metadata and shell status. KUANG MUST not translate arbitrary non-zero exit codes into misleading native error categories without an adapter.

### 16.5 Partial failure

Bulk operations MUST represent partial success. The shell should never collapse `97 succeeded, 3 failed` into a single ambiguous boolean.

# 17. Safety and Privilege Model

### 17.1 Risk visibility

The shell SHOULD calculate a risk descriptor for native mutations based on:

- destructive capability;
- number of targets;
- remote/local context;
- privilege level;
- filesystem boundary;
- irreversibility;
- provider hints.

### 17.2 Root context

Elevation SHOULD be explicit and visible. Possible models:

```text
sudo stop service nginx
```

or a scoped context:

```text
enter root
prod-db://!root:/etc >
```

A persistent elevated context is powerful but risky; if supported, its prompt distinction MUST be impossible to miss while remaining tasteful.

### 17.3 No ambiguous glob destruction

Native commands receive resolved objects where possible. `remove file *.tmp` can know exact targets before mutation. This allows previews and avoids some classes of quoting mistakes.

### 17.4 Script behavior

Scripts MUST never block waiting for interactive confirmation unless explicitly launched in interactive mode. Destructive policy violations should fail with a structured error and require flags/policy declarations.

### 17.5 Secret handling

Secret values SHOULD have a semantic type with redacted default rendering and explicit reveal/copy actions. Secrets MUST NOT accidentally enter history through renderer output.

# 18. Job Control, Live Streams and Watch

### 18.1 Traditional job control remains required

KUANG MUST support foreground processes, background jobs, signals, terminal process groups and PTYs well enough to run normal interactive Unix software.

### 18.2 Native live streams

`watch` converts a finite query or provider subscription into a stream of updates.

```text
watch process --every 1s | where cpu > 20
```

Providers MAY support event-driven updates; otherwise polling is explicit in metadata.

### 18.3 Rendering live values

TTY rendering SHOULD update rows in place when objects have stable identities. A process record keyed by PID can update CPU/memory fields without printing a new table each interval.

Piped mode emits ordinary event/snapshot values:

```text
watch process | to json
```

### 18.4 Background live views

A live query MAY be detached into a job:

```text
watch service nginx &
```

The prompt might show `+1`. `get job` returns structured job objects.

### 18.5 Cancellation

Cancellation must propagate through native pipelines and translate to appropriate signals for external processes.

# 19. Variables, Functions, Blocks and Scripting

### 19.1 Goals

KUANG scripting should feel like the interactive shell, not like a separate programming language bolted beside it. However, it must be deterministic enough for automation.

### 19.2 Variables

Suggested syntax:

```text
let hot = get process | where cpu > 50
$hot | select pid name cpu
```

Whether streams are lazy, materialized or single-consumption MUST be explicit. A safe default is that assigning a stream binds a lazy pipeline object only if it can be replayed; otherwise the language may require `collect`.

### 19.3 Functions

```text
fn hot-processes(limit: Float = 20) -> Stream<Process> {
    get process | where cpu > limit | sort cpu desc
}
```

### 19.4 Blocks

```text
get service | where state == failed | each {
    restart service @
}
```

The current pipeline item token should be explicit.

### 19.5 Control flow

Minimum useful set:

```text
if / else
for
match
try / catch
return
break / continue
```

Shell ergonomics SHOULD avoid turning KUANG into a general-purpose language too early. Complex software should still be written in normal languages.

### 19.6 Modules

Modules declare commands, functions, schemas, providers and render hints. Importing a module MUST NOT silently override core verbs/targets without namespace rules.

### 19.7 Script files

Suggested extension: `.kg` or `.kuang`. This remains an open naming decision.

Scripts SHOULD support a strict mode that disables interactive conveniences and requires explicit coercions.

# 20. History, Sessions and Reuse

### 20.1 History records semantics, not only strings

A history entry SHOULD include:

```text
HistoryEntry {
    id: Uuid
    timestamp: Timestamp
    command_text: String
    cwd: Path
    context: ContextSnapshot
    exit_status: Int?
    duration: Duration
    result_ref: ResultRef?
    mutations: List<ActionSummary>
}
```

### 20.2 Structured result retention

KUANG MAY retain bounded recent structured results in memory and optionally on disk. This allows reuse without parsing terminal text.

Potential interaction:

```text
history --results
@-1 | where memory > 1GiB
```

Retention policy must protect secrets and potentially large values.

### 20.3 Timeline view

A TTY history view MAY group by link/context and show meaningful state changes. It should still be derived from actual history records.

### 20.4 Reproducibility

A copied command must remain plain text and runnable. KUANG MUST not require invisible GUI state for essential semantics.

# 21. Remote Links

Remote links are optional for an initial release but strategically important to the "systems interface" identity.

### 21.1 Why not only SSH?

SSH remains available. A KUANG link adds persistent metadata, provider negotiation and object-aware remote execution.

### 21.2 Link handshake

A link could negotiate:

```text
protocol version
remote OS/arch
KUANG agent present? yes/no
available providers
schema versions
terminal/PTY capabilities
identity and privilege
latency
compression
```

### 21.3 Agentless mode

If no KUANG agent exists remotely, the link MAY fall back to SSH and a limited provider set implemented through standard commands/procfs reads. Fallback MUST be visible because semantics and performance may differ.

### 21.4 Agent mode

A small remote agent can expose native provider calls and typed streams over a versioned protocol.

```text
LOCAL KUANG
    |
    | typed RPC / multiplexed streams
    v
REMOTE kuang-agent
    |
    +-- procfs provider
    +-- systemd provider
    +-- netlink provider
    +-- filesystem provider
```

### 21.5 Security

Remote agent mode MUST use authenticated encryption, explicit host trust and least privilege. It SHOULD be possible to run the agent without root and expose capability-specific elevation only where needed.

# 22. Relationship Graph and `trace`

`trace` is the most cyberpunk-looking major feature, but it must be grounded in real provider data.

### 22.1 Graph model

```text
Node {
    id: ValueRef
    kind: TypeName
    value: RecordSummary
}

Edge {
    from: ValueRef
    to: ValueRef
    relation: String
    direction: directed | undirected
    confidence: exact | inferred
    provider: String
    metadata: Record
}
```

### 22.2 Exact vs inferred

KUANG MUST distinguish exact relationships from inferred ones. Examples:

- process -> open file via `/proc/<pid>/fd`: exact at observation time;
- process -> socket inode: exact at observation time;
- socket -> remote hostname from reverse DNS: derived but exact mapping at query time;
- service -> downstream service inferred from network traffic: inferred.

The UI must not visually imply certainty that the provider does not possess.

### 22.3 Useful traces

```text
trace process 812
trace service nginx
trace socket --port 443
trace file /var/lib/postgresql/... --users
trace connection --remote 10.4.2.11
```

### 22.4 ASCII default

A text renderer should produce useful output everywhere:

```text
nginx.service
+-- owns -> process/921 nginx
|   +-- listens -> tcp/:443
|   +-- reads -> /etc/nginx/nginx.conf
|   +-- writes -> /var/log/nginx/access.log
+-- requires -> network-online.target
```

### 22.5 Interactive graph view

A richer TUI MAY allow keyboard navigation, filtering by relation, expanding nodes and returning selected nodes to the prompt. This is a later experience layer, not a prerequisite for graph semantics.

# 23. Native Linux Provider Architecture

KUANG should avoid implementing native commands by parsing human output from classic tools where stable kernel/system APIs exist.

### 23.1 Process provider

Potential sources:

```text
/proc/<pid>/stat
/proc/<pid>/status
/proc/<pid>/cmdline
/proc/<pid>/exe
/proc/<pid>/fd
/proc/<pid>/cgroup
```

Use procfs parsing directly and carefully. Process identity SHOULD account for PID reuse by including start time where available.

### 23.2 Network provider

Prefer netlink for interfaces, addresses, routes and neighbors. Socket inspection may use procfs/netlink/eBPF depending on privileges and required fidelity.

### 23.3 Service provider

Use systemd D-Bus APIs where available rather than shelling out to `systemctl` and parsing text.

### 23.4 Filesystem provider

Use standard syscalls (`stat`, directory iteration, xattrs) and platform-specific APIs. File object identity should distinguish path from inode identity where relevant.

### 23.5 Mount/provider

Read mount information via `/proc/self/mountinfo` or equivalent APIs. Preserve structured mount options and propagation metadata where possible.

### 23.6 Identity provider

NSS lookups can be blocking and network-backed. The provider architecture SHOULD allow asynchronous resolution and represent unresolved IDs without discarding numeric identity.

### 23.7 Provider traits

Illustrative Rust-like interface:

```rust
trait Provider {
    fn id(&self) -> ProviderId;
    fn capabilities(&self) -> CapabilitySet;
    fn schemas(&self) -> &[Schema];
}

trait ProcessProvider: Provider {
    async fn list(&self, query: ProcessQuery) -> Result<Stream<Process>>;
    async fn signal(&self, targets: Vec<ProcessRef>, signal: Signal)
        -> Result<Stream<ActionResult>>;
    async fn trace(&self, target: ProcessRef, options: TraceOptions)
        -> Result<Graph>;
}
```

The real implementation should avoid trait shapes that make dynamic streaming awkward, but the semantic separation is important.

# 24. Engine Architecture in Rust

Rust is a natural implementation language because KUANG requires low-level Unix integration, async I/O, safe concurrency, a parser, PTY/job-control work and high performance.

### 24.1 High-level components

```text
kuang
|
+-- editor             line editing, keymap, syntax highlight
+-- parser             lexer, grammar, AST
+-- semantic           name resolution, type/schema checks
+-- evaluator          expressions, variables, control flow
+-- pipeline           streaming execution, backpressure
+-- values             dynamic Value model + schemas
+-- commands           native command registry
+-- providers          OS/service/container integrations
+-- process            external exec, PTY, signals, jobs
+-- render             table/list/tree/graph/json/raw
+-- context            cwd, link, object stack, privilege
+-- history            semantic history/result cache
+-- completion         metadata-driven completion
+-- module             extension loading/registry
+-- protocol           remote agent typed transport
+-- theme              semantic presentation tokens
+-- diagnostics        errors, explain plans, tracing
```

### 24.2 Suggested workspace layout

```text
kuang/
+-- Cargo.toml
+-- crates/
|   +-- kuang-cli/
|   +-- kuang-core/
|   +-- kuang-parser/
|   +-- kuang-value/
|   +-- kuang-pipeline/
|   +-- kuang-command/
|   +-- kuang-provider-api/
|   +-- kuang-provider-linux/
|   +-- kuang-provider-systemd/
|   +-- kuang-provider-netlink/
|   +-- kuang-render/
|   +-- kuang-editor/
|   +-- kuang-process/
|   +-- kuang-history/
|   +-- kuang-protocol/
|   +-- kuang-agent/
|   +-- kuang-testkit/
+-- spec/
|   +-- commands.yaml
|   +-- verbs.yaml
|   +-- schemas/
|   +-- errors.yaml
|   +-- grammar.ebnf
+-- docs/
+-- tests/
+-- examples/
```

### 24.3 Why separate spec registries from code

The registries can generate or validate:

- command help pages;
- completion trees;
- docs tables;
- parser keywords;
- option validation metadata;
- stable command IDs;
- provider conformance tests;
- JSON schema / TypeScript bindings for tooling;
- remote protocol schema IDs;
- test fixtures.

Code remains authoritative for behavior, but metadata defines the public contract and can be checked against implementations.

### 24.4 Parser

A hand-written parser, `chumsky`, `pest`, `lalrpop` or another Rust parser approach are all plausible. The important requirement is excellent error spans and incremental parse support for interactive editing.

The editor needs partial parsing while the user types. Therefore the grammar SHOULD be designed for recoverability, not merely batch correctness.

### 24.5 Async runtime

A runtime such as Tokio is plausible for:

- native stream pipelines;
- remote links;
- provider subscriptions;
- DNS/network I/O;
- background jobs;
- renderer updates.

External foreground processes still require careful Unix terminal process-group handling outside the naive "everything is async tasks" model.

# 25. Internal Value Representation

A dynamic shell needs runtime values without giving up type information.

Illustrative representation:

```rust
enum Value {
    Null,
    Bool(bool),
    Int(i128),
    Float(f64),
    String(Arc<str>),
    Bytes(Bytes),
    Path(PathBuf),
    Timestamp(DateTime),
    Duration(Duration),
    ByteSize(u128),
    Ip(IpAddr),
    List(Arc<[Value]>),
    Map(Arc<MapValue>),
    Record(Arc<RecordValue>),
    Error(Arc<ErrorValue>),
}
```

Streams SHOULD be execution-layer objects rather than ordinary clonable `Value` variants unless their consumption semantics are extremely clear.

### 25.1 Record representation

```rust
struct RecordValue {
    schema: SchemaId,
    fields: SmallVec<[Value; N]>,
    provenance: Provenance,
}
```

Indexing fields by schema position can be significantly cheaper than a hash map per process record. A dynamic extension map can cover provider-specific extras.

### 25.2 Provenance

Provenance makes `inspect` and `explain` trustworthy:

```text
provider     linux.procfs
observed     2026-08-26T08:13:04.182+02:00
source       /proc/4419/status + /proc/4419/stat
link         local
schema       kuang.process/1
```

Not every field needs per-field provenance initially; record-level provenance is a reasonable baseline.

# 26. Grammar Sketch

This is intentionally a sketch, not a frozen grammar.

```ebnf
program        = statement* ;
statement      = pipeline terminator
               | assignment terminator
               | function_decl
               | control_stmt ;

pipeline       = command ("|" command)* ;

command        = native_command
               | external_command
               | transform_command
               | block_invocation ;

native_command = verb target argument* option* ;
verb           = IDENT ;
target         = IDENT | qualified_ident ;

argument       = expression ;
option         = "--" IDENT ("=" expression | expression)? ;

expression     = logical_or ;
logical_or     = logical_and ("or" logical_and)* ;
logical_and    = comparison ("and" comparison)* ;
comparison     = additive (("==" | "!=" | ">" | ">=" | "<" | "<=" | "in" | "~=") additive)* ;
additive       = multiplicative (("+" | "-") multiplicative)* ;
multiplicative = unary (("*" | "/" | "%") unary)* ;
unary          = ("not" | "-") unary | primary ;
primary        = literal
               | variable
               | field_path
               | list
               | record
               | call
               | "(" expression ")" ;

field_path     = IDENT ("." IDENT)* ;
variable       = "$" IDENT ;
qualified_ident = IDENT ":" IDENT ;
```

### 26.1 Command/expression ambiguity

Shell grammars are difficult because bare words are convenient. KUANG SHOULD minimize quoting burden but avoid context rules that make code impossible to reason about. Native command arguments can parse identifiers as strings/selectors in declared positions while expressions after transforms such as `where` use expression grammar.

### 26.2 Strings

Quoted strings MUST have clear escaping. Bare tokens MAY be accepted as string-like command arguments where command metadata declares them, e.g. `get service nginx`, but expression strings SHOULD require quotes when ambiguity exists.

# 27. Command Metadata as Source of Truth

A key design goal is that public command contracts are data.

Example conceptual registry:

```yaml
id: kuang.process.get
verb: get
target: process
summary: Enumerate processes
stability: stable
input: null
output: stream<kuang.process/1>
provider_capability: process.list
selectors:
  - name: pid
    type: int
  - name: name
    type: string
options:
  - name: user
    type: user-ref
    repeatable: false
  - name: tree
    type: bool
privilege: none
streaming: true
examples:
  - get process
  - get process | where cpu > 20
```

### 27.1 Generated artifacts

From such registry files, the repository SHOULD automatically produce:

```text
command reference markdown
shell completion metadata
`help` pages
website reference pages
parser keyword list
stable command IDs
provider capability matrix
snapshot tests
CLI syntax diagrams
machine-readable JSON export
```

### 27.2 Implementation binding

Native code registers an implementation against a stable command ID. CI verifies that every stable registry command has an implementation for at least one provider or is explicitly provider-dependent.

### 27.3 Schema registries

Object types use separate schema definitions. Example:

```yaml
id: kuang.process/1
name: Process
identity: [pid, started]
fields:
  pid: {type: int, required: true}
  ppid: {type: int, nullable: true}
  name: {type: string, required: true}
  cpu: {type: float, unit: percent, nullable: true}
  memory: {type: bytesize, nullable: true}
  user: {type: ref<kuang.user/1>, nullable: true}
default_view:
  columns: [pid, name, cpu, memory, user]
```

This makes documentation and rendering behavior derivable without forcing providers to format text.

# 28. Canonical Object Schemas

The following schemas define an initial conceptual surface. Names and fields can evolve, but this level of explicitness should exist before implementation spreads across providers.

### 28.1 Process

```text
Process
identity: pid + started
fields:
  pid           Int
  ppid          Int?
  name          String
  command       List<String>?
  executable    Path?
  user          UserRef?
  group         GroupRef?
  state         Enum
  cpu           Float?        # percent of one logical CPU unless documented otherwise
  memory        ByteSize?
  virtual_mem   ByteSize?
  threads       Int?
  started       Timestamp?
  cwd           Path?
  service       ServiceRef?
  container     ContainerRef?
```

### 28.2 File

```text
File
identity: device + inode when available; path is a reference, not always identity
fields:
  path          Path
  name          String
  kind          file|dir|symlink|socket|fifo|device|other
  size          ByteSize?
  owner         UserRef?
  group         GroupRef?
  mode          PermissionMode?
  modified      Timestamp?
  accessed      Timestamp?
  created       Timestamp?
  inode         Int?
  device        DeviceRef?
  target        Path?          # symlink target
```

### 28.3 Service

```text
Service
identity: provider + unit/name
fields:
  name          String
  description   String?
  state         Enum
  substate      String?
  pid           Int?
  enabled       Bool?
  since         Timestamp?
  provider      String
  unit_file     Path?
```

### 28.4 Socket

```text
Socket
identity: provider-specific socket ref/inode
fields:
  protocol      Enum
  family        Enum
  local         Endpoint?
  remote        Endpoint?
  state         Enum?
  process       ProcessRef?
  user          UserRef?
  inode         Int?
```

### 28.5 Interface

```text
Interface
fields:
  name          String
  index         Int
  mac           String?
  state         Enum
  mtu           Int
  addresses     List<IpNetwork>
  rx_bytes      ByteSize?
  tx_bytes      ByteSize?
```

### 28.6 Mount

```text
Mount
fields:
  source        String
  target        Path
  filesystem    String
  options       List<String>
  read_only     Bool
  device        DeviceRef?
```

### 28.7 User

```text
User
identity: uid
fields:
  uid           Int
  name          String?
  primary_group GroupRef?
  home          Path?
  shell         Path?
  gecos         String?
```

### 28.8 ActionResult

ActionResult was defined earlier and SHOULD be used consistently for mutations.

# 29. External Command Execution and PTY Requirements

A shell lives or dies by external program compatibility.

KUANG MUST support:

- foreground execution;
- pipelines of external programs;
- mixed native/external pipelines with explicit conversion;
- stdout/stderr redirection;
- PTY allocation for interactive programs;
- signal forwarding;
- terminal resize propagation;
- job control (`fg`, `bg`, suspension semantics or equivalent);
- environment inheritance;
- current directory;
- exit status;
- shell scripts/shebang execution strategy;
- command substitution if included in the language.

### 29.1 Native -> external example

```text
get file . --recursive | select path | to text --field path | xargs wc -l
```

This works, but KUANG SHOULD also provide native consumers where the operation is common enough to avoid lossy conversion.

### 29.2 External -> native example

```text
journalctl -o json | from json | where PRIORITY <= 3
```

### 29.3 Interactive binaries

When the command resolves to an external binary and it owns the foreground terminal, KUANG's rich renderer MUST get out of the way completely. `vim` should behave like `vim`, not like content inside a Kuang widget.

# 30. Configuration

Configuration should be declarative, layered and inspectable.

Potential locations:

```text
/etc/kuang/config.kg
~/.config/kuang/config.kg
~/.config/kuang/themes/*.toml
~/.config/kuang/modules/
```

Suggested configuration domains:

```text
prompt
theme
history
render
completion
safety
links
providers
aliases
functions
keymap
interop
```

Example conceptual config:

```text
set config prompt.path = "smart"
set config render.table.max_rows = 200
set config history.result_cache = 64MiB
set config safety.confirm.remote_destructive = true
set config safety.confirm.bulk_threshold = 100
```

A `get config` command SHOULD return structured configuration with source/provenance so the user can see which file or environment variable set each value.

# 31. Plugin and Provider Model

### 31.1 Extension categories

KUANG extensions MAY provide:

- native commands;
- targets;
- provider capabilities;
- object schemas;
- render hints/views;
- completion sources;
- remote protocol handlers;
- adapters for external commands.

### 31.2 Safety boundary

Native dynamic plugins loaded into the shell process can crash or compromise it. A long-term architecture SHOULD consider out-of-process plugins over a typed protocol, especially for third-party extensions.

Possible model:

```text
kuang core
   |
   +-- built-in trusted providers (in-process)
   |
   +-- plugin host protocol
           |
           +-- k8s provider
           +-- cloud provider
           +-- database provider
```

### 31.3 Capability declaration

Plugins MUST declare capabilities and schemas before serving queries. `get provider` SHOULD make active plugins and trust level visible.

### 31.4 Versioning

Provider API, object schema and remote protocol versioning MUST be independent. This avoids coupling a schema field addition to a full shell protocol break.

# 32. The Cyberpunk Layer Without Cosplay

The aesthetic layer is deliberately defined after semantics and architecture.

### 32.1 What is allowed

- terse system vocabulary such as `link`, `trace`, `enter`, `detach` where the term accurately describes behavior;
- live tables whose movement comes from real events;
- graph views showing real relationships;
- subtle latency/status indicators for remote links;
- a prompt that feels like a location URI;
- dark, restrained themes;
- compact status banners for negotiated links;
- direct manipulation of selected objects;
- fast transitions between object views.

### 32.2 What is forbidden by default

- "ACCESS GRANTED" for normal successful commands;
- fake scanning progress;
- random glitches;
- Matrix rain;
- mandatory boot animation;
- fake hexadecimal noise;
- artificial keystroke delays;
- fake security terminology for ordinary filesystem operations;
- sound effects;
- failure messages written like a video game.

### 32.3 Why this matters

The target audience understands systems. The shell becomes cooler as it reveals more of the real machine, not as it invents fiction around it.

### 32.4 A useful design phrase

> **The machine is already strange enough. Reveal it. Do not decorate it.**

# 33. Interaction Sketches

### 33.1 Process investigation

```text
local://~ > get process | where memory > 1GiB | sort memory desc

PID    PROCESS      CPU    MEM       USER
4419   rustc        92.4%  3.8 GiB   masl
812    postgres     18.1%  1.2 GiB   postgres

local://~ > inspect @1

PROCESS/4419 rustc
pid          4419
parent       4381 cargo
user         masl
cpu          92.4%
memory       3.8 GiB
started      00:02:13 ago
service      -
cgroup       /user.slice/...
open_files   127
sockets      3

local://~ > trace @1
rustc/4419
+-- parent -> cargo/4381
+-- children -> rustc/4420 ...
+-- reads -> ./src/... (83)
+-- writes -> ./target/... (41)
+-- sockets -> 3
```

### 33.2 Service failure

```text
prod-web://~ > get service | where state == failed

SERVICE             STATE   SINCE       DETAIL
image-worker         failed  4m 12s      exit 1

prod-web://~ > inspect @1
...

prod-web://~ > get log --service @1 | tail 30
...

prod-web://~ > restart service @1
restart image-worker  success  412ms
```

### 33.3 Network exploration

```text
local://~ > get socket | where state == established | group process.name

GROUP       COUNT
chrome      42
code        11
ssh         3
postgres    2

local://~ > get socket | where process.name == "ssh"
...
```

### 33.4 Remote link

```text
local://~ > link host prod-db
LINK prod-db
address      10.4.2.11
latency      12ms
transport    ssh+kuang/1
agent        0.1.0
providers    process systemd netlink fs

prod-db://~ >
```

### 33.5 Structured serialization

```text
prod-db://~ > get process | where user.name == "postgres" | to json --pretty
[
  {
    "pid": 812,
    "name": "postgres",
    "cpu": 18.1,
    "memory": 1288490188,
    "user": {"uid": 113, "name": "postgres"}
  }
]
```

Serialization uses canonical values, not display strings such as `1.2 GiB`, unless a human-format option is explicitly requested.

# 34. Performance Requirements

KUANG is an interactive shell; latency is product quality.

Initial target budgets:

| Operation | Target |
|---|---:|
| cold shell startup on modern Linux | < 50 ms aspirational; < 100 ms acceptable |
| warm prompt startup | < 30 ms |
| keystroke-to-render editor latency | < 8 ms typical |
| completion first results | < 50 ms local metadata; async expansion allowed |
| parse/highlight update | < 5 ms for ordinary command lines |
| native `get process` first rows | < 50 ms on typical workstation |
| pipeline per-value overhead | low enough that common system queries feel instantaneous |
| renderer frame update | 60 Hz capable, but only update when state changes |

Performance tests SHOULD include pathological environments: tens of thousands of processes/paths, slow NSS, high-latency links, huge stdout and unbounded streams.

Startup MUST avoid eagerly loading every plugin or querying network-backed configuration.

# 35. Testing Strategy

### 35.1 Parser tests

- golden AST tests;
- invalid syntax diagnostics snapshots;
- incremental/partial parse tests;
- fuzzing;
- quoting/escaping corpus;
- compatibility cases for external command invocation.

### 35.2 Value and pipeline tests

- property access;
- null semantics;
- unit arithmetic;
- backpressure;
- cancellation;
- finite/unbounded behavior;
- type errors before execution;
- serialization round trips.

### 35.3 Provider conformance

Every provider capability gets a generated conformance suite from registry metadata. For process provider, for example:

```text
identity is stable within process lifetime
pid is required and positive
name is non-null
unknown memory is null, not zero
permission failure is represented, not fabricated
```

### 35.4 Integration tests

Use container/VM fixtures for:

- process trees;
- systemd services;
- sockets;
- namespaces/cgroups;
- files with unusual names/encodings;
- permission boundaries;
- PTY applications;
- signals and job control.

### 35.5 Snapshot tests

Renderer output can use terminal-width snapshot tests, but snapshots MUST test presentation only, not become data contracts.

### 35.6 Fuzzing and security

Fuzz parser, serializers, remote protocol, plugin protocol and procfs/netlink decoders. A shell consumes adversarial filenames and external output by nature.

# 36. Documentation and Automatic Derivation Pipeline

This section directly addresses the goal that this specification can become the basis for generating the rest of the project.

### 36.1 Source hierarchy

Proposed authority order:

```text
1. spec/grammar.ebnf              syntax contract
2. spec/verbs.yaml                controlled vocabulary
3. spec/commands.yaml             command public contracts
4. spec/schemas/*.yaml            object contracts
5. spec/errors.yaml               stable error taxonomy
6. spec/providers/*.yaml          provider capabilities
7. implementation code            behavioral implementation
8. generated docs/tests/bindings  derived artifacts
```

The narrative spec - this document - explains intent and semantics. Machine-readable registries become executable contracts.

### 36.2 Generated docs

A generator can produce:

```text
docs/reference/get-process.md
website command reference
`help` payloads
completion trie
man pages
schema reference
provider matrix
examples index
```

### 36.3 Generated tests

For every command:

```text
parses synopsis examples
required arguments validated
option types validated
input/output schema IDs resolve
provider capability exists
help example syntax parses
serialization examples match schema
```

### 36.4 Generated code

Selective codegen is useful for:

- schema IDs and field indices;
- protocol enums;
- command IDs;
- completion descriptors;
- documentation structs;
- TypeScript/JSON schema bindings for editor integrations.

Provider business logic should remain hand-written.

### 36.5 CI contract drift

CI SHOULD fail if:

- implementation registers an undocumented stable command;
- stable command metadata has no implementation/provider path;
- docs examples stop parsing;
- schema-breaking changes occur without a version bump;
- a core verb is introduced without registry review;
- provider output fails its advertised schema.

# 37. Implementation Sequence - Complete Product, Not MVP Thinking

The following phases are not "MVP then maybe finish it". They are a dependency-respecting path toward the complete product described here. Every phase should be engineered as production-quality infrastructure for later phases.

### Phase A - Language and Unix shell foundation

Deliver:

- parser and AST;
- editor integration;
- external command execution;
- quoting/escaping;
- environment variables;
- cwd;
- redirection;
- external pipelines;
- exit status;
- signals and job control;
- configuration and history foundations.

Success criterion: KUANG can replace Bash for ordinary interactive execution without native object features yet becoming a dead end.

### Phase B - Value system and native pipelines

Deliver:

- Value model;
- Stream engine;
- backpressure;
- `where`, `select`, `sort`, `take`, `skip`, `each`, `count`, `measure`;
- JSON/YAML/CSV/text conversion;
- renderer separation;
- structured error model.

Success criterion: synthetic and parsed structured data can flow end to end.

### Phase C - Linux core providers

Deliver high-quality:

```text
process
file/dir
user/group
env
mount/filesystem
interface/route/neighbor
socket/connection
service(systemd)
```

Success criterion: common inspection tasks no longer require parsing text.

### Phase D - Language consistency and discoverability

Deliver:

- command/verb/schema registries;
- metadata-driven help;
- semantic completion;
- `type`;
- `inspect`;
- `explain`;
- generated docs;
- provider conformance tests.

Success criterion: a new user can discover capabilities from the shell itself.

### Phase E - Contextual systems interface

Deliver:

- context stack;
- `enter`/`leave`;
- object-aware implicit selectors;
- improved prompt/HUD;
- interactive table selection;
- structured recent-result reuse.

Success criterion: objects can be investigated without constantly restating selectors.

### Phase F - Live system semantics

Deliver:

- `watch`;
- event/snapshot model;
- interactive in-place rendering;
- native background jobs;
- stable object identity handling.

### Phase G - Relationship graph

Deliver:

- graph value type;
- exact relationship providers;
- `trace process/service/socket`;
- tree/graph renderers;
- provenance/confidence model.

### Phase H - Remote links

Deliver:

- remote protocol;
- agent;
- SSH fallback;
- provider negotiation;
- security model;
- remote context prompt;
- multiplexed streams.

### Phase I - Extension ecosystem

Deliver:

- plugin protocol;
- provider SDK;
- module signing/trust story if desired;
- package/discovery mechanism;
- external command adapter API.

### Phase J - Advanced TUI views

Deliver only where semantics justify them:

- navigable graphs;
- multi-pane inspect/watch views;
- timeline/history exploration;
- object pickers;
- remote-link overview.

At this point the "cyberspace" feeling emerges from actual system data rather than from a theme.

# 38. Explicit Non-Goals

KUANG SHOULD NOT initially attempt to:

- be POSIX shell syntax compatible;
- execute arbitrary Bash scripts unchanged;
- replace coreutils as a compatibility layer;
- infer schemas from arbitrary text automatically;
- become a terminal emulator;
- become an IDE;
- embed an LLM in the command execution path;
- reinterpret natural-language commands without explicit mode/confirmation;
- ship a fake 3D cyberspace visualization;
- replace `git`, `kubectl`, `docker`, package managers or editors wholesale;
- support every operating system equally from day one;
- hide provider differences that materially affect semantics.

Linux-first is a reasonable starting point because the richest object/provider story depends on Linux APIs such as procfs, netlink, systemd, namespaces and cgroups. Portability can be designed at the provider boundary.

# 39. Open Design Questions

The following questions should be answered with prototypes and user testing among the target audience:

1. Should native commands be `get process` or `process get`, and is verb-target truly faster in daily use?
2. Should verbs allow aliases (`ls` -> `get file`) by default, or does that weaken the language identity?
3. What is the cleanest current-item syntax in pipelines and interactive selection?
4. How much implicit string parsing is acceptable in command arguments?
5. Should `enter service nginx` affect cwd/environment or only provider scope?
6. How should structured values cross into external commands with the least surprise?
7. Is `TextStream` line-oriented or chunk-oriented, and how are bytes preserved?
8. Should schemas be nominal, structural, or nominal-with-structural projection?
9. How should provider-specific fields be exposed without schema chaos?
10. What should the native scripting syntax look like for command substitution?
11. Should there be a stable shell protocol for editor/IDE integrations?
12. How much history result data may be persisted safely?
13. Is a remote agent acceptable to the initial audience, or should SSH-only mode lead?
14. What relationships can `trace` expose without root/eBPF?
15. How should multi-provider ambiguity be represented (`docker:container`, `podman:container`)?
16. What is the minimal controlled verb set that still feels expressive?
17. Should destructive operations default to plural-safe behavior when fed a stream?
18. How should table selection work without stealing terminal scrollback behavior?
19. What exact prompt URI syntax looks cool without becoming noisy?
20. What is the stable project name after trademark/domain/repository clearance?

# 40. Repository Governance and Language Discipline

A coherent shell can become incoherent through contributions faster than through original design. KUANG therefore needs language governance.

### 40.1 Verb review

Any new core verb proposal must answer:

- Why can no existing verb represent the semantics?
- What targets use it?
- Is it a producer, transform, mutation, context action or meta command?
- Does the word have a single understandable meaning?
- How does it compose in a pipeline?
- What is its inverse, if any?

### 40.2 Target review

A target should represent a domain object, not a command implementation detail.

### 40.3 Schema review

Fields are API. Once stable, names and meanings should be treated like public library interfaces.

### 40.4 Compatibility promise

A future compatibility policy might state:

- stable command IDs do not change semantics within a major version;
- schema version identifiers make breaking changes explicit;
- scripts can declare required KUANG language level/provider capabilities;
- deprecations are machine-readable and shown by help/linting, not surprise breakage.

# 41. Example End-to-End Workflows

### 41.1 "What is eating memory?"

```text
get process
| where memory > 500MiB
| sort memory desc
| select pid name memory cpu user
```

Then investigate the top result:

```text
inspect @1
trace @1
```

No field positions, no `awk`, no `grep` false positives.

### 41.2 "Which processes are listening externally?"

```text
get socket
| where state == listen
| where local.address not in [127.0.0.1, ::1]
| select protocol local process user
```

### 41.3 "Large old files"

```text
find file /var/log
| where size > 100MiB and modified < now()-30d
| sort size desc
| select path size modified owner
```

To remove after inspection:

```text
@-1 | remove file
```

Interactive KUANG previews the finite target set according to safety policy.

### 41.4 "Failed services and their last errors"

```text
get service
| where state == failed
| each {
    {
        service: @,
        errors: get log --service @ | where level >= error | take 20
    }
}
```

Exact block/record syntax is subject to grammar refinement, but the desired semantic composition is clear.

### 41.5 "Export current network state"

```text
{
  interfaces: get interface,
  routes: get route,
  neighbors: get neighbor,
  sockets: get socket
} | to json > network-state.json
```

### 41.6 "Observe a deployment host"

```text
link host prod-web-3
watch service nginx &
watch process --service nginx &
get job
```

The user retains one shell while multiple structured watches run as jobs.

# 42. Example `explain` Plans

### 42.1 Safe read pipeline

Input:

```text
explain get process | where memory > 1GiB | sort memory desc | take 10
```

Plan:

```text
PIPELINE
1. get process
   command      kuang.process.get
   provider     linux.procfs
   output       stream<kuang.process/1>
   streaming    yes
   privilege    none

2. where memory > 1GiB
   input        kuang.process/1
   field        memory: ByteSize?
   operation    comparison ByteSize > ByteSize
   streaming    yes

3. sort memory desc
   buffering    entire finite input
   memory       proportional to process count

4. take 10
   output       stream<kuang.process/1>
```

### 42.2 Destructive remote plan

```text
explain get process --user app | stop process
```

while connected to `prod-app`:

```text
EXECUTION CONTEXT
link          prod-app (remote)
identity      deploy
provider      linux.procfs

MUTATION
operation     signal TERM
targets       dynamic Stream<Process>
risk          destructive + remote + fan-out
privilege     owner or CAP_KILL
confirmation  required by interactive policy
```

This turns safety and semantics into inspectable product behavior.

# 43. Proposed Error Taxonomy

Stable machine-readable errors make scripts and providers composable.

```text
KUANG-E0001 parse.syntax
KUANG-E0002 parse.incomplete
KUANG-E0101 resolve.command_not_found
KUANG-E0102 resolve.target_not_found
KUANG-E0103 resolve.ambiguous
KUANG-E0201 type.mismatch
KUANG-E0202 type.unknown_field
KUANG-E0203 type.invalid_unit
KUANG-E0301 io.not_found
KUANG-E0302 io.permission_denied
KUANG-E0303 io.already_exists
KUANG-E0304 io.not_directory
KUANG-E0401 provider.unavailable
KUANG-E0402 provider.unsupported
KUANG-E0403 provider.schema_violation
KUANG-E0501 external.exit_nonzero
KUANG-E0502 external.signal
KUANG-E0601 remote.unreachable
KUANG-E0602 remote.protocol_mismatch
KUANG-E0603 remote.host_key_changed
KUANG-E0701 safety.confirmation_required
KUANG-E0702 safety.policy_denied
KUANG-E0801 stream.unbounded_operation
KUANG-E0802 stream.cancelled
KUANG-E0803 stream.backpressure_timeout
```

Codes should remain stable even if human messages improve.

# 44. Theme and Visual Token Specification

Themes should operate on semantic tokens, not hard-coded command colors.

```text
ui.fg
ui.dim
ui.accent
ui.success
ui.warning
ui.danger
ui.border
ui.selection
ui.prompt.link
ui.prompt.context
ui.prompt.root
ui.table.header
ui.table.key
ui.value.string
ui.value.number
ui.value.unit
ui.value.null
ui.error.code
ui.error.hint
ui.graph.node
ui.graph.edge
ui.graph.edge_inferred
```

The default "Kuang" theme should be dark, restrained and legible. A cyberpunk theme may use accent colors more aggressively, but semantic contrast and accessibility remain requirements.

No functionality may depend on color alone.

# 45. Definition of "Kuang-like"

This checklist can be applied in design review.

A feature is strongly Kuang-like if most statements are true:

- it preserves real structure rather than formatting text;
- its command fits the controlled vocabulary;
- it is useful in a pipeline;
- it exposes types and provenance;
- it has deterministic non-interactive behavior;
- it becomes richer on a TTY without changing semantics;
- it reduces memorization;
- it makes system state more legible;
- it looks satisfying because of real information density;
- it works locally and can plausibly work through a provider remotely;
- it can be documented/generated from metadata;
- failures are structured and inspectable.

A feature is weakly Kuang-like or anti-Kuang if:

- it exists primarily for visual effect;
- it adds a one-off syntax rule;
- it hides a text parser behind an object facade without declaring fragility;
- it invents an object type with no stable semantics;
- it only works interactively and cannot be scripted;
- it silently guesses conversions;
- it makes Unix interoperability harder;
- it increases command vocabulary without improving conceptual coverage.

# 46. Machine-Derivable Deliverables

Once the machine-readable registries exist, a project bootstrap generator should be able to derive most scaffolding automatically.

### 46.1 From `commands.yaml`

Generate:

```text
Rust command registration stubs
help pages
completion metadata
reference docs
syntax examples tests
command ID constants
provider capability references
```

### 46.2 From `schemas/*.yaml`

Generate:

```text
Rust field ID constants
schema descriptors
JSON Schema
TypeScript interfaces
render default-column descriptors
provider validation tests
serialization fixtures
remote protocol schema lookup
```

### 46.3 From `grammar.ebnf`

Generate or validate:

```text
parser tests
syntax diagrams
editor tokenization fixtures
syntax highlighting grammar hints
language reference sections
```

### 46.4 From `errors.yaml`

Generate:

```text
error code enum
reference docs
localized message keys if localization is added
client protocol error mappings
```

### 46.5 From examples

Every fenced `kuang`/`text` command example in normative docs SHOULD be extractable into a corpus. CI can parse every example and optionally run marked-safe examples inside fixtures.

This turns documentation into executable design material instead of prose that drifts away from code.

# 47. Suggested Initial Machine-Readable Files

The repository can be bootstrapped with the following contracts before major implementation begins:

```text
spec/
  language.yaml
  grammar.ebnf
  verbs.yaml
  targets.yaml
  errors.yaml
  capabilities.yaml
  commands/
    process.yaml
    service.yaml
    file.yaml
    network.yaml
    identity.yaml
    storage.yaml
    data.yaml
    meta.yaml
  schemas/
    process.v1.yaml
    service.v1.yaml
    file.v1.yaml
    socket.v1.yaml
    interface.v1.yaml
    route.v1.yaml
    user.v1.yaml
    mount.v1.yaml
    action-result.v1.yaml
    error.v1.yaml
    graph.v1.yaml
  providers/
    linux-procfs.yaml
    linux-netlink.yaml
    systemd.yaml
```

A `xtask` or dedicated `kuang-specgen` binary can validate and generate derived artifacts.

# 48. A More Concrete Parser-to-Execution Walkthrough

Given:

```text
get process | where cpu > 20 | select pid name cpu | to json
```

### Step 1 - Lexing/parsing

AST conceptually:

```text
Pipeline[
  NativeCommand(verb=get, target=process),
  Transform(where, Binary(>, Field(cpu), Float(20))),
  Transform(select, [Field(pid), Field(name), Field(cpu)]),
  Transform(to, Format(json))
]
```

### Step 2 - Resolution

`get process` resolves to command ID `kuang.process.get`. The active Linux provider advertises capability `process.list` and output schema `kuang.process/1`.

### Step 3 - Semantic checks

- `cpu` exists and is numeric/nullable;
- comparing nullable `cpu` requires defined null behavior; default predicate treats null as non-match or requires explicit coalescing - this policy must be frozen;
- `select` fields exist;
- `to json` accepts arbitrary serializable values.

### Step 4 - Execution graph

```text
procfs producer
   |
   v
where(cpu > 20)
   |
   v
project(pid,name,cpu)
   |
   v
json encoder
   |
   v
Text/Bytes sink
```

### Step 5 - Backpressure

The JSON encoder requests values as it can write. Producer channels stay bounded.

### Step 6 - Error propagation

A process disappearing between enumeration and detail reads can be represented according to provider policy: skip transiently missing process with diagnostic metadata, or emit a recoverable provider event. The behavior must be deterministic and tested.

# 49. Security Threat Model Sketch

KUANG is a shell and therefore executes untrusted filenames, commands, environment data and remote content. Threat modeling is not optional.

Threats include:

- malicious filenames containing control sequences;
- terminal escape injection from external stdout;
- plugin code execution;
- poisoned completion sources;
- remote agent impersonation;
- host key changes;
- schema/protocol bombs causing memory exhaustion;
- history leakage of secrets;
- unsafe rendering of OSC hyperlinks;
- command confusion between native and external namespaces;
- PATH shadowing;
- TOCTOU between preview and destructive action;
- PID reuse between selection and signal;
- symlink races;
- privilege escalation boundaries.

Mitigations SHOULD include:

- escape/sanitize untrusted terminal control sequences in native renderers;
- retain raw data separately from display;
- stable target identity tokens for mutations where possible;
- confirm object identity immediately before mutation;
- bounded protocol frames and streams;
- explicit trust stores for links/plugins;
- secret-aware history policy;
- `explain` resolution for ambiguous commands;
- fuzzing of all parsers/decoders.

# 50. Release Quality Bar

Because the target audience is highly technical, early credibility is fragile. A public release should not be judged only by feature count.

Minimum quality expectations for any advertised capability:

- help is complete;
- completion works;
- output schema is inspectable;
- redirected behavior is deterministic;
- error cases are structured;
- tests cover privilege and race conditions;
- performance is measured;
- external command compatibility is not regressed;
- examples in docs are executable;
- the feature looks intentional in narrow and wide terminals;
- the implementation does not parse unstable human-readable text unless clearly documented as an adapter fallback.

A smaller set of finished, trustworthy capabilities is more aligned with KUANG than a broad set of half-working integrations.

# 51. Short Product Narrative

A useful way to explain KUANG to a technical audience:

> Unix pipes were designed when the universal interchange format between small tools was a byte stream. That decision was simple, durable and brilliant. But the machine already knows that a process is a process, a socket is a socket and a service is a service. Traditional shells flatten those things into text before we can compose them.
>
> KUANG keeps Unix execution, but adds a native object world beside it. Its own commands use a predictable verb-target language and send typed records through pipelines. Tables are only views. JSON is a serializer, not the data model. Existing binaries still run normally. When an object crosses into the byte-stream world, the conversion is explicit.
>
> The result is a shell that is easier to discover, safer to automate and unusually good at exploring a live system. It can grow from structured process queries into live streams, object contexts, relationship traces and remote links without abandoning the terminal.
>
> It should feel less like typing incantations at Unix and more like having a direct interface into the machine.

# 52. Appendix A - Proposed Core Command Matrix

The following matrix is a planning artifact. `R` = read/producer, `M` = mutation, `C` = context, `V` = view/transform, `L` = live.

| Target | get | inspect | watch | trace | start | stop | restart | enter | add | remove | set |
|---|---|---|---|---|---|---|---|---|---|---|---|
| process | R | R | L | R | - | M | - | C? | - | - | M? |
| service | R | R | L | R | M | M | M | C | - | - | M |
| file | R | R | L? | R? | - | - | - | C? | - | M | M |
| dir | R | R | L? | R? | - | - | - | C | - | M | M |
| socket | R | R | L | R | - | M? | - | C? | - | - | - |
| interface | R | R | L | R? | M? | M? | - | C? | M? | M? | M |
| route | R | R | L | R | - | - | - | - | M | M | M |
| user | R | R | L? | R? | - | - | - | C? | M | M | M |
| group | R | R | L? | - | - | - | - | C? | M | M | M |
| mount | R | R | L? | R? | M | M | - | C? | M | M | M |
| container | R | R | L | R | M | M | M | C | - | M | M |
| host | R | R | L? | R | - | - | - | C/link | M? | M? | M? |
| link | R | R | L | R? | - | M/detach | - | C | M | M | M |

Question marks mean the semantic usefulness must be validated rather than mechanically implemented for symmetry.

# 53. Appendix B - Suggested Built-in Transform Reference

### `where`

Input: `Stream<T>` or `List<T>`  
Output: same element type  
Streaming: yes

```text
get process | where cpu > 20 and user.name != "root"
```

### `select`

Projects fields and expressions into records.

```text
get process | select pid name memory
```

Possible computed fields:

```text
get process | select pid name {mem_mb: memory / 1MiB}
```

Exact record-construction syntax remains open.

### `sort`

Finite input by default. Must reject truly unbounded streams without a window.

```text
... | sort memory desc
```

### `group`

```text
get process | group user.name
```

Returns `Group<T>` records rather than preformatted headings.

### `take` / `skip`

Streaming and lazy.

### `each`

Maps one input value to zero/one/many outputs depending on block semantics. This must be specified carefully to avoid accidental nested streams.

### `measure`

```text
get process | measure memory
```

Potential result:

```text
count  412
sum    18.2 GiB
mean   45.2 MiB
min    84 KiB
max    3.8 GiB
```

The values remain typed.

### `join`

Relational joins are powerful but complex. Syntax should not be frozen until common shell use cases justify it.

### `diff`

Can compare structured snapshots by identity/schema, useful for system state.

# 54. Appendix C - Compatibility Escape Hatches

A new shell needs explicit ways out of its own abstractions.

Potential escape hatches:

```text
exec <external> ...        force external resolution
raw <command>              execute with minimal KUANG interpretation (exact semantics TBD)
sh -c '...'                invoke POSIX shell
bash -c '...'              invoke Bash
command path <name>        inspect resolution
which/get command          structured command discovery
```

KUANG should not be ashamed of calling Bash when a Bash script already solves a task. The product value is not ideological purity.

# 55. Appendix D - Example Spec-Driven Work Breakdown

A code-generating agent could consume the spec tree and create claims/work packages such as:

```text
LANG-001  Lexer token model
LANG-002  Parser: pipeline
LANG-003  Parser: expressions
LANG-004  Diagnostic spans
VAL-001   Core scalar Value enum
VAL-002   Schema registry
VAL-003   Record storage
PIPE-001  Async bounded stream abstraction
PIPE-002  where transform
PIPE-003  select transform
PROC-001  External exec simple
PROC-002  External pipelines
PROC-003  PTY foreground process
PROC-004  Job control
PROV-001  Provider capability registry
LNX-001   procfs process enumeration
LNX-002   process detail fields
LNX-003   process signal action
RND-001   table renderer
RND-002   adaptive width policy
META-001  command registry loader
META-002  help generator
META-003  completion generator
TEST-001  parser fixture extractor from docs
TEST-002  provider conformance harness
```

Each work item can link back to stable section IDs or registry entries. This is the practical mechanism by which a large design document becomes agent-friendly rather than merely descriptive.

# 56. Appendix E - Design Review Checklist

Before accepting a native command or major interaction, ask:

- What exact input/output type does it have?
- Is it streaming?
- Does it require finite input?
- What provider capability implements it?
- Can it run remotely?
- What privileges does it need?
- What are partial-failure semantics?
- What happens when redirected?
- How does it render at 80 columns?
- How does completion discover its selectors/options?
- Can `explain` describe it?
- Does its help derive from metadata?
- Does it introduce a new verb or target unnecessarily?
- Does it preserve structured values?
- How does it interoperate with external programs?
- What error codes can it emit?
- What identity/race guarantees exist for destructive actions?
- What tests can be generated from its registry definition?
- Is any visual effect showing real state?
- Would an expert user understand why this belongs in KUANG?

# 57. Closing Statement

KUANG does not need to beat Bash by having more commands. It needs a sharper conceptual model.

The smallest version of the idea is already compelling: **a Unix-native shell with a coherent verb-target language and typed object pipelines, while retaining first-class execution of ordinary Unix software.** Everything else in this document - rich rendering, context stacks, live streams, relationship tracing and remote links - follows from taking the same premise seriously.

The most important implementation discipline is to avoid faking structure. If KUANG knows what a process is, it should preserve that process as an object. If it only has bytes from an external program, it should admit that it has bytes. If it knows a relationship exactly, it can draw it. If it only infers one, it must label the inference. If a visual effect has no real state behind it, it probably does not belong.

That discipline is also what protects the coolness factor. The target users do not need a terminal that looks like cyberpunk fiction. They need a terminal powerful enough that operating a real machine occasionally feels like it.

**Working one-line pitch:**

> **KUANG is a cyberpunk object shell for Unix: predictable commands, typed pipelines, live system objects, and no fake hacker theatre.**
