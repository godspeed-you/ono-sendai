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

## 5. Stopping rule

An agent stops when `scripts/release-check.sh` prints `release-check: the shell is
release-ready`. Any other outcome means there is a next task in `docs/STATE.md`.

Running out of easy work is not a stopping condition. Neither is a green quality gate, a passing
acceptance suite on its own, a completed phase, or a tidy-looking repository. If a box in
section 4 is unticked, the work is unfinished, and the next increment starts.
