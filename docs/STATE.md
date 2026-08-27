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
at commit 21b37d9.** All ten phases of spec §37 are complete, proven and tagged; every box in
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

**The v0.3 tranche is in progress (started 2026-08-27).** v0.2 was released as `v0.2.0` from
`main` at `273d3cd`; the External Command Adaptation Layer is implemented on `implementation`
in the same loop, against the same referee. Its definition of done is `docs/ACCEPTANCE.md`
§4.6 — 39 boxes derived from v0.3 §2.1–§2.6, §1.67 and §1.68 — and `scripts/release-check.sh`
is red until every one of them is ticked by a named automated proof. **ADR-0027 carries the
analysis**: what v0.3 requires, which five existing decisions grow (ADR-0006, -0011, -0013,
-0016, -0022), where v0.2 and v0.3 read differently, and the decomposition of the tranche. Read
ADR-0027 and §4.6 before picking a task; do not re-derive them from the 2182-line document.

Build order (v0.3 §1.69, one increment per line, each one RED-first):

1. `ADAPT-001` OutputDemand computed backwards from the consumer, reported by `explain`
2. `adapter.*` error family (E09xx) in `docs/spec/errors.yaml`
3. `ADAPT-003` guaranteed raw spelling
4. `ADAPT-009` manifest schema + `docs/spec/adapters/`, `spec-check` drift rules
5. `ADAPT-002` registry, identity pinning, negotiation states, conflict resolution
6. `ADAPT-004`/`005` plan execution through `ono-process`, streaming decoders, fuzz corpus
7. `ADAPT-007` provenance on adapted values, `inspect --provenance`
8. `ADAPT-006` version probe cache
9. `ADAPT-010` fixture harness generated from the contracts
10. Tier A tools: util-linux (`lsblk`/`findmnt`/`lsns`), `ip`, `journalctl`, `systemctl`
11. Tier B tools: `ps`, `stat`/`df`/`find`, `git`, `lsof`
12. Tier C tools: `ss`, `curl`
13. `ADAPT-008` KUANG/11 capability mapping, SDK, test host, packs and trust
14. `ADAPT-011` remote negotiation
15. integration surfaces: completion, history, script determinism, muscle-memory diff
16. release evidence: reference pages + compat matrix, overhead measurement, README

The container image gains the tools of step 10–12 when step 10 starts; a tool adapter is not
delivered until its live case runs there (`docs/ACCEPTANCE.md` §4.6.3).

---

## Product direction from the user (2026-08-26)

**"Es muss immer cool sein und Spaß machen, es zu benutzen. Es soll aufregend sein."** The shell
is the Ono-Sendai deck: correctness is the floor, not the ceiling. Where a decision is
otherwise free, prefer the option that feels alive — the prompt as a HUD, tables that update in
place, colour that means something, latency you never notice (spec §34's budgets are product
quality), and answers that invite the next question (`@2 | inspect`). Phase F's `watch` is the
showcase: a live view of the machine should feel like instrumentation, not like polling.

## In progress

- [language | 2026-08-27] **language family** (`crates/ono-cli/tests/language_missing.rs`) on
  branch `implementation-language` — **all 31 tests green and un-ignored**; the gate is green
  at every commit. Delivered: `let`/`( … )`/`$( … )` capture (ADR-0069); callable functions and
  `alias` (ADR-0070); `now()`, the RFC 3339 timestamp literal, prefix assignment
  `NAME=value cmd`, `each { … }` blocks, string `+`, keyless `sort`, `kill %N` (ADR-0071).
  Acceptance case `docker/acceptance/cases/035-scripting-language.case` is written and
  dry-run against the binary; **the integrator runs it in the container** when merging the
  branch. Left open (not in the RED suite): `explain` of a `NAME=value cmd` stage, functions
  and aliases in completion candidates, a function in a non-head pipeline position.
- [watch | 2026-08-27] **`watch`/`trace` for the declared-but-unbound targets** — done for
  file, user, group, interface, route and mount (ADR-0078..0080; commits on
  `implementation-watch`). Left ignored: the remote five in `remote_missing.rs` (`watch
  link|host`, `trace link|host`) — they need `link`/`host` as provider-backed records first
  (context.rs `get_link` renders by hand); the remote family picks them up.

- [agent | 2026-08-27] **RED suites for everything v0.2 declares but does not build** (user
  request; wiki pages "Command Index" and "What Is Not Built Yet"). 329 outcome tests, every
  one `#[ignore = "REASON: …"]` (AGENTS.md §7) so the tree stays green; **the increment that
  delivers a family removes the ignore lines of its tests in the same commit** — a family is
  done when its file has no `#[ignore` left and the gate is green. Work order: cross-cutting
  seams first (registry-dispatched `set`/`remove`, ActionResult exit status and error shape,
  generic `enter`/`watch`/`trace` for object targets), then the families. Each file is one
  family; each test asserts the behaviour the contract promises, never mere presence:
  - `crates/ono-cli/tests/files_missing.rs` (34) — read/write/copy/move/remove/set/open/tail/
    watch/trace/enter file, remove/set dir, globs for native selectors
  - `crates/ono-cli/tests/language_missing.rs` (31) — `let` capturing a pipeline, `$(…)`/`(…)`
    values, callable `fn`, `alias`, `now()`, timestamp literals, `FOO=bar cmd`, `each { … }`,
    string `+`, keyless `sort`, `kill %N`
  - `crates/ono-cli/tests/options_and_selectors_missing.rs` (15) — `--user/--tree`, `find file`
    options, `--mounted`, `trace socket --port`, `--human`, `get user 0`, `where local.port`
  - `crates/ono-cli/tests/meta_config_missing.rs` (24) — `resolve command`, `get config` layers/
    source/line, `set config` typed + effective (`render.table.max_rows`)
  - `crates/ono-cli/tests/processes_missing.rs` (18) — `inspect process`, `get job`, `enter
    process`, `set process --priority`, `send signal`, failed ActionResult ⇒ exit 1 (ADR-0006)
  - `crates/ono-cli/tests/identity_missing.rs` (25) — `get session`, user/group mutations,
    watch/trace/enter user|group
  - `crates/ono-cli/tests/network_missing.rs` (31) — `resolve dns`, `test port`, watch/trace/
    enter interface|route|socket, route/interface/socket mutations
  - `crates/ono-cli/tests/services_logs_missing.rs` (15) — `set service`, `get journal`,
    `tail journal`, `get log` — **13/15 done** by [services | 2026-08-27] on branch
    `implementation-services` (ADR-0084–0086, case 038); the two still ignored wait on the
    language (`now()`, bare words compared against a string field), see Deferred
  - `crates/ono-cli/tests/storage_missing.rs` (22) — `get device`, mount/unmount, mount verbs,
    watch/trace/enter mount
  - `crates/ono-cli/tests/data_missing.rs` (15) + `crates/ono-command/tests/completion_missing.rs`
    (6) — `tail`, `join`, `diff`, stacked records on narrow terminals, fields after `where`
    — **done** by [data | 2026-08-27] on branch `implementation-data` (ADR-0072–0074); no
    `#[ignore` left in either file
  - `crates/ono-cli/tests/remote_missing.rs` (36) — `get link` as data, host commands, link
    definitions, detach/rename, agentless visibility, mutations across a link
  - `crates/ono-cli/tests/plugins_missing.rs` (32) — `ono.plugin/1` records, inspect/find/
    verify/install/unload/set/remove plugin, capabilities, audit, reload, assistants/models
  - `crates/ono-cli/tests/containers_packages_missing.rs` (25) — a fake engine-API socket and
    fake package managers on PATH; E0401 when none answers

  Wiki claims found stale while writing them (already work, no test added): `get route
  --table/--family`, `format --max-rows`, backgrounding native stages, `let i = $i + 1`.

  Contract gaps the suites had to resolve by reading — each needs an ADR (or a registry change)
  before its GREEN increment: `alias` statement syntax (grammar.ebnf/language.yaml have none);
  `ono.command/1` resolution `kind` field; `set config` unknown key ⇒ E0202; `ono.device/1`
  shape (path/kind/major/minor); `ono.session/1` fields; `ono.link/1` lacks a `host` field;
  `ono.container/1`, `ono.image/1`, `ono.package/1` schemas and the runtime knobs
  (`DOCKER_HOST`/`CONTAINER_HOST`, managers found on PATH); `get journal`/`get log` referenced
  `ono.log-record/1` which neither existed nor was deferred (resolved: ADR-0085/0086);
  `join`/`diff` output shape and
  `--identity [pid]` spelling; failed ActionResult rows nest the error as
  `error.error.code = "io.permission_denied"` instead of `error.code = "Ono-Sendai-E…"`, and
  `operation` carries the bare verb instead of the command id; K11 codes not folded into
  `Ono-Sendai-K11xxx`; `--agentless` is accepted and ignored by `context.rs::link`.

---

## Next up (ordered)

- [ ] Acceptance case 049 (`a remote link answers from the other side, visibly`) ended with
  exit 129 (128+SIGHUP) three times in full-suite runs on 2026-08-27 while five agent builds
  loaded the machine, and passes alone and through the harness in isolation: `exit` with a live
  link races the hangup `script` sends when its piped stdin reaches EOF. Confirm under low load;
  if it recurs, make link teardown at exit not wait on the agent — exit test: the case green in
  three consecutive full runs
- [ ] The specification's `where state == failed` (§33.2, §41.4), `where status == failed`
  (§16.5) and `where level >= error` (§41.4) all fail with E0202: ADR-0009 makes a bare
  identifier in expression mode a field path, so the word is looked up as a field. Decide —
  an ADR superseding ADR-0009 in part — whether an unknown bare word compared against a string
  field is that word (the check of spec §11.3 knows the schema, so it can tell), then un-ignore
  `services_logs_missing.rs::should_run_the_failed_service_example…` — exit test: a
  language case running `get service | where state == failed`

- [ ] `explain get process` inside a frame prints the narrowed spelling (`get process 1`,
  `get process --user root`) — ADR-0023 promised it, ADR-0076 made the arguments available —
  exit test: a context.rs case explaining inside `enter process 1`
- [ ] `watch` composes `producer::ambient_selector` into its query on top of the narrowed
  arguments of ADR-0076 §5; move it onto the argument seam and delete the query-level form —
  exit test: a watch.rs case inside `enter process 1` (watch family)

- [ ] `to bytes --field body` refuses a record ("a record has no raw byte form") although
  `--field` names the one bytes field to emit — the natural way to write an adapted `curl`
  body to a file (`adapt curl url | to bytes --field body > page.html`) — exit test: a
  data_codecs case emitting one bytes field verbatim
- [ ] `explain` resolves a head by the registry alone, so `printf x | sort` is planned as
  `ono.data.sort` while the executor runs `/usr/bin/sort` (ADR-0028); the plan must use the
  executor's resolution — fixed with ADAPT-002, which needs it anyway (ADR-0052) — exit test:
  a builtins.rs case explaining `printf x | sort`
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
- [ ] Remaining `*-event/1` schemas (container, link, host) — each un-deferred as its watch is
  exercised; service, socket, interface, route, mount, file, user and group are written
  (ADR-0034, ADR-0078..0080) — exit test: a watch case per target
- [ ] `trace mount` propagation peers (storage.yaml promises them; ADR-0079 leaves them out
  until a peer group has an object to stand for it) — exit test: a storage case over a bind
  mount with `shared:` in mountinfo
- [ ] `watch file` through inotify and `watch interface|route` through the rtnetlink multicast
  groups, switching `source` to `subscription` (ADR-0078, ADR-0080) — exit test: a file
  created between two polls arrives before the next poll would
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
      `crates/ono-value/tests/` — ADR-0016 — commit d020129
- [x] B2 — Schema model and registry, the canonical schemas of spec §28, compatibility rules —
      `crates/ono-value/tests/{builtin_schemas,schema_compatibility}.rs` — commit d020129
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
      `schemas/*.v1.yaml`, `commands/*.yaml` — ADR-0012 — commit 6b107d0
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

- [x] seams 1 — `set`/`remove` of system targets dispatch through the registry — commit 7ec0d83
  (ADR-0068 §1; `crates/ono-cli/tests/builtins.rs`)
- [x] seams 2 — ActionResult contract: a failed row exits 1, a missing target is an E0301 row,
  `operation` is the command id, `error` is a flat `ono.error/1` — ADR-0068 §2;
  `processes_missing.rs` (3 tests), `remote_missing.rs` (2 tests) un-ignored
- [x] seams 3 — a mutating verb binds when a provider advertises its capability
  (`builtin_commands_for`, ADR-0068 §3); `crates/ono-command/tests/mutations.rs` (4 tests).
  Families deliver a mutation by advertising the capability and answering the verb in `act`:
  `set service` now reaches the systemd provider, which reports it has no `set` operation
  (E0402 row) until the services family maps `--enabled` onto enable/disable and reports a
  missing property as E0201 naming `--enabled`
  (`services_logs_missing.rs::should_refuse_set_service_without_a_property…` stays ignored).

- [x] services 1 — `set service <unit> --enabled true|false` reaches the systemd provider as the
  `set` operation with the property as an argument (EnableUnitFiles/DisableUnitFiles); a `set`
  with no property is E0201 naming `--enabled` before anything is resolved (ADR-0084);
  `services_logs_missing.rs` (4 `set service` tests un-ignored),
  `ono-provider-systemd/tests/service.rs` (2 tests)
- [x] services 2 — `get journal [--since --boot]` and `tail journal [--lines]` as
  `ono.journal-event/1` through `journalctl --output=json` and the systemd adapter pack's
  decoder (ADR-0085); a provider-kind stream failure exits 1; `StreamSink::closed()` lets a
  following producer stop when `take` is satisfied; the decoder reads journalctl's byte-array
  and multi-valued strings (fix); `services_logs_missing.rs` (6 journal tests un-ignored)
- [x] services 3 — `get log [--service <ref>] [--level <name>] [--since --until]` as
  `ono.log-record/1` (journal-event plus `level`, the severity name) from the same journal
  provider (ADR-0086); case 038; `services_logs_missing.rs` 13/15 green — the two ignored
  cases wait on the language (`now()`, bare words as strings), see Deferred
- [x] data family (ADR-0072) — `tail N [--follow]` (commit 0f68fe0), `join <right> --on key
  --kind inner|left|right|outer` with `$variables` and pre-run `(pipelines)` visible to native
  stages (1616fe1), `diff <right> [--identity [fields]]` by schema identity (1761cc9),
  stacked records once a cut column would drop below eight cells (ADR-0073, 98437d4),
  schema fields with their docs after `where`/`select` (ADR-0074) —
  `crates/ono-cli/tests/data_missing.rs` (15/15), `crates/ono-command/tests/
  completion_missing.rs` (6/6), case 036
- [x] the context stack for every object target — `enter` of process/user/group/interface/
  socket/mount/file by word or by pipe (`get socket 443 | enter socket`), frames narrowing
  every later command at the command-table seam (`pid 1`, `--user root`, `--interface lo`,
  `--port 443`), `--user`/`--group` honoured by the procfs provider, `--interface` declared on
  `get route`, `--port` honoured by `trace socket` — ADR-0075, ADR-0076 — 24 tests un-ignored
  in `crates/ono-cli/tests/{processes,identity,network,storage,files}_missing.rs`

- [x] v0.3 step 1 — ADAPT-001 OutputDemand computed backwards from the consumer, reported
  by `explain` (ADR-0052) — cases 070, 071
- [x] v0.3 step 2 — the `adapter.*` error family E0901–E0911 in `docs/spec/errors.yaml` and
  `ono_core::ErrorCode` (ADR-0053) — `error_taxonomy.rs`; the box in ACCEPTANCE §4.6.2 stays
  open until an adapter emits one with the §1.65 payload
- [x] v0.3 step 3 — ADAPT-003 the `raw` keyword; `adapt` spelled for §1.18 (ADR-0054) —
  case 072
- [x] v0.3 step 4 — ADAPT-009 the declarative adapter contract, the util-linux pack with
  fixtures, `ono.block-device/1` and `ono.namespace/1`, the validator and the spec-check rule
  (ADR-0055) — `ono-adapter/tests/contracts.rs`, `xtask/tests/contracts.rs`
- [x] v0.3 step 5 — ADAPT-002 registry, negotiation states, identity pinning, conflict
  resolution, the probe cache of ADAPT-006, and `explain`'s `adaptation`/`argv`/`candidates`
  rows (ADR-0056) — `ono-adapter/tests/negotiation.rs`, case 073
- [x] v0.3 step 6 — ADAPT-004/007/010 and COMPAT-LSBLK/FINDMNT/LSNS: adapted execution
  through `ono-process`, the json decoder, adapter provenance in `inspect`, the fixture
  harness in `spec-check`, util-linux end to end (ADR-0057) — `ono-cli/tests/adapters.rs`,
  cases 074, 075. ADAPT-005's streaming half waits for the first line-protocol tool.
- [x] v0.3 step 7 — COMPAT-IP: the iproute2 pack, `ono.interface-address/1`, the field-map
  derivations children/template/first/infer/literals/require (ADR-0058) — case 076
- [x] v0.3 step 8 — ADAPT-005 streamed adaptation (`Decoding`, `Output::Pipe`,
  `start_piped`/`finish_foreground`, cancellation to the producer), the systemd pack
  (journalctl jsonl, systemctl list-units/show with the `properties` decoder),
  `ono.journal-event/1`, the live view absorbing plain records; the image gains git, curl,
  lsof (ADR-0059) — case 077
- [x] v0.3 step 9 — COMPAT-PS: the procps pack, whitespace columns, `first` on strings,
  `program-name`/`started-from-elapsed` inferences, streaming `lines` (ADR-0060) — case 078
- [x] v0.3 step 10 — COMPAT-STAT/DF/FIND: the coreutils and findutils packs, trailing argv,
  header lines, basename, NUL records with the path last, typed-order pass-through
  (ADR-0061) — case 079
- [x] v0.3 step 11 — COMPAT-GIT/LSOF: builtin decoders `git-status-v2` and `lsof-fields-v1`,
  `ono.git-status-entry/1`, `ono.commit/1`, `ono.open-file/1`, hex escapes (ADR-0062) — case 080
- [x] v0.3 step 12 — COMPAT-SS: combined flags, nested record coercion, the `ss-text-v6`
  version-constrained parser, required flags as specificity (ADR-0063) — case 081
- [x] v0.3 step 13 — the `adapt` keyword of §1.18 (E0911 when nothing answers) and
  COMPAT-CURL: `ono.http-exchange/1`, the `curl-exchange-v1` decoder with the body kept as
  exact bytes, secrets never adapting (ADR-0064) — case 082
- [x] v0.3 step 14 — ADAPT-008: `contributions.adapters`, the `executables`/`argv_policy`
  scope of `process.exec`, packs loaded disabled under default deny and enabled by
  `--grant process.exec` (experimental packs by `--allow-experimental` besides), the test
  host's `check_adapter_package`, the SDK's example package (ADR-0065) — case 083
- [x] v0.3 step 15 — ADAPT-011: the `start-adapt` frame, the agent negotiating, running and
  decoding on its side, records marked with the host, `explain … on <host>`, visible
  degradation (ADR-0066) — case 084
- [x] v0.3 step 16 — integration surfaces: adapted stages are producers for the pre-flight
  check, `type`, completion and history; text tools pinned raw; the §1.71 session, script
  determinism and the muscle-memory diff as cases (ADR-0067) — cases 085, 086, 087
- [x] v0.3 step 17 — release evidence: generated adapter reference pages and the compatibility
  matrix, live conformance for every first-party adapter (case 088), measured overhead
  (case 089), the README section with examples that parse and run under xtask — all §4.6
  boxes ticked

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
      `timeout:` and repeatable assertions, with a self-test case — commit 036f89c
- [x] The gate refuses untracked unfinished work: `todo!()`, `unimplemented!()`, untracked
      `TODO`/`FIXME`, `#[ignore]` without a reason — `xtask/tests/scan.rs` — commit 6f7c308
- [x] `ono-testkit`: real-binary runs with a deadline, scratch directories, and a reproducible
      generator for fuzz-style tests — commits b2a0d2d, a20056c
- [x] `ono-render`: width-aware table and stacked-record layout, semantic theme tokens, the
      presentation contract of spec §4.6, and the ASCII tree of §22.4 — commits bb2d825,
      a3d3fac, 37f78de
- [x] `ono-history`: semantic entries, restart survival, secret policy — commit 0b1def8
- [x] A0 — Shared vocabulary in `ono-core`: `Span`, the complete error taxonomy of spec §43,
      the exit-status contract — ADR-0005/0006/0008 — commit 5551654 —
      tests `crates/ono-core/tests/{error_taxonomy,exit_status,span}.rs`
- [x] A0 — The concrete grammar: ADR-0009 and `docs/spec/grammar.ebnf`, resolving the
      command/expression ambiguity of spec §26.1 with the two argument modes

---

## Known defects (found by adversarial review, 2026-08-26)

Two independent reviewers were asked to falsify the implementation rather than describe it, as
AUTONOMOUS_IMPLEMENTATION.md §18 requires. Between them they found 27 things, each with a
reproduction they ran.

**Everything release-blocking is fixed**, each with a regression test that fails without the fix
(commits 0742918, aeae961). A ticked box below means fixed *and* guarded. What remains unticked is
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

- `get journal --since (now() - 1h)` — needs `now()`, which the language family delivers
  (`language_missing.rs`); the option itself is delivered — ignored test:
  `crates/ono-cli/tests/services_logs_missing.rs::should_only_emit_recent_events_when_since_is_a_relative_timestamp`
  — ADR-0085
- `get log --service X | where level >= error | take 20` (spec §41.4) — the command runs; the
  bare word `error` is a field path under ADR-0009 and E0202 before execution, as the spec's
  `where state == failed` examples are — ignored test:
  `crates/ono-cli/tests/services_logs_missing.rs::should_run_the_failed_service_example_when_a_level_threshold_composes`
  — ADR-0086

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
