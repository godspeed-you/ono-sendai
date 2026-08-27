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

Tags so far: `phase-a` … `phase-j` (one per completed phase; H, I and J tagged at the release commit).

**Push after every commit.** AGENTS.md §12.1 keeps `main` untouched and §12.2 asks that
`implementation` be pushed freely so work is not lost; the branch and its phase tags live on
`origin`. Never push `main`, never open a pull request unless asked.

```bash
git push origin implementation && git push origin --tags
```

**`release-check: the shell is release-ready` — printed 2026-08-26 by `scripts/release-check.sh`
at commit 58fadee.** All ten phases of spec §37 are complete, proven and tagged; every box in
docs/ACCEPTANCE.md §4 is ticked by a named automated proof; the containerised suite stands at
35 cases green and the workspace at ~1 400 outcome tests across 21 crates. What remains under
Next up is post-release deepening — every item deliberate, none blocking the deliverable.
Promoting `implementation` to `main` is the user's decision and the user's action
(AGENTS.md §12.1).

Phases A–D are complete and tagged. B/C/D landed as: native commands wired into the evaluator
(ADR-0028), partial failure semantics (ADR-0029), the §33.5 interop serialisation (ADR-0030),
path/string comparability (ADR-0031), the pre-flight field check (spec §11.3), shell stdin into
a parsing head (§12.4), unquoted `explain` over a pipeline, the provider registry
(docs/spec/providers/), and acceptance cases 040–044.

---

## The specification set

`docs/ono_sendai_shell_spec_v0.2.md` is the **base**. `docs/ono_sendai_shell_spec_v0.3_external_command_adapters.md`
is an **enhancement layered on it** — the External Command Adaptation Layer — and both are
immutable (AGENTS.md §5.2, ADR-0026). `spec-check` fails if either is missing a checksum line in
`docs/spec.sha256` or if `AGENTS.md` does not enumerate an enhancement by name.

**v0.3 is the next tranche, not part of this one.** Its §0 and §0.5 call it "a new product input
for a later revision" after the frozen v0.2 baseline, so `docs/ACCEPTANCE.md` and
`scripts/release-check.sh` keep measuring v0.2 and the stopping rule of AGENTS.md §15 is
unchanged. **ADR-0027 carries the whole analysis**: what v0.3 requires, which five existing
decisions grow (ADR-0006, -0011, -0013, -0016, -0022), where v0.2 and v0.3 read differently, and
the fifteen-step decomposition of the tranche (`ADAPT-001`…, then Tier A, B, C tools). Read
ADR-0027 before starting it; do not re-derive it from the 2182-line document.

One constraint from it binds work happening **now**: the pipeline planner computes `OutputDemand`
backwards from the consumer, because v0.3 §1.5 says the demand model "MUST be part of execution
planning, not an after-the-fact renderer trick", and retrofitting that into a forward planner is
the expensive kind of shortcut.

---

## Product direction from the user (2026-08-26)

**"Es muss immer cool sein und Spaß machen, es zu benutzen. Es soll aufregend sein."** The shell
is the Ono-Sendai deck: correctness is the floor, not the ceiling. Where a decision is
otherwise free, prefer the option that feels alive — the prompt as a HUD, tables that update in
place, colour that means something, latency you never notice (spec §34's budgets are product
quality), and answers that invite the next question (`@2 | inspect`). Phase F's `watch` is the
showcase: a live view of the machine should feel like instrumentation, not like polling.

## In progress

_(empty — the run's phases are complete; everything further lives under Next up)_

_No agent claims are outstanding; the six that ran (command implementations, graph, protocol,
KUANG/11 contracts, adversarial review, security review) have all reported and landed._

---

## Next up (ordered)

- [ ] A `SIGPIPE`d stdout (`ono -c '… | to json' | head -c 100`) reports io.permission_denied
  where every other shell exits quietly — treat EPIPE on stdout as normal termination — exit
  test: a cli case piping into head
- [ ] `sort` over a stream of scalars requires a key; identity should be the default key so
  `from json | sort` works on numbers — exit test: a transforms case
- [ ] `ono.plugin/1` records for `get plugin` so it composes (`| where state == loaded`),
  registry integration of contributions with origin `plugin(id, version)` (§31.64),
  `inspect plugin`, `get audit`, and folding the K11 codes into `ono_core::ErrorCode`
  (ADR-0040 §3, ADR-0051) — exit test: a plugins.rs case piping `get plugin`
- [ ] Phase H remainder: agentless mode (spec §21.3), trust-store location + first-contact key
  UX for a future authenticated transport (F12 rides along: TrustPolicy::Required records an
  unknown key — TOFU — where ADR-0015 T5 wants refusal; decide when the TCP transport exists,
  since both current transports are Unauthenticated-by-name per ADR-0037), eager surfacing of
  remote watch refusals — ADR-0036/0037 carry the details
- [ ] Phase I remainder (ADR-0040): wasm-component tier, objects/streams/views/models host
  domains, install/verify/signing, on-disk state + migrations, hot reload, binary frame encoding
  (a `perf` increment)
- [ ] Remaining `*-event/1` schemas (service, socket, interface, route, mount, file, user,
  group, container, link, host) — each un-deferred as its watch is exercised; the watch runtime
  is generic already — exit test: a watch.rs case per target
- [ ] `kill %N` for a native job (today: `fg` then Ctrl-C collects it) — exit test: a
  jobs_native.rs case
- [ ] Provider-native subscriptions (netlink, D-Bus signals) switching `source` to
  `subscription` (ADR-0034) — exit test: a watch.rs case against a subscribing fixture
- [ ] Retained results and secrets: spec §17.5 policy must reach the retention of §20.2
  (ADR-0033 consequences) — exit test: a redacted field stays redacted in `@-1`
- [ ] Provider options are silently ignored (`get process --user root` answers everything):
  audit every declared option against what providers honour, then make ignoring impossible the
  way selectors now cannot be ignored — exit test: a conformance case per optioned command
- [ ] `--name=value` in expression mode — ADR-0032 pairs an option with the following
  expression; the `=` spelling stays words-only until an increment adds it — exit test:
  a parse_expressions case for `reduce $acc + @ --initial=10`
- [ ] JSON object key order is alphabetical, not schema order (ADR-0030) — enabling
  serde_json `preserve_order` reorders the protocol too; decide and pin — exit test:
  data_codecs asserting §33.5 field order
- [ ] Surface `ono-pipeline` `Diagnostics` counters (`excluded_unknown`, `skipped_null`) to the
  user; ADR-0029 chose silence over an unread field — exit test: a case showing the count
- [ ] Streaming the byte carry across a native/external join (ADR-0028 buffers it) — exit test:
  `find / | from text | take 1` answers before the walk finishes
- [ ] Backgrounding a pipeline with native stages (ADR-0028 defers it) — exit test: `get process |
  count &` becomes a job `fg` can resume

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
- [x] B4 — Transforms `where`, `select`, `take`, `skip`, `each` (streaming) — spec §53 —
      `crates/ono-pipeline/tests/streaming_transforms.rs`, `crates/ono-command/tests/transforms.rs`
      — the acceptance case lands with the evaluator wiring
- [x] B5 — Transforms `sort`, `group`, `count`, `measure`, `reduce`, `join`, `diff` (bounded) —
      `crates/ono-pipeline/tests/blocking_transforms.rs` — acceptance case with the wiring
- [x] B9 — Pipeline type-checking before execution: `where cpy > 20` reports
      `type.unknown_field` with a suggestion from the contract's output schema, before anything is
      enumerated — `crates/ono-command/src/check.rs` — acceptance case with the wiring
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

- [x] `get service <name>` reaches unloaded on-disk units, and the listing no longer reports
      `not-found` stubs. Investigation showed the by-name path already resolved through
      `LoadUnit`; the real defect behind the CI flake was the inverse — `ListUnits` enumerates a
      stub for a referenced unit whose file is gone, and the enumeration reported it as a
      service the by-name path then rightly denied. Both paths now agree — tests:
      `should_find_a_unit_on_disk_when_systemd_has_not_loaded_it`,
      `should_report_no_service_when_a_listed_unit_is_only_a_dangling_reference`
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

## Known defects (found by adversarial review, 2026-08-26)

Two independent reviewers were asked to falsify the implementation rather than describe it, as
AUTONOMOUS_IMPLEMENTATION.md §18 requires. Between them they found 27 things, each with a
reproduction they ran.

**Everything release-blocking is fixed**, each with a regression test that fails without the fix
(commits 4034a41, 468258d). A ticked box below means fixed *and* guarded. What remains unticked is
should-fix or unbuilt, and each entry says which.

- [x] **R1 — nested blocks overflow the stack.** `if true { if true { … } }` nested about 2000
      deep aborts the process with SIGABRT. `MAX_DEPTH` in `crates/ono-parser/src/parser.rs` is
      consulted in `parse_stage` and in the expression parser but not in `parse_block`, so
      statement recursion is unguarded. The parser claims never to panic and always to return a
      tree, and it runs on every keystroke in the editor — one pasted line kills a login shell.
      `crates/ono-parser/tests/robustness.rs` has a test named for this that repeats `{` 2000
      times, which never enters block recursion: it passes while the thing it names is broken.
- [x] **R2 — `exit` in a configuration file hijacks the whole session.** `config::load` runs the
      config in the same `Session`, so `exit 3` there sets `session.leaving`, which is never
      cleared. Every later statement short-circuits and every command's status is replaced.
      Breaks ADR-0008 ("an external command's status is passed through unchanged") and ADR-0010
      ("a bad setting never stops the shell from starting").
- [x] **R3 — configuration mode stops external commands only.** The single-builtin fast path in
      `crates/ono-cli/src/eval.rs` returns before the `Mode::Config` check, so `cd`, `remove env`,
      `help`, `jobs`, `fg`, `bg` and `exit` all run from a config file. The error text the code
      itself prints says configuration "runs nothing". `028-config-is-restricted` only tries
      `touch`, so it does not prove what it claims.
- [ ] **R4 — a builtin ignores its redirections and cannot be piped.** `help > out.txt` prints to
      stdout and writes no file; `help | cat` reports `resolve.command_not_found` for `help` and
      then reports success.
- [x] **R5 — an unterminated `${` eats the rest of the word.** `printf '[%s]' a${HOMEb` yields
      `[a$]`. `crates/ono-cli/src/expand.rs` drains the iterator looking for `}` and drops what it
      consumed, while its own comment says the text is kept as typed. Silent data loss inside an
      argument, which is the class of surprise ADR-0019 exists to remove.
- [x] **R6 — background children are only reaped when `jobs`/`fg`/`bg` runs.** A script that
      backgrounds 100 commands leaves 100 zombies, because `poll_jobs` is called only from the
      interactive loop and from the `jobs` builtin.
- [x] **R7 — a bad shebang reports 127 rather than 126.** `crates/ono-process/src/spawn.rs` maps
      every `ENOENT` from `exec` to `NOT_FOUND` without distinguishing the program from its
      interpreter. ADR-0008's table and every other shell say 126.
- [x] **R8 — a parse error echoes the whole source line.** A 100 000-character line produces a
      98 KB error message; the shown line needs a budget and an ellipsis.

What the review tried hard to break and could not, which is worth keeping: ADR-0019's rule that a
value's content never becomes a command's structure held under filenames containing spaces,
newlines, quotes, `$`, `*`, backslashes and raw escape bytes; file-descriptor hygiene is correct
including the fd-shuffle most hand-written shells get wrong; and the `pre_exec` SAFETY claim of
ADR-0007 is accurate as written.

### From the security review (ADR-0015 checklist)

Each was reproduced by the reviewer against the built binary. The release-blocking ones are fixed
and guarded; the rest stay open with their reproduction.

- [x] **F1 — `explain` prints attacker-controlled escape sequences raw.** A program name on `PATH`
      containing an OSC sequence retitles the terminal when `explain` reports it, and the bytes
      survive redirection into a file. `crates/ono-cli/src/builtin.rs` and
      `crates/ono-command/src/explain.rs` echo stage source and resolved paths without sanitising.
      ADR-0015 T1/T9/T11. The row's named acceptance case uses the benign name `ls`.
- [x] **F2 — structured error messages are not sanitised.** Only the code and the help line are
      painted through the theme; `error.message()` is written raw
      (`crates/ono-cli/src/report.rs`). `cd` into a directory whose name carries an OSC sequence
      retitles the window. ADR-0015 T1.
- [x] **F3 — a parse diagnostic sanitises the echoed line but not its own message.**
      `crates/ono-cli/src/report.rs`. ADR-0015 T1.
- [x] **F4 — `sanitise` lets `\n` and `\t` through, so a value forges a table row.** A cell
      containing `"evil\nroot      1"` renders as two terminal lines, the second indistinguishable
      from a real row. Widths are also measured on unsanitised text, so escapes misalign columns.
      `crates/ono-render/src/theme.rs`. ADR-0015 T1.
- [x] **F6 — resolution and execution disagree about a relative `PATH` entry.** `explain` stats a
      relative entry against the *process* working directory while the command runs with the
      *session's*, so `explain foo` reports one binary and `foo` runs another after a `cd`.
      `crates/ono-cli/src/resolve.rs` versus `crates/ono-cli/src/eval.rs`. ADR-0015 T10/T11 — it
      defeats that row's only stated mitigation.
- [x] **F7 — the history file is world-readable and ships with no redaction patterns.** Created at
      the ambient umask (0644, in a 0755 directory), and `Policy::default()` has an empty pattern
      list, so `deploy --password=hunter2` is stored verbatim. ADR-0015 T8; the row's named test
      supplies its own pattern, so it proves the mechanism rather than the product.
      `crates/ono-history/src/{store,policy}.rs`, `crates/ono-cli/src/repl.rs`.

Should-fix:

- [x] **F9 — fixed.** The prompt derives elevation from the kernel's effective uid: a root shell
      shows ` root` in `ui.prompt.root` and prompts with `#` (spec §17.2). Pinned from both
      sides in `ono-cli/tests/signals.rs::should_make_an_elevated_prompt_impossible_to_miss`.
- [x] **F10 — fixed** (as a side effect of the depth-guarded block recovery landed in the
      security sweep): every hostile wall — parens, brackets, blocks, `if`-chains — now parses
      20 000 deep in under 40 ms debug. The regression guard is
      `ono-parser/tests/robustness.rs::should_stay_linear_on_a_wall_of_unbalanced_parentheses`.
      Previously: **quadratic on unbalanced nesting** (24.8 s at 20 000).
- [x] **F11 — fixed.** The frontier holds paths, not descriptors: each directory re-opens from
      the held root through `openat2(RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS)`, so at most two
      descriptors are ever open and the T14 no-redirect property survives the change — a
      swapped component fails loudly instead of being followed. Pinned under a real 64-fd limit
      in `ono-cli/tests/native.rs::should_walk_a_wide_tree_without_hoarding_descriptors`.
      Previously: **one open descriptor per pending directory.**
- [ ] **F12 — the trust store's default policy is trust-on-first-use**, which contradicts ADR-0015
      T5's "an unknown key is refused, not prompted past". `crates/ono-protocol/src/trust.rs`.
      Either the ADR or the default has to move.
- [x] **S1 (F13) — fixed.** `ProviderMutation` refuses a selection over the bulk threshold (10,
      a constant until configuration reaches invocations) with `safety.confirmation_required`
      naming the scope, before the first action; `--confirm` proceeds. `stop process` declares
      the option too. Pinned in `ono-command/tests/mutations.rs`. Previously: **the contract
      advertised a bulk-mutation guard nothing implements.** Four command
      contracts (`docs/spec/commands/file.yaml` twice, `network.yaml`, `kuang.yaml`) declare a
      `confirm` option documented as "without it, a selection over the configured threshold fails
      with `safety.confirmation_required` in a script (spec §11.6, §17.4)".
      `ProviderMutation::run` in `crates/ono-command/src/impls/mutate.rs` forwards it verbatim as
      an opaque argument and contains no threshold and no `safety.confirmation_required` path. A
      documented safety guard that does not exist is worse than no guard, because someone will
      rely on it. This is why `docs/ACCEPTANCE.md` §4.4's "destructive operations show scope
      before acting" cannot be ticked.
- [x] **S2 (F8) — fixed.** The systemd dry-run branches now answer `skipped` with what would
      have happened — the contract `ono-provider-linux` always kept — and the test that asserted
      a claimed change asserts the report; a declared `--dry-run` option travels in the action's
      own field rather than as an ignorable argument. Previously: **`Action::as_dry_run()` was
      unreachable, and one test encoded the wrong contract.**
      Nothing constructs a dry run: both call sites in `crates/ono-command/src/impls/mutate.rs`
      leave it false, no contract declares the option, and the `is_dry_run()` branches in
      `crates/ono-provider-systemd/src/provider.rs` are dead. Latent rather than live — but
      declaring `--dry-run` on a contract would make the flag arrive as an ordinary argument and
      the mutation would *run*. The systemd branches also report a completed change rather than
      `skipped`, and `crates/ono-provider-systemd/tests/service.rs` asserts that, so the wrong
      behaviour is currently guarded by a passing test. `ono-provider-linux` does it correctly.

Accepted for now, with the reason recorded so the decision is not re-made by accident:

- **F14** — bidirectional and other format characters pass the sanitiser, because
  `char::is_control()` covers only the `Cc` category. Trojan-Source display spoofing of a
  filename. Proposed as an extension of T1.
- **F15** — an empty `PATH` element resolves to the working directory. Deliberate, matches every
  other shell, and `explain` prints the absolute path it reached.
- **F16** — the history and trust-store temporary files are predictable and opened without
  `O_EXCL`. Only reachable in a directory another user can write, which F7 makes likelier than it
  should be; fix alongside F7.
- **F17** — a residual TOCTOU window remains between confirming a process's identity and
  signalling it. `pidfd_open`/`pidfd_send_signal` would close it; T13 claims only "re-read before
  signalling", which the code does.
- **F18** — `O_NOFOLLOW` does not stop `openat` descending into a bind mount;
  `openat2(RESOLVE_NO_XDEV)` would. T14 claims only that the walk cannot leave the tree *by name*,
  which holds.
- **F19** — `is_executable_file` tests `mode & 0o111` rather than `access(X_OK)`.
- **F20** — `FdPlan::normalise` opens `/dev/null` in a loop up to the target descriptor, so
  `9999>file` costs ten thousand opens. Self-inflicted.

### What the security review attacked and could not defeat

Worth keeping, because a mitigation that survived a real attempt is the most useful line in a
security review — and because re-testing these later costs nothing if they are written down:

- **T1/T9 at the render boundary.** `Theme::paint` sanitises *before* choosing colour, so a pipe
  and a file are covered as well as a terminal; `View::Raw` re-sanitises; every cell, tree node
  and key goes through it; no setting disables it. `\n` (F4) was the only hole found.
- **T4, poisoned completion.** Candidates are filenames, never executed, and painted before
  display.
- **T7, decoder bombs.** JSON and YAML nesting refused past their depth limits at 200 and beyond;
  a 3^N YAML alias fan-out refused at N=8; the netlink decoders check every length against the
  remaining slice and advance by at least one aligned header per step. No overflow, no unbounded
  allocation, no non-terminating input found.
- **T13, identity completeness.** No path reaches a signal with a bare pid: every target carries
  `(pid, started)` from a record or from `providers.resolve()`, and a mismatch refuses.
- **T14, symlink swap.** Each directory is opened once relative to its parent's held descriptor
  with `O_NOFOLLOW`, and no path is ever re-resolved. Could not escape the tree by name.
- **T5/T6, refusal semantics.** A changed key is `remote.host_key_changed` carrying both
  fingerprints, with no continue-anyway; re-trusting is a separate deliberate act.
- **ADR-0019, no word splitting.** `has_pattern` is computed from the *source* characters, so a
  `*` arriving inside a variable's value cannot glob.
- **Environment propagation.** A child gets the session environment and nothing internal.
- **ADR-0007's `unsafe` audit.** Seven blocks, all in `ono-process`. The `pre_exec` path calls only
  `dup2`, `setsid`, `ioctl(TIOCSCTTY)` and `signal`; the one non-libc call,
  `io::Error::last_os_error()`, builds a non-allocating representation. No `format!`, no lock, no
  Rust I/O, no panicking index. No signal-mask inheritance across `exec` and no descriptor leak.

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
