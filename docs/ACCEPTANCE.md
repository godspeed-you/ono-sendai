# ACCEPTANCE

**This document defines when the work is finished. An agent run ends when
`scripts/release-check.sh` passes — and not one step earlier.**

There is no MVP milestone in this project and no proof-of-concept exit. A shell that
demonstrates the idea is not the deliverable; a shell a person can set as their login shell and
keep is the deliverable. Everything below is written so that a machine, not a judgement call,
decides whether that has been reached.

---

## 1. The three referees

| Referee | Command | Answers |
|---|---|---|
| Quality gate | `scripts/gate.sh` | is the code correct, linted, tested, documented, contract-consistent? |
| Acceptance suite | `scripts/acceptance.sh` | does the real binary do the real thing in a clean Linux container? |
| Package check | `scripts/package-check.sh` | do the `.deb` and `.rpm` of `scripts/package.sh` install, run and register the login shell in fresh Debian and Fedora containers? |
| Release gate | `scripts/release-check.sh` | are the first three green **and** is the checklist in section 4 fully ticked? |

The quality gate proves the code is sound. It cannot prove the product exists: unit tests pass
in a workspace that has never been installed anywhere. That is what the container is for.

## 2. Why the container is the referee

`scripts/acceptance.sh` builds `docker/Dockerfile` and executes every case in
`docker/acceptance/cases/` against the `ono` binary inside the resulting image, as an
unprivileged user whose login shell is `ono`, with networking disabled.

This is deliberately hostile to the ways a shell can be secretly broken:

- it is a machine that has never seen the source tree, so nothing depends on the build directory;
- the user is not root, so privilege boundaries are met the way real users meet them;
- the process table, services, sockets, mounts and users are the container's own, so provider
  output is checked against a system nobody tuned for the test;
- `ono` is the login shell of a real account, so it must survive being started as one.

**A capability is not delivered until an acceptance case proves it in the container.** Passing
unit tests are necessary and not sufficient. Add the case in the same increment as the feature.

## 3. What each case must be

Cases assert what a user observes — printed output, exit status, resulting system state — never
how the result was produced (AGENTS.md section 11). A case that has to change when the
implementation is restructured is a defective case.

Format and directives: see `docker/README.md`.

## 4. Release checklist

Tick an item only when it is proven by an automated case or test that runs in the gate. An
unticked box means `scripts/release-check.sh` fails, which means the run continues.

### 4.1 Phase completion

Each phase's success criterion from spec section 37, each proven by a named acceptance case.

- [x] **A — Unix shell foundation.** `ono` can be used interactively in place of Bash for
      ordinary work: parsing, quoting and escaping, environment variables, `cd`, redirection,
      external commands and pipelines, exit status, signals, job control, history, config.
      Proven by `010-replaces-bash-for-ordinary-work`, with `020`–`029` covering each part:
      external commands and status, cwd and environment, redirection, pipelines, PTY ownership,
      quoting and expansion, an interactive session, job control, history across a restart,
      quiet startup, restricted configuration, and the prompt.
- [x] **B — Value system and native pipelines.** Typed values flow end to end through `where`,
      `select`, `sort`, `take`, `skip`, `each`, `count`, `measure`, with JSON/YAML/CSV/text
      conversion, backpressure, and rendering separated from data.
      Proven by `041-typed-values-flow-end-to-end` (every phase-B transform and format against
      parsed data) and `040-object-pipeline` (the §12.3 boundary in both directions).
- [x] **C — Linux core providers.** `process`, `file`/`dir`, `user`/`group`, `env`,
      `mount`/`filesystem`, `interface`/`route`/`neighbor`, `socket`/`connection` and
      `service` are answered from the kernel and systemd, not from parsed text.
      Proven by `042-inspection-without-text-parsing` (a typed field selected from each), with
      `crates/ono-cli/tests/provider_conformance.rs` — generated from `docs/spec/providers/*.yaml`
      — pinning what each provider advertises.
- [x] **D — Consistency and discoverability.** Command, verb and schema registries exist under
      `docs/spec/`; `help`, completion, `type`, `inspect` and `explain` are driven by them;
      `docs/reference/` is generated from them, and so is the provider conformance suite of spec
      §35.3: `crates/ono-cli/tests/provider_conformance.rs` is generated from
      `docs/spec/providers/*.yaml` and `docs/spec/schemas/*.v1.yaml` by `cargo xtask
      conformance`, so nothing a provider advertises — a target, a schema, a capability, an
      identity strategy — can go unexercised, and `spec-check` fails when the committed suite is
      not what the registries produce (ADR-0331).
      Proven by `043-discoverable-from-the-shell` (help, type, inspect, explain from the
      registries; explain never executes) and `044-semantic-completion` (a declared target
      completed on a real terminal), with `docs/reference/` staleness and provider drift both
      failing the gate.
- [x] **E — Contextual systems interface.** Context stack, `enter`/`leave`, object-aware
      selectors, prompt and HUD, interactive selection, structured reuse of recent results.
      Proven by `045-context-and-reuse` (enter/leave/get context, the §14.5 explicit spelling,
      result reuse by `@-1` and position by `@N`), with the object-narrowing mechanics pinned by
      `ono-command/tests/producers.rs` (a frame narrows the provider query; an unnarrowable
      target refused naming the frame) and `ono-cli/tests/context.rs`. Selection is positional
      addressing per ADR-0033; the visual cursor is Phase J's.
- [x] **F — Live system semantics.** `watch`, the event/snapshot model, in-place rendering,
      native background jobs, stable object identity.
      Proven by `046-live-system-semantics` (snapshot events with `source: poll` through a
      serializer, the piped refusal naming the fix, a live view at a real terminal, a
      backgrounded watch listed running, a bounded background pipeline finishing as done),
      `ono-command/tests/watch.rs` (the poll diff over a mutating fixture: snapshots first,
      `changed` naming its fields, identity-ordered), and `ono-cli/tests/watch_live.rs` (in-place
      repaints, Ctrl-C ending a watch with 128+SIGINT, `fg` reattaching a backgrounded watch).
- [x] **G — Relationship graph.** Graph values, relationship providers, `trace` for process,
      service and socket, tree and graph rendering, provenance and confidence.
      Proven by `047-relationship-graph` (a trace drawn as a tree, the graph value serialised
      and inspectable, a trace of nothing refused, every edge naming its asserter),
      `ono-graph/tests/` (relationships, rendering, the record round trip) and
      `ono-command/tests/trace.rs`. One trace is one observation (ADR-0035): 27.2s → 1.1s.
- [x] **H — Remote links.** Remote protocol, agent, SSH fallback, provider negotiation, security
      model, remote prompt, multiplexed streams.
      Proven by `049-remote-link` (a link to a real child agent: created, listed, entered with
      the prompt naming the host, answering with remote provenance, refused when never made) and
      offline by `ono-remote/tests/{agent,provider,trust,subprocess}.rs` — negotiation, mounted
      providers, interleaved cancellable streams, E0603/E0702 refusals, the exact ssh argv
      (ADR-0036, ADR-0037). The two gaps this box used to defer are closed:
      `170-agentless-link-is-visibly-reduced` proves §21.3's fallback — a reduced set really
      reading a far side with standard commands, and refusing by name what it cannot answer
      (ADR-0351, ADR-0352) — and `171-authenticated-link-refuses-a-changed-key` proves §21.5 on a
      transport that certifies its peer: an unpinned host refused, a pinned host linked, a changed
      key refused with `Ono-Sendai-E0603` (ADR-0353, ADR-0354, ADR-0355).
- [x] **I — KUANG/11 extension runtime.** The production path of spec section 31: manifests,
      capability model, isolation, host API, contribution model, audit trail, SDK, test host and
      conformance suite.
      Proven by `050-kuang-plugin` (a real package discovered, loaded under the default-deny
      broker, its contribution streaming through a pipe, `ono.*` refused at validation) and the
      65-test conformance suite covering every §31.74 area: manifest validation, the four
      capability-denial paths, cancellation, backpressure, quota exhaustion, protocol-violation
      quarantine (ADR-0040, ADR-0041, ADR-0051). Deferred increments are on the board.
- [x] **J — Advanced TUI views.** Only where semantics justify them, per spec section 37.
      Delivered as ADR-0050 argues §37 J's own discipline demands: `view` — the object picker
      whose cursor sets `@`, the inspect pane, the navigable tree over graphs, the deterministic
      plain fallback off-terminal — proven by `ono-cli/tests/view.rs` through a real PTY (pick,
      pane, quit, `@ | to json` answers the picked row). What J deliberately does not build is
      recorded with reasons in the ADR.

### 4.2 Per-capability quality bar (spec section 50)

For **every** advertised command, in the container:

- [x] `help` is complete for every command, and every documented example parses and executes —
      spec-check refuses a non-planned contract without summary and examples and an example that
      does not parse; `ono-command/tests/examples.rs` refuses one that does not bind; the
      acceptance cases 040–047 execute the load-bearing examples against the real system.
- [x] Completion produces correct candidates for every command, option and argument position —
      candidates are registry lookups, never lists that can drift (`ono-command/src/complete.rs`),
      pinned per position in `ono-command/tests/completion.rs` and on a real terminal by case
      `044-semantic-completion`.
- [x] Every command's output schema is inspectable via `inspect`/`type` and matches what it
      emits — `type` answers from the contract without running anything, `inspect` from the
      value; the provider conformance suites validate every emitted record against the schema it
      claims (`ono-provider-*/tests/schemas.rs`, `spec-check` drift rules).
- [x] Behaviour is deterministic when output is redirected or the terminal is not a TTY —
      `034-redirected-output-is-deterministic` runs the same script to a terminal, to a file and
      through a pipe and requires all three to be byte-identical, with no escape sequence
      reaching a file.
- [x] Every failure is a structured error of the taxonomy in spec section 43, never a bare
      string — `033-errors-are-structured` checks that each failure names a code of the form
      `Ono-Sendai-ENNNN`, for the resolution, I/O, parse and type families.
- [x] Privilege boundaries and race conditions are covered by tests, including denial paths —
      a kernel-refused signal is a structured failure outcome
      (`ono-provider-linux/tests/process.rs`), a recycled pid is confirmed and refused
      (T12/T13), the terminal-handover race is closed from both sides
      (`ono-process/tests/terminal_control.rs`), the mid-walk swap stays inside the tree under
      the two-descriptor walk (T14, F11), elevation is kernel-derived in the prompt (T15), and
      the KUANG broker's four denial paths are each a conformance case (§31.74).
- [x] No provider parses unstable human-readable text except where declared an adapter fallback
      — no provider crate spawns an external tool at all: every answer comes from procfs,
      netlink, statvfs, NSS or D-Bus, and `docs/spec/providers/*.yaml` pins the surface.
- [x] Unknown data is `null`, never fabricated and never silently zero — the provider
      conformance suites assert it per field (`ono-provider-linux/tests/schemas.rs`, the
      unreadable-cmdline and hidden-fd cases), and `ono-value` property tests pin the
      three-valued semantics (ADR-0014).
- [x] Output looks intentional in an 80-column and in a 200-column terminal —
      `ono-render/tests/table_layout.rs` pins alignment, display-cell measurement, truncation
      with its ellipsis, and the stacked fallback for narrow widths; acceptance cases run the
      shell at 80 (`029`) and 100–120 columns with byte-identical redirected output (`034`).

### 4.3 Performance (spec section 34)

Measured in the container, on the pathological fixtures of spec section 34 — tens of thousands
of processes and paths, slow NSS, high-latency links, huge stdout, unbounded streams. Case `060`
measures an ordinary container and case `100` a host with twenty thousand paths; cases `151`
(processes), `152` (sockets), `153` (a deep and wide filesystem) and `154` (a stalled provider,
huge stdout and an endless stream) build the rest (ADR-0333):

- [x] cold start < 100 ms (target < 50 ms) — `060-performance-budgets`, measured as a median of
      40 runs in the container and asserted against the 50 ms *target*, not the 100 ms cap
- [x] warm prompt < 30 ms — the prompt is one segment of the frame that
      `ono-editor/tests/latency.rs` bounds at ~140 µs per keystroke, two orders under budget.
- [x] keystroke to render < 8 ms typical — `ono-editor/tests/latency.rs` drives 7 000 keystrokes
      with a full frame each in under a second.
- [x] first completion results < 50 ms from local metadata —
      `ono-command/tests/completion.rs::should_stay_far_inside_the_first_completion_budget`:
      a thousand registry completions in under a second.
- [x] parse and highlight update < 5 ms for ordinary command lines — `060-performance-budgets`
      bounds a whole pipeline run, startup included, at 50 ms; the parser's own measurement
      (2.4 microseconds for a four-stage line) is in `crates/ono-parser/tests/robustness.rs` and
      the editor's keystroke-to-frame budget in `crates/ono-editor/tests/latency.rs`
- [x] first rows of `get process` < 50 ms — measured in the container by
      `060-performance-budgets` (`first-process-row`), green in the 35-case suite.
- [x] renderer updates only when state changes — the live model reports an event carrying the
      already-shown state as no change (`ono-cli/src/live.rs` tests), and the frame loop
      repaints only on change (spec §4.4).
- [x] every pathological environment section 34 names exists and the budgets are measured on it
      — `151-pathological-processes` (10 000 processes: cold start, first process row),
      `152-pathological-sockets` (5 000 listening sockets: first socket and connection rows),
      `153-pathological-filesystem` (50 000 entries in one directory, 200 levels deep, 100 000
      files over 1 000 directories), `154-pathological-streams-and-providers` (a tool on `PATH`
      that never answers, 100 MB of stdout, `watch process` under a bound). Each case prints the
      size it reached and every figure it measured, whether it passed or failed, and fails when
      the environment it built is not pathological (ADR-0333).

### 4.4 Interoperability and safety

- [x] External programs run correctly under a PTY, including full-screen applications —
      `024-pty-applications` proves the child owns the real terminal, and
      `031-full-screen-application` runs a pager that takes the terminal into raw mode and the
      alternate screen, draws, and gives it back with the shell still usable.
- [x] Signals, job control and process groups behave as they do under Bash for foreground work —
      `030-signals-and-process-groups` proves the foreground command runs in its own process
      group and dies when that group is signalled; `025-job-control` covers background, `jobs`
      and `fg`; and `crates/ono-cli/tests/signals.rs` drives a real Ctrl-C through a real
      pseudo-terminal, where the shell survives it, the command does not, and the status is 130.
- [x] Text, bytes and objects are never silently confused at an interop boundary — objects
      aimed at a child process are `type.mismatch` naming `to json` (spec §12.3), a value seed
      feeds a program only when it already is text or bytes, and bytes become objects only
      through an explicit `from` (spec §12.4). Pinned in `ono-cli/tests/native.rs` and case
      `040-object-pipeline` both ways across the boundary.
- [x] Destructive operations show scope before acting; privilege and remote target are visible.
      `032-resolution-is-inspectable` covers the resolution half — which binary a name reaches,
      including a shadowing one earlier in `PATH` (ADR-0015 T10, T11); the bulk guard names the
      scope and refuses before the first action (`ono-command/tests/mutations.rs`, spec §17.4);
      elevation is in the prompt from the kernel's own answer (`ono-cli/tests/signals.rs`,
      §17.2); and the active remote target replaces `local` in the prompt entirely
      (`049-remote-link`, §14.4).
- [x] Fuzzers run clean over parser, serializers, remote protocol, plugin protocol and the
      procfs/netlink decoders — spec §35.6's five areas, one target each, in `fuzz/`
      (`ono_fuzz::TARGETS`, ADR-0313). Each is a function from arbitrary bytes that must return
      without panicking; a mutation engine drives them from a committed seed corpus, a bounded
      run of 400 iterations per target is a step of `scripts/gate.sh`, and every past finding is
      a committed artifact that `fuzz/tests/corpus.rs` replays on every `cargo test --workspace`.
      `fuzz/tests/corpus.rs::should_have_one_target_for_every_area_the_specification_names`
      keeps the list from drifting from §35.6, and
      `should_find_a_planted_panic_and_write_it_where_it_can_be_reproduced` keeps the harness
      itself from going quietly deaf. The first campaigns found three defects and fixed them
      (two parser stack overflows, one quadratic YAML refusal); the fourth is open and named in
      ADR-0313. The bounded run finds panics, not proofs of absence — ADR-0313 §2 says what it
      cannot do and what would lift it. The seeded property and robustness suites stay as they
      are and cover a different thing: `ono-parser/tests/robustness.rs`,
      `ono-value/tests/codec_fuzzing.rs`, `ono-protocol/tests/{fuzz_protocol,framing}.rs`,
      the kuang conformance garbage/oversize/misframe cases, and
      `ono-provider-netlink/tests/malformed_messages.rs` assert *which* answer a hostile input
      gets, which a fuzz target cannot.
- [x] The threat model of spec section 49 has a test for each stated risk — the T1–T15 table of
      ADR-0245, which supersedes ADR-0015 by replacing every row's stated *intention* with the
      name of a test function that exists, runs in the gate and is not ignored (T1/T9 render
      sanitisation, T2 the raw byte boundary, T3 KUANG/11 denial paths, T4 hostile completion
      candidates, T5/T6 the trust store, T7 bounded frames and decoders, T8 history redaction,
      T10/T11 resolution and PATH shadowing, T12/T13 identity before mutation, T14 the symlink
      walk, T15 the elevated prompt). The rows are read back by
      `xtask/tests/spatial_evidence.rs::should_find_every_test_the_threat_model_names`, so a
      renamed or ignored proof turns the gate red rather than leaving this box ticked by nothing.

### 4.5 Delivery

- [x] `ono` installs and runs as a login shell in the container as an unprivileged user —
      `003-login-shell` and every interactive case, which run as the unprivileged `case` user.
- [x] Startup loads no plugin eagerly and queries no network-backed configuration —
      `027-startup-is-quiet`, in a container with networking disabled.
- [x] Installable `.deb` and `.rpm` packages for x86_64 and aarch64, proven by
      `scripts/package-check.sh` — `scripts/package.sh` builds `dist/ono_<v>_<arch>.deb` and
      `dist/ono-<v>-1.<arch>.rpm` from `crates/ono-cli/Cargo.toml` (ADR-0121), the shape is
      pinned in the gate by `xtask/tests/packaging.rs`, and the check installs the host
      architecture's packages into fresh `debian:bookworm` and `fedora:latest` containers with
      networking disabled: `ono --version`, a `get process` pipeline as root and as an
      unprivileged user whose login shell is `/usr/bin/ono`, `/etc/shells` registered on
      install and cleared on removal (ADR-0122). `scripts/release-check.sh` runs both scripts
      for the host architecture; the other architecture's runtime proof is the same two scripts
      on a native runner in `.github/workflows/release.yml` (ADR-0123), which is what ships the
      packages — locally, a foreign architecture is checked structurally only.
- [x] Generated documentation is reproducible from the registries and committed docs match it —
      `xtask/tests/reference.rs` regenerates every page and requires the committed files to be
      identical, and `spec-check` runs the same comparison in the gate (ADR-0018).
- [x] `docs/STATE.md` has an empty *In progress* section and no unexplained *Deferred* entries —
      `scripts/release-check.sh` reads the board and refuses the release line while a claim
      stands under *In progress* or a *Deferred* entry names no ADR (`cargo xtask state-check`,
      ADR-0402, driven by `xtask/tests/scan.rs`). The rest of the board is deliberately outside
      that rule: §4 is the stopping rule, and the post-release backlog that remains after it is
      the issue tracker (ADR-0425), which no gate reads.
- [x] Every `#[ignore]`d test is either removed or justified in *Deferred* with an ADR — the
      workspace holds none at all, which `cargo xtask spec-check`'s unfinished-work scan keeps
      true.

### 4.6 The v0.3 tranche — External Command Adaptation Layer

`docs/ono_sendai_shell_spec_v0.3_external_command_adapters.md` layers the adapter layer on the
released v0.2 shell (ADR-0026, ADR-0027). This subsection is its definition of done: the six
areas of its Integration Checklist (v0.3 §2.1–§2.6), the work packages of §1.67 and the release
bar of §1.68, in boxes a script can check. Every box is ticked only by a named automated proof
that runs in the gate or in the container — never on judgement (§3). The v0.3 tranche is
finished when this subsection has no unticked box and `scripts/release-check.sh` prints the
release line again.

#### 4.6.1 Core runtime (v0.3 §2.1, ADAPT-001 … ADAPT-007, ADAPT-011)

- [x] **ADAPT-001 — OutputDemand is part of planning.** The planner computes the stdout demand
      of every external stage backwards from its consumer — `RawBytes`, `Text`,
      `Structured(schema?)`, `Interactive`, `Discard` (v0.3 §1.4, §1.5) — before anything is
      spawned, and `explain <pipeline>` reports it per stage — ADR-0052;
      `ono-adapter/tests/demand.rs` (the derivation table), `ono-command/tests/explain.rs`
      (the plan), `ono-cli/tests/builtins.rs` (the rendering), cases `070` (at a pipe: structure
      for `where`, bytes for `grep`, bytes for a file, discard for `/dev/null`, the schema for
      `stop process`) and `071` (interactive at a PTY).
- [x] **ADAPT-003 — the raw path is guaranteed.** `raw <program> …` runs any external command
      with byte semantics untouched no matter which adapters are installed (v0.3 §1.17), it is
      documented in `help raw`, and byte semantics are preserved by default for every consumer
      that is not an explicit structured demand (§1.2, §2.1) — ADR-0054;
      `ono-cli/tests/external.rs` (argv as typed, exit status, verbatim bytes at a PTY, no
      native resolution, the program even where structure arrives),
      `ono-cli/tests/builtins.rs` (`explain` shows the bypass, `help raw`), case `072`
      (byte-identical to bash for `printf` and `ps` at a pipe, status 3 passes through).
      The "with adapters installed" half of the guarantee is re-proven by every tool case of
      §4.6.3, each of which runs its raw form beside its structured form.
- [x] **ADAPT-002 — deterministic registry and conflict resolution.** Adapters are resolved by
      executable identity, invocation and demand in a documented, deterministic order — exact
      path, invocation specificity, tier, id; never load order (v0.3 §1.24, §1.25, ADR-0056);
      two adapters claiming one invocation resolve the same way under both load orders and
      `explain` names the candidates and the selection reason; one adapter installed twice is
      `adapter.conflict`, never a coin toss — `ono-adapter/tests/negotiation.rs`
      (`should_resolve_two_adapters_claiming_one_invocation_the_same_way_every_time`,
      `should_report_a_conflict_when_one_adapter_is_installed_twice`), `ono-cli/tests/builtins.rs`,
      case `073`.
- [x] **Negotiation states are explicit.** An adapter answers `NotApplicable`, `RawPreferred`,
      `StructuredSupported`, `StructuredSupportedWithLimits`, `UnsupportedInvocation` or
      `IncompatibleVersion` (v0.3 §1.6, ADR-0056); unsupported invocations and incompatible
      versions fall back to raw under an interactive demand and fail a structured pipeline with
      the matching `adapter.*` error, and limits are visible in `explain` and provenance (§1.16)
      — `ono-adapter/tests/negotiation.rs` (one case per state and `describe` in the words of
      §1.57), `ono-cli/tests/adapters.rs` (`should_fail_a_structured_pipeline_on_an_undeclared_flag`
      → E0903 with the raw recovery), cases `073` and `074`.
- [x] **ADAPT-004 — the process subsystem executes the plan.** Adapters compile to an
      `AdapterPlan` (v0.3 §1.7) and `ono-process` runs it as the pipeline it would have been with
      the last command replaced — adapters never spawn; exit status and stderr keep Unix
      semantics (§1.20): a failing child never becomes success because its output decoded —
      ADR-0057; `ono-cli/tests/adapters.rs`
      (`should_never_turn_a_failing_child_into_success`: valid JSON, exit 2 → E0501 and nothing
      shown; `should_render_an_adapted_command_as_a_table_at_the_terminal`;
      `should_fall_back_to_the_captured_bytes_at_the_terminal_when_decoding_fails`: the tool
      ran exactly once), cases `074` and `075`.
- [x] **ADAPT-005 — streaming decoders that cannot crash the shell.** Decoded values flow
      while the child runs — `Decoding::feed` yields a record per complete line, the child runs
      in its own group with its stdout handed to a reader thread, the consumer drains a bounded
      channel, and a consumer that stops early cancels the child (ADR-0059); malformed,
      truncated, non-UTF-8 and hostile output produce `adapter.decode_failed` with the raw bytes
      retained, never a panic — `ono-cli/tests/adapters.rs`
      (`should_stream_decoded_records_while_the_child_still_runs`: `take 1` answers in well
      under the shim's five-second pause; `should_follow_the_journal_live_at_the_terminal_until_interrupted`),
      `ono-adapter/tests/decode.rs` (`should_never_panic_on_hostile_bytes`, a seeded walk),
      the truncated-line fixtures, case `077` (`streamed=early`).
- [x] **ADAPT-006 — version detection is bounded and cached.** Version probes are declared in
      the contract, run at most once per executable identity (path, device/inode, mtime, size —
      v0.3 §1.46) and a failed probe makes a version-constrained parser refuse rather than
      assume — `ono-adapter/tests/negotiation.rs`
      (`should_probe_an_executable_once_per_identity`: one probe for three negotiations, a
      second after the binary changes; `should_refuse_when_the_version_cannot_be_detected`;
      `should_refuse_an_executable_outside_the_supported_versions`), and case `073`'s shadowed
      `lsblk` that answers no version.
- [x] **ADAPT-007 — provenance on every adapted value.** `inspect` answers all ten questions
      of v0.3 §1.8 on an adapted record (executable, version, user and actual invocation, adapter
      id/version, decoder, timestamp, host via the link, per-field exactness, source stability)
      and limits are explicit (§2.2) — `ono_value::AdapterTrace` (ADR-0057 point 6);
      `ono-adapter/tests/decode.rs` (`should_attach_adapter_provenance_to_every_record`, each
      field asserted), `ono-cli/tests/adapters.rs`
      (`should_expose_adapter_provenance_through_inspect`), case `074`.
- [x] **Executable identity is pinned.** An adapter matches only executables whose resolved
      identity satisfies its contract (names/paths, version range — v0.3 §1.14, §1.44, ADR-0056
      point 2) and the plan pins the resolved path; a shadowing binary of the same name is
      refused as `adapter.executable_mismatch` when the contract names a path, and fails the
      version probe when it names a name — either way the raw path —
      `ono-adapter/tests/negotiation.rs` (`should_refuse_a_binary_that_is_not_the_one_the_contract_pins`,
      `plan.executable()` asserted), case `073` (a PATH-shadowed `lsblk` script).
- [x] **ADAPT-011 — remote negotiation.** Inside a link frame an adapted command is negotiated,
      run and decoded on the remote side against the remote executable (the `start-adapt`
      frame), provenance carries the host and the remote adapter, `explain` says
      `adaptation on <host>`, and a host lacking the tool or the adapter degrades to the local
      raw program with the reason printed — or, under a structured demand, refuses visibly
      (v0.3 §1.54, ADR-0066) — `ono-protocol/tests/messages.rs`, `ono-cli/tests/remote.rs`
      (`should_adapt_on_the_remote_side_inside_a_link_frame`,
      `should_explain_that_a_remote_host_negotiates_its_own_adapters`,
      `should_degrade_to_the_local_program_when_the_remote_has_no_adapter`), case `084`
      (against the child agent).

#### 4.6.2 Contracts, errors and KUANG/11 (v0.3 §2.2, §2.3, ADAPT-008 … ADAPT-010)

- [x] **`adapter.*` error family.** The eleven codes of v0.3 §1.65 exist in
      `docs/spec/errors.yaml` as the E09xx block mapped onto the spec §43 kinds (ADR-0053), and
      each emitted error carries adapter id/version, executable identity/version, the original
      invocation, whether raw fallback is safe, and a recovery under fixed metadata keys —
      `ono-core/tests/error_taxonomy.rs`, `ono-adapter/tests/decode.rs`
      (`should_fail_structurally_on_truncated_output_and_keep_the_bytes` asserts the payload),
      `ono-cli/tests/adapters.rs` (a rendered E0903 naming `raw lsblk -p`), case `074`.
- [x] **ADAPT-009 — declarative manifest schema.** `docs/spec/adapters/schema.yaml` is versioned
      (`ono-adapter-pack/1`) and machine-validated by `ono_adapter::validate`, the code the shell
      loads a pack with; every first-party contract lives under
      `docs/spec/adapters/first-party/*.yaml` in the shape of v0.3 §1.44 (util-linux first);
      `spec-check` fails on an unknown schema id, a builtin decoder the binary lacks, a missing or
      empty fixture directory, an executable outside the declared `process.exec` set, a
      first-party id outside `org.ono.compat.*`, a tier C adapter without a builtin decoder, a
      probe pattern without a capture, or a pack file the binary does not bundle — ADR-0055;
      `ono-adapter/tests/contracts.rs` (eleven cases) and `xtask/tests/contracts.rs`.
- [x] **Canonical schemas are reused.** Adapted values conform to the existing `ono.*/1`
      schemas wherever an equivalent exists (`findmnt` → `ono.mount/1`); adapter-specific
      schemas are added only where none does (`ono.block-device/1`, `ono.namespace/1`) and are
      registered like every other schema (v0.3 §1.11, §2.2) — `ono_adapter::validate` refuses an
      unregistered `schema:`, every decoded record passes the schema's own `validate`, and
      unmapped tool fields land in the extension map — `ono-adapter/tests/contracts.rs`,
      `decode.rs` (`should_keep_fields_the_map_does_not_name_as_extensions`), `conformance.rs`.
- [x] **ADAPT-010 — fixture conformance harness.** `ono_adapter::conformance::check_pack`
      runs fixture bytes → decoder → canonical value → schema conformance → provenance for every
      fixture of every first-party adapter, in the crate's tests and in `spec-check`, and
      `negotiation.rs` asserts the exact argv of every rewrite (v0.3 §1.47); every adapter ships
      fixtures for its output families (basic, empty, truncated, not-JSON, newer fields, wrong
      type) — `ono-adapter/tests/conformance.rs` (including a deliberately wrong sidecar that
      must be reported), `xtask/src/contracts.rs`.
- [x] **ADAPT-008 — capability mapping.** `process.exec` with `executables` and
      `argv_policy: declared-invocations-only` exists in `docs/spec/capabilities.yaml` and
      `docs/spec/kuang/capabilities.v1.yaml`, `roles: [adapter]` and `contributions.adapters`
      are part of the manifest contract, packs load under the default-deny broker as disabled
      until `--grant process.exec`, and no adapter can spawn outside its declared set
      (v0.3 §1.22, §1.26, §2.3, ADR-0065) — `ono-kuang-protocol/tests/manifest_validation.rs`,
      `ono-adapter/tests/negotiation.rs` (`Disabled` → E0902), `ono-cli/tests/plugins.rs`
      (`should_refuse_a_package_whose_adapter_runs_something_its_grant_does_not_name` → E0909),
      case `083`.
- [x] **Adapter SDK and test host.** A third-party package ships a declarative adapter with no
      runtime component (`crates/ono-kuang-sdk/examples/adapter-package/dev.example.users`,
      `getent passwd` → `ono.user/1`), and `ono_kuang_testhost::check_adapter_package` validates
      it — manifest, packs, fixtures (including the malformed families), the executables scope,
      what default deny and an explicit grant do — before it is loaded (v0.3 §1.45, §2.3,
      ADR-0065) — `ono-kuang-testhost/tests/adapter_package.rs`, `ono-cli/tests/plugins.rs`,
      case `083` (the example package installed and run in the container).
- [x] **Packs and trust.** First-party adapters are bundled (`ono_adapter::first_party`, only
      `org.ono.compat.*` may claim the tier); a package's packs are `community` or
      `experimental`; a community pack answers once `process.exec` is granted, an experimental
      pack only with `--allow-experimental` besides, and a pack claiming first-party or
      recommended for itself does not load (v0.3 §1.27, §1.56, ADR-0065) —
      `ono-adapter/tests/contracts.rs` (the first-party namespace), `ono-cli/tests/plugins.rs`
      (`should_keep_an_experimental_pack_out_of_structured_output_unless_allowed`,
      `should_adapt_through_a_third_party_pack_once_its_grant_is_explicit`),
      `ono-kuang-supervisor::validate_package` (the tier rule), case `083`.

#### 4.6.3 Compatibility program (v0.3 §2.5, §1.30, §1.31, COMPAT-*)

Each tool box requires, in one increment: a first-party contract, fixtures, the structured and
the raw pipeline acceptance-tested in the container against the real executable, an
unsupported-flag case that falls back safely, and `explain` showing the plan (v0.3 §1.68).

- [x] **COMPAT-LSBLK / FINDMNT / LSNS** — util-linux JSON: `lsblk` → `ono.block-device/1`,
      `findmnt` → `ono.mount/1`, `lsns` → `ono.namespace/1` (v0.3 §1.35) —
      `docs/spec/adapters/first-party/util-linux.yaml` with fixtures, `ono-cli/tests/adapters.rs`
      (the real tools, structured and raw, the undeclared flag), cases `073` (the plan), `074`
      (structured, provenance, raw byte-identical to bash, `--poll` refused) and `075` (the
      typed table at a PTY, `raw` keeps findmnt's own, a redirection keeps bytes).
- [x] **COMPAT-IP-001…003 + neigh** — `ip address` → `ono.interface-address/1` (one record
      per address), `ip link` → `ono.interface/1`, `ip route` / `ip -6 route` → `ono.route/1`
      with the family pinned by the invocation, `ip neigh` → `ono.neighbor/1`, all via `-j`
      (v0.3 §1.33, ADR-0058) — `docs/spec/adapters/first-party/iproute2.yaml` with fixtures,
      `ono-adapter/tests/{decode,negotiation}.rs`, `ono-cli/tests/adapters.rs`
      (`should_adapt_the_ip_family_into_canonical_network_records`, `-s` refused), case `076`
      (structured, provenance naming `ip -j address show`, raw byte-identical to bash, bytes
      downstream untouched, `explain`).
- [x] **COMPAT-JOURNAL-001** — `journalctl` via `--output=json` streaming into
      `ono.journal-event/1` records with cursor and boot id preserved, `-f` as a live view at
      the terminal (v0.3 §1.37) — `docs/spec/adapters/first-party/systemd.yaml` with fixtures,
      `ono-cli/tests/adapters.rs` (typed events, priorities, the follow), case `077` (fixture
      replay through a shim: the container has no journald, §1.48 "where applicable").
- [x] **COMPAT-SYSTEMD-001** — `systemctl list-units --output=json` and `systemctl show`'s
      key=value protocol (the `properties` decoder) → `ono.service/1`, never the human table;
      mutations stay external (v0.3 §1.36) — the systemd pack's fixtures through the harness,
      case `077` (fixture replay through a shim: the container is not booted with systemd).
- [x] **COMPAT-PS-001** — `ps` with an explicit `-o` field list → `ono.process/1`, so that
      `ps aux | where cpu > 20 | sort memory desc` composes and `ps aux | grep x` stays text;
      `ps` keeps its own selection semantics and `-o`/`-L`/`-T` run raw (v0.3 §1.34, §1.71,
      ADR-0060: whitespace columns with a greedy `args`, `program-name` and
      `started-from-elapsed` inferred and said so, streaming per line) —
      `docs/spec/adapters/first-party/procps.yaml` with fixtures, `ono-adapter/tests/decode.rs`,
      `ono-cli/tests/adapters.rs` (`should_make_ps_compose_while_keeping_its_selection_and_its_bytes`),
      case `078`.
- [x] **COMPAT-STAT / DF / FIND-001** — `stat --printf`, `df --output --block-size=1`,
      `find … -printf … \0` into `ono.file/1` and `ono.filesystem/1`, NUL-terminated records
      with the path last so hostile names survive, `find` streaming, human units and actions
      run raw (v0.3 §1.38, §1.39, ADR-0061) — `docs/spec/adapters/first-party/{coreutils,findutils}.yaml`
      with fixtures (a tab and a newline in a name), `ono-adapter/tests/negotiation.rs`,
      `ono-cli/tests/adapters.rs`, case `079`.
- [x] **COMPAT-GIT-001/002** — `git status --porcelain=v2 -z` → `ono.git-status-entry/1`
      through the `git-status-v2` builtin decoder, `git log` with an explicit NUL/RS format →
      `ono.commit/1`; human formats stay git (v0.3 §1.42, ADR-0062) —
      `docs/spec/adapters/first-party/git.yaml` with fixtures for every porcelain entry kind,
      `ono-cli/tests/adapters.rs` (`should_adapt_git_status_and_log_in_a_repository`), case `080`.
- [x] **COMPAT-LSOF** — `lsof -F pcuftn` → `ono.open-file/1` through the `lsof-fields-v1`
      builtin decoder, with the visibility limits stated (v0.3 §1.40, ADR-0062) —
      `docs/spec/adapters/first-party/lsof.yaml` with fixtures, `ono-cli/tests/adapters.rs`,
      case `080`.
- [x] **COMPAT-SS-001/002** — combined-flag matching (`-tunap`) and the `ss-text-v6` decoder,
      pinned to iproute2 5–6 and marked `version-constrained` in provenance, into `ono.socket/1`
      with nested `ono.endpoint/1` records, `LC_ALL=C` forced (v0.3 §1.32, §1.9 Tier C,
      ADR-0063) — the iproute2 pack's `ss*` fixtures through the harness,
      `ono-adapter/tests/{negotiation,decode}.rs`, `ono-cli/tests/adapters.rs`, case `081`.
- [x] **COMPAT-CURL-001** — `adapt curl …` is one `ono.http-exchange/1` record with the
      body as bytes and curl's write-out as the metadata; a plain `curl` stays the bytes it
      always was; headers and credentials never adapt (v0.3 §1.41, ADR-0064) — the curl pack's
      fixtures through the harness, `ono-cli/tests/adapters.rs`, case `082`.
- [x] **Text tools stay raw.** No first-party adapter claims `cat`, `grep`, `sed`, `awk`,
      `head`, `tail`, `sort`, `less`, editors or REPLs (v0.3 §1.70); terminal-owning tools keep
      the PTY (§1.19, §1.43). Exit test: a registry case asserting `NotApplicable` for each. — `ono-adapter/tests/negotiation.rs`
      (`should_leave_text_tools_raw_by_design`: `NotApplicable` for each), case `024`
      (a program run by `ono` owns the real terminal), case `071`.

#### 4.6.4 Integration surfaces (v0.3 §2.4)

- [x] **Invisible on success.** Adapted commands render exactly like native results at a TTY
      and compose in pipelines without new syntax; the prompt, tables and `@` reuse work on
      adapted values. Exit test: the §1.71 session as an acceptance case. — case `085` (the §1.71 session: `ps aux | where`,
      `@-1`, `ss`/`ip route`, `explain`, `raw`).
- [x] **Inspectable on demand.** `type`, `inspect` and `explain` show the selected adaptation
      plan, the negotiation state and the diagnostics of v0.3 §1.57, §1.61 for any external
      stage. Exit test: an explain case per negotiation state. — case `071` (every negotiation state through `explain`),
      `ono-cli/tests/builtins.rs`
      (`should_answer_type_with_the_adapters_schema_and_check_fields_before_running`, ADR-0067),
      `ono-adapter/tests/negotiation.rs` (`describe` per state).
- [x] **Adapter-aware completion invents nothing.** Completion after an adapted executable
      offers only invocations the contract declares and marks the rest as raw pass-through
      (v0.3 §1.59). Exit test: an `ono-editor` completion case. — `ono-cli/tests/adapters.rs`
      (`should_complete_fields_of_the_adapted_schema_after_the_pipe_and_declared_flags_before_it`:
      schema fields after the pipe, only declared flags before it, `--paths` never invented;
      ADR-0067).
- [x] **History records adaptation.** Each history entry names the adapter and plan used, and
      the history view can explain it (v0.3 §1.58). Exit test: an `ono-history` case. — `ono-history/tests/history.rs`
      (`should_remember_that_a_command_was_adapted_and_explain_it`), `ono-cli/tests/adapters.rs`
      (`should_record_the_adapter_in_history`).
- [x] **Scripts are deterministic.** The same invocation selects the same adapter with output
      redirected, in `-c`, in a script and at a TTY; redirection and TTY regressions are tested
      (v0.3 §1.53, §1.68 item 11). Exit test: an acceptance case comparing the three modes. — case `086` (`-c`, a script file and a `script`-driven
      terminal answer the same; a redirected program keeps the tool's bytes).
- [x] **Unix muscle memory holds.** `ps aux | grep foo`, `ip a | head`, `find . | wc -l` and
      `git status` produce the bytes the tool produces (v0.3 §1.2, §2.4). Exit test: an
      acceptance case diffing against a bash run of the same lines. — case `087` (each line `cmp`-identical to a `bash -c` run).

#### 4.6.5 Release evidence (v0.3 §2.6, §1.68)

- [x] **Support claims are machine-readable and published.** `docs/reference/adapters/` — a
      page per adapter and a compatibility matrix (tool, versions, invocations, schema, tier,
      limits) — is generated from the contracts and identical to the committed files (§1.66).
      Exit test: `xtask/tests/reference.rs` extended to the adapter pages. — `xtask/tests/reference.rs`
      (`should_publish_a_page_per_adapter_pack_and_a_compatibility_matrix_when_generated`,
      `should_find_this_repositorys_committed_reference_docs_up_to_date_when_checked`),
      `spec-check` on every gate run.
- [x] **Live conformance in the container.** The acceptance image installs every Tier A/B/C
      tool, and each adapter has at least one live case against the real executable
      (v0.3 §1.48); adapters for tools absent on a host degrade to raw with a visible reason. — case `088` (every first-party adapter
      negotiated and decoded against the installed tool; systemd through the shims of case
      `077`; an undetectable version degrades to raw with the reason), `xtask/tests/adapter_evidence.rs`
      (`should_have_a_live_acceptance_case_for_every_first_party_adapter`: the contracts and
      the cases cannot drift apart).
- [x] **Overhead is measured.** The adapter path adds a bounded, measured cost over the raw
      path — negotiation, rewrite and decode — reported by an acceptance case inside the §34
      budgets (v0.3 §1.50). — case `089` (`ps aux | count` and `lsblk | count`
      against `raw ps aux` / `raw lsblk`, per-run average, overhead bounded by the §34 first-rows
      figure of 50 ms, figures printed on every run).
- [x] **Limitations are documented.** Every first-party adapter's reference page states its
      unsupported invocations and known limits, and `README.md` presents the adapter layer to a
      new user with examples that run. Exit test: doc examples parse and run under `xtask`. — `docs/reference/adapters/*.md` (a *Limits* section
      per adapter, generated), `README.md` (*The Unix tools you already know become typed*),
      `xtask/tests/adapter_evidence.rs` (`should_find_ono_examples_in_the_readme_that_parse`,
      `should_run_every_readme_example_of_the_adapter_layer`), `spec-check` parsing the README's
      examples on every gate run.
- [x] **Delivery.** `docs/STATE.md` has an empty *In progress*, no `#[ignore]`d tests exist
      without a *Deferred* entry, the acceptance suite and CI are green on `implementation`,
      and this subsection has no unticked box. — `scripts/release-check.sh`, which now proves
      each clause instead of asserting it: `cargo xtask state-check` reads the board and refuses
      while *In progress* holds a claim (ADR-0402), `spec-check`'s unfinished-work scan refuses
      an `#[ignore]` the *Deferred* section does not track, the same script runs the gate and the
      containerised suite, and the unticked-box grep covers this subsection like every other. CI
      runs the gate and the acceptance suite on every push, which is the one clause no local
      script can observe.

### 4.7 The v0.4 tranche — Spatial Systems Interface

`docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md` layers a navigable projection of
the system onto the released v0.3 shell (ADR-0124 … ADR-0131). This subsection is its definition
of done: the release criteria of v0.4 §52 — §52.1 functional, §52.2 quality, §52.3 the product
experience — together with the ten acceptance scenarios of §44, the twenty invariants of §2 and
the performance budgets of §34, in boxes a script can check.

The executable requirements were written first and were red: the nine RED suites
`crates/ono-cli/tests/spatial_*_missing.rs` (175 `#[ignore]`d tests) and the ten
`docker/acceptance/cases/09x-spatial-*.case.v04` scenarios (139 assertions, kept out of the
referee by their suffix). They are green now, un-ignored and renamed, and
`xtask/tests/spatial_evidence.rs` fails the gate if any of that is undone. **A box below is
ticked only when the tests it names run un-ignored and green in the gate, or its case runs in
the container** — never on judgement (§3), never by
reading code, and never because a phase of v0.4 §50 is reported complete. Where a test named
here does not exist yet, the box names the file and behaviour the delivering increment must
create; writing it is part of that increment, not of a later one. ADR-0137 records what closes
a box that no test can prove — the security review of §52.2 and the dogfooding of §52.3 — and
the three artifacts this subsection requires the tranche to build.

Two conventions this subsection relies on:

- **The rename rule.** A `.case.v04` file is out of the suite (`scripts/acceptance.sh` collects
  `*.case`). The increment that delivers a scenario renames its file to `.case` in the same
  commit as the behaviour, and from then on the referee runs it (`docker/acceptance/cases/README-v0.4.md`).
  A scenario box is ticked only after that rename, so a green acceptance run is what ticks it.
- **The spelling.** ADR-0124 keeps bare `find` on findutils and spells the spatial search
  `find place`; the RED suites and four of the cases predate the ADR and are rewritten by the
  increment that delivers v0.4 §6.8, in that increment's commit.

The v0.4 tranche is finished when this subsection has no unticked box and
`scripts/release-check.sh` prints the release line again. Every box was ticked on 2026-08-28 by
agent `S11b`, from the evidence each one names; what that session found and could not close is
in `docs/dogfood/v0.4-2026-08-28.md`, and in the issue tracker (ADR-0425).

#### 4.7.1 Functional release criteria (v0.4 §52.1)

- [x] **Root `SYSTEM` and canonical domains exist** (§7, §4). `home` reports the root place,
      `look` at the root lists exactly the six canonical domains of §4 with a permission state
      on each, and the root's identity is the same across sessions —
      `spatial_topology.rs::should_report_the_system_root_as_the_current_place_when_home_runs`,
      `::should_list_exactly_the_six_canonical_domains_when_looking_at_the_system_root`,
      `::should_carry_a_permission_state_on_every_domain_so_an_unavailable_one_stays_visible`,
      `::should_keep_the_same_spatial_id_for_the_root_across_separate_sessions`,
      `::should_enter_every_canonical_domain_when_named_at_the_root`,
      `spatial_contracts.rs::should_start_every_session_at_the_local_system_root` and
      `::should_serve_exactly_the_canonical_spaces_the_registry_declares` (the registry and the
      shell cannot drift apart), case `090`.
- [x] **Users can discover objects without prior names** (§9, §2.1). A process, a listening
      socket and a running service are each reached from a predicate over visible metadata, with
      the name never typed —
      `spatial_topology.rs::should_reach_a_process_it_never_names_when_only_a_predicate_over_visible_metadata_is_known`,
      `::should_discover_a_listening_socket_by_its_port_and_follow_it_to_its_owning_process`,
      `::should_reach_a_running_service_by_its_visible_state_when_a_service_manager_answers`,
      `::should_answer_look_near_and_map_without_an_object_name_when_at_the_root`,
      `spatial_navigation.rs::should_stream_places_with_scope_and_provenance_when_find_searches_with_a_predicate`,
      cases `090` and `091`, whose house rule is that no case types the name of the object it
      discovers.
- [x] **All core spatial commands are implemented** (§6). `look`, `near`, `enter`, `follow`,
      `jump`, `back`, `up`, `home`, `trail`, `find place`, `map`, `pin`/`unpin` each answer with
      the contract §6 gives them —
      `spatial_navigation.rs` (one test per verb: `::should_describe_the_current_place_as_a_structured_view_when_look_runs_without_a_tty`,
      `::should_stream_neighbors_that_compose_with_the_pipeline_when_near_runs_in_a_script`,
      `::should_move_into_the_hierarchical_child_when_entering_a_canonical_domain_and_its_group`,
      `::should_traverse_the_relationship_edge_when_following_the_parent_relation`,
      `::should_move_across_scopes_and_record_both_ends_when_jumping_to_a_resolved_place`,
      `::should_return_to_the_process_when_back_follows_the_navigation_history`,
      `::should_move_to_the_network_hierarchy_parent_when_up_follows_the_canonical_hierarchy`,
      `::should_return_to_the_system_root_when_home_runs_after_deep_navigation`,
      `::should_record_every_movement_with_its_kind_and_relation_when_the_trail_is_read_as_json`,
      `::should_answer_a_bounded_graph_when_map_json_runs_without_a_tty`),
      `spatial_contracts.rs::should_keep_the_trail_session_local_while_a_pin_survives_the_session`,
      and `help spatial` complete for each with a generated reference page — `help spatial` is
      the §38.1 overview, proven by `crates/ono-cli/tests/spatial_help.rs`
      (`::should_explain_every_spatial_verb_of_the_overview_when_help_spatial_runs`,
      `::should_send_the_reader_from_the_overview_to_one_verbs_own_page`,
      `::should_offer_the_overview_among_the_topics_help_lists`) and case `102` (`s4t`, `s4u`),
      and the generated page is `docs/reference/commands.md`, which carries all fourteen
      `ono.place.*` commands and is held against the registries by `xtask/tests/reference.rs`
      and `spec-check` on every gate run.
- [x] **Hierarchy and graph traversal are distinct** (§11, §6.6). `up` walks the canonical
      hierarchy and `back` the trail; `follow` refuses a canonical child that is not a
      relationship edge; every object keeps all its relationship parents while naming one
      canonical parent —
      `spatial_relationships.rs::should_refuse_to_follow_a_canonical_child_that_is_not_a_relationship_edge`,
      `::should_leave_the_relationship_chain_with_up_after_following_a_socket_edge`,
      `spatial_identity.rs::should_keep_every_relationship_parent_while_naming_one_canonical_parent`,
      `::should_move_to_the_declared_canonical_parent_deterministically_when_going_up`,
      `spatial_navigation.rs::should_move_to_the_network_hierarchy_parent_when_up_follows_the_canonical_hierarchy`,
      case `095`.
- [x] **Typed pipeline and spatial selection interoperate** (§28). `look --json` and `near`
      read back into the v0.2 pipeline, a pipeline result is entered as a place, and a spatial
      result composes with `where`/`take`/`count` —
      `spatial_navigation.rs::should_read_back_into_the_pipeline_when_look_json_is_parsed_by_from_json`,
      `::should_move_into_the_selected_object_when_a_pipeline_result_is_entered`,
      `::should_compose_with_the_v02_pipeline_when_a_find_result_is_filtered_and_counted`,
      `spatial_topology.rs::should_stream_neighbors_as_pipeline_objects_when_near_runs_at_the_root`,
      case `091`.
- [x] **Storage paths integrate with cwd according to this spec** (§15, §30). Entering a
      directory moves cwd and place together, entering a process or a socket moves neither, `cd`
      moves the place only under `storage-only`, `PWD` never carries a non-directory place, and
      a mount boundary shows its source — the whole of
      `crates/ono-cli/tests/spatial_storage.rs` (12 tests) and case `092`.
- [x] **Remote host roots can be entered/jumped when links exist** (§19). A linked host is a
      place with a root distinct from the local one, `jump` announces the boundary, the trail
      records the host crossing, and a hostname that is not a link is refused —
      `crates/ono-cli/tests/spatial_remote.rs` (13 tests, notably
      `::should_give_a_linked_host_a_root_place_distinct_from_the_local_root`,
      `::should_announce_the_boundary_in_plain_text_when_jumping_to_a_linked_host`,
      `::should_refuse_to_jump_to_a_hostname_that_is_not_a_known_link`), case `094` (§19a–g).
- [x] **Map text rendering works without a full-screen TUI** (§23.2, §29.1). `map` renders as
      text into a pipe, `map --json` answers the §22 document off a terminal, and a narrow
      terminal collapses the layout without changing the semantics —
      `spatial_map.rs::should_render_a_text_map_when_stdout_is_a_pipe_and_no_full_screen_view_is_possible`,
      `::should_fit_the_text_map_into_the_terminal_when_the_terminal_is_narrow`,
      `::should_return_a_spatial_map_document_when_map_json_runs_without_a_tty`,
      `spatial_navigation.rs::should_answer_a_bounded_graph_when_map_json_runs_without_a_tty`,
      case `090` (the text map assertions).
- [x] **Full-screen map works on supported interactive terminals** (§23.3, §23.4). At a real
      PTY the view opens, focus moves without changing the place, Enter changes it, back
      returns, and closing restores the shell screen —
      `spatial_interactive.rs::should_restore_the_shell_screen_when_the_full_screen_map_closes`,
      `::should_change_the_place_only_on_enter_when_focus_moves_inside_the_map`,
      `::should_return_to_the_previous_place_when_back_is_used_at_the_prompt_and_in_the_map`,
      case `099`.
- [x] **The live map reflects real changes** (§25). An edge appears when a connection opens and
      is removed when it closes, a live view emits nothing while nothing happens, and no change
      section is invented where no event source exists —
      `spatial_relationships.rs::should_show_the_connection_edge_appear_and_vanish_when_the_connection_opens_and_closes`,
      `spatial_map.rs::should_not_invent_a_change_section_when_no_snapshot_or_event_source_exists`,
      case `098`, whose assertions require a real state change per §43.6 ("no test may pass
      based only on timer animation").
- [x] **Tombstones and lifetime identity prevent PID/object reuse confusion** (§10). A visited
      process that exits becomes a tombstone distinct from a place that never existed, a
      tombstone refuses traversal and never resolves to a live object, `back` returns the
      tombstone with the trail record intact, and the replacement process is a different
      identity — `crates/ono-cli/tests/spatial_identity.rs`
      (`::should_carry_a_lifetime_descriptor_rather_than_the_bare_pid_as_process_identity`,
      `::should_report_a_tombstone_rather_than_a_live_place_when_the_visited_process_has_exited`,
      `::should_distinguish_a_tombstone_from_a_place_that_never_existed`,
      `::should_refuse_to_traverse_a_relationship_when_the_place_is_a_tombstone`,
      `::should_never_resolve_a_tombstoned_place_to_a_live_object`,
      `::should_return_the_tombstone_and_keep_the_trail_record_when_back_points_at_a_dead_place`,
      `::should_not_confuse_the_old_and_the_new_process_when_a_place_is_replaced`),
      the §43.2 property `PID reuse -> different lifetime SpatialId` in
      `crates/ono-spatial-core/tests/properties.rs`, case `096`.
- [x] **Permissions remain honest** (§35.1, §35.2). A neighborhood group carries one of the six
      states of §35.2, denied is reported as denied rather than as an empty collection, an
      unavailable group is distinct from an empty one, and navigation triggers no escalation —
      `spatial_identity.rs::should_report_permission_denied_rather_than_zero_files_for_another_users_process`,
      `::should_report_a_real_file_list_for_a_process_this_user_owns`,
      `::should_name_one_of_the_defined_permission_states_for_every_neighborhood_group`,
      `spatial_contracts.rs::should_report_denied_information_as_denied_rather_than_as_an_empty_collection`,
      `spatial_topology.rs::should_distinguish_an_unavailable_group_from_an_empty_one_when_a_domain_has_no_provider`,
      `spatial_relationships.rs::should_report_the_unreadable_namespace_group_as_unknown_rather_than_absent`,
      `::should_not_report_the_owner_of_a_socket_nobody_looked_up_as_no_owner` (ADR-0209 — a
      reference field a provider left null is never an empty exit, which is what
      `docs/dogfood/v0.4-2026-08-28.md` finding 2 found the shell doing at a socket's owner),
      case `097`.
- [x] **v0.3 adapted canonical objects participate where available** (§37). An adapted
      observation and its native twin reconcile to one place with both sources retained, and raw
      command output never becomes a place —
      `spatial_contracts.rs::should_reconcile_an_adapted_object_with_its_native_twin_into_one_place`,
      `::should_never_let_raw_command_output_become_a_place`,
      `spatial_identity.rs::should_resolve_the_adapter_view_and_the_native_view_of_one_process_to_one_spatial_id`,
      case `110` (the §37.1 identity-merge assertions `s10-a`–`s10-f`). ADR-0193.
- [x] **KUANG/11 can extend spatial relationships under capabilities** (§36). A package's edges
      stay out of the map until its capability is granted and carry the contributing package as
      their origin when they appear —
      `spatial_contracts.rs::should_keep_a_package_relation_out_of_the_map_until_its_capability_is_granted`,
      `::should_carry_the_contributing_package_as_the_origin_of_every_plugin_edge`, case `110`
      (`s9-a`–`s9-g`), with the spatial contribution APIs validated before load by
      `ono_kuang_testhost` in the same shape as `ono-kuang-testhost/tests/adapter_package.rs`
      (§4.6.2) — `ono-kuang-testhost/tests/spatial_package.rs`. ADR-0194.

#### 4.7.2 Quality and product experience (v0.4 §52.2, §52.3)

v0.4 §52.2 states nine bullets; the second ("unit/property/integration/PTY tests pass") is one
sentence covering the four test layers of §43.1–§43.4, and is expanded here into one box per
layer so that each layer's own checklist is checkable.

- [x] **All spatial registries validate.** `docs/spec/spatial/{spatial,spaces,relations,landmarks}.yaml`
      (ADR-0126, ADR-0128) exist, are complete in the shape of §41.1/§41.2, and cannot drift
      from the shell: every declared space is served and every served space is declared, the
      same for relations, and the settings block equals the typed catalogue —
      `spatial_contracts.rs::should_ship_the_machine_readable_spatial_registry`,
      `::should_declare_every_canonical_space_with_the_fields_the_registry_requires`,
      `::should_declare_every_relation_with_its_direction_labels_and_confidence`,
      `::should_serve_exactly_the_canonical_spaces_the_registry_declares`,
      `::should_serve_every_relation_it_declares_and_declare_every_relation_it_serves`,
      `crates/ono-cli/tests/spatial_registry.rs` (the settings direction) and
      `cargo run -p xtask -- spec-check` on every gate run.
- [x] **Unit tests pass** (§43.1). Each of the thirteen areas §43.1 requires — `SpatialId`
      stability, canonical parent selection, selector precedence, ambiguity detection,
      neighborhood ranking, clustering, landmark thresholds, trail operations, tombstone
      resolution, relation inverse handling, scope boundary detection, map node/edge filtering,
      permission-state preservation — has a named test in the spatial crates
      (`crates/ono-spatial-core/tests/{identity,hierarchy,relations,trail,projection}.rs`,
      `crates/ono-spatial-index/tests/index.rs`, and the query/render/events crates as §45
      creates them), with `xtask/tests/spatial_evidence.rs` asserting that no §43.1 area is
      without one.
- [x] **Property tests pass** (§43.2). The seven properties §43.2 lists are seeded property
      tests, split by what they are statements about: the four about identity and navigation are
      in `crates/ono-spatial-core/tests/properties.rs` — `back(enter(x))` returns the prior place
      (`::should_return_to_the_prior_place_after_entering_one_whenever_both_still_exist`), `up`
      never traverses a graph edge (`::should_never_let_a_graph_edge_change_where_up_arrives`),
      one stable provider identity yields one `SpatialId`
      (`::should_resolve_the_same_provider_identity_to_the_same_spatial_id_every_time`) and PID
      reuse yields a different lifetime id
      (`::should_give_a_reused_pid_a_different_lifetime_id_for_every_generated_case`) — and the
      three about a *map* are in `crates/ono-spatial-query/tests/properties.rs`, which is where
      the projection lives (§45.3): map coordinates never affect identity
      (`::should_keep_every_identity_the_same_however_the_map_is_laid_out`), filtering cannot
      create unknown edges
      (`::should_keep_every_node_and_edge_a_filter_left_alone_and_invent_none`, red at seed 1
      before ADR-0202) and every rendered edge references a rendered node or an explicit off-map
      endpoint (`::should_resolve_every_drawn_edge_to_a_drawn_node_or_a_cluster_standing_for_one`,
      also `spatial_identity.rs::should_resolve_every_edge_endpoint_to_a_node_or_an_explicit_off_map_endpoint`).
- [x] **Integration fixtures pass** (§43.3). The deterministic fixture under
      `docker/acceptance/fixtures/spatial/`, together with the image it runs in, provides every
      element §43.3 names (`docker/acceptance/fixtures/spatial/README.md` says which comes from
      which, and asserts the honest degradation of §35.2 where the container genuinely cannot
      have one) — two services,
      one service with several processes, a process holding a known file, a TCP listener, a
      client/server connection, a mount boundary, a namespace or container boundary where the
      environment permits, several users, and a failing/restarting service — and the §43.3
      example acceptance path runs against it without naming the objects: cases `091`, `093`,
      `094`, `096`.
- [x] **PTY interaction tests pass** (§43.4). All nine PTY checks of §43.4 are driven through a
      real pseudo-terminal — `crates/ono-cli/tests/spatial_interactive.rs` (12 tests:
      startup horizon, ambiguity picker, map open/close, focus without place change, Enter
      changes place, back returns, resize preserves the place, Ctrl-C leaves the shell alive,
      an external program still works after the map closes) and case `099`.
- [x] **Acceptance scenarios pass.** All ten §44 scenarios of §4.7.3 are renamed from
      `.case.v04` to `.case` and green in `scripts/acceptance.sh`, and no `*.case.v04` file
      remains in `docker/acceptance/cases/` — asserted in the gate by
      `xtask/tests/spatial_evidence.rs`, so a scenario cannot be quietly left out of the suite.
- [x] **No release-blocking known defects remain.** `docs/STATE.md` *In progress* is empty, the
      workspace holds no `#[ignore]`d test (`cargo run -p xtask -- spec-check`'s unfinished-work
      scan), and every *Deferred* entry names an ADR saying why it does not block the release —
      the same bar §4.5 sets for v0.2 and §4.6.5 for v0.3, and the same check: `cargo xtask
      state-check` resolves both halves of this sentence against the board on every
      `scripts/release-check.sh` run (ADR-0402, `xtask/tests/scan.rs`), so the box cannot stay
      ticked once a claim or an undefended deferral appears.
- [x] **Performance targets are measured, and major violations resolved or documented.** Every
      box of §4.7.5 is ticked; any budget that is exceeded is recorded in an ADR naming the
      figure measured, the cause and the decision, and the ADR is cited by the §4.7.5 box that
      would otherwise be unticked. A budget is never ticked from a figure nobody measured.
- [x] **Security review completed** (§35, §51 SEC-S01). No test can conclude a review, so the
      accepted evidence is fixed here: an ADR titled *the spatial enumeration review* extends the
      T1–T15 threat table of ADR-0015 with a row per §35 boundary — §35.1 no revelation the
      provider would refuse, §35.2 the six states, §35.3 no escalation from navigation, §35.4 no
      connection a link did not authorise, §35.5 plugin nodes filtered by capability before the
      merge — and **each row names a passing test**, exactly as ADR-0015's rows do.
      `xtask/tests/spatial_evidence.rs` asserts that every test named in that table exists and
      is not ignored, so the box is ticked by the suite, not by the reviewer's opinion. Named
      today: `spatial_identity.rs::should_report_permission_denied_rather_than_zero_files_for_another_users_process`
      (§35.1/§35.2), `::should_name_one_of_the_defined_permission_states_for_every_neighborhood_group`
      (§35.2), case `097` (§35.3, no escalation),
      `spatial_remote.rs::should_refuse_to_jump_to_a_hostname_that_is_not_a_known_link`
      (§35.4), `spatial_contracts.rs::should_keep_a_package_relation_out_of_the_map_until_its_capability_is_granted`
      (§35.5).
- [x] **The renderer works with colour disabled and with an ASCII fallback** (§39.1, §39.2).
      The six distinctions §39.1 forbids colour to own — current node, inferred edge, failed
      state, remote boundary, root privilege, focused item — are legible without colour, and the
      map draws in plain ASCII on an ASCII-only terminal —
      `spatial_map.rs::should_render_the_map_in_plain_ascii_when_colour_is_disabled_and_the_terminal_is_ascii_only`,
      `spatial_interactive.rs::should_keep_the_same_spatial_semantics_when_look_runs_at_forty_columns`,
      `spatial_remote.rs::should_mark_the_remote_host_in_the_prompt_after_a_jump`, and
      the §43.5 renderer snapshots at 40, 80, 120 and 200 columns,
      `crates/ono-spatial-render/tests/widths.rs`
      (`::should_draw_the_same_map_inside_every_width_the_spec_names`,
      `::should_show_every_node_at_every_width_so_a_narrow_terminal_hides_nothing`,
      `::should_mark_exactly_one_focused_line_at_every_width`,
      `::should_render_the_snapshot_the_spec_asks_to_be_kept_at_each_width`) — snapshots as
      presentation tests only, never a data contract.
- [x] **Terminal state survives entering and exiting full-screen views** (§23.3, §49.8). After
      the map closes the shell screen is restored, an external interactive program still owns
      the terminal, and a resize while a view is open does not move the place —
      `spatial_interactive.rs::should_restore_the_shell_screen_when_the_full_screen_map_closes`,
      `::should_leave_the_terminal_in_order_for_an_external_program_after_the_map_closes`,
      `::should_preserve_the_current_place_when_the_terminal_is_resized_with_a_place_open`,
      case `099` (terminal size and mode after the view, jobs, clean exit).
- [x] **Provider conformance proves identity and permission semantics** (§42). Every provider
      that feeds the spatial index declares the §42 spatial claims and passes the four §42
      conformance tests — identity stability (§42.1), reuse safety (§42.2), relation integrity
      (§42.3), permission state (§42.4) — declared in `docs/spec/providers/*.yaml` and held
      against the tree by `spec-check` the way the v0.2 provider claims are:
      `spatial_contracts.rs::should_declare_the_spatial_claims_on_every_provider_that_feeds_the_spatial_index`,
      `::should_resolve_repeated_observations_of_one_object_to_the_same_spatial_id`,
      `::should_report_denied_information_as_denied_rather_than_as_an_empty_collection`, and the
      conformance suite the claims are checked through,
      `crates/ono-spatial-index/tests/conformance.rs` — identity stability
      (`::should_resolve_repeated_observations_of_one_object_to_the_same_identity`), reuse safety
      (`::should_never_resolve_a_tombstoned_place_to_the_object_that_reused_its_identifier`),
      relation integrity (`::should_refuse_to_hold_one_provider_object_as_two_places`,
      `::should_never_give_two_different_objects_one_identity`) and permission state
      (`::should_map_every_refusal_a_provider_can_state_to_one_of_the_six_states`) — with
      `xtask::contracts::check_provider_claims` holding every provider's `spatial:` block against
      the implementation on every gate run (ADR-0132).
- [x] **The product-experience statement is demonstrated** (v0.4 §52.3). §52.3 requires
      "concrete test scenarios **and** dogfooding", so it is ticked by two things together, and
      neither is an opinion. The scenarios: cases `090`–`099` green, with the house rule of
      `docker/acceptance/cases/README-v0.4.md` — no case types the name of the object it is
      supposed to discover — asserted mechanically by `xtask/tests/spatial_evidence.rs`, which
      is what makes the ten cases evidence *for this statement* rather than merely for §44. The
      dogfooding: at least one session of at least an hour on a host the author did not prepare,
      recorded in `docs/dogfood/v0.4-<date>.md` as the transcript of what was asked and what the
      shell answered, with every defect it produced filed in `docs/STATE.md` and either fixed or
      carried as a *Deferred* entry with an ADR. The record is the evidence; the box is ticked
      when the record exists, the defects it names are closed or deferred with a reason, and the
      cases above are green.

#### 4.7.3 The ten acceptance scenarios (v0.4 §44)

One box per scenario. Each is ticked when its file has been **renamed from `*.case.v04` to
`*.case`** in the same commit as the behaviour it proves, and the renamed case is green in
`scripts/acceptance.sh` — the rename is the delivery, because a `.case.v04` file is invisible to
the referee.

- [x] **§44.1 cold-start discovery** — `docker/acceptance/cases/090-spatial-cold-start-discovery.case`
      (18 assertions, `44.1a`–`44.1r`): the canonical domains and a meaningful object reached
      from `look`, `map`, `near`, completion and `find place` alone, the text map inside the
      §22 contract and the ~30-node budget, the startup horizon at a terminal, and the two §34
      budget assertions `44.1q`/`44.1r`.
- [x] **§44.2 unknown web service** — `091-spatial-unknown-web-service.case` (17 assertions):
      the fixture web service selected by visible metadata without its name, entered, its
      process and its listening socket followed, the trail naming the relation, an unavailable
      provider reported honestly, and the §37.1 adapter identity merge.
- [x] **§44.3 storage discovery** — `092-spatial-storage-discovery.case` (15 assertions): the
      storage domain walked without mount names, the secondary mount entered, its source and
      boundary shown, the mounted directory traversed, `cd` versus `enter` per §30, and a large
      directory summarised with its hidden count.
- [x] **§44.4 process → file → process** — `093-spatial-process-file-process.case` (14
      assertions): the open-file relation traversed in both directions, the edge record
      explaining it with provider, confidence and the descriptor it was read through
      (ADR-0164 makes the edge the "equivalent structured selection" of §11.4's
      `inspect relation`), and empty distinguished from denied.
- [x] **§44.5 network path** — `094-spatial-network-path.case` (15 assertions): service →
      process → socket → connection navigated by relationship discovery, plus the §19 half —
      the link map, `jump` across it, no auto-expansion of a remote graph, and no local/remote
      identity merge.
- [x] **§44.6 back versus up** — `095-spatial-back-versus-up.case` (15 assertions): after the
      §44.6 walk, `back` returns to the process and `up` to the socket's canonical network
      parent, with `trail --compact`, `history_empty` and `no_parent` asserted.
- [x] **§44.7 identity replacement** — `096-spatial-identity-replacement.case` (13 assertions):
      the entered service process is restarted, the old place becomes a tombstone that says what
      state it is in and how long ago, the service place stays stable and shows the replacement
      process, the trail record survives, and a movement onto the tombstone is refused. The
      tombstone names its `replacement:` candidate and the relation that identifies it — asked of
      the source that reached the dead object, when the tombstone is rendered, and offered only
      where that source now reaches exactly one live object of the same kind (ADR-0273).
- [x] **§44.8 permission honesty** — `097-spatial-permission-honesty.case` (12 assertions): a
      non-root user investigating a restricted process sees `permission_denied` and `unknown` as
      distinct from empty, no escalation is attempted, and a map of a denied place shows the
      boundary.
- [x] **§44.9 live map** — `098-spatial-live-map.case` (10 assertions): with `map --live`
      watching, an opened connection makes a real edge appear and a closed one makes it
      disappear or tombstone, nothing is emitted while nothing changes, freshness is shown, and
      Ctrl-C ends the view without killing the shell.
- [x] **§44.10 raw shell continuity** — `099-spatial-raw-shell-continuity.case` (10
      assertions): after extensive navigation and full-screen map use, interactive process
      control, terminal state, terminal size and mode, jobs and cwd are all still correct, and
      the shell exits cleanly.

#### 4.7.4 The twenty core spatial invariants (v0.4 §2)

One box per invariant, each naming the test that fails when the invariant is violated. Several
invariants are guarded by the same test; where that is so it is said, and no test is invented to
give an invariant one of its own.

- [x] **§2.1 Discovery before naming.** Violated the moment an object needs its name as input —
      caught by `spatial_topology.rs::should_reach_a_process_it_never_names_when_only_a_predicate_over_visible_metadata_is_known`
      and `::should_answer_look_near_and_map_without_an_object_name_when_at_the_root`; cases
      `090`/`091` (which never type the discovered name). Same proof as §4.7.1's discovery box.
- [x] **§2.2 Location is explicit.** Caught by
      `spatial_navigation.rs::should_describe_the_current_place_as_a_structured_view_when_look_runs_without_a_tty`,
      `spatial_topology.rs::should_describe_the_current_place_with_an_id_kind_name_scope_and_permission_when_looking`
      and, at a terminal, `spatial_interactive.rs::should_name_the_current_place_in_the_prompt_and_follow_it_when_the_place_changes`
      (shared with §2.20).
- [x] **§2.3 Movement changes context.** A verb that prints an object without moving fails
      `spatial_navigation.rs::should_move_into_the_hierarchical_child_when_entering_a_canonical_domain_and_its_group`,
      `::should_traverse_the_relationship_edge_when_following_the_parent_relation` and
      `::should_move_across_scopes_and_record_both_ends_when_jumping_to_a_resolved_place`, each
      of which reads the place *after* the command; the inverse — a command that moves and must
      not — is `spatial_relationships.rs::should_keep_the_current_place_when_trace_projects_the_relationship_graph`.
- [x] **§2.4 Every movement is reversible.** Caught by
      `spatial_navigation.rs::should_return_to_the_process_when_back_follows_the_navigation_history`,
      `::should_answer_history_empty_when_back_runs_with_no_previous_place`,
      `spatial_relationships.rs::should_return_to_the_process_with_back_after_following_a_socket_edge`
      and, for a destination that died, `spatial_identity.rs::should_return_the_tombstone_and_keep_the_trail_record_when_back_points_at_a_dead_place`;
      case `095`.
- [x] **§2.5 Every edge is explainable.** Caught by
      `spatial_relationships.rs::should_explain_every_edge_with_relation_provider_and_confidence_when_mapping_a_process`
      and `spatial_identity.rs::should_carry_source_provenance_and_confidence_on_every_relationship_edge`;
      case `093` (`inspect relation`).
- [x] **§2.6 Hierarchy and graph are separate concepts.** Caught by
      `spatial_relationships.rs::should_refuse_to_follow_a_canonical_child_that_is_not_a_relationship_edge`
      and `spatial_identity.rs::should_keep_every_relationship_parent_while_naming_one_canonical_parent`.
      Same proof as §4.7.1's hierarchy/graph box.
- [x] **§2.7 No fabricated geometry.** Caught by
      `spatial_map.rs::should_omit_screen_coordinates_when_map_json_returns_the_semantic_contract`
      and the §43.2 property `map coordinates never affect semantic identity` in
      `crates/ono-spatial-core/tests/properties.rs` (shared with §2.19).
- [x] **§2.8 Stable identity beats transient identifiers.** Caught by
      `spatial_identity.rs::should_carry_a_lifetime_descriptor_rather_than_the_bare_pid_as_process_identity`,
      `::should_give_different_spatial_ids_to_two_processes_that_share_a_display_name`,
      `::should_return_the_same_spatial_id_when_the_same_place_is_observed_by_two_shell_invocations`
      and the property `PID reuse -> different lifetime SpatialId`; case `096`.
- [x] **§2.9 The horizon is bounded.** Caught by
      `spatial_topology.rs::should_bound_the_root_horizon_instead_of_listing_every_known_object`,
      `::should_bound_the_neighborhood_and_count_what_it_hides_when_a_place_has_many_neighbors`,
      `spatial_map.rs::should_bound_the_default_map_when_the_host_holds_more_objects_than_the_view_budget`
      and `spatial_contracts.rs::should_bound_the_default_map_to_its_node_budget`
      (shared with §4.7.5's view-budget box).
- [x] **§2.10 Zoom is semantic.** Caught by
      `spatial_map.rs::should_aggregate_into_the_canonical_domains_when_the_zoom_level_is_coarse`,
      `::should_report_how_many_objects_a_cluster_stands_for_when_the_view_budget_is_exceeded`
      and `::should_yield_exactly_the_members_and_keep_the_place_when_a_cluster_is_expanded` —
      a view that merely hid rows fails the last of these.
- [x] **§2.11 Landmarks reflect significance.** Caught by
      `spatial_map.rs::should_expose_a_built_in_reason_for_every_landmark_when_map_json_reports_them`,
      `::should_mark_a_listener_on_every_interface_as_a_public_listener_landmark`,
      `::should_expose_landmark_thresholds_as_inspectable_and_configurable_settings` and
      `spatial_topology.rs::should_expose_a_reason_on_every_landmark_when_a_place_reports_landmarks`.
- [x] **§2.12 Live views reflect real change.** Caught by
      `spatial_relationships.rs::should_show_the_connection_edge_appear_and_vanish_when_the_connection_opens_and_closes`
      and `spatial_map.rs::should_not_invent_a_change_section_when_no_snapshot_or_event_source_exists`;
      case `098`, which requires a real change for every assertion (§43.6).
- [x] **§2.13 Text remains sufficient.** Every test in the eight non-PTY spatial suites drives
      the shell through `ono -c` with no terminal at all, so a spatial operation that needed a
      TUI could not pass any of them; named explicitly by
      `spatial_map.rs::should_render_a_text_map_when_stdout_is_a_pipe_and_no_full_screen_view_is_possible`
      and `spatial_navigation.rs::should_answer_a_bounded_graph_when_map_json_runs_without_a_tty`.
- [x] **§2.14 TTY richness is optional presentation.** Caught by
      `spatial_map.rs::should_return_the_same_node_identities_when_the_terminal_width_changes`
      and `spatial_interactive.rs::should_keep_the_same_spatial_semantics_when_look_runs_at_forty_columns`,
      with the v0.2 determinism floor of §4.2 (`034-redirected-output-is-deterministic`) still
      green for the spatial commands.
- [x] **§2.15 Unix remains underneath.** Caught by
      `spatial_navigation.rs::should_keep_running_external_commands_when_spatial_navigation_has_happened`,
      `::should_run_the_native_spatial_find_and_keep_the_external_find_reachable_when_both_exist`
      and `::should_run_the_native_spatial_look_and_keep_the_external_look_reachable_when_both_exist`
      (ADR-0124); case `099` and the still-green v0.3 case `087`.
- [x] **§2.16 Providers own facts.** Caught by
      `spatial_contracts.rs::should_resolve_repeated_observations_of_one_object_to_the_same_spatial_id`,
      `::should_never_let_raw_command_output_become_a_place` and
      `spatial_relationships.rs::should_name_the_same_relation_and_provider_as_trace_when_the_neighbor_is_the_open_file`
      — the spatial layer answering differently from the provider it composes is exactly what
      the last of these fails on; ADR-0131's refusing index is pinned by
      `crates/ono-spatial-index/tests/index.rs`.
- [x] **§2.17 Unknown is visible.** Caught by the six permission tests of §4.7.1's honesty box,
      chiefly `spatial_identity.rs::should_name_one_of_the_defined_permission_states_for_every_neighborhood_group`
      and `spatial_topology.rs::should_distinguish_an_unavailable_group_from_an_empty_one_when_a_domain_has_no_provider`;
      case `097`. Same proof as that box.
- [x] **§2.18 Remote boundaries are visible.** Caught by
      `spatial_remote.rs::should_announce_the_boundary_in_plain_text_when_jumping_to_a_linked_host`,
      `::should_record_the_host_and_the_scope_crossing_of_every_step_in_the_trail`,
      `::should_keep_a_remote_process_place_distinct_from_the_local_one_with_the_same_pid` and,
      for the mount boundary, `spatial_storage.rs::should_record_the_boundary_crossing_when_traversing_from_the_root_into_a_mounted_directory`.
- [x] **§2.19 The user's place survives rendering changes.** Caught by
      `spatial_interactive.rs::should_preserve_the_current_place_when_the_terminal_is_resized_with_a_place_open`,
      `spatial_map.rs::should_return_the_same_node_identities_when_the_terminal_width_changes`
      and `::should_not_change_the_current_place_when_a_map_focuses_a_node` (shared with §2.7).
- [x] **§2.20 Spatial state is inspectable and scriptable.** Caught by
      `spatial_map.rs::should_describe_identity_state_exits_and_landmarks_when_look_json_reports_a_place`,
      `spatial_navigation.rs::should_record_every_movement_with_its_kind_and_relation_when_the_trail_is_read_as_json`,
      `::should_read_back_into_the_pipeline_when_look_json_is_parsed_by_from_json` and
      `spatial_contracts.rs::should_keep_a_scripts_navigation_out_of_the_callers_place`
      (§29.2); case `092`.

#### 4.7.5 Performance budgets (v0.4 §34)

Measured in the acceptance container on the fixtures of §43.3, by a case in the shape of
`060-performance-budgets`: `docker/acceptance/cases/100-spatial-performance-budgets.case` prints
the figure it measured on every run and asserts it against the budget, as a median of repeated
runs so a loaded machine does not decide the release. The two in-suite timing tests
(`spatial_contracts.rs::should_answer_repeated_looks_far_inside_the_look_budget` and
`::should_bound_the_default_map_to_its_node_budget`) deliberately use a ten-times tolerance so
the gate is not flaky; **they do not tick these boxes** — the container case does, at the real
figure. A budget that cannot be met is documented per §4.7.2's performance box, and the ADR that
documents it is named in the box before it may be ticked.

- [x] **Interactive startup to usable prompt < 150 ms** — `100-spatial-performance-budgets`
      (`startup-to-prompt`), measured as a median of at least 40 runs, with case `090`'s
      startup-horizon assertions proving the horizon is what is being timed.
- [x] **Basic `look`, local and cached, < 50 ms** — `100-spatial-performance-budgets`
      (`warm-look`), the marginal cost of a repeated `look` in one session; case `090`
      assertion `44.1q`.
- [x] **`near` cached < 50 ms** — `100-spatial-performance-budgets` (`warm-near`), measured the
      same way against the neighborhood of a place with many neighbors.
- [x] **Map L0/L1 cached < 100 ms** — `100-spatial-performance-budgets` (`map-l0-l1`), with the
      zoom level pinned by `spatial_map.rs::should_report_the_requested_canonical_zoom_level_when_map_json_selects_one`.
- [x] **Map L2 on an ordinary host < 250 ms** — `100-spatial-performance-budgets` (`map-l2`);
      case `090` assertion `44.1r`.
- [x] **Focus and navigation inside a rendered map < 16 ms per frame** —
      `spatial_interactive.rs::should_repaint_a_focus_move_far_inside_the_frame_budget_when_the_map_is_open`,
      the frame cost of focus movement at a real PTY as the median of forty keystroke-to-paint
      round trips, in the shape of `ono-editor/tests/latency.rs` (§4.3). Measured: 88 µs median,
      386 µs slowest, against the 16 ms budget.
- [x] **Search of common indexed objects < 100 ms** — `100-spatial-performance-budgets`
      (`find-place`), a `find place --where …` over the fixture's objects answered from the
      index rather than a provider sweep (§33.1).
- [x] **Expensive discovery does not block the prompt** (§34.1). The prompt is usable before
      cold discovery finishes and the view updates progressively — asserted at a real terminal
      by `spatial_interactive.rs::should_show_the_spatial_horizon_when_the_session_starts_at_a_terminal_and_never_in_a_pipe`
      together with `100-spatial-performance-budgets` (`startup-under-load`), which repeats the
      startup measurement on a host made deliberately expensive to discover — two hundred extra
      processes and a directory of twenty thousand entries. Measured: unchanged at 0 ms over the
      harness baseline, which is what §34.1 holding looks like.
- [x] **View budgets are enforced, never unbounded** (§34.2). The text map stays at about 30
      nodes and the interactive map at 100 before mandatory clustering, and what was left out is
      disclosed — `spatial_contracts.rs::should_bound_the_default_map_to_its_node_budget`,
      `spatial_map.rs::should_bound_the_default_map_when_the_host_holds_more_objects_than_the_view_budget`,
      `::should_show_more_than_the_default_when_the_map_is_asked_for_all`,
      `spatial_storage.rs::should_summarize_a_large_directory_instead_of_enumerating_it`;
      cases `090` and `092` (shared with §2.9).

### 4.8 The v0.4.1 tranche — Hardening, Trust & Release Integrity

`docs/ono_sendai_shell_spec_v0.4.1_hardening_trust_release_integrity.md` is a maintenance layer
over the released v0.4 substrate: boundaries that enforce what they claim, resources that stay
bounded in bytes as well as in counts, streams that stream, performance measured as a curve
instead of at one small point, tests that report execution truth, and a release whose published
bytes can be traced to the inputs that produced them. This subsection is its definition of done —
the Release Definition of v0.4.1 §66.1–§66.9 and the fourteen acceptance families of §40.3, in
boxes a script can check.

The tranche is 101 GitHub issues carrying the milestones **H0 … H12**, one per phase of §57, and
every box below names the issues that deliver it; `docs/STATE.md` holds the intended order inside
each phase. **Nothing here is delivered yet.** Every box is open, so `scripts/release-check.sh`
stops at the first of them — which is what a tranche that has just started looks like, and is the
reason this subsection is written before the work rather than after it (#29). §4.9 is reserved
for the v0.5 Temporal & Causal Systems Interface.

**A box is ticked by a named automated proof** — a test that runs un-ignored in `scripts/gate.sh`,
or a case that runs in `scripts/acceptance.sh` — never by judgement, never by reading code, and
never because a phase of §57 is reported complete. That is §3's rule for every subsection and
§4.7's practice for the tranche before this one. Where the proving test does not exist yet, the
box names the file and the test name the delivering increment must create; writing it belongs to
that increment. Where a criterion of §66 is a document rather than a behaviour, the box names the
document *and* the gate check that fails when the document and the tree disagree, so no box is
closed by someone having read something.

Four conventions this subsection relies on:

- **Priorities decide what may stay open, never what may be judged.** Each box carries its
  priority class from §3.1. P0 (a boundary can claim safety without enforcing it) and P1 (realistic
  use can hang or grow without bound) are the mandatory scope of §3.2 and §3.3 and MUST be ticked.
  P2 and P3 are part of the complete product contract as well (§3.4): a release candidate MAY be
  cut with P2/P3 work in flight, and the final release satisfies all of §66. §4.8.14 holds the one
  rule that governs the whole subsection.
- **The case numbers 180–200 belong to this tranche**, and they ascend with the phase sequence, so
  a case that lands early leaves no lower number pointing at a file nobody wrote. An increment that
  has to deliver out of that order writes the case name in prose without backticks until its file
  exists — `xtask`'s reference check reads a backticked `NNN-kebab-name` as a claim and a plain one
  as a name recorded absent (`xtask/src/scan.rs::check_acceptance_case_references`, ADR-0401).
- **Appendix A is the source of every default a box asserts.** A limit box names the figure it
  measures against, and the figure comes from the contract registry rather than from a constant
  typed into the test (§52.2, #117).
- **§4.7's evidence harvester stops at this heading.** `xtask/tests/spatial_evidence.rs` reads
  `docs/ACCEPTANCE.md` §4.7 and resolves every proof it names; its passage now ends at `### 4.8`,
  so the two checklists are read apart and this one is held by its own harvester
  (`xtask/tests/hardening_evidence.rs`, the first box of §4.8.1).

#### 4.8.1 Baseline and guardrails (H0 — §57 H0, §0.5, §6, §52)

- [ ] **P0 · Every proof this subsection names resolves.** `xtask/tests/hardening_evidence.rs`
      holds §4.8 the way `xtask/tests/spatial_evidence.rs` holds §4.7 —
      `hardening_evidence.rs::should_find_every_test_the_v041_checklist_names_as_a_proof` (each
      `file.rs::name` exists, lives under a crate's `tests/` or under `xtask/tests/`, and carries
      no `#[ignore]`), `::should_find_every_acceptance_case_the_v041_checklist_names` (each case
      number is a `*.case` file `scripts/acceptance.sh` collects) and
      `::should_read_the_v041_checklist_apart_from_the_v04_one` (the §4.7 passage ends at this
      heading, so neither harvester can silently read the other's boxes). This box closes last: it
      is the mechanical statement that no other box here is ticked by nothing (#29, §3).
- [ ] **P0 · The four failing proofs of §57 H0 exist and are green.** §57 requires the failure
      before the fix, and §0.5 names the four defects the tranche is built around: the
      unauthenticated TLS client that reaches the protocol handshake (§0.5.1), the plugin that
      execs although a mandatory confinement control failed (§0.5.3), the `each` that emits nothing
      until its source closes (§0.5.5), and the high-cardinality spatial fixture that produces
      neither output nor progress (§0.5.7). Each was committed red — `#[ignore]`d with a
      `// REASON:` and a *Deferred* entry — by #31, and each is inverted by the phase that closes
      it: `crates/ono-remote/tests/authentication.rs::should_refuse_the_tls_handshake_when_the_client_presents_no_certificate`,
      `crates/ono-kuang-supervisor/tests/confinement.rs::should_never_exec_the_plugin_when_a_mandatory_control_cannot_be_installed`,
      `crates/ono-cli/tests/streaming.rs::should_emit_the_first_value_before_the_source_closes`,
      `crates/ono-spatial-query/tests/profiles.rs::should_answer_or_refuse_within_the_interactive_budget_on_the_profile_l_fixture`.
      The box is ticked when all four run un-ignored and green, and `docs/STATE.md` records that
      each was red first (#31).
- [ ] **P2 · The frozen baseline exists and is readable by the gate.** `docs/baselines/v0.4.1.json`
      records the tranche's starting point — test counts by outcome, the six §32.3 metrics for
      every benchmark of §37.1, and the release artifact hashes and workflow inputs of Appendix H —
      so a later regression has something to be a regression against (§32.4) —
      `xtask/tests/perf.rs::should_read_the_frozen_v041_baseline_and_find_every_metric_it_declares`
      and `::should_compare_a_benchmark_result_against_the_baseline_for_its_reference_environment`
      (#30).
- [ ] **P2 · The hardening policy data lives in machine-readable registries.** The seven contract
      domains of §52.1 — `security_boundaries`, `remote_limits`, `materialization_limits`,
      `kuang_confinement_controls`, `performance_profiles`, `expected_test_skips`, `release_inputs`
      — exist under `docs/spec/hardening/`, and runtime defaults, generated reference and tests read
      the same source (§52.2), so `max_connections = 32` is typed once —
      `xtask/tests/contracts.rs::should_hold_every_hardening_limit_against_the_value_the_shell_uses`,
      `::should_reject_an_unknown_capability_id_in_an_authorization_fixture`,
      `::should_reject_an_unknown_control_id_in_a_kuang_tier_definition` (§52.3) and
      `cargo run -p xtask -- spec-check` on every gate run (#117).
- [ ] **P2 · The security boundary inventory is generated and owned.** The twelve boundaries of
      §6.1 — from `remote.tcp.transport` to `release.publish` — are declared with their input
      trust, their required enforcement and the one crate that owns each (§6.2), and the page under
      `docs/reference/` is produced by `cargo xtask docs` rather than maintained by hand —
      `xtask/tests/reference.rs::should_render_a_boundary_page_that_matches_the_inventory` and
      `xtask/tests/contracts.rs::should_name_an_owning_crate_and_a_security_test_for_every_declared_boundary`,
      which is what makes §20's acceptance principle checkable: a boundary with no test naming it
      fails the gate (#118).

#### 4.8.2 The direct link authenticates both ends (H1 — §66.1, §7, §8, §13)

§66.1's first, fifth and sixth bullets. A directly listening TLS agent authenticates itself to the
client today and accepts clients that present no certificate (§0.5.1); the protocol-level
`Identity` is self-reported metadata (§65.1). Every box here is P0 because a boundary that claims
authentication without performing it is exactly §3.1's release blocker.

- [ ] **P0 · Transport identity is symmetric and transport-neutral.** `HostKey` becomes a
      `PeerIdentity` that names either end of a link, the runtime user of a session stays a
      separate field from the transport credential (§7.3), and the SSH-carried stdio agent keeps
      its existing external authentication (§4.3) —
      `crates/ono-protocol/tests/trust.rs::should_carry_the_peer_identity_of_either_end_of_a_link`,
      `::should_keep_the_runtime_user_separate_from_the_transport_credential`,
      `crates/ono-remote/tests/agentless.rs` still green over the ssh path (#32, §7.2, §56.1).
- [ ] **P0 · A client has a persistent identity of its own.** `~/.config/ono/link_identity.pem`
      is created on first use with `0600`, is reused across invocations so the fingerprint an
      operator authorizes stays the same, and an existing `host_key.pem` migrates without a manual
      step (§8.2) — `crates/ono-cli/tests/link_identity.rs::should_create_a_client_identity_with_owner_only_permissions_on_first_use`,
      `::should_present_the_same_fingerprint_on_every_later_invocation`,
      `::should_migrate_an_existing_host_key_file_into_the_canonical_identity_path` (#33, §8.1,
      §8.4).
- [ ] **P0 · A readable private key is refused, and the diagnostic prints no key material.** A
      `link_identity.pem` that is group- or world-readable stops the operation and names the path
      and the required mode —
      `crates/ono-cli/tests/link_identity.rs::should_refuse_an_identity_file_that_is_group_or_world_readable`,
      `::should_name_the_path_and_the_required_permissions_without_printing_key_material`, and the
      §59.6 half of case `181` (#34, §8.3).
- [ ] **P0 · The listener requires an authenticated client certificate.** No protocol byte from an
      unauthenticated TCP client reaches the agent, and `peer_key()` is `Some` for every accepted
      network client — `crates/ono-remote/tests/authentication.rs::should_refuse_the_tls_handshake_when_the_client_presents_no_certificate`,
      `::should_refuse_a_malformed_client_certificate`,
      `::should_refuse_a_client_certificate_whose_signature_proof_fails`,
      `::should_expose_the_authenticated_fingerprint_of_every_accepted_client`, and
      `crates/ono-remote/tests/agent.rs::should_reach_no_protocol_frame_from_an_unauthenticated_tcp_client`
      (#35, the work package written out in §58.1; §2.1, §2.2, §7, §13.1).
- [ ] **P0 · The client verifies the server the same way the server verifies the client.** Both
      ends prove possession of a persistent key, and a client that cannot verify the server's
      identity refuses the link rather than continuing —
      `crates/ono-remote/tests/authentication.rs::should_prove_possession_of_a_persistent_key_at_both_ends_of_a_link`,
      `::should_refuse_the_link_when_the_server_identity_cannot_be_verified`, case `180` (#36,
      §7.1, §7.3).
- [ ] **P0 · Downgrade is impossible and never automatic.** ALPN is `ono/2`, the negotiated
      protocol version is bound to the authenticated handshake, a wrong ALPN is refused, and no
      code path falls back to the legacy unauthenticated direct protocol on failure —
      `crates/ono-remote/tests/authentication.rs::should_refuse_a_client_that_offers_the_wrong_alpn_protocol`,
      `::should_never_retry_a_failed_authenticated_link_as_an_unauthenticated_one`,
      `crates/ono-protocol/tests/handshake.rs::should_bind_the_negotiated_version_to_the_authenticated_handshake`
      (#38, §13.2, §13.3, §13.4).
- [ ] **P0 · No unauthenticated network mode is reachable from the CLI.** No flag, config key or
      environment variable turns the canonical agent into an unauthenticated listener, and the
      attempt is refused with a stable error rather than ignored —
      `crates/ono-cli/tests/agent_startup.rs::should_offer_no_flag_that_starts_an_unauthenticated_network_listener`,
      `::should_refuse_a_configuration_that_asks_for_an_unauthenticated_network_mode`, and
      `xtask/tests/contracts.rs::should_declare_no_unauthenticated_transport_for_the_tcp_boundary`
      against the §6.1 inventory (#39, §7.4).
- [ ] **P1 · The fingerprint an operator has to compare is printable at both ends.**
      `ono --print-peer-key` prints the client fingerprint in the §8.5 display form, and
      `ono --agent --print-host-key` keeps working for the agent —
      `crates/ono-cli/tests/link_identity.rs::should_print_the_client_fingerprint_in_the_documented_display_form`,
      `::should_keep_printing_the_agent_host_key_under_the_existing_flag`, case `181` (#37, §8.5).
- [ ] **P0 · Host-key pinning is live on the production transport.** ADR-0037 §4 left ssh links on
      `TrustPolicy::Unauthenticated`, so the complete trust store of
      `crates/ono-remote/tests/trust.rs` was never consulted in production and no case asserted the
      E0603 refusal. The authenticated TCP transport this phase builds is what its exit test needs
      (ADR-0274) — `crates/ono-remote/tests/trust.rs::should_refuse_a_changed_host_key_with_the_stable_safety_code`
      reached through the production path by
      `crates/ono-cli/tests/authenticated_link.rs::should_consult_the_pin_store_on_the_transport_a_user_actually_gets`,
      and case `180` asserting E0603 in the container (#18).

#### 4.8.3 Authorization derives from the authenticated identity (H2 — §66.1, §9, §10, §14)

§66.1's second, third and fourth bullets. Authentication proves possession of a key; it does not
prove the operator wants that key to see the system (§9.1). §65.2 and §65.3 name the two ways this
is got wrong — self-reported authorization, and authorization that exists only in the negotiation.

- [x] **P0 · The authorization store is explicit and strictly parsed.**
      `~/.config/ono/authorized_clients` is line-oriented and human-readable, its entry model is
      the §9.3 record (fingerprint, optional label, `observe`, exact `actions`), an unknown field is
      rejected, and a malformed non-comment line fails the load —
      `crates/ono-cli/tests/authorized_clients.rs::should_parse_the_documented_entry_model_including_an_empty_action_set`,
      `::should_reject_an_unknown_field_in_an_authorization_entry`,
      `::should_fail_to_load_the_store_when_one_non_comment_line_is_malformed`,
      `::should_never_treat_a_malformed_store_as_an_empty_one`,
      `::should_distinguish_a_missing_store_from_a_corrupt_one`, case `187` (#40, §9.2, §9.3,
      §59.5, §65.2, §65.4; ADR-0466).
- [x] **P1 · Store updates are atomic.** A concurrent reader sees the old file or the new one, an
      interrupted write leaves the previous store intact, and the file's permissions survive the
      update — `crates/ono-cli/tests/authorized_clients.rs::should_replace_the_store_atomically_so_a_reader_never_sees_a_partial_file`,
      `::should_leave_the_previous_store_intact_when_a_write_is_interrupted`,
      `::should_keep_the_owner_only_permissions_of_the_store_across_an_update` (#41, §9.8;
      ADR-0467).
- [x] **P0 · The operator manages client keys through the command registry.**
      `get client-key`, `add client-key`, `set client-key` and `remove client-key` answer as
      objects, are declared in `docs/spec/commands/`, and carry help and completion —
      `crates/ono-cli/tests/client_keys.rs::should_list_every_authorized_client_as_an_object_when_get_client_key_runs`,
      `::should_add_a_client_key_and_show_it_in_the_next_listing`,
      `::should_change_exactly_the_grants_named_when_set_client_key_runs`,
      `::should_remove_a_client_key_so_the_store_no_longer_lists_it`,
      `::should_carry_help_and_completion_for_every_client_key_command`,
      `crates/ono-cli/tests/authenticated_link.rs::should_refuse_the_next_connection_from_a_revoked_client_key`,
      case `186`, and `cargo run -p xtask -- spec-check` for the registry half (#42, §9.7;
      ADR-0468).
- [x] **P0 · A newly added client observes and nothing more.** The default grant is
      `observe=true` with an empty action set, an unlisted client is refused before negotiation, and
      neither state is reachable by an accident of parsing —
      `crates/ono-cli/tests/client_keys.rs::should_grant_observe_only_when_a_client_key_is_added_without_grants`,
      `crates/ono-protocol/tests/authorization.rs::should_refuse_an_unlisted_client_before_provider_negotiation`,
      `::should_disclose_no_process_schema_or_capability_inventory_to_an_unlisted_client`,
      `::should_let_an_authorized_observer_read_and_refuse_it_an_action`,
      `crates/ono-cli/tests/authenticated_link.rs::should_refuse_an_authenticated_client_the_agent_never_authorized`,
      cases `182` and `184` (#43, §9.4, §59.1, §59.2; ADR-0468).
- [x] **P0 · An action grant is an exact capability ID.** A grant names one capability, no wildcard
      or risk-class pattern is accepted by the parser, an unknown capability ID is denied, and
      `service.manage` being granted leaves `process.signal` refused —
      `crates/ono-cli/tests/authorized_clients.rs::should_refuse_a_wildcard_or_risk_class_in_an_action_grant`,
      `::should_refuse_a_wildcard_from_the_command_that_writes_the_store`,
      `crates/ono-protocol/tests/authorization.rs::should_deny_an_action_whose_capability_id_is_unknown`,
      `::should_deny_a_capability_introduced_after_the_grant_was_written`,
      `::should_leave_every_ungranted_action_refused_when_one_action_is_granted`, case `185`
      (#44, §9.5, §9.6, §59.3, Appendix C; ADR-0469).
- [x] **P1 · The policy for a connection is decided once and cannot change under it.** The
      `AuthorizationContext` is built from the authenticated fingerprint at accept, is immutable
      for the life of the connection, and carries no self-reported field from the peer —
      `crates/ono-protocol/tests/authorization.rs::should_build_the_authorization_context_from_the_authenticated_fingerprint_alone`,
      `::should_keep_the_authorization_context_immutable_for_the_life_of_the_connection` (#47,
      §10.3; ADR-0470, which also records why live revocation is deferred to H3).
- [x] **P0 · The offer a client receives is filtered by its policy.** A capability the client may
      not use is absent from the negotiated offer, so the inventory itself carries no information
      the policy withholds — `crates/ono-protocol/tests/authorization.rs::should_offer_only_the_capabilities_the_clients_policy_allows`,
      `::should_leave_an_ungranted_action_capability_out_of_the_offer_the_provider_advertises`,
      case `183` (#45, §10.1; ADR-0471).
- [x] **P0 · Dispatch refuses independently of the offer.** A request for a capability the offer
      omitted is refused at dispatch as well, on every dispatch path, so a client that constructs
      the request by hand gains nothing (§65.3) —
      `crates/ono-protocol/tests/authorization.rs::should_refuse_a_request_for_a_capability_the_offer_omitted`,
      `::should_refuse_it_on_every_dispatch_path_the_server_exposes` — which drives query,
      subscribe, adapt and act and asserts that no provider code ran — and case `184` (#46, §10.2,
      §20; ADR-0472). The cross-check of the §6.1 boundary inventory needs
      `docs/spec/hardening/security_boundaries.yaml`, which no phase has written yet; it is
      recorded under *Deferred* in `docs/STATE.md` and belongs to the phase that creates the
      inventory.
- [x] **P0 · Refusals are stable, structured and non-interactive.**
      `remote.unauthenticated`, `remote.unauthorized` and `remote.capability_denied` are declared
      in `docs/spec/errors.yaml`, carry the deciding boundary in structured details, and never
      prompt — `crates/ono-protocol/tests/authorization.rs::should_declare_the_three_remote_refusal_codes_with_their_details`,
      `::should_answer_the_same_stable_code_for_the_same_refusal_every_time`,
      `::should_refuse_without_prompting_when_no_terminal_is_attached` (§59.9),
      `crates/ono-cli/tests/authenticated_link.rs::should_report_an_authenticated_but_unauthorized_link_as_exactly_that`,
      cases `182`, `184`, `186` and `187` (#48, §10.4, §53.1, §53.2, §53.3; ADR-0473).
- [x] **P1 · The agent records who connected and what was decided.** Accept, authentication
      outcome, authorization outcome, capability decisions and disconnect are structured audit
      events carrying the §14.2 fields, and no key material or payload appears in one —
      `crates/ono-remote/tests/audit.rs::should_emit_a_structured_event_for_every_connection_lifecycle_step`,
      `::should_record_the_refusal_of_a_client_nobody_authorized`,
      `::should_record_a_client_that_proved_no_key_as_a_verification_failure`,
      `::should_carry_the_fingerprint_and_the_decision_on_every_authorization_event`,
      `::should_never_write_key_material_or_payload_bytes_into_an_audit_event`,
      `::should_name_every_event_class_the_specification_lists`,
      `crates/ono-cli/tests/authenticated_link.rs::should_write_a_structured_audit_line_for_every_decision_the_agent_makes`,
      case `184` (#49, §14.1, §14.2; ADR-0474). `connection.limit_denied` is declared in the
      closed set and raised by nobody until H3 builds the connection semaphore of §12.1.
- [x] **P1 · `get link` distinguishes the trust concepts.** Authenticated, authorized, pinned
      and self-reported are separate fields with separate values, so a reader can tell a proved
      identity from a claimed one —
      `crates/ono-cli/tests/authenticated_link.rs::should_distinguish_authenticated_authorized_pinned_and_self_reported_on_a_link`,
      `::should_report_an_authenticated_but_unauthorized_link_as_exactly_that`,
      `::should_report_no_proved_key_over_a_transport_that_proves_nothing`, case `186`
      (#50, §14.3, §19.1; ADR-0475).

#### 4.8.4 The listening agent stays bounded (H3 — §3.3, §11, §12)

§3.3's last mandatory P1 item: connection and per-peer resource limits, so an agent a user exposes
on a network cannot be exhausted by a peer that simply keeps connecting.

- [ ] **P1 · One `Limits` contract, and no unlimited limit in production.** Every ceiling of
      Appendix A is a field of one typed contract read from `docs/spec/hardening/remote_limits.yaml`,
      no production path constructs an unlimited value, and the same numbers reach help, generated
      reference and the tests —
      `crates/ono-remote/tests/limits.rs::should_read_every_connection_ceiling_from_the_one_limits_contract`,
      `::should_offer_no_production_constructor_that_leaves_a_limit_unbounded`,
      `xtask/tests/contracts.rs::should_hold_every_hardening_limit_against_the_value_the_shell_uses`
      (#54, §12.4, §52.2).
- [ ] **P1 · The global connection ceiling holds at 32.** The thirty-third concurrent connection is
      refused with a stable error and the agent keeps serving the thirty-two it has —
      `crates/ono-remote/tests/limits.rs::should_refuse_the_connection_past_the_global_ceiling_and_keep_serving_the_rest`,
      `::should_release_a_slot_when_a_connection_closes`, case `188` (#51, §12.1, Appendix A).
- [ ] **P1 · Half-open handshakes cannot accumulate.** At most sixteen handshakes are pending at
      once, one that has not completed within ten seconds is dropped, and neither limit is reachable
      by a peer that opens a TCP connection and stops —
      `crates/ono-remote/tests/limits.rs::should_refuse_a_seventeenth_pending_handshake`,
      `::should_drop_a_handshake_that_has_not_completed_within_the_timeout`, case `188` (#52,
      §12.2, Appendix A).
- [ ] **P1 · One fingerprint gets four connections.** The per-client ceiling is keyed on the
      authenticated fingerprint rather than on the source address, so it cannot be sidestepped by
      reconnecting from elsewhere —
      `crates/ono-remote/tests/limits.rs::should_refuse_a_fifth_connection_from_one_authenticated_fingerprint`,
      `::should_key_the_per_client_ceiling_on_the_fingerprint_rather_than_the_address` (#53,
      §12.3, Appendix A).
- [ ] **P1 · One failing connection never takes the accept loop with it.** A panic, a decode
      failure or an abrupt disconnect on one connection leaves the listener accepting and the other
      sessions intact — `crates/ono-remote/tests/limits.rs::should_keep_accepting_after_one_connection_fails`,
      `::should_leave_every_other_session_intact_when_one_connection_is_aborted` (#57, §12.6).
- [ ] **P1 · `--listen` says what it will accept, and refuses to listen for nobody.** Startup
      prints the bind address, the effective limits and the number of authorized clients, an empty
      or absent store refuses to start rather than listening permissively, and the default bind is
      the one §11.2 names — `crates/ono-cli/tests/agent_startup.rs::should_print_the_bind_address_the_limits_and_the_authorized_client_count_when_listening_starts`,
      `::should_refuse_to_listen_when_the_authorization_store_is_empty_or_absent`,
      `::should_bind_the_documented_default_address_when_none_is_given`, case `187` (#55, §11.1,
      §11.2, §11.3).
- [ ] **P2 · Revocation has stated semantics.** Removing a client key refuses its next connection,
      and whether an established session is terminated is decided rather than left to chance: the
      box is ticked by `crates/ono-cli/tests/client_keys.rs::should_refuse_the_next_connection_after_a_client_key_is_removed`
      together with either `crates/ono-remote/tests/limits.rs::should_terminate_an_established_session_when_its_authorization_is_revoked`
      or an ADR that records live revocation as deferred and is named in this box before it is
      ticked (#56, §12.5).

#### 4.8.5 KUANG/11 native confinement fails closed (H4 — §66.1, §15–§20, Appendix D)

§66.1's seventh and eighth bullets. §0.5.3 found confinement syscalls whose return values were
ignored, and §65.5 names the documentation failure that travels with it: calling the native tier a
sandbox when it provides no filesystem or network isolation.

- [ ] **P0 · Mandatory and best-effort controls are one table, not scattered constants.** Appendix
      D's eleven rows exist in `docs/spec/hardening/kuang_confinement_controls.yaml` with their tier
      and their failure behaviour, the supervisor reads that table, and an unknown control ID fails
      the gate — `crates/ono-kuang-supervisor/tests/confinement.rs::should_classify_every_control_the_confinement_table_declares`,
      `::should_treat_a_control_the_table_calls_mandatory_as_mandatory`,
      `xtask/tests/contracts.rs::should_reject_an_unknown_control_id_in_a_kuang_tier_definition`
      (#58, §16.1, §16.4, §52.3).
- [ ] **P0 · Every security-relevant syscall return value is checked.** No confinement call
      discards its result, and the check is asserted mechanically rather than by review —
      `crates/ono-kuang-supervisor/tests/confinement.rs::should_report_the_failing_control_when_a_confinement_syscall_returns_an_error`,
      `::should_check_the_result_of_every_control_the_table_marks_mandatory`, and
      `xtask/tests/scan.rs::should_report_an_unchecked_confinement_syscall_result` over the
      supervisor's process-setup code (#59, §16.2, §0.5.3).
- [ ] **P0 · A pre-exec failure prevents the exec.** With an injected failure of
      `PR_SET_NO_NEW_PRIVS` or of a mandatory `setrlimit`, the spawn fails, the caller receives
      `plugin.no_new_privs_failed` / `plugin.resource_limit_failed` / `plugin.confinement_failed`
      naming the control, and a marker the plugin would create on startup stays absent —
      `crates/ono-kuang-supervisor/tests/confinement.rs::should_never_exec_the_plugin_when_a_mandatory_control_cannot_be_installed`,
      `::should_leave_the_plugins_startup_marker_absent_after_a_failed_confinement_setup`,
      `::should_name_the_control_that_could_not_be_installed_in_the_structured_error`, case `189`
      (#60, §16.3, §59.7, §59.8, §65.4).
- [ ] **P1 · Every spawn carries a confinement report.** The report states, per control, whether it
      was applied, skipped as best-effort or refused, and it is available to the operator without
      `RUST_LOG=debug` — `crates/ono-kuang-supervisor/tests/confinement.rs::should_report_the_state_of_every_control_after_a_successful_spawn`,
      `::should_mark_a_best_effort_control_that_was_not_available_as_skipped_rather_than_applied`,
      case `189` (#61, §16.5).
- [ ] **P1 · The four plugin failure classes are distinguishable.** Launch failure, protocol
      quarantine, resource-limit termination and crash are separate outcomes with separate codes,
      and a crash of one plugin leaves the shell and the other plugins running —
      `crates/ono-kuang-supervisor/tests/failure_classes.rs::should_distinguish_a_launch_failure_from_a_quarantine_a_resource_kill_and_a_crash`,
      `::should_keep_the_shell_and_the_other_plugins_running_when_one_plugin_crashes`, case `189`
      (#62, §18).
- [ ] **P0 · The documentation states the native tier honestly.** README, Wiki, `help` and the
      generated reference use the §19.1 terms, say that the native tier provides no filesystem and
      no network isolation, and never call it a sandbox — `xtask/tests/terminology.rs::should_reject_a_document_that_calls_the_native_tier_a_sandbox`,
      `::should_find_the_native_isolation_disclaimer_in_every_document_that_describes_the_kuang_tier`
      running over the repository on every gate run (#63, §15.1, §15.2, §51.2, §0.5.4, §65.5).
- [ ] **P2 · The execution tier is a name, not a boolean.** The `sandboxed: bool` field is replaced
      by a named tier that says which controls are in force, and no UI infers filesystem or network
      isolation from the presence of the other controls —
      `crates/ono-kuang-supervisor/tests/confinement.rs::should_report_a_named_execution_tier_rather_than_a_sandboxed_boolean`,
      `crates/ono-cli/tests/plugins.rs::should_show_the_execution_tier_and_its_controls_when_a_plugin_is_inspected`,
      and `cargo run -p xtask -- spec-check` for the `docs/spec/kuang/` contract change (#64,
      §17.2, §17.3).

#### 4.8.6 Resource correctness (H5 — §66.2, §21–§24, Appendix A)

§66.2's four bullets. §65.6 names the shape of the defect: a limit counted in items while the
bytes behind them are unbounded.

- [x] **P1 · A value's retained size is estimable, deterministically.** The estimator answers the
      same figure for the same value on every run, is defined for every `Value` variant, stays
      within a documented tolerance of the payload it can measure, and charges a shared `Arc`
      once — which is what separates a memory estimate from a fan-out count —
      `crates/ono-value/tests/size_estimate.rs::should_answer_the_same_estimate_for_the_same_value_on_every_run`,
      `::should_define_an_estimate_for_every_value_variant`,
      `::should_stay_within_the_documented_tolerance_of_the_measured_retained_size`,
      `::should_count_shared_payload_once_within_one_estimate` (#65, §21.2, §56.6, ADR-0452).
- [x] **P1 · One `Budget` type, with no unlimited default.** Materialization, captures and history
      share one abstraction, constructing a budget requires stating both ceilings, and exceeding
      one is a refusal rather than a truncation —
      `crates/ono-pipeline/tests/budget.rs::should_require_both_an_item_and_a_byte_ceiling_when_a_budget_is_constructed`,
      `::should_offer_no_default_that_leaves_a_budget_unlimited`,
      `::should_refuse_rather_than_truncate_when_a_budget_is_exceeded` (#66, §21.1, §21.3,
      ADR-0453).
- [x] **P1 · Materialization is bounded in items and in bytes.** The default global materializer
      refuses past 100 000 values and past 128 MiB, the byte ceiling triggers on a few large values
      while the item count stays far below its own, and one helper owns the enforcement so no caller
      recreates it — `crates/ono-pipeline/tests/budget.rs::should_refuse_the_hundred_thousand_and_first_value_a_global_operation_collects`,
      `::should_refuse_on_the_byte_ceiling_when_a_few_large_values_exceed_it`,
      `::should_bound_every_transform_that_buffers_its_whole_input`,
      `crates/ono-cli/tests/resource_limits.rs::should_route_every_global_collection_through_the_budget_aware_helper`,
      case `190` (#67, §22.1, §22.2, §30.2, §60.4, §60.5, Appendix A, ADR-0454).
- [x] **P1 · An operation that needs finite input refuses an unbounded one immediately.** `sort`
      and its kind answer a stable refusal naming the requirement when the upstream is declared
      unbounded, before waiting —
      `crates/ono-pipeline/tests/boundedness.rs::should_refuse_an_unbounded_upstream_before_waiting_when_the_operation_requires_finite_input`,
      `crates/ono-cli/tests/resource_limits.rs::should_name_the_finiteness_requirement_and_the_declaring_stage_in_the_refusal`,
      and `xtask/tests/contracts.rs::should_place_every_pipeline_operation_in_the_streaming_classification_matrix`
      for Appendix E, which §65.8 makes a release requirement (#68, §22.3, ADR-0455).
- [x] **P1 · `explain` shows what will be materialized.** A plan says which stages materialize,
      which require finite input and which budget applies, before the pipeline runs —
      `crates/ono-command/tests/explain.rs::should_mark_every_materializing_stage_in_the_plan`,
      `::should_name_the_finiteness_requirement_and_the_budget_of_each_materializing_stage`,
      case `192` (#69, §22.4, ADR-0460).
- [x] **P1 · Capture buffers go through the same budget.** A nested command capture is charged
      against a budget with a 256 MiB aggregate ceiling per command, nesting accumulates against
      that ceiling rather than resetting it, and exceeding it refuses —
      `crates/ono-cli/tests/resource_limits.rs::should_charge_a_nested_command_capture_against_the_shared_budget`,
      `::should_accumulate_nested_captures_against_the_one_per_command_ceiling`,
      `::should_refuse_a_capture_that_would_exceed_the_command_ceiling` (#70, §23.1, §23.2, §23.4,
      Appendix A, ADR-0457).
- [ ] **P1 · Cancellation stops a capture growing.** Cancelling a command whose capture is filling
      releases it, deterministically: the operation unwinds, the source stops producing, and the
      next capture has its whole allowance —
      `crates/ono-cli/tests/resource_limits.rs::should_stop_capture_growth_within_the_cancellation_budget`,
      `crates/ono-pipeline/tests/cancellation.rs::should_stop_a_capture_growing_when_the_scope_is_cancelled`.
      **The p95 < 100 ms / p99 < 250 ms half of §23.3 is not ticked here.** §37.2 measures a target
      on a *named reference environment*, which #84 delivers, and a millisecond threshold asserted
      on whatever ran `cargo test` is issue #21's defect rather than a proof; the measured
      distribution is owed by the benchmark harness of §4.8.8 (#83, #84). ADR-0459 records what was
      measured while writing this — 100 cancellations in 0.07 s, two orders of magnitude inside the
      p95 target — and why it is not asserted. This box is ticked when that benchmark reports
      (#71, §23.3, §61.5, Appendix A).
- [x] **P1 · Retained history is bounded and says when it truncated.** Sixteen slots, 10 000 values
      and 16 MiB per result, 64 MiB in total with oldest-first eviction, a truthful marker on any
      truncated entry, and — the invariant that matters — the pipeline's own output is unaffected by
      any of it — `crates/ono-history/tests/result_history.rs::should_evict_the_oldest_result_when_the_total_byte_ceiling_is_reached`,
      `::should_mark_a_result_it_kept_only_in_part_and_say_how_much_it_kept`,
      `::should_leave_the_emitted_output_complete_when_the_retained_copy_is_truncated`,
      `::should_not_retain_a_single_value_larger_than_the_per_result_byte_limit`,
      `crates/ono-cli/tests/resource_limits.rs::should_leave_the_pipeline_output_complete_when_history_could_not_keep_it_all`,
      case `191` (#72, §24, §60.6, §67.6, Appendix A, ADR-0458).
- [x] **P1 · The refusals are stable error codes.** `resource.item_limit`, `resource.byte_limit`
      and `resource.materialization_limit` are declared in `docs/spec/errors.yaml` with the details
      §53.3 requires — the limit, the observed figure and the stage that enforced it — so automation
      reads a code instead of a message (§53.2) —
      `crates/ono-value/tests/errors.rs::should_declare_the_three_resource_refusal_codes_with_their_details`,
      `crates/ono-cli/tests/resource_limits.rs::should_answer_the_same_resource_code_for_the_same_refusal_every_time`,
      `::should_carry_the_limit_and_the_consumption_but_no_payload_in_a_resource_refusal`,
      case `190` (#73, §21.4, §53.1, ADR-0453).
- [x] **P2 · The limits are configurable within validated ranges.** `limits.*` keys exist for the
      Appendix A defaults, a value outside the permitted range is refused at load with the range in
      the message, and the environment overrides of §55.4 behave as documented —
      `crates/ono-cli/tests/meta_config.rs::should_accept_every_documented_limits_key_and_reject_an_unknown_one`,
      `::should_refuse_a_limits_value_outside_its_permitted_range_and_name_the_range`,
      `::should_apply_the_documented_environment_override_for_a_limits_key`,
      `xtask/tests/contracts.rs::should_reject_a_limit_whose_default_lies_outside_its_own_range`
      (#74, §55.1–§55.4, ADR-0456).
- [x] **P3 · The effective limits are inspectable.** `inspect limits` answers the non-secret
      runtime limits in force as objects, so a test and a user read the same figures the shell uses
      — `crates/ono-cli/tests/resource_limits.rs::should_answer_the_effective_non_secret_limits_when_inspect_limits_runs`,
      `::should_answer_the_same_figures_inspect_limits_shows_from_the_contract_registry`, case
      `192` (#120, §54.3, ADR-0461).
- [x] **P1 · `each` consumes and emits incrementally.** `source | each { $it } | take 1` completes
      while the source is still waiting, the block runs for the first value before the second is
      required, order and seriality are unchanged, and no complete-input `Vec<Value>` capture
      remains in the path — `crates/ono-cli/tests/streaming.rs::should_emit_the_first_value_before_the_source_closes`,
      `::should_run_the_block_for_one_item_before_the_next_item_is_required`,
      `::should_keep_the_input_order_and_the_serial_execution_of_the_block`,
      `crates/ono-cli/tests/each_streaming.rs::should_answer_take_one_before_the_source_closes_when_each_transforms_a_waiting_stream`
      (the §57 phase H0 failure proof, un-ignored by this increment with its assertion unchanged),
      `crates/ono-pipeline/tests/streaming_transforms.rs::should_hold_no_more_than_the_bounded_channel_and_one_in_flight_frame`
      (#75, the work package written out in §58.2; §25.1–§25.4, §60.1, ADR-0480).
- [x] **P1 · `each` accepts an unbounded source.** A source declared unbounded is a legal input,
      the time to first output does not depend on how much the source will eventually produce, and
      memory stays flat while it runs —
      `crates/ono-cli/tests/streaming.rs::should_accept_a_source_declared_unbounded_without_refusing_it`,
      `::should_keep_memory_flat_while_an_unbounded_source_is_consumed`,
      `crates/ono-cli/tests/each_streaming.rs::should_accept_an_unbounded_source_when_each_transforms_it`
      (the second phase H0 failure proof, likewise un-ignored unchanged), case `193` (#76, §25.6,
      §25.7, §60.1, ADR-0480).
- [x] **P1 · Control flow survives the streamed `each`.** `break`, `continue`, `return`, an error
      and `exit` behave as they did, `break` stops upstream consumption promptly, and the `Flow`
      representation stays explicit —
      `crates/ono-cli/tests/streaming.rs::should_stop_upstream_consumption_promptly_when_the_block_breaks`,
      `::should_skip_exactly_one_item_when_the_block_continues`,
      `::should_return_from_the_enclosing_function_when_the_block_returns`,
      `::should_propagate_a_block_error_with_the_status_it_had_before_the_rewrite`,
      `::should_leave_the_shell_with_the_status_the_block_exited_with`,
      case `194` (#77, §25.5, §30.3, §60.3, ADR-0480).
- [x] **P1 · Every remaining capture is classified and justified.** An inventory names every
      `Vec<Value>` collection in the evaluator with the class of §26.1 it belongs to, each one is
      either removed or bounded by a budget, and a new unclassified capture fails the gate — as
      does an entry whose capture is gone, an invented class, and an `implementation_convenience`
      capture no ADR justifies —
      `xtask/tests/scan.rs::should_report_an_evaluator_capture_the_streaming_inventory_does_not_classify`,
      `::should_report_an_inventory_entry_whose_capture_is_no_longer_in_the_evaluator`,
      `::should_report_an_implementation_convenience_capture_that_no_decision_record_justifies`,
      `::should_report_a_capture_whose_class_is_not_one_the_specification_defines`,
      `::should_accept_an_evaluator_capture_the_inventory_classifies`,
      `::should_report_this_repository_as_classifying_every_evaluator_capture`, with the inventory
      in `docs/spec/hardening/streaming.yaml` (#78, §26.1, §65.7, ADR-0479).
- [x] **P1 · A function is a pipeline stage.** A function whose body is one native pipeline
      forwards values as it produces them, its scope lives exactly as long as its invocation, and
      the call does not turn the pipeline into a two-phase collection. A body that cannot be
      continued says so in `explain` and refuses an unbounded input, which is what §26.2 requires
      of the case that still collects —
      `crates/ono-cli/tests/streaming.rs::should_forward_values_from_a_function_as_it_produces_them`,
      `::should_drop_the_invocation_scope_when_the_function_call_ends`,
      `::should_keep_a_pipeline_streaming_when_a_function_sits_in_the_middle_of_it`,
      `::should_say_in_explain_which_calls_stream_and_which_collect`,
      `::should_refuse_an_unbounded_body_the_call_would_have_to_collect` (#79, §26.2, §26.3,
      §65.8, ADR-0481).
- [x] **P1 · Backpressure and cancellation survived the rewrite.** A fast source feeding a slow
      block grows no queue beyond the bounded channel plus the documented in-flight values,
      cancellation wins over an in-flight block, a cancelled child process is reaped, the
      reference capacity is still 64, and an unbounded channel on the data path fails the gate —
      `crates/ono-pipeline/tests/backpressure.rs::should_keep_the_retained_queue_within_the_bounded_channel_when_the_consumer_is_slow`,
      `::should_keep_the_reference_channel_capacity_the_specification_names`,
      `crates/ono-pipeline/tests/cancellation.rs::should_stop_an_in_flight_block_when_the_pipeline_is_cancelled`,
      `crates/ono-cli/tests/streaming.rs::should_reap_the_child_process_of_a_cancelled_stage`,
      `xtask/tests/scan.rs::should_report_an_unbounded_channel_on_the_pipeline_data_path`,
      `::should_find_no_unbounded_pipeline_channel_in_this_repository`,
      case `194` (#81, §28.1–§28.4, §60.2, §65.7, ADR-0482).
- [ ] **P2 · Cross-kind stream ordering is documented and tested.** Per-channel order is total,
      the ordering guarantee between values, diagnostics and status is stated in one place, and the
      way to obtain a total order where a caller needs one is exercised —
      `crates/ono-pipeline/tests/ordering.rs::should_deliver_every_event_of_one_channel_in_the_order_it_was_produced`,
      `::should_hold_the_documented_guarantee_between_values_diagnostics_and_status`,
      `::should_produce_a_total_order_when_the_caller_asks_for_one`, with the contract in
      `docs/reference/` and held by `xtask/tests/reference.rs::should_render_the_stream_ordering_contract_from_the_registry`
      (#80, §27.1–§27.4).

#### 4.8.8 Performance (H7 — §66.4, §32–§37, Appendix F)

§66.4's five bullets. §32.1 is the premise: one small fixture inside a latency budget proves
nothing about a real host, and §65.9 names progress-free interactive computation as the failure
mode this phase removes.

- [ ] **P1 · Profile S, M and L fixtures exist and are reproducible.** The process, graph and
      socket cardinalities of Appendix F.1 and F.2 are built by the harness rather than borrowed
      from the developer's machine, the payload profiles of F.3 exist for the byte budgets, and the
      same fixture is reconstructible from the registry —
      `crates/ono-spatial-query/tests/profiles.rs::should_build_every_declared_profile_at_the_cardinality_the_registry_states`,
      `::should_rebuild_the_same_profile_from_the_same_declaration`, with the declarations in
      `docs/spec/hardening/performance_profiles.yaml` and the fixture under
      `docker/acceptance/fixtures/performance/` (#82, §32.1, §32.2).
- [ ] **P1 · Six metrics per benchmark, against a machine-readable baseline.** Time to first value,
      time to completion, sampled RSS, values per second, estimated bytes and cancellation latency
      are recorded for every benchmark, and a result is compared against the baseline for its named
      reference environment rather than against a number in a test —
      `xtask/tests/perf.rs::should_record_all_six_required_metrics_for_every_benchmark`,
      `::should_compare_a_benchmark_result_against_the_baseline_for_its_reference_environment`,
      `::should_fail_when_a_benchmark_reports_only_a_total_runtime` (#83, §32.3, §32.4,
      Appendix F.4).
- [ ] **P2 · The benchmarks are invocable and the environment is named.** `cargo xtask perf` runs
      them, writes the §32.3 records, and states which reference environment produced a figure;
      warm and cold measurements are distinguished, and the statistical rule of §37.4 decides a
      pass — `xtask/tests/perf.rs::should_run_the_declared_benchmarks_and_write_their_records`,
      `::should_name_the_reference_environment_on_every_recorded_figure`,
      `::should_distinguish_a_warm_measurement_from_a_cold_one` (#84, §37.1–§37.4).
- [ ] **P1 · Time to first result is measured, and a blank hang fails.** The §33.2 targets hold on
      the reference environment — cached `look`/`near` first result under 50 ms p95, Profile M
      spatial query under 150 ms p95, Profile M `map --live` first frame under 500 ms p95, Profile L
      initial progress or a deterministic cost refusal under 1.5 s — and a supported interactive
      command that produces neither result nor progress inside the hard budget fails the suite —
      `crates/ono-spatial-query/tests/profiles.rs::should_answer_or_refuse_within_the_interactive_budget_on_the_profile_l_fixture`,
      `xtask/tests/perf.rs::should_hold_every_time_to_first_result_target_of_the_reference_targets_table`,
      cases `195` and `197`, the watchdog case being the one that fails on silence (#85, §33.1–§33.3,
      §61.1, §61.3).
- [ ] **P1 · `map --live` produces a first frame and can be cancelled.** The reproduced hang —
      `map --live --json | take 3 | to json` returning nothing for 30 s at 0 % CPU — is gone: the
      initial projection is bounded, updates are incremental, backpressure holds and Ctrl-C releases
      the query task promptly —
      `crates/ono-cli/tests/watch_live.rs::should_answer_a_bounded_first_projection_before_any_update_arrives`,
      `::should_complete_a_live_map_pipeline_that_takes_the_first_three_frames`,
      `::should_release_the_query_task_promptly_when_a_live_map_is_cancelled`, case `196` (#22,
      §35.1–§35.5, §61.2, §61.5).
- [ ] **P1 · A full-screen map stays responsive while a projection is in flight.** Focus movement
      and Ctrl-C are answered at a real PTY while COMPUTE is being projected, inside the 16 ms frame
      budget §4.7.5 already holds for focus —
      `crates/ono-cli/tests/spatial_interactive.rs::should_answer_focus_movement_while_a_projection_is_still_running`,
      `::should_close_the_full_screen_map_promptly_while_a_projection_is_in_flight`, case `196`
      (#20, §35.4, §61.5).
- [ ] **P1 · A selector miss costs about what a hit costs.** A miss stops at a bounded candidate
      set instead of consulting the whole index and projecting all six domains, holding Profile M
      p95 under 250 ms and Profile L p95 under 1 s —
      `crates/ono-spatial-index/tests/index.rs::should_answer_a_selector_miss_from_a_bounded_candidate_set`,
      `crates/ono-spatial-query/tests/resolution.rs::should_hold_the_profile_m_and_profile_l_selector_miss_targets`,
      case `195` (#8, §36.1; the design choice between a persistent index and a bounded last step is
      recorded in an ADR before the code, per `docs/STATE.md`).
- [ ] **P1 · Completion stops at its hard budget.** A completion request that triggers expensive
      discovery returns a partial set marked incomplete at 50 ms and stops discovery at 150 ms,
      measured directly rather than through a proxy —
      `crates/ono-cli/tests/completion.rs::should_return_a_partial_completion_marked_incomplete_at_the_soft_budget`,
      `::should_stop_discovery_at_the_hard_budget_and_answer_what_it_has`,
      `xtask/tests/perf.rs::should_measure_the_completion_budget_directly_rather_than_through_a_proxy`,
      case `198` (#21, #86 in part, §36.2, §61.4, Appendix A).
- [ ] **P2 · Cost is estimated, and an expensive relation is requestable.** Every canonical query
      carries a machine-readable cost class, `follow owner` and its kind either pay for the
      expensive relation when asked or refuse with the cost they estimated, and an advertised
      expensive relation is genuinely obtainable —
      `crates/ono-spatial-query/tests/cost.rs::should_assign_a_declared_cost_class_to_every_canonical_query`,
      `::should_pay_for_an_expensive_relation_when_it_is_explicitly_requested`,
      `::should_refuse_with_the_estimated_cost_when_the_estimate_exceeds_the_interactive_budget`,
      `crates/ono-cli/tests/spatial_relationships.rs::should_follow_the_owner_relation_when_it_is_requested_explicitly`
      (#86, #25, §34.1–§34.3).
- [ ] **P2 · A local question builds no global graph.** Asking for one place's neighbourhood
      touches a bounded part of the index, and the work it does is asserted as an observable cost
      rather than by inspecting the call path —
      `crates/ono-spatial-query/tests/cost.rs::should_keep_the_work_of_a_neighborhood_question_within_its_declared_cost_class`,
      `::should_answer_a_local_neighborhood_question_without_projecting_every_domain` (#87, §34.4).

#### 4.8.9 Verification (H8 — §66.5, §38–§42, Appendix G)

§66.5's six bullets. §65.10 names the defect this phase removes: a skip that reaches the summary as
a pass. ADR-0428 already made every skip announce itself; this phase makes an unexpected one fail.

- [ ] **P2 · A test run has three visible outcomes.** PASS, FAIL and SKIP with a reason category
      from the §38.4 taxonomy, emitted through the canonical helper of Appendix G, with the raw
      early-return form rejected by the gate —
      `crates/ono-testkit/tests/harness.rs::should_name_the_test_the_reason_and_the_category_when_a_skip_is_announced`,
      `::should_offer_a_require_helper_that_records_an_unmet_prerequisite`,
      `xtask/tests/scan.rs::should_reject_a_test_that_announces_a_skip_with_its_own_print` (ADR-0428,
      extended to carry the category) (#88, §38.1, §38.4).
- [ ] **P2 · An unexpected skip fails the gate.** The expected skip set is checked in as
      `docs/spec/hardening/expected_test_skips.yaml`, the verifier compares observed skip IDs and
      categories against it, and a skip nobody declared turns the run red —
      `xtask/tests/scan.rs::should_fail_on_a_skip_the_expectation_does_not_declare`,
      `::should_fail_when_a_declared_skip_no_longer_happens`,
      `::should_report_this_repositorys_observed_skips_as_exactly_the_declared_set` (#89, §38.2,
      §38.3, §65.10).
- [ ] **P2 · The shared test helpers are canonical.** One helper per job, used by every suite that
      needs it, with the divergence that ADR-0427 already forbids asserted over the whole tree —
      `xtask/tests/scan.rs::should_report_two_test_helpers_that_do_the_same_job_under_different_names`,
      `::should_report_this_repository_as_using_the_canonical_helper_everywhere` (#90, §39.1–§39.4).
- [ ] **P1 · The fourteen acceptance families exist and run.** Every box of §4.8.13 is ticked, the
      cases execute the real `ono` binary (§40.1), remote cases use an isolated container network
      rather than the public internet (§40.2), and every case has a finite timeout that counts as a
      failure when it fires (§40.4) —
      `xtask/tests/hardening_evidence.rs::should_find_a_case_for_every_one_of_the_fourteen_acceptance_families`,
      `::should_find_a_finite_timeout_on_every_v041_case` (#91, §40).
- [ ] **P2 · Coverage-guided fuzzing is scheduled and the gate fuzzing stays fast.** The
      deterministic gate tier keeps running in `scripts/gate.sh`, a scheduled workflow runs the
      coverage-guided tier over the §35.6 targets, and the schedule is declared rather than assumed
      — `xtask/tests/supply_chain.rs::should_declare_a_scheduled_coverage_guided_fuzzing_job_for_every_declared_target`,
      `::should_keep_the_deterministic_fuzz_tier_inside_the_gate` (#92, §41.1–§41.3).
- [ ] **P2 · Corpora persist and a hang is a failure.** Fuzz corpora are stored and reloaded
      between runs, each input has a timeout, and an input that exceeds it is recorded as a hang
      rather than silently dropped — `fuzz/tests/corpus.rs::should_reload_the_persisted_corpus_for_every_target`,
      `::should_record_an_input_that_exceeds_its_timeout_as_a_hang` (#93, §41.4, §41.5).
- [ ] **P2 · Miri and the sanitizers run on the unsafe boundary.** The targeted jobs exist, cover
      the `unsafe` code §42.1 names, and are green for the release commit —
      `xtask/tests/supply_chain.rs::should_declare_a_miri_job_covering_every_unsafe_boundary_module`,
      `::should_declare_an_address_and_undefined_behaviour_sanitizer_job_for_the_release_commit`,
      with the result recorded in the release evidence of §4.8.11 (#94, §42.1–§42.4).
- [ ] **P2 · The resize assertion needs a resize.** `should_preserve_the_current_place_when_the_terminal_is_resized_with_a_place_open`
      is satisfied only by output that the resize itself produced, so an earlier repaint cannot
      close it — `crates/ono-cli/tests/spatial_interactive.rs::should_preserve_the_current_place_when_the_terminal_is_resized_with_a_place_open`
      rewritten to wait on a resize-specific observation, and
      `xtask/tests/scan.rs::should_report_a_pty_assertion_that_an_earlier_repaint_can_satisfy`
      (#6, §65.10 — a test that can pass without exercising its subject is the skip-as-pass defect
      in a different costume).
- [ ] **P2 · The two known flaky tests are deterministic.**
      `should_report_a_failing_streamed_child_after_its_records` (#7) and
      `ono-process::should_run_a_text_script_without_a_shebang_through_the_shell` (#27) are made
      deterministic or their non-determinism is fixed at its source, and each is proven by the
      test's own repeated execution —
      `crates/ono-cli/tests/adapters.rs::should_report_a_failing_streamed_child_after_its_records`,
      `crates/ono-process/tests/external_command.rs::should_run_a_text_script_without_a_shebang_through_the_shell`,
      both named in the expected-skip and flake declarations of #89 so a recurrence is visible
      (#7, #27, §38.3).

#### 4.8.10 Maintainability (H9 — §66.6, §29–§31, Appendix I)

§66.6's four bullets. §65.12 governs the whole subsubsection: a refactor and a semantic redesign
never travel together, so every box here is closed with the test suite unchanged (AGENTS.md §11).

- [ ] **P2 · The parser is navigable by responsibility.** The seven responsibilities of §29.2 —
      state and token access, statements, expressions and precedence, pipelines and commands, blocks
      and control constructs, recovery and incomplete input, diagnostic construction — are separately
      navigable modules, and the recursive-descent strategy, recovery behaviour, depth guard and AST
      contracts are unchanged — `xtask/tests/architecture.rs::should_find_every_parser_responsibility_in_its_own_module`,
      with `crates/ono-parser/tests/` green and unedited across the change and the fuzz seeds of
      Appendix I.1 replayed (#95, §29.1–§29.4).
- [ ] **P2 · The evaluator is navigable by responsibility.** Statement, expression, pipeline, block,
      function, control, native execution and materialization are separate modules, `materialize`
      owns the budget-aware collection helpers so no caller recreates them, `Flow` stays an explicit
      representation, and no domain logic moved up into `ono-cli` to reduce a file's size —
      `xtask/tests/architecture.rs::should_find_every_evaluator_responsibility_in_its_own_module`,
      `::should_find_no_domain_logic_moved_up_into_the_composition_root`, with the control-flow,
      cancellation and job suites of Appendix I.2 green and unedited (#96, §30.1–§30.4).
- [ ] **P2 · Session state has owners.** The eight state groups of §31.2 exist, result-history
      budget enforcement lives in the history group rather than at evaluator call sites, and none of
      the five behaviours Appendix I.3 protects changed — config precedence, environment mutation,
      job reaping, navigation trail semantics, result-history identifiers —
      `xtask/tests/architecture.rs::should_find_every_session_state_group_the_specification_names`,
      `crates/ono-cli/tests/session_lifetime.rs` green and unedited, and
      `crates/ono-history/tests/history.rs::should_enforce_the_history_budget_inside_the_history_state_group`
      (#97, §31.1–§31.4).
- [ ] **P2 · No cross-crate dependency inversion was introduced.** The crate graph after the
      refactor holds the boundaries §56 states, and a new edge that inverts one fails the gate —
      `xtask/tests/architecture.rs::should_hold_the_crate_graph_against_the_declared_layering`,
      `::should_report_a_new_dependency_edge_that_inverts_a_declared_boundary` (§66.6's fourth
      bullet, closing #95, #96 and #97 together).

#### 4.8.11 Supply chain and release (H10, H11 — §66.7, §43–§49, Appendices H and J)

§66.7's nine bullets, which together answer one question: what exactly was trusted to produce
these bytes (Appendix H). §65.11 names the failure this phase removes — a release built from
mutable inputs.

- [x] **P2 · Every required Action is pinned by commit SHA.** No third-party `uses:` reference in a
      required workflow carries a tag or a branch, repository-local actions are the stated
      exception, and the scanner runs in the gate rather than in a review —
      `xtask/tests/supply_chain.rs::should_reject_an_action_referenced_by_a_floating_tag`,
      `::should_reject_an_action_referenced_by_a_branch_name`,
      `::should_reject_a_forty_character_reference_that_is_not_a_commit_sha`,
      `::should_reject_an_unpinned_action_inside_a_composite_action`,
      `::should_accept_an_action_that_lives_in_this_repository`,
      `::should_report_this_repository_as_pinning_every_action_it_uses` (#98, §43.1, §62.1,
      ADR-0433).
- [x] **P2 · Every release-critical image is pinned by digest.** A container reference without a
      digest in a release-critical script or workflow fails policy, an image this repository builds
      itself is the stated exception, and a digest hidden behind a shell variable is found —
      `xtask/tests/supply_chain.rs::should_reject_a_build_image_pulled_by_tag_alone`,
      `::should_reject_a_package_validation_image_named_by_a_shell_variable_without_a_digest`,
      `::should_reject_a_workflow_job_running_in_a_container_image_without_a_digest`,
      `::should_accept_an_image_this_repository_builds_itself`,
      `::should_report_this_repository_as_pinning_every_release_critical_image` (#99, §44.1,
      §62.2, ADR-0433).
- [x] **P2 · Workflows hold least privilege, and an untrusted pull request is isolated.** Each
      workflow declares the narrowest `permissions` it needs, a pull request from a fork reaches no
      secret and no write token, and a publishing workflow carries a concurrency guard —
      `xtask/tests/supply_chain.rs::should_reject_a_workflow_that_declares_no_permissions_at_all`,
      `::should_reject_a_workflow_that_grants_write_access_to_every_job`,
      `::should_reject_a_workflow_triggered_by_pull_request_target`,
      `::should_reject_a_secret_reachable_from_an_untrusted_pull_request`,
      `::should_reject_a_publishing_job_a_pull_request_can_reach`,
      `::should_reject_a_publishing_workflow_without_a_concurrency_guard`,
      `::should_report_this_repository_as_granting_least_privilege_in_every_workflow` (#100,
      §43.3–§43.5, ADR-0433).
- [x] **P2 · The dependency policy is enforced and provably fails.** Advisory, license and source
      policy run in the gate, a git dependency and a new cryptographic dependency each need the
      recorded justification §45.3 and §45.4 require, and a controlled fixture proves the check
      fails on a denied condition rather than being assumed to —
      `xtask/tests/supply_chain.rs::should_fail_the_dependency_policy_on_a_denied_advisory_fixture`,
      `::should_fail_the_dependency_policy_on_a_denied_license_fixture`,
      `::should_fail_the_dependency_policy_on_an_unjustified_git_dependency`,
      `::should_reject_an_ignored_advisory_whose_removal_deadline_has_passed`,
      `::should_report_this_repository_as_running_its_dependency_policy_in_the_gate`,
      `::should_report_this_repository_as_justifying_every_git_and_cryptographic_dependency`
      (#101, §45.1–§45.4, §62.3, ADR-0449 — the three named tests live in `supply_chain.rs`
      beside the other supply-chain rules, not in `contracts.rs`).
- [x] **P2 · Tools and toolchain are exact, and the fetch is reproducible.** The Rust toolchain
      stays pinned, packaging tool versions are exact rather than floating, `Cargo.lock` is
      committed and release builds are `--locked`, and dependency fetching is deterministic —
      `xtask/tests/supply_chain.rs::should_find_an_exact_version_for_every_release_tool`,
      `::should_build_the_release_with_a_locked_dependency_graph`,
      `::should_reject_a_fallback_that_builds_again_without_the_lock`,
      `::should_reject_a_rust_toolchain_that_follows_a_channel_instead_of_a_version`,
      `xtask/tests/packaging.rs::should_refuse_a_release_build_whose_lockfile_would_change` (#102,
      §44.2–§44.4, ADR-0450).
- [x] **P2 · The build inputs are written down.** The release workflow emits the Appendix H
      manifest — source commit and tag, toolchain, `Cargo.lock` hash, build and package-test
      container digests, Action SHAs, packaging tool versions, `SOURCE_DATE_EPOCH`, workflow run
      identity — to `dist/build-inputs.json`, read from the same files the pin scanners read, and
      the manifest is an input to provenance rather than a summary written afterwards
      — `xtask/tests/provenance.rs::should_emit_a_build_input_manifest_carrying_every_field_appendix_h_requires`,
      `::should_bind_the_build_input_manifest_to_the_release_it_describes` (#103, §43.2, §57 H10,
      Appendix H, ADR-0451).
- [ ] **P2 · The determinism inputs are fixed.** `SOURCE_DATE_EPOCH`, locale, timezone, file
      ordering, ownership and mode are set by the workflow rather than inherited from the runner,
      and a build that omits one is refused —
      `xtask/tests/packaging.rs::should_set_every_determinism_input_before_a_release_build`,
      `::should_normalize_file_ownership_and_mode_in_every_produced_package`,
      `::should_refuse_a_release_build_that_leaves_a_determinism_input_unset` (#104, §46.2–§46.4).
- [ ] **P2 · Two clean builds produce identical packages.** Every published artifact is built twice
      in fresh environments from one commit and compared, a mismatch fails the release check with a
      diagnostic naming the differing member, and each supported architecture satisfies this on its
      own — `xtask/tests/packaging.rs::should_produce_identical_hashes_for_two_clean_builds_of_one_commit`,
      `::should_name_the_differing_archive_member_when_a_seeded_difference_is_introduced`,
      `::should_require_reproducibility_of_every_supported_architecture_separately`, run by
      `scripts/release-check.sh` (#105, §46.1, §46.5, §46.6, §62.4).
- [ ] **P2 · `SHA256SUMS` covers every downloadable artifact, deterministically ordered.** The
      manifest lists every published executable and package, its ordering is stable across runs, and
      an artifact missing from it fails the release check —
      `xtask/tests/provenance.rs::should_list_every_downloadable_artifact_in_the_checksum_manifest`,
      `::should_order_the_checksum_manifest_deterministically`,
      `::should_fail_the_release_check_when_an_artifact_is_absent_from_the_manifest` (#106, §47.1,
      §47.2).
- [ ] **P2 · The manifest is signed and the signature verifies.** A verifiable signature is
      published beside `SHA256SUMS`, verification succeeds against the published identity, a
      tampered manifest fails verification, and the signing model is keyless or else an ADR defines
      custody, rotation, revocation and offline verification and is named in this box —
      `xtask/tests/provenance.rs::should_verify_the_published_signature_over_the_checksum_manifest`,
      `::should_fail_verification_when_the_checksum_manifest_is_altered` (#107, §47.3).
- [ ] **P2 · Provenance binds seven fields to every artifact digest.** Repository, source commit,
      release tag, workflow identity, builder and toolchain version, artifact digest and build
      timestamp are bound by provenance the trusted workflow produces, and the release check
      verifies each digest appears in both the checksum manifest and the provenance before
      publication — `xtask/tests/provenance.rs::should_bind_all_seven_required_fields_to_every_artifact_digest`,
      `::should_verify_every_artifact_digest_against_the_checksum_manifest_and_the_provenance_before_publication`,
      case `199` (#108, §47.4, §62.5).
- [ ] **P2 · Package validation covers the nine new checks.** Version equality, the installed path,
      ownership and mode, absent private build paths, metadata matching the filename, uninstall
      leaving user configuration, reinstall, the login-shell smoke behaviour and the checksum match
      all run in `scripts/package-check.sh`, on the oldest supported baseline as well as a current
      distribution — `xtask/tests/packaging.rs::should_run_every_new_package_check_the_specification_lists`,
      `::should_run_package_validation_on_the_oldest_supported_baseline_as_well_as_a_current_one`
      (#109, §48.1–§48.3).
- [ ] **P2 · The tested bytes are the published bytes.** The artifact package validation installed
      hashes identically to the asset later uploaded, the workflow builds once and promotes after
      proof, a public release needs no undocumented local step, final publication reruns the
      complete check on the final tag, and a failed step leaves no partially populated release —
      `xtask/tests/packaging.rs::should_publish_the_same_bytes_package_validation_installed`,
      `xtask/tests/supply_chain.rs::should_promote_an_already_tested_artifact_rather_than_rebuilding_it`,
      `::should_publish_the_release_only_after_the_asset_inventory_verifies`, case `199` (#110,
      §48.4, §49.1–§49.4).

#### 4.8.12 Documentation (H12 — §66.8, §19, §50, §51, §54, §63)

§66.8's five bullets. A document is closed here by the gate check that fails when the document and
the tree disagree, so no box in this subsubsection is ticked by someone having read something.

- [ ] **P1 · A refusal names the boundary that decided it.** The four examples of §54.1 — the
      authenticated client that is not authorized for an action, the plugin whose mandatory control
      could not be installed, the finite-input requirement against an unbounded upstream, the
      history budget that kept part of a result — appear in ordinary structured errors, with no
      `RUST_LOG=debug` needed to see them —
      `crates/ono-cli/tests/resource_limits.rs::should_name_the_deciding_boundary_in_every_hardening_refusal`,
      `crates/ono-protocol/tests/authorization.rs::should_say_which_policy_refused_an_authenticated_client`,
      `crates/ono-kuang-supervisor/tests/confinement.rs::should_name_the_control_that_could_not_be_installed_in_the_structured_error`,
      `xtask/tests/contracts.rs::should_find_a_deciding_boundary_on_every_declared_hardening_error`,
      case `200` (#119, §54.1, §54.2).
- [ ] **P2 · The security terminology contract holds across every document.** README, Wiki, `help`
      and the generated reference use the §19.1 canonical terms, a document that overstates a
      boundary fails the gate, and the generated pages carry the terms rather than a hand-written
      paraphrase — `xtask/tests/terminology.rs::should_report_a_document_that_overstates_a_security_boundary`,
      `::should_report_this_repositorys_documents_as_using_the_canonical_terms`,
      `xtask/tests/reference.rs::should_render_the_security_terms_into_the_generated_reference`
      (#112, §19.1, §19.2, §51.1).
- [ ] **P2 · Verification instructions exist and work.** The install documentation shows a short
      copyable sequence that verifies `SHA256SUMS` and its signature before installation, needs no
      proprietary service, and is executed rather than merely printed —
      `xtask/tests/provenance.rs::should_execute_the_documented_verification_sequence_against_a_release_fixture`,
      `::should_fail_the_documented_verification_sequence_on_a_tampered_artifact`, case `199`
      (#115, §47.5, §67.7).
- [ ] **P2 · The migration path is written down.** §63's five migrations — existing users, existing
      direct listening-agent users, an existing host identity, existing KUANG plugins, existing test
      infrastructure — are documented with the commands an operator runs, and the commands are
      checked against the command registry so a renamed flag turns the gate red —
      `xtask/tests/reference.rs::should_resolve_every_command_the_migration_guide_prints_against_the_registry`,
      `crates/ono-cli/tests/client_keys.rs::should_accept_the_migration_sequence_the_documentation_prints`
      (#116, §63.1–§63.5).
- [ ] **P3 · The repository metrics are computed, not typed.** Crate, test, acceptance-case, ADR and
      command-contract counts come from `cargo xtask metrics`, the gate fails when README disagrees
      with it, and an executed test is distinguished from a skip so no count claims proof it does
      not have — `xtask/tests/metrics.rs::should_compute_every_metric_the_readme_states`,
      `::should_fail_when_the_readme_disagrees_with_the_computed_metrics`,
      `::should_count_executed_tests_apart_from_skipped_ones` (#111, §50.1–§50.4).
- [ ] **P3 · The remote documentation separates the six trust concepts.** Transport encryption,
      transport authentication, host pinning, client authorization, self-reported identity and
      runtime user are described as six distinct things, and the page is held against the §6.1
      boundary inventory — `xtask/tests/terminology.rs::should_find_all_six_remote_trust_concepts_described_separately`,
      `xtask/tests/reference.rs::should_hold_the_remote_documentation_against_the_boundary_inventory`
      (#113, §51.3).
- [ ] **P3 · `SECURITY.md` states the model and the reporting path.** Supported versions, the
      reporting channel, the response expectation and the boundaries §5 protects are stated, and the
      file is held against the boundary inventory so a new boundary cannot be added without
      appearing there — `xtask/tests/terminology.rs::should_find_every_protected_asset_of_the_threat_model_in_the_security_document`,
      `::should_find_a_reporting_channel_and_a_response_expectation_in_the_security_document`
      (#114, §51.4, §5.1).
- [ ] **P2 · The status documents agree.** `docs/STATE.md`, this checklist and the release notes
      state the same thing about what is done: *In progress* is empty, the workspace holds no
      `#[ignore]`d test, every *Deferred* entry names an ADR saying why it does not block the
      release, and the release notes name the same tranche state — the bar §4.5, §4.6.5 and §4.7.2
      already set, checked by `cargo xtask state-check` on every `scripts/release-check.sh` run
      (ADR-0402, `xtask/tests/scan.rs`), extended by
      `xtask/tests/scan.rs::should_report_release_notes_that_disagree_with_the_checklist` (§66.8's
      fifth bullet).

#### 4.8.13 The fourteen acceptance families (§40.3) and the scenarios they carry

One box per family of §40.3, in the order the specification lists them. A box is ticked when the
case exists under `docker/acceptance/cases/`, runs the real `ono` binary (§40.1) inside the
timeout every case has (§40.4), and is green in `scripts/acceptance.sh`. The remote families use
an isolated container network created for the case (§40.2). Every scenario of §59, §60 and §61 is
carried by one of these cases or by one of the seven the tranche adds beside them — `181`
(§59.6), `188` (§12), `192` (§22.4, §54.3), `194` (§60.2, §60.3), `197` (§61.3), `198` (§61.4)
and `200` (§54.1) — and the six scenarios of §62 are carried by the gate checks and the release
check §4.8.11 names. §59.9 is asserted inside each of the security cases: every trust failure is
non-interactive and deterministic with no terminal attached (#91).

- [ ] **Direct mutual TLS authentication** — `180-remote-mutual-authentication`: both ends prove
      possession of a persistent key, the accepted client's fingerprint is available at accept, a
      wrong ALPN and an absent client certificate are refused, and a changed host key is refused
      with E0603 (#35, #36, #38, #18).
- [ ] **Unknown client refusal** — `182-remote-unknown-client-refused`: a client with a valid but
      unauthorized certificate is refused before provider negotiation, and the session learns no
      process, schema or capability inventory beyond the rejection (§59.1, #43, #45).
- [ ] **Authorization-constrained capability negotiation** —
      `183-remote-policy-filtered-negotiation`: the offer an observe-only client receives holds the
      read and observe capabilities and none of the actions the provider advertises (§10.1, #45,
      #47).
- [ ] **Unauthorized action refusal** — `184-remote-unauthorized-action-refused`: an observe-only
      client executes representative read and observe operations and is refused `service.restart`
      with `remote.capability_denied`, at dispatch as well as in the offer (§59.2, #46, #48).
- [ ] **Authorized exact action success** — `185-remote-exact-action-grant`: after the operator
      grants `service.restart`, that action succeeds under the provider's own rules and
      `process.signal` stays refused (§59.3, #44).
- [ ] **Changed client key refusal** — `186-remote-changed-client-key`: a new key at the same host
      is refused until its fingerprint is explicitly added, and `get link` shows it as
      authenticated and unauthorized (§59.4, #42, #50).
- [ ] **Malformed authorization store fails closed at startup** —
      `187-remote-corrupt-authorization-store`: one malformed line stops the agent
      deterministically, an empty store refuses to listen, and neither is treated as zero
      restrictions (§59.5, #40, #55).
- [ ] **KUANG mandatory confinement setup failure** — `189-kuang-confinement-fail-closed`: with
      `PR_SET_NO_NEW_PRIVS` and a mandatory `setrlimit` made to fail through the injectable platform
      layer, the spawn fails, the plugin's startup marker stays absent, and the confinement report
      names the control (§59.7, §59.8, #60, #61, #62).
- [ ] **`each` streams an unbounded source** — `193-each-streams-an-unbounded-source`: a source
      that emits `1`, waits and is marked unbounded lets `source | each { $it } | take 1` answer `1`
      and complete before the source closes (§60.1, #75, #76).
- [ ] **Materialization item and byte limits refuse** — `190-materialization-limits`: 100 001 small
      values hit the item limit, and a handful of large values hit the 128 MiB byte limit while the
      item count stays far below its own, both with `resource.materialization_limit` semantics
      (§60.4, §60.5, #67, #73).
- [ ] **Result-history truncation is visible** — `191-result-history-truncation`: a pipeline that
      exceeds the history limits emits its complete result to the user while the retained copy is
      truncated and says so (§60.6, §67.6, #72).
- [ ] **Profile M spatial first result** — `195-spatial-first-result-profile-m`: canonical `look`,
      `near` and selector operations hold their Profile M p95 targets on the reference environment,
      including the selector miss (§61.1, #82, #83, #85, #8).
- [ ] **Live map cancellation under load** — `196-live-map-cancellation`: Profile M `map --live`
      renders an initial frame within 500 ms p95, Profile L answers a frame or a truthful
      progress/cost response within 1.5 s, and cancelling the heaviest Profile L view releases the
      query task promptly and stops result growth (§61.2, §61.5, #22, #20).
- [ ] **Package signature, checksum and provenance** — `199-release-provenance`: the release
      fixture produces `SHA256SUMS`, a verifying signature and provenance binding the seven fields
      to each artifact digest; verification succeeds on the published bytes, fails on a tampered
      one, and the installed package hashes identically to the published asset (§62.5, §62.6,
      #106, #107, #108, #110, #115).

#### 4.8.14 Zero unresolved P0/P1, and what may be excluded (§66.9)

§66.9 is the binding release criterion of this tranche, and it governs every box above.

- [ ] **No known unresolved P0 or P1 issue in v0.4.1 scope remains at final release.** Every issue
      of the tranche labelled `p0` or `p1` is closed, and closure means the commit that closed it
      said `Closes #NN` and the box that names its proof is ticked here —
      `xtask/tests/hardening_evidence.rs::should_find_every_p0_and_p1_box_of_the_v041_checklist_ticked`
      run by `scripts/release-check.sh`, together with `cargo xtask state-check`, which already
      refuses a release-ready verdict while *In progress* holds a claim or a *Deferred* entry names
      no ADR (ADR-0402). This box is ticked last of all, after §4.8.1's first box proves that every
      proof named here resolves.
- [ ] **Every P2 or P3 exclusion is an ADR written before release-candidate freeze.** An excluded
      item is recorded in an ADR that states what is excluded, why, and what the user-visible
      consequence is; the ADR predates the freeze; and it is named in the box it leaves open, so an
      exclusion is readable from the checklist rather than from the tracker —
      `xtask/tests/hardening_evidence.rs::should_find_a_dated_adr_for_every_box_the_checklist_leaves_open`,
      `::should_refuse_an_exclusion_adr_dated_after_the_release_candidate_freeze` (§3.4, §66.9).
- [ ] **No exclusion waives a §66 criterion.** An ADR may remove a P2 or P3 item from the tranche's
      scope, and it may not remove a bullet of §66.1–§66.8: every criterion of §66 has at least one
      ticked box in §4.8.1–§4.8.13 naming its proof —
      `xtask/tests/hardening_evidence.rs::should_find_a_box_for_every_bullet_of_the_release_definition`,
      which reads §66 from the specification and this subsection from this file, so a criterion that
      lost its box fails the gate rather than passing unnoticed (§66.9's second paragraph).

## 5. Stopping rule

An agent stops when `scripts/release-check.sh` prints `release-check: the shell is
release-ready`. Any other outcome means there is a next task in `docs/STATE.md`.

Running out of easy work is not a stopping condition. Neither is a green quality gate, a passing
acceptance suite on its own, a completed phase, or a tidy-looking repository. If a box in
section 4 is unticked, the work is unfinished, and the next increment starts.

**Every subsection of section 4 counts, including the tranches.** The checklist grew with the
specification: sections 4.1–4.5 are the v0.2 shell, section 4.6 is the v0.3 External Command
Adaptation Layer, section 4.7 is the v0.4 Spatial Systems Interface, and section 4.8 is the v0.4.1
Hardening, Trust & Release Integrity tranche — open in full, because that tranche has just
started. Section 4.9 is reserved for v0.5. A tranche whose subsection still holds an unticked box
is an unfinished product, however green the gate and the acceptance suite are on their own, and
the run continues into it.

`scripts/release-check.sh` reads the checklist generically — it greps this file for lines
beginning `- [ ]` and fails on the first one — so a new subsection is seen the moment it is
written, and no subsection can be excluded from the stopping rule by being added late. The
counterpart of that generosity is that a box must never be written in a form no proof can close:
section 3 governs every subsection alike.
