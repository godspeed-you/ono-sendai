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
      and this subsection has no unticked box. — `scripts/release-check.sh`; `docs/STATE.md` *In progress* is empty, no
      `#[ignore]` exists in the tree, CI runs the gate and the acceptance suite on every push.

### 4.7 The v0.4 tranche — Spatial Systems Interface

`docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md` layers a navigable projection of
the system onto the released v0.3 shell (ADR-0124 … ADR-0131). This subsection is its definition
of done: the release criteria of v0.4 §52 — §52.1 functional, §52.2 quality, §52.3 the product
experience — together with the ten acceptance scenarios of §44, the twenty invariants of §2 and
the performance budgets of §34, in boxes a script can check.

The executable requirements exist already and are red: the nine RED suites
`crates/ono-cli/tests/spatial_*_missing.rs` (175 `#[ignore]`d tests) and the ten
`docker/acceptance/cases/09x-spatial-*.case.v04` scenarios (139 assertions, kept out of the
referee by their suffix). **A box below is ticked only when the tests it names run un-ignored
and green in the gate, or its case runs in the container** — never on judgement (§3), never by
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
`scripts/release-check.sh` prints the release line again.

#### 4.7.1 Functional release criteria (v0.4 §52.1)

- [ ] **Root `SYSTEM` and canonical domains exist** (§7, §4). `home` reports the root place,
      `look` at the root lists exactly the six canonical domains of §4 with a permission state
      on each, and the root's identity is the same across sessions —
      `spatial_topology_missing.rs::should_report_the_system_root_as_the_current_place_when_home_runs`,
      `::should_list_exactly_the_six_canonical_domains_when_looking_at_the_system_root`,
      `::should_carry_a_permission_state_on_every_domain_so_an_unavailable_one_stays_visible`,
      `::should_keep_the_same_spatial_id_for_the_root_across_separate_sessions`,
      `::should_enter_every_canonical_domain_when_named_at_the_root`,
      `spatial_contracts_missing.rs::should_start_every_session_at_the_local_system_root` and
      `::should_serve_exactly_the_canonical_spaces_the_registry_declares` (the registry and the
      shell cannot drift apart), case `090`.
- [ ] **Users can discover objects without prior names** (§9, §2.1). A process, a listening
      socket and a running service are each reached from a predicate over visible metadata, with
      the name never typed —
      `spatial_topology_missing.rs::should_reach_a_process_it_never_names_when_only_a_predicate_over_visible_metadata_is_known`,
      `::should_discover_a_listening_socket_by_its_port_and_follow_it_to_its_owning_process`,
      `::should_reach_a_running_service_by_its_visible_state_when_a_service_manager_answers`,
      `::should_answer_look_near_and_map_without_an_object_name_when_at_the_root`,
      `spatial_navigation_missing.rs::should_stream_places_with_scope_and_provenance_when_find_searches_with_a_predicate`,
      cases `090` and `091`, whose house rule is that no case types the name of the object it
      discovers.
- [ ] **All core spatial commands are implemented** (§6). `look`, `near`, `enter`, `follow`,
      `jump`, `back`, `up`, `home`, `trail`, `find place`, `map`, `pin`/`unpin` each answer with
      the contract §6 gives them —
      `spatial_navigation_missing.rs` (one test per verb: `::should_describe_the_current_place_as_a_structured_view_when_look_runs_without_a_tty`,
      `::should_stream_neighbors_that_compose_with_the_pipeline_when_near_runs_in_a_script`,
      `::should_move_into_the_hierarchical_child_when_entering_a_canonical_domain_and_its_group`,
      `::should_traverse_the_relationship_edge_when_following_the_parent_relation`,
      `::should_move_across_scopes_and_record_both_ends_when_jumping_to_a_resolved_place`,
      `::should_return_to_the_process_when_back_follows_the_navigation_history`,
      `::should_move_to_the_network_hierarchy_parent_when_up_follows_the_canonical_hierarchy`,
      `::should_return_to_the_system_root_when_home_runs_after_deep_navigation`,
      `::should_record_every_movement_with_its_kind_and_relation_when_the_trail_is_read_as_json`,
      `::should_answer_a_bounded_graph_when_map_json_runs_without_a_tty`),
      `spatial_contracts_missing.rs::should_keep_the_trail_session_local_while_a_pin_survives_the_session`,
      and `help spatial` complete for each with a generated reference page
      (`xtask/tests/reference.rs` extended to `docs/reference/spatial/`, `spec-check` in the gate).
- [ ] **Hierarchy and graph traversal are distinct** (§11, §6.6). `up` walks the canonical
      hierarchy and `back` the trail; `follow` refuses a canonical child that is not a
      relationship edge; every object keeps all its relationship parents while naming one
      canonical parent —
      `spatial_relationships_missing.rs::should_refuse_to_follow_a_canonical_child_that_is_not_a_relationship_edge`,
      `::should_leave_the_relationship_chain_with_up_after_following_a_socket_edge`,
      `spatial_identity_missing.rs::should_keep_every_relationship_parent_while_naming_one_canonical_parent`,
      `::should_move_to_the_declared_canonical_parent_deterministically_when_going_up`,
      `spatial_navigation_missing.rs::should_move_to_the_network_hierarchy_parent_when_up_follows_the_canonical_hierarchy`,
      case `095`.
- [ ] **Typed pipeline and spatial selection interoperate** (§28). `look --json` and `near`
      read back into the v0.2 pipeline, a pipeline result is entered as a place, and a spatial
      result composes with `where`/`take`/`count` —
      `spatial_navigation_missing.rs::should_read_back_into_the_pipeline_when_look_json_is_parsed_by_from_json`,
      `::should_move_into_the_selected_object_when_a_pipeline_result_is_entered`,
      `::should_compose_with_the_v02_pipeline_when_a_find_result_is_filtered_and_counted`,
      `spatial_topology_missing.rs::should_stream_neighbors_as_pipeline_objects_when_near_runs_at_the_root`,
      case `091`.
- [ ] **Storage paths integrate with cwd according to this spec** (§15, §30). Entering a
      directory moves cwd and place together, entering a process or a socket moves neither, `cd`
      moves the place only under `storage-only`, `PWD` never carries a non-directory place, and
      a mount boundary shows its source — the whole of
      `crates/ono-cli/tests/spatial_storage_missing.rs` (12 tests) and case `092`.
- [ ] **Remote host roots can be entered/jumped when links exist** (§19). A linked host is a
      place with a root distinct from the local one, `jump` announces the boundary, the trail
      records the host crossing, and a hostname that is not a link is refused —
      `crates/ono-cli/tests/spatial_remote_missing.rs` (13 tests, notably
      `::should_give_a_linked_host_a_root_place_distinct_from_the_local_root`,
      `::should_announce_the_boundary_in_plain_text_when_jumping_to_a_linked_host`,
      `::should_refuse_to_jump_to_a_hostname_that_is_not_a_known_link`), case `094` (§19a–g).
- [ ] **Map text rendering works without a full-screen TUI** (§23.2, §29.1). `map` renders as
      text into a pipe, `map --json` answers the §22 document off a terminal, and a narrow
      terminal collapses the layout without changing the semantics —
      `spatial_map_missing.rs::should_render_a_text_map_when_stdout_is_a_pipe_and_no_full_screen_view_is_possible`,
      `::should_fit_the_text_map_into_the_terminal_when_the_terminal_is_narrow`,
      `::should_return_a_spatial_map_document_when_map_json_runs_without_a_tty`,
      `spatial_navigation_missing.rs::should_answer_a_bounded_graph_when_map_json_runs_without_a_tty`,
      case `090` (the text map assertions).
- [ ] **Full-screen map works on supported interactive terminals** (§23.3, §23.4). At a real
      PTY the view opens, focus moves without changing the place, Enter changes it, back
      returns, and closing restores the shell screen —
      `spatial_interactive_missing.rs::should_restore_the_shell_screen_when_the_full_screen_map_closes`,
      `::should_change_the_place_only_on_enter_when_focus_moves_inside_the_map`,
      `::should_return_to_the_previous_place_when_back_is_used_at_the_prompt_and_in_the_map`,
      case `099`.
- [ ] **The live map reflects real changes** (§25). An edge appears when a connection opens and
      is removed when it closes, a live view emits nothing while nothing happens, and no change
      section is invented where no event source exists —
      `spatial_relationships_missing.rs::should_show_the_connection_edge_appear_and_vanish_when_the_connection_opens_and_closes`,
      `spatial_map_missing.rs::should_not_invent_a_change_section_when_no_snapshot_or_event_source_exists`,
      case `098`, whose assertions require a real state change per §43.6 ("no test may pass
      based only on timer animation").
- [ ] **Tombstones and lifetime identity prevent PID/object reuse confusion** (§10). A visited
      process that exits becomes a tombstone distinct from a place that never existed, a
      tombstone refuses traversal and never resolves to a live object, `back` returns the
      tombstone with the trail record intact, and the replacement process is a different
      identity — `crates/ono-cli/tests/spatial_identity_missing.rs`
      (`::should_carry_a_lifetime_descriptor_rather_than_the_bare_pid_as_process_identity`,
      `::should_report_a_tombstone_rather_than_a_live_place_when_the_visited_process_has_exited`,
      `::should_distinguish_a_tombstone_from_a_place_that_never_existed`,
      `::should_refuse_to_traverse_a_relationship_when_the_place_is_a_tombstone`,
      `::should_never_resolve_a_tombstoned_place_to_a_live_object`,
      `::should_return_the_tombstone_and_keep_the_trail_record_when_back_points_at_a_dead_place`,
      `::should_not_confuse_the_old_and_the_new_process_when_a_place_is_replaced`),
      the §43.2 property `PID reuse -> different lifetime SpatialId` in
      `crates/ono-spatial-core/tests/properties.rs`, case `096`.
- [ ] **Permissions remain honest** (§35.1, §35.2). A neighborhood group carries one of the six
      states of §35.2, denied is reported as denied rather than as an empty collection, an
      unavailable group is distinct from an empty one, and navigation triggers no escalation —
      `spatial_identity_missing.rs::should_report_permission_denied_rather_than_zero_files_for_another_users_process`,
      `::should_report_a_real_file_list_for_a_process_this_user_owns`,
      `::should_name_one_of_the_defined_permission_states_for_every_neighborhood_group`,
      `spatial_contracts_missing.rs::should_report_denied_information_as_denied_rather_than_as_an_empty_collection`,
      `spatial_topology_missing.rs::should_distinguish_an_unavailable_group_from_an_empty_one_when_a_domain_has_no_provider`,
      `spatial_relationships_missing.rs::should_report_the_unreadable_namespace_group_as_unknown_rather_than_absent`,
      case `097`.
- [x] **v0.3 adapted canonical objects participate where available** (§37). An adapted
      observation and its native twin reconcile to one place with both sources retained, and raw
      command output never becomes a place —
      `spatial_contracts_missing.rs::should_reconcile_an_adapted_object_with_its_native_twin_into_one_place`,
      `::should_never_let_raw_command_output_become_a_place`,
      `spatial_identity_missing.rs::should_resolve_the_adapter_view_and_the_native_view_of_one_process_to_one_spatial_id`,
      case `110` (the §37.1 identity-merge assertions `s10-a`–`s10-f`). ADR-0193.
- [x] **KUANG/11 can extend spatial relationships under capabilities** (§36). A package's edges
      stay out of the map until its capability is granted and carry the contributing package as
      their origin when they appear —
      `spatial_contracts_missing.rs::should_keep_a_package_relation_out_of_the_map_until_its_capability_is_granted`,
      `::should_carry_the_contributing_package_as_the_origin_of_every_plugin_edge`, case `110`
      (`s9-a`–`s9-g`), with the spatial contribution APIs validated before load by
      `ono_kuang_testhost` in the same shape as `ono-kuang-testhost/tests/adapter_package.rs`
      (§4.6.2) — `ono-kuang-testhost/tests/spatial_package.rs`. ADR-0194.

#### 4.7.2 Quality and product experience (v0.4 §52.2, §52.3)

v0.4 §52.2 states nine bullets; the second ("unit/property/integration/PTY tests pass") is one
sentence covering the four test layers of §43.1–§43.4, and is expanded here into one box per
layer so that each layer's own checklist is checkable.

- [ ] **All spatial registries validate.** `docs/spec/spatial/{spatial,spaces,relations,landmarks}.yaml`
      (ADR-0126, ADR-0128) exist, are complete in the shape of §41.1/§41.2, and cannot drift
      from the shell: every declared space is served and every served space is declared, the
      same for relations, and the settings block equals the typed catalogue —
      `spatial_contracts_missing.rs::should_ship_the_machine_readable_spatial_registry`,
      `::should_declare_every_canonical_space_with_the_fields_the_registry_requires`,
      `::should_declare_every_relation_with_its_direction_labels_and_confidence`,
      `::should_serve_exactly_the_canonical_spaces_the_registry_declares`,
      `::should_serve_every_relation_it_declares_and_declare_every_relation_it_serves`,
      `crates/ono-cli/tests/spatial_registry.rs` (the settings direction) and
      `cargo run -p xtask -- spec-check` on every gate run.
- [ ] **Unit tests pass** (§43.1). Each of the thirteen areas §43.1 requires — `SpatialId`
      stability, canonical parent selection, selector precedence, ambiguity detection,
      neighborhood ranking, clustering, landmark thresholds, trail operations, tombstone
      resolution, relation inverse handling, scope boundary detection, map node/edge filtering,
      permission-state preservation — has a named test in the spatial crates
      (`crates/ono-spatial-core/tests/{identity,hierarchy,relations,trail,projection}.rs`,
      `crates/ono-spatial-index/tests/index.rs`, and the query/render/events crates as §45
      creates them), with `xtask/tests/spatial_evidence.rs` asserting that no §43.1 area is
      without one.
- [ ] **Property tests pass** (§43.2). The seven properties §43.2 lists are seeded property
      tests in `crates/ono-spatial-core/tests/properties.rs`: `back(enter(x))` returns the prior
      place, `up` never traverses a graph edge, map coordinates never affect identity, filtering
      cannot create unknown edges, one stable provider identity yields one `SpatialId`, PID
      reuse yields a different lifetime id, and every rendered edge references a rendered node or
      an explicit off-map endpoint (also
      `spatial_identity_missing.rs::should_resolve_every_edge_endpoint_to_a_node_or_an_explicit_off_map_endpoint`).
- [ ] **Integration fixtures pass** (§43.3). The deterministic fixture under
      `docker/acceptance/fixtures/spatial/` creates every element §43.3 names — two services,
      one service with several processes, a process holding a known file, a TCP listener, a
      client/server connection, a mount boundary, a namespace or container boundary where the
      environment permits, several users, and a failing/restarting service — and the §43.3
      example acceptance path runs against it without naming the objects: cases `091`, `093`,
      `094`, `096`.
- [ ] **PTY interaction tests pass** (§43.4). All nine PTY checks of §43.4 are driven through a
      real pseudo-terminal — `crates/ono-cli/tests/spatial_interactive_missing.rs` (12 tests:
      startup horizon, ambiguity picker, map open/close, focus without place change, Enter
      changes place, back returns, resize preserves the place, Ctrl-C leaves the shell alive,
      an external program still works after the map closes) and case `099`.
- [ ] **Acceptance scenarios pass.** All ten §44 scenarios of §4.7.3 are renamed from
      `.case.v04` to `.case` and green in `scripts/acceptance.sh`, and no `*.case.v04` file
      remains in `docker/acceptance/cases/` — asserted in the gate by
      `xtask/tests/spatial_evidence.rs`, so a scenario cannot be quietly left out of the suite.
- [ ] **No release-blocking known defects remain.** `docs/STATE.md` *In progress* is empty, the
      workspace holds no `#[ignore]`d test (`cargo run -p xtask -- spec-check`'s unfinished-work
      scan), and every *Deferred* entry names an ADR saying why it does not block the release —
      the same bar §4.5 sets for v0.2 and §4.6.5 for v0.3.
- [ ] **Performance targets are measured, and major violations resolved or documented.** Every
      box of §4.7.5 is ticked; any budget that is exceeded is recorded in an ADR naming the
      figure measured, the cause and the decision, and the ADR is cited by the §4.7.5 box that
      would otherwise be unticked. A budget is never ticked from a figure nobody measured.
- [ ] **Security review completed** (§35, §51 SEC-S01). No test can conclude a review, so the
      accepted evidence is fixed here: an ADR titled *the spatial enumeration review* extends the
      T1–T15 threat table of ADR-0015 with a row per §35 boundary — §35.1 no revelation the
      provider would refuse, §35.2 the six states, §35.3 no escalation from navigation, §35.4 no
      connection a link did not authorise, §35.5 plugin nodes filtered by capability before the
      merge — and **each row names a passing test**, exactly as ADR-0015's rows do.
      `xtask/tests/spatial_evidence.rs` asserts that every test named in that table exists and
      is not ignored, so the box is ticked by the suite, not by the reviewer's opinion. Named
      today: `spatial_identity_missing.rs::should_report_permission_denied_rather_than_zero_files_for_another_users_process`
      (§35.1/§35.2), `::should_name_one_of_the_defined_permission_states_for_every_neighborhood_group`
      (§35.2), case `097` (§35.3, no escalation),
      `spatial_remote_missing.rs::should_refuse_to_jump_to_a_hostname_that_is_not_a_known_link`
      (§35.4), `spatial_contracts_missing.rs::should_keep_a_package_relation_out_of_the_map_until_its_capability_is_granted`
      (§35.5).
- [ ] **The renderer works with colour disabled and with an ASCII fallback** (§39.1, §39.2).
      The six distinctions §39.1 forbids colour to own — current node, inferred edge, failed
      state, remote boundary, root privilege, focused item — are legible without colour, and the
      map draws in plain ASCII on an ASCII-only terminal —
      `spatial_map_missing.rs::should_render_the_map_in_plain_ascii_when_colour_is_disabled_and_the_terminal_is_ascii_only`,
      `spatial_interactive_missing.rs::should_keep_the_same_spatial_semantics_when_look_runs_at_forty_columns`,
      `spatial_remote_missing.rs::should_mark_the_remote_host_in_the_prompt_after_a_jump`, and
      the §43.5 renderer snapshots at 40, 80, 120 and 200 columns in the spatial renderer crate
      (`crates/ono-spatial-render/tests/`) — snapshots as presentation tests only, never a data
      contract.
- [ ] **Terminal state survives entering and exiting full-screen views** (§23.3, §49.8). After
      the map closes the shell screen is restored, an external interactive program still owns
      the terminal, and a resize while a view is open does not move the place —
      `spatial_interactive_missing.rs::should_restore_the_shell_screen_when_the_full_screen_map_closes`,
      `::should_leave_the_terminal_in_order_for_an_external_program_after_the_map_closes`,
      `::should_preserve_the_current_place_when_the_terminal_is_resized_with_a_place_open`,
      case `099` (terminal size and mode after the view, jobs, clean exit).
- [ ] **Provider conformance proves identity and permission semantics** (§42). Every provider
      that feeds the spatial index declares the §42 spatial claims and passes the four §42
      conformance tests — identity stability (§42.1), reuse safety (§42.2), relation integrity
      (§42.3), permission state (§42.4) — generated from `docs/spec/providers/*.yaml` the way
      the v0.2 conformance suites are:
      `spatial_contracts_missing.rs::should_declare_the_spatial_claims_on_every_provider_that_feeds_the_spatial_index`,
      `::should_resolve_repeated_observations_of_one_object_to_the_same_spatial_id`,
      `::should_report_denied_information_as_denied_rather_than_as_an_empty_collection`, and the
      per-provider spatial conformance suites in `crates/ono-provider-*/tests/`.
- [ ] **The product-experience statement is demonstrated** (v0.4 §52.3). §52.3 requires
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

- [ ] **§44.1 cold-start discovery** — `docker/acceptance/cases/090-spatial-cold-start-discovery.case`
      (18 assertions, `44.1a`–`44.1r`): the canonical domains and a meaningful object reached
      from `look`, `map`, `near`, completion and `find place` alone, the text map inside the
      §22 contract and the ~30-node budget, the startup horizon at a terminal, and the two §34
      budget assertions `44.1q`/`44.1r`.
- [ ] **§44.2 unknown web service** — `091-spatial-unknown-web-service.case` (17 assertions):
      the fixture web service selected by visible metadata without its name, entered, its
      process and its listening socket followed, the trail naming the relation, an unavailable
      provider reported honestly, and the §37.1 adapter identity merge.
- [ ] **§44.3 storage discovery** — `092-spatial-storage-discovery.case` (15 assertions): the
      storage domain walked without mount names, the secondary mount entered, its source and
      boundary shown, the mounted directory traversed, `cd` versus `enter` per §30, and a large
      directory summarised with its hidden count.
- [ ] **§44.4 process → file → process** — `093-spatial-process-file-process.case` (14
      assertions): the open-file relation traversed in both directions, `inspect relation`
      explaining it with provider and confidence, and empty distinguished from denied.
- [ ] **§44.5 network path** — `094-spatial-network-path.case` (15 assertions): service →
      process → socket → connection navigated by relationship discovery, plus the §19 half —
      the link map, `jump` across it, no auto-expansion of a remote graph, and no local/remote
      identity merge.
- [ ] **§44.6 back versus up** — `095-spatial-back-versus-up.case` (15 assertions): after the
      §44.6 walk, `back` returns to the process and `up` to the socket's canonical network
      parent, with `trail --compact`, `history_empty` and `no_parent` asserted.
- [ ] **§44.7 identity replacement** — `096-spatial-identity-replacement.case` (13 assertions):
      the entered service process is restarted, the old place becomes a tombstone, the service
      place stays stable and shows the replacement, the trail record survives, and a movement
      onto the tombstone is refused.
- [ ] **§44.8 permission honesty** — `097-spatial-permission-honesty.case` (12 assertions): a
      non-root user investigating a restricted process sees `permission_denied` and `unknown` as
      distinct from empty, no escalation is attempted, and a map of a denied place shows the
      boundary.
- [ ] **§44.9 live map** — `098-spatial-live-map.case` (10 assertions): with `map --live`
      watching, an opened connection makes a real edge appear and a closed one makes it
      disappear or tombstone, nothing is emitted while nothing changes, freshness is shown, and
      Ctrl-C ends the view without killing the shell.
- [ ] **§44.10 raw shell continuity** — `099-spatial-raw-shell-continuity.case` (10
      assertions): after extensive navigation and full-screen map use, interactive process
      control, terminal state, terminal size and mode, jobs and cwd are all still correct, and
      the shell exits cleanly.

#### 4.7.4 The twenty core spatial invariants (v0.4 §2)

One box per invariant, each naming the test that fails when the invariant is violated. Several
invariants are guarded by the same test; where that is so it is said, and no test is invented to
give an invariant one of its own.

- [ ] **§2.1 Discovery before naming.** Violated the moment an object needs its name as input —
      caught by `spatial_topology_missing.rs::should_reach_a_process_it_never_names_when_only_a_predicate_over_visible_metadata_is_known`
      and `::should_answer_look_near_and_map_without_an_object_name_when_at_the_root`; cases
      `090`/`091` (which never type the discovered name). Same proof as §4.7.1's discovery box.
- [ ] **§2.2 Location is explicit.** Caught by
      `spatial_navigation_missing.rs::should_describe_the_current_place_as_a_structured_view_when_look_runs_without_a_tty`,
      `spatial_topology_missing.rs::should_describe_the_current_place_with_an_id_kind_name_scope_and_permission_when_looking`
      and, at a terminal, `spatial_interactive_missing.rs::should_name_the_current_place_in_the_prompt_and_follow_it_when_the_place_changes`
      (shared with §2.20).
- [ ] **§2.3 Movement changes context.** A verb that prints an object without moving fails
      `spatial_navigation_missing.rs::should_move_into_the_hierarchical_child_when_entering_a_canonical_domain_and_its_group`,
      `::should_traverse_the_relationship_edge_when_following_the_parent_relation` and
      `::should_move_across_scopes_and_record_both_ends_when_jumping_to_a_resolved_place`, each
      of which reads the place *after* the command; the inverse — a command that moves and must
      not — is `spatial_relationships_missing.rs::should_keep_the_current_place_when_trace_projects_the_relationship_graph`.
- [ ] **§2.4 Every movement is reversible.** Caught by
      `spatial_navigation_missing.rs::should_return_to_the_process_when_back_follows_the_navigation_history`,
      `::should_answer_history_empty_when_back_runs_with_no_previous_place`,
      `spatial_relationships_missing.rs::should_return_to_the_process_with_back_after_following_a_socket_edge`
      and, for a destination that died, `spatial_identity_missing.rs::should_return_the_tombstone_and_keep_the_trail_record_when_back_points_at_a_dead_place`;
      case `095`.
- [ ] **§2.5 Every edge is explainable.** Caught by
      `spatial_relationships_missing.rs::should_explain_every_edge_with_relation_provider_and_confidence_when_mapping_a_process`
      and `spatial_identity_missing.rs::should_carry_source_provenance_and_confidence_on_every_relationship_edge`;
      case `093` (`inspect relation`).
- [ ] **§2.6 Hierarchy and graph are separate concepts.** Caught by
      `spatial_relationships_missing.rs::should_refuse_to_follow_a_canonical_child_that_is_not_a_relationship_edge`
      and `spatial_identity_missing.rs::should_keep_every_relationship_parent_while_naming_one_canonical_parent`.
      Same proof as §4.7.1's hierarchy/graph box.
- [ ] **§2.7 No fabricated geometry.** Caught by
      `spatial_map_missing.rs::should_omit_screen_coordinates_when_map_json_returns_the_semantic_contract`
      and the §43.2 property `map coordinates never affect semantic identity` in
      `crates/ono-spatial-core/tests/properties.rs` (shared with §2.19).
- [ ] **§2.8 Stable identity beats transient identifiers.** Caught by
      `spatial_identity_missing.rs::should_carry_a_lifetime_descriptor_rather_than_the_bare_pid_as_process_identity`,
      `::should_give_different_spatial_ids_to_two_processes_that_share_a_display_name`,
      `::should_return_the_same_spatial_id_when_the_same_place_is_observed_by_two_shell_invocations`
      and the property `PID reuse -> different lifetime SpatialId`; case `096`.
- [ ] **§2.9 The horizon is bounded.** Caught by
      `spatial_topology_missing.rs::should_bound_the_root_horizon_instead_of_listing_every_known_object`,
      `::should_bound_the_neighborhood_and_count_what_it_hides_when_a_place_has_many_neighbors`,
      `spatial_map_missing.rs::should_bound_the_default_map_when_the_host_holds_more_objects_than_the_view_budget`
      and `spatial_contracts_missing.rs::should_bound_the_default_map_to_its_node_budget`
      (shared with §4.7.5's view-budget box).
- [ ] **§2.10 Zoom is semantic.** Caught by
      `spatial_map_missing.rs::should_aggregate_into_the_canonical_domains_when_the_zoom_level_is_coarse`,
      `::should_report_how_many_objects_a_cluster_stands_for_when_the_view_budget_is_exceeded`
      and `::should_yield_exactly_the_members_and_keep_the_place_when_a_cluster_is_expanded` —
      a view that merely hid rows fails the last of these.
- [ ] **§2.11 Landmarks reflect significance.** Caught by
      `spatial_map_missing.rs::should_expose_a_built_in_reason_for_every_landmark_when_map_json_reports_them`,
      `::should_mark_a_listener_on_every_interface_as_a_public_listener_landmark`,
      `::should_expose_landmark_thresholds_as_inspectable_and_configurable_settings` and
      `spatial_topology_missing.rs::should_expose_a_reason_on_every_landmark_when_a_place_reports_landmarks`.
- [ ] **§2.12 Live views reflect real change.** Caught by
      `spatial_relationships_missing.rs::should_show_the_connection_edge_appear_and_vanish_when_the_connection_opens_and_closes`
      and `spatial_map_missing.rs::should_not_invent_a_change_section_when_no_snapshot_or_event_source_exists`;
      case `098`, which requires a real change for every assertion (§43.6).
- [ ] **§2.13 Text remains sufficient.** Every test in the eight non-PTY spatial suites drives
      the shell through `ono -c` with no terminal at all, so a spatial operation that needed a
      TUI could not pass any of them; named explicitly by
      `spatial_map_missing.rs::should_render_a_text_map_when_stdout_is_a_pipe_and_no_full_screen_view_is_possible`
      and `spatial_navigation_missing.rs::should_answer_a_bounded_graph_when_map_json_runs_without_a_tty`.
- [ ] **§2.14 TTY richness is optional presentation.** Caught by
      `spatial_map_missing.rs::should_return_the_same_node_identities_when_the_terminal_width_changes`
      and `spatial_interactive_missing.rs::should_keep_the_same_spatial_semantics_when_look_runs_at_forty_columns`,
      with the v0.2 determinism floor of §4.2 (`034-redirected-output-is-deterministic`) still
      green for the spatial commands.
- [ ] **§2.15 Unix remains underneath.** Caught by
      `spatial_navigation_missing.rs::should_keep_running_external_commands_when_spatial_navigation_has_happened`,
      `::should_run_the_native_spatial_find_and_keep_the_external_find_reachable_when_both_exist`
      and `::should_run_the_native_spatial_look_and_keep_the_external_look_reachable_when_both_exist`
      (ADR-0124); case `099` and the still-green v0.3 case `087`.
- [ ] **§2.16 Providers own facts.** Caught by
      `spatial_contracts_missing.rs::should_resolve_repeated_observations_of_one_object_to_the_same_spatial_id`,
      `::should_never_let_raw_command_output_become_a_place` and
      `spatial_relationships_missing.rs::should_name_the_same_relation_and_provider_as_trace_when_the_neighbor_is_the_open_file`
      — the spatial layer answering differently from the provider it composes is exactly what
      the last of these fails on; ADR-0131's refusing index is pinned by
      `crates/ono-spatial-index/tests/index.rs`.
- [ ] **§2.17 Unknown is visible.** Caught by the six permission tests of §4.7.1's honesty box,
      chiefly `spatial_identity_missing.rs::should_name_one_of_the_defined_permission_states_for_every_neighborhood_group`
      and `spatial_topology_missing.rs::should_distinguish_an_unavailable_group_from_an_empty_one_when_a_domain_has_no_provider`;
      case `097`. Same proof as that box.
- [ ] **§2.18 Remote boundaries are visible.** Caught by
      `spatial_remote_missing.rs::should_announce_the_boundary_in_plain_text_when_jumping_to_a_linked_host`,
      `::should_record_the_host_and_the_scope_crossing_of_every_step_in_the_trail`,
      `::should_keep_a_remote_process_place_distinct_from_the_local_one_with_the_same_pid` and,
      for the mount boundary, `spatial_storage_missing.rs::should_record_the_boundary_crossing_when_traversing_from_the_root_into_a_mounted_directory`.
- [ ] **§2.19 The user's place survives rendering changes.** Caught by
      `spatial_interactive_missing.rs::should_preserve_the_current_place_when_the_terminal_is_resized_with_a_place_open`,
      `spatial_map_missing.rs::should_return_the_same_node_identities_when_the_terminal_width_changes`
      and `::should_not_change_the_current_place_when_a_map_focuses_a_node` (shared with §2.7).
- [ ] **§2.20 Spatial state is inspectable and scriptable.** Caught by
      `spatial_map_missing.rs::should_describe_identity_state_exits_and_landmarks_when_look_json_reports_a_place`,
      `spatial_navigation_missing.rs::should_record_every_movement_with_its_kind_and_relation_when_the_trail_is_read_as_json`,
      `::should_read_back_into_the_pipeline_when_look_json_is_parsed_by_from_json` and
      `spatial_contracts_missing.rs::should_keep_a_scripts_navigation_out_of_the_callers_place`
      (§29.2); case `092`.

#### 4.7.5 Performance budgets (v0.4 §34)

Measured in the acceptance container on the fixtures of §43.3, by a case in the shape of
`060-performance-budgets`: `docker/acceptance/cases/100-spatial-performance-budgets.case` prints
the figure it measured on every run and asserts it against the budget, as a median of repeated
runs so a loaded machine does not decide the release. The two in-suite timing tests
(`spatial_contracts_missing.rs::should_answer_repeated_looks_far_inside_the_look_budget` and
`::should_bound_the_default_map_to_its_node_budget`) deliberately use a ten-times tolerance so
the gate is not flaky; **they do not tick these boxes** — the container case does, at the real
figure. A budget that cannot be met is documented per §4.7.2's performance box, and the ADR that
documents it is named in the box before it may be ticked.

- [ ] **Interactive startup to usable prompt < 150 ms** — `100-spatial-performance-budgets`
      (`startup-to-prompt`), measured as a median of at least 40 runs, with case `090`'s
      startup-horizon assertions proving the horizon is what is being timed.
- [ ] **Basic `look`, local and cached, < 50 ms** — `100-spatial-performance-budgets`
      (`warm-look`), the marginal cost of a repeated `look` in one session; case `090`
      assertion `44.1q`.
- [ ] **`near` cached < 50 ms** — `100-spatial-performance-budgets` (`warm-near`), measured the
      same way against the neighborhood of a place with many neighbors.
- [ ] **Map L0/L1 cached < 100 ms** — `100-spatial-performance-budgets` (`map-l0-l1`), with the
      zoom level pinned by `spatial_map_missing.rs::should_report_the_requested_canonical_zoom_level_when_map_json_selects_one`.
- [ ] **Map L2 on an ordinary host < 250 ms** — `100-spatial-performance-budgets` (`map-l2`);
      case `090` assertion `44.1r`.
- [ ] **Focus and navigation inside a rendered map < 16 ms per frame** —
      `crates/ono-cli/tests/spatial_interactive_missing.rs` measuring the frame cost of focus
      movement at a real PTY, in the shape of `ono-editor/tests/latency.rs` (§4.3), which bounds
      the editor's keystroke-to-frame path two orders under its own budget.
- [ ] **Search of common indexed objects < 100 ms** — `100-spatial-performance-budgets`
      (`find-place`), a `find place --where …` over the fixture's objects answered from the
      index rather than a provider sweep (§33.1).
- [ ] **Expensive discovery does not block the prompt** (§34.1). The prompt is usable before
      cold discovery finishes and the view updates progressively — asserted at a real terminal
      by `spatial_interactive_missing.rs::should_show_the_spatial_horizon_when_the_session_starts_at_a_terminal_and_never_in_a_pipe`
      together with the `startup-to-prompt` measurement above on a host whose discovery is
      deliberately slow (the slow-NSS fixture of §4.3).
- [ ] **View budgets are enforced, never unbounded** (§34.2). The text map stays at about 30
      nodes and the interactive map at 100 before mandatory clustering, and what was left out is
      disclosed — `spatial_contracts_missing.rs::should_bound_the_default_map_to_its_node_budget`,
      `spatial_map_missing.rs::should_bound_the_default_map_when_the_host_holds_more_objects_than_the_view_budget`,
      `::should_show_more_than_the_default_when_the_map_is_asked_for_all`,
      `spatial_storage_missing.rs::should_summarize_a_large_directory_instead_of_enumerating_it`;
      cases `090` and `092` (shared with §2.9).

## 5. Stopping rule

An agent stops when `scripts/release-check.sh` prints `release-check: the shell is
release-ready`. Any other outcome means there is a next task in `docs/STATE.md`.

Running out of easy work is not a stopping condition. Neither is a green quality gate, a passing
acceptance suite on its own, a completed phase, or a tidy-looking repository. If a box in
section 4 is unticked, the work is unfinished, and the next increment starts.

**Every subsection of section 4 counts, including the tranches.** The checklist grew with the
specification: sections 4.1–4.5 are the v0.2 shell, section 4.6 is the v0.3 External Command
Adaptation Layer, and section 4.7 is the v0.4 Spatial Systems Interface. A tranche whose
subsection still holds an unticked box is an unfinished product, however green the gate and the
acceptance suite are on their own, and the run continues into it.

`scripts/release-check.sh` reads the checklist generically — it greps this file for lines
beginning `- [ ]` and fails on the first one — so a new subsection is seen the moment it is
written, and no subsection can be excluded from the stopping rule by being added late. The
counterpart of that generosity is that a box must never be written in a form no proof can close:
section 3 governs every subsection alike.
