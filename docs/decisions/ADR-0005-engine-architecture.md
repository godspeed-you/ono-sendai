# ADR-0005: Engine architecture, crate layout and library choices

- Status: accepted
- Date: 2026-08-26
- Spec refs: §5, §24.1, §24.2, §24.4, §24.5, §25
- Decided by: agent (autonomous)

## Context

Spec §24.2 sketches a workspace layout and §24.4/§24.5 call several parser and runtime
approaches "plausible" without choosing one. ADR-0001 deferred crate creation until a phase
needs it. Phase A now needs a shape that later phases can grow into without a rewrite, and
several agents will work on disjoint crates in parallel, so the boundaries and the shared
vocabulary must be fixed before the first line of Phase A code.

## Decision

### Crate layout

The workspace follows spec §24.2 with the `ono-` prefix of AGENTS.md §3. Crates are created
when a phase needs them. The dependency graph is a DAG, layered as in spec §5:

```text
ono-core        spans, the error-code taxonomy (§43), exit-status contract, no dependencies
ono-value       Value, Record, Schema, units, provenance, ErrorValue        -> ono-core
ono-parser      lexer, AST, recoverable parser, diagnostics                 -> ono-core
ono-process     external exec, redirection, PTY, signals, job control       -> ono-core
ono-pipeline    stream engine, backpressure, cancellation                   -> ono-value
ono-command     native command registry, invocation, metadata binding       -> ono-value, ono-pipeline
ono-render      table/list/tree/graph/json/yaml/raw renderers               -> ono-value
ono-provider-api    provider traits and capability declarations             -> ono-value, ono-pipeline
ono-provider-linux  procfs/sysfs/nss-backed providers                       -> ono-provider-api
ono-provider-systemd/-netlink   as their phase needs them
ono-editor      line editor, keymap, highlight from the incremental parse   -> ono-parser, ono-render
ono-history     semantic history and bounded structured result retention    -> ono-value
ono-protocol / ono-agent        remote links (phase H)
ono-kuang-protocol/-supervisor/-sdk, ono-model-broker, ono-view-protocol (phase I)
ono-cli         the `ono` binary: evaluator, resolution, REPL, wiring       -> everything
```

`ono-core` is deliberately dependency-free so that `ono-parser`, `ono-value` and `ono-process`
can be developed concurrently against a stable shared vocabulary.

The evaluator, name resolution and context stack live in `ono-cli` until a second consumer
exists (spec §24.1 names them as components, not as crates). Splitting them out earlier would
be speculative generality (AGENTS.md §4).

### Parser: hand-written

The lexer and parser are hand-written recursive descent. Spec §24.4 requires excellent error
spans and incremental parse of a line being typed, and §26.1 requires bare words to lex
differently in command-argument position than in expression position. Parser generators make
both hard: generated error recovery is generic, and context-sensitive lexing fights the
generated tokenizer. A hand-written parser also keeps the parse budget of §34 (< 5 ms) under
direct control and adds no dependency to the hot path.

### Async runtime: Tokio, but not for foreground processes

Tokio (`rt-multi-thread`, `sync`, `time`, `io-util`, `process`, `signal`, `net`, `fs`) drives
native stream pipelines, provider subscriptions, watches, remote links and plugin IPC, as
spec §24.5 suggests. Bounded `tokio::sync::mpsc` channels are the backpressure mechanism
required by §11.2.

Foreground external command execution does **not** run on the async runtime. It uses blocking
`std::process` plus direct terminal/process-group calls, because terminal ownership, `tcsetpgrp`
and signal delivery are defined in terms of the controlling terminal and the foreground process
group, which spec §24.5 explicitly flags as outside the "everything is a task" model.

### Third-party libraries

| Need | Choice | Why |
|---|---|---|
| Unix syscalls: termios, pty, signals, users, mounts | `nix` | complete, maintained, covers everything Phase A/C needs in one dependency |
| terminal input, raw mode, resize | `crossterm` | event model and raw-mode handling; the editor itself stays ours (spec §24.1) |
| serde formats | `serde`, `serde_json`, `serde_yaml_ng`, `csv` | §12.4, §46; `serde_yaml` is unmaintained |
| async runtime | `tokio` | §24.5 |
| regex (`~=` operator, §6.3) | `regex` | |
| timestamps | `jiff` | correct civil/zoned time; §10.2 `Timestamp`, §13.4 rendering |
| terminal width | `unicode-width` | §13.2 column layout must count display cells, not bytes |
| contracts (`docs/spec/*.yaml`) | `serde_yaml_ng` in `xtask` | §27, §36 |

Rationale for what is *not* used: no `clap` (the shell parses its own language and its own
argv), no `rustyline`/`reedline` (the editor must highlight from our incremental parse, §24.4),
no `anyhow` in library crates (errors are structured values, §16.1).

### Shared vocabulary fixed by this ADR

- `ono_core::Span` — byte offsets into the source line, `u32`, half-open.
- `ono_core::ErrorCode` — the complete taxonomy of spec §43, payload-free.
- `ono_core::ErrorKind` — the kinds of spec §16.1, extended by `Safety` and `Stream`.
- `ono_core::ExitStatus` — the exit-status contract (ADR-0008).
- `ono_value::Value` / `RecordValue` / `Schema` — the value model of §10 and §25.
- Every crate is `#![forbid(unsafe_code)]` except `ono-process` (ADR-0007).

## Consequences

Easy: parallel work on disjoint crates; replacing an internal choice (e.g. the renderer) without
touching the language; testing each layer through its public API only.

Hard: `ono-cli` becomes the integration point and will be the largest crate for a while;
it must be split when the evaluator grows a second consumer (phase H's agent, phase I's test
host).

Must be revisited: if `jiff` or `serde_yaml_ng` prove unsuitable, replacing them is a local
change behind `ono_value` and `xtask` respectively.

Encoded by: every crate's public API tests; `cargo xtask spec-check` once `docs/spec/` exists.

## Alternatives considered

- `chumsky`/`pest`/`lalrpop` — rejected: recovery and context-sensitive lexing, see above.
- `async-std`/`smol` — rejected: Tokio has the ecosystem for `process`, `signal` and `net` that
  phases F/H/I need.
- A single crate — rejected: prevents parallel agent work and lets layers leak into each other,
  which spec §5 forbids ("the shell language MUST not depend on any specific renderer").
- Splitting the evaluator into `ono-eval` upfront — rejected as speculative (AGENTS.md §4).
