# STATE

The shared work board. **Read it first, update it last, every session** (AGENTS.md section 9).
The stopping rule lives in `docs/ACCEPTANCE.md`: the run ends when `scripts/release-check.sh`
passes, not when this file looks tidy.

Working branch: **`implementation`** — never commit to `main` (AGENTS.md section 12.1)

**Commit every increment, and tag every completed phase.** A phase is done when its box in
`docs/ACCEPTANCE.md` section 4.1 is ticked; the commit that ticks it gets an annotated tag
`phase-<letter>` whose message names the exit criterion and the case that proves it. The tags are
how the state after each phase stays findable in a run of hundreds of commits:

```bash
git tag -n99 phase-a          # what Phase A delivered, and what proves it
git switch --detach phase-a   # the tree exactly as that phase left it
```

Tags so far: `phase-a`.

**Push after every commit.** AGENTS.md §12.1 keeps `main` untouched and §12.2 asks that
`implementation` be pushed freely so work is not lost; the branch and its phase tags live on
`origin`. Never push `main`, never open a pull request unless asked.

```bash
git push origin implementation && git push origin --tags
```

Current phase: **A — Language and Unix shell foundation** (spec section 37)

Phase A exit criterion: *Ono-Sendai can replace Bash for ordinary interactive execution without
native object features yet becoming a dead end.* Exit test: acceptance case
`010-replaces-bash-for-ordinary-work`, plus every box in `docs/ACCEPTANCE.md` section 4.1 A.

---

## In progress

- [orchestrator | 2026-08-26] Phase B/C/D integration: wiring the command registry, the providers
  and the pipeline into the evaluator — files: `crates/ono-cli/**`, `crates/ono-parser/**`,
  `crates/ono-core/**`, `crates/ono-render/**`, `crates/ono-history/**`, `crates/ono-testkit/**`,
  `crates/ono-provider-api/**`, `xtask/**`, `docs/**`, `docker/**`, `scripts/**`, `Cargo.toml`
- [agent:commands | 2026-08-26] B4–B6, C-wiring, D5: the native command implementations and the
  expression compiler — files: `crates/ono-command/**`
- [agent:graph | 2026-08-26] G1–G4 graph values, relationship providers, `trace` —
  files: `crates/ono-graph/**`
- [agent:protocol | 2026-08-26] H1 the remote protocol — files: `crates/ono-protocol/**`

---

## Next up (ordered)

Phase A is decomposed to increment level. Later phases are listed at their coarse shape and are
decomposed by the agent that starts them — decomposing early would invent detail the spec does
not fix yet.

### Phase A — Language and Unix shell foundation

**Phase A is complete.** Its exit criterion from spec §37 is proven by the acceptance case
`010-replaces-bash-for-ordinary-work`, and `docs/ACCEPTANCE.md` §4.1 A is ticked. The performance
budgets of §34 are tracked under *Cross-cutting*, not here.

- [x] A1 — Lexer: tokens, spans, quoting and escaping corpus — spec sections 6, 26 —
      exit test: `crates/ono-parser/tests/lexer.rs` golden corpus
- [x] A2 — Parser and AST with recoverable errors and precise spans — spec sections 24.4, 26 —
      exit test: golden AST snapshots + diagnostics snapshots
- [x] A3 — Incremental/partial parse for a line being typed — spec section 24.4 —
      exit test: partial-input parse tests
- [x] A4 — Evaluator skeleton: run an external command, propagate exit status — spec section 29 —
      exit test: acceptance `020-runs-external-commands`
- [x] A5 — Environment variables, `cd` and working directory — spec section 19 —
      exit test: acceptance `021-cwd-and-environment`
- [x] A6 — Redirection: `>`, `>>`, `<`, fd duplication, deterministic non-TTY behaviour —
      spec sections 12, 29 — exit test: acceptance `022-redirection`
- [x] A7 — External pipelines and exit status of a pipeline — spec section 11 —
      exit test: acceptance `023-external-pipelines`
- [x] A8 — PTY execution for full-screen programs — spec section 29 —
      exit test: acceptance `024-pty-applications`
- [x] A9 — Signals, process groups and foreground/background job control — spec section 18 —
      exit test: acceptance `025-job-control`
- [x] A10 — Line editor: keymap, editing, syntax highlight from the incremental parse —
      spec section 24.1 — exit test: editor behaviour tests + latency budget
- [x] A11 — History persistence and recall — spec section 20 — library done
      (`crates/ono-history/tests/history.rs`); wiring and acceptance case
      `026-history-survives-restart` land with the REPL
- [x] A12 — Configuration loading, with no eager plugin load and no network at startup —
      spec section 30 — exit test: acceptance `027-startup-is-quiet`
- [x] A13 — Prompt with location URI and privilege indication — spec sections 4, 17 —
      exit test: acceptance `028-prompt-shows-context`
- [x] A14 — Structured error model and exit-status contract — spec sections 16, 43 —
      exit test: error taxonomy tests
- [x] A15 — Phase A gate: `ono` as a login shell doing a real working session —
      exit test: acceptance `010-replaces-bash-for-ordinary-work` — **Phase A complete**

### Phase B — Value system and native pipelines (spec §10, §11, §12, §13, §25)

- [x] B3 — Stream engine: bounded channels, backpressure, cancellation, the streaming/blocking
      distinction — `crates/ono-pipeline/tests/{backpressure,boundedness,cancellation}.rs`
- [x] B6 — Conversion `to`/`from` json, yaml, csv, text, bytes — `crates/ono-value/tests/`
- [x] B7 — Renderer separated from data: table, stacked, list, tree, raw, hex; width-aware
      layout; visible truncation; semantic theme tokens — `crates/ono-render/tests/`
- [x] B1 — Value model: scalars, semantic scalars, units, `Record`, `Map`, `List`, provenance —
      `crates/ono-value/tests/` — ADR-0016 — commit 05eb85a
- [x] B2 — Schema model and registry, the canonical schemas of spec §28, compatibility rules —
      `crates/ono-value/tests/{builtin_schemas,schema_compatibility}.rs` — commit 05eb85a
- [ ] B3 — Stream engine: bounded channels, backpressure, cancellation, the
      streaming/blocking transform distinction of §11.1 — exit test: a slow consumer bounds a
      fast infinite producer's memory; `stream.unbounded_operation` on `sort` over an unbounded
      stream — acceptance `030-native-pipeline-backpressure`
- [ ] B4 — Transforms `where`, `select`, `take`, `skip`, `each` (streaming) — spec §53 —
      exit test: acceptance `031-transforms-stream`
- [ ] B5 — Transforms `sort`, `group`, `count`, `measure`, `reduce`, `join`, `diff` (bounded) —
      spec §53 — exit test: acceptance `032-transforms-bounded`
- [ ] B6 — Conversion `to`/`from` json, yaml, csv, text, bytes — spec §12.3, §12.4 —
      exit test: round-trip properties + acceptance `033-serialization-round-trip`
- [~] B7 — Renderer separated from data: table, list, stacked, json, yaml, raw, hex; width-aware
      layout; visible truncation; human formatting of semantic scalars — spec §13 —
      exit test: `tests/render/` snapshots at 80 and 200 columns, and identical values through
      every renderer — acceptance `034-render-is-presentation-only`
- [ ] B8 — Object-to-external and external-to-object boundaries: structured input to an external
      command is a structured error suggesting `to json`; external stdout enters as bytes/text
      without loss — spec §12.2, §12.3 — exit test: acceptance `035-interop-boundary`
- [ ] B9 — Pipeline type-checking before execution where schemas are known: `where cpy > 20`
      reports `type.unknown_field` with a suggestion, before enumeration starts — spec §11.3 —
      exit test: acceptance `036-typo-caught-before-execution`
- [ ] B10 — `ActionResult` and partial failure: bulk mutation reports per-target results and
      never collapses them — spec §11.5, §16.5 — exit test: acceptance `037-partial-failure`

### Phase C — Linux core providers (spec §23, §28, §35.3)

Every provider answers from the kernel, systemd or NSS — never by parsing unstable human text
(spec §50, AGENTS.md §6). Every provider ships its conformance case in the same increment.

- [x] C1 — `ono-provider-api`: the provider trait, capability declarations, and the
      `snapshot` / `subscribe` / `watch` triple with the `ObjectEvent` envelope of spec §31.14,
      shaped so KUANG/11 consumes it without special cases (spec §31 preamble, §31.13)
- [x] C2 — `process` from procfs: enumeration, `ono.process/1` fields, CPU as a rate not a
      cumulative, permission-denied fields as errors not zeros — spec §23.1, §28.1 —
      exit test: acceptance `040-process-provider`
- [x] C3 — `file`/`dir`: metadata, recursion, symlinks, permissions, xattrs where present —
      spec §23.4, §28.2 — exit test: acceptance `041-file-provider`
- [x] C4 — `user`/`group` from NSS, `env` — spec §23.6, §28.7 —
      exit test: acceptance `042-identity-provider`
- [x] C5 — `mount`/`filesystem` — spec §23.5, §28.6 — exit test: acceptance `043-mount-provider`
- [x] C6 — `interface`/`route`/`neighbor` over netlink — spec §23.2, §28.5 —
      exit test: acceptance `044-network-provider`
- [x] C7 — `socket`/`connection` over netlink sock_diag, joined to owning process —
      spec §23.2, §28.4 — exit test: acceptance `045-socket-provider`
- [x] C8 — `service` over the systemd D-Bus API, degrading to `provider.unavailable` where
      systemd is not running — spec §23.3, §28.3 — exit test: acceptance `046-service-provider`
      plus a D-Bus fixture test for the positive path (see *Deferred*)
- [ ] C9 — Generated provider conformance suite from `docs/spec/providers/*.yaml` — spec §35.3

### Phase D — Language consistency and discoverability (spec §15, §27, §36, §47)

- [x] D0 — The registries themselves: `docs/spec/{verbs,targets,errors,capabilities,language}.yaml`,
      `schemas/*.v1.yaml`, `commands/*.yaml` — ADR-0012 — commit e1363de
- [x] D1 — `xtask spec-check` validates the registries and cross-checks them against the
      implementation: undocumented stable command, metadata without implementation, doc example
      that no longer parses, schema break without version bump, provider output outside its
      advertised schema — spec §36.5
- [ ] D2 — The command registry drives dispatch: one stable id per command, bound to an
      implementation, verified by `spec-check` — spec §27.2
- [ ] D3 — `help` generated from metadata for every command, target and topic — spec §15.2
- [ ] D4 — Completion from metadata: commands, verbs, targets, options, argument positions, and
      live values where a provider is cheap — spec §15.1 — exit test: first results < 50 ms
- [ ] D5 — `type` and `inspect`, showing schema, provenance and the causal chain — spec §15.2
- [ ] D6 — `explain`: the resolution and execution plan without executing, in the shape of
      spec §42 — spec §15.3
- [ ] D7 — Fuzzy command discovery and the suggestion path of `resolve.command_not_found` —
      spec §15.4
- [x] D8 — Generated documentation under `docs/reference/`, reproducible from the registries and
      checked by the gate — spec §36.2, §46

### Phase E — Contextual systems interface (spec §14, §20)

- [ ] E1 — Context stack, `enter`/`leave`, filesystem and object contexts — spec §14.1–§14.3
- [ ] E2 — Implicit selectors from context — spec §14.3
- [ ] E3 — Prompt as a HUD: link, privilege, context, path, vcs, jobs — spec §4.2
- [ ] E4 — Interactive selection over rendered collections, never altering pipeline data —
      spec §13.5
- [ ] E5 — Semantic history and bounded structured result retention; `@`, `@-1`, `@3` —
      spec §20.1, §20.2, §6.4

### Phase F — Live system semantics (spec §18)

- [ ] F1 — `watch` over a query, event/snapshot model, explicit polling metadata — §18.2
- [ ] F2 — In-place rendering keyed by stable object identity — §18.3
- [ ] F3 — Native background jobs, `get job`, the prompt's job segment — §18.4
- [ ] F4 — Cancellation through native pipelines and into external processes — §18.5

### Phase G — Relationship graph (spec §22)

- [ ] G1 — Graph value type with provenance and confidence — §22.1, §22.2
- [ ] G2 — Exact relationship providers: process tree, socket to process, service to process,
      mount to device — §22.3
- [ ] G3 — `trace` for process, service and socket — §22.3
- [ ] G4 — Tree and ASCII graph renderers; the graph view never fabricates edges — §22.4

### Phase H — Remote links (spec §21)

- [ ] H1 — `ono-protocol`: typed transport, framing, versioning, multiplexed streams — §21.2
- [ ] H2 — `ono-agent`: the remote endpoint — §21.4
- [ ] H3 — Agentless SSH fallback — §21.3
- [ ] H4 — Provider negotiation and capability discovery — §21.2
- [ ] H5 — Security model: host key pinning, `remote.host_key_changed` — §21.5, §49
- [ ] H6 — Remote context and prompt — §14.4, §4.2

### Phase I — KUANG/11 extension runtime (spec §31)

- [ ] I1 — `docs/spec/kuang/` contracts: manifest, capability, protocol schemas — §31.78
- [ ] I2 — `ono-kuang-protocol`: the typed host/plugin protocol — §31.12
- [ ] I3 — Package identity, layout, manifest validation, verification — §31.5–§31.7, §31.9
- [ ] I4 — Supervisor: install/enable/load/run states, lifecycle, isolation — §31.8, §31.10
- [ ] I5 — Capability broker, scopes, grant UX, storage and policy, audit — §31.16–§31.19, §31.33
- [ ] I6 — Host API domains: objects, streams, schemas, commands, relations, views, context,
      history, filesystem, network, process, secrets, models, state, audit, clock — §31.12
- [ ] I7 — Backpressure, quotas and overflow policy — §31.15
- [ ] I8 — Contribution model: commands, targets, schemas, relations, views, annotations — §31.22–§31.27
- [ ] I9 — `ono-kuang-sdk` and the deterministic test host — §31.73
- [ ] I10 — Plugin conformance suite — §31.74
- [ ] I11 — `ono-model-broker`: operator-approved inference, no LLM in a privileged path — §31.12

### Phase J — Advanced TUI views (spec §37 Phase J, §13.6)

- [ ] J1 — Navigable graph view — §22.5
- [ ] J2 — Multi-pane inspect/watch — §37
- [ ] J3 — Timeline/history exploration — §20.3
- [ ] J4 — Object pickers — §13.5
- [ ] J5 — Remote link overview — §37

### Cross-cutting, tracked to the release checklist

- [ ] Performance budgets of spec §34 measured in the container on the pathological fixtures
- [ ] Fuzzers over parser, serializers, remote protocol, plugin protocol, procfs/netlink
      decoders — spec §35.6
- [ ] A test for each risk in the threat model of spec §49
- [ ] Theme and semantic visual tokens — spec §44
- [ ] The per-capability quality bar of spec §50 for every advertised command

---

## Done

- [x] Bootstrap: Cargo workspace (`ono-cli`, `ono-core`, `ono-testkit`, `xtask`), pinned
      toolchain, lint configuration, first outcome tests — ADR-0001
- [x] Quality gate `scripts/gate.sh` and contract check `cargo xtask spec-check` — ADR-0001
- [x] Containerised acceptance harness: `docker/Dockerfile`, `docker/acceptance/cases/`,
      `scripts/acceptance.sh`, verified green with four cases — ADR-0002
- [x] Release gate `scripts/release-check.sh` and the stopping rule in `docs/ACCEPTANCE.md` —
      ADR-0002
- [x] CI running the gate and the acceptance suite on every push — ADR-0002
- [x] Specification immutability enforced by checksum in `cargo xtask spec-check` — ADR-0003
- [x] Branch policy: implementation on a disposable `implementation` branch, guarded in
      `scripts/gate.sh` — ADR-0004
- [x] Acceptance harness extended: `|` block scripts, stdin, `pty:`, `columns:`/`lines:`, `env:`,
      `timeout:` and repeatable assertions, with a self-test case — commit 2001a1d
- [x] The gate refuses untracked unfinished work: `todo!()`, `unimplemented!()`, untracked
      `TODO`/`FIXME`, `#[ignore]` without a reason — `xtask/tests/scan.rs` — commit 6d855b1
- [x] `ono-testkit`: real-binary runs with a deadline, scratch directories, and a reproducible
      generator for fuzz-style tests — commits e27f481, 12b0a97
- [x] `ono-render`: width-aware table and stacked-record layout, semantic theme tokens, the
      presentation contract of spec §4.6, and the ASCII tree of §22.4 — commits f387b2c,
      6f047a3, 22b2f22
- [x] `ono-history`: semantic entries, restart survival, secret policy — commit 4d7d400
- [x] A0 — Shared vocabulary in `ono-core`: `Span`, the complete error taxonomy of spec §43,
      the exit-status contract — ADR-0005/0006/0008 — commit 1012fea —
      tests `crates/ono-core/tests/{error_taxonomy,exit_status,span}.rs`
- [x] A0 — The concrete grammar: ADR-0009 and `docs/spec/grammar.ebnf`, resolving the
      command/expression ambiguity of spec §26.1 with the two argument modes

---

## Deferred / blocked

_(empty — an entry here needs a reason, the ignored test's path, and the ADR that states the
assumption)_

---

## Notes for whoever starts phase A

- Switch to `implementation` before your first edit. The gate refuses to run on `main`.
- The workspace is green as delivered. Confirm it (`scripts/gate.sh`) before your first edit, so
  a later red gate is unambiguously yours.
- `crates/ono-cli/src/main.rs` is scaffolding: it answers `--version` and `--help` and refuses
  everything else. Replacing its argument handling with the real interpreter is expected and
  needs no ADR; the three acceptance cases guarding it must keep passing.
- Crate names not yet created (`ono-parser`, `ono-value`, `ono-pipeline`, …) come from spec
  section 24.2 with the `ono-` prefix. Create them as the phase needs them, not upfront.
- Add the acceptance case in the same increment as the capability. A feature without a case in
  the container does not count as delivered (`docs/ACCEPTANCE.md` section 2).
- The specification is read-only and checksum-enforced. When it is ambiguous, wrong or in your
  way, write an ADR with a `Spec deviation` heading and implement your decision — never edit the
  spec (AGENTS.md section 5.1, ADR-0003).
