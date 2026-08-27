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
| Release gate | `scripts/release-check.sh` | are the first two green **and** is the checklist in section 4 fully ticked? |

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
      `docs/spec/providers/*.yaml` and `ono-cli/tests/providers.rs` pinning what each provider
      advertises.
- [x] **D — Consistency and discoverability.** Command, verb and schema registries exist under
      `docs/spec/`; `help`, completion, `type`, `inspect` and `explain` are driven by them;
      docs and provider conformance tests are generated from them.
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
      (ADR-0036, ADR-0037). Deferred: agentless mode, trust-store UX for a future authenticated
      transport (the board carries both).
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
of processes and paths, slow NSS, high-latency links, huge stdout, unbounded streams:

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
      procfs/netlink decoders — seeded and deterministic per AGENTS.md §11:
      `ono-parser/tests/robustness.rs` (corpus + hostile walls), `ono-value/tests/codec_fuzzing.rs`,
      `ono-protocol/tests/{fuzz_protocol,framing}.rs` (length checked before allocation),
      the kuang conformance garbage/oversize/misframe cases, and
      `ono-provider-netlink/tests/malformed_messages.rs`.
- [x] The threat model of spec section 49 has a test for each stated risk — the T1–T15 table of
      ADR-0015, each row now naming a passing test: T1/T9 `ono-render/tests/presentation.rs` and
      case `048`; T2 `034`/`048`; T3 the §31.74 conformance suite; T4
      `ono-editor/tests/completion.rs`; T5/T6 `ono-remote/tests/trust.rs` (E0603, E0702); T7 the
      protocol and codec fuzz suites plus bounded frames; T8 `ono-history/tests/history.rs`
      (default and configured redaction); T10/T11 `032`; T12/T13 the confirm-before-signal
      tests; T14 `ono-provider-linux/tests/file.rs`; T15 `ono-cli/tests/signals.rs`.

### 4.5 Delivery

- [x] `ono` installs and runs as a login shell in the container as an unprivileged user —
      `003-login-shell` and every interactive case, which run as the unprivileged `case` user.
- [x] Startup loads no plugin eagerly and queries no network-backed configuration —
      `027-startup-is-quiet`, in a container with networking disabled.
- [x] Generated documentation is reproducible from the registries and committed docs match it —
      `xtask/tests/reference.rs` regenerates every page and requires the committed files to be
      identical, and `spec-check` runs the same comparison in the gate (ADR-0018).
- [x] `docs/STATE.md` has an empty *In progress* section and no unexplained *Deferred* entries —
      In progress is empty, Deferred is empty, and the Next up list is the deliberate
      post-release backlog with an exit test named per item.
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
- [ ] **Live conformance in the container.** The acceptance image installs every Tier A/B/C
      tool, and each adapter has at least one live case against the real executable
      (v0.3 §1.48); adapters for tools absent on a host degrade to raw with a visible reason.
- [ ] **Overhead is measured.** The adapter path adds a bounded, measured cost over the raw
      path — negotiation, rewrite and decode — reported by an acceptance case inside the §34
      budgets (v0.3 §1.50).
- [ ] **Limitations are documented.** Every first-party adapter's reference page states its
      unsupported invocations and known limits, and `README.md` presents the adapter layer to a
      new user with examples that run. Exit test: doc examples parse and run under `xtask`.
- [ ] **Delivery.** `docs/STATE.md` has an empty *In progress*, no `#[ignore]`d tests exist
      without a *Deferred* entry, the acceptance suite and CI are green on `implementation`,
      and this subsection has no unticked box.

## 5. Stopping rule

An agent stops when `scripts/release-check.sh` prints `release-check: the shell is
release-ready`. Any other outcome means there is a next task in `docs/STATE.md`.

Running out of easy work is not a stopping condition. Neither is a green quality gate, a passing
acceptance suite on its own, a completed phase, or a tidy-looking repository. If a box in
section 4 is unticked, the work is unfinished, and the next increment starts.
