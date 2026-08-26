# STATE

The shared work board. **Read it first, update it last, every session** (AGENTS.md section 9).
The stopping rule lives in `docs/ACCEPTANCE.md`: the run ends when `scripts/release-check.sh`
passes, not when this file looks tidy.

Current phase: **A — Language and Unix shell foundation** (spec section 37)

Phase A exit criterion: *Ono-Sendai can replace Bash for ordinary interactive execution without
native object features yet becoming a dead end.* Exit test: acceptance case
`010-replaces-bash-for-ordinary-work`, plus every box in `docs/ACCEPTANCE.md` section 4.1 A.

---

## In progress

_(empty — claim a task here before editing code: `[agent-id | timestamp] task — files: paths`)_

---

## Next up (ordered)

Phase A is decomposed to increment level. Later phases are listed at their coarse shape and are
decomposed by the agent that starts them — decomposing early would invent detail the spec does
not fix yet.

### Phase A — Language and Unix shell foundation

- [ ] A1 — Lexer: tokens, spans, quoting and escaping corpus — spec sections 6, 26 —
      exit test: `crates/ono-parser/tests/lexer.rs` golden corpus
- [ ] A2 — Parser and AST with recoverable errors and precise spans — spec sections 24.4, 26 —
      exit test: golden AST snapshots + diagnostics snapshots
- [ ] A3 — Incremental/partial parse for a line being typed — spec section 24.4 —
      exit test: partial-input parse tests
- [ ] A4 — Evaluator skeleton: run an external command, propagate exit status — spec section 29 —
      exit test: acceptance `020-runs-external-commands`
- [ ] A5 — Environment variables, `cd` and working directory — spec section 19 —
      exit test: acceptance `021-cwd-and-environment`
- [ ] A6 — Redirection: `>`, `>>`, `<`, fd duplication, deterministic non-TTY behaviour —
      spec sections 12, 29 — exit test: acceptance `022-redirection`
- [ ] A7 — External pipelines and exit status of a pipeline — spec section 11 —
      exit test: acceptance `023-external-pipelines`
- [ ] A8 — PTY execution for full-screen programs — spec section 29 —
      exit test: acceptance `024-pty-applications`
- [ ] A9 — Signals, process groups and foreground/background job control — spec section 18 —
      exit test: acceptance `025-job-control`
- [ ] A10 — Line editor: keymap, editing, syntax highlight from the incremental parse —
      spec section 24.1 — exit test: editor behaviour tests + latency budget
- [ ] A11 — History persistence and recall — spec section 20 —
      exit test: acceptance `026-history-survives-restart`
- [ ] A12 — Configuration loading, with no eager plugin load and no network at startup —
      spec section 30 — exit test: acceptance `027-startup-is-quiet`
- [ ] A13 — Prompt with location URI and privilege indication — spec sections 4, 17 —
      exit test: acceptance `028-prompt-shows-context`
- [ ] A14 — Structured error model and exit-status contract — spec sections 16, 43 —
      exit test: error taxonomy tests
- [ ] A15 — Phase A gate: `ono` as a login shell doing a real working session —
      exit test: acceptance `010-replaces-bash-for-ordinary-work`

### Phase B — Value system and native pipelines

- [ ] Value model and schemas; stream engine with backpressure and cancellation; `where`,
      `select`, `sort`, `take`, `skip`, `each`, `count`, `measure`; JSON/YAML/CSV/text
      conversion; renderer separated from data — spec sections 10, 11, 13, 25

### Phase C — Linux core providers

- [ ] `process`, `file`/`dir`, `user`/`group`, `env`, `mount`/`filesystem`,
      `interface`/`route`/`neighbor`, `socket`/`connection`, `service` — spec section 23

### Phase D — Language consistency and discoverability

- [ ] `docs/spec/` registries as the public contract; metadata-driven help and completion;
      `type`, `inspect`, `explain`; generated docs; generated provider conformance suites —
      spec sections 15, 27, 36, 47

### Phase E — Contextual systems interface

- [ ] Context stack, `enter`/`leave`, implicit selectors, prompt/HUD, interactive selection,
      structured recent-result reuse — spec sections 14, 20

### Phase F — Live system semantics

- [ ] `watch`, event/snapshot model, in-place rendering, native background jobs, stable object
      identity — spec section 18

### Phase G — Relationship graph

- [ ] Graph value type, relationship providers, `trace`, tree/graph renderers, provenance and
      confidence — spec section 22

### Phase H — Remote links

- [ ] Remote protocol, agent, SSH fallback, provider negotiation, security model, remote prompt,
      multiplexed streams — spec section 21

### Phase I — KUANG/11 extension runtime

- [ ] The production path of spec section 31: manifests, capabilities, isolation, host API,
      contributions, audit, SDK, `docs/spec/kuang/` contracts, test host, conformance suite

### Phase J — Advanced TUI views

- [ ] Navigable graphs, multi-pane inspect/watch, timeline exploration, object pickers, remote
      link overview — only where the semantics justify them

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

---

## Deferred / blocked

_(empty — an entry here needs a reason, the ignored test's path, and the ADR that states the
assumption)_

---

## Notes for whoever starts phase A

- The workspace is green as delivered. Confirm it (`scripts/gate.sh`) before your first edit, so
  a later red gate is unambiguously yours.
- `crates/ono-cli/src/main.rs` is scaffolding: it answers `--version` and `--help` and refuses
  everything else. Replacing its argument handling with the real interpreter is expected and
  needs no ADR; the three acceptance cases guarding it must keep passing.
- Crate names not yet created (`ono-parser`, `ono-value`, `ono-pipeline`, …) come from spec
  section 24.2 with the `ono-` prefix. Create them as the phase needs them, not upfront.
- Add the acceptance case in the same increment as the capability. A feature without a case in
  the container does not count as delivered (`docs/ACCEPTANCE.md` section 2).
